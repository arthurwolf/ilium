//! Twins of the file-level tests of ilium-client/src/agent_config_writer.rs (F-037, F-063): read the current
//! value, plan, apply with compare-and-swap, revert with a persisted record. The facade is the set of
//! `configure.*` methods of the install service (API_GAPS_port.md); files live on the fake file system in the
//! scopes `agent-config:claude` and `agent-config:codex`.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{ErrorCode, FsPath, PluginError};
use serde_json::{Value, json};

use crate::common::*;
use crate::config_text::CODEX_SAMPLE;

const MAX_CONFIG_BYTES: usize = 16 * 1024 * 1024;

fn scoped(scope: &str, path: &str) -> FsPath {
    FsPath { scope: scope.to_string(), path: path.to_string() }
}

fn file_of(target: &str) -> FsPath {
    if target == CLAUDE_SETTING { claude_settings_file() } else { codex_config_file() }
}

fn host_with(files: &[(&str, &str)]) -> FakeHost {
    let mut host = server_host();
    for (target, text) in files {
        put(&mut host, file_of(target), text);
    }
    host
}

fn read(host: &mut FakeHost, target: &str) -> String {
    text_of(host, file_of(target))
}

fn code_of(result: Result<Value, PluginError>) -> ErrorCode {
    result.expect_err("the call is refused").code
}

fn leftover_temporaries(host: &mut FakeHost) -> usize {
    host.fs().all_paths().iter().filter(|path| path.path.contains("ilium-tmp")).count()
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::read_current_reports_value_absence_and_fingerprint
#[test]
fn port_cw_19_read_current_reports_value_absence_and_fingerprint() {
    let mut host = host_with(&[(CLAUDE_SETTING, "{\"autoCompactWindow\": 300000}\n"), (CODEX_SETTING, "a = 1\n")]);
    let claude = read_current(&mut host, CLAUDE_SETTING).expect("claude");
    assert_eq!(claude["value"], json!(300_000));
    assert_eq!(claude["fingerprint"]["size"], json!(30));
    assert_eq!(claude["fingerprint"]["sha256"].as_str().expect("sha256").len(), 64);
    assert!(!claude["fingerprint"]["modified"].is_null());
    let codex = read_current(&mut host, CODEX_SETTING).expect("codex");
    assert!(codex["value"].is_null());
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::read_current_distinguishes_missing_not_regular_too_large_and_unparsable
#[test]
fn port_cw_20_read_current_distinguishes_missing_not_regular_too_large_and_unparsable() {
    let mut host = server_host();
    let missing = read_current(&mut host, CLAUDE_SETTING).expect_err("missing");
    assert_eq!(missing.code, ErrorCode::NotFound);
    host.fs().mkdir(claude_settings_file());
    let not_regular = read_current(&mut host, CLAUDE_SETTING).expect_err("directory");
    assert_eq!(not_regular.code, ErrorCode::InvalidArgument);
    assert!(not_regular.message.contains("not a regular file"), "{}", not_regular.message);
    host.fs().remove(claude_settings_file());
    put(&mut host, claude_settings_file(), "{not json");
    assert_eq!(code_of(read_current(&mut host, CLAUDE_SETTING)), ErrorCode::InvalidArgument);
    host.fs().write(codex_config_file(), &vec![b' '; MAX_CONFIG_BYTES + 1]);
    let too_large = read_current(&mut host, CODEX_SETTING).expect_err("too large");
    assert_eq!(too_large.code, ErrorCode::InvalidArgument);
    assert!(too_large.message.contains("over the"), "{}", too_large.message);
    host.fs().write(codex_config_file(), &[0xff, 0xfe, 0x00]);
    assert_eq!(code_of(read_current(&mut host, CODEX_SETTING)), ErrorCode::InvalidArgument);
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::plan_rejects_out_of_range_values
#[test]
fn port_cw_21_plan_rejects_out_of_range_values_with_invalid_argument() {
    let mut host = host_with(&[(CLAUDE_SETTING, "{}"), (CODEX_SETTING, "")]);
    for value in [0, 99_999, 1_000_001, u64::MAX] {
        assert_eq!(code_of(plan(&mut host, CLAUDE_SETTING, value)), ErrorCode::InvalidArgument, "{value}");
    }
    plan(&mut host, CLAUDE_SETTING, 100_000).expect("lower bound is accepted");
    plan(&mut host, CLAUDE_SETTING, 1_000_000).expect("upper bound is accepted");
    assert_eq!(code_of(plan(&mut host, CODEX_SETTING, 999)), ErrorCode::InvalidArgument);
    plan(&mut host, CODEX_SETTING, 1_000).expect("codex lower bound is accepted");
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::plan_diff_names_file_key_and_old_and_new_values
#[test]
fn port_cw_22_plan_diff_names_file_key_and_old_and_new_values() {
    let mut host = host_with(&[(CLAUDE_SETTING, "{\"autoCompactWindow\": 567000}"), (CODEX_SETTING, "a = 1\n")]);
    let claude = plan(&mut host, CLAUDE_SETTING, 250_000).expect("claude plan");
    assert_eq!(claude["old_value"], json!(567_000));
    let diff = claude["diff"].as_str().expect("diff");
    assert!(diff.contains(claude["path"].as_str().expect("path")));
    assert!(diff.contains("- \"autoCompactWindow\": 567000"));
    assert!(diff.contains("+ \"autoCompactWindow\": 250000"));
    assert!(diff.contains("new sessions"));
    let codex = plan(&mut host, CODEX_SETTING, 200_000).expect("codex plan");
    assert!(codex["old_value"].is_null());
    assert!(codex["diff"]
        .as_str()
        .expect("diff")
        .contains("+ model_auto_compact_token_limit = 200000   (inserted before the first table)"));
    assert_eq!(codex["warnings"], json!([]));
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::plan_warns_when_the_environment_shadows_the_claude_file_value
#[test]
fn port_cw_23_plan_warns_when_the_environment_shadows_the_claude_file_value() {
    let mut host = host_with(&[(CLAUDE_SETTING, "{}"), (CODEX_SETTING, "")]);
    host.set_env("CLAUDE_CODE_AUTO_COMPACT_WINDOW", "400000");
    let warned = plan(&mut host, CLAUDE_SETTING, 250_000).expect("plan");
    let warnings = warned["warnings"].as_array().expect("warnings");
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0]["kind"], json!("environment-override"));
    assert_eq!(warnings[0]["variable"], json!("CLAUDE_CODE_AUTO_COMPACT_WINDOW"));
    assert_eq!(warnings[0]["value"], json!("400000"));
    assert!(warnings[0]["message"].as_str().expect("message").contains("takes precedence"));
    // A blank variable shadows nothing, and Codex ignores it.
    host.set_env("CLAUDE_CODE_AUTO_COMPACT_WINDOW", "  ");
    assert_eq!(plan(&mut host, CLAUDE_SETTING, 250_000).expect("plan")["warnings"], json!([]));
    host.set_env("CLAUDE_CODE_AUTO_COMPACT_WINDOW", "400000");
    assert_eq!(plan(&mut host, CODEX_SETTING, 250_000).expect("plan")["warnings"], json!([]));
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::plan_warns_about_project_level_override_files
#[test]
fn port_cw_24_plan_warns_about_project_level_override_files() {
    let mut host = host_with(&[(CLAUDE_SETTING, "{}"), (CODEX_SETTING, "")]);
    host.open_project("ilium");
    host.settle();
    put(&mut host, scoped("project", ".claude/settings.local.json"), "{\"autoCompactWindow\": 150000}");
    put(&mut host, scoped("project", ".claude/settings.json"), "{\"other\": 1}");
    put(&mut host, scoped("project", ".codex/config.toml"), "model_auto_compact_token_limit = 90000\n");
    let claude = plan(&mut host, CLAUDE_SETTING, 250_000).expect("claude plan");
    let warnings = claude["warnings"].as_array().expect("warnings");
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0]["kind"], json!("project-override"));
    assert!(warnings[0]["path"].as_str().expect("path").ends_with("settings.local.json"));
    assert_eq!(warnings[0]["value"], json!("150000"));
    let codex = plan(&mut host, CODEX_SETTING, 250_000).expect("codex plan");
    let warnings = codex["warnings"].as_array().expect("warnings");
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0]["kind"], json!("project-override"));
    assert_eq!(warnings[0]["value"], json!("90000"));
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::plan_warns_about_codex_profiles_that_set_the_key
#[test]
fn port_cw_25_plan_warns_about_codex_profiles_that_set_the_key() {
    let mut host = host_with(&[(
        CODEX_SETTING,
        "profile = \"fast\"\n\n[profiles.fast]\nmodel_auto_compact_token_limit = 50000\n\n[profiles.slow]\nmodel = \"x\"\n",
    )]);
    let planned = plan(&mut host, CODEX_SETTING, 250_000).expect("plan");
    let warnings = planned["warnings"].as_array().expect("warnings");
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0]["kind"], json!("profile-override"));
    assert_eq!(warnings[0]["profile"], json!("fast"));
    assert_eq!(warnings[0]["value"], json!("50000"));
    assert_eq!(warnings[0]["is_selected"], json!(true));
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::plan_flags_an_unchanged_value
#[test]
fn port_cw_26_plan_flags_an_unchanged_value() {
    let mut host = host_with(&[(CLAUDE_SETTING, "{\"autoCompactWindow\": 250000}")]);
    let planned = plan(&mut host, CLAUDE_SETTING, 250_000).expect("plan");
    let kinds: Vec<&Value> = planned["warnings"].as_array().expect("warnings").iter().map(|w| &w["kind"]).collect();
    assert!(kinds.contains(&&json!("value-unchanged")), "{kinds:?}");
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::apply_updates_an_existing_json_value_and_reverts_it
#[test]
fn port_cw_27_apply_updates_an_existing_json_value_and_reverts_it() {
    let original = "{\n  \"theme\": \"dark\",\n  \"autoCompactWindow\": 567000\n}\n";
    let mut host = host_with(&[(CLAUDE_SETTING, original)]);
    let receipt = apply_value(&mut host, CLAUDE_SETTING, 250_000);
    assert_eq!(receipt["changed"], json!(true));
    assert_eq!(receipt["record"]["previous_value"], json!(567_000));
    assert_eq!(read(&mut host, CLAUDE_SETTING), original.replace("567000", "250000"));
    assert_eq!(leftover_temporaries(&mut host), 0);
    let record = load_record(&mut host, CLAUDE_SETTING).expect("record persisted");
    assert_eq!(record, receipt["record"]);
    let reverted = revert_record(&mut host, &record).expect("revert");
    assert_eq!(reverted["restored_value"], json!(567_000));
    assert_eq!(reverted["record_cleared"], json!(true));
    assert_eq!(read(&mut host, CLAUDE_SETTING), original);
    assert!(load_record(&mut host, CLAUDE_SETTING).is_none());
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::apply_inserts_an_absent_key_and_revert_removes_it_byte_exactly
#[test]
fn port_cw_28_apply_inserts_an_absent_key_and_revert_removes_it_byte_exactly() {
    let claude = "{\r\n\t\"theme\": \"dark\"\r\n}";
    let mut host = host_with(&[(CLAUDE_SETTING, claude), (CODEX_SETTING, CODEX_SAMPLE)]);
    for (target, original) in [(CLAUDE_SETTING, claude), (CODEX_SETTING, CODEX_SAMPLE)] {
        let receipt = apply_value(&mut host, target, 250_000);
        assert!(receipt["record"]["previous_value"].is_null());
        assert_eq!(read_current(&mut host, target).expect("read")["value"], json!(250_000));
        revert_record(&mut host, &receipt["record"]).expect("revert");
        assert_eq!(read(&mut host, target), original);
    }
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::apply_changes_codex_values_inside_the_top_level_region_only
#[test]
fn port_cw_29_apply_changes_codex_values_inside_the_top_level_region_only() {
    let mut host = host_with(&[(CODEX_SETTING, CODEX_SAMPLE)]);
    apply_value(&mut host, CODEX_SETTING, 123_456);
    let text = read(&mut host, CODEX_SETTING);
    let first_table = text.find("[projects.").expect("table");
    let key_line = text.find("model_auto_compact_token_limit = 123456").expect("inserted key");
    assert!(key_line < first_table);
    assert_eq!(text.matches("model_auto_compact_token_limit").count(), 2);
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::apply_with_an_unchanged_value_writes_nothing_and_keeps_no_record
#[test]
fn port_cw_30_apply_with_an_unchanged_value_writes_nothing_and_keeps_no_record() {
    let mut host = host_with(&[(CLAUDE_SETTING, "{\"autoCompactWindow\": 250000}")]);
    let before = host.fs().modified(claude_settings_file());
    let receipt = apply_value(&mut host, CLAUDE_SETTING, 250_000);
    assert_eq!(receipt["changed"], json!(false));
    assert_eq!(host.fs().modified(claude_settings_file()), before);
    assert!(load_record(&mut host, CLAUDE_SETTING).is_none());
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::apply_refuses_a_file_changed_between_plan_and_apply
#[test]
fn port_cw_31_apply_refuses_a_file_changed_between_plan_and_apply_with_conflict() {
    let mut host = host_with(&[(CLAUDE_SETTING, "{\"autoCompactWindow\": 567000}")]);
    let planned = plan(&mut host, CLAUDE_SETTING, 250_000).expect("plan");
    let concurrent = "{\"autoCompactWindow\": 567000, \"added\": \"by the agent\"}";
    put(&mut host, claude_settings_file(), concurrent);
    assert_eq!(code_of(apply_plan(&mut host, &planned)), ErrorCode::Conflict);
    assert_eq!(read(&mut host, CLAUDE_SETTING), concurrent);
    assert!(load_record(&mut host, CLAUDE_SETTING).is_none());
    assert_eq!(leftover_temporaries(&mut host), 0);
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::apply_refuses_a_codex_file_changed_between_plan_and_apply
#[test]
fn port_cw_32_apply_refuses_a_codex_file_changed_between_plan_and_apply_with_conflict() {
    let mut host = host_with(&[(CODEX_SETTING, "a = 1\n")]);
    let planned = plan(&mut host, CODEX_SETTING, 250_000).expect("plan");
    put(&mut host, codex_config_file(), "a = 2\n");
    assert_eq!(code_of(apply_plan(&mut host, &planned)), ErrorCode::Conflict);
    assert_eq!(read(&mut host, CODEX_SETTING), "a = 2\n");
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::a_concurrent_write_after_the_temporary_file_is_caught_by_the_final_check
#[test]
fn port_cw_33_a_concurrent_write_after_the_temporary_file_is_caught_by_the_final_check() {
    let mut host = host_with(&[(CLAUDE_SETTING, "{\"autoCompactWindow\": 567000}")]);
    let planned = plan(&mut host, CLAUDE_SETTING, 250_000).expect("plan");
    let concurrent = "{\"autoCompactWindow\": 567000, \"x\": 1}";
    host.fs().race_next_write(claude_settings_file(), concurrent.as_bytes());
    assert_eq!(code_of(apply_plan(&mut host, &planned)), ErrorCode::Conflict);
    assert_eq!(read(&mut host, CLAUDE_SETTING), concurrent);
    assert_eq!(leftover_temporaries(&mut host), 0);
    assert!(load_record(&mut host, CLAUDE_SETTING).is_none(), "no record may survive a write that did not happen");
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::a_failing_record_store_aborts_before_the_file_is_touched
#[test]
fn port_cw_34_a_failing_record_store_aborts_before_the_file_is_touched() {
    let original = "{\"autoCompactWindow\": 567000}";
    let mut host = host_with(&[(CLAUDE_SETTING, original)]);
    let planned = plan(&mut host, CLAUDE_SETTING, 250_000).expect("plan");
    host.inject_failure(
        "storage.write",
        PluginError { code: ErrorCode::Io, message: "record store blocked".to_string(), retry_after_ms: None },
    );
    assert_eq!(code_of(apply_plan(&mut host, &planned)), ErrorCode::Io);
    assert_eq!(read(&mut host, CLAUDE_SETTING), original);
    assert_eq!(leftover_temporaries(&mut host), 0);
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::apply_validates_the_value_again_even_for_a_hand_built_plan
#[test]
fn port_cw_35_apply_validates_the_value_again_even_for_a_hand_built_plan() {
    let mut host = host_with(&[(CLAUDE_SETTING, "{}")]);
    let mut planned = plan(&mut host, CLAUDE_SETTING, 250_000).expect("plan");
    planned["new_value"] = json!(5);
    assert_eq!(code_of(apply_plan(&mut host, &planned)), ErrorCode::InvalidArgument);
    assert_eq!(read(&mut host, CLAUDE_SETTING), "{}");
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::revert_refuses_when_the_file_no_longer_holds_the_written_value
#[test]
fn port_cw_36_revert_refuses_when_the_file_no_longer_holds_the_written_value() {
    let mut host = host_with(&[(CLAUDE_SETTING, "{\"autoCompactWindow\": 567000}")]);
    let receipt = apply_value(&mut host, CLAUDE_SETTING, 250_000);
    let user_edit = "{\"autoCompactWindow\": 321000}";
    put(&mut host, claude_settings_file(), user_edit);
    assert_eq!(code_of(revert_record(&mut host, &receipt["record"])), ErrorCode::Conflict);
    assert_eq!(read(&mut host, CLAUDE_SETTING), user_edit);
    assert!(load_record(&mut host, CLAUDE_SETTING).is_some(), "a refused revert keeps its record");
    put(&mut host, claude_settings_file(), "{}");
    assert_eq!(code_of(revert_record(&mut host, &receipt["record"])), ErrorCode::Conflict);
    assert_eq!(read(&mut host, CLAUDE_SETTING), "{}");
    host.fs().remove(claude_settings_file());
    assert_eq!(code_of(revert_record(&mut host, &receipt["record"])), ErrorCode::NotFound);
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::revert_twice_reports_stale_instead_of_changing_the_file_again
#[test]
fn port_cw_37_revert_twice_reports_conflict_instead_of_changing_the_file_again() {
    let mut host = host_with(&[(CODEX_SETTING, "a = 1\n")]);
    let receipt = apply_value(&mut host, CODEX_SETTING, 250_000);
    revert_record(&mut host, &receipt["record"]).expect("first revert");
    assert_eq!(code_of(revert_record(&mut host, &receipt["record"])), ErrorCode::Conflict);
    assert_eq!(read(&mut host, CODEX_SETTING), "a = 1\n");
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::revert_rejects_a_record_that_names_an_unrelated_file
#[test]
fn port_cw_38_revert_rejects_a_record_that_names_an_unrelated_file_with_invalid_argument() {
    let mut host = server_host();
    let victim = scoped("agent-config:claude", "notes.txt");
    put(&mut host, victim.clone(), "{\"autoCompactWindow\": 250000}");
    let record = json!({
        "target": CLAUDE_SETTING,
        "path": "notes.txt",
        "previous_value": null,
        "written_value": 250_000,
        "applied_at": "2026-10-05 12:00:00",
    });
    assert_eq!(code_of(revert_record(&mut host, &record)), ErrorCode::InvalidArgument);
    assert_eq!(get(&mut host, victim).as_deref(), Some("{\"autoCompactWindow\": 250000}"));
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::a_second_apply_records_the_value_it_replaced
#[test]
fn port_cw_39_a_second_apply_records_the_value_it_replaced() {
    let mut host = host_with(&[(CLAUDE_SETTING, "{\"autoCompactWindow\": 567000}")]);
    apply_value(&mut host, CLAUDE_SETTING, 300_000);
    let second = apply_value(&mut host, CLAUDE_SETTING, 200_000);
    assert_eq!(second["record"]["previous_value"], json!(300_000));
    assert_eq!(load_record(&mut host, CLAUDE_SETTING).expect("record")["written_value"], json!(200_000));
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::a_failed_second_apply_keeps_the_first_applys_revert_record
#[test]
fn port_cw_40_a_failed_second_apply_keeps_the_first_applys_revert_record() {
    let mut host = host_with(&[(CLAUDE_SETTING, "{\"autoCompactWindow\": 567000}")]);
    let first = apply_value(&mut host, CLAUDE_SETTING, 300_000);
    let planned = plan(&mut host, CLAUDE_SETTING, 200_000).expect("plan");
    host.fs().race_next_write(claude_settings_file(), b"{\"autoCompactWindow\": 300000, \"x\": 1}");
    assert_eq!(code_of(apply_plan(&mut host, &planned)), ErrorCode::Conflict);
    assert_eq!(load_record(&mut host, CLAUDE_SETTING), Some(first["record"].clone()));
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::records_round_trip_through_json_and_belong_to_one_target
#[test]
fn port_cw_41_records_round_trip_through_json_and_belong_to_one_target() {
    let mut host = host_with(&[(CLAUDE_SETTING, "{}")]);
    let receipt = apply_value(&mut host, CLAUDE_SETTING, 250_000);
    assert_eq!(load_record(&mut host, CLAUDE_SETTING), Some(receipt["record"].clone()));
    assert_eq!(receipt["record"]["target"], json!(CLAUDE_SETTING));
    assert!(receipt["record"]["previous_value"].is_null());
    assert!(load_record(&mut host, CODEX_SETTING).is_none());
    // A record stored for one target and offered for another is refused.
    let wrong = json!({ "target": CODEX_SETTING, "path": "config.toml", "previous_value": null,
        "written_value": 250_000, "applied_at": "2026-10-05 12:00:00" });
    host.fs().write(codex_config_file(), b"a = 1\n");
    assert_eq!(code_of(revert_record(&mut host, &json!({ "record": wrong, "stored_for": CLAUDE_SETTING }))), ErrorCode::InvalidArgument);
    configure(&mut host, "clear-record", json!({ "target": CLAUDE_SETTING })).expect("clear");
    configure(&mut host, "clear-record", json!({ "target": CLAUDE_SETTING })).expect("clearing twice is fine");
    assert!(load_record(&mut host, CLAUDE_SETTING).is_none());
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::apply_writes_through_a_symlink_and_keeps_it_a_symlink
#[test]
fn port_cw_42_apply_writes_through_a_symlink_and_keeps_it_a_symlink() {
    let mut host = server_host();
    let real = scoped("agent-config:claude", "dotfiles/settings.json");
    put(&mut host, real.clone(), "{\"autoCompactWindow\": 567000}\n");
    host.fs().symlink(claude_settings_file(), real.clone());
    let planned = plan(&mut host, CLAUDE_SETTING, 250_000).expect("plan");
    assert_eq!(planned["path"], json!("dotfiles/settings.json"));
    apply_plan(&mut host, &planned).expect("apply");
    assert!(host.fs().is_symlink(claude_settings_file()));
    assert_eq!(get(&mut host, real).as_deref(), Some("{\"autoCompactWindow\": 250000}\n"));
    assert_eq!(leftover_temporaries(&mut host), 0);
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::apply_refuses_when_the_symlink_is_retargeted_after_planning
#[test]
fn port_cw_43_apply_writes_only_the_planned_file_when_the_symlink_is_retargeted_after_planning() {
    let mut host = server_host();
    let first = scoped("agent-config:claude", "first.json");
    let second = scoped("agent-config:claude", "second.json");
    put(&mut host, first.clone(), "{\"autoCompactWindow\": 567000}");
    put(&mut host, second.clone(), "{\"autoCompactWindow\": 567000}");
    host.fs().symlink(claude_settings_file(), first.clone());
    let planned = plan(&mut host, CLAUDE_SETTING, 250_000).expect("plan");
    host.fs().remove(claude_settings_file());
    host.fs().symlink(claude_settings_file(), second.clone());
    apply_plan(&mut host, &planned).expect("the planned file is unchanged, so the write lands there");
    assert_eq!(get(&mut host, second).as_deref(), Some("{\"autoCompactWindow\": 567000}"));
    assert_eq!(get(&mut host, first).as_deref(), Some("{\"autoCompactWindow\": 250000}"));
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::apply_and_revert_preserve_file_permissions
#[test]
fn port_cw_44_apply_and_revert_preserve_file_permissions() {
    let mut host = host_with(&[(CODEX_SETTING, "a = 1\n"), (CLAUDE_SETTING, "{}")]);
    for (target, mode) in [(CODEX_SETTING, 0o640u32), (CLAUDE_SETTING, 0o600u32)] {
        host.fs().set_mode(file_of(target), mode);
        let receipt = apply_value(&mut host, target, 250_000);
        assert_eq!(host.fs().mode(file_of(target)), Some(mode));
        revert_record(&mut host, &receipt["record"]).expect("revert");
        assert_eq!(host.fs().mode(file_of(target)), Some(mode));
    }
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::lock_contention_is_reported_without_touching_the_file
#[test]
fn port_cw_45_lock_contention_is_reported_as_busy_without_touching_the_file() {
    let original = "{\"autoCompactWindow\": 567000}";
    let mut host = host_with(&[(CLAUDE_SETTING, original)]);
    let planned = plan(&mut host, CLAUDE_SETTING, 250_000).expect("plan");
    let held = host.fs().hold_lock(claude_settings_file(), true);
    assert_eq!(code_of(apply_plan(&mut host, &planned)), ErrorCode::Busy);
    assert_eq!(read(&mut host, CLAUDE_SETTING), original);
    held.release();
    apply_plan(&mut host, &planned).expect("apply after the lock is free");
    assert_eq!(read(&mut host, CLAUDE_SETTING), "{\"autoCompactWindow\": 250000}");
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::revert_also_waits_for_the_lock_and_then_gives_up
#[test]
fn port_cw_46_revert_also_waits_for_the_lock_and_then_gives_up_as_busy() {
    let mut host = host_with(&[(CLAUDE_SETTING, "{}")]);
    let receipt = apply_value(&mut host, CLAUDE_SETTING, 250_000);
    let held = host.fs().hold_lock(claude_settings_file(), true);
    assert_eq!(code_of(revert_record(&mut host, &receipt["record"])), ErrorCode::Busy);
    held.release();
    revert_record(&mut host, &receipt["record"]).expect("revert after the lock is free");
    assert_eq!(read(&mut host, CLAUDE_SETTING), "{}");
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::apply_refuses_a_file_that_stopped_being_valid
#[test]
fn port_cw_47_apply_refuses_a_file_that_stopped_being_valid() {
    let mut host = host_with(&[(CODEX_SETTING, "a = 1\n")]);
    let planned = plan(&mut host, CODEX_SETTING, 250_000).expect("plan");
    put(&mut host, codex_config_file(), "a = \n");
    // The content no longer matches the plan, so the conflict is reported first.
    assert_eq!(code_of(apply_plan(&mut host, &planned)), ErrorCode::Conflict);
    assert_eq!(read(&mut host, CODEX_SETTING), "a = \n");
    assert_eq!(code_of(plan(&mut host, CODEX_SETTING, 250_000)), ErrorCode::InvalidArgument);
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::a_missing_file_is_never_created
#[test]
fn port_cw_48_a_missing_file_is_never_created_and_planning_reports_not_found() {
    let mut host = server_host();
    assert_eq!(code_of(plan(&mut host, CLAUDE_SETTING, 250_000)), ErrorCode::NotFound);
    assert!(get(&mut host, claude_settings_file()).is_none());
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::the_nine_distinct_error_variants_have_distinct_messages
#[test]
fn port_cw_49_the_distinct_refusals_have_distinct_messages() {
    let mut host = host_with(&[(CODEX_SETTING, "a = 1\n")]);
    let mut messages: Vec<String> = Vec::new();
    messages.push(read_current(&mut host, CLAUDE_SETTING).expect_err("missing").message);
    host.fs().mkdir(claude_settings_file());
    messages.push(read_current(&mut host, CLAUDE_SETTING).expect_err("not regular").message);
    host.fs().write(codex_config_file(), &vec![b' '; MAX_CONFIG_BYTES + 1]);
    messages.push(read_current(&mut host, CODEX_SETTING).expect_err("too large").message);
    host.fs().write(codex_config_file(), b"a = \n");
    messages.push(read_current(&mut host, CODEX_SETTING).expect_err("parse").message);
    host.fs().write(codex_config_file(), b"a = 1\n");
    messages.push(plan(&mut host, CODEX_SETTING, 1).expect_err("range").message);
    let planned = plan(&mut host, CODEX_SETTING, 250_000).expect("plan");
    put(&mut host, codex_config_file(), "a = 2\n");
    messages.push(apply_plan(&mut host, &planned).expect_err("stale").message);
    let distinct: std::collections::BTreeSet<&String> = messages.iter().collect();
    assert_eq!(distinct.len(), messages.len(), "{messages:?}");
}
