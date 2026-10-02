//! Original surface block world, rendered as isometric two-dimensional tiles.
mod settings;
pub use settings::VoxelLandscapeSettings;

pub mod catalog;
pub mod chunks;
mod color;
pub mod noise;
pub mod placement;
pub mod render;
pub mod terrain;
pub mod world;

pub mod ecology;
pub mod features;
pub mod generation;
mod wetland;

pub mod engine;
pub use engine::VoxelLandscapeScene;
