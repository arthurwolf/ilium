//! The server side: compact action, model and context message, debug history.

use ilium_plugin_sdk::generated::host::HostJournal;
use ilium_plugin_sdk::primitives::TranscriptQuery;
use ilium_plugin_sdk::types::{
    Activation, AgentActionSpec, AgentModelEvent, EpochMs, Event, Intent, MessageEvent,
    MessageSpec, MessageTarget, NodeId, PluginError, PluginId, PluginSide, Submission,
};
use ilium_plugin_sdk::{host, ilium_plugin, Handler, Lifecycle};
use serde_json::json;

use crate::PLUGIN_ID;

/// The debug history shows this much of the journal.
const HISTORY_WINDOW_MS: u64 = 3_600_000;
/// The debug history shows at most this many entries.
const HISTORY_LIMIT: u32 = 50;

/// The server side of `ilium.core.agent-toolbar` (core-plugin-agent-toolbar.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = ["handler"])]
#[derive(Default)]
pub struct AgentToolbarServer;

fn client_message(channel: &str, payload: Vec<u8>) -> Submission {
    Submission::new(Intent::Message(MessageSpec {
        to: MessageTarget {
            plugin: PluginId(PLUGIN_ID.to_string()),
            side: PluginSide::Client,
            client: None,
        },
        channel: channel.to_string(),
        payload,
    }))
}

/// A pane id sent as plain decimal text; anything else is refused.
fn parse_pane(payload: &[u8]) -> Option<NodeId> {
    let text = std::str::from_utf8(payload).ok()?;
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse::<u64>().ok().map(NodeId)
}

fn context_percent(used_tokens: u32, window: Option<u32>) -> Option<u32> {
    let window = window.filter(|size| *size > 0)?;
    Some((u64::from(used_tokens) * 100 / u64::from(window)) as u32)
}

/// Used context tokens of a pane from its newest transcript turn that reports them.
fn used_tokens(pane: NodeId) -> u32 {
    let query = TranscriptQuery {
        pane,
        last_turns: 1,
        max_bytes: 65_536,
    };
    let Ok(request) = host().transcript(query) else {
        return 0;
    };
    host()
        .take_transcript(request)
        .and_then(|turns| turns.iter().rev().find_map(|turn| turn.tokens))
        .unwrap_or(0)
}

fn model_message(event: &AgentModelEvent) -> Submission {
    let model = &event.model;
    let text = match context_percent(used_tokens(event.pane), model.context_window) {
        Some(value) => format!("{} \u{b7} {value}% context", model.model),
        None => model.model.clone(),
    };
    client_message("model", format!("{}|{text}", event.pane.0).into_bytes())
}

fn history_reply(pane: NodeId) -> Submission {
    let now = host().now_unix_millis();
    let since = EpochMs(now.saturating_sub(HISTORY_WINDOW_MS));
    let mut api = host();
    let rows: Vec<serde_json::Value> = HostJournal::read(&mut *api, Some(pane), since, HISTORY_LIMIT)
        .into_iter()
        .map(|entry| {
            let detail = serde_json::from_str::<String>(&entry.payload_json)
                .unwrap_or(entry.payload_json);
            json!({ "at": entry.at.0, "kind": entry.kind, "detail": detail })
        })
        .collect();
    client_message("history", serde_json::Value::Array(rows).to_string().into_bytes())
}

impl AgentToolbarServer {
    fn on_message(message: &MessageEvent, out: &mut Vec<Submission>) {
        let Some(pane) = parse_pane(&message.payload) else {
            return;
        };
        match message.channel.as_str() {
            "compact" => out.push(Submission::keyed(
                Intent::AgentAction(AgentActionSpec {
                    pane,
                    action_id: "compact".to_string(),
                }),
                format!("compact:{}", pane.0),
            )),
            "history" => out.push(history_reply(pane)),
            _ => {}
        }
    }
}

impl Lifecycle for AgentToolbarServer {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        Ok(())
    }
}

impl Handler for AgentToolbarServer {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut out = Vec::new();
        for event in batch {
            match event {
                Event::AgentModelChanged(change) => out.push(model_message(&change)),
                Event::Message(message) => Self::on_message(&message, &mut out),
                _ => {}
            }
        }
        out
    }
}
