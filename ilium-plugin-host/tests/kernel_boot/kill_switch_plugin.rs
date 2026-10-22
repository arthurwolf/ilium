//! reference-admin-runbook and guide-admin-failure-runbook rows HST-516 and HST-517: the
//! per-plugin emergency kill switch.

use ilium_plugin_host::kill_switch::{KillSwitch, PluginState};

/// REQ: HST-516
#[test]
fn hst_516_kill_stops_both_client_sides_and_the_server_side() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut switch = KillSwitch::open(dir.path());
    switch.attach_client("acme.notes", "client-1");
    switch.attach_client("acme.notes", "client-2");

    let report = switch.kill("acme.notes", "bad build", "admin");

    assert!(report.server_stopped);
    assert_eq!(report.stopped_client_ids, vec!["client-1", "client-2"]);
    assert!(!switch.is_client_running("acme.notes", "client-1"));
    assert!(!switch.is_client_running("acme.notes", "client-2"));
}

/// REQ: HST-516
#[test]
fn hst_516_a_killed_plugin_cannot_be_activated_until_unkilled() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut switch = KillSwitch::open(dir.path());
    switch.kill("acme.notes", "bad build", "admin");
    assert!(switch.try_activate("acme.notes").is_err());
    switch.unkill("acme.notes");
    assert!(switch.try_activate("acme.notes").is_ok());
}

/// REQ: HST-516
#[test]
fn hst_516_the_killed_state_survives_a_restart() {
    let dir = tempfile::tempdir().expect("temp dir");
    {
        let mut switch = KillSwitch::open(dir.path());
        switch.kill("acme.notes", "bad build", "admin");
    }
    let reopened = KillSwitch::open(dir.path());
    assert_eq!(
        reopened.plugin_state("acme.notes"),
        PluginState::DisabledByAdmin
    );
}

/// REQ: HST-516
#[test]
fn hst_516_the_kill_works_in_safe_mode() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut switch = KillSwitch::open_safe_mode(dir.path());
    switch.attach_client("acme.notes", "client-1");
    let report = switch.kill("acme.notes", "bad build", "admin");
    assert!(report.server_stopped);
    assert_eq!(
        switch.plugin_state("acme.notes"),
        PluginState::DisabledByAdmin
    );
}

/// REQ: HST-517
#[test]
fn hst_517_the_kill_cancels_every_pending_request_as_cancelled() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut switch = KillSwitch::open(dir.path());
    switch.start_requests("acme.notes", 2);
    switch.kill("acme.notes", "bad build", "admin");
    assert_eq!(switch.request_outcomes("acme.notes"), vec!["cancelled", "cancelled"]);
}

/// REQ: HST-517
#[test]
fn hst_517_the_kill_writes_a_plugin_killed_audit_event_with_admin_and_reason() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut switch = KillSwitch::open(dir.path());
    switch.kill("acme.notes", "bad build", "admin");
    let event = switch.last_audit_event().expect("an audit event is written");
    assert_eq!(event.kind, "plugin.killed");
    assert_eq!(event.administrator, "admin");
    assert_eq!(event.reason, "bad build");
}

/// REQ: HST-517
#[test]
fn hst_517_a_client_that_reconnects_after_the_kill_learns_the_killed_state_first() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut switch = KillSwitch::open(dir.path());
    switch.kill("acme.notes", "bad build", "admin");
    let learned = switch.reconnect_client("acme.notes", "client-3");
    assert_eq!(learned, PluginState::DisabledByAdmin);
    assert!(!switch.is_client_running("acme.notes", "client-3"));
}
