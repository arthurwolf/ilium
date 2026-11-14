//! The `analyse` job: the server side reads the recent screen of each changed agent, asks the host
//! inference call to classify it and returns the replies for the client's queue.
//!
//! Wire format. Input: `{"generation": n, "concurrency": n, "analysed": [[pane, fingerprint]],
//! "panes": [{"id", "project", "title", "fingerprint"}]}`. Output: `{"generation": n,
//! "analysed": [[pane, fingerprint]], "items": [AttentionItem]}`.

use ilium_plugin_sdk::generated::host::{ChatMessage, CompletionRequest};
use ilium_plugin_sdk::host::{HostClock, HostInference, HostJobs};
use ilium_plugin_sdk::primitives::{HostTerminal, ScreenPart};
use ilium_plugin_sdk::types::{ErrorCode, NodeId, PluginError, RequestId};
use ilium_plugin_sdk::{host, HostApi};
use serde_json::{json, Value};

use std::collections::HashMap;

use crate::analyse::{run_batch, CallError, Model, Request};
use crate::queue::LlmReply;
use crate::turbo::TurboLimiter;

/// Rows of the visible screen read per agent.
const SCREEN_ROWS: u32 = 200;
/// How long one model call may take before it counts as failed.
const CALL_WAIT_MILLIS: u32 = 120_000;

/// The host-backed model: inference for the calls, the terminal for the evidence.
struct HostModel<'a> {
    api: &'a mut dyn HostApi,
}

fn call_error(error: &PluginError) -> CallError {
    if error.code == ErrorCode::RateLimited {
        CallError::Throttled(error.message.clone())
    } else {
        CallError::classify(&error.message)
    }
}

impl Model for HostModel<'_> {
    fn evidence(&mut self, pane: NodeId) -> String {
        HostTerminal::read_screen(self.api, pane, ScreenPart::Visible, SCREEN_ROWS)
            .map(|screen| screen.rows.join("\n"))
            .unwrap_or_default()
    }

    fn start(&mut self, prompt: &str) -> Result<u64, CallError> {
        let request = CompletionRequest {
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
            max_output_tokens: None,
            json_schema: None,
            purpose: "attention".to_string(),
        };
        HostInference::complete(self.api, request)
            .map(|id| id.0)
            .map_err(|error| call_error(&error))
    }

    fn finish(&mut self, handle: u64) -> Result<String, CallError> {
        let id = RequestId(handle);
        if let Err(error) = HostJobs::wait_request(self.api, id, CALL_WAIT_MILLIS) {
            return Err(call_error(&error));
        }
        HostInference::take_completion(self.api, id)
            .map(|completion| completion.text)
            .ok_or_else(|| CallError::Failed("no completion".to_string()))
    }

    fn cancelled(&mut self) -> bool {
        HostJobs::is_cancelled(self.api)
    }
}

/// Reads `[[pane, fingerprint], ...]`.
fn pairs(value: Option<&Value>) -> Vec<(NodeId, u64)> {
    value
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| {
                    let pair = entry.as_array()?;
                    Some((NodeId(pair.first()?.as_u64()?), pair.get(1)?.as_u64()?))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The job output for a finished batch.
pub fn reply_json(reply: &LlmReply) -> Value {
    json!({
        "generation": reply.generation,
        "analysed": reply.analysed.iter().map(|(pane, fingerprint)| json!([pane.0, fingerprint])).collect::<Vec<_>>(),
        "items": reply.items,
    })
}

/// Reads a job output back into the reply the queue applies.
pub fn reply_from_json(value: &Value) -> Option<LlmReply> {
    let items = value
        .get("items")
        .cloned()
        .and_then(|items| serde_json::from_value(items).ok())
        .unwrap_or_default();
    Some(LlmReply {
        generation: value.get("generation")?.as_u64()?,
        analysed: pairs(value.get("analysed")),
        items,
    })
}

/// Runs the `analyse` job over `input`; the limiter outlives the job so a throttled provider
/// keeps its smaller window for the next one.
pub fn run_analyse(limiter: &mut TurboLimiter, input: &[u8]) -> Result<Vec<u8>, PluginError> {
    let parsed: Value = serde_json::from_slice(input)
        .map_err(|error| PluginError::invalid_argument(format!("bad analyse input: {error}")))?;
    let generation = parsed
        .get("generation")
        .and_then(Value::as_u64)
        .ok_or_else(|| PluginError::invalid_argument("analyse input has no generation"))?;
    let concurrency = parsed
        .get("concurrency")
        .and_then(Value::as_u64)
        .map_or(1, |value| value.min(64) as usize);
    let requests: Vec<Request> = parsed
        .get("panes")
        .and_then(Value::as_array)
        .map(|entries| entries.iter().filter_map(Request::from_json).collect())
        .unwrap_or_default();
    let already: HashMap<NodeId, u64> = pairs(parsed.get("analysed")).into_iter().collect();
    limiter.set_configured(concurrency);
    let mut handle = host();
    let api: &mut dyn HostApi = &mut *handle;
    let now_ms = HostClock::now_unix_millis(api);
    let mut model = HostModel { api };
    let reply = run_batch(&mut model, requests, &already, limiter, generation, now_ms);
    serde_json::to_vec(&reply_json(&reply))
        .map_err(|error| PluginError::new(ErrorCode::Internal, error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{AttentionItem, AttentionKind, AttentionSource, Briefing};

    fn item(pane: u64) -> AttentionItem {
        AttentionItem {
            pane_id: NodeId(pane),
            project_id: Some(NodeId(1)),
            title: "api".to_string(),
            kind: AttentionKind::Question,
            reason: "needs you".to_string(),
            question: Some("Proceed?".to_string()),
            waiting_on: None,
            briefing: Briefing::default(),
            priority: 70,
            source: AttentionSource::Llm,
            fingerprint: 9,
            generation: 4,
            created_ms: 100,
        }
    }

    #[test]
    fn a_reply_survives_the_wire_round_trip() {
        let reply = LlmReply {
            generation: 4,
            analysed: vec![(NodeId(2), 9), (NodeId(3), 11)],
            items: vec![item(2)],
        };
        let back = reply_from_json(&reply_json(&reply)).expect("a reply");
        assert_eq!(back.generation, 4);
        assert_eq!(back.analysed, reply.analysed);
        assert_eq!(back.items, reply.items);
    }

    #[test]
    fn a_reply_without_a_generation_is_rejected() {
        assert!(reply_from_json(&json!({"items": []})).is_none());
    }

    #[test]
    fn input_without_a_generation_is_an_invalid_argument() {
        let mut limiter = TurboLimiter::new(2);
        let error = run_analyse(&mut limiter, br#"{"panes": []}"#).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        let error = run_analyse(&mut limiter, b"not json").unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    #[test]
    fn rate_limited_host_errors_count_as_throttling() {
        let throttled = PluginError::new(ErrorCode::RateLimited, "slow down");
        assert!(matches!(call_error(&throttled), CallError::Throttled(_)));
        let other = PluginError::new(ErrorCode::Network, "reset");
        assert!(matches!(call_error(&other), CallError::Failed(_)));
    }
}
