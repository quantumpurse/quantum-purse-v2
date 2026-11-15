//! The wallet's colors: the eight the Settings page lets users set by name,
//! and saving them between runs in `colors.json`, next to
//! `last_wallet.json`.

use eframe::egui::Color32;

use crate::types::AppColors;

/// File in the app's data directory holding the user's colors.
const COLORS_FILE: &str = "colors.json";

/// A color by how it looks: lightness from 0 (black) to 1 (white), chroma
/// (how colorful, 0 being gray) and hue in degrees. Equal steps here look
/// equal, so a color can turn to another hue at the same lightness, which is
/// what keeps its contrast. This is Björn Ottosson's Oklab in polar form:
/// https://bottosson.github.io/posts/oklab/
#[derive(Clone, Copy, Debug)]
struct Oklch {
    l: f64,
    c: f64,
    h: f64,
}

fn srgb_to_linear(v: u8) -> f64 {
    let v = v as f64 / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(v: f64) -> u8 {
    let v = v.clamp(0.0, 1.0);
    let v = if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    };
    (v * 255.0).round() as u8
}

fn to_oklch(color: Color32) -> Oklch {
    let [r, g, b, _] = color.to_srgba_unmultiplied();
    let (r, g, b) = (srgb_to_linear(r), srgb_to_linear(g), srgb_to_linear(b));
    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
    let a = 1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s;
    let b = 0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s;
    Oklch {
        l: 0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        c: a.hypot(b),
        h: b.atan2(a).to_degrees().rem_euclid(360.0),
    }
}

/// The color the screen can show nearest `o`: where it can't show that much
/// chroma at that lightness and hue, chroma is lowered until it can.
fn from_oklch(o: Oklch) -> Color32 {
    let l = o.l.clamp(0.0, 1.0);
    let (sin, cos) = o.h.to_radians().sin_cos();
    let linear = |c: f64| {
        let (a, b) = (c * cos, c * sin);
        let l_ = (l + 0.3963377774 * a + 0.2158037573 * b).powi(3);
        let m_ = (l - 0.1055613458 * a - 0.0638541728 * b).powi(3);
        let s_ = (l - 0.0894841775 * a - 1.2914855480 * b).powi(3);
        [
            4.0767416621 * l_ - 3.3077115913 * m_ + 0.2309699292 * s_,
            -1.2684380046 * l_ + 2.6097574011 * m_ - 0.3413193965 * s_,
            -0.0041960863 * l_ - 0.7034186147 * m_ + 1.7076147010 * s_,
        ]
    };
    let shows = |c: f64| linear(c).iter().all(|v| (-1e-6..=1.0 + 1e-6).contains(v));
    let mut c = o.c.max(0.0);
    if !shows(c) {
        // Halving 20 times pins chroma far finer than one 8-bit step.
        let (mut lo, mut hi) = (0.0, c);
        for _ in 0..20 {
            let mid = (lo + hi) / 2.0;
            if shows(mid) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        c = lo;
    }
    let [r, g, b] = linear(c);
    Color32::from_rgb(linear_to_srgb(r), linear_to_srgb(g), linear_to_srgb(b))
}

/// WCAG contrast ratio of two opaque colors, from 1 (the same) to 21 (black
/// on white). 4.5 is the usual floor for text.
pub(crate) fn contrast(a: Color32, b: Color32) -> f64 {
    let luminance = |c: Color32| {
        let [r, g, b, _] = c.to_srgba_unmultiplied();
        0.2126 * srgb_to_linear(r) + 0.7152 * srgb_to_linear(g) + 0.0722 * srgb_to_linear(b)
    };
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// `base` moved the way `factory_shade` sits from `factory_base`, by the
/// same steps of lightness, chroma and hue: a new background gets panels and
/// borders as much lighter as the factory's, a new accent a dark accent as
/// much darker.
fn same_step(base: Color32, factory_base: Color32, factory_shade: Color32) -> Color32 {
    let (b, fb, fs) = (
        to_oklch(base),
        to_oklch(factory_base),
        to_oklch(factory_shade),
    );
    // A base grayer than the factory's takes as much less of its chroma
    // step, so a gray stays gray: its hue means nothing.
    let grayness = (b.c / fb.c).min(1.0);
    from_oklch(Oklch {
        l: b.l + fs.l - fb.l,
        c: b.c + (fs.c - fb.c) * grayness,
        h: b.h + fs.h - fb.h,
    })
}

fn with_alpha(color: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

/// The see-through highlight for active rows, from the accent.
fn accent_tint(accent: Color32) -> Color32 {
    with_alpha(accent, AppColors::default().accent_tint.a())
}

/// The see-through caution fill for badges, from the warning.
fn warn_tint(warn: Color32) -> Color32 {
    with_alpha(warn, AppColors::default().warn_tint.a())
}

/// The colors users set by name. The rest of the palette follows from them:
/// the panel and border shades from the background, the dark accent and the
/// highlight from the accent, and the caution fill from the warning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ColorRole {
    Background,
    Text,
    MutedText,
    Accent,
    Positive,
    Negative,
    Warning,
    Network,
}

impl ColorRole {
    /// In the order the Settings page lists them.
    pub(crate) const ALL: [ColorRole; 8] = [
        ColorRole::Background,
        ColorRole::Text,
        ColorRole::MutedText,
        ColorRole::Accent,
        ColorRole::Positive,
        ColorRole::Negative,
        ColorRole::Warning,
        ColorRole::Network,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            ColorRole::Background => "Background",
            ColorRole::Text => "Text",
            ColorRole::MutedText => "Muted text",
            ColorRole::Accent => "Accent",
            ColorRole::Positive => "Positive",
            ColorRole::Negative => "Negative",
            ColorRole::Warning => "Warning",
            // Not "Network", which the Connection panel uses for the chain.
            ColorRole::Network => "Network figures",
        }
    }

    /// What the color is used for, shown on hover.
    pub(crate) fn description(self) -> &'static str {
        match self {
            ColorRole::Background => "The canvas. Panels and borders follow it.",
            ColorRole::Text => "Main text and figures.",
            ColorRole::MutedText => "Labels and secondary text.",
            ColorRole::Accent => "Buttons, links, highlights and active items.",
            ColorRole::Positive => "Rises, incoming funds, online and confirmed.",
            ColorRole::Negative => "Falls, errors and offline.",
            ColorRole::Warning => "Cautions.",
            ColorRole::Network => {
                "Figures from outside your wallet, like the network's QR lock adoption."
            }
        }
    }

    pub(crate) fn get(self, colors: &AppColors) -> Color32 {
        match self {
            ColorRole::Background => colors.bg,
            ColorRole::Text => colors.text,
            ColorRole::MutedText => colors.text_muted,
            ColorRole::Accent => colors.accent,
            ColorRole::Positive => colors.accent2,
            ColorRole::Negative => colors.danger,
            ColorRole::Warning => colors.warn,
            ColorRole::Network => colors.signal,
        }
    }

    /// Sets this color, and the shades that follow from it.
    pub(crate) fn set(self, colors: &mut AppColors, color: Color32) {
        let factory = AppColors::default();
        match self {
            ColorRole::Background => {
                colors.bg = color;
                colors.surface = same_step(color, factory.bg, factory.surface);
                colors.surface2 = same_step(color, factory.bg, factory.surface2);
                colors.border = same_step(color, factory.bg, factory.border);
                colors.border2 = same_step(color, factory.bg, factory.border2);
            }
            ColorRole::Text => colors.text = color,
            ColorRole::MutedText => colors.text_muted = color,
            ColorRole::Accent => {
                colors.accent = color;
                colors.accent3 = same_step(color, factory.accent, factory.accent3);
                colors.accent_tint = accent_tint(color);
            }
            ColorRole::Positive => colors.accent2 = color,
            ColorRole::Negative => colors.danger = color,
            ColorRole::Warning => {
                colors.warn = color;
                colors.warn_tint = warn_tint(color);
            }
            ColorRole::Network => colors.signal = color,
        }
    }
}

/// `#RRGGBB`.
pub(crate) fn to_hex(color: Color32) -> String {
    format!("#{:02X}{:02X}{:02X}", color.r(), color.g(), color.b())
}

/// A color from six hex digits, with or without a leading `#`, in any case.
pub(crate) fn parse_hex(text: &str) -> Option<Color32> {
    let text = text.trim();
    let digits = text.strip_prefix('#').unwrap_or(text);
    if digits.len() != 6 || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let v = u32::from_str_radix(digits, 16).ok()?;
    Some(Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

/// The palette's colors by field name, as saved. The two see-through fills
/// aren't saved: they are rebuilt from the accent and the warning.
fn saved_fields(colors: &mut AppColors) -> [(&'static str, &mut Color32); 13] {
    [
        ("bg", &mut colors.bg),
        ("surface", &mut colors.surface),
        ("surface2", &mut colors.surface2),
        ("border", &mut colors.border),
        ("border2", &mut colors.border2),
        ("accent", &mut colors.accent),
        ("accent2", &mut colors.accent2),
        ("accent3", &mut colors.accent3),
        ("danger", &mut colors.danger),
        ("warn", &mut colors.warn),
        ("text", &mut colors.text),
        ("text_muted", &mut colors.text_muted),
        ("signal", &mut colors.signal),
    ]
}

fn to_json(colors: &AppColors) -> String {
    let mut colors = *colors;
    let saved: serde_json::Map<String, serde_json::Value> = saved_fields(&mut colors)
        .into_iter()
        .map(|(name, color)| (name.to_string(), to_hex(*color).into()))
        .collect();
    format!("{:#}", serde_json::Value::Object(saved))
}

/// Colors from a saved file. A color the file leaves out stays factory, as
/// in a file from before that color existed; one that isn't `#RRGGBB` fails
/// the whole file, by name.
fn from_json(json: &str) -> Result<AppColors, String> {
    let saved: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(json).map_err(|e| format!("not a JSON object of colors: {e}"))?;
    let mut colors = AppColors::default();
    for (name, color) in saved_fields(&mut colors) {
        if let Some(value) = saved.get(name) {
            *color = value
                .as_str()
                .and_then(parse_hex)
                .ok_or_else(|| format!("{name} is {value}, not a #RRGGBB color"))?;
        }
    }
    colors.accent_tint = accent_tint(colors.accent);
    colors.warn_tint = warn_tint(colors.warn);
    Ok(colors)
}

fn colors_path() -> Result<std::path::PathBuf, String> {
    qpv2_core::db::get_data_dir()
        .map(|dir| dir.join(COLORS_FILE))
        .map_err(|e| format!("no data directory: {e}"))
}

/// The saved colors, or the factory palette when there are none. A file
/// that can't be used is logged and passed over, so it never stops the
/// wallet from opening.
pub(crate) fn load_colors() -> AppColors {
    let loaded = colors_path().and_then(|path| match std::fs::read_to_string(&path) {
        Ok(json) => from_json(&json)
            .map(Some)
            .map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("can't read {}: {e}", path.display())),
    });
    match loaded {
        Ok(colors) => colors.unwrap_or_default(),
        Err(e) => {
            tracing::warn!("Using the factory colors. Saved colors unusable: {e}");
            AppColors::default()
        }
    }
}

pub(crate) fn save_colors(colors: &AppColors) -> Result<(), String> {
    let path = colors_path()?;
    std::fs::write(&path, to_json(colors))
        .map_err(|e| format!("can't write {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Within one 8-bit step per channel: what rounding allows.
    fn close(a: Color32, b: Color32) -> bool {
        let (a, b) = (a.to_array(), b.to_array());
        a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 1)
    }

    #[test]
    fn factory_background_and_accent_keep_their_factory_shades() {
        let factory = AppColors::default();
        let mut colors = factory;
        ColorRole::Background.set(&mut colors, factory.bg);
        ColorRole::Accent.set(&mut colors, factory.accent);
        for (name, got, want) in [
            ("surface", colors.surface, factory.surface),
            ("surface2", colors.surface2, factory.surface2),
            ("border", colors.border, factory.border),
            ("border2", colors.border2, factory.border2),
            ("accent3", colors.accent3, factory.accent3),
            ("accent_tint", colors.accent_tint, factory.accent_tint),
        ] {
            assert!(
                close(got, want),
                "{name}: {} != {}",
                to_hex(got),
                to_hex(want)
            );
        }
    }

    #[test]
    fn gray_background_keeps_gray_shades() {
        let mut colors = AppColors::default();
        ColorRole::Background.set(&mut colors, Color32::from_rgb(20, 20, 20));
        for shade in [
            colors.surface,
            colors.surface2,
            colors.border,
            colors.border2,
        ] {
            let [r, g, b, _] = shade.to_array();
            assert!(
                r.abs_diff(g) <= 1 && g.abs_diff(b) <= 1,
                "{}",
                to_hex(shade)
            );
        }
    }

    #[test]
    fn saved_colors_load_back_unchanged() {
        let colors = AppColors::light();
        assert_eq!(from_json(&to_json(&colors)), Ok(colors));
    }

    #[test]
    fn a_color_the_file_leaves_out_stays_factory() {
        let factory = AppColors::default();
        let colors = from_json(r##"{"accent": "#112233"}"##).unwrap();
        assert_eq!(colors.accent, Color32::from_rgb(0x11, 0x22, 0x33));
        assert_eq!(colors.accent_tint, accent_tint(colors.accent));
        assert_eq!(colors.bg, factory.bg);
    }

    #[test]
    fn a_bad_color_fails_the_file_by_name() {
        let err = from_json(r#"{"bg": "blue"}"#).unwrap_err();
        assert!(err.contains("bg"), "{err}");
    }

    #[test]
    fn hex_codes_need_six_digits() {
        assert_eq!(parse_hex("#0a0C0d"), Some(Color32::from_rgb(10, 12, 13)));
        assert_eq!(parse_hex("0A0C0D"), Some(Color32::from_rgb(10, 12, 13)));
        for bad in ["", "#", "#0A0C0", "#0A0C0D0", "#0A0C0G", "+A0C0D"] {
            assert_eq!(parse_hex(bad), None, "{bad:?}");
        }
    }
}
