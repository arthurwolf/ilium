//! G10 `http`: a per-instance import that reaches only the hosts its `http:<host>` grants name,
//! answers a fetch through `request-completed` and `take-response`, delivers a stream as
//! `http-stream-data` and `http-stream-closed`, times out silent servers and stops its workers
//! when the instance ends (api-http.md, api-http-streams.md). The servers here are real
//! loopback sockets.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use ilium_plugin_api::generated::host::http::{
    HostHttp, HttpRequest, StreamFraming, StreamRequest,
};
use ilium_plugin_api::generated::types::errors::ErrorCode;
use ilium_plugin_api::generated::types::events::{Event, StreamOutcome};
use ilium_plugin_api::generated::types::ids::RequestId;
use ilium_plugin_api::generated::types::requests::{Outcome, RequestRef};
use ilium_plugin_host::imports::env::ImportEnv;
use ilium_plugin_host::imports::http::HttpImport;

const PATIENCE: Duration = Duration::from_secs(20);

fn bind() -> (TcpListener, u16) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("loopback listener");
    let port = listener.local_addr().expect("local address").port();
    (listener, port)
}

/// Answers one connection with `response` after reading the request head, then closes it.
fn serve_once(listener: TcpListener, response: &'static [u8]) -> JoinHandle<()> {
    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            read_head(&mut stream);
            let _ = stream.write_all(response);
        }
    })
}

/// Accepts one connection and keeps it silent for `hold`.
fn serve_silence(listener: TcpListener, hold: Duration) -> JoinHandle<()> {
    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            read_head(&mut stream);
            std::thread::sleep(hold);
        }
    })
}

fn read_head(stream: &mut TcpStream) {
    let mut seen = Vec::new();
    let mut byte = [0u8; 1];
    while !seen.ends_with(b"\r\n\r\n") {
        match stream.read(&mut byte) {
            Ok(1) => seen.push(byte[0]),
            _ => return,
        }
    }
}

fn env_granting(port: u16) -> ImportEnv {
    ImportEnv::unrestricted()
        .with_identity("test.http", 1)
        .with_grants(vec![format!("http:127.0.0.1:{port}")])
}

fn get(port: u16, timeout_millis: u32) -> HttpRequest {
    HttpRequest {
        method: "GET".to_owned(),
        url: format!("http://127.0.0.1:{port}/data"),
        headers: Vec::new(),
        body: None,
        timeout_millis,
    }
}

fn collect_until(env: &ImportEnv, done: impl Fn(&[Event]) -> bool) -> Vec<Event> {
    let started = Instant::now();
    let mut seen: Vec<Event> = Vec::new();
    while started.elapsed() < PATIENCE {
        seen.extend(env.events().drain().into_iter().map(|queued| queued.event));
        if done(&seen) {
            return seen;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    seen
}

fn completed(events: &[Event], request: RequestId) -> Option<Outcome> {
    events.iter().find_map(|event| match event {
        Event::RequestCompleted(result) if result.request == RequestRef::Call(request) => {
            Some(result.outcome.clone())
        }
        _ => None,
    })
}

const OK_HELLO: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello";
const LINES: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nline one\nline two\n";

/// REQ: API-070
#[test]
fn a_host_without_a_grant_is_refused_before_any_connection() {
    let (listener, port) = bind();
    let env = ImportEnv::unrestricted()
        .with_identity("test.http", 1)
        .with_grants(vec!["http:127.0.0.1:1".to_owned()]);
    let mut http = HttpImport::new(env);
    let refused = http.fetch(get(port, 1_000));
    assert_eq!(
        refused.expect_err("another port").code,
        ErrorCode::NotPermitted
    );
    listener.set_nonblocking(true).expect("nonblocking");
    assert!(
        listener.accept().is_err(),
        "the refused request never connected"
    );
}

/// REQ: API-070
#[test]
fn plain_http_to_a_remote_host_is_refused_even_with_a_grant() {
    let env = ImportEnv::unrestricted()
        .with_identity("test.http", 1)
        .with_grants(vec!["http:example.com".to_owned()]);
    let mut http = HttpImport::new(env);
    let request = HttpRequest {
        method: "GET".to_owned(),
        url: "http://example.com/".to_owned(),
        headers: Vec::new(),
        body: None,
        timeout_millis: 1_000,
    };
    assert_eq!(
        http.fetch(request).expect_err("plain http").code,
        ErrorCode::InvalidArgument
    );
}

/// REQ: API-062
#[test]
fn a_granted_fetch_completes_through_an_event_and_take_response() {
    let (listener, port) = bind();
    let server = serve_once(listener, OK_HELLO);
    let env = env_granting(port);
    let mut http = HttpImport::new(env.clone());
    let request = http.fetch(get(port, 5_000)).expect("fetch starts");
    let events = collect_until(&env, |seen| completed(seen, request).is_some());
    assert_eq!(completed(&events, request), Some(Outcome::Ok), "{events:?}");
    let response = http.take_response(request).expect("the response is ready");
    assert_eq!(response.status, 200);
    assert_eq!(response.body, b"hello");
    assert!(
        response
            .headers
            .iter()
            .any(|header| header.name.eq_ignore_ascii_case("content-type")
                && header.value == "text/plain"),
        "{:?}",
        response.headers
    );
    assert!(
        http.take_response(request).is_none(),
        "a response is taken once"
    );
    server.join().expect("server thread");
}

/// REQ: API-062
#[test]
fn a_request_id_of_one_instance_means_nothing_to_another() {
    let (listener, port) = bind();
    let server = serve_once(listener, OK_HELLO);
    let first_env = env_granting(port).with_identity("test.first", 1);
    let second_env = env_granting(port).with_identity("test.second", 1);
    let mut first = HttpImport::new(first_env.clone());
    let mut second = HttpImport::new(second_env.clone());
    let request = first.fetch(get(port, 5_000)).expect("fetch starts");
    collect_until(&first_env, |seen| completed(seen, request).is_some());
    assert!(second.take_response(request).is_none());
    assert!(second_env.events().is_empty());
    assert!(first.take_response(request).is_some());
    server.join().expect("server thread");
}

/// REQ: API-070
#[test]
fn a_silent_server_ends_the_fetch_as_timed_out() {
    let (listener, port) = bind();
    let server = serve_silence(listener, Duration::from_secs(3));
    let env = env_granting(port);
    let mut http = HttpImport::new(env.clone());
    let request = http.fetch(get(port, 300)).expect("fetch starts");
    let events = collect_until(&env, |seen| completed(seen, request).is_some());
    match completed(&events, request) {
        Some(Outcome::Failed(error)) => assert_eq!(error.code, ErrorCode::TimedOut),
        other => panic!("expected a timed-out failure, got {other:?}"),
    }
    server.join().expect("server thread");
}

/// REQ: ABI-041
#[test]
fn a_stream_delivers_its_lines_then_closes_as_ended() {
    let (listener, port) = bind();
    let server = serve_once(listener, LINES);
    let env = env_granting(port);
    let mut http = HttpImport::new(env.clone());
    let stream = http
        .open_stream(StreamRequest {
            request: get(port, 5_000),
            framing: StreamFraming::Lines,
            max_chunk_bytes: 1_024,
        })
        .expect("stream opens");
    let events = collect_until(&env, |seen| {
        seen.iter().any(
            |event| matches!(event, Event::HttpStreamClosed(closed) if closed.stream == stream),
        )
    });
    let mut text = String::new();
    let mut first_status = None;
    for event in &events {
        if let Event::HttpStreamData(data) = event {
            assert_eq!(data.stream, stream);
            if first_status.is_none() {
                first_status = data.status;
            }
            text.push_str(&String::from_utf8_lossy(&data.chunk));
        }
    }
    assert_eq!(
        first_status,
        Some(200),
        "the first chunk carries the status: {events:?}"
    );
    assert!(text.contains("line one"), "{text:?}");
    assert!(text.contains("line two"), "{text:?}");
    let closed = events
        .iter()
        .find_map(|event| match event {
            Event::HttpStreamClosed(closed) if closed.stream == stream => {
                Some(closed.outcome.clone())
            }
            _ => None,
        })
        .expect("the stream closed");
    assert_eq!(closed, StreamOutcome::Ended);
    server.join().expect("server thread");
}

/// REQ: ABI-041
#[test]
fn closing_an_open_stream_stops_its_worker_without_a_closed_event() {
    let (listener, port) = bind();
    let server = serve_silence(listener, Duration::from_secs(4));
    let env = env_granting(port);
    let mut http = HttpImport::new(env.clone());
    let stream = http
        .open_stream(StreamRequest {
            request: get(port, 30_000),
            framing: StreamFraming::Raw,
            max_chunk_bytes: 1_024,
        })
        .expect("stream opens");
    assert_eq!(http.open_streams(), 1);
    http.close_stream(stream);
    assert_eq!(http.open_streams(), 0);
    assert_eq!(env.events().workers(), 0, "closing joined the worker");
    assert!(
        env.events()
            .drain()
            .iter()
            .all(|queued| !matches!(queued.event, Event::HttpStreamClosed(_))),
        "a stream the plugin closed reports nothing"
    );
    server.join().expect("server thread");
}

/// REQ: ABI-041
#[test]
fn dropping_the_import_cancels_a_pending_fetch_and_its_stream() {
    let (listener, port) = bind();
    let server = serve_silence(listener, Duration::from_secs(4));
    let env = env_granting(port);
    let mut http = HttpImport::new(env.clone());
    http.open_stream(StreamRequest {
        request: get(port, 30_000),
        framing: StreamFraming::Raw,
        max_chunk_bytes: 1_024,
    })
    .expect("stream opens");
    drop(http);
    let started = Instant::now();
    while env.events().workers() > 0 && started.elapsed() < PATIENCE {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        env.events().workers(),
        0,
        "the worker ended with its import"
    );
    server.join().expect("server thread");
}

/// REQ: ABI-041
#[test]
fn at_most_four_streams_are_open_per_instance() {
    let env = ImportEnv::unrestricted()
        .with_identity("test.http", 1)
        .with_grants(vec!["http:127.0.0.1:1".to_owned()]);
    let (listeners, ports): (Vec<TcpListener>, Vec<u16>) = (0..5).map(|_| bind()).unzip();
    let mut grants = Vec::new();
    for port in &ports {
        grants.push(format!("http:127.0.0.1:{port}"));
    }
    let mut http = HttpImport::new(env.with_grants(grants));
    let servers: Vec<JoinHandle<()>> = listeners
        .into_iter()
        .map(|listener| serve_silence(listener, Duration::from_secs(3)))
        .collect();
    let mut opened = Vec::new();
    for port in &ports[..4] {
        opened.push(
            http.open_stream(StreamRequest {
                request: get(*port, 30_000),
                framing: StreamFraming::Raw,
                max_chunk_bytes: 1_024,
            })
            .expect("within the quota"),
        );
    }
    let fifth = http.open_stream(StreamRequest {
        request: get(ports[4], 30_000),
        framing: StreamFraming::Raw,
        max_chunk_bytes: 1_024,
    });
    assert_eq!(
        fifth.expect_err("fifth stream").code,
        ErrorCode::QuotaExceeded
    );
    for stream in opened {
        http.close_stream(stream);
    }
    for server in servers {
        let _ = server.join();
    }
}
