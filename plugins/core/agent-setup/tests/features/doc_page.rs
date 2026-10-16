//! Rules the page states that are checked by parsing the page and the traceability table, so no id is left
//! untagged: the status table, the file table, the kernel tab table, the extension point table, the markers.

use crate::common::{manual_page, page_text};

/// Rows `(first cell, second cell)` of the first table after `header_start`.
fn table_after(page: &str, header_start: &str) -> Vec<Vec<String>> {
    let start = page.find(header_start).unwrap_or_else(|| panic!("table {header_start} missing"));
    page[start..]
        .lines()
        .skip(2)
        .take_while(|line| line.starts_with('|'))
        .map(|line| line.trim().trim_matches('|').split('|').map(|cell| cell.trim().to_string()).collect())
        .collect()
}

/// REQ: F-063, COR-011
#[test]
fn doc_063_the_status_table_lists_the_five_badges_in_order() {
    let rows = table_after(&page_text(), "| Status badge | Meaning |");
    let names: Vec<&str> = rows.iter().map(|row| row[0].as_str()).collect();
    assert_eq!(
        names,
        ["Set up by Ilium", "Ilium update required", "Newer Ilium instructions detected", "Instruction detected", "Not set up"]
    );
}

/// REQ: F-063
#[test]
fn doc_063_each_status_has_its_documented_meaning() {
    let rows = table_after(&page_text(), "| Status badge | Meaning |");
    assert_eq!(rows[0][1], "The current managed block is present.");
    assert_eq!(rows[1][1], "An older managed block is present. It is refreshed automatically.");
    assert_eq!(rows[2][1], "The file was written by a newer Ilium. It is left untouched.");
    assert_eq!(rows[3][1], "Matching text that Ilium did not write.");
    assert_eq!(rows[4][1], "No block yet.");
}

/// REQ: F-037, F-063
#[test]
fn doc_037_the_scope_table_names_the_edited_files() {
    let rows = table_after(&page_text(), "| Scope | Files edited |");
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0], ["Global (Claude)", "`~/.claude/CLAUDE.md`, or the custom path of that feature"]);
    assert_eq!(rows[1], ["Global (Codex)", "`~/.codex/AGENTS.md`"]);
    assert_eq!(
        rows[2],
        ["Each open project", "`CLAUDE.md` and `AGENTS.md` in the project root, only from the Setup tab"]
    );
}

/// REQ: COR-030, COR-033
#[test]
fn doc_030_the_page_owns_one_kernel_tab_setup_with_the_agent_setup_table() {
    let rows = table_after(&page_text(), "| Tab id | Label | Config table | Readers |");
    assert_eq!(rows, [["`setup`", "Setup", "`[agent_setup]`", "none"]]);
}

/// REQ: COR-022
#[test]
fn doc_022_the_disabled_notice_text_is_stated_on_the_page() {
    assert!(page_text().contains("\"Agent setup is disabled. Values are kept.\""));
}

/// REQ: COR-011
#[test]
fn doc_011_the_page_states_that_this_plugin_is_the_single_owner_of_the_files() {
    let page = page_text();
    assert!(page.contains("The Setup tab belongs to this plugin, and the Chatroom and Progress pages link here"));
    assert!(page.contains("The two are separate actions with this plugin as the single owner of the files."));
}

/// REQ: COR-011, F-063
#[test]
fn doc_011_chatroom_and_progress_pages_link_to_this_page_instead_of_describing_the_tab() {
    let chatroom = manual_page("core-plugin-chatroom.md");
    assert!(chatroom.contains("/pages/core-plugin-agent-setup/"));
    assert!(chatroom.contains("Agent setup owns the Setup tab, both instruction blocks and the hook entries."));
}

/// REQ: F-037, F-063
#[test]
fn doc_063_the_traceability_table_assigns_both_features_to_the_agent_setup_plugin() {
    let table = manual_page("reference-feature-traceability.md");
    let f037 = table.lines().find(|line| line.starts_with("| F-037 ")).expect("F-037 row");
    let f063 = table.lines().find(|line| line.starts_with("| F-063 ")).expect("F-063 row");
    assert!(f037.contains("agent-setup plugin") && f037.contains("fs.write-scoped"), "{f037}");
    assert!(f063.contains("agent-setup plugin") && f063.contains("[agent_setup]"), "{f063}");
    assert!(f037.contains("COR-011") && f063.contains("COR-011"));
}

/// REQ: F-063
#[test]
fn doc_063_the_extension_point_table_has_the_three_points_with_their_kinds_and_risks() {
    let rows = table_after(&page_text(), "| Point | Kind | What it does | Payload | Risk |");
    let compact: Vec<(String, String, String)> =
        rows.iter().map(|row| (row[0].clone(), row[1].clone(), row[4].clone())).collect();
    assert_eq!(
        compact,
        [
            ("`instruction-blocks`".to_string(), "collect".to_string(), "standard".to_string()),
            ("`targets`".to_string(), "collect".to_string(), "standard".to_string()),
            ("`before-install`".to_string(), "hook (veto)".to_string(), "high".to_string()),
        ]
    );
}

/// REQ: F-063
#[test]
fn doc_063_the_page_names_the_provide_and_contribute_grants_of_the_points() {
    let page = page_text();
    assert!(page.contains("(grant `extensions.provide:ilium.core.agent-setup/<point>`)"));
    assert!(page.contains("Contributors need `extensions.contribute:ilium.core.agent-setup/<point>`"));
}

/// REQ: F-063
#[test]
fn doc_063_the_page_lists_the_install_service_with_its_three_methods() {
    assert!(page_text().contains("- `ilium.core.agent-setup/install`: install, remove, status: manage blocks"));
}

/// REQ: F-063
#[test]
fn doc_063_the_page_states_the_marker_lines_of_both_blocks() {
    let page = page_text();
    assert!(page.contains("`<!-- ilium-agent-feature: chatroom -->` before the block"));
    assert!(page.contains("`<!-- /ilium-agent-feature: chatroom -->` after it"));
    assert!(page.contains("`<!-- ilium-agent-feature: progress version=13 -->`"));
}

/// REQ: SEC-293
#[test]
fn doc_293_the_page_points_to_the_corrected_extension_security_risk_table() {
    assert!(page_text().contains("([SEC-293])"));
    assert!(manual_page("reference-requirements-security-policy.md").contains("| SEC-293 |"));
}

/// REQ: F-037
#[test]
fn doc_037_the_page_states_the_default_claude_path_and_the_codex_path() {
    let page = page_text();
    assert!(page.contains("`~/.claude/CLAUDE.md` (the path can be changed)"));
    assert!(page.contains("the global Codex file `~/.codex/AGENTS.md`"));
}

/// REQ: F-063
#[test]
fn doc_063_the_cli_example_is_marked_as_not_part_of_the_current_cli() {
    assert!(page_text().contains("It is not part of the current CLI."));
}
