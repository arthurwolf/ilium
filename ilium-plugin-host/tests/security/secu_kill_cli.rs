//! SCU-002, SCU-003, SCU-004, SCU-007 and SCU-010: exit codes, the required reason, policy
//! kills and repeated kills for `ilium plugin kill` and `ilium plugin unkill`.

use ilium_plugin_host::kill_switch::{KillSwitch, PluginState};

use crate::secu_support::{audit_text, env, plugin, snapshot, PLUGIN};

/// REQ: SCU-002
#[test]
fn secu_002_kill_of_an_unknown_id_exits_5_with_not_found_a() {
    let root = tempfile::tempdir().expect("temp dir");
    let env = env(root.path(), true);
    let kill = plugin(&env, &["kill", "no.such.plugin", "--reason", "test"]);
    let unkill = plugin(&env, &["unkill", "no.such.plugin"]);

    assert_eq!(kill.exit_code, 5);
    assert_eq!(unkill.exit_code, 5);
    assert!(kill.stderr.contains("not found"), "stderr: {}", kill.stderr);
    assert!(unkill.stderr.contains("not found"), "stderr: {}", unkill.stderr);
}

/// REQ: SCU-002
#[test]
fn secu_002_unknown_id_writes_no_state_and_no_audit_line_b() {
    let root = tempfile::tempdir().expect("temp dir");
    let env = env(root.path(), true);
    let audit_before = audit_text(&env);
    let state_before = snapshot(&root.path().join("state"));

    plugin(&env, &["kill", "no.such.plugin", "--reason", "test"]);
    plugin(&env, &["unkill", "no.such.plugin"]);

    assert_eq!(audit_text(&env), audit_before, "no new audit line may appear");
    assert_eq!(snapshot(&root.path().join("state")), state_before, "no state may change");
}

/// REQ: SCU-003
#[test]
fn secu_003_kill_and_unkill_without_a_session_exit_6_a() {
    let root = tempfile::tempdir().expect("temp dir");
    let env = env(root.path(), false);

    assert_eq!(plugin(&env, &["kill", PLUGIN, "--reason", "bad build"]).exit_code, 6);
    assert_eq!(plugin(&env, &["unkill", PLUGIN]).exit_code, 6);
}

/// REQ: SCU-003
#[test]
fn secu_003_no_session_leaves_installed_state_and_kill_flag_unchanged_b() {
    let root = tempfile::tempdir().expect("temp dir");
    let env = env(root.path(), false);
    let before = snapshot(&root.path().join("state"));

    plugin(&env, &["kill", PLUGIN, "--reason", "bad build"]);
    plugin(&env, &["unkill", PLUGIN]);

    assert_eq!(snapshot(&root.path().join("state")), before);
}

/// REQ: SCU-004
#[test]
fn secu_004_kill_without_reason_is_refused_and_records_nothing_a() {
    let root = tempfile::tempdir().expect("temp dir");
    let env = env(root.path(), true);
    let out = plugin(&env, &["kill", PLUGIN]);

    assert_ne!(out.exit_code, 0, "kill without --reason must not succeed");
    assert!(!out.stderr.is_empty(), "the refusal must be shown");
    assert_eq!(
        KillSwitch::open(&root.path().join("state")).plugin_state(PLUGIN),
        PluginState::Active,
        "no kill may be recorded"
    );
}

/// REQ: SCU-004
#[test]
fn secu_004_kill_stores_its_reason_in_the_plugin_killed_record_b() {
    let root = tempfile::tempdir().expect("temp dir");
    let env = env(root.path(), true);
    plugin(&env, &["kill", PLUGIN, "--reason", "exfiltrates tokens"]);

    let audit = audit_text(&env);
    let killed = audit
        .lines()
        .find(|line| line.contains("plugin-killed"))
        .expect("a plugin-killed record exists");
    assert!(killed.contains("exfiltrates tokens"), "record: {killed}");
}

/// REQ: SCU-007
#[test]
fn secu_007_unkill_is_refused_while_a_policy_kill_lists_the_plugin_a() {
    let root = tempfile::tempdir().expect("temp dir");
    let mut env = env(root.path(), true);
    let policy = root.path().join("policy.toml");
    std::fs::write(&policy, format!("[[kill]]\nplugin = \"{PLUGIN}\"\n")).expect("policy written");
    env.policy_file = Some(policy);

    plugin(&env, &["kill", PLUGIN, "--reason", "bad build"]);
    let state_before = snapshot(&root.path().join("state"));
    let out = plugin(&env, &["unkill", PLUGIN]);

    assert!(out.stdout.contains("kill.policy-held") || out.stderr.contains("kill.policy-held"));
    assert_eq!(snapshot(&root.path().join("state")), state_before, "state must not change");
}

/// REQ: SCU-007
#[test]
fn secu_007_removing_the_policy_entry_keeps_the_plugin_refused_until_unkill_b() {
    let root = tempfile::tempdir().expect("temp dir");
    let mut env = env(root.path(), true);
    let policy = root.path().join("policy.toml");
    std::fs::write(&policy, format!("[[kill]]\nplugin = \"{PLUGIN}\"\n")).expect("policy written");
    env.policy_file = Some(policy.clone());
    plugin(&env, &["kill", PLUGIN, "--reason", "bad build"]);

    std::fs::write(&policy, "").expect("policy entry removed");

    let mut switch = KillSwitch::open(&root.path().join("state"));
    assert!(
        switch.try_activate(PLUGIN).is_err(),
        "the plugin must stay refused until unkill runs"
    );
}

/// REQ: SCU-010
#[test]
fn secu_010_second_kill_writes_one_more_plugin_killed_record_a() {
    let root = tempfile::tempdir().expect("temp dir");
    let env = env(root.path(), true);
    plugin(&env, &["kill", PLUGIN, "--reason", "first reason"]);
    plugin(&env, &["kill", PLUGIN, "--reason", "second reason"]);

    let records = audit_text(&env).matches("plugin-killed").count();
    assert_eq!(records, 2, "two kills must leave two records");
}

/// REQ: SCU-010
#[test]
fn secu_010_second_kill_keeps_one_disabled_by_admin_state_b() {
    let root = tempfile::tempdir().expect("temp dir");
    let env = env(root.path(), true);
    plugin(&env, &["kill", PLUGIN, "--reason", "first reason"]);
    plugin(&env, &["kill", PLUGIN, "--reason", "second reason"]);

    assert_eq!(
        KillSwitch::open(&root.path().join("state")).plugin_state(PLUGIN),
        PluginState::DisabledByAdmin
    );
    let audit = audit_text(&env);
    assert!(audit.contains("first reason") && audit.contains("second reason"));
}
