//! Typed sentences through the real Realtime adapter against a local scripted
//! server. No network, no API key, no audio device.
//!
//! The adapter is pointed at a loopback WebSocket with
//! `ILIUM_VOICE_REALTIME_URL` and runs with `ILIUM_VOICE_AUDIO=none`
//! (see `openai.rs` and `audio.rs`). Both variables are process-global, so
//! every scenario lives in this one test function and runs sequentially.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use ilium_voice::{
    ReasoningEffort, VadEagerness, VoiceCommand, VoiceConnectionState, VoiceEvent, VoiceInputMode,
    VoiceModel, VoiceName, VoiceRuntimeConfig, VoiceService, VoiceToolOutput,
};
use secrecy::SecretString;
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

const SILENCE_WINDOW: Duration = Duration::from_millis(400);
const STEP_TIMEOUT: Duration = Duration::from_secs(10);

/// One scripted provider connection: everything the client sends arrives on
/// `received`, everything pushed into `script` is sent to the client.
struct MockRealtime {
    received: mpsc::UnboundedReceiver<Value>,
    script: mpsc::UnboundedSender<Value>,
    task: tokio::task::JoinHandle<()>,
}

impl MockRealtime {
    async fn start() -> (Self, String) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback");
        let address = listener.local_addr().expect("local address");
        let (received_tx, received) = mpsc::unbounded_channel();
        let (script, mut script_rx) = mpsc::unbounded_channel::<Value>();
        let task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept the adapter");
            let mut socket = tokio_tungstenite::accept_async(stream)
                .await
                .expect("websocket handshake");
            loop {
                tokio::select! {
                    incoming = socket.next() => {
                        let Some(Ok(Message::Text(text))) = incoming else { return };
                        let event: Value = serde_json::from_str(&text).expect("client JSON");
                        let is_session_update = event["type"] == "session.update";
                        let _ = received_tx.send(event);
                        if is_session_update {
                            socket
                                .send(Message::Text(json!({"type": "session.updated"}).to_string().into()))
                                .await
                                .expect("acknowledge session");
                        }
                    }
                    outgoing = script_rx.recv() => {
                        let Some(event) = outgoing else { return };
                        socket
                            .send(Message::Text(event.to_string().into()))
                            .await
                            .expect("push scripted event");
                    }
                }
            }
        });
        (
            Self {
                received,
                script,
                task,
            },
            format!("ws://{address}/v1/realtime"),
        )
    }

    /// The next client event, skipping nothing.
    async fn next_client_event(&mut self) -> Value {
        tokio::time::timeout(STEP_TIMEOUT, self.received.recv())
            .await
            .expect("timed out waiting for a client event")
            .expect("mock connection closed")
    }

    async fn expect_no_client_event(&mut self, why: &str) {
        if let Ok(Some(event)) = tokio::time::timeout(SILENCE_WINDOW, self.received.recv()).await {
            panic!("unexpected client event while {why}: {event}");
        }
    }

    /// Reads the user-message + `response.create` pair a typed turn produces.
    async fn expect_typed_turn(&mut self, text: &str) {
        let item = self.next_client_event().await;
        assert_eq!(item["type"], "conversation.item.create", "{item}");
        assert_eq!(item["item"]["type"], "message", "{item}");
        assert_eq!(item["item"]["role"], "user", "{item}");
        assert_eq!(item["item"]["content"][0]["type"], "input_text", "{item}");
        assert_eq!(item["item"]["content"][0]["text"], text, "{item}");
        let response = self.next_client_event().await;
        assert_eq!(response["type"], "response.create", "{response}");
    }

    fn finish_response(&self, output: Vec<Value>) {
        self.script
            .send(json!({"type": "response.done", "response": {"output": output}}))
            .expect("script channel open");
    }
}

fn config() -> VoiceRuntimeConfig {
    VoiceRuntimeConfig {
        api_key: SecretString::from("test-key-never-leaves-loopback".to_owned()),
        model: VoiceModel::GptRealtimeMini,
        voice: VoiceName::Marin,
        reasoning_effort: ReasoningEffort::Low,
        input_mode: VoiceInputMode::SemanticVad,
        vad_eagerness: VadEagerness::Auto,
        input_device_name: None,
        output_device_name: None,
        output_volume_percent: 0,
        instructions: "typed text test".to_owned(),
    }
}

async fn wait_until_listening(service: &mut VoiceService) {
    tokio::time::timeout(STEP_TIMEOUT, async {
        loop {
            match service.next_event().await.expect("voice event channel") {
                VoiceEvent::StateChanged(VoiceConnectionState::Listening) => break,
                VoiceEvent::StateChanged(VoiceConnectionState::Failed(error)) => {
                    panic!("startup failed: {error}")
                }
                _ => {}
            }
        }
    })
    .await
    .expect("session should become ready");
}

async fn next_tool_invocation(service: &mut VoiceService) -> ilium_voice::VoiceToolInvocation {
    tokio::time::timeout(STEP_TIMEOUT, async {
        loop {
            if let VoiceEvent::ToolInvocations(mut invocations) =
                service.next_event().await.expect("voice event channel")
            {
                return invocations.remove(0);
            }
        }
    })
    .await
    .expect("tool invocation should be delivered")
}

fn function_call(call_id: &str, name: &str, arguments: &str) -> Value {
    json!({"type": "function_call", "call_id": call_id, "name": name, "arguments": arguments})
}

#[tokio::test]
async fn typed_sentences_reach_the_model_as_ordered_user_turns() {
    let (mut mock, url) = MockRealtime::start().await;
    std::env::set_var("ILIUM_VOICE_REALTIME_URL", &url);
    std::env::set_var("ILIUM_VOICE_AUDIO", "none");

    let mut service = VoiceService::start(config(), Vec::new()).expect("start voice actor");
    let sender = service.command_sender();
    assert_eq!(mock.next_client_event().await["type"], "session.update");
    wait_until_listening(&mut service).await;

    // Idle session: a typed sentence becomes a user message and a response
    // request immediately, with the text trimmed.
    sender
        .send(VoiceCommand::SendText("  open the settings  ".to_owned()))
        .await
        .expect("send first sentence");
    mock.expect_typed_turn("open the settings").await;

    // Blank text is ignored, and a sentence sent while the model is still
    // answering waits its turn rather than colliding with the active response.
    sender
        .send(VoiceCommand::SendText("   ".to_owned()))
        .await
        .expect("send blank sentence");
    sender
        .send(VoiceCommand::SendText("then close it".to_owned()))
        .await
        .expect("send second sentence");
    sender
        .send(VoiceCommand::SendText("and go back".to_owned()))
        .await
        .expect("send third sentence");
    mock.expect_no_client_event("the first response is still active")
        .await;

    // Each completed response releases exactly one queued sentence, in order.
    mock.finish_response(Vec::new());
    mock.expect_typed_turn("then close it").await;
    mock.expect_no_client_event("the second response is still active")
        .await;
    mock.finish_response(Vec::new());
    mock.expect_typed_turn("and go back").await;

    // A response that calls a tool holds later sentences until the tool
    // output and the model's follow-up are done: the application's own
    // `response.create` must not race a typed turn.
    mock.finish_response(vec![function_call(
        "call-1",
        "ilium_ui",
        "{\"action\":\"open_help\"}",
    )]);
    let invocation = next_tool_invocation(&mut service).await;
    assert_eq!(invocation.name, "ilium_ui");
    sender
        .send(VoiceCommand::SendText("finally, say hi".to_owned()))
        .await
        .expect("send sentence behind a tool call");
    mock.expect_no_client_event("tool outputs are still pending")
        .await;

    sender
        .send(VoiceCommand::SubmitToolOutputs(vec![VoiceToolOutput {
            call_id: invocation.call_id,
            result: json!({"status": "ok"}),
            request_follow_up: true,
            terminate_session_after_delivery: false,
        }]))
        .await
        .expect("submit tool output");
    let output = mock.next_client_event().await;
    assert_eq!(output["item"]["type"], "function_call_output", "{output}");
    assert_eq!(mock.next_client_event().await["type"], "response.create");
    mock.expect_no_client_event("the follow-up response is still active")
        .await;
    mock.finish_response(Vec::new());
    mock.expect_typed_turn("finally, say hi").await;

    // A tool result that asks for no follow-up must still release a sentence
    // that queued behind it, since no `response.done` will follow.
    mock.finish_response(vec![function_call("call-2", "ilium_ui", "{}")]);
    let invocation = next_tool_invocation(&mut service).await;
    sender
        .send(VoiceCommand::SendText("one more thing".to_owned()))
        .await
        .expect("send sentence behind a silent tool call");
    mock.expect_no_client_event("tool outputs are still pending")
        .await;
    sender
        .send(VoiceCommand::SubmitToolOutputs(vec![VoiceToolOutput {
            call_id: invocation.call_id,
            result: json!({"status": "ok"}),
            request_follow_up: false,
            terminate_session_after_delivery: false,
        }]))
        .await
        .expect("submit silent tool output");
    assert_eq!(
        mock.next_client_event().await["item"]["type"],
        "function_call_output"
    );
    mock.expect_typed_turn("one more thing").await;

    service.shutdown().await;
    mock.task.abort();
    std::env::remove_var("ILIUM_VOICE_REALTIME_URL");
    std::env::remove_var("ILIUM_VOICE_AUDIO");
}
