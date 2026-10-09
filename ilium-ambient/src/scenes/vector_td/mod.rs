//! Vector TD: a tower defense that plays itself.
//!
//! A full-screen board of glowing vector shapes: monsters walk a path in
//! waves and an AI places, upgrades and unlocks towers, sends waves early and
//! moves from map to map and level to level, with the monsters and the
//! towers growing stronger as it goes. Everything is drawn as thin Braille
//! lines and dithered fills; the colour variant tints each cell by the
//! strongest shape in it.
//!
//! Inspired by the Vector TD game (see `INSPIRED_BY`). The towers, monsters,
//! maps and numbers here are this scene's own.
//!
//! Determinism: the game advances in fixed steps from a seeded generator, so
//! it is a pure function of (settings, `Frame::time`) as long as time moves
//! forward. Nothing reads the clock, blocks or spawns threads.
//!
//! No external data source is used.

mod ai;
mod director;
mod draw;
mod maps;
mod model;
mod palette;
mod render;
mod settings;
mod sim;
#[cfg(test)]
mod tests;

pub use settings::VectorTdSettings;

use crate::control::SceneSettings;
use crate::registry::AmbientSettings;
use crate::scene::{Frame, Scene, SceneEnv};
use crate::style::ScenePalette;
use director::Director;
use draw::Canvas;
use palette::Colors;
use render::{Painter, View};

pub const INSPIRED_BY: &[&str] = &["https://www.crazygames.com/game/vector-td"];

pub struct VectorTdScene {
    settings: VectorTdSettings,
    colors: Colors,
    /// The shared look's palette; role colours are mapped onto it when provided.
    palette: ScenePalette,
    /// Built on the first frame, when the screen shape is known.
    director: Option<Director>,
    /// Scratch: the strongest tone drawn into each cell.
    strongest: Vec<f32>,
    /// Frame time of the previous render, to turn time into steps.
    last_time: Option<f32>,
    /// Game seconds played: frame time scaled by the game speed at the time.
    game_clock: f32,
}

impl VectorTdScene {
    // PALETTE (native Scene contract): `env.palette` is the shared look's current
    // palette. A custom native Scene receives the
    // current palette and MUST follow it: scenes with natural colours shift them
    // onto it (`ScenePalette::recolor`/`at`), and `Scene::set_palette` delivers later
    // changes. This scene follows it natively: every role colour (towers,
    // monsters, path, text) is mapped onto the palette by lightness when the
    // colour table is built (`follows_palette`), so `PaletteScene` skips its
    // generic recolour.
    pub fn new(settings: &VectorTdSettings, env: &SceneEnv) -> Self {
        let settings = settings.normalized();
        Self {
            colors: Colors::new(&settings, &env.palette),
            palette: env.palette.clone(),
            settings,
            director: None,
            strongest: Vec::new(),
            last_time: None,
            game_clock: 0.0,
        }
    }
}

impl Scene for VectorTdScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        let width = frame.raster.width;
        let height = frame.raster.height;
        if width == 0 || height == 0 {
            return;
        }
        // The game clock advances by the elapsed frame time at the current
        // game speed, so changing the speed never jumps. Time that moves
        // backwards (the global Speed was lowered) only pauses the game.
        let now = frame.time.as_secs_f32();
        let elapsed = self.last_time.map_or(now, |last| (now - last).max(0.0));
        self.last_time = Some(now);
        self.game_clock += elapsed * self.settings.game_speed as f32 / 100.0;

        let columns = maps::grid_width_for(width, height);
        let settings = &self.settings;
        let director = self
            .director
            .get_or_insert_with(|| Director::new(settings, columns));
        director.resize(columns);
        director.advance_to(self.game_clock);

        let cells = usize::from(frame.width) * usize::from(frame.height);
        self.strongest.clear();
        self.strongest.resize(cells, 0.0);
        if frame.cell_colors.len() != cells {
            frame.cell_colors.resize(cells, [0; 3]);
        }
        frame.cell_colors.fill([0; 3]);
        let view = View::fit(width, height, director.width(), maps::GRID_HEIGHT);
        let mut canvas = Canvas::new(
            frame.raster,
            frame.cell_colors,
            &mut self.strongest,
            usize::from(frame.width),
        );
        let mut painter = Painter::new(
            &mut canvas,
            view,
            &self.colors,
            &self.settings,
            director.game.time,
        );
        painter.paint(&director.game);
    }

    fn set_palette(&mut self, palette: &ScenePalette) {
        self.palette = palette.clone();
        self.colors = Colors::new(&self.settings, palette);
    }

    fn follows_palette(&self) -> bool {
        true
    }

    fn uses_cell_colors(&self) -> bool {
        self.colors.is_color
    }

    fn frames_per_second(&self) -> u32 {
        20
    }

    /// Colours, glow, speed and the like change in place: the game keeps
    /// playing. Anything that changes what is played rebuilds the scene.
    fn reconfigure(&mut self, settings: &AmbientSettings) -> bool {
        let next = settings.vector_td.normalized();
        if next.gameplay() != self.settings.gameplay() {
            return false;
        }
        self.colors = Colors::new(&next, &self.palette);
        self.settings = next;
        true
    }
}
