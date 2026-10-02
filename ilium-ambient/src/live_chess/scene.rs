//! Dithered live chess presentation over authoritative TV positions.
use super::{
    draw::{self, ChessSettings},
    live::{LiveTv, TvSnapshot},
};
use crate::{Frame, Scene, SceneEnv, SceneSettings};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

pub struct ChessScene {
    settings: ChessSettings,
    live: Option<LiveTv>,
    snapshot: Arc<TvSnapshot>,
    startup_error: Option<String>,
    now_ms: i64,
}

impl ChessScene {
    pub fn new(settings: &ChessSettings, _env: &SceneEnv) -> Self {
        let mut scene = Self::offline(settings);
        match LiveTv::start() {
            Ok(live) => scene.live = Some(live),
            Err(error) => scene.startup_error = Some(error),
        }
        scene
    }

    fn offline(settings: &ChessSettings) -> Self {
        Self {
            settings: settings.normalized(),
            live: None,
            snapshot: Arc::new(TvSnapshot::default()),
            startup_error: None,
            now_ms: 0,
        }
    }

    pub fn apply_settings(&mut self, settings: &ChessSettings) -> bool {
        let next = settings.normalized();
        let changed = self.settings != next;
        self.settings = next;
        changed
    }
}

impl Scene for ChessScene {
    fn reconfigure(&mut self, settings: &crate::AmbientSettings) -> bool {
        self.apply_settings(&settings.chess);
        true
    }
    fn render(&mut self, frame: &mut Frame<'_>) {
        self.now_ms = frame.now.duration_since(UNIX_EPOCH).map_or(0, |elapsed| {
            elapsed.as_millis().min(i64::MAX as u128) as i64
        });
        if let Some(snapshot) = self.live.as_ref().and_then(LiveTv::try_snapshot) {
            self.snapshot = snapshot;
        }
        frame.raster.dots.fill(0.0);
        frame
            .cell_colors
            .resize(usize::from(frame.width) * usize::from(frame.height), [0; 3]);
        frame.cell_colors.fill([0; 3]);
        if let Some(game) = self.snapshot.game.as_ref() {
            draw::draw_board(
                &game.position,
                &self.settings,
                frame.raster,
                frame.cell_colors,
            );
        }
    }

    fn uses_cell_colors(&self) -> bool {
        true
    }
    fn frames_per_second(&self) -> u32 {
        4
    }
    fn status(&self) -> Option<String> {
        if let Some(error) = &self.startup_error {
            return Some(error.clone());
        }
        let state = &self.snapshot.state;
        let Some(game) = &self.snapshot.game else {
            return Some(state.error.clone().unwrap_or_else(|| {
                "Connecting to Lichess TV — waiting for an actual game".into()
            }));
        };
        let age = state.received_ms.map_or_else(
            || "unknown".into(),
            |received| {
                format!(
                    "{}s ago",
                    self.now_ms.saturating_sub(received).max(0) / 1000
                )
            },
        );
        let clock = |seconds: Option<u32>| {
            seconds.map_or_else(
                || "?".into(),
                |seconds| format!("{}:{:02}", seconds / 60, seconds % 60),
            )
        };
        Some(format!(
            "Lichess TV {} — {} to move · clocks {} / {} at last event · received {age}{}",
            game.id,
            if game.position.white_to_move {
                "White"
            } else {
                "Black"
            },
            clock(game.white_seconds),
            clock(game.black_seconds),
            state
                .error
                .as_ref()
                .map_or_else(String::new, |error| format!(
                    " · {error} (last position retained)"
                ))
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::super::feed;
    use super::*;
    #[test]
    fn actual_fixture_position_renders_and_color_edit_retains_game() {
        let settings = ChessSettings::default();
        let mut scene = ChessScene::offline(&settings);
        let mut game = None;
        feed::apply_line(&mut game,br#"{"t":"featured","d":{"id":"abcd1234","fen":"rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1","players":[]}}"#).unwrap();
        scene.snapshot = Arc::new(TvSnapshot {
            game: game.map(Arc::new),
            ..TvSnapshot::default()
        });
        let mut raster = crate::Raster::default();
        raster.resize(96, 96);
        let mut colors = Vec::new();
        let mut frame = Frame {
            raster: &mut raster,
            cell_colors: &mut colors,
            width: 48,
            height: 24,
            time: std::time::Duration::ZERO,
            wall: std::time::Duration::ZERO,
            now: UNIX_EPOCH,
        };
        scene.render(&mut frame);
        assert!(frame.raster.dots.iter().any(|dot| *dot > 0.0));
        let before = scene.snapshot.game.clone().unwrap();
        let mut edited = settings;
        edited.black_at_bottom = true;
        assert!(scene.apply_settings(&edited));
        assert_eq!(scene.snapshot.game.as_deref(), Some(before.as_ref()));
    }
}
