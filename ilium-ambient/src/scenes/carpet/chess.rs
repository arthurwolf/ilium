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
#[derive(Debug)]
enum SearchFailure {
    Cancelled,
}
struct SearchJob {
    request: SearchRequest,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    // Same owner allocation; remains charged if the frontend retires mid-search.
    _owner_storage: std::sync::Arc<ilium_execution::StorageAdmission>,
}
impl ilium_execution::Job for SearchJob {
    type Output = SearchResult;
    type Error = SearchFailure;
    fn run(self, context: ilium_execution::JobContext) -> Result<SearchResult, Self::Error> {
        let cancelled =
            || context.stop_requested() || self.stop.load(std::sync::atomic::Ordering::Acquire);
        if cancelled() {
            return Err(SearchFailure::Cancelled);
        }
        let chosen = self.request.position.choose_move_with_stop(
            self.request.depth,
            self.request.budget,
            self.request.seed,
            &cancelled,
        );
        if cancelled() {
            return Err(SearchFailure::Cancelled);
        }
        Ok(SearchResult {
            id: self.request.id,
            key: self.request.key,
            chosen,
        })
    }
}
// Per square: at most8 sliding rays×7 destinations, or12 promotion moves;
// king8+2 castling is smaller. Thus64×56 moves. Vec starts96 and doubles to
// at most6144;4 live frontiers (root + depth2/1/0) plus one old3072 buffer
// during growth. Positions/requests contain only fixed arrays, no heap.
fn search_cost() -> ilium_execution::JobCost {
    ilium_execution::JobCost {
        input_bytes: (4 * 6144 + 3072) * std::mem::size_of::<engine::Move>()
            + 8 * (std::mem::size_of::<Position>() + std::mem::size_of::<SearchRequest>())
            + 4096,
        result_bytes: 4096,
    }
}
enum SearchPoll {
    Pending,
    Ready(Box<ilium_execution::Retained<SearchResult>>),
    Failed(&'static str),
}
struct AiWorker {
    client: ilium_execution::Client,
    receipt: Option<ilium_execution::Receipt<SearchJob>>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    // Last: the shared stop allocation is destroyed before its storage credit.
    storage: std::sync::Arc<ilium_execution::StorageAdmission>,
}
impl AiWorker {
    fn start(
        resources: &crate::resources::AmbientResources,
    ) -> Result<Self, ilium_execution::RejectReason> {
        // Owner metadata plus AtomicBool and both Arc allocation headers.
        let storage = resources.reserve_storage(
            std::mem::size_of::<Self>()
                + std::mem::size_of::<std::sync::atomic::AtomicBool>()
                + std::mem::size_of::<ilium_execution::StorageAdmission>()
                + 8 * std::mem::size_of::<usize>(),
        )?;
        Ok(Self {
            client: resources.finite().clone(),
            receipt: None,
            stop: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            storage,
        })
    }
    fn submit(&mut self, request: SearchRequest) -> Result<(), ilium_execution::RejectReason> {
        if self.receipt.is_some() {
            return Err(ilium_execution::RejectReason::QueueFull);
        }
        // Admission precedes captured Arc cloning and every search allocation.
        let reservation = self
            .client
            .try_reserve(ilium_execution::Lane::Cpu, search_cost())?;
        let job = SearchJob {
            request,
            stop: std::sync::Arc::clone(&self.stop),
            _owner_storage: std::sync::Arc::clone(&self.storage),
        };
        self.receipt = Some(
            reservation
                .submit(job)
                .map_err(|rejected| rejected.reason)?,
        );
        Ok(())
    }
    fn poll(&mut self) -> SearchPoll {
        let Some(receipt) = self.receipt.as_mut() else {
            return SearchPoll::Pending;
        };
        match receipt.try_take() {
            ilium_execution::JobPoll::Pending => SearchPoll::Pending,
            ilium_execution::JobPoll::Ready(outcome) => {
                self.receipt = None;
                let (outcome, retention) = outcome.into_parts();
                match outcome {
                    ilium_execution::JobOutcome::Finished(Ok(result)) => {
                        SearchPoll::Ready(Box::new(retention.retain(result)))
                    }
                    ilium_execution::JobOutcome::NotStarted { .. } => SearchPoll::Failed(
                        "Automatic chess search canceled before execution; retrying",
                    ),
                    ilium_execution::JobOutcome::Panicked => {
                        SearchPoll::Failed("Automatic chess search panicked; retrying")
                    }
                    ilium_execution::JobOutcome::Finished(Err(SearchFailure::Cancelled)) => {
                        SearchPoll::Failed(
                            "Automatic chess search canceled during execution; retrying",
                        )
                    }
                }
            }
            ilium_execution::JobPoll::Lost | ilium_execution::JobPoll::Taken => {
                self.receipt = None;
                SearchPoll::Failed("Automatic chess search receipt lost; retrying")
            }
        }
    }
}
impl Drop for AiWorker {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Release);
        if let Some(receipt) = &self.receipt {
            receipt.cancel();
        }
    }
}
pub struct CarpetChess {
    resources: crate::resources::AmbientResources,
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
    pub fn new(seed: u64, resources: crate::resources::AmbientResources) -> Self {
        Self {
            resources,
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
            let result = self.ai.as_mut().map(AiWorker::poll);
            match result {
                Some(SearchPoll::Ready(result)) => {
                    let (result, retention) = (*result).into_parts();
                    self.apply_result(result, options, time);
                    drop(retention);
                }
                Some(SearchPoll::Failed(error)) => {
                    self.pending = None;
                    self.ai = None;
                    self.status_text = Some(error.into());
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
            match AiWorker::start(&self.resources) {
                Ok(ai) => self.ai = Some(ai),
                Err(reason) => {
                    self.status_text = Some(format!(
                        "Automatic chess owner admission: {reason:?}; retrying"
                    ));
                    self.next_move = Some(time + 0.05);
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
        if let Some(ai) = &mut self.ai {
            match ai.submit(request) {
                Ok(()) => {
                    self.pending = Some(self.request_id);
                    self.status_text = Some("Automatic chess: thinking".into());
                }
                Err(reason) => {
                    self.status_text =
                        Some(format!("Automatic chess admission: {reason:?}; retrying"));
                    self.next_move = Some(time + 0.05);
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
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            match self.ai.as_mut().unwrap().poll() {
                SearchPoll::Ready(result) => {
                    let (result, retention) = (*result).into_parts();
                    self.apply_result(result, options, time);
                    drop(retention);
                    return;
                }
                SearchPoll::Failed(error) => panic!("real AI worker failed: {error}"),
                SearchPoll::Pending => {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "real AI worker must complete bounded search"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
            }
        }
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
        let mut c = CarpetChess::new(1, crate::resources::test_resources());
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
        let mut c = CarpetChess::new(1, crate::resources::test_resources());
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
        let mut c = CarpetChess::new(42, crate::resources::test_resources());
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
        let mut carpet = CarpetChess::new(0, crate::resources::test_resources());
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
        let mut c = CarpetChess::new(1, crate::resources::test_resources());
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
        let mut c = CarpetChess::new(77, crate::resources::test_resources());
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
        let worker = c.ai.take().unwrap();
        let stop = std::sync::Arc::clone(&worker.stop);
        let storage = std::sync::Arc::clone(&worker.storage);
        drop(worker);
        assert!(stop.load(std::sync::atomic::Ordering::Relaxed));
        drop(stop);
        drop(storage);
    }
    fn isolated_search() -> (
        ilium_execution::Execution,
        crate::resources::AmbientResources,
        ilium_execution::QuotaGroup,
    ) {
        use ilium_execution::{
            ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaGroup, QuotaLimits,
        };
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 2,
            jobs: 3,
            service_jobs: 0,
            input_bytes: search_cost().input_bytes * 3,
            result_bytes: 32 * 1024,
            worker_threads: 1,
            worker_bytes: 64 * 1024,
        });
        let execution = Execution::start(
            quota.clone(),
            ExecutionConfig {
                cpu: LaneConfig {
                    threads: 1,
                    queue_slots: 2,
                    priority: None,
                    resident_bytes_per_thread: 1024,
                },
                io: LaneConfig {
                    threads: 0,
                    queue_slots: 0,
                    priority: None,
                    resident_bytes_per_thread: 0,
                },
                service: LaneConfig {
                    threads: 0,
                    queue_slots: 0,
                    priority: None,
                    resident_bytes_per_thread: 0,
                },
            },
        )
        .unwrap();
        let client = execution
            .client(ClientLimits {
                jobs: 3,
                service_jobs: 0,
                input_bytes: search_cost().input_bytes * 3,
                result_bytes: 32 * 1024,
            })
            .unwrap();
        (
            execution,
            crate::resources::AmbientResources::new(client),
            quota,
        )
    }
    fn request(id: u64) -> SearchRequest {
        let position = Position::new();
        SearchRequest {
            id,
            key: position.signature(),
            position,
            depth: 3,
            budget: 8192,
            seed: 77,
        }
    }
    struct BlockCpu {
        started: std::sync::mpsc::SyncSender<()>,
        release: std::sync::mpsc::Receiver<()>,
    }
    impl ilium_execution::Job for BlockCpu {
        type Output = ();
        type Error = ();
        fn run(self, _: ilium_execution::JobContext) -> Result<(), ()> {
            self.started.send(()).unwrap();
            self.release.recv().unwrap();
            Ok(())
        }
    }
    #[test]
    fn owner_metadata_refusal_precedes_stop_allocation_or_search_submission() {
        use ilium_execution::{RejectReason, ShutdownMode};
        use std::time::{Duration, Instant};
        let (mut execution, resources, quota) = isolated_search();
        let remaining = quota.snapshot().limits.worker_bytes - quota.snapshot().worker_bytes;
        let saturated = resources.reserve_storage(remaining).unwrap();
        assert!(matches!(
            AiWorker::start(&resources),
            Err(RejectReason::WorkerBytes)
        ));
        assert_eq!(quota.snapshot().jobs, 0);
        assert_eq!(quota.snapshot().input_bytes, 0);
        drop(saturated);
        let ai = AiWorker::start(&resources).unwrap();
        assert!(ai.receipt.is_none());
        drop(ai);
        execution.request_shutdown(ShutdownMode::Drain);
        execution
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .unwrap();
    }
    #[test]
    fn queued_search_retirement_keeps_original_job_credit_until_actual_cpu_release() {
        use ilium_execution::{JobCost, Lane, ShutdownMode};
        use std::time::{Duration, Instant};
        let (mut execution, resources, quota) = isolated_search();
        let (started, start) = std::sync::mpsc::sync_channel(1);
        let (release, gate) = std::sync::mpsc::sync_channel(1);
        let blocker = resources
            .finite()
            .try_reserve(
                Lane::Cpu,
                JobCost {
                    input_bytes: 4096,
                    result_bytes: 4096,
                },
            )
            .unwrap()
            .submit(BlockCpu {
                started,
                release: gate,
            })
            .unwrap();
        start.recv_timeout(Duration::from_secs(5)).unwrap();
        let mut ai = AiWorker::start(&resources).unwrap();
        ai.submit(request(1)).unwrap();
        assert_eq!(
            ai.submit(request(2)),
            Err(ilium_execution::RejectReason::QueueFull)
        );
        assert_eq!(quota.snapshot().jobs, 2);
        drop(ai);
        // Frontend retirement cancels but cannot free the queued original job.
        assert_eq!(quota.snapshot().jobs, 2);
        assert!(quota.snapshot().input_bytes >= search_cost().input_bytes);
        drop(blocker);
        release.send(()).unwrap();
        execution.request_shutdown(ShutdownMode::Drain);
        execution
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .unwrap();
        assert_eq!(quota.snapshot().jobs, 0);
        assert_eq!(quota.snapshot().input_bytes, 0);
    }
    #[test]
    fn finished_search_original_result_keeps_credit_through_bank_shutdown_and_last_consumer() {
        use ilium_execution::ShutdownMode;
        use std::time::{Duration, Instant};
        let (mut execution, resources, quota) = isolated_search();
        let mut ai = AiWorker::start(&resources).unwrap();
        ai.submit(request(19)).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let result = loop {
            match ai.poll() {
                SearchPoll::Ready(result) => break result,
                SearchPoll::Failed(error) => panic!("real search failed: {error}"),
                SearchPoll::Pending => {
                    assert!(Instant::now() < deadline);
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
        };
        assert!(ai.receipt.is_none());
        drop(ai);
        execution.request_shutdown(ShutdownMode::Drain);
        execution.join_until_background(deadline).unwrap();
        assert_eq!(quota.snapshot().jobs, 1);
        assert_eq!(quota.snapshot().input_bytes, search_cost().input_bytes);
        let retention = {
            let (result, retention) = (*result).into_parts();
            assert_eq!(result.id, 19);
            assert!(result.chosen.is_some());
            assert_eq!(quota.snapshot().jobs, 1);
            retention
        };
        drop(retention);
        assert_eq!(quota.snapshot().jobs, 0);
        assert_eq!(quota.snapshot().input_bytes, 0);
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
        let mut c = CarpetChess::new(0, crate::resources::test_resources());
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
        let mut c = CarpetChess::new(0, crate::resources::test_resources());
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

#[cfg(test)]
mod plugin_pixel_fixtures {
    use super::super::{render, CarpetScene, CarpetSettings};
    use super::*;
    use crate::{control::SceneSettings, Raster, SceneEnv};
    use serde_json::{json, Value};
    use sha2::{Digest, Sha256};
    use std::{io::Write, path::PathBuf, sync::Arc};

    struct Export {
        root: PathBuf,
        renderer: render::Renderer,
        raster: Raster,
        render_options: render::RenderOptions,
        samples: Vec<Value>,
    }

    impl Export {
        fn capture(
            &mut self,
            chess: &mut CarpetChess,
            options: &ChessOptions,
            case: &str,
            time: f64,
            input: Value,
        ) {
            assert!(chess.live.is_none(), "fixture must never acquire a TV feed");
            let mut bodies = Vec::new();
            chess.append_bodies(options, time, &mut bodies);
            // Match CarpetScene::render's geometry-to-height-field boundary.
            // Keep the raw bodies below for the separate simulation comparison.
            let mut render_bodies = bodies.clone();
            for body in &mut render_bodies {
                body.height /= super::super::MAX_BODY_HEIGHT;
            }
            self.renderer
                .render(&mut self.raster, &render_bodies, &self.render_options);
            assert!(self.raster.dots.iter().all(|dot| dot.is_finite()));
            let name = format!("sample-{:03}.f32", self.samples.len());
            let bytes: Vec<u8> = self
                .raster
                .dots
                .iter()
                .flat_map(|dot| dot.to_le_bytes())
                .collect();
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(self.root.join(&name))
                .unwrap()
                .write_all(&bytes)
                .unwrap();
            self.samples.push(json!({
                "case": case, "time": time, "input": input, "file": name,
                "sha256": format!("{:x}", Sha256::digest(&bytes)),
                "bodies": bodies.iter().map(|body| json!({
                    "from": body.from, "to": body.to,
                    "radius": body.radius, "height": body.height,
                })).collect::<Vec<_>>(),
            }));
        }
    }

    #[test]
    #[ignore = "exports native Chess/TV component pixels to a new explicit fixture directory"]
    fn export_actual_chess_and_tv_pixels_for_plugin_comparison() {
        let root = PathBuf::from(
            std::env::var_os("ILIUM_CARPET_NATIVE_FIXTURES")
                .expect("provide an absolute, nonexistent fixture directory"),
        );
        assert!(root.is_absolute());
        // Never overwrite an earlier capture or write inside a user save.
        std::fs::create_dir(&root).unwrap();
        let settings = CarpetSettings::default().normalized();
        let resources = crate::resources::test_resources();
        let environment = SceneEnv::for_test(root.join("unused-cache"), resources.clone());
        let scene = CarpetScene::new(&settings, &environment);
        let options = scene.chess_options();
        let mut raster = Raster::default();
        raster.resize(96, 64);
        let mut export = Export {
            root: root.clone(),
            renderer: render::Renderer::default(),
            raster,
            render_options: scene.render_options(),
            samples: Vec::new(),
        };
        let mut automatic = CarpetChess::new(settings.seed as u64, resources.clone());
        let mut unused = Vec::new();
        automatic.update(false, &options, 0.0, 0.0, &mut unused);
        export.capture(
            &mut automatic,
            &options,
            "automatic",
            0.0,
            json!({"mode":3,"operation":"update"}),
        );
        for ply in 0..4 {
            let start = f64::from(ply + 1) * 2.5;
            unused.clear();
            automatic.update(false, &options, start, 0.0, &mut unused);
            assert!(automatic.pending.is_some());
            // This waits for the actual admitted native AI job, rather than
            // duplicating its search or guessing completion from frame count.
            automatic.wait_for_move(&options, start);
            assert_eq!(automatic.plies, ply + 1);
            export.capture(
                &mut automatic,
                &options,
                "automatic",
                start,
                json!({"mode":3,"operation":"complete_search"}),
            );
            for fraction in [0.5, 1.0] {
                let time = start + options.easing_duration * fraction;
                export.capture(
                    &mut automatic,
                    &options,
                    "automatic",
                    time,
                    json!({"mode":3,"operation":"sample"}),
                );
            }
        }
        for (case, before, after) in [
            (
                "castling",
                "4k3/8/8/8/8/8/8/R3K2R w KQ - 0 1",
                "4k3/8/8/8/8/8/8/R4RK1 b - - 1 1",
            ),
            (
                "capture",
                "4k3/8/8/4p3/3P4/8/8/4K3 w - - 0 1",
                "4k3/8/8/4P3/8/8/8/4K3 b - - 0 1",
            ),
            (
                "promotion",
                "4k3/P7/8/8/8/8/8/4K3 w - - 0 1",
                "Q3k3/8/8/8/8/8/8/4K3 b - - 0 1",
            ),
        ] {
            let mut television = CarpetChess::new(settings.seed as u64, resources.clone());
            for (time, fen) in [
                (0.0, before),
                (1.0, after),
                (1.0 + options.easing_duration * 0.5, after),
                (1.0 + options.easing_duration, after),
            ] {
                let snapshot = TvSnapshot {
                    game: Some(Arc::new(crate::live_chess::feed::Game {
                        id: case.into(),
                        position: ChessPosition::from_fen(fen).unwrap(),
                        white_seconds: None,
                        black_seconds: None,
                        last_move: None,
                    })),
                    ..TvSnapshot::default()
                };
                television.accept_snapshot(&snapshot, &options, time, 0.0);
                export.capture(
                    &mut television,
                    &options,
                    case,
                    time,
                    json!({"mode":4,"operation":"snapshot","fen":fen,"game_id":case}),
                );
            }
        }
        let manifest = json!({
            "schema":1,"width":96,"height":64,"settings":settings,
            "scope":"actual native AI/snapshot transitions and Carpet renderer; synthetic TV positions; no network, V8 or terminal acceptance",
            "samples":export.samples,
        });
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(root.join("manifest.json"))
            .unwrap()
            .write_all(&serde_json::to_vec_pretty(&manifest).unwrap())
            .unwrap();
        println!(
            "{}",
            json!({"type":"artifact","path":root,"samples":25,
            "native_component_pixels":true,"actual_terminal_emission_proven":false})
        );
    }
}
