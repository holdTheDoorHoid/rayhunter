use async_trait::async_trait;
use image::{AnimationDecoder, DynamicImage, codecs::gif::GifDecoder, imageops::FilterType};
use std::io::Cursor;
use std::time::Duration;

use crate::config::{self, UiLevel};
use crate::display::DisplayState;
use rayhunter::analysis::analyzer::EventType;

use log::{error, info, warn};
use tokio::sync::mpsc::Receiver;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use include_dir::{Dir, include_dir};

const REFRESH_RATE: u64 = 1000; //how often in milliseconds to refresh the display

// Pause between GIF passes. Short, so a warning arriving just after a loop ends
// is picked up promptly rather than waiting out a full refresh interval.
const FRAME_YIELD: u64 = 50;

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
        let img_rgba8 = resized_img.as_rgba8().unwrap();
        let mut buf = Vec::with_capacity((height * width).try_into().unwrap());
        for y in 0..height {
            for x in 0..width {
                let px = img_rgba8.get_pixel(x, y);
                buf.push((px[0], px[1], px[2]));
            }
        }

        self.write_buffer(buf).await
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

pub fn update_ui(
    task_tracker: &TaskTracker,
    config: &config::Config,
    mut fb: impl GenericFramebuffer,
    shutdown_token: CancellationToken,
    mut ui_update_rx: Receiver<DisplayState>,
) {
    static IMAGE_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/images/");
    let display_level = config.ui_level;
    if display_level == UiLevel::Invisible {
        info!("Invisible mode, not spawning UI.");
        return;
    }

    let colorblind_mode = config.colorblind_mode;
    let display_colors = config.display_colors.clone();
    let configured_bar_height = config.status_bar_height;
    let display_gifs = config.display_gifs.clone();
    let gif_store_path = config.gif_store_path.clone();

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
                            match tokio::fs::read(&path).await {
                                Ok(bytes) => custom_gif = Some((path, bytes)),
                                Err(e) => {
                                    warn!("couldn't read custom GIF {path}: {e}");
                                    custom_gif = None;
                                }
                            }
                        }
                    }
                    None => custom_gif = None,
                }
            }

            // A height copied from a device with a taller screen would
            // otherwise draw past the end of the framebuffer.
            let mut status_bar_height = configured_bar_height
                .unwrap_or(DEFAULT_STATUS_BAR_HEIGHT)
                .clamp(1, fb.dimensions().height);
            match display_level {
                UiLevel::Demo => {
                    fb.draw_gif_interruptible(img.unwrap(), &ui_update_rx).await;
                }
                UiLevel::CustomGif => {
                    if let Some((_, bytes)) = &custom_gif {
                        fb.draw_gif_interruptible(bytes, &ui_update_rx).await;
                        // The GIF is the whole indicator in this mode; a status
                        // line would sit on top of the user's artwork.
                        status_bar_height = 0;
                    }
                    // With no GIF for this state we fall through to the
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
            if !matches!(display_level, UiLevel::Demo | UiLevel::CustomGif) {
                tokio::time::sleep(Duration::from_millis(REFRESH_RATE)).await;
            } else if ui_update_rx.is_empty() {
                tokio::time::sleep(Duration::from_millis(FRAME_YIELD)).await;
            }
        }
    });
}

/// Where the GIF for `state` is stored on disk.
pub fn gif_path(gif_store_path: &str, state: &str) -> String {
    format!("{gif_store_path}/{state}.gif")
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
