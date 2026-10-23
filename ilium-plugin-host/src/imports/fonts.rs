//! Host implementation of `ilium:anim/fonts` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostFonts` trait; hand-owned afterwards.

use ilium_plugin_api::generated::anim::fonts::{FontHandle, HostFonts};
use ilium_plugin_api::generated::types::errors::PluginError;

/// Host side of the `fonts` interface.
#[derive(Debug)]
pub struct FontsImport;

impl HostFonts for FontsImport {
    fn font_new(&mut self, _asset: String) -> FontHandle {
        unimplemented!("host::imports::fonts::font_new")
    }

    fn font_rasterize(
        &mut self,
        _handle: FontHandle,
        _text: String,
        _size_px: u16,
    ) -> Result<(u32, u32, Vec<u8>), PluginError> {
        unimplemented!("host::imports::fonts::font_rasterize")
    }
}

#[cfg(test)]
mod tests {
    use super::FontsImport;
    use ilium_plugin_api::generated::anim::fonts::{FontHandle, HostFonts};
    use ilium_plugin_api::generated::types::errors::ErrorCode;

    /// A font file inside the plugin package (WIT `font` constructor: "the path of a font file inside the plugin package").
    const FONT_ASSET_PATH: &str = "fonts/mono.ttf";

    /// Creates a font from the package asset and returns its handle.
    fn create_font_handle(fonts: &mut FontsImport) -> FontHandle {
        fonts.font_new(FONT_ASSET_PATH.to_string())
    }

    /// REQ: WIT:ilium:anim/fonts.font.constructor
    #[test]
    fn font_new_valid_asset_path_returns_usable_handle() {
        let mut fonts = FontsImport;
        let handle = create_font_handle(&mut fonts);
        let result = fonts.font_rasterize(handle, "A".to_string(), 16);
        assert!(
            result.is_ok(),
            "a handle from a valid asset must rasterize, got {result:?}"
        );
    }

    /// REQ: WIT:ilium:anim/fonts.font.rasterize
    #[test]
    fn font_rasterize_returns_width_height_and_gray8_bitmap_of_matching_length() {
        let mut fonts = FontsImport;
        let handle = create_font_handle(&mut fonts);
        let (width, height, bitmap) = fonts
            .font_rasterize(handle, "Hello".to_string(), 16)
            .expect("valid text and size must rasterize");
        assert!(
            width > 0 && height > 0,
            "rendered text must have a non-zero size, got {width}x{height}"
        );
        assert_eq!(
            bitmap.len(),
            width as usize * height as usize,
            "gray8 holds one byte per pixel"
        );
    }

    /// REQ: WIT:ilium:anim/fonts.font.rasterize
    #[test]
    fn font_rasterize_visible_glyph_produces_ink() {
        let mut fonts = FontsImport;
        let handle = create_font_handle(&mut fonts);
        let (_, _, bitmap) = fonts
            .font_rasterize(handle, "A".to_string(), 16)
            .expect("a visible glyph must rasterize");
        assert!(
            bitmap.iter().any(|value| *value > 0),
            "a visible glyph must have at least one inked pixel"
        );
    }

    /// REQ: WIT:ilium:anim/fonts.font.rasterize
    #[test]
    fn font_rasterize_empty_text_is_ok_with_blank_bitmap_of_matching_length() {
        let mut fonts = FontsImport;
        let handle = create_font_handle(&mut fonts);
        let (width, height, bitmap) = fonts
            .font_rasterize(handle, String::new(), 16)
            .expect("empty text is a valid list and must not fail");
        assert_eq!(
            bitmap.len(),
            width as usize * height as usize,
            "gray8 holds one byte per pixel"
        );
        assert!(
            bitmap.iter().all(|value| *value == 0),
            "empty text has no ink"
        );
    }

    /// REQ: WIT:ilium:anim/fonts.font.rasterize
    #[test]
    fn font_rasterize_smallest_positive_size_is_accepted() {
        let mut fonts = FontsImport;
        let handle = create_font_handle(&mut fonts);
        let (width, height, bitmap) = fonts
            .font_rasterize(handle, "A".to_string(), 1)
            .expect("size 1 px is the lower boundary of a valid size");
        assert_eq!(
            bitmap.len(),
            width as usize * height as usize,
            "gray8 holds one byte per pixel"
        );
    }

    /// REQ: API-061
    #[test]
    fn font_rasterize_zero_size_returns_invalid_argument() {
        let mut fonts = FontsImport;
        let handle = create_font_handle(&mut fonts);
        let error = fonts
            .font_rasterize(handle, "A".to_string(), 0)
            .expect_err("size 0 px is a bad value and must fail");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    /// REQ: WIT:ilium:anim/fonts.font.rasterize
    #[test]
    fn font_rasterize_multibyte_utf8_text_is_accepted() {
        let mut fonts = FontsImport;
        let handle = create_font_handle(&mut fonts);
        let (width, height, bitmap) = fonts
            .font_rasterize(handle, "é日".to_string(), 16)
            .expect("UTF-8 text is a valid string and must rasterize");
        assert_eq!(
            bitmap.len(),
            width as usize * height as usize,
            "gray8 holds one byte per pixel"
        );
    }
}
