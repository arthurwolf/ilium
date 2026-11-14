//! Budgeted, frame-local linear-light mip levels for generated diffuse sampling.
use super::{
    animation::{AnimationPlan, PixelRect},
    budget::{ByteBudget, Cancel, Reservation},
    error::{AssetError, Result},
    pixels::PixelImage,
    texture::srgb_byte_to_linear,
};

#[derive(Debug)]
struct Level {
    size: [u32; 2],
    pixels: Vec<[f32; 4]>,
    _reservation: Reservation,
}

#[derive(Debug)]
struct Frame {
    rect: PixelRect,
    levels: Vec<Level>,
    _reservation: Reservation,
}

#[derive(Debug)]
pub(crate) struct Mipmaps {
    frames: Vec<Frame>,
    _reservation: Reservation,
}

fn rect_key(rect: PixelRect) -> [u32; 4] {
    [rect.x, rect.y, rect.width, rect.height]
}

fn area_sample(
    source_size: [u32; 2],
    target_size: [u32; 2],
    target: [u32; 2],
    source: &impl Fn(u32, u32) -> Option<[f32; 4]>,
) -> Option<[f32; 4]> {
    let lower = std::array::from_fn::<_, 2, _>(|axis| {
        f64::from(target[axis]) * f64::from(source_size[axis]) / f64::from(target_size[axis])
    });
    let upper = std::array::from_fn::<_, 2, _>(|axis| {
        f64::from(target[axis] + 1) * f64::from(source_size[axis]) / f64::from(target_size[axis])
    });
    let area = (upper[0] - lower[0]) * (upper[1] - lower[1]);
    let mut sum = [0.0_f64; 4];
    for y in lower[1].floor() as u32..upper[1].ceil() as u32 {
        for x in lower[0].floor() as u32..upper[0].ceil() as u32 {
            let weight_x = (upper[0].min(f64::from(x + 1)) - lower[0].max(f64::from(x))).max(0.0);
            let weight_y = (upper[1].min(f64::from(y + 1)) - lower[1].max(f64::from(y))).max(0.0);
            let pixel = source(x.min(source_size[0] - 1), y.min(source_size[1] - 1))?;
            for channel in 0..4 {
                sum[channel] += f64::from(pixel[channel]) * weight_x * weight_y;
            }
        }
    }
    Some(sum.map(|value| (value / area) as f32))
}

impl Level {
    fn build(
        image: &PixelImage,
        rect: PixelRect,
        previous: Option<&Self>,
        budget: &ByteBudget,
        cancel: Cancel<'_>,
    ) -> Result<Self> {
        let source_size = previous.map_or([rect.width, rect.height], |level| level.size);
        let size = source_size.map(|extent| extent.div_ceil(2));
        let count = usize::try_from(u64::from(size[0]) * u64::from(size[1]))
            .map_err(|_| AssetError::Allocation)?;
        let bytes = u64::try_from(count)
            .ok()
            .and_then(|count| count.checked_mul(std::mem::size_of::<[f32; 4]>() as u64))
            .ok_or(AssetError::Allocation)?;
        let reservation = budget.reserve(bytes, cancel)?;
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(count)
            .map_err(|_| AssetError::Allocation)?;
        let source = |x: u32, y: u32| {
            if let Some(level) = previous {
                return level
                    .pixels
                    .get(y as usize * level.size[0] as usize + x as usize)
                    .copied();
            }
            let pixel = image.pixel(rect.x + x, rect.y + y)?;
            let alpha = f32::from(pixel[3]) / 255.0;
            Some([
                srgb_byte_to_linear(pixel[0]) * alpha,
                srgb_byte_to_linear(pixel[1]) * alpha,
                srgb_byte_to_linear(pixel[2]) * alpha,
                alpha,
            ])
        };
        for y in 0..size[1] {
            cancel.check()?;
            for x in 0..size[0] {
                pixels.push(
                    area_sample(source_size, size, [x, y], &source).ok_or_else(|| {
                        AssetError::InvalidImage("missing diffuse mip texel".into())
                    })?,
                );
            }
        }
        Ok(Self {
            size,
            pixels,
            _reservation: reservation,
        })
    }

    fn sample(&self, uv: [f32; 2], clamp: bool) -> [f32; 4] {
        let coordinate = std::array::from_fn::<_, 2, _>(|axis| {
            let value = f64::from(uv[axis]);
            let normalized = if clamp {
                value.clamp(0.0, 1.0)
            } else {
                value.rem_euclid(1.0)
            };
            normalized * f64::from(self.size[axis]) - 0.5
        });
        let address = |value: i64, axis: usize| {
            let extent = i64::from(self.size[axis]);
            if clamp {
                value.clamp(0, extent - 1) as usize
            } else {
                value.rem_euclid(extent) as usize
            }
        };
        let at = |x, y| self.pixels[address(y, 1) * self.size[0] as usize + address(x, 0)];
        let x = coordinate[0].floor() as i64;
        let y = coordinate[1].floor() as i64;
        let tx = (coordinate[0] - coordinate[0].floor()) as f32;
        let ty = (coordinate[1] - coordinate[1].floor()) as f32;
        let a = at(x, y);
        let b = at(x + 1, y);
        let c = at(x, y + 1);
        let d = at(x + 1, y + 1);
        std::array::from_fn(|channel| {
            let upper = a[channel] + (b[channel] - a[channel]) * tx;
            let lower = c[channel] + (d[channel] - c[channel]) * tx;
            upper + (lower - upper) * ty
        })
    }
}

impl Mipmaps {
    pub(crate) fn build(
        image: &PixelImage,
        animation: &AnimationPlan,
        budget: &ByteBudget,
        cancel: Cancel<'_>,
    ) -> Result<Self> {
        let frame_capacity = animation.frame_count();
        let frame_bytes = u64::try_from(frame_capacity)
            .ok()
            .and_then(|count| count.checked_mul(std::mem::size_of::<Frame>() as u64))
            .ok_or(AssetError::Allocation)?;
        let reservation = budget.reserve(frame_bytes, cancel)?;
        let mut frames: Vec<Frame> = Vec::new();
        frames
            .try_reserve_exact(frame_capacity)
            .map_err(|_| AssetError::Allocation)?;
        for rect in animation.frame_rects() {
            cancel.check()?;
            if frames.iter().any(|frame| frame.rect == rect) {
                continue;
            }
            let max_extent = rect.width.max(rect.height);
            let level_count = (u32::BITS - (max_extent - 1).leading_zeros()) as usize;
            let level_bytes = u64::try_from(level_count)
                .ok()
                .and_then(|count| count.checked_mul(std::mem::size_of::<Level>() as u64))
                .ok_or(AssetError::Allocation)?;
            let frame_reservation = budget.reserve(level_bytes, cancel)?;
            let mut levels: Vec<Level> = Vec::new();
            levels
                .try_reserve_exact(level_count)
                .map_err(|_| AssetError::Allocation)?;
            for _ in 0..level_count {
                levels.push(Level::build(image, rect, levels.last(), budget, cancel)?);
            }
            frames.push(Frame {
                rect,
                levels,
                _reservation: frame_reservation,
            });
        }
        frames.sort_unstable_by_key(|frame| rect_key(frame.rect));
        Ok(Self {
            frames,
            _reservation: reservation,
        })
    }

    pub(crate) fn sample(
        &self,
        rect: PixelRect,
        uv: [f32; 2],
        derivatives: [[f32; 2]; 2],
        clamp: bool,
        base: [f32; 4],
    ) -> Option<[f32; 4]> {
        let index = self
            .frames
            .binary_search_by_key(&rect_key(rect), |frame| rect_key(frame.rect))
            .ok()?;
        let frame = &self.frames[index];
        let extent = [f64::from(rect.width), f64::from(rect.height)];
        let footprint = derivatives
            .map(|derivative| {
                (f64::from(derivative[0]) * extent[0]).hypot(f64::from(derivative[1]) * extent[1])
            })
            .into_iter()
            .fold(1.0_f64, f64::max);
        let lod = footprint.log2().clamp(0.0, frame.levels.len() as f64);
        let lower = lod.floor() as usize;
        let upper = lod.ceil() as usize;
        let sample_level = |level: usize| {
            if level == 0 {
                base
            } else {
                frame.levels[level - 1].sample(uv, clamp)
            }
        };
        let a = sample_level(lower);
        let b = sample_level(upper);
        let blend = (lod - lower as f64) as f32;
        Some(std::array::from_fn(|channel| {
            a[channel] + (b[channel] - a[channel]) * blend
        }))
    }
}
