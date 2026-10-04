use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Asserts the log directory and file are readable only by their owner.
///
/// Owner-only *modes* are a Unix concept. On Windows the same guarantee comes
/// from the log directory's inherited profile ACL (see
/// `ilium_platform::secure_fs`), which exposes no mode bits to compare, so
/// there is nothing to assert rather than something weaker to assert.
#[cfg(unix)]
fn assert_owner_only(log_directory: &Path, log_path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    assert_eq!(
        std::fs::metadata(log_directory)
            .expect("log directory")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(log_path)
            .expect("log file")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[cfg(not(unix))]
fn assert_owner_only(log_directory: &Path, log_path: &Path) {
    // Still prove both exist; only the mode comparison is Unix-specific.
    assert!(log_directory.is_dir());
    assert!(log_path.is_file());
}

static FORMATTED_FIELDS: AtomicUsize = AtomicUsize::new(0);

fn expensive_field() -> &'static str {
    FORMATTED_FIELDS.fetch_add(1, Ordering::Relaxed);
    "complete prompt"
}

fn emit_repeated_diagnostic() {
    tracing::info!(request_body = %expensive_field(), "repeated diagnostic call site");
}

#[test]
fn tracing_events_follow_the_live_setting_and_keep_private_permissions() {
    // `initialize` reads `RUST_LOG` exactly once, at the first call below, to
    // build the process-wide `EnvFilter`. An ambient `RUST_LOG=debug` (common
    // on developer machines) would let the unrelated-target debug event
    // through and break the redaction assertion later in this test, so pin
    // the filter to this test's own expectations instead of the caller's
    // shell. Safe here: this integration test binary runs this single
    // `#[test]` function, with no other threads reading/writing the
    // environment concurrently.
    unsafe {
        std::env::remove_var("RUST_LOG");
    }

    let directory = tempfile::tempdir().expect("tempdir");
    let log_directory = directory.path().join("session");
    let log_path = log_directory.join("log-2026-07-19_12-00-00.000.txt");

    let quota = ilium_execution::QuotaGroup::new(ilium_execution::QuotaLimits {
        clients: 0,
        jobs: 0,
        service_jobs: 0,
        input_bytes: 0,
        result_bytes: 0,
        worker_threads: 2,
        worker_bytes: ilium_platform::owned_worker::supervisor_declared_bytes()
            + ilium_logging::LOGGER_STACK_BYTES
            + ilium_logging::logger_storage_bytes(&log_path).expect("actor declaration")
            + 258 * ilium_logging::logger_control_bytes(&log_path).expect("control declaration"),
    });
    assert!(ilium_execution::initialize_process_supervisor(&quota)
        .expect("admit and start one process supervisor before logging"));
    let supervisor =
        ilium_platform::owned_worker::supervisor_status().expect("native supervisor is live");
    assert_eq!(quota.snapshot().worker_threads, 1);
    ilium_logging::initialize(&log_path, false, "integration-test", &quota).expect("initialize");
    assert_eq!(quota.snapshot().worker_threads, 2);
    assert_eq!(
        ilium_platform::owned_worker::supervisor_status()
            .expect("same supervisor")
            .thread_id,
        supervisor.thread_id
    );
    tracing::error!(request_body = %expensive_field(), "diagnostic test event");
    emit_repeated_diagnostic();
    assert!(!log_path.exists());
    assert_eq!(FORMATTED_FIELDS.load(Ordering::Relaxed), 0);

    ilium_logging::set_enabled(true).expect("enable");
    ilium_logging::initialize(&log_path, true, "integration-test-handoff", &quota)
        .expect("same-path role handoff");
    tracing::info!(request_body = %expensive_field(), "HTTP request started");
    tracing::error!(response_body = "provider failure", "HTTP request failed");
    tracing::debug!(target: "ilium_inference", request_body = "complete provider payload", "provider payload detail");
    tracing::debug!(target: "ilium_client", evidence = "unrelated sensitive evidence", "unrelated debug detail");
    emit_repeated_diagnostic();
    assert_eq!(FORMATTED_FIELDS.load(Ordering::Relaxed), 2);

    // Event formatting only admits bytes. Read after the ordered file owner
    // acknowledges all preceding writes, preserving every content assertion.
    ilium_logging::request_flush()
        .expect("flush admission")
        .wait_timeout(std::time::Duration::from_secs(2))
        .expect("accepted events flushed before readback");
    let enabled_log = std::fs::read_to_string(&log_path).expect("enabled log");
    assert!(enabled_log.contains("process logging reused for an in-process role handoff"));
    assert!(enabled_log.contains("integration-test-handoff"));
    assert!(enabled_log.contains("complete prompt"));
    assert!(enabled_log.contains("provider failure"));
    assert!(enabled_log.contains("complete provider payload"));
    assert!(!enabled_log.contains("unrelated sensitive evidence"));
    assert!(enabled_log.contains("repeated diagnostic call site"));
    assert_eq!(
        enabled_log.matches("repeated diagnostic call site").count(),
        1
    );
    assert_owner_only(&log_directory, &log_path);

    ilium_logging::set_enabled(false).expect("disable");
    tracing::error!(response_body = "disabled again", "diagnostic test event");
    let disabled_log = std::fs::read_to_string(&log_path).expect("disabled log");
    assert!(!disabled_log.contains("disabled again"));

    let other_path = directory.path().join("other.txt");
    assert!(matches!(
        ilium_logging::initialize(other_path, true, "wrong-path-handoff", &quota),
        Err(ilium_logging::LoggingError::AlreadyInitialized)
    ));
    ilium_logging::request_shutdown_joined()
        .expect("ordered shutdown admitted")
        .wait_until(std::time::Instant::now() + std::time::Duration::from_secs(5))
        .expect("native logger joined")
        .into_result()
        .expect("all earlier writes flushed");
    assert_eq!(
        quota.snapshot().worker_threads,
        1,
        "logger thread charge retires while permanent supervisor remains"
    );
}
