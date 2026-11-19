use ilium_core::NodeId;
use ilium_ipc::{MonitoringEvent, MonitoringEventKind};

#[test]
fn monitoring_events_round_trip_every_kind_with_its_cursor_and_source() {
    let kinds = [
        MonitoringEventKind::Started,
        MonitoringEventKind::Idle,
        MonitoringEventKind::Done,
        MonitoringEventKind::NeedsInput,
        MonitoringEventKind::Error,
        MonitoringEventKind::Stuck,
        MonitoringEventKind::Closed,
        MonitoringEventKind::Progress,
        MonitoringEventKind::ResourceChanged,
    ];
    for kind in kinds {
        assert_eq!(MonitoringEventKind::parse(kind.as_str()), Some(kind));
        let event = MonitoringEvent {
            generation: 0,
            cursor: 42,
            pane_id: Some(NodeId(17)),
            project: Some("project".into()),
            title: Some("agent pane".into()),
            agent: Some("codex".into()),
            kind,
            observed_at_unix_millis: 1_700_000_000_000,
            summary: "fixture evidence".into(),
        };
        let encoded = bincode::serialize(&event).expect("serialize monitoring event");
        let decoded: MonitoringEvent =
            bincode::deserialize(&encoded).expect("decode monitoring event");
        assert_eq!(decoded, event);
    }
    assert_eq!(MonitoringEventKind::parse("not-an-event"), None);
}

#[test]
fn monitoring_query_round_trips_filters_and_output_budget() {
    use ilium_ipc::{MonitoringFormat, MonitoringQuery};
    let query = MonitoringQuery {
        view: ilium_ipc::MonitoringView::Events,
        since: Some("epoch:9".into()),
        consumer: Some("manager".into()),
        follow: true,
        min_severity: Some("warning".into()),
        kinds: vec![MonitoringEventKind::Error],
        project: Some("project".into()),
        agent: Some("codex".into()),
        pane_id: Some(NodeId(3)),
        max_items: 12,
        max_chars: 4000,
        format: MonitoringFormat::Compact,
    };
    let bytes = bincode::serialize(&query).unwrap();
    let decoded: MonitoringQuery = bincode::deserialize(&bytes).unwrap();
    assert_eq!(decoded, query);
    let request = ilium_ipc::ClientRequest::MonitoringEvents {
        request_id: 7,
        query,
    };
    let bytes = bincode::serialize(&request).unwrap();
    let decoded: ilium_ipc::ClientRequest = bincode::deserialize(&bytes).unwrap();
    assert_eq!(decoded, request);
    let response = ilium_ipc::ServerEvent::MonitoringEvents {
        request_id: 7,
        result: Ok("quiet cursor=epoch:9\n".into()),
    };
    let bytes = bincode::serialize(&response).unwrap();
    let decoded: ilium_ipc::ServerEvent = bincode::deserialize(&bytes).unwrap();
    assert_eq!(decoded, response);
}

#[test]
fn acknowledgement_round_trips_consumer_cursor_and_result() {
    let request = ilium_ipc::ClientRequest::MonitoringAck {
        request_id: 11,
        consumer: "manager".into(),
        cursor: "epoch:42".into(),
    };
    let bytes = bincode::serialize(&request).unwrap();
    assert_eq!(
        bincode::deserialize::<ilium_ipc::ClientRequest>(&bytes).unwrap(),
        request
    );
    let response = ilium_ipc::ServerEvent::MonitoringAck {
        request_id: 11,
        result: Ok("epoch:42".into()),
    };
    let bytes = bincode::serialize(&response).unwrap();
    assert_eq!(
        bincode::deserialize::<ilium_ipc::ServerEvent>(&bytes).unwrap(),
        response
    );
}

#[test]
fn audited_control_write_round_trips_without_exposing_a_terminal_subscription() {
    let request = ilium_ipc::ClientRequest::ControlWrite {
        request_id: 18,
        token: "fixture-token".into(),
        caller_pane: Some(ilium_core::NodeId(7)),
        operation: "{\"op\":\"rename\",\"node\":7,\"title\":\"fixture\"}".into(),
    };
    let encoded = bincode::serialize(&request).unwrap();
    let decoded: ilium_ipc::ClientRequest = bincode::deserialize(&encoded).unwrap();
    assert!(matches!(
        decoded,
        ilium_ipc::ClientRequest::ControlWrite {
            request_id: 18,
            caller_pane: Some(ilium_core::NodeId(7)),
            ..
        }
    ));
    let event = ilium_ipc::ServerEvent::ControlWrite {
        request_id: 18,
        result: Err("audit unavailable".into()),
    };
    let encoded = bincode::serialize(&event).unwrap();
    let decoded: ilium_ipc::ServerEvent = bincode::deserialize(&encoded).unwrap();
    assert!(matches!(
        decoded,
        ilium_ipc::ServerEvent::ControlWrite {
            request_id: 18,
            result: Err(_)
        }
    ));
}
