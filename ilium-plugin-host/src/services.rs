//! Service registry, versions, `call_service`, topics and peer messages (reference-services.md).

use ilium_plugin_api::PluginError;

/// A provided service (reference-services.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceEntry {
    /// Service name.
    pub name: String,
    /// Service version.
    pub version: String,
    /// The providing plugin.
    pub provider: String,
}

/// The registry of services.
#[derive(Debug)]
pub struct ServiceRegistry;

impl ServiceRegistry {
    /// Finds a provider satisfying a version requirement.
    pub fn find(&self, _name: &str, _requirement: &str) -> Option<&ServiceEntry> {
        unimplemented!("host::services::ServiceRegistry::find")
    }

    /// Calls a service; the reply is at most 1 MiB.
    pub fn call(
        &self,
        _caller: &str,
        _name: &str,
        _request: Vec<u8>,
    ) -> Result<Vec<u8>, PluginError> {
        unimplemented!("host::services::ServiceRegistry::call")
    }
}

#[cfg(test)]
mod tests {
    use super::ServiceRegistry;
    use ilium_plugin_api::{ErrorCode, PluginError};

    /// Largest payload of one call or one reply (services.md, Limits table: 1 MiB).
    const SERVICE_PAYLOAD_LIMIT_BYTES: usize = 1024 * 1024;

    /// Extracts the error code of a call that must fail.
    fn error_code_of(outcome: Result<Vec<u8>, PluginError>) -> ErrorCode {
        match outcome {
            Ok(_) => panic!("expected the call to fail, but it returned a reply"),
            Err(error) => error.code,
        }
    }

    /// REQ: SVC-002
    #[test]
    fn find_returns_none_when_no_provider_is_registered() {
        let registry = ServiceRegistry;
        assert!(
            registry
                .find("com.example.titles/suggest", "0.1.0")
                .is_none(),
            "an empty registry must have no provider for a plugin-defined service"
        );
    }

    /// REQ: SVC-002
    #[test]
    fn find_returns_none_for_empty_name_and_requirement() {
        let registry = ServiceRegistry;
        assert!(
            registry.find("", "").is_none(),
            "an empty name must never match a provider"
        );
    }

    /// REQ: SVC-002
    #[test]
    fn call_to_unprovided_service_returns_not_found() {
        let registry = ServiceRegistry;
        let outcome = registry.call(
            "com.example.notes-ui",
            "com.example.titles/suggest",
            br#"{"title":"draft"}"#.to_vec(),
        );
        assert!(
            matches!(error_code_of(outcome), ErrorCode::NotFound),
            "a call with no installed and enabled provider must return not-found"
        );
    }

    /// REQ: SVC-002
    #[test]
    fn call_with_empty_name_and_empty_payload_returns_not_found() {
        let registry = ServiceRegistry;
        let outcome = registry.call("com.example.notes-ui", "", Vec::new());
        assert!(
            matches!(error_code_of(outcome), ErrorCode::NotFound),
            "an empty service name with an empty payload has no provider and must return not-found"
        );
    }

    /// REQ: SVC-003
    #[test]
    fn call_with_payload_exactly_at_limit_is_not_invalid_argument() {
        let registry = ServiceRegistry;
        let payload = vec![b'a'; SERVICE_PAYLOAD_LIMIT_BYTES];
        let outcome = registry.call(
            "com.example.notes-ui",
            "com.example.titles/suggest",
            payload,
        );
        assert!(
            matches!(error_code_of(outcome), ErrorCode::NotFound),
            "a payload of exactly 1 MiB is within the limit, so only the missing provider (not-found) may fail the call"
        );
    }
}
