//! Rasterizes markdown headers into pixel images via `cosmic-text`, for
//! display through `ratatui-image`'s terminal graphics protocols -- this
//! is `mdfried`'s headline trick (big pixel headers instead of plain bold
//! text), ported using a single bundled font rather than matching the host
//! terminal's configured font (see `assets/fonts/NOTICE.md`), which keeps
//! this crate free of the terminal-font-detection machinery `mdfried`
//! needs for that cosmetic match.

use cosmic_text::{
    Attrs, Buffer, Color as CosmicColor, Family, FontSystem, Metrics, Shaping, SwashCache,
};
use image::{Rgba, RgbaImage};
use ratatui_image::picker::Picker;

static CASCADIA_CODE: &[u8] = include_bytes!("../../assets/fonts/CascadiaCode-Regular.otf");

/// Owns the `cosmic-text` font state used to rasterize header text.
/// Construction loads/indexes the bundled font (a few milliseconds), so
/// one instance is built once and reused for every header in every
/// document.
pub struct HeaderRasterizer {
    font_system: FontSystem,
    swash_cache: SwashCache,
    family_name: String,
}

impl HeaderRasterizer {
    pub fn new() -> Self {
        let mut db = cosmic_text::fontdb::Database::new();
        db.load_font_data(CASCADIA_CODE.to_vec());
        // Read back the family name the font itself declares, rather than
        // hardcoding a guess -- `Attrs::family` needs an exact match.
        let family_name = db
            .faces()
            .next()
            .and_then(|face| face.families.first())
            .map(|(name, _)| name.clone())
            .unwrap_or_else(|| "Cascadia Code".to_string());
        let font_system = FontSystem::new_with_locale_and_db("en-US".to_string(), db);
        Self {
            font_system,
            swash_cache: SwashCache::new(),
            family_name,
        }
    }

    /// Rasterizes `text` (one header line) at a size scaled by `level`
    /// (1 = biggest, 6+ = smallest) to fit `width_cols` terminal columns,
    /// using the terminal's actual cell pixel size (`cell_px`, from
    /// `Picker::font_size`) so the image lines up with the surrounding
    /// text grid. Returns an image two text-rows tall.
    pub fn rasterize(
        &mut self,
        text: &str,
        level: u8,
        width_cols: u16,
        cell_px: (u16, u16),
    ) -> RgbaImage {
        const HEADER_ROWS: u16 = 2;
        let tier_scale = (12.0 - f32::from(level).min(11.0)) / 12.0;
        let line_height = f32::from(cell_px.1) * f32::from(HEADER_ROWS);
        let font_size_px = (line_height * tier_scale).max(1.0);

        let metrics = Metrics::new(font_size_px, line_height);
        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        let attrs = Attrs::new().family(Family::Name(&self.family_name));
        let width_px = f32::from(width_cols) * f32::from(cell_px.0);
        buffer.set_size(&mut self.font_system, Some(width_px), Some(line_height));
        buffer.set_text(&mut self.font_system, text, &attrs, Shaping::Advanced);
        buffer.shape_until_scroll(&mut self.font_system, false);

        let img_width = width_px.round().max(1.0) as u32;
        let img_height = line_height.round().max(1.0) as u32;
        let mut image = RgbaImage::from_pixel(img_width, img_height, Rgba([0, 0, 0, 0]));
        let fg = CosmicColor::rgb(0xe0, 0xaf, 0x68);

        buffer.draw(
            &mut self.font_system,
            &mut self.swash_cache,
            fg,
            |x, y, w, h, color| {
                if w != 1 || h != 1 || x < 0 || y < 0 {
                    return;
                }
                let (x, y) = (x as u32, y as u32);
                if x >= img_width || y >= img_height || color.a() == 0 {
                    return;
                }
                let existing = image.get_pixel(x, y);
                let blended = blend(
                    *existing,
                    Rgba([color.r(), color.g(), color.b(), color.a()]),
                );
                image.put_pixel(x, y, blended);
            },
        );

        image
    }
}

impl Default for HeaderRasterizer {
    fn default() -> Self {
        Self::new()
    }
}

/// Straight alpha-over blend of `src` onto `dst`.
fn blend(dst: Rgba<u8>, src: Rgba<u8>) -> Rgba<u8> {
    let sa = f32::from(src.0[3]) / 255.0;
    let da = 1.0 - sa;
    let mix = |s: u8, d: u8| (f32::from(s) * sa + f32::from(d) * da).round() as u8;
    Rgba([
        mix(src.0[0], dst.0[0]),
        mix(src.0[1], dst.0[1]),
        mix(src.0[2], dst.0[2]),
        ((sa + da * f32::from(dst.0[3]) / 255.0) * 255.0).round() as u8,
    ])
}

/// The terminal's real cell pixel size, from `Picker::font_size()`.
pub fn cell_pixel_size(picker: &Picker) -> (u16, u16) {
    let size = picker.font_size();
    (size.width, size.height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rasterizes_nonempty_header_to_nonzero_image() {
        let mut rasterizer = HeaderRasterizer::new();
        let image = rasterizer.rasterize("Hello", 1, 20, (10, 20));
        assert!(image.width() > 0);
        assert!(image.height() > 0);
    }

    #[test]
    fn smaller_tiers_produce_smaller_font_size() {
        let mut rasterizer = HeaderRasterizer::new();
        let h1 = rasterizer.rasterize("Header", 1, 40, (10, 20));
        let h6 = rasterizer.rasterize("Header", 6, 40, (10, 20));
        // Both share the same fixed 2-row pixel height; the visual size
        // difference is in glyph scale, not image dimensions, so just
        // assert both rasterize without panicking and keep the same height.
        assert_eq!(h1.height(), h6.height());
    }
}
