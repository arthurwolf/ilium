//! Host implementation of `ilium:host/codec` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostCodec` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::codec::{
    Compression, DecodedImage, HostCodec, ImageInfo, RasterText,
};
use ilium_plugin_api::generated::types::errors::PluginError;

/// Host side of the `codec` interface.
#[derive(Debug)]
pub struct CodecImport;

impl HostCodec for CodecImport {
    fn inflate(
        &mut self,
        _format: Compression,
        _data: Vec<u8>,
        _max_output: u32,
    ) -> Result<Vec<u8>, PluginError> {
        unimplemented!("host::imports::codec::inflate")
    }

    fn deflate(
        &mut self,
        _format: Compression,
        _data: Vec<u8>,
        _level: u8,
    ) -> Result<Vec<u8>, PluginError> {
        unimplemented!("host::imports::codec::deflate")
    }

    fn image_info(&mut self, _data: Vec<u8>) -> Result<ImageInfo, PluginError> {
        unimplemented!("host::imports::codec::image_info")
    }

    fn decode_image(
        &mut self,
        _data: Vec<u8>,
        _frame: u32,
        _max_pixels: u32,
    ) -> Result<DecodedImage, PluginError> {
        unimplemented!("host::imports::codec::decode_image")
    }

    fn sha256(&mut self, _data: Vec<u8>) -> Vec<u8> {
        unimplemented!("host::imports::codec::sha256")
    }

    fn rasterize_text(&mut self, _request: RasterText) -> Result<DecodedImage, PluginError> {
        unimplemented!("host::imports::codec::rasterize_text")
    }
}
