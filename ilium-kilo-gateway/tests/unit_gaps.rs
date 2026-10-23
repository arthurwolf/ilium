//! Unit gaps for the public Kilo Gateway adapter contract.
//!
//! Every test drives `KiloGatewayClient` or a public constructor. Network
//! behaviour is checked against local TCP fixtures, never the real gateway.

use std::ffi::OsString;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use ilium_kilo_gateway::{
    choose_random_paid_proxy, ChatMessage, CompletionRequest, CompletionStreamEvent, GatewayError,
    KiloGatewayClient, PaidProxy, RetryPolicy, DEFAULT_BASE_URL, DEFAULT_FREE_MODEL,
    FALLBACK_FREE_MODELS, UNKNOWN_MODEL_MAX_OUTPUT_TOKENS,
};

const MIB: usize = 1024 * 1024;
const COMPLETION_JSON: &str =
    r#"{"choices":[{"message":{"content":"ok"},"finish_reason":"stop"}]}"#;

/// Serialises every test that changes `HOME`, because the scanner reads it
/// through the process-global environment.
static HOME_LOCK: Mutex<()> = Mutex::new(());

struct Fixture {
    base_url: String,
    connections: Arc<AtomicUsize>,
    requests: mpsc::Receiver<String>,
}

struct TestHome {
    path: PathBuf,
}

impl TestHome {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ilium-kilo-gateway-gaps-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create test home directory");
        Self { path }
    }

    fn write_publication(&self, bytes: &[u8]) {
        let directory = self.path.join(".model-scanner");
        fs::create_dir_all(&directory).expect("create scanner directory");
        fs::write(directory.join("scan.json"), bytes).expect("write scanner publication");
    }
}

impl Drop for TestHome {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// Points `HOME` at a test directory and restores the previous value on drop.
struct HomeOverride {
    previous: Option<OsString>,
    _serialised: MutexGuard<'static, ()>,
}

impl HomeOverride {
    fn set(home: &Path) -> Self {
        let serialised = HOME_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
        let previous = std::env::var_os("HOME");
        std::env::set_var("HOME", home);
        Self {
            previous,
            _serialised: serialised,
        }
    }
}

impl Drop for HomeOverride {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
    }
}

fn content_length(head: &str) -> usize {
    head.lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            if name.trim().eq_ignore_ascii_case("content-length") {
                value.trim().parse::<usize>().ok()
            } else {
                None
            }
        })
        .unwrap_or(0)
}

/// Reads one complete request (headers plus `Content-Length` body) and
/// returns it as text, so the fixture never closes with unread bytes.
fn read_request(stream: &mut TcpStream) -> String {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut received = Vec::new();
    let mut chunk = [0_u8; 4_096];
    while let Ok(read) = stream.read(&mut chunk) {
        if read == 0 {
            break;
        }
        received.extend_from_slice(&chunk[..read]);
        let Some(header_end) = received.windows(4).position(|window| window == b"\r\n\r\n") else {
            continue;
        };
        let head = String::from_utf8_lossy(&received[..header_end]).into_owned();
        if received.len() >= header_end + 4 + content_length(&head) {
            break;
        }
    }
    String::from_utf8_lossy(&received).into_owned()
}

/// Serves `body` with `status_line` to every connection and records each
/// request, so tests can count attempts and inspect what was sent.
fn spawn_fixture(status_line: &'static str, body: Vec<u8>) -> Fixture {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind local fixture listener");
    let address = listener.local_addr().expect("fixture listener address");
    let connections = Arc::new(AtomicUsize::new(0));
    let connection_count = Arc::clone(&connections);
    let (request_sender, requests) = mpsc::channel();
    thread::spawn(move || {
        for incoming in listener.incoming() {
            let Ok(mut stream) = incoming else {
                break;
            };
            connection_count.fetch_add(1, Ordering::SeqCst);
            let _ = request_sender.send(read_request(&mut stream));
            let head = format!(
                "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
        }
    });
    Fixture {
        base_url: format!("http://{address}"),
        connections,
        requests,
    }
}

/// Accepts connections without answering. With `keep_open` the sockets stay
/// alive, so the client can only finish by timing out. Without it each socket
/// is dropped at once, so the client sees a failed transport.
fn spawn_silent_fixture(keep_open: bool) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind silent fixture listener");
    let address = listener.local_addr().expect("silent fixture address");
    let connections = Arc::new(AtomicUsize::new(0));
    let connection_count = Arc::clone(&connections);
    thread::spawn(move || {
        let mut held_open = Vec::new();
        for incoming in listener.incoming() {
            let Ok(stream) = incoming else {
                break;
            };
            connection_count.fetch_add(1, Ordering::SeqCst);
            if keep_open {
                held_open.push(stream);
            }
        }
    });
    (format!("http://{address}"), connections)
}

fn retry_policy(max_attempts: u8) -> RetryPolicy {
    RetryPolicy {
        max_attempts,
        initial_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(1),
    }
}

fn client_for(base_url: &str, max_attempts: u8) -> KiloGatewayClient {
    KiloGatewayClient::new(base_url, retry_policy(max_attempts))
}

fn short_request() -> CompletionRequest {
    CompletionRequest::new("kilo-auto/free", vec![ChatMessage::user("hi")], 16)
}

fn padded_body(json: &str, total_bytes: usize) -> Vec<u8> {
    assert!(
        json.len() <= total_bytes,
        "fixture JSON must fit the target size"
    );
    let mut body = json.as_bytes().to_vec();
    body.resize(total_bytes, b' ');
    body
}

/// A single SSE comment line whose total length, newline included, is exactly
/// `total_bytes`. Comment lines carry no `data:` prefix, so they emit nothing.
fn comment_stream_of_length(total_bytes: usize) -> Vec<u8> {
    let mut body = Vec::with_capacity(total_bytes);
    body.push(b':');
    body.resize(total_bytes - 1, b'x');
    body.push(b'\n');
    body
}

fn sse_stream_body() -> Vec<u8> {
    concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n",
        "data: {\"choices\":[],\"usage\":{\"completion_tokens\":5}}\n\n",
        "data: [DONE]\n\n",
    )
    .as_bytes()
    .to_vec()
}

fn proxy_with_credentials(username: &str, password: &str) -> PaidProxy {
    PaidProxy {
        ip: "10.0.0.1".to_string(),
        port: 8080,
        protocol: "http".to_string(),
        username: username.to_string(),
        password: password.to_string(),
    }
}

fn publication_v1(
    recommended_base: &str,
    model_id: &str,
    context_length: u64,
    max_output_tokens: u64,
) -> Vec<u8> {
    serde_json::json!({
        "schema_version": 1,
        "recommendations": {"base": recommended_base},
        "models": [{
            "id": model_id,
            "context_length": context_length,
            "max_output_tokens": max_output_tokens,
            "text_completed": 10,
            "text_score": 10
        }]
    })
    .to_string()
    .into_bytes()
}

/// A schema-3 publication whose agentic evidence is internally consistent, so
/// only the context-length and `score_max` relationship decides acceptance.
fn scan_v3(context_length: u64, score_max: u64) -> Vec<u8> {
    let answers: Vec<serde_json::Value> = (0..15)
        .map(|index| {
            serde_json::json!({
                "question_id": format!("agentic-{index}"),
                "status": "correct",
                "tool_calls": 1
            })
        })
        .collect();
    serde_json::json!({
        "schema_version": 3,
        "benchmark_version": "2026-10-18.1",
        "recommendations": {"base": "measured"},
        "models": [{
            "id": "measured",
            "context_length": context_length,
            "max_output_tokens": 4096,
            "text_completed": 20,
            "text_score": 20,
            "visual_completed": 10,
            "visual_score": 10,
            "long_context_score": 0,
            "general_score": 30,
            "score": 45,
            "score_max": score_max,
            "agentic": answers,
            "agentic_score": 15,
            "agentic_completed": 15
        }]
    })
    .to_string()
    .into_bytes()
}

struct KiloRun {
    result: Result<(), GatewayError>,
    proxy_connections: usize,
    proxy_request: Option<String>,
}

/// Streams through the canonical Kilo base URL, which requires a paid proxy.
/// The proxy is a local fixture, so any scanner acceptance shows up as a
/// proxy connection and any rejection leaves the fixture untouched.
fn run_kilo_stream(home: &TestHome) -> KiloRun {
    let _home_env = HomeOverride::set(&home.path);
    let proxy = spawn_fixture("502 Bad Gateway", Vec::new());
    let client = KiloGatewayClient::new(DEFAULT_BASE_URL, retry_policy(1))
        .with_proxy_url(proxy.base_url.as_str());
    let mut ignore_event = |_event: CompletionStreamEvent| true;
    let result = client.stream_text(&short_request(), &mut ignore_event);
    KiloRun {
        result,
        proxy_connections: proxy.connections.load(Ordering::SeqCst),
        proxy_request: proxy.requests.try_recv().ok(),
    }
}

fn assert_rejected_by_scanner(run: &KiloRun, expected_message: &str) {
    match &run.result {
        Err(GatewayError::InvalidResponse(message)) => assert_eq!(message, expected_message),
        other => panic!("expected InvalidResponse {expected_message:?}, got {other:?}"),
    }
    assert_eq!(
        run.proxy_connections, 0,
        "a rejected publication must not reach the proxy"
    );
}

fn assert_accepted_by_scanner(run: &KiloRun) {
    assert!(
        matches!(run.result, Err(GatewayError::Transport(_))),
        "an accepted publication should fail at the proxy, got {:?}",
        run.result
    );
    assert!(
        run.proxy_connections >= 1,
        "an accepted publication should reach the proxy"
    );
}

fn collect_stream(
    client: &KiloGatewayClient,
    request: &CompletionRequest,
) -> (Result<(), GatewayError>, Vec<CompletionStreamEvent>) {
    let mut events = Vec::new();
    let result = {
        let mut on_event = |event: CompletionStreamEvent| {
            events.push(event);
            true
        };
        client.stream_text(request, &mut on_event)
    };
    (result, events)
}

#[test]
fn system_message_sets_system_role_and_str_content() {
    let message = ChatMessage::system("be brief");

    assert_eq!(message.role, "system");
    assert_eq!(message.content, "be brief");
}

#[test]
fn system_message_accepts_owned_string_content() {
    let message = ChatMessage::system(String::from("owned instructions"));

    assert_eq!(message.role, "system");
    assert_eq!(message.content, "owned instructions");
}

#[test]
fn system_message_accepts_empty_content() {
    let message = ChatMessage::system("");

    assert_eq!(message.role, "system");
    assert!(message.content.is_empty(), "empty content must be kept");
}

#[test]
fn system_message_preserves_non_ascii_content() {
    let text = "Grüße, 日本語";
    let message = ChatMessage::system(text);

    assert_eq!(message.content, text);
    assert_eq!(message.content.chars().count(), text.chars().count());
}

#[test]
fn with_default_free_model_uses_default_route_budget_and_messages() {
    let request = CompletionRequest::with_default_free_model(vec![
        ChatMessage::system("sys"),
        ChatMessage::user("usr"),
    ]);

    assert_eq!(request.model, DEFAULT_FREE_MODEL);
    assert_eq!(request.max_tokens, UNKNOWN_MODEL_MAX_OUTPUT_TOKENS);
    assert_eq!(request.messages.len(), 2);
    assert_eq!(request.messages[0].content, "sys");
    assert_eq!(request.messages[1].content, "usr");
    assert_eq!(request.temperature, 0.0);
}

#[test]
fn new_keeps_explicit_model_messages_and_budget_unchanged() {
    let request = CompletionRequest::new(
        "vendor/explicit-model",
        vec![ChatMessage::user("question")],
        4_096,
    );

    assert_eq!(request.model, "vendor/explicit-model");
    assert_eq!(request.messages.len(), 1);
    assert_eq!(request.messages[0].role, "user");
    assert_eq!(request.messages[0].content, "question");
    assert_eq!(request.max_tokens, 4_096);
    assert_eq!(request.temperature, 0.0);
}

#[test]
fn new_accepts_an_empty_message_list() {
    let request = CompletionRequest::new("vendor/model", Vec::new(), 1);

    assert!(request.messages.is_empty());
    assert_eq!(request.max_tokens, 1);
}

#[test]
fn new_keeps_zero_and_maximum_token_budgets() {
    let zero = CompletionRequest::new("vendor/model", Vec::new(), 0);
    let maximum = CompletionRequest::new("vendor/model", Vec::new(), u32::MAX);

    assert_eq!(zero.max_tokens, 0);
    assert_eq!(maximum.max_tokens, u32::MAX);
}

#[test]
fn fallback_free_models_keep_the_documented_order() {
    assert_eq!(FALLBACK_FREE_MODELS, ["kilo-auto/free", "openrouter/free"]);
    assert_eq!(FALLBACK_FREE_MODELS[0], DEFAULT_FREE_MODEL);
}

#[test]
fn fallback_free_model_ids_are_provider_slash_name_routes() {
    for model_id in FALLBACK_FREE_MODELS {
        let (provider, name) = model_id
            .split_once('/')
            .expect("fallback model id must contain a slash");

        assert!(!provider.is_empty(), "{model_id} has an empty provider");
        assert!(!name.is_empty(), "{model_id} has an empty model name");
        assert!(
            !model_id.contains(char::is_whitespace),
            "{model_id} must not contain whitespace"
        );
    }
}

#[test]
fn default_base_url_is_the_canonical_kilo_gateway() {
    assert_eq!(DEFAULT_BASE_URL, "https://api.kilo.ai/api/gateway");
}

#[test]
fn paid_proxy_default_has_empty_endpoint_and_no_credentials() {
    let proxy = PaidProxy::default();

    assert_eq!(proxy.ip, "");
    assert_eq!(proxy.port, 0);
    assert_eq!(proxy.protocol, "http");
    assert!(proxy.username.is_empty());
    assert!(proxy.password.is_empty());
    assert_eq!(proxy.connect_url(), "http://:0");
}

#[test]
fn choose_random_paid_proxy_returns_the_default_entry_when_it_is_the_only_one() {
    let proxies = [PaidProxy::default()];

    assert_eq!(choose_random_paid_proxy(&proxies), Some(&proxies[0]));
}

#[test]
fn connect_url_omits_credentials_unless_both_fields_are_set() {
    assert_eq!(
        proxy_with_credentials("", "").connect_url(),
        "http://10.0.0.1:8080"
    );
    assert_eq!(
        proxy_with_credentials("user", "").connect_url(),
        "http://10.0.0.1:8080"
    );
    assert_eq!(
        proxy_with_credentials("", "secret").connect_url(),
        "http://10.0.0.1:8080"
    );
}

#[test]
fn connect_url_passes_unreserved_credential_bytes_through() {
    let url = proxy_with_credentials("az-AZ09._~", "pw").connect_url();

    assert_eq!(url, "http://az-AZ09._~:pw@10.0.0.1:8080");
}

#[test]
fn connect_url_percent_encodes_reserved_credential_characters() {
    let url = proxy_with_credentials("a@b", "p:q/r?s#t%u v").connect_url();

    assert_eq!(url, "http://a%40b:p%3Aq%2Fr%3Fs%23t%25u%20v@10.0.0.1:8080");
}

#[test]
fn connect_url_percent_encodes_every_byte_of_multibyte_utf8() {
    let url = proxy_with_credentials("é", "日").connect_url();

    assert_eq!(url, "http://%C3%A9:%E6%97%A5@10.0.0.1:8080");
}

#[test]
fn connect_url_keeps_the_authority_when_password_has_colon_and_at_sign() {
    let url = proxy_with_credentials("user", "a:b@c/d").connect_url();

    assert_eq!(url, "http://user:a%3Ab%40c%2Fd@10.0.0.1:8080");
    assert!(
        ureq::Proxy::new(&url).is_ok(),
        "the encoded URL must still parse as a proxy: {url}"
    );
}

#[test]
fn retryable_http_statuses_are_retried_until_attempts_run_out() {
    let cases: [(&'static str, u16); 5] = [
        ("429 Too Many Requests", 429),
        ("500 Internal Server Error", 500),
        ("502 Bad Gateway", 502),
        ("503 Service Unavailable", 503),
        ("504 Gateway Timeout", 504),
    ];
    for (status_line, status) in cases {
        let fixture = spawn_fixture(status_line, br#"{"error":"busy"}"#.to_vec());
        let result = client_for(&fixture.base_url, 3).complete_text(&short_request());
        let observed_status = match &result {
            Err(GatewayError::Http { status, .. }) => Some(*status),
            _ => None,
        };
        let attempts = fixture.connections.load(Ordering::SeqCst);

        assert_eq!(observed_status, Some(status), "HTTP {status} must surface");
        assert_eq!(attempts, 3, "HTTP {status} must be retried to the limit");
    }
}

#[test]
fn client_error_statuses_400_403_404_and_501_are_not_retried() {
    let cases: [(&'static str, u16); 4] = [
        ("400 Bad Request", 400),
        ("403 Forbidden", 403),
        ("404 Not Found", 404),
        ("501 Not Implemented", 501),
    ];
    for (status_line, status) in cases {
        let fixture = spawn_fixture(status_line, br#"{"error":"no"}"#.to_vec());
        let result = client_for(&fixture.base_url, 3).complete_text(&short_request());
        let observed_status = match &result {
            Err(GatewayError::Http { status, .. }) => Some(*status),
            _ => None,
        };
        let attempts = fixture.connections.load(Ordering::SeqCst);

        assert_eq!(observed_status, Some(status), "HTTP {status} must surface");
        assert_eq!(attempts, 1, "HTTP {status} must not be retried");
    }
}

#[test]
fn http_401_is_not_retried() {
    let fixture = spawn_fixture("401 Unauthorized", br#"{"error":"auth"}"#.to_vec());

    let result = client_for(&fixture.base_url, 3).complete_text(&short_request());

    assert!(
        matches!(result, Err(GatewayError::Http { status: 401, .. })),
        "401 must surface as an Http error, got {result:?}"
    );
    assert_eq!(fixture.connections.load(Ordering::SeqCst), 1);
}

#[test]
fn transport_failures_are_retried_until_attempts_run_out() {
    let (base_url, connections) = spawn_silent_fixture(false);

    let result = client_for(&base_url, 3).complete_text(&short_request());

    assert!(
        matches!(result, Err(GatewayError::Transport(_))),
        "a dropped connection must be a Transport error, got {result:?}"
    );
    assert_eq!(connections.load(Ordering::SeqCst), 3);
}

#[test]
fn invalid_response_bodies_are_retried_until_attempts_run_out() {
    let fixture = spawn_fixture("200 OK", b"not json at all".to_vec());

    let result = client_for(&fixture.base_url, 3).complete_text(&short_request());

    assert!(
        matches!(result, Err(GatewayError::InvalidResponse(_))),
        "a non-JSON 200 body must be InvalidResponse, got {result:?}"
    );
    assert_eq!(fixture.connections.load(Ordering::SeqCst), 3);
}

#[test]
fn invalid_proxy_is_not_retried_and_opens_no_connection() {
    let fixture = spawn_fixture("200 OK", COMPLETION_JSON.as_bytes().to_vec());
    let slow_policy = RetryPolicy {
        max_attempts: 3,
        initial_delay: Duration::from_secs(10),
        max_delay: Duration::from_secs(10),
    };
    let client =
        KiloGatewayClient::new(fixture.base_url.as_str(), slow_policy).with_proxy_url("not a url");

    let started = Instant::now();
    let result = client.complete_text(&short_request());
    let elapsed = started.elapsed();

    assert!(
        matches!(result, Err(GatewayError::InvalidProxy(_))),
        "a malformed proxy must be InvalidProxy, got {result:?}"
    );
    assert_eq!(fixture.connections.load(Ordering::SeqCst), 0);
    assert!(
        elapsed < Duration::from_secs(5),
        "InvalidProxy must not sleep through the 10 s retry delay: {elapsed:?}"
    );
}

#[test]
fn request_timeout_expiry_returns_a_transport_error_without_a_response() {
    let (base_url, connections) = spawn_silent_fixture(true);
    let client = client_for(&base_url, 1).with_request_timeout(Duration::from_millis(200));

    let started = Instant::now();
    let result = client.complete_text(&short_request());
    let elapsed = started.elapsed();

    assert!(
        matches!(result, Err(GatewayError::Transport(_))),
        "a timed-out request must be a Transport error, got {result:?}"
    );
    assert_eq!(connections.load(Ordering::SeqCst), 1);
    assert!(
        elapsed < Duration::from_secs(10),
        "the 200 ms timeout was not applied: {elapsed:?}"
    );
}

#[test]
fn request_timeout_expiry_is_retried_when_attempts_remain() {
    let (base_url, connections) = spawn_silent_fixture(true);
    let client = client_for(&base_url, 2).with_request_timeout(Duration::from_millis(200));

    let result = client.complete_text(&short_request());

    assert!(
        matches!(result, Err(GatewayError::Transport(_))),
        "the final timed-out attempt must be a Transport error, got {result:?}"
    );
    assert_eq!(connections.load(Ordering::SeqCst), 2);
}

#[test]
fn zero_request_timeout_fails_as_transport_without_hanging() {
    let (base_url, _connections) = spawn_silent_fixture(false);
    let client = client_for(&base_url, 1).with_request_timeout(Duration::ZERO);

    let result = client.complete_text(&short_request());

    assert!(
        matches!(result, Err(GatewayError::Transport(_))),
        "a zero timeout must end in a Transport error, got {result:?}"
    );
}

#[test]
fn very_long_request_timeout_still_completes_a_normal_request() {
    let fixture = spawn_fixture("200 OK", COMPLETION_JSON.as_bytes().to_vec());
    let hundred_years = Duration::from_secs(100 * 365 * 24 * 60 * 60);
    let client = client_for(&fixture.base_url, 1).with_request_timeout(hundred_years);

    match client.complete_text(&short_request()) {
        Ok(text) => assert_eq!(text, "ok"),
        Err(error) => panic!("a long timeout must not break a normal call: {error}"),
    }
}

#[test]
fn completion_body_at_the_two_mib_limit_is_accepted() {
    let fixture = spawn_fixture("200 OK", padded_body(COMPLETION_JSON, 2 * MIB));

    match client_for(&fixture.base_url, 1).complete_text(&short_request()) {
        Ok(text) => assert_eq!(text, "ok"),
        Err(error) => panic!("a 2 MiB completion body must be accepted: {error}"),
    }
}

#[test]
fn completion_body_one_byte_over_two_mib_is_rejected_not_truncated() {
    let fixture = spawn_fixture("200 OK", padded_body(COMPLETION_JSON, 2 * MIB + 1));

    let result = client_for(&fixture.base_url, 1).complete_text(&short_request());

    assert!(
        matches!(result, Err(GatewayError::Transport(_))),
        "a body one byte over 2 MiB must fail, got {result:?}"
    );
}

#[test]
fn stream_at_the_sixteen_mib_limit_is_accepted() {
    let fixture = spawn_fixture("200 OK", comment_stream_of_length(16 * MIB));
    let client = client_for(&fixture.base_url, 1);

    let (result, events) = collect_stream(&client, &short_request());

    assert!(
        result.is_ok(),
        "a 16 MiB stream must be accepted: {result:?}"
    );
    assert!(events.is_empty(), "comment lines must emit no events");
}

#[test]
fn stream_one_byte_over_sixteen_mib_is_rejected() {
    let fixture = spawn_fixture("200 OK", comment_stream_of_length(16 * MIB + 1));
    let client = client_for(&fixture.base_url, 1);

    let (result, _events) = collect_stream(&client, &short_request());

    match result {
        Err(GatewayError::InvalidResponse(message)) => {
            assert!(
                message.contains("16 MiB"),
                "the limit error must name the limit: {message}"
            );
        }
        other => panic!("a stream one byte over 16 MiB must fail, got {other:?}"),
    }
}

#[test]
fn model_catalog_at_the_eight_mib_limit_is_accepted() {
    let fixture = spawn_fixture("200 OK", padded_body(r#"{"data":[]}"#, 8 * MIB));

    match client_for(&fixture.base_url, 1).list_free_models() {
        Ok(models) => assert!(models.is_empty(), "an empty catalog must list no models"),
        Err(error) => panic!("an 8 MiB catalog must be accepted: {error}"),
    }
}

#[test]
fn model_catalog_one_byte_over_eight_mib_is_rejected() {
    let fixture = spawn_fixture("200 OK", padded_body(r#"{"data":[]}"#, 8 * MIB + 1));

    let result = client_for(&fixture.base_url, 1).list_free_models();

    assert!(
        matches!(result, Err(GatewayError::Transport(_))),
        "a catalog one byte over 8 MiB must fail, got {result:?}"
    );
}

#[test]
fn default_client_refuses_direct_completion_without_a_paid_proxy() {
    let client = KiloGatewayClient::default();

    let result = client.complete_text(&short_request());

    match result {
        Err(GatewayError::InvalidProxy(message)) => assert!(
            message.contains("paid proxy"),
            "the refusal must name the paid proxy requirement: {message}"
        ),
        other => panic!("the default client must refuse a direct call, got {other:?}"),
    }
}

#[test]
fn default_client_refuses_direct_model_listing_without_a_paid_proxy() {
    let result = KiloGatewayClient::default().list_free_models();

    assert!(
        matches!(result, Err(GatewayError::InvalidProxy(_))),
        "the default client must refuse a direct catalog call, got {result:?}"
    );
}

#[test]
fn default_client_refuses_direct_stream_without_a_paid_proxy() {
    let client = KiloGatewayClient::default();
    let mut ignore_event = |_event: CompletionStreamEvent| true;

    let result = client.stream_text(&short_request(), &mut ignore_event);

    assert!(
        matches!(result, Err(GatewayError::InvalidProxy(_))),
        "the default client must refuse a direct stream, got {result:?}"
    );
}

#[test]
fn custom_base_url_streams_text_and_usage_events_without_reading_the_scanner() {
    // HOME has no publication, so any scanner read would fail the stream.
    let home = TestHome::new("custom-base-stream");
    let _home_env = HomeOverride::set(&home.path);
    let fixture = spawn_fixture("200 OK", sse_stream_body());
    let client = client_for(&fixture.base_url, 1);

    let (result, events) = collect_stream(&client, &short_request());

    assert!(result.is_ok(), "the stream must succeed: {result:?}");
    assert_eq!(
        events,
        vec![
            CompletionStreamEvent::TextDelta("Hel".to_string()),
            CompletionStreamEvent::TextDelta("lo".to_string()),
            CompletionStreamEvent::OutputTokens(5),
        ]
    );
}

#[test]
fn stream_stops_early_when_the_callback_returns_false() {
    let fixture = spawn_fixture("200 OK", sse_stream_body());
    let client = client_for(&fixture.base_url, 1);
    let mut events = Vec::new();
    let result = {
        let mut stop_after_first = |event: CompletionStreamEvent| {
            events.push(event);
            false
        };
        client.stream_text(&short_request(), &mut stop_after_first)
    };

    assert!(result.is_ok(), "stopping early must end successfully");
    assert_eq!(
        events,
        vec![CompletionStreamEvent::TextDelta("Hel".to_string())]
    );
}

#[test]
fn kilo_stream_without_a_publication_fails_before_any_request() {
    let home = TestHome::new("missing-publication");

    let run = run_kilo_stream(&home);

    match &run.result {
        Err(GatewayError::InvalidResponse(message)) => assert_eq!(
            message, "model-scanner publication unavailable",
            "a missing publication must report unavailable"
        ),
        other => panic!("expected an unavailable publication, got {other:?}"),
    }
    assert_eq!(run.proxy_connections, 0, "no HTTP request may be sent");
}

#[test]
fn kilo_stream_with_an_unreadable_publication_reports_unavailable() {
    // A directory where the file should be makes the read fail on every
    // platform. A permission-denied file cannot be built portably here.
    let home = TestHome::new("unreadable-publication");
    fs::create_dir_all(home.path.join(".model-scanner").join("scan.json"))
        .expect("create unreadable publication stand-in");

    let run = run_kilo_stream(&home);

    assert_rejected_by_scanner(&run, "model-scanner publication unavailable");
}

#[test]
fn kilo_stream_rejects_an_empty_or_non_json_publication() {
    let home = TestHome::new("empty-publication");
    home.write_publication(b"");
    let empty_run = run_kilo_stream(&home);
    assert_rejected_by_scanner(&empty_run, "invalid model-scanner publication");

    home.write_publication(b"not json");
    let text_run = run_kilo_stream(&home);
    assert_rejected_by_scanner(&text_run, "invalid model-scanner publication");
}

#[test]
fn kilo_stream_rejects_a_publication_that_is_not_utf8() {
    let home = TestHome::new("non-utf8-publication");
    home.write_publication(b"\xff\xfe\xfd");

    let run = run_kilo_stream(&home);

    assert_rejected_by_scanner(&run, "invalid model-scanner publication");
}

#[test]
fn kilo_stream_rejects_a_publication_without_a_recommendations_key() {
    let home = TestHome::new("no-recommendations");
    home.write_publication(br#"{"schema_version":1,"models":[]}"#);

    let run = run_kilo_stream(&home);

    assert_rejected_by_scanner(&run, "invalid model-scanner publication");
}

#[test]
fn kilo_stream_rejects_a_recommendations_block_without_a_base_model() {
    let home = TestHome::new("no-base");
    home.write_publication(br#"{"schema_version":1,"recommendations":{},"models":[]}"#);

    let run = run_kilo_stream(&home);

    assert_rejected_by_scanner(&run, "no qualified base model-scanner recommendation");
}

#[test]
fn kilo_stream_rejects_a_recommended_id_missing_from_the_models() {
    let home = TestHome::new("missing-recommended-model");
    home.write_publication(&publication_v1(
        "absent-model",
        "other-model",
        200_000,
        4_096,
    ));

    let run = run_kilo_stream(&home);

    assert_rejected_by_scanner(&run, "no qualified base model-scanner recommendation");
}

#[test]
fn kilo_stream_rejects_max_output_tokens_above_u32_max() {
    let home = TestHome::new("oversized-output-limit");
    home.write_publication(&publication_v1("dots", "dots", 200_000, 4_294_967_296));

    let run = run_kilo_stream(&home);

    assert_rejected_by_scanner(&run, "invalid model-scanner publication");
}

#[test]
fn kilo_stream_accepts_a_recommendation_without_max_output_tokens() {
    let home = TestHome::new("absent-output-limit");
    let publication = serde_json::json!({
        "schema_version": 1,
        "recommendations": {"base": "kilo-auto/free"},
        "models": [{
            "id": "kilo-auto/free",
            "context_length": 200_000,
            "text_completed": 10,
            "text_score": 10
        }]
    });
    home.write_publication(publication.to_string().as_bytes());

    let run = run_kilo_stream(&home);

    assert_accepted_by_scanner(&run);
}

#[test]
fn kilo_stream_accepts_a_valid_publication_and_only_then_contacts_the_proxy() {
    let home = TestHome::new("valid-publication");
    home.write_publication(&publication_v1(
        "dots-studio/dots-3-note-preview:free",
        "dots-studio/dots-3-note-preview:free",
        200_000,
        460_800,
    ));

    let run = run_kilo_stream(&home);

    assert_accepted_by_scanner(&run);
    assert!(
        run.proxy_request
            .as_deref()
            .is_some_and(|request| request.starts_with("CONNECT api.kilo.ai:443")),
        "the first proxy request must be a CONNECT to the canonical host"
    );
}

#[test]
fn kilo_stream_v3_context_210000_with_score_max_50_is_accepted() {
    let home = TestHome::new("v3-context-210000-max-50");
    home.write_publication(&scan_v3(210_000, 50));

    let run = run_kilo_stream(&home);

    assert_accepted_by_scanner(&run);
}

#[test]
fn kilo_stream_v3_context_209999_with_score_max_50_is_rejected() {
    let home = TestHome::new("v3-context-209999-max-50");
    home.write_publication(&scan_v3(209_999, 50));

    let run = run_kilo_stream(&home);

    assert_rejected_by_scanner(&run, "no qualified base model-scanner recommendation");
}

#[test]
fn kilo_stream_v3_context_209999_with_score_max_45_is_accepted() {
    let home = TestHome::new("v3-context-209999-max-45");
    home.write_publication(&scan_v3(209_999, 45));

    let run = run_kilo_stream(&home);

    assert_accepted_by_scanner(&run);
}

#[test]
fn kilo_stream_v3_context_210000_with_score_max_45_is_rejected() {
    let home = TestHome::new("v3-context-210000-max-45");
    home.write_publication(&scan_v3(210_000, 45));

    let run = run_kilo_stream(&home);

    assert_rejected_by_scanner(&run, "no qualified base model-scanner recommendation");
}
