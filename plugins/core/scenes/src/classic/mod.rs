//! The ten classic scenes: shoreline, moonlit water, sleeping ridge, windy hillside, tea
//! steam, kelp, stone caustics, cloudlets, two ripples and the quiet pond. They were the
//! deterministic built-in scenes of the client; the code moved here unchanged. Each scene is
//! seekable (a frame depends only on the elapsed animation time and the settings), prepares
//! its fixed geometry once per control set and size, and draws tone only; colour, palette,
//! dithering and display stay with the shared look of the host.

mod scenes;
pub mod settings;
pub mod shoreline;

pub use settings::{
    CloudletSettings, KelpSettings, MoonlitWaterSettings, QuietPondSettings, SleepingRidgeSettings,
    StoneCausticsSettings, TeaSteamSettings, TwoRipplesSettings, WindyHillsideSettings,
};
pub use shoreline::{ShorelineSettings, ShorelineStyle};

use crate::kit::scene::{Frame, Scene, SceneEnv};
use crate::scenes2::settings_from_spec;
use scenes::SceneCache;
use serde_json::Value;

/// Which classic scene draws.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ClassicKind {
    #[default]
    Shoreline,
    MoonlitWater,
    SleepingRidge,
    WindyHillside,
    TeaSteam,
    Kelp,
    StoneCaustics,
    Cloudlets,
    TwoRipples,
    QuietPond,
}

/// The settings of every classic scene next to the selected kind; the scene functions read
/// only the block of their own kind.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClassicSettings {
    pub kind: ClassicKind,
    pub shoreline: ShorelineSettings,
    pub moonlit_water: MoonlitWaterSettings,
    pub sleeping_ridge: SleepingRidgeSettings,
    pub windy_hillside: WindyHillsideSettings,
    pub tea_steam: TeaSteamSettings,
    pub kelp: KelpSettings,
    pub stone_caustics: StoneCausticsSettings,
    pub cloudlets: CloudletSettings,
    pub two_ripples: TwoRipplesSettings,
    pub quiet_pond: QuietPondSettings,
}

/// Manifest ids of the classic scenes, in catalogue order.
pub const IDS: &[&str] = &[
    "shoreline",
    "moonlit-water",
    "sleeping-ridge",
    "windy-hillside",
    "tea-steam",
    "kelp",
    "stone-caustics",
    "cloudlets",
    "two-ripples",
    "quiet-pond",
];

/// One classic scene as the kit draws it.
pub struct ClassicScene {
    settings: ClassicSettings,
    cache: SceneCache,
}

impl ClassicScene {
    pub fn new(settings: ClassicSettings) -> Self {
        Self {
            settings,
            cache: SceneCache::default(),
        }
    }
}

impl Scene for ClassicScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        if frame.raster.width == 0 || frame.raster.height == 0 {
            return;
        }
        scenes::render(
            frame.raster,
            &mut self.cache,
            &self.settings,
            frame.time.as_secs_f64(),
        );
    }
}

/// Builds the classic scene `scene_id` from the `spec`; `None` for an id that is not one of
/// [`IDS`].
pub fn create(scene_id: &str, spec: &Value, _env: &SceneEnv) -> Option<Box<dyn Scene>> {
    let mut settings = ClassicSettings::default();
    match scene_id {
        "shoreline" => {
            settings.kind = ClassicKind::Shoreline;
            settings.shoreline = settings_from_spec(spec);
        }
        "moonlit-water" => {
            settings.kind = ClassicKind::MoonlitWater;
            settings.moonlit_water = settings_from_spec(spec);
        }
        "sleeping-ridge" => {
            settings.kind = ClassicKind::SleepingRidge;
            settings.sleeping_ridge = settings_from_spec(spec);
        }
        "windy-hillside" => {
            settings.kind = ClassicKind::WindyHillside;
            settings.windy_hillside = settings_from_spec(spec);
        }
        "tea-steam" => {
            settings.kind = ClassicKind::TeaSteam;
            settings.tea_steam = settings_from_spec(spec);
        }
        "kelp" => {
            settings.kind = ClassicKind::Kelp;
            settings.kelp = settings_from_spec(spec);
        }
        "stone-caustics" => {
            settings.kind = ClassicKind::StoneCaustics;
            settings.stone_caustics = settings_from_spec(spec);
        }
        "cloudlets" => {
            settings.kind = ClassicKind::Cloudlets;
            settings.cloudlets = settings_from_spec(spec);
        }
        "two-ripples" => {
            settings.kind = ClassicKind::TwoRipples;
            settings.two_ripples = settings_from_spec(spec);
        }
        "quiet-pond" => {
            settings.kind = ClassicKind::QuietPond;
            settings.quiet_pond = settings_from_spec(spec);
        }
        _ => return None,
    }
    Some(Box::new(ClassicScene::new(settings)))
}

#[cfg(test)]
mod tests;
