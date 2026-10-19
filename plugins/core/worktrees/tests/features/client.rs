//! The client side of the command (page sample "New agent in a worktree (Ctrl+B W)"): on
//! `command-invoked` it sends the picked branch to the server side as a `message` on `new-agent`.

use ilium_plugin_sdk::types::{CommandOrigin, Intent, PluginId, PluginSide};
use proptest::prelude::*;

use crate::common::*;

/// REQ: F-067
#[test]
fn feature_067_the_command_sends_one_message_to_the_server_side_of_this_plugin() {
    let mut host = client_host();
    let submitted = host.deliver_one(command_invoked(
        COMMAND,
        r#"{"branch":"fix-auth"}"#,
        CommandOrigin::Palette,
    ));
    assert_eq!(submitted.len(), 1);
    match &submitted[0].intent {
        Intent::Message(spec) => {
            assert_eq!(spec.to.plugin, PluginId(PLUGIN_ID.to_string()));
            assert_eq!(spec.to.side, PluginSide::Server);
            assert_eq!(spec.to.client, None);
            assert_eq!(spec.channel, "new-agent");
        }
        other => panic!("expected a message intent, got {other:?}"),
    }
}

/// REQ: F-067
#[test]
fn feature_067_the_message_payload_is_the_arguments_json_bytes() {
    let mut host = client_host();
    let submitted = host.deliver_one(command_invoked(
        COMMAND,
        r#"{"branch":"fix-auth"}"#,
        CommandOrigin::Palette,
    ));
    assert_eq!(
        server_messages(&submitted),
        [(
            "new-agent".to_string(),
            r#"{"branch":"fix-auth"}"#.to_string()
        )]
    );
}

/// REQ: F-067
#[test]
fn feature_067_the_prefix_key_origin_sends_the_same_message() {
    let mut host = client_host();
    let submitted = host.deliver_one(command_invoked(COMMAND, "{}", CommandOrigin::Keybinding));
    assert_eq!(
        server_messages(&submitted),
        [("new-agent".to_string(), "{}".to_string())]
    );
}

/// REQ: F-067
#[test]
fn feature_067_every_origin_sends_the_message() {
    for origin in [
        CommandOrigin::Keybinding,
        CommandOrigin::Palette,
        CommandOrigin::Toolbar,
        CommandOrigin::Menu,
        CommandOrigin::Voice,
        CommandOrigin::Cli,
    ] {
        let mut host = client_host();
        let submitted = host.deliver_one(command_invoked(COMMAND, "{}", origin));
        assert_eq!(server_messages(&submitted).len(), 1);
    }
}

/// REQ: F-067
#[test]
fn feature_067_a_command_of_another_plugin_submits_nothing() {
    let mut host = client_host();
    let submitted = host.deliver_one(command_invoked(
        "ilium.core.fleet/new-agent",
        "{}",
        CommandOrigin::Palette,
    ));
    assert!(submitted.is_empty());
}

/// REQ: F-067
#[test]
fn feature_067_the_short_command_id_without_the_plugin_prefix_submits_nothing() {
    let mut host = client_host();
    let submitted = host.deliver_one(command_invoked("new-agent", "{}", CommandOrigin::Palette));
    assert!(submitted.is_empty());
}

/// REQ: F-067
#[test]
fn feature_067_an_event_that_is_not_a_command_submits_nothing() {
    let mut host = client_host();
    assert!(host.deliver_one(pane_created(3, "x")).is_empty());
}

/// REQ: F-067
#[test]
fn feature_067_the_client_side_never_runs_git_itself() {
    let mut host = client_host();
    host.deliver_one(command_invoked(
        COMMAND,
        r#"{"branch":"fix-auth"}"#,
        CommandOrigin::Palette,
    ));
    host.settle();
    assert!(host.process().calls().is_empty());
}

proptest! {
    // REQ: F-067
    #[test]
    fn feature_067_any_arguments_json_text_is_forwarded_byte_exact(arguments in "[ -~]{0,64}") {
        let mut host = client_host();
        let submitted = host.deliver_one(command_invoked(COMMAND, &arguments, CommandOrigin::Palette));
        prop_assert_eq!(server_messages(&submitted), vec![("new-agent".to_string(), arguments.clone())]);
    }
}
