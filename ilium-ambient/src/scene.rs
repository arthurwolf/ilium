//! The contract between a scene engine and its host (the ilium client).

use crate::gpu::GpuRunner;
use crate::location::GeoLocation;
use crate::raster::Raster;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// Everything a scene may need besides its own settings.
#[derive(Clone)]
pub struct SceneEnv {
    /// The shared observer position (stars, clouds, night lights).
    pub location: GeoLocation,
    /// Root for downloaded tiles, catalogs and thumbnails. Scenes create their
    /// own sub-directory below it. May not exist yet.
    pub cache_dir: PathBuf,
    /// The shared GPU device, when the host found a usable one. Scenes with a
    /// GPU renderer use it only when their setting asks for it.
    pub gpu: Option<Arc<dyn GpuRunner>>,
}

impl std::fmt::Debug for SceneEnv {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SceneEnv")
            .field("location", &self.location)
            .field("cache_dir", &self.cache_dir)
            .field(
                "gpu",
                &self.gpu.as_ref().map(|runner| runner.adapter_name()),
            )
            .finish()
    }
}

impl SceneEnv {
    /// Test/probe environment with a throw-away cache directory.
    pub fn for_test(cache_dir: PathBuf) -> Self {
        Self {
            location: GeoLocation::default(),
            cache_dir,
            gpu: None,
        }
    }
}

/// One frame request. The raster is already resized to `2 * width` by
/// `4 * height` dots (Braille sub-cells) and cleared to zero.
pub struct Frame<'a> {
    /// Dot intensities 0.0..=1.0. The host thresholds them with the user's
    /// density and dither settings to produce Braille.
    pub raster: &'a mut Raster,
    /// One RGB value per terminal cell, row-major, `width * height` long.
    /// Only read by the host when the scene reports `uses_cell_colors()`.
    pub cell_colors: &'a mut Vec<[u8; 3]>,
    pub width: u16,
    pub height: u16,
    /// Animation clock: wall time since the scene started, multiplied by the
    /// user's global Speed setting. Monotonic non-decreasing within a scene.
    pub time: Duration,
    /// Unscaled wall time since the scene started.
    pub wall: Duration,
    /// Current civil time, for scenes that show the real sky/weather "now".
    pub now: SystemTime,
}

impl Frame<'_> {
    /// Set both the intensity of a Braille dot and nothing else.
    pub fn cell_color_mut(&mut self, cell_x: u16, cell_y: u16) -> Option<&mut [u8; 3]> {
        if cell_x >= self.width || cell_y >= self.height {
            return None;
        }
        self.cell_colors
            .get_mut(usize::from(cell_y) * usize::from(self.width) + usize::from(cell_x))
    }
}

/// A stateful scene. Created by the registry from settings; the host drops it
/// (which must stop every thread and child process it owns) whenever the
/// settings that affect it change or another scene is selected.
///
/// Rules:
/// * `render` must never block on I/O, a child process, a lock held by a
///   worker, or a sleep. Slow work belongs on an owned background thread
///   (see `source::Worker`); `render` shows the last good data and a
///   tasteful placeholder until data arrive.
/// * `render` must overwrite the whole raster (it arrives zeroed) and, when
///   `uses_cell_colors()`, every cell color.
/// * Time may only be read from `Frame`, never from the system clock, so
///   scenes are testable.
pub trait Scene: Send {
    fn render(&mut self, frame: &mut Frame<'_>);

    /// True when the scene supplies `Frame::cell_colors`; otherwise the
    /// host paints every dot in the user's global palette.
    fn uses_cell_colors(&self) -> bool {
        false
    }

    /// Requested redraw cadence, 1..=30.
    fn frames_per_second(&self) -> u32 {
        12
    }

    /// One short status line for the Settings panel ("Downloading tiles 40%",
    /// "ffmpeg not found", "No audio input"). `None` when all is well.
    fn status(&self) -> Option<String> {
        None
    }
}

/// A scene that draws nothing and says why. Registry stub and failure fallback.
pub struct MessageScene(pub String);

impl Scene for MessageScene {
    fn render(&mut self, _frame: &mut Frame<'_>) {}

    fn status(&self) -> Option<String> {
        Some(self.0.clone())
    }
}
