//! Host implementation of `ilium:host/codec`; image and text operations remain stubs.
//!
//! Written once by the skeleton lane from the generated `HostCodec` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::codec::{
    Compression, DecodedImage, HostCodec, ImageInfo, RasterText,
};
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};

/// Host side of the `codec` interface.
#[derive(Debug)]
pub struct CodecImport;

const MAX_INPUT: usize = 16 * 1024 * 1024;
const MAX_OUTPUT: usize = 64 * 1024 * 1024;

fn invalid_stream() -> PluginError {
    PluginError::new(
        ErrorCode::InvalidArgument,
        "invalid or incomplete compressed stream",
    )
}

fn output_exceeded() -> PluginError {
    PluginError::new(
        ErrorCode::QuotaExceeded,
        "decompressed output exceeds limit",
    )
}

fn inflate_stream(data: &[u8], max_output: usize, zlib: bool) -> Result<Vec<u8>, PluginError> {
    let mut decoder = flate2::Decompress::new(zlib);
    let mut output = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let before_in = decoder.total_in();
        let before_out = decoder.total_out();
        let available = (max_output - output.len())
            .saturating_add(1)
            .min(buffer.len());
        let status = decoder
            .decompress(
                &data[before_in as usize..],
                &mut buffer[..available],
                flate2::FlushDecompress::None,
            )
            .map_err(|_| invalid_stream())?;
        let produced = (decoder.total_out() - before_out) as usize;
        if produced > max_output - output.len() {
            return Err(output_exceeded());
        }
        output.extend_from_slice(&buffer[..produced]);
        if status == flate2::Status::StreamEnd {
            return if decoder.total_in() as usize == data.len() {
                Ok(output)
            } else {
                Err(invalid_stream())
            };
        }
        if decoder.total_in() == before_in && produced == 0 {
            return Err(invalid_stream());
        }
    }
}

fn inflate_gzip(data: &[u8], max_output: usize) -> Result<Vec<u8>, PluginError> {
    let mut decoder = flate2::bufread::GzDecoder::new(data);
    let mut output = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let available = (max_output - output.len())
            .saturating_add(1)
            .min(buffer.len());
        let count = decoder
            .read(&mut buffer[..available])
            .map_err(|_| invalid_stream())?;
        if count > max_output - output.len() {
            return Err(output_exceeded());
        }
        if count == 0 {
            return if decoder.get_ref().is_empty() {
                Ok(output)
            } else {
                Err(invalid_stream())
            };
        }
        output.extend_from_slice(&buffer[..count]);
    }
}

impl HostCodec for CodecImport {
    fn inflate(
        &mut self,
        format: Compression,
        data: Vec<u8>,
        max_output: u32,
    ) -> Result<Vec<u8>, PluginError> {
        if data.len() > MAX_INPUT || max_output as usize > MAX_OUTPUT {
            return Err(PluginError::new(
                ErrorCode::InvalidArgument,
                "invalid compression input or output limit",
            ));
        }
        match format {
            Compression::Zlib => inflate_stream(&data, max_output as usize, true),
            Compression::DeflateRaw => inflate_stream(&data, max_output as usize, false),
            Compression::Gzip => inflate_gzip(&data, max_output as usize),
        }
    }

    fn deflate(
        &mut self,
        format: Compression,
        data: Vec<u8>,
        level: u8,
    ) -> Result<Vec<u8>, PluginError> {
        if data.len() > MAX_INPUT || level > 9 {
            return Err(PluginError::new(
                ErrorCode::InvalidArgument,
                "invalid compression input or level",
            ));
        }
        let level = flate2::Compression::new(u32::from(level));
        let result = match format {
            Compression::Zlib => {
                let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), level);
                encoder.write_all(&data).and_then(|()| encoder.finish())
            }
            Compression::Gzip => {
                let mut encoder = flate2::write::GzEncoder::new(Vec::new(), level);
                encoder.write_all(&data).and_then(|()| encoder.finish())
            }
            Compression::DeflateRaw => {
                let mut encoder = flate2::write::DeflateEncoder::new(Vec::new(), level);
                encoder.write_all(&data).and_then(|()| encoder.finish())
            }
        };
        result.map_err(|_| PluginError::new(ErrorCode::Internal, "compression failed"))
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

    fn sha256(&mut self, data: Vec<u8>) -> Vec<u8> {
        Sha256::digest(data).to_vec()
    }

    fn rasterize_text(&mut self, _request: RasterText) -> Result<DecodedImage, PluginError> {
        unimplemented!("host::imports::codec::rasterize_text")
    }
}
