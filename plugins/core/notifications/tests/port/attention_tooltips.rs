//! Twin of the Attention-mode tooltip test of `ilium-client/src/ui.rs`: the plugin supplies the reason of the
//! attention mark, and a stalled goal alone never becomes the reason of an approval prompt.

use ilium_plugin_sdk::types::AgentActivity;

use crate::common::*;

/// REQ: PORT:ilium-client/src/ui.rs::attention_mode_hides_tooltips_for_status_slots_without_a_glyph
#[test]
fn port_009_an_approval_prompt_is_marked_with_its_own_reason_and_not_a_stalled_goal_text() {
    let mut host = server_host();
    let node = add_pane(&mut host, "codex-api", Some(AgentActivity::Working));
    let submissions = transition(&mut host, node, AgentActivity::Working, AgentActivity::AwaitingApproval, 1);
    let found = marks(&submissions);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].node, node);
    assert!(!found[0].reason.trim().is_empty());
    assert!(!found[0].reason.contains("Goal stalled"), "{}", found[0].reason);
}
