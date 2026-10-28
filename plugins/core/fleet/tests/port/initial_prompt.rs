//! Twins of `ilium-server/src/initial_prompt.rs`. The bracketed-paste framing, the line-ending
//! normalisation and the stripping of embedded paste markers belong to the kernel (terminal-write
//! and send-input pages). The plugin-visible contract is that `POST /create_agent` hands the prompt
//! to `create-pane` as `initial_input`, byte for byte, and lets the kernel frame it.

use ilium_plugin_sdk::testing::FakeHost;
use serde_json::json;

use crate::common::*;

fn initial_input_for(prompt: &str) -> Option<String> {
    let mut host: FakeHost = server_host();
    let body = json!({"agent_type": "claude", "project": "/home/me/dev/app", "prompt": prompt});
    host.deliver_one(http_call(
        1,
        "POST",
        "/create_agent",
        body.to_string().as_bytes(),
    ));
    host.settle();
    let specs = created_panes(&host);
    assert_eq!(specs.len(), 1, "one create-pane expected for {prompt:?}");
    specs[0].initial_input.clone()
}

/// REQ: PORT:ilium-server/src/initial_prompt.rs::single_line_initial_prompt_is_written_verbatim
#[test]
fn initial_prompt_single_line_initial_prompt_is_written_verbatim() {
    assert_eq!(
        initial_input_for("/goal fix it").as_deref(),
        Some("/goal fix it")
    );
}

/// REQ: PORT:ilium-server/src/initial_prompt.rs::multiline_initial_prompt_is_one_bracketed_paste_body
#[test]
fn initial_prompt_multiline_initial_prompt_is_one_bracketed_paste_body() {
    assert_eq!(
        initial_input_for("line one\nline two").as_deref(),
        Some("line one\nline two")
    );
}

/// REQ: PORT:ilium-server/src/initial_prompt.rs::single_line_initial_prompt_is_written_verbatim
#[test]
fn initial_prompt_single_line_keeps_tabs_and_inner_spacing() {
    assert_eq!(initial_input_for("a\tb  c").as_deref(), Some("a\tb  c"));
}

/// REQ: PORT:ilium-server/src/initial_prompt.rs::multiline_initial_prompt_is_one_bracketed_paste_body
#[test]
fn initial_prompt_multiline_keeps_blank_lines_inside_the_body() {
    assert_eq!(initial_input_for("a\n\nb").as_deref(), Some("a\n\nb"));
}
