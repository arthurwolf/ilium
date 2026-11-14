//! Numeric Wikipedia edit aggregates; article titles and editor identities are never retained.
//!
//! The provider is a server-sent-event stream. The scene world has no stream import (a scene
//! exports no `handler`, so a stream's events are dropped), so the aggregation is complete here
//! and fed line by line through [`WikiFeed::push_line`], but nothing connects the stream yet:
//! the scene reports the source as unavailable instead of showing invented data.
use super::{model::Observation, poll::Snapshot, series::DataSeries};
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::sync::Arc;

const MAX_BUCKETS: usize = 4096;
const MAX_IDS: usize = 8192;

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

/// Why the Wikipedia source shows no data in a scene.
pub const STREAM_UNAVAILABLE: &str =
    "Wikipedia live stream unavailable: the scene host offers no stream connection";

/// The aggregation of one feed and the snapshot it publishes.
pub struct WikiFeed {
    metric: WikiMetric,
    aggregate: Aggregate,
    snapshot: Arc<Snapshot<DataSeries>>,
}

impl WikiFeed {
    /// A feed with no connection: its snapshot carries [`STREAM_UNAVAILABLE`] until lines
    /// arrive through [`WikiFeed::push_line`].
    pub fn unconnected(metric: WikiMetric) -> Self {
        let mut snapshot = Snapshot::default();
        snapshot.state.failed(STREAM_UNAVAILABLE.into());
        Self {
            metric,
            aggregate: Aggregate::default(),
            snapshot: Arc::new(snapshot),
        }
    }

    /// Feeds one stream line received at `received_ms` and republishes when it was an edit.
    pub fn push_line(&mut self, line: &[u8], received_ms: i64) {
        match decode(line) {
            Ok(Some(edit)) => {
                if self.aggregate.accept(edit) {
                    let mut next = (*self.snapshot).clone();
                    next.data = Some(Arc::new(self.aggregate.series(self.metric)));
                    next.state.received(received_ms, self.aggregate.latest_ms());
                    self.snapshot = Arc::new(next);
                }
            }
            Ok(None) => {}
            Err(_) => self.aggregate.rejected = self.aggregate.rejected.saturating_add(1),
        }
    }

    /// The stream ended: the gap is never filled with zero bins.
    pub fn disconnected(&mut self, reason: &str) {
        self.aggregate.disconnect();
        let mut next = (*self.snapshot).clone();
        next.state
            .failed(format!("Wikipedia stream disconnected: {reason}"));
        self.snapshot = Arc::new(next);
    }

    pub fn snapshot(&self) -> Arc<Snapshot<DataSeries>> {
        Arc::clone(&self.snapshot)
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
    fn feed_publishes_actual_input_and_provider_times_and_a_disconnect_retains_data() {
        let mut feed = WikiFeed::unconnected(WikiMetric::EditRate);
        assert!(feed.snapshot().data.is_none());
        assert_eq!(
            feed.snapshot().state.error.as_deref(),
            Some(STREAM_UNAVAILABLE)
        );
        feed.push_line(&line("a", 10, false), 12001);
        let original = feed.snapshot();
        assert_eq!(original.state.received_ms, Some(12001));
        assert_eq!(original.state.observed_ms, Some(10000));
        assert!(original.state.error.is_none());
        feed.disconnected("reset");
        let failed = feed.snapshot();
        assert_eq!(failed.state.received_ms, original.state.received_ms);
        assert_eq!(failed.state.observed_ms, original.state.observed_ms);
        assert!(failed.state.error.is_some());
        assert_eq!(
            failed.data.as_ref().unwrap().samples,
            original.data.as_ref().unwrap().samples
        );
    }

    #[test]
    fn an_unconnected_feed_never_seeds_history() {
        let feed = WikiFeed::unconnected(WikiMetric::BotShare);
        assert!(feed.snapshot().data.is_none());
        assert!(feed.snapshot().state.received_ms.is_none());
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
