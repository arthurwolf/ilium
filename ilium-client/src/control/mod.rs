//! Provider-independent semantic control plane.
//!
//! Voice is one transport over this boundary. Commands remain typed,
//! synchronous, policy-checked, deduplicated, and unit-testable without a
//! microphone, network, terminal, or model.

mod command;
mod executor;
mod policy;
mod resolver;
mod settings;
mod snapshot;
mod tools;

pub(crate) use snapshot::mode_label;

use std::collections::{HashMap, VecDeque};

use ilium_core::{NodeKind, PaneStatus};
use ilium_voice::{VoiceToolDefinition, VoiceToolInvocation, VoiceToolOutput};
use serde::de::DeserializeOwned;
use serde_json::json;

use crate::app::App;

use command::{
    ConfirmationCommand, ControlCommand, TerminalSubmissionCommand, TerminalTypingCommand,
};

const MAX_CACHED_CALLS: usize = 256;
const MAX_PENDING_CONFIRMATIONS: usize = 32;

/// Live fact the Realtime model needs to distinguish a bare coding prompt
/// from an unclear request directed at ilium itself. Omitted tool targets are
/// still resolved at execution time, so pane ids do not belong in this prompt
/// context and cannot become stale.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum VoiceTargetContext {
    DetectedAgent,
    #[default]
    NoDetectedAgent,
}

impl VoiceTargetContext {
    /// Captures only the semantic fact needed by the dictation fallback. The
    /// provider and activity do not change how exact text is submitted.
    pub fn capture(app: &App) -> Self {
        let is_detected_agent = app
            .active_pane_id()
            .and_then(|pane_id| app.tree.get(pane_id))
            .is_some_and(|node| {
                matches!(
                    &node.kind,
                    NodeKind::Pane {
                        status: PaneStatus::Agent(_),
                        ..
                    }
                )
            });

        if is_detected_agent {
            Self::DetectedAgent
        } else {
            Self::NoDetectedAgent
        }
    }

    fn prompt_status(self) -> &'static str {
        match self {
            Self::DetectedAgent => {
                ilium_prompts::voice::VOICE_MOD_THE_ACTIVE_PANE_IS_CURRENTLY_A_DETECTED
            }
            Self::NoDetectedAgent => {
                ilium_prompts::voice::VOICE_MOD_THE_ACTIVE_PANE_IS_NOT_CURRENTLY_A
            }
        }
    }
}

#[derive(Debug, Clone)]
struct PendingConfirmation {
    command: ControlCommand,
    cancellation_message: String,
}

/// Stable command router and policy state owned by the TUI event loop.
pub struct ControlPlane {
    completed_outputs: HashMap<String, VoiceToolOutput>,
    completed_order: VecDeque<String>,
    pending_confirmations: HashMap<String, PendingConfirmation>,
    pending_confirmation_order: VecDeque<String>,
    next_confirmation_id: u64,
}

impl Default for ControlPlane {
    fn default() -> Self {
        Self {
            completed_outputs: HashMap::new(),
            completed_order: VecDeque::new(),
            pending_confirmations: HashMap::new(),
            pending_confirmation_order: VecDeque::new(),
            next_confirmation_id: 1,
        }
    }
}

impl ControlPlane {
    pub fn tool_definitions(&self) -> Vec<VoiceToolDefinition> {
        tools::definitions()
    }

    pub fn execute_invocation(
        &mut self,
        app: &mut App,
        invocation: VoiceToolInvocation,
    ) -> VoiceToolOutput {
        tracing::info!(
            call_id = %invocation.call_id,
            tool_name = %invocation.name,
            arguments_json = %diagnostic_tool_arguments(&invocation.arguments_json),
            "voice LLM tool invocation received"
        );
        if let Some(output) = self.completed_outputs.get(&invocation.call_id) {
            tracing::info!(
                call_id = %invocation.call_id,
                tool_name = %invocation.name,
                result = %output.result,
                "voice LLM tool invocation replayed from deduplication cache"
            );
            return output.clone();
        }

        let output = if invocation.name == tools::CONFIRM_ACTION_TOOL_NAME {
            self.execute_confirmation(app, &invocation)
        } else {
            match decode_command(&invocation) {
                Ok(command) => {
                    self.execute_or_request_confirmation(app, &invocation.call_id, command)
                }
                Err(error) => tool_error(&invocation.call_id, error),
            }
        };
        if output
            .result
            .get("status")
            .and_then(|status| status.as_str())
            == Some("error")
        {
            tracing::error!(
                call_id = %invocation.call_id,
                tool_name = %invocation.name,
                result = %output.result,
                "voice LLM tool invocation failed"
            );
        } else {
            tracing::info!(
                call_id = %invocation.call_id,
                tool_name = %invocation.name,
                result = %output.result,
                request_follow_up = output.request_follow_up,
                terminate_session_after_delivery = output.terminate_session_after_delivery,
                "voice LLM tool invocation completed"
            );
        }
        self.cache_output(output.clone());
        output
    }

    fn execute_or_request_confirmation(
        &mut self,
        app: &mut App,
        call_id: &str,
        command: ControlCommand,
    ) -> VoiceToolOutput {
        let plan = match policy::confirmation_plan(app, &command) {
            Ok(plan) => plan,
            Err(error) => return tool_error(call_id, error),
        };
        if let Some(plan) = plan {
            let preparation = match plan.preparation {
                Some(command) => match executor::execute(app, command) {
                    Ok(receipt) => Some(receipt),
                    Err(error) => return tool_error(call_id, error),
                },
                None => None,
            };
            let token = format!("voice-confirm-{}", self.next_confirmation_id);
            self.next_confirmation_id = self.next_confirmation_id.saturating_add(1);
            if self.pending_confirmations.len() >= MAX_PENDING_CONFIRMATIONS {
                if let Some(oldest) = self.pending_confirmation_order.pop_front() {
                    self.pending_confirmations.remove(&oldest);
                }
            }
            self.pending_confirmation_order.push_back(token.clone());
            self.pending_confirmations.insert(
                token.clone(),
                PendingConfirmation {
                    command: plan.confirmed_command,
                    cancellation_message: plan.cancellation_message,
                },
            );
            return VoiceToolOutput {
                call_id: call_id.to_owned(),
                result: json!({
                    "status": "confirmation_required",
                    "token": token,
                    "question": plan.question,
                    "preparation": preparation,
                    "instruction": ilium_prompts::voice::VOICE_MOD_ASK_ONLY_THE_EXACT_QUESTION_DO_NOT,
                }),
                request_follow_up: true,
                terminate_session_after_delivery: false,
            };
        }

        execution_output(call_id, executor::execute(app, command))
    }

    fn execute_confirmation(
        &mut self,
        app: &mut App,
        invocation: &VoiceToolInvocation,
    ) -> VoiceToolOutput {
        let confirmation = match decode_arguments::<ConfirmationCommand>(&invocation.arguments_json)
        {
            Ok(confirmation) => confirmation,
            Err(error) => return tool_error(&invocation.call_id, error),
        };
        let Some(pending) = self.pending_confirmations.remove(&confirmation.token) else {
            return tool_error(
                &invocation.call_id,
                ilium_prompts::voice::VOICE_MOD_THAT_CONFIRMATION_TOKEN_IS_MISSING_EXPIRED_OR
                    .to_owned(),
            );
        };
        self.pending_confirmation_order
            .retain(|token| token != &confirmation.token);
        if !confirmation.confirmed {
            return VoiceToolOutput {
                call_id: invocation.call_id.clone(),
                result: json!({
                    "status": "cancelled",
                    "message": pending.cancellation_message,
                }),
                request_follow_up: true,
                terminate_session_after_delivery: false,
            };
        }
        execution_output(&invocation.call_id, executor::execute(app, pending.command))
    }

    fn cache_output(&mut self, output: VoiceToolOutput) {
        if self.completed_outputs.contains_key(&output.call_id) {
            return;
        }
        self.completed_order.push_back(output.call_id.clone());
        self.completed_outputs
            .insert(output.call_id.clone(), output);
        if self.completed_order.len() > MAX_CACHED_CALLS {
            if let Some(evicted) = self.completed_order.pop_front() {
                self.completed_outputs.remove(&evicted);
            }
        }
    }
}

/// Preserves complete semantic tool arguments except credential values. Most
/// settings mutations use `{ "path": "voice.api_key", "value": "..." }`
/// rather than naming the secret field directly, so both shapes are covered.
fn diagnostic_tool_arguments(arguments_json: &str) -> String {
    ilium_logging::redacted_json_string_credentials(arguments_json)
}

/// Base instructions shared by every voice model. The custom user prompt is
/// an additive section and cannot silently replace the safety/control
/// contract required for deterministic tool use.
pub fn system_instructions(custom_prompt: &str, target_context: VoiceTargetContext) -> String {
    ilium_prompts::render_value(
        "voice/mod/system-instructions",
        &serde_json::json!({"v0": (target_context.prompt_status()).to_string(), "v1": (custom_prompt.trim()).to_string()}),
    )
}

fn decode_command(invocation: &VoiceToolInvocation) -> Result<ControlCommand, String> {
    match invocation.name.as_str() {
        tools::GET_STATE_TOOL_NAME => {
            decode_arguments(&invocation.arguments_json).map(ControlCommand::State)
        }
        tools::STOP_VOICE_MODE_TOOL_NAME => {
            decode_arguments::<command::StopVoiceModeCommand>(&invocation.arguments_json)
                .map(ControlCommand::StopVoiceMode)
        }
        tools::UI_TOOL_NAME => decode_arguments(&invocation.arguments_json).map(ControlCommand::Ui),
        tools::TREE_TOOL_NAME => {
            decode_arguments(&invocation.arguments_json).map(ControlCommand::Tree)
        }
        tools::SEND_TO_TERMINAL_TOOL_NAME => {
            decode_arguments::<TerminalSubmissionCommand>(&invocation.arguments_json)
                .map(ControlCommand::TerminalSubmission)
        }
        tools::TYPE_IN_TERMINAL_TOOL_NAME => {
            decode_arguments::<TerminalTypingCommand>(&invocation.arguments_json)
                .map(ControlCommand::TerminalTyping)
        }
        tools::TERMINAL_TOOL_NAME => {
            decode_arguments(&invocation.arguments_json).map(ControlCommand::Terminal)
        }
        tools::EDITOR_TOOL_NAME => {
            decode_arguments(&invocation.arguments_json).map(ControlCommand::Editor)
        }
        tools::BOARD_TOOL_NAME => {
            decode_arguments(&invocation.arguments_json).map(ControlCommand::Board)
        }
        tools::SETTINGS_TOOL_NAME => {
            decode_arguments(&invocation.arguments_json).map(ControlCommand::Settings)
        }
        tools::SEARCH_TOOL_NAME => {
            decode_arguments(&invocation.arguments_json).map(ControlCommand::Search)
        }
        tools::SESSION_TOOL_NAME => {
            decode_arguments(&invocation.arguments_json).map(ControlCommand::Session)
        }
        _ => Err(ilium_prompts::render_value(
            "voice/mod/unknown-ilium-tool",
            &serde_json::json!({"v0": format!("{:?}", invocation.name)}),
        )),
    }
}

fn decode_arguments<T: DeserializeOwned>(arguments_json: &str) -> Result<T, String> {
    serde_json::from_str(arguments_json).map_err(|error| {
        ilium_prompts::render_value(
            "voice/mod/invalid-tool-arguments",
            &serde_json::json!({"v0": (error).to_string()}),
        )
    })
}

fn execution_output(
    call_id: &str,
    result: Result<executor::ExecutionReceipt, String>,
) -> VoiceToolOutput {
    match result {
        Ok(receipt) => {
            let terminate_session_after_delivery = receipt.terminate_session_after_delivery;
            VoiceToolOutput {
                call_id: call_id.to_owned(),
                result: serde_json::to_value(receipt).unwrap_or_else(
                    |error| json!({ "status": "error", "message": error.to_string() }),
                ),
                request_follow_up: !terminate_session_after_delivery,
                terminate_session_after_delivery,
            }
        }
        Err(error) => tool_error(call_id, error),
    }
}

fn tool_error(call_id: &str, error: String) -> VoiceToolOutput {
    VoiceToolOutput {
        call_id: call_id.to_owned(),
        result: json!({
            "status": "error",
            "message": error,
        }),
        request_follow_up: true,
        terminate_session_after_delivery: false,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use ilium_core::{AgentActivity, AgentClass, PaneContentKind, PaneStatus, ROOT_ID};

    use super::*;

    #[test]
    fn diagnostic_tool_arguments_redact_direct_and_path_addressed_credentials() {
        let direct = diagnostic_tool_arguments(
            r#"{"api_key":"direct-secret","nested":{"access_token":"nested-secret"},"action":"test"}"#,
        );
        assert!(!direct.contains("direct-secret"));
        assert!(!direct.contains("nested-secret"));
        assert!(direct.contains("<redacted>"));
        assert!(direct.contains("test"));

        let path_addressed = diagnostic_tool_arguments(
            r#"{"path":"voice.api_key","value":"path-secret","unrelated":"kept"}"#,
        );
        assert!(!path_addressed.contains("path-secret"));
        assert!(path_addressed.contains("<redacted>"));
        assert!(path_addressed.contains("kept"));
    }

    #[test]
    fn duplicate_call_ids_return_the_original_output_without_reexecution() {
        let mut app = App::new("default".to_owned(), PathBuf::from("/tmp/project"));
        let mut plane = ControlPlane::default();
        let invocation = VoiceToolInvocation {
            call_id: "call-1".to_owned(),
            name: tools::SESSION_TOOL_NAME.to_owned(),
            arguments_json: r#"{"action":"detach"}"#.to_owned(),
        };

        let first = plane.execute_invocation(&mut app, invocation.clone());
        let first_outbox = app.take_outbound_requests();
        let second = plane.execute_invocation(&mut app, invocation);

        assert_eq!(first, second);
        assert_eq!(first_outbox.len(), 1);
        assert!(app.take_outbound_requests().is_empty());
    }

    #[test]
    fn high_impact_command_cannot_execute_without_one_time_confirmation() {
        let mut app = App::new("default".to_owned(), PathBuf::from("/tmp/project"));
        let mut plane = ControlPlane::default();
        let request = plane.execute_invocation(
            &mut app,
            VoiceToolInvocation {
                call_id: "kill-1".to_owned(),
                name: tools::SESSION_TOOL_NAME.to_owned(),
                arguments_json: r#"{"action":"kill_session"}"#.to_owned(),
            },
        );
        let token = request.result["token"].as_str().unwrap();
        assert!(app.take_outbound_requests().is_empty());

        let confirmation = plane.execute_invocation(
            &mut app,
            VoiceToolInvocation {
                call_id: "confirm-1".to_owned(),
                name: tools::CONFIRM_ACTION_TOOL_NAME.to_owned(),
                arguments_json: json!({ "token": token, "confirmed": true }).to_string(),
            },
        );

        assert_eq!(confirmation.result["status"], "queued");
        assert_eq!(
            app.take_outbound_requests(),
            vec![ilium_ipc::ClientRequest::KillSession]
        );
    }

    #[test]
    fn pending_confirmation_capacity_evicts_the_oldest_token_by_insertion_order() {
        let mut app = App::new("default".to_owned(), PathBuf::from("/tmp/project"));
        let mut plane = ControlPlane::default();

        for index in 1..=(MAX_PENDING_CONFIRMATIONS + 1) {
            plane.execute_invocation(
                &mut app,
                VoiceToolInvocation {
                    call_id: format!("kill-{index}"),
                    name: tools::SESSION_TOOL_NAME.to_owned(),
                    arguments_json: r#"{"action":"kill_session"}"#.to_owned(),
                },
            );
        }

        assert_eq!(plane.pending_confirmations.len(), MAX_PENDING_CONFIRMATIONS);
        assert!(!plane.pending_confirmations.contains_key("voice-confirm-1"));
        assert!(plane.pending_confirmations.contains_key("voice-confirm-2"));
        assert_eq!(
            plane.pending_confirmation_order.front().map(String::as_str),
            Some("voice-confirm-2")
        );
        assert_eq!(
            plane.pending_confirmation_order.back().map(String::as_str),
            Some("voice-confirm-33")
        );
    }

    #[test]
    fn custom_prompt_cannot_replace_the_safety_contract() {
        let prompt = system_instructions("Use a calm voice.", VoiceTargetContext::DetectedAgent);

        assert!(prompt.contains("Preserve the user's clean IP reputation"));
        assert!(prompt.contains("send /clear to the currently open terminal"));
        assert!(prompt.contains("ilium_send_to_terminal"));
        assert!(prompt.contains("ilium_type_in_terminal"));
        assert!(prompt.contains("ilium_stop_voice_mode"));
        assert!(prompt.contains("always appends a final Enter key"));
        assert!(!prompt.contains("send_enter"));
        assert!(prompt.contains("never read, quote, or summarize the staged text aloud"));
        assert!(prompt.contains("agent-default dictation rule is active"));
        assert!(prompt.contains("user's complete utterance as `text`"));
        assert!(prompt.contains("They do not need words such as \"type\""));
        assert!(prompt.contains("what does this function do?"));
        assert!(prompt.contains("Use a calm voice."));
    }

    #[test]
    fn voice_target_context_follows_only_the_active_detected_agent() {
        let (mut app, plain_pane_id) = active_terminal_app();
        let group_id = app.tree.parent_of(plain_pane_id).unwrap();
        let agent_pane_id = app
            .tree
            .add_pane(group_id, "detected agent", PaneContentKind::Terminal)
            .unwrap();
        app.tree
            .set_pane_status(
                agent_pane_id,
                PaneStatus::from_activity(AgentClass::Codex, AgentActivity::Idle, None),
            )
            .unwrap();

        assert_eq!(
            VoiceTargetContext::capture(&app),
            VoiceTargetContext::NoDetectedAgent
        );

        app.focus_pane(agent_pane_id);

        assert_eq!(
            VoiceTargetContext::capture(&app),
            VoiceTargetContext::DetectedAgent
        );

        app.tree
            .set_pane_status(
                agent_pane_id,
                PaneStatus::from_activity(
                    AgentClass::Codex,
                    AgentActivity::Working,
                    Some(ilium_core::GoalState::Active),
                ),
            )
            .unwrap();

        assert_eq!(
            VoiceTargetContext::capture(&app),
            VoiceTargetContext::DetectedAgent
        );
    }

    #[test]
    fn non_agent_prompt_explicitly_disables_bare_utterance_fallback() {
        let prompt = system_instructions("", VoiceTargetContext::NoDetectedAgent);

        assert!(prompt.contains("agent-default dictation rule is inactive"));
        assert!(prompt.contains("Do not apply this fallback"));
        assert!(prompt.contains("ask one concise clarification question"));
    }

    #[test]
    fn dedicated_stop_tool_disables_voice_and_finishes_after_its_result() {
        let mut app = App::new("default".to_owned(), PathBuf::from("/tmp/project"));
        app.voice_settings.enabled = true;
        let mut plane = ControlPlane::default();

        let output = plane.execute_invocation(
            &mut app,
            VoiceToolInvocation {
                call_id: "stop-voice".to_owned(),
                name: tools::STOP_VOICE_MODE_TOOL_NAME.to_owned(),
                arguments_json: "{}".to_owned(),
            },
        );

        assert_eq!(output.result["status"], "ok");
        assert!(output
            .result
            .get("terminate_session_after_delivery")
            .is_none());
        assert!(!output.request_follow_up);
        assert!(output.terminate_session_after_delivery);
        assert!(!app.voice_settings.enabled);
        assert_eq!(
            app.take_voice_runtime_request(),
            Some(crate::app::VoiceRuntimeRequest::Stop)
        );
    }

    #[test]
    fn stop_tool_rejects_unknown_arguments_without_disabling_voice() {
        let mut app = App::new("default".to_owned(), PathBuf::from("/tmp/project"));
        app.voice_settings.enabled = true;
        let mut plane = ControlPlane::default();

        let output = plane.execute_invocation(
            &mut app,
            VoiceToolInvocation {
                call_id: "invalid-stop-voice".to_owned(),
                name: tools::STOP_VOICE_MODE_TOOL_NAME.to_owned(),
                arguments_json: r#"{"reason":"done"}"#.to_owned(),
            },
        );

        assert_eq!(output.result["status"], "error");
        assert!(output.request_follow_up);
        assert!(!output.terminate_session_after_delivery);
        assert!(app.voice_settings.enabled);
        assert!(app.take_voice_runtime_request().is_none());
    }

    #[test]
    fn dictated_terminal_submission_sends_immediately_by_default() {
        let (mut app, pane_id) = active_terminal_app();
        let mut plane = ControlPlane::default();

        let output = plane.execute_invocation(
            &mut app,
            VoiceToolInvocation {
                call_id: "send-clear".to_owned(),
                name: tools::SEND_TO_TERMINAL_TOOL_NAME.to_owned(),
                arguments_json: r#"{"text":"/clear"}"#.to_owned(),
            },
        );

        assert_eq!(output.result["status"], "queued");
        assert_eq!(
            app.take_outbound_requests(),
            vec![ilium_ipc::ClientRequest::SubmitTerminalText {
                pane_id,
                text: "/clear".to_owned(),
                source: ilium_ipc::PromptSubmissionSource::VoiceControl,
            }]
        );
    }

    #[test]
    fn explicit_terminal_typing_preserves_unsubmitted_text_without_weakening_send() {
        let (mut app, pane_id) = active_terminal_app();
        let mut plane = ControlPlane::default();

        let output = plane.execute_invocation(
            &mut app,
            VoiceToolInvocation {
                call_id: "type-clear".to_owned(),
                name: tools::TYPE_IN_TERMINAL_TOOL_NAME.to_owned(),
                arguments_json: r#"{"text":"/clear"}"#.to_owned(),
            },
        );

        assert_eq!(output.result["status"], "queued");
        assert_eq!(
            app.take_outbound_requests(),
            vec![ilium_ipc::ClientRequest::KeyInput {
                pane_id,
                bytes: b"/clear".to_vec(),
                submission: None,
            }]
        );
    }

    #[test]
    fn enabled_terminal_confirmation_stages_without_enter_then_submits_on_yes() {
        let (mut app, pane_id) = active_terminal_app();
        app.voice_settings.confirm_terminal_submissions = true;
        let mut plane = ControlPlane::default();

        let request = plane.execute_invocation(
            &mut app,
            VoiceToolInvocation {
                call_id: "stage-clear".to_owned(),
                name: tools::SEND_TO_TERMINAL_TOOL_NAME.to_owned(),
                arguments_json: r#"{"text":"/clear"}"#.to_owned(),
            },
        );
        let token = request.result["token"].as_str().unwrap();

        assert_eq!(request.result["status"], "confirmation_required");
        assert!(!request.result["question"]
            .as_str()
            .unwrap()
            .contains("/clear"));
        assert_eq!(
            app.take_outbound_requests(),
            vec![ilium_ipc::ClientRequest::KeyInput {
                pane_id,
                bytes: b"/clear".to_vec(),
                submission: None,
            }]
        );

        let confirmation = plane.execute_invocation(
            &mut app,
            VoiceToolInvocation {
                call_id: "confirm-clear".to_owned(),
                name: tools::CONFIRM_ACTION_TOOL_NAME.to_owned(),
                arguments_json: json!({ "token": token, "confirmed": true }).to_string(),
            },
        );

        assert_eq!(confirmation.result["status"], "queued");
        assert_eq!(
            app.take_outbound_requests(),
            vec![ilium_ipc::ClientRequest::KeyInput {
                pane_id,
                bytes: b"\r".to_vec(),
                submission: Some(ilium_ipc::PromptSubmissionSource::VoiceControl),
            }]
        );
    }

    #[test]
    fn rejected_terminal_confirmation_leaves_staged_text_unsubmitted() {
        let (mut app, pane_id) = active_terminal_app();
        app.voice_settings.confirm_terminal_submissions = true;
        let mut plane = ControlPlane::default();

        let request = plane.execute_invocation(
            &mut app,
            VoiceToolInvocation {
                call_id: "stage-clear".to_owned(),
                name: tools::SEND_TO_TERMINAL_TOOL_NAME.to_owned(),
                arguments_json: r#"{"text":"/clear"}"#.to_owned(),
            },
        );
        let token = request.result["token"].as_str().unwrap();
        assert_eq!(
            app.take_outbound_requests(),
            vec![ilium_ipc::ClientRequest::KeyInput {
                pane_id,
                bytes: b"/clear".to_vec(),
                submission: None,
            }]
        );

        let cancellation = plane.execute_invocation(
            &mut app,
            VoiceToolInvocation {
                call_id: "cancel-clear".to_owned(),
                name: tools::CONFIRM_ACTION_TOOL_NAME.to_owned(),
                arguments_json: json!({ "token": token, "confirmed": false }).to_string(),
            },
        );

        assert_eq!(cancellation.result["status"], "cancelled");
        assert!(cancellation.result["message"]
            .as_str()
            .unwrap()
            .contains("visible and unsubmitted"));
        assert!(app.take_outbound_requests().is_empty());
    }

    #[test]
    fn voice_model_cannot_disable_enter_for_immediate_text() {
        let (mut app, _) = active_terminal_app();
        let mut plane = ControlPlane::default();

        let output = plane.execute_invocation(
            &mut app,
            VoiceToolInvocation {
                call_id: "attempt-stage".to_owned(),
                name: tools::SEND_TO_TERMINAL_TOOL_NAME.to_owned(),
                arguments_json: r#"{"text":"/clear","send_enter":false}"#.to_owned(),
            },
        );

        assert_eq!(output.result["status"], "error");
        assert!(output.result["message"]
            .as_str()
            .unwrap()
            .contains("unknown field `send_enter`"));
        assert!(app.take_outbound_requests().is_empty());
    }

    #[test]
    fn scheduled_voice_text_always_requests_enter_submission() {
        let (mut app, pane_id) = active_terminal_app();
        let mut plane = ControlPlane::default();

        let output = plane.execute_invocation(
            &mut app,
            VoiceToolInvocation {
                call_id: "schedule-next".to_owned(),
                name: tools::TERMINAL_TOOL_NAME.to_owned(),
                arguments_json:
                    r#"{"action":"schedule_input","text":"continue","delay_seconds":5}"#.to_owned(),
            },
        );

        assert_eq!(output.result["status"], "queued");
        assert_eq!(
            app.take_outbound_requests(),
            vec![ilium_ipc::ClientRequest::SchedulePaneInput {
                pane_id,
                delay_seconds: 5,
                text: "continue".to_owned(),
                send_enter: true,
            }]
        );
    }

    fn active_terminal_app() -> (App, ilium_core::NodeId) {
        let mut app = App::new("default".to_owned(), PathBuf::from("/tmp/project"));
        let group_id = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = app
            .tree
            .add_pane(group_id, "agent", PaneContentKind::Terminal)
            .unwrap();
        app.panes.insert(
            pane_id,
            crate::app::PaneRuntime::Terminal(Box::new(crate::terminal_view::TerminalView::new(
                24, 80,
            ))),
        );
        app.focus_pane(pane_id);
        app.take_outbound_requests();

        (app, pane_id)
    }

    #[test]
    fn settings_results_redact_both_inference_and_voice_credentials() {
        let mut app = App::new("default".to_owned(), PathBuf::from("/tmp/project"));
        app.voice_settings.api_key = "voice-secret-value".to_owned();
        app.inference_settings.openai.api_key = "inference-secret-value".to_owned();
        let mut plane = ControlPlane::default();

        let output = plane.execute_invocation(
            &mut app,
            VoiceToolInvocation {
                call_id: "settings-1".to_owned(),
                name: tools::SETTINGS_TOOL_NAME.to_owned(),
                arguments_json: r#"{"action":"get"}"#.to_owned(),
            },
        );
        let serialized = output.result.to_string();

        assert!(!serialized.contains("voice-secret-value"));
        assert!(!serialized.contains("inference-secret-value"));
        assert!(serialized.contains("api_key_configured"));
        assert!(serialized.contains("writable_path_patterns"));
    }

    #[test]
    fn git_settings_control_paths_persist_and_read_back() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::new("default".to_owned(), directory.path().join("project"));
        app.config_dir = Some(directory.path().to_path_buf());
        let mut plane = ControlPlane::default();

        for (call_id, arguments_json) in [
            (
                "git-setting-1",
                r#"{"action":"set","path":"git.branch_prefix","value":"review/"}"#,
            ),
            (
                "git-setting-2",
                r#"{"action":"set","path":"git.branch_line","value":"off"}"#,
            ),
        ] {
            let result = plane.execute_invocation(
                &mut app,
                VoiceToolInvocation {
                    call_id: call_id.to_owned(),
                    name: tools::SETTINGS_TOOL_NAME.to_owned(),
                    arguments_json: arguments_json.to_owned(),
                },
            );
            assert_eq!(result.result["status"], "ok", "{}", result.result);
        }

        let saved = crate::config::load(directory.path()).unwrap();
        assert_eq!(saved.git.branch_prefix, "review/");
        assert_eq!(saved.git.branch_line, crate::config::GitBranchLine::Off);
        let readback = plane.execute_invocation(
            &mut app,
            VoiceToolInvocation {
                call_id: "git-setting-3".to_owned(),
                name: tools::SETTINGS_TOOL_NAME.to_owned(),
                arguments_json: r#"{"action":"get"}"#.to_owned(),
            },
        );
        assert_eq!(readback.result["data"]["git"]["branch_prefix"], "review/");
        assert_eq!(readback.result["data"]["git"]["branch_line"], "off");
    }

    #[test]
    fn semantic_search_returns_local_content_and_opens_the_exact_result() {
        let mut app = App::new("default".to_owned(), PathBuf::from("/tmp/project"));
        let group_id = app.tree.add_group(ROOT_ID, "work").unwrap();
        let editor_id = app
            .tree
            .add_pane(group_id, "notes", PaneContentKind::Editor)
            .unwrap();
        let mut editor = crate::editor_pane::EditorPane::empty();
        editor.insert_text("alpha voice-control needle omega");
        app.panes
            .insert(editor_id, crate::app::PaneRuntime::Editor(Box::new(editor)));
        let mut plane = ControlPlane::default();

        let search = plane.execute_invocation(
            &mut app,
            VoiceToolInvocation {
                call_id: "search-1".to_owned(),
                name: tools::SEARCH_TOOL_NAME.to_owned(),
                arguments_json: r#"{"action":"query","query":"voice-control needle"}"#.to_owned(),
            },
        );
        assert_eq!(search.result["status"], "ok");
        assert_eq!(search.result["data"]["results"][0]["pane_id"], editor_id.0);

        let opened = plane.execute_invocation(
            &mut app,
            VoiceToolInvocation {
                call_id: "search-2".to_owned(),
                name: tools::SEARCH_TOOL_NAME.to_owned(),
                arguments_json: r#"{"action":"open_result","index":0}"#.to_owned(),
            },
        );
        assert_eq!(opened.result["status"], "ok");
        assert_eq!(app.active_pane_id(), Some(editor_id));
    }

    #[test]
    fn built_in_agent_tool_uses_the_registered_provider_command() {
        let mut app = App::new("default".to_owned(), PathBuf::from("/tmp/project"));
        let mut plane = ControlPlane::default();

        let output = plane.execute_invocation(
            &mut app,
            VoiceToolInvocation {
                call_id: "agent-1".to_owned(),
                name: tools::TREE_TOOL_NAME.to_owned(),
                arguments_json:
                    r#"{"action":"create_agent","parent":{"id":0},"provider":"codex","initial_input":"/goal inspect this"}"#
                        .to_owned(),
            },
        );

        assert_eq!(output.result["status"], "queued");
        assert!(matches!(
            app.take_outbound_requests().as_slice(),
            [ilium_ipc::ClientRequest::NewPane {
                parent_group: ROOT_ID,
                kind: ilium_ipc::NewPaneKind::CommandWithInitialInput {
                    command_line,
                    initial_input,
                },
                ..
            }] if command_line == "codex" && initial_input == "/goal inspect this"
        ));
    }

    #[test]
    fn built_in_agent_tool_can_request_a_server_derived_worktree() {
        let mut app = App::new("default".to_owned(), PathBuf::from("/tmp/project"));
        let mut plane = ControlPlane::default();
        let output = plane.execute_invocation(
            &mut app,
            VoiceToolInvocation {
                call_id: "agent-worktree".to_owned(),
                name: tools::TREE_TOOL_NAME.to_owned(),
                arguments_json: r#"{"action":"create_agent","parent":{"id":0},"provider":"codex","workspace":{"branch":"agent/fix","base":"main"}}"#.to_owned(),
            },
        );

        assert_eq!(output.result["status"], "queued");
        assert!(matches!(
            app.take_outbound_requests().as_slice(),
            [ilium_ipc::ClientRequest::CreateAgentInWorkspace {
                parent_group: ROOT_ID,
                provider: ilium_core::BuiltinAgentProvider::Codex,
                spec: ilium_ipc::WorkspaceCreateSpec::NewAtDefaultPath {
                    branch,
                    base_ref: Some(base),
                },
                ..
            }] if branch == "agent/fix" && base == "main"
        ));

        app.git_settings.setup_command = "printf ready".to_string();
        let configured = plane.execute_invocation(
            &mut app,
            VoiceToolInvocation {
                call_id: "agent-worktree-configured".to_owned(),
                name: tools::TREE_TOOL_NAME.to_owned(),
                arguments_json: r#"{"action":"create_agent","parent":{"id":0},"provider":"codex","workspace":{"branch":"agent/next","base":"main"}}"#.to_owned(),
            },
        );
        assert_eq!(configured.result["status"], "queued");
        assert!(matches!(
            app.take_outbound_requests().as_slice(),
            [ilium_ipc::ClientRequest::CreateAgentInWorkspace {
                spec: ilium_ipc::WorkspaceCreateSpec::NewAtDefaultPathWithSetup {
                    branch,
                    base_ref: Some(base),
                    setup_command,
                },
                ..
            }] if branch == "agent/next" && base == "main" && setup_command == "printf ready"
        ));
    }
}
