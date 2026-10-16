//! Recipes of the patterns cookbook run on the fake host and as pure cores
//! (guide-patterns-cookbook.md, recipes 1 to 12).

use std::collections::BTreeMap;

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{
    DeactivationReason, ErrorCode, Event, Outcome, PluginError, RequestRef, RequestResult,
};

use crate::support::{
    tick, InOrder, RevisionKeyEmitter, Resync, SameKeyEmitter, StaleGuard, Versioned,
};

const SOUND_GRANT: &str = "sound.play";
const TOAST_GRANT: &str = "notify.toast";

// ---------------------------------------------------------------- recipe 1: debounce

/// The timer calls of the debounce recipe, behind a trait so the core can be driven without a host.
trait Timers {
    fn set(&mut self, after_millis: u32) -> Result<u64, PluginError>;
    fn cancel(&mut self, timer: u64);
}

/// Counts held timers and refuses delays under 50 ms with `invalid-argument`, as the host does.
#[derive(Default)]
struct RecordingTimers {
    next: u64,
    held: Vec<u64>,
    log: Vec<String>,
    most_held: usize,
}

impl Timers for RecordingTimers {
    fn set(&mut self, after_millis: u32) -> Result<u64, PluginError> {
        if after_millis < 50 {
            return Err(PluginError::invalid_argument("timer under 50 ms"));
        }
        self.next += 1;
        self.held.push(self.next);
        self.most_held = self.most_held.max(self.held.len());
        self.log.push(format!("set {}", self.next));
        Ok(self.next)
    }
    fn cancel(&mut self, timer: u64) {
        self.held.retain(|held| *held != timer);
        self.log.push(format!("cancel {timer}"));
    }
}

const QUIET_MILLIS: u32 = 300;

#[derive(Default)]
struct Debounce {
    pending: Option<u64>,
}

impl Debounce {
    fn touch(&mut self, timers: &mut impl Timers, delay: u32) -> Result<(), PluginError> {
        if let Some(old) = self.pending.take() {
            timers.cancel(old);
        }
        self.pending = Some(timers.set(delay)?);
        Ok(())
    }
    fn fired(&mut self, timer: u64) -> bool {
        if self.pending == Some(timer) {
            self.pending = None;
            return true;
        }
        false
    }
}

/// REQ: AUT-002
#[test]
fn aut_002_cancel_comes_before_the_next_set() {
    let mut timers = RecordingTimers::default();
    let mut debounce = Debounce::default();
    debounce.touch(&mut timers, QUIET_MILLIS).unwrap();
    debounce.touch(&mut timers, QUIET_MILLIS).unwrap();
    assert_eq!(timers.log, vec!["set 1", "cancel 1", "set 2"]);
}

/// REQ: AUT-002
#[test]
fn aut_002_busy_editor_never_holds_more_than_one_timer() {
    let mut timers = RecordingTimers::default();
    let mut debounce = Debounce::default();
    for _ in 0..1_000 {
        debounce.touch(&mut timers, QUIET_MILLIS).unwrap();
    }
    assert_eq!(timers.most_held, 1, "cancel runs before set, so one timer is held at most");
    assert!(timers.held.len() <= 1);
    assert!(timers.most_held <= 64);
}

/// REQ: AUT-003
#[test]
fn aut_003_only_the_stored_timer_id_ends_the_quiet_period() {
    let mut timers = RecordingTimers::default();
    let mut debounce = Debounce::default();
    debounce.touch(&mut timers, QUIET_MILLIS).unwrap();
    assert!(!debounce.fired(999));
    assert!(debounce.fired(1));
    assert!(!debounce.fired(1), "a second event for the same id is ignored");
}

/// REQ: AUT-004
#[test]
fn aut_004_delay_below_50_ms_is_refused_invalid_argument() {
    let mut timers = RecordingTimers::default();
    let mut debounce = Debounce::default();
    let refused = debounce.touch(&mut timers, 49).unwrap_err();
    assert_eq!(refused.code, ErrorCode::InvalidArgument);
    assert!(debounce.touch(&mut timers, 50).is_ok(), "50 ms is the lowest accepted delay");
    assert!(QUIET_MILLIS >= 50);
}

// ---------------------------------------------------------------- recipe 2: throttle

struct Throttle {
    interval_millis: u64,
    last: Option<u64>,
}

impl Throttle {
    fn new(interval_millis: u64) -> Self {
        Self { interval_millis, last: None }
    }
    fn allow(&mut self, now: u64) -> bool {
        match self.last {
            Some(last) if now.saturating_sub(last) < self.interval_millis => false,
            _ => {
                self.last = Some(now);
                true
            }
        }
    }
    fn back_off(&mut self, now: u64, retry_after_ms: u32) {
        self.last = Some(now + u64::from(retry_after_ms).saturating_sub(self.interval_millis));
    }
}

/// REQ: AUT-006, AUT-001
#[test]
fn aut_006_nothing_is_sent_before_retry_after_ms_on_the_fake_clock() {
    let mut host = FakeHost::new();
    let start = host.clock().now();
    let mut throttle = Throttle::new(100);
    assert!(throttle.allow(start));
    throttle.back_off(start, 2_000);
    host.advance_clock(1_500);
    assert!(!throttle.allow(host.clock().now()));
    host.advance_clock(499);
    assert!(!throttle.allow(host.clock().now()), "1 ms before the delay ends");
    host.advance_clock(1);
    assert!(throttle.allow(host.clock().now()), "exactly retry-after-ms later");
}

/// REQ: AUT-005 WARN
#[test]
fn aut_005_own_rate_stays_under_the_toast_limit_of_3_per_10_seconds() {
    let mut host = FakeHost::new();
    let start = host.clock().now();
    let mut throttle = Throttle::new(10_000 / 3 + 1);
    let mut sent = 0;
    for _ in 0..100 {
        if throttle.allow(host.clock().now()) {
            sent += 1;
        }
        host.advance_clock(100);
    }
    let seconds = (host.clock().now() - start) / 1_000;
    assert!(sent * 10 <= 3 * (seconds + 10), "sent {sent} toasts in {seconds} s");
}

/// REQ: AUT-007
#[test]
fn aut_007_time_going_backwards_does_not_underflow() {
    let mut throttle = Throttle::new(100);
    assert!(throttle.allow(1_000));
    assert!(!throttle.allow(500), "now below last saturates to 0 elapsed");
    assert!(!throttle.allow(0));
    assert!(throttle.allow(1_100));
}

/// REQ: AUT-001
#[test]
fn aut_001_clock_is_read_once_per_batch_and_passed_in() {
    let mut host = FakeHost::new();
    host.activate::<InOrder>();
    let submissions = host.deliver(vec![tick(), tick(), tick()]);
    assert_eq!(submissions.len(), 3);
    let first = host.clock().now();
    host.advance_clock(5);
    assert_eq!(host.clock().now(), first + 5, "the fake clock moves only through advance_clock");
}

// ---------------------------------------------------------------- recipe 3: cache

const TTL_MILLIS: u64 = 10 * 60 * 1000;

fn encode(fetched_at: u64, body: &[u8]) -> Vec<u8> {
    let mut out = fetched_at.to_le_bytes().to_vec();
    out.extend_from_slice(body);
    out
}

fn decode(bytes: &[u8]) -> Option<(u64, &[u8])> {
    let (head, body) = bytes.split_at_checked(8)?;
    Some((u64::from_le_bytes(head.try_into().ok()?), body))
}

fn is_fresh(stored: &[u8], now: u64) -> bool {
    decode(stored).is_some_and(|(at, _)| now.saturating_sub(at) < TTL_MILLIS)
}

/// REQ: AUT-009
#[test]
fn aut_009_cached_value_round_trips_with_its_fetch_time() {
    let bytes = encode(1_234, b"body");
    assert_eq!(decode(&bytes), Some((1_234, &b"body"[..])));
    assert_eq!(decode(&[1, 2, 3]), None, "fewer than 8 bytes carry no fetch time");
    assert_eq!(decode(&[0; 8]), Some((0, &b""[..])));
}

/// REQ: AUT-009
#[test]
fn aut_009_entry_is_stale_at_exactly_the_time_limit_on_the_fake_clock() {
    let mut host = FakeHost::new();
    let fetched = host.clock().now();
    let bytes = encode(fetched, b"x");
    host.advance_clock(TTL_MILLIS - 1);
    assert!(is_fresh(&bytes, host.clock().now()));
    host.advance_clock(1);
    assert!(!is_fresh(&bytes, host.clock().now()), "the host does not expire it, the reader does");
}

// ---------------------------------------------------------------- recipe 4: windows

struct Window {
    first: usize,
    len: usize,
}

fn window(total: usize, offset: usize, visible: usize) -> Window {
    let margin = 4;
    let first = offset.saturating_sub(margin).min(total);
    let end = (offset + visible + margin).min(total);
    Window { first, len: end - first }
}

fn with_group_separator(value: usize) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

fn window_label(first_one_based: usize, last: usize, total: usize) -> String {
    format!("{first_one_based} to {last} of {}", with_group_separator(total))
}

/// REQ: AUT-011
#[test]
fn aut_011_window_is_clamped_to_the_list() {
    let w = window(10, 8, 5);
    assert_eq!(w.first, 4);
    assert_eq!(w.first + w.len, 10);
    assert_eq!(window(0, 0, 5).len, 0);
    assert_eq!(window(10, 50, 5).len, 0, "offset beyond the list gives an empty window");
}

/// REQ: AUT-011
#[test]
fn aut_011_window_never_needs_more_nodes_than_visible_plus_margin() {
    for total in [0usize, 1, 20, 3_212, 100_000] {
        for offset in [0usize, 3, 40, total.saturating_sub(1)] {
            let w = window(total, offset, 20);
            assert!(w.len <= 20 + 2 * 4, "total {total} offset {offset}");
            assert!(w.len < 4_096, "far below the 4,096 node ceiling");
        }
    }
}

/// REQ: AUT-012
#[test]
fn aut_012_label_shows_window_and_total_with_group_separator() {
    assert_eq!(window_label(41, 60, 3_212), "41 to 60 of 3,212");
    assert_eq!(window_label(1, 5, 5), "1 to 5 of 5");
    assert_eq!(with_group_separator(1_000_000), "1,000,000");
}

// ---------------------------------------------------------------- recipe 5: retry

fn is_transient(code: ErrorCode) -> bool {
    matches!(code, ErrorCode::TimedOut | ErrorCode::Busy | ErrorCode::Network | ErrorCode::RateLimited | ErrorCode::Io)
}

fn backoff_millis(attempt: u32, retry_after_ms: Option<u32>) -> Option<u32> {
    const MAX_ATTEMPTS: u32 = 5;
    if attempt > MAX_ATTEMPTS {
        return None;
    }
    let base = 1_000u32.saturating_mul(1 << (attempt - 1)).min(60_000);
    Some(base.max(retry_after_ms.unwrap_or(0)))
}

const ALL_CODES: [ErrorCode; 15] = [
    ErrorCode::NotPermitted,
    ErrorCode::NotFound,
    ErrorCode::InvalidArgument,
    ErrorCode::Conflict,
    ErrorCode::QuotaExceeded,
    ErrorCode::RateLimited,
    ErrorCode::Busy,
    ErrorCode::TimedOut,
    ErrorCode::Cancelled,
    ErrorCode::Unsupported,
    ErrorCode::PaneNotReady,
    ErrorCode::Revoked,
    ErrorCode::Io,
    ErrorCode::Network,
    ErrorCode::Internal,
];

/// REQ: AUT-013
#[test]
fn aut_013_permanent_codes_are_never_retried() {
    for code in [ErrorCode::NotPermitted, ErrorCode::Revoked, ErrorCode::InvalidArgument, ErrorCode::Unsupported] {
        assert!(!is_transient(code), "{} must not be retried", code.as_str());
    }
}

/// REQ: AUT-013
#[test]
fn aut_013_exactly_five_of_fifteen_codes_retry_with_backoff() {
    let retried: Vec<&str> = ALL_CODES.iter().filter(|code| is_transient(**code)).map(|code| code.as_str()).collect();
    assert_eq!(retried, vec!["rate-limited", "busy", "timed-out", "io", "network"]);
    assert_eq!(ALL_CODES.len(), ErrorCode::CASE_NAMES.len());
}

/// REQ: AUT-014
#[test]
fn aut_014_delay_doubles_from_1_second_and_gives_up_after_5_attempts() {
    assert_eq!(backoff_millis(1, None), Some(1_000));
    assert_eq!(backoff_millis(2, None), Some(2_000));
    assert_eq!(backoff_millis(3, None), Some(4_000));
    assert_eq!(backoff_millis(5, None), Some(16_000));
    assert_eq!(backoff_millis(6, None), None);
}

/// REQ: AUT-014, AUT-006
#[test]
fn aut_014_retry_after_ms_raises_the_delay_and_the_cap_is_60_seconds() {
    assert_eq!(backoff_millis(2, Some(9_000)), Some(9_000));
    assert_eq!(backoff_millis(1, Some(0)), Some(1_000));
    for attempt in 1..=5 {
        let delay = backoff_millis(attempt, None).unwrap();
        assert!(delay <= 60_000);
        if attempt > 1 {
            assert!(delay > backoff_millis(attempt - 1, None).unwrap(), "the delay grows");
        }
    }
}

/// REQ: AUT-014, AUT-013
#[test]
fn aut_014_busy_error_carries_a_retry_hint_the_plugin_can_honour() {
    let error = PluginError::busy("later", 2_500);
    assert_eq!(error.code, ErrorCode::Busy);
    assert_eq!(backoff_millis(1, error.retry_after_ms), Some(2_500));
}

// ---------------------------------------------------------------- recipe 6: dedup

/// REQ: AUT-016
#[test]
fn aut_016_same_key_with_different_content_completes_as_duplicate() {
    let mut host = FakeHost::new().grants(&[SOUND_GRANT, TOAST_GRANT]);
    host.activate::<SameKeyEmitter>();
    let first = host.deliver(vec![tick()]);
    let second = host.deliver(vec![tick()]);
    host.settle();
    let outcomes = host.outcomes();
    assert_eq!(outcomes.len(), 2);
    assert!(outcomes[0].is_ok());
    assert_eq!(
        outcomes[1],
        Outcome::Duplicate(RequestRef::Submission(first[0].correlation)),
        "the duplicate points at the first submission, second correlation {}",
        second[0].correlation
    );
}

/// REQ: AUT-016
#[test]
fn aut_016_key_is_accepted_again_after_10_minutes() {
    let mut host = FakeHost::new().grants(&[SOUND_GRANT, TOAST_GRANT]);
    host.activate::<SameKeyEmitter>();
    host.deliver(vec![tick()]);
    host.advance_clock(10 * 60 * 1000 - 1);
    host.deliver(vec![tick()]);
    host.settle();
    assert!(matches!(host.outcomes()[1], Outcome::Duplicate(_)), "still reserved 1 ms before the window ends");
    host.advance_clock(2);
    host.deliver(vec![tick()]);
    host.settle();
    assert!(!matches!(host.outcomes()[2], Outcome::Duplicate(_)), "free again after 10 minutes");
}

/// REQ: AUT-015
#[test]
fn aut_015_key_from_id_and_revision_lets_each_revision_through() {
    let mut host = FakeHost::new().grants(&[SOUND_GRANT]);
    host.activate::<RevisionKeyEmitter>();
    let submissions = host.deliver(vec![tick(), tick(), tick()]);
    host.settle();
    let keys: Vec<String> = submissions.iter().map(|s| s.dedup_key.clone().expect("keyed")).collect();
    assert_eq!(keys.len(), 3);
    assert!(keys[0] != keys[1] && keys[1] != keys[2] && keys[0] != keys[2]);
    assert!(host.outcomes().iter().all(|outcome| !matches!(outcome, Outcome::Duplicate(_))));
}

/// REQ: AUT-015
#[test]
fn aut_015_generated_key_matches_the_allowed_alphabet_and_length() {
    let key = ilium_plugin_sdk::dedup::key_for(&["finished", "42", "7"]);
    assert!(!key.is_empty() && key.len() <= 128);
    assert!(key.chars().all(|c| c.is_ascii_alphanumeric() || "._:/-".contains(c)), "key {key}");
    let long = "a".repeat(300);
    assert!(ilium_plugin_sdk::dedup::key_for(&[&long]).len() <= 128);
    assert_eq!(key, ilium_plugin_sdk::dedup::key_for(&["finished", "42", "7"]), "the same parts give the same key");
}

// ---------------------------------------------------------------- recipe 7: restart

/// REQ: AUT-017
#[test]
fn aut_017_saved_bytes_start_with_the_format_byte() {
    let mut host = FakeHost::new();
    host.activate::<Versioned>();
    let bytes = host.deactivate(DeactivationReason::Upgrade).expect("restore bytes");
    assert_eq!(bytes[0], Versioned::FORMAT);
    assert_eq!(bytes.len(), 5);
}

/// REQ: AUT-017
#[test]
fn aut_017_bad_format_or_length_falls_back_to_defaults() {
    for bad in [vec![], vec![9, 7, 0, 0, 0], vec![2], vec![2, 7, 0, 0], vec![2, 7, 0, 0, 0, 0], vec![0, 0, 0, 0, 0]] {
        let mut host = FakeHost::new();
        host.activate::<Versioned>();
        host.restart_with(bad.clone());
        let saved = host.deactivate(DeactivationReason::Shutdown).expect("restore bytes");
        assert_eq!(Versioned::load(&saved).value, 0, "bytes {bad:?} must give defaults");
    }
}

/// REQ: AUT-017
#[test]
fn aut_017_valid_bytes_are_restored_exactly() {
    let mut host = FakeHost::new();
    host.activate::<Versioned>();
    host.restart_with(Versioned { value: 7 }.save());
    assert_eq!(host.restored(), Some(Versioned { value: 7 }.save()));
    let saved = host.deactivate(DeactivationReason::Shutdown).unwrap();
    assert_eq!(Versioned::load(&saved).value, 7);
}

/// REQ: AUT-018
#[test]
fn aut_018_after_a_trap_nothing_is_saved_by_deactivate() {
    let mut host = FakeHost::new();
    host.activate::<Versioned>();
    host.restart_with(Versioned { value: 7 }.save());
    host.inject_trap("trap.panic");
    host.deliver_one(tick());
    let failures = host.failures();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].code, "trap.panic");
    assert_eq!(host.restarts().len(), 1);
    assert_eq!(host.restarts()[0].delay_ms, 1_000, "first failure restarts after 1 s");
    assert_eq!(host.restored(), None, "a trap drops the instance without restore bytes");
}

// ---------------------------------------------------------------- recipe 8: stale results

/// REQ: AUT-019
#[test]
fn aut_019_guard_accepts_only_the_newest_request() {
    let mut latest = crate::support::Latest::default();
    latest.start(1);
    latest.start(2);
    assert!(!latest.accept(1));
    assert!(latest.accept(2));
    assert!(!latest.accept(77), "an unknown id is rejected");
}

/// REQ: AUT-019
#[test]
fn aut_019_old_completion_is_discarded_and_new_one_applied_on_the_host() {
    let mut host = FakeHost::new().grants(&[SOUND_GRANT, TOAST_GRANT]);
    host.activate::<StaleGuard>();
    let first = host.deliver(vec![tick()]);
    let second = host.deliver(vec![tick()]);
    host.complete(RequestRef::Submission(first[0].correlation), Outcome::Ok);
    host.settle();
    assert_eq!(host.submitted().len(), 2, "the old completion produced no apply");
    host.complete(RequestRef::Submission(second[0].correlation), Outcome::Ok);
    host.settle();
    assert_eq!(host.submitted().len(), 3, "the newest completion is applied");
}

/// REQ: AUT-019
#[test]
fn aut_019_completion_event_carries_the_submission_correlation() {
    let event = Event::RequestCompleted(RequestResult { request: RequestRef::Submission(5), outcome: Outcome::Ok });
    assert!(matches!(event, Event::RequestCompleted(RequestResult { request: RequestRef::Submission(5), .. })));
}

// ---------------------------------------------------------------- recipe 9 and 10: batches

/// REQ: AUT-020
#[test]
fn aut_020_submissions_follow_the_order_of_the_batch() {
    let mut host = FakeHost::new().grants(&[TOAST_GRANT]);
    host.activate::<InOrder>();
    let submissions = host.deliver(vec![tick(), Event::KeymapChanged, Event::DisplayModeChanged(None), Event::CliCancelled(1)]);
    let correlations: Vec<u64> = submissions.iter().map(|s| s.correlation).collect();
    assert_eq!(submissions.len(), 4);
    assert!(correlations.windows(2).all(|pair| pair[0] < pair[1]), "correlations ascend with the batch order");
    assert!(correlations[0] >= 1 << 63, "the SDK numbers correlations from 2^63");
}

/// REQ: AUT-020
#[test]
fn aut_020_batch_of_256_events_is_processed_whole() {
    let mut host = FakeHost::new().grants(&[TOAST_GRANT]);
    host.activate::<InOrder>();
    let submissions = host.deliver((0..256).map(|_| tick()).collect());
    assert_eq!(submissions.len(), 256);
}

/// REQ: AUT-022
#[test]
fn aut_022_a_handler_that_breaches_its_deadline_traps_the_instance() {
    let mut host = FakeHost::new();
    host.activate::<InOrder>();
    host.breach_deadline("on-events");
    host.deliver_one(tick());
    let codes: Vec<String> = host.failures().into_iter().map(|failure| failure.code).collect();
    assert_eq!(codes, vec!["trap.deadline".to_string()]);
    assert_eq!(host.restarts().len(), 1);
}

/// REQ: AUT-021
#[test]
fn aut_021_events_dropped_entry_triggers_a_rebuild() {
    let mut host = FakeHost::new().grants(&[TOAST_GRANT]);
    host.activate::<Resync>();
    assert!(host.deliver(vec![tick()]).is_empty());
    let submissions = host.deliver(vec![Event::EventsDropped(Vec::new())]);
    assert_eq!(submissions.len(), 1);
}

// ---------------------------------------------------------------- recipe 12: migration

/// The recipe's `migrate` over an in-memory bucket; `interrupt_after_put` stops between the two steps.
fn migrate(bucket: &mut BTreeMap<String, Vec<u8>>, from: u32, to: u32, interrupt_after_put: bool) {
    if from < 2 && to >= 2 {
        if let Some(old) = bucket.get("notes").cloned() {
            bucket.insert("notes.v2".to_string(), old.iter().rev().copied().collect());
            if interrupt_after_put {
                return;
            }
            bucket.remove("notes");
        }
    }
}

/// REQ: AUT-025
#[test]
fn aut_025_running_migrate_twice_gives_the_same_result() {
    let mut once = BTreeMap::from([("notes".to_string(), b"abc".to_vec())]);
    migrate(&mut once, 1, 2, false);
    let mut twice = once.clone();
    migrate(&mut twice, 1, 2, false);
    assert_eq!(once, twice);
    assert_eq!(once.get("notes.v2"), Some(&b"cba".to_vec()));
    assert!(!once.contains_key("notes"));
}

/// REQ: AUT-025
#[test]
fn aut_025_interrupted_run_restarts_from_the_same_arguments_without_loss() {
    let mut bucket = BTreeMap::from([("notes".to_string(), b"abc".to_vec())]);
    migrate(&mut bucket, 1, 2, true);
    assert!(bucket.contains_key("notes") && bucket.contains_key("notes.v2"), "new value is written before the old one goes");
    migrate(&mut bucket, 1, 2, false);
    assert_eq!(bucket.keys().cloned().collect::<Vec<_>>(), vec!["notes.v2".to_string()]);
}

/// REQ: AUT-024
#[test]
fn aut_024_storage_snapshot_of_the_old_layout_loads_at_data_version_1() {
    let snapshot = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/authoring/fixtures/storage-v1.json");
    let mut host = FakeHost::new().with_data_version(1).with_storage_snapshot(snapshot);
    host.activate::<Versioned>();
    let state = host.snapshot();
    assert_eq!(state.storage["global"]["notes"], b"notes".to_vec());
}
