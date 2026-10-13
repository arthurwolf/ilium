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

    fn view(&mut self, pane: NodeId) -> Option<AgentView> {
        unimplemented!("host::imports::agent::view")
    }

    fn session(&mut self, pane: NodeId) -> Option<SessionRef> {
        unimplemented!("host::imports::agent::session")
    }

    fn last_prompt(&mut self, pane: NodeId) -> Option<String> {
        unimplemented!("host::imports::agent::last_prompt")
    }

    fn register_signature(&mut self, spec: SignatureSpec) -> Result<(), PluginError> {
        unimplemented!("host::imports::agent::register_signature")
    }

    fn set_toolbar_state(
        &mut self,
        button: String,
        pane: Option<NodeId>,
        enabled: bool,
        label: Option<String>,
    ) -> Result<(), PluginError> {
        unimplemented!("host::imports::agent::set_toolbar_state")
    }

    fn model(&mut self, pane: NodeId) -> Option<AgentModel> {
        unimplemented!("host::imports::agent::model")
    }

    fn prompt_epoch(&mut self, pane: NodeId) -> Option<u64> {
        unimplemented!("host::imports::agent::prompt_epoch")
    }

    fn transcript(&mut self, query: TranscriptQuery) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::agent::transcript")
    }

    fn take_transcript(&mut self, request: RequestId) -> Option<Vec<TranscriptTurn>> {
        unimplemented!("host::imports::agent::take_transcript")
    }
}
