use super::*;
use crate::minecraft::{history_store::Repository, tours::Controller};
use std::time::{Duration, Instant};

fn history(completed: u64) -> History {
    let mut value = serde_json::to_value(History::default()).unwrap();
    value["completed"] = completed.into();
    value["revision"] = completed.into();
    let history: History = serde_json::from_value(value).unwrap();
    Controller::new(1, history).unwrap();
    history
}

#[test]
fn successor_gate_waits_for_accepted_writer_and_preserves_committed_history() {
    let temporary = tempfile::tempdir().unwrap();
    let repository = Repository::new(temporary.path().join("private synthetic history")).unwrap();
    let runtime = SavedRuntime::new();
    runtime
        .install(Writer::start(repository.clone(), 0, &crate::resources::test_resources()).unwrap())
        .unwrap();
    let latest = history(1);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match runtime.submit(latest) {
            Ok(_) => break,
            Err(Error::Busy | Error::Writer(history_writer::Error::Busy)) => (),
            result => panic!("unexpected admission: {result:?}"),
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    runtime.retire().unwrap();
    loop {
        match runtime.gate() {
            Gate::Ready => break,
            Gate::Busy | Gate::Draining => (),
            Gate::ReloadRequired => panic!("own writer conflicted"),
            Gate::Poisoned => panic!("own runtime poisoned"),
        }
        assert!(Instant::now() < deadline, "accepted writer did not drain");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(repository.load(&|| false).unwrap().history(), latest);
}

#[test]
fn empty_runtime_has_no_history_admission() {
    let runtime = SavedRuntime::new();
    assert_eq!(runtime.gate(), Gate::Ready);
    assert!(matches!(
        runtime.submit(History::default()),
        Err(Error::NoWriter)
    ));
}

#[test]
fn caller_contention_is_busy_and_poison_is_a_terminal_failure() {
    let runtime = SavedRuntime::new();
    let guard = runtime.inner.lock().unwrap();
    assert_eq!(runtime.gate(), Gate::Busy);
    assert!(matches!(
        runtime.submit(History::default()),
        Err(Error::Busy)
    ));
    assert!(matches!(runtime.retire(), Err(Error::Busy)));
    drop(guard);
    assert_eq!(runtime.gate(), Gate::Ready);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = runtime.inner.lock().unwrap();
        panic!("synthetic runtime poison");
    }));
    assert!(result.is_err());
    assert_eq!(runtime.gate(), Gate::Poisoned);
    assert!(matches!(runtime.retire(), Err(Error::Poisoned)));
}

#[test]
fn external_revision_conflict_requires_reload_before_successor_admission() {
    let temporary = tempfile::tempdir().unwrap();
    let repository = Repository::new(temporary.path().join("synthetic conflict history")).unwrap();
    let runtime = SavedRuntime::new();
    runtime
        .install(Writer::start(repository.clone(), 0, &crate::resources::test_resources()).unwrap())
        .unwrap();
    repository.commit_history(0, history(1), &|| false).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match runtime.submit(history(2)) {
            Ok(_) => break,
            Err(Error::Busy | Error::Writer(history_writer::Error::Busy)) => (),
            result => panic!("unexpected initial admission: {result:?}"),
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    loop {
        match runtime.gate() {
            Gate::ReloadRequired => break,
            Gate::Busy | Gate::Draining => (),
            result => panic!("conflicting writer did not fence reload: {result:?}"),
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(matches!(
        runtime.submit(history(2)),
        Err(Error::ReloadRequired)
    ));
    let authoritative = repository.load(&|| false).unwrap();
    assert_eq!(authoritative.history(), history(1));
    runtime.acknowledge_authoritative_reload().unwrap();
    runtime
        .install(
            Writer::start(
                repository.clone(),
                authoritative.revision(),
                &crate::resources::test_resources(),
            )
            .unwrap(),
        )
        .unwrap();
    assert!(runtime.submit(history(2)).is_ok());
    runtime.retire().unwrap();
    loop {
        match runtime.gate() {
            Gate::Ready => break,
            Gate::Busy | Gate::Draining => (),
            result => panic!("reloaded writer failed: {result:?}"),
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(repository.load(&|| false).unwrap().history(), history(2));
}

#[test]
fn failed_final_handoff_waits_for_accepted_writer_and_never_clears_under_contention() {
    let temporary = tempfile::tempdir().unwrap();
    let repository = Repository::new(temporary.path().join("sticky final history")).unwrap();
    let runtime = SavedRuntime::new();
    runtime
        .install(Writer::start(repository.clone(), 0, &crate::resources::test_resources()).unwrap())
        .unwrap();
    let latest = history(1);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match runtime.submit(latest) {
            Ok(_) => break,
            Err(Error::Busy | Error::Writer(history_writer::Error::Busy)) => {}
            result => panic!("unexpected admission: {result:?}"),
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    runtime.begin_handoff().unwrap();
    runtime.finish_handoff(false);
    runtime.with_test_lock(|| {
        assert_eq!(runtime.gate(), Gate::Busy);
        assert!(matches!(
            runtime.acknowledge_authoritative_reload(),
            Err(Error::Busy)
        ));
    });
    loop {
        match runtime.gate() {
            Gate::ReloadRequired => break,
            Gate::Busy | Gate::Draining => {}
            gate => panic!("failed handoff released old writer prematurely: {gate:?}"),
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    let authoritative = repository.load(&|| false).unwrap();
    assert_eq!(authoritative.history(), latest);
    runtime.acknowledge_authoritative_reload().unwrap();
    assert_eq!(runtime.gate(), Gate::Ready);
}

#[test]
fn overlapping_final_handoff_is_sticky_without_clearing_first_worker_fence() {
    let runtime = SavedRuntime::new();
    runtime.begin_handoff().unwrap();
    assert!(matches!(runtime.begin_handoff(), Err(Error::Draining)));
    assert_eq!(runtime.gate(), Gate::Draining);
    assert!(matches!(
        runtime.acknowledge_authoritative_reload(),
        Err(Error::Draining)
    ));
    runtime.finish_handoff(true);
    assert_eq!(runtime.gate(), Gate::ReloadRequired);
    runtime.acknowledge_authoritative_reload().unwrap();
    assert_eq!(runtime.gate(), Gate::Ready);
}
