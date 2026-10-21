//! Host implementation of `ilium:host/commands` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostCommands` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::commands::HostCommands;
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::ids::RequestId;

/// Host side of the `commands` interface.
#[derive(Debug)]
pub struct CommandsImport;

impl HostCommands for CommandsImport {
    fn set_state(
        &mut self,
        _command: String,
        _enabled: bool,
        _checked: Option<bool>,
    ) -> Result<(), PluginError> {
        unimplemented!("host::imports::commands::set_state")
    }

    fn invoke(
        &mut self,
        _command: String,
        _arguments_json: String,
    ) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::commands::invoke")
    }
}

#[cfg(test)]
mod tests {
    use super::CommandsImport;
    use ilium_plugin_api::generated::host::commands::HostCommands;
    use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
    use ilium_plugin_api::generated::types::ids::RequestId;

    /// Largest `arguments-json` the manual accepts: 64 KiB, one JSON object.
    const MAX_ARGUMENTS_JSON_BYTES: usize = 64 * 1024;
    /// Longest `message` of a plugin error record, in characters.
    const MAX_ERROR_MESSAGE_CHARACTERS: usize = 256;
    /// A global command id of a plugin that is not installed, so no provider contributes it.
    const UNKNOWN_COMMAND: &str = "no.such.plugin/none";
    /// Global command id of a plausible own command; used only where the argument checks come first.
    const EXAMPLE_COMMAND: &str = "example.notes/new-note";
    /// Bytes of the JSON prefix and suffix that `object_of_length` wraps around its padding.
    const OBJECT_OVERHEAD_BYTES: usize = r#"{"text":""}"#.len();

    /// Builds one JSON object of exactly `total_bytes` bytes, padded with a string value.
    fn object_of_length(total_bytes: usize) -> String {
        assert!(total_bytes >= OBJECT_OVERHEAD_BYTES, "object must hold its own wrapper");
        let padding = "a".repeat(total_bytes - OBJECT_OVERHEAD_BYTES);
        let object = format!(r#"{{"text":"{padding}"}}"#);
        assert_eq!(object.len(), total_bytes, "test fixture has the requested length");
        object
    }

    /// Calls `set_state` and returns the error; panics when the call succeeds.
    fn set_state_error(command: &str) -> PluginError {
        let mut imports = CommandsImport;
        match imports.set_state(command.to_owned(), true, None) {
            Ok(()) => panic!("set_state({command:?}) was expected to fail, but it returned Ok"),
            Err(error) => error,
        }
    }

    /// Calls `invoke` and returns the result.
    fn invoke_with(command: &str, arguments_json: &str) -> Result<RequestId, PluginError> {
        let mut imports = CommandsImport;
        imports.invoke(command.to_owned(), arguments_json.to_owned())
    }

    /// Calls `invoke` and returns the error; panics when the call succeeds.
    fn invoke_error(command: &str, arguments_json: &str) -> PluginError {
        match invoke_with(command, arguments_json) {
            Ok(request_id) => panic!(
                "invoke({command:?}, {arguments_json:?}) was expected to fail, got request {}",
                request_id.0
            ),
            Err(error) => error,
        }
    }

    /// REQ: API-005
    #[test]
    fn set_state_with_empty_command_id_is_not_found() {
        let error = set_state_error("");
        assert_eq!(error.code, ErrorCode::NotFound);
    }

    /// REQ: API-005
    #[test]
    fn set_state_on_command_of_uninstalled_plugin_is_not_found() {
        let error = set_state_error(UNKNOWN_COMMAND);
        assert_eq!(error.code, ErrorCode::NotFound);
    }

    /// REQ: API-002
    #[test]
    fn set_state_error_code_is_one_of_the_fifteen_codes() {
        let error = set_state_error(UNKNOWN_COMMAND);
        assert!(
            ErrorCode::CASE_NAMES.contains(&error.code.as_str()),
            "error code {:?} is not one of the 15 ABI codes",
            error.code
        );
    }

    /// REQ: API-003
    #[test]
    fn set_state_not_found_error_has_no_retry_after() {
        let error = set_state_error(UNKNOWN_COMMAND);
        assert_eq!(error.retry_after_ms, None);
    }

    /// REQ: API-001
    #[test]
    fn set_state_error_message_is_non_empty_and_at_most_256_characters() {
        let error = set_state_error(UNKNOWN_COMMAND);
        let length = error.message.chars().count();
        assert!(length > 0, "error message must describe the failure");
        assert!(
            length <= MAX_ERROR_MESSAGE_CHARACTERS,
            "error message has {length} characters, the limit is {MAX_ERROR_MESSAGE_CHARACTERS}"
        );
    }

    /// REQ: API-002
    #[test]
    fn invoke_with_unknown_command_and_empty_object_is_not_found() {
        let error = invoke_error(UNKNOWN_COMMAND, "{}");
        assert_eq!(error.code, ErrorCode::NotFound);
    }

    /// REQ: API-005
    #[test]
    fn invoke_with_empty_command_id_is_not_found() {
        let error = invoke_error("", "{}");
        assert_eq!(error.code, ErrorCode::NotFound);
    }

    /// REQ: API-061
    #[test]
    fn invoke_rejects_empty_string_arguments_as_invalid_argument() {
        let error = invoke_error(EXAMPLE_COMMAND, "");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    /// REQ: API-061
    #[test]
    fn invoke_rejects_malformed_json_as_invalid_argument() {
        let error = invoke_error(EXAMPLE_COMMAND, "{");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    /// REQ: API-061
    #[test]
    fn invoke_rejects_a_json_array_as_invalid_argument() {
        let error = invoke_error(EXAMPLE_COMMAND, "[]");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    /// REQ: API-061
    #[test]
    fn invoke_rejects_two_json_objects_as_invalid_argument() {
        let error = invoke_error(EXAMPLE_COMMAND, "{} {}");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    /// REQ: API-061
    #[test]
    fn invoke_accepts_arguments_of_exactly_64_kib_by_size() {
        let arguments = object_of_length(MAX_ARGUMENTS_JSON_BYTES);
        let error = invoke_error(UNKNOWN_COMMAND, &arguments);
        assert_ne!(
            error.code,
            ErrorCode::InvalidArgument,
            "a 64 KiB object is at the limit and must pass the size check"
        );
        assert_eq!(error.code, ErrorCode::NotFound);
    }

    /// REQ: API-062
    #[test]
    fn invoke_rejects_arguments_one_byte_over_64_kib_as_invalid_argument() {
        let arguments = object_of_length(MAX_ARGUMENTS_JSON_BYTES + 1);
        let error = invoke_error(UNKNOWN_COMMAND, &arguments);
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    /// REQ: API-003
    #[test]
    fn invoke_invalid_argument_error_has_no_retry_after() {
        let error = invoke_error(EXAMPLE_COMMAND, "[]");
        assert_eq!(error.retry_after_ms, None);
    }

    /// REQ: API-001
    #[test]
    fn invoke_error_message_is_non_empty_and_at_most_256_characters() {
        let error = invoke_error(EXAMPLE_COMMAND, "[]");
        let length = error.message.chars().count();
        assert!(length > 0, "error message must describe the failure");
        assert!(
            length <= MAX_ERROR_MESSAGE_CHARACTERS,
            "error message has {length} characters, the limit is {MAX_ERROR_MESSAGE_CHARACTERS}"
        );
    }

    /// REQ: API-002
    #[test]
    fn invoke_error_code_is_one_of_the_fifteen_codes() {
        let error = invoke_error(UNKNOWN_COMMAND, "{}");
        assert!(
            ErrorCode::CASE_NAMES.contains(&error.code.as_str()),
            "error code {:?} is not one of the 15 ABI codes",
            error.code
        );
    }
}
