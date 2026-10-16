//! Sample 2 of the page: streaming replies over server-sent events. The streaming path serves the
//! `provider-test` purpose (the Test provider dialog "filling in line by line"); the plugin appends
//! each `http-stream-data` chunk and finishes the reply on `http-stream-closed`.

use ilium_plugin_core_inference::InferenceServer;
use ilium_plugin_sdk::exports::ProviderCall;
use ilium_plugin_sdk::host::{Completion, StreamFraming};
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{
    ErrorCode, Event, HttpStreamClosedEvent, HttpStreamDataEvent, StreamId, StreamOutcome,
};
use ilium_plugin_sdk::InferenceProvider;
use proptest::prelude::*;

use crate::common::{error, request, server_host, sse_delta};

fn open_stream(host: &mut FakeHost, number: u64) -> StreamId {
    let accepted = host
        .instance_mut::<InferenceServer>()
        .complete(ProviderCall { call: number, request: request("provider-test", "Reply with the word ok.") });
    assert!(accepted.is_ok());
    host.settle();
    host.http().streams().last().expect("a stream was opened").0
}

fn data(stream: StreamId, chunk: Vec<u8>) -> Event {
    Event::HttpStreamData(HttpStreamDataEvent { stream, status: None, headers: None, chunk })
}

fn closed(stream: StreamId, outcome: StreamOutcome) -> Event {
    Event::HttpStreamClosed(HttpStreamClosedEvent { stream, outcome })
}

fn replies(host: &mut FakeHost) -> Vec<(u64, Result<Completion, ilium_plugin_sdk::types::PluginError>)> {
    host.inference().provider_replies()
}

/// REQ: F-084
#[test]
fn stream_084_a_provider_test_call_opens_one_server_sent_events_stream() {
    let mut host = server_host();
    open_stream(&mut host, 1);
    let streams = host.http().streams();
    assert_eq!(streams.len(), 1);
    assert_eq!(streams[0].1.framing, StreamFraming::ServerSentEvents);
}

/// REQ: F-084
#[test]
fn stream_084_the_stream_posts_to_the_chat_completions_url_with_a_16_kib_chunk_limit() {
    let mut host = server_host();
    open_stream(&mut host, 1);
    let request = host.http().streams().remove(0).1;
    assert_eq!(request.request.method, "POST");
    assert_eq!(request.request.url, "https://api.openai.com/v1/chat/completions");
    assert_eq!(request.max_chunk_bytes, 16_384);
}

/// REQ: F-084
#[test]
fn stream_084_the_stream_request_allows_120_seconds() {
    let mut host = server_host();
    open_stream(&mut host, 1);
    assert_eq!(host.http().streams().remove(0).1.request.timeout_millis, 120_000);
}

/// REQ: F-084
#[test]
fn stream_084_the_stream_body_asks_for_streaming() {
    let mut host = server_host();
    open_stream(&mut host, 1);
    let request = host.http().streams().remove(0).1;
    let body: serde_json::Value = serde_json::from_slice(request.request.body.as_ref().expect("body")).expect("JSON");
    assert_eq!(body["stream"], true);
    assert_eq!(body["model"], "gpt-5.6-terra");
}

/// REQ: F-084
#[test]
fn stream_084_no_reply_is_made_while_chunks_arrive() {
    let mut host = server_host();
    let stream = open_stream(&mut host, 1);
    host.deliver_one(data(stream, sse_delta("o")));
    host.deliver_one(data(stream, sse_delta("k")));
    assert!(replies(&mut host).is_empty());
}

/// REQ: F-084
#[test]
fn stream_084_closing_the_stream_finishes_the_reply_with_the_joined_text() {
    let mut host = server_host();
    let stream = open_stream(&mut host, 9);
    host.deliver_one(data(stream, sse_delta("Hel")));
    host.deliver_one(data(stream, sse_delta("lo")));
    host.deliver_one(closed(stream, StreamOutcome::Ended));
    let replies = replies(&mut host);
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0].0, 9);
    let completion = replies[0].1.clone().expect("ok");
    assert_eq!(completion.text, "Hello");
    assert_eq!(completion.model, "gpt-5.6-terra");
}

/// REQ: F-084
#[test]
fn stream_084_the_done_marker_adds_no_text() {
    let mut host = server_host();
    let stream = open_stream(&mut host, 1);
    host.deliver_one(data(stream, sse_delta("ok")));
    host.deliver_one(data(stream, b"[DONE]".to_vec()));
    host.deliver_one(closed(stream, StreamOutcome::Ended));
    assert_eq!(replies(&mut host)[0].1.clone().expect("ok").text, "ok");
}

/// REQ: F-084
#[test]
fn stream_084_a_chunk_without_content_adds_no_text() {
    let mut host = server_host();
    let stream = open_stream(&mut host, 1);
    host.deliver_one(data(stream, br#"{"choices":[{"delta":{"role":"assistant"}}]}"#.to_vec()));
    host.deliver_one(data(stream, sse_delta("ok")));
    host.deliver_one(closed(stream, StreamOutcome::Ended));
    assert_eq!(replies(&mut host)[0].1.clone().expect("ok").text, "ok");
}

/// REQ: F-084
#[test]
fn stream_084_a_failed_stream_answers_with_the_error_of_the_stream() {
    let mut host = server_host();
    let stream = open_stream(&mut host, 2);
    host.deliver_one(data(stream, sse_delta("par")));
    host.deliver_one(closed(stream, StreamOutcome::Failed(error(ErrorCode::Network))));
    let replies = replies(&mut host);
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0].1.clone().expect_err("err").code, ErrorCode::Network);
}

/// REQ: F-084
#[test]
fn stream_084_a_cancelled_stream_answers_with_an_error() {
    let mut host = server_host();
    let stream = open_stream(&mut host, 2);
    host.deliver_one(closed(stream, StreamOutcome::Cancelled));
    let replies = replies(&mut host);
    assert_eq!(replies.len(), 1);
    assert!(replies[0].1.is_err());
}

/// REQ: F-084
#[test]
fn stream_084_a_timed_out_stream_keeps_the_timed_out_code() {
    let mut host = server_host();
    let stream = open_stream(&mut host, 2);
    host.deliver_one(closed(stream, StreamOutcome::Failed(error(ErrorCode::TimedOut))));
    assert_eq!(replies(&mut host)[0].1.clone().expect_err("err").code, ErrorCode::TimedOut);
}

/// REQ: F-084
#[test]
fn stream_084_data_for_an_unknown_stream_is_ignored() {
    let mut host = server_host();
    open_stream(&mut host, 1);
    host.deliver_one(data(StreamId(99_999), sse_delta("x")));
    host.deliver_one(closed(StreamId(99_999), StreamOutcome::Ended));
    assert!(replies(&mut host).is_empty());
}

/// REQ: F-084
#[test]
fn stream_084_a_second_close_does_not_reply_twice() {
    let mut host = server_host();
    let stream = open_stream(&mut host, 1);
    host.deliver_one(data(stream, sse_delta("ok")));
    host.deliver_one(closed(stream, StreamOutcome::Ended));
    host.deliver_one(closed(stream, StreamOutcome::Ended));
    assert_eq!(replies(&mut host).len(), 1);
}

/// REQ: F-084
#[test]
fn stream_084_two_open_streams_keep_their_texts_apart() {
    let mut host = server_host();
    let first = open_stream(&mut host, 1);
    let second = open_stream(&mut host, 2);
    assert_ne!(first, second);
    host.deliver_one(data(first, sse_delta("alpha")));
    host.deliver_one(data(second, sse_delta("beta")));
    host.deliver_one(closed(second, StreamOutcome::Ended));
    host.deliver_one(closed(first, StreamOutcome::Ended));
    let mut texts: Vec<(u64, String)> =
        replies(&mut host).into_iter().map(|(number, outcome)| (number, outcome.expect("ok").text)).collect();
    texts.sort();
    assert_eq!(texts, [(1, "alpha".to_string()), (2, "beta".to_string())]);
}

/// REQ: F-084
#[test]
fn stream_084_a_stream_with_no_chunks_replies_with_empty_text() {
    let mut host = server_host();
    let stream = open_stream(&mut host, 1);
    host.deliver_one(closed(stream, StreamOutcome::Ended));
    assert_eq!(replies(&mut host)[0].1.clone().expect("ok").text, "");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    // REQ: F-084
    #[test]
    fn stream_084_the_reply_is_the_concatenation_of_the_deltas(parts in proptest::collection::vec("[a-z ]{0,12}", 0..8)) {
        let mut host = server_host();
        let stream = open_stream(&mut host, 1);
        for part in &parts {
            host.deliver_one(data(stream, sse_delta(part)));
        }
        host.deliver_one(closed(stream, StreamOutcome::Ended));
        let text = replies(&mut host)[0].1.clone().expect("ok").text;
        prop_assert_eq!(text, parts.concat());
    }
}
