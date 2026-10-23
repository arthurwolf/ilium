//! Host implementation of `ilium:host/journal` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostJournal` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::journal::{HostJournal, JournalEntry};
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::ids::{EpochMs, NodeId};

/// Host side of the `journal` interface.
#[derive(Debug)]
pub struct JournalImport;

impl HostJournal for JournalImport {
    fn log_event(
        &mut self,
        _pane: Option<NodeId>,
        _kind: String,
        _payload_json: String,
    ) -> Result<(), PluginError> {
        unimplemented!("host::imports::journal::log_event")
    }

    fn read(&mut self, _pane: Option<NodeId>, _since: EpochMs, _limit: u32) -> Vec<JournalEntry> {
        unimplemented!("host::imports::journal::read")
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

    /// Calls `log-event` on a fresh import, as the plugin side does.
    fn log_event(pane: Option<NodeId>, kind: &str, payload_json: &str) -> Result<(), PluginError> {
        let mut journal = JournalImport;
        journal.log_event(pane, kind.to_string(), payload_json.to_string())
    }

    /// Returns a kind string unique to one test, so entries written by parallel tests can be told apart.
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
        let mut journal = JournalImport;
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
        log_event(Some(pane), &kind, "{\"key\":\"abc\"}").expect("log-event must be accepted");
        let mut journal = JournalImport;
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
        log_event(None, &first_kind, "{}").expect("first log-event must be accepted");
        log_event(None, &second_kind, "{}").expect("second log-event must be accepted");
        let mut journal = JournalImport;
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
        let mut journal = JournalImport;
        let entries: Vec<JournalEntry> = journal.read(None, EpochMs(u64::MAX), 1_000);
        assert!(
            entries.is_empty(),
            "no entry can be newer than the far-future since value"
        );
    }

    /// REQ: API-060
    #[test]
    fn read_limit_above_cap_returns_at_most_one_thousand_entries() {
        let mut journal = JournalImport;
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
        let mut journal = JournalImport;
        let entries: Vec<JournalEntry> =
            journal.read(Some(NodeId(u64::MAX)), EpochMs(u64::MAX), 10);
        assert!(entries.is_empty());
    }
}
