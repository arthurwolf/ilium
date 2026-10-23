//! Host implementation of `ilium:host/extensions` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostExtensions` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::extensions::{
    ExtensionEntry, ExtensionError, HostExtensions,
};
use ilium_plugin_api::generated::types::ids::RequestId;

/// Host side of the `extensions` interface.
#[derive(Debug)]
pub struct ExtensionsImport;

impl HostExtensions for ExtensionsImport {
    fn list(&mut self, _point: String) -> Result<Vec<ExtensionEntry>, ExtensionError> {
        unimplemented!("host::imports::extensions::list")
    }

    fn run_hook(
        &mut self,
        _point: String,
        _payload_json: String,
    ) -> Result<RequestId, ExtensionError> {
        unimplemented!("host::imports::extensions::run_hook")
    }

    fn take_reply(&mut self, _request: RequestId) -> Option<String> {
        unimplemented!("host::imports::extensions::take_reply")
    }

    fn register(
        &mut self,
        _point: String,
        _extension: String,
        _priority: u8,
        _entry_json: String,
    ) -> Result<(), ExtensionError> {
        unimplemented!("host::imports::extensions::register")
    }

    fn unregister(&mut self, _point: String, _extension: String) -> Result<(), ExtensionError> {
        unimplemented!("host::imports::extensions::unregister")
    }
}

#[cfg(test)]
mod tests {
    use super::ExtensionsImport;
    use ilium_plugin_api::generated::host::extensions::{ExtensionError, HostExtensions};
    use ilium_plugin_api::generated::types::ids::RequestId;

    /// Largest payload, reply or chain result in bytes (EXT-023, "Payloads").
    const PAYLOAD_CAP_BYTES: usize = 64 * 1024;

    /// Largest contributor entry in bytes (EXT-023).
    const ENTRY_CAP_BYTES: usize = 16 * 1024;

    /// Highest priority a manifest may declare; `register` takes a `u8` but applies the same range.
    const MAX_MANIFEST_PRIORITY: u8 = 100;

    /// A global point name that no installed owner declares in a fresh import.
    const UNDECLARED_POINT: &str = "ilium.core.tree/before-close";

    /// A point name built from a 65-character id, one over the 64-character point id limit.
    fn overlong_point_name() -> String {
        format!("{}/before-close", "a".repeat(65))
    }

    /// REQ: EXT-024
    #[test]
    fn list_undeclared_point_fails_with_unknown_point() {
        let mut import = ExtensionsImport;
        assert_eq!(
            import.list(UNDECLARED_POINT.to_string()),
            Err(ExtensionError::UnknownPoint)
        );
    }

    /// REQ: EXT-024
    #[test]
    fn list_empty_point_name_fails_with_unknown_point() {
        let mut import = ExtensionsImport;
        assert_eq!(
            import.list(String::new()),
            Err(ExtensionError::UnknownPoint)
        );
    }

    /// REQ: EXT-024
    #[test]
    fn list_point_id_over_64_characters_fails_with_unknown_point() {
        let mut import = ExtensionsImport;
        assert_eq!(
            import.list(overlong_point_name()),
            Err(ExtensionError::UnknownPoint)
        );
    }

    /// REQ: EXT-024
    #[test]
    fn run_hook_undeclared_point_fails_with_unknown_point() {
        let mut import = ExtensionsImport;
        assert_eq!(
            import.run_hook(UNDECLARED_POINT.to_string(), "{}".to_string()),
            Err(ExtensionError::UnknownPoint)
        );
    }

    /// REQ: EXT-024
    #[test]
    fn run_hook_empty_payload_on_undeclared_point_fails_with_unknown_point() {
        let mut import = ExtensionsImport;
        assert_eq!(
            import.run_hook(UNDECLARED_POINT.to_string(), String::new()),
            Err(ExtensionError::UnknownPoint)
        );
    }

    /// REQ: EXT-004
    #[test]
    fn run_hook_payload_one_byte_over_64_kib_fails_with_cap_exceeded() {
        let mut import = ExtensionsImport;
        let oversized_payload = "a".repeat(PAYLOAD_CAP_BYTES + 1);
        assert_eq!(
            import.run_hook(UNDECLARED_POINT.to_string(), oversized_payload),
            Err(ExtensionError::CapExceeded)
        );
    }

    /// REQ: EXT-023
    #[test]
    fn run_hook_payload_exactly_64_kib_is_not_rejected_for_size() {
        let mut import = ExtensionsImport;
        let payload_at_cap = "a".repeat(PAYLOAD_CAP_BYTES);
        assert_eq!(
            import.run_hook(UNDECLARED_POINT.to_string(), payload_at_cap),
            Err(ExtensionError::UnknownPoint)
        );
    }

    /// REQ: EXT-024
    #[test]
    fn register_undeclared_point_fails_with_unknown_point() {
        let mut import = ExtensionsImport;
        assert_eq!(
            import.register(
                UNDECLARED_POINT.to_string(),
                "project-key".to_string(),
                50,
                "{}".to_string(),
            ),
            Err(ExtensionError::UnknownPoint)
        );
    }

    /// REQ: EXT-023
    #[test]
    fn register_entry_one_byte_over_16_kib_fails_with_cap_exceeded() {
        let mut import = ExtensionsImport;
        let oversized_entry = "a".repeat(ENTRY_CAP_BYTES + 1);
        assert_eq!(
            import.register(
                UNDECLARED_POINT.to_string(),
                "project-key".to_string(),
                50,
                oversized_entry,
            ),
            Err(ExtensionError::CapExceeded)
        );
    }

    /// REQ: EXT-007
    #[test]
    fn register_priority_above_manifest_range_fails_with_schema_mismatch() {
        let mut import = ExtensionsImport;
        assert_eq!(
            import.register(
                UNDECLARED_POINT.to_string(),
                "project-key".to_string(),
                MAX_MANIFEST_PRIORITY + 1,
                "{}".to_string(),
            ),
            Err(ExtensionError::SchemaMismatch)
        );
    }

    /// REQ: EXT-024
    #[test]
    fn unregister_undeclared_point_fails_with_unknown_point() {
        let mut import = ExtensionsImport;
        assert_eq!(
            import.unregister(UNDECLARED_POINT.to_string(), "project-key".to_string()),
            Err(ExtensionError::UnknownPoint)
        );
    }

    /// REQ: EXT-013
    #[test]
    fn take_reply_for_unknown_request_returns_none() {
        let mut import = ExtensionsImport;
        assert_eq!(import.take_reply(RequestId(0)), None);
    }

    /// REQ: EXT-013
    #[test]
    fn take_reply_for_maximum_request_id_returns_none() {
        let mut import = ExtensionsImport;
        assert_eq!(import.take_reply(RequestId(u64::MAX)), None);
    }
}
