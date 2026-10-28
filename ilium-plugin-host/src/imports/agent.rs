//! Host implementation of `ilium:primitives/agent` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostAgent` trait; hand-owned afterwards.

use ilium_plugin_api::generated::primitives::agent::{
    AgentPane, HostAgent, SessionRef, SignatureSpec, TranscriptQuery, TranscriptTurn,
};
use std::cell::RefCell;
use std::collections::HashMap;

use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_api::generated::types::ids::{NodeId, RequestId};
use ilium_plugin_api::generated::types::tree::{AgentModel, AgentView};

/// Longest signature id, in characters.
const MAX_ID_CHARS: usize = 64;
/// Most process names, aliases or actions one signature holds.
const MAX_LIST_ENTRIES: usize = 32;
/// Longest process name, in bytes.
const MAX_PROCESS_NAME_BYTES: usize = 255;
/// Longest action key sequence, in bytes.
const MAX_ACTION_KEYS_BYTES: usize = 1024;
/// Longest toolbar button label, in cells.
const MAX_LABEL_CELLS: usize = 32;
/// Most turns a transcript request returns.
const MAX_TRANSCRIPT_TURNS: u32 = 512;
/// Most bytes a transcript request returns.
const MAX_TRANSCRIPT_BYTES: u32 = 4 * 1024 * 1024;

thread_local! {
    /// Registered signatures by id.
    static SIGNATURES: RefCell<HashMap<String, SignatureSpec>> = RefCell::new(HashMap::new());
}

/// Builds a refusal with the given code and a plain-text message.
fn refuse(code: ErrorCode, message: &str) -> PluginError {
    PluginError {
        code,
        message: message.to_owned(),
        retry_after_ms: None,
    }
}

/// Checks every signature rule; the first broken rule is the refusal.
fn validate_signature(spec: &SignatureSpec) -> Result<(), PluginError> {
    let invalid = |message: &str| Err(refuse(ErrorCode::InvalidArgument, message));
    let id_chars = spec.id.chars().count();
    if id_chars == 0 || id_chars > MAX_ID_CHARS {
        return invalid("signature id must be 1 to 64 characters");
    }
    let id_is_valid = spec
        .id
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !id_is_valid {
        return invalid("signature id may hold only a-z, 0-9 and hyphen");
    }
    if spec.process_names.is_empty() || spec.process_names.len() > MAX_LIST_ENTRIES {
        return invalid("process-names must hold 1 to 32 entries");
    }
    for name in &spec.process_names {
        if name.is_empty() || name.len() > MAX_PROCESS_NAME_BYTES {
            return invalid("a process name must be 1 to 255 bytes");
        }
        if name.contains('/') && !name.starts_with('/') {
            return invalid("a process name with a slash must be an absolute path");
        }
    }
    if spec.aliases.len() > MAX_LIST_ENTRIES {
        return invalid("aliases may hold at most 32 entries");
    }
    if spec.actions.len() > MAX_LIST_ENTRIES {
        return invalid("actions may hold at most 32 entries");
    }
    if spec.actions.iter().any(|action| action.keys.len() > MAX_ACTION_KEYS_BYTES) {
        return invalid("action keys may hold at most 1 KiB");
    }
    Ok(())
}

/// Host side of the `agent` interface.
#[derive(Debug)]
pub struct AgentImport;

impl HostAgent for AgentImport {
    fn agents(&mut self) -> Vec<AgentPane> {
        // No pane is identified as an agent until the detection loop feeds this import.
        Vec::new()
    }

    fn view(&mut self, _pane: NodeId) -> Option<AgentView> {
        None
    }

    fn session(&mut self, _pane: NodeId) -> Option<SessionRef> {
        None
    }

    fn last_prompt(&mut self, _pane: NodeId) -> Option<String> {
        None
    }

    fn register_signature(&mut self, spec: SignatureSpec) -> Result<(), PluginError> {
        validate_signature(&spec)?;
        SIGNATURES.with(|signatures| {
            signatures.borrow_mut().insert(spec.id.clone(), spec);
        });
        Ok(())
    }

    fn set_toolbar_state(
        &mut self,
        button: String,
        pane: Option<NodeId>,
        _enabled: bool,
        label: Option<String>,
    ) -> Result<(), PluginError> {
        if label
            .as_deref()
            .is_some_and(|text| text.chars().count() > MAX_LABEL_CELLS)
        {
            return Err(refuse(
                ErrorCode::InvalidArgument,
                "label may be at most 32 cells",
            ));
        }
        // No pane is identified yet, so a named pane never exists.
        if pane.is_some() {
            return Err(refuse(ErrorCode::NotFound, "pane is not an identified agent"));
        }
        // A button belongs to the plugin only when a registered signature declares it as an action.
        let owns_button = SIGNATURES.with(|signatures| {
            signatures
                .borrow()
                .values()
                .any(|spec| spec.actions.iter().any(|action| action.id == button))
        });
        if !owns_button {
            return Err(refuse(ErrorCode::NotFound, "button is not owned by this plugin"));
        }
        Ok(())
    }

    fn model(&mut self, _pane: NodeId) -> Option<AgentModel> {
        None
    }

    fn prompt_epoch(&mut self, _pane: NodeId) -> Option<u64> {
        None
    }

    fn transcript(&mut self, query: TranscriptQuery) -> Result<RequestId, PluginError> {
        if query.last_turns > MAX_TRANSCRIPT_TURNS || query.max_bytes > MAX_TRANSCRIPT_BYTES {
            return Err(refuse(
                ErrorCode::InvalidArgument,
                "transcript is limited to 512 turns and 4 MiB",
            ));
        }
        // No pane has an agent session yet, so there is no transcript to read.
        Err(refuse(ErrorCode::NotFound, "pane has no agent session"))
    }

    fn take_transcript(&mut self, _request: RequestId) -> Option<Vec<TranscriptTurn>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::AgentImport;
    use ilium_plugin_api::generated::primitives::agent::{
        HostAgent, SignatureAction, SignatureSpec, TranscriptQuery,
    };
    use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
    use ilium_plugin_api::generated::types::ids::NodeId;
    use ilium_plugin_api::generated::types::tree::AgentActivity;

    /// A pane id that no agent pane in a fresh host can own.
    const UNKNOWN_PANE: NodeId = NodeId(u64::MAX);

    /// A spec that satisfies every signature rule, used as the base for boundary variants.
    fn valid_spec() -> SignatureSpec {
        SignatureSpec {
            id: "example-agent".to_string(),
            process_names: vec!["example-agent".to_string()],
            aliases: Vec::new(),
            icon: None,
            resume: None,
            actions: Vec::new(),
            session_scope: None,
            submit_keys: None,
        }
    }

    /// Returns the valid spec with one field changed by `mutate`.
    fn spec_with(mutate: impl FnOnce(&mut SignatureSpec)) -> SignatureSpec {
        let mut spec = valid_spec();
        mutate(&mut spec);
        spec
    }

    /// Panics unless `result` is a refusal with exactly `expected` as its code.
    fn assert_refused_with(result: Result<(), PluginError>, expected: ErrorCode) {
        match result {
            Ok(()) => panic!("expected refusal {expected:?}, got Ok(())"),
            Err(error) => assert_eq!(error.code, expected, "refusal message: {}", error.message),
        }
    }

    /// REQ: PRI-010
    #[test]
    fn register_signature_accepts_a_valid_minimal_spec() {
        let mut import = AgentImport;
        assert_eq!(import.register_signature(valid_spec()), Ok(()));
    }

    /// REQ: PRI-010
    #[test]
    fn register_signature_refuses_an_empty_id_with_invalid_argument() {
        let mut import = AgentImport;
        let spec = spec_with(|spec| spec.id = String::new());
        assert_refused_with(import.register_signature(spec), ErrorCode::InvalidArgument);
    }

    /// REQ: PRI-010
    #[test]
    fn register_signature_accepts_an_id_of_exactly_64_characters() {
        let mut import = AgentImport;
        let spec = spec_with(|spec| spec.id = "a".repeat(64));
        assert_eq!(import.register_signature(spec), Ok(()));
    }

    /// REQ: PRI-010
    #[test]
    fn register_signature_refuses_an_id_of_65_characters() {
        let mut import = AgentImport;
        let spec = spec_with(|spec| spec.id = "a".repeat(65));
        assert_refused_with(import.register_signature(spec), ErrorCode::InvalidArgument);
    }

    /// REQ: PRI-010
    #[test]
    fn register_signature_refuses_ids_outside_lowercase_digits_and_hyphen() {
        let mut import = AgentImport;
        for bad_id in [
            "Example-Agent",
            "example_agent",
            "example agent",
            "example/agent",
        ] {
            let spec = spec_with(|spec| spec.id = bad_id.to_string());
            assert_refused_with(import.register_signature(spec), ErrorCode::InvalidArgument);
        }
    }

    /// REQ: PRI-010
    #[test]
    fn register_signature_refuses_an_empty_process_name_list() {
        let mut import = AgentImport;
        let spec = spec_with(|spec| spec.process_names = Vec::new());
        assert_refused_with(import.register_signature(spec), ErrorCode::InvalidArgument);
    }

    /// REQ: PRI-010
    #[test]
    fn register_signature_accepts_exactly_32_process_names() {
        let mut import = AgentImport;
        let spec = spec_with(|spec| {
            spec.process_names = (0..32).map(|index| format!("agent-{index}")).collect();
        });
        assert_eq!(import.register_signature(spec), Ok(()));
    }

    /// REQ: PRI-010
    #[test]
    fn register_signature_refuses_33_process_names() {
        let mut import = AgentImport;
        let spec = spec_with(|spec| {
            spec.process_names = (0..33).map(|index| format!("agent-{index}")).collect();
        });
        assert_refused_with(import.register_signature(spec), ErrorCode::InvalidArgument);
    }

    /// REQ: PRI-010
    #[test]
    fn register_signature_refuses_an_empty_process_name_entry() {
        let mut import = AgentImport;
        let spec = spec_with(|spec| spec.process_names = vec![String::new()]);
        assert_refused_with(import.register_signature(spec), ErrorCode::InvalidArgument);
    }

    /// REQ: PRI-010
    #[test]
    fn register_signature_accepts_a_process_name_of_exactly_255_bytes() {
        let mut import = AgentImport;
        let spec = spec_with(|spec| spec.process_names = vec!["a".repeat(255)]);
        assert_eq!(import.register_signature(spec), Ok(()));
    }

    /// REQ: PRI-010
    #[test]
    fn register_signature_refuses_a_process_name_of_256_bytes() {
        let mut import = AgentImport;
        let spec = spec_with(|spec| spec.process_names = vec!["a".repeat(256)]);
        assert_refused_with(import.register_signature(spec), ErrorCode::InvalidArgument);
    }

    /// REQ: PRI-010
    #[test]
    fn register_signature_refuses_a_relative_process_name_with_a_slash() {
        let mut import = AgentImport;
        let spec = spec_with(|spec| spec.process_names = vec!["bin/example-agent".to_string()]);
        assert_refused_with(import.register_signature(spec), ErrorCode::InvalidArgument);
    }

    /// REQ: PRI-010
    #[test]
    fn register_signature_accepts_an_absolute_process_path() {
        let mut import = AgentImport;
        let spec = spec_with(|spec| {
            spec.process_names = vec!["/usr/local/bin/example-agent".to_string()];
        });
        assert_eq!(import.register_signature(spec), Ok(()));
    }

    /// REQ: PRI-010
    #[test]
    fn register_signature_accepts_exactly_32_aliases() {
        let mut import = AgentImport;
        let spec = spec_with(|spec| {
            spec.aliases = (0..32).map(|index| format!("alias-{index}")).collect();
        });
        assert_eq!(import.register_signature(spec), Ok(()));
    }

    /// REQ: PRI-010
    #[test]
    fn register_signature_refuses_33_aliases() {
        let mut import = AgentImport;
        let spec = spec_with(|spec| {
            spec.aliases = (0..33).map(|index| format!("alias-{index}")).collect();
        });
        assert_refused_with(import.register_signature(spec), ErrorCode::InvalidArgument);
    }

    /// REQ: PRI-010
    #[test]
    fn register_signature_accepts_exactly_32_actions() {
        let mut import = AgentImport;
        let spec = spec_with(|spec| {
            spec.actions = (0..32)
                .map(|index| SignatureAction {
                    id: format!("action-{index}"),
                    keys: "/compact\r".to_string(),
                    allowed_when: Some(vec![AgentActivity::Idle]),
                })
                .collect();
        });
        assert_eq!(import.register_signature(spec), Ok(()));
    }

    /// REQ: PRI-010
    #[test]
    fn register_signature_refuses_33_actions() {
        let mut import = AgentImport;
        let spec = spec_with(|spec| {
            spec.actions = (0..33)
                .map(|index| SignatureAction {
                    id: format!("action-{index}"),
                    keys: "/compact\r".to_string(),
                    allowed_when: None,
                })
                .collect();
        });
        assert_refused_with(import.register_signature(spec), ErrorCode::InvalidArgument);
    }

    /// REQ: PRI-010
    #[test]
    fn register_signature_accepts_action_keys_of_exactly_1_kib() {
        let mut import = AgentImport;
        let spec = spec_with(|spec| {
            spec.actions = vec![SignatureAction {
                id: "compact".to_string(),
                keys: "k".repeat(1024),
                allowed_when: None,
            }];
        });
        assert_eq!(import.register_signature(spec), Ok(()));
    }

    /// REQ: PRI-010
    #[test]
    fn register_signature_refuses_action_keys_over_1_kib() {
        let mut import = AgentImport;
        let spec = spec_with(|spec| {
            spec.actions = vec![SignatureAction {
                id: "compact".to_string(),
                keys: "k".repeat(1025),
                allowed_when: None,
            }];
        });
        assert_refused_with(import.register_signature(spec), ErrorCode::InvalidArgument);
    }

    /// REQ: PRI-012
    #[test]
    fn set_toolbar_state_refuses_an_unknown_pane_with_not_found() {
        let mut import = AgentImport;
        let result =
            import.set_toolbar_state("compact".to_string(), Some(UNKNOWN_PANE), true, None);
        assert_refused_with(result, ErrorCode::NotFound);
    }

    /// REQ: PRI-012
    #[test]
    fn set_toolbar_state_refuses_a_button_the_plugin_does_not_own_with_not_found() {
        let mut import = AgentImport;
        let result = import.set_toolbar_state("no-such-button".to_string(), None, true, None);
        assert_refused_with(result, ErrorCode::NotFound);
    }

    /// REQ: PRI-012
    #[test]
    fn set_toolbar_state_refuses_a_label_of_33_cells_with_invalid_argument() {
        let mut import = AgentImport;
        let result =
            import.set_toolbar_state("compact".to_string(), None, true, Some("x".repeat(33)));
        assert_refused_with(result, ErrorCode::InvalidArgument);
    }

    /// REQ: PRI-012
    #[test]
    fn set_toolbar_state_does_not_reject_a_label_of_exactly_32_cells() {
        let mut import = AgentImport;
        let result =
            import.set_toolbar_state("compact".to_string(), None, true, Some("x".repeat(32)));
        if let Err(error) = result {
            assert_ne!(
                error.code,
                ErrorCode::InvalidArgument,
                "a 32 cell label is within the limit: {}",
                error.message
            );
        }
    }

    /// REQ: PRI-013
    #[test]
    fn agents_returns_an_empty_list_when_no_pane_is_identified() {
        let mut import = AgentImport;
        assert!(import.agents().is_empty());
    }

    /// REQ: PRI-013
    #[test]
    fn view_returns_none_for_an_unidentified_pane() {
        let mut import = AgentImport;
        assert_eq!(import.view(UNKNOWN_PANE), None);
    }

    /// REQ: PRI-013
    #[test]
    fn view_returns_none_for_pane_zero() {
        let mut import = AgentImport;
        assert_eq!(import.view(NodeId(0)), None);
    }

    /// REQ: PRI-013
    #[test]
    fn session_returns_none_for_an_unidentified_pane() {
        let mut import = AgentImport;
        assert_eq!(import.session(UNKNOWN_PANE), None);
    }

    /// REQ: PRI-013
    #[test]
    fn last_prompt_returns_none_for_an_unidentified_pane() {
        let mut import = AgentImport;
        assert_eq!(import.last_prompt(UNKNOWN_PANE), None);
    }

    /// REQ: PRI-015
    #[test]
    fn view_reports_no_activity_before_identification() {
        let mut import = AgentImport;
        let activity = import.view(UNKNOWN_PANE).map(|view| view.activity);
        assert!(
            activity.is_none() || activity == Some(AgentActivity::None),
            "an unidentified pane must report no activity, got {activity:?}"
        );
    }

    /// REQ: PRI-014
    #[test]
    fn transcript_refuses_a_pane_without_a_session_with_not_found() {
        let mut import = AgentImport;
        let result = import.transcript(TranscriptQuery {
            pane: UNKNOWN_PANE,
            last_turns: 20,
            max_bytes: 256 * 1024,
        });
        match result {
            Ok(request) => panic!("expected not-found, got request {request:?}"),
            Err(error) => assert_eq!(error.code, ErrorCode::NotFound, "{}", error.message),
        }
    }

    /// REQ: PRI-014
    #[test]
    fn transcript_accepts_the_limit_of_512_turns_and_4_mib() {
        let mut import = AgentImport;
        let result = import.transcript(TranscriptQuery {
            pane: UNKNOWN_PANE,
            last_turns: 512,
            max_bytes: 4 * 1024 * 1024,
        });
        if let Err(error) = result {
            assert_ne!(
                error.code,
                ErrorCode::InvalidArgument,
                "512 turns and 4 MiB are within the limits: {}",
                error.message
            );
        }
    }
}
