//! api-durable-timers.md, journal half: `journal.log-event` and `journal.read`.

use ilium_plugin_api::generated::host::HostJournal;
use ilium_plugin_api::generated::types::{EpochMs, ErrorCode};
use ilium_plugin_api::Tier;
use proptest::prelude::*;

use crate::support::*;

fn payload(bytes: usize) -> String {
    let overhead = r#"{"v":""}"#.len();
    format!(r#"{{"v":"{}"}}"#, "a".repeat(bytes - overhead))
}

/// REQ: WIT:ilium:host/journal.log-event
#[test]
fn journal_log_event_appends_an_entry_readable_back() {
    let mut fixture = fixture(Tier::T2, &[]);
    expect_ok(
        fixture
            .journal()
            .log_event(None, "note".to_owned(), r#"{"a":1}"#.to_owned()),
    );
    let entries = fixture.journal().read(None, EpochMs(0), 10);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].kind, "note");
    assert_eq!(entries[0].payload_json, r#"{"a":1}"#);
    assert_eq!(entries[0].source, PLUGIN);
}

/// REQ: WIT:ilium:host/journal.log-event
#[test]
fn journal_log_event_is_stamped_with_the_clock() {
    let mut fixture = fixture(Tier::T2, &[]);
    fixture.advance(std::time::Duration::from_secs(10));
    expect_ok(
        fixture
            .journal()
            .log_event(None, "note".to_owned(), "{}".to_owned()),
    );
    let entries = fixture.journal().read(None, EpochMs(0), 10);
    assert_eq!(entries[0].at.0, TEST_CLOCK_START_MILLIS + 10_000);
}

/// REQ: WIT:ilium:host/journal.log-event
#[test]
fn journal_log_event_payload_of_exactly_4_kib_accepted_and_one_more_invalid_argument() {
    let mut fixture = fixture(Tier::T2, &[]);
    expect_ok(
        fixture
            .journal()
            .log_event(None, "k".to_owned(), payload(4 * KIB)),
    );
    assert_eq!(
        expect_code(
            fixture
                .journal()
                .log_event(None, "k".to_owned(), payload(4 * KIB + 1))
        ),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/journal.log-event
#[test]
fn journal_log_event_payload_that_is_not_json_invalid_argument() {
    let mut fixture = fixture(Tier::T2, &[]);
    assert_eq!(
        expect_code(
            fixture
                .journal()
                .log_event(None, "k".to_owned(), "{not json".to_owned())
        ),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/journal.log-event
#[test]
fn journal_log_event_full_quota_quota_exceeded() {
    let mut fixture = builder_in(std::path::Path::new("."), Tier::T2, &[])
        .limit("journal_entries", 3)
        .build();
    for _ in 0..3 {
        expect_ok(
            fixture
                .journal()
                .log_event(None, "k".to_owned(), "{}".to_owned()),
        );
    }
    assert_eq!(
        expect_code(
            fixture
                .journal()
                .log_event(None, "k".to_owned(), "{}".to_owned())
        ),
        ErrorCode::QuotaExceeded
    );
}

/// REQ: WIT:ilium:host/journal.log-event
#[test]
fn journal_log_event_is_always_granted_without_any_capability() {
    let mut fixture = fixture(Tier::T2, &[]);
    assert!(fixture
        .journal()
        .log_event(None, "k".to_owned(), "{}".to_owned())
        .is_ok());
}

/// REQ: WIT:ilium:host/journal.read
#[test]
fn journal_read_since_filters_older_entries() {
    let mut fixture = fixture(Tier::T2, &[]);
    expect_ok(
        fixture
            .journal()
            .log_event(None, "old".to_owned(), "{}".to_owned()),
    );
    fixture.advance(std::time::Duration::from_secs(5));
    expect_ok(
        fixture
            .journal()
            .log_event(None, "new".to_owned(), "{}".to_owned()),
    );
    let entries = fixture
        .journal()
        .read(None, EpochMs(TEST_CLOCK_START_MILLIS + 5000), 10);
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.kind.as_str())
            .collect::<Vec<_>>(),
        ["new"]
    );
}

/// REQ: WIT:ilium:host/journal.read
#[test]
fn journal_read_limit_bounds_the_result_and_zero_gives_nothing() {
    let mut fixture = fixture(Tier::T2, &[]);
    for _ in 0..5 {
        expect_ok(
            fixture
                .journal()
                .log_event(None, "k".to_owned(), "{}".to_owned()),
        );
    }
    assert_eq!(fixture.journal().read(None, EpochMs(0), 2).len(), 2);
    assert!(fixture.journal().read(None, EpochMs(0), 0).is_empty());
}

/// REQ: WIT:ilium:host/journal.read
#[test]
fn journal_read_huge_limit_is_clamped_and_does_not_fail() {
    let mut fixture = fixture(Tier::T2, &[]);
    expect_ok(
        fixture
            .journal()
            .log_event(None, "k".to_owned(), "{}".to_owned()),
    );
    assert_eq!(fixture.journal().read(None, EpochMs(0), u32::MAX).len(), 1);
}

/// REQ: WIT:ilium:host/journal.read
#[test]
fn journal_read_without_the_grant_returns_only_the_plugins_own_entries() {
    let mut fixture = fixture(Tier::T2, &[]);
    let pane = fixture.add_terminal_pane("a", 80, 24);
    fixture.record_host_journal_entry(pane, "pane-created");
    expect_ok(
        fixture
            .journal()
            .log_event(Some(pane), "mine".to_owned(), "{}".to_owned()),
    );
    let entries = fixture.journal().read(Some(pane), EpochMs(0), 50);
    assert!(
        entries.iter().all(|entry| entry.source == PLUGIN),
        "{entries:?}"
    );
    assert_eq!(entries.len(), 1);
}

/// REQ: WIT:ilium:host/journal.read
#[test]
fn journal_read_with_the_grant_includes_the_host_kinds() {
    let mut fixture = fixture(Tier::T2, &["journal.read"]);
    let pane = fixture.add_terminal_pane("a", 80, 24);
    fixture.record_host_journal_entry(pane, "pane-created");
    fixture.record_host_journal_entry(pane, "input-sent");
    fixture.record_host_journal_entry(pane, "pane-exited");
    let kinds: Vec<String> = fixture
        .journal()
        .read(Some(pane), EpochMs(0), 50)
        .into_iter()
        .map(|entry| entry.kind)
        .collect();
    for kind in ["pane-created", "input-sent", "pane-exited"] {
        assert!(
            kinds.iter().any(|candidate| candidate == kind),
            "{kind}: {kinds:?}"
        );
    }
}

/// REQ: WIT:ilium:host/journal.read
#[test]
fn journal_read_for_a_pane_returns_only_that_panes_entries() {
    let mut fixture = fixture(Tier::T2, &[]);
    let first = fixture.add_terminal_pane("a", 80, 24);
    let second = fixture.add_terminal_pane("b", 80, 24);
    expect_ok(
        fixture
            .journal()
            .log_event(Some(first), "one".to_owned(), "{}".to_owned()),
    );
    expect_ok(
        fixture
            .journal()
            .log_event(Some(second), "two".to_owned(), "{}".to_owned()),
    );
    let entries = fixture.journal().read(Some(first), EpochMs(0), 10);
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.kind.as_str())
            .collect::<Vec<_>>(),
        ["one"]
    );
}

/// REQ: WIT:ilium:host/journal.read
#[test]
fn journal_read_entries_come_back_in_time_order() {
    let mut fixture = fixture(Tier::T2, &[]);
    for name in ["a", "b", "c"] {
        expect_ok(
            fixture
                .journal()
                .log_event(None, name.to_owned(), "{}".to_owned()),
        );
        fixture.advance(std::time::Duration::from_millis(10));
    }
    let kinds: Vec<String> = fixture
        .journal()
        .read(None, EpochMs(0), 10)
        .into_iter()
        .map(|e| e.kind)
        .collect();
    assert_eq!(kinds, ["a", "b", "c"]);
}

proptest! {
    // REQ: WIT:ilium:host/journal.read, WIT:ilium:host/journal.log-event
    #[test]
    fn journal_read_never_returns_more_than_limit_and_never_fails(count in 0usize..30, limit in 0u32..40) {
        let mut fixture = fixture(Tier::T2, &[]);
        for _ in 0..count {
            expect_ok(fixture.journal().log_event(None, "k".to_owned(), "{}".to_owned()));
        }
        let entries = fixture.journal().read(None, EpochMs(0), limit);
        prop_assert_eq!(entries.len(), count.min(limit as usize));
    }
}
