use ilium_core::NodeId;
use ilium_ipc::{AntigravityStatuslineAction, ClientRequest, ServerEvent};

fn round_trip<T>(value: &T) -> T
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    bincode::deserialize(&bincode::serialize(value).expect("serialize")).expect("deserialize")
}

#[test]
fn terminate_and_replace_requests_round_trip_with_stable_names() {
    let terminate = ClientRequest::TerminatePaneProcess { pane_id: NodeId(4) };
    assert_eq!(round_trip(&terminate), terminate);
    assert_eq!(terminate.diagnostic_name(), "terminate_pane_process");

    let replace = ClientRequest::ReplacePaneWithCommand {
        pane_id: NodeId(4),
        command_line: "codex resume 11111111-1111-4111-8111-111111111111".to_string(),
    };
    assert_eq!(round_trip(&replace), replace);
    assert_eq!(replace.diagnostic_name(), "replace_pane_with_command");
}

#[test]
fn unfreeze_request_round_trips_at_the_append_only_request_tail() {
    let previous = ClientRequest::DiscardTerminalDelivery {
        pane_ids: vec![NodeId(4)],
    };
    let unfreeze = ClientRequest::UnfreezePane { pane_id: NodeId(4) };
    assert_eq!(round_trip(&unfreeze), unfreeze);
    assert_eq!(unfreeze.diagnostic_name(), "unfreeze_pane");

    let previous_index = bincode::serialize(&previous).expect("serialize")[..4].to_vec();
    let unfreeze_index = bincode::serialize(&unfreeze).expect("serialize")[..4].to_vec();
    let previous_value = u32::from_le_bytes(previous_index.try_into().expect("4 bytes"));
    let unfreeze_value = u32::from_le_bytes(unfreeze_index.try_into().expect("4 bytes"));
    assert_eq!(unfreeze_value, previous_value + 1);
}

#[test]
fn antigravity_statusline_actions_round_trip_after_the_existing_request_tail() {
    let unfreeze = ClientRequest::UnfreezePane { pane_id: NodeId(4) };
    let enable = ClientRequest::UpdateAntigravityStatusline {
        generation: 7,
        action: AntigravityStatuslineAction::SetCommand {
            command: "/usr/bin/ilium __antigravity-model-statusline".to_owned(),
        },
    };
    let delete = ClientRequest::UpdateAntigravityStatusline {
        generation: 8,
        action: AntigravityStatuslineAction::DeleteCommand,
    };
    let cancel = ClientRequest::UpdateAntigravityStatusline {
        generation: 9,
        action: AntigravityStatuslineAction::CancelPending,
    };
    let disable = ClientRequest::UpdateAntigravityStatusline {
        generation: 10,
        action: AntigravityStatuslineAction::DisableCommand,
    };

    assert_eq!(round_trip(&enable), enable);
    assert_eq!(round_trip(&delete), delete);
    assert_eq!(round_trip(&cancel), cancel);
    assert_eq!(round_trip(&disable), disable);
    assert_eq!(enable.diagnostic_name(), "update_antigravity_statusline");

    let unfreeze_index = bincode::serialize(&unfreeze).expect("serialize")[..4].to_vec();
    let update_index = bincode::serialize(&enable).expect("serialize")[..4].to_vec();
    let unfreeze_value = u32::from_le_bytes(unfreeze_index.try_into().expect("4 bytes"));
    let update_value = u32::from_le_bytes(update_index.try_into().expect("4 bytes"));
    assert_eq!(update_value, unfreeze_value + 1);
}

#[test]
fn pane_process_terminated_round_trips_success_and_failure() {
    for result in [Ok(()), Err("process tree still alive".to_string())] {
        let event = ServerEvent::PaneProcessTerminated {
            pane_id: NodeId(9),
            result,
        };
        assert_eq!(round_trip(&event), event);
    }
}

#[test]
fn antigravity_statusline_completion_round_trips_at_the_append_only_event_tail() {
    let previous = ServerEvent::PaneResizeRejected {
        pane_id: NodeId(5),
        rows: 24,
        cols: 80,
        message: "resize refused".to_owned(),
    };
    let completed = ServerEvent::AntigravityStatuslineCompleted {
        generation: 19,
        result: Ok(()),
    };
    let failed = ServerEvent::AntigravityStatuslineCompleted {
        generation: 20,
        result: Err("composer changed before delivery".to_owned()),
    };

    assert_eq!(round_trip(&completed), completed);
    assert_eq!(round_trip(&failed), failed);
    let previous_index = bincode::serialize(&previous).expect("serialize")[..4].to_vec();
    let completed_index = bincode::serialize(&completed).expect("serialize")[..4].to_vec();
    let previous_value = u32::from_le_bytes(previous_index.try_into().expect("4 bytes"));
    let completed_value = u32::from_le_bytes(completed_index.try_into().expect("4 bytes"));
    assert_eq!(completed_value, previous_value + 1);
}

#[test]
fn new_variants_do_not_shift_the_previous_last_variant() {
    // `UpdateAgentDetectionSettings` was the final request before these
    // variants; appending must leave its encoding (variant index) unchanged.
    let previous = ClientRequest::UpdateAgentDetectionSettings {
        request_id: None,
        settings: ilium_ipc::AgentDetectionSettings {
            working_poll_seconds: 0,
            idle_poll_seconds: 45,
            custom_signatures: Vec::new(),
        },
    };
    let terminate = ClientRequest::TerminatePaneProcess { pane_id: NodeId(1) };
    let previous_index = bincode::serialize(&previous).expect("serialize")[..4].to_vec();
    let terminate_index = bincode::serialize(&terminate).expect("serialize")[..4].to_vec();
    let previous_value = u32::from_le_bytes(previous_index.try_into().expect("4 bytes"));
    let terminate_value = u32::from_le_bytes(terminate_index.try_into().expect("4 bytes"));
    assert_eq!(terminate_value, previous_value + 1);
}
