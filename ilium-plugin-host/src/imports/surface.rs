//! Host implementation of `ilium:anim/surface` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostSurface` trait; hand-owned afterwards.

use ilium_plugin_api::generated::anim::surface::{
    FrameReceipt, FrameSurfaceHandle, GlyphCell, HostSurface, OccupancyGrid, ScenePointer,
};
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::ui::PixelFormat;

/// Host side of the `surface` interface.
#[derive(Debug)]
pub struct SurfaceImport;

impl HostSurface for SurfaceImport {
    fn frame_surface_size(&mut self, _handle: FrameSurfaceHandle) -> (u32, u32) {
        unimplemented!("host::imports::surface::frame_surface_size")
    }

    fn frame_surface_write_rows(
        &mut self,
        _handle: FrameSurfaceHandle,
        _first_row: u32,
        _format: PixelFormat,
        _data: Vec<u8>,
    ) -> Result<(), PluginError> {
        unimplemented!("host::imports::surface::frame_surface_write_rows")
    }

    fn frame_surface_write_cell_colors(
        &mut self,
        _handle: FrameSurfaceHandle,
        _first_row: u32,
        _colors: Vec<u32>,
    ) -> Result<(), PluginError> {
        unimplemented!("host::imports::surface::frame_surface_write_cell_colors")
    }

    fn frame_surface_occupancy(&mut self, _handle: FrameSurfaceHandle) -> Option<OccupancyGrid> {
        unimplemented!("host::imports::surface::frame_surface_occupancy")
    }

    fn frame_surface_pointer(&mut self, _handle: FrameSurfaceHandle) -> Option<ScenePointer> {
        unimplemented!("host::imports::surface::frame_surface_pointer")
    }

    fn frame_surface_present(&mut self, _handle: FrameSurfaceHandle) {
        unimplemented!("host::imports::surface::frame_surface_present")
    }

    fn frame_surface_write_glyphs(
        &mut self,
        _handle: FrameSurfaceHandle,
        _cells: Vec<GlyphCell>,
    ) -> Result<(), PluginError> {
        unimplemented!("host::imports::surface::frame_surface_write_glyphs")
    }

    fn frame_surface_clear_glyphs(&mut self, _handle: FrameSurfaceHandle) {
        unimplemented!("host::imports::surface::frame_surface_clear_glyphs")
    }

    fn frame_surface_set_owner_ids(
        &mut self,
        _handle: FrameSurfaceHandle,
        _first_row: u32,
        _ids: Vec<u16>,
    ) -> Result<(), PluginError> {
        unimplemented!("host::imports::surface::frame_surface_set_owner_ids")
    }

    fn frame_surface_last_receipt(&mut self, _handle: FrameSurfaceHandle) -> Option<FrameReceipt> {
        unimplemented!("host::imports::surface::frame_surface_last_receipt")
    }
}

#[cfg(test)]
mod tests {
    use super::SurfaceImport;
    use ilium_plugin_api::generated::anim::surface::{FrameSurfaceHandle, GlyphCell, HostSurface};
    use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
    use ilium_plugin_api::generated::types::text::Modifiers;
    use ilium_plugin_api::generated::types::ui::PixelFormat;

    /// Handle of the one frame surface the scene under test owns.
    const SURFACE_HANDLE: FrameSurfaceHandle = FrameSurfaceHandle(0);
    /// Glyph cells one frame may write (write-glyphs limit, RND-016).
    const GLYPH_CELLS_PER_FRAME_LIMIT: usize = 131_072;

    fn glyph_cell(column: u16, row: u16, text: &str) -> GlyphCell {
        GlyphCell {
            column,
            row,
            text: text.to_string(),
            fg: None,
            bg: None,
            add: Modifiers::default(),
        }
    }

    fn error_code_of(result: Result<(), PluginError>) -> ErrorCode {
        match result {
            Ok(()) => panic!("expected a plugin error, got Ok"),
            Err(error) => error.code,
        }
    }

    fn expect_ok(result: Result<(), PluginError>) {
        if let Err(error) = result {
            panic!(
                "expected Ok, got plugin error {:?}: {}",
                error.code, error.message
            );
        }
    }

    /// Boundary: exactly 131,072 cells per frame is accepted.
    /// REQ: RND-016
    #[test]
    fn write_glyphs_accepts_exactly_the_per_frame_cell_limit() {
        let mut surface = SurfaceImport;
        let cells = vec![glyph_cell(0, 0, " "); GLYPH_CELLS_PER_FRAME_LIMIT];
        expect_ok(surface.frame_surface_write_glyphs(SURFACE_HANDLE, cells));
    }

    /// Boundary: one cell over the per-frame limit returns quota-exceeded.
    /// REQ: RND-016
    #[test]
    fn write_glyphs_rejects_one_cell_over_the_per_frame_limit() {
        let mut surface = SurfaceImport;
        let cells = vec![glyph_cell(0, 0, " "); GLYPH_CELLS_PER_FRAME_LIMIT + 1];
        let code = error_code_of(surface.frame_surface_write_glyphs(SURFACE_HANDLE, cells));
        assert_eq!(code, ErrorCode::QuotaExceeded);
    }

    /// Empty input: an empty cell list is valid and writes nothing (no error is documented for it).
    /// REQ: RND-016
    #[test]
    fn write_glyphs_accepts_an_empty_cell_list() {
        let mut surface = SurfaceImport;
        expect_ok(surface.frame_surface_write_glyphs(SURFACE_HANDLE, Vec::new()));
    }

    /// Error table on api-surface-glyph-plane (no requirement id): a cell outside the surface is invalid-argument.
    /// Row u16::MAX is past any surface height in cells.
    /// REQ: RND-016
    #[test]
    fn write_glyphs_rejects_a_cell_outside_the_surface() {
        let mut surface = SurfaceImport;
        let cells = vec![glyph_cell(0, u16::MAX, "x")];
        let code = error_code_of(surface.frame_surface_write_glyphs(SURFACE_HANDLE, cells));
        assert_eq!(code, ErrorCode::InvalidArgument);
    }

    /// Empty input: clearing a surface that holds no glyphs is accepted.
    /// REQ: RND-016
    #[test]
    fn clear_glyphs_is_accepted_on_a_surface_without_glyphs() {
        let mut surface = SurfaceImport;
        surface.frame_surface_clear_glyphs(SURFACE_HANDLE);
    }

    /// Valid input: a zero id means no owner, so an all-zero row is accepted.
    /// REQ: RND-006
    #[test]
    fn set_owner_ids_accepts_a_full_row_of_zero_ids() {
        let mut surface = SurfaceImport;
        let (width, _height) = surface.frame_surface_size(SURFACE_HANDLE);
        let ids = vec![0u16; width as usize];
        expect_ok(surface.frame_surface_set_owner_ids(SURFACE_HANDLE, 0, ids));
    }

    /// Error table on api-surface-glyph-plane (no requirement id): id rows outside the surface are invalid-argument.
    /// REQ: RND-006
    #[test]
    fn set_owner_ids_rejects_a_first_row_past_the_height() {
        let mut surface = SurfaceImport;
        let (_width, height) = surface.frame_surface_size(SURFACE_HANDLE);
        let code =
            error_code_of(surface.frame_surface_set_owner_ids(SURFACE_HANDLE, height, vec![1u16]));
        assert_eq!(code, ErrorCode::InvalidArgument);
    }

    /// Valid input: one whole gray8 row is exactly `width` bytes.
    /// REQ: RND-004
    #[test]
    fn write_rows_accepts_one_whole_gray8_row() {
        let mut surface = SurfaceImport;
        let (width, _height) = surface.frame_surface_size(SURFACE_HANDLE);
        let data = vec![0u8; width as usize];
        expect_ok(surface.frame_surface_write_rows(SURFACE_HANDLE, 0, PixelFormat::Gray8, data));
    }

    /// Valid input: two whole rgb8 rows are exactly `2 x 3 x width` bytes.
    /// REQ: RND-004
    #[test]
    fn write_rows_accepts_two_whole_rgb8_rows() {
        let mut surface = SurfaceImport;
        let (width, _height) = surface.frame_surface_size(SURFACE_HANDLE);
        let data = vec![0u8; 3 * width as usize * 2];
        expect_ok(surface.frame_surface_write_rows(SURFACE_HANDLE, 0, PixelFormat::Rgb8, data));
    }

    /// Valid input: one whole rgba8 row is exactly `4 x width` bytes.
    /// REQ: RND-004
    #[test]
    fn write_rows_accepts_one_whole_rgba8_row() {
        let mut surface = SurfaceImport;
        let (width, _height) = surface.frame_surface_size(SURFACE_HANDLE);
        let data = vec![0u8; 4 * width as usize];
        expect_ok(surface.frame_surface_write_rows(SURFACE_HANDLE, 0, PixelFormat::Rgba8, data));
    }

    /// Boundary: a partial trailing row (width + 1 gray8 bytes) is dropped and the call still succeeds.
    /// REQ: RND-004
    #[test]
    fn write_rows_drops_a_partial_trailing_row_without_error() {
        let mut surface = SurfaceImport;
        let (width, _height) = surface.frame_surface_size(SURFACE_HANDLE);
        let data = vec![0u8; width as usize + 1];
        expect_ok(surface.frame_surface_write_rows(SURFACE_HANDLE, 0, PixelFormat::Gray8, data));
    }

    /// Empty input: empty data is invalid-argument (surface rules, api-scenes).
    /// REQ: RND-004
    #[test]
    fn write_rows_rejects_empty_data() {
        let mut surface = SurfaceImport;
        let code = error_code_of(surface.frame_surface_write_rows(
            SURFACE_HANDLE,
            0,
            PixelFormat::Gray8,
            Vec::new(),
        ));
        assert_eq!(code, ErrorCode::InvalidArgument);
    }

    /// Boundary: a first row at the height is invalid-argument (surface rules, api-scenes).
    /// RND-005 says rows at or past the height are ignored; the surface rules name first-row as the one invalid case.
    /// REQ: RND-005
    #[test]
    fn write_rows_rejects_a_first_row_at_the_height() {
        let mut surface = SurfaceImport;
        let (width, height) = surface.frame_surface_size(SURFACE_HANDLE);
        let data = vec![0u8; width as usize];
        let code = error_code_of(surface.frame_surface_write_rows(
            SURFACE_HANDLE,
            height,
            PixelFormat::Gray8,
            data,
        ));
        assert_eq!(code, ErrorCode::InvalidArgument);
    }

    /// Boundary: a row written past the last visible row is ignored, so the call succeeds.
    /// REQ: RND-005
    #[test]
    fn write_rows_ignores_rows_that_fall_past_the_height() {
        let mut surface = SurfaceImport;
        let (width, height) = surface.frame_surface_size(SURFACE_HANDLE);
        let data = vec![0u8; 2 * width as usize];
        expect_ok(surface.frame_surface_write_rows(
            SURFACE_HANDLE,
            height.saturating_sub(1),
            PixelFormat::Gray8,
            data,
        ));
    }

    /// Valid input with the grant absent: write_cell_colors is ignored, so the call succeeds.
    /// REQ: RND-006
    #[test]
    fn write_cell_colors_is_ignored_without_uses_cell_colors() {
        let mut surface = SurfaceImport;
        expect_ok(surface.frame_surface_write_cell_colors(SURFACE_HANDLE, 0, vec![0x00ff00]));
    }

    /// occupancy() returns none unless wants-occupancy is true; this handle has no such grant.
    /// REQ: RND-006
    #[test]
    fn occupancy_is_none_without_wants_occupancy() {
        let mut surface = SurfaceImport;
        assert!(surface.frame_surface_occupancy(SURFACE_HANDLE).is_none());
    }

    /// pointer() returns none when wants-pointer is false; this handle has no such grant.
    /// REQ: RND-006
    #[test]
    fn pointer_is_none_without_wants_pointer() {
        let mut surface = SurfaceImport;
        assert!(surface.frame_surface_pointer(SURFACE_HANDLE).is_none());
    }

    /// Before the first presented frame there is no receipt.
    /// REQ: RND-003
    #[test]
    fn last_receipt_is_none_before_the_first_frame_is_presented() {
        let mut surface = SurfaceImport;
        assert!(surface.frame_surface_last_receipt(SURFACE_HANDLE).is_none());
    }

    /// Valid input: presenting a frame produces a receipt for it.
    /// REQ: RND-003
    #[test]
    fn present_produces_a_receipt_for_the_frame() {
        let mut surface = SurfaceImport;
        surface.frame_surface_present(SURFACE_HANDLE);
        assert!(surface.frame_surface_last_receipt(SURFACE_HANDLE).is_some());
    }
}
