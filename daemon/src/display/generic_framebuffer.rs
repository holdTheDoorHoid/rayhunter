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

    async fn draw_gif(&mut self, img_buffer: &[u8]) {
        let cursor = Cursor::new(img_buffer);
        if let Ok(decoder) = GifDecoder::new(cursor) {
            let frames: Vec<_> = decoder
                .into_frames()
                .filter_map(|f| f.ok())
                .map(|frame| {
                    let (numerator, _) = frame.delay().numer_denom_ms();
                    let img = DynamicImage::from(frame.into_buffer());
                    (img, numerator as u64)
                })
                .collect();

            for (img, delay_ms) in frames {
                self.write_dynamic_image(img).await;
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }
        }
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
    let mut display_style =
        display_style_from_state(DisplayState::Recording, colorblind_mode, &display_colors);

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
        loop {
            if shutdown_token.is_cancelled() {
                info!("received UI shutdown");
                break;
            }
            match ui_update_rx.try_recv() {
                Ok(state) => {
                    display_style =
                        display_style_from_state(state, colorblind_mode, &display_colors);
                }
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {}
                Err(e) => error!("error receiving framebuffer update message: {e}"),
            }

            let mut status_bar_height = 2;
            match display_level {
                UiLevel::Demo => fb.draw_gif(img.unwrap()).await,
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
            let (color, pattern) = display_style;
            fb.draw_patterned_line(color, status_bar_height, pattern)
                .await;
            tokio::time::sleep(Duration::from_millis(REFRESH_RATE)).await;
        }
    });
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
