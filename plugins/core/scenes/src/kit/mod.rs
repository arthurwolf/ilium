//! The scene kit: the pieces of the scene engine every scene of this package builds on
//! (data-driven controls, the tone raster, the dither thresholds, the palette and the
//! `Scene` contract). They are copies of the kernel engine's modules, so a scene moved here
//! keeps its code; `lib.rs` re-exports the modules at the crate root (`crate::raster`,
//! `crate::control`, ...) to keep the scenes' paths unchanged.

pub mod control;
pub mod debug;
pub mod dither;
pub mod geo_projection;
pub mod geo_tiles;
pub mod raster;
pub mod scene;
pub mod settings;
pub mod style;
pub mod worldmap;
