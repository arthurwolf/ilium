use std::cell::Cell;

use ilium_core::polling_detection::PollingSettings;
use ilium_server::polling_dispatch::{dispatch, DispatchError};
use ilium_server::polling_ledger::{Completion, Ledger, NewAttempt};

fn attempt() -> NewAttempt {
    NewAttempt {
        request_id: "request".into(),
        window_id: "window".into(),
        month: "2026-11".into(),
        day: "2026-11-15".into(),
        created_utc: "2026-11-15T12:00:00Z".into(),
        project_id: "project".into(),
        model: "gpt-6-luna".into(),
        reserved_nanodollars: 100,
    }
}

fn completion() -> Completion {
    Completion {
        input_tokens: 10,
        cached_tokens: 0,
        cache_write_tokens: 0,
        pricing_version: "test".into(),
        uncached_rate: 1_000_000,
        cached_rate: 100_000,
        cache_write_rate: 1_250_000,
    }
}

#[test]
fn disabled_missing_key_and_missing_ledger_never_call_provider() {
    let root = tempfile::tempdir().unwrap();
    let mut ledger = Ledger::open(root.path()).unwrap();
    let called = Cell::new(false);
    let settings = PollingSettings::default();
    let run = |settings: &PollingSettings, key: &str, ledger: Option<&mut Ledger>| {
        dispatch(settings, key, ledger, &attempt(), || {
            called.set(true);
            Ok::<_, ()>(((), completion()))
        })
    };
    assert!(matches!(
        run(&settings, "key", Some(&mut ledger)),
        Err(DispatchError::Disabled)
    ));
    let enabled = PollingSettings {
        enabled: true,
        ..settings
    };
    assert!(matches!(
        run(&enabled, "", Some(&mut ledger)),
        Err(DispatchError::MissingKey)
    ));
    assert!(matches!(
        run(&enabled, "key", None),
        Err(DispatchError::LedgerUnavailable)
    ));
    assert!(!called.get());
    assert_eq!(ledger.summary_all_time().unwrap().attempts, 0);
}

#[test]
fn successful_request_is_recorded_and_duplicate_window_is_not_sent() {
    let root = tempfile::tempdir().unwrap();
    let mut ledger = Ledger::open(root.path()).unwrap();
    let settings = PollingSettings {
        enabled: true,
        ..Default::default()
    };
    let first = dispatch(&settings, "key", Some(&mut ledger), &attempt(), || {
        Ok::<_, ()>((42, completion()))
    });
    assert_eq!(first.unwrap(), 42);
    assert_eq!(
        ledger.summary("2026-11").unwrap().known_cost_nanodollars,
        10
    );
    let called = Cell::new(false);
    let second = dispatch(&settings, "key", Some(&mut ledger), &attempt(), || {
        called.set(true);
        Ok::<_, ()>(((), completion()))
    });
    assert!(second.is_err());
    assert!(!called.get());
}

#[test]
fn uncertain_transport_retains_the_reservation_after_restart() {
    let root = tempfile::tempdir().unwrap();
    let settings = PollingSettings {
        enabled: true,
        ..Default::default()
    };
    {
        let mut ledger = Ledger::open(root.path()).unwrap();
        let result = dispatch::<(), _>(&settings, "key", Some(&mut ledger), &attempt(), || Err(()));
        assert!(matches!(result, Err(DispatchError::OutcomeUnknown)));
    }
    let ledger = Ledger::open(root.path()).unwrap();
    let summary = ledger.summary("2026-11").unwrap();
    assert_eq!(summary.unknown_attempts, 1);
    assert_eq!(summary.reserved_nanodollars, 100);
}

#[test]
fn zero_budget_never_calls_provider() {
    let root = tempfile::tempdir().unwrap();
    let mut ledger = Ledger::open(root.path()).unwrap();
    let settings = PollingSettings {
        enabled: true,
        monthly_limit_microdollars: 0,
        ..Default::default()
    };
    let called = Cell::new(false);
    let result = dispatch(&settings, "key", Some(&mut ledger), &attempt(), || {
        called.set(true);
        Ok::<_, ()>(((), completion()))
    });
    assert!(result.is_err());
    assert!(!called.get());
    assert_eq!(ledger.summary_all_time().unwrap().attempts, 0);
}

#[test]
fn dispatched_attempt_is_durable_before_the_provider_is_called() {
    let root = tempfile::tempdir().unwrap();
    let mut ledger = Ledger::open(root.path()).unwrap();
    let settings = PollingSettings {
        enabled: true,
        ..Default::default()
    };
    let path = root.path().join("polling-ledger.sqlite3");
    dispatch(&settings, "key", Some(&mut ledger), &attempt(), || {
        let other =
            rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .unwrap();
        let state: String = other
            .query_row(
                "SELECT state FROM attempts WHERE request_id='request'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(state, "dispatched");
        Ok::<_, ()>(((), completion()))
    })
    .unwrap();
}

#[test]
fn an_unexpected_overrun_prevents_following_requests() {
    let root = tempfile::tempdir().unwrap();
    let mut ledger = Ledger::open(root.path()).unwrap();
    let settings = PollingSettings {
        enabled: true,
        monthly_limit_microdollars: 1,
        ..Default::default()
    };
    let mut expensive = completion();
    expensive.input_tokens = 2000;
    dispatch(&settings, "key", Some(&mut ledger), &attempt(), || {
        Ok::<_, ()>(((), expensive))
    })
    .unwrap();
    let mut next = attempt();
    next.request_id = "next".into();
    next.window_id = "next-window".into();
    let called = Cell::new(false);
    let result = dispatch(&settings, "key", Some(&mut ledger), &next, || {
        called.set(true);
        Ok::<_, ()>(((), completion()))
    });
    assert!(result.is_err());
    assert!(!called.get());
}

#[test]
fn independent_dispatches_release_the_repository_while_preserving_combined_budget() {
    use ilium_server::polling_dispatch::{begin_dispatch, finish_dispatch};
    let root = tempfile::tempdir().unwrap();
    let mut ledger = Ledger::open(root.path()).unwrap();
    let settings = PollingSettings {
        enabled: true,
        monthly_limit_microdollars: 1,
        ..Default::default()
    };
    let mut first = attempt();
    first.reserved_nanodollars = 400;
    let mut second = attempt();
    second.request_id = "second-request".into();
    second.window_id = "second-window".into();
    second.reserved_nanodollars = 400;
    let first_ticket = begin_dispatch(&settings, "synthetic-key", Some(&mut ledger), &first)
        .expect("first durable admission");
    let second_ticket = begin_dispatch(&settings, "synthetic-key", Some(&mut ledger), &second)
        .expect("second admission while first provider operation remains outstanding");
    assert_eq!(ledger.summary_all_time().unwrap().reserved_nanodollars, 800);
    let mut third = attempt();
    third.request_id = "third-request".into();
    third.window_id = "third-window".into();
    third.reserved_nanodollars = 400;
    assert!(matches!(
        begin_dispatch(&settings, "synthetic-key", Some(&mut ledger), &third),
        Err(DispatchError::Ledger(
            ilium_server::polling_ledger::LedgerError::BudgetReached
        ))
    ));
    assert_eq!(
        finish_dispatch(&mut ledger, second_ticket, Ok((42, completion()))).unwrap(),
        42
    );
    assert!(matches!(
        finish_dispatch::<()>(&mut ledger, first_ticket, Err(())),
        Err(DispatchError::OutcomeUnknown)
    ));
    let summary = ledger.summary_all_time().unwrap();
    assert_eq!(summary.attempts, 2);
    assert_eq!(summary.known_cost_nanodollars, 10);
    assert_eq!(summary.reserved_nanodollars, 400);
    assert_eq!(summary.unknown_attempts, 1);
    drop(ledger);
    let ledger = Ledger::open(root.path()).unwrap();
    assert_eq!(ledger.summary_all_time().unwrap(), summary);
}

#[test]
fn a_previous_owner_cannot_commit_a_late_response_after_ledger_reacquisition() {
    use ilium_server::polling_dispatch::{begin_dispatch, finish_dispatch};
    let root = tempfile::tempdir().unwrap();
    let mut ledger = Ledger::open(root.path()).unwrap();
    let settings = PollingSettings {
        enabled: true,
        ..Default::default()
    };
    let ticket = begin_dispatch(&settings, "synthetic-key", Some(&mut ledger), &attempt()).unwrap();
    drop(ledger);
    let mut replacement = Ledger::open(root.path()).unwrap();
    assert!(matches!(
        finish_dispatch(&mut replacement, ticket, Ok((42, completion()))),
        Err(DispatchError::StaleLeadership)
    ));
    let summary = replacement.summary_all_time().unwrap();
    assert_eq!(summary.known_cost_nanodollars, 0);
    assert_eq!(summary.reserved_nanodollars, 100);
    assert_eq!(summary.unknown_attempts, 1);
}

#[test]
fn a_ticket_cannot_complete_a_different_ledger_with_the_same_request_identity() {
    use ilium_server::polling_dispatch::{begin_dispatch, finish_dispatch};
    let first_root = tempfile::tempdir().unwrap();
    let second_root = tempfile::tempdir().unwrap();
    let mut first = Ledger::open(first_root.path()).unwrap();
    let mut second = Ledger::open(second_root.path()).unwrap();
    let settings = PollingSettings {
        enabled: true,
        ..Default::default()
    };
    let first_ticket =
        begin_dispatch(&settings, "synthetic-key", Some(&mut first), &attempt()).unwrap();
    let second_ticket =
        begin_dispatch(&settings, "synthetic-key", Some(&mut second), &attempt()).unwrap();
    assert!(matches!(
        finish_dispatch(&mut second, first_ticket, Ok((42, completion()))),
        Err(DispatchError::StaleLeadership)
    ));
    let summary = second.summary_all_time().unwrap();
    assert_eq!(summary.known_cost_nanodollars, 0);
    assert_eq!(summary.reserved_nanodollars, 100);
    assert!(matches!(
        finish_dispatch::<()>(&mut second, second_ticket, Err(())),
        Err(DispatchError::OutcomeUnknown)
    ));
}

#[test]
fn replaced_lease_inode_blocks_new_paid_admission_even_if_epoch_is_copied() {
    use ilium_platform::file_lock::ExclusiveFileLock;
    use ilium_server::polling_dispatch::begin_dispatch;
    let root = tempfile::tempdir().unwrap();
    let mut ledger = Ledger::open(root.path()).unwrap();
    let lease = root.path().join("polling-detector.lock");
    std::fs::rename(&lease, root.path().join("retained-original.lock")).unwrap();
    let _replacement = ExclusiveFileLock::acquire(&lease).unwrap();
    std::fs::write(&lease, ledger.leader_epoch()).unwrap();
    let settings = PollingSettings {
        enabled: true,
        ..Default::default()
    };
    assert!(matches!(
        begin_dispatch(&settings, "synthetic-key", Some(&mut ledger), &attempt()),
        Err(DispatchError::Ledger(
            ilium_server::polling_ledger::LedgerError::LostOwnership
        ))
    ));
    assert_eq!(ledger.summary_all_time().unwrap().attempts, 0);
}

#[test]
fn replaced_lease_inode_blocks_completion_before_accounting_changes() {
    use ilium_platform::file_lock::ExclusiveFileLock;
    use ilium_server::polling_dispatch::{begin_dispatch, finish_dispatch};
    let root = tempfile::tempdir().unwrap();
    let mut ledger = Ledger::open(root.path()).unwrap();
    let settings = PollingSettings {
        enabled: true,
        ..Default::default()
    };
    let ticket = begin_dispatch(&settings, "synthetic-key", Some(&mut ledger), &attempt()).unwrap();
    let lease = root.path().join("polling-detector.lock");
    std::fs::rename(&lease, root.path().join("retained-original.lock")).unwrap();
    let _replacement = ExclusiveFileLock::acquire(&lease).unwrap();
    std::fs::write(&lease, ledger.leader_epoch()).unwrap();
    assert!(matches!(
        finish_dispatch(&mut ledger, ticket, Ok((42, completion()))),
        Err(DispatchError::Ledger(
            ilium_server::polling_ledger::LedgerError::LostOwnership
        ))
    ));
    let summary = ledger.summary_all_time().unwrap();
    assert_eq!(summary.known_cost_nanodollars, 0);
    assert_eq!(summary.reserved_nanodollars, 100);
}
