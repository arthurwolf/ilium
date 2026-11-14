//! The client side of `ilium.core.recap`: it answers the one question the server cannot know,
//! whether first-run onboarding is finished, so a half-finished setup never sends text to a
//! provider, and it shows the in-pane recap line in the footer of an agent pane.

use ilium_plugin_sdk::host::{HostClientRequests, HostKernelSettings};
use ilium_plugin_sdk::types::{
    CommandInvokedEvent, ErrorCode, Intent, MessageSpec, MessageTarget, NodeId, PluginError,
    PluginId, PluginSide, RichSpan, SetFrameSlotSpec, SpanTarget, Style, Submission,
};
use ilium_plugin_sdk::HostApi;
use serde_json::{json, Value};

use crate::dot::{self, DotRecap};
use crate::server::onboarding_finished;

/// The kind of client request the server sends.
pub const ONBOARDING_REQUEST: &str = "onboarding-state";

/// The `{"finished": bool}` answer, read from the kernel `setup` tab. An unreadable tab means
/// not finished, so the gate stays closed.
pub fn onboarding_answer(api: &mut dyn HostApi) -> String {
    let finished = HostKernelSettings::read(api, "setup".to_string())
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .is_some_and(|value| onboarding_finished(&value));
    json!({"finished": finished}).to_string()
}

/// Answers one client request; an unknown kind is refused.
pub fn answer(api: &mut dyn HostApi, call: u64, kind: &str) {
    let outcome = if kind == ONBOARDING_REQUEST {
        Ok(onboarding_answer(api))
    } else {
        Err(PluginError::new(
            ErrorCode::Unsupported,
            format!("ilium.core.recap: unknown client request {kind}"),
        ))
    };
    let _ = HostClientRequests::respond(api, call, outcome);
}

/// The footer-row span of one pane: the recap text, a click asks for a fresh recap, the hover
/// text is the long recap.
fn line_slot(pane: NodeId, text: String, tooltip: Option<String>) -> Submission {
    Submission::new(Intent::SetFrameSlot(SetFrameSlotSpec {
        pane,
        slot: dot::SLOT.to_string(),
        content: vec![RichSpan {
            text,
            style: Style::default(),
            target: SpanTarget::Command(dot::COMMAND.to_string()),
            tooltip,
        }],
        icon: None,
        tooltip: None,
    }))
}

fn request_message(pane: NodeId) -> Submission {
    Submission::new(Intent::Message(MessageSpec {
        to: MessageTarget {
            plugin: PluginId(crate::PLUGIN_ID.to_string()),
            side: PluginSide::Server,
            client: None,
        },
        channel: dot::REQUEST_CHANNEL.to_string(),
        payload: pane.0.to_string().into_bytes(),
    }))
}

/// The pane a command was made on: the footer click names its pane, a menu or key uses the
/// pane in focus.
pub fn command_pane(invoked: &CommandInvokedEvent) -> Option<NodeId> {
    let clicked = serde_json::from_str::<Value>(&invoked.arguments_json)
        .ok()
        .and_then(|arguments| arguments.get("node").and_then(Value::as_u64));
    clicked.map(NodeId).or(invoked.context.pane)
}

/// The `recap` command: shows that a recap is being written and asks the server for it.
pub fn on_command(invoked: &CommandInvokedEvent, out: &mut Vec<Submission>) {
    let Some(pane) = command_pane(invoked) else {
        return;
    };
    out.push(line_slot(pane, dot::working_line(), None));
    out.push(request_message(pane));
}

/// One result of the server: replaces the footer line of its pane.
pub fn on_result(payload: &[u8], out: &mut Vec<Submission>) {
    let Some(recap) = DotRecap::from_payload(payload) else {
        return;
    };
    out.push(line_slot(NodeId(recap.pane), recap.line(), recap.tooltip()));
}

#[cfg(test)]
mod dot_tests {
    use super::*;
    use ilium_plugin_sdk::types::{CommandContext, CommandOrigin};

    fn invoked(arguments_json: &str, pane: Option<u64>) -> CommandInvokedEvent {
        CommandInvokedEvent {
            command: dot::COMMAND.to_string(),
            arguments_json: arguments_json.to_string(),
            origin: CommandOrigin::Menu,
            context: CommandContext {
                pane: pane.map(NodeId),
                ..CommandContext::default()
            },
        }
    }

    fn slot_text(submission: &Submission) -> String {
        match &submission.intent {
            Intent::SetFrameSlot(spec) => spec.content[0].text.clone(),
            other => panic!("not a frame slot: {other:?}"),
        }
    }

    #[test]
    fn a_footer_click_names_its_pane_and_a_menu_uses_the_focused_one() {
        assert_eq!(
            command_pane(&invoked("{\"node\": 9}", Some(4))),
            Some(NodeId(9))
        );
        assert_eq!(command_pane(&invoked("{}", Some(4))), Some(NodeId(4)));
        assert_eq!(command_pane(&invoked("not json", None)), None);
    }

    #[test]
    fn the_command_shows_working_then_asks_the_server() {
        let mut out = Vec::new();
        on_command(&invoked("{}", Some(4)), &mut out);
        assert_eq!(out.len(), 2);
        assert_eq!(slot_text(&out[0]), dot::working_line());
        match &out[1].intent {
            Intent::Message(message) => {
                assert_eq!(message.channel, dot::REQUEST_CHANNEL);
                assert_eq!(message.to.side, PluginSide::Server);
                assert_eq!(message.payload, b"4");
            }
            other => panic!("not a message: {other:?}"),
        }
        let mut none = Vec::new();
        on_command(&invoked("{}", None), &mut none);
        assert!(none.is_empty());
    }

    #[test]
    fn a_result_replaces_the_footer_line_and_garbage_is_ignored() {
        let recap = DotRecap {
            pane: 4,
            session: None,
            state: dot::SUMMARIZED.to_string(),
            short: Some("Fixing auth.".to_string()),
            long: Some("The agent is fixing auth.".to_string()),
            failure: None,
        };
        let mut out = Vec::new();
        on_result(&recap.to_payload(), &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(slot_text(&out[0]), " Fixing auth. ");
        match &out[0].intent {
            Intent::SetFrameSlot(spec) => {
                assert_eq!(spec.pane, NodeId(4));
                assert_eq!(
                    spec.content[0].tooltip.as_deref(),
                    Some("The agent is fixing auth.")
                );
            }
            other => panic!("not a frame slot: {other:?}"),
        }
        let mut ignored = Vec::new();
        on_result(b"garbage", &mut ignored);
        assert!(ignored.is_empty());
    }
}
