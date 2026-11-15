//! Video: video files, folders and the Germination series played as dithered Braille.
//!
//! The scene runs as a state machine advanced once per frame (`player`). A picked video file or
//! folder is read through the host's read-only `picked/<n>` scopes and decoded by the host
//! `media` import, which hands back packed `rgb8` frames of exactly the size asked for and
//! answers `timed-out` while the next frame is not ready, so nothing here waits. The Germination
//! clips are pinned Wikimedia Commons files fetched in ranged chunks through the host `http`
//! import, checked against their SHA-256 (`germination`) and decoded from memory. The colour
//! treatment is the shared look's: the scene only chooses between greyscale dots, the
//! picture's own cell colours and a pen-and-ink edge rendering (`convert`).
//!
//! PALETTE (future plugin contract): `env.palette` is the shared look's current palette. This
//! scene follows it natively: its cell colours are mapped onto the palette by brightness
//! (`ScenePalette::recolor_cells`) as each frame is produced, and `Scene::set_palette` delivers
//! later changes (applied from the next render). With no palette the colours are untouched.
//!
//! Redraw cadence: `frames_per_second()` is the Frame rate setting (6 to 24). The scene shows
//! the newest frame that is due, so a clip with a higher native rate is thinned and one with a
//! lower rate repeats its frame.

mod convert;
mod germination;
mod player;
mod schedule;
mod settings;
mod source;
#[cfg(test)]
mod tests;

use crate::control::SceneSettings;
use crate::kit::raster::Raster;
use crate::kit::scene::{Frame, Scene, SceneEnv};
use crate::kit::style::ScenePalette;
use convert::{decode_frame, place_on_canvas, Decoded};
use player::{Player, RawFrame};
use schedule::format_clock;
pub use settings::VideoSettings;
use settings::{PlaybackMode, RenderStyle};
use source::{HostIo, VideoIo};
use std::time::Duration;

/// A changed terminal size must hold this long before the decoder is asked for a new size.
const RESIZE_SETTLE: Duration = Duration::from_millis(500);
/// Largest canvas decoded, in cells.
const MAX_CELLS: (u16, u16) = (512, 128);
/// Colour of a cell that has no picture colour yet.
const BLANK_COLOR: [u8; 3] = [90, 90, 90];

/// Builds the Video scene from the spec of the host.
pub fn create(spec: &serde_json::Value, env: &SceneEnv) -> Box<dyn Scene> {
    let settings: VideoSettings = crate::kit::settings::from_spec(spec);
    Box::new(VideoScene::new(&settings, env, Box::new(HostIo)))
}

/// The picture on screen and what the status line says about it.
struct Current {
    decoded: Decoded,
    name: String,
    source_seconds: f64,
    duration_seconds: Option<f64>,
}

pub struct VideoScene {
    settings: VideoSettings,
    player: Player,
    current: Option<Current>,
    /// Size (cells) the decoder was last asked for.
    requested: Option<(u16, u16)>,
    /// A different size seen since this wall time, not yet acted upon.
    settling: Option<((u16, u16), Duration)>,
    palette: ScenePalette,
}

impl VideoScene {
    pub fn new(settings: &VideoSettings, env: &SceneEnv, io: Box<dyn VideoIo>) -> Self {
        let settings = settings.normalized();
        let player = Player::new(&settings, io);
        Self::with_player(settings, env, player)
    }

    fn with_player(settings: VideoSettings, env: &SceneEnv, player: Player) -> Self {
        Self {
            player,
            settings,
            current: None,
            requested: None,
            settling: None,
            palette: env.palette.clone(),
        }
    }

    /// Debounced size tracking: the first size is requested at once, later changes only after
    /// they have held for `RESIZE_SETTLE`.
    fn track_size(&mut self, size: (u16, u16), wall: Duration) {
        match self.requested {
            None => self.requested = Some(size),
            Some(requested) if requested == size => self.settling = None,
            Some(_) => match self.settling {
                Some((candidate, since)) if candidate == size => {
                    if wall.saturating_sub(since) >= RESIZE_SETTLE {
                        self.requested = Some(size);
                        self.settling = None;
                    }
                }
                _ => self.settling = Some((size, wall)),
            },
        }
    }

    fn show(&mut self, raw: RawFrame, canvas: (u32, u32)) {
        let placed = place_on_canvas(&raw.rgb, raw.size, canvas);
        let decoded = decode_frame(
            &placed,
            canvas.0 as usize,
            canvas.1 as usize,
            self.settings.style,
            self.settings.detail,
        );
        if let Some(decoded) = decoded {
            self.current = Some(Current {
                decoded,
                name: raw.name,
                source_seconds: raw.source_seconds,
                duration_seconds: raw.duration_seconds,
            });
        }
    }

    fn colored(&self) -> bool {
        self.settings.style == RenderStyle::Colored
    }
}

impl Scene for VideoScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        if frame.width == 0 || frame.height == 0 {
            return;
        }
        self.track_size(
            (frame.width.min(MAX_CELLS.0), frame.height.min(MAX_CELLS.1)),
            frame.wall,
        );
        let Some((columns, rows)) = self.requested else {
            return;
        };
        let canvas = (u32::from(columns) * 2, u32::from(rows) * 4);
        if let Some(raw) = self.player.advance(frame.wall, frame.now, canvas) {
            self.show(raw, canvas);
        }
        let colored = self.colored();
        match &self.current {
            Some(current) => {
                let decoded = &current.decoded;
                if (decoded.width, decoded.height) == (frame.raster.width, frame.raster.height) {
                    frame.raster.dots.copy_from_slice(&decoded.dots);
                } else {
                    let dots = decoded.resampled_dots(frame.raster.width, frame.raster.height);
                    if dots.len() == frame.raster.dots.len() {
                        frame.raster.dots.copy_from_slice(&dots);
                    }
                }
                if colored {
                    let (width, height) = (usize::from(frame.width), usize::from(frame.height));
                    let mut colors = decoded.resampled_colors(width, height);
                    colors.resize(width * height, BLANK_COLOR);
                    *frame.cell_colors = colors;
                    self.palette.recolor_cells(frame.cell_colors);
                }
            }
            None => {
                draw_placeholder(frame.raster, frame.wall);
                if colored {
                    let cells = usize::from(frame.width) * usize::from(frame.height);
                    frame.cell_colors.clear();
                    frame.cell_colors.resize(cells, BLANK_COLOR);
                    self.palette.recolor_cells(frame.cell_colors);
                }
            }
        }
    }

    fn set_palette(&mut self, palette: &ScenePalette) {
        self.palette = palette.clone();
    }

    fn follows_palette(&self) -> bool {
        true
    }

    fn uses_cell_colors(&self) -> bool {
        self.colored()
    }

    fn frames_per_second(&self) -> u32 {
        u32::from(self.settings.frame_rate).clamp(6, 24)
    }

    fn status(&self) -> Option<String> {
        if let Some(notice) = self.player.notice() {
            return Some(notice.to_owned());
        }
        let Some(current) = &self.current else {
            return Some("Starting video...".to_owned());
        };
        if self.requested.is_some_and(|(columns, rows)| {
            current.decoded.width != usize::from(columns) * 2
                || current.decoded.height != usize::from(rows) * 4
        }) {
            return Some(format!("Resizing: {}", current.name));
        }
        let mut text = format!(
            "Playing: {} {}",
            current.name,
            format_clock(current.source_seconds)
        );
        if let Some(total) = current.duration_seconds {
            text.push_str(&format!(" / {}", format_clock(total)));
        }
        if self.settings.mode == PlaybackMode::Slowed {
            text.push_str(&format!(" ({}% speed)", self.settings.slowed_percent));
        }
        if let Some(note) = self.player.note() {
            text.push_str(&format!(" [{note}]"));
        }
        Some(text)
    }
}

/// Dim "no picture yet" mark: a 16:9 screen outline with a play triangle that breathes slowly.
/// Kept faint so it reads as a background, not content.
fn draw_placeholder(raster: &mut Raster, wall: Duration) {
    let (width, height) = (raster.width as f32, raster.height as f32);
    let side = (width * 0.5).min(height * 0.5 * 16.0 / 9.0);
    if side < 8.0 {
        return;
    }
    let (screen_width, screen_height) = (side, side * 9.0 / 16.0);
    let breath = 0.4 + 0.12 * (wall.as_secs_f32() * 1.4).sin();
    let point = |dx: f32, dy: f32| ((width / 2.0 + dx) / width, (height / 2.0 + dy) / height);
    let (half_width, half_height) = (screen_width / 2.0, screen_height / 2.0);
    let corners = [
        point(-half_width, -half_height),
        point(half_width, -half_height),
        point(half_width, half_height),
        point(-half_width, half_height),
    ];
    for index in 0..4 {
        raster.line(corners[index], corners[(index + 1) % 4], 0.5, breath);
    }
    let triangle = [
        point(-screen_width * 0.1, -screen_height * 0.24),
        point(-screen_width * 0.1, screen_height * 0.24),
        point(screen_width * 0.14, 0.0),
    ];
    for index in 0..3 {
        raster.line(
            triangle[index],
            triangle[(index + 1) % 3],
            0.5,
            breath * 0.9,
        );
    }
}
