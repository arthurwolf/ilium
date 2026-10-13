//! Host implementation of `ilium:anim/fonts` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostFonts` trait; hand-owned afterwards.

use ilium_plugin_api::generated::anim::fonts::{FontHandle, HostFonts};
use ilium_plugin_api::generated::types::errors::PluginError;

/// Host side of the `fonts` interface.
#[derive(Debug)]
pub struct FontsImport;

impl HostFonts for FontsImport {
    fn font_new(&mut self, asset: String) -> FontHandle {
        unimplemented!("host::imports::fonts::font_new")
    }

    fn font_rasterize(
        &mut self,
        handle: FontHandle,
        text: String,
        size_px: u16,
    ) -> Result<(u32, u32, Vec<u8>), PluginError> {
        unimplemented!("host::imports::fonts::font_rasterize")
    }
}
