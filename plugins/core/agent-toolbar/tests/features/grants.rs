//! Grants: a revoked grant refuses the intents that need it (DEV-416), and the manifest requests only what the page lists.

use ilium_plugin_sdk::types::{AgentActivity, ErrorCode, Outcome, PluginSide};

use crate::common::*;

fn is_refused_not_permitted(outcome: &Outcome) -> bool {
    matches!(outcome, Outcome::Refused(error) if error.code == ErrorCode::NotPermitted)
}

/// REQ: F-033
#[test]
fn f_033_without_ui_chrome_the_banner_intent_is_refused_not_permitted() {
    let mut host = client_host();
    let pane = add_agent_pane(&mut host, "claude", "claude", AgentActivity::Working);
    host.revoke("ui.chrome");
    host.deliver_one(user_line(pane, "hello"));
    host.settle();
    let outcomes = host.outcomes();
    assert_eq!(outcomes.len(), 1, "{outcomes:?}");
    assert!(is_refused_not_permitted(&outcomes[0]), "{outcomes:?}");
}

/// REQ: F-033
#[test]
fn f_033_without_ui_chrome_the_clear_and_recover_intents_are_refused_not_permitted() {
    let mut host = client_host();
    let pane = add_agent_pane(&mut host, "claude", "claude", AgentActivity::Working);
    host.revoke("ui.chrome");
    host.deliver_one(user_line(pane, "hello"));
    host.deliver_one(activity(pane, AgentActivity::Working, AgentActivity::Done));
    host.settle();
    let outcomes = host.outcomes();
    assert_eq!(outcomes.len(), 3, "{outcomes:?}");
    assert!(
        outcomes.iter().all(is_refused_not_permitted),
        "{outcomes:?}"
    );
}

/// REQ: F-035
#[test]
fn f_035_without_agent_control_the_server_action_is_refused_not_permitted() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude", "claude", AgentActivity::Idle);
    host.revoke("agent.control");
    host.deliver_one(message_from(
        PluginSide::Client,
        "compact",
        pane.to_string().as_bytes(),
    ));
    host.settle();
    let outcomes = host.outcomes();
    assert_eq!(outcomes.len(), 1, "{outcomes:?}");
    assert!(is_refused_not_permitted(&outcomes[0]), "{outcomes:?}");
}

/// REQ: F-035
#[test]
fn f_035_granting_agent_control_again_lets_the_action_through() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude", "claude", AgentActivity::Idle);
    host.revoke("agent.control");
    host.grant_now("agent.control")
        .expect("the manifest declares agent.control");
    host.deliver_one(message_from(
        PluginSide::Client,
        "compact",
        pane.to_string().as_bytes(),
    ));
    host.settle();
    assert!(
        host.outcomes().iter().all(|outcome| outcome.is_ok()),
        "{:?}",
        host.outcomes()
    );
}

/// REQ: F-035
#[test]
fn f_035_a_grant_outside_the_manifest_cannot_be_added() {
    let mut host = client_host();
    let error = host
        .grant_now("fs.write")
        .expect_err("not declared by the manifest");
    assert_eq!(
        error.code,
        ilium_plugin_sdk::testing::codes::GRANT_NOT_DECLARED
    );
}

/// REQ: F-035
#[test]
fn f_035_the_activation_of_each_side_succeeds_with_the_manifest_grants() {
    let mut server = ilium_plugin_sdk::testing::FakeHost::for_manifest(manifest_path())
        .expect("loads")
        .side("server")
        .build();
    assert!(server
        .try_activate::<ilium_plugin_core_agent_toolbar::AgentToolbarServer>()
        .is_ok());
    let mut client = ilium_plugin_sdk::testing::FakeHost::for_manifest(manifest_path())
        .expect("loads")
        .side("client")
        .build();
    assert!(client
        .try_activate::<ilium_plugin_core_agent_toolbar::AgentToolbarClient>()
        .is_ok());
}

/// REQ: F-035
#[test]
fn f_035_first_activation_of_the_client_side_is_instance_1() {
    let mut host = ilium_plugin_sdk::testing::FakeHost::for_manifest(manifest_path())
        .expect("loads")
        .side("client")
        .build();
    let handle = host.activate::<ilium_plugin_core_agent_toolbar::AgentToolbarClient>();
    assert_eq!(handle.side, "client");
    assert_eq!(handle.instance, 1);
}

/// REQ: F-035
#[test]
fn f_035_deactivation_with_shutdown_returns_cleanly() {
    let mut host = client_host();
    let _ = host.deactivate(ilium_plugin_sdk::types::DeactivationReason::Shutdown);
    assert!(host.failures().is_empty());
}
