//! Port twins of ilium-server/src/agent_prompt.rs. The kernel rebuilds the composer text from the
//! bytes written to an agent pane and records it as the pane's last prompt only when it can vouch
//! for every byte. A plugin sees the result through `last-prompt` (primitives-agent.md,
//! capability-agent-read-prompts.md): `some(text)` is the exact prompt, `none` means the kernel
//! has no exact prompt. The manual does not list which edits are exact (see API_GAPS.md), so
//! these twins pin the behaviour of the original tests, one edit sequence each.

use ilium_plugin_api::generated::primitives::agent::HostAgent;
use ilium_plugin_api::generated::types::ids::NodeId;
use ilium_plugin_api::generated::types::tree::AgentActivity;

use crate::common::*;

const AGENT_PANE: NodeId = NodeId(20);
const GRANTS: &[&str] = &["agent.read", "agent.read-prompts"];

fn rig() -> Rig {
    let rig = server_rig(GRANTS);
    let mut tree = standard_tree();
    tree.push(agent_node(AGENT_PANE, "claude", AgentActivity::Idle));
    rig.kernel.set_tree(tree);
    rig
}

fn type_bytes(rig: &Rig, bytes: &[u8]) {
    rig.kernel.user_types_into(AGENT_PANE, bytes);
}

fn last_prompt(rig: &Rig) -> Option<String> {
    rig.host.agent().last_prompt(AGENT_PANE)
}

/// REQ: PORT:ilium-server/src/agent_prompt.rs::automated_edit_invalidates_current_prompt_only
#[test]
fn port_901_automated_edit_invalidates_current_prompt_only() {
    let rig = rig();
    type_bytes(&rig, b"human ");
    rig.kernel.automation_writes_into(AGENT_PANE, b"automatic");
    type_bytes(&rig, b"\r");
    assert_eq!(last_prompt(&rig), None);
    type_bytes(&rig, b"next\r");
    assert_eq!(last_prompt(&rig).as_deref(), Some("next"));
}

/// REQ: PORT:ilium-server/src/agent_prompt.rs::moving_across_unicode_or_multiline_text_is_unavailable
#[test]
fn port_902_moving_across_unicode_or_multiline_text_is_unavailable() {
    for text in ["e\u{301}", "a\nb"] {
        let rig = rig();
        type_bytes(&rig, format!("\x1b[200~{text}\x1b[201~").as_bytes());
        type_bytes(&rig, b"\x1b[D");
        type_bytes(&rig, b"\r");
        assert_eq!(last_prompt(&rig), None, "cursor moved across {text:?}");
        type_bytes(&rig, b"after\r");
        assert_eq!(
            last_prompt(&rig).as_deref(),
            Some("after"),
            "next prompt after {text:?}"
        );
    }
}

/// REQ: PORT:ilium-server/src/agent_prompt.rs::backspace_after_mixed_unicode_ascii_fails_closed_without_panicking
#[test]
fn port_903_backspace_after_mixed_unicode_ascii_fails_closed_without_panicking() {
    let rig = rig();
    type_bytes(&rig, "éx".as_bytes());
    type_bytes(&rig, b"\x7f\r");
    assert_eq!(last_prompt(&rig), None);
    type_bytes(&rig, b"still alive\r");
    assert_eq!(last_prompt(&rig).as_deref(), Some("still alive"));
}

/// REQ: PORT:ilium-server/src/agent_prompt.rs::split_utf8_character_and_crlf_are_one_submission
#[test]
fn port_904_split_utf8_character_and_crlf_are_one_submission() {
    let rig = rig();
    type_bytes(&rig, &[0xC3]);
    assert_eq!(last_prompt(&rig), None);
    type_bytes(&rig, &[0xA9, b'\r']);
    assert_eq!(last_prompt(&rig).as_deref(), Some("é"));
    // The line feed that completes the CRLF is not a second submission.
    type_bytes(&rig, b"\n");
    assert_eq!(last_prompt(&rig).as_deref(), Some("é"));
}

/// REQ: PORT:ilium-server/src/agent_prompt.rs::control_between_utf8_chunks_fails_closed
#[test]
fn port_905_control_between_utf8_chunks_fails_closed() {
    let rig = rig();
    type_bytes(&rig, &[0xc3]);
    type_bytes(&rig, &[0x01, 0xa9]);
    type_bytes(&rig, b"\r");
    assert_eq!(last_prompt(&rig), None);
    type_bytes(&rig, b"clean\r");
    assert_eq!(last_prompt(&rig).as_deref(), Some("clean"));
}

/// REQ: PORT:ilium-server/src/agent_prompt.rs::terminal_owned_history_remains_unavailable_after_ctrl_u
#[test]
fn port_906_terminal_owned_history_remains_unavailable_after_ctrl_u() {
    let rig = rig();
    type_bytes(&rig, b"old\x1b[A");
    type_bytes(&rig, b"\r");
    assert_eq!(last_prompt(&rig), None);
    type_bytes(&rig, b"old\t\x15new  ");
    type_bytes(&rig, b"\r");
    assert_eq!(last_prompt(&rig), None);
    type_bytes(&rig, b"next  \r");
    assert_eq!(last_prompt(&rig).as_deref(), Some("next  "));
}

/// REQ: PORT:ilium-server/src/agent_prompt.rs::deleting_non_ascii_grapheme_is_not_called_exact
#[test]
fn port_907_deleting_non_ascii_grapheme_is_not_called_exact() {
    let rig = rig();
    type_bytes(&rig, "café".as_bytes());
    type_bytes(&rig, b"\x7f\r");
    assert_eq!(last_prompt(&rig), None);
}

/// REQ: PORT:ilium-server/src/agent_prompt.rs::long_unicode_multiline_paste_and_trailing_spaces_survive_split_chunks
#[test]
fn port_908_long_unicode_multiline_paste_and_trailing_spaces_survive_split_chunks() {
    let rig = rig();
    let prompt = format!("{}\ncafé  ", "authored text ".repeat(800));
    let bytes = format!("\x1b[200~{prompt}\x1b[201~").into_bytes();
    for chunk in bytes.chunks(3) {
        type_bytes(&rig, chunk);
    }
    assert_eq!(last_prompt(&rig), None);
    type_bytes(&rig, b"\r");
    assert_eq!(last_prompt(&rig).as_deref(), Some(prompt.as_str()));
}
