use ilium_core::custom_agent::CustomAgentIdentity;
use ilium_core::{BuiltinAgentProvider, NodeId};
use ilium_ipc::{
    ClientRequest, NewPaneKind, NewPaneWorkingDirectory, ServerEvent, WorkspaceCreateSpec,
};

fn round_trip<T: serde::Serialize + serde::de::DeserializeOwned>(value: &T) -> T {
    bincode::deserialize(&bincode::serialize(value).unwrap()).unwrap()
}

#[test]
fn profile_launches_and_identity_projection_round_trip_without_environment_values() {
    let identity = CustomAgentIdentity {
        profile_id: "stable-profile".into(),
        base_provider: BuiltinAgentProvider::Codex,
    };
    let request = ClientRequest::NewPane {
        request_id: 1,
        parent_group: NodeId(2),
        kind: NewPaneKind::CustomAgent {
            identity: identity.clone(),
            initial_input: None,
        },
        working_directory: NewPaneWorkingDirectory::ProjectRoot,
    };
    assert_eq!(round_trip(&request), request);
    let workspace = ClientRequest::CreateCustomAgentInWorkspace {
        request_id: 2,
        parent_group: NodeId(2),
        identity: identity.clone(),
        spec: WorkspaceCreateSpec::Existing {
            path: "/fixture/worktree".into(),
        },
        initial_input: Some("start".into()),
    };
    assert_eq!(round_trip(&workspace), workspace);
    assert_eq!(
        workspace.diagnostic_name(),
        "create_custom_agent_in_workspace"
    );
    for identity in [Some(identity), None] {
        let event = ServerEvent::PaneCustomAgentIdentity {
            pane_id: NodeId(3),
            identity,
        };
        assert_eq!(round_trip(&event), event);
    }
}
