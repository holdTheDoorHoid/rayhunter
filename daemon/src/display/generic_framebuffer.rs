use async_trait::async_trait;
use image::{AnimationDecoder, DynamicImage, codecs::gif::GifDecoder, imageops::FilterType};
use std::io::Cursor;
use std::time::Duration;

use crate::config::{self, UiLevel};
use crate::display::{DisplayState, SharedOverride, SharedSuppression};
use rayhunter::analysis::analyzer::EventType;

use log::{error, info, warn};
use tokio::sync::mpsc::Receiver;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use include_dir::{Dir, include_dir};

const REFRESH_RATE: u64 = 1000; //how often in milliseconds to refresh the display

// Pause between GIF passes. Short, so a warning arriving just after a loop ends
// is picked up promptly rather than waiting out a full refresh interval.
const FRAME_YIELD: u64 = 50;

/// How often a full-screen override is put back, in milliseconds.
///
/// Faster than the ordinary refresh. The device's own interface writes over
/// parts of the framebuffer on its own schedule, and a pairing code with a
/// corner missing does not scan; a phone needs a whole frame to be clean at
/// the moment it looks. A quarter of a second keeps the damage brief without
/// costing much on a single core: the pixels are already converted, so each
/// pass is one write.
const OVERRIDE_REFRESH: u64 = 250;

// Height of the status line when the user hasn't chosen one. Deliberately thin:
// Rayhunter draws over the device's own UI, so the default stays out of the way.
const DEFAULT_STATUS_BAR_HEIGHT: u32 = 2;

#[derive(Copy, Clone)]
pub struct Dimensions {
    pub height: u32,
    pub width: u32,
}

#[derive(Copy, Clone)]
pub enum LinePattern {
    Solid,
    Dashed, // _ _ _ _
    Dotted, // . . . .
}

#[allow(dead_code)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Color {
    Red,
    Green,
    Blue,
    White,
    Black,
    Cyan,
    Yellow,
    Pink,
    Orange,
    /// An arbitrary color, used for the user's `display_colors` overrides.
    Rgb(u8, u8, u8),
}

impl Color {
    fn rgb(self) -> (u8, u8, u8) {
        match self {
            Color::Red => (0xff, 0, 0),
            Color::Green => (0, 0xff, 0),
            Color::Blue => (0, 0, 0xff),
            Color::White => (0xff, 0xff, 0xff),
            Color::Black => (0, 0, 0),
            Color::Cyan => (0, 0xff, 0xff),
            Color::Yellow => (0xff, 0xff, 0),
            Color::Pink => (0xfe, 0x24, 0xff),
            Color::Orange => (0xff, 0xa5, 0),
            Color::Rgb(r, g, b) => (r, g, b),
        }
    }
}

/// Apply a user override from the config, falling back to `default` when the
/// override is unset or isn't a valid `#rrggbb` string.
fn override_or(override_hex: &Option<String>, default: Color) -> Color {
    match override_hex {
        Some(hex) => match config::parse_hex_color(hex) {
            Some((r, g, b)) => Color::Rgb(r, g, b),
            None => {
                warn!("ignoring invalid display color {hex:?}, using built-in color");
                default
            }
        },
        None => default,
    }
}

/// The config key naming a display state, shared by the per-state color and GIF
/// settings. Informational events deliberately share the recording slot, so
/// they look the same as ordinary recording.
fn state_key(state: DisplayState) -> &'static str {
    match state {
        DisplayState::Paused => "paused",
        DisplayState::Recording => "recording",
        DisplayState::WarningDetected { event_type } => match event_type {
            EventType::Informational => "recording",
            EventType::Low => "warning_low",
            EventType::Medium => "warning_medium",
            EventType::High => "warning_high",
        },
    }
}

fn display_style_from_state(
    state: DisplayState,
    colorblind_mode: bool,
    colors: &config::DisplayColors,
) -> (Color, LinePattern) {
    // The built-in color for an active recording, which doubles as the color
    // for informational events.
    let recording_default = if colorblind_mode {
        Color::Blue
    } else {
        Color::Green
    };
    let recording = override_or(&colors.recording, recording_default);

    match state {
        DisplayState::Paused => (
            override_or(&colors.paused, Color::White),
            LinePattern::Solid,
        ),
        DisplayState::Recording => (recording, LinePattern::Solid),
        DisplayState::WarningDetected { event_type } => match event_type {
            EventType::Informational => (recording, LinePattern::Solid),
            EventType::Low => (
                override_or(&colors.warning_low, Color::Yellow),
                LinePattern::Dotted,
            ),
            EventType::Medium => (
                override_or(&colors.warning_medium, Color::Orange),
                LinePattern::Dashed,
            ),
            EventType::High => (
                override_or(&colors.warning_high, Color::Red),
                LinePattern::Solid,
            ),
        },
    }
}

#[async_trait]
pub trait GenericFramebuffer: Send + 'static {
    fn dimensions(&self) -> Dimensions;

    async fn write_buffer(&mut self, buffer: Vec<(u8, u8, u8)>); // rgb, row-wise, left-to-right, top-to-bottom

    async fn write_dynamic_image(&mut self, img: DynamicImage) {
        let buffer = self.image_to_buffer(img);
        self.write_buffer(buffer).await
    }

    /// Turn an image into the pixels this display wants, without drawing it.
    ///
    /// Split out so a still image can be converted once and repainted from the
    /// result. Repainting matters: Rayhunter does not own the framebuffer, and
    /// the device's own interface keeps redrawing its parts of the screen over
    /// whatever is there. Drawing a still image a single time leaves it looking
    /// half erased within seconds, which is exactly what happened on hardware.
    fn image_to_buffer(&self, img: DynamicImage) -> Vec<(u8, u8, u8)> {
        let dimensions = self.dimensions();
        let mut width = img.width();
        let mut height = img.height();
        let resized_img: DynamicImage;
        if height > dimensions.height || width > dimensions.width {
            resized_img = img.resize(dimensions.width, dimensions.height, FilterType::CatmullRom);
            width = dimensions.width.min(resized_img.width());
            height = dimensions.height.min(resized_img.height());
        } else {
            resized_img = img;
        }
        // Converted rather than asserted. `as_rgba8` returns the buffer only
        // when the image already happens to be in that layout, and unwrapping
        // it panicked the display thread on any image that was not: a PNG
        // saved without an alpha channel is stored as RGB and decodes that
        // way. GIFs always decode to RGBA, which is why this held for as long
        // as GIFs were the only thing being drawn.
        //
        // Asking for RGB is also closer to what is wanted. Alpha was already
        // being dropped a line below, since the panel has no notion of it.
        let rgb = resized_img.to_rgb8();
        let mut buf = Vec::with_capacity((height * width).try_into().unwrap());
        for y in 0..height {
            for x in 0..width {
                let px = rgb.get_pixel(x, y);
                buf.push((px[0], px[1], px[2]));
            }
        }
        buf
    }

    /// Play one pass of a GIF, returning early if `interrupted` reports that
    /// something more important needs the screen.
    ///
    /// Frames are decoded one at a time rather than collected up front. On a
    /// 128x128 device an expanded frame is ~64KB, so decoding a long animation
    /// eagerly could exhaust the tens of megabytes of RAM these devices have
    /// free; this way only the current frame is ever held.
    ///
    /// `ui_update_rx` is checked between frames, which bounds how long a
    /// warning can be held off by a running animation to a single frame delay.
    async fn draw_gif_interruptible(
        &mut self,
        img_buffer: &[u8],
        ui_update_rx: &Receiver<DisplayState>,
    ) -> bool {
        // Decoding happens on a blocking thread and frames arrive over a short
        // channel. That keeps the GIF frame iterator (which is neither Send nor
        // cheap) off this task, while the channel's small bound is what caps
        // memory: at most a couple of expanded frames exist at once, however
        // long the animation is.
        // Belt and braces with the upload check: a file could reach the disk
        // some other way, or predate that check. Decoding is where the memory
        // is actually spent, so refusing here is what protects the daemon.
        match gif_dimensions(img_buffer) {
            Some((width, height)) if width > MAX_GIF_DIMENSION || height > MAX_GIF_DIMENSION => {
                error!(
                    "refusing to play a {width}x{height} GIF: over the {MAX_GIF_DIMENSION} pixel \
                     limit and would likely exhaust memory"
                );
                return false;
            }
            None => {
                error!("refusing to play a GIF with no readable dimensions");
                return false;
            }
            _ => {}
        }

        let bytes = img_buffer.to_vec();
        let (tx, mut rx) = tokio::sync::mpsc::channel::<(DynamicImage, u64)>(2);
        let decoder_task = tokio::task::spawn_blocking(move || {
            let Ok(decoder) = GifDecoder::new(Cursor::new(bytes)) else {
                error!("failed to decode GIF, skipping");
                return;
            };
            for frame in decoder.into_frames() {
                let Ok(frame) = frame else { continue };
                let (delay_ms, _) = frame.delay().numer_denom_ms();
                let img = DynamicImage::from(frame.into_buffer());
                // Fails once the receiver is dropped, which is how an
                // interrupted playthrough stops decoding early.
                if tx.blocking_send((img, delay_ms as u64)).is_err() {
                    return;
                }
            }
        });

        let mut interrupted = false;
        while let Some((img, delay_ms)) = rx.recv().await {
            if !ui_update_rx.is_empty() {
                interrupted = true;
                break;
            }
            self.write_dynamic_image(img).await;
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        }

        // Drop before awaiting, so a decoder blocked on a full channel is
        // released rather than deadlocking us.
        drop(rx);
        let _ = decoder_task.await;
        interrupted
    }

    async fn draw_img(&mut self, img_buffer: &[u8]) {
        let img = image::load_from_memory(img_buffer).unwrap();
        self.write_dynamic_image(img).await
    }

    /// Decode a single frame image into ready to write pixels.
    ///
    /// Separate from `draw_img` because this one is handed bytes a person
    /// uploaded. Decoding those must not be able to take the daemon down, so a
    /// file that will not decode is reported and skipped, and the caller falls
    /// back to the status line. `draw_img` keeps its unwrap for the images
    /// compiled into the binary, where a failure really is a build fault.
    ///
    /// Returns the pixels rather than drawing them, so the caller can decode
    /// once and repaint cheaply. The device's own interface keeps writing over
    /// the framebuffer, so a still image has to be put back regularly.
    async fn decode_still_image(&mut self, img_buffer: &[u8]) -> Option<Vec<(u8, u8, u8)>> {
        // Same reasoning as the GIF path: check the declared size before a
        // decoder ever allocates for it.
        match image_dimensions(img_buffer) {
            Some((width, height))
                if width > MAX_GIF_DIMENSION as u32 || height > MAX_GIF_DIMENSION as u32 =>
            {
                error!(
                    "refusing to draw a {width}x{height} image: over the {MAX_GIF_DIMENSION} \
                     pixel limit and would likely exhaust memory"
                );
                return None;
            }
            None => {
                error!("refusing to draw an image with no readable dimensions");
                return None;
            }
            _ => {}
        }

        // Decoding is CPU work on a device with one core, so it goes to a
        // blocking thread rather than stalling the display task.
        let bytes = img_buffer.to_vec();
        let decoded =
            tokio::task::spawn_blocking(move || image::load_from_memory(&bytes).ok()).await;

        match decoded {
            Ok(Some(img)) => Some(self.image_to_buffer(img)),
            Ok(None) => {
                error!("could not decode the custom image, falling back to the status line");
                None
            }
            Err(e) => {
                error!("image decoding task failed: {e}");
                None
            }
        }
    }

    async fn draw_line(&mut self, color: Color, height: u32) {
        self.draw_patterned_line(color, height, LinePattern::Solid)
            .await
    }

    async fn draw_patterned_line(&mut self, color: Color, height: u32, pattern: LinePattern) {
        let width = self.dimensions().width;
        let mut buffer = Vec::with_capacity((height * width).try_into().unwrap());

        for _row in 0..height {
            for col in 0..width {
                let should_draw = match pattern {
                    LinePattern::Solid => true,
                    LinePattern::Dashed => (col / 4) % 2 == 0, // 4 pixels on, 4 pixels off
                    LinePattern::Dotted => col % 4 == 0,       // 1 pixel on, 3 pixels off
                };

                if should_draw {
                    buffer.push(color.rgb());
                } else {
                    buffer.push((0, 0, 0)); // Black background
                }
            }
        }

        self.write_buffer(buffer).await
    }
}

/// Keep only the override serviced, for a display level that draws nothing.
///
/// Invisible mode means no status line, no logo, no sign of Rayhunter. It
/// does not mean no pairing code: a unit that cannot show its code cannot be
/// set up, whatever its display level. So this loop paints an override when
/// there is one and otherwise leaves the screen entirely alone, and clears
/// exactly once when a picture comes down, so the last frame is not left
/// sitting on a panel the device will not repaint.
fn run_override_only(
    task_tracker: &TaskTracker,
    mut fb: impl GenericFramebuffer,
    override_: SharedOverride,
    shutdown_token: CancellationToken,
) {
    task_tracker.spawn(async move {
        let mut was_showing = false;
        loop {
            if shutdown_token.is_cancelled() {
                break;
            }
            match override_.current() {
                Some(px) => {
                    fb.write_buffer((*px).clone()).await;
                    was_showing = true;
                    tokio::time::sleep(Duration::from_millis(OVERRIDE_REFRESH)).await;
                }
                None => {
                    if was_showing {
                        let Dimensions { width, height } = fb.dimensions();
                        fb.write_buffer(vec![(0, 0, 0); (width * height) as usize])
                            .await;
                        was_showing = false;
                    }
                    tokio::time::sleep(Duration::from_millis(REFRESH_RATE)).await;
                }
            }
        }
        if was_showing {
            let Dimensions { width, height } = fb.dimensions();
            fb.write_buffer(vec![(0, 0, 0); (width * height) as usize])
                .await;
        }
    });
}

pub fn update_ui(
    task_tracker: &TaskTracker,
    config: &config::Config,
    mut fb: impl GenericFramebuffer,
    suppression: SharedSuppression,
    override_: SharedOverride,
    shutdown_token: CancellationToken,
    mut ui_update_rx: Receiver<DisplayState>,
) {
    static IMAGE_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/images/");
    let display_level = config.ui_level;
    if display_level == UiLevel::Invisible {
        info!("Invisible mode, not spawning UI; only a pairing code will ever be drawn.");
        run_override_only(task_tracker, fb, override_, shutdown_token);
        return;
    }

    let colorblind_mode = config.colorblind_mode;
    let display_colors = config.display_colors.clone();
    let configured_bar_height = config.status_bar_height;
    let display_gifs = config.display_gifs.clone();
    let gif_store_path = config.gif_store_path.clone();

    // Only the levels that actually cover the screen have anything to give
    // back. A thin status line is not hiding the device's own interface, so
    // pausing it there would cost the status indicator for nothing.
    let covers_the_screen = matches!(
        display_level,
        UiLevel::CustomGif | UiLevel::HighVisibility | UiLevel::EffLogo | UiLevel::Demo
    );

    task_tracker.spawn(async move {
        // this feels wrong, is there a more rusty way to do this?
        let mut img: Option<&[u8]> = None;
        if display_level == UiLevel::Demo {
            img = Some(
                IMAGE_DIR
                    .get_file("orca.gif")
                    .expect("failed to read orca.gif")
                    .contents(),
            );
        } else if display_level == UiLevel::EffLogo {
            img = Some(
                IMAGE_DIR
                    .get_file("eff.png")
                    .expect("failed to read eff.png")
                    .contents(),
            );
        }

        let mut state = DisplayState::Recording;
        // The custom GIF for the state we're currently showing. Only one is
        // held at a time, so uploading several large GIFs can't add up to a
        // memory problem on a device with very little RAM to spare.
        let mut custom_gif: Option<(String, Vec<u8>)> = None;
        // A still image, decoded once and kept ready to repaint. Rayhunter does
        // not own the framebuffer: the device's own interface writes over parts
        // of it constantly, so a picture drawn a single time ends up looking
        // half erased. It gets put back on every pass, like the status line,
        // but without paying to decode it again.
        let mut still_pixels: Option<Vec<(u8, u8, u8)>> = None;
        // Whether the last pass painted an override, so the screen can be
        // cleared exactly once when it comes down. See the note at the end of
        // this loop about devices that never repaint their own interface.
        let mut override_was_showing = false;

        loop {
            if shutdown_token.is_cancelled() {
                info!("received UI shutdown");
                break;
            }
            // Take the most recent state, not merely the next one, so a burst
            // of updates can't leave us rendering a stale one.
            loop {
                match ui_update_rx.try_recv() {
                    Ok(new_state) => state = new_state,
                    Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                    Err(e) => {
                        error!("error receiving framebuffer update message: {e}");
                        break;
                    }
                }
            }

            if display_level == UiLevel::CustomGif {
                let key = state_key(state);
                let wanted = display_gifs
                    .get(key)
                    .map(|_| gif_path(&gif_store_path, key));

                match wanted {
                    Some(path) => {
                        // Load only when the state actually changed.
                        if custom_gif.as_ref().map(|(p, _)| p.as_str()) != Some(path.as_str()) {
                            still_pixels = None;
                            match tokio::fs::read(&path).await {
                                Ok(bytes) => custom_gif = Some((path, bytes)),
                                Err(e) => {
                                    warn!("couldn't read custom image {path}: {e}");
                                    custom_gif = None;
                                }
                            }
                        }
                    }
                    None => {
                        custom_gif = None;
                        still_pixels = None;
                    }
                }
            }

            // A height copied from a device with a taller screen would
            // otherwise draw past the end of the framebuffer.
            let mut status_bar_height = configured_bar_height
                .unwrap_or(DEFAULT_STATUS_BAR_HEIGHT)
                .clamp(1, fb.dimensions().height);

            // Somebody has just pressed a button, so hand the screen back for a
            // moment. Deliberately not a blank: Rayhunter shrinks to its thin
            // line rather than disappearing, so the device's own screens are
            // readable and the status indicator is still there. Going dark
            // would mean a button press could hide a high severity warning,
            // which is not a trade worth making for either side of it.
            // A full-screen override covers the screen whatever the level,
            // so a button press steps aside from it too.
            let override_frame = override_.current();
            let paused_for_keypress =
                (covers_the_screen || override_frame.is_some()) && suppression.active();
            if paused_for_keypress {
                // `status_bar_height` is already the configured height, so the
                // line drawn here is the one the person chose. Forcing the
                // built-in 2px instead made this invisible on devices whose top
                // rows are not shown at all: the Moxee needs about 6px before
                // anything appears, which is exactly why the height is settable.
                // Stepping aside is no use if what remains cannot be seen.
                if status_bar_height > 0 {
                    let (color, pattern) =
                        display_style_from_state(state, colorblind_mode, &display_colors);
                    fb.draw_patterned_line(color, status_bar_height, pattern)
                        .await;
                }
                tokio::time::sleep(Duration::from_millis(REFRESH_RATE)).await;
                continue;
            }

            // Something that must be on screen regardless of the level: the
            // pairing code, mainly. It replaces everything below, status line
            // included, for as long as it is up. Repainted every pass and on
            // a shorter interval than the ordinary display, because a code
            // with a corner overwritten by the device's own interface is a
            // code that does not scan.
            if let Some(px) = override_frame {
                fb.write_buffer((*px).clone()).await;
                override_was_showing = true;
                tokio::time::sleep(Duration::from_millis(OVERRIDE_REFRESH)).await;
                continue;
            }
            if override_was_showing {
                // The picture has just come down. Clear it before going back
                // to normal drawing, since a thin status line on top of a
                // pairing code is what a device that never repaints its own
                // screen would otherwise show for ever.
                let Dimensions { width, height } = fb.dimensions();
                fb.write_buffer(vec![(0, 0, 0); (width * height) as usize])
                    .await;
                override_was_showing = false;
            }

            match display_level {
                UiLevel::Demo => {
                    fb.draw_gif_interruptible(img.unwrap(), &ui_update_rx).await;
                }
                UiLevel::CustomGif => {
                    if let Some((_, bytes)) = &custom_gif {
                        // A still image and an animation are both allowed here.
                        // Which one this is comes from the file's own bytes, so
                        // uploading a PNG needs no separate mode and no setting.
                        let drawn = match image_kind(bytes) {
                            Some(ImageKind::Animated) => {
                                fb.draw_gif_interruptible(bytes, &ui_update_rx).await;
                                still_pixels = None;
                                true
                            }
                            Some(ImageKind::Still) => {
                                // Decoded on first sight of this image, then
                                // repainted from the result on every pass.
                                if still_pixels.is_none() {
                                    still_pixels = fb.decode_still_image(bytes).await;
                                }
                                match &still_pixels {
                                    Some(px) => {
                                        fb.write_buffer(px.clone()).await;
                                        true
                                    }
                                    None => false,
                                }
                            }
                            None => {
                                warn!("custom image for this state is neither a GIF nor a PNG");
                                false
                            }
                        };
                        if drawn {
                            // The image is the whole indicator in this mode; a
                            // status line would sit on top of the user's
                            // artwork.
                            status_bar_height = 0;
                        }
                    }
                    // With no image for this state we fall through to the
                    // ordinary status line, so the device is never blank.
                }
                UiLevel::EffLogo => fb.draw_img(img.unwrap()).await,
                UiLevel::HighVisibility => {
                    status_bar_height = fb.dimensions().height;
                }
                UiLevel::TransFlag => {
                    fb.draw_line(Color::Cyan, 128).await;
                    fb.draw_line(Color::Pink, 102).await;
                    fb.draw_line(Color::White, 76).await;
                    fb.draw_line(Color::Pink, 50).await;
                    fb.draw_line(Color::Cyan, 25).await;
                }
                // UiLevel::Subtle (1) and anything else: just the status bar line
                _ => {}
            };

            if status_bar_height > 0 {
                let (color, pattern) =
                    display_style_from_state(state, colorblind_mode, &display_colors);
                fb.draw_patterned_line(color, status_bar_height, pattern)
                    .await;
            }

            // A GIF pass already took real time, and re-sleeping the full
            // refresh interval on top of it makes state changes feel sluggish.
            // A still image is the opposite case: nothing is animating, so the
            // short yield would just spin this loop against an unchanging
            // picture, which on a single core device is worth avoiding.
            let animating = matches!(display_level, UiLevel::Demo)
                || (display_level == UiLevel::CustomGif && still_pixels.is_none());
            if !animating {
                tokio::time::sleep(Duration::from_millis(REFRESH_RATE)).await;
            } else if ui_update_rx.is_empty() {
                tokio::time::sleep(Duration::from_millis(FRAME_YIELD)).await;
            }
        }

        // Put the screen back before letting go of it.
        //
        // Rayhunter does not own the framebuffer, and how the manufacturer's
        // interface behaves differs by device. An Orbic repaints its own
        // screens continuously, so anything Rayhunter drew is scrubbed away
        // within seconds by itself. A TP-Link M7350 does not: `oledd` repaints
        // the status icons along the top on a timer and the body of the screen
        // only when something happens, such as a button press. A mode that
        // filled the screen therefore left its last frame sitting there for
        // ever. Measured on an M7350 v8.0: five minutes after switching from
        // High Visibility to Invisible, 16160 of 16384 pixels were still
        // Rayhunter's green.
        //
        // That is worst for Invisible, whose whole purpose is that there is no
        // sign Rayhunter is running, and which instead left a full screen orca
        // on the display. It also applies when the daemon stops or is
        // uninstalled from one of these modes.
        //
        // Clear exactly what this mode painted and no more. Blanking the whole
        // screen on the way out of Subtle would take away the manufacturer's
        // own interface on a device that will not redraw it, and every config
        // save goes through here. Clearing just the status line still matters:
        // leaving a coloured line at the top is precisely the indicator
        // Invisible mode exists to remove.
        let Dimensions { width, height } = fb.dimensions();
        let clear_height = if covers_the_screen || override_was_showing {
            height
        } else {
            configured_bar_height
                .unwrap_or(DEFAULT_STATUS_BAR_HEIGHT)
                .clamp(1, height)
        };
        let blank = vec![(0, 0, 0); (width * clear_height) as usize];
        fb.write_buffer(blank).await;
    });
}

/// Largest picture we will play, per side.
///
/// The screens involved are 128 pixels square, so this is already generous.
/// The limit exists because GIF compresses flat colour extremely well: a 13KB
/// file can declare a 4000 by 4000 canvas that expands to 61MB per frame, and
/// these devices run with around 20MB of RAM free. Playing one would get the
/// daemon killed, which would stop detection while still looking like
/// Rayhunter was running. Checked here rather than only in the browser, since
/// the API can be called directly.
pub const MAX_GIF_DIMENSION: u16 = 512;

/// The canvas size a GIF declares in its header, without decoding it.
///
/// Bytes 6 to 9 of every GIF are the logical screen width and height, little
/// endian, so this costs nothing and happens before any allocation.
pub fn gif_dimensions(bytes: &[u8]) -> Option<(u16, u16)> {
    if bytes.len() < 10 {
        return None;
    }
    Some((
        u16::from_le_bytes([bytes[6], bytes[7]]),
        u16::from_le_bytes([bytes[8], bytes[9]]),
    ))
}

/// Where the custom image for `state` is stored on disk.
///
/// Still named `.gif` because that is what devices already in the field have
/// on them, and one file per state is the whole storage scheme. What the file
/// actually holds is decided by looking at its first bytes, not its name.
pub fn gif_path(gif_store_path: &str, state: &str) -> String {
    format!("{gif_store_path}/{state}.gif")
}

/// A custom display image is either animated or a single frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ImageKind {
    /// A GIF, played frame by frame.
    Animated,
    /// A PNG or anything else with one frame, drawn once and left up.
    Still,
}

/// What kind of image `bytes` holds, or `None` if it is not one we can draw.
///
/// Decided from the magic bytes rather than the file name, since the name is
/// chosen by whoever uploaded it and says nothing reliable.
pub fn image_kind(bytes: &[u8]) -> Option<ImageKind> {
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some(ImageKind::Animated);
    }
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        return Some(ImageKind::Still);
    }
    None
}

/// The declared pixel size of a GIF or PNG, read from its header.
///
/// The point is to learn how big an image claims to be *before* handing it to
/// a decoder. A tiny file can declare an enormous canvas, and expanding one on
/// a device with a few megabytes free is how the daemon gets killed rather
/// than how it draws a picture.
pub fn image_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    match image_kind(bytes)? {
        // Bytes 6 to 9 are the logical screen width and height, little endian.
        ImageKind::Animated => {
            let (w, h) = gif_dimensions(bytes)?;
            Some((w as u32, h as u32))
        }
        // The IHDR chunk always comes first, so width and height sit at a
        // fixed offset: 8 bytes of signature, 8 of chunk header, then the two
        // dimensions as big endian u32s.
        ImageKind::Still => {
            if bytes.len() < 24 {
                return None;
            }
            let w = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
            let h = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
            Some((w, h))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Color, LinePattern, display_style_from_state};
    use crate::config::DisplayColors;
    use crate::display::DisplayState;
    use rayhunter::analysis::analyzer::EventType;

    /// Colors chosen by the user take precedence over the built-in ones.
    #[test]
    fn custom_colors_override_defaults() {
        let colors = DisplayColors {
            recording: Some("#123456".to_string()),
            warning_high: Some("#abcdef".to_string()),
            ..Default::default()
        };

        let (color, _) = display_style_from_state(DisplayState::Recording, false, &colors);
        assert_eq!(color, Color::Rgb(0x12, 0x34, 0x56));

        let (color, _) = display_style_from_state(
            DisplayState::WarningDetected {
                event_type: EventType::High,
            },
            false,
            &colors,
        );
        assert_eq!(color, Color::Rgb(0xab, 0xcd, 0xef));
    }

    /// States the user hasn't customized keep their built-in colors, including
    /// the colorblind green-to-blue substitution.
    #[test]
    fn unset_colors_fall_back_to_builtins() {
        let colors = DisplayColors {
            warning_low: Some("#ffffff".to_string()),
            ..Default::default()
        };

        let (color, _) = display_style_from_state(DisplayState::Recording, false, &colors);
        assert_eq!(color, Color::Green);

        let (color, _) = display_style_from_state(DisplayState::Recording, true, &colors);
        assert_eq!(color, Color::Blue);

        let (color, _) = display_style_from_state(DisplayState::Paused, false, &colors);
        assert_eq!(color, Color::White);
    }

    /// A custom recording color wins even when colorblind mode is on, so the
    /// picker never appears to do nothing.
    #[test]
    fn custom_recording_color_wins_over_colorblind_mode() {
        let colors = DisplayColors {
            recording: Some("#00ffff".to_string()),
            ..Default::default()
        };
        let (color, _) = display_style_from_state(DisplayState::Recording, true, &colors);
        assert_eq!(color, Color::Rgb(0, 0xff, 0xff));
    }

    /// A malformed color must not blank the display; it falls back instead.
    #[test]
    fn invalid_colors_fall_back_to_builtins() {
        let colors = DisplayColors {
            warning_medium: Some("not-a-color".to_string()),
            ..Default::default()
        };
        let (color, pattern) = display_style_from_state(
            DisplayState::WarningDetected {
                event_type: EventType::Medium,
            },
            false,
            &colors,
        );
        assert_eq!(color, Color::Orange);
        assert!(matches!(pattern, LinePattern::Dashed));
    }

    /// Line patterns convey severity without color, so they must not change
    /// when colors are customized.
    #[test]
    fn patterns_are_unaffected_by_custom_colors() {
        let colors = DisplayColors {
            warning_low: Some("#111111".to_string()),
            warning_medium: Some("#222222".to_string()),
            warning_high: Some("#333333".to_string()),
            ..Default::default()
        };
        let pattern_for = |event_type| {
            display_style_from_state(DisplayState::WarningDetected { event_type }, false, &colors).1
        };
        assert!(matches!(pattern_for(EventType::Low), LinePattern::Dotted));
        assert!(matches!(
            pattern_for(EventType::Medium),
            LinePattern::Dashed
        ));
        assert!(matches!(pattern_for(EventType::High), LinePattern::Solid));
    }
}

#[cfg(test)]
mod gif_tests {
    use super::{gif_path, state_key};
    use crate::config::{DISPLAY_STATE_KEYS, DisplayGifs};
    use crate::display::DisplayState;
    use rayhunter::analysis::analyzer::EventType;

    /// Every state maps to a key the config actually knows about, so a GIF
    /// uploaded for a state is always found again at playback time.
    #[test]
    fn state_keys_match_config_keys() {
        let states = [
            DisplayState::Paused,
            DisplayState::Recording,
            DisplayState::WarningDetected {
                event_type: EventType::Informational,
            },
            DisplayState::WarningDetected {
                event_type: EventType::Low,
            },
            DisplayState::WarningDetected {
                event_type: EventType::Medium,
            },
            DisplayState::WarningDetected {
                event_type: EventType::High,
            },
        ];
        for state in states {
            let key = state_key(state);
            assert!(
                DISPLAY_STATE_KEYS.contains(&key),
                "a display state produced unknown key {key}"
            );
        }
    }

    /// Informational events share the recording slot, matching how colors work.
    #[test]
    fn informational_shares_the_recording_slot() {
        assert_eq!(state_key(DisplayState::Recording), "recording");
        assert_eq!(
            state_key(DisplayState::WarningDetected {
                event_type: EventType::Informational
            }),
            "recording"
        );
    }

    #[test]
    fn severities_get_their_own_slots() {
        assert_eq!(
            state_key(DisplayState::WarningDetected {
                event_type: EventType::Low
            }),
            "warning_low"
        );
        assert_eq!(
            state_key(DisplayState::WarningDetected {
                event_type: EventType::High
            }),
            "warning_high"
        );
    }

    #[test]
    fn gif_paths_are_per_state() {
        assert_eq!(
            gif_path("/data/rayhunter/gifs", "warning_high"),
            "/data/rayhunter/gifs/warning_high.gif"
        );
        assert_ne!(
            gif_path("/x", "warning_low"),
            gif_path("/x", "warning_medium")
        );
    }

    /// Slots are independent: setting one leaves the others untouched.
    #[test]
    fn gif_slots_are_independent() {
        let gifs = DisplayGifs {
            warning_high: Some("warning_high.gif".into()),
            ..Default::default()
        };
        assert_eq!(
            gifs.get("warning_high").map(String::as_str),
            Some("warning_high.gif")
        );
        assert_eq!(gifs.get("warning_low"), None);
        assert_eq!(gifs.get("paused"), None);
    }

    /// A key that isn't a display state never resolves to a GIF.
    #[test]
    fn unknown_states_have_no_gif() {
        let gifs = DisplayGifs {
            recording: Some("recording.gif".into()),
            ..Default::default()
        };
        assert_eq!(gifs.get("not_a_state"), None);
        assert_eq!(gifs.get(""), None);
    }
}

#[cfg(test)]
mod gif_safety_tests {
    use super::{MAX_GIF_DIMENSION, gif_dimensions};

    /// A GIF header carries the canvas size at a fixed offset, so an enormous
    /// canvas can be rejected before a decoder allocates anything for it.
    fn header(width: u16, height: u16) -> Vec<u8> {
        let mut v = b"GIF89a".to_vec();
        v.extend_from_slice(&width.to_le_bytes());
        v.extend_from_slice(&height.to_le_bytes());
        v
    }

    #[test]
    fn reads_the_declared_canvas_size() {
        assert_eq!(gif_dimensions(&header(128, 128)), Some((128, 128)));
        assert_eq!(gif_dimensions(&header(4000, 4000)), Some((4000, 4000)));
    }

    #[test]
    fn a_truncated_header_yields_nothing_rather_than_garbage() {
        assert_eq!(gif_dimensions(b"GIF89a"), None);
        assert_eq!(gif_dimensions(b""), None);
        assert_eq!(gif_dimensions(b"GIF89a\x01"), None);
    }

    /// The case that matters: GIF compresses flat colour so well that a file of
    /// a few kilobytes can declare a canvas needing tens of megabytes per frame,
    /// which would exhaust a device that has around 20MB free and take the
    /// daemon down with it. Size on disk is no guide at all.
    #[test]
    fn a_decompression_bomb_is_over_the_limit() {
        let (width, height) = gif_dimensions(&header(4000, 4000)).unwrap();
        assert!(width > MAX_GIF_DIMENSION && height > MAX_GIF_DIMENSION);
        // What that canvas would actually cost, expanded.
        let bytes_per_frame = 4000u64 * 4000 * 4;
        assert!(bytes_per_frame > 60 * 1024 * 1024);
    }

    #[test]
    fn the_device_screen_size_is_comfortably_allowed() {
        let (width, height) = gif_dimensions(&header(128, 128)).unwrap();
        assert!(width <= MAX_GIF_DIMENSION && height <= MAX_GIF_DIMENSION);
    }

    #[test]
    fn the_limit_leaves_a_frame_small_enough_to_be_safe() {
        let worst = MAX_GIF_DIMENSION as u64 * MAX_GIF_DIMENSION as u64 * 4;
        // Two frames are held at once by the playback channel.
        assert!(
            worst * 2 < 4 * 1024 * 1024,
            "worst case frame pair too large"
        );
    }
}

#[cfg(test)]
mod image_format_tests {
    use super::{ImageKind, image_dimensions, image_kind};

    /// A 1x1 PNG, header intact. Dimensions live at a fixed offset in IHDR.
    fn png_header(width: u32, height: u32) -> Vec<u8> {
        let mut v = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        v.extend_from_slice(&13u32.to_be_bytes()); // IHDR length
        v.extend_from_slice(b"IHDR");
        v.extend_from_slice(&width.to_be_bytes());
        v.extend_from_slice(&height.to_be_bytes());
        v
    }

    fn gif_header(width: u16, height: u16) -> Vec<u8> {
        let mut v = b"GIF89a".to_vec();
        v.extend_from_slice(&width.to_le_bytes());
        v.extend_from_slice(&height.to_le_bytes());
        v
    }

    /// The kind comes from the bytes, never the file name. Whoever uploads a
    /// file chooses its name, so the name is not evidence of anything.
    #[test]
    fn format_is_read_from_the_magic_bytes() {
        assert_eq!(image_kind(&gif_header(128, 128)), Some(ImageKind::Animated));
        assert_eq!(
            image_kind(b"GIF87a\x00\x00\x00\x00"),
            Some(ImageKind::Animated)
        );
        assert_eq!(image_kind(&png_header(128, 128)), Some(ImageKind::Still));
    }

    #[test]
    fn anything_else_is_refused_rather_than_guessed_at() {
        assert_eq!(image_kind(b"<html>hello</html>"), None);
        assert_eq!(image_kind(b"\xff\xd8\xff\xe0 jpeg"), None);
        assert_eq!(image_kind(b""), None);
        assert_eq!(image_kind(b"GIF"), None);
    }

    /// Dimensions have to be readable from the header alone, because the whole
    /// point is to know how big a picture claims to be before a decoder
    /// allocates for it. A small file can declare an enormous canvas.
    #[test]
    fn dimensions_come_from_the_header_of_either_format() {
        assert_eq!(image_dimensions(&gif_header(128, 64)), Some((128, 64)));
        assert_eq!(
            image_dimensions(&png_header(1920, 1080)),
            Some((1920, 1080))
        );
    }

    #[test]
    fn a_truncated_header_reports_nothing_rather_than_a_wrong_size() {
        assert_eq!(image_dimensions(&png_header(128, 128)[..20]), None);
        assert_eq!(image_dimensions(b"GIF89a\x80"), None);
        assert_eq!(image_dimensions(b"not an image at all"), None);
    }
}

#[cfg(test)]
mod pixel_conversion_tests {
    use image::{DynamicImage, Rgb, RgbImage, Rgba, RgbaImage};

    /// Every image format the display accepts has to survive being turned into
    /// pixels, not just the one GIFs happen to decode to.
    ///
    /// This is a real bug that shipped to a device: the conversion asked for
    /// an RGBA buffer and unwrapped it, which is `None` for an image that is
    /// not already RGBA. A PNG saved without transparency decodes as RGB, so
    /// uploading one panicked the display thread and left the screen black
    /// while the daemon carried on as though nothing had happened.
    #[test]
    fn images_without_an_alpha_channel_convert_rather_than_panicking() {
        let rgb = DynamicImage::ImageRgb8(RgbImage::from_pixel(4, 4, Rgb([255, 0, 200])));
        let converted = rgb.to_rgb8();
        assert_eq!(converted.get_pixel(0, 0), &Rgb([255, 0, 200]));

        let rgba = DynamicImage::ImageRgba8(RgbaImage::from_pixel(4, 4, Rgba([1, 2, 3, 255])));
        let converted = rgba.to_rgb8();
        assert_eq!(converted.get_pixel(0, 0), &Rgb([1, 2, 3]));

        // Greyscale is the other thing a PNG is commonly saved as.
        let luma = DynamicImage::ImageLuma8(image::GrayImage::from_pixel(4, 4, image::Luma([128])));
        let converted = luma.to_rgb8();
        assert_eq!(converted.get_pixel(0, 0), &Rgb([128, 128, 128]));
    }
}
