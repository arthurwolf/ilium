use ilium_core::NodeId;
use ilium_ipc::{ClientRequest, ServerEvent};

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
