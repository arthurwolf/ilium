//! Bounded prepared-data drawing. All acquisition and evidence authentication
//! precede this adapter; script handle strings never mint source ownership.
#![cfg(feature = "native-host")]
use crate::{
    native_media::{
        Admitted, ImageHandle, ImagePixels, NativeMedia, TextSpan, TextStyle as MediaStyle,
    },
    native_worlds::NativeWorldFrame,
    surface::{
        Blend, ColourSpace, Command, Data, Format, Mode, NativeOutput, NativePatch, NativeRenderer,
        NativeSpan, NativeText, Rect, Shape, SourceToken, SurfaceError, VectorOp,
    },
};
use ilium_ambient::raster::Raster;
use ilium_execution::{QuotaGroup, StorageAdmission};
use ilium_platform::owned_worker::StopToken;
use std::{collections::BTreeMap, sync::Arc};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;
type Result<T> = std::result::Result<T, SurfaceError>;
const BRAILLE: [[u8; 2]; 4] = [[1, 8], [2, 16], [4, 32], [64, 128]];

/// Native activation stamp, intentionally neither Serialize nor Deserialize.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrawBinding {
    pub package_digest: String,
    pub instance_id: u64,
    pub plan_generation: u64,
    pub authorization_epoch: u64,
}
#[derive(Clone, Copy, Debug)]
pub struct DrawLimits {
    pub geometry_work: usize,
    pub text_bytes: usize,
}
impl Default for DrawLimits {
    fn default() -> Self {
        Self {
            geometry_work: 16 * 1024 * 1024,
            text_bytes: 16 * 1024,
        }
    }
}
/// The host checks its CURRENT channel/binding and supplies ONLY previously
/// prepared handles. Implementations must not acquire resources in these calls.
pub trait DrawAuthority {
    fn check_frame(&mut self, binding: &DrawBinding) -> Result<()>;
    fn prepared(&mut self, handle: &str, binding: &DrawBinding) -> Result<PreparedBlit>;
}
#[derive(Clone)]
pub struct PreparedBlit {
    binding: DrawBinding,
    pixels: PreparedPixels,
}
#[derive(Clone)]
enum PreparedPixels {
    Image {
        image: Arc<Admitted<ImagePixels>>,
        owner: Option<SourceToken>,
    },
    World {
        frame: Arc<NativeWorldFrame>,
        owners: Arc<WorldOwners>,
    },
}
struct WorldOwners {
    tokens: Vec<Option<SourceToken>>,
    palette_rgb: [u8; 3],
    _storage: StorageAdmission,
}
impl PreparedBlit {
    /// Host-only preparation AFTER authorizing the original native image and
    /// authenticating its evidence key. `None` draws without attribution.
    pub fn image(
        media: &NativeMedia,
        handle: ImageHandle,
        binding: DrawBinding,
        authenticated_evidence: Option<u64>,
    ) -> Result<Self> {
        let image = media
            .snapshot(handle)
            .map_err(|_| SurfaceError::Invalid("unknown prepared image"))?;
        let pixels = image.view();
        let size = (pixels.width as usize)
            .checked_mul(pixels.height as usize)
            .and_then(|size| size.checked_mul(4))
            .ok_or(SurfaceError::Capacity)?;
        if pixels.width == 0 || pixels.height == 0 || pixels.rgba.len() != size {
            return Err(SurfaceError::Invalid("prepared image dimensions"));
        }
        let owner = authenticated_evidence
            .map(SourceToken::from_native)
            .transpose()?;
        Ok(Self {
            binding,
            pixels: PreparedPixels::Image { image, owner },
        })
    }
    /// Preparation retains the actual world frame/receipt and maps its native
    /// owner IDs through the host's authenticated receipt evidence store. The
    /// mapping callback must verify that frame/source/receipt, not trust JSON.
    pub fn world(
        frame: Arc<NativeWorldFrame>,
        binding: DrawBinding,
        quota: &QuotaGroup,
        palette_rgb: [u8; 3],
        stop: &StopToken,
        mut authenticate: impl FnMut(&NativeWorldFrame, u32) -> Result<Option<u64>>,
    ) -> Result<Self> {
        if stop.is_stopped() {
            return Err(SurfaceError::Stale);
        }
        let raster = frame.raster();
        let count = raster
            .width
            .checked_mul(raster.height)
            .ok_or(SurfaceError::Capacity)?;
        if raster.width == 0
            || raster.height == 0
            || !raster.width.is_multiple_of(2)
            || !raster.height.is_multiple_of(4)
            || count > crate::surface::MAX_CELLS * 8
            || raster.dots.len() != count
            || raster.owner_ids.len() != count
            || frame.colors().len() != count / 8
            || raster
                .dots
                .iter()
                .any(|dot| !dot.is_finite() || !(0.0..=1.0).contains(dot))
        {
            return Err(SurfaceError::Invalid("prepared world dimensions"));
        }
        let storage = quota
            .reserve_external_storage(
                count
                    .checked_mul(8)
                    .and_then(|size| size.checked_add(8192 * 128 + 256))
                    .ok_or(SurfaceError::Capacity)?,
            )
            .map_err(|_| SurfaceError::Capacity)?;
        let mut tokens = Vec::with_capacity(count);
        let mut authenticated = BTreeMap::new();
        for (index, owner) in raster.owner_ids.iter().enumerate() {
            if index.is_multiple_of(64) && stop.is_stopped() {
                return Err(SurfaceError::Stale);
            }
            let token = if *owner == 0 {
                None
            } else if let Some(token) = authenticated.get(owner) {
                *token
            } else {
                if authenticated.len() >= 8192 {
                    return Err(SurfaceError::Capacity);
                }
                let token = authenticate(&frame, *owner)?
                    .map(SourceToken::from_native)
                    .transpose()?;
                authenticated.insert(*owner, token);
                token
            };
            tokens.push(token);
        }
        Ok(Self {
            binding,
            pixels: PreparedPixels::World {
                frame,
                owners: Arc::new(WorldOwners {
                    tokens,
                    palette_rgb,
                    _storage: storage,
                }),
            },
        })
    }
    fn dimensions(&self) -> (usize, usize) {
        match &self.pixels {
            PreparedPixels::Image { image, .. } => {
                (image.view().width as usize, image.view().height as usize)
            }
            PreparedPixels::World { frame, .. } => (frame.raster().width, frame.raster().height),
        }
    }
    fn sample(&self, x: usize, y: usize) -> Sample {
        match &self.pixels {
            PreparedPixels::Image { image, owner } => {
                let image = image.view();
                let offset = (y * image.width as usize + x) * 4;
                let rgb = [
                    image.rgba[offset] as f32 / 255.,
                    image.rgba[offset + 1] as f32 / 255.,
                    image.rgba[offset + 2] as f32 / 255.,
                ];
                let alpha = image.rgba[offset + 3] as f32 / 255.;
                Sample {
                    rgb,
                    alpha,
                    intensity: luma(rgb) * alpha,
                    owner: if alpha == 0. { None } else { *owner },
                }
            }
            PreparedPixels::World { frame, owners } => {
                let raster = frame.raster();
                let index = y * raster.width + x;
                let color = if frame.has_cell_colors() {
                    frame.colors()[y / 4 * (raster.width / 2) + x / 2]
                } else {
                    owners.palette_rgb
                };
                let intensity = raster.dots[index];
                Sample {
                    rgb: color.map(|value| value as f32 / 255.),
                    alpha: intensity,
                    intensity,
                    owner: if intensity == 0. {
                        None
                    } else {
                        owners.tokens[index]
                    },
                }
            }
        }
    }
}
#[derive(Clone, Copy)]
struct Sample {
    rgb: [f32; 3],
    alpha: f32,
    intensity: f32,
    owner: Option<SourceToken>,
}

struct VectorSpec<'a> {
    op: VectorOp,
    points: &'a [[f32; 2]],
    width: f32,
    fill: bool,
    closed: bool,
    value: &'a [f32],
    rgb: Option<[u8; 3]>,
}

/// A frame-scoped owner. Construct under the original root quota outside the
/// synchronous frame call; keep it alive until each returned output is consumed.
/// Recreate per frame to reset aggregate geometry/text limits. No I/O occurs.
/// Mask8 overwrite replaces touched cell masks; Max unions their bits. Pixel
/// formats have per-dot state. This follows Surface native patch semantics.
pub struct NativeDraw<'a, A: DrawAuthority> {
    shape: Shape,
    binding: DrawBinding,
    media: &'a NativeMedia,
    authority: &'a mut A,
    stop: &'a StopToken,
    work: usize,
    text_bytes: usize,
    _scratch: StorageAdmission,
}
impl<'a, A: DrawAuthority> NativeDraw<'a, A> {
    pub fn new(
        quota: &QuotaGroup,
        shape: Shape,
        limits: DrawLimits,
        binding: DrawBinding,
        media: &'a NativeMedia,
        authority: &'a mut A,
        stop: &'a StopToken,
    ) -> Result<Self> {
        if stop.is_stopped() {
            return Err(SurfaceError::Stale);
        }
        let layout = shape.layout()?;
        if limits.geometry_work == 0
            || limits.geometry_work > 64 * 1024 * 1024
            || limits.text_bytes == 0
            || limits.text_bytes > crate::surface::MAX_TEXT_BYTES
            || binding.instance_id == 0
            || binding.package_digest.len() != 64
            || !binding
                .package_digest
                .bytes()
                .all(|value| value.is_ascii_hexdigit())
        {
            return Err(SurfaceError::Invalid("native drawing limits or binding"));
        }
        authority.check_frame(&binding)?;
        let bytes = layout
            .dots
            .checked_mul(64)
            .and_then(|bytes| bytes.checked_add(limits.text_bytes.checked_mul(16)?))
            .and_then(|bytes| bytes.checked_add(65536))
            .ok_or(SurfaceError::Capacity)?;
        let scratch = quota
            .reserve_external_storage(bytes)
            .map_err(|_| SurfaceError::Capacity)?;
        Ok(Self {
            shape,
            binding,
            media,
            authority,
            stop,
            work: limits.geometry_work,
            text_bytes: limits.text_bytes,
            _scratch: scratch,
        })
    }
    fn check(&mut self) -> Result<()> {
        if self.stop.is_stopped() {
            return Err(SurfaceError::Stale);
        }
        self.authority.check_frame(&self.binding)
    }
    fn charge_work(&mut self, amount: usize) -> Result<()> {
        self.work = self
            .work
            .checked_sub(amount)
            .ok_or(SurfaceError::Capacity)?;
        Ok(())
    }
    fn text(
        &mut self,
        x: u32,
        y: u32,
        text: &str,
        style: &crate::surface::TextStyle,
    ) -> Result<NativeOutput> {
        if x >= self.shape.cell_width
            || y >= self.shape.cell_height
            || text.is_empty()
            || text.chars().any(char::is_control)
        {
            return Err(SurfaceError::Invalid("native text"));
        }
        self.text_bytes = self
            .text_bytes
            .checked_sub(text.len())
            .ok_or(SurfaceError::Capacity)?;
        let columns = self.shape.cell_width - x;
        let mut width = 0usize;
        let mut end = 0usize;
        for glyph in text.graphemes(true) {
            let glyph_width = UnicodeWidthStr::width(glyph);
            if glyph_width == 0 || glyph_width > 2 || glyph.len() > 256 {
                return Err(SurfaceError::Invalid("native grapheme width"));
            }
            if width + glyph_width > columns as usize {
                break;
            }
            width += glyph_width;
            end += glyph.len();
        }
        if end == 0 {
            return Ok(NativeOutput::default());
        }
        self.charge_work(width * 8)?;
        let layout = self
            .media
            .styled_cells(
                &[TextSpan {
                    text: &text[..end],
                    style: MediaStyle {
                        foreground: style.rgb,
                        background: style.background,
                        bold: style.bold,
                        italic: style.italic,
                        underline: style.underline,
                    },
                }],
                columns,
                self.stop,
            )
            .map_err(|_| SurfaceError::Invalid("native styled text layout"))?;
        let output = layout
            .view()
            .iter()
            .filter(|cell| !cell.continuation)
            .map(|cell| NativeText {
                x: x + cell.x,
                y,
                text: cell.glyph.clone(),
                width: cell.width,
                style: style.clone(),
            })
            .collect();
        Ok(NativeOutput {
            text: output,
            ..Default::default()
        })
    }
    fn text_spans(
        &mut self,
        x: u32,
        y: u32,
        spans: &[NativeSpan],
        max_cells: u32,
    ) -> Result<NativeOutput> {
        if x >= self.shape.cell_width
            || y >= self.shape.cell_height
            || max_cells == 0
            || max_cells > self.shape.cell_width - x
            || spans.is_empty()
            || spans.len() > 64
        {
            return Err(SurfaceError::Invalid("native styled text bounds"));
        }
        let bytes = spans
            .iter()
            .try_fold(0usize, |sum, span| sum.checked_add(span.text.len()))
            .ok_or(SurfaceError::Capacity)?;
        self.text_bytes = self
            .text_bytes
            .checked_sub(bytes)
            .ok_or(SurfaceError::Capacity)?;
        self.charge_work(max_cells as usize * 8)?;
        let borrowed: Vec<TextSpan<'_>> = spans
            .iter()
            .map(|span| TextSpan {
                text: &span.text,
                style: MediaStyle {
                    foreground: span.style.rgb,
                    background: span.style.background,
                    bold: span.style.bold,
                    italic: span.style.italic,
                    underline: span.style.underline,
                },
            })
            .collect();
        let layout = self
            .media
            .styled_cells(&borrowed, max_cells, self.stop)
            .map_err(|error| media_surface_error(error, self.stop))?;
        let text = layout
            .view()
            .iter()
            .filter(|cell| !cell.continuation)
            .map(|cell| NativeText {
                x: x + cell.x,
                y,
                text: cell.glyph.clone(),
                width: cell.width,
                style: crate::surface::TextStyle {
                    rgb: cell.style.foreground,
                    background: cell.style.background,
                    bold: cell.style.bold,
                    italic: cell.style.italic,
                    underline: cell.style.underline,
                },
            })
            .collect();
        Ok(NativeOutput {
            text,
            ..Default::default()
        })
    }
    fn raster_text(
        &mut self,
        origin: [u32; 2],
        text: &str,
        size_px: f32,
        intensity: f32,
        rgb: Option<[u8; 3]>,
        remaining: usize,
    ) -> Result<NativeOutput> {
        let [x, y] = origin;
        let canvas_width = self.shape.cell_width as usize * 2;
        let canvas_height = self.shape.cell_height as usize * 4;
        if x as usize >= canvas_width
            || y as usize >= canvas_height
            || text.is_empty()
            || !size_px.is_finite()
            || !(8.0..=128.0).contains(&size_px)
            || !intensity.is_finite()
            || !(0.0..=1.0).contains(&intensity)
        {
            return Err(SurfaceError::Invalid("native font raster bounds"));
        }
        if rgb.is_some()
            && !self.shape.cell_rgb
            && !matches!(self.shape.format, Format::Rgb8 | Format::Rgba8)
        {
            return Err(SurfaceError::Invalid("undeclared raster text colour"));
        }
        self.text_bytes = self
            .text_bytes
            .checked_sub(text.len())
            .ok_or(SurfaceError::Capacity)?;
        let (metric_width, metric_height) = self
            .media
            .measure_text(text, size_px, self.stop)
            .map_err(|error| media_surface_error(error, self.stop))?;
        if metric_width == 0 || metric_height == 0 {
            return Ok(NativeOutput::default());
        }
        let visible_width = (metric_width as usize).min(canvas_width - x as usize);
        let visible_height = (metric_height as usize).min(canvas_height - y as usize);
        let left = if self.shape.mode == Mode::Cells {
            x as usize / 2 * 2
        } else {
            x as usize
        };
        let top = if self.shape.mode == Mode::Cells {
            y as usize / 4 * 4
        } else {
            y as usize
        };
        let right = if self.shape.mode == Mode::Cells {
            (x as usize + visible_width).div_ceil(2) * 2
        } else {
            x as usize + visible_width
        };
        let bottom = if self.shape.mode == Mode::Cells {
            (y as usize + visible_height).div_ceil(4) * 4
        } else {
            y as usize + visible_height
        };
        let rect = dot_rect(self.shape, left, top, right - left, bottom - top);
        require_samples(rect, remaining)?;
        let dots = (right - left)
            .checked_mul(bottom - top)
            .ok_or(SurfaceError::Capacity)?;
        self.charge_work(dots.checked_mul(4).ok_or(SurfaceError::Capacity)?)?;
        let mask = self
            .media
            .raster_text(
                text,
                size_px,
                visible_width as u32,
                visible_height as u32,
                self.stop,
            )
            .map_err(|error| media_surface_error(error, self.stop))?;
        let mut raster = Raster::default();
        raster.resize(right - left, bottom - top);
        let font = mask.view();
        for row in 0..font.height as usize {
            for column in 0..font.width as usize {
                raster.dots[(row + y as usize - top) * raster.width + column + x as usize - left] =
                    f32::from(font.mask[row * font.width as usize + column]) / 255. * intensity;
            }
        }
        let colour = rgb.unwrap_or([255, 255, 255]);
        let values: Vec<f32> = match self.shape.format {
            Format::Mask8 => vec![255.],
            Format::Mono1 | Format::Mono8 => vec![1.],
            Format::Gray8 => vec![255.],
            Format::Gray32 => vec![1.],
            Format::Rgb8 => colour.iter().map(|byte| f32::from(*byte)).collect(),
            Format::Rgba8 => vec![
                f32::from(colour[0]),
                f32::from(colour[1]),
                f32::from(colour[2]),
                255.,
            ],
        };
        let patch = vector_patch(self.shape, rect, &raster, &values)?;
        let mut colours = Vec::new();
        if let Some(rgb) = rgb.filter(|_| self.shape.cell_rgb) {
            let mut touched = std::collections::BTreeSet::new();
            for (index, state) in patch.state.iter().enumerate() {
                if *state != 2 {
                    continue;
                }
                if self.shape.format == Format::Mask8
                    && matches!(&patch.data, Data::U8(data) if data[index] == 0)
                {
                    continue;
                }
                let sample_x = rect.x as usize + index % rect.width as usize;
                let sample_y = rect.y as usize + index / rect.width as usize;
                touched.insert(if self.shape.mode == Mode::Cells {
                    (sample_x as u32, sample_y as u32)
                } else {
                    ((sample_x / 2) as u32, (sample_y / 4) as u32)
                });
            }
            colours.extend(
                touched
                    .into_iter()
                    .map(|(cell_x, cell_y)| (cell_x, cell_y, Some(rgb))),
            );
        }
        self.check()?;
        Ok(NativeOutput {
            patches: vec![patch],
            colours,
            ..Default::default()
        })
    }
    fn vector(&mut self, vector: VectorSpec<'_>, remaining: usize) -> Result<NativeOutput> {
        let VectorSpec {
            op,
            points,
            width,
            fill,
            closed,
            value,
            rgb,
        } = vector;
        let count = match op {
            VectorOp::Line | VectorOp::Ellipse => points.len() == 2,
            VectorOp::Triangle => points.len() == 3,
            VectorOp::Path => (2..=1024).contains(&points.len()),
        };
        if !count
            || !width.is_finite()
            || !(0.0..=1024.0).contains(&width)
            || points
                .iter()
                .flatten()
                .any(|value| !value.is_finite() || value.abs() > 1_000_000.)
        {
            return Err(SurfaceError::Invalid("native vector geometry"));
        }
        validate_value(self.shape.format, value)?;
        if op == VectorOp::Ellipse && (points[1][0] < 0. || points[1][1] < 0.) {
            return Err(SurfaceError::Invalid("ellipse radii"));
        }
        let canvas = (
            self.shape.cell_width as usize * 2,
            self.shape.cell_height as usize * 4,
        );
        let radius = width / 2.;
        let (min, max) = bounds(op, points, radius + 0.6);
        let mut left = min[0].floor().clamp(0., canvas.0 as f32) as usize;
        let mut top = min[1].floor().clamp(0., canvas.1 as f32) as usize;
        let mut right = max[0].ceil().clamp(0., canvas.0 as f32) as usize;
        let mut bottom = max[1].ceil().clamp(0., canvas.1 as f32) as usize;
        if self.shape.mode == Mode::Cells {
            left = left / 2 * 2;
            top = top / 4 * 4;
            right = right.div_ceil(2) * 2;
            bottom = bottom.div_ceil(4) * 4;
        }
        if left >= right || top >= bottom || (!fill && width == 0.) {
            return Ok(NativeOutput::default());
        }
        let rect = dot_rect(self.shape, left, top, right - left, bottom - top);
        require_samples(rect, remaining)?;
        let dots = (right - left) * (bottom - top);
        let edges = if op == VectorOp::Ellipse {
            48
        } else {
            points.len() * if fill && width > 0. { 2 } else { 1 }
        };
        self.charge_work(dots.checked_mul(edges).ok_or(SurfaceError::Capacity)?)?;
        let mut raster = Raster::default();
        raster.resize(right - left, bottom - top);
        if op == VectorOp::Ellipse {
            ellipse(&mut raster, points[0], points[1], radius, fill, left, top);
        } else {
            if fill && matches!(op, VectorOp::Triangle | VectorOp::Path) {
                for y in 0..raster.height {
                    for x in 0..raster.width {
                        if polygon(
                            points,
                            [left as f32 + x as f32 + 0.5, top as f32 + y as f32 + 0.5],
                        ) {
                            raster.dots[y * raster.width + x] = 1.;
                        }
                    }
                }
            }
            if width > 0. {
                let raster_width = raster.width as f32;
                let raster_height = raster.height as f32;
                let convert = |point: [f32; 2]| {
                    (
                        (point[0] - left as f32) / raster_width,
                        (point[1] - top as f32) / raster_height,
                    )
                };
                for pair in points.windows(2) {
                    raster.line(convert(pair[0]), convert(pair[1]), radius, 1.);
                }
                if op == VectorOp::Triangle || (op == VectorOp::Path && closed) {
                    raster.line(
                        convert(points[points.len() - 1]),
                        convert(points[0]),
                        radius,
                        1.,
                    );
                }
            }
        }
        self.check()?;
        let patch = vector_patch(self.shape, rect, &raster, value)?;
        let mut colours = Vec::new();
        if let Some(rgb) = rgb {
            if !self.shape.cell_rgb {
                return Err(SurfaceError::Invalid(
                    "undeclared native vector cell colour",
                ));
            }
            // The raster's actual touched samples determine colour damage;
            // the clipped bounding rectangle must not tint empty neighbours.
            let mut cells = std::collections::BTreeSet::new();
            for (index, state) in patch.state.iter().enumerate() {
                if *state != 2 {
                    continue;
                }
                let x = rect.x + index as u32 % rect.width;
                let y = rect.y + index as u32 / rect.width;
                cells.insert(if self.shape.mode == Mode::Cells {
                    (x, y)
                } else {
                    (x / 2, y / 4)
                });
            }
            colours.extend(cells.into_iter().map(|(x, y)| (x, y, Some(rgb))));
        }
        Ok(NativeOutput {
            colours,
            patches: vec![patch],
            ..Default::default()
        })
    }
    fn blit(
        &mut self,
        handle: &str,
        source: Rect,
        target: Rect,
        remaining: usize,
    ) -> Result<NativeOutput> {
        if handle.is_empty()
            || handle.len() > 128
            || !handle
                .bytes()
                .all(|value| value.is_ascii_alphanumeric() || matches!(value, b'_' | b'-' | b'.'))
        {
            return Err(SurfaceError::Invalid("prepared handle"));
        }
        let prepared = self.authority.prepared(handle, &self.binding)?;
        if prepared.binding != self.binding {
            return Err(SurfaceError::Stale);
        }
        let dimensions = prepared.dimensions();
        check_rect(source, dimensions)?;
        check_rect(
            target,
            (self.shape.layout()?.width, self.shape.layout()?.height),
        )?;
        if source.width == 0 || source.height == 0 {
            return Err(SurfaceError::Invalid("empty blit source"));
        }
        if target.width == 0 || target.height == 0 {
            return Ok(NativeOutput::default());
        }
        require_samples(target, remaining)?;
        let dots_per_sample = if self.shape.mode == Mode::Cells { 8 } else { 1 };
        let count = target.width as usize * target.height as usize;
        self.charge_work(
            count
                .checked_mul(dots_per_sample)
                .and_then(|count| count.checked_mul(10))
                .ok_or(SurfaceError::Capacity)?,
        )?;
        let dot_width = target.width as usize * if self.shape.mode == Mode::Cells { 2 } else { 1 };
        let dot_height =
            target.height as usize * if self.shape.mode == Mode::Cells { 4 } else { 1 };
        let mut patch = patch(self.shape.format, target, dots_per_sample)?;
        let mut colours = Vec::new();
        let mut samples = Vec::with_capacity(dot_width * dot_height);
        for y in 0..dot_height {
            for x in 0..dot_width {
                let sx = source.x as usize + (2 * x + 1) * source.width as usize / (2 * dot_width);
                let sy =
                    source.y as usize + (2 * y + 1) * source.height as usize / (2 * dot_height);
                samples.push(prepared.sample(sx, sy));
            }
        }
        for y in 0..dot_height {
            for x in 0..dot_width {
                let sample = samples[y * dot_width + x];
                if self.shape.mode == Mode::Cells {
                    let cell = y / 4 * target.width as usize + x / 2;
                    if sample.intensity >= 0.5 {
                        let Data::U8(data) = &mut patch.data else {
                            return Err(SurfaceError::Invalid("mask data"));
                        };
                        data[cell] |= BRAILLE[y % 4][x % 2];
                    }
                    patch.state[cell] = 2;
                    patch.owners[y * dot_width + x] = sample.owner;
                } else {
                    let index = y * dot_width + x;
                    write_sample(
                        &mut patch.data,
                        self.shape.format,
                        self.shape.colour_space,
                        dot_width,
                        index,
                        sample,
                    );
                    patch.state[index] = if sample.alpha == 0. { 1 } else { 2 };
                    patch.owners[index] = sample.owner;
                }
            }
        }
        if self.shape.cell_rgb {
            // Native colour commands affect a complete cell; Surface drops old
            // ownership before applying the exact new per-dot source patch.
            let (sx, sy) = if self.shape.mode == Mode::Cells {
                (1, 1)
            } else {
                (2, 4)
            };
            let first_x = target.x as usize / sx;
            let first_y = target.y as usize / sy;
            let last_x = (target.x as usize + target.width as usize).div_ceil(sx);
            let last_y = (target.y as usize + target.height as usize).div_ceil(sy);
            for cy in first_y..last_y {
                for cx in first_x..last_x {
                    let mut total = [0.; 3];
                    let mut weight = 0.;
                    for dy in 0..4 {
                        for dx in 0..2 {
                            let global_x = cx * 2 + dx;
                            let global_y = cy * 4 + dy;
                            let origin_x = target.x as usize
                                * if self.shape.mode == Mode::Cells { 2 } else { 1 };
                            let origin_y = target.y as usize
                                * if self.shape.mode == Mode::Cells { 4 } else { 1 };
                            if global_x < origin_x
                                || global_y < origin_y
                                || global_x >= origin_x + dot_width
                                || global_y >= origin_y + dot_height
                            {
                                continue;
                            }
                            let sample =
                                samples[(global_y - origin_y) * dot_width + global_x - origin_x];
                            for (sum, color) in total.iter_mut().zip(sample.rgb) {
                                *sum += linear(color) * sample.alpha;
                            }
                            weight += sample.alpha;
                        }
                    }
                    colours.push((
                        cx as u32,
                        cy as u32,
                        (weight > 0.).then(|| {
                            total.map(|sum| {
                                byte(output_colour(sum / weight, self.shape.colour_space))
                            })
                        }),
                    ));
                }
            }
        }
        self.check()?;
        Ok(NativeOutput {
            patches: vec![patch],
            colours,
            text: Vec::new(),
        })
    }
}
impl<A: DrawAuthority> NativeRenderer for NativeDraw<'_, A> {
    fn render(
        &mut self,
        command: &Command,
        shape: Shape,
        remaining_samples: usize,
    ) -> Result<NativeOutput> {
        if shape != self.shape
            || command.order() == 0
            || command.order() > crate::surface::MAX_EDITS
        {
            return Err(SurfaceError::Stale);
        }
        self.check()?;
        let output = match command {
            Command::Text {
                x, y, text, style, ..
            } => self.text(*x, *y, text, style)?,
            Command::TextSpans {
                x,
                y,
                spans,
                max_cells,
                ..
            } => self.text_spans(*x, *y, spans, *max_cells)?,
            Command::RasterText {
                x,
                y,
                text,
                font,
                size_px,
                intensity,
                rgb,
                ..
            } => {
                if font != "CascadiaCode-Regular" {
                    return Err(SurfaceError::Invalid("unsupported bundled font"));
                }
                self.raster_text(
                    [*x, *y],
                    text,
                    *size_px,
                    *intensity,
                    *rgb,
                    remaining_samples,
                )?
            }
            Command::Vector {
                op,
                points,
                width,
                fill,
                closed,
                value,
                rgb,
                blend,
                ..
            } => {
                if *blend == Blend::Alpha && shape.format != Format::Rgba8 {
                    return Err(SurfaceError::Invalid("alpha output format"));
                }
                self.vector(
                    VectorSpec {
                        op: *op,
                        points,
                        width: *width,
                        fill: *fill,
                        closed: *closed,
                        value,
                        rgb: *rgb,
                    },
                    remaining_samples,
                )?
            }
            Command::Blit {
                handle,
                source,
                target,
                blend,
                ..
            } => {
                if *blend == Blend::Alpha && shape.format != Format::Rgba8 {
                    return Err(SurfaceError::Invalid("alpha output format"));
                }
                self.blit(handle, *source, *target, remaining_samples)?
            }
        };
        self.check()?;
        Ok(output)
    }
}
fn media_surface_error(error: crate::error::AnimationError, stop: &StopToken) -> SurfaceError {
    if stop.is_stopped() {
        SurfaceError::Stale
    } else if matches!(error, crate::error::AnimationError::Budget(_)) {
        SurfaceError::Capacity
    } else if matches!(&error, crate::error::AnimationError::Runtime(message)
        if message == "native media: bundled font lacks requested glyph" || message == "native media: unsupported_glyph")
    {
        SurfaceError::Invalid("unsupported bundled font glyph")
    } else {
        SurfaceError::Invalid("native media text rejected")
    }
}
fn channels(format: Format) -> usize {
    match format {
        Format::Rgb8 => 3,
        Format::Rgba8 => 4,
        _ => 1,
    }
}
fn validate_value(format: Format, values: &[f32]) -> Result<()> {
    let maximum = if matches!(format, Format::Mono1 | Format::Mono8 | Format::Gray32) {
        1.
    } else {
        255.
    };
    if values.len() != channels(format)
        || values.iter().any(|value| {
            !value.is_finite()
                || !(0.0..=maximum).contains(value)
                || (format != Format::Gray32 && value.fract() != 0.)
        })
    {
        return Err(SurfaceError::Invalid("native vector value"));
    }
    Ok(())
}
fn require_samples(rect: Rect, remaining: usize) -> Result<()> {
    if (rect.width as usize)
        .checked_mul(rect.height as usize)
        .is_none_or(|count| count > remaining)
    {
        Err(SurfaceError::Capacity)
    } else {
        Ok(())
    }
}
fn check_rect(rect: Rect, dimensions: (usize, usize)) -> Result<()> {
    if rect.x as usize > dimensions.0
        || rect.y as usize > dimensions.1
        || rect.width as usize > dimensions.0 - rect.x as usize
        || rect.height as usize > dimensions.1 - rect.y as usize
    {
        Err(SurfaceError::Invalid("native blit bounds"))
    } else {
        Ok(())
    }
}
fn dot_rect(shape: Shape, left: usize, top: usize, width: usize, height: usize) -> Rect {
    let (sx, sy) = if shape.mode == Mode::Cells {
        (2, 4)
    } else {
        (1, 1)
    };
    Rect {
        x: (left / sx) as u32,
        y: (top / sy) as u32,
        width: (width / sx) as u32,
        height: (height / sy) as u32,
    }
}
fn bounds(op: VectorOp, points: &[[f32; 2]], padding: f32) -> ([f32; 2], [f32; 2]) {
    if op == VectorOp::Ellipse {
        return (
            [
                points[0][0] - points[1][0] - padding,
                points[0][1] - points[1][1] - padding,
            ],
            [
                points[0][0] + points[1][0] + padding,
                points[0][1] + points[1][1] + padding,
            ],
        );
    }
    let mut min = points[0];
    let mut max = points[0];
    for point in points {
        for ((minimum, maximum), coordinate) in min.iter_mut().zip(&mut max).zip(point) {
            *minimum = minimum.min(*coordinate);
            *maximum = maximum.max(*coordinate);
        }
    }
    (
        [min[0] - padding, min[1] - padding],
        [max[0] + padding, max[1] + padding],
    )
}
fn polygon(points: &[[f32; 2]], sample: [f32; 2]) -> bool {
    let mut inside = false;
    let mut previous = points[points.len() - 1];
    for next in points {
        if (next[1] > sample[1]) != (previous[1] > sample[1])
            && sample[0]
                < (previous[0] - next[0]) * (sample[1] - next[1]) / (previous[1] - next[1])
                    + next[0]
        {
            inside = !inside;
        }
        previous = *next;
    }
    inside
}
fn ellipse(
    raster: &mut Raster,
    center: [f32; 2],
    radii: [f32; 2],
    radius: f32,
    fill: bool,
    left: usize,
    top: usize,
) {
    for y in 0..raster.height {
        for x in 0..raster.width {
            let dx = (left as f32 + x as f32 + 0.5 - center[0]).abs();
            let dy = (top as f32 + y as f32 + 0.5 - center[1]).abs();
            let coverage = if radii[0] == 0. || radii[1] == 0. {
                if fill {
                    0.
                } else {
                    (radius + 0.6
                        - ((dx - radii[0]).max(0.).powi(2) + (dy - radii[1]).max(0.).powi(2))
                            .sqrt())
                    .clamp(0., 1.)
                }
            } else {
                let normalized = ((dx / radii[0]).powi(2) + (dy / radii[1]).powi(2)).sqrt();
                if fill && normalized <= 1. {
                    1.
                } else if radius > 0. {
                    (radius + 0.6 - ellipse_distance(dx, dy, radii)).clamp(0., 1.)
                } else {
                    0.
                }
            };
            raster.dots[y * raster.width + x] = coverage;
        }
    }
}
fn patch(format: Format, rect: Rect, dots_per_sample: usize) -> Result<NativePatch> {
    let count = (rect.width as usize)
        .checked_mul(rect.height as usize)
        .ok_or(SurfaceError::Capacity)?;
    let elements = if format == Format::Mono1 {
        (rect.width as usize).div_ceil(8) * rect.height as usize
    } else {
        count
            .checked_mul(channels(format))
            .ok_or(SurfaceError::Capacity)?
    };
    Ok(NativePatch {
        rect,
        data: if format == Format::Gray32 {
            Data::F32(vec![0.; elements])
        } else {
            Data::U8(vec![0; elements])
        },
        state: vec![0; count],
        owners: vec![None; count * dots_per_sample],
    })
}
fn vector_patch(shape: Shape, rect: Rect, raster: &Raster, value: &[f32]) -> Result<NativePatch> {
    let mut output = patch(
        shape.format,
        rect,
        if shape.mode == Mode::Cells { 8 } else { 1 },
    )?;
    for y in 0..raster.height {
        for x in 0..raster.width {
            let coverage = raster.dots[y * raster.width + x];
            if coverage == 0. {
                continue;
            }
            if shape.mode == Mode::Cells {
                let cell = y / 4 * rect.width as usize + x / 2;
                let Data::U8(data) = &mut output.data else {
                    return Err(SurfaceError::Invalid("native mask data"));
                };
                if coverage >= 0.5 {
                    data[cell] |= value[0] as u8 & BRAILLE[y % 4][x % 2];
                }
                output.state[cell] = 2;
            } else {
                let index = y * raster.width + x;
                match &mut output.data {
                    Data::F32(data) => data[index] = value[0] * coverage,
                    Data::U8(data) if shape.format == Format::Mono1 => {
                        if value[0] * coverage >= 0.5 {
                            data[y * raster.width.div_ceil(8) + x / 8] |= 1 << (7 - x % 8);
                        }
                    }
                    Data::U8(data) => {
                        for (channel, component) in value.iter().copied().enumerate() {
                            let scaled = if shape.format == Format::Rgb8 {
                                let normalized = component / 255.;
                                let intensity = if shape.colour_space == ColourSpace::Srgb {
                                    linear(normalized)
                                } else {
                                    normalized
                                };
                                output_colour(intensity * coverage, shape.colour_space) * 255.
                            } else if shape.format == Format::Rgba8 && channel < 3 {
                                component
                            } else {
                                component * coverage
                            };
                            data[index * channels(shape.format) + channel] =
                                if shape.format == Format::Mono8 {
                                    u8::from(scaled >= 0.5)
                                } else {
                                    scaled.round() as u8
                                };
                        }
                    }
                }
                output.state[index] = 2;
            }
        }
    }
    Ok(output)
}
// IEC sRGB transfer, matching the canonical surface's linear blend/packing.
fn linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}
fn output_colour(value: f32, space: ColourSpace) -> f32 {
    if space == ColourSpace::Linear {
        value
    } else if value <= 0.0031308 {
        value * 12.92
    } else {
        1.055 * value.powf(1. / 2.4) - 0.055
    }
}
fn luma(rgb: [f32; 3]) -> f32 {
    rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722
}
fn byte(value: f32) -> u8 {
    (value.clamp(0., 1.) * 255.).round() as u8
}
fn write_sample(
    data: &mut Data,
    format: Format,
    colour_space: ColourSpace,
    width: usize,
    index: usize,
    sample: Sample,
) {
    match data {
        Data::F32(values) => values[index] = sample.intensity,
        Data::U8(values) if format == Format::Mono1 => {
            if sample.intensity >= 0.5 {
                values[index / width * width.div_ceil(8) + index % width / 8] |=
                    1 << (7 - index % width % 8);
            }
        }
        Data::U8(values) if format == Format::Mono8 => {
            values[index] = u8::from(sample.intensity >= 0.5)
        }
        Data::U8(values) if format == Format::Gray8 => values[index] = byte(sample.intensity),
        Data::U8(values) => {
            for (channel, value) in sample.rgb.into_iter().enumerate() {
                values[index * channels(format) + channel] = byte(output_colour(
                    linear(value)
                        * if format == Format::Rgba8 {
                            1.
                        } else {
                            sample.alpha
                        },
                    colour_space,
                ));
            }
            if format == Format::Rgba8 {
                values[index * 4 + 3] = byte(sample.alpha);
            }
        }
    }
}

// Closest distance to an axis-aligned ellipse, rather than scaling an implicit
// equation by its smallest radius (which thickens eccentric ellipse strokes).
// Forty-eight bisections are included in the declared per-sample work charge.
fn ellipse_distance(dx: f32, dy: f32, radii: [f32; 2]) -> f32 {
    let (major, minor, x, y) = if radii[0] >= radii[1] {
        (radii[0] as f64, radii[1] as f64, dx as f64, dy as f64)
    } else {
        (radii[1] as f64, radii[0] as f64, dy as f64, dx as f64)
    };
    if major == minor {
        return (x.hypot(y) - major).abs() as f32;
    }
    if y == 0. {
        let denominator = major * major - minor * minor;
        let fraction = major * x / denominator;
        if fraction < 1. {
            return (major * fraction - x).hypot(minor * (1. - fraction * fraction).sqrt()) as f32;
        }
        return (x - major).abs() as f32;
    }
    if x == 0. {
        return (y - minor).abs() as f32;
    }
    let mut low = -minor * minor;
    let mut high = (major * x).hypot(minor * y);
    for _ in 0..48 {
        let middle = (low + high) / 2.;
        let a = major * x / (middle + major * major);
        let b = minor * y / (middle + minor * minor);
        if a * a + b * b > 1. {
            low = middle;
        } else {
            high = middle;
        }
    }
    let parameter = (low + high) / 2.;
    let closest_x = major * major * x / (parameter + major * major);
    let closest_y = minor * minor * y / (parameter + minor * minor);
    (closest_x - x).hypot(closest_y - y) as f32
}
