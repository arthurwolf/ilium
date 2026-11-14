//! The live Lichess TV source: polled over the host's http import, one request at a time.
//!
//! A scene runs inside a render frame and cannot own a worker or a stream, so the source
//! asks for the TV channel list, then for the featured game, a few seconds apart, and
//! publishes the last good position. A failed or malformed answer keeps that position and
//! only records the error.
use super::feed::Game;
use super::position::ChessPosition;
use ilium_plugin_sdk::host::HttpRequest;
use ilium_plugin_sdk::types::RequestId;
use serde_json::Value;
use std::sync::Arc;

const CHANNELS_URL: &str = "https://lichess.org/api/tv/channels";
const GAME_URL: &str = "https://lichess.org/game/export/";
/// Seconds between two position requests (Lichess asks API clients to stay well below 1/s).
const POLL_SECONDS: f64 = 3.0;
/// Seconds to back off after a failure, and after a rate-limit answer.
const FAILURE_BACKOFF_SECONDS: f64 = 15.0;
const RATE_LIMIT_BACKOFF_SECONDS: f64 = 60.0;
const MAX_BODY_BYTES: usize = 256 * 1024;
const REQUEST_TIMEOUT_MILLIS: u32 = 10_000;

/// Whether a source has heard from the network, and what went wrong last.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FeedState {
    pub received_ms: Option<i64>,
    pub observed_ms: Option<i64>,
    pub error: Option<String>,
}

impl FeedState {
    pub fn received(&mut self, received_ms: i64, observed_ms: Option<i64>) {
        if received_ms < 0 {
            return;
        }
        self.received_ms = Some(received_ms);
        self.observed_ms = observed_ms.filter(|time| *time >= 0);
        self.error = None;
    }

    pub fn failed(&mut self, error: String) {
        self.error = Some(error.chars().take(240).collect());
    }
}

#[derive(Debug, Clone, Default)]
pub struct TvSnapshot {
    pub game: Option<Arc<Game>>,
    pub state: FeedState,
}

/// Where live positions come from.
pub trait TvSource: Send {
    /// The latest published snapshot, advancing the source's polling to `now_unix_seconds`.
    fn snapshot(&mut self, now_unix_seconds: f64) -> Option<Arc<TvSnapshot>>;
}

/// One finished answer.
#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    Body(Vec<u8>),
    /// A non-success HTTP status.
    Status(u16),
    Failed(String),
}

/// The transport under a [`HostTv`].
pub trait Transport: Send {
    fn start(&mut self, url: &str) -> Result<u64, String>;
    /// The finished answer of request `id`, or `None` while it is still running.
    fn take(&mut self, id: u64) -> Option<Answer>;
}

/// The host's http import.
#[derive(Default)]
pub struct HostTransport;

impl Transport for HostTransport {
    fn start(&mut self, url: &str) -> Result<u64, String> {
        ilium_plugin_sdk::host()
            .fetch(HttpRequest {
                method: "GET".into(),
                url: url.into(),
                headers: vec![ilium_plugin_sdk::types::net::Header {
                    name: "Accept".into(),
                    value: "application/json".into(),
                }],
                body: None,
                timeout_millis: REQUEST_TIMEOUT_MILLIS,
            })
            .map(|RequestId(id)| id)
            .map_err(|error| error.message)
    }

    fn take(&mut self, id: u64) -> Option<Answer> {
        let response = ilium_plugin_sdk::host().take_response(RequestId(id))?;
        Some(if (200..300).contains(&response.status) {
            if response.body.len() > MAX_BODY_BYTES {
                Answer::Failed("chess answer exceeds 256 KiB".into())
            } else {
                Answer::Body(response.body)
            }
        } else {
            Answer::Status(response.status)
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Stage {
    Idle,
    Channels(u64),
    Game(u64),
}

/// Polls Lichess TV through a [`Transport`].
pub struct HostTv<T: Transport = HostTransport> {
    transport: T,
    stage: Stage,
    next_poll: f64,
    game_id: Option<String>,
    snapshot: Arc<TvSnapshot>,
}

impl Default for HostTv<HostTransport> {
    fn default() -> Self {
        Self::new(HostTransport)
    }
}

impl<T: Transport> HostTv<T> {
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            stage: Stage::Idle,
            next_poll: 0.0,
            game_id: None,
            snapshot: Arc::new(TvSnapshot::default()),
        }
    }

    fn fail(&mut self, now: f64, message: String, backoff: f64) {
        let mut next = (*self.snapshot).clone();
        next.state.failed(message);
        self.snapshot = Arc::new(next);
        self.stage = Stage::Idle;
        self.next_poll = now + backoff;
    }

    fn accept(&mut self, now: f64, game: Game) {
        let mut next = (*self.snapshot).clone();
        next.game = Some(Arc::new(game));
        next.state.received((now * 1000.0) as i64, None);
        self.snapshot = Arc::new(next);
        self.stage = Stage::Idle;
        self.next_poll = now + POLL_SECONDS;
    }

    fn start(&mut self, now: f64, url: &str, stage: impl FnOnce(u64) -> Stage) {
        match self.transport.start(url) {
            Ok(id) => self.stage = stage(id),
            Err(message) => self.fail(
                now,
                format!("Lichess TV: {message}"),
                FAILURE_BACKOFF_SECONDS,
            ),
        }
    }

    fn step(&mut self, now: f64) {
        match self.stage.clone() {
            Stage::Idle => {
                if !now.is_finite() || now < self.next_poll {
                    return;
                }
                match self.game_id.clone() {
                    None => self.start(now, CHANNELS_URL, Stage::Channels),
                    Some(id) => {
                        let url = format!("{GAME_URL}{id}?lastFen=true&moves=false&clocks=true");
                        self.start(now, &url, Stage::Game);
                    }
                }
            }
            Stage::Channels(id) => {
                let Some(answer) = self.transport.take(id) else {
                    return;
                };
                match answer {
                    Answer::Body(body) => match featured_game_id(&body) {
                        Ok(game) => {
                            self.game_id = Some(game);
                            self.stage = Stage::Idle;
                            self.next_poll = now;
                        }
                        Err(message) => self.fail(now, message, FAILURE_BACKOFF_SECONDS),
                    },
                    other => self.fail_answer(now, other),
                }
            }
            Stage::Game(id) => {
                let Some(answer) = self.transport.take(id) else {
                    return;
                };
                match answer {
                    Answer::Body(body) => {
                        let id = self.game_id.clone().unwrap_or_default();
                        match Game::from_export(&id, &body) {
                            Ok(game) => self.accept(now, game),
                            Err(message) => {
                                // The featured game may have ended: ask for the channel again.
                                self.game_id = None;
                                self.fail(now, message, FAILURE_BACKOFF_SECONDS);
                            }
                        }
                    }
                    other => {
                        self.game_id = None;
                        self.fail_answer(now, other);
                    }
                }
            }
        }
    }

    fn fail_answer(&mut self, now: f64, answer: Answer) {
        match answer {
            Answer::Status(429) => self.fail(
                now,
                "Lichess TV rate limited; waiting a minute".into(),
                RATE_LIMIT_BACKOFF_SECONDS,
            ),
            Answer::Status(status) => self.fail(
                now,
                format!("Lichess TV answered HTTP {status}"),
                FAILURE_BACKOFF_SECONDS,
            ),
            Answer::Failed(message) => self.fail(
                now,
                format!("Lichess TV: {message}"),
                FAILURE_BACKOFF_SECONDS,
            ),
            Answer::Body(_) => {}
        }
    }
}

impl<T: Transport> TvSource for HostTv<T> {
    fn snapshot(&mut self, now_unix_seconds: f64) -> Option<Arc<TvSnapshot>> {
        self.step(now_unix_seconds);
        Some(Arc::clone(&self.snapshot))
    }
}

/// The game id of the TV channel list (the top-rated channel, else any channel).
pub fn featured_game_id(body: &[u8]) -> Result<String, String> {
    let channels: Value = serde_json::from_slice(body).map_err(|error| error.to_string())?;
    let valid = |id: &str| {
        !id.is_empty() && id.len() <= 32 && id.chars().all(|c| c.is_ascii_alphanumeric())
    };
    let pick = |channel: &Value| {
        channel["gameId"]
            .as_str()
            .filter(|id| valid(id))
            .map(str::to_owned)
    };
    channels
        .get("Top Rated")
        .and_then(pick)
        .or_else(|| channels.as_object()?.values().find_map(pick))
        .ok_or_else(|| "Lichess TV lists no game".to_owned())
}

impl Game {
    /// The game of a Lichess game export (`lastFen=true&clocks=true`): the last position,
    /// the last move and the clocks in whole seconds.
    pub fn from_export(id: &str, body: &[u8]) -> Result<Game, String> {
        let value: Value = serde_json::from_slice(body).map_err(|error| error.to_string())?;
        let fen = value["lastFen"]
            .as_str()
            .ok_or_else(|| "chess export has no last position".to_owned())?;
        // The export may give only the board part: complete it for the FEN parser.
        let fen = if fen.split_whitespace().count() < 2 {
            let turn = if value["moves"].as_str().map_or(0, |m| m.split(' ').count()) % 2 == 0 {
                "w"
            } else {
                "b"
            };
            format!("{fen} {turn} - - 0 1")
        } else {
            fen.to_owned()
        };
        let position = ChessPosition::from_fen(&fen)?;
        let clock = |index: usize| {
            value["clocks"]
                .as_array()
                .filter(|clocks| clocks.len() >= 2)
                .and_then(|clocks| {
                    let last = clocks.len() - 1;
                    let at = if (last % 2) == index { last } else { last - 1 };
                    clocks.get(at)
                })
                .and_then(Value::as_u64)
                .and_then(|centis| u32::try_from(centis / 100).ok())
        };
        Ok(Game {
            id: id.to_owned(),
            position,
            white_seconds: clock(0),
            black_seconds: clock(1),
            last_move: value["lastMove"]
                .as_str()
                .filter(|text| text.len() <= 5 && text.is_ascii())
                .map(str::to_owned),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// A transport that answers from a script and records the urls asked.
    #[derive(Default)]
    struct Scripted {
        answers: VecDeque<Option<Answer>>,
        asked: Vec<String>,
    }

    impl Transport for Scripted {
        fn start(&mut self, url: &str) -> Result<u64, String> {
            self.asked.push(url.to_owned());
            Ok(self.asked.len() as u64)
        }
        fn take(&mut self, _id: u64) -> Option<Answer> {
            self.answers.pop_front().flatten()
        }
    }

    fn body(text: &str) -> Option<Answer> {
        Some(Answer::Body(text.as_bytes().to_vec()))
    }

    const CHANNELS: &str =
        r#"{"Bullet":{"gameId":"bulletgame"},"Top Rated":{"gameId":"topgame1"}}"#;
    const EXPORT: &str = r#"{"lastFen":"4k3/8/8/8/8/8/8/4K3 w - - 0 1","lastMove":"e2e4","clocks":[30000,29000,28000,27000]}"#;

    #[test]
    fn channel_list_then_game_export_publishes_the_position() {
        let mut tv = HostTv::new(Scripted {
            answers: VecDeque::from([body(CHANNELS), body(EXPORT)]),
            ..Default::default()
        });
        assert!(tv.snapshot(100.0).unwrap().game.is_none());
        let snapshot = tv.snapshot(100.0).unwrap();
        assert!(snapshot.game.is_none());
        let snapshot = tv.snapshot(100.0).unwrap();
        let game = snapshot.game.as_ref().unwrap();
        assert_eq!(game.id, "topgame1");
        assert_eq!(game.last_move.as_deref(), Some("e2e4"));
        assert_eq!(game.white_seconds, Some(280));
        assert_eq!(game.black_seconds, Some(270));
        assert_eq!(snapshot.state.error, None);
        let asked = &tv.transport.asked;
        assert_eq!(asked[0], CHANNELS_URL);
        assert!(asked[1].starts_with("https://lichess.org/game/export/topgame1?"));
    }

    #[test]
    fn polling_is_spaced_and_a_pending_request_is_not_repeated() {
        let mut tv = HostTv::new(Scripted {
            answers: VecDeque::from([None, None, body(CHANNELS), body(EXPORT)]),
            ..Default::default()
        });
        tv.snapshot(0.0);
        tv.snapshot(0.1);
        tv.snapshot(0.2);
        assert_eq!(
            tv.transport.asked.len(),
            1,
            "one request in flight at a time"
        );
        tv.snapshot(0.3);
        tv.snapshot(0.4);
        assert_eq!(tv.transport.asked.len(), 2);
        tv.snapshot(0.5);
        assert!(tv.snapshot(1.0).unwrap().game.is_some());
        tv.snapshot(2.0);
        assert_eq!(
            tv.transport.asked.len(),
            2,
            "next poll waits for the interval"
        );
        tv.snapshot(4.5);
        assert_eq!(tv.transport.asked.len(), 3);
    }

    #[test]
    fn failures_keep_the_last_position_and_back_off() {
        let mut tv = HostTv::new(Scripted {
            answers: VecDeque::from([body(CHANNELS), body(EXPORT), Some(Answer::Status(429))]),
            ..Default::default()
        });
        for _ in 0..3 {
            tv.snapshot(0.0);
        }
        tv.snapshot(10.0);
        let snapshot = tv.snapshot(10.0).unwrap();
        assert!(snapshot.game.is_some(), "last good position survives");
        assert!(snapshot
            .state
            .error
            .as_deref()
            .unwrap()
            .contains("rate limited"));
        let asked = tv.transport.asked.len();
        tv.snapshot(40.0);
        assert_eq!(
            tv.transport.asked.len(),
            asked,
            "a minute of silence after 429"
        );
    }

    #[test]
    fn malformed_answers_are_errors_not_panics() {
        assert!(featured_game_id(b"{}").is_err());
        assert!(featured_game_id(b"nope").is_err());
        assert!(Game::from_export("x", b"{}").is_err());
        assert!(Game::from_export("x", br#"{"lastFen":"garbage"}"#).is_err());
    }

    #[test]
    fn a_board_only_fen_is_completed() {
        let game =
            Game::from_export("abc", br#"{"lastFen":"4k3/8/8/8/8/8/8/4K3","moves":"e4"}"#).unwrap();
        assert_eq!(game.position.board[60], Some('K'));
    }
}
