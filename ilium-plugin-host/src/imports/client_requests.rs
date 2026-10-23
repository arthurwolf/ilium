//! Host implementation of `ilium:host/client-requests` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostClientRequests` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::client_requests::HostClientRequests;
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::ids::{ClientId, RequestId};

/// Host side of the `client-requests` interface.
#[derive(Debug)]
pub struct ClientRequestsImport;

impl HostClientRequests for ClientRequestsImport {
    fn request(
        &mut self,
        _client: ClientId,
        _kind: String,
        _payload_json: String,
        _timeout_millis: u32,
    ) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::client_requests::request")
    }

    fn take_answer(&mut self, _request: RequestId) -> Option<String> {
        unimplemented!("host::imports::client_requests::take_answer")
    }

    fn respond(
        &mut self,
        _call: u64,
        _outcome: Result<String, PluginError>,
    ) -> Result<(), PluginError> {
        unimplemented!("host::imports::client_requests::respond")
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
