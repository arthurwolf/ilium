//! Voice-lane testing hooks on `FakeHost` (realtime tool bridge, onboarding demo).
//!
//! Plugin-specific scaffolding kept out of the shared harness type. Every hook calls the plugin's own services
//! through `FakeHost::call_service`: `ilium.core.voice/realtime` and `ilium.core.voice/onboarding-demo`
//! (names assumed, see API_GAPS_ports.md); a plugin that does not answer them fails the calling test.
//!
//! Mounted only by the `port` test target; the `features` target does not use these hooks.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::PluginError;
use serde_json::{json, Value};

const REALTIME: &str = "ilium.core.voice/realtime";
const DEMO: &str = "ilium.core.voice/onboarding-demo";

/// Calls `method` of `service` and parses the JSON reply.
fn ask(host: &mut FakeHost, service: &str, method: &str, payload: Value) -> Result<Value, PluginError> {
    let text = host.call_service(service, method, &payload.to_string())?;
    serde_json::from_str(&text).map_err(|error| {
        PluginError::new(
            ilium_plugin_sdk::types::ErrorCode::Internal,
            format!("{service}.{method} replied with invalid JSON: {error}"),
        )
    })
}

/// Like [`ask`] for the read-only hooks, which have no error channel: a failed call stops the test.
fn read(host: &mut FakeHost, service: &str, method: &str, payload: Value) -> Value {
    ask(host, service, method, payload)
        .unwrap_or_else(|error| panic!("{service}.{method} failed: {error:?}"))
}

fn strings(value: &Value, key: &str) -> Vec<String> {
    value[key]
        .as_array()
        .map(|items| items.iter().filter_map(|item| item.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

fn text_of(value: &Value, key: &str) -> Option<String> {
    value[key].as_str().map(str::to_string)
}


/// One tool the realtime session exposes to the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RealtimeTool {
    pub name: String,
    pub parameters_json: String,
}

/// Handle on the plugin's realtime tool bridge.
pub struct Realtime<'a> {
    pub _host: &'a mut FakeHost,
}

impl Realtime<'_> {
    /// Tools currently offered to the model.
    pub fn tools(&mut self) -> Vec<RealtimeTool> {
        let reply = read(self._host, REALTIME, "tools", json!({}));
        reply["tools"]
            .as_array()
            .map(|tools| {
                tools
                    .iter()
                    .map(|tool| RealtimeTool {
                        name: tool["name"].as_str().unwrap_or_default().to_string(),
                        parameters_json: tool["parameters_json"]
                            .as_str()
                            .map(str::to_string)
                            .unwrap_or_else(|| tool["parameters"].to_string()),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
    /// Calls a tool with JSON arguments.
    pub fn call_tool(&mut self, name: &str, arguments_json: &str) -> Result<String, PluginError> {
        let reply = ask(
            self._host,
            REALTIME,
            "call-tool",
            json!({ "name": name, "arguments": arguments_json }),
        )?;
        Ok(text_of(&reply, "result").unwrap_or_default())
    }
    /// Calls a tool with an explicit call id.
    pub fn call_tool_with_id(
        &mut self,
        call_id: &str,
        name: &str,
        arguments_json: &str,
    ) -> Result<String, PluginError> {
        let reply = ask(
            self._host,
            REALTIME,
            "call-tool",
            json!({ "call_id": call_id, "name": name, "arguments": arguments_json }),
        )?;
        Ok(text_of(&reply, "result").unwrap_or_default())
    }
    /// How often `name` was executed.
    pub fn executions(&mut self, name: &str) -> usize {
        let reply = read(self._host, REALTIME, "executions", json!({ "name": name }));
        reply["count"].as_u64().unwrap_or(0) as usize
    }
    /// Whether the last typed submission pressed Enter.
    pub fn last_submit_enter(&mut self) -> Option<bool> {
        read(self._host, REALTIME, "last-submit-enter", json!({}))["enter"].as_bool()
    }
    /// The confirmation currently awaiting the user.
    pub fn pending_confirmation(&mut self) -> Option<String> {
        text_of(
            &read(self._host, REALTIME, "pending-confirmation", json!({})),
            "confirmation",
        )
    }
    /// Answers the pending confirmation; returns the tool result.
    pub fn answer_confirmation(&mut self, answer: &str) -> String {
        let reply = read(
            self._host,
            REALTIME,
            "answer-confirmation",
            json!({ "answer": answer }),
        );
        text_of(&reply, "result").unwrap_or_default()
    }
    /// Sets the agent pane the tools target.
    pub fn set_active_agent(&mut self, agent: &str) {
        read(self._host, REALTIME, "set-active-agent", json!({ "agent": agent }));
    }
    /// The system prompt sent to the model.
    pub fn system_prompt(&mut self) -> String {
        text_of(&read(self._host, REALTIME, "system-prompt", json!({})), "text").unwrap_or_default()
    }
    /// The target context text, if any.
    pub fn target_context(&mut self) -> Option<String> {
        text_of(&read(self._host, REALTIME, "target-context", json!({})), "text")
    }
    /// Tool call names in order.
    pub fn tool_calls(&mut self) -> Vec<String> {
        strings(&read(self._host, REALTIME, "tool-calls", json!({})), "names")
    }
    /// Texts typed into the target pane.
    pub fn typed_text(&mut self) -> Vec<String> {
        strings(&read(self._host, REALTIME, "typed-text", json!({})), "texts")
    }
}

/// Handle on an onboarding demo the plugin registered.
pub struct OnboardingDemo<'a> {
    pub _host: &'a mut FakeHost,
    pub _name: String,
}

impl OnboardingDemo<'_> {
    /// Tool names of the demo.
    pub fn tools(&mut self) -> Vec<String> {
        let name = self._name.clone();
        strings(&read(self._host, DEMO, "tools", json!({ "demo": name })), "tools")
    }
    /// Calls a demo tool: `(call_id, name, arguments_json)`.
    pub fn call(
        &mut self,
        call_id: &str,
        name: &str,
        arguments_json: &str,
    ) -> Result<String, PluginError> {
        let demo = self._name.clone();
        let reply = ask(
            self._host,
            DEMO,
            "call",
            json!({ "demo": demo, "call_id": call_id, "name": name, "arguments": arguments_json }),
        )?;
        Ok(text_of(&reply, "result").unwrap_or_default())
    }
    /// The demo bulb state, if shown.
    pub fn bulb(&mut self) -> Option<bool> {
        let name = self._name.clone();
        read(self._host, DEMO, "bulb", json!({ "demo": name }))["on"].as_bool()
    }
}

/// Voice-lane hooks. Implemented for `FakeHost`.
pub trait VoiceHostExt {
    /// The realtime tool bridge.
    fn realtime(&mut self) -> Realtime<'_>;
    /// The onboarding demo named `name`.
    fn onboarding_demo(&mut self, name: &str) -> OnboardingDemo<'_>;
}

impl VoiceHostExt for FakeHost {
    fn realtime(&mut self) -> Realtime<'_> {
        Realtime { _host: self }
    }

    fn onboarding_demo(&mut self, name: &str) -> OnboardingDemo<'_> {
        OnboardingDemo {
            _host: self,
            _name: name.to_string(),
        }
    }
}
