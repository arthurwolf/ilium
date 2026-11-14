//! Dithered live chess presentation over authoritative TV positions.
use super::{
    draw::{self, ChessSettings},
    live::{HostTransport, HostTv, TvSnapshot, TvSource},
};
use crate::control::SceneSettings;
use crate::scene::{Frame, Scene, SceneEnv};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

pub struct ChessScene {
    settings: ChessSettings,
    live: Option<Box<dyn TvSource>>,
    snapshot: Arc<TvSnapshot>,
    now_ms: i64,
}

impl ChessScene {
    // PALETTE (future plugin contract): `env.palette` is the shared look's current palette.
    // This scene draws tone and fixed board colours, which the shared look recolours.
    pub fn new(settings: &ChessSettings, _env: &SceneEnv) -> Self {
        let mut scene = Self::offline(settings);
        scene.live = Some(Box::new(HostTv::new(HostTransport)));
        scene
    }

    /// A scene whose positions come from `source`.
    pub fn with_source(settings: &ChessSettings, source: Box<dyn TvSource>) -> Self {
        let mut scene = Self::offline(settings);
        scene.live = Some(source);
        scene
    }

    fn offline(settings: &ChessSettings) -> Self {
        Self {
            settings: settings.normalized(),
            live: None,
            snapshot: Arc::new(TvSnapshot::default()),
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
    fn reconfigure(&mut self, settings: &serde_json::Value) -> bool {
        let next: ChessSettings = crate::scenes2::settings_from_spec(settings);
        self.apply_settings(&next);
        true
    }
    fn render(&mut self, frame: &mut Frame<'_>) {
        self.now_ms = frame.now.duration_since(UNIX_EPOCH).map_or(0, |elapsed| {
            elapsed.as_millis().min(i64::MAX as u128) as i64
        });
        let now_seconds = self.now_ms as f64 / 1000.0;
        if let Some(snapshot) = self
            .live
            .as_mut()
            .and_then(|live| live.snapshot(now_seconds))
        {
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

/// The scene a spec describes (the host's spec with a `settings` object).
pub fn create(spec: &serde_json::Value, env: &SceneEnv) -> Box<dyn Scene> {
    let settings: ChessSettings = crate::scenes2::settings_from_spec(spec);
    Box::new(ChessScene::new(&settings, env))
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
        let mut raster = crate::raster::Raster::default();
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
