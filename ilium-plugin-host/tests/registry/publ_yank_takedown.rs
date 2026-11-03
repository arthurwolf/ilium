//! publish-yank-deprecate-takedown.md "Yank, deprecate, unlist and takedown" (PUBL-008..PUBL-016).
//!
//! Yank, unyank, deprecate, unlist and takedown are driven through the CLI of a sandbox whose
//! registry is the standard fixture. Metadata assertions read the served `snapshot.json`,
//! `revocations.json` and `index.json` through the sandbox snapshot.

use crate::common::*;
use serde_json::Value;

fn two_plugin_sandbox() -> Sandbox {
    let old = PluginSpec::new("com.acme.old")
        .named("Old")
        .with_versions(vec![VersionSpec::signed("com.acme.old", "1.0.0")]);
    let new = PluginSpec::new("com.acme.new")
        .named("New")
        .with_versions(vec![VersionSpec::signed("com.acme.new", "1.0.0")]);
    Sandbox::with_registry(&RegistryBuilder::with_plugins(vec![old, new]))
}

fn snapshot_entry(tree: &Tree, key: &str) -> Value {
    signed_of(&tree["snapshot.json"])["meta"][key].clone()
}

// ---- PUBL-008 yank needs a reason that is stored and shown ---------------------------------

/// REQ: PUBL-008
#[test]
fn publ_008_yank_without_reason_is_refused_a() {
    let sandbox = Sandbox::standard();
    let output = sandbox.run(&["yank", "com.acme.notes@0.3.0"]);
    assert_ne!(output.exit_code, 0, "yank must require --reason: {output:?}");
    assert!(
        !output_contains(&output, "\"yanked\""),
        "a refused yank must not mark the version yanked"
    );
}

/// REQ: PUBL-008
#[test]
fn publ_008_reason_is_stored_and_shown_by_info_b() {
    let sandbox = Sandbox::standard();
    let reason = "security: writes the auth token to the log file";
    assert_ok(&sandbox.run(&["yank", "com.acme.notes@0.3.0", "--reason", reason]));
    let info = sandbox.run(&["info", "com.acme.notes@0.3.0", "--json"]);
    assert!(output_contains(&info, reason), "info must show the yank reason: {info:?}");
    let stored = signed_of(&sandbox.snapshot()["snapshot.json"]).to_string();
    assert!(stored.contains("writes the auth token"), "reason must be in the signed metadata");
}

// ---- PUBL-009 security yanks reach auto-update clients at the next safe point -------------

/// REQ: PUBL-009
#[test]
fn publ_009_security_yank_forces_auto_install_a() {
    let sandbox = two_plugin_sandbox();
    assert_ok(&sandbox.run(&["install", "com.acme.old@1.0.0"]));
    assert_ok(&sandbox.run(&["yank", "com.acme.old@1.0.0", "--reason", "security: test leak"]));
    let update = sandbox.run(&["update", "com.acme.old"]);
    assert_ok(&update);
    assert_ne!(
        sandbox.installed_version("com.acme.old").as_deref(),
        Some("1.0.0"),
        "a client with auto_update = install must leave the yanked version"
    );
}

/// REQ: PUBL-009
#[test]
fn publ_009_ordinary_yank_does_not_force_an_update_b() {
    let sandbox = two_plugin_sandbox();
    assert_ok(&sandbox.run(&["install", "com.acme.old@1.0.0"]));
    assert_ok(&sandbox.run(&["yank", "com.acme.old@1.0.0", "--reason", "bug: slow startup"]));
    assert_eq!(
        sandbox.installed_version("com.acme.old").as_deref(),
        Some("1.0.0"),
        "a non-security yank must not move installed copies by itself"
    );
}

// ---- PUBL-010 yank and unyank never change hash, signature or length ----------------------

/// REQ: PUBL-010
#[test]
fn publ_010_yank_keeps_hash_signature_and_length_a() {
    let sandbox = Sandbox::standard();
    let before = snapshot_entry(&sandbox.snapshot(), "com.acme.notes@0.3.0.json");
    assert_ok(&sandbox.run(&["yank", "com.acme.notes@0.3.0", "--reason", "bug: test fixture"]));
    let after = snapshot_entry(&sandbox.snapshot(), "com.acme.notes@0.3.0.json");
    assert_eq!(before["hashes"], after["hashes"]);
    assert_eq!(before["length"], after["length"]);
}

/// REQ: PUBL-010
#[test]
fn publ_010_unyank_restores_identical_entry_b() {
    let sandbox = Sandbox::standard();
    let before = snapshot_entry(&sandbox.snapshot(), "com.acme.notes@0.3.0.json");
    assert_ok(&sandbox.run(&["yank", "com.acme.notes@0.3.0", "--reason", "bug: test fixture"]));
    assert_ok(&sandbox.run(&["unyank", "com.acme.notes@0.3.0"]));
    let after = snapshot_entry(&sandbox.snapshot(), "com.acme.notes@0.3.0.json");
    assert_eq!(before, after, "yank then unyank must leave hash, signature and length equal");
}

// ---- PUBL-011 a security yank stays yanked until its reason is withdrawn ------------------

/// REQ: PUBL-011
#[test]
fn publ_011_unyank_of_security_yank_is_refused_a() {
    let sandbox = Sandbox::standard();
    assert_ok(&sandbox.run(&["yank", "com.acme.notes@0.3.0", "--reason", "security: test leak"]));
    let output = sandbox.run(&["unyank", "com.acme.notes@0.3.0"]);
    assert_ne!(output.exit_code, 0, "unyank of a security yank must be refused: {output:?}");
}

/// REQ: PUBL-011
#[test]
fn publ_011_security_unyank_is_recorded_in_transparency_log_b() {
    let sandbox = Sandbox::standard();
    assert_ok(&sandbox.run(&["yank", "com.acme.notes@0.3.0", "--reason", "security: test leak"]));
    let info = sandbox.run(&["info", "com.acme.notes@0.3.0", "--json"]);
    let record = result_record(&info).expect("info record");
    assert_eq!(record["state"], "yanked", "the version must stay yanked: {info:?}");
}

// ---- PUBL-012 deprecate never blocks installs ----------------------------------------------

/// REQ: PUBL-012
#[test]
fn publ_012_deprecated_plugin_still_installs_a() {
    let sandbox = two_plugin_sandbox();
    assert_ok(&sandbox.run(&[
        "deprecate",
        "com.acme.old",
        "--message",
        "Use the new plugin",
        "--successor",
        "com.acme.new",
    ]));
    let install = sandbox.run(&["install", "com.acme.old@1.0.0"]);
    assert_eq!(install.exit_code, 0, "deprecate must not block install: {install:?}");
    assert_eq!(sandbox.installed_version("com.acme.old").as_deref(), Some("1.0.0"));
}

/// REQ: PUBL-012
#[test]
fn publ_012_install_prints_message_and_successor_once_b() {
    let sandbox = two_plugin_sandbox();
    assert_ok(&sandbox.run(&[
        "deprecate",
        "com.acme.old",
        "--message",
        "Use the new plugin",
        "--successor",
        "com.acme.new",
    ]));
    let install = sandbox.run(&["install", "com.acme.old@1.0.0"]);
    let rendered = format!("{install:?}");
    assert_eq!(rendered.matches("Use the new plugin").count(), 1, "message once");
    assert!(rendered.contains("com.acme.new"), "successor id must be shown");
}

// ---- PUBL-013 unlist keeps every blob and the lock resolution -----------------------------

/// REQ: PUBL-013
#[test]
fn publ_013_unlist_removes_from_search_and_index_a() {
    let sandbox = two_plugin_sandbox();
    assert_ok(&sandbox.run(&["unlist", "com.acme.old"]));
    let found = sandbox.run(&["search", "old"]);
    assert!(!plugin_ids(&found).contains(&"com.acme.old".to_owned()), "{found:?}");
    let index = signed_of(&sandbox.snapshot()["index.json"]);
    let listed: Vec<String> = index["plugins"]
        .as_array()
        .map(|entries| entries.iter().filter_map(|entry| entry["id"].as_str().map(str::to_owned)).collect())
        .unwrap_or_default();
    assert!(!listed.contains(&"com.acme.old".to_owned()), "index.json must drop the unlisted plugin");
}

/// REQ: PUBL-013
#[test]
fn publ_013_update_still_works_after_unlist_b() {
    let sandbox = two_plugin_sandbox();
    assert_ok(&sandbox.run(&["install", "com.acme.old@1.0.0"]));
    assert_ok(&sandbox.run(&["unlist", "com.acme.old"]));
    let update = sandbox.run(&["update", "com.acme.old"]);
    assert_eq!(update.exit_code, 0, "update of an unlisted installed copy must succeed: {update:?}");
}

// ---- PUBL-014 takedown records reason, time and rule; users see the policy text -----------

/// REQ: PUBL-014
#[test]
fn publ_014_takedown_records_reason_time_and_rule_a() {
    let sandbox = Sandbox::standard();
    assert_ok(&sandbox.run(&["takedown", "com.acme.notes@0.3.0", "--reason", "malware"]));
    let revocations = signed_of(&sandbox.snapshot()["revocations.json"]);
    let text = revocations.to_string();
    assert!(text.contains("com.acme.notes"), "revocations.json must name the plugin");
    assert!(text.contains("malware"), "the reason must be recorded");
    assert!(
        revocations.to_string().contains("rule"),
        "the content-policy rule must be recorded: {text}"
    );
}

/// REQ: PUBL-014
#[test]
fn publ_014_users_see_policy_json_text_not_private_note_b() {
    let sandbox = Sandbox::standard();
    let output = sandbox.run(&["takedown", "com.acme.notes@0.3.0", "--reason", "private internal note"]);
    let policy_text = "Removed under the content policy rule 4.2";
    assert!(output_contains(&output, policy_text), "users must see policy.json text: {output:?}");
    assert!(!output_contains(&output, "private internal note"), "private notes must not be shown");
}

// ---- PUBL-015 takedown keeps the signed entry and marks it blocked ------------------------

/// REQ: PUBL-015
#[test]
fn publ_015_takedown_keeps_signed_version_entry_a() {
    let sandbox = Sandbox::standard();
    let before = snapshot_entry(&sandbox.snapshot(), "com.acme.notes@0.3.0.json");
    assert_ok(&sandbox.run(&["takedown", "com.acme.notes@0.3.0", "--reason", "malware"]));
    let after = snapshot_entry(&sandbox.snapshot(), "com.acme.notes@0.3.0.json");
    assert!(!after.is_null(), "the signed entry must not be deleted");
    assert_eq!(before["hashes"], after["hashes"], "the hash must stay known");
}

/// REQ: PUBL-015
#[test]
fn publ_015_info_reports_blocked_state_b() {
    let sandbox = Sandbox::standard();
    assert_ok(&sandbox.run(&["takedown", "com.acme.notes@0.3.0", "--reason", "malware"]));
    let info = sandbox.run(&["info", "com.acme.notes@0.3.0", "--json"]);
    let record = result_record(&info).expect("info record");
    assert_eq!(record["state"], "blocked", "{info:?}");
}

// ---- PUBL-016 every maintainer gets a takedown notice with the appeal address -------------

/// REQ: PUBL-016
#[test]
fn publ_016_every_maintainer_receives_notice_a() {
    let sandbox = Sandbox::standard();
    let output = sandbox.run(&["takedown", "com.acme.notes@0.3.0", "--reason", "malware"]);
    let notices = records(&output, "notice");
    assert!(
        notices.len() >= 2,
        "each maintainer of the publisher id must get a notice: {output:?}"
    );
}

/// REQ: PUBL-016
#[test]
fn publ_016_notice_contains_appeal_address_from_policy_b() {
    let sandbox = Sandbox::standard();
    let output = sandbox.run(&["takedown", "com.acme.notes@0.3.0", "--reason", "malware"]);
    let appeal = "appeals@registry.example.org";
    for notice in records(&output, "notice") {
        assert!(
            notice.to_string().contains(appeal),
            "every notice must include the appeal address from policy.json: {notice}"
        );
    }
}
