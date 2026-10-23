//! Host implementation of `ilium:primitives/agent` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostAgent` trait; hand-owned afterwards.

use ilium_plugin_api::generated::primitives::agent::{
    AgentPane, HostAgent, SessionRef, SignatureSpec, TranscriptQuery, TranscriptTurn,
};
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::ids::{NodeId, RequestId};
use ilium_plugin_api::generated::types::tree::{AgentModel, AgentView};

/// Host side of the `agent` interface.
#[derive(Debug)]
pub struct AgentImport;

impl HostAgent for AgentImport {
    fn agents(&mut self) -> Vec<AgentPane> {
        unimplemented!("host::imports::agent::agents")
    }

    fn view(&mut self, _pane: NodeId) -> Option<AgentView> {
        unimplemented!("host::imports::agent::view")
    }

    fn session(&mut self, _pane: NodeId) -> Option<SessionRef> {
        unimplemented!("host::imports::agent::session")
    }

    fn last_prompt(&mut self, _pane: NodeId) -> Option<String> {
        unimplemented!("host::imports::agent::last_prompt")
    }

    fn register_signature(&mut self, _spec: SignatureSpec) -> Result<(), PluginError> {
        unimplemented!("host::imports::agent::register_signature")
    }

    fn set_toolbar_state(
        &mut self,
        _button: String,
        _pane: Option<NodeId>,
        _enabled: bool,
        _label: Option<String>,
    ) -> Result<(), PluginError> {
        unimplemented!("host::imports::agent::set_toolbar_state")
    }

    fn model(&mut self, _pane: NodeId) -> Option<AgentModel> {
        unimplemented!("host::imports::agent::model")
    }

    fn prompt_epoch(&mut self, _pane: NodeId) -> Option<u64> {
        unimplemented!("host::imports::agent::prompt_epoch")
    }

    fn transcript(&mut self, _query: TranscriptQuery) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::agent::transcript")
    }

    fn take_transcript(&mut self, _request: RequestId) -> Option<Vec<TranscriptTurn>> {
        unimplemented!("host::imports::agent::take_transcript")
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
