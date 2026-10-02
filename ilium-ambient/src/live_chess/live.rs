//! Owned streaming TV connection with bounded lines and coalesced snapshots.
use super::feed::{self, Game};
use crate::live_data::model::FeedState;
use crate::source::{http_stream_lines, sleep_unless_stopped, FetchError, Worker};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Default)]
pub struct TvSnapshot {
    pub game: Option<Arc<Game>>,
    pub state: FeedState,
}

pub struct LiveTv {
    snapshot: Arc<Mutex<Arc<TvSnapshot>>>,
    worker: Option<Worker>,
}

impl LiveTv {
    pub fn start() -> Result<Self, String> {
        let snapshot = Arc::new(Mutex::new(Arc::new(TvSnapshot::default())));
        let shared = Arc::clone(&snapshot);
        let worker = Worker::try_spawn("live-chess", move |stop| {
            ilium_platform::thread_priority::lower_current_thread(
                ilium_platform::thread_priority::WorkerPriority::Lowest,
            );
            run_connections(
                &shared,
                &stop,
                |receive| {
                    http_stream_lines(
                        "https://lichess.org/api/tv/feed",
                        16_384,
                        Duration::from_secs(60),
                        &stop,
                        receive,
                    )
                },
                |delay| sleep_unless_stopped(&stop, delay),
            );
        })
        .map_err(|error| format!("could not start live chess worker: {error}"))?;
        Ok(Self {
            snapshot,
            worker: Some(worker),
        })
    }

    pub fn try_snapshot(&self) -> Option<Arc<TvSnapshot>> {
        self.snapshot
            .try_lock()
            .ok()
            .map(|guard| Arc::clone(&guard))
    }
}

/// Own decoder lifetime, reconnect policy and cooperative cooldown independently
/// of the HTTP adapter, so reconnect boundaries can be checked without a network.
fn run_connections(
    shared: &Mutex<Arc<TvSnapshot>>,
    stop: &AtomicBool,
    mut connect: impl FnMut(&mut dyn FnMut(&[u8]) -> bool) -> Result<(), FetchError>,
    mut wait: impl FnMut(Duration) -> bool,
) {
    let mut failures = 0_u32;
    while !stop.load(Ordering::Relaxed) {
        // FEN updates have no game id: a new connection must establish its own
        // featured identity. The independently published last-good board survives.
        let mut game = None;
        let mut received_position = false;
        let result = connect(&mut |line| {
            if stop.load(Ordering::Relaxed) {
                return false;
            }
            match feed::apply_line(&mut game, line) {
                Ok(true) => {
                    received_position = true;
                    let received_ms = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map_or(0, |elapsed| {
                            elapsed.as_millis().min(i64::MAX as u128) as i64
                        });
                    if let Ok(mut guard) = shared.lock() {
                        let mut next = (**guard).clone();
                        next.game = game.clone().map(Arc::new);
                        // The TV protocol carries FEN/clocks, not an observation
                        // timestamp. Show receipt time and leave source time unknown.
                        next.state.received(received_ms, None);
                        *guard = Arc::new(next);
                    }
                    failures = 0;
                }
                Err(error) => publish_error(shared, error),
                Ok(false) => {}
            }
            true
        });
        if stop.load(Ordering::Relaxed) {
            break;
        }
        failures = if received_position {
            0
        } else {
            failures.saturating_add(1)
        };
        let delay = reconnect_delay(result.as_ref().err(), failures);
        publish_error(
            shared,
            result.err().map_or_else(
                || "Lichess TV disconnected; reconnecting".into(),
                |error| format!("Lichess TV reconnecting: {error}"),
            ),
        );
        if !wait(delay) {
            break;
        }
    }
}

fn reconnect_delay(error: Option<&FetchError>, failures: u32) -> Duration {
    // Lichess API guidance requires at least one minute after HTTP 429.
    // The adapter preserves the status; arbitrary error text is never parsed.
    if matches!(error, Some(FetchError::HttpStatus(429))) {
        return Duration::from_secs(60);
    }
    Duration::from_secs(5_u64.saturating_mul(1_u64 << failures.min(4)).min(60))
}

fn publish_error(shared: &Mutex<Arc<TvSnapshot>>, error: String) {
    if let Ok(mut guard) = shared.lock() {
        let mut next = (**guard).clone();
        next.state.failed(error);
        *guard = Arc::new(next);
    }
}

impl Drop for LiveTv {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.stop_in_background();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FEATURED: &[u8] =
        br#"{"t":"featured","d":{"id":"oldgame","fen":"4k3/8/8/8/8/8/8/4K3 w - - 0 1"}}"#;
    const ORPHAN_POSITION: &[u8] =
        br#"{"t":"fen","d":{"fen":"4k3/8/8/8/8/8/8/3K4 b - - 1 1","lm":"e1d1"}}"#;

    #[test]
    fn first_http_429_cooldown_waits_at_least_one_minute() {
        assert!(reconnect_delay(Some(&FetchError::HttpStatus(429)), 1) >= Duration::from_secs(60));
        for (failures, seconds) in [(0, 5), (1, 10), (2, 20), (3, 40), (4, 60), (u32::MAX, 60)] {
            assert_eq!(
                reconnect_delay(None, failures),
                Duration::from_secs(seconds)
            );
        }
        // Provider status must be typed: an arbitrary message mentioning 429 is not a rate limit.
        assert_eq!(
            reconnect_delay(Some(&FetchError::Request("HTTP 429 text".into())), 1),
            Duration::from_secs(10)
        );
        assert_eq!(
            reconnect_delay(Some(&FetchError::HttpStatus(503)), 1),
            Duration::from_secs(10)
        );
    }

    #[test]
    fn reconnect_requires_featured_identity_and_preserves_last_good_snapshot() {
        let shared = Mutex::new(Arc::new(TvSnapshot::default()));
        let stop = AtomicBool::new(false);
        let mut connection = 0;
        run_connections(
            &shared,
            &stop,
            |receive| {
                connection += 1;
                if connection == 1 {
                    assert!(receive(FEATURED));
                    return Err(FetchError::Request("disconnect".into()));
                }
                let previous = Arc::clone(&shared.lock().unwrap());
                assert_eq!(previous.game.as_ref().unwrap().id, "oldgame");
                assert!(previous.state.error.is_some());
                assert!(receive(ORPHAN_POSITION));
                let after = Arc::clone(&shared.lock().unwrap());
                assert_eq!(
                    after.game, previous.game,
                    "orphan position contaminated the prior game"
                );
                assert_eq!(after.state.received_ms, previous.state.received_ms);
                assert_eq!(after.state.observed_ms, previous.state.observed_ms);
                assert!(after
                    .state
                    .error
                    .as_deref()
                    .unwrap()
                    .contains("before featured"));
                assert!(receive(br#"{"t":"featured","d":{"id":"newgame","fen":"4k3/8/8/8/8/8/8/3K4 b - - 1 1"}}"#));
                assert_eq!(shared.lock().unwrap().game.as_ref().unwrap().id, "newgame");
                assert!(shared.lock().unwrap().state.error.is_none());
                stop.store(true, Ordering::Relaxed);
                Ok(())
            },
            |_| true,
        );
        assert_eq!(connection, 2);
    }

    #[test]
    fn malformed_featured_switch_preserves_published_board_and_receipt_until_valid_identity() {
        let shared = Mutex::new(Arc::new(TvSnapshot::default()));
        let stop = AtomicBool::new(false);
        run_connections(
            &shared,
            &stop,
            |receive| {
                assert!(receive(FEATURED));
                let before = Arc::clone(&shared.lock().unwrap());
                assert!(before.state.received_ms.is_some());
                assert!(receive(
                    br#"{"t":"featured","d":{"id":"newgame","fen":"invalid"}}"#
                ));
                let failed = Arc::clone(&shared.lock().unwrap());
                assert_eq!(failed.game, before.game);
                assert_eq!(failed.state.received_ms, before.state.received_ms);
                assert_eq!(failed.state.observed_ms, before.state.observed_ms);
                assert!(failed.state.error.is_some());
                assert!(receive(ORPHAN_POSITION));
                let orphan = Arc::clone(&shared.lock().unwrap());
                assert_eq!(
                    orphan.game, before.game,
                    "FEN after malformed featured contaminated old game"
                );
                assert_eq!(orphan.state.received_ms, before.state.received_ms);
                assert_eq!(orphan.state.observed_ms, before.state.observed_ms);
                assert!(orphan.state.error.is_some());
                assert!(receive(br#"{"t":"featured","d":{"id":"newgame","fen":"4k3/8/8/8/8/8/8/3K4 b - - 1 1"}}"#));
                let recovered = Arc::clone(&shared.lock().unwrap());
                assert_eq!(recovered.game.as_ref().unwrap().id, "newgame");
                assert_eq!(
                    recovered.game.as_ref().unwrap().position.board[59],
                    Some('K')
                );
                assert!(recovered.state.received_ms.is_some());
                assert!(recovered.state.error.is_none());
                stop.store(true, Ordering::Relaxed);
                Ok(())
            },
            |_| panic!("stopped connection must not reconnect"),
        );
    }
    #[test]
    fn cancelled_cooldown_does_not_admit_another_connection() {
        let shared = Mutex::new(Arc::new(TvSnapshot::default()));
        let stop = AtomicBool::new(false);
        let mut connections = 0;
        let mut waits = vec![];
        run_connections(
            &shared,
            &stop,
            |_| {
                connections += 1;
                Err(FetchError::HttpStatus(429))
            },
            |delay| {
                waits.push(delay);
                stop.store(true, Ordering::Relaxed);
                false
            },
        );
        assert_eq!(connections, 1);
        assert_eq!(waits, [Duration::from_secs(60)]);
    }
}
