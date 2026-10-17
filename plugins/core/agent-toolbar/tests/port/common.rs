//! Shared helper of the port twins: one request/reply exchange with the plugin over a message channel.

use std::path::PathBuf;

use ilium_plugin_core_agent_toolbar::{AgentToolbarClient, AgentToolbarServer};
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{Event, Intent, MessageEvent, PluginId, PluginSide};
use serde_json::Value;

pub const PLUGIN_ID: &str = "ilium.core.agent-toolbar";

/// Which side of the plugin answers.
#[derive(Clone, Copy)]
pub enum Place {
    Client,
    Server,
}

fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json")
}

/// Sends `request` as JSON on channel `port.<op>` to the chosen side of the plugin and returns the JSON payload
/// of the first `message` intent the plugin submits in answer, or `Value::Null` when it answers nothing.
pub fn ask(place: Place, op: &str, request: Value) -> Value {
    let (side_name, from_side) = match place {
        Place::Client => ("client", PluginSide::Server),
        Place::Server => ("server", PluginSide::Client),
    };
    let mut host = FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side(side_name)
        .build();
    match place {
        Place::Client => host.activate::<AgentToolbarClient>(),
        Place::Server => host.activate::<AgentToolbarServer>(),
    };
    let submissions = host.deliver_one(Event::Message(MessageEvent {
        from: PluginId(PLUGIN_ID.to_string()),
        from_side,
        from_client: None,
        channel: format!("port.{op}"),
        payload: request.to_string().into_bytes(),
    }));
    submissions
        .iter()
        .find_map(|submission| match &submission.intent {
            Intent::Message(spec) => serde_json::from_slice::<Value>(&spec.payload).ok(),
            _ => None,
        })
        .unwrap_or(Value::Null)
}

/// Text of a JSON string value, empty for anything else.
pub fn text(value: &Value) -> &str {
    value.as_str().unwrap_or("")
}

/// True when the JSON string value contains `needle`.
pub fn has(value: &Value, needle: &str) -> bool {
    text(value).contains(needle)
}
