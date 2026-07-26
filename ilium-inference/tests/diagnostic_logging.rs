use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::time::Duration;

use ilium_inference::{
    provider_from_settings, ApiKeyProviderSettings, InferenceError, InferenceProviderKind,
    InferenceRequest, InferenceSettings,
};

/// Serves one deterministic provider failure and returns the complete request
/// captured by the fixture so the test proves transport payloads and durable
/// diagnostics both retain the complete non-credential exchange.
fn spawn_http_error_fixture() -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind local HTTP fixture");
    let address = listener.local_addr().expect("fixture address");
    let (request_sender, request_receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept local HTTP request");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("set request timeout");
        let mut request = Vec::new();
        let mut buffer = [0_u8; 4_096];
        loop {
            let read = stream.read(&mut buffer).expect("read HTTP request");
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..read]);
            let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n") else {
                continue;
            };
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
                .unwrap_or(0);
            if request.len() >= header_end + 4 + content_length {
                break;
            }
        }
        request_sender
            .send(String::from_utf8_lossy(&request).into_owned())
            .expect("capture HTTP request");
        let response_body =
            r#"{"error":{"message":"provider-overloaded-detail","code":"overloaded"}}"#;
        write!(
            stream,
            "HTTP/1.1 503 Service Unavailable\r\nContent-Type: application/json\r\nSet-Cookie: session=provider-secret-cookie\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response_body}",
            response_body.len()
        )
        .expect("write HTTP response");
    });
    (format!("http://{address}"), request_receiver)
}

#[test]
fn file_log_records_complete_llm_and_http_failure_context_without_credentials() {
    let directory = tempfile::tempdir().expect("temporary log directory");
    let log_path = directory.path().join("diagnostic-log.txt");
    ilium_logging::initialize(&log_path, true, "inference-diagnostic-test")
        .expect("initialize diagnostics");

    let (base_url, request_receiver) = spawn_http_error_fixture();
    let api_key = "integration-secret-api-key";
    let settings = InferenceSettings {
        selected_provider: InferenceProviderKind::OpenAi,
        openai: ApiKeyProviderSettings {
            base_url,
            api_key: api_key.to_owned(),
            model: "fixture-model".to_owned(),
        },
        ..InferenceSettings::default()
    };
    let request = InferenceRequest {
        system_prompt: "diagnostic-system-prompt".to_owned(),
        user_prompt: "diagnostic-user-prompt".to_owned(),
        max_tokens: 73,
    };

    let result = provider_from_settings(&settings).complete(&request);

    assert!(matches!(
        result,
        Err(InferenceError::Http { status: 503, ref message })
            if message.contains("provider-overloaded-detail")
    ));
    let captured_request = request_receiver.recv().expect("captured HTTP request");
    assert!(captured_request.contains("diagnostic-system-prompt"));
    assert!(captured_request.contains("diagnostic-user-prompt"));
    assert!(captured_request.contains(api_key));

    let log = std::fs::read_to_string(&log_path).expect("read diagnostic log");
    assert!(log.contains("LLM inference started"));
    assert!(log.contains("fixture-model"));
    assert!(log.contains("max_tokens=73"));
    assert!(log.contains("system_prompt_characters="));
    assert!(log.contains("user_prompt_characters="));
    assert!(log.contains("HTTP request started"));
    assert!(log.contains("HTTP request failed"));
    assert!(log.contains("status=503"));
    assert!(log.contains("LLM inference failed"));
    assert!(log.contains("error_kind=\"http\""));
    assert!(log.contains("<redacted>"));
    assert!(log.contains("diagnostic-system-prompt"));
    assert!(log.contains("diagnostic-user-prompt"));
    assert!(log.contains("provider-overloaded-detail"));
    assert!(!log.contains(api_key));
    assert!(!log.contains("provider-secret-cookie"));
}
