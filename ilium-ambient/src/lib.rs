//! Ambient background scenes for ilium: pure engines that draw into a Braille
//! dot raster. No terminal, no ratatui, no PTY. The client hosts them.
//!
//! Layering: this crate owns scene math, data fetching/decoding and helper
//! processes; the client owns the Settings UI, compositing and persistence.

pub mod control;
pub mod debug;
pub mod dither;
pub mod geocode;
pub mod gpu;
pub mod live_chess;
pub mod live_data;
pub mod location;
pub mod pi_digits;
pub mod raster;
pub mod registry;
pub mod scene;
mod scenes;
pub mod source;
pub mod style;
pub mod worldmap;

pub use control::{Control, ControlKind, ControlValue, SceneSettings};
pub use location::GeoLocation;
pub use raster::{DitherMode, Raster};
pub use registry::{AmbientKind, AmbientSettings};
pub use scene::{Frame, MessageScene, Scene, SceneEnv};

pub use scenes::{
    atlantic_dusk::AtlanticDuskSettings, box_machine::BoxMachineSettings, clouds::CloudsSettings,
    cube_clock::CubeClockSettings, dither_water::DitherWaterSettings,
    dithered_waves::DitheredWavesSettings, dithr_patterns::DithrPatternsSettings,
    fbm_clouds::FbmCloudsSettings, hex_expedition::HexExpeditionSettings, images::ImagesSettings,
    machine_screen::MachineScreenSettings, night_lights::NightLightsSettings, pipes::PipesSettings,
    solar_system::SolarSystemSettings, spectrum::SpectrumSettings, stars::StarsSettings,
    video::VideoSettings,
};

pub use scenes::vector_td::VectorTdSettings;
// Pure world/renderer contracts support offline capture and acceptance probes.
pub use scenes::voxel_landscape;
pub use scenes::voxel_landscape::VoxelLandscapeSettings;

pub use scenes::galactic_empires::GalacticEmpiresSettings;
pub use scenes::topographic_maps::TopographicMapsSettings;

pub use scenes::openstreetmap::{GeometryMap, OpenStreetMapSettings, SourceElement};
