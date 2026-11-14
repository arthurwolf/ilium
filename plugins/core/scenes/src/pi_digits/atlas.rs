//! Coverage of the digits `0`-`9` and `.` from the bundled Cascadia Code, rasterized with
//! swash (pure Rust, so it runs inside the scene component).

use swash::scale::{Render, ScaleContext, Source};
use swash::zeno::Format;
use swash::FontRef;

const FONT: &[u8] = include_bytes!("../../assets/fonts/CascadiaCode-Regular.otf");

/// The characters of the atlas, in glyph order: index 0-9 are the digits, 10 is the separator.
pub const CHARACTERS: &str = "0123456789.";

pub struct Atlas {
    /// Width of one glyph cell in dots.
    pub width: usize,
    /// Height of one glyph cell in dots.
    pub height: usize,
    /// Coverage 0.0..=1.0 of each character, `width * height` values row by row.
    pub glyphs: Vec<Vec<f32>>,
}

/// The atlas size for a font size, in dots: (width, height).
pub fn cell_size(size: i32) -> (usize, usize) {
    let size = size.clamp(8, 48) as f32;
    let width = ((size * 0.75).ceil() as usize + 2).div_ceil(2) * 2;
    let height = ((size * 1.4).ceil() as usize).div_ceil(4) * 4;
    (width, height)
}

/// Rasterizes the atlas at `size` dots per em.
pub fn build(size: i32) -> Result<Atlas, String> {
    let size = size.clamp(8, 48) as f32;
    let font = FontRef::from_index(FONT, 0).ok_or("Bundled Pi font cannot be read")?;
    let metrics = font.metrics(&[]).scale(size);
    let (width, height) = cell_size(size as i32);
    // The line box is centred on the font's ascent and descent, like a one-line text layout.
    let baseline = ((height as f32 - (metrics.ascent + metrics.descent)) / 2.0 + metrics.ascent)
        .round() as i32;
    let mut context = ScaleContext::new();
    let mut scaler = context.builder(font).size(size).hint(false).build();
    let charmap = font.charmap();
    let mut glyphs = Vec::with_capacity(CHARACTERS.len());
    for character in CHARACTERS.chars() {
        let glyph_id = charmap.map(character);
        let image = Render::new(&[Source::Outline])
            .format(Format::Alpha)
            .render(&mut scaler, glyph_id)
            .ok_or_else(|| format!("Bundled font produced no outline for {character}"))?;
        let mut ink = vec![0.0f32; width * height];
        let placement = image.placement;
        for row in 0..placement.height as i32 {
            for column in 0..placement.width as i32 {
                let x = placement.left + column;
                let y = baseline - placement.top + row;
                if x < 0 || y < 0 || x as usize >= width || y as usize >= height {
                    continue;
                }
                let coverage = image
                    .data
                    .get((row as usize) * placement.width as usize + column as usize)
                    .copied()
                    .unwrap_or(0);
                let sample = &mut ink[y as usize * width + x as usize];
                *sample = sample.max(f32::from(coverage) / 255.0);
            }
        }
        if !ink.iter().any(|value| *value > 0.0) {
            return Err(format!("Bundled font produced no ink for {character}"));
        }
        glyphs.push(ink);
    }
    Ok(Atlas {
        width,
        height,
        glyphs,
    })
}
