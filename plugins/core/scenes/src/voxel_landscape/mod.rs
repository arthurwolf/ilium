//! Original surface world rendered through selected texture-pack models.
//!
//! Module list of the moved voxel landscape (it was `ilium-ambient/src/scenes/voxel_landscape/mod.rs`).
//! The files `surface_structures`, `surface_tint`, `surface_viewport`, `surface_village_assembly`,
//! `terrain`, `terrain_fields`, `tree_*`, `village_kit`, `wetland`, `workstation_regressions` and
//! `world` already live here; the remaining modules arrive with the rest of the move, and
//! `lib.rs` declares `mod voxel_landscape;` once all of them are present.
mod settings;
pub use settings::{VoxelLandscapeSettings, GENERATED_TEXTURE_SOURCE_JAVA_DEFAULT};

/// The block world takes its look from Minecraft; no Minecraft assets ship
/// with Ilium.
pub const INSPIRED_BY: &[&str] = &["https://www.minecraft.net/"];

/// Source and required-material evidence tied to the most recently painted saved-world frame.
/// This qualifies provenance and coverage only; it does not claim that the image is visually
/// recognizable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct SavedWorldFrameEvidence {
    pub bank_epoch: assets::identity::Digest256,
    /// Native base-asset profile; selected texture-pack identity is reported by
    /// `selected_archive_sha256` and is intentionally independent of this ID.
    pub source_profile: &'static str,
    pub native_archive_sha256: assets::identity::Digest256,
    pub selected_archive_sha256: Option<assets::identity::Digest256>,
    pub required_materials: usize,
    pub required_materials_satisfied: usize,
    pub visible_pixels: usize,
    /// Saved-tour corridor the frame was painted from, in quarter blocks (`RouteKey` units).
    /// `None` when the frame is not from a route.
    pub route_endpoints_quarter_blocks: Option<[[i64; 2]; 2]>,
}

impl SavedWorldFrameEvidence {
    /// Require actual visible output and complete, non-empty material coverage.
    pub fn is_qualified(&self) -> bool {
        self.visible_pixels > 0
            && self.required_materials > 0
            && self.required_materials_satisfied == self.required_materials
    }
}

pub mod assets;
pub mod catalog;
pub mod chunks;
mod color;
pub(crate) use color::composite_selected;
pub mod noise;
pub mod pack_profiles;
pub mod pack_registry;
pub mod pack_sources;
pub mod placement;
pub mod render;
pub mod surface_binding;
pub mod surface_biome_selector;
pub mod surface_biomes;
pub mod surface_camp_assembly;
pub mod surface_camps;
pub mod surface_context;
pub mod surface_entities;
pub mod surface_entity_binding;
pub mod surface_entity_raster;
pub mod surface_events;
pub mod surface_flora;
pub mod surface_flora_vocabulary;
pub mod surface_fluid;
pub mod surface_generation;
pub mod surface_geology;
pub mod surface_landmark_assembly;
pub mod surface_landmarks;
pub mod surface_mesh;
#[cfg(test)]
mod surface_pipeline_tests;
pub mod surface_raster;
mod surface_retirement;
pub(crate) use surface_retirement::Retirement;
pub mod surface_ruin_assembly;
pub mod surface_ruins;
pub mod surface_state_geometry;
pub mod surface_structures;
mod surface_tint;
mod surface_viewport;
pub mod surface_village_assembly;
pub mod terrain;
pub mod terrain_fields;
pub mod tree_decoration_profiles;
pub mod tree_decorations;
pub mod tree_forms;
pub mod tree_geometry;
pub mod tree_profiles;
pub mod village_kit;
pub mod world;

pub mod ecology;
pub mod features;
pub mod generation;
mod wetland;

#[cfg(test)]
mod settings_integration_tests;

pub mod engine;
pub use engine::{TextureSourceReceipt, VoxelLandscapeScene};
