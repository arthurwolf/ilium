//! Kind enumeration, aggregate settings and scene construction. This is the
//! only file the client needs to know about; scene modules stay private to it.

use crate::control::{Control, ControlValue, SceneSettings};
use crate::location::GeoLocation;
use crate::scene::{Scene, SceneEnv};
use crate::scenes::{
    atlantic_dusk::{AtlanticDuskScene, AtlanticDuskSettings},
    box_machine::{BoxMachineScene, BoxMachineSettings},
    clouds::{CloudsScene, CloudsSettings},
    cube_clock::{CubeClockScene, CubeClockSettings},
    dither_water::{DitherWaterScene, DitherWaterSettings},
    dithered_waves::{DitheredWavesScene, DitheredWavesSettings},
    dithr_patterns::{DithrPatternsScene, DithrPatternsSettings},
    fbm_clouds::{FbmCloudsScene, FbmCloudsSettings},
    images::{ImagesScene, ImagesSettings},
    machine_screen::{MachineScreenScene, MachineScreenSettings},
    night_lights::{NightLightsScene, NightLightsSettings},
    pipes::{PipesScene, PipesSettings},
    solar_system::{SolarSystemScene, SolarSystemSettings},
    spectrum::{SpectrumScene, SpectrumSettings},
    stars::{StarsScene, StarsSettings},
    video::{VideoScene, VideoSettings},
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
    SolarSystem,
}

impl AmbientKind {
    pub const ALL: [Self; 16] = [
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
        Self::SolarSystem,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Pipes => "3D pipes",
            Self::Stars => "Stars overhead",
            Self::SolarSystem => "Solar system",
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
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Pipes => "Dithered black-and-white pipes grow through 3D space, like the classic screensaver.",
            Self::SolarSystem => "Eight planets orbit the Sun, with independent distance and size scales and simulated time.",
            Self::Stars => "The real night sky above your location, right now, as a perfect star map.",
            Self::NightLights => "City lights seen from orbit, on a borderless map of the dark Earth.",
            Self::Clouds => "Live weather-satellite clouds, global or over your location.",
            Self::Video => "Play video files, folders or URLs as dithered Braille.",
            Self::Spectrum => "A spectrum analyzer of whatever your system is playing.",
            Self::Images => "Colored Braille images from files, folders or URLs, with slow pan and zoom.",
            Self::DitherWater => "Bayer-dithered 1-bit water: drifting caustic bands, horizon fade and slow ripple rings.",
            Self::AtlanticDusk => "Dithered sea and sky through a full day: drifting clouds, sinking sun, moon and stars.",
            Self::CubeClock => "A quiet clock beside a slowly turning dotted cube.",
            Self::BoxMachine => "A generative machine of boxes and rails that slowly builds and rearranges itself.",
            Self::MachineScreen => "A generative machine display of scanning patterns and glyph-like blocks.",
            Self::FbmClouds => "Domain-warped noise clouds thresholded into one-bit dots (software, slow-mo by default).",
            Self::DitheredWaves => "Layered wave shader rendered on the CPU with ordered dithering (software, slow-mo by default).",
            Self::DithrPatterns => "Many dithr-style animated patterns to choose from, with selectable dither algorithms (software, slow-mo by default).",
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
    pub location: GeoLocation,
    pub pipes: PipesSettings,
    pub stars: StarsSettings,
    pub solar_system: SolarSystemSettings,
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
}

impl AmbientSettings {
    pub fn normalized(&self) -> Self {
        Self {
            location: self.location.normalized(),
            pipes: self.pipes.normalized(),
            stars: self.stars.normalized(),
            solar_system: self.solar_system.normalized(),
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
        }
    }

    pub fn controls(&self, kind: AmbientKind) -> Vec<Control> {
        match kind {
            AmbientKind::Pipes => self.pipes.controls(),
            AmbientKind::Stars => self.stars.controls(),
            AmbientKind::SolarSystem => self.solar_system.controls(),
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
        }
    }

    pub fn set_control(
        &mut self,
        kind: AmbientKind,
        id: &str,
        value: ControlValue,
    ) -> Result<bool, String> {
        match kind {
            AmbientKind::Pipes => self.pipes.set_control(id, value),
            AmbientKind::Stars => self.stars.set_control(id, value),
            AmbientKind::SolarSystem => self.solar_system.set_control(id, value),
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
        }
    }

    /// Identity of everything that affects `kind`'s scene. A host rebuilds the
    /// scene exactly when this string changes (or the kind changes).
    pub fn scene_key(&self, kind: AmbientKind) -> String {
        let normalized = self.normalized();
        let settings = match kind {
            AmbientKind::Pipes => serde_json::to_string(&normalized.pipes),
            AmbientKind::Stars => serde_json::to_string(&normalized.stars),
            AmbientKind::SolarSystem => serde_json::to_string(&normalized.solar_system),
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
            AmbientKind::Pipes => Box::new(PipesScene::new(&settings.pipes, env)),
            AmbientKind::Stars => Box::new(StarsScene::new(&settings.stars, env)),
            AmbientKind::SolarSystem => {
                Box::new(SolarSystemScene::new(&settings.solar_system, env))
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
        }
    }
}
