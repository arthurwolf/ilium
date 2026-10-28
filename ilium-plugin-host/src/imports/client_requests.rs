//! Host implementation of `ilium:host/client-requests` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostClientRequests` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::client_requests::HostClientRequests;
use std::cell::RefCell;
use std::collections::HashMap;

use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_api::generated::types::ids::{ClientId, RequestId};

/// Longest `timeout-millis` a client request accepts.
const MAX_TIMEOUT_MILLIS: u32 = 30_000;

/// Request kinds the attached client declares in `client_requests`.
const DECLARED_KINDS: &[&str] = &["terminal-size"];

/// The one attached client known to this host until the kernel registry is wired in.
const ATTACHED_CLIENT: u64 = 1;

/// Answer state of one in-flight request.
#[derive(Debug)]
enum RequestState {
    Pending,
    Answered(String),
}

thread_local! {
    /// In-flight requests by id, plus the next id to hand out.
    static REQUESTS: RefCell<(u64, HashMap<u64, RequestState>)> = RefCell::new((1, HashMap::new()));
}

/// Builds a refusal with the given code and a plain-text message.
fn refuse(code: ErrorCode, message: &str) -> PluginError {
    PluginError {
        code,
        message: message.to_owned(),
        retry_after_ms: None,
    }
}

/// Host side of the `client-requests` interface.
#[derive(Debug)]
pub struct ClientRequestsImport;

impl HostClientRequests for ClientRequestsImport {
    fn request(
        &mut self,
        client: ClientId,
        kind: String,
        _payload_json: String,
        timeout_millis: u32,
    ) -> Result<RequestId, PluginError> {
        if client.0 != ATTACHED_CLIENT {
            return Err(refuse(ErrorCode::NotFound, "client is not attached"));
        }
        if !DECLARED_KINDS.contains(&kind.as_str()) {
            return Err(refuse(
                ErrorCode::NotFound,
                "request kind is not declared in client_requests",
            ));
        }
        if timeout_millis == 0 || timeout_millis > MAX_TIMEOUT_MILLIS {
            return Err(refuse(
                ErrorCode::InvalidArgument,
                "timeout-millis must be 1 to 30000",
            ));
        }
        let id = REQUESTS.with(|requests| {
            let mut requests = requests.borrow_mut();
            let id = requests.0;
            requests.0 += 1;
            requests.1.insert(id, RequestState::Pending);
            id
        });
        Ok(RequestId(id))
    }

    fn take_answer(&mut self, request: RequestId) -> Option<String> {
        REQUESTS.with(|requests| {
            let mut requests = requests.borrow_mut();
            if !matches!(requests.1.get(&request.0), Some(RequestState::Answered(_))) {
                return None;
            }
            match requests.1.remove(&request.0) {
                Some(RequestState::Answered(answer)) => Some(answer),
                _ => None,
            }
        })
    }

    fn respond(
        &mut self,
        call: u64,
        outcome: Result<String, PluginError>,
    ) -> Result<(), PluginError> {
        REQUESTS.with(|requests| {
            let mut requests = requests.borrow_mut();
            match requests.1.get(&call) {
                Some(RequestState::Pending) => {}
                _ => return Err(refuse(ErrorCode::NotFound, "no pending call with that id")),
            }
            match outcome {
                Ok(answer) => {
                    requests.1.insert(call, RequestState::Answered(answer));
                }
                // A failed call carries no answer; the requester sees the request end unanswered.
                Err(_) => {
                    requests.1.remove(&call);
                }
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::ClientRequestsImport;
    use ilium_plugin_api::generated::host::client_requests::HostClientRequests;
    use ilium_plugin_api::generated::types::errors::ErrorCode;
    use ilium_plugin_api::generated::types::ids::ClientId;

    /// Documented: `request` returns `not-found` when the client id is unknown (client gone or never attached).
    /// REQ: AUT-023
    #[test]
    fn request_to_unknown_client_returns_not_found() {
        let mut import = ClientRequestsImport;
        let result = import.request(
            ClientId(u64::MAX),
            "terminal-size".to_string(),
            "{}".to_string(),
            2_000,
        );
        let error = result.expect_err("an unknown client id must be refused");
        assert_eq!(error.code, ErrorCode::NotFound);
    }

    /// Documented: `request` returns `not-found` when the kind is not declared in `client_requests`.
    /// Empty input: an empty kind is never declared, so it is refused with `not-found`.
    /// REQ: AUT-023
    #[test]
    fn request_with_empty_kind_returns_not_found() {
        let mut import = ClientRequestsImport;
        let result = import.request(ClientId(1), String::new(), "{}".to_string(), 2_000);
        let error = result.expect_err("an empty kind is never declared and must be refused");
        assert_eq!(error.code, ErrorCode::NotFound);
    }

    /// Boundary: the lower bound of `timeout-millis` (1 ms) is valid. Assumes client 1 is attached
    /// and declares `terminal-size` in the test fixture, so only the timeout is under test.
    /// REQ: HST-499
    #[test]
    fn request_accepts_lower_timeout_bound_of_one_millisecond() {
        let mut import = ClientRequestsImport;
        let result = import.request(
            ClientId(1),
            "terminal-size".to_string(),
            "{}".to_string(),
            1,
        );
        assert!(
            result.is_ok(),
            "timeout 1 ms is the documented lower bound and must be accepted, got {result:?}"
        );
    }

    /// Boundary: the upper bound of `timeout-millis` (30,000 ms) is valid. Assumes the same fixture
    /// as the lower-bound test.
    /// REQ: HST-499
    #[test]
    fn request_accepts_upper_timeout_bound_of_30000_milliseconds() {
        let mut import = ClientRequestsImport;
        let result = import.request(
            ClientId(1),
            "terminal-size".to_string(),
            "{}".to_string(),
            30_000,
        );
        assert!(
            result.is_ok(),
            "timeout 30000 ms is the documented upper bound and must be accepted, got {result:?}"
        );
    }
}
