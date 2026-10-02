//! Chess bumps share a renderer while keeping automatic and authoritative games separate.
use super::model::Body;
use crate::live_chess::{
    live::{LiveTv, TvSnapshot},
    position::ChessPosition,
};
#[path = "chess_engine.rs"]
mod engine;
use engine::Position;

#[derive(Clone, Debug)]
pub struct ChessOptions {
    pub move_interval: f64,
    pub easing_duration: f64,
    pub radius: f32,
    /// Pawn, knight, bishop, rook, queen and king, in normalized ground units.
    pub heights: [f32; 6],
    pub ai_depth: u8,
    pub node_budget: usize,
    pub restart_delay: f64,
    pub max_plies: u16,
}
impl Default for ChessOptions {
    fn default() -> Self {
        Self {
            move_interval: 2.5,
            easing_duration: 0.8,
            radius: 0.043,
            heights: [0.045, 0.075, 0.09, 0.085, 0.12, 0.14],
            ai_depth: 2,
            node_budget: 512,
            restart_delay: 4.0,
            max_plies: 320,
        }
    }
}
#[derive(Clone)]
struct Transition {
    piece: char,
    from: [f32; 2],
    to: usize,
    start_height: f32,
    end_height: f32,
    start: f64,
    disappearing: bool,
}
struct SearchRequest {
    id: u64,
    key: engine::PositionKey,
    position: Position,
    depth: u8,
    budget: usize,
    seed: u64,
}
struct SearchResult {
    id: u64,
    key: engine::PositionKey,
    chosen: Option<engine::Move>,
}
struct AiWorker {
    request: std::sync::mpsc::SyncSender<SearchRequest>,
    results: std::sync::mpsc::Receiver<SearchResult>,
    worker: Option<crate::source::Worker>,
}
impl AiWorker {
    fn start() -> Result<Self, String> {
        let (request, requests) = std::sync::mpsc::sync_channel::<SearchRequest>(1);
        let (result_sender, results) = std::sync::mpsc::sync_channel(1);
        let worker = crate::source::Worker::try_spawn("carpet-chess", move |stop| {
            ilium_platform::thread_priority::lower_current_thread(
                ilium_platform::thread_priority::WorkerPriority::Lowest,
            );
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                let request = match requests.recv_timeout(std::time::Duration::from_millis(50)) {
                    Ok(request) => request,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                };
                let chosen = request.position.choose_move_cancellable(
                    request.depth,
                    request.budget,
                    request.seed,
                    Some(&stop),
                );
                if stop.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                if result_sender
                    .try_send(SearchResult {
                        id: request.id,
                        key: request.key,
                        chosen,
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .map_err(|error| format!("Automatic chess worker: {error}"))?;
        Ok(Self {
            request,
            results,
            worker: Some(worker),
        })
    }
}
impl Drop for AiWorker {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.stop_in_background();
        }
    }
}
pub struct CarpetChess {
    auto: Position,
    ai: Option<AiWorker>,
    request_id: u64,
    pending: Option<u64>,
    seed: u64,
    plies: u16,
    next_move: Option<f64>,
    restart_at: Option<f64>,
    history: Vec<engine::PositionKey>,
    board: Option<[Option<char>; 64]>,
    transitions: Vec<Transition>,
    live: Option<LiveTv>,
    live_started: bool,
    mode: Option<bool>,
    game_id: Option<String>,
    last_time: Option<f64>,
    status_text: Option<String>,
}
impl CarpetChess {
    pub fn new(seed: u64) -> Self {
        Self {
            auto: Position::new(),
            ai: None,
            request_id: 0,
            pending: None,
            seed,
            plies: 0,
            next_move: None,
            restart_at: None,
            history: Vec::new(),
            board: None,
            transitions: Vec::with_capacity(64),
            live: None,
            live_started: false,
            mode: None,
            game_id: None,
            last_time: None,
            status_text: None,
        }
    }
    pub fn status(&self) -> Option<String> {
        self.status_text.clone()
    }
    pub fn update(
        &mut self,
        live: bool,
        options: &ChessOptions,
        time: f64,
        now_unix_seconds: f64,
        bodies: &mut Vec<Body>,
    ) {
        let time = if time.is_finite() {
            time
        } else {
            self.last_time.unwrap_or(0.0)
        };
        if self.mode != Some(live) {
            self.mode = Some(live);
            self.ai = None;
            self.pending = None;
            self.request_id = self.request_id.wrapping_add(1);
            self.board = None;
            self.transitions.clear();
            self.game_id = None;
            self.status_text = None;
            self.last_time = None;
            self.auto = Position::new();
            self.plies = 0;
            self.history.clear();
            self.history.push(self.auto.signature());
            self.next_move = None;
            self.restart_at = None;
            self.live = None;
            self.live_started = false;
        }
        if self.last_time.is_some_and(|previous| time < previous) {
            self.next_move = Some(time + positive(options.move_interval, 2.5, 0.05, 60.0));
            self.restart_at = self
                .restart_at
                .map(|_| time + positive(options.restart_delay, 4.0, 0.1, 60.0));
            for t in &mut self.transitions {
                t.from = square(t.to);
                t.start = time;
            }
        }
        self.last_time = Some(time);
        if live {
            if !self.live_started {
                self.live_started = true;
                match LiveTv::start() {
                    Ok(feed) => self.live = Some(feed),
                    Err(error) => self.status_text = Some(error),
                }
            }
            if let Some(snapshot) = self.live.as_ref().and_then(LiveTv::try_snapshot) {
                self.accept_snapshot(&snapshot, options, time, now_unix_seconds);
            }
        } else {
            self.advance_auto(options, time);
        }
        self.append_bodies(options, time, bodies);
    }
    fn advance_auto(&mut self, options: &ChessOptions, time: f64) {
        if self.board.is_none() {
            self.set_board(self.auto.board, options, time, true);
            self.next_move = Some(time + positive(options.move_interval, 2.5, 0.05, 60.0));
        }
        if let Some(restart) = self.restart_at {
            if time < restart {
                return;
            }
            self.auto = Position::new();
            self.plies = 0;
            self.history.clear();
            self.history.push(self.auto.signature());
            self.restart_at = None;
            self.pending = None;
            self.ai = None;
            self.set_board(self.auto.board, options, time, true);
            self.status_text = None;
            self.next_move = Some(time + positive(options.move_interval, 2.5, 0.05, 60.0));
            return;
        }
        // A single pending request bounds both channels and render-frame work.
        if self.pending.is_some() {
            let result = self.ai.as_ref().map(|ai| ai.results.try_recv());
            match result {
                Some(Ok(result)) => self.apply_result(result, options, time),
                Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => {
                    self.pending = None;
                    self.ai = None;
                    self.status_text = Some("Automatic chess worker disconnected; retrying".into());
                    self.next_move = Some(time + 1.0);
                }
                _ => {}
            }
            return;
        }
        if self.next_move.is_some_and(|next| time < next) {
            return;
        }
        let repeated = self
            .history
            .iter()
            .filter(|p| **p == self.auto.signature())
            .count()
            >= 3;
        let insufficient = insufficient_material(&self.auto.board);
        if repeated
            || insufficient
            || self.auto.halfmove >= 100
            || self.plies >= options.max_plies.clamp(20, 1000)
        {
            self.finish_game(options, time, repeated, insufficient);
            return;
        }
        if self.ai.is_none() {
            match AiWorker::start() {
                Ok(ai) => self.ai = Some(ai),
                Err(error) => {
                    self.status_text = Some(error);
                    self.next_move = Some(time + 1.0);
                    return;
                }
            }
        }
        self.request_id = self.request_id.wrapping_add(1);
        let request = SearchRequest {
            id: self.request_id,
            key: self.auto.signature(),
            position: self.auto.clone(),
            depth: options.ai_depth,
            budget: options.node_budget,
            seed: self.seed.wrapping_add(u64::from(self.plies)),
        };
        if let Some(ai) = &self.ai {
            match ai.request.try_send(request) {
                Ok(()) => {
                    self.pending = Some(self.request_id);
                    self.status_text = Some("Automatic chess: thinking".into());
                }
                Err(std::sync::mpsc::TrySendError::Full(_)) => {
                    self.next_move = Some(time + 0.05);
                }
                Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                    self.ai = None;
                    self.status_text = Some("Automatic chess worker disconnected; retrying".into());
                    self.next_move = Some(time + 1.0);
                }
            }
        }
    }
    fn apply_result(&mut self, result: SearchResult, options: &ChessOptions, time: f64) {
        if self.pending != Some(result.id) || result.key != self.auto.signature() {
            return;
        }
        self.pending = None;
        if let Some(chosen) = result.chosen {
            self.auto = self.auto.play(chosen);
            self.plies = self.plies.saturating_add(1);
            self.history.push(self.auto.signature());
            self.status_text = None;
            self.set_board(self.auto.board, options, time, false);
            self.next_move = Some(time + positive(options.move_interval, 2.5, 0.05, 60.0));
        } else {
            self.finish_game(options, time, false, false);
        }
    }
    fn finish_game(
        &mut self,
        options: &ChessOptions,
        time: f64,
        repeated: bool,
        insufficient: bool,
    ) {
        self.status_text = Some(
            if repeated {
                "Automatic chess: repetition draw"
            } else if insufficient {
                "Automatic chess: insufficient material"
            } else if self.auto.halfmove >= 100 {
                "Automatic chess: fifty-move draw"
            } else if self.plies >= options.max_plies.clamp(20, 1000) {
                "Automatic chess: configured game limit"
            } else if self.auto.in_check(self.auto.white) {
                "Automatic chess: checkmate"
            } else {
                "Automatic chess: stalemate"
            }
            .into(),
        );
        self.restart_at = Some(time + positive(options.restart_delay, 4.0, 0.1, 60.0));
    }
    #[cfg(test)]
    fn wait_for_move(&mut self, options: &ChessOptions, time: f64) {
        if self.pending.is_none() {
            return;
        }
        let result = self
            .ai
            .as_ref()
            .unwrap()
            .results
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("real AI worker must complete bounded search");
        self.apply_result(result, options, time);
    }
    fn accept_snapshot(
        &mut self,
        snapshot: &TvSnapshot,
        options: &ChessOptions,
        time: f64,
        wall: f64,
    ) {
        if let Some(game) = &snapshot.game {
            let reset = self.game_id.as_ref() != Some(&game.id);
            self.game_id = Some(game.id.clone());
            if reset || self.board != Some(game.position.board) {
                self.set_position(&game.position, options, time, reset);
            }
        }
        self.status_text = if let Some(error) = &snapshot.state.error {
            Some(format!("Lichess TV: {error}; retaining last position"))
        } else if snapshot.game.is_none() {
            Some("Lichess TV: awaiting authoritative position".into())
        } else if wall.is_finite()
            && snapshot
                .state
                .received_ms
                .is_some_and(|received| wall * 1000.0 - received as f64 > 120_000.0)
        {
            Some("Lichess TV: stale; retaining last position".into())
        } else {
            Some(format!(
                "Lichess TV: {}",
                self.game_id.as_deref().unwrap_or("unknown")
            ))
        };
    }
    fn set_position(
        &mut self,
        position: &ChessPosition,
        options: &ChessOptions,
        time: f64,
        reset: bool,
    ) {
        self.set_board(position.board, options, time, reset);
    }
    fn set_board(
        &mut self,
        board: [Option<char>; 64],
        options: &ChessOptions,
        time: f64,
        reset: bool,
    ) {
        let mut old = self.board.unwrap_or([None; 64]);
        let mut tracks = Vec::with_capacity(64);
        let duration = positive(options.easing_duration, 0.8, 0.0, 10.0);
        let origin = |index: usize| {
            self.transitions
                .iter()
                .find(|t| !t.disappearing && t.to == index)
                .map_or(
                    (
                        square(index),
                        self.board
                            .and_then(|b| b[index])
                            .map_or(0.0, |p| height(p, options)),
                    ),
                    |t| sample(t, time, duration),
                )
        };
        // Retain stationary pieces before matching displaced pieces, including both castling movers.
        for (to, piece) in board.iter().enumerate() {
            if piece.is_some() && *piece == old[to] {
                let (from, start_height) = if reset {
                    (square(to), height(piece.unwrap_or('P'), options))
                } else {
                    origin(to)
                };
                tracks.push(Transition {
                    piece: piece.unwrap_or('P'),
                    from,
                    to,
                    start_height,
                    end_height: height(piece.unwrap_or('P'), options),
                    start: time,
                    disappearing: false,
                });
                old[to] = None;
            }
        }
        for (to, piece) in board.iter().enumerate() {
            let Some(piece) = *piece else { continue };
            if tracks.iter().any(|t| t.to == to) {
                continue;
            }
            let source = old
                .iter()
                .enumerate()
                .filter(|(_, p)| **p == Some(piece))
                .min_by_key(|(i, _)| i.abs_diff(to))
                .map(|(i, _)| i)
                .or_else(|| {
                    if (to / 8 == 0 || to / 8 == 7) && !piece.eq_ignore_ascii_case(&'p') {
                        old.iter()
                            .enumerate()
                            .filter(|(_, p)| {
                                **p == Some(if piece.is_ascii_uppercase() { 'P' } else { 'p' })
                            })
                            .min_by_key(|(i, _)| i.abs_diff(to))
                            .map(|(i, _)| i)
                    } else {
                        None
                    }
                });
            let (from, start_height) = source.map_or((square(to), 0.0), origin);
            tracks.push(Transition {
                piece,
                from: if reset { square(to) } else { from },
                to,
                start_height: if reset {
                    height(piece, options)
                } else {
                    start_height
                },
                end_height: height(piece, options),
                start: time,
                disappearing: false,
            });
            if let Some(i) = source {
                old[i] = None;
            }
        }
        if !reset {
            for (to, piece) in old.iter().enumerate() {
                if let Some(piece) = piece {
                    let (from, start_height) = origin(to);
                    tracks.push(Transition {
                        piece: *piece,
                        from,
                        to,
                        start_height,
                        end_height: 0.0,
                        start: time,
                        disappearing: true,
                    });
                }
            }
        }
        // Feed updates can arrive while an earlier capture is still fading.
        // Carry its original start time forward instead of restarting or dropping it.
        if !reset {
            for fading in self
                .transitions
                .iter()
                .rev()
                .filter(|track| track.disappearing && time - track.start < duration)
            {
                if tracks.len() >= 128 {
                    break;
                }
                if !tracks.iter().any(|track| {
                    track.disappearing
                        && track.piece == fading.piece
                        && track.to == fading.to
                        && track.from == fading.from
                        && track.start == fading.start
                }) {
                    tracks.push(fading.clone());
                }
            }
        }
        self.board = Some(board);
        self.transitions = tracks;
    }
    fn append_bodies(&mut self, options: &ChessOptions, time: f64, bodies: &mut Vec<Body>) {
        let duration = positive(options.easing_duration, 0.8, 0.0, 10.0);
        self.transitions
            .retain(|t| !t.disappearing || time - t.start < duration);
        for t in &self.transitions {
            let (point, value) = sample(t, time, duration);
            let target = if t.disappearing {
                0.0
            } else {
                height(t.piece, options)
            };
            let elapsed = if duration == 0.0 {
                1.0
            } else {
                ((time - t.start) / duration).clamp(0.0, 1.0) as f32
            };
            let value = if elapsed >= 1.0 { target } else { value };
            if value > 0.00001 {
                bodies.push(Body {
                    from: point,
                    to: point,
                    radius: if options.radius.is_finite() {
                        options.radius.clamp(0.005, 0.12)
                    } else {
                        0.043
                    },
                    height: value,
                });
            }
        }
    }
}
fn positive(value: f64, fallback: f64, min: f64, max: f64) -> f64 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        fallback
    }
}
fn square(index: usize) -> [f32; 2] {
    [
        0.15 + (index % 8) as f32 * 0.1,
        0.15 + (index / 8) as f32 * 0.1,
    ]
}
fn height(piece: char, options: &ChessOptions) -> f32 {
    let i = match piece.to_ascii_lowercase() {
        'p' => 0,
        'n' => 1,
        'b' => 2,
        'r' => 3,
        'q' => 4,
        _ => 5,
    };
    if options.heights[i].is_finite() {
        options.heights[i].clamp(0.0, 0.5)
    } else {
        ChessOptions::default().heights[i]
    }
}
fn sample(t: &Transition, time: f64, duration: f64) -> ([f32; 2], f32) {
    let u = if duration == 0.0 {
        1.0
    } else {
        ((time - t.start) / duration).clamp(0.0, 1.0) as f32
    };
    let ease = u * u * (3.0 - 2.0 * u);
    let end = square(t.to);
    (
        [
            t.from[0] + (end[0] - t.from[0]) * ease,
            t.from[1] + (end[1] - t.from[1]) * ease,
        ],
        t.start_height + (t.end_height - t.start_height) * ease,
    )
}
fn insufficient_material(board: &[Option<char>; 64]) -> bool {
    let others: Vec<_> = board
        .iter()
        .enumerate()
        .filter_map(|(i, p)| {
            p.filter(|p| !p.eq_ignore_ascii_case(&'k'))
                .map(|p| (i, p.to_ascii_lowercase()))
        })
        .collect();
    others.is_empty()
        || (others.len() == 1 && matches!(others[0].1, 'n' | 'b'))
        || (others.iter().all(|(_, p)| *p == 'b')
            && others
                .iter()
                .all(|(i, _)| (i % 8 + i / 8) % 2 == (others[0].0 % 8 + others[0].0 / 8) % 2))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn position(fen: &str) -> ChessPosition {
        ChessPosition::from_fen(fen).unwrap()
    }
    #[test]
    fn castle_en_passant_and_promotion_have_bounded_eased_tracks() {
        let o = ChessOptions::default();
        let mut c = CarpetChess::new(1);
        c.set_position(&position("4k3/8/8/8/8/8/8/R3K2R w KQ - 0 1"), &o, 0.0, true);
        c.set_position(&position("4k3/8/8/8/8/8/8/R4RK1 b - - 1 1"), &o, 1.0, false);
        let king = c.transitions.iter().find(|t| t.piece == 'K').unwrap();
        assert_eq!(king.from, square(60));
        assert_eq!(king.to, 62);
        let rook = c.transitions.iter().find(|t| t.to == 61).unwrap();
        assert_eq!(rook.from, square(63));
        assert!(sample(king, 1.4, 0.8).0[0] > square(60)[0]);
        assert!(sample(king, 1.4, 0.8).0[0] < square(62)[0]);
        c.set_position(
            &position("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1"),
            &o,
            2.0,
            true,
        );
        c.set_position(&position("4k3/8/3P4/8/8/8/8/4K3 b - - 0 1"), &o, 3.0, false);
        assert!(c
            .transitions
            .iter()
            .any(|t| t.piece == 'p' && t.disappearing));
        assert_eq!(
            c.transitions.iter().find(|t| t.piece == 'P').unwrap().from,
            square(28)
        );
        c.set_position(&position("4k3/P7/8/8/8/8/8/4K3 w - - 0 1"), &o, 4.0, true);
        c.set_position(&position("Q3k3/8/8/8/8/8/8/4K3 b - - 0 1"), &o, 5.0, false);
        let queen = c.transitions.iter().find(|t| t.piece == 'Q').unwrap();
        assert_eq!(queen.from, square(8));
        assert_eq!(queen.start_height, o.heights[0]);
        assert_eq!(queen.end_height, o.heights[4]);
        let mut bodies = Vec::new();
        c.append_bodies(&o, 6.0, &mut bodies);
        assert_eq!(bodies.len(), 3);
        assert!(c.transitions.len() <= 64);
    }
    #[test]
    fn authoritative_snapshot_failure_keeps_position_and_game_change_resets() {
        use crate::live_chess::feed::Game;
        use std::sync::Arc;
        let o = ChessOptions::default();
        let mut c = CarpetChess::new(1);
        let p = position("4k3/8/8/8/8/8/8/4K3 w - - 0 1");
        let mut snapshot = TvSnapshot {
            game: Some(Arc::new(Game {
                id: "realfixture".into(),
                position: p.clone(),
                white_seconds: Some(30),
                black_seconds: Some(40),
                last_move: None,
            })),
            ..TvSnapshot::default()
        };
        c.accept_snapshot(&snapshot, &o, 0.0, 0.0);
        assert_eq!(c.board, Some(p.board));
        snapshot.state.error = Some("transport failed".into());
        c.accept_snapshot(&snapshot, &o, 1.0, 1.0);
        assert_eq!(c.board, Some(p.board));
        assert!(c.status().unwrap().contains("transport failed"));
        let next = position("4k3/8/8/8/8/8/8/3K4 w - - 0 1");
        snapshot.game = Some(Arc::new(Game {
            id: "newgame".into(),
            position: next.clone(),
            white_seconds: None,
            black_seconds: None,
            last_move: None,
        }));
        c.accept_snapshot(&snapshot, &o, 2.0, 2.0);
        assert_eq!(c.board, Some(next.board));
        assert!(c.transitions.iter().all(|t| t.from == square(t.to)));
    }
    #[test]
    fn automatic_updates_do_not_start_feed_and_suspend_is_one_move() {
        let mut c = CarpetChess::new(42);
        let o = ChessOptions::default();
        let mut b = Vec::new();
        c.update(false, &o, 0.0, 0.0, &mut b);
        assert_eq!(b.len(), 32);
        b.clear();
        c.update(false, &o, 500.0, 500.0, &mut b);
        c.wait_for_move(&o, 500.0);
        assert_eq!(c.plies, 1);
        assert!(!c.live_started);
        assert!(c.live.is_none());
        b.clear();
        c.update(false, &o, -1.0, 1.0, &mut b);
        assert_eq!(c.plies, 1);
        assert!(b.iter().all(|b| b.height.is_finite()));
        for i in 1..400 {
            b.clear();
            c.update(false, &o, i as f64 * 10.0, i as f64, &mut b);
            c.wait_for_move(&o, i as f64 * 10.0);
            assert!(b.len() <= 64);
            assert!(c.history.len() <= 1001);
        }
    }
}

#[cfg(test)]
mod captured_feed_test {
    use super::*;
    #[test]
    fn captured_lichess_game_0dwsvfei_moves_and_captures_under_carpet() {
        // Authoritative TV FEN/id/last-move/clocks captured 2026-10-02 from
        // https://lichess.org/api/tv/feed. Player metadata omitted; full raw
        // acquisition retained at /tmp/ilium-carpet-chess/lichess-tv-capture.ndjson.
        // SHA256 fec5c3f90349b4b31ea89300ca68d7b2be094aa12bbef79c18bc3d12a029d9d7.
        let lines = [
            r#"{"t":"featured","d":{"id":"0DwSvFei","fen":"r3k2r/pp2ppbp/2n3p1/qB1pP3/3P2b1/2P2N2/P4PPP/R1BQR1K1 w kq - 1 12"}}"#,
            r#"{"t":"fen","d":{"fen":"r3k2r/pp2ppbp/2n3p1/qB1pP3/3P2b1/1QP2N2/P4PPP/R1B1R1K1 b kq - 2 12","lm":"d1b3","wc":162,"bc":154}}"#,
            r#"{"t":"fen","d":{"fen":"r3k2r/pp2ppbp/2n3p1/qB1pP3/3P4/1QP2b2/P4PPP/R1B1R1K1 w kq - 0 13","lm":"g4f3","wc":162,"bc":148}}"#,
        ];
        let mut game = None;
        let mut carpet = CarpetChess::new(0);
        let options = ChessOptions::default();
        for (i, line) in lines.iter().enumerate() {
            assert!(crate::live_chess::feed::apply_line(&mut game, line.as_bytes()).unwrap());
            let snapshot = TvSnapshot {
                game: game.clone().map(std::sync::Arc::new),
                ..TvSnapshot::default()
            };
            carpet.accept_snapshot(&snapshot, &options, i as f64, 0.0);
        }
        assert_eq!(carpet.board, Some(game.unwrap().position.board));
        let bishop = carpet
            .transitions
            .iter()
            .find(|t| t.piece == 'b' && t.to == 45)
            .unwrap();
        assert_eq!(bishop.from, square(38));
        assert!(carpet
            .transitions
            .iter()
            .any(|t| t.piece == 'N' && t.disappearing && t.to == 45));
        assert!(carpet.live.is_none());
    }
}

#[cfg(test)]
mod freshness_tests {
    use super::*;
    #[test]
    fn epoch_freshness_distinguishes_stale_nonfinite_and_backward_clock() {
        let game = crate::live_chess::feed::Game {
            id: "fixture".into(),
            position: ChessPosition::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap(),
            white_seconds: None,
            black_seconds: None,
            last_move: None,
        };
        let state = crate::live_data::model::FeedState {
            received_ms: Some(1_000_000),
            ..Default::default()
        };
        let snapshot = TvSnapshot {
            game: Some(std::sync::Arc::new(game)),
            state,
        };
        let mut c = CarpetChess::new(1);
        let o = ChessOptions::default();
        c.accept_snapshot(&snapshot, &o, 0.0, 1121.0);
        assert!(c.status().unwrap().contains("stale"));
        c.accept_snapshot(&snapshot, &o, 1.0, 999.0);
        assert!(!c.status().unwrap().contains("stale"));
        c.accept_snapshot(&snapshot, &o, 2.0, f64::NAN);
        assert!(!c.status().unwrap().contains("stale"));
        assert_eq!(c.board, Some(snapshot.game.unwrap().position.board));
    }
}

#[cfg(test)]
mod async_tests {
    use super::*;
    #[test]
    fn actual_worker_completes_and_stale_result_cannot_mutate_board() {
        let mut c = CarpetChess::new(77);
        let options = ChessOptions {
            ai_depth: 3,
            node_budget: 5000,
            ..Default::default()
        };
        let mut bodies = Vec::new();
        c.update(false, &options, 0.0, 0.0, &mut bodies);
        bodies.clear();
        c.update(false, &options, 4.0, 0.0, &mut bodies);
        let before = c.auto.signature();
        assert!(c.pending.is_some());
        let pending = c.pending.unwrap();
        c.apply_result(
            SearchResult {
                id: pending.wrapping_add(1),
                key: before,
                chosen: c.auto.choose_move(1, 32, 0),
            },
            &options,
            4.0,
        );
        assert_eq!(c.auto.signature(), before);
        assert_eq!(c.pending, Some(pending));
        c.wait_for_move(&options, 4.1);
        assert_ne!(c.auto.signature(), before);
        assert_eq!(c.plies, 1);
        assert!(c.pending.is_none());
        let mut worker = c.ai.take().unwrap();
        let stop = worker.worker.as_ref().unwrap().stop_flag();
        worker.worker.take().unwrap().stop_in_background();
        assert!(stop.load(std::sync::atomic::Ordering::Relaxed));
    }
    #[test]
    fn cancelled_engine_does_not_search() {
        let stop = std::sync::atomic::AtomicBool::new(true);
        assert!(Position::new()
            .choose_move_cancellable(3, 8192, 0, Some(&stop))
            .is_none());
    }
}

#[cfg(test)]
mod interrupted_fade_tests {
    use super::*;
    #[test]
    fn interrupted_capture_fade_survives_next_move_without_extending_lifetime() {
        let options = ChessOptions {
            easing_duration: 2.0,
            ..Default::default()
        };
        let mut c = CarpetChess::new(0);
        let before = ChessPosition::from_fen("4k3/8/8/4p3/3P4/8/8/4K3 w - - 0 1").unwrap();
        let captured = ChessPosition::from_fen("4k3/8/8/4P3/8/8/8/4K3 b - - 0 1").unwrap();
        let moved = ChessPosition::from_fen("3k4/8/8/4P3/8/8/8/4K3 w - - 1 2").unwrap();
        c.set_position(&before, &options, 0.0, true);
        c.set_position(&captured, &options, 1.0, false);
        c.set_position(&moved, &options, 1.1, false);
        let fades: Vec<_> = c
            .transitions
            .iter()
            .filter(|t| t.piece == 'p' && t.disappearing)
            .collect();
        assert_eq!(fades.len(), 1);
        assert_eq!(fades[0].start, 1.0);
        assert!(sample(fades[0], 1.2, 2.0).1 > 0.0);
        let mut bodies = Vec::new();
        c.append_bodies(&options, 3.01, &mut bodies);
        assert!(!c
            .transitions
            .iter()
            .any(|t| t.piece == 'p' && t.disappearing));
        assert_eq!(bodies.len(), 3);
    }
}

#[cfg(test)]
mod bound_tests {
    use super::*;
    #[test]
    fn synthetic_burst_of_capture_snapshots_caps_retained_fades() {
        let options = ChessOptions {
            easing_duration: 10.0,
            ..Default::default()
        };
        let mut c = CarpetChess::new(0);
        let mut full = [None; 64];
        full[0] = Some('k');
        full[63] = Some('K');
        for piece in &mut full[8..24] {
            *piece = Some('p');
        }
        let mut sparse = full;
        for piece in &mut sparse[8..24] {
            *piece = None;
        }
        c.set_board(full, &options, 0.0, true);
        for i in 1..=40 {
            c.set_board(
                if i % 2 == 0 { full } else { sparse },
                &options,
                f64::from(i) * 0.01,
                false,
            );
            assert!(c.transitions.len() <= 128);
        }
        let mut bodies = Vec::new();
        c.append_bodies(&options, 11.0, &mut bodies);
        assert_eq!(c.transitions.len(), 18);
        assert_eq!(bodies.len(), 18);
    }
}
