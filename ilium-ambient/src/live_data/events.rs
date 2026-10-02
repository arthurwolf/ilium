//! Numeric Wikipedia edit aggregates; article titles and editor identities are never retained.
use super::{model::Observation, poll::Snapshot, series::DataSeries};
use crate::source::{http_stream_lines, sleep_unless_stopped, Worker};
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::sync::{atomic::Ordering, Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MAX_BUCKETS: usize = 4096;
const MAX_IDS: usize = 8192;
const STREAM_URL: &str = "https://stream.wikimedia.org/v2/stream/recentchange";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WikiMetric {
    EditRate,
    BotShare,
}

struct Edit {
    id: String,
    second: i64,
    bot: bool,
}

/// EventStreams emits one JSON value per data line. Other SSE fields and
/// comments are ignored; unsupported multiline JSON fails closed.
fn decode(line: &[u8]) -> Result<Option<Edit>, String> {
    let Some(bytes) = line.strip_prefix(b"data:") else {
        return Ok(None);
    };
    if bytes.len() > 262_144 {
        return Err("Wikipedia event exceeds 256 KB".into());
    }
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "invalid Wikipedia event JSON".to_owned())?;
    if value["meta"]["domain"] == "canary"
        || value["type"] != "edit"
        || !value["server_name"]
            .as_str()
            .is_some_and(|name| name.ends_with(".wikipedia.org"))
    {
        return Ok(None);
    }
    let second = value["timestamp"]
        .as_i64()
        .filter(|time| *time >= 0 && time.checked_mul(1000).is_some())
        .ok_or_else(|| "Wikipedia event lacks valid provider time".to_owned())?;
    let bot = value["bot"]
        .as_bool()
        .ok_or_else(|| "Wikipedia event lacks bot classification".to_owned())?;
    let id = value["meta"]["id"]
        .as_str()
        .filter(|id| !id.is_empty() && id.len() <= 256)
        .ok_or_else(|| "Wikipedia event lacks bounded identity".to_owned())?;
    Ok(Some(Edit {
        id: id.to_owned(),
        second,
        bot,
    }))
}

#[derive(Default)]
struct Bucket {
    edits: u64,
    bots: u64,
}

#[derive(Default)]
struct Aggregate {
    buckets: BTreeMap<i64, Bucket>,
    ids: HashSet<String>,
    id_order: VecDeque<String>,
    connection_latest: Option<i64>,
    rejected: usize,
}

impl Aggregate {
    fn disconnect(&mut self) {
        self.connection_latest = None;
    }

    fn latest_ms(&self) -> Option<i64> {
        self.buckets
            .last_key_value()
            .map(|(second, _)| second * 1000)
    }

    fn accept(&mut self, edit: Edit) -> bool {
        let latest = self
            .buckets
            .last_key_value()
            .map_or(edit.second, |(time, _)| *time);
        let floor = latest
            .max(edit.second)
            .saturating_sub(MAX_BUCKETS as i64 - 1);
        if edit.second < floor || self.ids.contains(&edit.id) {
            return false;
        }
        self.ids.insert(edit.id.clone());
        self.id_order.push_back(edit.id);
        if self.id_order.len() > MAX_IDS {
            if let Some(oldest) = self.id_order.pop_front() {
                self.ids.remove(&oldest);
            }
        }
        // Zero bins only bridge actual provider timestamps on one uninterrupted
        // connection. No padding before first event, after latest, or across outages.
        if let Some(previous) = self
            .connection_latest
            .filter(|previous| *previous >= latest)
        {
            for second in previous.saturating_add(1).max(floor)..edit.second {
                self.buckets.entry(second).or_default();
            }
        }
        // A replayed retained-second event cannot establish coverage after
        // reconnect. Start coverage only when provider time advances again.
        if self.connection_latest.is_some() || self.buckets.is_empty() || edit.second > latest {
            self.connection_latest = Some(
                self.connection_latest
                    .map_or(edit.second, |old| old.max(edit.second)),
            );
        }
        let bucket = self.buckets.entry(edit.second).or_default();
        bucket.edits = bucket.edits.saturating_add(1);
        bucket.bots = bucket.bots.saturating_add(u64::from(edit.bot));
        while self
            .buckets
            .first_key_value()
            .is_some_and(|(second, _)| *second < floor)
        {
            self.buckets.pop_first();
        }
        true
    }

    fn series(&self, metric: WikiMetric) -> DataSeries {
        DataSeries {
            samples: self.buckets.iter().filter_map(|(second, bucket)| {
                let value = match metric {
                    WikiMetric::EditRate => bucket.edits as f64,
                    WikiMetric::BotShare if bucket.edits > 0 => 100.0 * bucket.bots as f64 / bucket.edits as f64,
                    WikiMetric::BotShare => return None,
                };
                Some(Observation { observed_ms: second * 1000, value })
            }).collect(),
            detail: Some("Wikipedia edits; provider-second bins, current bin provisional; disconnects leave gaps".into()),
            rejected: self.rejected,
            ..DataSeries::default()
        }
    }
}

fn receipt_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_millis().min(i64::MAX as u128) as i64)
}

fn publish(
    target: &Mutex<Arc<Snapshot<DataSeries>>>,
    aggregate: &Aggregate,
    metric: WikiMetric,
    received_ms: i64,
) {
    // Construct vectors outside the publication lock; render readers never wait.
    let data = Arc::new(aggregate.series(metric));
    if let Ok(mut guard) = target.lock() {
        let mut next = (**guard).clone();
        next.data = Some(data);
        next.state.received(received_ms, aggregate.latest_ms());
        *guard = Arc::new(next);
    }
}

/// One owned SSE connection per feed. Publication cadence is independent of
/// one-second provider bins and does not alter observation or receipt times.
/// No historical seed is invented. Drop requests stop and reaps the finite
/// 60-second connection away from the presentation thread.
pub struct WikiFeed {
    snapshot: Arc<Mutex<Arc<Snapshot<DataSeries>>>>,
    worker: Option<Worker>,
}

impl WikiFeed {
    pub fn start(metric: WikiMetric, publish_seconds: u64) -> Result<Self, String> {
        let interval = Duration::from_secs(publish_seconds.clamp(5, 60));
        let snapshot = Arc::new(Mutex::new(Arc::new(Snapshot::default())));
        let worker_snapshot = Arc::clone(&snapshot);
        let worker = Worker::try_spawn("wikipedia-events", move |stop| {
            ilium_platform::thread_priority::lower_current_thread(
                ilium_platform::thread_priority::WorkerPriority::Lowest,
            );
            let mut aggregate = Aggregate::default();
            let mut failures = 0_u32;
            let mut last_publish = Instant::now();
            let mut last_receipt = 0;
            let mut dirty = false;
            while !stop.load(Ordering::Relaxed) {
                let mut received_edit = false;
                let mut received_line = false;
                let result = http_stream_lines(
                    STREAM_URL,
                    262_144,
                    Duration::from_secs(60),
                    &stop,
                    |line| {
                        if stop.load(Ordering::Relaxed) {
                            return false;
                        }
                        last_receipt = receipt_ms();
                        received_line = true;
                        match decode(line) {
                            Ok(Some(edit)) => {
                                let first = aggregate.buckets.is_empty();
                                if aggregate.accept(edit) {
                                    received_edit = true;
                                    dirty = true;
                                    if first {
                                        publish(&worker_snapshot, &aggregate, metric, last_receipt);
                                        dirty = false;
                                        last_publish = Instant::now();
                                    }
                                }
                            }
                            Ok(None) => {}
                            Err(_) => aggregate.rejected = aggregate.rejected.saturating_add(1),
                        }
                        if !aggregate.buckets.is_empty() && last_publish.elapsed() >= interval {
                            publish(&worker_snapshot, &aggregate, metric, last_receipt);
                            dirty = false;
                            last_publish = Instant::now();
                        }
                        true
                    },
                );
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                if dirty {
                    publish(&worker_snapshot, &aggregate, metric, last_receipt);
                    dirty = false;
                    last_publish = Instant::now();
                }
                aggregate.disconnect();
                // Even a clean EOF denotes a coverage gap, never a zero period.
                if let Ok(mut guard) = worker_snapshot.lock() {
                    let mut next = (**guard).clone();
                    if received_line {
                        next.state.received_ms = Some(last_receipt);
                    }
                    next.state.failed(result.err().map_or_else(
                        || "Wikipedia stream ended; reconnecting".to_owned(),
                        |error| format!("Wikipedia stream disconnected: {error}"),
                    ));
                    *guard = Arc::new(next);
                }
                failures = if received_edit {
                    0
                } else {
                    failures.saturating_add(1)
                };
                let seconds = 5_u64.saturating_mul(1_u64 << failures.min(4)).min(60);
                if !sleep_unless_stopped(&stop, Duration::from_secs(seconds)) {
                    break;
                }
            }
        })
        .map_err(|error| format!("could not start Wikipedia stream worker: {error}"))?;
        Ok(Self {
            snapshot,
            worker: Some(worker),
        })
    }

    pub fn try_snapshot(&self) -> Option<Arc<Snapshot<DataSeries>>> {
        self.snapshot
            .try_lock()
            .ok()
            .map(|guard| Arc::clone(&guard))
    }
}

impl Drop for WikiFeed {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.stop_in_background();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn line(id: &str, second: i64, bot: bool) -> Vec<u8> {
        format!("data: {{\"meta\":{{\"id\":\"{id}\",\"domain\":\"en.wikipedia.org\"}},\"type\":\"edit\",\"server_name\":\"en.wikipedia.org\",\"timestamp\":{second},\"bot\":{bot}}}\n").into_bytes()
    }
    #[test]
    fn filters_non_events_and_invalid_metadata() {
        assert!(decode(b": heartbeat\n").unwrap().is_none());
        assert!(decode(b"event: message\n").unwrap().is_none());
        for (from, to) in [
            ("edit", "new"),
            ("en.wikipedia.org", "en.wiktionary.org"),
            ("en.wikipedia.org", "canary"),
        ] {
            let text = String::from_utf8(line("a", 2, false))
                .unwrap()
                .replace(from, to);
            assert!(decode(text.as_bytes()).unwrap().is_none());
        }
        assert!(decode(&line("a", -1, false)).is_err());
        assert!(decode(b"data: broken\n").is_err());
        assert!(decode(b"data: {\"type\":\"edit\",\"server_name\":\"en.wikipedia.org\",\"timestamp\":2,\"bot\":null}").is_err());
    }
    #[test]
    fn aggregates_provider_seconds_revises_late_events_and_deduplicates() {
        let mut aggregate = Aggregate::default();
        assert!(aggregate.accept(decode(&line("a", 10, false)).unwrap().unwrap()));
        assert!(!aggregate.accept(decode(&line("a", 10, false)).unwrap().unwrap()));
        aggregate.accept(decode(&line("b", 12, true)).unwrap().unwrap());
        aggregate.accept(decode(&line("c", 10, true)).unwrap().unwrap());
        let rate = aggregate.series(WikiMetric::EditRate);
        assert_eq!(
            rate.samples
                .iter()
                .map(|s| (s.observed_ms, s.value))
                .collect::<Vec<_>>(),
            [(10000, 2.0), (11000, 0.0), (12000, 1.0)]
        );
        let bots = aggregate.series(WikiMetric::BotShare);
        assert_eq!(
            bots.samples
                .iter()
                .map(|s| (s.observed_ms, s.value))
                .collect::<Vec<_>>(),
            [(10000, 50.0), (12000, 100.0)]
        );
    }
    #[test]
    fn disconnect_does_not_fabricate_coverage_and_history_is_bounded() {
        let mut aggregate = Aggregate::default();
        aggregate.accept(decode(&line("a", 10, false)).unwrap().unwrap());
        aggregate.disconnect();
        aggregate.accept(decode(&line("b", 20, true)).unwrap().unwrap());
        assert_eq!(aggregate.series(WikiMetric::EditRate).samples.len(), 2);
        aggregate.accept(decode(&line("c", 10000, true)).unwrap().unwrap());
        assert!(aggregate.buckets.len() <= MAX_BUCKETS);
        assert!(!aggregate.accept(decode(&line("old", 10, false)).unwrap().unwrap()));
        assert_eq!(aggregate.latest_ms(), Some(10_000_000));
    }
    #[test]
    fn publication_keeps_actual_input_and_provider_times_and_failure_retains_data() {
        let mut aggregate = Aggregate::default();
        aggregate.accept(decode(&line("a", 10, false)).unwrap().unwrap());
        let target = Mutex::new(Arc::new(Snapshot::default()));
        publish(&target, &aggregate, WikiMetric::EditRate, 12001);
        let original = target.lock().unwrap().clone();
        assert_eq!(original.state.received_ms, Some(12001));
        assert_eq!(original.state.observed_ms, Some(10000));
        publish(&target, &aggregate, WikiMetric::EditRate, 12001);
        let mut failed = (**target.lock().unwrap()).clone();
        failed.state.failed("disconnected".into());
        assert_eq!(failed.state.received_ms, original.state.received_ms);
        assert_eq!(failed.state.observed_ms, original.state.observed_ms);
        assert_eq!(
            failed.data.as_ref().unwrap().samples,
            original.data.as_ref().unwrap().samples
        );
    }
    #[test]
    fn empty_snapshot_and_contended_render_reads_do_not_wait_or_seed_history() {
        let feed = WikiFeed {
            snapshot: Arc::new(Mutex::new(Arc::new(Snapshot::default()))),
            worker: None,
        };
        assert!(feed.try_snapshot().unwrap().data.is_none());
        let _guard = feed.snapshot.lock().unwrap();
        assert!(feed.try_snapshot().is_none());
    }
    #[test]
    fn event_and_identity_storage_remain_bounded() {
        assert!(decode(&line("overflow", i64::MAX, false)).is_err());
        assert!(decode(&line(&"x".repeat(257), 10, false)).is_err());
        let mut oversized = b"data:".to_vec();
        oversized.extend(vec![b' '; 262_145]);
        assert!(decode(&oversized).is_err());
        let mut aggregate = Aggregate::default();
        for index in 0..MAX_IDS + 10 {
            aggregate.accept(Edit {
                id: index.to_string(),
                second: 10,
                bot: false,
            });
        }
        assert_eq!(aggregate.ids.len(), MAX_IDS);
        assert_eq!(aggregate.id_order.len(), MAX_IDS);
        assert_eq!(
            aggregate.series(WikiMetric::EditRate).samples[0].value,
            (MAX_IDS + 10) as f64
        );
    }
    #[test]
    fn late_reconnect_event_cannot_fill_the_outage() {
        let mut aggregate = Aggregate::default();
        aggregate.accept(decode(&line("a", 10, false)).unwrap().unwrap());
        aggregate.accept(decode(&line("b", 20, true)).unwrap().unwrap());
        aggregate.disconnect();
        aggregate.accept(decode(&line("late", 15, false)).unwrap().unwrap());
        aggregate.accept(decode(&line("same", 20, false)).unwrap().unwrap());
        aggregate.accept(decode(&line("c", 30, true)).unwrap().unwrap());
        assert!(!aggregate.buckets.contains_key(&25));
        assert_eq!(aggregate.buckets[&15].edits, 1);
    }
}
