//! Host implementation of `ilium:anim/fonts`.
//!
//! A font handle names a font asset of the plugin package. Rasterising uses the host's built-in
//! 5x7 bitmap face scaled to `size_px` by nearest-neighbour sampling, so the result is the same on
//! every machine; characters outside printable ASCII render as a hollow box. The import is a unit
//! struct, so handles are kept per thread.

use std::cell::RefCell;
use std::collections::HashMap;

use ilium_plugin_api::generated::anim::fonts::{FontHandle, HostFonts};
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};

/// Largest bitmap one call returns (16 MiB of gray8 pixels).
const BITMAP_MAX_BYTES: usize = 16 * 1024 * 1024;
/// Glyph cell of the built-in face: 5 ink columns plus one spacing column, 8 rows.
const CELL_COLUMNS: usize = 6;
const CELL_ROWS: usize = 8;
const FACE_FIRST_CHAR: u32 = 0x20;

/// Column data of printable ASCII (0x20 to 0x7E), five columns per glyph, bit 0 is the top row.
const FACE: [[u8; 5]; 95] = [
    [0x00, 0x00, 0x00, 0x00, 0x00], [0x00, 0x00, 0x5F, 0x00, 0x00], [0x00, 0x07, 0x00, 0x07, 0x00],
    [0x14, 0x7F, 0x14, 0x7F, 0x14], [0x24, 0x2A, 0x7F, 0x2A, 0x12], [0x23, 0x13, 0x08, 0x64, 0x62],
    [0x36, 0x49, 0x56, 0x20, 0x50], [0x00, 0x08, 0x07, 0x03, 0x00], [0x00, 0x1C, 0x22, 0x41, 0x00],
    [0x00, 0x41, 0x22, 0x1C, 0x00], [0x2A, 0x1C, 0x7F, 0x1C, 0x2A], [0x08, 0x08, 0x3E, 0x08, 0x08],
    [0x00, 0x80, 0x70, 0x30, 0x00], [0x08, 0x08, 0x08, 0x08, 0x08], [0x00, 0x00, 0x60, 0x60, 0x00],
    [0x20, 0x10, 0x08, 0x04, 0x02], [0x3E, 0x51, 0x49, 0x45, 0x3E], [0x00, 0x42, 0x7F, 0x40, 0x00],
    [0x72, 0x49, 0x49, 0x49, 0x46], [0x21, 0x41, 0x49, 0x4D, 0x33], [0x18, 0x14, 0x12, 0x7F, 0x10],
    [0x27, 0x45, 0x45, 0x45, 0x39], [0x3C, 0x4A, 0x49, 0x49, 0x31], [0x41, 0x21, 0x11, 0x09, 0x07],
    [0x36, 0x49, 0x49, 0x49, 0x36], [0x46, 0x49, 0x49, 0x29, 0x1E], [0x00, 0x00, 0x14, 0x00, 0x00],
    [0x00, 0x40, 0x34, 0x00, 0x00], [0x00, 0x08, 0x14, 0x22, 0x41], [0x14, 0x14, 0x14, 0x14, 0x14],
    [0x00, 0x41, 0x22, 0x14, 0x08], [0x02, 0x01, 0x59, 0x09, 0x06], [0x3E, 0x41, 0x5D, 0x59, 0x4E],
    [0x7C, 0x12, 0x11, 0x12, 0x7C], [0x7F, 0x49, 0x49, 0x49, 0x36], [0x3E, 0x41, 0x41, 0x41, 0x22],
    [0x7F, 0x41, 0x41, 0x41, 0x3E], [0x7F, 0x49, 0x49, 0x49, 0x41], [0x7F, 0x09, 0x09, 0x09, 0x01],
    [0x3E, 0x41, 0x41, 0x51, 0x73], [0x7F, 0x08, 0x08, 0x08, 0x7F], [0x00, 0x41, 0x7F, 0x41, 0x00],
    [0x20, 0x40, 0x41, 0x3F, 0x01], [0x7F, 0x08, 0x14, 0x22, 0x41], [0x7F, 0x40, 0x40, 0x40, 0x40],
    [0x7F, 0x02, 0x1C, 0x02, 0x7F], [0x7F, 0x04, 0x08, 0x10, 0x7F], [0x3E, 0x41, 0x41, 0x41, 0x3E],
    [0x7F, 0x09, 0x09, 0x09, 0x06], [0x3E, 0x41, 0x51, 0x21, 0x5E], [0x7F, 0x09, 0x19, 0x29, 0x46],
    [0x26, 0x49, 0x49, 0x49, 0x32], [0x03, 0x01, 0x7F, 0x01, 0x03], [0x3F, 0x40, 0x40, 0x40, 0x3F],
    [0x1F, 0x20, 0x40, 0x20, 0x1F], [0x3F, 0x40, 0x38, 0x40, 0x3F], [0x63, 0x14, 0x08, 0x14, 0x63],
    [0x03, 0x04, 0x78, 0x04, 0x03], [0x61, 0x59, 0x49, 0x4D, 0x43], [0x00, 0x7F, 0x41, 0x41, 0x41],
    [0x02, 0x04, 0x08, 0x10, 0x20], [0x00, 0x41, 0x41, 0x41, 0x7F], [0x04, 0x02, 0x01, 0x02, 0x04],
    [0x40, 0x40, 0x40, 0x40, 0x40], [0x00, 0x03, 0x07, 0x08, 0x00], [0x20, 0x54, 0x54, 0x78, 0x40],
    [0x7F, 0x28, 0x44, 0x44, 0x38], [0x38, 0x44, 0x44, 0x44, 0x28], [0x38, 0x44, 0x44, 0x28, 0x7F],
    [0x38, 0x54, 0x54, 0x54, 0x18], [0x00, 0x08, 0x7E, 0x09, 0x02], [0x18, 0xA4, 0xA4, 0x9C, 0x78],
    [0x7F, 0x08, 0x04, 0x04, 0x78], [0x00, 0x44, 0x7D, 0x40, 0x00], [0x20, 0x40, 0x40, 0x3D, 0x00],
    [0x7F, 0x10, 0x28, 0x44, 0x00], [0x00, 0x41, 0x7F, 0x40, 0x00], [0x7C, 0x04, 0x78, 0x04, 0x78],
    [0x7C, 0x08, 0x04, 0x04, 0x78], [0x38, 0x44, 0x44, 0x44, 0x38], [0xFC, 0x18, 0x24, 0x24, 0x18],
    [0x18, 0x24, 0x24, 0x18, 0xFC], [0x7C, 0x08, 0x04, 0x04, 0x08], [0x48, 0x54, 0x54, 0x54, 0x24],
    [0x04, 0x04, 0x3F, 0x44, 0x24], [0x3C, 0x40, 0x40, 0x20, 0x7C], [0x1C, 0x20, 0x40, 0x20, 0x1C],
    [0x3C, 0x40, 0x30, 0x40, 0x3C], [0x44, 0x28, 0x10, 0x28, 0x44], [0x4C, 0x90, 0x90, 0x90, 0x7C],
    [0x44, 0x64, 0x54, 0x4C, 0x44], [0x00, 0x08, 0x36, 0x41, 0x00], [0x00, 0x00, 0x77, 0x00, 0x00],
    [0x00, 0x41, 0x36, 0x08, 0x00], [0x02, 0x01, 0x02, 0x04, 0x02],
];

/// Hollow box drawn for characters the face lacks.
const MISSING_GLYPH: [u8; 5] = [0x7F, 0x41, 0x41, 0x41, 0x7F];

#[derive(Debug, Default)]
struct FontsState {
    next_handle: u32,
    /// Handle to the package asset path it was created from.
    assets: HashMap<u32, String>,
}

thread_local! {
    static STATE: RefCell<FontsState> = RefCell::new(FontsState::default());
}

fn glyph_columns(character: char) -> &'static [u8; 5] {
    let code = character as u32;
    if (FACE_FIRST_CHAR..FACE_FIRST_CHAR + FACE.len() as u32).contains(&code) {
        &FACE[(code - FACE_FIRST_CHAR) as usize]
    } else {
        &MISSING_GLYPH
    }
}

/// Host side of the `fonts` interface.
#[derive(Debug)]
pub struct FontsImport;

impl HostFonts for FontsImport {
    fn font_new(&mut self, asset: String) -> FontHandle {
        STATE.with(|cell| {
            let mut state = cell.borrow_mut();
            state.next_handle += 1;
            let handle = state.next_handle;
            state.assets.insert(handle, asset);
            FontHandle(handle)
        })
    }

    fn font_rasterize(
        &mut self,
        handle: FontHandle,
        text: String,
        size_px: u16,
    ) -> Result<(u32, u32, Vec<u8>), PluginError> {
        let is_known = STATE.with(|cell| cell.borrow().assets.contains_key(&handle.0));
        if !is_known {
            return Err(PluginError {
                code: ErrorCode::NotFound,
                message: "font handle is not open".to_owned(),
                retry_after_ms: None,
            });
        }
        if size_px == 0 {
            return Err(PluginError {
                code: ErrorCode::InvalidArgument,
                message: "size-px must be at least 1".to_owned(),
                retry_after_ms: None,
            });
        }
        let height = usize::from(size_px);
        let advance = (height * CELL_COLUMNS).div_ceil(CELL_ROWS).max(1);
        let glyphs: Vec<char> = text.chars().collect();
        let width = advance.saturating_mul(glyphs.len());
        if width.saturating_mul(height) > BITMAP_MAX_BYTES {
            return Err(PluginError {
                code: ErrorCode::QuotaExceeded,
                message: "bitmap would exceed 16 MiB".to_owned(),
                retry_after_ms: None,
            });
        }
        let mut bitmap = vec![0_u8; width * height];
        for (glyph_index, character) in glyphs.iter().enumerate() {
            let columns = glyph_columns(*character);
            for row in 0..height {
                let glyph_row = row * CELL_ROWS / height;
                for offset in 0..advance {
                    let glyph_column = offset * CELL_COLUMNS / advance;
                    let is_inked = glyph_column < columns.len()
                        && columns[glyph_column] >> glyph_row & 1 == 1;
                    if is_inked {
                        bitmap[row * width + glyph_index * advance + offset] = 255;
                    }
                }
            }
        }
        Ok((width as u32, height as u32, bitmap))
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
