//! Shared helper of the port twins: one request/reply exchange with the plugin over a message channel.
//!
//! The manual defines no surface for the pure-logic domains the original tests cover (price table, level
//! scales, chart, transcript statistics, reset parsing, settings model). The twins assume the convention of
//! the other core-plugin port suites: a `message` event on channel `port.<op>` carrying a JSON request is
//! answered by one `message` intent whose payload is the JSON reply (see ../API_GAPS_ports.md).
use std::path::PathBuf;

use ilium_plugin_core_cost::{CostClient, CostServer};
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{Event, Intent, MessageEvent, PluginId, PluginSide};
use serde_json::Value;

pub const PLUGIN_ID: &str = "ilium.core.cost";

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
        Place::Client => host.activate::<CostClient>(),
        Place::Server => host.activate::<CostServer>(),
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

/// Shorthand for the server side.
pub fn server(op: &str, request: Value) -> Value {
    ask(Place::Server, op, request)
}

/// Shorthand for the client side.
pub fn client(op: &str, request: Value) -> Value {
    ask(Place::Client, op, request)
}

/// Rounds a JSON number to compare floats: true when `value` is within `1e-6` of `expected`.
pub fn near(value: &Value, expected: f64) -> bool {
    value
        .as_f64()
        .is_some_and(|number| (number - expected).abs() < 1e-6)
}
