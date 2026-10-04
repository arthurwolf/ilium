//! CPU batch kernels behind the existing compute.submit/poll/cancel family.
//! No GPU claims, code evaluation, I/O, process RNG, hidden bank, or source-credit
//! generation. The root broker authenticates calls and binary ingress/delivery.
use crate::error::{AnimationError, Result};
use ilium_ambient::{
    resources::AmbientResources,
    voxel_landscape::{
        noise,
        render::{Canvas, Vertex},
    },
    AudioFft,
};
use ilium_execution::{
    Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, QuotaGroup, Receipt, StorageAdmission,
};
use ilium_platform::owned_worker::StopToken;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};
const MAX_INPUT: usize = 262_144;
const MAX_OUTPUT: usize = 1_048_576;
const MAX_JOBS: usize = 32;
const ALGORITHM: &str = "ilium-native-math-v1";
fn invalid(message: &str) -> AnimationError {
    AnimationError::Runtime(format!("native math: {message}"))
}
fn charge(quota: &QuotaGroup, bytes: usize) -> Result<StorageAdmission> {
    quota
        .reserve_external_storage(bytes.max(1))
        .map_err(|error| AnimationError::Budget(format!("native math admission: {error:?}")))
}
fn allocate(count: usize) -> Result<Vec<f32>> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| AnimationError::Budget("native math allocation".into()))?;
    values.resize(count, 0.);
    Ok(values)
}
fn finite(values: &[f64]) -> Result<()> {
    if values
        .iter()
        .any(|value| !value.is_finite() || value.abs() > 1e12)
    {
        return Err(invalid("nonfinite or excessive coefficient"));
    }
    Ok(())
}
fn output_number(value: f64) -> Result<f32> {
    if !value.is_finite() || value.abs() > f64::from(f32::MAX) {
        return Err(invalid("unrepresentable result"));
    }
    Ok(value as f32)
}
/// Caller supplies an already bounded borrowed native view. Charge precedes copy;
/// Arc sharing covers this original allocation, not arbitrary uncharged clones.
pub struct MathInput {
    values: Vec<f32>,
    digest: [u8; 32],
    quota: QuotaGroup,
    _admission: StorageAdmission,
}
impl MathInput {
    pub fn from_host(values: &[f32], quota: QuotaGroup) -> Result<Arc<Self>> {
        if values.is_empty()
            || values.len() > MAX_INPUT
            || values
                .iter()
                .any(|value| !value.is_finite() || value.abs() > 1e12)
        {
            return Err(invalid("input count/range"));
        }
        let admission = charge(&quota, values.len() * 4 + 256)?;
        let mut copy = allocate(values.len())?;
        copy.copy_from_slice(values);
        let mut hash = Sha256::new();
        for value in values {
            hash.update(value.to_le_bytes());
        }
        Ok(Arc::new(Self {
            values: copy,
            digest: hash.finalize().into(),
            quota,
            _admission: admission,
        }))
    }
    pub fn values(&self) -> &[f32] {
        &self.values
    }
    pub fn digest(&self) -> [u8; 32] {
        self.digest
    }
}
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kernel {
    Noise,
    Transform,
    Fft,
    Mesh,
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kernel", rename_all = "snake_case")]
pub enum KernelParameters {
    /// Interleaved integer x/z coordinates; exact native normalized fBm in [-1,1].
    Noise { seed: u64, period: u32, octaves: u8 },
    /// operation 0: row-major mat4 applied to xyz/xyzw. 1: normalize vectors;
    /// 2: cross paired xyz; 3: dot paired vectors; 4: multiply paired mat4;
    /// 5: invert mat4. Projection divides xyz by the resulting homogeneous w.
    Transform {
        operation: u8,
        components: u8,
        matrix: [f64; 16],
        project: bool,
    },
    /// Forward unnormalized or inverse 1/N. Output interleaves real/imag.
    /// window 0 none,1 periodic Hann,2 periodic Blackman; complex input optional.
    Fft {
        complex: bool,
        inverse: bool,
        window: u8,
    },
    /// Triples of screen-space xyz vertices (nine floats per triangle). Native
    /// camera-depth renderer; output interleaves finite depth/coverage. No source
    /// owner IDs, texture handle, world proof or presentation credit is returned.
    Mesh { width: u32, height: u32 },
}
impl KernelParameters {
    fn kernel(&self) -> Kernel {
        match self {
            Self::Noise { .. } => Kernel::Noise,
            Self::Transform { .. } => Kernel::Transform,
            Self::Fft { .. } => Kernel::Fft,
            Self::Mesh { .. } => Kernel::Mesh,
        }
    }
}
/// Exact numeric wire contract for Compute.submit.parameters. Unknown fields and
/// unsupported parameter variants fail closed; no generic native entry point.
pub fn parameters_from_wire(
    kernel: &str,
    parameters: &BTreeMap<String, f64>,
) -> Result<KernelParameters> {
    if parameters.len() > 24 {
        return Err(invalid("parameter count"));
    }
    let allowed = |key: &str| match kernel {
        "noise" => matches!(key, "seed" | "period" | "octaves"),
        "fft" => matches!(key, "complex" | "inverse" | "window"),
        "mesh" => matches!(key, "width" | "height"),
        "transform" => {
            matches!(key, "operation" | "components" | "project") || {
                let bytes = key.as_bytes();
                bytes.len() == 3
                    && bytes[0] == b'm'
                    && (b'0'..=b'3').contains(&bytes[1])
                    && (b'0'..=b'3').contains(&bytes[2])
            }
        }
        _ => false,
    };
    if !matches!(kernel, "noise" | "fft" | "mesh" | "transform") {
        return Err(invalid("unknown kernel"));
    }
    if parameters
        .iter()
        .any(|(key, value)| !allowed(key) || !value.is_finite())
    {
        return Err(invalid("unknown/nonfinite parameter"));
    }
    let integer = |name: &str, default: u64, maximum: u64| -> Result<u64> {
        let value = parameters.get(name).copied().unwrap_or(default as f64);
        if value < 0. || value > maximum as f64 || value.fract() != 0. {
            return Err(invalid("integer parameter"));
        }
        Ok(value as u64)
    };
    let boolean = |name: &str| integer(name, 0, 1).map(|value| value == 1);
    match kernel {
        "noise" => Ok(KernelParameters::Noise {
            seed: integer("seed", 0, u32::MAX as u64)?,
            period: integer("period", 64, u32::MAX as u64)? as u32,
            octaves: integer("octaves", 4, 8)? as u8,
        }),
        "fft" => Ok(KernelParameters::Fft {
            complex: boolean("complex")?,
            inverse: boolean("inverse")?,
            window: integer("window", 0, 2)? as u8,
        }),
        "mesh" => Ok(KernelParameters::Mesh {
            width: integer("width", 0, 1024)? as u32,
            height: integer("height", 0, 1024)? as u32,
        }),
        "transform" => {
            let mut matrix = identity4();
            for (key, value) in parameters {
                let bytes = key.as_bytes();
                if bytes.len() == 3 && bytes[0] == b'm' {
                    matrix[usize::from(bytes[1] - b'0') * 4 + usize::from(bytes[2] - b'0')] =
                        *value;
                }
            }
            finite(&matrix)?;
            Ok(KernelParameters::Transform {
                operation: integer("operation", 0, 5)? as u8,
                components: integer("components", 3, 4)? as u8,
                matrix,
                project: boolean("project")?,
            })
        }
        _ => Err(invalid("unknown kernel")),
    }
}
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OutputLayout {
    Scalars,
    Complex,
    Vectors { components: u8 },
    Matrices,
    Mesh { width: u32, height: u32 },
}
/// Immutable mathematical content provenance. This digest is not authority to
/// observe another principal or a saved-world terminal emission receipt.
pub struct MathOutput {
    values: Vec<f32>,
    layout: OutputLayout,
    kernel: Kernel,
    request_digest: [u8; 32],
    _admission: StorageAdmission,
}
impl MathOutput {
    pub fn values(&self) -> &[f32] {
        &self.values
    }
    pub fn layout(&self) -> OutputLayout {
        self.layout
    }
    pub fn kernel(&self) -> Kernel {
        self.kernel
    }
    pub fn request_digest(&self) -> [u8; 32] {
        self.request_digest
    }
    pub fn algorithm(&self) -> &'static str {
        ALGORITHM
    }
}
pub struct MathRequest {
    input: Arc<MathInput>,
    parameters: KernelParameters,
    max_bytes: usize,
    deadline: Instant,
}
impl MathRequest {
    pub fn new(
        input: Arc<MathInput>,
        parameters: KernelParameters,
        max_bytes: usize,
        timeout_ms: u64,
    ) -> Result<Self> {
        if max_bytes == 0 || max_bytes > MAX_OUTPUT * 4 || !(1..=60_000).contains(&timeout_ms) {
            return Err(invalid("request budget/deadline"));
        }
        let request = Self {
            input,
            parameters,
            max_bytes,
            deadline: Instant::now() + Duration::from_millis(timeout_ms),
        };
        request.layout()?;
        Ok(request)
    }
    pub(crate) fn cap_deadline(mut self, original: Instant) -> Result<Self> {
        self.deadline = self.deadline.min(original);
        if Instant::now() >= self.deadline {
            return Err(invalid("original host deadline exhausted"));
        }
        Ok(self)
    }
    fn layout(&self) -> Result<(usize, OutputLayout, usize)> {
        let count = self.input.values.len();
        let (elements, layout, scratch) = match &self.parameters {
            KernelParameters::Noise {
                period, octaves, ..
            } => {
                if !count.is_multiple_of(2)
                    || *period == 0
                    || *octaves > 8
                    || self
                        .input
                        .values
                        .iter()
                        .any(|value| value.fract() != 0. || value.abs() > 16_777_216.)
                {
                    return Err(invalid("integer noise coordinates/parameters"));
                }
                (count / 2, OutputLayout::Scalars, 0)
            }
            KernelParameters::Transform {
                operation,
                components,
                matrix,
                project,
            } => {
                finite(matrix)?;
                let components = usize::from(*components);
                if !(2..=4).contains(&components)
                    || *operation > 5
                    || (*project && (*operation != 0 || components < 3))
                {
                    return Err(invalid("transform variant"));
                }
                let stride = match operation {
                    0 | 1 => components,
                    2 => 6,
                    3 => components * 2,
                    4 => 32,
                    _ => 16,
                };
                if !count.is_multiple_of(stride)
                    || (*operation == 2 && components != 3)
                    || (*operation >= 4 && components != 4)
                {
                    return Err(invalid("transform input stride"));
                }
                let result = match operation {
                    2 => count / 2,
                    3 => count / stride,
                    4 => count / 2,
                    _ => count,
                };
                let layout = match operation {
                    3 => OutputLayout::Scalars,
                    4 | 5 => OutputLayout::Matrices,
                    _ => OutputLayout::Vectors {
                        components: components as u8,
                    },
                };
                (result, layout, 1024)
            }
            KernelParameters::Fft {
                complex,
                inverse,
                window,
            } => {
                let size = if *complex {
                    if !count.is_multiple_of(2) {
                        return Err(invalid("complex input stride"));
                    }
                    count / 2
                } else {
                    count
                };
                if !(2..=8192).contains(&size)
                    || !size.is_power_of_two()
                    || *window > 2
                    || (*inverse && (!*complex || *window != 0))
                {
                    return Err(invalid("FFT shape/window/inverse"));
                }
                (size * 2, OutputLayout::Complex, size * 40 + 1024)
            }
            KernelParameters::Mesh { width, height } => {
                if *width == 0
                    || *height == 0
                    || *width > 1024
                    || *height > 1024
                    || !count.is_multiple_of(9)
                    || count / 9 > 2048
                    || self.input.values.iter().any(|value| value.abs() > 1e6)
                {
                    return Err(invalid("mesh shape/vertices"));
                }
                let pixels = (*width as usize)
                    .checked_mul(*height as usize)
                    .ok_or_else(|| invalid("mesh pixel overflow"))?;
                // Native Canvas allocations and worst-case raster bounding-box work
                // are admitted before its constructor, not after rasterization.
                let mut work = 0usize;
                for triangle in self.input.values.chunks_exact(9) {
                    let (left, right, top, bottom) = triangle_bounds(triangle, *width, *height);
                    work = work
                        .checked_add((right - left) * (bottom - top))
                        .ok_or_else(|| invalid("mesh work overflow"))?;
                    if work > 16 * 1024 * 1024 {
                        return Err(AnimationError::Budget("native mesh work".into()));
                    }
                }
                (
                    pixels * 2,
                    OutputLayout::Mesh {
                        width: *width,
                        height: *height,
                    },
                    pixels * 64 + 1024,
                )
            }
        };
        if elements > MAX_OUTPUT
            || elements
                .checked_mul(4)
                .is_none_or(|bytes| bytes > self.max_bytes)
        {
            return Err(AnimationError::Budget("native math output bound".into()));
        }
        Ok((elements, layout, scratch))
    }
}
struct Deadline {
    stop: StopToken,
    at: Instant,
}
impl Deadline {
    fn check(&self) -> Result<()> {
        if self.stop.is_stopped() {
            return Err(invalid("cancelled"));
        }
        if Instant::now() >= self.at {
            return Err(invalid("deadline exceeded"));
        }
        Ok(())
    }
}
/// Call on the existing admitted CPU bank. Loops have fixed maximum sizes and
/// cooperative checkpoints; native FFT/one bounded triangle are non-preemptive.
pub fn execute(
    request: MathRequest,
    quota: &QuotaGroup,
    stop: &StopToken,
) -> Result<Arc<MathOutput>> {
    if !request.input.quota.shares_root(quota) {
        return Err(invalid("foreign quota input"));
    }
    let deadline = Deadline {
        stop: stop.clone(),
        at: request.deadline,
    };
    deadline.check()?;
    let (count, layout, scratch) = request.layout()?;
    let _scratch = charge(quota, scratch + 4096)?;
    let admission = charge(quota, count * 4 + 512)?;
    let mut values = allocate(count)?;
    let input = request.input.values();
    match &request.parameters {
        KernelParameters::Noise {
            seed,
            period,
            octaves,
        } => {
            for (index, pair) in input.chunks_exact(2).enumerate() {
                if index % 256 == 0 {
                    deadline.check()?;
                }
                values[index] =
                    noise::fbm2(*seed, pair[0] as i64, pair[1] as i64, *period, *octaves) as f32;
            }
        }
        KernelParameters::Transform {
            operation,
            components,
            matrix,
            project,
        } => {
            let components = usize::from(*components);
            match operation {
                0 => {
                    for (index, vector) in input.chunks_exact(components).enumerate() {
                        if index % 256 == 0 {
                            deadline.check()?;
                        }
                        let mut source = [0., 0., 0., 1.];
                        for (target, value) in source.iter_mut().zip(vector) {
                            *target = f64::from(*value);
                        }
                        let mut transformed = transform4(matrix, &source)?;
                        if *project {
                            if transformed[3].abs() < 1e-12 {
                                return Err(invalid("homogeneous division by zero"));
                            }
                            for axis in 0..3 {
                                transformed[axis] /= transformed[3];
                            }
                        }
                        for axis in 0..components {
                            values[index * components + axis] = output_number(transformed[axis])?;
                        }
                    }
                }
                1 => {
                    for (index, vector) in input.chunks_exact(components).enumerate() {
                        if index % 256 == 0 {
                            deadline.check()?;
                        }
                        let magnitude = vector
                            .iter()
                            .map(|value| f64::from(*value).powi(2))
                            .sum::<f64>()
                            .sqrt();
                        if magnitude == 0. {
                            return Err(invalid("normalize zero vector"));
                        }
                        for axis in 0..components {
                            values[index * components + axis] =
                                output_number(f64::from(vector[axis]) / magnitude)?;
                        }
                    }
                }
                2 => {
                    for (index, pair) in input.chunks_exact(6).enumerate() {
                        if index % 256 == 0 {
                            deadline.check()?;
                        }
                        let a = [pair[0] as f64, pair[1] as f64, pair[2] as f64];
                        let b = [pair[3] as f64, pair[4] as f64, pair[5] as f64];
                        for (axis, value) in cross3(a, b)?.iter().enumerate() {
                            values[index * 3 + axis] = output_number(*value)?;
                        }
                    }
                }
                3 => {
                    for (index, pair) in input.chunks_exact(components * 2).enumerate() {
                        if index % 256 == 0 {
                            deadline.check()?;
                        }
                        values[index] = output_number(
                            pair[..components]
                                .iter()
                                .zip(&pair[components..])
                                .map(|(a, b)| f64::from(*a) * f64::from(*b))
                                .sum(),
                        )?;
                    }
                }
                4 | 5 => {
                    let stride = if *operation == 4 { 32 } else { 16 };
                    for (index, source) in input.chunks_exact(stride).enumerate() {
                        if index % 64 == 0 {
                            deadline.check()?;
                        }
                        let a = std::array::from_fn(|axis| f64::from(source[axis]));
                        let result = if *operation == 4 {
                            multiply4(
                                &a,
                                &std::array::from_fn(|axis| f64::from(source[16 + axis])),
                            )?
                        } else {
                            inverse4(&a)?
                        };
                        for (axis, value) in result.iter().enumerate() {
                            values[index * 16 + axis] = output_number(*value)?;
                        }
                    }
                }
                _ => return Err(invalid("unknown transform")),
            }
        }
        KernelParameters::Fft {
            complex,
            inverse,
            window,
        } => {
            let size = count / 2;
            let fft = AudioFft::new(size).map_err(|message| invalid(&message))?;
            let mut real = allocate(size)?;
            let mut imag = allocate(size)?;
            for index in 0..size {
                let angle = std::f64::consts::TAU * index as f64 / size as f64;
                let weight = match window {
                    1 => 0.5 - 0.5 * angle.cos(),
                    2 => 0.42 - 0.5 * angle.cos() + 0.08 * (2. * angle).cos(),
                    _ => 1.,
                };
                real[index] =
                    output_number(f64::from(input[index * if *complex { 2 } else { 1 }]) * weight)?;
                if *complex {
                    imag[index] = output_number(
                        f64::from(input[index * 2 + 1]) * weight * if *inverse { -1. } else { 1. },
                    )?;
                }
            }
            deadline.check()?;
            fft.forward(&mut real, &mut imag);
            deadline.check()?;
            for index in 0..size {
                values[index * 2] = output_number(
                    f64::from(real[index]) / if *inverse { size as f64 } else { 1. },
                )?;
                values[index * 2 + 1] = output_number(
                    f64::from(imag[index]) * if *inverse { -1. } else { 1. }
                        / if *inverse { size as f64 } else { 1. },
                )?;
            }
        }
        KernelParameters::Mesh { width, height } => {
            let mut canvas = Canvas::new(*width as usize, *height as usize);
            for (index, triangle) in input.chunks_exact(9).enumerate() {
                deadline.check()?;
                let vertices = std::array::from_fn::<_, 3, _>(|axis| {
                    Vertex::new(
                        triangle[axis * 3],
                        triangle[axis * 3 + 1],
                        triangle[axis * 3 + 2],
                        0.,
                        0.,
                    )
                });
                // Native face uses two triangles; a degenerate second triangle
                // leaves the supplied three-vertex geometry unchanged.
                canvas.face_owned(
                    [vertices[0], vertices[1], vertices[2], vertices[2]],
                    index as u64,
                    |_, _| [255; 3],
                );
            }
            for (index, depth) in canvas.depth.iter().enumerate() {
                if depth.is_finite() {
                    values[index * 2] = *depth;
                    values[index * 2 + 1] = 1.;
                }
            }
        }
    }
    deadline.check()?;
    if values.iter().any(|value| !value.is_finite()) {
        return Err(invalid("nonfinite output"));
    }
    let mut hash = Sha256::new();
    hash.update(ALGORITHM.as_bytes());
    hash.update(request.input.digest);
    hash.update(serde_json::to_vec(&request.parameters)?);
    Ok(Arc::new(MathOutput {
        values,
        layout,
        kernel: request.parameters.kernel(),
        request_digest: hash.finalize().into(),
        _admission: admission,
    }))
}
fn triangle_bounds(triangle: &[f32], width: u32, height: u32) -> (usize, usize, usize, usize) {
    let axis = |offset: usize, limit: u32| {
        let low = (0..3)
            .map(|vertex| triangle[vertex * 3 + offset])
            .fold(f32::INFINITY, f32::min)
            .floor()
            .clamp(0., limit as f32) as usize;
        let high = (0..3)
            .map(|vertex| triangle[vertex * 3 + offset])
            .fold(f32::NEG_INFINITY, f32::max)
            .ceil()
            .clamp(0., limit as f32) as usize;
        (low, high)
    };
    let (left, right) = axis(0, width);
    let (top, bottom) = axis(1, height);
    (left, right, top, bottom)
}
pub fn identity4() -> [f64; 16] {
    std::array::from_fn(|index| if index / 4 == index % 4 { 1. } else { 0. })
}
pub fn transform4(matrix: &[f64; 16], vector: &[f64; 4]) -> Result<[f64; 4]> {
    finite(matrix)?;
    finite(vector)?;
    let result =
        std::array::from_fn(|row| (0..4).map(|col| matrix[row * 4 + col] * vector[col]).sum());
    finite(&result)?;
    Ok(result)
}
pub fn multiply4(a: &[f64; 16], b: &[f64; 16]) -> Result<[f64; 16]> {
    finite(a)?;
    finite(b)?;
    let result = std::array::from_fn(|index| {
        (0..4)
            .map(|axis| a[index / 4 * 4 + axis] * b[axis * 4 + index % 4])
            .sum()
    });
    finite(&result)?;
    Ok(result)
}
pub fn inverse4(matrix: &[f64; 16]) -> Result<[f64; 16]> {
    finite(matrix)?;
    let mut rows = [[0.; 8]; 4];
    for row in 0..4 {
        for col in 0..4 {
            rows[row][col] = matrix[row * 4 + col];
            rows[row][col + 4] = if row == col { 1. } else { 0. };
        }
    }
    for pivot in 0..4 {
        let selected = (pivot..4)
            .max_by(|a, b| rows[*a][pivot].abs().total_cmp(&rows[*b][pivot].abs()))
            .ok_or_else(|| invalid("matrix pivot"))?;
        if rows[selected][pivot].abs() < 1e-12 {
            return Err(invalid("singular/ill-conditioned matrix"));
        }
        rows.swap(pivot, selected);
        let divisor = rows[pivot][pivot];
        for value in &mut rows[pivot] {
            *value /= divisor;
        }
        let pivot_values = rows[pivot];
        for (row_index, row_values) in rows.iter_mut().enumerate() {
            if row_index != pivot {
                let factor = row_values[pivot];
                for (value, pivot_value) in row_values.iter_mut().zip(pivot_values) {
                    *value -= factor * pivot_value;
                }
            }
        }
    }
    let result = std::array::from_fn(|index| rows[index / 4][index % 4 + 4]);
    finite(&result)?;
    Ok(result)
}
pub fn cross3(a: [f64; 3], b: [f64; 3]) -> Result<[f64; 3]> {
    finite(&a)?;
    finite(&b)?;
    let result = [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ];
    finite(&result)?;
    Ok(result)
}
/// Versioned reproducible randomness; not a cryptographic random source.
pub struct SeededRandom {
    seed: u64,
    state: u64,
}
impl SeededRandom {
    pub fn new(seed: u64) -> Self {
        Self { seed, state: seed }
    }
    pub fn reset(&mut self) {
        self.state = self.seed;
    }
    fn word(&mut self) -> u64 {
        let value = noise::mix64(self.state);
        self.state = self.state.wrapping_add(1);
        value
    }
    pub fn next_unit(&mut self) -> f64 {
        (self.word() >> 11) as f64 / 9_007_199_254_740_992.
    }
    pub fn integer(&mut self, minimum: i64, maximum: i64) -> Result<i64> {
        if minimum > maximum
            || minimum.unsigned_abs() > 9_007_199_254_740_991
            || maximum.unsigned_abs() > 9_007_199_254_740_991
        {
            return Err(invalid("random integer bounds"));
        }
        let range = (i128::from(maximum) - i128::from(minimum) + 1) as u64;
        let threshold = range.wrapping_neg() % range;
        for _ in 0..64 {
            let word = self.word();
            if word >= threshold {
                return Ok((i128::from(minimum) + i128::from(word % range)) as i64);
            }
        }
        Err(invalid("random rejection budget"))
    }
}
#[derive(Debug, PartialEq)]
pub struct StepBatch {
    pub steps: u32,
    pub step_seconds: f64,
    pub pending_seconds: f64,
}
/// Keeps elapsed simulation debt rather than silently truncating frame delta.
/// Excessive backlog is explicit refusal and leaves scheduler state unchanged.
pub struct FixedSteps {
    step: f64,
    pending: f64,
    max_pending: f64,
    max_steps: u32,
}
impl FixedSteps {
    pub fn new(step: f64, max_pending: f64, max_steps: u32) -> Result<Self> {
        if !step.is_finite()
            || !max_pending.is_finite()
            || !(0.0001..=1.).contains(&step)
            || max_pending < step
            || max_pending > 60.
            || !(1..=1024).contains(&max_steps)
        {
            return Err(invalid("fixed-step limits"));
        }
        Ok(Self {
            step,
            pending: 0.,
            max_pending,
            max_steps,
        })
    }
    pub fn advance(&mut self, delta: f64) -> Result<StepBatch> {
        let total = self.pending + delta;
        if !delta.is_finite() || delta < 0. || !total.is_finite() || total > self.max_pending {
            return Err(invalid("simulation backlog"));
        }
        // Compensate only floating-point division roundoff near an integer;
        // no meaningful elapsed simulation interval is invented or discarded.
        let ratio = total / self.step;
        let complete = (ratio + 8. * f64::EPSILON * ratio.abs().max(1.)).floor();
        let steps = (complete as u32).min(self.max_steps);
        let pending = (total - f64::from(steps) * self.step).max(0.);
        self.pending = pending;
        Ok(StepBatch {
            steps,
            step_seconds: self.step,
            pending_seconds: pending,
        })
    }
    pub fn reset(&mut self) {
        self.pending = 0.;
    }
}
/// Opaque original-native handle. Root RPC correlation IDs must resolve within
/// this service's authenticated instance; no public from_id/deserialization.
#[derive(Clone)]
pub struct MathHandle {
    issuer: Arc<()>,
    id: u64,
}
impl MathHandle {
    pub fn id(&self) -> u64 {
        self.id
    }
}
struct ComputeJob {
    request: MathRequest,
    quota: QuotaGroup,
}
impl Job for ComputeJob {
    type Output = Arc<MathOutput>;
    type Error = AnimationError;
    fn run(self, context: JobContext) -> Result<Self::Output> {
        execute(self.request, &self.quota, &context.stop_token())
    }
}
struct Slot {
    receipt: Receipt<ComputeJob>,
    cancelled: bool,
}
pub enum ComputeStatus {
    Pending,
    Cancelling,
    Ready(Arc<MathOutput>),
    Cancelled,
    Failed(String),
}
pub struct NativeMath {
    resources: AmbientResources,
    quota: QuotaGroup,
    issuer: Arc<()>,
    epoch: u64,
    next_id: u64,
    slots: BTreeMap<u64, Slot>,
    closed: bool,
    settlement_unconfirmed: bool,
    _metadata: StorageAdmission,
}
impl NativeMath {
    pub fn new(resources: AmbientResources, quota: QuotaGroup, epoch: u64) -> Result<Self> {
        if epoch == 0 || !quota.shares_root(&resources.finite().quota_group()) {
            return Err(invalid("math host identity/resources"));
        }
        let metadata = charge(&quota, MAX_JOBS * 2048)?;
        Ok(Self {
            resources,
            quota,
            issuer: Arc::new(()),
            epoch,
            next_id: 1,
            slots: BTreeMap::new(),
            closed: false,
            settlement_unconfirmed: false,
            _metadata: metadata,
        })
    }
    pub fn submit(&mut self, request: MathRequest, current_epoch: u64) -> Result<MathHandle> {
        if current_epoch != self.epoch {
            self.close();
        }
        if self.closed || !request.input.quota.shares_root(&self.quota) {
            return Err(invalid("stale/foreign math instance"));
        }
        if self.slots.len() >= MAX_JOBS {
            return Err(AnimationError::Budget("math handle capacity".into()));
        }
        let (count, _, scratch) = request.layout()?;
        let cost = JobCost {
            input_bytes: request.input.values.len() * 4 + scratch + 4096,
            result_bytes: count * 4 + 4096,
        };
        let next = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| invalid("math handle identity exhausted"))?;
        // Reserve the shared existing bank BEFORE capturing the actual job.
        let reservation = self
            .resources
            .finite()
            .try_reserve(Lane::Cpu, cost)
            .map_err(|reason| AnimationError::Budget(format!("math CPU admission: {reason:?}")))?;
        let receipt = reservation
            .submit(ComputeJob {
                request,
                quota: self.quota.clone(),
            })
            .map_err(|rejected| {
                AnimationError::Budget(format!("math CPU submission: {:?}", rejected.reason))
            })?;
        let handle = MathHandle {
            issuer: Arc::clone(&self.issuer),
            id: self.next_id,
        };
        self.next_id = next;
        self.slots.insert(
            handle.id,
            Slot {
                receipt,
                cancelled: false,
            },
        );
        Ok(handle)
    }
    fn check_handle(&self, handle: &MathHandle) -> Result<()> {
        if !Arc::ptr_eq(&self.issuer, &handle.issuer) || !self.slots.contains_key(&handle.id) {
            return Err(invalid("foreign/stale math handle"));
        }
        Ok(())
    }
    pub fn cancel(&mut self, handle: &MathHandle) -> Result<()> {
        self.check_handle(handle)?;
        let slot = self
            .slots
            .get_mut(&handle.id)
            .ok_or_else(|| invalid("unknown math handle"))?;
        slot.cancelled = true;
        slot.receipt.cancel();
        Ok(())
    }
    /// Event-driven owner calls this after an existing completion wake. No wait,
    /// thread creation or promise resolution under an authorization lock.
    pub fn poll(&mut self, handle: &MathHandle, current_epoch: u64) -> Result<ComputeStatus> {
        self.check_handle(handle)?;
        if current_epoch != self.epoch {
            self.close();
        }
        let stale = self.closed;
        let slot = self
            .slots
            .get_mut(&handle.id)
            .ok_or_else(|| invalid("unknown math handle"))?;
        if stale {
            slot.cancelled = true;
            slot.receipt.cancel();
        }
        match slot.receipt.try_take() {
            JobPoll::Pending => Ok(if slot.cancelled {
                ComputeStatus::Cancelling
            } else {
                ComputeStatus::Pending
            }),
            JobPoll::Ready(retained) => {
                let cancelled = slot.cancelled;
                self.slots.remove(&handle.id);
                if cancelled {
                    drop(retained);
                    return Ok(ComputeStatus::Cancelled);
                }
                let result = match retained.view() {
                    JobOutcome::Finished(Ok(output)) => ComputeStatus::Ready(Arc::clone(output)),
                    JobOutcome::Finished(Err(error)) => ComputeStatus::Failed(error.to_string()),
                    JobOutcome::NotStarted { .. } => ComputeStatus::Cancelled,
                    JobOutcome::Panicked => ComputeStatus::Failed("math worker panicked".into()),
                };
                Ok(result)
            }
            JobPoll::Lost | JobPoll::Taken => {
                self.settlement_unconfirmed = true;
                self.slots.remove(&handle.id);
                Ok(ComputeStatus::Failed("math completion lost".into()))
            }
        }
    }
    /// Closed admission and genuine terminal receipt observations are required.
    /// A lost receipt remains unknown even after its public handle is terminal.
    pub fn is_drained(&self) -> bool {
        self.closed && self.slots.is_empty() && !self.settlement_unconfirmed
    }

    pub fn close(&mut self) {
        self.closed = true;
        for slot in self.slots.values_mut() {
            slot.cancelled = true;
            slot.receipt.cancel();
        }
    }
}
impl Drop for NativeMath {
    fn drop(&mut self) {
        self.close();
    }
}
