//! The `[ui]` settings of the toolbar: defaults, validation, live effect (reference-config-and-data-migration.md "[ui] toolbar").

use ilium_plugin_sdk::types::{AgentActivity, ErrorCode};
use proptest::prelude::*;
use serde_json::{json, Value};

use crate::common::*;

const MIN_LINES: i64 = 1;
const MAX_LINES: i64 = 20;

fn set_lines(value: Value) -> Result<(), ilium_plugin_sdk::types::PluginError> {
    client_host().settings().set("ui.last_prompt_max_lines", value)
}

/// REQ: F-035, COR-011
#[test]
fn f_035_agent_toolbar_enabled_defaults_to_on() {
    assert_eq!(client_host().settings().get("ui.agent_toolbar_enabled"), Some(json!(true)));
}

/// REQ: F-035, COR-011
#[test]
fn f_035_show_toolbar_labels_defaults_to_on() {
    assert_eq!(client_host().settings().get("ui.show_toolbar_labels"), Some(json!(true)));
}

/// REQ: F-033, COR-011
#[test]
fn f_033_last_prompt_enabled_defaults_to_on() {
    assert_eq!(client_host().settings().get("ui.last_prompt_enabled"), Some(json!(true)));
}

/// REQ: F-033, COR-011
#[test]
fn f_033_last_prompt_max_lines_defaults_to_4() {
    assert_eq!(client_host().settings().get("ui.last_prompt_max_lines"), Some(json!(4)));
}

/// REQ: F-033
#[test]
fn f_033_max_lines_of_1_is_accepted() {
    assert!(set_lines(json!(MIN_LINES)).is_ok());
}

/// REQ: F-033
#[test]
fn f_033_max_lines_of_20_is_accepted() {
    assert!(set_lines(json!(MAX_LINES)).is_ok());
}

/// REQ: F-033
#[test]
fn f_033_max_lines_of_0_is_refused_as_invalid_argument() {
    let error = set_lines(json!(0)).expect_err("0 is below the minimum");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

/// REQ: F-033
#[test]
fn f_033_max_lines_of_21_is_refused_as_invalid_argument() {
    let error = set_lines(json!(21)).expect_err("21 is above the maximum");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

/// REQ: F-033
#[test]
fn f_033_out_of_range_message_names_the_key_value_and_range() {
    let error = set_lines(json!(21)).expect_err("refused");
    assert_eq!(error.message, "ui.last_prompt_max_lines = 21 must be between 1 and 20");
    let error = set_lines(json!(0)).expect_err("refused");
    assert_eq!(error.message, "ui.last_prompt_max_lines = 0 must be between 1 and 20");
}

/// REQ: F-033
#[test]
fn f_033_max_lines_rejects_a_negative_number_a_fraction_and_a_string() {
    for value in [json!(-1), json!(4.5), json!("4"), json!(null), json!(true), json!(256)] {
        let error = set_lines(value.clone()).expect_err("must be refused");
        assert_eq!(error.code, ErrorCode::InvalidArgument, "{value}");
    }
}

/// REQ: F-033
#[test]
fn f_033_a_refused_value_leaves_the_previous_one_in_place() {
    let mut host = client_host();
    host.settings().set("ui.last_prompt_max_lines", json!(7)).expect("accepted");
    let _ = host.settings().set("ui.last_prompt_max_lines", json!(99));
    assert_eq!(host.settings().get("ui.last_prompt_max_lines"), Some(json!(7)));
}

/// REQ: F-033
#[test]
fn f_033_an_accepted_value_reads_back_under_the_same_key() {
    let mut host = client_host();
    host.settings().set("ui.last_prompt_max_lines", json!(12)).expect("accepted");
    assert_eq!(host.settings().get("ui.last_prompt_max_lines"), Some(json!(12)));
}

/// REQ: F-033, F-035
#[test]
fn f_035_boolean_settings_reject_non_boolean_values() {
    let mut host = client_host();
    for key in ["ui.agent_toolbar_enabled", "ui.show_toolbar_labels", "ui.last_prompt_enabled"] {
        for value in [json!(1), json!("true"), json!(null)] {
            let error = host.settings().set(key, value.clone()).expect_err("must be refused");
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{key} {value}");
        }
    }
}

/// REQ: F-033, F-035
#[test]
fn f_035_boolean_settings_accept_true_and_false() {
    let mut host = client_host();
    for key in ["ui.agent_toolbar_enabled", "ui.show_toolbar_labels", "ui.last_prompt_enabled"] {
        for value in [false, true] {
            host.settings().set(key, json!(value)).expect("accepted");
            assert_eq!(host.settings().get(key), Some(json!(value)));
        }
    }
}

/// REQ: F-033
#[test]
fn f_033_turning_the_banner_off_stops_prompt_slot_updates() {
    let mut host = client_host();
    let pane = add_agent_pane(&mut host, "claude", "claude", AgentActivity::Working);
    host.settings().set("ui.last_prompt_enabled", json!(false)).expect("accepted");
    assert!(set_slots(&host.deliver_one(user_line(pane, "hidden"))).is_empty());
}

/// REQ: F-033
#[test]
fn f_033_turning_the_banner_back_on_restores_prompt_slot_updates() {
    let mut host = client_host();
    let pane = add_agent_pane(&mut host, "claude", "claude", AgentActivity::Working);
    host.settings().set("ui.last_prompt_enabled", json!(false)).expect("accepted");
    host.settings().set("ui.last_prompt_enabled", json!(true)).expect("accepted");
    assert_eq!(set_slots(&host.deliver_one(user_line(pane, "shown"))).len(), 1);
}

/// REQ: F-033
#[test]
fn f_033_the_banner_switch_is_independent_of_the_toolbar_switch() {
    let mut host = client_host();
    let pane = add_agent_pane(&mut host, "claude", "claude", AgentActivity::Working);
    host.settings().set("ui.agent_toolbar_enabled", json!(false)).expect("accepted");
    assert_eq!(set_slots(&host.deliver_one(user_line(pane, "still shown"))).len(), 1);
}

proptest! {
    // REQ: F-033
    #[test]
    fn f_033_property_max_lines_is_accepted_exactly_between_1_and_20(lines in -5i64..300) {
        let result = set_lines(json!(lines));
        prop_assert_eq!(result.is_ok(), (MIN_LINES..=MAX_LINES).contains(&lines));
    }
}
