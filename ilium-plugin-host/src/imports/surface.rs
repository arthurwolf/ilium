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
