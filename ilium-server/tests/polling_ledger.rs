use ilium_server::polling_ledger::{Completion, Ledger, LedgerError, NewAttempt};

fn attempt(id: &str, window: &str, reserve: i64) -> NewAttempt {
    NewAttempt {
        request_id: id.into(),
        window_id: window.into(),
        month: "2026-11".into(),
        day: "2026-11-15".into(),
        created_utc: "2026-11-15T12:00:00Z".into(),
        project_id: "project".into(),
        model: "gpt-6-luna".into(),
        reserved_nanodollars: reserve,
    }
}

#[test]
fn reserves_before_dispatch_and_never_resubmits_a_window() {
    let root = tempfile::tempdir().unwrap();
    let mut ledger = Ledger::open(root.path()).unwrap();
    ledger
        .prepare(&attempt("first", "window-a", 60), Some(100))
        .unwrap();
    assert!(matches!(
        ledger.prepare(&attempt("second", "window-b", 41), Some(100)),
        Err(LedgerError::BudgetReached)
    ));
    ledger.mark_dispatched("first").unwrap();
    ledger
        .complete(
            "first",
            &Completion {
                input_tokens: 5,
                cached_tokens: 0,
                cache_write_tokens: 0,
                pricing_version: "2026-11-15".into(),
                uncached_rate: 4_000_000,
                cached_rate: 10,
                cache_write_rate: 125,
            },
        )
        .unwrap();
    assert!(matches!(
        ledger.prepare(&attempt("repeat", "window-a", 1), Some(100)),
        Err(LedgerError::DuplicateWindow)
    ));
    ledger
        .prepare(&attempt("second", "window-b", 80), Some(100))
        .unwrap();
    let summary = ledger.summary("2026-11").unwrap();
    assert_eq!(summary.attempts, 2);
    assert_eq!(summary.known_cost_nanodollars, 20);
    assert_eq!(summary.reserved_nanodollars, 80);
    let first_page = ledger.history_page(None, 1).unwrap();
    assert_eq!(first_page.records.len(), 1);
    let second_page = ledger.history_page(first_page.next, 1).unwrap();
    assert_eq!(second_page.records.len(), 1);
    assert_ne!(
        first_page.records[0].request_id,
        second_page.records[0].request_id
    );
}

#[test]
fn unknown_dispatch_survives_restart_and_remains_reserved() {
    let root = tempfile::tempdir().unwrap();
    {
        let mut ledger = Ledger::open(root.path()).unwrap();
        ledger
            .prepare(&attempt("first", "window-a", 60), Some(100))
            .unwrap();
        ledger.mark_dispatched("first").unwrap();
        ledger
            .prepare(&attempt("prepared", "window-b", 30), Some(100))
            .unwrap();
        assert!(matches!(
            Ledger::open(root.path()),
            Err(LedgerError::AlreadyOwned)
        ));
    }
    let mut ledger = Ledger::open(root.path()).unwrap();
    let summary = ledger.summary("2026-11").unwrap();
    assert_eq!(summary.unknown_attempts, 1);
    assert_eq!(summary.reserved_nanodollars, 60);
    assert!(matches!(
        ledger.prepare(&attempt("third", "window-c", 41), Some(100)),
        Err(LedgerError::BudgetReached)
    ));
}

#[test]
fn zero_budget_and_unknown_outcomes_fail_closed() {
    let root = tempfile::tempdir().unwrap();
    let mut ledger = Ledger::open(root.path()).unwrap();
    assert!(matches!(
        ledger.prepare(&attempt("zero", "zero", 0), Some(0)),
        Err(LedgerError::BudgetReached)
    ));
    ledger
        .prepare(&attempt("first", "window-a", 50), Some(100))
        .unwrap();
    ledger.mark_dispatched("first").unwrap();
    ledger.mark_unknown("first").unwrap();
    assert_eq!(ledger.summary("2026-11").unwrap().reserved_nanodollars, 50);
    let completed = Completion {
        input_tokens: 1,
        cached_tokens: 0,
        cache_write_tokens: 0,
        pricing_version: "test".into(),
        uncached_rate: 100,
        cached_rate: 10,
        cache_write_rate: 125,
    };
    assert!(matches!(
        ledger.complete("first", &completed),
        Err(LedgerError::Invalid)
    ));
}

#[test]
fn invalid_month_cannot_evade_a_budget_bucket() {
    let root = tempfile::tempdir().unwrap();
    let mut ledger = Ledger::open(root.path()).unwrap();
    let mut bad = attempt("first", "window-a", 1);
    bad.month = "2026-13".into();
    assert!(matches!(
        ledger.prepare(&bad, Some(100)),
        Err(LedgerError::Invalid)
    ));
}

#[test]
fn day_and_all_time_totals_preserve_month_boundaries() {
    let root = tempfile::tempdir().unwrap();
    let mut ledger = Ledger::open(root.path()).unwrap();
    let first = attempt("first", "window-a", 10);
    ledger.prepare(&first, None).unwrap();
    let mut second = attempt("second", "window-b", 20);
    second.month = "2026-12".into();
    second.day = "2026-12-01".into();
    second.created_utc = "2026-12-01T12:00:00Z".into();
    ledger.prepare(&second, None).unwrap();
    assert_eq!(
        ledger
            .summary_day("2026-11-15")
            .unwrap()
            .unwrap()
            .reserved_nanodollars,
        10
    );
    assert_eq!(ledger.summary("2026-12").unwrap().reserved_nanodollars, 20);
    assert_eq!(ledger.summary_all_time().unwrap().reserved_nanodollars, 30);
    second.request_id = "third".into();
    second.window_id = "window-c".into();
    second.day = "2026-11-30".into();
    assert!(matches!(
        ledger.prepare(&second, None),
        Err(LedgerError::Invalid)
    ));
    second.day = "2026-12-25".into();
    assert!(matches!(
        ledger.prepare(&second, None),
        Err(LedgerError::Invalid)
    ));
    assert!(matches!(
        ledger.summary_day("2026-02-30"),
        Err(LedgerError::Invalid)
    ));
}

#[test]
fn migration_does_not_invent_local_days_for_old_attempts() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("polling-ledger.sqlite3");
    drop(
        ilium_platform::secure_fs::open_private_regular_file(
            &path,
            ilium_platform::secure_fs::PrivateFileMode::CreateNew,
        )
        .unwrap(),
    );
    let database = rusqlite::Connection::open(path).unwrap();
    database.execute_batch("CREATE TABLE attempts (
        request_id TEXT PRIMARY KEY, window_id TEXT NOT NULL UNIQUE,
        month TEXT NOT NULL, created_utc TEXT NOT NULL, project_id TEXT NOT NULL,
        model TEXT NOT NULL, state TEXT NOT NULL, reserved_nanodollars INTEGER NOT NULL,
        input_tokens INTEGER, cached_tokens INTEGER, cache_write_tokens INTEGER,
        cost_nanodollars INTEGER, pricing_version TEXT, uncached_rate INTEGER,
        cached_rate INTEGER, cache_write_rate INTEGER
    );
    INSERT INTO attempts(request_id,window_id,month,created_utc,project_id,model,state,reserved_nanodollars)
    VALUES('old','window','2026-11','2026-11-15T23:00:00Z','project','model','unknown',7);
    PRAGMA user_version=1;").unwrap();
    drop(database);
    let ledger = Ledger::open(root.path()).unwrap();
    assert!(ledger.summary_day("2026-11-15").unwrap().is_none());
    assert_eq!(ledger.summary("2026-11").unwrap().reserved_nanodollars, 7);
    assert_eq!(ledger.summary_all_time().unwrap().unknown_attempts, 1);
}

#[test]
fn transactional_totals_follow_completion_cancellation_and_restart() {
    let root = tempfile::tempdir().unwrap();
    {
        let mut ledger = Ledger::open(root.path()).unwrap();
        ledger
            .prepare(&attempt("completed", "window-a", 60), None)
            .unwrap();
        ledger
            .prepare(&attempt("cancelled", "window-b", 30), None)
            .unwrap();
        ledger
            .prepare(&attempt("unresolved", "window-c", 40), None)
            .unwrap();
        ledger.mark_dispatched("completed").unwrap();
        ledger
            .complete(
                "completed",
                &Completion {
                    input_tokens: 5,
                    cached_tokens: 0,
                    cache_write_tokens: 0,
                    pricing_version: "synthetic-test".into(),
                    uncached_rate: 4_000_000,
                    cached_rate: 10,
                    cache_write_rate: 125,
                },
            )
            .unwrap();
        ledger.cancel_prepared("cancelled").unwrap();
        ledger.mark_dispatched("unresolved").unwrap();
        let totals = ledger.summary_all_time().unwrap();
        assert_eq!(totals.attempts, 3);
        assert_eq!(totals.known_cost_nanodollars, 20);
        assert_eq!(totals.known_input_tokens, 5);
        assert_eq!(totals.reserved_nanodollars, 40);
        assert_eq!(totals.unknown_attempts, 0);
        assert_eq!(ledger.summary_day("2026-11-15").unwrap().unwrap(), totals);
        assert_eq!(ledger.summary("2026-11").unwrap(), totals);
    }
    let mut ledger = Ledger::open(root.path()).unwrap();
    let totals = ledger.summary_all_time().unwrap();
    assert_eq!(totals.attempts, 3);
    assert_eq!(totals.known_cost_nanodollars, 20);
    assert_eq!(totals.known_input_tokens, 5);
    assert_eq!(totals.reserved_nanodollars, 40);
    assert_eq!(totals.unknown_attempts, 1);
    assert_eq!(ledger.summary_day("2026-11-15").unwrap().unwrap(), totals);
    assert_eq!(ledger.summary("2026-11").unwrap(), totals);
    assert_eq!(ledger.history_page(None, 100).unwrap().records.len(), 3);
}

#[test]
fn history_pages_share_a_revision_and_do_not_repeat_or_omit_attempts() {
    let root = tempfile::tempdir().unwrap();
    let mut ledger = Ledger::open(root.path()).unwrap();
    for id in ["one", "two", "three"] {
        ledger.prepare(&attempt(id, id, 10), None).unwrap();
    }
    let first = ledger.history_page(None, 2).unwrap();
    assert_eq!(
        first
            .records
            .iter()
            .map(|row| row.request_id.as_str())
            .collect::<Vec<_>>(),
        ["one", "two"]
    );
    let next = first.next.expect("one more record remains");
    assert_eq!(next.revision, first.revision);
    let second = ledger.history_page(Some(next), 2).unwrap();
    assert_eq!(second.revision, first.revision);
    assert_eq!(second.records.len(), 1);
    assert_eq!(second.records[0].request_id, "three");
    assert!(second.next.is_none());
}

#[test]
fn history_cursor_is_invalidated_by_state_changes_without_new_rows() {
    let root = tempfile::tempdir().unwrap();
    let mut ledger = Ledger::open(root.path()).unwrap();
    ledger.prepare(&attempt("one", "one", 10), None).unwrap();
    ledger.prepare(&attempt("two", "two", 10), None).unwrap();
    let cursor = ledger.history_page(None, 1).unwrap().next.unwrap();
    ledger.mark_dispatched("two").unwrap();
    assert!(matches!(
        ledger.history_page(Some(cursor), 1),
        Err(LedgerError::StaleCursor)
    ));
    let cursor = ledger.history_page(None, 1).unwrap().next.unwrap();
    drop(ledger);
    let mut ledger = Ledger::open(root.path()).unwrap();
    assert!(
        matches!(
            ledger.history_page(Some(cursor), 1),
            Err(LedgerError::StaleCursor)
        ),
        "restart recovery changes the accounting revision"
    );
}

#[test]
fn history_page_rejects_unbounded_and_negative_cursors() {
    use ilium_server::polling_ledger::LedgerCursor;
    let root = tempfile::tempdir().unwrap();
    let mut ledger = Ledger::open(root.path()).unwrap();
    for limit in [0, 101, 255] {
        assert!(matches!(
            ledger.history_page(None, limit),
            Err(LedgerError::Invalid)
        ));
    }
    assert!(matches!(
        ledger.history_page(
            Some(LedgerCursor {
                revision: 0,
                after_sequence: -1
            }),
            10
        ),
        Err(LedgerError::Invalid)
    ));
}

#[test]
fn repository_owner_keeps_the_lease_when_an_io_connection_closes() {
    let root = tempfile::tempdir().unwrap();
    let mut ledger = Ledger::open(root.path()).unwrap();
    ledger.prepare(&attempt("one", "one", 10), None).unwrap();
    ledger.mark_dispatched("one").unwrap();
    ledger.prepare(&attempt("two", "two", 20), None).unwrap();
    let epoch = ledger.leader_epoch().to_owned();
    let owner = ledger.into_owner();
    assert!(
        matches!(Ledger::open(root.path()), Err(LedgerError::AlreadyOwned)),
        "closing a connection must not release user-wide leadership"
    );
    let mut ledger = Ledger::connect(owner.clone()).unwrap();
    assert_eq!(ledger.leader_epoch(), epoch);
    assert_eq!(ledger.summary_all_time().unwrap().unknown_attempts, 0);
    let rows = ledger.history_page(None, 100).unwrap().records;
    assert_eq!(rows[0].state, "dispatched");
    assert_eq!(rows[1].state, "prepared");
    drop(ledger);
    assert!(matches!(
        Ledger::open(root.path()),
        Err(LedgerError::AlreadyOwned)
    ));
    drop(owner);
    let ledger = Ledger::open(root.path()).unwrap();
    assert_ne!(ledger.leader_epoch(), epoch);
    assert_eq!(ledger.summary_all_time().unwrap().unknown_attempts, 1);
    assert_eq!(ledger.summary_all_time().unwrap().reserved_nanodollars, 10);
}

#[test]
fn repository_owner_remains_held_until_the_last_io_connection_finishes() {
    let root = tempfile::tempdir().unwrap();
    let owner = Ledger::open(root.path()).unwrap().into_owner();
    let ledger = Ledger::connect(owner.clone()).unwrap();
    drop(owner);
    assert!(matches!(
        Ledger::open(root.path()),
        Err(LedgerError::AlreadyOwned)
    ));
    drop(ledger);
    assert!(Ledger::open(root.path()).is_ok());
}
