//! Sandbox page (SEC-707, SEC-708, SEC-709, SEC-710, SEC-711, SEC-712, SEC-732).

use crate::support::*;

/// REQ: SEC-707
#[test]
fn sec_707_component_over_the_function_count_bound_is_refused_before_compilation() {
    let rig = rig();
    let output = install(&rig, &pkg(NOTES, "1.0.0").component_functions(100_001));
    assert_ne!(
        output.exit_code, 0,
        "a 100001-function component must be refused"
    );
    assert!(
        error_codes(&output)
            .iter()
            .any(|code| code == "manifest.component-too-large"),
        "expected manifest.component-too-large, got {:?}",
        error_codes(&output)
    );
    assert_eq!(
        rig.compile_count(NOTES),
        0,
        "the bound is checked before any compilation starts"
    );
}

/// REQ: SEC-708
#[test]
fn sec_708_compilation_runs_in_the_helper_and_a_bad_digest_artifact_is_refused() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    assert!(
        rig.compile_ran_in_helper(NOTES),
        "T2/T3 compilation must run inside the plugin's sandbox helper"
    );
    let server = rig.start_server("s1");
    rig.corrupt_artifact_digest(NOTES);
    rig.advance(secs(5));
    assert_eq!(
        server.active_version(NOTES),
        None,
        "the server must not accept an artifact whose digest does not match"
    );
    assert!(
        server
            .log_codes()
            .iter()
            .any(|code| code == "artifact.digest-mismatch"),
        "the refusal must be logged, got {:?}",
        server.log_codes()
    );
}

/// REQ: SEC-709
#[test]
fn sec_709_helper_rlimits_are_nofile_64_fsize_quota_and_memlock_zero() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").grant("storage.write"));
    let limits = rig.helper_rlimits(NOTES);
    assert_eq!(limits["RLIMIT_NOFILE"], 64);
    assert_eq!(limits["RLIMIT_MEMLOCK"], 0);
    assert_eq!(
        limits["RLIMIT_FSIZE"],
        rig.granted_storage_quota(NOTES),
        "RLIMIT_FSIZE must equal the granted storage quota"
    );
}

/// REQ: SEC-709
#[test]
fn sec_709_a_call_over_the_fsize_limit_returns_quota_exceeded_with_the_limit_name() {
    let rig = rig();
    install_ok(
        &rig,
        &pkg(NOTES, "1.0.0")
            .grant("storage.write")
            .write_over_quota(),
    );
    let server = rig.start_server("s1");
    rig.advance(secs(1));
    let error = server
        .call_plugin(NOTES, "write-big-file")
        .expect_err("a write beyond the storage quota must fail");
    let text = format!("{error:?}");
    assert!(
        text.contains("quota-exceeded"),
        "expected quota-exceeded, got {text}"
    );
    assert!(
        text.contains("RLIMIT_FSIZE"),
        "the detail must name RLIMIT_FSIZE, got {text}"
    );
}

/// REQ: SEC-710
#[test]
fn sec_710_three_fuel_exhaustions_in_60s_disable_the_guest_until_the_user_re_enables_it() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").spins_forever());
    let server = rig.start_server("s1");
    for _ in 0..3 {
        rig.exhaust_fuel(NOTES);
    }
    assert!(
        server.is_disabled_by_failure(NOTES),
        "three repeated limit hits must disable"
    );
    assert!(
        server
            .notices()
            .iter()
            .any(|notice| notice == "Plugin stopped: repeated limit hits"),
        "the user must see the stop notice, got {:?}",
        server.notices()
    );
    assert!(
        rig.audit_records("limit-disable").len() == 1,
        "one limit-disable audit line must be written"
    );
    rig.advance(secs(600));
    assert_eq!(
        server.active_version(NOTES),
        None,
        "the guest must not restart automatically"
    );
}

/// REQ: SEC-711
#[test]
fn sec_711_guest_clock_is_coarsened_to_100_microseconds_without_the_high_resolution_grant() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").clock_probe());
    let resolution_nanos = rig.guest_clock_resolution_nanos(NOTES);
    assert_eq!(
        resolution_nanos, 100_000,
        "T3 guests without high-resolution-time must see 100 microsecond clocks"
    );
}

/// REQ: SEC-711
#[test]
fn sec_711_host_exposes_no_shared_memory_counter_to_any_guest() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let imports = rig.exposed_imports(NOTES);
    assert!(
        imports
            .iter()
            .all(|name| !name.contains("shared") && !name.contains("SharedArrayBuffer")),
        "no shared-memory counter may be exposed, got {imports:?}"
    );
}

/// REQ: SEC-712
#[test]
fn sec_712_landlock_failure_is_audited_as_sandbox_degraded_and_loading_still_succeeds() {
    let rig = rig();
    rig.fail_landlock_ruleset();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("s1");
    rig.advance(secs(2));
    assert_eq!(
        server.active_version(NOTES).as_deref(),
        Some("1.0.0"),
        "a Landlock failure alone must not stop loading when namespaces and seccomp succeeded"
    );
    assert_eq!(rig.audit_records("sandbox-degraded").len(), 1);
}

/// REQ: SEC-732
#[test]
fn sec_732_guest_string_with_controls_and_bidi_overrides_is_escaped_before_display() {
    let rig = rig();
    install_ok(
        &rig,
        &pkg(NOTES, "1.0.0").guest_message("safe\u{202E}evil\u{1b}[2J\u{2066}x\u{2069}"),
    );
    let server = rig.start_server("s1");
    rig.advance(secs(2));
    for shown in server.notices().into_iter().chain(server.toasts()) {
        assert!(
            !shown.chars().any(|c| ('\u{202A}'..='\u{202E}').contains(&c)
                || ('\u{2066}'..='\u{2069}').contains(&c)
                || c.is_control()),
            "guest text reached the UI unescaped: {shown:?}"
        );
    }
}

/// REQ: SEC-707
#[test]
fn sec_707_component_over_the_64_mib_byte_bound_is_refused_before_compilation() {
    let rig = rig();
    let output = install(
        &rig,
        &pkg(NOTES, "1.0.0").declared_size(64 * 1024 * 1024 + 1),
    );
    assert_ne!(
        output.exit_code, 0,
        "a component over 64 MiB must be refused"
    );
    assert!(
        error_codes(&output)
            .iter()
            .any(|code| code == "manifest.component-too-large"),
        "expected manifest.component-too-large, got {:?}",
        error_codes(&output)
    );
    assert_eq!(
        rig.compile_count(NOTES),
        0,
        "no compilation starts for an oversized component"
    );
}

/// REQ: SEC-708
#[test]
fn sec_708_a_server_start_reuses_the_helper_artifact_and_does_not_compile_again() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    assert_eq!(
        rig.compile_count(NOTES),
        1,
        "one helper compilation at install time"
    );
    let _server = rig.start_server("s1");
    assert_eq!(
        rig.compile_count(NOTES),
        1,
        "the server accepts the serialised artifact instead of compiling it"
    );
}

/// REQ: SEC-710
#[test]
fn sec_710_two_fuel_exhaustions_do_not_disable_the_guest() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").spins_forever());
    let server = rig.start_server("s1");
    for _ in 0..2 {
        rig.exhaust_fuel(NOTES);
    }
    assert!(
        !server.is_disabled_by_failure(NOTES),
        "two limit hits are below the threshold of three"
    );
    assert_eq!(
        server.active_version(NOTES).as_deref(),
        Some("1.0.0"),
        "the guest keeps running"
    );
    assert!(rig.audit_records("limit-disable").is_empty());
}

/// REQ: SEC-712
#[test]
fn sec_712_no_sandbox_degraded_audit_when_landlock_succeeds() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let _server = rig.start_server("s1");
    assert!(
        rig.audit_records("sandbox-degraded").is_empty(),
        "a fully applied sandbox writes no sandbox-degraded audit line"
    );
}

/// REQ: SEC-732
#[test]
fn sec_732_plain_guest_text_without_controls_is_shown_unchanged() {
    let rig = rig();
    install_ok(
        &rig,
        &pkg(NOTES, "1.0.0").guest_message("plain status text"),
    );
    let server = rig.start_server("s1");
    rig.advance(secs(2));
    assert!(
        server
            .notices()
            .iter()
            .chain(server.toasts().iter())
            .any(|shown| shown.contains("plain status text")),
        "text without control or bidi characters is displayed as written"
    );
}
