//! Request/reply surface for the pure-logic domains of the cost plugin.
//!
//! A `message` event on channel `port.<op>` carrying a JSON request is answered by one `message` intent
//! whose payload is the JSON reply. NaN travels as JSON null and enums are snake_case.

mod chart_ops;
mod model_ops;
mod timeline_ops;

use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::{
    Intent, MessageSpec, MessageTarget, Submission,
};
use ilium_plugin_sdk::types::PluginId;
use serde_json::Value;

/// The id of this plugin, the target of the replies.
const PLUGIN_ID: &str = "ilium.core.cost";

/// Answers one `port.<op>` message event, or `None` when the event is not one.
pub(crate) fn answer(event: &Event) -> Option<Submission> {
    let Event::Message(message) = event else {
        return None;
    };
    let operation = message.channel.strip_prefix("port.")?;
    let request: Value = serde_json::from_slice(&message.payload).unwrap_or(Value::Null);
    let reply = dispatch(operation, &request);
    let payload = serde_json::to_vec(&reply).unwrap_or_default();
    Some(Submission::new(Intent::Message(MessageSpec {
        to: MessageTarget {
            plugin: PluginId(PLUGIN_ID.to_string()),
            side: message.from_side,
            client: message.from_client.clone(),
        },
        channel: format!("port-reply.{operation}"),
        payload,
    })))
}

fn dispatch(operation: &str, request: &Value) -> Value {
    let (domain, name) = operation.split_once('.').unwrap_or((operation, ""));
    match domain {
        "model" => model_ops::run(name, request),
        "chart" => chart_ops::run(name, request),
        "timeline" => timeline_ops::run(name, request),
        _ => Value::Null,
    }
}

/// Reads `request[key]` as f64, NaN for null/missing.
pub(crate) fn number(request: &Value, key: &str) -> f64 {
    request.get(key).and_then(Value::as_f64).unwrap_or(f64::NAN)
}

/// Reads a JSON array of numbers; null becomes NaN.
pub(crate) fn numbers(value: &Value) -> Vec<f64> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|item| item.as_f64().unwrap_or(f64::NAN))
                .collect()
        })
        .unwrap_or_default()
}

/// A finite f64 as JSON, or null for NaN/infinity.
pub(crate) fn float(value: f64) -> Value {
    serde_json::Number::from_f64(value).map_or(Value::Null, Value::Number)
}
