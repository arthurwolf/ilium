//! Fixed-subpixel top-left triangle coverage and bounded order-independent alpha layers.
//! An overflow invalidates the candidate frame; it never silently discards a visible layer.
use super::{
    assets::{
        bank::TextureBank,
        budget::{ByteBudget, Cancel, Reservation},
        error::{AssetError, Result},
        material_data::{FresnelSource, LabNormal, LabSpecular},
        metadata,
        texture::{LinearRgba, Texture},
    },
    surface_fluid::FluidMesh,
    surface_mesh::{AlphaMode, BoundQuad, FaceOwner, PreparedMesh},
};
use std::time::Duration;
#[derive(Clone, Copy, Debug)]
pub struct RasterLimits {
    pub pixels: usize,
    pub layers: u8,
    pub triangles: usize,
    pub sample_tests: u64,
}
impl Default for RasterLimits {
    fn default() -> Self {
        Self {
            pixels: 1_048_576,
            layers: 8,
            triangles: 2_000_000,
            sample_tests: 64_000_000,
        }
    }
}
impl RasterLimits {
    fn validate(self) -> Result<()> {
        if self.pixels > 1_048_576
            || self.pixels == 0
            || !(1..=8).contains(&self.layers)
            || self.triangles == 0
            || self.triangles > 2_000_000
            || self.sample_tests == 0
            || self.sample_tests > 64_000_000
        {
            return Err(metadata::invalid("invalid raster limits"));
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Vertex {
    pub x: f64,
    pub y: f64,
    pub depth: f64,
    pub uv: [f32; 2],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct FragmentRank {
    depth: i64,
    layer: u16,
    owner: FaceOwner,
}
#[derive(Clone, Copy, Debug, PartialEq)]
struct Fragment {
    rank: FragmentRank,
    color: LinearRgba,
}
#[derive(Clone, Copy, Debug)]
pub struct PixelResult {
    pub color: LinearRgba,
    pub contributors: [Option<FaceOwner>; 9],
    pub front_owner: Option<FaceOwner>,
}
pub trait FragmentShader {
    fn sample(&self, uv: [f32; 2]) -> Result<LinearRgba>;
}
pub struct FlatShader(pub LinearRgba);
impl FragmentShader for FlatShader {
    fn sample(&self, _uv: [f32; 2]) -> Result<LinearRgba> {
        Ok(self.0)
    }
}
#[derive(Debug)]
pub struct RasterFrame {
    width: usize,
    height: usize,
    limits: RasterLimits,
    opaque: Vec<Option<Fragment>>,
    blends: Vec<Option<Fragment>>,
    counts: Vec<u8>,
    submitted_quads: usize,
    projected_attempts: usize,
    triangles: usize,
    sample_tests: u64,
    valid: bool,
    budget: ByteBudget,
    _reservation: Reservation,
}
impl RasterFrame {
    pub fn new(
        size: [usize; 2],
        limits: RasterLimits,
        budget: &ByteBudget,
        cancel: Cancel<'_>,
    ) -> Result<Self> {
        cancel.check()?;
        limits.validate()?;
        let pixels = size[0].checked_mul(size[1]).ok_or(AssetError::Allocation)?;
        if pixels > limits.pixels || size.iter().any(|side| *side > 65536) {
            return Err(AssetError::Limit {
                resource: "raster pixels",
                requested: pixels as u64,
                limit: limits.pixels as u64,
            });
        }
        let blend_count = pixels
            .checked_mul(usize::from(limits.layers))
            .ok_or(AssetError::Allocation)?;
        let charge = (pixels + blend_count) as u64 * std::mem::size_of::<Option<Fragment>>() as u64
            + pixels as u64
            + 4096;
        let reservation = budget.reserve(charge, cancel)?;
        fn empty<T: Clone>(len: usize, value: T) -> Result<Vec<T>> {
            let mut values = Vec::new();
            values
                .try_reserve_exact(len)
                .map_err(|_| AssetError::Allocation)?;
            values.resize(len, value);
            Ok(values)
        }
        Ok(Self {
            width: size[0],
            height: size[1],
            limits,
            opaque: empty(pixels, None)?,
            blends: empty(blend_count, None)?,
            counts: empty(pixels, 0)?,
            submitted_quads: 0,
            projected_attempts: 0,
            triangles: 0,
            sample_tests: 0,
            valid: true,
            budget: budget.clone(),
            _reservation: reservation,
        })
    }
    pub fn size(&self) -> [usize; 2] {
        [self.width, self.height]
    }
    pub fn uses_budget(&self, budget: &ByteBudget) -> bool {
        self._reservation.belongs_to(budget)
    }
    pub fn is_valid(&self) -> bool {
        self.valid
    }
    pub fn sample_tests(&self) -> u64 {
        self.sample_tests
    }
    pub fn projected_attempts(&self) -> usize {
        self.projected_attempts
    }
    pub fn submitted_quads(&self) -> usize {
        self.submitted_quads
    }
    pub fn visible_triangles(&self) -> usize {
        self.triangles
    }
    pub fn clear(&mut self) {
        self.opaque.fill(None);
        self.blends.fill(None);
        self.counts.fill(0);
        self.submitted_quads = 0;
        self.projected_attempts = 0;
        self.triangles = 0;
        self.sample_tests = 0;
        self.valid = true;
    }
    pub fn quad(
        &mut self,
        vertices: [Vertex; 4],
        owner: FaceOwner,
        mode: AlphaMode,
        shader: &impl FragmentShader,
        cancel: Cancel<'_>,
    ) -> Result<()> {
        if !self.valid {
            return Err(metadata::invalid(
                "candidate raster is invalid; clear before reuse",
            ));
        }
        let result = self.quad_inner(vertices, owner, mode, shader, cancel);
        if result.is_err() {
            self.valid = false;
        }
        result
    }
    fn quad_inner(
        &mut self,
        vertices: [Vertex; 4],
        owner: FaceOwner,
        mode: AlphaMode,
        shader: &impl FragmentShader,
        cancel: Cancel<'_>,
    ) -> Result<()> {
        cancel.check()?;
        // Every submitted face is finite work, even if its projected extent is
        // wholly outside this frame. Culling before the two raster triangles
        // prevents distant source-height sweeps from exhausting the smaller
        // on-screen projection budget without losing any covered sample.
        const MAX_SUBMITTED_QUADS: usize = 16_000_000;
        self.submitted_quads = self
            .submitted_quads
            .checked_add(1)
            .ok_or(AssetError::Allocation)?;
        if self.submitted_quads > MAX_SUBMITTED_QUADS {
            return Err(AssetError::Limit {
                resource: "raster submitted quads",
                requested: self.submitted_quads as u64,
                limit: MAX_SUBMITTED_QUADS as u64,
            });
        }
        if vertices.iter().any(|v| {
            !v.x.is_finite()
                || !v.y.is_finite()
                || !v.depth.is_finite()
                || v.x.abs() > 1e9
                || v.y.abs() > 1e9
                || v.depth.abs() > 1e10
                || v.uv.iter().any(|c| !c.is_finite())
        }) {
            return Err(metadata::invalid("nonfinite/out-of-budget raster vertex"));
        }
        let width = self.width as f64;
        let height = self.height as f64;
        if vertices.iter().all(|v| v.x <= 0.0)
            || vertices.iter().all(|v| v.x >= width)
            || vertices.iter().all(|v| v.y <= 0.0)
            || vertices.iter().all(|v| v.y >= height)
        {
            return Ok(());
        }
        self.triangle(
            [vertices[0], vertices[1], vertices[2]],
            owner,
            mode,
            shader,
            cancel,
        )?;
        self.triangle(
            [vertices[0], vertices[2], vertices[3]],
            owner,
            mode,
            shader,
            cancel,
        )
    }
    pub fn triangle(
        &mut self,
        vertices: [Vertex; 3],
        owner: FaceOwner,
        mode: AlphaMode,
        shader: &impl FragmentShader,
        cancel: Cancel<'_>,
    ) -> Result<()> {
        if !self.valid {
            return Err(metadata::invalid(
                "candidate raster is invalid; clear before reuse",
            ));
        }
        let result = self.triangle_inner(vertices, owner, mode, shader, cancel);
        if result.is_err() {
            self.valid = false;
        }
        result
    }
    fn triangle_inner(
        &mut self,
        mut vertices: [Vertex; 3],
        owner: FaceOwner,
        mode: AlphaMode,
        shader: &impl FragmentShader,
        cancel: Cancel<'_>,
    ) -> Result<()> {
        cancel.check()?;
        // Off-screen faces still consume bounded projection work, but cannot
        // consume the on-screen triangle/sample admission budget.
        self.projected_attempts += 1;
        const MAX_PROJECTED_ATTEMPTS: usize = 16_000_000;
        if self.projected_attempts > MAX_PROJECTED_ATTEMPTS {
            return Err(AssetError::Limit {
                resource: "raster projected attempts",
                requested: self.projected_attempts as u64,
                limit: MAX_PROJECTED_ATTEMPTS as u64,
            });
        }
        if vertices.iter().any(|v| {
            !v.x.is_finite()
                || !v.y.is_finite()
                || !v.depth.is_finite()
                || v.x.abs() > 1e9
                || v.y.abs() > 1e9
                || v.depth.abs() > 1e10
                || v.uv.iter().any(|c| !c.is_finite())
        }) {
            return Err(metadata::invalid("nonfinite/out-of-budget raster vertex"));
        }
        if self.width == 0 || self.height == 0 {
            return Ok(());
        }
        let mut points =
            vertices.map(|v| [(v.x * 256.0).round() as i64, (v.y * 256.0).round() as i64]);
        let mut area = edge(points[0], points[1], points[2]);
        if area == 0 {
            return Ok(());
        }
        if area < 0 {
            points.swap(1, 2);
            vertices.swap(1, 2);
            area = -area;
        }
        let left = points
            .iter()
            .map(|p| p[0])
            .min()
            .unwrap_or(0)
            .div_euclid(256)
            .clamp(0, self.width as i64) as usize;
        let right = (points.iter().map(|p| p[0]).max().unwrap_or(0) + 255)
            .div_euclid(256)
            .clamp(0, self.width as i64) as usize;
        let top = points
            .iter()
            .map(|p| p[1])
            .min()
            .unwrap_or(0)
            .div_euclid(256)
            .clamp(0, self.height as i64) as usize;
        let bottom = (points.iter().map(|p| p[1]).max().unwrap_or(0) + 255)
            .div_euclid(256)
            .clamp(0, self.height as i64) as usize;
        if left == right || top == bottom {
            return Ok(());
        }
        let tests = right.saturating_sub(left) as u64 * bottom.saturating_sub(top) as u64;
        self.sample_tests = self
            .sample_tests
            .checked_add(tests)
            .ok_or(AssetError::Allocation)?;
        if self.sample_tests > self.limits.sample_tests {
            return Err(AssetError::Limit {
                resource: "raster sample tests",
                requested: self.sample_tests,
                limit: self.limits.sample_tests,
            });
        }
        let inclusive = [
            top_left(points[1], points[2]),
            top_left(points[2], points[0]),
            top_left(points[0], points[1]),
        ];
        let mut counted_visible_triangle = false;
        for y in top..bottom {
            cancel.check()?;
            for x in left..right {
                let sample = [x as i64 * 256 + 128, y as i64 * 256 + 128];
                let weights = [
                    edge(points[1], points[2], sample),
                    edge(points[2], points[0], sample),
                    edge(points[0], points[1], sample),
                ];
                if (0..3).any(|i| weights[i] < 0 || (weights[i] == 0 && !inclusive[i])) {
                    continue;
                }
                let weights = weights.map(|w| w as f64 / area as f64);
                let depth = (0..3).map(|i| weights[i] * vertices[i].depth).sum::<f64>();
                let uv = std::array::from_fn(|channel| {
                    (0..3)
                        .map(|i| weights[i] * f64::from(vertices[i].uv[channel]))
                        .sum::<f64>() as f32
                });
                let mut color = shader.sample(uv)?;
                if color.alpha() == 0.0 {
                    continue;
                }
                match mode {
                    AlphaMode::Opaque | AlphaMode::NativeSolid => {
                        if color.alpha() != 1.0 {
                            return Err(metadata::invalid(
                                "opaque shader produced translucent color",
                            ));
                        }
                    }
                    AlphaMode::Cutout { threshold } => {
                        if threshold == 0 {
                            return Err(metadata::invalid("cutout threshold must be positive"));
                        }
                        if color.alpha() < f32::from(threshold) / 255.0 {
                            continue;
                        }
                        color = LinearRgba::from_straight(color.straight(), 1.0)
                            .ok_or_else(|| metadata::invalid("invalid cutout sample"))?;
                    }
                    AlphaMode::NativeCutout | AlphaMode::NativeCutoutMipped => {
                        let threshold = if mode == AlphaMode::NativeCutout {
                            0.1
                        } else {
                            0.5
                        };
                        if color.alpha() < threshold {
                            continue;
                        }
                        color = LinearRgba::from_straight(color.straight(), 1.0)
                            .ok_or_else(|| metadata::invalid("invalid native cutout sample"))?;
                    }
                    AlphaMode::Blend | AlphaMode::NativeBlend => {}
                }
                // A clipped integer bounding box is only a work estimate.
                // Charge the visible-triangle budget when a real pixel sample
                // survives geometric coverage, alpha and cutout admission.
                if !counted_visible_triangle {
                    self.triangles += 1;
                    if self.triangles > self.limits.triangles {
                        return Err(AssetError::Limit {
                            resource: "raster triangles",
                            requested: self.triangles as u64,
                            limit: self.limits.triangles as u64,
                        });
                    }
                    counted_visible_triangle = true;
                }
                let fragment = Fragment {
                    rank: FragmentRank {
                        depth: (depth * 1_000_000.0).round() as i64,
                        layer: owner.layer,
                        owner,
                    },
                    color,
                };
                self.insert(y * self.width + x, fragment, mode)?;
            }
        }
        Ok(())
    }
    fn insert(&mut self, index: usize, fragment: Fragment, mode: AlphaMode) -> Result<()> {
        if !matches!(mode, AlphaMode::Blend | AlphaMode::NativeBlend) {
            if let Some(previous) = self.opaque[index] {
                if previous.rank == fragment.rank && previous.color != fragment.color {
                    return Err(metadata::invalid(
                        "conflicting samples share one opaque owner/rank",
                    ));
                }
                if previous.rank >= fragment.rank {
                    return Ok(());
                }
            }
            self.opaque[index] = Some(fragment);
            return Ok(());
        }
        let start = index * usize::from(self.limits.layers);
        let count = usize::from(self.counts[index]);
        // Keep candidate layers even behind an already submitted opaque face: admission is order-independent.
        for previous in self.blends[start..start + count].iter().flatten() {
            if previous.rank != fragment.rank {
                continue;
            }
            if previous.color != fragment.color {
                return Err(metadata::invalid(
                    "conflicting samples share one translucent owner/rank",
                ));
            }
            return Ok(());
        }
        // Native fluids follow the opaque pass. A known opaque rank only moves
        // closer until clear(), so a hidden native sample cannot contribute now
        // or later. Preserve generic Blend's order-independent admission policy.
        if mode == AlphaMode::NativeBlend
            && self.opaque[index].is_some_and(|opaque| fragment.rank <= opaque.rank)
        {
            return Ok(());
        }
        if count >= usize::from(self.limits.layers) {
            return Err(AssetError::Limit {
                resource: "translucent layers per pixel",
                requested: count as u64 + 1,
                limit: u64::from(self.limits.layers),
            });
        }
        self.blends[start + count] = Some(fragment);
        self.counts[index] += 1;
        Ok(())
    }
    pub fn pixel(&self, x: usize, y: usize) -> Result<PixelResult> {
        if !self.valid {
            return Err(metadata::invalid("invalid raster cannot publish pixels"));
        }
        if x >= self.width || y >= self.height {
            return Err(metadata::invalid("pixel outside raster"));
        }
        let index = y * self.width + x;
        let opaque = self.opaque[index];
        let mut result = PixelResult {
            color: opaque.map(|v| v.color).unwrap_or(LinearRgba::CLEAR),
            contributors: [None; 9],
            front_owner: opaque.map(|v| v.rank.owner),
        };
        let mut owners = 0;
        if let Some(opaque) = opaque {
            result.contributors[owners] = Some(opaque.rank.owner);
            owners += 1;
        }
        let mut ordered: [Option<Fragment>; 8] = [None; 8];
        let mut count = 0;
        let start = index * usize::from(self.limits.layers);
        for value in self.blends[start..start + usize::from(self.counts[index])]
            .iter()
            .flatten()
        {
            if opaque.is_some_and(|opaque| value.rank <= opaque.rank) {
                continue;
            }
            let mut at = count;
            while at > 0 && ordered[at - 1].is_some_and(|previous| previous.rank > value.rank) {
                ordered[at] = ordered[at - 1];
                at -= 1;
            }
            ordered[at] = Some(*value);
            count += 1;
        }
        for fragment in ordered[..count].iter().flatten() {
            result.color = fragment.color.over(result.color);
            if fragment.color.alpha() == 1.0 {
                result.contributors.fill(None);
                owners = 0;
            }
            result.contributors[owners] = Some(fragment.rank.owner);
            owners += 1;
            result.front_owner = Some(fragment.rank.owner);
        }
        Ok(result)
    }
}
fn edge(a: [i64; 2], b: [i64; 2], p: [i64; 2]) -> i128 {
    i128::from(b[0] - a[0]) * i128::from(p[1] - a[1])
        - i128::from(b[1] - a[1]) * i128::from(p[0] - a[0])
}
fn top_left(a: [i64; 2], b: [i64; 2]) -> bool {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    dy < 0 || (dy == 0 && dx > 0)
}
#[derive(Clone, Copy, Debug)]
pub struct DirectionalLight {
    pub direction: [f32; 3],
    pub ambient: f32,
    pub diffuse: f32,
}
impl Default for DirectionalLight {
    fn default() -> Self {
        Self {
            direction: [0.2, 0.5, 1.0],
            ambient: 0.35,
            diffuse: 0.65,
        }
    }
}
impl DirectionalLight {
    pub(crate) fn factor(self, normal: [f32; 3], enabled: bool) -> Result<f32> {
        if self.direction.iter().any(|v| !v.is_finite())
            || !self.ambient.is_finite()
            || !self.diffuse.is_finite()
            || self.ambient < 0.0
            || self.diffuse < 0.0
            || self.ambient + self.diffuse > 1.0
        {
            return Err(metadata::invalid("invalid finite diffuse light"));
        }
        let length = self.direction.iter().map(|v| v * v).sum::<f32>().sqrt();
        if length <= 1e-8 || !length.is_finite() {
            return Err(metadata::invalid("zero/overflowing light direction"));
        }
        if !enabled {
            return Ok(1.0);
        }
        let dot = normal
            .iter()
            .zip(self.direction)
            .map(|(a, b)| a * b / length)
            .sum::<f32>()
            .max(0.0);
        Ok((self.ambient + self.diffuse * dot).clamp(0.0, 1.0))
    }
}
struct TextureShader<'a> {
    texture: &'a Texture,
    normal_map: Option<&'a Texture>,
    specular_map: Option<&'a Texture>,
    time: Duration,
    tint: [f32; 3],
    quad: &'a BoundQuad,
    light: DirectionalLight,
    position: [i32; 3],
    flow: bool,
    water_time_seconds: f64,
}
impl FragmentShader for TextureShader<'_> {
    fn sample(&self, uv: [f32; 2]) -> Result<LinearRgba> {
        // All motion is keyed by frame time and absolute world coordinates.
        // Camera motion has no effect on material phase.
        let surface_uv = uv;
        let uv = if self.flow {
            let t = self.time.as_secs_f64();
            // One translation for all generated cells keeps repeated authored
            // pixels aligned where adjacent UVs wrap from 1 back to 0.
            let ripple = (t * 1.7).sin() * 0.008;
            [
                uv[0] + (t * 0.035 + ripple) as f32,
                uv[1] + (t * 0.019 - ripple) as f32,
            ]
        } else {
            uv
        };
        let (base, alpha) = if matches!(
            self.quad.material.alpha,
            AlphaMode::NativeSolid
                | AlphaMode::NativeCutout
                | AlphaMode::NativeCutoutMipped
                | AlphaMode::NativeBlend
        ) {
            let raw = self
                .texture
                .sample_native_color(uv, self.time)
                .ok_or_else(|| metadata::invalid("invalid native diffuse sample"))?;
            ([raw[0], raw[1], raw[2]], raw[3])
        } else {
            let color = self
                .texture
                .sample_color(uv, self.time)
                .ok_or_else(|| metadata::invalid("invalid diffuse sample"))?;
            (color.straight(), color.alpha())
        };
        let mapped_normal = if let Some(map) = self.normal_map {
            let texel = map
                .sample_data_nearest(uv, self.time)
                .ok_or_else(|| metadata::invalid("invalid normal-map sample"))?;
            let encoded = LabNormal::decode(texel);
            let tangent = normalize3(delta(self.quad.points[1], self.quad.points[0]))?;
            let bitangent = normalize3(delta(self.quad.points[3], self.quad.points[0]))?;
            normalize3([
                tangent[0] * encoded.tangent[0]
                    + bitangent[0] * encoded.tangent[1]
                    + self.quad.normal[0] * encoded.tangent[2],
                tangent[1] * encoded.tangent[0]
                    + bitangent[1] * encoded.tangent[1]
                    + self.quad.normal[1] * encoded.tangent[2],
                tangent[2] * encoded.tangent[0]
                    + bitangent[2] * encoded.tangent[1]
                    + self.quad.normal[2] * encoded.tangent[2],
            ])?
        } else {
            self.quad.normal
        };
        // Generated top-face UVs are local XY in FluidMesh::build. Evaluate
        // the lighting normal from absolute XY before shifting diffuse UVs:
        // two adjacent cells then agree at their shared world coordinate.
        // Native fluid never enters this branch, and source pixels, timeline,
        // biome tint, PBR channels and diffuse alpha remain the inputs above.
        let mapped_normal = if self.flow && self.quad.face == 0 && self.quad.shade {
            let slopes = generated_water_slopes(self.position, surface_uv, self.water_time_seconds);
            normalize3([
                mapped_normal[0] - slopes[0],
                mapped_normal[1] - slopes[1],
                mapped_normal[2],
            ])?
        } else {
            mapped_normal
        };
        let ao = if let Some(map) = self.normal_map {
            let texel = map
                .sample_data_nearest(uv, self.time)
                .ok_or_else(|| metadata::invalid("invalid normal-map AO sample"))?;
            LabNormal::decode(texel).ambient_occlusion
        } else {
            1.0
        };
        let shade = self.light.factor(mapped_normal, self.quad.shade)?;
        let mut rgb = std::array::from_fn::<_, 3, _>(|i| base[i] * self.tint[i] * shade * ao);
        if let Some(map) = self.specular_map {
            let texel = map
                .sample_data_nearest(uv, self.time)
                .ok_or_else(|| metadata::invalid("invalid specular-map sample"))?;
            let data = LabSpecular::decode(texel);
            // Bounded single directional-light approximation: Schlick Fresnel,
            // roughness broadening, and emission. No PBR environment or shadow pass.
            let f0 = match data.fresnel {
                FresnelSource::Dielectric(value) => value.clamp(0.0, 1.0),
                FresnelSource::MetalId(_) | FresnelSource::AlbedoBased => {
                    (base[0] + base[1] + base[2]) / 3.0
                }
            };
            let view = normalize3([0.45, 0.45, 0.77])?;
            let cos_view = dot3(mapped_normal, view).max(0.0);
            let fresnel = f0 + (1.0 - f0) * (1.0 - cos_view).powi(5);
            let light_dir = normalize3(self.light.direction)?;
            let half = normalize3(std::array::from_fn(|i| view[i] + light_dir[i]))?;
            let gloss = dot3(mapped_normal, half)
                .max(0.0)
                .powf(2.0 + (1.0 - data.roughness) * 64.0);
            let specular = fresnel * gloss * self.light.diffuse * 0.45;
            for channel in &mut rgb {
                *channel = (*channel + specular).min(1.0);
            }
            if let Some(emission) = data.emission {
                for i in 0..3 {
                    rgb[i] = (rgb[i] + base[i] * emission * 0.35).min(1.0);
                }
            }
        }
        let output_alpha = if self.quad.material.alpha == AlphaMode::NativeSolid {
            1.0
        } else {
            alpha
        };
        LinearRgba::from_straight(rgb, output_alpha)
            .ok_or_else(|| metadata::invalid("invalid material result"))
    }
}
/// Original generated-water lighting only. The phase repeats exactly after
/// 20 seconds (nine and six cycles), so Duration precision cannot freeze it
/// after long-running sessions. No texture bytes or geometry are synthesized.
fn generated_water_slopes(position: [i32; 3], uv: [f32; 2], time_seconds: f64) -> [f32; 2] {
    let x = f64::from(position[0]) + f64::from(uv[0]);
    let y = f64::from(position[1]) + f64::from(uv[1]);
    let first = std::f64::consts::TAU * (0.31 * x + 0.17 * y - 0.45 * time_seconds);
    let second = std::f64::consts::TAU * (-0.13 * x + 0.29 * y - 0.30 * time_seconds);
    let first_slope = 0.12 * std::f64::consts::TAU * first.cos();
    let second_slope = 0.065 * std::f64::consts::TAU * second.cos();
    [
        (0.31 * first_slope - 0.13 * second_slope) as f32,
        (0.17 * first_slope + 0.29 * second_slope) as f32,
    ]
}
fn delta(a: [f64; 3], b: [f64; 3]) -> [f32; 3] {
    std::array::from_fn(|i| (a[i] - b[i]) as f32)
}
fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}
fn normalize3(v: [f32; 3]) -> Result<[f32; 3]> {
    let length = dot3(v, v).sqrt();
    if !length.is_finite() || length <= 1e-8 {
        return Err(metadata::invalid("degenerate material basis"));
    }
    Ok(v.map(|value| value / length))
}
fn resolve_shader<'a>(
    bank: &'a TextureBank,
    quad: &'a BoundQuad,
    time: Duration,
    light: DirectionalLight,
    position: [i32; 3],
    flow: bool,
) -> Result<TextureShader<'a>> {
    let texture = bank
        .texture(quad.material.texture)
        .ok_or_else(|| metadata::invalid("stale diffuse handle during draw"))?;
    let normal_map = quad
        .material
        .normal_map
        .map(|handle| {
            bank.texture(handle)
                .ok_or_else(|| metadata::invalid("stale normal-map handle during draw"))
        })
        .transpose()?;
    let specular_map = quad
        .material
        .specular_map
        .map(|handle| {
            bank.texture(handle)
                .ok_or_else(|| metadata::invalid("stale specular-map handle during draw"))
        })
        .transpose()?;
    Ok(TextureShader {
        texture,
        normal_map,
        specular_map,
        time,
        tint: quad.material.tint,
        quad,
        light,
        position,
        flow,
        water_time_seconds: if flow && quad.face == 0 && quad.shade {
            (time.as_nanos() % 20_000_000_000) as f64 * 1e-9
        } else {
            0.0
        },
    })
}
fn project_quad(
    position: [i32; 3],
    quad: &BoundQuad,
    camera: [f64; 3],
    scale: f64,
    size: [usize; 2],
) -> [Vertex; 4] {
    let base = camera.map(|value| value.floor() as i64);
    let local_camera = std::array::from_fn::<_, 3, _>(|i| camera[i] - base[i] as f64);
    std::array::from_fn(|i| {
        let p = std::array::from_fn::<_, 3, _>(|axis| {
            (i64::from(position[axis]) - base[axis]) as f64 + quad.points[i][axis]
                - local_camera[axis]
        });
        Vertex {
            x: size[0] as f64 * 0.5 + (p[0] - p[1]) * 0.8660254037844386 * scale,
            y: size[1] as f64 * 0.5 + ((p[0] + p[1]) * 0.5 - p[2]) * scale,
            depth: p.iter().sum(),
            uv: quad.uv[i],
        }
    })
}
/// Native geometry-to-pixels component; host/world/probe integration belongs to D2c.
/// This initial diffuse shader is not a claim of full LabPBR appearance.
#[expect(
    clippy::too_many_arguments,
    reason = "render inputs keep bank, camera, frame time, light and cancellation explicit"
)]
pub fn draw_mesh(
    mesh: &PreparedMesh,
    bank: &TextureBank,
    camera: [f64; 3],
    scale: f64,
    time: Duration,
    light: DirectionalLight,
    output: &mut RasterFrame,
    cancel: Cancel<'_>,
) -> Result<()> {
    output.clear();
    let result = draw_mesh_inner(mesh, bank, camera, scale, time, light, output, cancel);
    if result.is_err() {
        output.valid = false;
    }
    result
}
/// Append one solid tile to a shared frame without resetting earlier depth,
/// color, or face-owner contributions. Callers clear once before the first
/// tile and append all solid tiles before appending their fluid geometry.
/// Any failed tile invalidates the complete frame; only an explicit clear
/// starts a new frame after failure.
#[expect(
    clippy::too_many_arguments,
    reason = "render inputs keep bank, camera, frame time, light and cancellation explicit"
)]
pub fn draw_mesh_layer(
    mesh: &PreparedMesh,
    bank: &TextureBank,
    camera: [f64; 3],
    scale: f64,
    time: Duration,
    light: DirectionalLight,
    output: &mut RasterFrame,
    cancel: Cancel<'_>,
) -> Result<()> {
    if !output.is_valid() {
        return Err(metadata::invalid(
            "cannot append mesh to an invalid raster frame",
        ));
    }
    let result = draw_mesh_inner(mesh, bank, camera, scale, time, light, output, cancel);
    if result.is_err() {
        output.valid = false;
    }
    result
}
#[expect(
    clippy::too_many_arguments,
    reason = "render inputs keep bank, camera, frame time, light and cancellation explicit"
)]
pub(super) fn draw_mesh_inner(
    mesh: &PreparedMesh,
    bank: &TextureBank,
    camera: [f64; 3],
    scale: f64,
    time: Duration,
    light: DirectionalLight,
    output: &mut RasterFrame,
    cancel: Cancel<'_>,
) -> Result<()> {
    cancel.check()?;
    if !mesh.uses_budget(&output.budget) {
        return Err(metadata::invalid("mesh and raster accounts differ"));
    } // Count geometry and frame memory against one scene budget.
    if mesh.bank != bank.identity() {
        return Err(metadata::invalid("mesh and texture bank identities differ"));
    }
    if !scale.is_finite()
        || !(0.01..=1024.0).contains(&scale)
        || camera
            .iter()
            .any(|c| !c.is_finite() || c.abs() > f64::from(i32::MAX))
    {
        return Err(metadata::invalid(
            "camera or scale outside finite render domain",
        ));
    }
    for face in &mesh.faces {
        cancel.check()?;
        let quad = face
            .model
            .quads
            .get(usize::from(face.quad_index))
            .ok_or_else(|| metadata::invalid("mesh references missing quad"))?;
        if dot3(quad.normal, quad.normal) <= 1e-8 {
            continue;
        }
        let shader = resolve_shader(bank, quad, time, light, face.position, false)?;
        let vertices = project_quad(face.position, quad, camera, scale, output.size());
        output.quad(vertices, face.owner, quad.material.alpha, &shader, cancel)?;
    }
    Ok(())
}

/// Composite animated translucent fluid over the opaque/model frame without
/// clearing its riverbed or banks. Both meshes must retain one bank and budget.
#[expect(
    clippy::too_many_arguments,
    reason = "fluid composition keeps bank, camera, frame time, light and cancellation explicit"
)]
pub fn draw_fluid_mesh(
    mesh: &FluidMesh,
    bank: &TextureBank,
    camera: [f64; 3],
    scale: f64,
    time: Duration,
    light: DirectionalLight,
    output: &mut RasterFrame,
    cancel: Cancel<'_>,
) -> Result<()> {
    let result = (|| {
        cancel.check()?;
        if !output.is_valid()
            || !mesh.uses_budget(&output.budget)
            || mesh.bank != bank.identity()
            || !scale.is_finite()
            || !(0.01..=1024.0).contains(&scale)
            || camera
                .iter()
                .any(|value| !value.is_finite() || value.abs() > f64::from(i32::MAX))
        {
            return Err(metadata::invalid(
                "incompatible fluid frame/bank/account/camera",
            ));
        }
        for face in &mesh.faces {
            cancel.check()?;
            let native = matches!(
                face.quad.material.alpha,
                AlphaMode::NativeSolid
                    | AlphaMode::NativeCutout
                    | AlphaMode::NativeCutoutMipped
                    | AlphaMode::NativeBlend
            );
            let shader = resolve_shader(bank, &face.quad, time, light, face.position, !native)?;
            let vertices = project_quad(face.position, &face.quad, camera, scale, output.size());
            output.quad(
                vertices,
                face.owner,
                face.quad.material.alpha,
                &shader,
                cancel,
            )?;
        }
        Ok(())
    })();
    if result.is_err() {
        output.valid = false;
    }
    result
}
pub fn linear_to_srgb_byte(value: f32) -> u8 {
    let value = value.clamp(0.0, 1.0);
    let encoded = if value <= 0.0031308 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0).round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod generated_water_tests {
    use super::generated_water_slopes;

    #[test]
    fn generated_water_phase_joins_signed_world_cells_without_a_tile_origin() {
        for x in [-1_000_000_000, -3, 0, 1_000_000_000] {
            for time in [0.0, 0.4, 7.25, 19.9] {
                let east = generated_water_slopes([x, -17, 2], [1.0, 0.375], time);
                let west = generated_water_slopes([x + 1, -17, 2], [0.0, 0.375], time);
                assert_eq!(east, west);
                let north = generated_water_slopes([x, -17, 2], [0.625, 1.0], time);
                let south = generated_water_slopes([x, -16, 2], [0.625, 0.0], time);
                assert_eq!(north, south);
            }
        }
        assert_ne!(
            generated_water_slopes([-3, -17, 2], [0.375, 0.625], 0.0),
            generated_water_slopes([-3, -17, 2], [0.375, 0.625], 0.4)
        );
    }
}
