//! The match: owns the running game and moves on from level to level.

use super::model;
use super::settings::{Gameplay, VectorTdSettings};
use super::sim::{Game, Rules, Transition, STEP};

/// Most simulation steps per render call; a long gap is skipped, not replayed.
const MAX_STEPS_PER_CALL: u32 = 600;
/// Number of maps (see `maps::MAPS`).
const MAP_COUNT: usize = 6;

pub struct Director {
    gameplay: Gameplay,
    pub game: Game,
    /// Cleared levels so far plus the start level offset.
    pub stage: u32,
    pub defeats: u32,
    /// Grid width the current game was built for.
    width: i32,
    sim_time: f32,
    /// Levels started, to rotate maps and vary seeds.
    levels_started: u32,
}

impl Director {
    pub fn new(settings: &VectorTdSettings, width: i32) -> Self {
        let gameplay = settings.gameplay();
        let stage = gameplay.start_level.saturating_sub(1);
        let game = Self::build_game(&gameplay, width, stage, 0, 0);
        Self {
            gameplay,
            game,
            stage,
            defeats: 0,
            width,
            sim_time: 0.0,
            levels_started: 1,
        }
    }

    fn map_for(settings: &Gameplay, stage: u32) -> usize {
        if settings.map == 0 {
            // The seed picks which map the cycle starts on.
            (stage as usize + settings.seed as usize) % MAP_COUNT
        } else {
            settings.map as usize - 1
        }
    }

    fn build_game(settings: &Gameplay, width: i32, stage: u32, defeats: u32, started: u32) -> Game {
        let rules = Rules {
            difficulty: settings.difficulty.health() * model::retry_relief(defeats),
            waves: settings.waves,
        };
        Game::new(
            Self::map_for(settings, stage),
            width,
            stage,
            defeats,
            rules,
            u64::from(settings.seed) * 1_000_003 + u64::from(started) * 7919 + u64::from(stage),
        )
    }

    pub fn width(&self) -> i32 {
        self.width
    }

    #[cfg(test)]
    pub fn sim_time(&self) -> f32 {
        self.sim_time
    }

    /// Rebuild for a new grid width (the screen changed shape).
    pub fn resize(&mut self, width: i32) {
        if width == self.width {
            return;
        }
        self.width = width;
        self.game = Self::build_game(
            &self.gameplay,
            width,
            self.stage,
            self.defeats,
            self.levels_started,
        );
    }

    /// Advance the simulation to `seconds` of game time.
    pub fn advance_to(&mut self, seconds: f32) {
        let mut steps = 0;
        while self.sim_time + STEP <= seconds {
            self.sim_time += STEP;
            if let Some(transition) = self.game.step() {
                self.transition(transition);
            }
            steps += 1;
            if steps >= MAX_STEPS_PER_CALL {
                self.sim_time = seconds;
                break;
            }
        }
    }

    fn transition(&mut self, transition: Transition) {
        match transition {
            Transition::NextLevel => {
                self.stage += 1;
                self.defeats /= 2;
            }
            Transition::Retry => self.defeats += 1,
        }
        self.levels_started += 1;
        self.game = Self::build_game(
            &self.gameplay,
            self.width,
            self.stage,
            self.defeats,
            self.levels_started,
        );
    }
}
