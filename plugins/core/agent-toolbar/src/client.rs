//! The client side: banner, recover hint, model header, compact routing and debug history storage.

use std::collections::BTreeMap;

use ilium_plugin_sdk::types::{
    Activation, AgentActivity, ClearFrameSlotSpec, CommandInvokedEvent, Event, Intent,
    InvokeCommandSpec, MessageEvent, MessageSpec, MessageTarget, NodeId, PluginError, PluginId, PluginSide,
    RichSpan, SetFrameSlotSpec, SpanTarget, Style, Submission,
};
use ilium_plugin_sdk::{host, ilium_plugin, Handler, Lifecycle};

use crate::settings::UiSettings;
use crate::PLUGIN_ID;

/// The banner caps the trimmed prompt at this many characters.
const BANNER_CHARS: usize = 120;
/// The remote compaction command of `ilium.core.compaction`.
const COMPACTION_COMMAND: &str = "ilium.core.compaction/compact";

/// The client side of `ilium.core.agent-toolbar` (core-plugin-agent-toolbar.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = ["handler"])]
#[derive(Default)]
pub struct AgentToolbarClient {
    /// Trimmed last prompt per pane.
    last_prompts: BTreeMap<u64, String>,
    /// Goal text per pane, remembered for the frame.
    goals: BTreeMap<u64, Option<String>>,
    /// Latest history reply received from the server side, kept for the popup.
    history_payload: Option<Vec<u8>>,
}

pub(crate) fn plain(text: &str) -> Vec<RichSpan> {
    vec![RichSpan {
        text: text.to_string(),
        style: Style::default(),
        target: SpanTarget::None,
        tooltip: None,
    }]
}

fn set_slot(pane: NodeId, slot: &str, text: &str) -> Submission {
    Submission::new(Intent::SetFrameSlot(SetFrameSlotSpec {
        pane,
        slot: slot.to_string(),
        content: plain(text),
        icon: None,
        tooltip: None,
    }))
}

fn clear_slot(pane: NodeId, slot: &str) -> Submission {
    Submission::new(Intent::ClearFrameSlot(ClearFrameSlotSpec {
        pane,
        slot: slot.to_string(),
    }))
}

fn message_to(side: PluginSide, channel: &str, payload: Vec<u8>) -> Submission {
    Submission::new(Intent::Message(MessageSpec {
        to: MessageTarget {
            plugin: PluginId(PLUGIN_ID.to_string()),
            side,
            client: None,
        },
        channel: channel.to_string(),
        payload,
    }))
}

/// Trimmed prompt capped at the banner length, or `None` for an empty prompt.
fn banner_text(prompt: &str) -> Option<String> {
    let trimmed = prompt.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.chars().take(BANNER_CHARS).collect())
}

impl AgentToolbarClient {
    fn on_user_line(&mut self, pane: NodeId, text: &str, out: &mut Vec<Submission>) {
        let Some(prompt) = banner_text(text) else {
            return;
        };
        self.last_prompts.insert(pane.0, prompt.clone());
        if UiSettings::read().last_prompt_enabled {
            out.push(set_slot(pane, "prompt", &format!("> {prompt}")));
        }
    }

    fn on_activity(&mut self, node: NodeId, current: AgentActivity, out: &mut Vec<Submission>) {
        if !matches!(current, AgentActivity::None | AgentActivity::Done) {
            return;
        }
        out.push(clear_slot(node, "prompt"));
        if let Some(prompt) = self.last_prompts.get(&node.0) {
            out.push(set_slot(node, "recover", &format!("Stopped. Last prompt: {prompt}")));
        }
    }

    fn on_command(&mut self, invoked: &CommandInvokedEvent, out: &mut Vec<Submission>) {
        let Some(pane) = invoked.context.pane else {
            return;
        };
        if invoked.command == "compact" && self.routes_to_remote_compaction(pane) {
            out.push(Submission::new(Intent::InvokeCommand(InvokeCommandSpec {
                command: COMPACTION_COMMAND.to_string(),
                arguments_json: "{}".to_string(),
            })));
            return;
        }
        out.push(message_to(
            PluginSide::Server,
            &invoked.command,
            pane.0.to_string().into_bytes(),
        ));
    }

    /// Remote compaction needs the setting on and a pane with a verified transcript session.
    fn routes_to_remote_compaction(&self, pane: NodeId) -> bool {
        UiSettings::read().remote_compaction && host().session(pane).is_some()
    }

    fn on_message(&mut self, message: &MessageEvent, out: &mut Vec<Submission>) {
        match message.channel.as_str() {
            "model" => {
                let text = String::from_utf8_lossy(&message.payload).into_owned();
                if let Some((pane, label)) = text.split_once('|') {
                    if let Ok(pane) = pane.parse::<u64>() {
                        out.push(set_slot(NodeId(pane), "model", label));
                    }
                }
            }
            "history" => self.history_payload = Some(message.payload.clone()),
            _ => {}
        }
    }
}

impl Lifecycle for AgentToolbarClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        Ok(())
    }
}

impl Handler for AgentToolbarClient {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut out = Vec::new();
        for event in batch {
            match event {
                Event::UserLineSubmitted(line) => self.on_user_line(line.pane, &line.text, &mut out),
                Event::AgentActivityChanged(change) => {
                    self.on_activity(change.node, change.current, &mut out);
                }
                Event::AgentGoalChanged(goal) => {
                    self.goals.insert(goal.node.0, goal.goal);
                }
                Event::CommandInvoked(invoked) => self.on_command(&invoked, &mut out),
                Event::Message(message) => self.on_message(&message, &mut out),
                _ => {}
            }
        }
        out
    }
}
