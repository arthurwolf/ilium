//! Pure admitted media kernels. Call only on a broker-owned CPU job; this module
//! never opens files, fetches URLs, spawns workers or authorizes a principal.
//! Codec allocation limits are best effort: physical decoder containment remains
//! the broker's responsibility. Every published allocation retains its original
//! quota admission through the final reader, including after handle retirement.
use crate::{
    error::{AnimationError, Result},
    package::Package,
};
use ilium_execution::{QuotaGroup, StorageAdmission};
use ilium_platform::owned_worker::StopToken;
use image::{DynamicImage, ImageDecoder, ImageReader};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::Cursor, sync::Arc};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

fn invalid(message: &str) -> AnimationError {
    AnimationError::Runtime(format!("native media: {message}"))
}
fn check(stop: &StopToken) -> Result<()> {
    if stop.is_stopped() {
        Err(invalid("cancelled"))
    } else {
        Ok(())
    }
}
fn charge(quota: &QuotaGroup, bytes: usize) -> Result<StorageAdmission> {
    quota
        .reserve_external_storage(bytes.max(1))
        .map_err(|error| AnimationError::Budget(format!("native media storage: {error:?}")))
}
fn allocation<T: Default + Clone>(count: usize) -> Result<Vec<T>> {
    let mut output = Vec::new();
    output
        .try_reserve_exact(count)
        .map_err(|_| AnimationError::Budget("native media allocation".into()))?;
    output.resize(count, T::default());
    Ok(output)
}
/// Immutable admitted payload; no Clone or mutable view. A caller using
/// into_parts must retain the returned guard through every queued/active use.
pub struct Admitted<T> {
    value: T,
    quota: QuotaGroup,
    admission: StorageAdmission,
}
impl<T: std::fmt::Debug> std::fmt::Debug for Admitted<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Admitted")
            .field("value", &self.value)
            .field("admission", &self.admission)
            .finish_non_exhaustive()
    }
}
impl<T> Admitted<T> {
    pub fn view(&self) -> &T {
        &self.value
    }
    /// Exact original ledger identity, never equivalent configured ceilings.
    /// A matching root is allocation custody, not source/grant authority.
    pub fn shares_root(&self, quota: &QuotaGroup) -> bool {
        self.quota.shares_root(quota)
    }
    pub fn into_parts(self) -> (T, StorageAdmission) {
        (self.value, self.admission)
    }
}
#[derive(Debug, Clone, Copy)]
pub struct MediaLimits {
    pub encoded_bytes: usize,
    pub max_dimension: u32,
    pub pixels: usize,
    pub handles: usize,
    pub decoder_scratch_bytes: usize,
    pub batch_samples: usize,
    pub mesh_triangles: usize,
    pub mesh_work: usize,
    pub text_bytes: usize,
    pub text_graphemes: usize,
}
impl Default for MediaLimits {
    fn default() -> Self {
        Self {
            encoded_bytes: 16 * 1024 * 1024,
            max_dimension: 8192,
            pixels: 4 * 1024 * 1024,
            handles: 64,
            decoder_scratch_bytes: 128 * 1024 * 1024,
            batch_samples: 1024 * 1024,
            mesh_triangles: 2048,
            mesh_work: 16 * 1024 * 1024,
            text_bytes: 16 * 1024,
            text_graphemes: 1024,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ImageHandle(u64);
impl ImageHandle {
    pub fn id(self) -> u64 {
        self.0
    }
    pub fn from_id(id: u64) -> Self {
        Self(id)
    }
}
#[derive(Debug)]
pub struct ImagePixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}
#[derive(Debug, Clone, Copy)]
pub enum Sampling {
    Nearest,
    Bilinear,
    Triangle,
}
#[derive(Debug)]
pub struct AssetInfo {
    pub name: String,
    pub bytes: usize,
    pub sha256: String,
}
#[derive(Debug)]
pub struct MeshOutput {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<[u8; 3]>,
    pub depth: Vec<f32>,
}
#[derive(Debug, Clone, Copy)]
pub struct MeshVertex {
    pub x: f32,
    pub y: f32,
    pub depth: f32,
    pub uv: [f32; 2],
}
#[derive(Debug, Clone, Copy)]
pub struct MeshTriangle {
    pub vertices: [MeshVertex; 3],
    pub texture: ImageHandle,
    pub sort_key: u64,
}
#[derive(Debug)]
pub struct FontRaster {
    pub width: u32,
    pub height: u32,
    pub mask: Vec<u8>,
    pub advance: f32,
    pub line_height: f32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextStyle {
    pub foreground: Option<[u8; 3]>,
    pub background: Option<[u8; 3]>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}
#[derive(Debug)]
pub struct TextSpan<'a> {
    pub text: &'a str,
    pub style: TextStyle,
}
#[derive(Debug)]
pub struct StyledCell {
    pub x: u32,
    pub glyph: String,
    pub width: u8,
    pub continuation: bool,
    pub style: TextStyle,
}

pub struct NativeMedia {
    quota: QuotaGroup,
    limits: MediaLimits,
    images: BTreeMap<ImageHandle, Arc<Admitted<ImagePixels>>>,
    next_handle: u64,
    _metadata: StorageAdmission,
}
impl NativeMedia {
    pub fn new(quota: QuotaGroup, limits: MediaLimits) -> Result<Self> {
        if limits.encoded_bytes == 0
            || limits.encoded_bytes > 64 * 1024 * 1024
            || limits.max_dimension == 0
            || limits.max_dimension > 16384
            || limits.pixels == 0
            || limits.pixels > 16 * 1024 * 1024
            || limits.handles == 0
            || limits.handles > 256
            || limits.decoder_scratch_bytes < 8 * 1024 * 1024
            || limits.decoder_scratch_bytes > 1024 * 1024 * 1024
            || limits.batch_samples == 0
            || limits.batch_samples > 4 * 1024 * 1024
            || limits.mesh_triangles == 0
            || limits.mesh_triangles > 8192
            || limits.mesh_work == 0
            || limits.mesh_work > 64 * 1024 * 1024
            || limits.text_bytes == 0
            || limits.text_bytes > 64 * 1024
            || limits.text_graphemes == 0
            || limits.text_graphemes > 4096
        {
            return Err(AnimationError::Budget("native media limits".into()));
        }
        let metadata = charge(&quota, limits.handles * 512 + std::mem::size_of::<Self>())?;
        Ok(Self {
            quota,
            limits,
            images: BTreeMap::new(),
            next_handle: 1,
            _metadata: metadata,
        })
    }
    fn pixels(&self, width: u32, height: u32) -> Result<usize> {
        let count = (width as usize)
            .checked_mul(height as usize)
            .ok_or_else(|| invalid("pixel overflow"))?;
        if width == 0
            || height == 0
            || width > self.limits.max_dimension
            || height > self.limits.max_dimension
            || count > self.limits.pixels
        {
            return Err(AnimationError::Budget(
                "native media dimensions/pixels".into(),
            ));
        }
        Ok(count)
    }
    fn insert(&mut self, image: Admitted<ImagePixels>) -> Result<ImageHandle> {
        self.retain_admitted_image(Arc::new(image))
    }
    /// Register a fresh target-local handle for the exact original immutable
    /// allocation. Call only inside the native owner's current source/demand
    /// authorization fence; this method itself authenticates no principal.
    /// Target metadata was admitted at construction; pixel bytes keep their
    /// original admission through every Arc, without copying or re-decoding.
    pub fn retain_admitted_image(
        &mut self,
        image: Arc<Admitted<ImagePixels>>,
    ) -> Result<ImageHandle> {
        if !image.shares_root(&self.quota) {
            return Err(AnimationError::PermissionDenied(
                "native image import original root mismatch".into(),
            ));
        }
        let pixels = image.view();
        let count = self.pixels(pixels.width, pixels.height)?;
        let bytes = count
            .checked_mul(4)
            .ok_or_else(|| invalid("imported RGBA byte overflow"))?;
        if pixels.rgba.len() != bytes || pixels.rgba.capacity() > bytes {
            return Err(invalid("imported RGBA shape/capacity"));
        }
        if self.images.len() >= self.limits.handles {
            return Err(AnimationError::Budget("native image handles".into()));
        }
        let handle = ImageHandle(self.next_handle);
        let next_handle = self
            .next_handle
            .checked_add(1)
            .ok_or_else(|| invalid("image handle space exhausted"))?;
        self.images.insert(handle, image);
        self.next_handle = next_handle;
        Ok(handle)
    }
    pub fn snapshot(&self, handle: ImageHandle) -> Result<Arc<Admitted<ImagePixels>>> {
        self.images.get(&handle).cloned().ok_or_else(|| {
            AnimationError::PermissionDenied(
                "unknown native image handle in this broker instance".into(),
            )
        })
    }
    pub fn close(&mut self, handle: ImageHandle) -> Result<()> {
        self.images
            .remove(&handle)
            .ok_or_else(|| invalid("unknown image handle"))?;
        Ok(())
    }
    pub fn decode(&mut self, encoded: &[u8], stop: &StopToken) -> Result<ImageHandle> {
        check(stop)?;
        if encoded.len() > self.limits.encoded_bytes || self.images.len() >= self.limits.handles {
            return Err(AnimationError::Budget(
                "native image encoded bytes/handles".into(),
            ));
        }
        // Reserve before even constructing a decoder: format/header parsers can allocate.
        let _scratch = charge(&self.quota, self.limits.decoder_scratch_bytes)?;
        let mut reader = ImageReader::new(Cursor::new(encoded))
            .with_guessed_format()
            .map_err(|error| invalid(&error.to_string()))?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(self.limits.max_dimension);
        limits.max_image_height = Some(self.limits.max_dimension);
        limits.max_alloc = Some(self.limits.decoder_scratch_bytes as u64);
        reader.limits(limits);
        let mut decoder = reader
            .into_decoder()
            .map_err(|error| invalid(&error.to_string()))?;
        let (width, height) = decoder.dimensions();
        let count = self.pixels(width, height)?;
        if decoder.total_bytes() > self.limits.decoder_scratch_bytes as u64 {
            return Err(AnimationError::Budget("native decoded source bytes".into()));
        }
        let decode_peak = usize::try_from(decoder.total_bytes())
            .ok()
            .and_then(|n| n.checked_mul(3))
            .and_then(|n| n.checked_add(count * 4))
            .and_then(|n| n.checked_add(encoded.len() * 2))
            .and_then(|n| n.checked_add(4 * 1024 * 1024))
            .ok_or_else(|| invalid("decoder peak estimate overflow"))?;
        if decode_peak > self.limits.decoder_scratch_bytes {
            return Err(AnimationError::Budget(
                "native decode/orientation/conversion peak".into(),
            ));
        }
        let admission = charge(&self.quota, count * 4 + 256)?;
        check(stop)?;
        let orientation = decoder
            .orientation()
            .map_err(|error| invalid(&error.to_string()))?;
        let mut image =
            DynamicImage::from_decoder(decoder).map_err(|error| invalid(&error.to_string()))?;
        check(stop)?;
        image.apply_orientation(orientation);
        let image = image.into_rgba8();
        let (width, height) = image.dimensions();
        let count = self.pixels(width, height)?;
        let rgba = image.into_raw();
        if rgba.len() != count * 4 || rgba.capacity() > count * 4 {
            return Err(invalid("decoded RGBA shape"));
        }
        check(stop)?;
        self.insert(Admitted {
            quota: self.quota.clone(),
            value: ImagePixels {
                width,
                height,
                rgba,
            },
            admission,
        })
    }
    pub fn decode_asset(
        &mut self,
        package: &Package,
        name: &str,
        stop: &StopToken,
    ) -> Result<ImageHandle> {
        check(stop)?;
        let bytes = package
            .files()
            .get(name)
            .ok_or_else(|| invalid("package asset not found"))?;
        if !name.starts_with("assets/") {
            return Err(AnimationError::PermissionDenied(
                "media reads only immutable package assets".into(),
            ));
        }
        self.decode(bytes, stop)
    }
    pub fn asset_list(
        &self,
        package: &Package,
        prefix: &str,
        limit: usize,
        stop: &StopToken,
    ) -> Result<Admitted<Vec<AssetInfo>>> {
        check(stop)?;
        if !prefix.starts_with("assets/") || prefix.contains("..") || limit == 0 || limit > 256 {
            return Err(invalid("asset list scope/limit"));
        }
        let _inventory_scratch = charge(&self.quota, (limit + 1) * 32 + 128)?;
        let entries: Vec<_> = package
            .files()
            .iter()
            .filter(|(name, _)| name.starts_with(prefix))
            .take(limit + 1)
            .collect();
        if entries.len() > limit {
            return Err(AnimationError::Budget(
                "asset list entries; narrow prefix".into(),
            ));
        }
        let bytes = entries.iter().try_fold(256usize, |sum, (name, _)| {
            sum.checked_add(name.len() + 256)
                .ok_or_else(|| invalid("asset inventory overflow"))
        })?;
        let admission = charge(&self.quota, bytes)?;
        let mut output = Vec::new();
        output
            .try_reserve_exact(entries.len())
            .map_err(|_| invalid("asset inventory allocation"))?;
        for (name, data) in entries {
            check(stop)?;
            output.push(AssetInfo {
                name: name.clone(),
                bytes: data.len(),
                sha256: self
                    .asset_hash(package, name, stop)?
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect(),
            });
        }
        Ok(Admitted {
            quota: self.quota.clone(),
            value: output,
            admission,
        })
    }
    pub fn asset_hash(&self, package: &Package, name: &str, stop: &StopToken) -> Result<[u8; 32]> {
        check(stop)?;
        if !name.starts_with("assets/") {
            return Err(AnimationError::PermissionDenied(
                "immutable package asset hash scope".into(),
            ));
        }
        let bytes = package
            .files()
            .get(name)
            .ok_or_else(|| invalid("package asset not found"))?;
        let mut hash = Sha256::new();
        for chunk in bytes.chunks(65536) {
            check(stop)?;
            hash.update(chunk);
        }
        Ok(hash.finalize().into())
    }
    pub fn asset_read(
        &self,
        package: &Package,
        name: &str,
        maximum: usize,
        stop: &StopToken,
    ) -> Result<Admitted<Vec<u8>>> {
        check(stop)?;
        if !name.starts_with("assets/") || maximum > self.limits.encoded_bytes {
            return Err(AnimationError::PermissionDenied(
                "immutable asset read scope/bound".into(),
            ));
        }
        let bytes = package
            .files()
            .get(name)
            .ok_or_else(|| invalid("package asset not found"))?;
        if bytes.len() > maximum {
            return Err(AnimationError::Budget("asset read bytes".into()));
        }
        let admission = charge(&self.quota, bytes.len() + 128)?;
        let mut output = Vec::new();
        output
            .try_reserve_exact(bytes.len())
            .map_err(|_| invalid("asset allocation"))?;
        for chunk in bytes.chunks(65536) {
            check(stop)?;
            output.extend_from_slice(chunk);
        }
        Ok(Admitted {
            quota: self.quota.clone(),
            value: output,
            admission,
        })
    }
    pub fn sample(
        &self,
        handle: ImageHandle,
        coordinates: &[[f32; 2]],
        sampling: Sampling,
        stop: &StopToken,
    ) -> Result<Admitted<Vec<[f32; 4]>>> {
        check(stop)?;
        if coordinates.len() > self.limits.batch_samples
            || coordinates.iter().flatten().any(|value| !value.is_finite())
        {
            return Err(invalid("sample batch bounds/nonfinite coordinates"));
        }
        let image = self.snapshot(handle)?;
        let admission = charge(&self.quota, coordinates.len() * 16 + 128)?;
        let mut output = allocation(coordinates.len())?;
        for (index, uv) in coordinates.iter().enumerate() {
            if index % 1024 == 0 {
                check(stop)?;
            }
            output[index] = sample(image.view(), *uv, sampling);
        }
        Ok(Admitted {
            quota: self.quota.clone(),
            value: output,
            admission,
        })
    }
    pub fn resample(
        &mut self,
        handle: ImageHandle,
        width: u32,
        height: u32,
        sampling: Sampling,
        stop: &StopToken,
    ) -> Result<ImageHandle> {
        check(stop)?;
        let count = self.pixels(width, height)?;
        if self.images.len() >= self.limits.handles {
            return Err(AnimationError::Budget("native image handles".into()));
        }
        let image = self.snapshot(handle)?;
        let admission = charge(&self.quota, count * 4 + 256)?;
        let mut rgba = allocation(count * 4)?;
        if matches!(sampling, Sampling::Triangle) {
            let source = image.view();
            let source_bytes = source.rgba.len();
            let scratch_bytes = (source.width as usize)
                .checked_mul(height as usize)
                .and_then(|n| n.checked_mul(16))
                .and_then(|n| n.checked_add(source_bytes))
                .and_then(|n| n.checked_add(count * 4))
                .and_then(|n| {
                    n.checked_add(
                        (self.limits.max_dimension as usize + width as usize + height as usize)
                            * 64
                            + 4096,
                    )
                })
                .ok_or_else(|| invalid("triangle filter scratch overflow"))?;
            if scratch_bytes > self.limits.decoder_scratch_bytes {
                return Err(AnimationError::Budget("triangle filter scratch".into()));
            }
            let _scratch = charge(&self.quota, scratch_bytes + 1024)?;
            let mut premultiplied = allocation::<u8>(source_bytes)?;
            for (index, pixel) in source.rgba.chunks_exact(4).enumerate() {
                if index % 1024 == 0 {
                    check(stop)?;
                }
                for channel in 0..3 {
                    premultiplied[index * 4 + channel] =
                        ((pixel[channel] as u32 * pixel[3] as u32 + 127) / 255) as u8;
                }
                premultiplied[index * 4 + 3] = pixel[3];
            }
            let original = image::RgbaImage::from_raw(source.width, source.height, premultiplied)
                .ok_or_else(|| invalid("triangle source shape"))?;
            let filtered = image::imageops::resize(
                &original,
                width,
                height,
                image::imageops::FilterType::Triangle,
            );
            check(stop)?;
            for (index, pixel) in filtered.pixels().enumerate() {
                if index % 1024 == 0 {
                    check(stop)?;
                }
                for channel in 0..3 {
                    rgba[index * 4 + channel] = if pixel[3] == 0 {
                        0
                    } else {
                        ((pixel[channel] as u32 * 255 + pixel[3] as u32 / 2) / pixel[3] as u32)
                            .min(255) as u8
                    };
                }
                rgba[index * 4 + 3] = pixel[3];
            }
            return self.insert(Admitted {
                quota: self.quota.clone(),
                value: ImagePixels {
                    width,
                    height,
                    rgba,
                },
                admission,
            });
        }
        for y in 0..height {
            check(stop)?;
            for x in 0..width {
                let color = sample(
                    image.view(),
                    [
                        (x as f32 + 0.5) / width as f32,
                        (y as f32 + 0.5) / height as f32,
                    ],
                    sampling,
                );
                let index = (y as usize * width as usize + x as usize) * 4;
                let alpha = color[3];
                for c in 0..3 {
                    rgba[index + c] = if alpha > 0.0 {
                        byte(color[c] / alpha)
                    } else {
                        0
                    };
                }
                rgba[index + 3] = byte(alpha);
            }
        }
        check(stop)?;
        self.insert(Admitted {
            quota: self.quota.clone(),
            value: ImagePixels {
                width,
                height,
                rgba,
            },
            admission,
        })
    }
    pub fn thumbnail(
        &mut self,
        handle: ImageHandle,
        max_width: u32,
        max_height: u32,
        stop: &StopToken,
    ) -> Result<ImageHandle> {
        self.pixels(max_width, max_height)?;
        let image = self.snapshot(handle)?;
        let ratio = (max_width as f64 / image.view().width as f64)
            .min(max_height as f64 / image.view().height as f64)
            .min(1.0);
        let width = (image.view().width as f64 * ratio).round().max(1.0) as u32;
        let height = (image.view().height as f64 * ratio).round().max(1.0) as u32;
        self.resample(handle, width, height, Sampling::Triangle, stop)
    }
    /// Creates levels below the original. Transactionally retires partial levels on refusal/cancel.
    pub fn mipmaps(
        &mut self,
        handle: ImageHandle,
        maximum_levels: usize,
        stop: &StopToken,
    ) -> Result<Admitted<Vec<ImageHandle>>> {
        check(stop)?;
        if maximum_levels == 0 || maximum_levels > 16 {
            return Err(AnimationError::Budget("native mip levels/handles".into()));
        }
        let original = self.snapshot(handle)?;
        let (mut width, mut height) = (original.view().width, original.view().height);
        let mut needed = 0;
        while needed < maximum_levels && (width > 1 || height > 1) {
            needed += 1;
            width = (width / 2).max(1);
            height = (height / 2).max(1);
        }
        if self.images.len() + needed > self.limits.handles {
            return Err(AnimationError::Budget("native mip handles".into()));
        }
        let admission = charge(
            &self.quota,
            needed * std::mem::size_of::<ImageHandle>() + 128,
        )?;
        let mut levels = Vec::new();
        levels
            .try_reserve_exact(needed)
            .map_err(|_| invalid("mip inventory allocation"))?;
        let mut current = handle;
        for _ in 0..needed {
            let image = self.snapshot(current)?;
            if image.view().width == 1 && image.view().height == 1 {
                break;
            }
            let width = (image.view().width / 2).max(1);
            let height = (image.view().height / 2).max(1);
            match self.resample(current, width, height, Sampling::Triangle, stop) {
                Ok(next) => {
                    levels.push(next);
                    current = next;
                }
                Err(error) => {
                    for level in levels {
                        let _ = self.close(level);
                    }
                    return Err(error);
                }
            }
        }
        Ok(Admitted {
            quota: self.quota.clone(),
            value: levels,
            admission,
        })
    }
    pub fn luminance(&self, handle: ImageHandle, stop: &StopToken) -> Result<Admitted<Vec<f32>>> {
        check(stop)?;
        let image = self.snapshot(handle)?;
        let count = self.pixels(image.view().width, image.view().height)?;
        let admission = charge(&self.quota, count * 4 + 128)?;
        let mut values = allocation(count)?;
        for (index, pixel) in image.view().rgba.chunks_exact(4).enumerate() {
            if index % 1024 == 0 {
                check(stop)?;
            }
            let alpha = pixel[3] as f32 / 255.0;
            values[index] =
                (0.2126 * pixel[0] as f32 + 0.7152 * pixel[1] as f32 + 0.0722 * pixel[2] as f32)
                    * alpha
                    / 255.0;
        }
        Ok(Admitted {
            quota: self.quota.clone(),
            value: values,
            admission,
        })
    }
    pub fn palette(
        &self,
        handle: ImageHandle,
        palette: &[[u8; 3]],
        stop: &StopToken,
    ) -> Result<Admitted<Vec<u8>>> {
        check(stop)?;
        let image = self.snapshot(handle)?;
        let count = self.pixels(image.view().width, image.view().height)?;
        if palette.is_empty()
            || palette.len() > 256
            || count
                .checked_mul(palette.len())
                .is_none_or(|work| work > self.limits.mesh_work)
        {
            return Err(AnimationError::Budget("palette work/colors".into()));
        }
        let admission = charge(&self.quota, count + 128)?;
        let mut indices = allocation(count)?;
        for (index, pixel) in image.view().rgba.chunks_exact(4).enumerate() {
            if index % 1024 == 0 {
                check(stop)?;
            }
            let rgb = [0, 1, 2].map(|c| ((pixel[c] as u32 * pixel[3] as u32 + 127) / 255) as i32);
            let nearest = palette
                .iter()
                .enumerate()
                .min_by_key(|(_, color)| {
                    [0, 1, 2]
                        .iter()
                        .map(|&c| (rgb[c] - color[c] as i32).pow(2))
                        .sum::<i32>()
                })
                .ok_or_else(|| invalid("empty palette"))?;
            indices[index] = nearest.0 as u8;
        }
        Ok(Admitted {
            quota: self.quota.clone(),
            value: indices,
            admission,
        })
    }
    /// Explicit normalized source crop, destination rectangle, and source-over
    /// composition. The returned RGBA buffer is immutable and independently owned.
    pub fn blit(
        &self,
        handle: ImageHandle,
        dimensions: [u32; 2],
        rectangle: [u32; 4],
        source_rect: [f32; 4],
        background: [u8; 4],
        stop: &StopToken,
    ) -> Result<Admitted<ImagePixels>> {
        check(stop)?;
        let [width, height] = dimensions;
        let count = self.pixels(width, height)?;
        let [x, y, w, h] = rectangle;
        if w == 0
            || h == 0
            || x.checked_add(w).is_none_or(|right| right > width)
            || y.checked_add(h).is_none_or(|bottom| bottom > height)
            || source_rect
                .iter()
                .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
            || source_rect[0] > source_rect[2]
            || source_rect[1] > source_rect[3]
        {
            return Err(invalid("image blit rectangle/crop"));
        }
        let image = self.snapshot(handle)?;
        let admission = charge(&self.quota, count * 4 + 256)?;
        let mut rgba = allocation(count * 4)?;
        for pixel in rgba.chunks_exact_mut(4) {
            pixel.copy_from_slice(&background);
        }
        let background_alpha = background[3] as f32 / 255.0;
        for row in 0..h {
            check(stop)?;
            for column in 0..w {
                let u = source_rect[0]
                    + (source_rect[2] - source_rect[0]) * (column as f32 + 0.5) / w as f32;
                let v = source_rect[1]
                    + (source_rect[3] - source_rect[1]) * (row as f32 + 0.5) / h as f32;
                let source = sample(image.view(), [u, v], Sampling::Bilinear);
                let alpha = source[3] + background_alpha * (1.0 - source[3]);
                let index = ((y + row) as usize * width as usize + (x + column) as usize) * 4;
                for c in 0..3 {
                    rgba[index + c] = if alpha > 0.0 {
                        byte(
                            (source[c]
                                + background[c] as f32 / 255.0
                                    * background_alpha
                                    * (1.0 - source[3]))
                                / alpha,
                        )
                    } else {
                        0
                    };
                }
                rgba[index + 3] = byte(alpha);
            }
        }
        Ok(Admitted {
            quota: self.quota.clone(),
            value: ImagePixels {
                width,
                height,
                rgba,
            },
            admission,
        })
    }
    /// CPU-native opaque affine/depth batch, identical to the existing ambient Canvas.
    /// sort_key is a local tie rank, never authenticated world/source provenance.
    pub fn mesh(
        &self,
        width: u32,
        height: u32,
        triangles: &[MeshTriangle],
        stop: &StopToken,
    ) -> Result<Admitted<MeshOutput>> {
        check(stop)?;
        let count = self.pixels(width, height)?;
        if triangles.len() > self.limits.mesh_triangles {
            return Err(AnimationError::Budget("native mesh triangles".into()));
        }
        let _validation = charge(&self.quota, triangles.len() * 256 + 128)?;
        let mut work = 0usize;
        let mut ranks = std::collections::BTreeSet::new();
        for triangle in triangles {
            check(stop)?;
            if !ranks.insert(triangle.sort_key)
                || triangle.vertices.iter().any(|v| {
                    [v.x, v.y, v.depth, v.uv[0], v.uv[1]]
                        .iter()
                        .any(|value| !value.is_finite())
                })
            {
                return Err(invalid("native mesh nonfinite vertex/duplicate rank"));
            }
            self.snapshot(triangle.texture)?;
            let left = triangle
                .vertices
                .iter()
                .map(|v| v.x)
                .fold(f32::INFINITY, f32::min)
                .floor()
                .clamp(0., width as f32) as usize;
            let right = triangle
                .vertices
                .iter()
                .map(|v| v.x)
                .fold(f32::NEG_INFINITY, f32::max)
                .ceil()
                .clamp(0., width as f32) as usize;
            let top = triangle
                .vertices
                .iter()
                .map(|v| v.y)
                .fold(f32::INFINITY, f32::min)
                .floor()
                .clamp(0., height as f32) as usize;
            let bottom = triangle
                .vertices
                .iter()
                .map(|v| v.y)
                .fold(f32::NEG_INFINITY, f32::max)
                .ceil()
                .clamp(0., height as f32) as usize;
            work = work
                .checked_add((right - left) * (bottom - top))
                .ok_or_else(|| invalid("mesh work overflow"))?;
            if work > self.limits.mesh_work {
                return Err(AnimationError::Budget("native mesh raster work".into()));
            }
        }
        let admission = charge(&self.quota, count * 48 + triangles.len() * 128 + 256)?;
        let mut canvas =
            ilium_ambient::voxel_landscape::render::Canvas::new(width as usize, height as usize);
        for triangle in triangles {
            check(stop)?;
            let image = self.snapshot(triangle.texture)?;
            let vertices = triangle.vertices.map(|v| {
                ilium_ambient::voxel_landscape::render::Vertex::new(
                    v.x, v.y, v.depth, v.uv[0], v.uv[1],
                )
            });
            canvas.face_owned(
                [vertices[0], vertices[1], vertices[2], vertices[2]],
                triangle.sort_key,
                |u, v| {
                    let color = sample(image.view(), [u, v], Sampling::Bilinear);
                    [byte(color[0]), byte(color[1]), byte(color[2])]
                },
            );
        }
        check(stop)?;
        Ok(Admitted {
            quota: self.quota.clone(),
            value: MeshOutput {
                width,
                height,
                rgb: canvas.colors,
                depth: canvas.depth,
            },
            admission,
        })
    }
    /// Real bundled Cascadia coverage, using the same shaping/raster path as Pi.
    /// No system font discovery or mutable global font cache.
    pub fn raster_text(
        &self,
        text: &str,
        size: f32,
        width: u32,
        height: u32,
        stop: &StopToken,
    ) -> Result<Admitted<FontRaster>> {
        use cosmic_text::{
            Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache, Wrap,
        };
        check(stop)?;
        let count = self.pixels(width, height)?;
        if !size.is_finite()
            || !(8.0..=128.0).contains(&size)
            || text.len() > self.limits.text_bytes
            || text.graphemes(true).count() > self.limits.text_graphemes
            || text.chars().any(char::is_control)
        {
            return Err(invalid("native font text/size bounds"));
        }
        let _font_setup = charge(&self.quota, 32 * 1024 * 1024)?;
        let admission = charge(&self.quota, count + 256)?;
        let mut database = cosmic_text::fontdb::Database::new();
        database.load_font_data(
            include_bytes!("../../ilium-ambient/assets/fonts/CascadiaCode-Regular.otf").to_vec(),
        );
        let family = database
            .faces()
            .next()
            .and_then(|face| face.families.first())
            .map(|(name, _)| name.clone())
            .ok_or_else(|| invalid("bundled font has no face"))?;
        let mut fonts = FontSystem::new_with_locale_and_db("en-US".into(), database);
        let mut cache = SwashCache::new();
        let line_height = height as f32;
        let mut buffer = Buffer::new(&mut fonts, Metrics::new(size, line_height));
        buffer.set_size(&mut fonts, Some(width as f32), Some(height as f32));
        buffer.set_wrap(&mut fonts, Wrap::None);
        buffer.set_text(
            &mut fonts,
            text,
            &Attrs::new().family(Family::Name(&family)),
            Shaping::Advanced,
        );
        buffer.shape_until_scroll(&mut fonts, false);
        check(stop)?;
        let advance = buffer
            .layout_runs()
            .map(|run| run.line_w)
            .fold(0.0_f32, f32::max);
        if buffer
            .layout_runs()
            .flat_map(|run| run.glyphs.iter())
            .any(|glyph| glyph.glyph_id == 0)
        {
            return Err(invalid("bundled font lacks requested glyph"));
        }
        let glyph_count = buffer
            .layout_runs()
            .map(|run| run.glyphs.len())
            .sum::<usize>();
        let cache_bytes = glyph_count
            .checked_mul(size.ceil() as usize)
            .and_then(|n| n.checked_mul(size.ceil() as usize))
            .and_then(|n| n.checked_mul(16))
            .ok_or_else(|| invalid("font cache budget overflow"))?;
        if cache_bytes > self.limits.decoder_scratch_bytes {
            return Err(AnimationError::Budget("native font glyph cache".into()));
        }
        let _glyph_cache = charge(&self.quota, cache_bytes + 4096)?;
        let mut mask = allocation::<u8>(count)?;
        buffer.draw(
            &mut fonts,
            &mut cache,
            Color::rgb(255, 255, 255),
            |x, y, _, _, color| {
                if x >= 0 && y >= 0 && (x as u32) < width && (y as u32) < height {
                    let cell = &mut mask[y as usize * width as usize + x as usize];
                    *cell = (*cell).max(color.a());
                }
            },
        );
        check(stop)?;
        Ok(Admitted {
            quota: self.quota.clone(),
            value: FontRaster {
                width,
                height,
                mask,
                advance,
                line_height,
            },
            admission,
        })
    }
    pub fn glyph_mask(
        &self,
        glyph: &str,
        size: f32,
        stop: &StopToken,
    ) -> Result<Admitted<FontRaster>> {
        if !size.is_finite() || !(8.0..=128.0).contains(&size) || glyph.graphemes(true).count() != 1
        {
            return Err(invalid("glyph mask requires one grapheme"));
        }
        let width = ((size * 0.75).ceil() as u32 + 2).div_ceil(2) * 2;
        let height = ((size * 1.4).ceil() as u32).div_ceil(4) * 4;
        self.raster_text(glyph, size, width, height, stop)
    }
    /// Single-row native text. A width-two grapheme always has an explicit empty
    /// continuation; unsupported controls/standalone zero-width clusters reject.
    pub fn styled_cells(
        &self,
        spans: &[TextSpan<'_>],
        maximum_columns: u32,
        stop: &StopToken,
    ) -> Result<Admitted<Vec<StyledCell>>> {
        check(stop)?;
        if maximum_columns == 0 || maximum_columns > 16384 || spans.len() > 256 {
            return Err(invalid("styled text bounds"));
        }
        let text_bytes = spans.iter().try_fold(0usize, |sum, span| {
            sum.checked_add(span.text.len())
                .ok_or_else(|| invalid("styled text bytes overflow"))
        })?;
        if text_bytes > self.limits.text_bytes {
            return Err(AnimationError::Budget("styled text bytes".into()));
        }
        let admission = charge(
            &self.quota,
            maximum_columns as usize * 256 + text_bytes + 256,
        )?;
        let mut cells = Vec::new();
        cells
            .try_reserve_exact(maximum_columns as usize)
            .map_err(|_| invalid("styled cell allocation"))?;
        let mut x = 0u32;
        let mut graphemes = 0usize;
        for span in spans {
            if span.text.chars().any(char::is_control) {
                return Err(invalid("styled text contains control/escape"));
            }
            for glyph in span.text.graphemes(true) {
                check(stop)?;
                graphemes += 1;
                if graphemes > self.limits.text_graphemes {
                    return Err(AnimationError::Budget("styled text graphemes".into()));
                }
                let width = UnicodeWidthStr::width(glyph);
                if width == 0 || width > 2 || x + width as u32 > maximum_columns {
                    return Err(invalid("styled glyph width/continuation bounds"));
                }
                cells.push(StyledCell {
                    x,
                    glyph: glyph.to_owned(),
                    width: width as u8,
                    continuation: false,
                    style: span.style,
                });
                if width == 2 {
                    cells.push(StyledCell {
                        x: x + 1,
                        glyph: String::new(),
                        width: 0,
                        continuation: true,
                        style: span.style,
                    });
                }
                x += width as u32;
            }
        }
        Ok(Admitted {
            quota: self.quota.clone(),
            value: cells,
            admission,
        })
    }
}
fn byte(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}
/// Premultiplied RGBA sampling prevents invisible colored texels bleeding at edges.
fn sample(image: &ImagePixels, uv: [f32; 2], sampling: Sampling) -> [f32; 4] {
    let texel = |x: i64, y: i64| {
        let x = x.clamp(0, image.width as i64 - 1) as usize;
        let y = y.clamp(0, image.height as i64 - 1) as usize;
        let index = (y * image.width as usize + x) * 4;
        let alpha = image.rgba[index + 3] as f32 / 255.0;
        [
            image.rgba[index] as f32 / 255.0 * alpha,
            image.rgba[index + 1] as f32 / 255.0 * alpha,
            image.rgba[index + 2] as f32 / 255.0 * alpha,
            alpha,
        ]
    };
    let x = uv[0].clamp(0.0, 1.0) * image.width as f32 - 0.5;
    let y = uv[1].clamp(0.0, 1.0) * image.height as f32 - 0.5;
    if matches!(sampling, Sampling::Nearest) {
        return texel((x + 0.5).floor() as i64, (y + 0.5).floor() as i64);
    }
    let ix = x.floor();
    let iy = y.floor();
    let fx = x - ix;
    let fy = y - iy;
    let [a, b, c, d] = [
        texel(ix as i64, iy as i64),
        texel(ix as i64 + 1, iy as i64),
        texel(ix as i64, iy as i64 + 1),
        texel(ix as i64 + 1, iy as i64 + 1),
    ];
    let mut output = [0.0; 4];
    for channel in 0..4 {
        let top = a[channel] + (b[channel] - a[channel]) * fx;
        let bottom = c[channel] + (d[channel] - c[channel]) * fx;
        output[channel] = top + (bottom - top) * fy;
    }
    output
}

#[cfg(test)]
mod admitted_image_import_tests {
    use super::*;
    use ilium_execution::QuotaLimits;
    use image::{ImageBuffer, ImageFormat, Rgba};

    fn quota() -> QuotaGroup {
        QuotaGroup::new(QuotaLimits {
            clients: 0,
            jobs: 0,
            service_jobs: 0,
            input_bytes: 0,
            result_bytes: 0,
            worker_threads: 0,
            worker_bytes: 512 * 1024 * 1024,
        })
    }
    // Synthetic in-memory codec fixture, never provider/network/user data.
    fn decoded(media: &mut NativeMedia, color: [u8; 4]) -> ImageHandle {
        let _encoding = charge(&media.quota, 1024 * 1024).unwrap();
        let fixture = ImageBuffer::from_pixel(1, 1, Rgba(color));
        let mut encoded = Cursor::new(Vec::new());
        fixture.write_to(&mut encoded, ImageFormat::Png).unwrap();
        media
            .decode(encoded.get_ref(), &StopToken::default())
            .unwrap()
    }
    // Private malformed shape fixture. Its actual allocation is charged BEFORE
    // constructing the Vec; no public constructor may forge these values.
    fn shape(
        quota: &QuotaGroup,
        width: u32,
        height: u32,
        len: usize,
        capacity: usize,
    ) -> Arc<Admitted<ImagePixels>> {
        let admission = charge(quota, capacity + 256).unwrap();
        let mut rgba = Vec::with_capacity(capacity);
        rgba.resize(len, 0);
        Arc::new(Admitted {
            value: ImagePixels {
                width,
                height,
                rgba,
            },
            quota: quota.clone(),
            admission,
        })
    }
    #[test]
    fn decoded_original_pixel_debit_survives_import_and_last_escaped_alias() {
        let quota = quota();
        let mut producer = NativeMedia::new(quota.clone(), MediaLimits::default()).unwrap();
        let original = decoded(&mut producer, [11, 23, 37, 255]);
        let pixels = producer.snapshot(original).unwrap();
        assert!(pixels.shares_root(&quota));
        let original_pixel_debit = 4 + 256;
        drop(producer);
        assert_eq!(quota.snapshot().worker_bytes, original_pixel_debit);
        let mut target = NativeMedia::new(quota.clone(), MediaLimits::default()).unwrap();
        let before_import = quota.snapshot().worker_bytes;
        let handle = target.retain_admitted_image(Arc::clone(&pixels)).unwrap();
        assert_eq!(
            quota.snapshot().worker_bytes,
            before_import,
            "import does not replace or duplicate the original pixel debit"
        );
        let escaped = target.snapshot(handle).unwrap();
        assert!(Arc::ptr_eq(&pixels, &escaped));
        assert_eq!(escaped.view().rgba, [11, 23, 37, 255]);
        target.close(handle).unwrap();
        drop(target);
        assert_eq!(quota.snapshot().worker_bytes, original_pixel_debit);
        drop(pixels);
        assert_eq!(quota.snapshot().worker_bytes, original_pixel_debit);
        drop(escaped);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
    #[test]
    fn equal_looking_foreign_root_is_refused_before_registry_mutation() {
        let root = quota();
        let foreign = quota();
        assert_eq!(root.snapshot().limits, foreign.snapshot().limits);
        let mut producer = NativeMedia::new(root.clone(), MediaLimits::default()).unwrap();
        let original = decoded(&mut producer, [1, 2, 3, 255]);
        let pixels = producer.snapshot(original).unwrap();
        assert!(!pixels.shares_root(&foreign));
        let mut target = NativeMedia::new(foreign.clone(), MediaLimits::default()).unwrap();
        let before_root = root.snapshot().worker_bytes;
        let before_foreign = foreign.snapshot().worker_bytes;
        assert!(matches!(
            target.retain_admitted_image(Arc::clone(&pixels)),
            Err(AnimationError::PermissionDenied(_))
        ));
        assert!(target.images.is_empty());
        assert_eq!(target.next_handle, 1);
        assert_eq!(root.snapshot().worker_bytes, before_root);
        assert_eq!(foreign.snapshot().worker_bytes, before_foreign);
        assert_eq!(Arc::strong_count(&pixels), 2); // Actual producing registry + caller, no retained refused insertion.
    }
    #[test]
    fn importer_issues_new_target_local_handle_without_replacing_existing_image() {
        let quota = quota();
        let mut producer = NativeMedia::new(quota.clone(), MediaLimits::default()).unwrap();
        let source_handle = decoded(&mut producer, [0, 0, 255, 255]);
        let source = producer.snapshot(source_handle).unwrap();
        let mut target = NativeMedia::new(quota.clone(), MediaLimits::default()).unwrap();
        let first = decoded(&mut target, [255, 0, 0, 255]);
        let second = decoded(&mut target, [0, 255, 0, 255]);
        let original_target = target.snapshot(first).unwrap();
        // Existing raw IDs can coincide across registries; they are NOT guest
        // authority. Import allocates in target, never reuses the source ID.
        assert_eq!(source_handle.id(), first.id());
        let imported = target.retain_admitted_image(Arc::clone(&source)).unwrap();
        assert_ne!(imported, first);
        assert_ne!(imported, second);
        assert_eq!(imported.id(), 3);
        assert!(Arc::ptr_eq(&target.snapshot(imported).unwrap(), &source));
        assert!(Arc::ptr_eq(
            &target.snapshot(first).unwrap(),
            &original_target
        ));
        assert!(!Arc::ptr_eq(
            &target.snapshot(source_handle).unwrap(),
            &source
        ));
        producer.close(source_handle).unwrap();
        drop(producer);
        assert_eq!(
            target.snapshot(imported).unwrap().view().rgba,
            [0, 0, 255, 255]
        );
        target.close(imported).unwrap();
        assert_eq!(
            target.snapshot(first).unwrap().view().rgba,
            [255, 0, 0, 255]
        );
    }
    #[test]
    fn shape_dimensions_capacity_and_pixel_ceiling_refuse_without_consuming_handle() {
        let quota = quota();
        let limits = MediaLimits {
            max_dimension: 2,
            pixels: 2,
            ..MediaLimits::default()
        };
        let mut target = NativeMedia::new(quota.clone(), limits).unwrap();
        let baseline = quota.snapshot().worker_bytes;
        for (width, height, len, capacity) in [
            (0, 1, 4, 4),
            (1, 0, 4, 4),
            (3, 1, 4, 4),
            (2, 2, 4, 4),
            (1, 1, 3, 3),
            (1, 1, 5, 5),
            (1, 1, 4, 8),
            (u32::MAX, u32::MAX, 4, 4),
        ] {
            let pixels = shape(&quota, width, height, len, capacity);
            assert!(target.retain_admitted_image(pixels).is_err());
            assert!(target.images.is_empty());
            assert_eq!(target.next_handle, 1);
            assert_eq!(quota.snapshot().worker_bytes, baseline);
        }
    }
    #[test]
    fn handle_limit_and_counter_exhaustion_preserve_existing_registry_and_charge() {
        let quota = quota();
        let mut target = NativeMedia::new(
            quota.clone(),
            MediaLimits {
                handles: 1,
                ..MediaLimits::default()
            },
        )
        .unwrap();
        let pixels = shape(&quota, 1, 1, 4, 4);
        let handle = target.retain_admitted_image(Arc::clone(&pixels)).unwrap();
        let before = quota.snapshot().worker_bytes;
        assert!(target.retain_admitted_image(Arc::clone(&pixels)).is_err());
        assert_eq!(target.images.len(), 1);
        assert_eq!(target.next_handle, 2);
        assert_eq!(quota.snapshot().worker_bytes, before);
        assert!(Arc::ptr_eq(&target.snapshot(handle).unwrap(), &pixels));
        target.close(handle).unwrap();
        target.next_handle = u64::MAX;
        assert!(target.retain_admitted_image(Arc::clone(&pixels)).is_err());
        assert!(target.images.is_empty());
        assert_eq!(target.next_handle, u64::MAX);
    }
    #[cfg(feature = "native-network")]
    #[test]
    fn source_wrapper_borrows_genuine_admitted_pixels_without_id_rebinding() {
        let foreign = quota();
        let quota = quota();
        let mut producer = NativeMedia::new(quota.clone(), MediaLimits::default()).unwrap();
        let original = decoded(&mut producer, [19, 23, 29, 255]);
        let source = crate::sources::NativeSourceImage::from_native(&producer, original).unwrap();
        assert!(source.shares_root(&quota));
        assert!(!source.shares_root(&foreign));
        let mut target = NativeMedia::new(quota.clone(), MediaLimits::default()).unwrap();
        let imported = target
            .retain_admitted_image(Arc::clone(source.admitted_pixels()))
            .unwrap();
        assert!(Arc::ptr_eq(
            source.admitted_pixels(),
            &target.snapshot(imported).unwrap()
        ));
        drop(producer);
        drop(source);
        assert_eq!(
            target.snapshot(imported).unwrap().view().rgba,
            [19, 23, 29, 255]
        );
        target.close(imported).unwrap();
        drop(target);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
    #[test]
    fn generic_admitted_root_debug_and_into_parts_preserve_original_contract() {
        let foreign = quota();
        let quota = quota();
        let mut media = NativeMedia::new(quota.clone(), MediaLimits::default()).unwrap();
        let handle = decoded(&mut media, [7, 11, 19, 255]);
        let sampled = media
            .sample(
                handle,
                &[[0.5, 0.5]],
                Sampling::Nearest,
                &StopToken::default(),
            )
            .unwrap();
        assert!(sampled.shares_root(&quota));
        assert!(!sampled.shares_root(&foreign));
        assert!(format!("{sampled:?}").starts_with("Admitted"));
        let (values, original_guard): (Vec<[f32; 4]>, StorageAdmission) = sampled.into_parts();
        drop(media);
        assert_eq!(quota.snapshot().worker_bytes, 16 + 128);
        assert_eq!(values.len(), 1);
        drop(values);
        assert_eq!(quota.snapshot().worker_bytes, 16 + 128);
        drop(original_guard);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
}
