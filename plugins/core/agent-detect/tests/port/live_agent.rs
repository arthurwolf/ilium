//! Twins of the live-agent pipeline tests that the plugin can observe: identity then activity then decorations.

use ilium_plugin_sdk::types::{
    AgentActivity, AgentActivityEvent, AgentIdentifiedEvent, Event, NodeId,
};

use crate::common::{decorations, goal, identify_argv, plain, server_host};

/// REQ: PORT:ilium-server/tests/live_agent_detection.rs::a_real_process_named_codex_preserves_its_pursuing_goal_status_through_the_whole_pipeline
#[test]
fn port_026_a_process_named_codex_flows_from_identity_to_activity_decoration() {
    let mut host = server_host();
    assert_eq!(
        identify_argv(&["/tmp/fixtures/codex"]),
        Some("codex".to_string())
    );
    host.deliver_one(Event::AgentIdentified(AgentIdentifiedEvent {
        node: NodeId(7),
        signature: "codex".to_string(),
    }));
    let submissions = host.deliver_one(Event::AgentActivityChanged(AgentActivityEvent {
        node: NodeId(7),
        previous: AgentActivity::Idle,
        current: AgentActivity::Working,
        revision: 2,
    }));
    let specs = decorations(&submissions, "activity");
    assert_eq!(specs.len(), 1);
    assert_eq!(plain(&specs[0].frames[0]), "⠋");
}

const CODEX_FOOTER_PREFIX: &str =
    "• Done\n\n› Ask Codex to do anything\n\n  model · workspace · Ready · Conte… ";
const CODEX_FOOTER_SUFFIX: &str = "    ⚠ 1 warning · f2 to view";

/// REQ: PORT:ilium-server/tests/live_agent_detection.rs::a_real_process_named_codex_preserves_its_pursuing_goal_status_through_the_whole_pipeline
#[test]
fn port_026_pursuing_goal_footer_reaches_the_goal_slot_of_a_codex_pane() {
    let footer = format!("{CODEX_FOOTER_PREFIX}Pursuing goal (5m){CODEX_FOOTER_SUFFIX}");
    assert!(goal("codex", &footer).is_some());
}
