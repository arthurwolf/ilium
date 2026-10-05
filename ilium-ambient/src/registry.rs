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
use crate::scenes::quiet::{
    almost_touching::{AlmostTouchingScene, AlmostTouchingSettings},
    aurora::{AuroraScene, AuroraSettings},
    crop_circles::{CropCirclesScene, CropCirclesSettings},
    delayed_reflection::{DelayedReflectionScene, DelayedReflectionSettings},
    embroidery::{EmbroideryScene, EmbroiderySettings},
    fireflies::{FirefliesScene, FirefliesSettings},
    frost::{FrostScene, FrostSettings},
    hesitating_ink::{HesitatingInkScene, HesitatingInkSettings},
    hidden_wheel::{HiddenWheelScene, HiddenWheelSettings},
    lighthouse::{LighthouseScene, LighthouseSettings},
    needle_threads::{NeedleThreadsScene, NeedleThreadsSettings},
    paper_fold::{PaperFoldScene, PaperFoldSettings},
    pollen::{PollenScene, PollenSettings},
    prime_constellations::{PrimeConstellationsScene, PrimeConstellationsSettings},
    unfinished_circle::{UnfinishedCircleScene, UnfinishedCircleSettings},
    wallpaper::{WallpaperScene, WallpaperSettings},
    window_sunlight::{WindowSunlightScene, WindowSunlightSettings},
};
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
    wind::{WindScene, WindSettings},
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
    Wind,
    Aurora,
    Pollen,
    Fireflies,
    WindowSunlight,
    Frost,
    Lighthouse,
    PaperFold,
    Embroidery,
    PrimeConstellations,
    Wallpaper,
    UnfinishedCircle,
    NeedleThreads,
    HesitatingInk,
    CropCircles,
    DelayedReflection,
    AlmostTouching,
    HiddenWheel,
}

impl AmbientKind {
    pub const ALL: [Self; 47] = [
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
        Self::Wind,
        Self::Aurora,
        Self::Pollen,
        Self::Fireflies,
        Self::WindowSunlight,
        Self::Frost,
        Self::Lighthouse,
        Self::PaperFold,
        Self::Embroidery,
        Self::PrimeConstellations,
        Self::Wallpaper,
        Self::UnfinishedCircle,
        Self::NeedleThreads,
        Self::HesitatingInk,
        Self::CropCircles,
        Self::DelayedReflection,
        Self::AlmostTouching,
        Self::HiddenWheel,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Aurora => "Northern lights",
            Self::Pollen => "Pollen in a sunbeam",
            Self::Fireflies => "Fireflies finding a rhythm",
            Self::WindowSunlight => "Window sunlight",
            Self::Frost => "Frost",
            Self::Lighthouse => "Distant lighthouse",
            Self::PaperFold => "Paper-fold trace",
            Self::Embroidery => "Embroidery orbit",
            Self::PrimeConstellations => "Prime constellations",
            Self::Wallpaper => "Turning wallpaper",
            Self::UnfinishedCircle => "Unfinished circle",
            Self::NeedleThreads => "Threads through a needle",
            Self::HesitatingInk => "Ink that hesitates",
            Self::CropCircles => "Crop circles",
            Self::DelayedReflection => "Delayed reflection",
            Self::AlmostTouching => "Almost touching",
            Self::HiddenWheel => "Hidden wheel",
            Self::OpenStreetMap => "OpenStreetMap",
            Self::Carpet => "Carpet",
            Self::Wind => "Wind",
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
            Self::Aurora => "Luminous curtains sway above a dark horizon with adjustable hills and optional seeded trees.",
            Self::Pollen => "Small drifting specks brighten only inside a slowly swaying shaft of sunlight.",
            Self::Fireflies => "Seeded wandering lights gradually gather into a common pulse and drift out of agreement.",
            Self::WindowSunlight => "One or several sheared window projections move across the field, with separate 2 x 2 or 2 x 3 panes and optional pollen.",
            Self::Frost => "Fine branching ice grows and retreats around the screen edges or around the foreground character mask.",
            Self::Lighthouse => "A small dark lighthouse sweeps its light over short shimmering marks on the sea.",
            Self::PaperFold => "An angular dragon-curve trace gradually folds and opens, pausing between movements.",
            Self::Embroidery => "A moving stitch progressively reveals a delicate geometric flower.",
            Self::PrimeConstellations => "A slow illumination sweep reveals prime-number alignments on an Ulam spiral.",
            Self::Wallpaper => "Repeated geometric motifs rotate into temporary larger shapes.",
            Self::UnfinishedCircle => "Imperfect concentric arcs slowly turn and occasionally align their wandering gaps.",
            Self::NeedleThreads => "Drifting curved threads gather through one narrow opening before fanning apart.",
            Self::HesitatingInk => "A gently curling stroke pauses, resumes, fades and begins again.",
            Self::CropCircles => "Several visible drawers progressively trace bounded geometric formations across a textured field.",
            Self::DelayedReflection => "A swaying curve has a reflected partner that follows slightly behind, with gentle distortion.",
            Self::AlmostTouching => "Two arcs approach, linger near one another and retreat without meeting.",
            Self::HiddenWheel => "Orbiting dashes briefly light up to imply a wheel whose rim is never drawn.",
            Self::OpenStreetMap => "Real OpenStreetMap streets, buildings, waterways, parks and railways around ten world places, drawn as Braille dots with fixed or panning cameras.",
            Self::Wind => {
                "Dots blown by a fixed or rotating wind through the empty parts of your screen; scrolling and new text push them around."
            }
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
            Self::PaperFold => &["https://thecodingtrain.com/challenges"],
            Self::Embroidery => &["https://thecodingtrain.com/challenges"],
            Self::PrimeConstellations => &["https://thecodingtrain.com/challenges"],
            Self::Wallpaper => &["https://genuary.art/prompts"],
            Self::CropCircles => &["https://en.wikipedia.org/wiki/Crop_circle"],
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
            Self::VoxelLandscape => crate::scenes::voxel_landscape::INSPIRED_BY,
            // Classic screensaver look and the public sources each scene draws on.
            Self::Pipes => &["https://en.wikipedia.org/wiki/3D_Pipes"],
            Self::Chess => &["https://lichess.org/tv"],
            Self::OpenStreetMap => &["https://www.openstreetmap.org/copyright"],
            Self::Clouds | Self::NightLights => &["https://earthdata.nasa.gov/gibs"],
            Self::SolarSystem => &["https://ssd.jpl.nasa.gov/planets/approx_pos.html"],
            Self::TopographicMaps => {
                &["https://www.ncei.noaa.gov/products/etopo-global-relief-model"]
            }
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
    pub wind: WindSettings,
    pub aurora: AuroraSettings,
    pub pollen: PollenSettings,
    pub fireflies: FirefliesSettings,
    pub window_sunlight: WindowSunlightSettings,
    pub frost: FrostSettings,
    pub lighthouse: LighthouseSettings,
    pub paper_fold: PaperFoldSettings,
    pub embroidery: EmbroiderySettings,
    pub prime_constellations: PrimeConstellationsSettings,
    pub wallpaper: WallpaperSettings,
    pub unfinished_circle: UnfinishedCircleSettings,
    pub needle_threads: NeedleThreadsSettings,
    pub hesitating_ink: HesitatingInkSettings,
    pub crop_circles: CropCirclesSettings,
    pub delayed_reflection: DelayedReflectionSettings,
    pub almost_touching: AlmostTouchingSettings,
    pub hidden_wheel: HiddenWheelSettings,
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
            wind: self.wind.normalized(),
            aurora: self.aurora.normalized(),
            pollen: self.pollen.normalized(),
            fireflies: self.fireflies.normalized(),
            window_sunlight: self.window_sunlight.normalized(),
            frost: self.frost.normalized(),
            lighthouse: self.lighthouse.normalized(),
            paper_fold: self.paper_fold.normalized(),
            embroidery: self.embroidery.normalized(),
            prime_constellations: self.prime_constellations.normalized(),
            wallpaper: self.wallpaper.normalized(),
            unfinished_circle: self.unfinished_circle.normalized(),
            needle_threads: self.needle_threads.normalized(),
            hesitating_ink: self.hesitating_ink.normalized(),
            crop_circles: self.crop_circles.normalized(),
            delayed_reflection: self.delayed_reflection.normalized(),
            almost_touching: self.almost_touching.normalized(),
            hidden_wheel: self.hidden_wheel.normalized(),
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
            AmbientKind::Wind => self.wind.controls(),
            AmbientKind::Aurora => self.aurora.controls(),
            AmbientKind::Pollen => self.pollen.controls(),
            AmbientKind::Fireflies => self.fireflies.controls(),
            AmbientKind::WindowSunlight => self.window_sunlight.controls(),
            AmbientKind::Frost => self.frost.controls(),
            AmbientKind::Lighthouse => self.lighthouse.controls(),
            AmbientKind::PaperFold => self.paper_fold.controls(),
            AmbientKind::Embroidery => self.embroidery.controls(),
            AmbientKind::PrimeConstellations => self.prime_constellations.controls(),
            AmbientKind::Wallpaper => self.wallpaper.controls(),
            AmbientKind::UnfinishedCircle => self.unfinished_circle.controls(),
            AmbientKind::NeedleThreads => self.needle_threads.controls(),
            AmbientKind::HesitatingInk => self.hesitating_ink.controls(),
            AmbientKind::CropCircles => self.crop_circles.controls(),
            AmbientKind::DelayedReflection => self.delayed_reflection.controls(),
            AmbientKind::AlmostTouching => self.almost_touching.controls(),
            AmbientKind::HiddenWheel => self.hidden_wheel.controls(),
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
            AmbientKind::Wind => self.wind.set_control(id, value),
            AmbientKind::Aurora => self.aurora.set_control(id, value),
            AmbientKind::Pollen => self.pollen.set_control(id, value),
            AmbientKind::Fireflies => self.fireflies.set_control(id, value),
            AmbientKind::WindowSunlight => self.window_sunlight.set_control(id, value),
            AmbientKind::Frost => self.frost.set_control(id, value),
            AmbientKind::Lighthouse => self.lighthouse.set_control(id, value),
            AmbientKind::PaperFold => self.paper_fold.set_control(id, value),
            AmbientKind::Embroidery => self.embroidery.set_control(id, value),
            AmbientKind::PrimeConstellations => self.prime_constellations.set_control(id, value),
            AmbientKind::Wallpaper => self.wallpaper.set_control(id, value),
            AmbientKind::UnfinishedCircle => self.unfinished_circle.set_control(id, value),
            AmbientKind::NeedleThreads => self.needle_threads.set_control(id, value),
            AmbientKind::HesitatingInk => self.hesitating_ink.set_control(id, value),
            AmbientKind::CropCircles => self.crop_circles.set_control(id, value),
            AmbientKind::DelayedReflection => self.delayed_reflection.set_control(id, value),
            AmbientKind::AlmostTouching => self.almost_touching.set_control(id, value),
            AmbientKind::HiddenWheel => self.hidden_wheel.set_control(id, value),
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
            AmbientKind::Wind => serde_json::to_string(&normalized.wind),
            AmbientKind::Aurora => serde_json::to_string(&normalized.aurora),
            AmbientKind::Pollen => serde_json::to_string(&normalized.pollen),
            AmbientKind::Fireflies => serde_json::to_string(&normalized.fireflies),
            AmbientKind::WindowSunlight => serde_json::to_string(&normalized.window_sunlight),
            AmbientKind::Frost => serde_json::to_string(&normalized.frost),
            AmbientKind::Lighthouse => serde_json::to_string(&normalized.lighthouse),
            AmbientKind::PaperFold => serde_json::to_string(&normalized.paper_fold),
            AmbientKind::Embroidery => serde_json::to_string(&normalized.embroidery),
            AmbientKind::PrimeConstellations => {
                serde_json::to_string(&normalized.prime_constellations)
            }
            AmbientKind::Wallpaper => serde_json::to_string(&normalized.wallpaper),
            AmbientKind::UnfinishedCircle => serde_json::to_string(&normalized.unfinished_circle),
            AmbientKind::NeedleThreads => serde_json::to_string(&normalized.needle_threads),
            AmbientKind::HesitatingInk => serde_json::to_string(&normalized.hesitating_ink),
            AmbientKind::CropCircles => serde_json::to_string(&normalized.crop_circles),
            AmbientKind::DelayedReflection => serde_json::to_string(&normalized.delayed_reflection),
            AmbientKind::AlmostTouching => serde_json::to_string(&normalized.almost_touching),
            AmbientKind::HiddenWheel => serde_json::to_string(&normalized.hidden_wheel),
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
        Box::new(crate::scene::PaletteScene::new(
            self.create_raw_scene(kind, env),
            env.palette.clone(),
        ))
    }

    fn create_raw_scene(&self, kind: AmbientKind, env: &SceneEnv) -> Box<dyn Scene> {
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
            AmbientKind::Wind => Box::new(WindScene::new(&settings.wind, env)),
            AmbientKind::Aurora => Box::new(AuroraScene::new(&settings.aurora, env)),
            AmbientKind::Pollen => Box::new(PollenScene::new(&settings.pollen, env)),
            AmbientKind::Fireflies => Box::new(FirefliesScene::new(&settings.fireflies, env)),
            AmbientKind::WindowSunlight => {
                Box::new(WindowSunlightScene::new(&settings.window_sunlight, env))
            }
            AmbientKind::Frost => Box::new(FrostScene::new(&settings.frost, env)),
            AmbientKind::Lighthouse => Box::new(LighthouseScene::new(&settings.lighthouse, env)),
            AmbientKind::PaperFold => Box::new(PaperFoldScene::new(&settings.paper_fold, env)),
            AmbientKind::Embroidery => Box::new(EmbroideryScene::new(&settings.embroidery, env)),
            AmbientKind::PrimeConstellations => Box::new(PrimeConstellationsScene::new(
                &settings.prime_constellations,
                env,
            )),
            AmbientKind::Wallpaper => Box::new(WallpaperScene::new(&settings.wallpaper, env)),
            AmbientKind::UnfinishedCircle => {
                Box::new(UnfinishedCircleScene::new(&settings.unfinished_circle, env))
            }
            AmbientKind::NeedleThreads => {
                Box::new(NeedleThreadsScene::new(&settings.needle_threads, env))
            }
            AmbientKind::HesitatingInk => {
                Box::new(HesitatingInkScene::new(&settings.hesitating_ink, env))
            }
            AmbientKind::CropCircles => {
                Box::new(CropCirclesScene::new(&settings.crop_circles, env))
            }
            AmbientKind::DelayedReflection => Box::new(DelayedReflectionScene::new(
                &settings.delayed_reflection,
                env,
            )),
            AmbientKind::AlmostTouching => {
                Box::new(AlmostTouchingScene::new(&settings.almost_touching, env))
            }
            AmbientKind::HiddenWheel => {
                Box::new(HiddenWheelScene::new(&settings.hidden_wheel, env))
            }
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
mod quiet_catalog_tests {
    use super::*;
    use crate::control::ControlKind;
    use std::collections::HashSet;

    const QUIET_KINDS: [AmbientKind; 17] = [
        AmbientKind::Aurora,
        AmbientKind::Pollen,
        AmbientKind::Fireflies,
        AmbientKind::WindowSunlight,
        AmbientKind::Frost,
        AmbientKind::Lighthouse,
        AmbientKind::PaperFold,
        AmbientKind::Embroidery,
        AmbientKind::PrimeConstellations,
        AmbientKind::Wallpaper,
        AmbientKind::UnfinishedCircle,
        AmbientKind::NeedleThreads,
        AmbientKind::HesitatingInk,
        AmbientKind::CropCircles,
        AmbientKind::DelayedReflection,
        AmbientKind::AlmostTouching,
        AmbientKind::HiddenWheel,
    ];

    #[test]
    fn seventeen_distinct_entries_append_without_reordering_existing_kinds() {
        assert_eq!(&AmbientKind::ALL[30..], &QUIET_KINDS);
        let unique: HashSet<_> = AmbientKind::ALL.into_iter().collect();
        assert_eq!(unique.len(), AmbientKind::ALL.len());
        for kind in QUIET_KINDS {
            assert!(!kind.label().is_empty());
            assert!(!kind.description().is_empty());
            let json = serde_json::to_string(&kind).unwrap();
            assert_eq!(serde_json::from_str::<AmbientKind>(&json).unwrap(), kind);
        }
    }

    #[test]
    fn every_quiet_control_roundtrips_and_one_scene_edit_is_isolated() {
        let original = AmbientSettings::default().normalized();
        for kind in QUIET_KINDS {
            let controls = original.controls(kind);
            assert!(!controls.is_empty(), "{kind:?}");
            let mut ids = HashSet::new();
            for row in &controls {
                assert!(ids.insert(row.id), "duplicate {kind:?}: {}", row.id);
                assert!(!row.help.is_empty(), "{kind:?}: {}", row.id);
                let mut unchanged = original.clone();
                assert_eq!(
                    unchanged.set_control(kind, row.id, row.value.clone()),
                    Ok(false)
                );
                assert_eq!(unchanged, original);
            }
            // Change a concrete numeric control through the production dispatch,
            // then reload the complete settings and check every other scene key.
            let row = controls
                .iter()
                .find(|row| matches!(row.kind, ControlKind::Slider { .. }))
                .unwrap();
            let ControlKind::Slider { min, max, .. } = row.kind else {
                unreachable!()
            };
            let ControlValue::Number(current) = row.value else {
                unreachable!()
            };
            let next = if current == min { max } else { min };
            let mut changed = original.clone();
            assert_eq!(
                changed.set_control(kind, row.id, ControlValue::Number(next)),
                Ok(true)
            );
            assert_ne!(changed.scene_key(kind), original.scene_key(kind));
            let saved = serde_json::to_string(&changed).unwrap();
            let restored: AmbientSettings = serde_json::from_str(&saved).unwrap();
            assert_eq!(restored, changed);
            assert_eq!(restored.controls(kind), changed.controls(kind));
            for other in AmbientKind::ALL {
                if other != kind {
                    assert_eq!(
                        restored.scene_key(other),
                        original.scene_key(other),
                        "{kind:?} changed {other:?}"
                    );
                }
            }
        }
    }

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

        assert_eq!(
            scene.status().as_deref(),
            Some("Saved maps folder must be an absolute path, or blank for automatic discovery")
        );
    }
}
