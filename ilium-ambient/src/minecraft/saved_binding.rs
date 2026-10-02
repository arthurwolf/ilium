//! Worker-only exact saved-cell conversion and prepared geometry measurement.
use super::{projection, render_cells::RenderCells, tours::PreparedMap};
use crate::voxel_landscape::{
    assets::{block_state::BlockState, identity::ResourceId},
    surface_binding::PreparedSurface,
    surface_fluid::FluidMesh,
    surface_generation::{Region, SourceOwner, SurfaceBlock, SurfaceWorld},
    surface_mesh::{BoundQuad, PreparedMesh},
};
use std::{collections::BTreeMap, mem::size_of, sync::Arc};
const MAX_CELLS: usize = 1_000_000;
const MAX_COPY_BYTES: usize = 512 << 20;
const MAX_WORK: usize = 16_000_000;
// Portable conservative logical charges, not an allocator layout or RSS bound.
// Each entry includes its key/value sizes separately. The root charge covers
// small partially occupied property nodes, including one-property states.
const TREE_ENTRY_BYTES: usize = 128;
const PROPERTY_ROOT_BYTES: usize = 512;
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub cells: usize,
    pub owned_bytes: usize,
    pub work_units: usize,
    pub faces: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            cells: 1_000_000,
            owned_bytes: 256 << 20,
            work_units: 16_000_000,
            faces: 1_000_000,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prerequisite {
    FluidGeometryAndBiomeTint,
    WaterloggingGeometry,
    ExactSavedModelTint,
}
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid saved binding: {0}")]
    Invalid(&'static str),
    #[error("saved binding limit exceeded: {0}")]
    Limit(&'static str),
    #[error("saved binding cancelled")]
    Cancelled,
    #[error("saved cell {java_position:?} requires {prerequisite:?}")]
    Unsupported {
        java_position: [i32; 3],
        prerequisite: Prerequisite,
    },
    #[error("prepared saved geometry is empty")]
    EmptyGeometry,
    #[error("unsupported prepared geometry: {0}")]
    Geometry(&'static str),
    #[error(transparent)]
    Asset(#[from] crate::voxel_landscape::assets::AssetError),
}
pub struct SavedBinding {
    pub world: SurfaceWorld,
    pub map: Arc<PreparedMap>,
    pub heights: [i32; 2],
    pub storage_charge: usize,
    pub work_used: usize,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    pub ground_bounds: [[f64; 2]; 2],
    pub vertical_bounds: [f64; 2],
    pub model_overhang: f64,
    pub faces: usize,
    pub work_used: usize,
}
struct Work<'a> {
    used: usize,
    limit: usize,
    cancelled: &'a dyn Fn() -> bool,
}
impl Work<'_> {
    fn charge(&mut self, units: usize) -> Result<(), Error> {
        if (self.cancelled)() {
            return Err(Error::Cancelled);
        }
        self.used = self.used.checked_add(units).ok_or(Error::Limit("work"))?;
        if self.used > self.limit {
            return Err(Error::Limit("work"));
        }
        Ok(())
    }
}
fn validate_limits(limits: Limits) -> Result<(), Error> {
    if !(1..=MAX_CELLS).contains(&limits.cells)
        || !(size_of::<SavedBinding>()..=MAX_COPY_BYTES).contains(&limits.owned_bytes)
        || !(1..=MAX_WORK).contains(&limits.work_units)
        || !(1..=MAX_CELLS).contains(&limits.faces)
    {
        return Err(Error::Invalid("resource limits"));
    }
    Ok(())
}
fn add_bytes(total: &mut usize, bytes: usize, limit: usize) -> Result<(), Error> {
    *total = total
        .checked_add(bytes)
        .ok_or(Error::Limit("state copy bytes"))?;
    if *total > limit {
        return Err(Error::Limit("state copy bytes"));
    }
    Ok(())
}
fn raw_state(cells: &RenderCells, position: [i32; 3]) -> Result<&super::chunk::BlockState, Error> {
    if position[0] < cells.core.minimum[0]
        || position[0] > cells.core.maximum[0]
        || position[2] < cells.core.minimum[1]
        || position[2] > cells.core.maximum[1]
        || position[1] < cells.heights[0]
        || position[1] > cells.heights[1]
    {
        return Err(Error::Invalid("position outside admitted volume"));
    }
    let state = cells
        .map
        .state(position)
        .ok_or(Error::Invalid("missing exact saved state"))?;
    if state.is_air() {
        return Err(Error::Invalid("position refers to exact air"));
    }
    let prerequisite = if matches!(state.name.as_str(), "minecraft:water" | "minecraft:lava") {
        Some(Prerequisite::FluidGeometryAndBiomeTint)
    } else if state
        .properties
        .get("waterlogged")
        .is_some_and(|value| value == "true")
    {
        Some(Prerequisite::WaterloggingGeometry)
    } else {
        None
    };
    if let Some(prerequisite) = prerequisite {
        return Err(Error::Unsupported {
            java_position: position,
            prerequisite,
        });
    }
    Ok(state)
}
/// Copies only admitted exact states. The caller retains raw palettes on every
/// failure. This separate copy charge belongs inside the native Scene account;
/// RenderCells' position-vector charge is not spent a second time here.
/// No tint dependency can be inferred from a block name: the selected model's
/// normalized quads must pass prepare_supplied's saved-tint prerequisite guard.
pub fn prepare(
    cells: &RenderCells,
    limits: Limits,
    cancelled: &dyn Fn() -> bool,
) -> Result<SavedBinding, Error> {
    let mut work = Work {
        used: 0,
        limit: limits.work_units,
        cancelled,
    };
    work.charge(0)?;
    validate_limits(limits)?;
    if cells.positions.is_empty()
        || cells.heights[0] < -64
        || cells.heights[1] > 319
        || cells.heights[0] > cells.heights[1]
    {
        return Err(Error::Invalid("empty or invalid height band"));
    }
    let mut maximum = [0; 2];
    for (axis, output) in maximum.iter_mut().enumerate() {
        *output = cells.core.maximum[axis]
            .checked_add(1)
            .ok_or(Error::Invalid("exclusive region overflow"))?;
        let width = i64::from(*output) - i64::from(cells.core.minimum[axis]);
        if !(1..=256).contains(&width) {
            return Err(Error::Invalid("ground region extent"));
        }
    }
    if cells.positions.len() > limits.cells {
        return Err(Error::Limit("cells"));
    }
    // Complete preflight before allocating a native state/map. ResourceId's
    // private to_owned String has no public capacity accessor; charge decoded
    // name capacity plus its exact parse length. Property clones are charged
    // by source capacities plus BlockState::new's temporary extra key clone.
    let mut storage_charge = size_of::<SavedBinding>();
    add_bytes(
        &mut storage_charge,
        (size_of::<[i32; 3]>() + size_of::<SurfaceBlock>()) * 16 + PROPERTY_ROOT_BYTES,
        limits.owned_bytes,
    )?;
    for position in &cells.positions {
        work.charge(1)?;
        let state = raw_state(cells, *position)?;
        if state.name.len() > 512 || state.properties.len() > 32 {
            return Err(Error::Invalid("native state vocabulary limits"));
        }
        add_bytes(
            &mut storage_charge,
            size_of::<[i32; 3]>() + size_of::<SurfaceBlock>() + TREE_ENTRY_BYTES,
            limits.owned_bytes,
        )?;
        add_bytes(
            &mut storage_charge,
            state.name.capacity(),
            limits.owned_bytes,
        )?;
        add_bytes(&mut storage_charge, state.name.len(), limits.owned_bytes)?;
        work.charge(state.name.len())?;
        if !state.properties.is_empty() {
            add_bytes(&mut storage_charge, PROPERTY_ROOT_BYTES, limits.owned_bytes)?;
        }
        for (key, value) in &state.properties {
            add_bytes(
                &mut storage_charge,
                size_of::<(String, String)>() + TREE_ENTRY_BYTES,
                limits.owned_bytes,
            )?;
            for bytes in [key.capacity(), value.capacity(), key.len()] {
                add_bytes(&mut storage_charge, bytes, limits.owned_bytes)?;
            }
            work.charge(
                key.len()
                    .checked_add(value.len())
                    .ok_or(Error::Limit("work"))?,
            )?;
        }
    }
    let mut blocks = BTreeMap::new();
    for position in &cells.positions {
        work.charge(1)?;
        let raw = raw_state(cells, *position)?;
        let renderer_position = [position[0], position[2], position[1]];
        if blocks.contains_key(&renderer_position) {
            return Err(Error::Invalid("duplicate saved position"));
        }
        let id = ResourceId::parse(&raw.name)?;
        if id.as_str() != raw.name {
            return Err(Error::Invalid("native identifier changed saved name"));
        }
        let state = BlockState::new(
            id,
            raw.properties
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        )?;
        // Charge observable destination capacities if an implementation gives a
        // clone more spare capacity than its source; never publish excess copy.
        for (key, value) in state.properties() {
            let source = raw
                .properties
                .get_key_value(key)
                .ok_or(Error::Invalid("native property changed"))?;
            add_bytes(
                &mut storage_charge,
                key.capacity().saturating_sub(source.0.capacity()),
                limits.owned_bytes,
            )?;
            add_bytes(
                &mut storage_charge,
                value.capacity().saturating_sub(source.1.capacity()),
                limits.owned_bytes,
            )?;
        }
        blocks.insert(
            renderer_position,
            SurfaceBlock {
                state,
                owner: SourceOwner::Saved {
                    java_position: *position,
                },
            },
        );
    }
    work.charge(0)?;
    Ok(SavedBinding {
        world: SurfaceWorld {
            region: Region {
                minimum: cells.core.minimum,
                maximum,
            },
            // Required native variant-selection field, not a discovered world seed.
            seed: 0,
            columns: BTreeMap::new(),
            biomes: BTreeMap::new(),
            blocks,
            fluids: BTreeMap::new(),
            trees: Vec::new(),
            flora: Vec::new(),
            structures: Vec::new(),
            entities: Vec::new(),
            source_limitations: Vec::new(),
        },
        map: Arc::clone(&cells.map),
        heights: cells.heights,
        storage_charge,
        work_used: work.used,
    })
}

struct Extents {
    minimum: [f64; 3],
    maximum: [f64; 3],
    overhang: f64,
}
impl Extents {
    fn quad(
        &mut self,
        position: [i32; 3],
        quad: &BoundQuad,
        work: &mut Work<'_>,
    ) -> Result<(), Error> {
        work.charge(1)?;
        if quad.normal.iter().any(|value| !value.is_finite())
            || quad.uv.iter().flatten().any(|value| !value.is_finite())
            || quad.material.tint.iter().any(|value| !value.is_finite())
        {
            return Err(Error::Geometry("nonfinite quad attributes"));
        }
        for point in &quad.points {
            work.charge(3)?;
            for axis in 0..3 {
                let absolute = f64::from(position[axis]) + point[axis];
                if !point[axis].is_finite() || !absolute.is_finite() {
                    return Err(Error::Geometry("nonfinite vertex"));
                }
                self.minimum[axis] = self.minimum[axis].min(absolute);
                self.maximum[axis] = self.maximum[axis].max(absolute);
                if axis < 2 {
                    self.overhang = self.overhang.max(-point[axis]).max(point[axis] - 1.0);
                }
            }
        }
        Ok(())
    }
}
/// Measures all renderer-exposed solid and fluid vertices, using renderer
/// [Java x, Java z, Java y] axes. No guessed unit cube or surface height is used.
pub fn measure(
    mesh: &PreparedMesh,
    fluid: Option<&FluidMesh>,
    limits: Limits,
    cancelled: &dyn Fn() -> bool,
) -> Result<Geometry, Error> {
    let mut work = Work {
        used: 0,
        limit: limits.work_units,
        cancelled,
    };
    work.charge(0)?;
    validate_limits(limits)?;
    let faces = mesh
        .faces
        .len()
        .checked_add(fluid.map_or(0, |value| value.faces.len()))
        .ok_or(Error::Limit("faces"))?;
    if faces > limits.faces {
        return Err(Error::Limit("faces"));
    }
    if faces == 0 {
        return Err(Error::EmptyGeometry);
    }
    if fluid.is_some_and(|value| {
        value.bank != mesh.bank
            || value.region.minimum != mesh.region.minimum
            || value.region.maximum != mesh.region.maximum
    }) {
        return Err(Error::Geometry(
            "fluid mesh differs from solid bank or region",
        ));
    }
    let mut extents = Extents {
        minimum: [f64::INFINITY; 3],
        maximum: [f64::NEG_INFINITY; 3],
        overhang: 0.0,
    };
    for face in &mesh.faces {
        if face.model.bank != mesh.bank {
            return Err(Error::Geometry("model differs from mesh bank"));
        }
        let quad = face
            .model
            .quads
            .get(usize::from(face.quad_index))
            .ok_or(Error::Geometry("unknown model quad"))?;
        extents.quad(face.position, quad, &mut work)?;
    }
    if let Some(fluid) = fluid {
        for face in &fluid.faces {
            extents.quad(face.position, &face.quad, &mut work)?;
        }
    }
    work.charge(0)?;
    Ok(Geometry {
        ground_bounds: [
            [extents.minimum[0], extents.minimum[1]],
            [extents.maximum[0], extents.maximum[1]],
        ],
        vertical_bounds: [extents.minimum[2], extents.maximum[2]],
        model_overhang: extents.overhang,
        faces,
        work_used: work.used,
    })
}
pub fn measure_surface(
    surface: &PreparedSurface,
    limits: Limits,
    cancelled: &dyn Fn() -> bool,
) -> Result<Geometry, Error> {
    let mut work = Work {
        used: 0,
        limit: limits.work_units,
        cancelled,
    };
    work.charge(0)?;
    validate_limits(limits)?;
    if surface
        .world
        .blocks
        .len()
        .checked_add(surface.world.fluids.len())
        .is_none_or(|cells| cells > limits.cells)
    {
        return Err(Error::Limit("cells"));
    }
    if !surface.world.columns.is_empty()
        || !surface.world.biomes.is_empty()
        || !surface.world.trees.is_empty()
        || !surface.world.flora.is_empty()
        || !surface.world.structures.is_empty()
        || !surface.world.entities.is_empty()
        || !surface.entities.faces.is_empty()
        || surface.entities.rendered_entities != 0
    {
        return Err(Error::Geometry(
            "generated or entity geometry requires a separate exact saved adapter",
        ));
    }
    for (position, block) in &surface.world.blocks {
        work.charge(1)?;
        if !matches!(block.owner, SourceOwner::Saved { java_position } if [java_position[0], java_position[2], java_position[1]] == *position)
        {
            return Err(Error::Geometry("non-exact saved source owner"));
        }
    }
    let remaining = limits.work_units - work.used;
    if remaining == 0 {
        return Err(Error::Limit("work"));
    }
    let mut geometry = measure(
        &surface.mesh,
        surface.fluid.as_ref(),
        Limits {
            work_units: remaining,
            ..limits
        },
        cancelled,
    )?;
    geometry.work_used += work.used;
    Ok(geometry)
}
impl Geometry {
    pub fn framing(
        self,
        size: [usize; 2],
        scale: f64,
        camera_height: f64,
        neighbor_halo: f64,
    ) -> Result<projection::Framing, projection::InvalidFraming> {
        if !neighbor_halo.is_finite()
            || neighbor_halo < 0.0
            || !self.model_overhang.is_finite()
            || self.model_overhang < 0.0
        {
            return Err(projection::InvalidFraming);
        }
        let value = projection::Framing {
            size,
            scale,
            camera_height,
            vertical_bounds: self.vertical_bounds,
            horizontal_halo: self.model_overhang + neighbor_halo,
        };
        value.radius()?;
        Ok(value)
    }
}
#[cfg(test)]
#[path = "saved_binding_tests.rs"]
mod tests;
