//! Original surface world rendered through selected texture-pack models.
mod settings;
pub use settings::{VoxelLandscapeSettings, GENERATED_TEXTURE_SOURCE_JAVA_DEFAULT};

/// The block world takes its look from Minecraft; no Minecraft assets ship
/// with Ilium.
pub const INSPIRED_BY: &[&str] = &["https://www.minecraft.net/"];

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

pub mod engine;
pub use engine::{TextureSourceReceipt, VoxelLandscapeScene};
