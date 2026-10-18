//! Rules the page states in prose and tables: the enums the plugin reports, the kernel tab it owns, the
//! polling defaults, the detection budget, the example package and the traceability rows (doc_ tests).

use serde_json::Value;

use crate::common::{page_text, traceability_text};

/// Rows of the first table that follows the line `anchor`: cells of every row after the separator.
fn table_after(page: &str, anchor: &str) -> Vec<Vec<String>> {
    let start = page
        .find(anchor)
        .unwrap_or_else(|| panic!("anchor {anchor} missing"));
    page[start..]
        .lines()
        .skip_while(|line| !line.starts_with('|'))
        .skip(2)
        .take_while(|line| line.starts_with('|'))
        .map(|line| {
            line.trim_matches('|')
                .split('|')
                .map(|cell| cell.trim().to_string())
                .collect()
        })
        .collect()
}

fn enum_row(name: &str) -> String {
    let rows = table_after(&page_text(), "**Enums the plugin reports.**");
    rows.into_iter()
        .find(|row| row[0] == name)
        .unwrap_or_else(|| panic!("enum row {name}"))[1]
        .clone()
}

/// REQ: COR-011, F-026
#[test]
fn doc_011_agent_class_enum_lists_claude_codex_antigravity_and_other() {
    assert_eq!(
        enum_row("Agent class"),
        "`claude`, `codex`, `antigravity`, `other(name)`. `antimatter` is accepted as a process-name alias of `antigravity`"
    );
}

/// REQ: F-027
#[test]
fn doc_027_activity_enum_lists_six_values_in_order() {
    assert_eq!(
        enum_row("Activity"),
        "`none`, `working`, `idle`, `awaiting-approval`, `done`, `menu` (the `agent-activity` enum of [WIT](/pages/reference-wit/))"
    );
}

/// REQ: F-027
#[test]
fn doc_027_turn_enum_lists_five_values_in_order() {
    assert_eq!(
        enum_row("Turn"),
        "`working`, `waiting-approval`, `waiting-subagents`, `settling`, `idle`"
    );
}

/// REQ: F-029
#[test]
fn doc_029_goal_state_enum_lists_five_values_in_order() {
    assert_eq!(
        enum_row("Goal state"),
        "`active`, `paused`, `blocked`, `usage-limited`, `reached`"
    );
}

/// REQ: F-028
#[test]
fn doc_028_pane_kind_enum_lists_five_values_in_order() {
    assert_eq!(
        enum_row("Pane kind"),
        "`plain-shell`, `agent`, `editor`, `board`, `agent-unavailable`"
    );
}

/// REQ: COR-030, COR-033
#[test]
fn doc_030_the_plugin_owns_exactly_the_agent_monitoring_tab() {
    let rows = table_after(&page_text(), "| Tab id | Label | Config table | Readers |");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0],
        [
            "`agent-monitoring`",
            "Agent Monitoring",
            "`[ui]`, `[detection]`",
            "Progress, Agent toolbar, Notifications"
        ]
    );
}

/// REQ: COR-022
#[test]
fn doc_022_disabled_notice_keeps_the_values_read_only() {
    let page = page_text();
    assert!(page.contains("the tab stays in the rail with its stored values shown read-only and the notice \"Agent detection is disabled. Values are kept.\""));
}

/// REQ: F-030
#[test]
fn doc_030_polling_defaults_are_ten_and_forty_five_seconds() {
    let page = page_text();
    assert!(page.contains(
        "`working_poll_seconds` (default 10) and `idle_poll_seconds` (default 45) in `[detection]`"
    ));
}

/// REQ: F-027
#[test]
fn doc_027_one_poll_cycle_spends_at_most_fifty_milliseconds_on_detectors() {
    let page = page_text();
    assert!(page.contains("One poll cycle spends at most 50 ms on detector calls."));
    assert!(page.contains("A third breach in a row traps the call"));
}

/// REQ: F-030
#[test]
fn doc_030_custom_signature_toml_sample_has_class_other() {
    let page = page_text();
    let start = page.find("```toml\n").expect("toml block") + "```toml\n".len();
    let end = page[start..].find("```").expect("block end");
    let block = &page[start..start + end];
    assert!(block.contains("[[detection.custom_signatures]]"));
    assert!(block.contains("process_name = \"myagent\""));
    assert!(block.contains("agent_class  = \"other\""));
}

/// REQ: F-030
#[test]
fn doc_030_a_custom_signature_has_no_launch_resume_or_transcript_contract() {
    let page = page_text();
    assert!(page.contains("A custom signature has the class `other`. It gets an identity and an icon, but no launch, resume or transcript contract"));
}

/// REQ: F-028
#[test]
fn doc_028_status_line_capture_stays_in_the_kernel_f111() {
    let page = page_text();
    assert!(page.contains("The kernel keeps the hidden helper commands that capture it"));
    assert!(page.contains("(F-111)"));
}

/// REQ: COR-011
#[test]
fn doc_011_auto_answer_key_is_in_the_detection_table_and_on_by_default() {
    let page = page_text();
    assert!(page.contains("`auto_answer_interstitial_prompts` in the `[detection]` table"));
    assert!(page.contains("The behaviour is on by default"));
    assert!(page.contains("through `terminal.write`, once per process"));
}

/// REQ: F-026, F-027, F-028, F-029, F-030, F-034
#[test]
fn doc_026_traceability_rows_name_the_agent_detect_plugin_as_owner() {
    let traceability = traceability_text();
    for id in ["F-026", "F-027", "F-028", "F-029", "F-030", "F-034"] {
        let row = traceability
            .lines()
            .find(|line| line.starts_with(&format!("| {id} |")))
            .unwrap_or_else(|| panic!("row {id}"));
        assert!(row.contains("agent-detect plugin"), "{row}");
        assert!(row.contains("`agent-monitoring`"), "{row}");
        assert!(row.contains("primitive-extended"), "{row}");
    }
}

/// REQ: F-030, F-034
#[test]
fn doc_034_traceability_names_the_icon_and_mode_config_keys() {
    let traceability = traceability_text();
    let row30 = traceability
        .lines()
        .find(|line| line.starts_with("| F-030 |"))
        .expect("row");
    assert!(row30.contains("[ui] agent_monitoring_mode; [detection] custom_signatures"));
    let row34 = traceability
        .lines()
        .find(|line| line.starts_with("| F-034 |"))
        .expect("row");
    assert!(row34.contains(
        "[ui] claude_agent_icon, codex_agent_icon, antigravity_agent_icon, agent_identifier_mode"
    ));
}

/// REQ: F-026
#[test]
fn doc_026_traceability_sides_server_for_identity_and_both_for_goals() {
    let traceability = traceability_text();
    let row26 = traceability
        .lines()
        .find(|line| line.starts_with("| F-026 |"))
        .expect("row");
    assert!(row26.contains("| server |"));
    let row29 = traceability
        .lines()
        .find(|line| line.starts_with("| F-029 |"))
        .expect("row");
    assert!(row29.contains("| both |"));
    let row34 = traceability
        .lines()
        .find(|line| line.starts_with("| F-034 |"))
        .expect("row");
    assert!(row34.contains("| client |"));
}

/// REQ: F-027
#[test]
fn doc_027_example_package_manifest_is_valid_json_with_the_activity_rule_entry() {
    let page = page_text();
    let start = page
        .find("#### Example: Marks a pane")
        .expect("example heading");
    let rest = &page[start..];
    let open = rest.find("```json\n").expect("json block") + "```json\n".len();
    let close = rest[open..].find("```").expect("end");
    let value: Value = serde_json::from_str(&rest[open..open + close]).expect("JSON");
    let entry = &value["contributes"]["extensions"][0];
    assert_eq!(entry["point"], "ilium.core.agent-detect/activity-rules");
    assert_eq!(entry["id"], "build-waiting");
    assert_eq!(entry["priority"], 50);
    assert_eq!(entry["entry"]["state"], "awaiting-approval");
    assert_eq!(entry["entry"]["region"], "last-lines");
    assert_eq!(
        value["capabilities"]["server"][0],
        "extensions.contribute:ilium.core.agent-detect/activity-rules"
    );
}
