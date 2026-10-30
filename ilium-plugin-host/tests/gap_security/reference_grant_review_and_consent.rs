//! Grant review and consent page (SEC-717, SEC-719, SEC-720).

use crate::support::*;

/// Updates a spec without `--yes`, so a non-interactive session has to decide on its own.
fn update_non_interactive(rig: &OpsRig, spec: &PackageSpec) -> ilium_plugin_host::cli::CliOutput {
    let path = rig.write_package(spec);
    let path_text = path.to_string_lossy().into_owned();
    rig.cli(&["update", &path_text, "--allow-unsigned"])
}

/// REQ: SEC-717
#[test]
fn sec_717_expired_grant_is_treated_as_absent_and_audited_as_grant_expired() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").grant("fs.read:project"));
    let until = rig.clock_now_rfc3339_plus_secs(60);
    let stored = rig.cli(&["plugin", "grant", NOTES, "fs.read:project", "--until", &until]);
    assert_eq!(stored.exit_code, 0, "the grant with --until must be stored");
    rig.advance(secs(120));
    let server = rig.start_server("s1");
    rig.advance(secs(1));
    let refused = server
        .call_plugin(NOTES, "read-user-file")
        .expect_err("an expired grant must count as absent at the next call");
    assert!(format!("{refused:?}").contains("not-permitted"));
    assert_eq!(
        rig.audit_records("grant-expired").len(),
        1,
        "one grant-expired audit line must be written"
    );
}

/// REQ: SEC-717
#[test]
fn sec_717_plugins_screen_shows_an_expired_grant_for_30_days_after_expiry() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").grant("fs.read:project"));
    let until = rig.clock_now_rfc3339_plus_secs(60);
    rig.cli(&["plugin", "grant", NOTES, "fs.read:project", "--until", &until]);
    rig.advance(secs(60 + 29 * 86_400));
    let screen = rig.plugins_screen();
    let row = screen
        .row_for(NOTES)
        .expect("the Plugins screen must still list the plugin 29 days after expiry");
    assert!(
        row.grant_states()
            .iter()
            .any(|(grant, state)| grant == "fs.read:project" && state == "expired"),
        "the grant must show as expired, got {:?}",
        row.grant_states()
    );
    rig.advance(secs(2 * 86_400));
    assert!(
        rig.plugins_screen()
            .row_for(NOTES)
            .map(|row| row.grant_states().is_empty())
            .unwrap_or(true),
        "after 30 days the expired grant must disappear from the Plugins screen"
    );
}

/// REQ: SEC-719
#[test]
fn sec_719_a_plugin_cannot_answer_the_consent_prompt_it_triggered() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").first_use_prompt("net.connect"));
    let server = rig.start_server("s1");
    rig.advance(secs(2));
    let attempt = server.call_plugin(NOTES, "answer-consent");
    assert!(
        attempt.is_err(),
        "no plugin may answer, draw or dismiss the consent prompt"
    );
    assert!(
        rig.prompts_for(NOTES, "net.connect") <= 1,
        "the prompt must not be redrawn by the plugin"
    );
}

/// REQ: SEC-719
#[test]
fn sec_719_consent_prompt_is_rate_limited_to_one_per_plugin_per_30_seconds() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").first_use_prompt("net.connect"));
    let server = rig.start_server("s1");
    for _ in 0..5 {
        let _ = server.call_plugin(NOTES, "connect-probe");
        rig.advance(secs(1));
    }
    assert_eq!(
        rig.prompts_for(NOTES, "net.connect"),
        1,
        "five requests within 5 s must raise a single prompt"
    );
}

/// REQ: SEC-719
#[test]
fn sec_719_unanswered_consent_prompt_auto_denies_after_120_seconds_with_consent_timeout() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").first_use_prompt("net.connect"));
    let server = rig.start_server("s1");
    let _ = server.call_plugin(NOTES, "connect-probe");
    rig.advance(secs(119));
    assert!(
        rig.audit_records("consent-timeout").is_empty(),
        "no timeout before 120 s"
    );
    rig.advance(secs(2));
    assert_eq!(
        rig.audit_records("consent-timeout").len(),
        1,
        "the unanswered prompt must auto-deny and record consent-timeout"
    );
    assert!(!rig.grants_json()[NOTES]["net.connect"]
        .as_bool()
        .unwrap_or(false));
}

/// REQ: SEC-720
#[test]
fn sec_720_widening_update_keeps_the_old_version_and_old_grants_until_the_user_answers() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").grant("fs.read:project"));
    let server = rig.start_server("s1");
    rig.advance(secs(2));
    let grants_before = rig.grants_json()[NOTES].clone();
    update_non_interactive(
        &rig,
        &pkg(NOTES, "1.1.0").grant("fs.read:project").grant("net.connect"),
    );
    rig.advance(secs(2));
    assert_eq!(
        server.active_version(NOTES).as_deref(),
        Some("1.0.0"),
        "the widening update must not activate before the user answers"
    );
    assert_eq!(
        rig.grants_json()[NOTES],
        grants_before,
        "the old grants must stay in force"
    );
}

/// REQ: SEC-720
#[test]
fn sec_720_non_interactive_session_denies_the_widening_and_keeps_the_old_version() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").grant("fs.read:project"));
    let output = update_non_interactive(&rig, &pkg(NOTES, "1.1.0").grant("net.connect"));
    assert_ne!(
        output.exit_code, 0,
        "a non-interactive widening must be denied"
    );
    let server = rig.start_server("s1");
    rig.advance(secs(2));
    assert_eq!(server.active_version(NOTES).as_deref(), Some("1.0.0"));
    assert!(rig.grants_json()[NOTES]["net.connect"].is_null());
}
