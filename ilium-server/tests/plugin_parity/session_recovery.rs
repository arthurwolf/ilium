//! Baseline scenarios of src/docs/session-recovery.md: F-073 snapshots, F-074 rolling backups,
//! F-075 restore after restart. Paths and numbers are the documented ones.

use ilium_plugin_host::lifecycle::StartMode;

use crate::common::*;

const HALF_HOUR_MS: u64 = 30 * 60 * 1000;
const DAY_MS: u64 = 24 * 60 * 60 * 1000;

fn small_tree(kernel: &ilium_server::plugin_host::ServerKernel) {
    let root = project(kernel, "api");
    terminal(kernel, Some(root), "shell", &["sh"]);
}

/// REQ: F-073, COR-001
#[test]
fn f_073_snapshot_is_written_to_the_session_file_after_a_structural_change() {
    in_both_modes(|kernel| {
        small_tree(kernel);
        kernel.advance_clock_ms(750);
        kernel.settle();
        assert!(kernel.project_file_exists(".ilium/sessions/default.json"));
    });
}

/// REQ: F-073, COR-001
#[test]
fn f_073_bursts_of_changes_are_debounced_into_one_write_per_750_ms() {
    in_both_modes(|kernel| {
        for n in 0..10 {
            terminal(kernel, None, &format!("p{n}"), &["sh"]);
        }
        kernel.advance_clock_ms(749);
        kernel.settle();
        assert_eq!(kernel.snapshot_write_count(), 0, "nothing is written before 750 ms");
        kernel.advance_clock_ms(1);
        kernel.settle();
        assert_eq!(kernel.snapshot_write_count(), 1);
    });
}

/// REQ: F-073, COR-001
#[test]
fn f_073_snapshot_is_json_and_holds_the_titles_of_every_node() {
    in_both_modes(|kernel| {
        small_tree(kernel);
        kernel.advance_clock_ms(750);
        kernel.settle();
        let text = kernel.read_project_file(".ilium/sessions/default.json").expect("snapshot");
        let json: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        assert!(json.is_object(), "snapshot root is a JSON object: {json}");
        let mut strings = Vec::new();
        collect_strings(&json, &mut strings);
        assert!(strings.iter().any(|value| value == "api"), "{strings:?}");
        assert!(strings.iter().any(|value| value == "shell"), "{strings:?}");
    });
}

/// REQ: F-073, COR-001
#[test]
fn f_073_snapshot_file_is_owner_only() {
    in_both_modes(|kernel| {
        small_tree(kernel);
        kernel.advance_clock_ms(750);
        kernel.settle();
        assert_eq!(kernel.project_file_mode(".ilium/sessions/default.json"), Some(0o600));
    });
}

/// REQ: F-073, COR-001
#[test]
fn f_073_a_named_session_uses_its_own_snapshot_file() {
    let kernel = ilium_server::plugin_host::ServerKernel::for_parity_world_named(StartMode::Normal, "", "review");
    small_tree(&kernel);
    kernel.advance_clock_ms(750);
    kernel.settle();
    assert!(kernel.project_file_exists(".ilium/sessions/review.json"));
    assert!(!kernel.project_file_exists(".ilium/sessions/default.json"));
}

/// REQ: F-074, COR-001
#[test]
fn f_074_a_copy_is_taken_at_server_start_when_a_snapshot_exists() {
    in_both_modes(|kernel| {
        small_tree(kernel);
        kernel.advance_clock_ms(750);
        kernel.settle();
        let restarted = kernel.restart_from_disk();
        assert_eq!(restarted.backup_files().len(), 1);
    });
}

/// REQ: F-074, COR-001
#[test]
fn f_074_copy_names_are_milliseconds_dash_uuid_json() {
    in_both_modes(|kernel| {
        small_tree(kernel);
        kernel.advance_clock_ms(750);
        kernel.settle();
        let name = kernel.restart_from_disk().backup_files().remove(0);
        let (millis, rest) = name.split_once('-').expect("dash");
        assert!(millis.chars().all(|c| c.is_ascii_digit()), "{name}");
        assert!(rest.ends_with(".json") && rest.len() > ".json".len(), "{name}");
    });
}

/// REQ: F-074, COR-001
#[test]
fn f_074_at_most_one_copy_per_half_hour() {
    in_both_modes(|kernel| {
        small_tree(kernel);
        kernel.advance_clock_ms(750);
        kernel.settle();
        let before = kernel.backup_files().len();
        kernel.advance_clock_ms(HALF_HOUR_MS - 1);
        kernel.touch_tree();
        kernel.settle();
        assert_eq!(kernel.backup_files().len(), before, "29 min 59 s later no new copy");
        kernel.advance_clock_ms(1);
        kernel.touch_tree();
        kernel.settle();
        assert_eq!(kernel.backup_files().len(), before + 1, "after 30 min a copy is taken");
    });
}

/// REQ: F-074, COR-001
#[test]
fn f_074_copies_under_one_day_old_keep_one_per_half_hour_up_to_48() {
    in_both_modes(|kernel| {
        small_tree(kernel);
        for _ in 0..60 {
            kernel.advance_clock_ms(HALF_HOUR_MS);
            kernel.touch_tree();
            kernel.settle();
        }
        assert!(kernel.backup_files_younger_than_ms(DAY_MS).len() <= 48);
    });
}

/// REQ: F-074, COR-001
#[test]
fn f_074_turning_backups_off_stops_new_copies_and_keeps_existing_ones() {
    in_both_modes(|kernel| {
        small_tree(kernel);
        kernel.advance_clock_ms(750);
        kernel.settle();
        let existing = kernel.restart_from_disk().backup_files().len();
        assert_eq!(kernel.apply_setting("session", "backups_enabled", "false"), Ok(()));
        kernel.advance_clock_ms(HALF_HOUR_MS);
        kernel.touch_tree();
        kernel.settle();
        assert_eq!(kernel.backup_files().len(), existing);
    });
}

/// REQ: F-074, COR-001
#[test]
fn f_074_turning_backups_on_again_takes_a_fresh_copy_at_once() {
    in_both_modes(|kernel| {
        small_tree(kernel);
        kernel.apply_setting("session", "backups_enabled", "false").expect("off");
        kernel.advance_clock_ms(750);
        kernel.settle();
        let before = kernel.backup_files().len();
        kernel.apply_setting("session", "backups_enabled", "true").expect("on");
        kernel.settle();
        assert_eq!(kernel.backup_files().len(), before + 1);
    });
}

/// REQ: F-075, COR-001
#[test]
fn f_075_default_recovery_policy_restores_the_tree_automatically() {
    in_both_modes(|kernel| {
        assert_eq!(kernel.setting("session", "recovery_policy").as_deref(), Some("restore_automatically"));
        small_tree(kernel);
        kernel.advance_clock_ms(750);
        kernel.settle();
        let restarted = kernel.restart_from_disk();
        assert_eq!(titles(&restarted), vec!["api", "shell"]);
    });
}

/// REQ: F-075, COR-001
#[test]
fn f_075_start_fresh_ignores_the_snapshot_and_backs_it_up_first() {
    in_both_modes(|kernel| {
        kernel.apply_setting("session", "recovery_policy", "start_fresh").expect("policy");
        small_tree(kernel);
        kernel.advance_clock_ms(750);
        kernel.settle();
        let restarted = kernel.restart_from_disk();
        assert!(titles(&restarted).is_empty());
        assert_eq!(restarted.backup_files().len(), 1);
    });
}

/// REQ: F-075, COR-001
#[test]
fn f_075_ask_before_restore_starts_empty_and_asks_with_the_pane_count() {
    in_both_modes(|kernel| {
        kernel.apply_setting("session", "recovery_policy", "ask_before_restore").expect("policy");
        small_tree(kernel);
        kernel.advance_clock_ms(750);
        kernel.settle();
        let restarted = kernel.restart_from_disk();
        assert!(titles(&restarted).is_empty());
        assert_eq!(restarted.pending_restore_prompt(), Some(1));
    });
}

/// REQ: F-075, COR-001
#[test]
fn f_075_an_unknown_recovery_policy_value_is_rejected() {
    in_both_modes(|kernel| {
        assert!(kernel.apply_setting("session", "recovery_policy", "sometimes").is_err());
        assert_eq!(kernel.setting("session", "recovery_policy").as_deref(), Some("restore_automatically"));
    });
}

/// REQ: F-075, COR-001
#[test]
fn f_075_a_pane_that_cannot_start_keeps_its_place_in_the_tree() {
    in_both_modes(|kernel| {
        let pane = terminal(kernel, None, "gone", &["/nonexistent/ilium-parity-binary"]);
        kernel.advance_clock_ms(750);
        kernel.settle();
        let restarted = kernel.restart_from_disk();
        assert_eq!(titles(&restarted), vec!["gone"]);
        assert!(pane.0 > 0);
    });
}

/// REQ: F-075, COR-001
#[test]
fn f_075_each_start_keeps_the_loaded_snapshot_as_pre_restore_json() {
    in_both_modes(|kernel| {
        small_tree(kernel);
        kernel.advance_clock_ms(750);
        kernel.settle();
        let restarted = kernel.restart_from_disk();
        assert!(restarted.project_file_exists(".ilium/sessions/default.pre-restore.json"));
    });
}

/// REQ: F-075, COR-001
#[test]
fn f_075_unsaved_editor_buffers_are_not_part_of_the_snapshot() {
    in_both_modes(|kernel| {
        kernel.write_project_file("notes.txt", "one\n");
        let pane = created(apply(
            kernel,
            ilium_plugin_api::generated::types::Intent::EditorOpen(ilium_plugin_api::generated::types::EditorOpenSpec {
                path: ilium_plugin_api::generated::types::FsPath { scope: "project".to_owned(), path: "notes.txt".to_owned() },
                line: None,
                placement: ilium_plugin_api::generated::types::EditorPlacement::NewPane,
            }),
        ));
        kernel.editor_type(pane, "unsaved ");
        kernel.advance_clock_ms(750);
        kernel.settle();
        let text = kernel.read_project_file(".ilium/sessions/default.json").expect("snapshot");
        assert!(!text.contains("unsaved"), "{text}");
    });
}

fn collect_strings(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::String(text) => out.push(text.clone()),
        serde_json::Value::Array(items) => items.iter().for_each(|item| collect_strings(item, out)),
        serde_json::Value::Object(map) => map.values().for_each(|item| collect_strings(item, out)),
        _ => {}
    }
}
