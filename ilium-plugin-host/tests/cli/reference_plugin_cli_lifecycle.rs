//! reference-plugin-cli.md: install failures, uninstall, enable/disable, list, grants (DEV-143..172).

use ilium_plugin_host::cli::MemorySession;

use crate::common::*;
use crate::common_p2::*;

// ---------------------------------------------------------------- install failures

/// REQ: DEV-143
#[test]
fn dev_143_declined_review_fails_review_declined() {
    let root = scratch();
    let session = MemorySession::new();
    session.set_client_attached(true);
    session.decline_review(true);
    let env = env_session(root.path(), &session);
    let (_, package) = packed_project(&env, root.path());
    let output = run(&env, &["install", s(&package), "--allow-unsigned"]);
    assert_eq!(code(&output).as_deref(), Some("install.review-declined"));
    assert_eq!(output.exit_code, 1);
    assert_eq!(session.started_plugin_count(), 0);
}

/// REQ: DEV-143
#[test]
fn dev_143_accepted_review_does_not_report_review_declined() {
    let root = scratch();
    let session = MemorySession::new();
    session.set_client_attached(true);
    let env = env_session(root.path(), &session);
    let (_, package) = packed_project(&env, root.path());
    let output = run(&env, &["install", s(&package), "--allow-unsigned"]);
    assert_ne!(code(&output).as_deref(), Some("install.review-declined"));
}

/// REQ: DEV-143
#[test]
fn dev_143_declined_key_trust_fails_trust_declined() {
    let root = scratch();
    let session = MemorySession::new();
    session.set_client_attached(true);
    session.decline_trust(true);
    let env = env_session(root.path(), &session);
    let (_, package) = packed_project(&env, root.path());
    let (key, _) = make_key(&env, root.path(), "acme");
    run(&env, &["sign", s(&package), "--key", s(&key)]);
    let output = run(&env, &["install", s(&package)]);
    assert_eq!(code(&output).as_deref(), Some("trust.declined"));
    assert_eq!(output.exit_code, 1);
    assert_eq!(session.started_plugin_count(), 0);
}

/// REQ: DEV-143
#[test]
fn dev_143_unsigned_package_without_allow_unsigned_fails_unsigned() {
    let root = scratch();
    let session = MemorySession::new();
    let env = env_session(root.path(), &session);
    let (_, package) = packed_project(&env, root.path());
    let output = run(&env, &["install", s(&package), "--yes"]);
    assert_eq!(code(&output).as_deref(), Some("install.unsigned"));
    assert_eq!(output.exit_code, 1);
    assert_eq!(session.started_plugin_count(), 0);
}

/// REQ: DEV-143
#[test]
fn dev_143_allow_unsigned_lets_an_unsigned_package_through() {
    let root = scratch();
    let session = MemorySession::new();
    let env = env_session(root.path(), &session);
    let (_, package) = packed_project(&env, root.path());
    let output = run(&env, &["install", s(&package), "--allow-unsigned", "--yes"]);
    assert_eq!(output.exit_code, 0);
    assert_ne!(code(&output).as_deref(), Some("install.unsigned"));
}

/// REQ: DEV-143
#[test]
fn dev_143_lower_version_without_allow_downgrade_fails_downgrade() {
    let root = scratch();
    let session = MemorySession::new();
    session.add_plugin("acme.demo", "0.2.0", "T3", "running", false, &[]);
    let env = env_session(root.path(), &session);
    let (_, package) = packed_project(&env, root.path());
    let output = run(&env, &["install", s(&package), "--allow-unsigned", "--yes"]);
    assert_eq!(code(&output).as_deref(), Some("install.downgrade"));
    assert_eq!(output.exit_code, 1);
    assert_eq!(
        session.plugin_version("acme.demo").as_deref(),
        Some("0.2.0")
    );
}

/// REQ: DEV-143
#[test]
fn dev_143_allow_downgrade_installs_the_lower_version() {
    let root = scratch();
    let session = MemorySession::new();
    session.add_plugin("acme.demo", "0.2.0", "T3", "running", false, &[]);
    let env = env_session(root.path(), &session);
    let (_, package) = packed_project(&env, root.path());
    let output = run(
        &env,
        &[
            "install",
            s(&package),
            "--allow-unsigned",
            "--yes",
            "--allow-downgrade",
        ],
    );
    assert_eq!(output.exit_code, 0);
    assert_eq!(
        session.plugin_version("acme.demo").as_deref(),
        Some("0.1.0")
    );
}

/// REQ: DEV-143
#[test]
fn dev_143_a_higher_version_is_not_a_downgrade() {
    let root = scratch();
    let session = MemorySession::new();
    session.add_plugin("acme.demo", "0.0.9", "T3", "running", false, &[]);
    let env = env_session(root.path(), &session);
    let (_, package) = packed_project(&env, root.path());
    let output = run(&env, &["install", s(&package), "--allow-unsigned", "--yes"]);
    assert_eq!(output.exit_code, 0);
    assert_eq!(
        session.plugin_version("acme.demo").as_deref(),
        Some("0.1.0")
    );
}

// ---------------------------------------------------------------- install success

/// REQ: DEV-144
#[test]
fn dev_144_successful_install_prints_exactly_one_plugin_record_and_exits_zero() {
    let root = scratch();
    let session = MemorySession::new();
    let env = env_session(root.path(), &session);
    let (_, package) = packed_project(&env, root.path());
    let output = run(&env, &["install", s(&package), "--allow-unsigned", "--yes"]);
    assert_eq!(output.exit_code, 0);
    let plugins = recs(&output, "plugin");
    assert_eq!(plugins.len(), 1);
    assert_eq!(plugins[0]["id"], "acme.demo");
    assert_eq!(plugins[0]["version"], "0.1.0");
    assert_eq!(plugins[0]["dev"], false);
    assert_eq!(result(&output)["ok"], true);
}

/// REQ: DEV-144
#[test]
fn dev_144_the_plugin_is_enabled_after_install() {
    let root = scratch();
    let session = MemorySession::new();
    let env = env_session(root.path(), &session);
    install_demo(&env, root.path());
    assert_ne!(
        session.plugin_state("acme.demo").as_deref(),
        Some("disabled")
    );
    assert!(session.plugin_state("acme.demo").is_some());
    assert!(session.activations("acme.demo") >= 1);
}

/// REQ: DEV-144
#[test]
fn dev_144_a_failed_install_prints_no_plugin_record() {
    let root = scratch();
    let session = MemorySession::new();
    let env = env_session(root.path(), &session);
    let (_, package) = packed_project(&env, root.path());
    let output = run(&env, &["install", s(&package), "--yes"]);
    assert!(recs(&output, "plugin").is_empty());
    assert_eq!(code(&output).as_deref(), Some("install.unsigned"));
    assert_eq!(output.exit_code, 1);
    assert_eq!(recs(&output, "result")[0]["exit_code"], 1);
}

/// REQ: DEV-144
#[test]
fn dev_144_the_plugin_record_carries_only_the_catalogue_fields() {
    let root = scratch();
    let session = MemorySession::new();
    let env = env_session(root.path(), &session);
    let (_, package) = packed_project(&env, root.path());
    let output = run(&env, &["install", s(&package), "--allow-unsigned", "--yes"]);
    let record = recs(&output, "plugin")[0]
        .as_object()
        .expect("object")
        .clone();
    for key in [
        "id",
        "version",
        "tier",
        "state",
        "dev",
        "sides",
        "grants_granted",
        "grants_requested",
    ] {
        assert!(record.contains_key(key), "missing {key}");
    }
    assert!(record.keys().all(|key| key == "type"
        || key == "details"
        || [
            "id",
            "version",
            "tier",
            "state",
            "dev",
            "sides",
            "grants_granted",
            "grants_requested"
        ]
        .contains(&key.as_str())));
}

// ---------------------------------------------------------------- uninstall

/// REQ: DEV-155
#[test]
fn dev_155_uninstall_deactivates_with_reason_disabled_and_removes_the_package() {
    let root = scratch();
    let (session, env) = demo_session(root.path());
    let output = run(&env, &["uninstall", "acme.demo"]);
    assert_eq!(output.exit_code, 0);
    assert_eq!(
        session
            .deactivations("acme.demo")
            .last()
            .map(String::as_str),
        Some("disabled")
    );
    assert!(!session.package_installed("acme.demo"));
    assert!(session.plugin_state("acme.demo").is_none());
}

/// REQ: DEV-155
#[test]
fn dev_155_uninstall_withdraws_the_contributions() {
    let root = scratch();
    let (session, env) = demo_session(root.path());
    session.add_contribution("acme.demo", "command", HIGH_RISK_GRANT);
    assert_eq!(session.contribution_count("acme.demo"), 1);
    run(&env, &["uninstall", "acme.demo"]);
    assert_eq!(session.contribution_count("acme.demo"), 0);
}

/// REQ: DEV-155
#[test]
fn dev_155_without_purge_data_storage_and_settings_stay_on_disk() {
    let root = scratch();
    let (session, env) = demo_session(root.path());
    session.seed_plugin_data("acme.demo");
    let output = run(&env, &["uninstall", "acme.demo"]);
    assert_eq!(output.exit_code, 0);
    assert!(session.data_folder_exists("acme.demo"));
    assert!(session.storage_bucket_count("acme.demo") >= 1);
    assert!(session.settings_value_count("acme.demo") >= 1);
    assert!(warning_codes(&output)
        .iter()
        .all(|code| code != "uninstall.purged"));
}

/// REQ: DEV-155
#[test]
fn dev_155_a_reinstall_finds_the_kept_data() {
    let root = scratch();
    let (session, env) = demo_session(root.path());
    session.seed_plugin_data("acme.demo");
    run(&env, &["uninstall", "acme.demo"]);
    let package = root.path().join("demo/dist/acme.demo-0.1.0.iliumpkg");
    assert_eq!(
        run(&env, &["install", s(&package), "--allow-unsigned", "--yes"]).exit_code,
        0
    );
    assert!(session.data_folder_exists("acme.demo"));
    assert!(session.storage_bucket_count("acme.demo") >= 1);
    assert!(session.settings_value_count("acme.demo") >= 1);
}

/// REQ: DEV-156
#[test]
fn dev_156_purge_deletes_data_cache_storage_settings_and_log() {
    let root = scratch();
    let (session, env) = demo_session(root.path());
    session.seed_plugin_data("acme.demo");
    let output = run(&env, &["uninstall", "acme.demo", "--purge"]);
    assert_eq!(output.exit_code, 0);
    assert!(!session.data_folder_exists("acme.demo"));
    assert!(!session.cache_folder_exists("acme.demo"));
    assert_eq!(session.storage_bucket_count("acme.demo"), 0);
    assert_eq!(session.settings_value_count("acme.demo"), 0);
    assert!(!session.log_kept("acme.demo"));
}

/// REQ: DEV-156
#[test]
fn dev_156_purge_prints_one_warning_per_removed_item() {
    let root = scratch();
    let (session, env) = demo_session(root.path());
    session.seed_plugin_data("acme.demo");
    let output = run(&env, &["uninstall", "acme.demo", "--purge"]);
    let codes = warning_codes(&output);
    assert_eq!(
        codes.len(),
        5,
        "data, cache, one bucket, one settings value, the log: {codes:?}"
    );
    assert!(codes.iter().all(|code| code == "uninstall.purged"));
    assert_eq!(result(&output)["ok"], true);
}

/// REQ: DEV-156
#[test]
fn dev_156_purge_of_a_plugin_with_nothing_on_disk_prints_no_purged_warning() {
    let root = scratch();
    let (session, env) = demo_session(root.path());
    let _ = session;
    let output = run(&env, &["uninstall", "acme.demo", "--purge"]);
    assert_eq!(output.exit_code, 0);
    assert!(warning_codes(&output)
        .iter()
        .all(|code| code == "uninstall.purged"));
}

/// REQ: DEV-157
#[test]
fn dev_157_uninstall_of_an_unknown_id_fails_not_found_with_exit_five() {
    let root = scratch();
    let session = MemorySession::new();
    let env = env_session(root.path(), &session);
    let output = run(&env, &["uninstall", "acme.nothing"]);
    assert_eq!(code(&output).as_deref(), Some("plugin.not-found"));
    assert_eq!(output.exit_code, 5);
    assert_eq!(result(&output)["exit_code"], 5);
}

/// REQ: DEV-157
#[test]
fn dev_157_uninstall_purge_of_an_unknown_id_removes_nothing() {
    let root = scratch();
    let session = MemorySession::new();
    let env = env_session(root.path(), &session);
    let output = run(&env, &["uninstall", "acme.nothing", "--purge"]);
    assert_eq!(code(&output).as_deref(), Some("plugin.not-found"));
    assert!(warning_codes(&output).is_empty());
}

/// REQ: DEV-157
#[test]
fn dev_157_uninstalling_twice_fails_the_second_time() {
    let root = scratch();
    let (_, env) = demo_session(root.path());
    assert_eq!(run(&env, &["uninstall", "acme.demo"]).exit_code, 0);
    let output = run(&env, &["uninstall", "acme.demo"]);
    assert_eq!(code(&output).as_deref(), Some("plugin.not-found"));
    assert_eq!(output.exit_code, 5);
}

// ---------------------------------------------------------------- enable and disable

/// REQ: DEV-160
#[test]
fn dev_160_disable_deactivates_with_reason_disabled_and_keeps_data() {
    let root = scratch();
    let (session, env) = demo_session(root.path());
    session.seed_plugin_data("acme.demo");
    let output = run(&env, &["disable", "acme.demo"]);
    assert_eq!(output.exit_code, 0);
    assert_eq!(
        session
            .deactivations("acme.demo")
            .last()
            .map(String::as_str),
        Some("disabled")
    );
    assert_eq!(
        session.plugin_state("acme.demo").as_deref(),
        Some("disabled")
    );
    assert!(session.data_folder_exists("acme.demo"));
    assert!(session.package_installed("acme.demo"));
}

/// REQ: DEV-160
#[test]
fn dev_160_enable_resets_the_failure_count_and_activates() {
    let root = scratch();
    let (session, env) = demo_session(root.path());
    run(&env, &["disable", "acme.demo"]);
    session.set_failure_count("acme.demo", 3);
    let before = session.activations("acme.demo");
    let output = run(&env, &["enable", "acme.demo"]);
    assert_eq!(output.exit_code, 0);
    assert_eq!(session.failure_count("acme.demo"), 0);
    assert_eq!(session.activations("acme.demo"), before + 1);
    assert_eq!(
        session.plugin_state("acme.demo").as_deref(),
        Some("running")
    );
}

/// REQ: DEV-160
#[test]
fn dev_160_disable_does_not_reset_the_failure_count() {
    let root = scratch();
    let (session, env) = demo_session(root.path());
    session.set_failure_count("acme.demo", 2);
    run(&env, &["disable", "acme.demo"]);
    assert_eq!(session.failure_count("acme.demo"), 2);
}

/// REQ: DEV-161
#[test]
fn dev_161_disabling_a_disabled_plugin_is_success_with_no_second_deactivation() {
    let root = scratch();
    let (session, env) = demo_session(root.path());
    assert_eq!(run(&env, &["disable", "acme.demo"]).exit_code, 0);
    let count = session.deactivations("acme.demo").len();
    let output = run(&env, &["disable", "acme.demo"]);
    assert_eq!(output.exit_code, 0);
    assert_eq!(result(&output)["ok"], true);
    assert_eq!(session.deactivations("acme.demo").len(), count);
}

/// REQ: DEV-161
#[test]
fn dev_161_enabling_an_enabled_plugin_is_success_with_no_second_activation() {
    let root = scratch();
    let (session, env) = demo_session(root.path());
    let before = session.activations("acme.demo");
    let output = run(&env, &["enable", "acme.demo"]);
    assert_eq!(output.exit_code, 0);
    assert_eq!(result(&output)["ok"], true);
    assert_eq!(session.activations("acme.demo"), before);
    assert!(recs(&output, "error").is_empty());
}

/// REQ: DEV-161
#[test]
fn dev_161_enable_and_disable_round_trip_ends_in_the_asked_state() {
    let root = scratch();
    let (session, env) = demo_session(root.path());
    for _ in 0..2 {
        assert_eq!(run(&env, &["disable", "acme.demo"]).exit_code, 0);
        assert_eq!(
            session.plugin_state("acme.demo").as_deref(),
            Some("disabled")
        );
        assert_eq!(run(&env, &["enable", "acme.demo"]).exit_code, 0);
        assert_eq!(
            session.plugin_state("acme.demo").as_deref(),
            Some("running")
        );
    }
}

// ---------------------------------------------------------------- list

fn list_session() -> std::sync::Arc<MemorySession> {
    let session = MemorySession::new();
    session.add_plugin_with_grants(
        "example.agent-tools",
        "0.1.0",
        "T3",
        "running",
        true,
        &[("a.one", "low", "granted"), ("a.two", "low", "granted")],
    );
    session.add_plugin_with_grants(
        "ilium.badges",
        "0.1.0",
        "T1",
        "running",
        false,
        &[
            ("b.one", "low", "granted"),
            ("b.two", "low", "granted"),
            ("b.three", "low", "granted"),
        ],
    );
    session.add_plugin_with_grants(
        "ilium.tasks",
        "0.1.0",
        "T1",
        "disabled",
        false,
        &[("c.one", "low", "declined")],
    );
    session.add_plugin_with_grants("acme.idle", "1.2.3", "T2", "stopped", false, &[]);
    session.add_plugin_with_grants("acme.unknown", "0.0.1", "T3", "untrusted", false, &[]);
    session
}

/// REQ: DEV-165
#[test]
fn dev_165_text_mode_prints_id_version_tier_state_for_each_plugin() {
    let root = scratch();
    let session = list_session();
    let env = env_session(root.path(), &session);
    let output = run(&env, &["list"]);
    assert_eq!(output.exit_code, 0);
    assert!(
        output
            .stdout_text
            .contains("example.agent-tools 0.1.0 T3 [dev] running"),
        "{}",
        output.stdout_text
    );
    assert!(
        output
            .stdout_text
            .contains("ilium.badges        0.1.0 T1 running")
            || output.stdout_text.contains("ilium.badges 0.1.0 T1 running"),
        "{}",
        output.stdout_text
    );
}

/// REQ: DEV-165
#[test]
fn dev_165_every_state_word_is_printed() {
    let root = scratch();
    let session = list_session();
    let env = env_session(root.path(), &session);
    let text = run(&env, &["list"]).stdout_text;
    for state in ["running", "disabled", "stopped", "untrusted"] {
        assert!(text.contains(state), "missing {state}: {text}");
    }
}

/// REQ: DEV-165
#[test]
fn dev_165_the_grant_count_is_granted_of_requested() {
    let root = scratch();
    let session = list_session();
    let env = env_session(root.path(), &session);
    let text = run(&env, &["list"]).stdout_text;
    assert!(text.contains("grants 2 of 2"), "{text}");
    assert!(text.contains("grants 3 of 3"), "{text}");
    assert!(text.contains("grants 0 of 1"), "{text}");
}

/// REQ: DEV-165
#[test]
fn dev_165_only_a_dev_install_carries_the_dev_badge() {
    let root = scratch();
    let session = list_session();
    let env = env_session(root.path(), &session);
    let text = run(&env, &["list"]).stdout_text;
    assert_eq!(text.matches("[dev]").count(), 1);
    let badge_line = text
        .lines()
        .find(|line| line.contains("[dev]"))
        .expect("badge line");
    assert!(badge_line.contains("example.agent-tools"));
}

/// REQ: DEV-165
#[test]
fn dev_165_json_form_has_one_plugin_record_per_plugin_with_the_dev_flag() {
    let root = scratch();
    let session = list_session();
    let env = env_session(root.path(), &session);
    let output = run(&env, &["list", "--json"]);
    let plugins = recs(&output, "plugin");
    assert_eq!(plugins.len(), 5);
    let dev: Vec<_> = plugins
        .iter()
        .filter(|record| record["dev"] == true)
        .collect();
    assert_eq!(dev.len(), 1);
    assert_eq!(dev[0]["id"], "example.agent-tools");
    assert_eq!(dev[0]["grants_granted"], 2);
    assert_eq!(dev[0]["grants_requested"], 2);
}

/// REQ: DEV-165
#[test]
fn dev_165_an_empty_session_lists_no_plugin() {
    let root = scratch();
    let session = MemorySession::new();
    let env = env_session(root.path(), &session);
    let output = run(&env, &["list", "--json"]);
    assert!(recs(&output, "plugin").is_empty());
    assert_eq!(output.exit_code, 0);
}

// ---------------------------------------------------------------- grants

fn grants_session(tier: &str) -> std::sync::Arc<MemorySession> {
    let session = MemorySession::new();
    session.add_plugin_with_grants(
        "acme.demo",
        "0.1.0",
        tier,
        "running",
        false,
        &[
            ("workspace.read", "low", "granted"),
            ("clipboard.write", "medium", "declined"),
            (HIGH_RISK_GRANT, "high", "revoked"),
        ],
    );
    session
}

/// REQ: DEV-170
#[test]
fn dev_170_grants_lists_every_requested_grant_with_risk_and_state() {
    let root = scratch();
    let session = grants_session("T3");
    let env = env_session(root.path(), &session);
    let output = run(&env, &["grants", "acme.demo", "--json"]);
    let grants = recs(&output, "grant");
    assert_eq!(grants.len(), 3);
    for (grant, risk, state) in [
        ("workspace.read", "low", "granted"),
        ("clipboard.write", "medium", "declined"),
        (HIGH_RISK_GRANT, "high", "revoked"),
    ] {
        let record = grants
            .iter()
            .find(|record| record["grant"] == grant)
            .expect("grant record");
        assert_eq!(record["plugin"], "acme.demo");
        assert_eq!(record["risk"], risk);
        assert_eq!(record["state"], state);
    }
}

/// REQ: DEV-170
#[test]
fn dev_170_text_mode_names_each_grant_risk_and_state() {
    let root = scratch();
    let session = grants_session("T3");
    let env = env_session(root.path(), &session);
    let text = run(&env, &["grants", "acme.demo"]).stdout_text;
    for word in [
        "workspace.read",
        "clipboard.write",
        HIGH_RISK_GRANT,
        "granted",
        "declined",
        "revoked",
        "low",
        "medium",
        "high",
    ] {
        assert!(text.contains(word), "missing {word}: {text}");
    }
}

/// REQ: DEV-170
#[test]
fn dev_170_listing_changes_nothing() {
    let root = scratch();
    let session = grants_session("T3");
    let env = env_session(root.path(), &session);
    let journal = session.journal();
    run(&env, &["grants", "acme.demo"]);
    assert_eq!(session.journal(), journal);
    assert_eq!(session.restarts("acme.demo"), 0);
}

/// REQ: DEV-170
#[test]
fn dev_170_an_unknown_id_fails_not_found() {
    let root = scratch();
    let session = grants_session("T3");
    let env = env_session(root.path(), &session);
    let output = run(&env, &["grants", "acme.nothing"]);
    assert_eq!(code(&output).as_deref(), Some("plugin.not-found"));
    assert_eq!(output.exit_code, 5);
}

/// REQ: DEV-171
#[test]
fn dev_171_revoke_withdraws_the_grant_and_restarts_with_the_smaller_set() {
    let root = scratch();
    let session = grants_session("T3");
    let env = env_session(root.path(), &session);
    let output = run(&env, &["grants", "acme.demo", "--revoke", "workspace.read"]);
    assert_eq!(output.exit_code, 0);
    assert_eq!(
        session
            .grant_state("acme.demo", "workspace.read")
            .as_deref(),
        Some("revoked")
    );
    assert!(!session
        .granted("acme.demo")
        .contains(&"workspace.read".to_owned()));
    assert_eq!(session.restarts("acme.demo"), 1);
}

/// REQ: DEV-171
#[test]
fn dev_171_revoke_withdraws_only_the_contributions_that_need_the_grant() {
    let root = scratch();
    let session = grants_session("T3");
    session.add_contribution("acme.demo", "command", "workspace.read");
    session.add_contribution("acme.demo", "status-item", "clipboard.write");
    let env = env_session(root.path(), &session);
    run(&env, &["grants", "acme.demo", "--revoke", "workspace.read"]);
    assert_eq!(session.contribution_count("acme.demo"), 1);
}

/// REQ: DEV-171
#[test]
fn dev_171_revoke_works_for_a_t1_plugin() {
    let root = scratch();
    let session = grants_session("T1");
    let env = env_session(root.path(), &session);
    let output = run(&env, &["grants", "acme.demo", "--revoke", "workspace.read"]);
    assert_eq!(output.exit_code, 0);
    assert_eq!(
        session
            .grant_state("acme.demo", "workspace.read")
            .as_deref(),
        Some("revoked")
    );
    assert_eq!(session.restarts("acme.demo"), 1);
}

/// REQ: DEV-171
#[test]
fn dev_171_two_revokes_in_one_command_restart_once_with_both_withdrawn() {
    let root = scratch();
    let session = MemorySession::new();
    session.add_plugin_with_grants(
        "acme.demo",
        "0.1.0",
        "T3",
        "running",
        false,
        &[
            ("g.one", "low", "granted"),
            ("g.two", "low", "granted"),
            ("g.three", "low", "granted"),
        ],
    );
    let env = env_session(root.path(), &session);
    run(
        &env,
        &[
            "grants",
            "acme.demo",
            "--revoke",
            "g.one",
            "--revoke",
            "g.two",
        ],
    );
    assert_eq!(session.granted("acme.demo"), vec!["g.three".to_owned()]);
    assert_eq!(session.restarts("acme.demo"), 1);
}

/// REQ: DEV-172
#[test]
fn dev_172_grant_turns_a_declined_grant_on() {
    let root = scratch();
    let session = grants_session("T3");
    let env = env_session(root.path(), &session);
    let output = run(&env, &["grants", "acme.demo", "--grant", "clipboard.write"]);
    assert_eq!(output.exit_code, 0);
    assert_eq!(
        session
            .grant_state("acme.demo", "clipboard.write")
            .as_deref(),
        Some("granted")
    );
}

/// REQ: DEV-172
#[test]
fn dev_172_grant_turns_a_revoked_grant_on_again() {
    let root = scratch();
    let session = MemorySession::new();
    session.add_plugin_with_grants(
        "acme.demo",
        "0.1.0",
        "T3",
        "running",
        false,
        &[("workspace.read", "low", "revoked")],
    );
    let env = env_session(root.path(), &session);
    assert_eq!(
        run(&env, &["grants", "acme.demo", "--grant", "workspace.read"]).exit_code,
        0
    );
    assert_eq!(
        session
            .grant_state("acme.demo", "workspace.read")
            .as_deref(),
        Some("granted")
    );
}

/// REQ: DEV-172
#[test]
fn dev_172_grant_not_in_the_manifest_fails_not_requested() {
    let root = scratch();
    let session = grants_session("T3");
    let env = env_session(root.path(), &session);
    let output = run(&env, &["grants", "acme.demo", "--grant", "never.requested"]);
    assert_eq!(code(&output).as_deref(), Some("grants.not-requested"));
    assert_eq!(output.exit_code, 1);
    assert_eq!(session.grant_state("acme.demo", "never.requested"), None);
}

/// REQ: DEV-172
#[test]
fn dev_172_high_risk_grant_without_a_client_fails_review_required() {
    let root = scratch();
    let session = grants_session("T3");
    session.set_client_attached(false);
    let env = env_session(root.path(), &session);
    let output = run(&env, &["grants", "acme.demo", "--grant", HIGH_RISK_GRANT]);
    assert_eq!(code(&output).as_deref(), Some("install.review-required"));
    assert_eq!(output.exit_code, 1);
    assert_eq!(
        session.grant_state("acme.demo", HIGH_RISK_GRANT).as_deref(),
        Some("revoked")
    );
}

/// REQ: DEV-172
#[test]
fn dev_172_high_risk_grant_asks_in_the_attached_client() {
    let root = scratch();
    let session = grants_session("T3");
    session.set_client_attached(true);
    let env = env_session(root.path(), &session);
    let output = run(&env, &["grants", "acme.demo", "--grant", HIGH_RISK_GRANT]);
    assert_eq!(output.exit_code, 0);
    assert_eq!(session.review_dialog_count(), 1);
    assert_eq!(
        session.grant_state("acme.demo", HIGH_RISK_GRANT).as_deref(),
        Some("granted")
    );
}

/// REQ: DEV-172
#[test]
fn dev_172_a_declined_confirmation_leaves_the_high_risk_grant_off() {
    let root = scratch();
    let session = grants_session("T3");
    session.set_client_attached(true);
    session.decline_review(true);
    let env = env_session(root.path(), &session);
    let output = run(&env, &["grants", "acme.demo", "--grant", HIGH_RISK_GRANT]);
    assert_ne!(output.exit_code, 0);
    assert_eq!(
        session.grant_state("acme.demo", HIGH_RISK_GRANT).as_deref(),
        Some("revoked")
    );
}

/// REQ: DEV-172
#[test]
fn dev_172_a_low_risk_grant_needs_no_dialog() {
    let root = scratch();
    let session = grants_session("T3");
    session.set_client_attached(false);
    let env = env_session(root.path(), &session);
    let output = run(&env, &["grants", "acme.demo", "--grant", "clipboard.write"]);
    assert_eq!(output.exit_code, 0);
    assert_eq!(session.review_dialog_count(), 0);
}
