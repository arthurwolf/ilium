//! Kind enumeration, aggregate settings and scene construction. This is the
//! only file the client needs to know about; scene modules stay private to it.

use crate::control::{Control, ControlValue, SceneSettings};
use crate::live_chess::{draw::ChessSettings, scene::ChessScene};
use crate::live_data::{
    graph::{GraphScene, GraphSettings},
    maps::{LiveMapScene, MapKind, MapSettings},
};
use crate::location::GeoLocation;
use crate::pi_digits::presentation::{PiScene, PiSettings};
use crate::scene::{Scene, SceneEnv};
use crate::scenes::{
    atlantic_dusk::{AtlanticDuskScene, AtlanticDuskSettings},
    box_machine::{BoxMachineScene, BoxMachineSettings},
    carpet::{CarpetScene, CarpetSettings},
    clouds::{CloudsScene, CloudsSettings},
    cube_clock::{CubeClockScene, CubeClockSettings},
    dither_water::{DitherWaterScene, DitherWaterSettings},
    dithered_waves::{DitheredWavesScene, DitheredWavesSettings},
    dithr_patterns::{DithrPatternsScene, DithrPatternsSettings},
    fbm_clouds::{FbmCloudsScene, FbmCloudsSettings},
    galactic_empires::{GalacticEmpiresScene, GalacticEmpiresSettings},
    hex_expedition::{HexExpeditionScene, HexExpeditionSettings},
    images::{ImagesScene, ImagesSettings},
    machine_screen::{MachineScreenScene, MachineScreenSettings},
    night_lights::{NightLightsScene, NightLightsSettings},
    openstreetmap::{OpenStreetMapScene, OpenStreetMapSettings},
    pipes::{PipesScene, PipesSettings},
    solar_system::{SolarSystemScene, SolarSystemSettings},
    spectrum::{SpectrumScene, SpectrumSettings},
    stars::{StarsScene, StarsSettings},
    topographic_maps::{TopographicMapsScene, TopographicMapsSettings},
    vector_td::{VectorTdScene, VectorTdSettings},
    video::{VideoScene, VideoSettings},
    voxel_landscape::{VoxelLandscapeScene, VoxelLandscapeSettings},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AmbientKind {
    Pipes,
    Stars,
    NightLights,
    Clouds,
    Video,
    Spectrum,
    Images,
    DitherWater,
    AtlanticDusk,
    CubeClock,
    BoxMachine,
    MachineScreen,
    FbmClouds,
    DitheredWaves,
    DithrPatterns,
    HexExpedition,
    VectorTd,
    GalacticEmpires,
    VoxelLandscape,
    SolarSystem,
    TopographicMaps,
    Graph,
    Pi,
    Earthquakes,
    Aircraft,
    Boats,
    Chess,
    OpenStreetMap,
    Carpet,
}

impl AmbientKind {
    pub const ALL: [Self; 29] = [
        Self::Pipes,
        Self::Stars,
        Self::NightLights,
        Self::Clouds,
        Self::Video,
        Self::Spectrum,
        Self::Images,
        Self::DitherWater,
        Self::AtlanticDusk,
        Self::CubeClock,
        Self::BoxMachine,
        Self::MachineScreen,
        Self::FbmClouds,
        Self::DitheredWaves,
        Self::DithrPatterns,
        Self::HexExpedition,
        Self::VectorTd,
        Self::GalacticEmpires,
        Self::VoxelLandscape,
        Self::SolarSystem,
        Self::TopographicMaps,
        Self::Graph,
        Self::Pi,
        Self::Earthquakes,
        Self::Aircraft,
        Self::Boats,
        Self::Chess,
        Self::OpenStreetMap,
        Self::Carpet,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::OpenStreetMap => "OpenStreetMap",
            Self::Carpet => "Carpet",
            Self::Pipes => "3D pipes",
            Self::Stars => "Stars overhead",
            Self::SolarSystem => "Solar system",
            Self::TopographicMaps => "Topographic maps",
            Self::Graph => "Live graphs",
            Self::Pi => "Digits of Pi",
            Self::Earthquakes => "Live earthquakes",
            Self::Aircraft => "Live aircraft",
            Self::Boats => "Live boats",
            Self::Chess => "Live chess",
            Self::GalacticEmpires => "Galactic empires",
            Self::NightLights => "Earth at night",
            Self::Clouds => "Satellite clouds",
            Self::Video => "Video",
            Self::Spectrum => "Audio spectrum",
            Self::Images => "Images",
            Self::DitherWater => "Dithered water",
            Self::AtlanticDusk => "Atlantic dusk",
            Self::CubeClock => "Cube clock",
            Self::BoxMachine => "Box machine",
            Self::MachineScreen => "Machine screen",
            Self::FbmClouds => "Dithered fBm clouds",
            Self::DitheredWaves => "Dithered waves",
            Self::DithrPatterns => "Dithr patterns",
            Self::HexExpedition => "Hex expedition",
            Self::VectorTd => "Vector TD",
            Self::VoxelLandscape => "Voxel landscape",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::OpenStreetMap => "Real OpenStreetMap streets, buildings, waterways, parks and railways around ten world places, drawn as Braille dots with fixed or panning cameras.",
            Self::Carpet => {
                "Isometric hatch lines lift over hidden moving spheres and tubes: mouse hunters, Snake, Life, legal chess, Lichess TV, a DVD ball, planets and civil clocks."
            }
            Self::Graph => "Public observations as Braille lines, bars or genuine OHLC candles, with selectable sources and time scales.",
            Self::Pi => "Exact Pi digits as terminal text or real-font Braille, with scrolling and separate hues for each digit.",
            Self::Earthquakes => "USGS events of every reported magnitude on a coastline map, with pulsing markers and magnitude labels.",
            Self::Aircraft => "OpenSky's reported airborne positions worldwide, with independent map and aircraft styling; anonymous updates every fifteen minutes.",
            Self::Boats => "Received AIS positions on a world coastline: broader OpenSeaFeed by default, with Finnish Digitraffic as an explicit alternative. Coverage is incomplete.",
            Self::Chess => "The featured Lichess TV game's actual positions with dithered piece silhouettes.",
            Self::TopographicMaps => "Contour maps of Earth, the Moon, Mars, Venus, Mercury, Ceres and fictional worlds from real elevation surveys, drawn as Braille dots on a slowly panning map or turning globe.",
            Self::VoxelLandscape => "A seeded isometric block world with forests, deserts, villages, caves and ravines, drifting past in monochrome or pastel dithering.",
            Self::GalacticEmpires => {
                "Procedural star empires expand along hyperlanes, negotiate, fight and unify while a slow camera circles the galaxy."
            }
            Self::Pipes => {
                "Dithered black-and-white pipes grow through 3D space, like the classic screensaver."
            }
            Self::SolarSystem => {
                "Eight planets orbit the Sun, with independent distance and size scales and simulated time."
            }
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
            Self::DitherWater => {
                "Bayer-dithered 1-bit water: drifting caustic bands, horizon fade and slow ripple rings."
            }
            Self::AtlanticDusk => {
                "Dithered sea and sky through a full day: drifting clouds, sinking sun, moon and stars."
            }
            Self::CubeClock => "A quiet clock beside a slowly turning dotted cube.",
            Self::BoxMachine => {
                "A generative machine of boxes and rails that slowly builds and rearranges itself."
            }
            Self::MachineScreen => {
                "A generative machine display of scanning patterns and glyph-like blocks."
            }
            Self::FbmClouds => {
                "Domain-warped noise clouds thresholded into one-bit dots (software, slow-mo by default)."
            }
            Self::DitheredWaves => {
                "Layered wave shader rendered on the CPU with ordered dithering (software, slow-mo by default)."
            }
            Self::DithrPatterns => {
                "Many dithr-style animated patterns to choose from, with selectable dither algorithms (software, slow-mo by default)."
            }
            Self::HexExpedition => {
                "An endless explorer's hex map, generated as you watch: jungle, savanna, desert, arctic and volcanic lands with animated water, trees, smoke, fires and lava, panned by a slow camera."
            }
            Self::VectorTd => {
                "A tower defense that plays itself: an AI builds, upgrades and unlocks glowing vector towers against waves of monsters, level after level, across several maps."
            }
        }
    }

    /// Where the design of this scene came from; shown only in the demo.
    pub fn inspired_by(self) -> &'static [&'static str] {
        match self {
            Self::DitherWater => crate::scenes::dither_water::INSPIRED_BY,
            Self::AtlanticDusk => crate::scenes::atlantic_dusk::INSPIRED_BY,
            Self::CubeClock => crate::scenes::cube_clock::INSPIRED_BY,
            Self::BoxMachine => crate::scenes::box_machine::INSPIRED_BY,
            Self::MachineScreen => crate::scenes::machine_screen::INSPIRED_BY,
            Self::FbmClouds => crate::scenes::fbm_clouds::INSPIRED_BY,
            Self::DitheredWaves => crate::scenes::dithered_waves::INSPIRED_BY,
            Self::DithrPatterns => crate::scenes::dithr_patterns::INSPIRED_BY,
            Self::HexExpedition => crate::scenes::hex_expedition::INSPIRED_BY,
            Self::VectorTd => crate::scenes::vector_td::INSPIRED_BY,
            _ => &[],
        }
    }

    /// Scenes with an optional GPU renderer (the `render_backend` row).
    pub fn has_gpu_backend(self) -> bool {
        matches!(
            self,
            Self::FbmClouds | Self::DitheredWaves | Self::DithrPatterns
        )
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
    pub pi: PiSettings,
    pub earthquakes: MapSettings,
    pub aircraft: MapSettings,
    pub boats: MapSettings,
    pub chess: ChessSettings,
    pub location: GeoLocation,
    pub galactic_empires: GalacticEmpiresSettings,
    pub pipes: PipesSettings,
    pub stars: StarsSettings,
    pub solar_system: SolarSystemSettings,
    pub topographic_maps: TopographicMapsSettings,
    pub openstreetmap: OpenStreetMapSettings,
    pub carpet: CarpetSettings,
    pub night_lights: NightLightsSettings,
    pub clouds: CloudsSettings,
    pub video: VideoSettings,
    pub spectrum: SpectrumSettings,
    pub images: ImagesSettings,
    pub dither_water: DitherWaterSettings,
    pub atlantic_dusk: AtlanticDuskSettings,
    pub cube_clock: CubeClockSettings,
    pub box_machine: BoxMachineSettings,
    pub machine_screen: MachineScreenSettings,
    pub fbm_clouds: FbmCloudsSettings,
    pub dithered_waves: DitheredWavesSettings,
    pub dithr_patterns: DithrPatternsSettings,
    pub hex_expedition: HexExpeditionSettings,
    pub vector_td: VectorTdSettings,
    pub voxel_landscape: VoxelLandscapeSettings,
}

impl AmbientSettings {
    pub fn normalized(&self) -> Self {
        Self {
            graph: self.graph.normalized(),
            pi: self.pi.normalized(),
            earthquakes: self.earthquakes.normalized(),
            aircraft: self.aircraft.normalized(),
            boats: self.boats.normalized(),
            chess: self.chess.normalized(),
            location: self.location.normalized(),
            galactic_empires: self.galactic_empires.normalized(),
            pipes: self.pipes.normalized(),
            stars: self.stars.normalized(),
            solar_system: self.solar_system.normalized(),
            topographic_maps: self.topographic_maps.normalized(),
            openstreetmap: self.openstreetmap.normalized(),
            carpet: self.carpet.normalized(),
            night_lights: self.night_lights.normalized(),
            clouds: self.clouds.normalized(),
            video: self.video.normalized(),
            spectrum: self.spectrum.normalized(),
            images: self.images.normalized(),
            dither_water: self.dither_water.normalized(),
            atlantic_dusk: self.atlantic_dusk.normalized(),
            cube_clock: self.cube_clock.normalized(),
            box_machine: self.box_machine.normalized(),
            machine_screen: self.machine_screen.normalized(),
            fbm_clouds: self.fbm_clouds.normalized(),
            dithered_waves: self.dithered_waves.normalized(),
            dithr_patterns: self.dithr_patterns.normalized(),
            hex_expedition: self.hex_expedition.normalized(),
            vector_td: self.vector_td.normalized(),
            voxel_landscape: self.voxel_landscape.normalized(),
        }
    }

    pub fn controls(&self, kind: AmbientKind) -> Vec<Control> {
        match kind {
            AmbientKind::Graph => self.graph.controls(),
            AmbientKind::Pi => self.pi.controls(),
            AmbientKind::Earthquakes => self.earthquakes.controls_for(MapKind::Earthquakes),
            AmbientKind::Aircraft => self.aircraft.controls_for(MapKind::Aircraft),
            AmbientKind::Boats => self.boats.controls_for(MapKind::Boats),
            AmbientKind::Chess => self.chess.controls(),
            AmbientKind::GalacticEmpires => self.galactic_empires.controls(),
            AmbientKind::Pipes => self.pipes.controls(),
            AmbientKind::Stars => self.stars.controls(),
            AmbientKind::SolarSystem => self.solar_system.controls(),
            AmbientKind::TopographicMaps => self.topographic_maps.controls(),
            AmbientKind::OpenStreetMap => self.openstreetmap.controls(),
            AmbientKind::Carpet => self.carpet.controls(),
            AmbientKind::NightLights => self.night_lights.controls(),
            AmbientKind::Clouds => self.clouds.controls(),
            AmbientKind::Video => self.video.controls(),
            AmbientKind::Spectrum => self.spectrum.controls(),
            AmbientKind::Images => self.images.controls(),
            AmbientKind::DitherWater => self.dither_water.controls(),
            AmbientKind::AtlanticDusk => self.atlantic_dusk.controls(),
            AmbientKind::CubeClock => self.cube_clock.controls(),
            AmbientKind::BoxMachine => self.box_machine.controls(),
            AmbientKind::MachineScreen => self.machine_screen.controls(),
            AmbientKind::FbmClouds => self.fbm_clouds.controls(),
            AmbientKind::DitheredWaves => self.dithered_waves.controls(),
            AmbientKind::DithrPatterns => self.dithr_patterns.controls(),
            AmbientKind::HexExpedition => self.hex_expedition.controls(),
            AmbientKind::VectorTd => self.vector_td.controls(),
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
            AmbientKind::Pi => self.pi.set_control(id, value),
            AmbientKind::Earthquakes => self.earthquakes.set_control(id, value),
            AmbientKind::Aircraft => self.aircraft.set_control(id, value),
            AmbientKind::Boats => self.boats.set_control(id, value),
            AmbientKind::Chess => self.chess.set_control(id, value),
            AmbientKind::GalacticEmpires => self.galactic_empires.set_control(id, value),
            AmbientKind::Pipes => self.pipes.set_control(id, value),
            AmbientKind::Stars => self.stars.set_control(id, value),
            AmbientKind::SolarSystem => self.solar_system.set_control(id, value),
            AmbientKind::TopographicMaps => self.topographic_maps.set_control(id, value),
            AmbientKind::OpenStreetMap => self.openstreetmap.set_control(id, value),
            AmbientKind::Carpet => self.carpet.set_control(id, value),
            AmbientKind::NightLights => self.night_lights.set_control(id, value),
            AmbientKind::Clouds => self.clouds.set_control(id, value),
            AmbientKind::Video => self.video.set_control(id, value),
            AmbientKind::Spectrum => self.spectrum.set_control(id, value),
            AmbientKind::Images => self.images.set_control(id, value),
            AmbientKind::DitherWater => self.dither_water.set_control(id, value),
            AmbientKind::AtlanticDusk => self.atlantic_dusk.set_control(id, value),
            AmbientKind::CubeClock => self.cube_clock.set_control(id, value),
            AmbientKind::BoxMachine => self.box_machine.set_control(id, value),
            AmbientKind::MachineScreen => self.machine_screen.set_control(id, value),
            AmbientKind::FbmClouds => self.fbm_clouds.set_control(id, value),
            AmbientKind::DitheredWaves => self.dithered_waves.set_control(id, value),
            AmbientKind::DithrPatterns => self.dithr_patterns.set_control(id, value),
            AmbientKind::HexExpedition => self.hex_expedition.set_control(id, value),
            AmbientKind::VectorTd => self.vector_td.set_control(id, value),
            AmbientKind::VoxelLandscape => self.voxel_landscape.set_control(id, value),
        }
    }

    /// Identity of everything that affects `kind`'s scene. A host rebuilds the
    /// scene exactly when this string changes (or the kind changes).
    pub fn scene_key(&self, kind: AmbientKind) -> String {
        let normalized = self.normalized();
        let settings = match kind {
            AmbientKind::Graph => serde_json::to_string(&normalized.graph),
            AmbientKind::Pi => serde_json::to_string(&normalized.pi),
            AmbientKind::Earthquakes => serde_json::to_string(&normalized.earthquakes),
            AmbientKind::Aircraft => serde_json::to_string(&normalized.aircraft),
            AmbientKind::Boats => serde_json::to_string(&normalized.boats),
            AmbientKind::Chess => serde_json::to_string(&normalized.chess),
            AmbientKind::GalacticEmpires => serde_json::to_string(&normalized.galactic_empires),
            AmbientKind::Pipes => serde_json::to_string(&normalized.pipes),
            AmbientKind::Stars => serde_json::to_string(&normalized.stars),
            AmbientKind::SolarSystem => serde_json::to_string(&normalized.solar_system),
            AmbientKind::TopographicMaps => serde_json::to_string(&normalized.topographic_maps),
            AmbientKind::OpenStreetMap => serde_json::to_string(&normalized.openstreetmap),
            AmbientKind::Carpet => serde_json::to_string(&normalized.carpet),
            AmbientKind::NightLights => serde_json::to_string(&normalized.night_lights),
            AmbientKind::Clouds => serde_json::to_string(&normalized.clouds),
            AmbientKind::Video => serde_json::to_string(&normalized.video),
            AmbientKind::Spectrum => serde_json::to_string(&normalized.spectrum),
            AmbientKind::Images => serde_json::to_string(&normalized.images),
            AmbientKind::DitherWater => serde_json::to_string(&normalized.dither_water),
            AmbientKind::AtlanticDusk => serde_json::to_string(&normalized.atlantic_dusk),
            AmbientKind::CubeClock => serde_json::to_string(&normalized.cube_clock),
            AmbientKind::BoxMachine => serde_json::to_string(&normalized.box_machine),
            AmbientKind::MachineScreen => serde_json::to_string(&normalized.machine_screen),
            AmbientKind::FbmClouds => serde_json::to_string(&normalized.fbm_clouds),
            AmbientKind::DitheredWaves => serde_json::to_string(&normalized.dithered_waves),
            AmbientKind::DithrPatterns => serde_json::to_string(&normalized.dithr_patterns),
            AmbientKind::HexExpedition => serde_json::to_string(&normalized.hex_expedition),
            AmbientKind::VectorTd => serde_json::to_string(&normalized.vector_td),
            AmbientKind::VoxelLandscape => serde_json::to_string(&normalized.voxel_landscape),
        }
        .unwrap_or_default();
        // The GPU scenes must be rebuilt when the background probe finishes, or
        // a scene built while it was still checking would never get a runner.
        let settings = if kind.has_gpu_backend() {
            let ready = matches!(
                crate::gpu::gpu_availability(),
                crate::gpu::GpuAvailability::Ready { .. }
            );
            format!("{settings}|gpu_ready={ready}")
        } else {
            settings
        };
        if kind.uses_location() {
            let location = serde_json::to_string(&normalized.location).unwrap_or_default();
            format!("{kind:?}|{settings}|{location}")
        } else {
            format!("{kind:?}|{settings}")
        }
    }

    pub fn create_scene(&self, kind: AmbientKind, env: &SceneEnv) -> Box<dyn Scene> {
        let settings = self.normalized();
        match kind {
            AmbientKind::Graph => Box::new(GraphScene::new(&settings.graph, env)),
            AmbientKind::Pi => Box::new(PiScene::new(settings.pi, env)),
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
            AmbientKind::Chess => Box::new(ChessScene::new(&settings.chess, env)),
            AmbientKind::Carpet => Box::new(CarpetScene::new(&settings.carpet, env)),
            AmbientKind::GalacticEmpires => {
                Box::new(GalacticEmpiresScene::new(&settings.galactic_empires, env))
            }
            AmbientKind::Pipes => Box::new(PipesScene::new(&settings.pipes, env)),
            AmbientKind::Stars => Box::new(StarsScene::new(&settings.stars, env)),
            AmbientKind::SolarSystem => {
                Box::new(SolarSystemScene::new(&settings.solar_system, env))
            }
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
            AmbientKind::Images => Box::new(ImagesScene::new(&settings.images, env)),
            AmbientKind::DitherWater => {
                Box::new(DitherWaterScene::new(&settings.dither_water, env))
            }
            AmbientKind::AtlanticDusk => {
                Box::new(AtlanticDuskScene::new(&settings.atlantic_dusk, env))
            }
            AmbientKind::CubeClock => Box::new(CubeClockScene::new(&settings.cube_clock, env)),
            AmbientKind::BoxMachine => Box::new(BoxMachineScene::new(&settings.box_machine, env)),
            AmbientKind::MachineScreen => {
                Box::new(MachineScreenScene::new(&settings.machine_screen, env))
            }
            AmbientKind::FbmClouds => Box::new(FbmCloudsScene::new(&settings.fbm_clouds, env)),
            AmbientKind::DitheredWaves => {
                Box::new(DitheredWavesScene::new(&settings.dithered_waves, env))
            }
            AmbientKind::DithrPatterns => {
                Box::new(DithrPatternsScene::new(&settings.dithr_patterns, env))
            }
            AmbientKind::HexExpedition => {
                Box::new(HexExpeditionScene::new(&settings.hex_expedition, env))
            }
            AmbientKind::VectorTd => Box::new(VectorTdScene::new(&settings.vector_td, env)),
            AmbientKind::VoxelLandscape => {
                Box::new(VoxelLandscapeScene::new(&settings.voxel_landscape, env))
            }
        }
    }
}
