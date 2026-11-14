//! Kind enumeration, aggregate settings and scene construction. This is the
//! only file the client needs to know about; scene modules stay private to it.

use crate::control::{Control, ControlValue, SceneSettings};
use crate::live_data::{
    graph::{GraphScene, GraphSettings},
    maps::{LiveMapScene, MapKind, MapSettings},
};
use crate::location::GeoLocation;
use crate::plugin_scene::PluginSceneSettings;
use crate::scene::{Scene, SceneEnv};
use crate::scenes::{
    clouds::{CloudsScene, CloudsSettings},
    images::{ImagesScene, ImagesSettings},
    night_lights::{NightLightsScene, NightLightsSettings},
    openstreetmap::{OpenStreetMapScene, OpenStreetMapSettings},
    spectrum::{SpectrumScene, SpectrumSettings},
    stars::{StarsScene, StarsSettings},
    topographic_maps::{TopographicMapsScene, TopographicMapsSettings},
    video::{VideoScene, VideoSettings},
    voxel_landscape::{VoxelLandscapeScene, VoxelLandscapeSettings},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AmbientKind {
    Stars,
    NightLights,
    Clouds,
    Video,
    Spectrum,
    Images,
    /// A scene supplied by a plugin; the Scene row picks which one.
    Plugin,
    VoxelLandscape,
    TopographicMaps,
    Graph,
    Earthquakes,
    Aircraft,
    Boats,
    OpenStreetMap,
}

impl AmbientKind {
    pub const ALL: [Self; 14] = [
        Self::Stars,
        Self::NightLights,
        Self::Clouds,
        Self::Video,
        Self::Spectrum,
        Self::Images,
        Self::Plugin,
        Self::VoxelLandscape,
        Self::TopographicMaps,
        Self::Graph,
        Self::Earthquakes,
        Self::Aircraft,
        Self::Boats,
        Self::OpenStreetMap,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::OpenStreetMap => "OpenStreetMap",
            Self::Stars => "Stars overhead",
            Self::TopographicMaps => "Topographic maps",
            Self::Graph => "Live graphs",
            Self::Earthquakes => "Live earthquakes",
            Self::Aircraft => "Live aircraft",
            Self::Boats => "Live boats",
            Self::NightLights => "Earth at night",
            Self::Clouds => "Satellite clouds",
            Self::Video => "Video",
            Self::Spectrum => "Audio spectrum",
            Self::Images => "Images",
            Self::Plugin => "Plugin scene",
            Self::VoxelLandscape => "Voxel landscape",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::OpenStreetMap => "Real OpenStreetMap streets, buildings, waterways, parks and railways around ten world places, drawn as Braille dots with fixed or panning cameras.",
            Self::Graph => "Public observations as Braille lines, bars or genuine OHLC candles, with selectable sources and time scales.",
            Self::Earthquakes => "USGS events of every reported magnitude on a coastline map, with pulsing markers and magnitude labels.",
            Self::Aircraft => "OpenSky's reported airborne positions worldwide, with independent map and aircraft styling; anonymous updates every fifteen minutes.",
            Self::Boats => "Received AIS positions on a world coastline: broader OpenSeaFeed by default, with Finnish Digitraffic as an explicit alternative. Coverage is incomplete.",
            Self::TopographicMaps => "Contour maps of Earth, the Moon, Mars, Venus, Mercury, Ceres and fictional worlds from real elevation surveys, drawn as Braille dots on a slowly panning map or turning globe.",
            Self::VoxelLandscape => "A seeded isometric block world with forests, deserts, villages, caves and ravines, drifting past in monochrome or pastel dithering.",
            Self::Stars => {
                "The real night sky above your location, right now, as a perfect star map."
            }
            Self::NightLights => {
                "City lights seen from orbit, on a borderless map of the dark Earth."
            }
            Self::Clouds => "Live weather-satellite clouds, global or over your location.",
            Self::Video => "Play video files, folders or URLs as dithered Braille.",
            Self::Spectrum => "A spectrum analyzer of whatever your system is playing.",
            Self::Images => {
                "Colored Braille images from files, folders or URLs, with slow pan and zoom."
            }
            Self::Plugin => {
                "A scene supplied by an enabled plugin, such as Dithered waves from the Scenes plugin. Pick the scene and set its own options; colour, dithering and display use the shared rows."
            }
        }
    }

    /// Where the design of this scene came from; shown only in the demo.
    pub fn inspired_by(self) -> &'static [&'static str] {
        match self {
            Self::VoxelLandscape => crate::scenes::voxel_landscape::INSPIRED_BY,
            // Classic screensaver look and the public sources each scene draws on.
            Self::OpenStreetMap => &["https://www.openstreetmap.org/copyright"],
            Self::Clouds | Self::NightLights => &["https://earthdata.nasa.gov/gibs"],
            Self::TopographicMaps => {
                &["https://www.ncei.noaa.gov/products/etopo-global-relief-model"]
            }
            _ => &[],
        }
    }

    /// Whether the shared observer location matters to this scene.
    pub fn uses_location(self) -> bool {
        matches!(self, Self::Stars | Self::NightLights | Self::Clouds)
    }

    /// Scenes whose output is not a pure function of elapsed time (network,
    /// child processes, wall clock, audio) cannot be cached as a loop.
    pub fn is_live_only(self) -> bool {
        true
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AmbientSettings {
    pub graph: GraphSettings,
    pub earthquakes: MapSettings,
    pub aircraft: MapSettings,
    pub boats: MapSettings,
    pub location: GeoLocation,
    pub stars: StarsSettings,
    pub topographic_maps: TopographicMapsSettings,
    pub openstreetmap: OpenStreetMapSettings,
    pub night_lights: NightLightsSettings,
    pub clouds: CloudsSettings,
    pub video: VideoSettings,
    pub spectrum: SpectrumSettings,
    pub images: ImagesSettings,
    pub plugin_scene: PluginSceneSettings,
    pub voxel_landscape: VoxelLandscapeSettings,
}

impl AmbientSettings {
    pub fn normalized(&self) -> Self {
        Self {
            graph: self.graph.normalized(),
            earthquakes: self.earthquakes.normalized(),
            aircraft: self.aircraft.normalized(),
            boats: self.boats.normalized(),
            location: self.location.normalized(),
            stars: self.stars.normalized(),
            topographic_maps: self.topographic_maps.normalized(),
            openstreetmap: self.openstreetmap.normalized(),
            night_lights: self.night_lights.normalized(),
            clouds: self.clouds.normalized(),
            video: self.video.normalized(),
            spectrum: self.spectrum.normalized(),
            images: self.images.normalized(),
            plugin_scene: self.plugin_scene.normalized(),
            voxel_landscape: self.voxel_landscape.normalized(),
        }
    }

    pub fn controls(&self, kind: AmbientKind) -> Vec<Control> {
        match kind {
            AmbientKind::Graph => self.graph.controls(),
            AmbientKind::Earthquakes => self.earthquakes.controls_for(MapKind::Earthquakes),
            AmbientKind::Aircraft => self.aircraft.controls_for(MapKind::Aircraft),
            AmbientKind::Boats => self.boats.controls_for(MapKind::Boats),
            AmbientKind::Stars => self.stars.controls(),
            AmbientKind::TopographicMaps => self.topographic_maps.controls(),
            AmbientKind::OpenStreetMap => self.openstreetmap.controls(),
            AmbientKind::NightLights => self.night_lights.controls(),
            AmbientKind::Clouds => self.clouds.controls(),
            AmbientKind::Video => self.video.controls(),
            AmbientKind::Spectrum => self.spectrum.controls(),
            AmbientKind::Images => self.images.controls(),
            AmbientKind::Plugin => self.plugin_scene.controls(),
            AmbientKind::VoxelLandscape => self.voxel_landscape.controls(),
        }
    }

    pub fn set_control(
        &mut self,
        kind: AmbientKind,
        id: &str,
        value: ControlValue,
    ) -> Result<bool, String> {
        match kind {
            AmbientKind::Graph => self.graph.set_control(id, value),
            AmbientKind::Earthquakes => self.earthquakes.set_control(id, value),
            AmbientKind::Aircraft => self.aircraft.set_control(id, value),
            AmbientKind::Boats => self.boats.set_control(id, value),
            AmbientKind::Stars => self.stars.set_control(id, value),
            AmbientKind::TopographicMaps => self.topographic_maps.set_control(id, value),
            AmbientKind::OpenStreetMap => self.openstreetmap.set_control(id, value),
            AmbientKind::NightLights => self.night_lights.set_control(id, value),
            AmbientKind::Clouds => self.clouds.set_control(id, value),
            AmbientKind::Video => self.video.set_control(id, value),
            AmbientKind::Spectrum => self.spectrum.set_control(id, value),
            AmbientKind::Images => self.images.set_control(id, value),
            AmbientKind::Plugin => self.plugin_scene.set_control(id, value),
            AmbientKind::VoxelLandscape => self.voxel_landscape.set_control(id, value),
        }
    }

    /// Identity of everything that affects `kind`'s scene. A host rebuilds the
    /// scene exactly when this string changes (or the kind changes).
    pub fn scene_key(&self, kind: AmbientKind) -> String {
        let normalized = self.normalized();
        let settings = match kind {
            AmbientKind::Graph => serde_json::to_string(&normalized.graph),
            AmbientKind::Earthquakes => serde_json::to_string(&normalized.earthquakes),
            AmbientKind::Aircraft => serde_json::to_string(&normalized.aircraft),
            AmbientKind::Boats => serde_json::to_string(&normalized.boats),
            AmbientKind::Stars => serde_json::to_string(&normalized.stars),
            AmbientKind::TopographicMaps => serde_json::to_string(&normalized.topographic_maps),
            AmbientKind::OpenStreetMap => serde_json::to_string(&normalized.openstreetmap),
            AmbientKind::NightLights => serde_json::to_string(&normalized.night_lights),
            AmbientKind::Clouds => serde_json::to_string(&normalized.clouds),
            AmbientKind::Video => serde_json::to_string(&normalized.video),
            AmbientKind::Spectrum => serde_json::to_string(&normalized.spectrum),
            AmbientKind::Images => serde_json::to_string(&normalized.images),
            AmbientKind::Plugin => Ok(normalized.plugin_scene.scene_key()),
            AmbientKind::VoxelLandscape => serde_json::to_string(&normalized.voxel_landscape),
        }
        .unwrap_or_default();
        if kind.uses_location() || kind == AmbientKind::Plugin {
            let location = serde_json::to_string(&normalized.location).unwrap_or_default();
            format!("{kind:?}|{settings}|{location}")
        } else {
            format!("{kind:?}|{settings}")
        }
    }

    pub fn create_scene(&self, kind: AmbientKind, env: &SceneEnv) -> Box<dyn Scene> {
        if kind == AmbientKind::Plugin {
            // The plugin scene registry already wraps the scene in `PaletteScene`.
            return self.create_raw_scene(kind, env);
        }
        Box::new(crate::scene::PaletteScene::new(
            self.create_raw_scene(kind, env),
            env.palette.clone(),
        ))
    }

    fn create_raw_scene(&self, kind: AmbientKind, env: &SceneEnv) -> Box<dyn Scene> {
        let settings = self.normalized();
        match kind {
            AmbientKind::Graph => Box::new(GraphScene::new(&settings.graph, env)),
            AmbientKind::Earthquakes => Box::new(LiveMapScene::new(
                MapKind::Earthquakes,
                &settings.earthquakes,
                env,
            )),
            AmbientKind::Aircraft => Box::new(LiveMapScene::new(
                MapKind::Aircraft,
                &settings.aircraft,
                env,
            )),
            AmbientKind::Boats => Box::new(LiveMapScene::new(MapKind::Boats, &settings.boats, env)),
            AmbientKind::Stars => Box::new(StarsScene::new(&settings.stars, env)),
            AmbientKind::TopographicMaps => {
                Box::new(TopographicMapsScene::new(&settings.topographic_maps, env))
            }
            AmbientKind::OpenStreetMap => {
                Box::new(OpenStreetMapScene::new(&settings.openstreetmap, env))
            }
            AmbientKind::NightLights => {
                Box::new(NightLightsScene::new(&settings.night_lights, env))
            }
            AmbientKind::Clouds => Box::new(CloudsScene::new(&settings.clouds, env)),
            AmbientKind::Video => Box::new(VideoScene::new(&settings.video, env)),
            AmbientKind::Spectrum => Box::new(SpectrumScene::new(&settings.spectrum, env)),
            AmbientKind::Images => match ImagesScene::new(&settings.images, env) {
                Ok(scene) => Box::new(scene),
                Err(crate::scenes::images::ImagesStartError::Admission(_)) => {
                    Box::new(crate::scene::MessageScene(
                        "Images: resource capacity unavailable; retry after pending work completes"
                            .to_owned(),
                    ))
                }
                Err(crate::scenes::images::ImagesStartError::Loader(_)) => {
                    Box::new(crate::scene::MessageScene(
                        "Images: background loader could not start".to_owned(),
                    ))
                }
            },
            AmbientKind::Plugin => settings.plugin_scene.create_scene(env),
            AmbientKind::VoxelLandscape => {
                if settings.voxel_landscape.saved_maps.source
                    == crate::minecraft::settings::WorldSource::SavedMaps
                {
                    Box::new(crate::minecraft::saved_scene::SavedScene::new(
                        &settings.voxel_landscape.saved_maps,
                        &settings.voxel_landscape,
                        env,
                    ))
                } else {
                    Box::new(VoxelLandscapeScene::new(&settings.voxel_landscape, env))
                }
            }
        }
    }
}

#[cfg(test)]
mod saved_map_tests {
    use super::*;

    #[test]
    fn saved_map_source_reaches_saved_scene_without_opening_a_world() {
        let mut settings = AmbientSettings::default();
        settings.voxel_landscape.saved_maps.source =
            crate::minecraft::settings::WorldSource::SavedMaps;
        settings.voxel_landscape.saved_maps.saves_folder = "relative/saves".into();
        let environment = crate::scene::SceneEnv::for_test(
            std::env::temp_dir(),
            crate::resources::test_resources(),
        );

        let scene = settings.create_scene(AmbientKind::VoxelLandscape, &environment);

        let status = scene
            .status()
            .expect("invalid saved-map folder should remain visible in the scene status");
        assert!(
            status.starts_with(
                "Saved maps folder must be an absolute path, or blank for automatic discovery"
            ),
            "saved-map validation should remain visible before live preparation details: {status}"
        );
    }
}
