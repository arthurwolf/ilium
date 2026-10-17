//! The Kanban Board settings tab (manifest rows `card_preview` and `column_width`) and the kernel
//! `[kanban_board]` table it replaces the editing of (COR-030, COR-033).

use ilium_plugin_sdk::types::ErrorCode;
use serde_json::{json, Value};

use crate::common::{client_host_with, manifest, page_text, user_doc};

fn tab() -> Value {
    manifest()["contributes"]["settings"][0].clone()
}

fn row(key: &str) -> Value {
    tab()["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .find(|row| row["key"] == key)
        .unwrap_or_else(|| panic!("row {key}"))
        .clone()
}

/// REQ: F-024
#[test]
fn settings_024_one_client_tab_named_kanban_board() {
    let settings = manifest()["contributes"]["settings"].clone();
    assert_eq!(settings.as_array().expect("array").len(), 1);
    let tab = tab();
    assert_eq!(tab["kind"], "tab");
    assert_eq!(tab["id"], "kanban");
    assert_eq!(tab["title"], "Kanban Board");
    assert_eq!(tab["scope"], "client");
}

/// REQ: F-024
#[test]
fn settings_024_the_tab_has_exactly_two_rows_in_order() {
    let keys: Vec<String> = tab()["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .map(|row| row["key"].as_str().expect("key").to_string())
        .collect();
    assert_eq!(keys, ["card_preview", "column_width"]);
}

/// REQ: F-024
#[test]
fn settings_024_card_preview_is_a_toggle_on_by_default() {
    let row = row("card_preview");
    assert_eq!(row["type"], "toggle");
    assert_eq!(row["title"], "Card preview");
    assert_eq!(row["default"], true);
}

/// REQ: F-024
#[test]
fn settings_024_column_width_is_a_cells_number_from_12_to_60_default_24() {
    let row = row("column_width");
    assert_eq!(row["type"], "number");
    assert_eq!(row["title"], "Column width");
    assert_eq!(row["minimum"], 12);
    assert_eq!(row["maximum"], 60);
    assert_eq!(row["step"], 1);
    assert_eq!(row["unit"], "cells");
    assert_eq!(row["default"], 24);
}

/// REQ: F-024
#[test]
fn settings_024_the_default_width_lies_inside_the_range() {
    let row = row("column_width");
    let default = row["default"].as_i64().expect("default");
    assert!(
        row["minimum"].as_i64().expect("min") <= default
            && default <= row["maximum"].as_i64().expect("max")
    );
}

/// REQ: F-024
#[test]
fn settings_024_plugin_settings_start_at_their_defaults() {
    let mut host = client_host_with(None);
    assert_eq!(host.settings().get("card_preview"), Some(json!(true)));
    assert_eq!(host.settings().get("column_width"), Some(json!(24)));
}

/// REQ: F-024
#[test]
fn settings_024_column_width_accepts_a_value_inside_the_range() {
    let mut host = client_host_with(None);
    host.settings()
        .set("column_width", json!(30))
        .expect("30 is valid");
    assert_eq!(host.settings().get("column_width"), Some(json!(30)));
}

/// REQ: F-024
#[test]
fn settings_024_column_width_accepts_the_minimum_12() {
    let mut host = client_host_with(None);
    host.settings()
        .set("column_width", json!(12))
        .expect("12 is valid");
    assert_eq!(host.settings().get("column_width"), Some(json!(12)));
}

/// REQ: F-024
#[test]
fn settings_024_column_width_accepts_the_maximum_60() {
    let mut host = client_host_with(None);
    host.settings()
        .set("column_width", json!(60))
        .expect("60 is valid");
    assert_eq!(host.settings().get("column_width"), Some(json!(60)));
}

/// REQ: F-024
#[test]
fn settings_024_column_width_11_is_refused_invalid_argument_and_the_value_stays() {
    let mut host = client_host_with(None);
    let error = host
        .settings()
        .set("column_width", json!(11))
        .expect_err("11 is below the minimum");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert_eq!(host.settings().get("column_width"), Some(json!(24)));
}

/// REQ: F-024
#[test]
fn settings_024_column_width_61_is_refused_invalid_argument_and_the_value_stays() {
    let mut host = client_host_with(None);
    let error = host
        .settings()
        .set("column_width", json!(61))
        .expect_err("61 is above the maximum");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert_eq!(host.settings().get("column_width"), Some(json!(24)));
}

/// REQ: F-024
#[test]
fn settings_024_column_width_rejects_a_string() {
    let mut host = client_host_with(None);
    let error = host
        .settings()
        .set("column_width", json!("wide"))
        .expect_err("not a number");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

/// REQ: F-024
#[test]
fn settings_024_card_preview_rejects_a_non_boolean() {
    let mut host = client_host_with(None);
    let error = host
        .settings()
        .set("card_preview", json!("yes"))
        .expect_err("not a boolean");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert_eq!(host.settings().get("card_preview"), Some(json!(true)));
}

/// REQ: F-024
#[test]
fn settings_024_card_preview_can_be_switched_off_and_on() {
    let mut host = client_host_with(None);
    host.settings()
        .set("card_preview", json!(false))
        .expect("valid");
    assert_eq!(host.settings().get("card_preview"), Some(json!(false)));
    host.settings()
        .set("card_preview", json!(true))
        .expect("valid");
    assert_eq!(host.settings().get("card_preview"), Some(json!(true)));
}

/// REQ: F-024
#[test]
fn settings_024_a_null_merge_value_resets_the_row_to_its_default() {
    let mut host = client_host_with(None);
    host.settings()
        .set("column_width", json!(40))
        .expect("valid");
    host.settings()
        .set("column_width", Value::Null)
        .expect("null resets");
    assert_eq!(host.settings().get("column_width"), Some(json!(24)));
}

/// REQ: F-024
#[test]
fn settings_024_the_user_docs_list_the_kernel_table_and_its_keys() {
    let settings = user_doc("settings.md");
    assert!(settings
        .contains("| Kanban Board | Board card preview and column width | `[kanban_board]` |"));
    assert!(
        settings.contains("| `[kanban_board]` | `card_preview_lines`, `minimum_column_width` |")
    );
}

/// REQ: F-024
#[test]
fn settings_024_the_user_docs_give_the_kernel_defaults_and_ranges() {
    let boards = user_doc("editors-and-boards.md");
    assert!(boards.contains(
        "| Card preview lines | `card_preview_lines` | 4 | 1 to 10 wrapped lines per card |"
    ));
    assert!(
        boards.contains("| Minimum column width | `minimum_column_width` | 45 | 10 to 80 cells;")
    );
    assert!(boards.contains("Values outside the range are rejected when the file is read."));
}

/// REQ: F-024, COR-033
#[test]
fn settings_033_the_values_stay_in_the_kernel_config_table_so_no_user_setting_moves() {
    let page = page_text();
    assert!(page.contains(
        "The values stay in the same `config.toml` tables as before, so no user setting moves."
    ));
    assert!(page.contains("| `kanban-board` | Kanban Board | `[kanban_board]` | none |"));
}

/// REQ: F-024
#[test]
fn settings_024_the_migration_page_keeps_both_kernel_keys_in_kanban_board() {
    let path = crate::common::workspace_root()
        .join("docs/plugins-manual/pages/reference-config-and-data-migration.md");
    let page = std::fs::read_to_string(path).expect("migration page");
    assert!(page.contains(
        "| `[kanban_board]` | `card_preview_lines`, `minimum_column_width` | Kanban | yes |"
    ));
}

/// REQ: F-024
#[test]
fn settings_024_a_wider_column_width_redraws_the_board_wider() {
    let (mut host, node) = crate::common::board_host(crate::common::SAMPLE_BOARD);
    host.settings()
        .set("column_width", json!(12))
        .expect("valid");
    host.settings()
        .set("column_width", json!(60))
        .expect("valid");
    host.settings()
        .set("column_width", json!(24))
        .expect("valid");
    let narrow = crate::common::draw(&mut host, node, 120, 20);
    host.settings()
        .set("column_width", json!(40))
        .expect("valid");
    host.settings()
        .set("column_width", json!(60))
        .expect("valid");
    let wide = crate::common::draw(&mut host, node, 120, 20);
    let (_, narrow_doing) = narrow
        .find("Doing")
        .map(|(row, column)| (row, column))
        .expect("Doing header");
    let (_, wide_doing) = wide
        .find("Doing")
        .map(|(row, column)| (row, column))
        .expect("Doing header");
    assert!(
        wide_doing > narrow_doing,
        "Doing starts at {narrow_doing} then {wide_doing}"
    );
}
