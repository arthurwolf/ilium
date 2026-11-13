//! Voice-lane testing hooks on `FakeHost` (realtime tool bridge, onboarding demo).
//!
//! Plugin-specific scaffolding kept out of the shared harness type. Skeleton bodies.
//!
//! Mounted only by the `port` test target; the `features` target does not use these hooks.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::PluginError;

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
    pub fn tools(&self) -> Vec<RealtimeTool> {
        unimplemented!("voice.realtime-tools")
    }
    /// Calls a tool with JSON arguments.
    pub fn call_tool(&mut self, _name: &str, _arguments_json: &str) -> Result<String, PluginError> {
        unimplemented!("voice.realtime-call-tool")
    }
    /// Calls a tool with an explicit call id.
    pub fn call_tool_with_id(
        &mut self,
        _call_id: &str,
        _name: &str,
        _arguments_json: &str,
    ) -> Result<String, PluginError> {
        unimplemented!("voice.realtime-call-tool-with-id")
    }
    /// How often `name` was executed.
    pub fn executions(&self, _name: &str) -> usize {
        unimplemented!("voice.realtime-executions")
    }
    /// Whether the last typed submission pressed Enter.
    pub fn last_submit_enter(&self) -> Option<bool> {
        unimplemented!("voice.realtime-last-submit-enter")
    }
    /// The confirmation currently awaiting the user.
    pub fn pending_confirmation(&self) -> Option<String> {
        unimplemented!("voice.realtime-pending-confirmation")
    }
    /// Answers the pending confirmation; returns the tool result.
    pub fn answer_confirmation(&mut self, _answer: &str) -> String {
        unimplemented!("voice.realtime-answer-confirmation")
    }
    /// Sets the agent pane the tools target.
    pub fn set_active_agent(&mut self, _agent: &str) {
        unimplemented!("voice.realtime-set-active-agent")
    }
    /// The system prompt sent to the model.
    pub fn system_prompt(&self) -> String {
        unimplemented!("voice.realtime-system-prompt")
    }
    /// The target context text, if any.
    pub fn target_context(&self) -> Option<String> {
        unimplemented!("voice.realtime-target-context")
    }
    /// Tool call names in order.
    pub fn tool_calls(&self) -> Vec<String> {
        unimplemented!("voice.realtime-tool-calls")
    }
    /// Texts typed into the target pane.
    pub fn typed_text(&self) -> Vec<String> {
        unimplemented!("voice.realtime-typed-text")
    }
}

/// Handle on an onboarding demo the plugin registered.
pub struct OnboardingDemo<'a> {
    pub _host: &'a mut FakeHost,
    pub _name: String,
}

impl OnboardingDemo<'_> {
    /// Tool names of the demo.
    pub fn tools(&self) -> Vec<String> {
        unimplemented!("voice.onboarding-demo-tools")
    }
    /// Calls a demo tool: `(call_id, name, arguments_json)`.
    pub fn call(
        &mut self,
        _call_id: &str,
        _name: &str,
        _arguments_json: &str,
    ) -> Result<String, PluginError> {
        unimplemented!("voice.onboarding-demo-call")
    }
    /// The demo bulb state, if shown.
    pub fn bulb(&self) -> Option<bool> {
        unimplemented!("voice.onboarding-demo-bulb")
    }
}

/// Voice-lane hooks. Implemented for `FakeHost` with skeleton bodies.
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
