//! Twins of ilium-client/src/agent_from_line.rs: the prompt a TODO-line click hands to a new
//! terminal pane (F-023, F-114). Dialog layout and provider-button tests are covered by the
//! kernel-owned dialog (see skipped list).

use crate::helpers::{click_event, plain_host};
use ilium_plugin_sdk::generated::types::Intent;

fn prompt_for(file: &str, text: &str, line: u32) -> String {
    let mut host = plain_host(file, Some("rust"), text, &[]);
    let submissions = host.deliver_one(click_event(line, 1));
    assert_eq!(submissions.len(), 1, "one create-pane submission");
    let Intent::CreatePane(spec) = &submissions[0].intent else { return String::new() };
    spec.initial_input.clone().unwrap_or_default()
}

/// REQ: PORT:ilium-client/src/agent_from_line.rs::default_prompt_contains_line_file_and_one_based_line_number
#[test]
fn port_agent_from_line_prompt_contains_line_file_and_one_based_line_number() {
    let prompt = prompt_for("src/main.rs", "fn a() {}\nTODO fix_this();\n", 1);
    assert!(prompt.contains("TODO fix_this();"), "{prompt}");
    assert!(prompt.contains("src/main.rs"), "{prompt}");
    assert!(prompt.contains("at line 2"), "{prompt}");
}

/// REQ: PORT:ilium-client/src/agent_from_line.rs::default_prompt_preserves_source_text_without_template_or_html_interpretation
#[test]
fn port_agent_from_line_prompt_preserves_source_text_verbatim() {
    let line = "TODO {{untrusted}} <tag> & \"quoted\"";
    let prompt = prompt_for("src/main.rs", &format!("{line}\n"), 0);
    assert!(prompt.contains(line), "{prompt}");
    assert!(!prompt.contains("&amp;") && !prompt.contains("&lt;"), "{prompt}");
}
