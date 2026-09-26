//! Real-server test of the `ilium voice say` relay: a one-shot requester
//! connection, a connection that registered as a voice host, and the server
//! brokering between them over the real socket and wire format.

use std::time::Duration;

use ilium_ipc::{
    write_frame, ClientRequest, ServerEvent, VoiceTextAccepted, VoiceTextPhase, VoiceTextRejection,
    VoiceTextRejectionCode,
};

mod common;
use common::{expect_event, TestServer};

const STEP: Duration = Duration::from_secs(10);

async fn attach(server: &TestServer, session: &str) -> ilium_transport::SessionStream {
    let mut stream = server.connect().await;
    write_frame(
        &mut stream,
        &ClientRequest::AttachInteractive {
            session: session.to_owned(),
        },
    )
    .await
    .expect("attach request");
    let _ = expect_event(&mut stream, STEP, |event| {
        matches!(event, ServerEvent::InitialStateSyncComplete)
    })
    .await;
    stream
}

/// Registers `stream` as a voice host and waits until the server has
/// processed that request. Requests on one connection are handled in order,
/// so the reply to a following request proves the registration is in effect;
/// without this a request from another connection could overtake it.
async fn register_voice_host(stream: &mut ilium_transport::SessionStream) {
    write_frame(stream, &ClientRequest::RegisterVoiceTextReceiver)
        .await
        .expect("register voice host");
    write_frame(
        stream,
        &ClientRequest::GetPaneGoalStatus {
            request_id: 1,
            pane_id: ilium_core::ROOT_ID,
        },
    )
    .await
    .expect("barrier request");
    let _ = expect_event(stream, STEP, |event| {
        matches!(event, ServerEvent::PaneGoalStatusReported { .. })
    })
    .await;
}

fn submit(request_id: u64, start_voice: bool) -> ClientRequest {
    ClientRequest::SubmitVoiceText {
        request_id,
        sentences: vec!["  open the settings ".to_owned(), "close it".to_owned()],
        start_voice,
    }
}

#[tokio::test]
async fn typed_sentences_are_offered_to_the_registered_voice_client_and_the_answer_returns() {
    let session = "voice-text-relay";
    let server = TestServer::start(session).await;
    let mut voice_host = attach(&server, session).await;
    let mut requester = attach(&server, session).await;

    // Nothing hosts voice yet: a structured refusal, not a hang.
    write_frame(&mut requester, &submit(1, false))
        .await
        .expect("submit request");
    let event = expect_event(&mut requester, STEP, |event| {
        matches!(event, ServerEvent::VoiceTextResult { request_id: 1, .. })
    })
    .await;
    let ServerEvent::VoiceTextResult { result, .. } = event else {
        unreachable!()
    };
    assert_eq!(
        result.expect_err("no host").code,
        VoiceTextRejectionCode::NoVoiceClient
    );

    register_voice_host(&mut voice_host).await;

    // The host is offered the trimmed sentences (and only the host: the
    // requester's own stream never sees an offer).
    write_frame(&mut requester, &submit(2, true))
        .await
        .expect("submit request");
    let offer = expect_event(&mut voice_host, STEP, |event| {
        matches!(event, ServerEvent::VoiceTextOffered { .. })
    })
    .await;
    let ServerEvent::VoiceTextOffered {
        request_id,
        sentences,
        start_voice,
    } = offer
    else {
        unreachable!()
    };
    assert_eq!(request_id, 2);
    assert_eq!(sentences, ["open the settings", "close it"]);
    assert!(!start_voice, "the first offer never switches voice on");

    write_frame(
        &mut voice_host,
        &ClientRequest::AnswerVoiceText {
            request_id: 2,
            result: Ok(VoiceTextAccepted {
                sentence_count: 2,
                phase: VoiceTextPhase::Listening,
                started_voice: false,
            }),
        },
    )
    .await
    .expect("answer offer");
    let event = expect_event(&mut requester, STEP, |event| {
        matches!(event, ServerEvent::VoiceTextResult { request_id: 2, .. })
    })
    .await;
    let ServerEvent::VoiceTextResult { result, .. } = event else {
        unreachable!()
    };
    assert_eq!(
        result,
        Ok(VoiceTextAccepted {
            sentence_count: 2,
            phase: VoiceTextPhase::Listening,
            started_voice: false,
        })
    );

    // With voice off, the same request is offered a second time with
    // `start_voice` set, and the host's refusal reaches the requester intact.
    write_frame(&mut requester, &submit(3, true))
        .await
        .expect("submit request");
    for expected_start in [false, true] {
        let offer = expect_event(&mut voice_host, STEP, |event| {
            matches!(event, ServerEvent::VoiceTextOffered { request_id: 3, .. })
        })
        .await;
        let ServerEvent::VoiceTextOffered { start_voice, .. } = offer else {
            unreachable!()
        };
        assert_eq!(start_voice, expected_start);
        let reply: Result<VoiceTextAccepted, VoiceTextRejection> = Err(VoiceTextRejection::new(
            if expected_start {
                VoiceTextRejectionCode::VoiceUnavailable
            } else {
                VoiceTextRejectionCode::VoiceOff
            },
            if expected_start {
                "OpenAI API key must not be empty"
            } else {
                "voice control is off"
            },
        ));
        write_frame(
            &mut voice_host,
            &ClientRequest::AnswerVoiceText {
                request_id: 3,
                result: reply,
            },
        )
        .await
        .expect("answer offer");
    }
    let event = expect_event(&mut requester, STEP, |event| {
        matches!(event, ServerEvent::VoiceTextResult { request_id: 3, .. })
    })
    .await;
    let ServerEvent::VoiceTextResult { result, .. } = event else {
        unreachable!()
    };
    let rejection = result.expect_err("start failed");
    assert_eq!(rejection.code, VoiceTextRejectionCode::VoiceUnavailable);
    assert!(rejection.message.contains("API key"));
}

#[tokio::test]
async fn a_malformed_request_is_refused_with_the_invalid_request_code() {
    let session = "voice-text-invalid";
    let server = TestServer::start(session).await;
    let mut voice_host = attach(&server, session).await;
    let mut requester = attach(&server, session).await;
    register_voice_host(&mut voice_host).await;

    write_frame(
        &mut requester,
        &ClientRequest::SubmitVoiceText {
            request_id: 9,
            sentences: vec!["   ".to_owned()],
            start_voice: false,
        },
    )
    .await
    .expect("submit request");
    let event = expect_event(&mut requester, STEP, |event| {
        matches!(event, ServerEvent::VoiceTextResult { request_id: 9, .. })
    })
    .await;
    let ServerEvent::VoiceTextResult { result, .. } = event else {
        unreachable!()
    };
    assert_eq!(
        result.expect_err("invalid").code,
        VoiceTextRejectionCode::InvalidRequest
    );
}
