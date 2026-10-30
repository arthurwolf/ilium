//! HTTP streams page (API-204): chunk limits, producer back-pressure and one stream-closed event.

use ilium_plugin_host::testing::harness::{HarnessHandle, HostHarness};
use serde_json::json;

fn host() -> HarnessHandle {
    HostHarness::new()
        .grants(&["http:example.invalid"])
        .build()
        .expect("build harness")
}

/// REQ: API-204
#[test]
fn api_204_chunks_are_capped_at_64_kib() {
    let handle = host();
    let stream = handle
        .call_import_value(
            "http.open-stream",
            json!({"url": "https://example.invalid/big"}),
        )
        .expect("opening the stream must succeed");
    let max_chunk = stream
        .get("max-chunk-bytes")
        .and_then(|value| value.as_u64())
        .expect("the stream must declare max-chunk-bytes");
    assert_eq!(
        max_chunk,
        64 * 1024,
        "every stream must define a 64 KiB maximum chunk"
    );
}

/// REQ: API-204
#[test]
fn api_204_overflow_pauses_producer_and_drops_nothing() {
    let handle = host();
    let stream = handle
        .call_import_value(
            "http.open-stream",
            json!({"url": "https://example.invalid/flood"}),
        )
        .expect("opening the stream must succeed");
    let stream_id = stream
        .get("stream-id")
        .and_then(|value| value.as_u64())
        .expect("the stream must return a stream-id");
    handle.fill_stream_unread(stream_id, 9);
    assert!(
        handle.producer_paused(stream_id),
        "with more than 8 unread chunks the host must pause the producer"
    );
    assert_eq!(
        handle.dropped_chunks(stream_id),
        0,
        "overflow must not drop any chunk"
    );
}

/// REQ: API-204
#[test]
fn api_204_closed_stream_delivers_exactly_one_stream_closed_event() {
    let handle = host();
    let stream = handle
        .call_import_value(
            "http.open-stream",
            json!({"url": "https://example.invalid/short"}),
        )
        .expect("opening the stream must succeed");
    let stream_id = stream
        .get("stream-id")
        .and_then(|value| value.as_u64())
        .expect("the stream must return a stream-id");
    handle.close_stream_from_producer(stream_id, "eof");
    handle.close_stream_from_consumer(stream_id, "cancelled");
    handle.pump(8);
    assert_eq!(
        handle.stream_closed_events(stream_id).len(),
        1,
        "a stream closed by either side must deliver exactly one stream-closed event"
    );
    assert_eq!(
        handle.stream_closed_reason(stream_id).as_deref(),
        Some("eof"),
        "the stream-closed event must carry the reason code of the first close"
    );
}
