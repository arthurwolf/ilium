//! Read-only saved Minecraft map decoding and tour preparation.
pub mod catalog;
pub mod chunk;
pub mod coverage;
pub mod discovery;
pub mod evidence;
pub mod fluids;
pub mod history_store;
pub mod history_writer;
pub mod index;
mod io;
pub mod loader;
pub mod native_assets;
pub mod native_binding;
pub mod native_biome;
pub mod native_block_colors;
pub mod native_builtin;
pub mod native_colormap;
pub mod native_fluid;
pub mod native_fluid_assembly;
pub mod native_render_layer;
pub mod native_shape;
pub mod native_swamp_noise;
pub mod native_tint;
pub mod nbt;
pub mod paint_owners;
pub mod pipeline;
pub mod preparation;
pub mod projection;
pub mod region;
pub mod render_cells;
pub mod saved_binding;
pub mod saved_runtime;
pub mod session_catalog;
pub mod settings;
pub mod surface;
pub mod tours;
pub mod windows;
pub mod worker;

#[cfg(test)]
mod decoder_tests;

#[cfg(test)]
mod surface_tests;

pub mod source_footprint;

pub mod sparse_cells;

pub mod projected_source;

pub mod projected_route;
pub mod saved_scene;
