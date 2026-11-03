//! SCU-001, SCU-006 and SCU-011: how a user disable, an administrator kill, a policy kill and
//! a block combine on one plugin, and which reason the row shows.

use ilium_plugin_host::kill_switch::{KillSwitch, PluginState};
use ilium_plugin_host::trust::{RevocationReason, TrustDocument, TrustRepository};

use crate::secu_support::{env, plugin, row, PLUGIN};

/// Records a compromised-key revocation that reaches the plugin, through the trust store the
/// CLI reads from its config directory.
fn revoke_key_compromised(config_dir: &std::path::Path) -> ilium_plugin_host::trust::TrustChange {
    let repository =
        TrustRepository::new(config_dir.join("trust.json"), TrustDocument::default())
            .expect("trust repository opens");
    repository
        .revoke(
            "acme-key",
            RevocationReason::Compromised,
            "2026-11-03T10:00:00Z",
            &[PLUGIN.to_owned()],
        )
        .expect("compromised revocation is recorded")
}

/// REQ: SCU-001
#[test]
fn secu_001_user_disable_is_named_after_kill_and_unkill_a() {
    let root = tempfile::tempdir().expect("temp dir");
    let env = env(root.path(), true);
    plugin(&env, &["disable", PLUGIN]);
    plugin(&env, &["kill", PLUGIN, "--reason", "bad build"]);
    plugin(&env, &["unkill", PLUGIN]);

    let shown = row(&env, PLUGIN);
    assert!(
        shown.contains("installed-disabled-by-user"),
        "row must name the user disable, got: {shown}"
    );
}

/// REQ: SCU-001
#[test]
fn secu_001_unkill_lifts_only_the_kill_and_keeps_the_user_disable_b() {
    let root = tempfile::tempdir().expect("temp dir");
    let env = env(root.path(), true);
    plugin(&env, &["disable", PLUGIN]);
    plugin(&env, &["kill", PLUGIN, "--reason", "bad build"]);
    plugin(&env, &["unkill", PLUGIN]);

    let switch = KillSwitch::open(&root.path().join("state"));
    assert_eq!(switch.plugin_state(PLUGIN), PluginState::Active);
    assert!(
        row(&env, PLUGIN).contains("installed-disabled-by-user"),
        "the user disable must still be recorded after unkill"
    );
}

/// REQ: SCU-006
#[test]
fn secu_006_unkill_on_a_blocked_plugin_keeps_it_blocked_a() {
    let root = tempfile::tempdir().expect("temp dir");
    let env = env(root.path(), true);
    revoke_key_compromised(&env.config_dir);
    plugin(&env, &["kill", PLUGIN, "--reason", "bad build"]);
    plugin(&env, &["unkill", PLUGIN]);

    let shown = row(&env, PLUGIN);
    assert!(shown.contains("disabled-blocked"), "state must stay disabled-blocked: {shown}");
    assert!(
        shown.contains("trust.key-revoked"),
        "the block reason trust.key-revoked must be shown: {shown}"
    );
}

/// REQ: SCU-006
#[test]
fn secu_006_block_reason_is_shown_before_and_after_kill_and_unkill_b() {
    let root = tempfile::tempdir().expect("temp dir");
    let env = env(root.path(), true);
    revoke_key_compromised(&env.config_dir);
    let before = row(&env, PLUGIN);
    plugin(&env, &["kill", PLUGIN, "--reason", "bad build"]);
    plugin(&env, &["unkill", PLUGIN]);
    let after = row(&env, PLUGIN);

    assert!(before.contains("trust.key-revoked"), "reason before kill: {before}");
    assert!(after.contains("trust.key-revoked"), "reason after unkill: {after}");
}

/// REQ: SCU-011
#[test]
fn secu_011_revocation_of_a_killed_plugin_sets_blocked_and_keeps_kill_flag_a() {
    let root = tempfile::tempdir().expect("temp dir");
    let env = env(root.path(), true);
    plugin(&env, &["kill", PLUGIN, "--reason", "bad build"]);
    let change = revoke_key_compromised(&env.config_dir);

    assert!(
        change.disabled_blocked.iter().any(|id| id == PLUGIN),
        "revocation must mark the plugin blocked: {change:?}"
    );
    assert_eq!(
        KillSwitch::open(&root.path().join("state")).plugin_state(PLUGIN),
        PluginState::DisabledByAdmin,
        "the kill flag must stay set after the block"
    );
    assert!(row(&env, PLUGIN).contains("Blocked"), "row must show Blocked");
}

/// REQ: SCU-011
#[test]
fn secu_011_unkill_after_block_leaves_disabled_blocked_b() {
    let root = tempfile::tempdir().expect("temp dir");
    let env = env(root.path(), true);
    plugin(&env, &["kill", PLUGIN, "--reason", "bad build"]);
    revoke_key_compromised(&env.config_dir);
    plugin(&env, &["unkill", PLUGIN]);

    let shown = row(&env, PLUGIN);
    assert!(
        shown.contains("disabled-blocked"),
        "after unkill the state must be disabled-blocked: {shown}"
    );
}
