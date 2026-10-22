//! Unit tests for public ilium-platform items that had no direct coverage:
//! owned worker ownership and join semantics, the animation storage leaf
//! validator, the configuration directory resolution, the Minecraft Java
//! directory layouts, and the external opener byte declaration.

use ilium_platform::animation_files::validate_leaf;
use ilium_platform::minecraft::{java_directory_for, MinecraftPlatform};
use ilium_platform::open_external::external_opener_declared_bytes;
use ilium_platform::owned_worker::{
    registered_worker_count, reserve_owned_worker, spawn_owned, spawn_owned_with_completion,
    supervisor_declared_bytes, supervisor_stack_bytes, supervisor_status,
    worker_retained_state_bytes, JoinDeadline, StopToken, WorkerExit, WorkerKind, WorkerMetadata,
    MAX_OWNED_WORKERS,
};
use ilium_platform::paths::{config_dir, CONFIG_DIR_ENV};
use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

/// The owned-worker registry and the process environment are process-wide, so
/// tests touching them run one at a time within this test binary.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial_guard() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|error| error.into_inner())
}

fn deadline_in(duration: Duration) -> Instant {
    Instant::now() + duration
}

fn wait_until(condition: impl Fn() -> bool) -> bool {
    let limit = deadline_in(Duration::from_secs(10));
    while Instant::now() < limit {
        if condition() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    condition()
}

fn reserved_count() -> usize {
    supervisor_status().map_or(0, |snapshot| snapshot.reserved_workers)
}

fn assert_leaf_rejected(name: &str) {
    match validate_leaf(name) {
        Ok(()) => panic!("storage leaf {name:?} should be rejected"),
        Err(error) => assert_eq!(
            error.kind(),
            io::ErrorKind::InvalidInput,
            "storage leaf {name:?} should be rejected as invalid input"
        ),
    }
}

fn assert_leaf_accepted(name: &str) {
    if let Err(error) = validate_leaf(name) {
        panic!("storage leaf {name:?} should be accepted, got {error}");
    }
}

/// Restores `ILIUM_CONFIG_DIR` to its previous state when dropped.
struct ConfigEnvGuard(Option<OsString>);

impl ConfigEnvGuard {
    fn set(value: Option<&OsStr>) -> Self {
        let previous = std::env::var_os(CONFIG_DIR_ENV);
        match value {
            Some(value) => std::env::set_var(CONFIG_DIR_ENV, value),
            None => std::env::remove_var(CONFIG_DIR_ENV),
        }
        Self(previous)
    }
}

impl Drop for ConfigEnvGuard {
    fn drop(&mut self) {
        match self.0.take() {
            Some(previous) => std::env::set_var(CONFIG_DIR_ENV, previous),
            None => std::env::remove_var(CONFIG_DIR_ENV),
        }
    }
}

// ---------------------------------------------------------------------------
// owned_worker: WorkerTicket / OwnedWorker
// ---------------------------------------------------------------------------

#[test]
fn drop_while_worker_running_cancels_and_joins() {
    let _serial = serial_guard();
    let (started_tx, started_rx) = mpsc::channel::<()>();
    let owner = spawn_owned(
        "unit-gap-running",
        WorkerKind::Cooperative,
        StopToken::default(),
        || {},
        move |stop| {
            let _ = started_tx.send(());
            while !stop.is_stopped() {
                std::thread::sleep(Duration::from_millis(1));
            }
        },
    )
    .expect("spawning a cooperative worker should succeed");
    started_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("the worker should start running before its owner is dropped");
    let ticket = owner.ticket();
    drop(owner);
    let outcome = ticket.join_until(deadline_in(Duration::from_secs(10)));
    assert_eq!(
        outcome,
        Ok(WorkerExit::Joined),
        "dropping the owner should cancel the running worker and join it"
    );
}

#[test]
fn drop_after_worker_already_exited_is_harmless() {
    let _serial = serial_guard();
    let owner = spawn_owned(
        "unit-gap-finished",
        WorkerKind::Cooperative,
        StopToken::default(),
        || {},
        |_stop| {},
    )
    .expect("spawning a short worker should succeed");
    let ticket = owner.ticket();
    let joined = ticket.join_until(deadline_in(Duration::from_secs(10)));
    assert_eq!(
        joined,
        Ok(WorkerExit::Joined),
        "the short worker should join"
    );
    drop(owner);
    assert_eq!(
        ticket.exit(),
        Some(WorkerExit::Joined),
        "dropping the owner after exit must not change the recorded outcome"
    );
}

#[test]
fn ticket_handed_out_then_owner_dropped_still_cancels_and_joins() {
    let _serial = serial_guard();
    let owner = spawn_owned(
        "unit-gap-ticket",
        WorkerKind::Cooperative,
        StopToken::default(),
        || {},
        |stop| {
            while !stop.is_stopped() {
                std::thread::sleep(Duration::from_millis(1));
            }
        },
    )
    .expect("spawning a cooperative worker should succeed");
    let ticket = owner.ticket();
    let cloned = ticket.clone();
    drop(owner);
    assert_eq!(
        cloned.join_until(deadline_in(Duration::from_secs(10))),
        Ok(WorkerExit::Joined),
        "a cloned ticket must observe the cancellation caused by dropping the owner"
    );
    assert_eq!(ticket.id(), cloned.id(), "clones must name the same worker");
}

#[test]
fn spawn_failure_leaves_no_registered_worker() {
    let _serial = serial_guard();
    let registered_before = registered_worker_count();
    let reserved_before = reserved_count();
    let error = spawn_owned(
        "bad\0name",
        WorkerKind::Cooperative,
        StopToken::default(),
        || {},
        |_stop| {},
    )
    .err()
    .expect("a worker name containing NUL must be refused");
    assert_eq!(
        error.kind(),
        io::ErrorKind::InvalidInput,
        "a NUL in the worker name is invalid input"
    );
    assert_eq!(
        registered_worker_count(),
        registered_before,
        "a failed spawn must not leave a registered worker"
    );
    assert_eq!(
        reserved_count(),
        reserved_before,
        "a failed spawn must release its reserved slot"
    );
}

#[test]
fn metadata_is_identical_before_and_after_exit() {
    let _serial = serial_guard();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let owner = reserve_owned_worker::<()>(Some(256 * 1024), ())
        .expect("a reservation with an explicit stack should be admitted")
        .spawn(
            "unit-gap-metadata",
            WorkerKind::SynchronousIo,
            StopToken::default(),
            || {},
            move |_stop| {
                let _ = release_rx.recv_timeout(Duration::from_secs(10));
            },
        )
        .expect("spawning with a reserved slot should succeed");
    let ticket = owner.ticket();
    let before = ticket.metadata();
    assert_eq!(
        before.kind,
        WorkerKind::SynchronousIo,
        "kind should be recorded"
    );
    assert_eq!(
        before.requested_stack_bytes,
        Some(256 * 1024),
        "an explicit stack request should be recorded as requested"
    );
    release_tx
        .send(())
        .expect("the worker should still be waiting");
    assert_eq!(
        ticket.join_until(deadline_in(Duration::from_secs(10))),
        Ok(WorkerExit::Joined),
        "the released worker should join"
    );
    assert_eq!(
        ticket.metadata(),
        before,
        "metadata is the immutable declaration and must not change at exit"
    );
}

#[test]
fn exit_is_none_while_running_and_some_after_join() {
    let _serial = serial_guard();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let owner = spawn_owned(
        "unit-gap-exit",
        WorkerKind::Cooperative,
        StopToken::default(),
        || {},
        move |_stop| {
            let _ = release_rx.recv_timeout(Duration::from_secs(10));
        },
    )
    .expect("spawning a blocked worker should succeed");
    let ticket = owner.ticket();
    assert_eq!(
        ticket.exit(),
        None,
        "a running worker has no recorded exit yet"
    );
    release_tx
        .send(())
        .expect("the worker should still be waiting");
    assert_eq!(
        ticket.join_until(deadline_in(Duration::from_secs(10))),
        Ok(WorkerExit::Joined),
        "the released worker should join"
    );
    assert_eq!(
        ticket.exit(),
        Some(WorkerExit::Joined),
        "a joined worker should report its exit"
    );
}

#[test]
fn join_until_with_expired_deadline_is_not_a_successful_join() {
    let _serial = serial_guard();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let owner = spawn_owned(
        "unit-gap-deadline",
        WorkerKind::Cooperative,
        StopToken::default(),
        || {},
        move |_stop| {
            let _ = release_rx.recv_timeout(Duration::from_secs(10));
        },
    )
    .expect("spawning a blocked worker should succeed");
    let ticket = owner.ticket();
    let timeout = ticket.join_until(Instant::now());
    assert!(
        timeout.is_err(),
        "an expired deadline must not report a join"
    );
    let deadline: JoinDeadline = timeout
        .err()
        .expect("the expired deadline should be reported as JoinDeadline");
    assert_eq!(
        deadline.worker_id,
        ticket.id(),
        "the deadline error should name the still-running worker"
    );
    release_tx
        .send(())
        .expect("the worker should still be waiting");
    assert_eq!(
        ticket.join_until(deadline_in(Duration::from_secs(10))),
        Ok(WorkerExit::Joined),
        "the worker is still owned after a timed-out wait and joins later"
    );
}

#[test]
fn worker_ids_are_unique_across_two_workers() {
    let _serial = serial_guard();
    let first = spawn_owned(
        "unit-gap-id-a",
        WorkerKind::Cooperative,
        StopToken::default(),
        || {},
        |_stop| {},
    )
    .expect("the first worker should spawn");
    let second = spawn_owned(
        "unit-gap-id-b",
        WorkerKind::Cooperative,
        StopToken::default(),
        || {},
        |_stop| {},
    )
    .expect("the second worker should spawn");
    let first_ticket = first.ticket();
    let second_ticket = second.ticket();
    assert_ne!(
        first_ticket.id(),
        second_ticket.id(),
        "two live workers must have distinct identities"
    );
    assert_eq!(
        first_ticket.join_until(deadline_in(Duration::from_secs(10))),
        Ok(WorkerExit::Joined)
    );
    assert_eq!(
        second_ticket.join_until(deadline_in(Duration::from_secs(10))),
        Ok(WorkerExit::Joined)
    );
}

// ---------------------------------------------------------------------------
// owned_worker: reservations and completion hooks
// ---------------------------------------------------------------------------

#[test]
fn reserve_at_capacity_returns_would_block() {
    let _serial = serial_guard();
    let mut reservations = Vec::new();
    let refusal = loop {
        match reserve_owned_worker::<()>(None, ()) {
            Ok(reservation) => reservations.push(reservation),
            Err(error) => break error,
        }
    };
    assert_eq!(
        refusal.kind(),
        io::ErrorKind::WouldBlock,
        "a reservation beyond the fixed owned-worker capacity must be refused as would-block"
    );
    assert!(
        !reservations.is_empty(),
        "some reservations should be admitted before the cap is reached"
    );
    let snapshot = supervisor_status().expect("the supervisor exists after reservations");
    assert_eq!(
        snapshot.reserved_workers,
        reservations.len(),
        "every admitted reservation should occupy one reserved slot"
    );
    assert!(
        snapshot.registered_workers + snapshot.reserved_workers <= MAX_OWNED_WORKERS,
        "registered plus reserved workers must never exceed the fixed capacity"
    );
    drop(reservations);
    assert_eq!(
        reserved_count(),
        0,
        "dropping every unused reservation must release every slot"
    );
    assert!(
        reserve_owned_worker::<()>(None, ()).is_ok(),
        "capacity must be available again once the reservations are released"
    );
}

#[test]
fn dropping_unused_reservation_releases_slot() {
    let _serial = serial_guard();
    let reserved_before = reserved_count();
    let reservation = reserve_owned_worker::<()>(None, ())
        .expect("a reservation below capacity should be admitted");
    assert_eq!(
        reserved_count(),
        reserved_before + 1,
        "an admitted reservation should occupy one reserved slot"
    );
    drop(reservation);
    assert_eq!(
        reserved_count(),
        reserved_before,
        "dropping an unused reservation should release its slot"
    );
}

#[test]
fn spawn_consumes_reservation_exactly_once() {
    let _serial = serial_guard();
    let registered_before = registered_worker_count();
    let reserved_before = reserved_count();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let reservation = reserve_owned_worker::<()>(None, ())
        .expect("a reservation below capacity should be admitted");
    let owner = reservation
        .spawn(
            "unit-gap-consume",
            WorkerKind::Cooperative,
            StopToken::default(),
            || {},
            move |_stop| {
                let _ = release_rx.recv_timeout(Duration::from_secs(10));
            },
        )
        .expect("spawning with a reserved slot should succeed");
    assert_eq!(
        reserved_count(),
        reserved_before,
        "spawn should convert the reservation into a registered worker"
    );
    assert_eq!(
        registered_worker_count(),
        registered_before + 1,
        "the spawned worker should be registered exactly once"
    );
    let ticket = owner.ticket();
    release_tx
        .send(())
        .expect("the worker should still be waiting");
    assert_eq!(
        ticket.join_until(deadline_in(Duration::from_secs(10))),
        Ok(WorkerExit::Joined)
    );
    assert_eq!(
        registered_worker_count(),
        registered_before,
        "a joined worker must leave the registry"
    );
}

#[test]
fn reservation_does_not_register_a_worker() {
    let _serial = serial_guard();
    let registered_before = registered_worker_count();
    let reserved_before = reserved_count();
    let reservation = reserve_owned_worker::<()>(None, ())
        .expect("a reservation below capacity should be admitted");
    assert_eq!(
        registered_worker_count(),
        registered_before,
        "a reservation alone creates no native worker record"
    );
    assert_eq!(reserved_count(), reserved_before + 1);
    drop(reservation);
    assert_eq!(reserved_count(), reserved_before);
}

#[test]
fn registered_worker_count_is_zero_with_no_workers() {
    let _serial = serial_guard();
    assert!(
        wait_until(|| registered_worker_count() == 0),
        "earlier workers in this binary should have retired"
    );
    assert_eq!(
        registered_worker_count(),
        0,
        "with no live workers the registered count must be zero"
    );
}

#[test]
fn completion_runs_once_on_normal_exit() {
    let _serial = serial_guard();
    let completions = Arc::new(AtomicUsize::new(0));
    let on_exit: Arc<dyn Fn() + Send + Sync> = {
        let completions = Arc::clone(&completions);
        Arc::new(move || {
            completions.fetch_add(1, Ordering::SeqCst);
        })
    };
    let owner = spawn_owned_with_completion(
        "unit-gap-completion-normal",
        WorkerKind::Cooperative,
        StopToken::default(),
        || {},
        |_stop| {},
        on_exit,
    )
    .expect("spawning with a completion hook should succeed");
    let ticket = owner.ticket();
    assert_eq!(
        ticket.join_until(deadline_in(Duration::from_secs(10))),
        Ok(WorkerExit::Joined)
    );
    assert!(
        wait_until(|| completions.load(Ordering::SeqCst) >= 1),
        "the completion hook should run after the worker is joined"
    );
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(
        completions.load(Ordering::SeqCst),
        1,
        "the completion hook must run exactly once"
    );
}

#[test]
fn completion_runs_once_when_worker_panics() {
    let _serial = serial_guard();
    let completions = Arc::new(AtomicUsize::new(0));
    let on_exit: Arc<dyn Fn() + Send + Sync> = {
        let completions = Arc::clone(&completions);
        Arc::new(move || {
            completions.fetch_add(1, Ordering::SeqCst);
        })
    };
    let owner = spawn_owned_with_completion(
        "unit-gap-completion-panic",
        WorkerKind::Cooperative,
        StopToken::default(),
        || {},
        |_stop| panic!("owned worker panic under test"),
        on_exit,
    )
    .expect("spawning with a completion hook should succeed");
    let ticket = owner.ticket();
    assert_eq!(
        ticket.join_until(deadline_in(Duration::from_secs(10))),
        Ok(WorkerExit::Panicked),
        "a panicking body must be reported as Panicked"
    );
    assert!(
        wait_until(|| completions.load(Ordering::SeqCst) >= 1),
        "the completion hook should still run after a panic"
    );
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(
        completions.load(Ordering::SeqCst),
        1,
        "the completion hook must run exactly once after a panic"
    );
}

#[test]
fn completion_runs_once_when_cancelled_before_body_observes_it() {
    let _serial = serial_guard();
    let completions = Arc::new(AtomicUsize::new(0));
    let on_exit: Arc<dyn Fn() + Send + Sync> = {
        let completions = Arc::clone(&completions);
        Arc::new(move || {
            completions.fetch_add(1, Ordering::SeqCst);
        })
    };
    let owner = spawn_owned_with_completion(
        "unit-gap-completion-cancel",
        WorkerKind::Cooperative,
        StopToken::default(),
        || {},
        |_stop| {},
        on_exit,
    )
    .expect("spawning with a completion hook should succeed");
    let ticket = owner.ticket();
    ticket.cancel();
    drop(owner);
    assert_eq!(
        ticket.join_until(deadline_in(Duration::from_secs(10))),
        Ok(WorkerExit::Joined),
        "a cancelled short worker still joins normally"
    );
    assert!(
        wait_until(|| completions.load(Ordering::SeqCst) >= 1),
        "the completion hook should run after a cancelled worker is joined"
    );
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(completions.load(Ordering::SeqCst), 1);
}

// ---------------------------------------------------------------------------
// owned_worker: declarations and derived traits
// ---------------------------------------------------------------------------

#[test]
fn supervisor_byte_declarations_are_nonzero_and_ordered() {
    let stack_bytes = supervisor_stack_bytes();
    let retained_bytes = worker_retained_state_bytes();
    let declared_bytes = supervisor_declared_bytes();
    assert!(
        stack_bytes > 0,
        "the supervisor stack request must be non-zero"
    );
    assert!(
        retained_bytes > 0,
        "retained per-worker state must be non-zero"
    );
    assert!(
        declared_bytes > 0,
        "the declared supervisor bytes must be non-zero"
    );
    assert!(
        declared_bytes >= retained_bytes,
        "the supervisor declaration must cover at least one retained worker state"
    );
    assert!(
        declared_bytes >= stack_bytes,
        "the supervisor declaration must include the explicit supervisor stack"
    );
}

#[test]
fn supervisor_byte_declarations_are_repeatable() {
    assert_eq!(supervisor_stack_bytes(), supervisor_stack_bytes());
    assert_eq!(worker_retained_state_bytes(), worker_retained_state_bytes());
    assert_eq!(supervisor_declared_bytes(), supervisor_declared_bytes());
}

#[test]
fn external_opener_declared_bytes_is_stable_and_covers_worker_state() {
    let declared = external_opener_declared_bytes();
    assert!(
        declared > 0,
        "the external opener declaration must be non-zero"
    );
    assert!(
        declared >= worker_retained_state_bytes() + 4096,
        "the opener declaration must include retained worker state and its fixed bookkeeping"
    );
    assert_eq!(
        external_opener_declared_bytes(),
        declared,
        "repeated calls must return the same declaration"
    );
}

#[test]
fn worker_metadata_debug_and_copy_output() {
    let metadata = WorkerMetadata {
        worker_id: 7,
        kind: WorkerKind::SynchronousIo,
        requested_stack_bytes: None,
    };
    let copied = metadata;
    assert_eq!(
        copied, metadata,
        "the copied metadata must equal the original"
    );
    let rendered = format!("{metadata:?}");
    assert!(
        rendered.contains("SynchronousIo"),
        "Debug should name the kind: {rendered}"
    );
    assert!(
        rendered.contains("worker_id: 7"),
        "Debug should name the id: {rendered}"
    );
    assert!(
        rendered.contains("requested_stack_bytes: None"),
        "Debug should show the default stack request: {rendered}"
    );
}

#[test]
fn worker_exit_panicked_is_distinct_from_joined() {
    assert_ne!(
        WorkerExit::Panicked,
        WorkerExit::Joined,
        "a panic must be distinguishable from a normal join"
    );
    assert_eq!(format!("{:?}", WorkerExit::Panicked), "Panicked");
    assert_eq!(format!("{:?}", WorkerExit::Joined), "Joined");
}

#[test]
fn join_deadline_and_worker_kind_debug_output() {
    let deadline = JoinDeadline { worker_id: 9 };
    let copied = deadline;
    assert_eq!(copied, deadline);
    let rendered = format!("{deadline:?}");
    assert!(
        rendered.contains("JoinDeadline"),
        "Debug should name the type: {rendered}"
    );
    assert!(
        rendered.contains("worker_id: 9"),
        "Debug should name the worker: {rendered}"
    );
    assert_eq!(format!("{:?}", WorkerKind::Cooperative), "Cooperative");
    assert_eq!(format!("{:?}", WorkerKind::SynchronousIo), "SynchronousIo");
}

// ---------------------------------------------------------------------------
// animation_files::validate_leaf
// ---------------------------------------------------------------------------

#[test]
fn validate_leaf_rejects_empty_name() {
    assert_leaf_rejected("");
}

#[test]
fn validate_leaf_rejects_dot() {
    assert_leaf_rejected(".");
}

#[test]
fn validate_leaf_rejects_dot_dot() {
    assert_leaf_rejected("..");
}

#[test]
fn validate_leaf_rejects_path_separators_and_colon() {
    assert_leaf_rejected("nested/leaf");
    assert_leaf_rejected("nested\\leaf");
    assert_leaf_rejected("drive:leaf");
}

#[test]
fn validate_leaf_rejects_nul_byte() {
    assert_leaf_rejected("bad\0leaf");
}

#[test]
fn validate_leaf_accepts_255_byte_name() {
    let name = "a".repeat(255);
    assert_leaf_accepted(&name);
}

#[test]
fn validate_leaf_rejects_256_byte_name() {
    let name = "a".repeat(256);
    assert_leaf_rejected(&name);
}

#[test]
fn validate_leaf_accepts_ordinary_name() {
    assert_leaf_accepted("scene-01.json");
}

// ---------------------------------------------------------------------------
// paths::config_dir
// ---------------------------------------------------------------------------

#[test]
fn config_dir_honours_explicit_override() {
    let _serial = serial_guard();
    let root = tempfile::tempdir().expect("a temporary directory should be created");
    let override_path = root.path().join("ilium-config");
    let _env = ConfigEnvGuard::set(Some(override_path.as_os_str()));
    assert_eq!(
        config_dir(),
        Some(override_path),
        "an absolute ILIUM_CONFIG_DIR must be returned unchanged"
    );
}

#[test]
fn config_dir_unset_returns_platform_default_or_none() {
    let _serial = serial_guard();
    let _env = ConfigEnvGuard::set(None);
    match config_dir() {
        None => {}
        Some(path) => {
            assert!(
                path.is_absolute(),
                "the platform default must be absolute: {path:?}"
            );
            assert!(
                path.ends_with("ilium"),
                "the platform default must end in ilium: {path:?}"
            );
        }
    }
}

#[test]
fn config_dir_empty_override_is_ignored() {
    let _serial = serial_guard();
    let unset = {
        let _env = ConfigEnvGuard::set(None);
        config_dir()
    };
    let empty = {
        let _env = ConfigEnvGuard::set(Some(OsStr::new("")));
        config_dir()
    };
    assert_eq!(
        empty, unset,
        "an empty override must behave as if it were unset"
    );
}

#[test]
fn config_dir_relative_override_is_anchored_to_working_directory() {
    let _serial = serial_guard();
    let relative = "relative-ilium-config";
    let _env = ConfigEnvGuard::set(Some(OsStr::new(relative)));
    let resolved = config_dir().expect("a non-empty override should resolve to a path");
    assert!(
        resolved.is_absolute(),
        "a relative override must be anchored: {resolved:?}"
    );
    assert!(
        resolved.ends_with(relative),
        "the anchored override must keep the requested name: {resolved:?}"
    );
}

// ---------------------------------------------------------------------------
// minecraft::java_directory_for
// ---------------------------------------------------------------------------

#[test]
fn java_directory_linux_is_dot_minecraft_under_home() {
    let home = Path::new("/home/player");
    assert_eq!(
        java_directory_for(MinecraftPlatform::Linux, home, None),
        Some(PathBuf::from("/home/player/.minecraft")),
        "the Linux launcher directory lives under the home directory"
    );
}

#[test]
fn java_directory_macos_is_application_support_under_home() {
    let home = Path::new("/Users/player");
    assert_eq!(
        java_directory_for(MinecraftPlatform::MacOs, home, None),
        Some(PathBuf::from(
            "/Users/player/Library/Application Support/minecraft"
        )),
        "the macOS launcher directory lives under Application Support"
    );
}

#[test]
fn java_directory_windows_is_dot_minecraft_under_roaming_root() {
    let home = Path::new("C:/Users/player");
    let roaming = Path::new("C:/Users/player/AppData/Roaming");
    assert_eq!(
        java_directory_for(MinecraftPlatform::Windows, home, Some(roaming)),
        Some(PathBuf::from("C:/Users/player/AppData/Roaming/.minecraft")),
        "the Windows launcher directory lives under the roaming application data"
    );
}

#[test]
fn java_directory_windows_without_roaming_root_is_none() {
    let home = Path::new("C:/Users/player");
    assert_eq!(
        java_directory_for(MinecraftPlatform::Windows, home, None),
        None,
        "without a roaming root there is no Windows launcher directory"
    );
}
