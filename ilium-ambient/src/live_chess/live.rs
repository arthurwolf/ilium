//! Owned streaming TV connection with bounded lines and coalesced snapshots.
use super::feed::{self, Game};
use crate::live_data::model::FeedState;
use crate::source::{http_stream_lines, sleep_unless_stopped, Worker};
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
            let mut game = None;
            let mut failures = 0_u32;
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                let mut received_position = false;
                let result = http_stream_lines(
                    "https://lichess.org/api/tv/feed",
                    16_384,
                    Duration::from_secs(60),
                    &stop,
                    |line| {
                        if stop.load(std::sync::atomic::Ordering::Relaxed) {
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
                            Err(error) => publish_error(&shared, error),
                            Ok(false) => {}
                        }
                        true
                    },
                );
                if stop.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                failures = if received_position {
                    0
                } else {
                    failures.saturating_add(1)
                };
                publish_error(
                    &shared,
                    result.err().map_or_else(
                        || "Lichess TV disconnected; reconnecting".into(),
                        |error| format!("Lichess TV reconnecting: {error}"),
                    ),
                );
                let seconds = 5_u64.saturating_mul(1_u64 << failures.min(4)).min(60);
                if !sleep_unless_stopped(&stop, Duration::from_secs(seconds)) {
                    break;
                }
            }
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
