//! Twins of the debug log key tests of ilium-client/src/keys.rs. The keys `r` (toggle the panel resize
//! filter) and `s` (save) become the `view`, `save_prompt` and `save_log` methods of the service
//! `ilium.core.diagnostics/agent-debug`.

use serde_json::{json, Value};

use crate::common::{agent_debug, session};

fn resize_history() -> Vec<Value> {
    vec![
        json!({ "sequence": 1, "at": 1_700_000_000_001_i64, "severity": "information", "source": "detector",
            "kind": "detection-cycle", "summary": "Detected Codex" }),
        json!({ "sequence": 2, "at": 1_700_000_000_002_i64, "severity": "information", "source": "pty",
            "kind": "pane-resized", "summary": "Left panel focus/hover animation", "resize_cause": "tree-panel-animation" }),
    ]
}

/// REQ: F-010, PORT:ilium-client/src/keys.rs::debug_log_r_key_reveals_panel_animation_resize_events
#[test]
fn port_004_r_key_reveals_panel_animation_resize_events() {
    let mut session = session();
    let hidden = agent_debug(
        &mut session,
        "view",
        json!({ "entries": resize_history(), "hide_tree_animation": true }),
    )
    .expect("hidden view");
    let hidden_text = hidden["text"].as_str().expect("text");
    assert!(
        hidden_text.contains("Panel resizes hidden"),
        "{hidden_text}"
    );
    assert!(
        !hidden_text.contains("Left panel focus/hover animation"),
        "{hidden_text}"
    );
    assert_eq!(
        (hidden["visible"].clone(), hidden["hidden"].clone()),
        (json!(1), json!(1))
    );
    let shown = agent_debug(
        &mut session,
        "view",
        json!({ "entries": resize_history(), "hide_tree_animation": false }),
    )
    .expect("shown view");
    let shown_text = shown["text"].as_str().expect("text");
    assert!(shown_text.contains("Panel resizes shown"), "{shown_text}");
    assert!(
        shown_text.contains("Left panel focus/hover animation"),
        "{shown_text}"
    );
    assert_eq!(
        (shown["visible"].clone(), shown["hidden"].clone()),
        (json!(2), json!(0))
    );
}

/// REQ: F-010, PORT:ilium-client/src/keys.rs::debug_log_s_key_opens_path_prompt_and_enter_saves_complete_report
#[test]
fn port_005_s_key_opens_a_log_path_prompt_and_enter_saves_the_complete_report() {
    let mut session = session();
    let prompt = agent_debug(
        &mut session,
        "save_prompt",
        json!({ "pane": 7, "pane_name": "Codex", "has_loaded_retained_history": true }),
    )
    .expect("prompt");
    let path = prompt["path"].as_str().expect("path").to_string();
    assert!(path.starts_with('/'), "{path}");
    assert!(path.ends_with(".log"), "{path}");
    assert!(path.contains("-debug-"), "{path}");
    let parent =
        path.rsplit_once('/').map_or(
            "/",
            |(directory, _)| if directory.is_empty() { "/" } else { directory },
        );
    agent_debug(&mut session, "make_directory", json!({ "path": parent })).expect("mkdir");
    let saved = agent_debug(
        &mut session,
        "save_log",
        json!({ "pane": 7, "pane_name": "Codex", "path": path, "entries": resize_history(), "hide_tree_animation": false }),
    );
    let reply = saved.expect("save_log");
    assert_eq!(reply["saved"], true);
    let report = agent_debug(&mut session, "read_file", json!({ "path": path })).expect("read")
        ["text"]
        .as_str()
        .expect("text")
        .to_string();
    assert!(report.starts_with("ILIUM AGENT DEBUG LOG\n"), "{report}");
    assert!(
        report.contains("Left panel focus/hover animation"),
        "{report}"
    );
    assert!(report.contains("Detected Codex"), "{report}");
}
