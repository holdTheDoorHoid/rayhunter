//! QR codes, and a line of text, on the device's own screen.
//!
//! This is what a person sees when a unit needs pairing: a code to scan and,
//! under it, the same token in letters for anyone whose phone will not scan a
//! screen this small. Everything here produces pixels; nothing here draws.
//! The display loop paints the result over and over, because Rayhunter does
//! not own the framebuffer and the device's own interface keeps writing over
//! parts of it.
//!
//! Dark modules on a white ground, never the reverse. Some phone cameras do
//! read inverted codes, but not all of them, and the one that does not is the
//! one the buyer has.

use qrcodegen::{QrCode, QrCodeEcc};

/// What is known about a screen before anything is drawn on it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenGeometry {
    pub width: u32,
    pub height: u32,
    /// Rows at the top the panel does not actually show.
    ///
    /// The Moxee's screen starts a few rows below the top of its framebuffer;
    /// a status line thinner than that is invisible on it. Anything placed
    /// here is wasted, so the layout starts below it.
    pub top_inset: u32,
}

impl ScreenGeometry {
    /// A plain square screen that shows every row.
    pub const fn square(side: u32) -> Self {
        Self {
            width: side,
            height: side,
            top_inset: 0,
        }
    }
}

/// Smallest module we will draw. Below two pixels a module is a single dot
/// on a screen where a pixel is a fifth of a millimetre, and no camera reads
/// that.
pub const MIN_MODULE_PX: u32 = 2;

/// Modules of white around the code.
///
/// The specification asks for four. On a screen this small four modules of
/// margin is a third of the width, and every scanner tested copes with less
/// when the surround is plain. Two is the design's choice; a layout that
/// still does not fit drops to one and a half before it shrinks the modules.
pub const QUIET_MODULES: u32 = 2;

/// Pixels between the bottom of the code's quiet zone and the caption.
const CAPTION_GAP: u32 = 2;

pub const WHITE: (u8, u8, u8) = (0xff, 0xff, 0xff);
pub const BLACK: (u8, u8, u8) = (0, 0, 0);

/// Encode `text` as a QR code.
///
/// Medium error correction, raised for free when a higher level fits in the
/// same size. The text is passed as given: an uppercase URL is encoded in
/// alphanumeric mode, which is what keeps a setup link at 25 modules a side.
/// The caller decides the case; this does not change it.
pub fn encode(text: &str) -> Option<QrCode> {
    QrCode::encode_text(text, QrCodeEcc::Medium).ok()
}

/// Where everything goes on the screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    /// Pixels per module actually used, which may be fewer than asked for.
    pub module_px: u32,
    /// White margin around the code, in pixels.
    pub quiet_px: u32,
    /// Top left of the first module.
    pub code_x: u32,
    pub code_y: u32,
    /// Top of the caption's glyphs, when there is one.
    pub caption_y: Option<u32>,
    pub caption_scale: u32,
}

/// Find a layout for a code `size` modules a side.
///
/// Tries the module size asked for and works down. At each size the full
/// quiet zone is tried first, then a reduced one, so a code that is one row
/// too tall keeps its module size and loses a little margin instead. The
/// caption, when there is one, is a fixed cost below the code. `None` means
/// nothing fits even at the smallest module, which is a code far too big for
/// this screen.
pub fn layout(
    size: u32,
    wanted_px: u32,
    caption: bool,
    caption_scale: u32,
    geo: ScreenGeometry,
) -> Option<Layout> {
    let caption_scale = caption_scale.max(1);
    let avail_h = geo.height.saturating_sub(geo.top_inset);
    let caption_h = if caption {
        CAPTION_GAP + font::GLYPH_H * caption_scale
    } else {
        0
    };
    for px in (MIN_MODULE_PX..=wanted_px.max(MIN_MODULE_PX)).rev() {
        let code = size * px;
        // Full quiet zone, then three quarters of it.
        for quiet in [QUIET_MODULES * px, (QUIET_MODULES * px * 3).div_ceil(4)] {
            let needed_w = code + 2 * quiet;
            let needed_h = quiet + code + quiet + caption_h;
            if needed_w > geo.width || needed_h > avail_h {
                continue;
            }
            let block_y = geo.top_inset + (avail_h - needed_h) / 2;
            let code_y = block_y + quiet;
            return Some(Layout {
                module_px: px,
                quiet_px: quiet,
                code_x: (geo.width - code) / 2,
                code_y,
                caption_y: caption.then_some(code_y + code + quiet + CAPTION_GAP),
                caption_scale,
            });
        }
    }
    None
}

/// A full screen of pixels: the code, its margin, and the caption, on white.
///
/// Row-major, `width * height` long, which is exactly what the framebuffer
/// drivers expect. The whole screen is painted white rather than just the
/// quiet zone, so the margin around the code is as wide as the screen allows
/// whatever the layout, and so no strip of the device's own interface is left
/// showing next to the code.
pub fn render(
    code: &QrCode,
    layout: Layout,
    caption: Option<&str>,
    geo: ScreenGeometry,
) -> Vec<(u8, u8, u8)> {
    let mut buf = vec![WHITE; (geo.width * geo.height) as usize];
    let size = code.size() as u32;
    for my in 0..size {
        for mx in 0..size {
            if !code.get_module(mx as i32, my as i32) {
                continue;
            }
            let x0 = layout.code_x + mx * layout.module_px;
            let y0 = layout.code_y + my * layout.module_px;
            fill_rect(
                &mut buf,
                geo,
                x0,
                y0,
                layout.module_px,
                layout.module_px,
                BLACK,
            );
        }
    }
    if let (Some(text), Some(y)) = (caption, layout.caption_y) {
        let text_w = font::text_width(text, layout.caption_scale);
        let x = geo.width.saturating_sub(text_w) / 2;
        font::draw_text(&mut buf, geo, text, x, y, layout.caption_scale, BLACK);
    }
    buf
}

/// The code as an SVG, for a web page to show. One `<path>` of unit
/// squares in a `viewBox` with `border` modules of margin, so it scales to
/// any size the page wants without a raster in between.
pub fn svg(code: &QrCode, border: u32) -> String {
    let size = code.size() as u32;
    let span = size + 2 * border;
    let mut path = String::new();
    for y in 0..size {
        for x in 0..size {
            if code.get_module(x as i32, y as i32) {
                path.push_str(&format!("M{} {}h1v1h-1z", x + border, y + border));
            }
        }
    }
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {span} {span}\" shape-rendering=\"crispEdges\"><rect width=\"{span}\" height=\"{span}\" fill=\"#fff\"/><path d=\"{path}\" fill=\"#000\"/></svg>"
    )
}

/// A screen of centred text lines, each at its own scale, black on white.
///
/// For the few words a unit ever has to say on its own: the step-up code,
/// or "press the button". Lines that do not fit are drawn anyway and
/// clipped, which is still more use than nothing.
pub fn text_screen(lines: &[(&str, u32)], geo: ScreenGeometry) -> Vec<(u8, u8, u8)> {
    let mut buf = vec![WHITE; (geo.width * geo.height) as usize];
    let gap = 3;
    let total: u32 = lines
        .iter()
        .map(|(_, s)| font::GLYPH_H * s.max(&1) + gap)
        .sum::<u32>()
        .saturating_sub(gap);
    let avail = geo.height.saturating_sub(geo.top_inset);
    let mut y = geo.top_inset + avail.saturating_sub(total) / 2;
    for (text, scale) in lines {
        let scale = (*scale).max(1);
        let w = font::text_width(text, scale);
        let x = geo.width.saturating_sub(w) / 2;
        font::draw_text(&mut buf, geo, text, x, y, scale, BLACK);
        y += font::GLYPH_H * scale + gap;
    }
    buf
}

/// Rows drawn under the status line.
pub const BANNER_HEIGHT: u32 = font::GLYPH_H + 2;

/// A strip of white text on black, `BANNER_HEIGHT` rows by `width`.
pub fn banner_strip(width: u32, text: &str) -> Vec<(u8, u8, u8)> {
    let geo = ScreenGeometry {
        width,
        height: BANNER_HEIGHT,
        top_inset: 0,
    };
    let mut buf = vec![BLACK; (width * BANNER_HEIGHT) as usize];
    let w = font::text_width(text, 1);
    let x = width.saturating_sub(w) / 2;
    font::draw_text(&mut buf, geo, text, x, 1, 1, WHITE);
    buf
}

/// Paint a rectangle, clipped to the screen.
fn fill_rect(
    buf: &mut [(u8, u8, u8)],
    geo: ScreenGeometry,
    x0: u32,
    y0: u32,
    w: u32,
    h: u32,
    color: (u8, u8, u8),
) {
    for y in y0..(y0 + h).min(geo.height) {
        for x in x0..(x0 + w).min(geo.width) {
            buf[(y * geo.width + x) as usize] = color;
        }
    }
}

/// A five by seven pixel font, enough for a token, a URL, or a fingerprint.
///
/// Written out as pictures rather than bit patterns so a wrong glyph is
/// visible in the source. Lowercase is drawn as uppercase; anything not here
/// draws as a question mark, so a stray character is seen rather than
/// silently dropped, which would make the token on screen wrong.
pub mod font {
    use super::ScreenGeometry;

    pub const GLYPH_W: u32 = 5;
    pub const GLYPH_H: u32 = 7;
    /// Blank column between glyphs.
    pub const SPACING: u32 = 1;

    type Glyph = [&'static str; 7];

    const UNKNOWN: Glyph = [
        " ### ", "#   #", "    #", "   # ", "  #  ", "     ", "  #  ",
    ];

    pub fn glyph(c: char) -> Glyph {
        match c.to_ascii_uppercase() {
            ' ' => [
                "     ", "     ", "     ", "     ", "     ", "     ", "     ",
            ],
            '0' => [
                " ### ", "#   #", "#  ##", "# # #", "##  #", "#   #", " ### ",
            ],
            '1' => [
                "  #  ", " ##  ", "  #  ", "  #  ", "  #  ", "  #  ", " ### ",
            ],
            '2' => [
                " ### ", "#   #", "    #", "   # ", "  #  ", " #   ", "#####",
            ],
            '3' => [
                "#####", "   # ", "  #  ", "   # ", "    #", "#   #", " ### ",
            ],
            '4' => [
                "   # ", "  ## ", " # # ", "#  # ", "#####", "   # ", "   # ",
            ],
            '5' => [
                "#####", "#    ", "#### ", "    #", "    #", "#   #", " ### ",
            ],
            '6' => [
                "  ## ", " #   ", "#    ", "#### ", "#   #", "#   #", " ### ",
            ],
            '7' => [
                "#####", "    #", "   # ", "  #  ", " #   ", " #   ", " #   ",
            ],
            '8' => [
                " ### ", "#   #", "#   #", " ### ", "#   #", "#   #", " ### ",
            ],
            '9' => [
                " ### ", "#   #", "#   #", " ####", "    #", "   # ", " ##  ",
            ],
            'A' => [
                " ### ", "#   #", "#   #", "#####", "#   #", "#   #", "#   #",
            ],
            'B' => [
                "#### ", "#   #", "#   #", "#### ", "#   #", "#   #", "#### ",
            ],
            'C' => [
                " ### ", "#   #", "#    ", "#    ", "#    ", "#   #", " ### ",
            ],
            'D' => [
                "###  ", "#  # ", "#   #", "#   #", "#   #", "#  # ", "###  ",
            ],
            'E' => [
                "#####", "#    ", "#    ", "#### ", "#    ", "#    ", "#####",
            ],
            'F' => [
                "#####", "#    ", "#    ", "#### ", "#    ", "#    ", "#    ",
            ],
            'G' => [
                " ### ", "#   #", "#    ", "# ###", "#   #", "#   #", " ####",
            ],
            'H' => [
                "#   #", "#   #", "#   #", "#####", "#   #", "#   #", "#   #",
            ],
            'I' => [
                " ### ", "  #  ", "  #  ", "  #  ", "  #  ", "  #  ", " ### ",
            ],
            'J' => [
                "  ###", "   # ", "   # ", "   # ", "   # ", "#  # ", " ##  ",
            ],
            'K' => [
                "#   #", "#  # ", "# #  ", "##   ", "# #  ", "#  # ", "#   #",
            ],
            'L' => [
                "#    ", "#    ", "#    ", "#    ", "#    ", "#    ", "#####",
            ],
            'M' => [
                "#   #", "## ##", "# # #", "# # #", "#   #", "#   #", "#   #",
            ],
            'N' => [
                "#   #", "#   #", "##  #", "# # #", "#  ##", "#   #", "#   #",
            ],
            'O' => [
                " ### ", "#   #", "#   #", "#   #", "#   #", "#   #", " ### ",
            ],
            'P' => [
                "#### ", "#   #", "#   #", "#### ", "#    ", "#    ", "#    ",
            ],
            'Q' => [
                " ### ", "#   #", "#   #", "#   #", "# # #", "#  # ", " ## #",
            ],
            'R' => [
                "#### ", "#   #", "#   #", "#### ", "# #  ", "#  # ", "#   #",
            ],
            'S' => [
                " ####", "#    ", "#    ", " ### ", "    #", "    #", "#### ",
            ],
            'T' => [
                "#####", "  #  ", "  #  ", "  #  ", "  #  ", "  #  ", "  #  ",
            ],
            'U' => [
                "#   #", "#   #", "#   #", "#   #", "#   #", "#   #", " ### ",
            ],
            'V' => [
                "#   #", "#   #", "#   #", "#   #", "#   #", " # # ", "  #  ",
            ],
            'W' => [
                "#   #", "#   #", "#   #", "# # #", "# # #", "# # #", " # # ",
            ],
            'X' => [
                "#   #", "#   #", " # # ", "  #  ", " # # ", "#   #", "#   #",
            ],
            'Y' => [
                "#   #", "#   #", "#   #", " # # ", "  #  ", "  #  ", "  #  ",
            ],
            'Z' => [
                "#####", "    #", "   # ", "  #  ", " #   ", "#    ", "#####",
            ],
            '.' => [
                "     ", "     ", "     ", "     ", "     ", " ##  ", " ##  ",
            ],
            ':' => [
                "     ", " ##  ", " ##  ", "     ", " ##  ", " ##  ", "     ",
            ],
            '/' => [
                "     ", "    #", "   # ", "  #  ", " #   ", "#    ", "     ",
            ],
            '-' => [
                "     ", "     ", "     ", "#####", "     ", "     ", "     ",
            ],
            _ => UNKNOWN,
        }
    }

    /// Pixels a line of text takes across, at `scale`.
    pub fn text_width(text: &str, scale: u32) -> u32 {
        let n = text.chars().count() as u32;
        if n == 0 {
            return 0;
        }
        (n * GLYPH_W + (n - 1) * SPACING) * scale
    }

    /// Draw `text` with its top left at (`x`, `y`), clipped to the screen.
    pub fn draw_text(
        buf: &mut [(u8, u8, u8)],
        geo: ScreenGeometry,
        text: &str,
        x: u32,
        y: u32,
        scale: u32,
        color: (u8, u8, u8),
    ) {
        let scale = scale.max(1);
        let mut pen_x = x;
        for c in text.chars() {
            let g = glyph(c);
            for (row, line) in g.iter().enumerate() {
                for (col, cell) in line.bytes().enumerate() {
                    if cell != b'#' {
                        continue;
                    }
                    let px = pen_x + col as u32 * scale;
                    let py = y + row as u32 * scale;
                    super::fill_rect(buf, geo, px, py, scale, scale, color);
                }
            }
            pen_x += (GLYPH_W + SPACING) * scale;
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// Every picture must be exactly five wide and seven tall, or the
        /// glyph it draws is not the one in the source.
        #[test]
        fn every_glyph_is_five_by_seven() {
            let chars = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ .:/-?";
            for c in chars.chars() {
                let g = glyph(c);
                assert_eq!(g.len(), 7, "{c:?}");
                for line in g {
                    assert_eq!(line.len(), 5, "{c:?} row {line:?}");
                    assert!(
                        line.bytes().all(|b| b == b'#' || b == b' '),
                        "{c:?} row {line:?}"
                    );
                }
            }
        }

        /// The token alphabet has to be told apart at a glance, so no two of
        /// its letters may share a picture.
        #[test]
        fn token_alphabet_glyphs_are_distinct() {
            let alphabet = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
            let glyphs: Vec<Glyph> = alphabet.chars().map(glyph).collect();
            for (i, a) in glyphs.iter().enumerate() {
                for (j, b) in glyphs.iter().enumerate() {
                    if i != j {
                        assert_ne!(
                            a,
                            b,
                            "{:?} and {:?} look the same",
                            &alphabet[i..=i],
                            &alphabet[j..=j]
                        );
                    }
                }
            }
        }

        #[test]
        fn lowercase_draws_as_uppercase_and_unknowns_are_visible() {
            assert_eq!(glyph('a'), glyph('A'));
            assert_eq!(glyph('~'), UNKNOWN);
            assert_ne!(glyph('~'), glyph(' '));
        }

        #[test]
        fn text_width_counts_spacing_between_glyphs_only() {
            assert_eq!(text_width("", 1), 0);
            assert_eq!(text_width("A", 1), 5);
            assert_eq!(text_width("AB", 1), 11);
            assert_eq!(text_width("AB", 2), 22);
        }
    }
}

/// The address a screen of a given device draws with.
///
/// `None` for devices whose display is not a framebuffer this code can paint:
/// they show nothing here and pair by button press instead.
pub fn screen_geometry(device: &rayhunter::Device) -> Option<ScreenGeometry> {
    use rayhunter::Device;
    match device {
        Device::Orbic | Device::Tplink => Some(ScreenGeometry::square(128)),
        // The panel starts a few rows below the framebuffer's top. Measured
        // by eye: a six pixel status line is the thinnest that shows.
        Device::Moxee => Some(ScreenGeometry {
            width: 128,
            height: 128,
            top_inset: 6,
        }),
        Device::Wingtech => Some(ScreenGeometry {
            width: 160,
            height: 128,
            top_inset: 0,
        }),
        Device::Tmobile | Device::Pinephone | Device::Uz801 | Device::Netgear => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORBIC: ScreenGeometry = ScreenGeometry::square(128);
    const MOXEE: ScreenGeometry = ScreenGeometry {
        width: 128,
        height: 128,
        top_inset: 6,
    };

    /// The setup link the design settles on, in the form that keeps it in
    /// alphanumeric mode: 35 characters, which is a version 2 code.
    const SETUP_URL: &str = "HTTPS://192.168.1.1:8443/S/ABCDEFGH";

    #[test]
    fn the_setup_link_is_a_version_two_code() {
        let code = encode(SETUP_URL).unwrap();
        assert_eq!(code.version().value(), 2);
        assert_eq!(code.size(), 25);
        // Medium is what was asked for; the encoder may only raise it.
        assert!(matches!(
            code.error_correction_level(),
            QrCodeEcc::Medium | QrCodeEcc::Quartile | QrCodeEcc::High
        ));
    }

    /// Lowercase forces byte mode, which needs a bigger code for the same
    /// text. That is the whole reason the link is shouted.
    #[test]
    fn lowercase_costs_a_version() {
        let upper = encode(SETUP_URL).unwrap();
        let lower = encode(&SETUP_URL.to_lowercase()).unwrap();
        assert!(lower.version().value() > upper.version().value());
    }

    /// On the Orbic the biggest layout the design wants fits with its full
    /// margin and the caption under it.
    #[test]
    fn four_pixel_modules_with_caption_fit_the_orbic() {
        let l = layout(25, 4, true, 1, ORBIC).unwrap();
        assert_eq!(l.module_px, 4);
        assert_eq!(l.quiet_px, 8);
        assert_eq!(l.code_x, 14);
        // 8 quiet + 100 code + 8 quiet + 2 gap + 7 caption = 125, centred.
        assert_eq!(l.code_y, 9);
        assert_eq!(l.caption_y, Some(119));
        assert!(l.caption_y.unwrap() + font::GLYPH_H <= 128);
    }

    /// The Moxee loses six rows at the top. Keeping four pixel modules there
    /// costs a little margin, not the module size.
    #[test]
    fn the_moxee_keeps_its_module_size_and_gives_up_margin() {
        let l = layout(25, 4, true, 1, MOXEE).unwrap();
        assert_eq!(l.module_px, 4);
        assert_eq!(l.quiet_px, 6);
        assert!(l.code_y >= MOXEE.top_inset + l.quiet_px);
        assert!(l.caption_y.unwrap() + font::GLYPH_H <= 128);
    }

    #[test]
    fn without_a_caption_the_code_is_centred() {
        let l = layout(25, 4, false, 1, ORBIC).unwrap();
        assert_eq!(l.caption_y, None);
        // 116 of 128 used, 6 spare each side.
        assert_eq!(l.code_y, 6 + 8);
        assert_eq!(l.code_x, 14);
    }

    /// A code that cannot fit at the size asked for shrinks rather than
    /// failing, and one that cannot fit at all says so.
    #[test]
    fn too_big_shrinks_then_gives_up() {
        let l = layout(25, 6, true, 1, ORBIC).unwrap();
        assert!(l.module_px < 6);
        assert!(layout(177, 4, false, 1, ORBIC).is_none());
        assert!(layout(25, 4, true, 1, ScreenGeometry::square(32)).is_none());
    }

    #[test]
    fn nothing_is_drawn_above_the_inset() {
        let code = encode(SETUP_URL).unwrap();
        let l = layout(25, 4, true, 1, MOXEE).unwrap();
        let px = render(&code, l, Some("ABCD EFGH"), MOXEE);
        assert_eq!(px.len(), 128 * 128);
        for y in 0..MOXEE.top_inset {
            for x in 0..128 {
                assert_eq!(px[(y * 128 + x) as usize], WHITE, "dark pixel at {x},{y}");
            }
        }
    }

    /// The rendered modules must be the encoder's modules, pixel for pixel,
    /// or the screen shows a code that says something else.
    #[test]
    fn rendered_modules_match_the_code() {
        let code = encode(SETUP_URL).unwrap();
        let l = layout(25, 4, true, 1, ORBIC).unwrap();
        let px = render(&code, l, Some("ABCD EFGH"), ORBIC);
        for my in 0..25 {
            for mx in 0..25 {
                let want = if code.get_module(mx, my) {
                    BLACK
                } else {
                    WHITE
                };
                for dy in 0..4 {
                    for dx in 0..4 {
                        let x = l.code_x + mx as u32 * 4 + dx;
                        let y = l.code_y + my as u32 * 4 + dy;
                        assert_eq!(px[(y * 128 + x) as usize], want, "module {mx},{my}");
                    }
                }
            }
        }
        // The finder pattern's top left module is always dark.
        assert_eq!(px[(l.code_y * 128 + l.code_x) as usize], BLACK);
        // And the quiet zone is clean.
        let above = l.code_y - 1;
        assert!((0..128).all(|x| px[(above * 128 + x) as usize] == WHITE));
    }

    #[test]
    fn the_caption_lands_where_the_layout_says() {
        let code = encode(SETUP_URL).unwrap();
        let l = layout(25, 4, true, 1, ORBIC).unwrap();
        let cy = l.caption_y.unwrap();
        let px = render(&code, l, Some("ABCD EFGH"), ORBIC);
        let dark_in = |y: u32| {
            (0..128)
                .filter(|&x| px[(y * 128 + x) as usize] == BLACK)
                .count()
        };
        // Something is drawn on every caption row.
        assert!((cy..cy + font::GLYPH_H).all(|y| dark_in(y) > 0));
        // The gap between code and caption is empty.
        assert_eq!(dark_in(cy - 1), 0);
        // And so is everything below the caption.
        assert!((cy + font::GLYPH_H..128).all(|y| dark_in(y) == 0));
    }

    #[test]
    fn the_svg_has_one_square_per_dark_module() {
        let code = encode(SETUP_URL).unwrap();
        let s = svg(&code, 2);
        let dark = (0..25)
            .flat_map(|y| (0..25).map(move |x| (x, y)))
            .filter(|&(x, y)| code.get_module(x, y))
            .count();
        assert_eq!(s.matches("h1v1h-1z").count(), dark);
        assert!(s.contains("viewBox=\"0 0 29 29\""));
    }

    #[test]
    fn text_screens_and_banners_are_the_right_size() {
        let px = text_screen(&[("TERMINAL", 2), ("CODE", 2), ("4815", 4)], ORBIC);
        assert_eq!(px.len(), 128 * 128);
        assert!(px.contains(&BLACK));
        let strip = banner_strip(128, "TERMINAL ACTIVE");
        assert_eq!(strip.len(), (128 * BANNER_HEIGHT) as usize);
        assert!(strip.contains(&WHITE));
        // Top and bottom rows are margin.
        assert!(strip[..128].iter().all(|&p| p == BLACK));
        assert!(
            strip[(128 * (BANNER_HEIGHT - 1)) as usize..]
                .iter()
                .all(|&p| p == BLACK)
        );
    }

    #[test]
    fn every_supported_screen_has_a_geometry_and_the_rest_do_not() {
        use rayhunter::Device;
        assert!(screen_geometry(&Device::Orbic).is_some());
        assert!(screen_geometry(&Device::Moxee).is_some());
        assert!(screen_geometry(&Device::Tplink).is_some());
        assert!(screen_geometry(&Device::Pinephone).is_none());
    }
}
