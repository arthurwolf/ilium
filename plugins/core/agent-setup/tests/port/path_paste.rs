//! Twin of the setup path paste test of ilium-client/src/keys.rs. The instruction-file path prompt is the
//! Setup tab's Global file field (page core-plugin-agent-setup.md, "Global file"): a pasted multi-line text is
//! refused with one notice and never submits or leaks trailing keys, a pasted single line is taken as typed.

use ilium_plugin_sdk::testing::FakeHost;
use serde_json::json;

use crate::common::*;
use crate::host_ext::AgentSetupHostExt;

fn saved_path(host: &mut FakeHost) -> Option<serde_json::Value> {
    host.advance_clock(600);
    host.settle();
    host.settings().get("agent_setup.global_file")
}

/// REQ: F-037, PORT:ilium-client/src/keys.rs::setup_path_paste_never_submits_or_leaks_trailing_keys_into_settings
#[test]
fn port_sp_10_a_pasted_second_line_is_refused_and_never_submits_or_leaks_keys() {
    let mut host = server_host();
    host.type_into_field("setup", "global-file", "/tmp/custom-CLAUDE.md\nr");
    assert_eq!(
        saved_path(&mut host),
        Some(json!("~/.claude/CLAUDE.md")),
        "nothing is saved and the r did not restore"
    );
    let notice = host.last_notice().expect("a notice explains the refusal");
    assert_eq!(notice, "The instruction-file path must be one line");
}

/// REQ: F-037, PORT:ilium-client/src/keys.rs::setup_path_paste_never_submits_or_leaks_trailing_keys_into_settings
#[test]
fn port_sp_11_a_pasted_line_with_a_trailing_line_break_fills_the_field_without_submitting() {
    let mut host = server_host();
    host.type_into_field("setup", "global-file", "/tmp/custom-CLAUDE.md\r\n");
    assert_eq!(host.last_notice(), None);
    assert_eq!(saved_path(&mut host), Some(json!("/tmp/custom-CLAUDE.md")));
}
