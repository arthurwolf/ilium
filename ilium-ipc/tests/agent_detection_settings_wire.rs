use ilium_core::AgentClass;
use ilium_ipc::{AgentDetectionSettings, ClientRequest, CustomAgentSignature, ServerEvent};

#[test]
fn agent_detection_update_request_round_trips_over_bincode() {
    let request = ClientRequest::UpdateAgentDetectionSettings {
        settings: AgentDetectionSettings {
            working_poll_seconds: 0,
            idle_poll_seconds: 45,
            custom_signatures: vec![CustomAgentSignature {
                name_substring: "my-agent".to_string(),
                class: AgentClass::Other("my-agent".to_string()),
            }],
        },
    };

    let decoded: ClientRequest =
        bincode::deserialize(&bincode::serialize(&request).expect("serialize request"))
            .expect("deserialize request");

    assert_eq!(decoded, request);
    assert_eq!(request.diagnostic_name(), "update_agent_detection_settings");
}

#[test]
fn agent_detection_settings_event_round_trips_both_success_and_rejection() {
    let settings = AgentDetectionSettings {
        working_poll_seconds: 0,
        idle_poll_seconds: 45,
        custom_signatures: Vec::new(),
    };
    let events = [
        ServerEvent::AgentDetectionSettingsChanged {
            result: Ok(settings.clone()),
        },
        ServerEvent::AgentDetectionSettingsChanged {
            result: Err(ilium_ipc::AgentDetectionSettingsError {
                message: "working poll interval must be at least 500 ms".to_string(),
            }),
        },
    ];

    for event in events {
        let decoded: ServerEvent =
            bincode::deserialize(&bincode::serialize(&event).expect("serialize event"))
                .expect("deserialize event");
        assert_eq!(decoded, event);
    }
}
