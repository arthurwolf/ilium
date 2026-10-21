//! Host implementation of `ilium:host/services` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostServices` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::services::HostServices;
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::ids::RequestId;

/// Host side of the `services` interface.
#[derive(Debug)]
pub struct ServicesImport;

impl HostServices for ServicesImport {
    fn call(
        &mut self,
        _service: String,
        _method: String,
        _payload_json: String,
    ) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::services::call")
    }

    fn take_reply(&mut self, _request: RequestId) -> Option<String> {
        unimplemented!("host::imports::services::take_reply")
    }

    fn reply(
        &mut self,
        _call: u64,
        _outcome: Result<String, PluginError>,
    ) -> Result<(), PluginError> {
        unimplemented!("host::imports::services::reply")
    }
}

#[cfg(test)]
mod tests {
    use super::ServicesImport;
    use ilium_plugin_api::generated::host::services::HostServices;
    use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
    use ilium_plugin_api::generated::types::ids::RequestId;

    const SUGGEST_SERVICE: &str = "com.example.titles/suggest";
    const SUGGEST_METHOD: &str = "suggest";
    const SHORT_PAYLOAD: &str = r#"{"text":"Fix the parser"}"#;

    /// Returns the code of a plugin error, and fails the test if the call succeeded.
    fn code_of<T: std::fmt::Debug>(result: Result<T, PluginError>) -> ErrorCode {
        match result {
            Ok(value) => panic!("expected a plugin error, got {value:?}"),
            Err(error) => error.code,
        }
    }

    fn error_outcome(code: ErrorCode) -> Result<String, PluginError> {
        Err(PluginError {
            code,
            message: "provider failed".to_string(),
            retry_after_ms: None,
        })
    }

    /// REQ: SVC-001
    #[test]
    fn call_without_services_use_grant_is_not_permitted() {
        let mut services = ServicesImport;
        let code = code_of(services.call(
            SUGGEST_SERVICE.to_string(),
            SUGGEST_METHOD.to_string(),
            SHORT_PAYLOAD.to_string(),
        ));
        assert_eq!(code, ErrorCode::NotPermitted);
    }

    /// REQ: SVC-001
    #[test]
    fn call_with_empty_service_name_is_not_permitted_without_grant() {
        let mut services = ServicesImport;
        let code = code_of(services.call(
            String::new(),
            SUGGEST_METHOD.to_string(),
            SHORT_PAYLOAD.to_string(),
        ));
        assert_eq!(code, ErrorCode::NotPermitted);
    }

    /// REQ: SVC-001
    #[test]
    fn call_with_empty_method_and_empty_payload_is_not_permitted_without_grant() {
        let mut services = ServicesImport;
        let code = code_of(services.call(SUGGEST_SERVICE.to_string(), String::new(), String::new()));
        assert_eq!(code, ErrorCode::NotPermitted);
    }

    /// REQ: SVC-001
    #[test]
    fn call_with_longest_legal_service_name_is_not_permitted_without_grant() {
        // Plugin-defined names carry a 1 to 64 character name after the slash.
        let longest_name = format!("com.example.titles/{}", "a".repeat(64));
        let mut services = ServicesImport;
        let code = code_of(services.call(
            longest_name,
            SUGGEST_METHOD.to_string(),
            SHORT_PAYLOAD.to_string(),
        ));
        assert_eq!(code, ErrorCode::NotPermitted);
    }

    /// REQ: SVC-001
    #[test]
    fn reply_without_services_provide_grant_is_not_permitted() {
        let mut services = ServicesImport;
        let code = code_of(services.reply(1, Ok(r#"{"title":"Fix the parser"}"#.to_string())));
        assert_eq!(code, ErrorCode::NotPermitted);
    }

    /// REQ: SVC-001
    #[test]
    fn reply_with_error_outcome_without_provide_grant_is_not_permitted() {
        let mut services = ServicesImport;
        let code = code_of(services.reply(1, error_outcome(ErrorCode::Cancelled)));
        assert_eq!(code, ErrorCode::NotPermitted);
    }

    /// REQ: SVC-001
    #[test]
    fn reply_boundary_call_numbers_without_grant_are_not_permitted() {
        let mut services = ServicesImport;
        let zero_call = code_of(services.reply(0, Ok(String::new())));
        let largest_call = code_of(services.reply(u64::MAX, Ok(String::new())));
        assert_eq!(zero_call, ErrorCode::NotPermitted);
        assert_eq!(largest_call, ErrorCode::NotPermitted);
    }

    /// REQ: SVC-010
    #[test]
    fn take_reply_for_unknown_request_is_none() {
        let mut services = ServicesImport;
        assert_eq!(services.take_reply(RequestId(1)), None);
    }

    /// REQ: SVC-010
    #[test]
    fn take_reply_for_boundary_request_ids_is_none() {
        let mut services = ServicesImport;
        assert_eq!(services.take_reply(RequestId(0)), None);
        assert_eq!(services.take_reply(RequestId(u64::MAX)), None);
    }

    /// REQ: SVC-010
    #[test]
    fn take_reply_for_same_unknown_request_twice_stays_none() {
        let mut services = ServicesImport;
        assert_eq!(services.take_reply(RequestId(7)), None);
        assert_eq!(services.take_reply(RequestId(7)), None);
    }
}
