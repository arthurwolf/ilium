//! Statements of the page, its linked reference pages and the user documentation that the
//! plugin has to keep (kernel tab, extension points, service, topic, figures, recovery settings).

use serde_json::Value;

use crate::common::*;

fn points_page() -> String {
    page("reference-core-extension-points")
}

fn example() -> Value {
    let blocks = json_blocks(&page_text());
    serde_json::from_str(blocks.last().expect("example block")).expect("example is JSON")
}

/// REQ: COR-030, COR-033
#[test]
fn doc_030_the_plugin_owns_the_session_tab_over_the_session_table() {
    let row = table_row(&page_text(), "`session`");
    assert_eq!(row[1], "Session");
    assert_eq!(row[2], "`[session]`");
    assert_eq!(row[3], "none");
}

/// REQ: COR-030
#[test]
fn doc_030_config_page_names_sessions_as_the_owner_of_the_session_tab() {
    let page = page("reference-config-and-data-migration");
    assert!(page.contains("| `session` | Session | `[session]` | Sessions | none | `[session]` |"));
}

/// REQ: COR-033
#[test]
fn doc_033_session_table_keeps_exactly_the_two_documented_keys() {
    let page = page("reference-config-and-data-migration");
    assert!(page.contains("| `[session]` | `recovery_policy`, `backups_enabled` | Sessions, kernel snapshot writer | yes (server-owned) |"));
}

/// REQ: COR-033
#[test]
fn doc_033_page_states_that_no_user_setting_moves() {
    assert!(page_text().contains(
        "The values stay in the same `config.toml` tables as before, so no user setting moves."
    ));
}

/// REQ: COR-022
#[test]
fn doc_022_disabled_notice_text_is_exact() {
    assert!(page_text().contains("the notice \"Sessions is disabled. Values are kept.\""));
}

/// REQ: COR-022
#[test]
fn doc_022_disabled_tab_stays_in_the_rail_with_stored_values_read_only() {
    assert!(
        page_text().contains("the tab stays in the rail with its stored values shown read-only")
    );
}

/// REQ: COR-022
#[test]
fn doc_022_kernel_and_boot_row_keeps_the_snapshot_writer_running() {
    let row = table_row(&page("reference-kernel-and-boot"), "Sessions");
    assert!(row.len() >= 3);
    let text = page("reference-kernel-and-boot");
    assert!(text.contains(
        "| Sessions | No session list or recovery dialogs | The kernel snapshot writer still runs and keeps the last stored plugin sections |"
    ));
}

/// REQ: COR-022
#[test]
fn doc_022_kernel_fallback_when_off_is_the_snapshot_writer() {
    let row = table_row(
        &page("reference-kernel-and-boot"),
        "Kernel fallback when off",
    );
    assert_eq!(row[1], "snapshot writer");
}

/// REQ: F-076
#[test]
fn doc_076_snapshot_sections_is_a_standard_collect_point() {
    let row = table_row(&page_text(), "`snapshot-sections`");
    assert_eq!(row[1], "collect");
    assert_eq!(
        row[2],
        "Store extra JSON in the session snapshot and receive it back on restore."
    );
    assert_eq!(
        row[3],
        "entry: id, max_bytes (the plugin answers the `snapshot-section` and `restore-section` requests in its handler)"
    );
    assert_eq!(row[4], "standard");
}

/// REQ: F-076
#[test]
fn doc_076_before_restore_is_a_standard_observe_hook() {
    let row = table_row(&page_text(), "`before-restore`");
    assert_eq!(row[1], "hook (observe)");
    assert_eq!(row[2], "Learn that a restore starts, to prepare state.");
    assert_eq!(row[3], "session, pane_count");
    assert_eq!(row[4], "standard");
}

/// REQ: F-076
#[test]
fn doc_076_core_extension_points_page_agrees_on_both_points() {
    let page = points_page();
    assert!(page.contains(
        "| `ilium.core.sessions/snapshot-sections` | collect | standard | Store extra JSON in the session snapshot and receive it back on restore. |"
    ));
    assert!(page.contains(
        "| `ilium.core.sessions/before-restore` | hook: observe | standard | Learn that a restore starts, to prepare state. |"
    ));
}

/// REQ: F-076
#[test]
fn doc_076_the_plugin_owns_two_points_one_service_one_topic_and_is_not_replaceable() {
    let row = table_row(&points_page(), "[Sessions](/pages/core-plugin-sessions/)");
    assert_eq!(&row[1..], ["`ilium.core.sessions`", "2", "1", "1", "no"]);
}

/// REQ: F-076
#[test]
fn doc_076_page_states_the_snapshots_service_and_core_page_agrees() {
    assert!(page_text()
        .contains("- `ilium.core.sessions/snapshots`: list, save, restore: manage snapshots"));
    assert!(points_page()
        .contains("| `ilium.core.sessions/snapshots` | list, save, restore: manage snapshots |"));
}

/// REQ: F-076
#[test]
fn doc_076_page_states_the_saved_topic_and_core_page_agrees() {
    assert!(page_text().contains("- `ilium.core.sessions/saved`: { session, bytes }"));
    assert!(points_page().contains("| `ilium.core.sessions/saved` | { session, bytes } |"));
}

/// REQ: F-076
#[test]
fn doc_076_page_says_the_plugin_is_not_replaceable() {
    assert!(page_text().contains("`replaceable` is `false`"));
    let row = table_row(&page("reference-kernel-and-boot"), "Replaceable");
    assert_eq!(row[1], "no");
}

/// REQ: F-076
#[test]
fn doc_076_harness_core_stub_exposes_the_two_points_and_no_service() {
    let page = page("reference-test-harness");
    assert!(page.contains(
        "| `ilium.core.sessions` | `snapshot-sections` (collect), `before-restore` (observe) | none | collect: empty list; hooks: `pass` |"
    ));
}

/// REQ: F-076
#[test]
fn doc_076_example_contribution_targets_the_snapshot_sections_point() {
    let example = example();
    assert_eq!(example["api"], "ilium:plugin@0.1.0");
    assert_eq!(
        strings(&example["capabilities"]["server"]),
        ["extensions.contribute:ilium.core.sessions/snapshot-sections"]
    );
    let entry = &example["contributes"]["extensions"][0];
    assert_eq!(entry["point"], "ilium.core.sessions/snapshot-sections");
    assert_eq!(entry["id"], "my-notes");
    assert_eq!(entry["priority"], 50);
    assert_eq!(entry["entry"]["max_bytes"], 65536);
}

/// REQ: COR-032
#[test]
fn doc_032_example_max_bytes_is_within_the_256_kib_ceiling() {
    let example = example();
    let max_bytes = example["contributes"]["extensions"][0]["entry"]["max_bytes"]
        .as_u64()
        .expect("number");
    assert!(max_bytes <= 262_144);
}

/// REQ: COR-032
#[test]
fn doc_032_snapshot_section_rules_name_the_limits_and_codes() {
    let page = page("reference-config-and-data-migration");
    assert!(page.contains("`max_bytes` is at most 262144"));
    assert!(page.contains("at most once per 750 ms"));
    assert!(page.contains("`snapshot.section-too-large`"));
    assert!(page.contains("`section-unsupported`"));
    assert!(page.contains("`restore-section { schema_version, data }`"));
}

/// REQ: COR-032
#[test]
fn doc_032_section_entry_shape_is_id_title_max_bytes() {
    let page = page("reference-config-and-data-migration");
    assert!(page.contains("`{ \"id\": \"<section id>\", \"title\": \"...\", \"max_bytes\": <n> }`"));
}

/// REQ: LIF-276
#[test]
fn doc_276_core_plugins_page_lists_sessions_among_the_essential_plugins() {
    let page = page("core-plugins");
    assert!(page.contains(
        "except the essential set: `ilium.core.tree`, `ilium.core.layout`, `ilium.core.settings` and `ilium.core.sessions`"
    ));
}

/// REQ: F-076
#[test]
fn doc_076_restore_summary_figure_shows_panes_agents_and_one_frozen_pane() {
    let blocks = term_blocks(&page_text());
    let figure = &blocks[0];
    assert!(figure.contains("╭─ Restored "));
    assert!(figure.contains("Panes restored"));
    assert!(figure.contains("7 of 8"));
    assert!(figure.contains("Agents resumed"));
    assert!(figure.contains("3 of 3"));
    assert!(figure.contains("Docs sweep is frozen"));
    assert!(figure.contains("Its folder is missing. Press u to unfreeze."));
}

/// REQ: F-076
#[test]
fn doc_076_restore_summary_figure_has_uniform_line_width() {
    let blocks = term_blocks(&page_text());
    let widths: Vec<usize> = blocks[0].lines().map(|line| line.chars().count()).collect();
    assert!(
        widths.windows(2).all(|pair| pair[0] == pair[1]),
        "widths differ: {widths:?}"
    );
}

/// REQ: F-076
#[test]
fn doc_076_restore_summary_figure_fits_the_stated_48_columns() {
    let blocks = term_blocks(&page_text());
    assert!(blocks[0].lines().all(|line| line.chars().count() <= 48));
}

/// REQ: F-076
#[test]
fn doc_076_ilium_ls_figure_prints_one_json_line_per_session_then_a_summary() {
    let blocks = term_blocks(&page_text());
    let lines: Vec<&str> = blocks[1].lines().collect();
    assert_eq!(lines[0], "$ ilium ls");
    let records: Vec<Value> = lines[1..]
        .iter()
        .map(|line| serde_json::from_str(line).expect("JSON line"))
        .collect();
    let (summary, sessions) = records.split_last().expect("records");
    for session in sessions {
        assert_eq!(session["type"], "session");
        assert!(session["name"].is_string());
        assert!(matches!(
            session["state"].as_str(),
            Some("attached" | "detached")
        ));
        assert!(session["panes"].is_u64());
    }
    assert_eq!(summary["type"], "summary");
    assert_eq!(summary["sessions"].as_u64(), Some(sessions.len() as u64));
}

/// REQ: F-076
#[test]
fn doc_076_ilium_ls_figure_fits_80_columns() {
    let blocks = term_blocks(&page_text());
    assert!(blocks[1].lines().all(|line| line.chars().count() <= 80));
}

/// REQ: F-076
#[test]
fn doc_076_recovery_policy_values_and_default_match_the_user_documentation() {
    let doc = user_doc("session-recovery.md");
    assert!(doc.contains("| Restore automatically (default) | `restore_automatically` |"));
    assert!(doc.contains("| Ask before restoring | `ask_before_restore` |"));
    assert!(doc.contains("| Start fresh | `start_fresh` |"));
    assert!(doc.contains("recovery_policy = \"restore_automatically\""));
    assert!(user_doc("settings.md").contains("| `session.recovery_policy` | `restore_automatically`, `ask_before_restore`, `start_fresh` |"));
}

/// REQ: F-076
#[test]
fn doc_076_unrecognised_policy_is_a_configuration_error_and_applies_at_next_start() {
    let doc = user_doc("session-recovery.md");
    assert!(doc.contains("An unrecognised value is a configuration error, not a silent fallback"));
    assert!(doc.contains("Changing the policy affects the next start, not the running session."));
}

/// REQ: F-076
#[test]
fn doc_076_backups_default_on_and_resume_needs_a_verified_session() {
    let doc = user_doc("session-recovery.md");
    assert!(doc.contains("(`[session].backups_enabled`, default on)"));
    assert!(doc.contains("A resume command is saved only when Ilium has **verified** the session"));
}
