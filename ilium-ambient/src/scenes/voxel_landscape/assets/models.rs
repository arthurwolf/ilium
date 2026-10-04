//! Bounded Java element/parent/state normalization into ground-x, ground-y, height quads.
//! Missing definitions are errors; optional explicit compatibility definitions retain origin.
use super::{
    block_state::{BlockState, ModelApplication, StateDefinition},
    budget::{ByteBudget, Cancel, Limits, Reservation},
    error::{AssetError, Result},
    identity::{BlobOrigin, Digest256, Label, ResourceId},
    layers::{ResourceKey, ResourceKind},
    metadata::{self, allowed, array, boolean, object, required, text, uint, Document},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub enum Direction {
    Down,
    Up,
    North,
    South,
    West,
    East,
}
impl Direction {
    pub const ALL: [Self; 6] = [
        Self::Down,
        Self::Up,
        Self::North,
        Self::South,
        Self::West,
        Self::East,
    ];
    pub fn parse(name: &str) -> Result<Self> {
        match name {
            "down" => Ok(Self::Down),
            "up" => Ok(Self::Up),
            "north" => Ok(Self::North),
            "south" => Ok(Self::South),
            "west" => Ok(Self::West),
            "east" => Ok(Self::East),
            _ => Err(metadata::invalid("unknown model face")),
        }
    }
    pub const fn name(self) -> &'static str {
        match self {
            Self::Down => "down",
            Self::Up => "up",
            Self::North => "north",
            Self::South => "south",
            Self::West => "west",
            Self::East => "east",
        }
    }
    pub const fn index(self) -> usize {
        self as usize
    }
    pub const fn opposite(self) -> Self {
        match self {
            Self::Down => Self::Up,
            Self::Up => Self::Down,
            Self::North => Self::South,
            Self::South => Self::North,
            Self::West => Self::East,
            Self::East => Self::West,
        }
    }
    /// Direction in the destination ground-x, ground-y, height convention.
    pub const fn step(self) -> [i32; 3] {
        match self {
            Self::Down => [0, 0, -1],
            Self::Up => [0, 0, 1],
            Self::North => [0, -1, 0],
            Self::South => [0, 1, 0],
            Self::West => [-1, 0, 0],
            Self::East => [1, 0, 0],
        }
    }
    fn normal_java(self) -> [f64; 3] {
        let [x, z, y] = self.step();
        [f64::from(x), f64::from(y), f64::from(z)]
    }
    fn from_java_normal(normal: [f64; 3]) -> Option<Self> {
        Self::ALL.into_iter().find(|face| {
            face.normal_java()
                .iter()
                .zip(normal)
                .all(|(a, b)| (*a - b).abs() < 1e-7)
        })
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct DefinitionOrigin {
    pub resource: ResourceKey,
    pub origin: BlobOrigin,
    pub sha256: Digest256,
    pub compatibility: Option<Label>,
}
pub struct DefinitionInput {
    pub document: Document,
    pub compatibility: Option<Label>,
}
pub trait DefinitionProvider {
    /// Called only while preparing assets, never by the screen rasterizer.
    fn definition(&self, key: &ResourceKey, cancel: Cancel<'_>) -> Result<Option<DefinitionInput>>;
}
#[derive(Clone, Debug, PartialEq)]
pub struct ModelQuad {
    pub points: [[f64; 3]; 4],
    pub uv: [[f32; 2]; 4],
    pub normal: [f32; 3],
    pub texture: ResourceId,
    pub tint_index: Option<u16>,
    pub cull_face: Option<Direction>,
    /// Only a complete unit boundary can hide a neighbor's entire face.
    pub complete_boundary: Option<Direction>,
    pub shade: bool,
    pub element: u16,
    pub face: Direction,
}
#[derive(Debug)]
pub struct NormalizedModel {
    pub id: ResourceId,
    pub quads: Vec<ModelQuad>,
    pub origins: Vec<DefinitionOrigin>,
    pub ignored_non_world_fields: Vec<Label>,
    _reservation: Reservation,
}
impl NormalizedModel {
    pub(crate) fn uses_budget(&self, budget: &ByteBudget) -> bool {
        self._reservation.belongs_to(budget)
    }
}
#[derive(Debug)]
pub struct NormalizedState {
    pub state: BlockState,
    pub applications: Vec<(ModelApplication, Arc<NormalizedModel>)>,
    pub state_origin: DefinitionOrigin,
    _reservation: Reservation,
}
impl NormalizedState {
    pub(crate) fn uses_budget(&self, budget: &ByteBudget) -> bool {
        self._reservation.belongs_to(budget)
    }
}
/// No global cache or frame-time dependence: callers own and discard this finite preparation cache.
pub struct ModelCompiler<'a, P: DefinitionProvider + ?Sized> {
    provider: &'a P,
    budget: ByteBudget,
    limits: Limits,
    state_cache: BTreeMap<ResourceId, CachedStateDefinition>,
    cache: BTreeMap<ResourceId, Arc<NormalizedModel>>,
    cache_reservations: Vec<Reservation>,
    state_cache_enabled: bool,
    recycle_on_full: bool,
}
struct CachedStateDefinition {
    definition: StateDefinition,
    origin: DefinitionOrigin,
    // The source Document's 128x encoded-byte allowance transfers to the
    // parsed selector. The original JSON tree is released after first use.
    _reservation: Reservation,
}
impl<'a, P: DefinitionProvider + ?Sized> ModelCompiler<'a, P> {
    pub fn new(provider: &'a P, limits: Limits, budget: ByteBudget) -> Result<Self> {
        limits.validate()?;
        Ok(Self {
            provider,
            budget,
            limits,
            state_cache: BTreeMap::new(),
            cache: BTreeMap::new(),
            cache_reservations: Vec::new(),
            state_cache_enabled: false,
            recycle_on_full: false,
        })
    }

    /// Only call after the mounted source proves every definition layer is an
    /// immutable in-memory archive. Default compilers re-read blockstates on
    /// every call, retaining the existing mutable DefinitionProvider contract.
    pub(crate) fn enable_immutable_source_cache(&mut self) {
        self.state_cache_enabled = true;
        self.recycle_on_full = true;
    }
    /// Match the old one-compiler-per-tile model-cache lifetime for a live or
    /// otherwise unverified source. Blockstate definitions were never cached.
    pub(crate) fn reset_for_next_tile(&mut self) {
        self.state_cache.clear();
        self.cache.clear();
        self.cache_reservations.clear();
    }
    pub fn compile_state(
        &mut self,
        state: &BlockState,
        anchor: [i32; 3],
        seed: u64,
        cancel: Cancel<'_>,
    ) -> Result<NormalizedState> {
        cancel.check()?;
        let key = ResourceKey {
            kind: ResourceKind::Blockstate,
            id: state.id().clone(),
        };
        let (choices, state_origin) = if self.state_cache_enabled {
            if !self.state_cache.contains_key(state.id()) {
                if self.state_cache.len() >= self.limits.textures.min(4096) {
                    if self.recycle_on_full {
                        self.state_cache.clear();
                    } else {
                        return Err(metadata::invalid("blockstate definition cache limit"));
                    }
                }
                let input = self.provider.definition(&key, cancel)?.ok_or_else(|| {
                    AssetError::InvalidMetadata(format!(
                        "missing blockstate definition: {}",
                        key.id
                    ))
                })?;
                if !input.document.uses_budget(&self.budget) {
                    return Err(metadata::invalid(
                        "state metadata and compiler accounts differ",
                    ));
                }
                let DefinitionInput {
                    document,
                    compatibility,
                } = input;
                let definition = StateDefinition::parse(&document.value)?;
                cancel.check()?;
                let origin = DefinitionOrigin {
                    resource: key.clone(),
                    origin: document.origin.clone(),
                    sha256: document.sha256,
                    compatibility,
                };
                let reservation = document.into_charge();
                self.state_cache.insert(
                    key.id.clone(),
                    CachedStateDefinition {
                        definition,
                        origin,
                        _reservation: reservation,
                    },
                );
            }
            // Only the immutable parsed document is cached. Selection still
            // uses this exact state, position and signed seed on every call.
            let cached = self
                .state_cache
                .get(state.id())
                .ok_or_else(|| metadata::invalid("missing cached blockstate definition"))?;
            (
                cached.definition.select(state, anchor, seed)?,
                cached.origin.clone(),
            )
        } else {
            let input = self.provider.definition(&key, cancel)?.ok_or_else(|| {
                AssetError::InvalidMetadata(format!("missing blockstate definition: {}", key.id))
            })?;
            if !input.document.uses_budget(&self.budget) {
                return Err(metadata::invalid(
                    "state metadata and compiler accounts differ",
                ));
            }
            let definition = StateDefinition::parse(&input.document.value)?;
            let choices = definition.select(state, anchor, seed)?;
            (
                choices,
                DefinitionOrigin {
                    resource: key,
                    origin: input.document.origin.clone(),
                    sha256: input.document.sha256,
                    compatibility: input.compatibility,
                },
            )
        };
        let reservation = self
            .budget
            .reserve(16_384 + choices.len() as u64 * 4096, cancel)?;
        let mut applications = Vec::new();
        for application in choices {
            cancel.check()?;
            let model = self.compile_model(&application.model, cancel)?;
            applications.push((application, model));
        }
        Ok(NormalizedState {
            state: state.clone(),
            applications,
            state_origin,
            _reservation: reservation,
        })
    }
    pub fn compile_model(
        &mut self,
        id: &ResourceId,
        cancel: Cancel<'_>,
    ) -> Result<Arc<NormalizedModel>> {
        cancel.check()?;
        if let Some(model) = self.cache.get(id) {
            return Ok(Arc::clone(model));
        }
        if self.cache.len() >= self.limits.textures.min(4096) {
            if self.recycle_on_full {
                self.cache.clear();
                self.cache_reservations.clear();
            } else {
                return Err(metadata::invalid("normalized model cache limit"));
            }
        }
        let mut chain = Vec::new();
        let mut seen = BTreeSet::new();
        let mut current = id.clone();
        loop {
            cancel.check()?;
            if chain.len() >= 32 || !seen.insert(current.clone()) {
                return Err(metadata::invalid("model parent cycle/depth limit"));
            }
            let key = ResourceKey {
                kind: ResourceKind::Model,
                id: current.clone(),
            };
            let input = self.provider.definition(&key, cancel)?.ok_or_else(|| {
                AssetError::InvalidMetadata(format!("missing model definition: {current}"))
            })?;
            if !input.document.uses_budget(&self.budget) {
                return Err(metadata::invalid(
                    "model metadata and compiler accounts differ",
                ));
            } // Keep retained parent metadata inside the compiler account.
            let fields = object(&input.document.value)?;
            if fields.contains_key("loader") {
                return Err(AssetError::Unsupported(
                    "custom model loader is not executable or silently treated as cubes".into(),
                ));
            }
            allowed(
                fields,
                &[
                    "parent",
                    "textures",
                    "elements",
                    "ambientocclusion",
                    "display",
                    "gui_light",
                    "credit",
                    "texture_size",
                    // The 1.19.3 BlockModel deserializer reads named fields and
                    // ignores these exporter labels; they carry no world mesh.
                    "format_version",
                    "groups",
                    "__createdwith",
                    "render_type",
                ],
            )?;
            // Render-layer metadata is semantically significant and must be consumed by the binding policy.
            if fields.contains_key("render_type") {
                return Err(AssetError::Unsupported(
                    "model render_type requires an explicit supported binding adapter".into(),
                ));
            }
            let parent = fields
                .get("parent")
                .map(|v| ResourceId::parse(text(v)?))
                .transpose()?;
            chain.push((key, input));
            let Some(parent) = parent else {
                break;
            };
            current = parent;
        }
        let work_reservation = self.budget.reserve(4 * 1024 * 1024, cancel)?;
        let mut textures = BTreeMap::<String, String>::new();
        let mut elements: Option<&Value> = None;
        let mut origins = Vec::new();
        let mut ignored = BTreeSet::new();
        for (key, input) in chain.iter().rev() {
            if !input.document.uses_budget(&self.budget) {
                return Err(metadata::invalid(
                    "model metadata and compiler accounts differ",
                ));
            } // Keep retained parent metadata inside the compiler account.
            let fields = object(&input.document.value)?;
            if let Some(value) = fields.get("textures") {
                let values = object(value)?;
                if values.len() > 1024 {
                    return Err(metadata::invalid("model texture variable limit"));
                }
                for (key, value) in values {
                    if !texture_variable(key) {
                        return Err(metadata::invalid("bad model texture variable"));
                    }
                    let value = text(value)?;
                    if value.len() > 1024 {
                        return Err(metadata::invalid("model texture reference too long"));
                    }
                    textures.insert(key.clone(), value.into());
                }
                if textures.len() > 1024 {
                    return Err(metadata::invalid("inherited texture variable limit"));
                }
            }
            if let Some(value) = fields.get("elements") {
                elements = Some(value);
            }
            for name in [
                "display",
                "gui_light",
                "credit",
                "texture_size",
                "ambientocclusion",
                "format_version",
                "groups",
                "__createdwith",
            ] {
                if fields.contains_key(name) {
                    ignored.insert(name);
                }
            }
            origins.push(DefinitionOrigin {
                resource: key.clone(),
                origin: input.document.origin.clone(),
                sha256: input.document.sha256,
                compatibility: input.compatibility.clone(),
            });
        }
        let elements = elements.map(array).transpose()?.unwrap_or(&[]);
        if elements.len() > 256 {
            return Err(metadata::invalid("model element limit"));
        }
        let mut quads = Vec::new();
        for (element_index, element) in elements.iter().enumerate() {
            cancel.check()?;
            let fields = object(element)?;
            allowed(
                fields,
                &[
                    "from",
                    "to",
                    "rotation",
                    "shade",
                    "faces",
                    "name",
                    // Present in native fence/wall elements; it labels the
                    // authored part and has no geometry or rendering behavior.
                    "__comment",
                    // The pinned 1.19.3 BlockElement deserializer does not read
                    // this later exporter hint; native shading still uses shade.
                    "shade_direction_override",
                    "light_emission",
                ],
            )?;
            if fields.contains_key("shade_direction_override") {
                ignored.insert("shade_direction_override");
            }
            if fields.contains_key("light_emission") {
                // The pinned 1.19.3 BlockElement deserializer never reads this
                // newer exporter hint. Preserve its source provenance; texture
                // selection and the separately parsed shade flag remain active.
                ignored.insert("light_emission");
            }
            let from = triple(required(fields, "from")?)?;
            let to = triple(required(fields, "to")?)?;
            if (0..3).any(|i| from[i] > to[i]) {
                return Err(metadata::invalid("reversed cuboid bounds"));
            }
            let rotation = fields
                .get("rotation")
                .map(ElementRotation::parse)
                .transpose()?;
            let shade = fields
                .get("shade")
                .map(boolean)
                .transpose()?
                .unwrap_or(true);
            let faces = object(required(fields, "faces")?)?;
            if faces.len() > 6 {
                return Err(metadata::invalid("cuboid face count"));
            }
            for (name, value) in faces {
                let face = Direction::parse(name)?;
                let values = object(value)?;
                allowed(
                    values,
                    &["uv", "texture", "cullface", "rotation", "tintindex"],
                )?;
                let texture = resolve_texture(text(required(values, "texture")?)?, &textures)?;
                let uv_rect = values
                    .get("uv")
                    .map(quad_uv)
                    .transpose()?
                    .unwrap_or_else(|| default_uv(face, from, to));
                let turns = values.get("rotation").map(uint).transpose()?.unwrap_or(0);
                if turns > 270 || turns % 90 != 0 {
                    return Err(metadata::invalid("face UV rotation must be 0/90/180/270"));
                }
                let tint_index = values
                    .get("tintindex")
                    .map(|v| {
                        let value = v
                            .as_i64()
                            .ok_or_else(|| metadata::invalid("tint index integer required"))?;
                        if value == -1 {
                            return Ok(None);
                        }
                        u16::try_from(value)
                            .map(Some)
                            .map_err(|_| metadata::invalid("tint index outside -1..65535"))
                    })
                    .transpose()?
                    .flatten();
                let cull_face = values
                    .get("cullface")
                    .map(|v| Direction::parse(text(v)?))
                    .transpose()?;
                // Plane elements can author all six faces. Their collapsed
                // edges have no pixel area; retain both visible sides without
                // inventing thickness. Validate every face field before omitting
                // these exactly zero-area faces, including culling and UV data.
                let zero_area = match face {
                    Direction::North | Direction::South => from[0] == to[0] || from[1] == to[1],
                    Direction::East | Direction::West => from[1] == to[1] || from[2] == to[2],
                    Direction::Up | Direction::Down => from[0] == to[0] || from[2] == to[2],
                };
                if zero_area {
                    continue;
                }
                let mut points = face_points(face, from, to);
                if let Some(rotation) = rotation {
                    for point in &mut points {
                        *point = rotation.apply(*point);
                    }
                }
                let normal_java = geometric_normal(points)
                    .ok_or_else(|| metadata::invalid("degenerate model face"))?
                    .map(|c| -c);
                let complete_boundary = complete_boundary(points, normal_java);
                // A rotated element's cull face must match an actual complete boundary to avoid false removals.
                let cull_face = cull_face.filter(|face| complete_boundary == Some(*face));
                let [u0, v0, u1, v1] = uv_rect;
                let uv = [[u0, v0], [u1, v0], [u1, v1], [u0, v1]];
                let uv = std::array::from_fn(|i| {
                    uv[(i + turns as usize / 90) % 4].map(|c| (c / 16.0) as f32)
                });
                quads.push(ModelQuad {
                    points: points.map(to_world),
                    uv,
                    normal: [
                        normal_java[0] as f32,
                        normal_java[2] as f32,
                        normal_java[1] as f32,
                    ],
                    texture,
                    tint_index,
                    cull_face,
                    complete_boundary,
                    shade,
                    element: element_index as u16,
                    face,
                });
            }
        }
        let ignored_non_world_fields = ignored
            .into_iter()
            .map(Label::new)
            .collect::<Result<Vec<_>>>()?;
        let reservation = self.budget.reserve(
            65536 + quads.len() as u64 * 2048 + origins.len() as u64 * 4096,
            cancel,
        )?;
        drop(work_reservation);
        let model = Arc::new(NormalizedModel {
            id: id.clone(),
            quads,
            origins,
            ignored_non_world_fields,
            _reservation: reservation,
        });
        self.cache_reservations
            .push(self.budget.reserve(4096, cancel)?);
        self.cache.insert(id.clone(), Arc::clone(&model));
        Ok(model)
    }
}
fn texture_variable(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}
fn resolve_texture(value: &str, textures: &BTreeMap<String, String>) -> Result<ResourceId> {
    let mut current = value;
    let mut seen = BTreeSet::new();
    for _ in 0..64 {
        let Some(key) = current.strip_prefix('#') else {
            return ResourceId::parse(current);
        };
        if !texture_variable(key) || !seen.insert(key) {
            return Err(metadata::invalid("texture variable cycle/invalid name"));
        }
        current = textures
            .get(key)
            .ok_or_else(|| metadata::invalid("unresolved model texture variable"))?;
    }
    Err(metadata::invalid("texture variable chain limit")) // Preserve the explicit failure instead of treating it as absence.
}
fn number(value: &Value) -> Result<f64> {
    let value = value
        .as_f64()
        .ok_or_else(|| metadata::invalid("finite model number required"))?;
    if !value.is_finite() {
        return Err(metadata::invalid("nonfinite model number"));
    }
    Ok(value)
}
fn triple(value: &Value) -> Result<[f64; 3]> {
    let values = array(value)?;
    if values.len() != 3 {
        return Err(metadata::invalid("three model coordinates required"));
    }
    let result = [
        number(&values[0])?,
        number(&values[1])?,
        number(&values[2])?,
    ];
    if result.iter().any(|v| !(-16.0..=32.0).contains(v)) {
        return Err(metadata::invalid("model coordinate outside -16..32"));
    }
    Ok(result)
}
fn quad_uv(value: &Value) -> Result<[f64; 4]> {
    let values = array(value)?;
    if values.len() != 4 {
        return Err(metadata::invalid("four UV coordinates required"));
    }
    let result = [
        number(&values[0])?,
        number(&values[1])?,
        number(&values[2])?,
        number(&values[3])?,
    ];
    if result.iter().any(|v| v.abs() > 1024.0) {
        return Err(metadata::invalid("UV extent budget"));
    }
    Ok(result)
}
#[derive(Clone, Copy)]
struct ElementRotation {
    origin: [f64; 3],
    axis: usize,
    degrees: f64,
    rescale: bool,
}
impl ElementRotation {
    fn parse(value: &Value) -> Result<Self> {
        let fields = object(value)?;
        allowed(fields, &["origin", "axis", "angle", "rescale"])?;
        let axis = match text(required(fields, "axis")?)? {
            "x" => 0,
            "y" => 1,
            "z" => 2,
            _ => return Err(metadata::invalid("rotation axis")),
        };
        let degrees = number(required(fields, "angle")?)?;
        if ![-45.0, -22.5, 0.0, 22.5, 45.0].contains(&degrees) {
            return Err(metadata::invalid("unsupported element rotation angle"));
        }
        Ok(Self {
            origin: triple(required(fields, "origin")?)?,
            axis,
            degrees,
            rescale: fields
                .get("rescale")
                .map(boolean)
                .transpose()?
                .unwrap_or(false),
        })
    }
    fn apply(self, point: [f64; 3]) -> [f64; 3] {
        let mut relative = std::array::from_fn(|i| point[i] - self.origin[i]);
        relative = rotate_axis(relative, self.axis, self.degrees.to_radians());
        if self.rescale {
            for (i, value) in relative.iter_mut().enumerate() {
                if i != self.axis {
                    *value /= self.degrees.to_radians().cos();
                }
            }
        }
        std::array::from_fn(|i| relative[i] + self.origin[i])
    }
}
pub(crate) fn rotate_axis(point: [f64; 3], axis: usize, radians: f64) -> [f64; 3] {
    let (s, c) = radians.sin_cos();
    let [x, y, z] = point;
    match axis {
        0 => [x, y * c - z * s, y * s + z * c],
        1 => [x * c + z * s, y, -x * s + z * c],
        _ => [x * c - y * s, x * s + y * c, z],
    }
}
fn to_world([x, y, z]: [f64; 3]) -> [f64; 3] {
    [x / 16.0, z / 16.0, y / 16.0]
}
fn face_points(face: Direction, [x0, y0, z0]: [f64; 3], [x1, y1, z1]: [f64; 3]) -> [[f64; 3]; 4] {
    match face {
        Direction::Down => [[x0, y0, z1], [x1, y0, z1], [x1, y0, z0], [x0, y0, z0]],
        Direction::Up => [[x0, y1, z0], [x1, y1, z0], [x1, y1, z1], [x0, y1, z1]],
        Direction::North => [[x1, y1, z0], [x0, y1, z0], [x0, y0, z0], [x1, y0, z0]],
        Direction::South => [[x0, y1, z1], [x1, y1, z1], [x1, y0, z1], [x0, y0, z1]],
        Direction::West => [[x0, y1, z0], [x0, y1, z1], [x0, y0, z1], [x0, y0, z0]],
        Direction::East => [[x1, y1, z1], [x1, y1, z0], [x1, y0, z0], [x1, y0, z1]],
    }
}
fn default_uv(face: Direction, from: [f64; 3], to: [f64; 3]) -> [f64; 4] {
    match face {
        Direction::Down => [from[0], 16.0 - to[2], to[0], 16.0 - from[2]],
        Direction::Up => [from[0], from[2], to[0], to[2]],
        Direction::North => [16.0 - to[0], 16.0 - to[1], 16.0 - from[0], 16.0 - from[1]],
        Direction::South => [from[0], 16.0 - to[1], to[0], 16.0 - from[1]],
        Direction::West => [from[2], 16.0 - to[1], to[2], 16.0 - from[1]],
        Direction::East => [16.0 - to[2], 16.0 - to[1], 16.0 - from[2], 16.0 - from[1]],
    }
}
fn geometric_normal(points: [[f64; 3]; 4]) -> Option<[f64; 3]> {
    let a: [f64; 3] = std::array::from_fn(|i| points[1][i] - points[0][i]);
    let b: [f64; 3] = std::array::from_fn(|i| points[2][i] - points[0][i]);
    let n = [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ];
    let length = (n.iter().map(|c| c * c).sum::<f64>()).sqrt();
    if length <= 1e-9 {
        return None;
    }
    Some(n.map(|c| c / length))
}
fn complete_boundary(points: [[f64; 3]; 4], normal: [f64; 3]) -> Option<Direction> {
    let face = Direction::from_java_normal(normal)?;
    let expected = face_points(face, [0.0; 3], [16.0; 3]);
    if !expected.iter().all(|e| {
        points
            .iter()
            .any(|p| (0..3).all(|i| (e[i] - p[i]).abs() < 1e-7))
    }) {
        return None;
    }
    Some(face)
}
/// Apply a blockstate's right-angle orientation without modifying the cached base model.
/// Rotation follows Java x then y, clockwise for positive blockstate angles.
pub fn oriented_quad(quad: &ModelQuad, application: &ModelApplication) -> Result<ModelQuad> {
    if application.x_turns > 3 || application.y_turns > 3 {
        return Err(metadata::invalid("unchecked model application rotation"));
    }
    let rotate = |point: [f64; 3]| {
        let java = [point[0], point[2], point[1]];
        let java = rotate_axis(
            java,
            0,
            -f64::from(application.x_turns) * std::f64::consts::FRAC_PI_2,
        );
        let java = rotate_axis(
            java,
            1,
            -f64::from(application.y_turns) * std::f64::consts::FRAC_PI_2,
        );
        [java[0], java[2], java[1]]
    };
    let mut output = quad.clone();
    output.points = quad
        .points
        .map(|p| rotate(p.map(|c| c - 0.5)).map(|c| c + 0.5));
    output.normal = rotate(quad.normal.map(f64::from)).map(|c| c as f32);
    let rotate_face = |face: Direction| {
        let normal = rotate(face.step().map(f64::from));
        Direction::from_java_normal([normal[0], normal[2], normal[1]])
    };
    output.complete_boundary = quad.complete_boundary.and_then(rotate_face);
    output.cull_face = quad.cull_face.and_then(rotate_face);
    if !application.uvlock || (application.x_turns == 0 && application.y_turns == 0) {
        // Keep identity UVs exact.
        return Ok(output);
    }
    // UV-lock follows the authored face chart under blockstate rotation only.
    // Element tilt/rescale already affects points and normals, never this chart.
    let target =
        rotate_face(quad.face) // Rotate the nominal face, not the physical normal.
            .ok_or_else(|| metadata::invalid("invalid UV-lock nominal face rotation"))?; // Preserve invariant errors.
    let source_basis = face_points(quad.face, [0.0; 3], [1.0; 3]);
    let target_basis = face_points(target, [0.0; 3], [1.0; 3]);
    let source_u = [
        source_basis[1][0] - source_basis[0][0],
        source_basis[1][2] - source_basis[0][2],
        source_basis[1][1] - source_basis[0][1],
    ];
    let source_v = [
        source_basis[3][0] - source_basis[0][0],
        source_basis[3][2] - source_basis[0][2],
        source_basis[3][1] - source_basis[0][1],
    ];
    let u = rotate(source_u);
    let v = rotate(source_v);
    let target_u = [
        target_basis[1][0] - target_basis[0][0],
        target_basis[1][2] - target_basis[0][2],
        target_basis[1][1] - target_basis[0][1],
    ];
    let target_v = [
        target_basis[3][0] - target_basis[0][0],
        target_basis[3][2] - target_basis[0][2],
        target_basis[3][1] - target_basis[0][1],
    ];
    let dot = |a: [f64; 3], b: [f64; 3]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f64>();
    output.uv = quad.uv.map(|[s, t]| {
        let (s, t) = (f64::from(s) - 0.5, f64::from(t) - 0.5);
        [
            (0.5 + s * dot(u, target_u) + t * dot(v, target_u)) as f32,
            (0.5 + s * dot(u, target_v) + t * dot(v, target_v)) as f32,
        ]
    });
    Ok(output)
}
