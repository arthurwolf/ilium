//! Reference, plugin scope and sessions (LIF-160..166).

use ilium_plugin_api::Side;

use crate::common::*;

/// REQ: LIF-160
#[test]
fn lif_160_install_with_scope_project_refused_install_scope_unsupported() {
    let rig = rig();
    let path = rig.write_package(&pkg(NOTES, "1.0.0"));
    let path_text = path.to_string_lossy().into_owned();
    let output = rig.cli(&[
        "install",
        &path_text,
        "--yes",
        "--allow-unsigned",
        "--scope",
        "project",
    ]);
    assert_eq!(output.exit_code, 1);
    assert_eq!(
        error_codes(&output),
        vec!["install.scope-unsupported".to_owned()]
    );
    assert!(registry_ids(&rig).is_empty());
}

/// REQ: LIF-160
#[test]
fn lif_160_install_with_scope_user_succeeds_and_is_the_default() {
    let rig = rig();
    let explicit = rig.write_package(&pkg(NOTES, "1.0.0"));
    let text = explicit.to_string_lossy().into_owned();
    let output = rig.cli(&[
        "install",
        &text,
        "--yes",
        "--allow-unsigned",
        "--scope",
        "user",
    ]);
    assert_eq!(output.exit_code, 0);
    assert_eq!(registry_ids(&rig), vec![NOTES.to_owned()]);
}

/// REQ: LIF-160
#[test]
fn lif_160_install_with_scope_global_or_unknown_is_not_accepted_as_project() {
    let rig = rig();
    let path = rig.write_package(&pkg(NOTES, "1.0.0"));
    let text = path.to_string_lossy().into_owned();
    let output = rig.cli(&[
        "install",
        &text,
        "--yes",
        "--allow-unsigned",
        "--scope",
        "galaxy",
    ]);
    assert_ne!(output.exit_code, 0);
    assert!(registry_ids(&rig).is_empty());
}

/// REQ: LIF-160
#[test]
fn lif_160_project_scope_refusal_leaves_no_staging_files() {
    let rig = rig();
    let path = rig.write_package(&pkg(NOTES, "1.0.0"));
    let text = path.to_string_lossy().into_owned();
    rig.cli(&[
        "install",
        &text,
        "--yes",
        "--allow-unsigned",
        "--scope",
        "project",
    ]);
    assert!(staging_entries(&rig).is_empty());
}

/// Levels: session, project, global. `None` = not set. The first set level wins; none = enabled.
fn expected_enabled(session: Option<bool>, project: Option<bool>, global: Option<bool>) -> bool {
    session.or(project).or(global).unwrap_or(true)
}

/// REQ: LIF-161
#[test]
fn lif_161_all_eight_combinations_of_set_levels_follow_session_project_global() {
    // Each level is either unset or set; with a set level chosen to disagree with the next one,
    // the eight set/unset combinations show the priority order.
    for mask in 0..8u8 {
        let session = (mask & 1 != 0).then_some(false);
        let project = (mask & 2 != 0).then_some(true);
        let global = (mask & 4 != 0).then_some(false);
        let rig = rig();
        install_ok(&rig, &pkg(NOTES, "1.0.0"));
        let server = rig.start_server("s1");
        rig.advance(secs(3));
        rig.set_enabled_levels(NOTES, "s1", session, project, global);
        rig.advance(secs(3));
        assert_eq!(
            server.is_enabled(NOTES),
            expected_enabled(session, project, global),
            "session {session:?} project {project:?} global {global:?}"
        );
    }
}

/// REQ: LIF-161
#[test]
fn lif_161_session_true_beats_project_and_global_false() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("s1");
    rig.set_enabled_levels(NOTES, "s1", Some(true), Some(false), Some(false));
    rig.advance(secs(3));
    assert!(server.is_enabled(NOTES));
}

/// REQ: LIF-161
#[test]
fn lif_161_project_false_beats_global_true_when_the_session_is_unset() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("s1");
    rig.set_enabled_levels(NOTES, "s1", None, Some(false), Some(true));
    rig.advance(secs(3));
    assert!(!server.is_enabled(NOTES));
}

/// REQ: LIF-162
#[test]
fn lif_162_plugins_toml_naming_an_uninstalled_package_installs_nothing() {
    let rig = rig();
    rig.write_project_plugins_toml(&format!("[plugins.\"{NOTES}\"]\nenabled = true\n"));
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    assert!(registry_ids(&rig).is_empty());
    assert_eq!(server.active_version(NOTES), None);
}

/// REQ: LIF-162
#[test]
fn lif_162_plugins_toml_grants_nothing() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").high_risk_grant("process.spawn"));
    let before = rig.grants_json();
    rig.write_project_plugins_toml(&format!(
        "[plugins.\"{NOTES}\"]\nenabled = true\ngrants = [\"process.spawn\"]\n"
    ));
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    assert_eq!(rig.grants_json(), before);
    assert!(!server.granted(NOTES).contains(&"process.spawn".to_owned()));
}

/// REQ: LIF-162
#[test]
fn lif_162_plugins_toml_cannot_raise_a_tier() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let before = server_tier(&rig);
    rig.write_project_plugins_toml(&format!("[plugins.\"{NOTES}\"]\ntier = \"trusted\"\n"));
    let _server = rig.start_server("s1");
    rig.advance(secs(3));
    assert_eq!(server_tier(&rig), before);
}

fn server_tier(rig: &OpsRig) -> serde_json::Value {
    registry_entry(rig, NOTES)["tier"].clone()
}

/// REQ: LIF-162
#[test]
fn lif_162_plugins_toml_enabled_true_is_a_request_for_an_installed_plugin() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.set_enabled_levels(NOTES, "s1", None, None, Some(true));
    rig.write_project_plugins_toml(&format!("[plugins.\"{NOTES}\"]\nenabled = false\n"));
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    assert!(!server.is_enabled(NOTES));
}

/// REQ: LIF-163
#[test]
fn lif_163_global_disable_reaches_two_sessions_within_2_seconds() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let a = rig.start_server("a");
    let b = rig.start_server("b");
    rig.advance(secs(3));
    rig.cli(&["disable", NOTES]);
    rig.advance(secs(2));
    assert!(!a.is_enabled(NOTES));
    assert!(!b.is_enabled(NOTES));
}

/// REQ: LIF-163
#[test]
fn lif_163_global_enable_reaches_two_sessions_within_2_seconds() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.cli(&["disable", NOTES]);
    let a = rig.start_server("a");
    let b = rig.start_server("b");
    rig.advance(secs(3));
    rig.cli(&["enable", NOTES]);
    rig.advance(secs(2));
    assert!(a.is_enabled(NOTES) && b.is_enabled(NOTES));
}

/// REQ: LIF-163
#[test]
fn lif_163_global_uninstall_reaches_two_sessions_within_2_seconds() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let a = rig.start_server("a");
    let b = rig.start_server("b");
    rig.advance(secs(3));
    rig.cli(&["uninstall", NOTES]);
    rig.advance(secs(2));
    assert_eq!(a.active_version(NOTES), None);
    assert_eq!(b.active_version(NOTES), None);
}

/// REQ: LIF-163
#[test]
fn lif_163_the_change_has_not_reached_a_session_after_only_one_millisecond_of_polling_less_than_the_bound(
) {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let a = rig.start_server("a");
    rig.advance(secs(3));
    rig.cli(&["disable", NOTES]);
    rig.advance(ms(2_000));
    assert!(!a.is_enabled(NOTES), "bound is 2 s");
    assert!(rig.virtual_elapsed() >= secs(5));
}

/// REQ: LIF-164
#[test]
fn lif_164_one_session_result_line_per_session_and_exit_0_when_all_succeed() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let _a = rig.start_server_manual("a");
    let _b = rig.start_server_manual("b");
    let output = rig.cli(&["disable", NOTES]);
    assert_eq!(output.exit_code, 0);
    let lines = lines_of(&output, "session-result");
    let mut sessions: Vec<&str> = lines.iter().filter_map(|l| l["session"].as_str()).collect();
    sessions.sort();
    assert_eq!(sessions, vec!["a", "b"]);
    assert!(lines.iter().all(|line| line["ok"] == true));
}

/// REQ: LIF-164
#[test]
fn lif_164_unreachable_session_gives_a_failed_line_and_a_non_zero_exit() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let _a = rig.start_server_manual("a");
    let b = rig.start_server_manual("b");
    b.drop_socket();
    let output = rig.cli(&["disable", NOTES]);
    assert_ne!(output.exit_code, 0);
    let lines = lines_of(&output, "session-result");
    assert_eq!(lines.len(), 2);
    assert!(lines
        .iter()
        .any(|line| line["session"] == "b" && line["ok"] == false));
    assert!(lines
        .iter()
        .any(|line| line["session"] == "a" && line["ok"] == true));
}

/// REQ: LIF-164
#[test]
fn lif_164_with_no_running_session_there_are_no_session_result_lines_and_exit_is_0() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let output = rig.cli(&["disable", NOTES]);
    assert_eq!(output.exit_code, 0);
    assert!(lines_of(&output, "session-result").is_empty());
}

/// REQ: LIF-165
#[test]
fn lif_165_failure_disable_in_session_a_leaves_session_b_enabled() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let a = rig.start_server("a");
    let b = rig.start_server("b");
    rig.advance(secs(3));
    for _ in 0..3 {
        a.fail(NOTES, Side::Server);
        rig.advance(secs(6));
    }
    assert!(a.is_disabled_by_failure(NOTES));
    assert!(!b.is_disabled_by_failure(NOTES));
    assert_eq!(
        b.state_of(NOTES, Side::Server),
        ilium_plugin_api::generated::host::plugins::InstanceState::Active
    );
}

/// REQ: LIF-165
#[test]
fn lif_165_failure_disable_is_not_written_to_the_shared_registry() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let a = rig.start_server("a");
    rig.advance(secs(3));
    let before = rig.registry_bytes();
    for _ in 0..3 {
        a.fail(NOTES, Side::Server);
        rig.advance(secs(6));
    }
    assert_eq!(rig.registry_bytes(), before);
}

/// REQ: LIF-165
#[test]
fn lif_165_calls_to_the_plugin_in_session_b_still_succeed() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let a = rig.start_server("a");
    let b = rig.start_server("b");
    rig.advance(secs(3));
    for _ in 0..3 {
        a.fail(NOTES, Side::Server);
        rig.advance(secs(6));
    }
    assert!(b.call_plugin(NOTES, "ping").is_ok());
    assert!(a.call_plugin(NOTES, "ping").is_err());
}

/// REQ: LIF-166
#[test]
fn lif_166_grant_change_applies_to_every_session() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").grant("fs.read"));
    let a = rig.start_server("a");
    let b = rig.start_server("b");
    rig.advance(secs(3));
    rig.cli(&["revoke", NOTES, "fs.read"]);
    rig.advance(secs(3));
    assert!(!a.granted(NOTES).contains(&"fs.read".to_owned()));
    assert!(!b.granted(NOTES).contains(&"fs.read".to_owned()));
}

/// REQ: LIF-166
#[test]
fn lif_166_not_now_counter_is_shared_between_sessions() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").first_use_prompt("net.http"));
    let a = rig.start_server("a");
    let b = rig.start_server("b");
    rig.advance(secs(3));
    a.answer_first_use(NOTES, "net.http", "not-now");
    assert_eq!(b.not_now_count(NOTES, "net.http"), 1);
    b.answer_first_use(NOTES, "net.http", "not-now");
    assert_eq!(a.not_now_count(NOTES, "net.http"), 2);
}

/// REQ: LIF-166
#[test]
fn lif_166_answer_in_one_session_means_the_other_session_does_not_ask_again() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").first_use_prompt("net.http"));
    let a = rig.start_server("a");
    a.answer_first_use(NOTES, "net.http", "allow");
    let b = rig.start_server("b");
    rig.advance(secs(3));
    assert_eq!(b.prompts_for(NOTES, "net.http"), 0);
    assert!(b.granted(NOTES).contains(&"net.http".to_owned()));
}
