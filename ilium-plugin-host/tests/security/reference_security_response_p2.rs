//! reference-security-response.md, part 2: plugin vulnerability reports (SEC-325, 326), takedown
//! (SEC-327), safe mode (SEC-328 .. 330) and dependency advisories (SEC-331).

use ilium_plugin_api::Tier;
use ilium_plugin_host::lifecycle::{resolve_safe_mode, SafeMode};
use serde_json::{json, Value};

use crate::support::*;

fn blocklist_entry(kind: &str, value: &str) -> Value {
    json!({ "kind": kind, "value": value, "versions": ">=0.0.0", "reason": "No answer to a serious report" })
}

fn blocked_rig(kind: &str, value: &str) -> Rig {
    setup(&[]).option("blocklist", json!([blocklist_entry(kind, value)])).start()
}

fn flags(words: &[&str]) -> Vec<String> {
    args(words)
}

// ---- SEC-325 ----

/// REQ: SEC-325 WARN
#[test]
fn sec_325_a_manifest_with_security_contact_has_no_error() {
    let manifest = server_manifest(json!({ "security_contact": "security@example.com" }));
    assert!(manifest_error_codes(&manifest).is_empty(), "{:?}", manifest_error_codes(&manifest));
}

/// REQ: SEC-325 WARN
#[test]
fn sec_325_a_manifest_without_security_contact_is_still_accepted() {
    assert!(manifest_error_codes(&server_manifest(json!({}))).is_empty());
}

/// REQ: SEC-325 WARN
#[test]
fn sec_325_the_details_view_shows_the_security_contact() {
    let rig = t3(&[]);
    let text = rig.host.details_text_with_contact("security@example.com");
    assert!(text.contains("security@example.com"), "{text}");
}

/// REQ: SEC-325 WARN
#[test]
fn sec_325_the_manual_sends_reports_about_a_plugin_to_its_contact_not_to_the_project() {
    assert_page_has(
        "reference-security-response.md",
        &["The Details view of the install dialog and `ilium plugin inspect` show it.", "unless the flaw is in the host"],
    );
}

// ---- SEC-326 ----

/// REQ: SEC-326 MAY
#[test]
fn sec_326_a_blocklist_entry_for_the_package_disables_the_plugin() {
    let hash = setup(&[]).start().host.package_sha256();
    let rig = blocked_rig("package", &hash);
    assert!(rig.host.plugin_disabled("test.plugin"));
}

/// REQ: SEC-326 MAY
#[test]
fn sec_326_a_blocklist_entry_for_the_plugin_id_with_a_version_range_disables_the_plugin() {
    let rig = blocked_rig("id", "test.plugin");
    assert!(rig.host.plugin_disabled("test.plugin"));
}

/// REQ: SEC-326 MAY
#[test]
fn sec_326_a_blocklist_entry_for_the_publisher_key_disables_the_plugin() {
    let fingerprint = setup(&[]).start().host.publisher_fingerprint();
    let rig = blocked_rig("publisher-key", &fingerprint);
    assert!(rig.host.plugin_disabled("test.plugin"));
}

/// REQ: SEC-326 MAY
#[test]
fn sec_326_without_a_blocklist_entry_the_plugin_stays_enabled() {
    let rig = setup(&[]).start();
    assert!(!rig.host.plugin_disabled("test.plugin"));
}

/// REQ: SEC-326 MAY
#[test]
fn sec_326_an_entry_for_another_plugin_id_does_not_disable_this_plugin() {
    let rig = blocked_rig("id", "other.plugin");
    assert!(!rig.host.plugin_disabled("test.plugin"));
}

// ---- SEC-327 ----

/// REQ: SEC-327
#[test]
fn sec_327_a_matching_blocklist_entry_disables_the_plugin_at_the_next_load() {
    let mut rig = setup(&[]).start();
    assert!(!rig.host.plugin_disabled("test.plugin"));
    rig.setup.options.push(("blocklist".to_owned(), json!([blocklist_entry("id", "test.plugin")])));
    let rig = rig.restart();
    assert!(rig.host.plugin_disabled("test.plugin"));
}

/// REQ: SEC-327
#[test]
fn sec_327_the_user_sees_blocked_followed_by_the_reason() {
    let rig = blocked_rig("id", "test.plugin");
    assert!(rig.notices().iter().any(|notice| notice == "Blocked: No answer to a serious report"), "{:?}", rig.notices());
}

/// REQ: SEC-327
#[test]
fn sec_327_the_audit_log_records_one_blocklist_hit() {
    let rig = blocked_rig("id", "test.plugin");
    assert_eq!(rig.audit_events("blocklist-hit").len(), 1);
}

/// REQ: SEC-327
#[test]
fn sec_327_the_plugin_data_is_kept() {
    let rig = setup(&[]).start();
    rig.host.write_plugin_data("notes", b"kept");
    rig.host.persist_all_stores();
    let mut rig = rig;
    rig.setup.options.push(("blocklist".to_owned(), json!([blocklist_entry("id", "test.plugin")])));
    let rig = rig.restart();
    assert_eq!(rig.host.read_plugin_data("notes"), Some(b"kept".to_vec()));
}

/// REQ: SEC-327
#[test]
fn sec_327_the_user_can_uninstall_a_blocked_plugin() {
    let rig = blocked_rig("id", "test.plugin");
    assert_eq!(rig.host.uninstall("test.plugin"), Ok(()));
    assert!(!rig.host.is_installed("test.plugin"));
}

/// REQ: SEC-327
#[test]
fn sec_327_the_user_cannot_re_enable_a_blocked_plugin() {
    let rig = blocked_rig("id", "test.plugin");
    assert!(rig.host.enable("test.plugin").is_err());
    assert!(rig.host.plugin_disabled("test.plugin"));
}

// ---- SEC-328 ----

/// REQ: SEC-328
#[test]
fn sec_328_safe_mode_without_a_value_is_third_party() {
    assert_eq!(resolve_safe_mode(&flags(&["--safe-mode"]), None), Ok(SafeMode::ThirdParty));
}

/// REQ: SEC-328
#[test]
fn sec_328_safe_mode_flag_accepts_third_party_and_all_optional() {
    assert_eq!(resolve_safe_mode(&flags(&["--safe-mode=third-party"]), None), Ok(SafeMode::ThirdParty));
    assert_eq!(resolve_safe_mode(&flags(&["--safe-mode=all-optional"]), None), Ok(SafeMode::AllOptional));
}

/// REQ: SEC-328
#[test]
fn sec_328_the_environment_variable_accepts_its_four_values() {
    assert_eq!(resolve_safe_mode(&[], Some("third-party")), Ok(SafeMode::ThirdParty));
    assert_eq!(resolve_safe_mode(&[], Some("all-optional")), Ok(SafeMode::AllOptional));
    assert_eq!(resolve_safe_mode(&[], Some("1")), Ok(SafeMode::ThirdParty));
    assert_eq!(resolve_safe_mode(&[], Some("0")), Ok(SafeMode::Off));
}

/// REQ: SEC-328
#[test]
fn sec_328_any_other_environment_value_exits_2_with_usage_invalid_value() {
    for value in ["yes", "2", "true", "Third-Party", "all"] {
        let error = resolve_safe_mode(&[], Some(value)).expect_err(value);
        assert_eq!(error.exit_code, 2, "{value}");
        assert_eq!(error.code, "usage.invalid-value", "{value}");
    }
}

/// REQ: SEC-328
#[test]
fn sec_328_safe_plugins_is_an_alias_of_safe_mode_third_party() {
    assert_eq!(resolve_safe_mode(&flags(&["--safe-plugins"]), None), resolve_safe_mode(&flags(&["--safe-mode=third-party"]), None));
}

/// REQ: SEC-328
#[test]
fn sec_328_no_plugins_starts_no_plugin_host() {
    assert_eq!(resolve_safe_mode(&flags(&["--no-plugins"]), None), Ok(SafeMode::NoPlugins));
}

/// REQ: SEC-328
#[test]
fn sec_328_the_flag_wins_over_the_variable() {
    assert_eq!(resolve_safe_mode(&flags(&["--safe-mode=all-optional"]), Some("0")), Ok(SafeMode::AllOptional));
    assert_eq!(resolve_safe_mode(&flags(&["--safe-mode"]), Some("0")), Ok(SafeMode::ThirdParty));
}

/// REQ: SEC-328
#[test]
fn sec_328_no_flag_and_no_variable_is_off() {
    assert_eq!(resolve_safe_mode(&[], None), Ok(SafeMode::Off));
}

/// REQ: SEC-328
#[test]
fn sec_328_all_t2_and_t3_plugins_are_disabled() {
    for tier in [Tier::T2, Tier::T3] {
        let rig = setup(&[]).tier(tier).safe_mode().start();
        assert!(rig.host.plugin_disabled("test.plugin"), "{tier:?}");
    }
}

/// REQ: SEC-328
#[test]
fn sec_328_a_bundled_t1_plugin_runs_normally() {
    let rig = setup(&[]).tier(Tier::T1).safe_mode().start();
    assert!(!rig.host.plugin_disabled("test.plugin"));
}

/// REQ: SEC-328
#[test]
fn sec_328_core_plugin_replacements_are_off() {
    let rig = setup(&[]).replacement_inference_plugin().safe_mode().start();
    assert!(!rig.host.replacement_active("test.plugin"));
    let normal = setup(&[]).replacement_inference_plugin().start();
    assert!(normal.host.replacement_active("test.plugin"));
}

/// REQ: SEC-328
#[test]
fn sec_328_the_admin_policy_still_applies() {
    let rig = setup(&[]).system_policy("version = 1\n[plugins]\nblock = [\"test.plugin\"]\n", true).safe_mode().start();
    assert!(!rig.policy_show().is_empty());
}

/// REQ: SEC-328
#[test]
fn sec_328_no_plugin_is_deleted_and_no_data_changes() {
    let rig = setup(&["clipboard.read"]).start();
    rig.host.write_plugin_data("notes", b"data");
    rig.host.persist_all_stores();
    let mut rig = rig;
    rig.setup.options.push(("safe-mode".to_owned(), json!(true)));
    let rig = rig.restart();
    assert!(rig.host.is_installed("test.plugin"));
    assert_eq!(rig.host.read_plugin_data("notes"), Some(b"data".to_vec()));
}

/// REQ: SEC-328
#[test]
fn sec_328_the_flag_is_not_stored_and_the_next_normal_start_returns_to_the_previous_state() {
    let rig = setup(&[]).safe_mode().start();
    assert!(rig.host.plugin_disabled("test.plugin"));
    let mut rig = rig;
    rig.setup.options.retain(|(key, _)| key != "safe-mode");
    let rig = rig.restart();
    assert!(!rig.host.plugin_disabled("test.plugin"));
}

// ---- SEC-329 ----

/// REQ: SEC-329
#[test]
fn sec_329_the_status_bar_shows_the_safe_badge_while_safe_mode_is_on() {
    let rig = setup(&[]).safe_mode().start();
    assert!(rig.host.status_bar_text().contains("SAFE"));
}

/// REQ: SEC-329
#[test]
fn sec_329_the_badge_is_absent_without_safe_mode() {
    let rig = setup(&[]).start();
    assert!(!rig.host.status_bar_text().contains("SAFE"));
}

/// REQ: SEC-329
#[test]
fn sec_329_the_badge_persists_over_time() {
    let mut rig = setup(&[]).safe_mode().start();
    rig.host.advance_clock(std::time::Duration::from_secs(3600));
    assert!(rig.host.status_bar_text().contains("SAFE"));
}

/// REQ: SEC-329
#[test]
fn sec_329_the_plugins_screen_lists_disabled_plugins_as_off_in_safe_mode() {
    let rig = setup(&[]).safe_mode().start();
    let text = rig.host.plugins_screen_text();
    assert!(text.contains("test.plugin") && text.contains("Off in safe mode"), "{text}");
}

/// REQ: SEC-329
#[test]
fn sec_329_the_user_can_uninstall_a_plugin_from_the_safe_mode_screen() {
    let rig = setup(&[]).safe_mode().start();
    assert_eq!(rig.host.uninstall("test.plugin"), Ok(()));
    assert!(!rig.host.is_installed("test.plugin"));
}

// ---- SEC-330 ----

/// REQ: SEC-330
#[test]
fn sec_330_the_server_writes_safe_mode_to_the_audit_log_at_start() {
    let rig = setup(&[]).safe_mode().start();
    assert_eq!(rig.audit_events("safe-mode").len(), 1);
}

/// REQ: SEC-330
#[test]
fn sec_330_a_normal_start_writes_no_safe_mode_record() {
    let rig = setup(&[]).start();
    assert!(rig.audit_events("safe-mode").is_empty());
}

/// REQ: SEC-330
#[test]
fn sec_330_a_project_with_plugin_values_is_untrusted_in_safe_mode() {
    let rig = setup(&[]).safe_mode().start();
    rig.host.trust_project("/work/project");
    rig.host.write_project_value("/work/project", "test.plugin", "mode", json!("project"));
    assert_ne!(rig.host.effective_setting("mode"), Some(json!("project")));
}

/// REQ: SEC-330
#[test]
fn sec_330_the_same_project_is_trusted_without_safe_mode() {
    let rig = setup(&[]).start();
    rig.host.trust_project("/work/project");
    rig.host.write_project_value("/work/project", "test.plugin", "mode", json!("project"));
    assert_eq!(rig.host.effective_setting("mode"), Some(json!("project")));
}

// ---- SEC-331 ----

/// REQ: SEC-331
#[test]
fn sec_331_a_runtime_advisory_refuses_third_party_plugins_when_the_policy_blocks_vulnerable_runtimes() {
    let rig = setup(&[])
        .system_policy("version = 1\n[packages]\nblock_vulnerable_runtime = true\n", true)
        .option("runtime-advisory", json!("GHSA-xxxx-yyyy-zzzz"))
        .start();
    let message = rig.host.load_error().unwrap_or_default();
    assert!(message.contains("sandbox.unavailable"), "{message}");
    assert!(message.contains("runtime-vulnerable"), "{message}");
}

/// REQ: SEC-331
#[test]
fn sec_331_by_default_the_host_warns_and_the_plugin_loads() {
    let rig = setup(&[]).option("runtime-advisory", json!("GHSA-xxxx-yyyy-zzzz")).start();
    assert!(rig.host.load_error().is_none());
    assert!(rig.notices().iter().any(|notice| notice.contains("GHSA-xxxx-yyyy-zzzz")), "{:?}", rig.notices());
}

/// REQ: SEC-331
#[test]
fn sec_331_the_manual_states_the_patch_deadlines() {
    assert_page_has("reference-security-response.md", &["within 14 days of the upstream fix (48 hours when exploited in the wild)", "a release-blocklist entry for the vulnerable runtime"]);
}
