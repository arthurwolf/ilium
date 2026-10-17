//! Plugin policy page, part 2 (reference-plugin-policy.md): USR-216 to USR-220.

use crate::support::*;

const DENIED_AND_ALLOWED: &str =
    "version = 1\n[plugins]\nallow_ids = [\"com.example.t3\"]\ndeny_ids = [\"com.example.t3\"]\n";

fn first_message(output: &CliOutput) -> String {
    error_messages(output)
        .into_iter()
        .next()
        .unwrap_or_default()
}

fn four_failing_keys_policy() -> String {
    "version = 1\n\
     [packages]\nallow_third_party = false\nallowed_tiers = [\"T1\"]\nallowed_sources = [\"https://registry.example.org/\"]\n\
     [plugins]\ndeny_ids = [\"com.example.t3\"]\n\
     [publishers]\nallow = [\"someone-else\"]\n"
        .to_string()
}

// ---- USR-216 ----

/// REQ: USR-216
#[test]
fn usr_216_id_in_deny_and_allow_cannot_be_installed_with_policy_blocked() {
    let mut rig = UserRig::new();
    rig.write_system_policy(DENIED_AND_ALLOWED);
    rig.start_server();
    let output = rig.install_file(
        &spec("com.example.t3", "1.0.0", &[]),
        &["--allow-unsigned", "--yes"],
    );
    assert_eq!(error_codes(&output), vec!["policy.blocked".to_string()]);
    assert_ne!(result_exit_code(&output), 0);
}

/// REQ: USR-216
#[test]
fn usr_216_the_message_starts_with_policy_id_denied_colon_space() {
    let mut rig = UserRig::new();
    rig.write_system_policy(DENIED_AND_ALLOWED);
    rig.start_server();
    let output = rig.install_file(
        &spec("com.example.t3", "1.0.0", &[]),
        &["--allow-unsigned", "--yes"],
    );
    assert!(
        first_message(&output).starts_with("policy.id-denied: "),
        "{}",
        first_message(&output)
    );
}

/// REQ: USR-216
#[test]
fn usr_216_an_allow_only_entry_still_installs() {
    let mut rig = UserRig::new();
    rig.write_system_policy("version = 1\n[plugins]\nallow_ids = [\"com.example.t3\"]\n");
    rig.start_server();
    let output = rig.install_file(
        &spec("com.example.t3", "1.0.0", &[]),
        &["--allow-unsigned", "--yes"],
    );
    assert_eq!(result_exit_code(&output), 0, "{:?}", error_codes(&output));
}

/// REQ: USR-216
#[test]
fn usr_216_an_id_missing_from_the_allow_list_is_denied_with_the_same_detail() {
    let mut rig = UserRig::new();
    rig.write_system_policy("version = 1\n[plugins]\nallow_ids = [\"com.example.other\"]\n");
    rig.start_server();
    let output = rig.install_file(
        &spec("com.example.t3", "1.0.0", &[]),
        &["--allow-unsigned", "--yes"],
    );
    assert_eq!(error_codes(&output), vec!["policy.blocked".to_string()]);
    assert!(first_message(&output).starts_with("policy.id-denied: "));
}

// ---- USR-217 ----

/// REQ: USR-217
#[test]
fn usr_217_four_failing_keys_report_the_first_in_the_order() {
    let mut rig = UserRig::new();
    rig.write_system_policy(&four_failing_keys_policy());
    rig.start_server();
    let output = rig.install_file(
        &spec("com.example.t3", "1.0.0", &[]),
        &["--allow-unsigned", "--yes"],
    );
    assert_eq!(error_codes(&output), vec!["policy.blocked".to_string()]);
    assert!(
        first_message(&output).starts_with("policy.third-party-denied: "),
        "{}",
        first_message(&output)
    );
}

/// REQ: USR-217
#[test]
fn usr_217_without_the_first_key_the_second_key_is_reported() {
    let mut rig = UserRig::new();
    rig.write_system_policy(
        "version = 1\n[packages]\nallowed_tiers = [\"T1\"]\n[plugins]\ndeny_ids = [\"com.example.t3\"]\n",
    );
    rig.start_server();
    let output = rig.install_file(
        &spec("com.example.t3", "1.0.0", &[]),
        &["--allow-unsigned", "--yes"],
    );
    assert!(
        first_message(&output).starts_with("policy.id-denied: "),
        "{}",
        first_message(&output)
    );
}

/// REQ: USR-217
#[test]
fn usr_217_tier_check_comes_before_publisher_and_source_checks() {
    let mut rig = UserRig::new();
    rig.write_system_policy(
        "version = 1\n[packages]\nallowed_tiers = [\"T1\"]\nallowed_sources = [\"https://registry.example.org/\"]\n[publishers]\nallow = [\"someone-else\"]\n",
    );
    rig.start_server();
    let output = rig.install_file(
        &spec("com.example.t3", "1.0.0", &[]),
        &["--allow-unsigned", "--yes"],
    );
    assert!(
        first_message(&output).starts_with("policy.tier-denied: "),
        "{}",
        first_message(&output)
    );
}

/// REQ: USR-217
#[test]
fn usr_217_publisher_check_comes_before_the_source_check() {
    let mut rig = UserRig::new();
    rig.write_system_policy(
        "version = 1\n[packages]\nallowed_sources = [\"https://registry.example.org/\"]\n[publishers]\nallow = [\"someone-else\"]\n",
    );
    rig.start_server();
    let output = rig.install_file(
        &spec("com.example.t3", "1.0.0", &[]),
        &["--allow-unsigned", "--yes"],
    );
    assert!(
        first_message(&output).starts_with("policy.publisher-denied: "),
        "{}",
        first_message(&output)
    );
}

// ---- USR-218 ----

/// REQ: USR-218
#[test]
fn usr_218_all_thirty_core_plugins_start_under_the_strictest_package_policy() {
    let mut rig = UserRig::new();
    rig.write_system_policy(
        "version = 1\n[packages]\nallow_third_party = false\nallowed_tiers = []\n",
    );
    rig.start_server();
    let ids = core_plugin_ids_from_manual();
    assert_eq!(ids.len(), 30, "{ids:?}");
    for id in ids {
        let row = list_row(&mut rig, &id);
        assert!(
            !row.state.starts_with("Blocked") && !row.state.starts_with("Refused"),
            "{id} is {} ({:?})",
            row.state,
            row.reason
        );
    }
}

/// REQ: USR-218
#[test]
fn usr_218_the_same_policy_blocks_an_installed_third_party_plugin() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.t3", "1.0.0", &[]));
    rig.write_system_policy(
        "version = 1\n[packages]\nallow_third_party = false\nallowed_tiers = []\n",
    );
    rig.start_server();
    assert_eq!(
        list_row(&mut rig, "com.example.t3").state,
        "Blocked by policy"
    );
    assert_eq!(list_row(&mut rig, "ilium.core.tree").state, "On");
}

/// REQ: USR-218
#[test]
fn usr_218_core_plugins_ignore_a_plugin_deny_list_naming_them() {
    let mut rig = UserRig::new();
    rig.write_system_policy(
        "version = 1\n[plugins]\ndeny_ids = [\"ilium.core.tree\"]\n[publishers]\nallow = [\"x\"]\n",
    );
    rig.start_server();
    assert_eq!(list_row(&mut rig, "ilium.core.tree").state, "On");
}

// ---- USR-219 ----

/// REQ: USR-219
#[test]
fn usr_219_start_with_a_denying_policy_writes_one_loaded_and_one_violation_record() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.t3", "1.0.0", &[]));
    rig.write_system_policy("version = 1\n[plugins]\ndeny_ids = [\"com.example.t3\"]\n");
    rig.start_server();
    assert_eq!(rig.audit_records_of("policy-loaded").len(), 1);
    assert_eq!(rig.audit_records_of("policy-violation").len(), 1);
}

/// REQ: USR-219
#[test]
fn usr_219_no_policy_file_writes_no_policy_loaded_record() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.t3", "1.0.0", &[]));
    rig.start_server();
    assert!(rig.audit_records_of("policy-loaded").is_empty());
    assert!(rig.audit_records_of("policy-violation").is_empty());
}

/// REQ: USR-219
#[test]
fn usr_219_a_policy_that_nothing_violates_writes_only_the_loaded_record() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.t3", "1.0.0", &[]));
    rig.write_system_policy("version = 1\n[plugins]\ndeny_ids = [\"com.example.not-installed\"]\n");
    rig.start_server();
    assert_eq!(rig.audit_records_of("policy-loaded").len(), 1);
    assert!(rig.audit_records_of("policy-violation").is_empty());
}

/// REQ: USR-219
#[test]
fn usr_219_the_records_contain_no_plugin_data() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.t3", "1.0.0", &[]));
    rig.set_plugin_setting("com.example.t3", "note", "secret-marker-8f3a");
    rig.write_system_policy("version = 1\n[plugins]\ndeny_ids = [\"com.example.t3\"]\n");
    rig.start_server();
    for event in ["policy-loaded", "policy-violation"] {
        for record in rig.audit_records_of(event) {
            let text = record.to_string();
            assert!(!text.contains("secret-marker-8f3a"), "{event}: {text}");
            assert!(
                record.get("data").is_none() && record.get("settings").is_none(),
                "{event}: {text}"
            );
        }
    }
}

// ---- USR-220 ----

/// REQ: USR-220
#[test]
fn usr_220_a_rotated_file_aged_366_days_is_deleted_at_restart() {
    let mut rig = UserRig::new();
    rig.start_server();
    let rotated = rig.audit_dir().join("audit-1.jsonl");
    std::fs::create_dir_all(rotated.parent().expect("audit dir")).expect("create audit dir");
    std::fs::write(&rotated, b"{}\n").expect("write rotated");
    rig.age_file(&rotated, 366);
    rig.restart_server();
    assert!(!rotated.exists());
}

/// REQ: USR-220
#[test]
fn usr_220_a_rotated_file_aged_364_days_is_kept() {
    let mut rig = UserRig::new();
    rig.start_server();
    let rotated = rig.audit_dir().join("audit-1.jsonl");
    std::fs::create_dir_all(rotated.parent().expect("audit dir")).expect("create audit dir");
    std::fs::write(&rotated, b"{}\n").expect("write rotated");
    rig.age_file(&rotated, 364);
    rig.restart_server();
    assert!(rotated.exists());
}

/// REQ: USR-220
#[test]
fn usr_220_the_policy_keep_days_value_moves_the_cutoff() {
    let mut rig = UserRig::new();
    rig.write_system_policy("version = 1\n[audit]\nkeep_days = 30\n");
    rig.start_server();
    let old = rig.audit_dir().join("audit-1.jsonl");
    let recent = rig.audit_dir().join("audit-2.jsonl");
    std::fs::create_dir_all(old.parent().expect("audit dir")).expect("create audit dir");
    std::fs::write(&old, b"{}\n").expect("write old");
    std::fs::write(&recent, b"{}\n").expect("write recent");
    rig.age_file(&old, 31);
    rig.age_file(&recent, 29);
    rig.restart_server();
    assert!(!old.exists());
    assert!(recent.exists());
}

/// REQ: USR-220
#[test]
fn usr_220_the_live_audit_file_is_never_deleted_by_age() {
    let mut rig = UserRig::new();
    rig.start_server();
    let live = rig.audit_dir().join("audit.jsonl");
    rig.age_file(&live, 400);
    rig.restart_server();
    assert!(live.exists());
}
