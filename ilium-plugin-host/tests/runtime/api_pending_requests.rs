//! Page reference-requirements-api-host.md: pending requests of a trapped instance, the
//! 120-second lifetime of a pending request, and `cancel(request-id)` (API-090, API-091, API-096).

use ilium_plugin_api::generated::types::ids::RequestId;
use ilium_plugin_api::generated::types::requests::{Outcome, RequestRef};
use ilium_plugin_host::testing::HarnessHandle;

use crate::support::*;

const CANCEL_FUNCTION: &str = "cancel";
const JOBS_INTERFACE: &str = "ilium:host/jobs";

fn started_request(host: &HarnessHandle) -> u64 {
    host.start_slow_request()
        .request_id()
        .expect("a slow call returns a request id")
}

fn cancel_args(request: u64) -> String {
    format!("{{\"job\":{request}}}")
}

fn completed_as_cancelled(host: &HarnessHandle, request: u64) -> bool {
    host.outcomes(&plugin_id()).iter().any(|result| {
        result.request == RequestRef::Call(RequestId(request)) && result.outcome == Outcome::Cancelled
    })
}

// ---------------------------------------------------------------- API-090

/// REQ: API-090
#[test]
fn api_090_trap_reclaims_the_pending_slots_of_the_old_instance() {
    let mut host = host_with(TrapGuest("trap.panic"));
    started_request(&host);
    fail_once(&mut host);
    assert_eq!(host.pending_requests(), 0);
}

/// REQ: API-090
#[test]
fn api_090_take_of_an_old_request_id_after_a_trap_returns_nothing() {
    let mut host = host_with(TrapGuest("trap.panic"));
    let request = started_request(&host);
    fail_once(&mut host);
    advance_ms(&mut host, 1_000);
    host.pump(8);
    assert!(host.take_result(RequestId(request)).is_err());
}

// ---------------------------------------------------------------- API-091

/// REQ: API-091
#[test]
fn api_091_pending_request_expires_as_cancelled_at_120_seconds() {
    let mut host = host_with(QuietGuest);
    let request = started_request(&host);
    advance_ms(&mut host, 119_999);
    host.pump(8);
    assert!(!completed_as_cancelled(&host, request));
    advance_ms(&mut host, 1);
    host.pump(8);
    assert!(completed_as_cancelled(&host, request));
}

/// REQ: API-091
#[test]
fn api_091_expired_slot_is_free_so_the_next_call_is_not_busy() {
    let mut host = host_with(QuietGuest);
    for _ in 0..64 {
        started_request(&host);
    }
    assert_eq!(host.start_slow_request().error_code(), Some("busy"));
    advance_ms(&mut host, 120_000);
    host.pump(8);
    assert!(host.start_slow_request().error_code().is_none());
}

// ---------------------------------------------------------------- API-096

/// REQ: API-096
#[test]
fn api_096_cancel_of_a_pending_request_completes_it_as_cancelled() {
    let mut host = host_with(QuietGuest);
    let request = started_request(&host);
    let reply = host.call_import_json(JOBS_INTERFACE, CANCEL_FUNCTION, &cancel_args(request));
    assert!(reply.error_code().is_none());
    host.pump(8);
    assert_eq!(host.pending_requests(), 0);
    assert!(completed_as_cancelled(&host, request));
}

/// REQ: API-096
#[test]
fn api_096_cancel_of_an_unknown_id_returns_not_found_without_effect() {
    let mut host = host_with(QuietGuest);
    started_request(&host);
    let reply = host.call_import_json(JOBS_INTERFACE, CANCEL_FUNCTION, &cancel_args(987_654));
    assert_eq!(reply.error_code(), Some("not-found"));
    assert_eq!(host.pending_requests(), 1);
}

/// REQ: API-096
#[test]
fn api_096_repeated_cancel_has_no_further_effect() {
    let mut host = host_with(QuietGuest);
    let request = started_request(&host);
    let args = cancel_args(request);
    host.call_import_json(JOBS_INTERFACE, CANCEL_FUNCTION, &args);
    host.call_import_json(JOBS_INTERFACE, CANCEL_FUNCTION, &args);
    host.pump(8);
    assert_eq!(host.pending_requests(), 0);
    let cancelled = host
        .outcomes(&plugin_id())
        .iter()
        .filter(|result| {
            result.request == RequestRef::Call(RequestId(request))
                && result.outcome == Outcome::Cancelled
        })
        .count();
    assert_eq!(cancelled, 1);
}
