//! Page reference-requirements-api-host.md: unknown error codes, early retry after
//! `retry-after-ms`, and the dedup key of retried calls (API-088, API-089, API-097).

use ilium_plugin_api::generated::types::events::Event;
use ilium_plugin_api::generated::types::intents::{Intent, Submission};
use ilium_plugin_api::generated::types::requests::{Outcome, RequestRef};
use ilium_plugin_api::generated::types::ErrorCode;
use ilium_plugin_host::calls::Trap;
use ilium_plugin_host::error_code_decode::{decode_error_code, DecodedErrorCode};
use ilium_plugin_host::testing::{HarnessHandle, ImportReply, NativeGuest};

use crate::support::*;

fn play(host: &HarnessHandle) -> ImportReply {
    host.call_import_json("ilium:host/sounds", "play", "{\"name\":\"bell\"}")
}

/// Four allowed plays at the same instant, so the next play is refused by the rate window.
fn fill_the_window(host: &HarnessHandle) {
    for _ in 0..4 {
        assert!(play(host).error_code().is_none());
    }
}

/// Gives every submission the same dedup key on every call.
struct DedupGuest;

impl NativeGuest for DedupGuest {
    fn on_events(&mut self, _batch: Vec<Event>) -> Result<Vec<Submission>, Trap> {
        Ok(vec![Submission {
            intent: Intent::PlaySound("bell".to_owned()),
            correlation: 1,
            dedup_key: Some("order-42".to_owned()),
        }])
    }
}

// ---------------------------------------------------------------- API-088

/// REQ: API-088
#[test]
fn api_088_unknown_error_code_is_kept_as_unknown_with_its_text() {
    assert_eq!(
        decode_error_code("vendor-quota-9"),
        DecodedErrorCode::Unknown("vendor-quota-9".to_owned())
    );
}

/// REQ: API-088
#[test]
fn api_088_known_error_code_decodes_to_its_named_variant() {
    assert_eq!(
        decode_error_code("rate-limited"),
        DecodedErrorCode::Known(ErrorCode::RateLimited)
    );
}

// ---------------------------------------------------------------- API-089

/// REQ: API-089
#[test]
fn api_089_early_retry_gets_the_same_error_and_does_not_trap() {
    let mut host = host_with(QuietGuest);
    fill_the_window(&host);
    assert!(play(&host).error_code().is_some());
    advance_ms(&mut host, 100);
    let early = play(&host);
    assert_eq!(early.error_code(), Some("rate-limited"));
    assert!(early.trap_code().is_none());
}

/// REQ: API-089
#[test]
fn api_089_refused_early_retry_does_not_extend_the_window() {
    let mut host = host_with(QuietGuest);
    fill_the_window(&host);
    let hint = u64::from(
        play(&host)
            .retry_after_ms()
            .expect("the refusal carries a retry-after hint"),
    );
    advance_ms(&mut host, 100);
    assert_eq!(play(&host).error_code(), Some("rate-limited"));
    advance_ms(&mut host, hint.saturating_sub(100));
    assert!(play(&host).error_code().is_none());
}

// ---------------------------------------------------------------- API-097

/// REQ: API-097
#[test]
fn api_097_retry_with_the_same_dedup_key_is_applied_once() {
    let host = host_with(DedupGuest);
    host.deliver(one_event());
    host.pump(8);
    host.deliver(one_event());
    host.pump(8);
    let duplicates = host
        .outcomes(&plugin_id())
        .iter()
        .filter(|result| matches!(result.outcome, Outcome::Duplicate(_)))
        .count();
    assert_eq!(duplicates, 1);
}

/// REQ: API-097
#[test]
fn api_097_a_dedup_retry_is_refused_as_duplicate_for_the_same_request() {
    let host = host_with(DedupGuest);
    host.deliver(one_event());
    host.pump(8);
    host.deliver(one_event());
    host.pump(8);
    assert!(host.outcomes(&plugin_id()).iter().any(|result| matches!(
        &result.request,
        RequestRef::Submission(1)
    ) && matches!(
        result.outcome,
        Outcome::Duplicate(_)
    )));
}
