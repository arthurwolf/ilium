//! Advisory visibility checks; terminal default colours are estimates.
//!
//! Ported from `ilium-client/src/animation_visibility.rs`. The kernel samples its own composed
//! frame; the pure decisions (what counts as blank, how bright a colour is, when a bounded sample
//! of the field warns) live here so they are testable without a terminal. Pure: no host call.

/// Foreground and backdrop pair closer than this contrast ratio count as faint.
pub const FAINT_CONTRAST: f32 = 1.2;
/// Fewer sampled cells than this never warn: the field is too small to judge.
pub const MINIMUM_SAMPLE: usize = 128;
/// Upper bound on cells inspected per check.
pub const SAMPLE_BUDGET: usize = 2048;
/// Whole seconds between two checks.
pub const CHECK_INTERVAL_SECONDS: u64 = 1;
/// Backdrop assumed on a light terminal without a reported background.
pub const LIGHT_BACKDROP: (u8, u8, u8) = (245, 245, 245);
/// Backdrop assumed on a dark terminal without a reported background.
pub const DARK_BACKDROP: (u8, u8, u8) = (26, 26, 26);

/// A cell that carries no ink: whitespace or the blank braille pattern.
pub fn is_blank_glyph(glyph: char) -> bool {
    glyph.is_whitespace() || glyph == '\u{2800}'
}

/// Relative luminance of an sRGB colour.
pub fn luminance((r, g, b): (u8, u8, u8)) -> f32 {
    let linear = |value: u8| {
        let v = f32::from(value) / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
}

/// WCAG-style contrast ratio of two colours, at least 1.
pub fn contrast(foreground: (u8, u8, u8), background: (u8, u8, u8)) -> f32 {
    let a = luminance(foreground);
    let b = luminance(background);
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// Whether one inked cell is too close to its backdrop to read.
pub fn is_faint(foreground: (u8, u8, u8), background: (u8, u8, u8)) -> bool {
    contrast(foreground, background) < FAINT_CONTRAST
}

/// The xterm 256-colour palette entry as RGB, for the entries a terminal may report as a
/// background (the 6x6x6 cube and the grey ramp, plus black and white).
pub fn indexed_rgb(index: u8) -> (u8, u8, u8) {
    match index {
        0 | 16 => (0, 0, 0),
        15 | 231 => (255, 255, 255),
        232..=255 => {
            let grey = 8 + 10 * (index - 232);
            (grey, grey, grey)
        }
        16..=231 => {
            let n = index - 16;
            let levels = [0, 95, 135, 175, 215, 255];
            (
                levels[usize::from(n / 36)],
                levels[usize::from((n / 6) % 6)],
                levels[usize::from(n % 6)],
            )
        }
        _ => (0, 0, 0),
    }
}

/// Every `stride`-th cell of a `width` by `height` field, bounded by [`SAMPLE_BUDGET`].
pub fn sample_stride(width: u16, height: u16) -> usize {
    (usize::from(width) * usize::from(height))
        .div_ceil(SAMPLE_BUDGET)
        .max(1)
}

/// Whether a sample of `sampled` cells with `ink` inked and `faint` faint cells warrants the
/// advisory. Some scenes deliberately have sparse or dark phases, so this is only a hint.
pub fn warns(sampled: usize, ink: usize, faint: usize) -> bool {
    sampled >= MINIMUM_SAMPLE && (ink * 200 < sampled || (ink > 0 && faint * 5 >= ink * 4))
}
