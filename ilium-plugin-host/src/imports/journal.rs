//! Host implementation of `ilium:host/journal`.
//!
//! One bounded in-memory journal per runtime ([`JournalBook`]), shared by the imports of its
//! plugins. Writes are limited to 20 per second per plugin and event kind, and payloads to
//! 4 KiB. Two runtimes in one process keep two journals.

use ilium_plugin_api::generated::host::journal::{HostJournal, JournalEntry};
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_api::generated::types::ids::{EpochMs, NodeId};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const PAYLOAD_LIMIT_BYTES: usize = 4 * 1024;
const READ_CAP_ENTRIES: usize = 1_000;
const RETAINED_ENTRIES: usize = 10_000;
const RATE_PER_WINDOW: u32 = 20;
const RATE_WINDOW_MILLIS: u64 = 1_000;

#[derive(Default)]
struct JournalState {
    entries: VecDeque<JournalEntry>,
    /// Per (plugin, kind): window start and number of accepted writes in it.
    windows: HashMap<(String, String), (Instant, u32)>,
}

/// The journal of one runtime. Cloning shares the entries.
#[derive(Debug, Clone, Default)]
pub struct JournalBook {
    state: Arc<Mutex<JournalState>>,
}

impl std::fmt::Debug for JournalState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("JournalState")
            .field("entries", &self.entries.len())
            .finish()
    }
}

impl JournalBook {
    /// An empty journal.
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, JournalState> {
        // Every writer pushes one entry, so the journal is consistent after a panic.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// How many entries the journal holds.
    pub fn len(&self) -> usize {
        self.lock().entries.len()
    }

    /// Whether the journal holds no entry.
    pub fn is_empty(&self) -> bool {
        self.lock().entries.is_empty()
    }

    /// Drops the rate windows of a plugin that stopped; its entries stay in the journal.
    pub fn forget_plugin(&self, plugin: &str) {
        self.lock().windows.retain(|(owner, _), _| owner != plugin);
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// Host side of the `journal` interface of one plugin. Cloning shares the journal.
#[derive(Debug, Clone)]
pub struct JournalImport {
    book: JournalBook,
    plugin: String,
}

impl Default for JournalImport {
    fn default() -> Self {
        Self::new(JournalBook::new(), "ilium.host.standalone")
    }
}

impl JournalImport {
    /// The import of `plugin` over the runtime's journal.
    pub fn new(book: JournalBook, plugin: &str) -> Self {
        Self {
            book,
            plugin: plugin.to_owned(),
        }
    }
}

impl HostJournal for JournalImport {
    fn log_event(
        &mut self,
        pane: Option<NodeId>,
        kind: String,
        payload_json: String,
    ) -> Result<(), PluginError> {
        if payload_json.len() > PAYLOAD_LIMIT_BYTES {
            return Err(PluginError {
                code: ErrorCode::InvalidArgument,
                message: "payload-json exceeds 4 KiB".to_string(),
                retry_after_ms: None,
            });
        }
        let mut state = self.book.lock();
        let now = Instant::now();
        let window = state
            .windows
            .entry((self.plugin.clone(), kind.clone()))
            .or_insert((now, 0));
        let elapsed_millis = u64::try_from(now.duration_since(window.0).as_millis()).unwrap_or(u64::MAX);
        if elapsed_millis >= RATE_WINDOW_MILLIS {
            *window = (now, 0);
        }
        if window.1 >= RATE_PER_WINDOW {
            let wait = RATE_WINDOW_MILLIS.saturating_sub(elapsed_millis).max(1);
            return Err(PluginError {
                code: ErrorCode::RateLimited,
                message: "journal.log-event is limited to 20 calls per second".to_string(),
                retry_after_ms: Some(u32::try_from(wait).unwrap_or(u32::MAX)),
            });
        }
        window.1 += 1;
        if state.entries.len() >= RETAINED_ENTRIES {
            state.entries.pop_front();
        }
        state.entries.push_back(JournalEntry {
            at: EpochMs(now_millis()),
            pane,
            source: "plugin".to_string(),
            kind,
            payload_json,
        });
        Ok(())
    }

    fn read(&mut self, pane: Option<NodeId>, since: EpochMs, limit: u32) -> Vec<JournalEntry> {
        let wanted = (limit as usize).min(READ_CAP_ENTRIES);
        let state = self.book.lock();
        let mut newest_first: Vec<JournalEntry> = state
            .entries
            .iter()
            .rev()
            .filter(|entry| entry.at.0 >= since.0 && (pane.is_none() || entry.pane == pane))
            .take(wanted)
            .cloned()
            .collect();
        newest_first.reverse();
        newest_first
    }
}

#[cfg(test)]
mod tests {
    use super::JournalImport;
    use ilium_plugin_api::generated::host::journal::{HostJournal, JournalEntry};
    use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
    use ilium_plugin_api::generated::types::ids::{EpochMs, NodeId};

    /// Largest `payload-json` a journal entry accepts, in bytes (4 KiB, per-call cap table).
    const PAYLOAD_LIMIT_BYTES: usize = 4 * 1024;

    /// Largest number of entries one `read` call returns (`journal.read` limit clamp).
    const READ_CAP_ENTRIES: usize = 1_000;

    /// Calls `log-event` on a fresh import with its own journal, as the plugin side does.
    fn log_event(pane: Option<NodeId>, kind: &str, payload_json: &str) -> Result<(), PluginError> {
        let mut journal = JournalImport::default();
        journal.log_event(pane, kind.to_string(), payload_json.to_string())
    }

    /// Returns a kind string for one test.
    fn unique_kind(label: &str) -> String {
        format!("test.journal.{label}")
    }

    /// Asserts that the call returned an `invalid-argument` error.
    fn assert_invalid_argument(result: Result<(), PluginError>) {
        match result {
            Err(error) => assert_eq!(
                error.code,
                ErrorCode::InvalidArgument,
                "message: {}",
                error.message
            ),
            Ok(()) => panic!("expected invalid-argument, got Ok"),
        }
    }

    /// REQ: CAP-011
    #[test]
    fn log_event_without_pane_returns_ok() {
        assert_eq!(log_event(None, &unique_kind("no-pane"), "{}"), Ok(()));
    }

    /// REQ: CAP-011
    #[test]
    fn log_event_with_pane_returns_ok() {
        assert_eq!(
            log_event(Some(NodeId(1)), &unique_kind("with-pane"), "{\"count\":2}"),
            Ok(())
        );
    }

    /// REQ: API-060
    #[test]
    fn log_event_payload_at_four_kib_boundary_is_accepted() {
        let padding = "x".repeat(PAYLOAD_LIMIT_BYTES - "{\"v\":\"\"}".len());
        let payload = format!("{{\"v\":\"{padding}\"}}");
        assert_eq!(payload.len(), PAYLOAD_LIMIT_BYTES);
        assert_eq!(
            log_event(None, &unique_kind("payload-boundary"), &payload),
            Ok(())
        );
    }

    /// REQ: API-060
    #[test]
    fn log_event_payload_one_byte_over_four_kib_is_refused_as_invalid_argument() {
        let padding = "x".repeat(PAYLOAD_LIMIT_BYTES + 1 - "{\"v\":\"\"}".len());
        let payload = format!("{{\"v\":\"{padding}\"}}");
        assert_eq!(payload.len(), PAYLOAD_LIMIT_BYTES + 1);
        assert_invalid_argument(log_event(None, &unique_kind("payload-over"), &payload));
    }

    /// REQ: API-060
    #[test]
    fn log_event_oversized_payload_is_not_silently_truncated() {
        let payload = "y".repeat(PAYLOAD_LIMIT_BYTES * 2);
        assert_invalid_argument(log_event(
            Some(NodeId(2)),
            &unique_kind("payload-huge"),
            &payload,
        ));
    }

    /// REQ: API-071
    #[test]
    fn log_event_burst_beyond_twenty_per_second_is_rate_limited_with_retry_after() {
        let kind = unique_kind("burst");
        let mut journal = JournalImport::default();
        let mut accepted_count = 0usize;
        let mut rate_limited_retry_after: Option<u32> = None;
        let mut saw_rate_limited = false;
        for _ in 0..25 {
            match journal.log_event(None, kind.clone(), "{}".to_string()) {
                Ok(()) => accepted_count += 1,
                Err(error) => {
                    assert_eq!(
                        error.code,
                        ErrorCode::RateLimited,
                        "message: {}",
                        error.message
                    );
                    saw_rate_limited = true;
                    rate_limited_retry_after = error.retry_after_ms;
                }
            }
        }
        assert!(
            saw_rate_limited,
            "25 calls in one burst must hit the 20-per-second rate"
        );
        assert!(
            accepted_count <= 20,
            "accepted {accepted_count} calls, the rate is 20 per second"
        );
        assert!(
            rate_limited_retry_after.is_some(),
            "rate-limited must carry retry-after-ms"
        );
    }

    /// REQ: CAP-011
    #[test]
    fn read_returns_own_entry_with_same_kind_pane_and_payload() {
        let kind = unique_kind("round-trip");
        let pane = NodeId(7);
        let mut journal = JournalImport::default();
        journal
            .log_event(Some(pane), kind.clone(), "{\"key\":\"abc\"}".to_string())
            .expect("log-event must be accepted");
        let entries: Vec<JournalEntry> = journal.read(Some(pane), EpochMs(0), 1_000);
        let matching: Vec<&JournalEntry> =
            entries.iter().filter(|entry| entry.kind == kind).collect();
        assert_eq!(
            matching.len(),
            1,
            "the written entry must be readable without any grant"
        );
        assert_eq!(matching[0].pane, Some(pane));
        assert_eq!(matching[0].payload_json, "{\"key\":\"abc\"}");
    }

    /// REQ: CAP-011
    #[test]
    fn read_lists_entries_newest_last() {
        let first_kind = unique_kind("order-first");
        let second_kind = unique_kind("order-second");
        let mut journal = JournalImport::default();
        journal
            .log_event(None, first_kind.clone(), "{}".to_string())
            .expect("first log-event must be accepted");
        journal
            .log_event(None, second_kind.clone(), "{}".to_string())
            .expect("second log-event must be accepted");
        let entries: Vec<JournalEntry> = journal.read(None, EpochMs(0), 1_000);
        let first_index = entries.iter().rposition(|entry| entry.kind == first_kind);
        let second_index = entries.iter().rposition(|entry| entry.kind == second_kind);
        match (first_index, second_index) {
            (Some(first), Some(second)) => assert!(first < second, "newest entry must come last"),
            _ => panic!("both written entries must be returned"),
        }
    }

    /// REQ: CAP-011
    #[test]
    fn read_since_in_the_far_future_returns_no_entries() {
        let mut journal = JournalImport::default();
        let entries: Vec<JournalEntry> = journal.read(None, EpochMs(u64::MAX), 1_000);
        assert!(
            entries.is_empty(),
            "no entry can be newer than the far-future since value"
        );
    }

    /// REQ: API-060
    #[test]
    fn read_limit_above_cap_returns_at_most_one_thousand_entries() {
        let mut journal = JournalImport::default();
        let entries: Vec<JournalEntry> = journal.read(None, EpochMs(0), u32::MAX);
        assert!(
            entries.len() <= READ_CAP_ENTRIES,
            "a limit above 1,000 is clamped to 1,000, got {} entries",
            entries.len()
        );
    }

    /// REQ: CAP-011
    #[test]
    fn read_with_no_matching_pane_entries_returns_empty_list() {
        let mut journal = JournalImport::default();
        let entries: Vec<JournalEntry> =
            journal.read(Some(NodeId(u64::MAX)), EpochMs(u64::MAX), 10);
        assert!(entries.is_empty());
    }

    #[test]
    fn two_journals_do_not_see_each_other() {
        let mut first = JournalImport::default();
        let mut second = JournalImport::default();
        first
            .log_event(None, unique_kind("separate"), "{}".to_string())
            .expect("accepted");
        assert_eq!(first.read(None, EpochMs(0), 10).len(), 1);
        assert!(second.read(None, EpochMs(0), 10).is_empty());
    }

    #[test]
    fn the_rate_limit_is_kept_per_plugin() {
        let book = super::JournalBook::new();
        let mut busy = JournalImport::new(book.clone(), "a.busy");
        let mut quiet = JournalImport::new(book.clone(), "a.quiet");
        let kind = unique_kind("per-plugin");
        for _ in 0..20 {
            busy.log_event(None, kind.clone(), "{}".to_string())
                .expect("within the rate");
        }
        let refused = busy
            .log_event(None, kind.clone(), "{}".to_string())
            .expect_err("the busy plugin is limited");
        assert_eq!(refused.code, ErrorCode::RateLimited);
        quiet
            .log_event(None, kind, "{}".to_string())
            .expect("another plugin has its own window");
        assert_eq!(book.len(), 21);
    }
}
