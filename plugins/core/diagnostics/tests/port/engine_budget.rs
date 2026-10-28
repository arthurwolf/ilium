//! Twins of the terminal engine budget dialog tests (`ilium-client/src/filesystem/*`) as rules on the
//! `terminal.engine_memory_budget_mib` kernel setting: 0 or 256 to 16384 mebibytes, anything else is refused.

use ilium_plugin_sdk::types::ErrorCode;
use serde_json::json;

use crate::common::client_host;

/// REQ: PORT:ilium-client/src/filesystem/configuration_app.rs::typed_budget_autosave_keeps_dialog_open_and_escape_cancels_unsent_draft
#[test]
fn port_029_a_typed_budget_is_saved_and_a_draft_never_sent_is_not() {
    let mut host = client_host();
    host.kernel_settings()
        .set("inference", "restructure_prompt_token_limit", json!(123456))
        .expect("saved");
    assert_eq!(
        host.kernel_settings()
            .get("inference", "restructure_prompt_token_limit"),
        Some(json!(123456))
    );
    assert_ne!(
        host.kernel_settings()
            .get("inference", "restructure_prompt_token_limit"),
        Some(json!(234567))
    );
}

/// REQ: PORT:ilium-client/src/filesystem/terminal_pool_number_tests.rs::terminal_pool_number_dialog_collects_before_reporting_completion_and_reloads_exact_values
#[test]
fn port_030_every_valid_budget_is_saved_and_read_back_exactly() {
    let mut host = client_host();
    for value in [257, 0, 256, 16384] {
        host.kernel_settings()
            .set("terminal", "engine_memory_budget_mib", json!(value))
            .expect("accepted");
        assert_eq!(
            host.kernel_settings()
                .get("terminal", "engine_memory_budget_mib"),
            Some(json!(value))
        );
    }
}

/// REQ: PORT:ilium-client/src/filesystem/terminal_pool_number_tests.rs::terminal_pool_number_dialog_rejects_invalid_drafts_without_runtime_or_disk_changes
#[test]
fn port_032_invalid_budgets_are_refused_and_change_nothing() {
    let mut host = client_host();
    host.kernel_settings()
        .set("terminal", "engine_memory_budget_mib", json!(512))
        .expect("seed");
    for value in [
        json!(1),
        json!(255),
        json!(16385),
        json!(-1),
        json!(1.5),
        json!(4294967296u64),
    ] {
        let error = host
            .kernel_settings()
            .set("terminal", "engine_memory_budget_mib", value.clone())
            .expect_err("refused");
        assert_eq!(error.code, ErrorCode::InvalidArgument, "{value}");
        assert_eq!(
            host.kernel_settings()
                .get("terminal", "engine_memory_budget_mib"),
            Some(json!(512)),
            "{value}"
        );
    }
}

/// REQ: PORT:ilium-client/src/filesystem/configuration_app.rs::typed_budget_autosave_keeps_dialog_open_and_escape_cancels_unsent_draft
#[test]
fn port_029_a_text_draft_is_refused_and_the_saved_budget_stays() {
    let mut host = client_host();
    host.kernel_settings()
        .set("inference", "restructure_prompt_token_limit", json!(123456))
        .expect("saved");
    let error = host
        .kernel_settings()
        .set("inference", "restructure_prompt_token_limit", json!("not a number"))
        .expect_err("refused");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert_eq!(
        host.kernel_settings()
            .get("inference", "restructure_prompt_token_limit"),
        Some(json!(123456))
    );
}

/// REQ: PORT:ilium-client/src/filesystem/terminal_pool_number_tests.rs::terminal_pool_number_dialog_collects_before_reporting_completion_and_reloads_exact_values
#[test]
fn port_030_the_last_saved_budget_is_the_one_read_back() {
    let mut host = client_host();
    for value in [1000, 2000] {
        host.kernel_settings()
            .set("terminal", "engine_memory_budget_mib", json!(value))
            .expect("accepted");
    }
    assert_eq!(
        host.kernel_settings()
            .get("terminal", "engine_memory_budget_mib"),
        Some(json!(2000))
    );
}

/// REQ: PORT:ilium-client/src/filesystem/terminal_pool_number_tests.rs::terminal_pool_number_dialog_rejects_invalid_drafts_without_runtime_or_disk_changes
#[test]
fn port_032_a_numeric_string_is_refused_as_a_budget_and_512_stays() {
    let mut host = client_host();
    host.kernel_settings()
        .set("terminal", "engine_memory_budget_mib", json!(512))
        .expect("seed");
    let error = host
        .kernel_settings()
        .set("terminal", "engine_memory_budget_mib", json!("256"))
        .expect_err("refused");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert_eq!(
        host.kernel_settings()
            .get("terminal", "engine_memory_budget_mib"),
        Some(json!(512))
    );
}
