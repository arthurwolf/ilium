//! Worker-only exact saved-cell conversion and prepared geometry measurement.
use super::{projection, render_cells::RenderCells, tours::PreparedMap};
use crate::voxel_landscape::{
    surface_binding::PreparedSurface, surface_fluid::FluidMesh, surface_generation::SurfaceWorld,
    surface_mesh::PreparedMesh,
};
use std::sync::Arc;
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
pub fn prepare(_: &RenderCells, _: Limits, _: &dyn Fn() -> bool) -> Result<SavedBinding, Error> {
    Err(Error::Invalid("not implemented"))
}
pub fn measure(
    _: &PreparedMesh,
    _: Option<&FluidMesh>,
    _: Limits,
    _: &dyn Fn() -> bool,
) -> Result<Geometry, Error> {
    Err(Error::Geometry("not implemented"))
}
pub fn measure_surface(
    surface: &PreparedSurface,
    limits: Limits,
    cancelled: &dyn Fn() -> bool,
) -> Result<Geometry, Error> {
    measure(&surface.mesh, surface.fluid.as_ref(), limits, cancelled)
}
impl Geometry {
    pub fn framing(
        self,
        size: [usize; 2],
        scale: f64,
        camera_height: f64,
        neighbor_halo: f64,
    ) -> Result<projection::Framing, projection::InvalidFraming> {
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
