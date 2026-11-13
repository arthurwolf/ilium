//! Twins of ilium-client/src/value_cost.rs: value-dialog targets for the cost settings rows.
//!
//! Request: `{settings:{...overrides}, row, ...}`; reply carries `ok`, the `settings` after the edit and,
//! for choice dialogs, `options` (count) and `selected_id`.

use serde_json::{json, Value};

use crate::common::client;

fn edit(request: Value) -> Value {
    client("value-cost.edit", request)
}

/// REQ: PORT:ilium-client/src/value_cost.rs::budget_rejects_changed_units_and_keeps_exact_valid_intermediates
#[test]
fn port_value_cost_budget_rejects_changed_units_and_keeps_exact_valid_intermediates() {
    let settings = json!({ "calibration": "budget" });
    let accepted = edit(json!({ "settings": settings, "row": "budget", "set_number": "17.125" }));
    assert_eq!(accepted["ok"], json!(true));
    assert_eq!(accepted["settings"]["budget_usd"], json!(17.125));
    let changed = json!({ "calibration": "budget", "metric": "quota", "budget_usd": 17.125 });
    let rejected = edit(
        json!({ "settings": changed.clone(), "row": "budget", "target_built_with": settings, "set_number": "17.125" }),
    );
    assert_eq!(rejected["ok"], json!(false));
    assert_eq!(rejected["settings"]["budget_usd"], json!(17.125));
    assert_eq!(rejected["settings"]["metric"], json!("quota"));
}

/// REQ: PORT:ilium-client/src/value_cost.rs::custom_thresholds_and_every_preset_are_recoverable_without_cycling
#[test]
fn port_value_cost_custom_thresholds_and_every_preset_are_recoverable_without_cycling() {
    let settings = json!({ "calibration": "fixed_bands", "fixed_cuts": [1.1, 2.2, 3.3, 4.4] });
    let dialog = edit(json!({ "settings": settings.clone(), "row": "fixed_preset" }));
    assert_eq!(
        dialog["options"],
        json!(dialog["preset_count"].as_u64().unwrap_or(0) + 1)
    );
    assert_eq!(dialog["selected_id"], json!("custom"));
    let same = edit(
        json!({ "settings": settings.clone(), "row": "fixed_preset", "set_choice": "custom" }),
    );
    assert_eq!(same["settings"], settings);
    let preset =
        edit(json!({ "settings": settings.clone(), "row": "fixed_preset", "set_choice": "4" }));
    assert_eq!(preset["settings"]["fixed_cuts"], preset["preset_4_cuts"]);
    let back = edit(
        json!({ "settings": preset["settings"].clone(), "row": "fixed_preset", "set_choice": "custom" }),
    );
    assert_eq!(back["ok"], json!(false));
}

/// REQ: PORT:ilium-client/src/value_cost.rs::hidden_rows_invalid_numbers_and_unknown_options_never_mutate
#[test]
fn port_value_cost_hidden_rows_invalid_numbers_and_unknown_options_never_mutate() {
    let settings = json!({});
    assert_eq!(
        edit(json!({ "settings": settings.clone(), "row": "budget" }))["target"],
        json!(false)
    );
    for input in ["0", "3651", "1.5", ""] {
        let reply = edit(
            json!({ "settings": settings.clone(), "row": "history_days", "set_number": input }),
        );
        assert_eq!(reply["ok"], json!(false), "{input}");
        assert_eq!(reply["settings"], settings);
    }
    let reply = edit(
        json!({ "settings": settings.clone(), "row": "visibility:meter", "set_choice": "invalid" }),
    );
    assert_eq!(reply["ok"], json!(false));
    assert_eq!(reply["settings"], settings);
}

/// REQ: PORT:ilium-client/src/value_cost.rs::selectors_offer_the_full_metric_and_calibration_catalogs
#[test]
fn port_value_cost_selectors_offer_the_full_metric_and_calibration_catalogs() {
    let metric = edit(json!({ "settings": {}, "row": "metric", "set_choice": "quota" }));
    assert_eq!(metric["options"], metric["metric_catalog_len"]);
    assert_eq!(metric["settings"]["metric"], json!("quota"));
    let calibration = edit(
        json!({ "settings": metric["settings"].clone(), "row": "calibration", "set_choice": "burn_rate" }),
    );
    assert_eq!(
        calibration["options"],
        calibration["calibration_catalog_len"]
    );
    assert_eq!(calibration["settings"]["calibration"], json!("burn_rate"));
    let unknown = edit(
        json!({ "settings": calibration["settings"].clone(), "row": "calibration", "set_choice": "unknown" }),
    );
    assert_eq!(unknown["ok"], json!(false));
}
