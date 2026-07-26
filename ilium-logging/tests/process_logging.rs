use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicUsize, Ordering};

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
    let directory = tempfile::tempdir().expect("tempdir");
    let log_directory = directory.path().join("session");
    let log_path = log_directory.join("log-2026-07-19_12-00-00.000.txt");

    ilium_logging::initialize(&log_path, false, "integration-test").expect("initialize");
    tracing::error!(message = %expensive_field(), "diagnostic test event");
    emit_repeated_diagnostic();
    assert!(!log_path.exists());
    assert_eq!(FORMATTED_FIELDS.load(Ordering::Relaxed), 0);

    ilium_logging::set_enabled(true).expect("enable");
    ilium_logging::initialize(&log_path, true, "integration-test-handoff")
        .expect("same-path role handoff");
    tracing::info!(request_body = %expensive_field(), "HTTP request started");
    tracing::error!(response_body = "provider failure", "HTTP request failed");
    tracing::debug!(target: "ilium_inference", request_body = "complete provider payload", "provider payload detail");
    tracing::debug!(target: "ilium_client", evidence = "unrelated sensitive evidence", "unrelated debug detail");
    emit_repeated_diagnostic();
    assert_eq!(FORMATTED_FIELDS.load(Ordering::Relaxed), 2);

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
    assert_eq!(
        std::fs::metadata(&log_directory)
            .expect("log directory")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(&log_path)
            .expect("log file")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );

    ilium_logging::set_enabled(false).expect("disable");
    tracing::error!(message = "disabled again", "diagnostic test event");
    let disabled_log = std::fs::read_to_string(&log_path).expect("disabled log");
    assert!(!disabled_log.contains("disabled again"));

    let other_path = directory.path().join("other.txt");
    assert!(matches!(
        ilium_logging::initialize(other_path, true, "wrong-path-handoff"),
        Err(ilium_logging::LoggingError::AlreadyInitialized)
    ));
}
