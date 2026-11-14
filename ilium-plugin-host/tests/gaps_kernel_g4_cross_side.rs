//! Gap G4: topics and service calls between the server runtime and the client runtime. The
//! crossing state is exercised on its own; two states stand in for the two processes and the
//! test moves the messages between them the way the envelope channel does.

use std::time::{Duration, Instant};

use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_host::imports::services::{PendingServiceCall, ServiceBrokerState};
use ilium_plugin_host::imports::topics::Published;
use ilium_plugin_host::runtime::cross_side::{
    CrossSideState, RemoteMessage, RemoteWire, QUEUE_LIMIT, REMOTE_CALLER_PREFIX,
    REMOTE_CALL_TIMEOUT,
};

fn call(caller: &str) -> PendingServiceCall {
    let (request, call) = ServiceBrokerState::fresh_ids();
    PendingServiceCall {
        call,
        request,
        caller: caller.to_owned(),
        service: "ilium.core.fleet/status".to_owned(),
        method: "get".to_owned(),
        payload_json: "{}".to_owned(),
    }
}

fn published(topic: &str) -> Published {
    Published {
        from: "ilium.core.fleet".to_owned(),
        topic: topic.to_owned(),
        payload_json: "{\"n\":1}".to_owned(),
    }
}

#[test]
fn a_published_topic_crosses_and_arrives_as_a_local_message() {
    let mut server = CrossSideState::default();
    let mut client = CrossSideState::default();
    let mut client_broker = ServiceBrokerState::default();
    server.export_topic(&published("ilium.core.fleet/changed"));
    client.receive(server.take_outbound());
    let delivered = client.process_inbound(&mut client_broker, Instant::now());
    assert_eq!(delivered, vec![published("ilium.core.fleet/changed")]);
    // Delivering it did not queue it for export again.
    assert!(client.take_outbound().is_empty());
}

#[test]
fn kernel_topics_never_cross() {
    let mut state = CrossSideState::default();
    state.export_topic(&published("ilium.kernel/focus-changed"));
    assert!(state.take_outbound().is_empty());
}

#[test]
fn a_service_call_crosses_is_served_and_its_reply_comes_back() {
    let mut client = CrossSideState::default();
    let mut client_broker = ServiceBrokerState::default();
    let mut server = CrossSideState::default();
    let mut server_broker = ServiceBrokerState::default();
    let now = Instant::now();

    let asked = call("ilium.core.ui");
    client.send_call(&asked, now).expect("sent");
    server.receive(client.take_outbound());
    server.process_inbound(&mut server_broker, now);

    let mut pending = server_broker.take_pending();
    assert_eq!(pending.len(), 1);
    let served = pending.remove(0);
    assert_eq!(
        served.caller,
        format!("{REMOTE_CALLER_PREFIX}ilium.core.ui")
    );
    server_broker.complete(&served, "{\"ok\":true}".to_owned());
    for end in server_broker.take_finished() {
        assert!(server.finish_served(&mut server_broker, &end));
    }

    client.receive(server.take_outbound());
    client.process_inbound(&mut client_broker, now);
    let finished = client_broker.take_finished();
    assert_eq!(finished.len(), 1);
    assert_eq!(finished[0].caller, "ilium.core.ui");
    assert_eq!(finished[0].request, asked.request);
    assert_eq!(finished[0].error, None);
    assert_eq!(
        client_broker.take_reply_of("ilium.core.ui", asked.request),
        Some("{\"ok\":true}".to_owned())
    );
}

#[test]
fn a_failed_served_call_returns_its_error() {
    let mut client = CrossSideState::default();
    let mut client_broker = ServiceBrokerState::default();
    let mut server = CrossSideState::default();
    let mut server_broker = ServiceBrokerState::default();
    let now = Instant::now();
    let asked = call("ilium.core.ui");
    client.send_call(&asked, now).expect("sent");
    server.receive(client.take_outbound());
    server.process_inbound(&mut server_broker, now);
    let served = server_broker.take_pending().remove(0);
    server_broker.fail(
        &served,
        PluginError::new(ErrorCode::NotFound, "no such fleet"),
    );
    for end in server_broker.take_finished() {
        assert!(server.finish_served(&mut server_broker, &end));
    }
    client.receive(server.take_outbound());
    client.process_inbound(&mut client_broker, now);
    let finished = client_broker.take_finished();
    assert_eq!(
        finished[0].error.as_ref().map(|error| error.code),
        Some(ErrorCode::NotFound)
    );
}

#[test]
fn a_call_without_an_answer_times_out_after_five_seconds() {
    let mut client = CrossSideState::default();
    let mut broker = ServiceBrokerState::default();
    let start = Instant::now();
    let asked = call("ilium.core.ui");
    client.send_call(&asked, start).expect("sent");
    assert_eq!(client.next_deadline(), Some(start + REMOTE_CALL_TIMEOUT));
    client.process_inbound(&mut broker, start + Duration::from_secs(4));
    assert!(broker.take_finished().is_empty());
    client.process_inbound(&mut broker, start + REMOTE_CALL_TIMEOUT);
    let finished = broker.take_finished();
    assert_eq!(finished.len(), 1);
    assert_eq!(
        finished[0].error.as_ref().map(|error| error.code),
        Some(ErrorCode::TimedOut)
    );
    // A late reply finds nothing waiting and changes nothing.
    client.receive(vec![RemoteMessage::ServiceReply {
        call: 1,
        outcome: Ok("{}".to_owned()),
    }]);
    client.process_inbound(&mut broker, start + REMOTE_CALL_TIMEOUT);
    assert!(broker.take_finished().is_empty());
    assert_eq!(client.next_deadline(), None);
}

#[test]
fn a_call_that_came_from_the_other_side_is_not_sent_back() {
    let mut state = CrossSideState::default();
    let echo = call("remote:ilium.core.ui");
    let error = state.send_call(&echo, Instant::now()).expect_err("refused");
    assert_eq!(error.code, ErrorCode::NotFound);
    assert!(state.take_outbound().is_empty());
}

#[test]
fn a_full_outbound_queue_refuses_calls_and_drops_the_oldest_topic() {
    let mut state = CrossSideState::default();
    for index in 0..QUEUE_LIMIT {
        state.export_topic(&published(&format!("ilium.core.fleet/t{index}")));
    }
    let error = state
        .send_call(&call("ilium.core.ui"), Instant::now())
        .expect_err("busy");
    assert_eq!(error.code, ErrorCode::Busy);
    state.export_topic(&published("ilium.core.fleet/last"));
    let queued = state.take_outbound();
    assert_eq!(queued.len(), QUEUE_LIMIT);
    assert!(matches!(
        queued.first(),
        Some(RemoteMessage::Topic { topic, .. }) if topic == "ilium.core.fleet/t1"
    ));
}

#[test]
fn a_stopped_caller_leaves_no_waiting_call() {
    let mut state = CrossSideState::default();
    let mut broker = ServiceBrokerState::default();
    let now = Instant::now();
    state.send_call(&call("ilium.core.ui"), now).expect("sent");
    state.forget_plugin("ilium.core.ui");
    assert!(!state.has_work());
    state.process_inbound(&mut broker, now + REMOTE_CALL_TIMEOUT);
    assert!(broker.take_finished().is_empty());
}

#[test]
fn every_message_survives_its_wire_shape() {
    let messages = [
        RemoteMessage::Topic {
            topic: "ilium.core.fleet/changed".to_owned(),
            from: "ilium.core.fleet".to_owned(),
            payload_json: "{\"n\":1}".to_owned(),
        },
        RemoteMessage::ServiceCall {
            call: 9,
            caller: "ilium.core.ui".to_owned(),
            service: "ilium.core.fleet/status".to_owned(),
            method: "get".to_owned(),
            payload_json: "{}".to_owned(),
        },
        RemoteMessage::ServiceReply {
            call: 9,
            outcome: Ok("{\"ok\":true}".to_owned()),
        },
        RemoteMessage::ServiceReply {
            call: 10,
            outcome: Err(PluginError::new(ErrorCode::Busy, "later")),
        },
    ];
    for message in messages {
        let wire = message.to_wire();
        assert_eq!(RemoteMessage::from_wire(&wire), Some(message));
    }
}

#[test]
fn a_malformed_or_unknown_wire_shape_is_dropped() {
    let bad_event = RemoteWire::Event {
        topic: "a/b".to_owned(),
        payload: b"not json".to_vec(),
    };
    assert_eq!(RemoteMessage::from_wire(&bad_event), None);
    let unknown_request = RemoteWire::Request {
        call_id: 1,
        name: "something.else".to_owned(),
        payload: b"{}".to_vec(),
    };
    assert_eq!(RemoteMessage::from_wire(&unknown_request), None);
    let both = RemoteWire::Reply {
        call_id: 1,
        payload: br#"{"ok":"x","error":null}"#.to_vec(),
    };
    assert_eq!(RemoteMessage::from_wire(&both), None);
}
