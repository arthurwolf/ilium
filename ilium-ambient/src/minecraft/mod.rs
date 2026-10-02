//! Read-only saved Minecraft map decoding and tour preparation.
pub mod catalog;
pub mod chunk;
pub mod coverage;
pub mod discovery;
pub mod evidence;
pub mod history_store;
pub mod history_writer;
pub mod index;
mod io;
pub mod loader;
pub mod native_colormap;
pub mod nbt;
pub mod paint_owners;
pub mod pipeline;
pub mod preparation;
pub mod projection;
pub mod region;
pub mod render_cells;
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
