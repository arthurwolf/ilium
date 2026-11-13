//! The 25 tabs, their owners and the four tabs this plugin owns (COR-030, COR-033, COR-022).

use crate::common::*;

fn owned_rows() -> Vec<Vec<String>> {
    let page = page_text();
    let start = page
        .find("### Kernel tabs this plugin owns")
        .expect("section");
    table_rows(&page[start..], "| Tab id |")
}

/// REQ: COR-030, F-090
#[test]
fn tabs_030_the_rail_has_25_tabs_at_positions_1_to_25() {
    let rows = tab_rows();
    assert_eq!(rows.len(), 25);
    for (index, row) in rows.iter().enumerate() {
        assert_eq!(row[0], (index + 1).to_string());
    }
}

/// REQ: COR-030, F-090
#[test]
fn tabs_030_rail_order_is_the_order_of_the_user_documentation() {
    let ids: Vec<String> = tab_rows().into_iter().map(|row| row[2].clone()).collect();
    let documentation = user_doc("settings.md");
    let start = documentation
        .find("## Settings tabs at a glance")
        .expect("glance table");
    let documented: Vec<String> = table_rows(&documentation[start..], "| Tab |")
        .into_iter()
        .map(|row| row[0].clone())
        .collect();
    assert_eq!(ids, documented);
}

/// REQ: COR-030, F-090
#[test]
fn tabs_030_tab_ids_are_the_documented_kebab_case_ids_in_order() {
    let ids: Vec<String> = tab_rows()
        .into_iter()
        .map(|row| row[1].trim_matches('`').to_string())
        .collect();
    assert_eq!(
        ids,
        [
            "user-interface",
            "icons",
            "agent-monitoring",
            "agent-cost",
            "optimization",
            "remote-compaction",
            "keyboard",
            "terminal",
            "editor",
            "session",
            "git",
            "kanban-board",
            "animations",
            "sound",
            "voice-control",
            "reset-planning",
            "inference",
            "llm-instructions",
            "titles",
            "triggers",
            "text-triggers",
            "debug",
            "api",
            "about",
            "setup",
        ]
    );
}

/// REQ: COR-030, F-090
#[test]
fn tabs_030_exactly_four_tabs_are_owned_by_the_settings_plugin() {
    let owned: Vec<String> = tab_rows()
        .into_iter()
        .filter(|row| row[3].starts_with("[Settings]"))
        .map(|row| row[1].trim_matches('`').to_string())
        .collect();
    assert_eq!(owned, ["user-interface", "icons", "debug", "about"]);
}

/// REQ: COR-030
#[test]
fn tabs_030_the_owned_tab_table_lists_user_interface_icons_debug_about() {
    let ids: Vec<String> = owned_rows()
        .into_iter()
        .map(|row| row[0].trim_matches('`').to_string())
        .collect();
    assert_eq!(ids, ["user-interface", "icons", "debug", "about"]);
}

/// REQ: COR-030
#[test]
fn tabs_030_the_plugin_holds_one_write_kernel_grant_per_owned_tab() {
    let grants: Vec<String> = client_grants()
        .into_iter()
        .filter_map(|grant| {
            grant
                .strip_prefix("settings.write-kernel:")
                .map(str::to_string)
        })
        .collect();
    assert_eq!(grants, ["user-interface", "icons", "debug", "about"]);
}

/// REQ: COR-030
#[test]
fn tabs_030_every_other_tab_is_written_by_its_owner_plugin_not_by_settings() {
    let granted: Vec<String> = client_grants();
    for row in tab_rows() {
        if row[3].starts_with("[Settings]") {
            continue;
        }
        let tab = row[1].trim_matches('`');
        assert!(
            !granted.contains(&format!("settings.write-kernel:{tab}")),
            "{tab}"
        );
    }
}

/// REQ: COR-033
#[test]
fn tabs_033_config_tables_of_the_owned_tabs() {
    let rows = owned_rows();
    let table = |tab: &str| {
        rows.iter()
            .find(|row| row[0] == format!("`{tab}`"))
            .expect("row")[2]
            .clone()
    };
    assert_eq!(table("user-interface"), "`[ui]`, `[theme]`");
    assert_eq!(table("icons"), "`[ui.icons]`");
    assert_eq!(table("debug"), "`[debug]`");
    assert_eq!(table("about"), "none");
}

/// REQ: COR-033
#[test]
fn tabs_033_readers_of_the_owned_tabs() {
    let rows = owned_rows();
    let readers = |tab: &str| {
        rows.iter()
            .find(|row| row[0] == format!("`{tab}`"))
            .expect("row")[3]
            .clone()
    };
    assert_eq!(
        readers("user-interface"),
        "Tree, Agent toolbar, Titles, Diagnostics, Terminal"
    );
    assert_eq!(readers("icons"), "Tree, Agent toolbar");
    assert_eq!(readers("debug"), "Diagnostics");
    assert_eq!(readers("about"), "none");
}

/// REQ: COR-033
#[test]
fn tabs_033_labels_of_the_owned_tabs_match_the_rail() {
    let rail = tab_rows();
    for row in owned_rows() {
        let rail_row = rail
            .iter()
            .find(|candidate| candidate[1] == row[0])
            .expect("rail row");
        assert_eq!(rail_row[2], row[1]);
    }
}

/// REQ: COR-022
#[test]
fn tabs_022_a_disabled_plugin_keeps_the_tab_with_a_read_only_notice() {
    let page = page_text();
    assert!(page.contains("If the plugin is disabled, the tab stays in the rail with its stored values shown read-only and the notice \"Settings is disabled. Values are kept.\""));
}

/// REQ: COR-011, F-090
#[test]
fn tabs_011_the_owner_plugins_of_other_tabs_are_the_documented_ones() {
    let owners: Vec<(String, String)> = tab_rows()
        .into_iter()
        .map(|row| {
            (
                row[1].trim_matches('`').to_string(),
                row[3]
                    .split(']')
                    .next()
                    .unwrap_or("")
                    .trim_start_matches('[')
                    .to_string(),
            )
        })
        .collect();
    let expect = |tab: &str, owner: &str| {
        assert!(
            owners.contains(&(tab.to_string(), owner.to_string())),
            "{tab} {owner}"
        );
    };
    expect("agent-monitoring", "Agent detect");
    expect("agent-cost", "Cost");
    expect("optimization", "Compaction");
    expect("keyboard", "Keys and help");
    expect("sound", "Notifications");
    expect("api", "Fleet");
    expect("setup", "Agent setup");
    expect("text-triggers", "Text triggers");
}
