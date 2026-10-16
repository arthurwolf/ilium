//! Plugin privacy (user-privacy.md): USR-060 to USR-062, USR-102, USR-103.

use crate::support::*;

// ---- USR-060 ----

/// REQ: USR-060
#[test]
fn usr_060_one_hour_with_five_plugins_sends_no_request_carrying_a_plugin_id() {
    let mut rig = UserRig::new();
    let ids = ["com.example.p1", "com.example.p2", "com.example.p3", "com.example.p4", "com.example.p5"];
    for id in ids {
        install_ok(&mut rig, &spec(id, "1.0.0", &["tree.read"]));
    }
    rig.set_request_logging(true);
    rig.start_server();
    for id in ids {
        rig.inject_trap(id);
    }
    rig.advance_ms(3_600_000);
    let requests = rig.network_requests();
    for id in ids {
        assert!(requests.iter().all(|request| !request.contains(id)), "{id} leaked in {requests:?}");
    }
    assert!(requests.iter().all(|request| !request.contains("com.example")));
}

/// REQ: USR-060
#[test]
fn usr_060_no_request_carries_plugin_logs_or_errors() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.noisy", "1.0.0", &[]));
    rig.set_request_logging(true);
    rig.start_server();
    rig.plugin_logs("com.example.noisy", "error", "secret-error-text");
    rig.inject_trap("com.example.noisy");
    rig.advance_ms(3_600_000);
    assert!(rig.network_requests().iter().all(|request| !request.contains("secret-error-text")));
}

// ---- USR-061 ----

const PRIVATE_ITEMS: [&str; 9] = [
    "terminal.read-screen",
    "terminal.read-cells",
    "terminal.history",
    "agent.read-prompts",
    "agent.transcript",
    "clipboard.read",
    "audio.capture",
    "audio.loopback",
    "location.observer",
];

/// REQ: USR-061
#[test]
fn usr_061_each_private_item_is_not_permitted_without_its_grant() {
    for grant in PRIVATE_ITEMS {
        let mut rig = UserRig::new();
        install_ok(&mut rig, &spec("com.example.nosy", "1.0.0", &[grant]));
        rig.start_server();
        match rig.call_needing("com.example.nosy", "server", grant) {
            CallOutcome::Refused { code, .. } => assert_eq!(code, "not-permitted", "{grant}"),
            other => panic!("{grant}: expected not-permitted, got {other:?}"),
        }
    }
}

/// REQ: USR-061
#[test]
fn usr_061_the_same_call_works_after_the_grant_is_given() {
    for grant in PRIVATE_ITEMS {
        let mut rig = UserRig::new();
        install_ok(&mut rig, &spec("com.example.nosy", "1.0.0", &[grant]));
        let _ = rig.cli(&["grants", "com.example.nosy", "--grant", grant]);
        rig.start_server();
        assert_ne!(
            rig.call_needing("com.example.nosy", "server", grant),
            CallOutcome::Refused { code: "not-permitted".into(), reason: None },
            "{grant}"
        );
    }
}

// ---- USR-062 ----

/// REQ: USR-062
#[test]
fn usr_062_install_verify_update_uninstall_and_list_work_without_a_network() {
    let mut rig = UserRig::new();
    rig.deny_network();
    let install = rig.install_file(&spec("com.example.offline", "1.0.0", &[]), &["--allow-unsigned", "--yes"]);
    assert_eq!(result_exit_code(&install), 0);
    assert_eq!(result_exit_code(&rig.cli(&["list"])), 0);
    let verify = rig.cli(&["verify", "com.example.offline"]);
    assert_eq!(result_exit_code(&verify), 0);
    let update = rig.install_file(&spec("com.example.offline", "1.1.0", &[]), &["--allow-unsigned", "--yes"]);
    assert_eq!(result_exit_code(&update), 0);
    let uninstall = rig.cli(&["uninstall", "com.example.offline", "--yes"]);
    assert_eq!(result_exit_code(&uninstall), 0);
    assert_eq!(rig.fetch_calls(), 0);
    assert!(rig.network_requests().is_empty());
}

/// REQ: USR-062
#[test]
fn usr_062_an_unreachable_network_is_never_the_reason_for_a_failure_of_these_commands() {
    let mut rig = UserRig::new();
    rig.deny_network();
    let output = rig.install_file(&spec("com.example.offline", "1.0.0", &[]), &["--allow-unsigned", "--yes"]);
    assert!(!error_codes(&output).iter().any(|code| code.contains("network") || code.contains("fetch")), "{:?}", error_codes(&output));
}

// ---- USR-102 ----

/// REQ: USR-102
#[test]
fn usr_102_raw_socket_from_a_third_party_plugin_fails() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.sock", "1.0.0", &["http:api.example.com"]));
    let _ = rig.cli(&["grants", "com.example.sock", "--grant", "http:api.example.com"]);
    rig.start_server();
    match rig.plugin_raw_socket("com.example.sock", "93.184.216.34:80") {
        CallOutcome::Refused { code, .. } => assert_eq!(code, "component.wasi-forbidden"),
        other => panic!("expected component.wasi-forbidden, got {other:?}"),
    }
    assert!(rig.connections_attempted().is_empty());
}

/// REQ: USR-102
#[test]
fn usr_102_request_to_an_ungranted_host_is_not_permitted_and_a_granted_host_is_allowed() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.web", "1.0.0", &["http:api.example.com"]));
    let _ = rig.cli(&["grants", "com.example.web", "--grant", "http:api.example.com"]);
    rig.start_server();
    assert_eq!(rig.plugin_http("com.example.web", "https://api.example.com/x"), CallOutcome::Ok);
    match rig.plugin_http("com.example.web", "https://evil.example.net/x") {
        CallOutcome::Refused { code, .. } => assert_eq!(code, "not-permitted"),
        other => panic!("expected not-permitted, got {other:?}"),
    }
}

/// REQ: USR-102
#[test]
fn usr_102_plain_http_to_a_non_local_host_is_refused_but_localhost_is_allowed() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.web", "1.0.0", &["http:api.example.com", "http:localhost"]));
    let _ = rig.cli(&["grants", "com.example.web", "--grant", "http:api.example.com", "--grant", "http:localhost"]);
    rig.start_server();
    assert!(matches!(rig.plugin_http("com.example.web", "http://api.example.com/x"), CallOutcome::Refused { .. }));
    assert_eq!(rig.plugin_http("com.example.web", "http://localhost/x"), CallOutcome::Ok);
}

// ---- USR-103 ----

/// REQ: USR-103
#[test]
fn usr_103_redirect_to_an_ungranted_host_is_not_permitted() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.web", "1.0.0", &["http:api.example.com"]));
    let _ = rig.cli(&["grants", "com.example.web", "--grant", "http:api.example.com"]);
    rig.start_server();
    rig.script_redirect("https://api.example.com/go", "https://evil.example.net/landing");
    match rig.plugin_http("com.example.web", "https://api.example.com/go") {
        CallOutcome::Refused { code, .. } => assert_eq!(code, "not-permitted"),
        other => panic!("expected not-permitted, got {other:?}"),
    }
    assert!(!rig.connections_attempted().contains(&"evil.example.net".to_string()));
}

/// REQ: USR-103
#[test]
fn usr_103_redirect_to_a_second_granted_host_is_followed() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.web", "1.0.0", &["http:api.example.com", "http:cdn.example.com"]));
    let _ = rig.cli(&["grants", "com.example.web", "--grant", "http:api.example.com", "--grant", "http:cdn.example.com"]);
    rig.start_server();
    rig.script_redirect("https://api.example.com/go", "https://cdn.example.com/file");
    assert_eq!(rig.plugin_http("com.example.web", "https://api.example.com/go"), CallOutcome::Ok);
}
