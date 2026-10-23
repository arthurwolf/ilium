//! Host implementation of `ilium:host/kernel-settings` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostKernelSettings` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::kernel_settings::HostKernelSettings;
use ilium_plugin_api::generated::types::errors::PluginError;

/// Host side of the `kernel-settings` interface.
#[derive(Debug)]
pub struct KernelSettingsImport;

impl HostKernelSettings for KernelSettingsImport {
    fn read(&mut self, _tab: String) -> Result<String, PluginError> {
        unimplemented!("host::imports::kernel_settings::read")
    }

    fn write(&mut self, _tab: String, _key: String, _value: String) -> Result<(), PluginError> {
        unimplemented!("host::imports::kernel_settings::write")
    }
}

#[cfg(test)]
mod tests {
    use super::KernelSettingsImport;
    use ilium_plugin_api::generated::host::kernel_settings::HostKernelSettings;
    use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};

    /// The 25 frozen kernel tab ids, from the "kernel tab ids" table of the kernel settings API page.
    const KERNEL_TAB_IDS: [&str; 25] = [
        "user-interface",
        "icons",
        "agent-monitoring",
        "agent-cost",
        "optimization",
        "remote-compaction",
        "keyboard",
        "terminal",
        "editor",
        "session",
        "git",
        "kanban-board",
        "animations",
        "sound",
        "voice-control",
        "reset-planning",
        "inference",
        "llm-instructions",
        "titles",
        "triggers",
        "text-triggers",
        "debug",
        "api",
        "about",
        "setup",
    ];

    /// Returns the error code of a failed call; panics when the call succeeded.
    fn error_code_of<T>(result: Result<T, PluginError>) -> ErrorCode {
        match result {
            Ok(_) => panic!("expected an error result, got Ok"),
            Err(error) => error.code,
        }
    }

    /// Panics with the error code when the call failed; passes the success value through.
    fn expect_ok<T>(result: Result<T, PluginError>) -> T {
        match result {
            Ok(value) => value,
            Err(error) => panic!("expected Ok, got error {:?}", error.code),
        }
    }

    /// REQ: ABI-030
    #[test]
    fn read_known_tab_returns_a_json_object() {
        let mut import = KernelSettingsImport;
        let text = expect_ok(import.read("terminal".to_string()));
        let values: serde_json::Value =
            serde_json::from_str(&text).expect("read result must be valid JSON");
        assert!(
            values.is_object(),
            "read result must be a JSON object, got {text}"
        );
    }

    #[test]
    fn read_accepts_every_frozen_tab_id() {
        let mut import = KernelSettingsImport;
        for tab_id in KERNEL_TAB_IDS {
            let text = expect_ok(import.read(tab_id.to_string()));
            let values: serde_json::Value = serde_json::from_str(&text)
                .unwrap_or_else(|error| panic!("tab {tab_id} returned invalid JSON: {error}"));
            assert!(values.is_object(), "tab {tab_id} must return a JSON object");
        }
    }

    /// REQ: SEC-244
    #[test]
    fn read_api_tab_replaces_secret_values_with_redacted_placeholder() {
        let mut import = KernelSettingsImport;
        let text = expect_ok(import.read("api".to_string()));
        let values: serde_json::Value =
            serde_json::from_str(&text).expect("api tab read must be valid JSON");
        let object = values
            .as_object()
            .expect("api tab read must be a JSON object");
        let has_redacted_value = object
            .values()
            .any(|value| value.as_str() == Some("<redacted>"));
        assert!(
            has_redacted_value,
            "the api tab holds credentials; a secret must read as \"<redacted>\", got {text}"
        );
    }

    #[test]
    fn read_unknown_tab_is_not_found() {
        let mut import = KernelSettingsImport;
        let code = error_code_of(import.read("no-such-tab".to_string()));
        assert_eq!(code, ErrorCode::NotFound);
    }

    #[test]
    fn read_empty_tab_id_is_not_found() {
        let mut import = KernelSettingsImport;
        let code = error_code_of(import.read(String::new()));
        assert_eq!(code, ErrorCode::NotFound);
    }

    #[test]
    fn read_wildcard_tab_id_is_not_found() {
        let mut import = KernelSettingsImport;
        let code = error_code_of(import.read("*".to_string()));
        assert_eq!(code, ErrorCode::NotFound);
    }

    #[test]
    fn read_tab_id_is_case_sensitive_kebab_case() {
        let mut import = KernelSettingsImport;
        let code = error_code_of(import.read("Terminal".to_string()));
        assert_eq!(code, ErrorCode::NotFound);
    }

    /// REQ: ABI-030
    #[test]
    fn write_existing_key_of_known_tab_succeeds() {
        let mut import = KernelSettingsImport;
        expect_ok(import.write(
            "terminal".to_string(),
            "scrollback_budget_mib".to_string(),
            "32".to_string(),
        ));
    }

    #[test]
    fn write_unknown_tab_is_not_found() {
        let mut import = KernelSettingsImport;
        let code = error_code_of(import.write(
            "no-such-tab".to_string(),
            "scrollback_budget_mib".to_string(),
            "32".to_string(),
        ));
        assert_eq!(code, ErrorCode::NotFound);
    }

    #[test]
    fn write_unknown_key_is_not_found() {
        let mut import = KernelSettingsImport;
        let code = error_code_of(import.write(
            "terminal".to_string(),
            "no_such_key".to_string(),
            "32".to_string(),
        ));
        assert_eq!(code, ErrorCode::NotFound);
    }

    #[test]
    fn write_empty_key_is_not_found() {
        let mut import = KernelSettingsImport;
        let code =
            error_code_of(import.write("terminal".to_string(), String::new(), "32".to_string()));
        assert_eq!(code, ErrorCode::NotFound);
    }

    #[test]
    fn write_value_of_wrong_json_type_is_invalid_argument() {
        let mut import = KernelSettingsImport;
        let code = error_code_of(import.write(
            "terminal".to_string(),
            "scrollback_budget_mib".to_string(),
            "\"big\"".to_string(),
        ));
        assert_eq!(code, ErrorCode::InvalidArgument);
    }

    #[test]
    fn write_malformed_json_value_is_invalid_argument() {
        let mut import = KernelSettingsImport;
        let code = error_code_of(import.write(
            "terminal".to_string(),
            "scrollback_budget_mib".to_string(),
            "{not json".to_string(),
        ));
        assert_eq!(code, ErrorCode::InvalidArgument);
    }

    #[test]
    fn write_empty_value_is_invalid_argument() {
        let mut import = KernelSettingsImport;
        let code = error_code_of(import.write(
            "terminal".to_string(),
            "scrollback_budget_mib".to_string(),
            String::new(),
        ));
        assert_eq!(code, ErrorCode::InvalidArgument);
    }
}
