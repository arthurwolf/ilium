//! Twin of the cost-related test in ilium-client/src/ui.rs: late layers and the scene-history withholding.

use serde_json::json;

use crate::common::client;

/// REQ: PORT:ilium-client/src/ui.rs::late_modal_and_suspended_parent_withhold_scene_history
#[test]
fn port_ui_late_modal_and_suspended_parent_withhold_scene_history() {
    let transparent = |mode: &str, stacked: Option<&str>| {
        client(
            "ui.late-layers-source-transparent",
            json!({ "mode": mode, "modal_stack": stacked.map(|mode| vec![mode]).unwrap_or_default() }),
        )
    };
    assert_eq!(transparent("normal", None), json!(true));
    assert_eq!(transparent("help", None), json!(false));
    assert_eq!(transparent("normal", Some("help")), json!(false));
}

/// REQ: PORT:ilium-client/src/ui.rs::late_modal_and_suspended_parent_withhold_scene_history
#[test]
fn port_ui_help_mode_withholds_history_even_under_a_normal_modal_stack() {
    let reply = client(
        "ui.late-layers-source-transparent",
        json!({ "mode": "help", "modal_stack": ["normal"] }),
    );
    assert_eq!(reply, json!(false));
}
