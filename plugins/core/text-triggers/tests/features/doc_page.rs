//! Rules the page and the reference pages state in tables and prose: the kernel tab, the config table, the boot
//! trigger, the traceability row, the security marks of the extension points (SEC-293) and the samples.

use crate::manual::{manual_page, page_text, table_after};

/// REQ: COR-030, COR-033
#[test]
fn doc_030_kernel_tabs_table_lists_the_text_triggers_tab_with_its_config_table() {
    let rows = table_after(&page_text(), "### Kernel tabs this plugin owns");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(
        rows[0],
        [
            "`text-triggers`",
            "Text Triggers",
            "`[text_triggers]`",
            "none"
        ]
    );
}

/// REQ: COR-030
#[test]
fn doc_030_page_requires_one_holder_per_tab_for_t1_only() {
    let page = page_text();
    assert!(page.contains(
        "It holds `settings.write-kernel:<tab>` for each one (T1 only, one holder per tab"
    ));
}

/// REQ: COR-033, COR-011
#[test]
fn doc_033_page_keeps_the_values_in_the_same_config_tables() {
    let page = page_text();
    assert!(page.contains(
        "The values stay in the same `config.toml` tables as before, so no user setting moves."
    ));
}

/// REQ: COR-033
#[test]
fn doc_033_migration_page_lists_the_text_triggers_table_as_server_owned() {
    let rows = table_after(
        &manual_page("reference-config-and-data-migration.md"),
        "| `[git]` |",
    );
    let _ = rows;
    let page = manual_page("reference-config-and-data-migration.md");
    assert!(page.contains("| `[text_triggers]` | rule list | Text triggers | yes (server-owned) |"));
}

/// REQ: COR-033, COR-011
#[test]
fn doc_033_migration_page_maps_the_tab_to_the_config_table() {
    let page = manual_page("reference-config-and-data-migration.md");
    assert!(page.contains("| `text-triggers` | Text Triggers | `[text_triggers]` | Text triggers | none | `[text_triggers]` |"));
}

/// REQ: COR-030
#[test]
fn doc_030_settings_schema_places_the_tab_21st_in_the_main_page() {
    let page = manual_page("reference-settings-schema.md");
    assert!(page.contains("| 21 | `text-triggers` | Text Triggers | yes, `main` |"));
}

/// REQ: COR-022, COR-050
#[test]
fn doc_022_kernel_boot_table_activates_at_startup_with_the_rules_view() {
    let page = manual_page("reference-kernel-and-boot.md");
    assert!(page.contains(
        "| Text triggers | `startup` | `view:text-triggers.rules` | The output filter must be in place before the first byte |"
    ));
}

/// REQ: F-056, COR-011
#[test]
fn doc_056_traceability_row_assigns_the_feature_to_this_plugin_on_the_server() {
    let page = manual_page("reference-feature-traceability.md");
    let row = page
        .lines()
        .find(|line| line.starts_with("| F-056 |"))
        .expect("F-056 row");
    assert_eq!(
        row,
        "| F-056 | Text triggers | `automation` | primitive-extended | text-triggers plugin | server | terminal.output-filter, terminal.write | [text_triggers] | none | 4a | COR-011 |"
    );
}

/// REQ: F-056
#[test]
fn doc_056_the_page_says_the_host_filter_does_the_matching() {
    let page = page_text();
    assert!(page.contains("The pattern matching itself is done by the host filter, so the plugin never reads the full output stream."));
    assert!(page.contains("The plugin turns each rule into one output filter and answers each `output-matched` event with a `send-input` intent."));
}

/// REQ: F-056
#[test]
fn doc_056_the_page_lists_the_runs_on_both_sides_statement() {
    assert!(page_text().contains("it runs on both sides: the server side matches output and sends input, the client side draws the Text Triggers settings tab."));
}

/// REQ: SEC-293
#[test]
fn doc_293_the_security_section_defers_to_the_corrected_risk_table() {
    let page = page_text();
    assert!(page.contains("Risk marks of this plugin's extension points follow the corrected table in [Extension security](/pages/reference-extension-security/) ([SEC-293])."));
}

/// REQ: SEC-293
#[test]
fn doc_293_before_fire_is_high_risk_on_the_page_and_in_the_corrected_table() {
    let rows = table_after(&page_text(), "| Point | Kind |");
    let row = rows
        .iter()
        .find(|row| row[0] == "`before-fire`")
        .expect("row");
    assert_eq!(row[4], "high");
    let security = manual_page("reference-extension-security.md");
    assert!(security
        .contains("| `ilium.core.text-triggers/before-fire` | high | Can stop a rule that is about to send text to a terminal |"));
}

/// REQ: SEC-293
#[test]
fn doc_293_rule_packs_is_high_risk_in_the_corrected_table_and_so_on_the_page() {
    let security = manual_page("reference-extension-security.md");
    assert!(security.contains(
        "| `ilium.core.text-triggers/rule-packs` | high | Rules send text to terminals |"
    ));
    let rows = table_after(&page_text(), "| Point | Kind |");
    let row = rows
        .iter()
        .find(|row| row[0] == "`rule-packs`")
        .expect("row");
    assert_eq!(
        row[4], "high",
        "the page repeats the corrected mark (SEC-293)"
    );
}

/// REQ: F-056
#[test]
fn doc_056_core_extension_points_index_counts_two_points_one_service_one_topic_replaceable() {
    let page = manual_page("reference-core-extension-points.md");
    assert!(page.contains("| [Text triggers](/pages/core-plugin-text-triggers/) | `ilium.core.text-triggers` | 2 | 1 | 1 | yes |"));
}

/// REQ: F-056
#[test]
fn doc_056_samples_use_two_filters_per_second_and_a_32_rule_limit() {
    let page = page_text();
    assert!(page.contains("rate_per_second: 2,"));
    assert!(page.contains("\"max_items\": 32, \"reorderable\": true, \"default\": [] }"));
    assert!(page.contains("\"item_format\": \"regex\""));
}
