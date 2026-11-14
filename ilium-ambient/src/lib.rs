//! Ambient background scenes for ilium: pure engines that draw into a Braille
//! dot raster. No terminal, no ratatui, no PTY. The client hosts them.
//!
//! Layering: this crate owns scene math, data fetching/decoding and helper
//! processes; the client owns the Settings UI, compositing and persistence.

pub mod animation_services;
pub mod control;
pub mod debug;
pub mod dither;
pub mod external;
pub mod geocode;
pub mod gpu;
pub mod live_data;
pub mod location;
pub mod minecraft;
pub mod plugin_scene;
pub mod prepared_cache;
pub mod raster;
pub mod registry;
pub mod resources;
pub mod scene;
mod scenes;
pub mod source;
pub mod style;
pub mod style_filters;
pub mod worldmap;

pub use control::{Control, ControlKind, ControlValue, SceneSettings};
pub use location::GeoLocation;
pub use raster::{DitherMode, Raster};
pub use registry::{AmbientKind, AmbientSettings};
pub use scene::{Frame, MessageScene, OccupancyMask, Scene, SceneEnv};
pub use scenes::spectrum::{
    native_pipewire_audio_command, native_pulse_audio_command, AudioFft, NativeAudioCommand,
    NativeAudioPcmDecoder, NativeAudioTarget,
};

pub use scenes::{
    clouds::CloudsSettings, images::ImagesSettings, night_lights::NightLightsSettings,
    spectrum::SpectrumSettings, stars::StarsSettings, video::VideoSettings,
};

// Pure world/renderer contracts support offline capture and acceptance probes.
pub use scenes::voxel_landscape;
pub use scenes::voxel_landscape::VoxelLandscapeSettings;

pub use scenes::topographic_maps::TopographicMapsSettings;

pub use scenes::openstreetmap::address_search as openstreetmap_address_search;
pub use scenes::openstreetmap::{
    AddressProvider, AddressSearchSettings, GeometryMap, OpenStreetMapSettings, SourceElement,
};
