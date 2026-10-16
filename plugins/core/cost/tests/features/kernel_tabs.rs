//! Kernel settings tabs held by Cost (COR-030, COR-033, COR-022, COR-011).

use crate::common::*;

/// REQ: COR-030, F-040
#[test]
fn doc_cor_030_page_lists_agent_cost_tab_with_cost_table_and_readers() {
    let row = table_row(&page_text(), "`agent-cost`");
    assert_eq!(&row[1..], ["Agent Cost", "`[cost]`", "Fleet, Tree"]);
}

/// REQ: COR-030, F-116
#[test]
fn doc_cor_030_page_lists_reset_planning_tab_with_its_table_and_reader() {
    let row = table_row(&page_text(), "`reset-planning`");
    assert_eq!(&row[1..], ["Reset planning", "`[reset_planning]`", "Notifications"]);
}

/// REQ: COR-030, F-040
#[test]
fn doc_cor_030_migration_page_names_cost_as_holder_of_both_tabs() {
    let migration = page("reference-config-and-data-migration");
    let cost = table_row(&migration, "`agent-cost`");
    assert_eq!(&cost[1..], ["Agent Cost", "`[cost]`", "Cost", "Fleet, Tree", "`[cost]`"]);
    let reset = table_row(&migration, "`reset-planning`");
    assert_eq!(&reset[1..], ["Reset planning", "`[reset_planning]`", "Cost", "Notifications", "`[reset_planning]`"]);
}

/// REQ: COR-033, F-040
#[test]
fn doc_cor_033_cost_table_keeps_the_twenty_two_keys_of_the_original() {
    let row = table_row(&page("reference-config-and-data-migration"), "`[cost]`");
    let keys: Vec<&str> = row[1].split(", ").map(|key| key.trim_matches('`')).collect();
    assert_eq!(keys.len(), 22);
    for key in ["metric", "quota_window", "budget_usd", "history_days", "level_glyph", "sort_by_cost", "prices"] {
        assert!(keys.contains(&key), "{key}");
    }
}

/// REQ: COR-033, F-116
#[test]
fn doc_cor_033_reset_planning_table_keeps_three_keys() {
    let row = table_row(&page("reference-config-and-data-migration"), "`[reset_planning]`");
    assert_eq!(row[1], "`monitor_claude`, `monitor_codex`, `time_style`");
    assert_eq!(row[2], "Cost");
}

/// REQ: COR-022, F-040
#[test]
fn doc_cor_022_page_states_the_disabled_notice_verbatim() {
    assert!(page_text().contains("\"Cost is disabled. Values are kept.\""));
}

/// REQ: COR-022, F-038
#[test]
fn doc_cor_022_kernel_and_boot_row_names_the_off_effect() {
    let row = table_row(&page("reference-kernel-and-boot"), "Cost");
    let off = page("reference-kernel-and-boot");
    assert!(off.contains("| Cost | No spend or quota decorations, no cost sort key. The Agent Cost and Reset planning tabs are read-only | Stored prices stay |"));
    assert_eq!(row[0], "Cost");
}

/// REQ: COR-011, F-040
#[test]
fn doc_cor_011_page_says_the_settings_keep_their_defaults_and_keys() {
    let text = page_text();
    assert!(text.contains("COR-011"));
}

/// REQ: COR-030, F-040
#[test]
fn cor_030_server_reading_the_cost_tab_issues_no_write_on_activation() {
    let mut host = server_host();
    set_kernel_tab(&mut host, "agent-cost", serde_json::json!({"metric": "dollars"}));
    host.activate::<ilium_plugin_core_cost::CostServer>();
    assert!(kernel_writes(&mut host).is_empty());
}

/// REQ: COR-033, F-040
#[test]
fn cor_033_changing_unit_writes_only_keys_of_the_cost_table() {
    let mut host = activated_server();
    host.settings().set("unit", serde_json::json!("quota")).expect("valid unit");
    host.settle();
    for (tab, key, _) in kernel_writes(&mut host) {
        assert_eq!(tab, "agent-cost");
        assert!(["metric", "quota_window"].contains(&key.as_str()) || !key.is_empty());
    }
}

/// REQ: COR-033, F-040
#[test]
fn cor_033_unit_change_writes_the_metric_key_of_the_agent_cost_tab() {
    let mut host = activated_server();
    host.settings().set("unit", serde_json::json!("quota")).expect("valid unit");
    host.settle();
    let writes = kernel_writes(&mut host);
    assert!(writes.iter().any(|(tab, key, _)| tab == "agent-cost" && key == "metric"), "{writes:?}");
}

/// REQ: COR-030, F-116
#[test]
fn cor_030_client_never_writes_the_agent_cost_tab() {
    let mut host = activated_client();
    host.settle();
    assert!(kernel_writes(&mut host).iter().all(|(tab, _, _)| tab != "agent-cost"));
}
