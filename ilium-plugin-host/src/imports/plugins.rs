//! Host implementation of `ilium:host/plugins` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostPlugins` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::plugins::{HostPlugins, PluginInfo};
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::ids::PluginId;

/// Host side of the `plugins` interface.
#[derive(Debug)]
pub struct PluginsImport;

impl HostPlugins for PluginsImport {
    fn list(&mut self) -> Vec<PluginInfo> {
        unimplemented!("host::imports::plugins::list")
    }

    fn set_enabled(
        &mut self,
        _plugin: PluginId,
        _enabled: bool,
        _scope: String,
    ) -> Result<(), PluginError> {
        unimplemented!("host::imports::plugins::set_enabled")
    }
}

#[cfg(test)]
mod tests {
    use crate::imports::plugins::PluginsImport;
    use ilium_plugin_api::generated::host::plugins::HostPlugins;
    use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
    use ilium_plugin_api::generated::types::ids::PluginId;

    fn plugin_id(text: &str) -> PluginId {
        PluginId(text.to_string())
    }

    /// Asserts that a call returned an error with the given code, failing with the actual result otherwise.
    fn assert_error_code(result: Result<(), PluginError>, expected: ErrorCode) {
        match result {
            Ok(()) => panic!("expected error code {expected:?}, but the call returned Ok"),
            Err(error) => assert_eq!(error.code, expected),
        }
    }

    /// REQ: LIF-017
    #[test]
    fn list_without_installed_plugins_returns_empty_vec() {
        let mut host = PluginsImport;
        let records = host.list();
        assert!(records.is_empty(), "expected no records, got {}", records.len());
    }

    /// REQ: CAP-018
    #[test]
    fn list_returns_empty_when_plugins_manage_is_declined() {
        let mut host = PluginsImport;
        let records = host.list();
        assert!(records.is_empty(), "declined grant must yield an empty list");
    }

    /// REQ: LIF-019
    #[test]
    fn set_enabled_true_on_enabled_plugin_returns_ok_and_changes_nothing() {
        let mut host = PluginsImport;
        let result = host.set_enabled(plugin_id("com.example.notes"), true, "global".to_string());
        assert_eq!(result, Ok(()));
    }

    /// REQ: LIF-019
    #[test]
    fn set_enabled_false_on_disabled_plugin_returns_ok_and_changes_nothing() {
        let mut host = PluginsImport;
        let result = host.set_enabled(plugin_id("com.example.notes"), false, "global".to_string());
        assert_eq!(result, Ok(()));
    }

    /// REQ: LIF-163
    #[test]
    fn set_enabled_global_scope_returns_ok() {
        let mut host = PluginsImport;
        let result = host.set_enabled(plugin_id("com.example.notes"), false, "global".to_string());
        assert_eq!(result, Ok(()));
    }

    /// REQ: LIF-161
    #[test]
    fn set_enabled_project_scope_returns_ok() {
        let mut host = PluginsImport;
        let result = host.set_enabled(plugin_id("com.example.notes"), false, "project".to_string());
        assert_eq!(result, Ok(()));
    }

    /// REQ: LIF-161
    #[test]
    fn set_enabled_session_scope_returns_ok() {
        let mut host = PluginsImport;
        let result = host.set_enabled(plugin_id("com.example.notes"), false, "session".to_string());
        assert_eq!(result, Ok(()));
    }

    /// REQ: LIF-161
    #[test]
    fn set_enabled_with_unknown_scope_returns_invalid_argument_and_changes_nothing() {
        let mut host = PluginsImport;
        let result = host.set_enabled(plugin_id("com.example.notes"), false, "user".to_string());
        assert_error_code(result, ErrorCode::InvalidArgument);
    }

    /// REQ: LIF-161
    #[test]
    fn set_enabled_with_empty_scope_returns_invalid_argument() {
        let mut host = PluginsImport;
        let result = host.set_enabled(plugin_id("com.example.notes"), true, String::new());
        assert_error_code(result, ErrorCode::InvalidArgument);
    }

    /// REQ: LIF-161
    #[test]
    fn set_enabled_scope_match_is_case_sensitive() {
        let mut host = PluginsImport;
        let result = host.set_enabled(plugin_id("com.example.notes"), true, "Global".to_string());
        assert_error_code(result, ErrorCode::InvalidArgument);
    }

    /// REQ: LIF-017
    #[test]
    fn set_enabled_with_unknown_plugin_id_returns_not_found() {
        let mut host = PluginsImport;
        let result = host.set_enabled(plugin_id("com.example.missing"), true, "global".to_string());
        assert_error_code(result, ErrorCode::NotFound);
    }

    /// REQ: LIF-017
    #[test]
    fn set_enabled_with_empty_plugin_id_returns_not_found() {
        let mut host = PluginsImport;
        let result = host.set_enabled(plugin_id(""), true, "global".to_string());
        assert_error_code(result, ErrorCode::NotFound);
    }

    /// REQ: LIF-007
    #[test]
    fn set_enabled_true_on_refused_at_load_plugin_returns_not_permitted() {
        let mut host = PluginsImport;
        let result = host.set_enabled(plugin_id("com.example.refused"), true, "global".to_string());
        assert_error_code(result, ErrorCode::NotPermitted);
    }

    /// REQ: LIF-007
    #[test]
    fn set_enabled_true_on_disabled_by_policy_plugin_returns_not_permitted() {
        let mut host = PluginsImport;
        let result = host.set_enabled(plugin_id("com.example.blocked"), true, "global".to_string());
        assert_error_code(result, ErrorCode::NotPermitted);
    }

    /// REQ: CAP-018
    #[test]
    fn set_enabled_without_plugins_manage_grant_returns_not_permitted() {
        let mut host = PluginsImport;
        let result = host.set_enabled(plugin_id("com.example.notes"), true, "global".to_string());
        assert_error_code(result, ErrorCode::NotPermitted);
    }

    /// REQ: ABI-043
    #[test]
    fn set_enabled_after_grant_revocation_returns_revoked() {
        let mut host = PluginsImport;
        let result = host.set_enabled(plugin_id("com.example.notes"), false, "global".to_string());
        assert_error_code(result, ErrorCode::Revoked);
    }

    /// REQ: HST-412
    #[test]
    fn set_enabled_true_on_failed_plugin_clears_failed_flag_and_returns_ok() {
        let mut host = PluginsImport;
        let result = host.set_enabled(plugin_id("com.example.failed"), true, "global".to_string());
        assert_eq!(result, Ok(()));
    }
}
