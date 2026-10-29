//! UI theme: the single colour palette of cmd_ai (constitution, principle VII).
//!
//! This is the ONLY file of `src/` allowed to contain colour literals (`Color::Rgb`,
//! `Color::Indexed`): the CI refuses them anywhere else.
//!
//! # Method
//!
//! Every colour is derived from a documented OKLCH constant (perceptual lightness `L`,
//! chroma `C`, hue `h` in degrees), converted OKLCH → OKLab → linear sRGB → gamma sRGB.
//! Out-of-gamut colours keep their lightness and hue and lose chroma (binary search), but
//! the constants below are chosen inside the gamut, so what is documented is what is shown.
//!
//! The criteria are measurable and checked by the unit tests of this module:
//!
//! - **Contrast**: every role reaches the WCAG 2 AA ratio (≥ 4.5:1) against representative
//!   backgrounds — dark `#000000`, `#1E1E1E`, `#282C34`, `#0C0C0C`; light `#FFFFFF`,
//!   `#F5F5F5`, `#FDF6E3` — and so does its 256-colour approximation.
//! - **Colour-vision deficiencies**: hues follow the Okabe–Ito families. Protanopia and
//!   deuteranopia collapse the red/green axis (OKLab `a`) but keep the blue/yellow one
//!   (OKLab `b`), so the critical pairs sit on opposite sides of that axis: plan (blue) vs
//!   bypass (orange), success (blue-green) vs error (vermilion) and warning (amber).
//! - **Never colour alone**: errors, warnings, admin and both modes are also bold, and the
//!   callers always print a label or a symbol; with [`ColorLevel::None`] the interface
//!   stays readable.
//! - **One hue per role**: a role keeps its hue on both backgrounds; only `L` and `C` move
//!   (light colours on dark terminals, dark colours on light ones).
//!
//! | Role        | h (°) | Dark: L, C → sRGB, min contrast | Light: L, C → sRGB, min contrast |
//! | ----------- | ----- | ------------------------------- | -------------------------------- |
//! | `Text`      | 250   | 0.93, 0.010 → `#E3E8EE`, 11.4   | 0.25, 0.015 → `#1C2229`, 14.7    |
//! | `Dim`       | 250   | 0.72, 0.012 → `#9FA5AC`, 5.6    | 0.50, 0.012 → `#5E646A`, 5.5     |
//! | `Accent`    | 237   | 0.78, 0.105 → `#72C2F2`, 7.1    | 0.48, 0.100 → `#0E658F`, 5.9     |
//! | `Success`   | 185   | 0.80, 0.125 → `#42D8C8`, 7.9    | 0.46, 0.075 → `#14655D`, 6.3     |
//! | `Warning`   | 85    | 0.84, 0.140 → `#F4C352`, 8.5    | 0.52, 0.100 → `#836412`, 5.1     |
//! | `Error`     | 35    | 0.72, 0.155 → `#F57C5F`, 5.3    | 0.47, 0.160 → `#A12A07`, 6.8     |
//! | `Admin`     | 350   | 0.76, 0.130 → `#EE8DBD`, 6.1    | 0.46, 0.155 → `#922764`, 7.1     |
//! | `Plan`      | 265   | 0.75, 0.110 → `#8CADF4`, 6.3    | 0.47, 0.150 → `#3054AE`, 6.4     |
//! | `Bypass`    | 58    | 0.79, 0.135 → `#FBA35D`, 7.0    | 0.48, 0.110 → `#8A4B0B`, 6.2     |
//! | `Reasoning` | 300   | 0.72, 0.040 → `#A89FBB`, 5.6    | 0.50, 0.045 → `#675E7A`, 5.6     |
//! | `Command`   | 205   | 0.90, 0.060 → `#B0EAF1`, 10.6   | 0.40, 0.060 → `#155157`, 8.2     |
//!
//! Lower colour levels are derived from the same values: nearest entry of the xterm
//! 6×6×6 cube or grey ramp for 256 colours; for 16 colours, a hue-preserving
//! classification (nearest primary hue, bright variant when `L` ≥ 0.62, greys for low
//! chroma). The 16 colours themselves belong to the user's terminal palette, so only the
//! hue family and the bold emphasis are guaranteed at that level.

use ratatui::style::{Color, Modifier, Style};

/// A gamma-encoded sRGB colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Rgb {
        Rgb { r, g, b }
    }
}

/// Terminal background family the palette is tuned for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Background {
    Dark,
    Light,
}

/// Semantic role of a piece of text; the only way to ask for a colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    /// Regular text.
    Text,
    /// Secondary text (hints, durations, separators).
    Dim,
    /// Identity colour: prompt, headings, focus.
    Accent,
    /// A command or a task that succeeded.
    Success,
    /// Something needs attention.
    Warning,
    /// A failure.
    Error,
    /// Administrator / root privileges.
    Admin,
    /// Plan mode badge.
    Plan,
    /// Bypass mode badge.
    Bypass,
    /// Model reasoning (shown dimmed).
    Reasoning,
    /// The exact text of a shell command.
    Command,
}

impl Role {
    /// Every role, in declaration order.
    pub const ALL: [Role; 11] = [
        Role::Text,
        Role::Dim,
        Role::Accent,
        Role::Success,
        Role::Warning,
        Role::Error,
        Role::Admin,
        Role::Plan,
        Role::Bypass,
        Role::Reasoning,
        Role::Command,
    ];
}

/// How many colours the terminal can show.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ColorLevel {
    /// 24-bit colour (`38;2;r;g;b`).
    TrueColor,
    /// xterm 256-colour palette (`38;5;n`).
    Ansi256,
    /// The 16 named colours of the terminal palette.
    Ansi16,
    /// No colour at all (`NO_COLOR`, `--color none`, not a terminal).
    None,
}

// ---- colour science ---------------------------------------------------------------------

/// Tolerance of the gamut test, in linear sRGB (rounding noise of the OKLab matrices).
const GAMUT_EPSILON: f64 = 1e-4;

fn finite_or(x: f64, default: f64) -> f64 {
    if x.is_finite() { x } else { default }
}

/// OKLab → linear sRGB (Björn Ottosson's reference matrices).
fn oklab_to_linear_srgb(l: f64, a: f64, b: f64) -> [f64; 3] {
    let l_ = l + 0.396_337_777_4 * a + 0.215_803_757_3 * b;
    let m_ = l - 0.105_561_345_8 * a - 0.063_854_172_8 * b;
    let s_ = l - 0.089_484_177_5 * a - 1.291_485_548_0 * b;
    let (l3, m3, s3) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
    [
        4.076_741_662_1 * l3 - 3.307_711_591_3 * m3 + 0.230_969_929_2 * s3,
        -1.268_438_004_6 * l3 + 2.609_757_401_1 * m3 - 0.341_319_396_5 * s3,
        -0.004_196_086_3 * l3 - 0.703_418_614_7 * m3 + 1.707_614_701_0 * s3,
    ]
}

fn oklch_to_linear(l: f64, c: f64, h_deg: f64) -> [f64; 3] {
    let h = h_deg.to_radians();
    oklab_to_linear_srgb(l, c * h.cos(), c * h.sin())
}

fn in_gamut(rgb: [f64; 3]) -> bool {
    rgb.iter()
        .all(|x| (-GAMUT_EPSILON..=1.0 + GAMUT_EPSILON).contains(x))
}

/// Linear channel → 8-bit gamma-encoded sRGB.
fn encode_channel(x: f64) -> u8 {
    let x = x.clamp(0.0, 1.0);
    let v = if x <= 0.003_130_8 {
        12.92 * x
    } else {
        1.055 * x.powf(1.0 / 2.4) - 0.055
    };
    (v * 255.0).round().clamp(0.0, 255.0) as u8
}

/// 8-bit gamma-encoded sRGB → linear channel (IEC 61966-2-1 threshold 0.04045).
fn decode_channel(v: u8) -> f64 {
    let x = f64::from(v) / 255.0;
    if x <= 0.040_45 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}

/// OKLCH → sRGB. `l` ≤ 0 is black and `l` ≥ 1 white (as in CSS Color 4 gamut mapping), a
/// negative or non-finite chroma counts as 0, a non-finite hue as 0°. Out-of-gamut
/// colours keep `l` and `h` and lose chroma.
pub fn oklch_to_rgb(l: f64, c: f64, h_deg: f64) -> Rgb {
    let l = finite_or(l, 0.0);
    if l <= 0.0 {
        return Rgb::new(0, 0, 0);
    }
    if l >= 1.0 {
        return Rgb::new(255, 255, 255);
    }
    let c = finite_or(c, 0.0).max(0.0);
    let h = finite_or(h_deg, 0.0);
    let mut linear = oklch_to_linear(l, c, h);
    if !in_gamut(linear) {
        // Chroma 0 (a grey) is always inside the gamut: search the largest chroma that fits.
        let (mut lo, mut hi) = (0.0_f64, c);
        for _ in 0..32 {
            let mid = 0.5 * (lo + hi);
            if in_gamut(oklch_to_linear(l, mid, h)) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        linear = oklch_to_linear(l, lo, h);
    }
    Rgb::new(
        encode_channel(linear[0]),
        encode_channel(linear[1]),
        encode_channel(linear[2]),
    )
}

/// sRGB → OKLCH `(l, c, h_deg)`, hue in `0..360`.
pub fn rgb_to_oklch(c: Rgb) -> (f64, f64, f64) {
    let (r, g, b) = (
        decode_channel(c.r),
        decode_channel(c.g),
        decode_channel(c.b),
    );
    let l = 0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b;
    let m = 0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b;
    let s = 0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b;
    let (l_, m_, s_) = (l.cbrt(), m.cbrt(), s.cbrt());
    let ok_l = 0.210_454_255_3 * l_ + 0.793_617_785_0 * m_ - 0.004_072_046_8 * s_;
    let ok_a = 1.977_998_495_1 * l_ - 2.428_592_205_0 * m_ + 0.450_593_709_9 * s_;
    let ok_b = 0.025_904_037_1 * l_ + 0.782_771_766_2 * m_ - 0.808_675_766_0 * s_;
    let chroma = ok_a.hypot(ok_b);
    let hue = ok_b.atan2(ok_a).to_degrees().rem_euclid(360.0);
    // rem_euclid can round up to exactly 360.0 for tiny negative angles.
    (ok_l, chroma, if hue >= 360.0 { 0.0 } else { hue })
}

/// WCAG 2 relative luminance (0 = black, 1 = white).
pub fn relative_luminance(c: Rgb) -> f64 {
    0.2126 * decode_channel(c.r) + 0.7152 * decode_channel(c.g) + 0.0722 * decode_channel(c.b)
}

/// WCAG 2 contrast ratio, from 1 (identical) to 21 (black on white); symmetric.
pub fn contrast_ratio(a: Rgb, b: Rgb) -> f64 {
    let (la, lb) = (relative_luminance(a), relative_luminance(b));
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

// ---- palette ------------------------------------------------------------------------------

/// One OKLCH constant of the palette.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Spec {
    l: f64,
    c: f64,
    h: f64,
}

/// Hue of each role (degrees), shared by both backgrounds (Okabe–Ito families).
const fn role_hue(role: Role) -> f64 {
    match role {
        Role::Text | Role::Dim => 250.0, // neutral, faintly cool
        Role::Accent => 237.0,           // sky blue
        Role::Success => 185.0,          // blue-green
        Role::Warning => 85.0,           // amber
        Role::Error => 35.0,             // vermilion
        Role::Admin => 350.0,            // reddish purple
        Role::Plan => 265.0,             // blue: the safe mode
        Role::Bypass => 58.0,            // orange: the risky mode
        Role::Reasoning => 300.0,        // muted lavender
        Role::Command => 205.0,          // pale cyan
    }
}

/// The documented OKLCH constant of `role` on `bg` (see the module table).
const fn spec(bg: Background, role: Role) -> Spec {
    let (l, c) = match bg {
        Background::Dark => match role {
            Role::Text => (0.93, 0.010),
            Role::Dim => (0.72, 0.012),
            Role::Accent => (0.78, 0.105),
            Role::Success => (0.80, 0.125),
            Role::Warning => (0.84, 0.140),
            Role::Error => (0.72, 0.155),
            Role::Admin => (0.76, 0.130),
            Role::Plan => (0.75, 0.110),
            Role::Bypass => (0.79, 0.135),
            Role::Reasoning => (0.72, 0.040),
            Role::Command => (0.90, 0.060),
        },
        Background::Light => match role {
            Role::Text => (0.25, 0.015),
            Role::Dim => (0.50, 0.012),
            Role::Accent => (0.48, 0.100),
            Role::Success => (0.46, 0.075),
            Role::Warning => (0.52, 0.100),
            Role::Error => (0.47, 0.160),
            Role::Admin => (0.46, 0.155),
            Role::Plan => (0.47, 0.150),
            Role::Bypass => (0.48, 0.110),
            Role::Reasoning => (0.50, 0.045),
            Role::Command => (0.40, 0.060),
        },
    };
    Spec {
        l,
        c,
        h: role_hue(role),
    }
}

/// The sRGB colour of `role` on a `bg` background.
pub fn palette_color(bg: Background, role: Role) -> Rgb {
    let s = spec(bg, role);
    oklch_to_rgb(s.l, s.c, s.h)
}

// ---- detection ------------------------------------------------------------------------------

/// Colour level from the `ui.color` / `--color` choice (`auto|truecolor|256|16|none`).
///
/// Explicit choices are honoured as is. `auto` (and any unknown value): `NO_COLOR`
/// non-empty → none; not a terminal → none; `TERM=dumb` → none; `COLORTERM`
/// `truecolor`/`24bit`, `WT_SESSION`, `TERM_PROGRAM` `iTerm.app`/`WezTerm`/`vscode`, or
/// Windows without `TERM` → truecolor; `TERM` containing `256color` or `TERM_PROGRAM`
/// `Apple_Terminal` → 256; otherwise 16. Empty variables count as absent.
pub fn detect_level(
    choice: &str,
    env: &dyn Fn(&str) -> Option<String>,
    is_tty: bool,
) -> ColorLevel {
    detect_level_on(choice, env, is_tty, cfg!(windows))
}

fn detect_level_on(
    choice: &str,
    env: &dyn Fn(&str) -> Option<String>,
    is_tty: bool,
    windows: bool,
) -> ColorLevel {
    match choice.trim().to_ascii_lowercase().as_str() {
        "truecolor" | "24bit" => return ColorLevel::TrueColor,
        "256" | "ansi256" => return ColorLevel::Ansi256,
        "16" | "ansi16" => return ColorLevel::Ansi16,
        "none" => return ColorLevel::None,
        _ => {} // "auto", or a value config validation should have refused: be lenient.
    }
    let var = |name: &str| env(name).filter(|v| !v.is_empty());
    if var("NO_COLOR").is_some() || !is_tty {
        return ColorLevel::None;
    }
    let term = var("TERM").map(|t| t.to_ascii_lowercase());
    // A terminal that declares itself dumb gets no escape sequence, whatever else says.
    if term.as_deref() == Some("dumb") {
        return ColorLevel::None;
    }
    let colorterm = var("COLORTERM").map(|t| t.to_ascii_lowercase());
    let program = var("TERM_PROGRAM");
    if matches!(colorterm.as_deref(), Some("truecolor" | "24bit"))
        || var("WT_SESSION").is_some()
        || matches!(program.as_deref(), Some("iTerm.app" | "WezTerm" | "vscode"))
        || (windows && term.is_none())
    {
        return ColorLevel::TrueColor;
    }
    if term.as_deref().is_some_and(|t| t.contains("256color"))
        || program.as_deref() == Some("Apple_Terminal")
    {
        return ColorLevel::Ansi256;
    }
    ColorLevel::Ansi16
}

/// Background from the `ui.theme` choice (`auto|dark|light`).
///
/// `auto` (and any unknown value): `COLORFGBG` (`fg;bg` or `fg;default;bg`) whose last
/// field is 7 or 15 → light; otherwise dark (OSC 11 queries are unreliable under conhost).
pub fn detect_background(choice: &str, env: &dyn Fn(&str) -> Option<String>) -> Background {
    match choice.trim().to_ascii_lowercase().as_str() {
        "dark" => Background::Dark,
        "light" => Background::Light,
        _ => {
            let bg = env("COLORFGBG").and_then(|v| {
                v.rsplit(';')
                    .next()
                    .and_then(|field| field.trim().parse::<u8>().ok())
            });
            if matches!(bg, Some(7 | 15)) {
                Background::Light
            } else {
                Background::Dark
            }
        }
    }
}

// ---- quantisation ---------------------------------------------------------------------------

/// Channel levels of the xterm 6×6×6 cube (indices 16..=231).
const CUBE_LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];

fn nearest_cube_level(v: u8) -> usize {
    (0..CUBE_LEVELS.len())
        .min_by_key(|&i| CUBE_LEVELS[i].abs_diff(v))
        .unwrap_or(0)
}

fn distance2(a: Rgb, b: Rgb) -> u32 {
    let d = |x: u8, y: u8| u32::from(x.abs_diff(y)).pow(2);
    d(a.r, b.r) + d(a.g, b.g) + d(a.b, b.b)
}

/// Nearest xterm 256-colour index (6×6×6 cube 16..=231 or grey ramp 232..=255, sRGB
/// Euclidean distance; the cube wins ties).
pub fn rgb_to_ansi256(c: Rgb) -> u8 {
    let (ri, gi, bi) = (
        nearest_cube_level(c.r),
        nearest_cube_level(c.g),
        nearest_cube_level(c.b),
    );
    let cube = Rgb::new(CUBE_LEVELS[ri], CUBE_LEVELS[gi], CUBE_LEVELS[bi]);
    let cube_index = 16 + 36 * ri + 6 * gi + bi;
    // Grey ramp: 8, 18, ..., 238; the nearest one to the channel mean minimises the distance.
    let mean = (f64::from(c.r) + f64::from(c.g) + f64::from(c.b)) / 3.0;
    let step = ((mean - 8.0) / 10.0).round().clamp(0.0, 23.0) as u8;
    let grey_value = 8 + 10 * step;
    let grey = Rgb::new(grey_value, grey_value, grey_value);
    if distance2(c, grey) < distance2(c, cube) {
        232 + step
    } else {
        cube_index as u8
    }
}

/// OKLab hues of the pure sRGB primaries and the ANSI index of their family.
const ANSI16_HUES: [(f64, u8); 6] = [
    (29.23, 1),  // red
    (109.77, 3), // yellow
    (142.50, 2), // green
    (194.77, 6), // cyan
    (264.05, 4), // blue
    (328.36, 5), // magenta
];
/// Below this OKLCH chroma a colour is treated as a grey.
const ANSI16_GREY_CHROMA: f64 = 0.05;
/// From this OKLCH lightness on, the bright variant (index + 8) is used.
const ANSI16_BRIGHT_LIGHTNESS: f64 = 0.62;

fn hue_distance(a: f64, b: f64) -> f64 {
    let d = (a - b).rem_euclid(360.0);
    d.min(360.0 - d)
}

/// 16-colour index (0..=15) by hue-preserving classification: greys by lightness
/// (black, dark grey, grey, white); other colours take the family of the nearest primary
/// hue, bright when OKLCH lightness ≥ 0.62.
pub fn rgb_to_ansi16(c: Rgb) -> u8 {
    let (l, chroma, h) = rgb_to_oklch(c);
    if chroma < ANSI16_GREY_CHROMA {
        return if l < 0.35 {
            0
        } else if l < ANSI16_BRIGHT_LIGHTNESS {
            8
        } else if l < 0.85 {
            7
        } else {
            15
        };
    }
    let family = ANSI16_HUES
        .iter()
        .min_by(|a, b| hue_distance(a.0, h).total_cmp(&hue_distance(b.0, h)))
        .map_or(1, |&(_, index)| index);
    if l >= ANSI16_BRIGHT_LIGHTNESS {
        family + 8
    } else {
        family
    }
}

/// Named ratatui colour of a 16-colour index.
fn ansi16_color(index: u8) -> Color {
    match index {
        0 => Color::Black,
        1 => Color::Red,
        2 => Color::Green,
        3 => Color::Yellow,
        4 => Color::Blue,
        5 => Color::Magenta,
        6 => Color::Cyan,
        7 => Color::Gray,
        8 => Color::DarkGray,
        9 => Color::LightRed,
        10 => Color::LightGreen,
        11 => Color::LightYellow,
        12 => Color::LightBlue,
        13 => Color::LightMagenta,
        14 => Color::LightCyan,
        _ => Color::White,
    }
}

// ---- Theme ----------------------------------------------------------------------------------

/// Roles that are also bold, so that they never rely on colour alone.
fn is_emphasised(role: Role) -> bool {
    matches!(
        role,
        Role::Error | Role::Warning | Role::Admin | Role::Plan | Role::Bypass
    )
}

/// Colour level + background: everything needed to style a role.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    pub level: ColorLevel,
    pub background: Background,
}

impl Theme {
    pub fn new(level: ColorLevel, background: Background) -> Theme {
        Theme { level, background }
    }

    /// ratatui style of `role`: foreground only (the terminal background is never
    /// painted), bold for error, warning, admin, plan and bypass at every level.
    pub fn style(&self, role: Role) -> Style {
        let base = if is_emphasised(role) {
            Style::new().add_modifier(Modifier::BOLD)
        } else {
            Style::new()
        };
        let rgb = palette_color(self.background, role);
        match self.level {
            ColorLevel::TrueColor => base.fg(Color::Rgb(rgb.r, rgb.g, rgb.b)),
            ColorLevel::Ansi256 => base.fg(Color::Indexed(rgb_to_ansi256(rgb))),
            ColorLevel::Ansi16 => base.fg(ansi16_color(rgb_to_ansi16(rgb))),
            ColorLevel::None => base,
        }
    }

    /// SGR parameters selecting the colour of `role`, `None` without colour.
    fn sgr(&self, role: Role) -> Option<String> {
        let rgb = palette_color(self.background, role);
        let colour = match self.level {
            ColorLevel::TrueColor => format!("38;2;{};{};{}", rgb.r, rgb.g, rgb.b),
            ColorLevel::Ansi256 => format!("38;5;{}", rgb_to_ansi256(rgb)),
            ColorLevel::Ansi16 => {
                let index = rgb_to_ansi16(rgb);
                if index < 8 {
                    format!("3{index}")
                } else {
                    format!("9{}", index - 8)
                }
            }
            ColorLevel::None => return None,
        };
        Some(if is_emphasised(role) {
            format!("1;{colour}")
        } else {
            colour
        })
    }

    /// `text` wrapped in ANSI SGR sequences for plain (non-TUI) output. Each non-empty line
    /// is wrapped on its own, so interleaved output never inherits a colour. Without colour
    /// the text is returned unchanged.
    pub fn paint(&self, role: Role, text: &str) -> String {
        let Some(sgr) = self.sgr(role) else {
            return text.to_string();
        };
        let mut out = String::with_capacity(text.len() + 16);
        for (i, line) in text.split('\n').enumerate() {
            if i > 0 {
                out.push('\n');
            }
            if !line.is_empty() {
                out.push_str("\u{1b}[");
                out.push_str(&sgr);
                out.push('m');
                out.push_str(line);
                out.push_str("\u{1b}[0m");
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Representative terminal backgrounds (constitution budget: contrast >= 4.5:1).
    /// Dark: pure black, VS Code / Windows Terminal "One Half Dark"-like greys, Campbell.
    const DARK_BACKGROUNDS: [Rgb; 4] = [
        Rgb::new(0x00, 0x00, 0x00),
        Rgb::new(0x1E, 0x1E, 0x1E),
        Rgb::new(0x28, 0x2C, 0x34),
        Rgb::new(0x0C, 0x0C, 0x0C),
    ];
    /// Light: pure white, light grey, Solarized Light base3.
    const LIGHT_BACKGROUNDS: [Rgb; 3] = [
        Rgb::new(0xFF, 0xFF, 0xFF),
        Rgb::new(0xF5, 0xF5, 0xF5),
        Rgb::new(0xFD, 0xF6, 0xE3),
    ];
    const MIN_CONTRAST: f64 = 4.5;

    fn backgrounds(bg: Background) -> &'static [Rgb] {
        match bg {
            Background::Dark => &DARK_BACKGROUNDS,
            Background::Light => &LIGHT_BACKGROUNDS,
        }
    }

    fn close(a: Rgb, b: Rgb, tol: u8) -> bool {
        a.r.abs_diff(b.r) <= tol && a.g.abs_diff(b.g) <= tol && a.b.abs_diff(b.b) <= tol
    }

    /// The standard xterm 256-colour values (indices 16..=255).
    fn xterm256(index: u8) -> Rgb {
        const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
        assert!(index >= 16, "only the cube and the grey ramp are standard");
        if index >= 232 {
            let v = 8 + 10 * (index - 232);
            return Rgb::new(v, v, v);
        }
        let i = usize::from(index - 16);
        Rgb::new(LEVELS[i / 36], LEVELS[(i / 6) % 6], LEVELS[i % 6])
    }

    fn oklab_b(c: Rgb) -> f64 {
        let (_, chroma, hue) = rgb_to_oklch(c);
        chroma * hue.to_radians().sin()
    }

    /// Environment variables of a test case.
    type Vars = &'static [(&'static str, &'static str)];

    fn env_from<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        let map: HashMap<&str, &str> = pairs.iter().copied().collect();
        move |k: &str| map.get(k).map(|v| (*v).to_string())
    }

    // ---- colour science -------------------------------------------------------------

    #[test]
    fn oklch_red_matches_srgb_red() {
        let red = oklch_to_rgb(0.628, 0.2577, 29.23);
        assert!(close(red, Rgb::new(255, 0, 0), 2), "{red:?}");
    }

    #[test]
    fn oklch_white_black_and_grey() {
        assert_eq!(oklch_to_rgb(1.0, 0.0, 0.0), Rgb::new(255, 255, 255));
        assert_eq!(oklch_to_rgb(0.0, 0.0, 0.0), Rgb::new(0, 0, 0));
        let grey = oklch_to_rgb(0.5, 0.0, 123.0);
        assert_eq!(grey.r, grey.g);
        assert_eq!(grey.g, grey.b);
        // L = 0.5 => linear 0.125 => sRGB 0.3896 => 99.
        assert!(grey.r.abs_diff(99) <= 1, "{grey:?}");
    }

    #[test]
    fn oklch_known_secondary_colours() {
        // Reference values from the OKLab definition (Björn Ottosson).
        assert!(close(
            oklch_to_rgb(0.4520, 0.3132, 264.05),
            Rgb::new(0, 0, 255),
            2
        ));
        assert!(close(
            oklch_to_rgb(0.8664, 0.2948, 142.50),
            Rgb::new(0, 255, 0),
            2
        ));
        assert!(close(
            oklch_to_rgb(0.9680, 0.2110, 109.77),
            Rgb::new(255, 255, 0),
            2
        ));
    }

    #[test]
    fn oklch_out_of_gamut_reduces_chroma_and_keeps_lightness_and_hue() {
        let c = oklch_to_rgb(0.7, 0.4, 150.0);
        let (l, chroma, hue) = rgb_to_oklch(c);
        assert!((l - 0.7).abs() < 0.01, "lightness {l}");
        assert!(chroma < 0.4 && chroma > 0.1, "chroma {chroma}");
        assert!((hue - 150.0).abs() < 3.0, "hue {hue}");
        // L = 1 cannot carry any chroma: pure white.
        assert_eq!(oklch_to_rgb(1.0, 0.3, 40.0), Rgb::new(255, 255, 255));
    }

    #[test]
    fn oklch_sanitises_out_of_range_inputs() {
        assert_eq!(oklch_to_rgb(1.5, 0.0, 0.0), Rgb::new(255, 255, 255));
        assert_eq!(oklch_to_rgb(-0.2, 0.1, 0.0), Rgb::new(0, 0, 0));
        assert_eq!(oklch_to_rgb(f64::NAN, 0.1, 0.0), Rgb::new(0, 0, 0));
        let grey = oklch_to_rgb(0.6, -0.2, 10.0);
        assert!(
            grey.r == grey.g && grey.g == grey.b,
            "negative chroma = grey"
        );
        let a = oklch_to_rgb(0.7, 0.1, 30.0);
        assert_eq!(a, oklch_to_rgb(0.7, 0.1, 390.0), "hue wraps around");
        assert_eq!(a, oklch_to_rgb(0.7, 0.1, -330.0));
        assert_eq!(
            oklch_to_rgb(0.7, 0.1, f64::INFINITY),
            oklch_to_rgb(0.7, 0.1, 0.0)
        );
    }

    #[test]
    fn rgb_to_oklch_round_trips() {
        for c in [
            Rgb::new(255, 0, 0),
            Rgb::new(18, 52, 86),
            Rgb::new(250, 200, 100),
            Rgb::new(1, 2, 3),
            Rgb::new(128, 128, 128),
            Rgb::new(255, 255, 255),
        ] {
            let (l, ch, h) = rgb_to_oklch(c);
            assert!(close(oklch_to_rgb(l, ch, h), c, 1), "{c:?}");
            assert!((0.0..360.0).contains(&h), "hue {h}");
        }
        let (l, ch, _) = rgb_to_oklch(Rgb::new(255, 0, 0));
        assert!((l - 0.628).abs() < 0.001 && (ch - 0.2577).abs() < 0.001);
    }

    #[test]
    fn relative_luminance_reference_values() {
        assert!((relative_luminance(Rgb::new(255, 255, 255)) - 1.0).abs() < 1e-9);
        assert!(relative_luminance(Rgb::new(0, 0, 0)).abs() < 1e-12);
        assert!((relative_luminance(Rgb::new(128, 128, 128)) - 0.2158605).abs() < 1e-4);
        assert!((relative_luminance(Rgb::new(255, 0, 0)) - 0.2126).abs() < 1e-6);
        assert!((relative_luminance(Rgb::new(0, 255, 0)) - 0.7152).abs() < 1e-6);
        assert!((relative_luminance(Rgb::new(0, 0, 255)) - 0.0722).abs() < 1e-6);
    }

    #[test]
    fn contrast_ratio_reference_values() {
        let white = Rgb::new(255, 255, 255);
        let black = Rgb::new(0, 0, 0);
        assert!((contrast_ratio(white, black) - 21.0).abs() < 1e-9);
        assert!(
            (contrast_ratio(black, white) - 21.0).abs() < 1e-9,
            "symmetric"
        );
        assert!((contrast_ratio(white, white) - 1.0).abs() < 1e-12);
        // #777777 on white is the classic "just below AA" grey (4.48:1).
        let r = contrast_ratio(Rgb::new(0x77, 0x77, 0x77), white);
        assert!((r - 4.48).abs() < 0.01, "{r}");
    }

    // ---- palette ----------------------------------------------------------------------

    #[test]
    fn every_role_reaches_aa_contrast_on_every_representative_background() {
        for bg in [Background::Dark, Background::Light] {
            for role in Role::ALL {
                let fg = palette_color(bg, role);
                for &back in backgrounds(bg) {
                    let r = contrast_ratio(fg, back);
                    assert!(
                        r >= MIN_CONTRAST,
                        "{bg:?}/{role:?} {fg:?} on {back:?}: {r:.2}"
                    );
                }
            }
        }
    }

    #[test]
    fn ansi256_approximation_keeps_aa_contrast() {
        for bg in [Background::Dark, Background::Light] {
            for role in Role::ALL {
                let idx = rgb_to_ansi256(palette_color(bg, role));
                let fg = xterm256(idx);
                for &back in backgrounds(bg) {
                    let r = contrast_ratio(fg, back);
                    assert!(
                        r >= MIN_CONTRAST,
                        "{bg:?}/{role:?} -> {idx} {fg:?} on {back:?}: {r:.2}"
                    );
                }
            }
        }
    }

    #[test]
    fn palette_constants_are_inside_the_srgb_gamut() {
        // No hidden clipping: the documented OKLCH constant is what the user sees.
        for bg in [Background::Dark, Background::Light] {
            for role in Role::ALL {
                let s = spec(bg, role);
                let (l, c, h) = rgb_to_oklch(palette_color(bg, role));
                assert!((l - s.l).abs() < 0.01, "{bg:?}/{role:?} L {l} vs {}", s.l);
                assert!((c - s.c).abs() < 0.01, "{bg:?}/{role:?} C {c} vs {}", s.c);
                if s.c >= 0.03 {
                    let dh = (h - s.h).rem_euclid(360.0);
                    assert!(dh.min(360.0 - dh) < 3.0, "{bg:?}/{role:?} h {h} vs {}", s.h);
                }
            }
        }
    }

    #[test]
    fn palette_matches_the_documented_table() {
        // The module documentation lists these values: changing a constant must update it.
        let expected: [(Role, u32, u32); 11] = [
            (Role::Text, 0xE3E8EE, 0x1C2229),
            (Role::Dim, 0x9FA5AC, 0x5E646A),
            (Role::Accent, 0x72C2F2, 0x0E658F),
            (Role::Success, 0x42D8C8, 0x14655D),
            (Role::Warning, 0xF4C352, 0x836412),
            (Role::Error, 0xF57C5F, 0xA12A07),
            (Role::Admin, 0xEE8DBD, 0x922764),
            (Role::Plan, 0x8CADF4, 0x3054AE),
            (Role::Bypass, 0xFBA35D, 0x8A4B0B),
            (Role::Reasoning, 0xA89FBB, 0x675E7A),
            (Role::Command, 0xB0EAF1, 0x155157),
        ];
        let hex = |c: Rgb| format!("#{:02X}{:02X}{:02X}", c.r, c.g, c.b);
        for (role, dark, light) in expected {
            assert_eq!(
                hex(palette_color(Background::Dark, role)),
                format!("#{dark:06X}"),
                "{role:?} dark"
            );
            assert_eq!(
                hex(palette_color(Background::Light, role)),
                format!("#{light:06X}"),
                "{role:?} light"
            );
        }
    }

    #[test]
    fn each_role_keeps_its_hue_across_themes() {
        for role in Role::ALL {
            assert_eq!(
                spec(Background::Dark, role).h,
                spec(Background::Light, role).h,
                "{role:?}"
            );
        }
    }

    #[test]
    fn critical_pairs_differ_on_the_blue_yellow_axis() {
        // Protanopia and deuteranopia collapse the red/green axis (OKLab a) and keep the
        // blue/yellow one (OKLab b): critical pairs must sit on opposite sides of it.
        for bg in [Background::Dark, Background::Light] {
            for (cool, warm) in [
                (Role::Plan, Role::Bypass),
                (Role::Success, Role::Error),
                (Role::Success, Role::Warning),
            ] {
                let b_cool = oklab_b(palette_color(bg, cool));
                let b_warm = oklab_b(palette_color(bg, warm));
                assert!(
                    b_cool < 0.0 && b_warm > 0.0,
                    "{bg:?} {cool:?}={b_cool} {warm:?}={b_warm}"
                );
                assert!(
                    b_warm - b_cool >= 0.08,
                    "{bg:?} {cool:?}/{warm:?}: {}",
                    b_warm - b_cool
                );
            }
        }
    }

    #[test]
    fn dim_and_reasoning_are_dimmer_than_text() {
        for bg in [Background::Dark, Background::Light] {
            for &back in backgrounds(bg) {
                let text = contrast_ratio(palette_color(bg, Role::Text), back);
                for role in [Role::Dim, Role::Reasoning] {
                    assert!(
                        contrast_ratio(palette_color(bg, role), back) < text,
                        "{bg:?} {role:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn roles_are_distinct_colours() {
        for bg in [Background::Dark, Background::Light] {
            for (i, a) in Role::ALL.iter().enumerate() {
                for b in &Role::ALL[i + 1..] {
                    assert_ne!(
                        palette_color(bg, *a),
                        palette_color(bg, *b),
                        "{bg:?} {a:?}/{b:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn role_all_lists_each_role_once() {
        for (i, a) in Role::ALL.iter().enumerate() {
            assert!(!Role::ALL[i + 1..].contains(a), "{a:?} twice");
        }
        assert_eq!(Role::ALL.len(), 11);
    }

    // ---- quantisation -----------------------------------------------------------------

    #[test]
    fn ansi256_cube_and_grey_ramp() {
        assert_eq!(rgb_to_ansi256(Rgb::new(0, 0, 0)), 16);
        assert_eq!(rgb_to_ansi256(Rgb::new(255, 255, 255)), 231);
        assert_eq!(rgb_to_ansi256(Rgb::new(255, 0, 0)), 196);
        assert_eq!(rgb_to_ansi256(Rgb::new(0, 255, 0)), 46);
        assert_eq!(rgb_to_ansi256(Rgb::new(0, 0, 255)), 21);
        assert_eq!(rgb_to_ansi256(Rgb::new(95, 135, 175)), 67);
        assert_eq!(rgb_to_ansi256(Rgb::new(128, 128, 128)), 244);
        assert_eq!(rgb_to_ansi256(Rgb::new(8, 8, 8)), 232);
        assert_eq!(rgb_to_ansi256(Rgb::new(238, 238, 238)), 255);
        assert_eq!(
            rgb_to_ansi256(Rgb::new(100, 102, 98)),
            241,
            "near-grey goes to the ramp"
        );
        // Every index of the standard table maps back to itself.
        for idx in 16..=255u8 {
            assert_eq!(rgb_to_ansi256(xterm256(idx)), idx, "index {idx}");
        }
    }

    #[test]
    fn ansi16_achromatic_ladder() {
        assert_eq!(rgb_to_ansi16(Rgb::new(0, 0, 0)), 0);
        assert_eq!(rgb_to_ansi16(Rgb::new(30, 30, 30)), 0);
        assert_eq!(rgb_to_ansi16(Rgb::new(100, 100, 100)), 8);
        assert_eq!(rgb_to_ansi16(Rgb::new(170, 170, 170)), 7);
        assert_eq!(rgb_to_ansi16(Rgb::new(255, 255, 255)), 15);
    }

    #[test]
    fn ansi16_primaries_keep_their_hue() {
        assert_eq!(rgb_to_ansi16(Rgb::new(255, 0, 0)), 9);
        assert_eq!(rgb_to_ansi16(Rgb::new(170, 0, 0)), 1);
        assert_eq!(rgb_to_ansi16(Rgb::new(0, 255, 0)), 10);
        assert_eq!(rgb_to_ansi16(Rgb::new(0, 130, 0)), 2);
        assert_eq!(rgb_to_ansi16(Rgb::new(255, 255, 0)), 11);
        assert_eq!(rgb_to_ansi16(Rgb::new(130, 130, 0)), 3);
        assert_eq!(rgb_to_ansi16(Rgb::new(0, 0, 255)), 4);
        assert_eq!(rgb_to_ansi16(Rgb::new(100, 150, 255)), 12);
        assert_eq!(rgb_to_ansi16(Rgb::new(255, 0, 255)), 13);
        assert_eq!(rgb_to_ansi16(Rgb::new(130, 0, 130)), 5);
        assert_eq!(rgb_to_ansi16(Rgb::new(0, 255, 255)), 14);
        assert_eq!(rgb_to_ansi16(Rgb::new(0, 130, 130)), 6);
    }

    #[test]
    fn ansi16_hue_anchors_match_the_pure_primaries() {
        let primaries = [
            (Rgb::new(255, 0, 0), 1),
            (Rgb::new(255, 255, 0), 3),
            (Rgb::new(0, 255, 0), 2),
            (Rgb::new(0, 255, 255), 6),
            (Rgb::new(0, 0, 255), 4),
            (Rgb::new(255, 0, 255), 5),
        ];
        for (rgb, idx) in primaries {
            let (_, _, h) = rgb_to_oklch(rgb);
            let anchor = ANSI16_HUES.iter().find(|a| a.1 == idx).map(|a| a.0);
            assert!(
                anchor.is_some_and(|a| (a - h).abs() < 0.05),
                "{idx}: {h} vs {anchor:?}"
            );
        }
    }

    #[test]
    fn ansi16_palette_uses_bright_on_dark_and_normal_on_light() {
        for role in Role::ALL {
            let dark = rgb_to_ansi16(palette_color(Background::Dark, role));
            let light = rgb_to_ansi16(palette_color(Background::Light, role));
            assert!(dark == 7 || dark >= 9, "{role:?} on dark -> {dark}");
            assert!(light <= 6 || light == 8, "{role:?} on light -> {light}");
        }
        for bg in [Background::Dark, Background::Light] {
            let q = |r| rgb_to_ansi16(palette_color(bg, r));
            assert_ne!(q(Role::Plan), q(Role::Bypass), "{bg:?}");
            assert_ne!(q(Role::Success), q(Role::Error), "{bg:?}");
            assert_ne!(q(Role::Text), q(Role::Dim), "{bg:?}");
        }
    }

    // ---- detection --------------------------------------------------------------------

    #[test]
    fn explicit_level_choices_win_over_the_environment() {
        let env = env_from(&[("NO_COLOR", "1"), ("TERM", "dumb")]);
        assert_eq!(
            detect_level_on("truecolor", &env, false, false),
            ColorLevel::TrueColor
        );
        assert_eq!(
            detect_level_on("24bit", &env, false, false),
            ColorLevel::TrueColor
        );
        assert_eq!(
            detect_level_on("256", &env, false, false),
            ColorLevel::Ansi256
        );
        assert_eq!(
            detect_level_on("ansi256", &env, false, false),
            ColorLevel::Ansi256
        );
        assert_eq!(
            detect_level_on("16", &env, false, false),
            ColorLevel::Ansi16
        );
        assert_eq!(
            detect_level_on("ansi16", &env, false, false),
            ColorLevel::Ansi16
        );
        assert_eq!(
            detect_level_on(" TrueColor ", &env, false, false),
            ColorLevel::TrueColor
        );
        let rich = env_from(&[("COLORTERM", "truecolor")]);
        assert_eq!(detect_level_on("none", &rich, true, true), ColorLevel::None);
    }

    #[test]
    fn auto_level_table() {
        use ColorLevel as L;
        // (environment, is_tty, windows, expected)
        let cases: &[(Vars, bool, bool, ColorLevel)] = &[
            (
                &[("NO_COLOR", "1"), ("COLORTERM", "truecolor")],
                true,
                false,
                L::None,
            ),
            (
                &[("NO_COLOR", ""), ("COLORTERM", "truecolor")],
                true,
                false,
                L::TrueColor,
            ),
            (&[("COLORTERM", "truecolor")], false, false, L::None),
            (&[("TERM", "dumb")], true, false, L::None),
            (&[("TERM", "dumb")], true, true, L::None),
            (
                &[("COLORTERM", "truecolor"), ("TERM", "xterm")],
                true,
                false,
                L::TrueColor,
            ),
            (
                &[("COLORTERM", "24bit"), ("TERM", "xterm")],
                true,
                false,
                L::TrueColor,
            ),
            (&[("COLORTERM", "TrueColor")], true, false, L::TrueColor),
            (
                &[("WT_SESSION", "0c1d-42"), ("TERM", "xterm")],
                true,
                true,
                L::TrueColor,
            ),
            (
                &[("TERM_PROGRAM", "iTerm.app"), ("TERM", "xterm")],
                true,
                false,
                L::TrueColor,
            ),
            (
                &[("TERM_PROGRAM", "WezTerm"), ("TERM", "xterm")],
                true,
                false,
                L::TrueColor,
            ),
            (
                &[("TERM_PROGRAM", "vscode"), ("TERM", "xterm")],
                true,
                false,
                L::TrueColor,
            ),
            (&[], true, true, L::TrueColor),
            (&[("TERM", "")], true, true, L::TrueColor),
            (&[("TERM", "xterm-256color")], true, true, L::Ansi256),
            (&[("TERM", "xterm-256color")], true, false, L::Ansi256),
            (&[("TERM", "screen-256color")], true, false, L::Ansi256),
            (
                &[("TERM_PROGRAM", "Apple_Terminal"), ("TERM", "xterm")],
                true,
                false,
                L::Ansi256,
            ),
            (&[("TERM", "xterm")], true, false, L::Ansi16),
            (&[("TERM", "linux")], true, false, L::Ansi16),
            (&[], true, false, L::Ansi16),
            (
                &[("COLORTERM", "yes"), ("TERM", "xterm")],
                true,
                false,
                L::Ansi16,
            ),
        ];
        for (pairs, tty, windows, expected) in cases {
            let env = env_from(pairs);
            for choice in ["auto", "", "AUTO", "nonsense"] {
                assert_eq!(
                    detect_level_on(choice, &env, *tty, *windows),
                    *expected,
                    "choice={choice:?} env={pairs:?} tty={tty} windows={windows}"
                );
            }
        }
    }

    #[test]
    fn public_detect_level_uses_the_current_os() {
        let env = env_from(&[]);
        let expected = if cfg!(windows) {
            ColorLevel::TrueColor
        } else {
            ColorLevel::Ansi16
        };
        assert_eq!(detect_level("auto", &env, true), expected);
        assert_eq!(detect_level("auto", &env, false), ColorLevel::None);
        assert_eq!(detect_level("256", &env, false), ColorLevel::Ansi256);
    }

    #[test]
    fn background_table() {
        use Background::{Dark, Light};
        let cases: &[(&str, Vars, Background)] = &[
            ("dark", &[("COLORFGBG", "0;15")], Dark),
            ("light", &[("COLORFGBG", "15;0")], Light),
            (" Light ", &[], Light),
            ("auto", &[], Dark),
            ("auto", &[("COLORFGBG", "0;15")], Light),
            ("auto", &[("COLORFGBG", "0;7")], Light),
            ("auto", &[("COLORFGBG", "15;0")], Dark),
            ("auto", &[("COLORFGBG", "7;8")], Dark),
            ("auto", &[("COLORFGBG", "0;default;15")], Light),
            ("auto", &[("COLORFGBG", "15;default;0")], Dark),
            ("auto", &[("COLORFGBG", "0; 15 ")], Light),
            ("auto", &[("COLORFGBG", "15")], Light),
            ("auto", &[("COLORFGBG", "")], Dark),
            ("auto", &[("COLORFGBG", "garbage")], Dark),
            ("auto", &[("COLORFGBG", "0;default")], Dark),
            ("nonsense", &[("COLORFGBG", "0;15")], Light),
        ];
        for (choice, pairs, expected) in cases {
            let env = env_from(pairs);
            assert_eq!(
                detect_background(choice, &env),
                *expected,
                "{choice:?} {pairs:?}"
            );
        }
    }

    // ---- Theme --------------------------------------------------------------------------

    const EMPHASISED: [Role; 5] = [
        Role::Error,
        Role::Warning,
        Role::Admin,
        Role::Plan,
        Role::Bypass,
    ];

    #[test]
    fn style_without_colour_is_default_or_bold() {
        for bg in [Background::Dark, Background::Light] {
            let t = Theme::new(ColorLevel::None, bg);
            for role in Role::ALL {
                let expected = if EMPHASISED.contains(&role) {
                    Style::default().add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                assert_eq!(t.style(role), expected, "{role:?}");
            }
        }
    }

    #[test]
    fn style_per_level_uses_the_palette() {
        for bg in [Background::Dark, Background::Light] {
            for role in Role::ALL {
                let rgb = palette_color(bg, role);
                let tc = Theme::new(ColorLevel::TrueColor, bg).style(role);
                assert_eq!(
                    tc.fg,
                    Some(Color::Rgb(rgb.r, rgb.g, rgb.b)),
                    "{bg:?}/{role:?}"
                );
                let c256 = Theme::new(ColorLevel::Ansi256, bg).style(role);
                assert_eq!(c256.fg, Some(Color::Indexed(rgb_to_ansi256(rgb))));
                let c16 = Theme::new(ColorLevel::Ansi16, bg).style(role);
                assert!(
                    !matches!(
                        c16.fg,
                        None | Some(Color::Rgb(..) | Color::Indexed(_) | Color::Reset)
                    ),
                    "named colour expected, got {:?}",
                    c16.fg
                );
                for s in [tc, c256, c16] {
                    assert_eq!(s.bg, None, "the terminal background is never painted");
                    assert_eq!(
                        s.add_modifier.contains(Modifier::BOLD),
                        EMPHASISED.contains(&role),
                        "{bg:?}/{role:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn style_ansi16_named_colours() {
        let t = Theme::new(ColorLevel::Ansi16, Background::Dark);
        assert_eq!(t.style(Role::Text).fg, Some(Color::White));
        assert_eq!(t.style(Role::Dim).fg, Some(Color::Gray));
        assert_eq!(t.style(Role::Error).fg, Some(Color::LightRed));
        assert_eq!(t.style(Role::Plan).fg, Some(Color::LightBlue));
        let t = Theme::new(ColorLevel::Ansi16, Background::Light);
        assert_eq!(t.style(Role::Text).fg, Some(Color::Black));
        assert_eq!(t.style(Role::Dim).fg, Some(Color::DarkGray));
        assert_eq!(t.style(Role::Error).fg, Some(Color::Red));
        assert_eq!(t.style(Role::Plan).fg, Some(Color::Blue));
    }

    #[test]
    fn paint_without_colour_is_identity() {
        for bg in [Background::Dark, Background::Light] {
            let t = Theme::new(ColorLevel::None, bg);
            for role in Role::ALL {
                for text in ["", "bonjour", "éàçù\nligne 2\n", "\u{1b}[31mdéjà coloré"] {
                    assert_eq!(t.paint(role, text), text);
                }
            }
        }
    }

    #[test]
    fn paint_truecolor_sequences() {
        let t = Theme::new(ColorLevel::TrueColor, Background::Dark);
        let c = palette_color(Background::Dark, Role::Text);
        assert_eq!(
            t.paint(Role::Text, "salut"),
            format!("\u{1b}[38;2;{};{};{}msalut\u{1b}[0m", c.r, c.g, c.b)
        );
        let e = palette_color(Background::Dark, Role::Error);
        assert_eq!(
            t.paint(Role::Error, "échec"),
            format!("\u{1b}[1;38;2;{};{};{}méchec\u{1b}[0m", e.r, e.g, e.b)
        );
    }

    #[test]
    fn paint_ansi256_and_ansi16_sequences() {
        let bg = Background::Light;
        let c = palette_color(bg, Role::Accent);
        let t = Theme::new(ColorLevel::Ansi256, bg);
        assert_eq!(
            t.paint(Role::Accent, "x"),
            format!("\u{1b}[38;5;{}mx\u{1b}[0m", rgb_to_ansi256(c))
        );
        let t16 = Theme::new(ColorLevel::Ansi16, Background::Light);
        assert_eq!(t16.paint(Role::Text, "x"), "\u{1b}[30mx\u{1b}[0m");
        assert_eq!(t16.paint(Role::Error, "x"), "\u{1b}[1;31mx\u{1b}[0m");
        let d16 = Theme::new(ColorLevel::Ansi16, Background::Dark);
        assert_eq!(d16.paint(Role::Text, "x"), "\u{1b}[97mx\u{1b}[0m");
        assert_eq!(d16.paint(Role::Dim, "x"), "\u{1b}[37mx\u{1b}[0m");
        assert_eq!(d16.paint(Role::Error, "x"), "\u{1b}[1;91mx\u{1b}[0m");
    }

    #[test]
    fn paint_handles_empty_text_and_lines() {
        let t = Theme::new(ColorLevel::Ansi16, Background::Dark);
        assert_eq!(t.paint(Role::Text, ""), "");
        // Each line is self-contained, so interleaved output never inherits a colour.
        assert_eq!(
            t.paint(Role::Text, "a\n\nb\n"),
            "\u{1b}[97ma\u{1b}[0m\n\n\u{1b}[97mb\u{1b}[0m\n"
        );
    }

    #[test]
    fn theme_new_keeps_its_fields() {
        let t = Theme::new(ColorLevel::Ansi256, Background::Light);
        assert_eq!(
            t,
            Theme {
                level: ColorLevel::Ansi256,
                background: Background::Light
            }
        );
    }
}
