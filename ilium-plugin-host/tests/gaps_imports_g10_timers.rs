//! G10 `timers` and `clock`: durable timers survive a restart and fire in due-time order
//! (api-durable-timers.md, PRF-027; api-clock-and-timers.md), checked through the public
//! import, the book the runtime fires from and the file store the runtime keeps.

use std::sync::Arc;
use std::time::Instant;

use ilium_plugin_api::generated::host::timers::HostTimers;
use ilium_plugin_api::generated::types::errors::ErrorCode;
use ilium_plugin_api::generated::types::ids::EpochMs;
use ilium_plugin_api::generated::types::Event;
use ilium_plugin_host::imports::timers::{
    durable_entries, restore_durable, take_due, TimerBook, TimersImport,
};
use ilium_plugin_host::runtime::durable_timers::DurableTimerStore;

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}

/// A new instance of a plugin: a book and the import over it.
fn instance() -> (TimerBook, TimersImport) {
    let book = TimerBook::default();
    let import = TimersImport::with_book(Arc::clone(&book));
    (book, import)
}

fn fired_names(events: &[Event]) -> Vec<(String, u64)> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::DurableTimerFired(fired) => Some((fired.name.clone(), fired.late_by_millis)),
            _ => None,
        })
        .collect()
}

/// REQ: PRF-027
#[test]
fn a_durable_timer_survives_a_restart_and_fires_once_with_its_lateness() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let file = directory.path().join("durable-timers-server.json");
    // `at` refuses a time in the past, so the timer is armed for the near future and the
    // restart happens "later": the clock handed to `take_due` is two minutes past the due time.
    let due = now_millis() + 1_000;
    {
        let (book, mut import) = instance();
        import
            .at(EpochMs(due), "nightly".to_owned())
            .expect("armed");
        let (mut store, warning) = DurableTimerStore::open(file.clone());
        assert_eq!(warning, None, "a missing file is an empty store");
        store.replace("a.b", durable_entries(&book));
        store.save_if_changed().expect("saved");
        // The process ends here, with the timer unfired.
    }
    let (store, warning) = DurableTimerStore::open(file.clone());
    assert_eq!(warning, None);
    let (book, _import) = instance();
    restore_durable(&book, store.entries_of("a.b"));
    let after_outage = due + 120_000;
    let events = take_due(&book, Instant::now(), after_outage);
    let fired = fired_names(&events);
    assert_eq!(fired, vec![("nightly".to_owned(), 120_000)], "{events:?}");
    assert!(
        take_due(&book, Instant::now(), after_outage).is_empty(),
        "it fires once"
    );
}

/// REQ: PRF-027
#[test]
fn overdue_timers_fire_in_due_time_order_whatever_their_names() {
    let (book, mut import) = instance();
    let base = now_millis() + 1_000;
    for (name, offset) in [("zulu", 1), ("alpha", 5), ("mike", 3), ("bravo", 2)] {
        import
            .at(EpochMs(base + offset), name.to_owned())
            .expect("armed");
    }
    // Two timers due at the very same millisecond fire in name order.
    import
        .at(EpochMs(base + 3), "lima".to_owned())
        .expect("armed");
    let events = take_due(&book, Instant::now(), base + 60_000);
    let order: Vec<String> = fired_names(&events)
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(order, ["zulu", "bravo", "lima", "mike", "alpha"]);
}

/// REQ: PRF-027
#[test]
fn all_sixty_four_overdue_timers_return_in_order_after_a_restart() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let file = directory.path().join("timers.json");
    let base = now_millis().saturating_sub(100_000);
    let entries: Vec<(String, u64)> = (0..64u64)
        .map(|index| (format!("job-{:02}", 63 - index), base + index))
        .collect();
    let (mut store, _) = DurableTimerStore::open(file.clone());
    store.replace("a.b", entries);
    store.save_if_changed().expect("saved");
    let (store, _) = DurableTimerStore::open(file);
    let (book, _import) = instance();
    restore_durable(&book, store.entries_of("a.b"));
    let events = take_due(&book, Instant::now(), now_millis());
    let order: Vec<String> = fired_names(&events)
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    let expected: Vec<String> = (0..64u64)
        .map(|index| format!("job-{:02}", 63 - index))
        .collect();
    assert_eq!(order, expected, "due-time order, every timer exactly once");
}

/// REQ: PRF-027
#[test]
fn a_damaged_store_cannot_give_a_plugin_more_than_sixty_four_timers() {
    let (book, mut import) = instance();
    let due = now_millis() + 3_600_000;
    restore_durable(
        &book,
        (0..200u64).map(|index| (format!("t-{index:03}"), due + index)),
    );
    assert_eq!(durable_entries(&book).len(), 64, "the held limit applies");
    // The book is full, so the plugin's own new timer is refused as `quota-exceeded`.
    let refused = import
        .at(EpochMs(due), "one-more".to_owned())
        .expect_err("over the limit");
    assert_eq!(refused.code, ErrorCode::QuotaExceeded);
}

/// REQ: PRF-027
#[test]
fn a_cancelled_timer_is_not_stored_and_does_not_come_back() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let file = directory.path().join("timers.json");
    let due = now_millis() + 3_600_000;
    let (book, mut import) = instance();
    import.at(EpochMs(due), "keep".to_owned()).expect("armed");
    import.at(EpochMs(due), "drop".to_owned()).expect("armed");
    import.cancel_durable("drop".to_owned());
    let (mut store, _) = DurableTimerStore::open(file.clone());
    store.replace("a.b", durable_entries(&book));
    store.save_if_changed().expect("saved");
    let (reopened, _) = DurableTimerStore::open(file);
    assert_eq!(reopened.entries_of("a.b"), vec![("keep".to_owned(), due)]);
}

/// REQ: PRF-027
#[test]
fn each_instance_has_its_own_book_and_deactivate_leaves_only_the_store() {
    let (first_book, mut first) = instance();
    let (second_book, _second) = instance();
    let due = now_millis() + 3_600_000;
    first.at(EpochMs(due), "mine".to_owned()).expect("armed");
    assert_eq!(durable_entries(&first_book).len(), 1);
    assert!(
        durable_entries(&second_book).is_empty(),
        "another instance's book is untouched"
    );
    // The runtime copies the book into the store before it drops the instance.
    let directory = tempfile::tempdir().expect("temporary directory");
    let (mut store, _) = DurableTimerStore::open(directory.path().join("t.json"));
    store.replace("a.b", durable_entries(&first_book));
    drop(first);
    drop(first_book);
    assert_eq!(store.entries_of("a.b"), vec![("mine".to_owned(), due)]);
}

/// REQ: PRF-027
#[test]
fn an_unreadable_store_starts_empty_and_says_why() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let file = directory.path().join("timers.json");
    std::fs::write(&file, "{ not json").expect("written");
    let (store, warning) = DurableTimerStore::open(file);
    assert!(store.is_empty());
    assert!(warning.is_some(), "the damage is reported, not hidden");
}
