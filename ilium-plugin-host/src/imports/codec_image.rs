//! Bounded image metadata and frame decoding for the plugin codec import.
use std::io::Cursor;

use ilium_plugin_api::generated::host::codec::{DecodedImage, ImageInfo};
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use image::{AnimationDecoder, ImageDecoder, ImageFormat, ImageReader};

const MAX_INPUT: usize = 16 * 1024 * 1024;
const MAX_PIXELS: u32 = 67_108_864;

fn invalid() -> PluginError {
    PluginError::new(ErrorCode::InvalidArgument, "invalid or truncated image")
}

fn unsupported() -> PluginError {
    PluginError::new(ErrorCode::Unsupported, "unsupported image format")
}

fn format(data: &[u8]) -> Result<ImageFormat, PluginError> {
    if data.len() > MAX_INPUT {
        return Err(invalid());
    }
    match image::guess_format(data) {
        Ok(
            value @ (ImageFormat::Png
            | ImageFormat::Jpeg
            | ImageFormat::Gif
            | ImageFormat::Bmp
            | ImageFormat::WebP),
        ) => Ok(value),
        _ => Err(unsupported()),
    }
}

fn range(data: &[u8], offset: usize, length: usize) -> Result<&[u8], PluginError> {
    data.get(offset..offset.checked_add(length).ok_or_else(invalid)?)
        .ok_or_else(invalid)
}

fn gif_frames(data: &[u8]) -> Result<u32, PluginError> {
    let header = range(data, 0, 13)?;
    let canvas_width = u16::from_le_bytes([header[6], header[7]]) as u32;
    let canvas_height = u16::from_le_bytes([header[8], header[9]]) as u32;
    let mut pos = 13usize;
    if header[10] & 0x80 != 0 {
        pos += 3 * (1usize << ((header[10] & 7) + 1));
    }
    let mut frames = 0u32;
    loop {
        let marker = *range(data, pos, 1)?.first().ok_or_else(invalid)?;
        pos += 1;
        match marker {
            0x3b => {
                return if frames > 0 {
                    Ok(frames)
                } else {
                    Err(invalid())
                }
            }
            0x21 => {
                range(data, pos, 1)?;
                pos += 1;
            }
            0x2c => {
                let descriptor = range(data, pos, 9)?;
                let left = u16::from_le_bytes([descriptor[0], descriptor[1]]) as u32;
                let top = u16::from_le_bytes([descriptor[2], descriptor[3]]) as u32;
                let width = u16::from_le_bytes([descriptor[4], descriptor[5]]) as u32;
                let height = u16::from_le_bytes([descriptor[6], descriptor[7]]) as u32;
                if width == 0
                    || height == 0
                    || left + width > canvas_width
                    || top + height > canvas_height
                {
                    return Err(invalid());
                }
                pos += 9;
                if descriptor[8] & 0x80 != 0 {
                    pos += 3 * (1usize << ((descriptor[8] & 7) + 1));
                }
                range(data, pos, 1)?;
                pos += 1;
                frames = frames.checked_add(1).ok_or_else(invalid)?;
            }
            _ => return Err(invalid()),
        }
        loop {
            let len = range(data, pos, 1)?[0] as usize;
            pos += 1;
            if len == 0 {
                break;
            }
            range(data, pos, len)?;
            pos += len;
        }
    }
}

fn png_frames(data: &[u8]) -> Result<u32, PluginError> {
    let mut pos = 8usize;
    while pos < data.len() {
        let header = range(data, pos, 8)?;
        let len = u32::from_be_bytes(header[..4].try_into().map_err(|_| invalid())?) as usize;
        let chunk = range(data, pos + 8, len.checked_add(4).ok_or_else(invalid)?)?;
        if &header[4..8] == b"acTL" {
            if len != 8 {
                return Err(invalid());
            }
            let count = u32::from_be_bytes(chunk[..4].try_into().map_err(|_| invalid())?);
            return if count > 0 { Ok(count) } else { Err(invalid()) };
        }
        if &header[4..8] == b"IDAT" {
            return Ok(1);
        }
        pos = pos
            .checked_add(12)
            .and_then(|value| value.checked_add(len))
            .ok_or_else(invalid)?;
    }
    Err(invalid())
}

fn webp_frames(data: &[u8]) -> Result<u32, PluginError> {
    let header = range(data, 0, 12)?;
    let size = u32::from_le_bytes(header[4..8].try_into().map_err(|_| invalid())?) as usize;
    let end = size.checked_add(8).ok_or_else(invalid)?;
    if end > data.len() || end < 12 {
        return Err(invalid());
    }
    let mut pos = 12usize;
    let mut frames = 0u32;
    while pos < end {
        let chunk = range(data, pos, 8)?;
        let len = u32::from_le_bytes(chunk[4..8].try_into().map_err(|_| invalid())?) as usize;
        let next = pos
            .checked_add(8)
            .and_then(|value| value.checked_add(len))
            .and_then(|value| value.checked_add(len % 2))
            .ok_or_else(invalid)?;
        if next > end {
            return Err(invalid());
        }
        if &chunk[..4] == b"ANMF" {
            frames = frames.checked_add(1).ok_or_else(invalid)?;
        }
        pos = next;
    }
    Ok(frames.max(1))
}

pub(super) fn info(data: &[u8]) -> Result<ImageInfo, PluginError> {
    let kind = format(data)?;
    let reader = ImageReader::with_format(Cursor::new(data), kind);
    let decoder = reader.into_decoder().map_err(|_| invalid())?;
    let (width, height) = decoder.dimensions();
    if width == 0 || height == 0 {
        return Err(invalid());
    }
    let frames = match kind {
        ImageFormat::Gif => gif_frames(data)?,
        ImageFormat::Png => png_frames(data)?,
        ImageFormat::WebP => webp_frames(data)?,
        _ => 1,
    };
    Ok(ImageInfo {
        width,
        height,
        frames,
    })
}

pub(super) fn decode(
    data: &[u8],
    frame: u32,
    max_pixels: u32,
) -> Result<DecodedImage, PluginError> {
    if max_pixels > MAX_PIXELS {
        return Err(invalid());
    }
    let metadata = info(data)?;
    if u64::from(metadata.width) * u64::from(metadata.height) > u64::from(max_pixels) {
        return Err(PluginError::new(
            ErrorCode::QuotaExceeded,
            "image exceeds pixel limit",
        ));
    }
    if frame >= metadata.frames {
        return Err(invalid());
    }
    let kind = format(data)?;
    let animated = match kind {
        ImageFormat::Gif => true,
        ImageFormat::Png => image::codecs::png::PngDecoder::new(Cursor::new(data))
            .map_err(|_| invalid())?
            .is_apng()
            .map_err(|_| invalid())?,
        ImageFormat::WebP => image::codecs::webp::WebPDecoder::new(Cursor::new(data))
            .map_err(|_| invalid())?
            .has_animation(),
        _ => false,
    };
    let rgba = if animated {
        let image = match kind {
            ImageFormat::Gif => image::codecs::gif::GifDecoder::new(Cursor::new(data))
                .map_err(|_| invalid())?
                .into_frames()
                .nth(frame as usize),
            ImageFormat::Png => image::codecs::png::PngDecoder::new(Cursor::new(data))
                .map_err(|_| invalid())?
                .apng()
                .map_err(|_| invalid())?
                .into_frames()
                .nth(frame as usize),
            ImageFormat::WebP => image::codecs::webp::WebPDecoder::new(Cursor::new(data))
                .map_err(|_| invalid())?
                .into_frames()
                .nth(frame as usize),
            _ => return Err(invalid()),
        }
        .ok_or_else(invalid)?
        .map_err(|_| invalid())?;
        image.into_buffer().into_raw()
    } else {
        let mut reader = ImageReader::with_format(Cursor::new(data), kind);
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(metadata.width);
        limits.max_image_height = Some(metadata.height);
        reader.limits(limits);
        reader
            .decode()
            .map_err(|_| invalid())?
            .into_rgba8()
            .into_raw()
    };
    Ok(DecodedImage {
        width: metadata.width,
        height: metadata.height,
        rgba,
    })
}
