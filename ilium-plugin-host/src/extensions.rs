//! Extension points, contributions, hook chains and core-plugin replacement
//! (reference-extension-points.md).

use ilium_plugin_api::error::Code;
use ilium_plugin_api::{ErrorCode, PluginError};

/// Largest payload a hook chain accepts, in bytes (EXT-004).
const PAYLOAD_CAP_BYTES: usize = 64 * 1024;
/// Largest single contribution entry, in bytes (EXT-023).
const ENTRY_CAP_BYTES: usize = 16 * 1024;

fn unknown_point(point: &str) -> PluginError {
    PluginError::with_prefix(
        ErrorCode::NotFound,
        &Code::from_static("extension.unknown-point"),
        &format!("no installed plugin declares the extension point {point:?}"),
    )
}

fn cap_exceeded(what: &str, size: usize, cap: usize) -> PluginError {
    PluginError::with_prefix(
        ErrorCode::InvalidArgument,
        &Code::from_static("extension.cap-exceeded"),
        &format!("{what} of {size} bytes is over the {cap} byte cap"),
    )
}

/// How a hook chain combines its handlers (reference-extension-points.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ChainMode {
    /// Any handler may refuse.
    Veto,
    /// Each handler transforms the payload in turn.
    Transform,
    /// Every answer is collected.
    Collect,
}

/// One registered contribution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contribution {
    /// The contributing plugin.
    pub plugin: String,
    /// The extension point.
    pub point: String,
    /// The contribution as JSON.
    pub payload_json: String,
}

/// The registry of extension points and contributions.
#[derive(Debug)]
pub struct ExtensionRegistry;

impl ExtensionRegistry {
    /// Adds a contribution.
    ///
    /// The size cap is checked before the point lookup. No owner declares a point in this
    /// registry yet, so every point is unknown (EXT-023, EXT-024).
    pub fn contribute(&mut self, contribution: Contribution) -> Result<(), PluginError> {
        if contribution.payload_json.len() > ENTRY_CAP_BYTES {
            return Err(cap_exceeded(
                "entry",
                contribution.payload_json.len(),
                ENTRY_CAP_BYTES,
            ));
        }
        Err(unknown_point(&contribution.point))
    }

    /// Runs the chain of an extension point over a payload.
    pub fn run_chain(
        &self,
        point: &str,
        _mode: ChainMode,
        payload_json: String,
    ) -> Result<Vec<String>, PluginError> {
        if payload_json.len() > PAYLOAD_CAP_BYTES {
            return Err(cap_exceeded("payload", payload_json.len(), PAYLOAD_CAP_BYTES));
        }
        Err(unknown_point(point))
    }
}

#[cfg(test)]
mod tests {
    use super::{ChainMode, Contribution, ExtensionRegistry};
    use ilium_plugin_api::{ErrorCode, PluginError};

    /// Payload and entry size caps from the manual (EXT-023): 64 KiB for a payload, 16 KiB for an entry.
    const PAYLOAD_CAP_BYTES: usize = 64 * 1024;
    const ENTRY_CAP_BYTES: usize = 16 * 1024;

    /// A JSON string literal of exactly `total_bytes` bytes, so that size is the only variable.
    fn json_string_of_bytes(total_bytes: usize) -> String {
        assert!(
            total_bytes >= 2,
            "a JSON string literal needs two quote bytes"
        );
        format!("\"{}\"", "a".repeat(total_bytes - 2))
    }

    /// A contribution to a point that no installed owner declares.
    fn contribution_with_payload(payload_json: String) -> Contribution {
        Contribution {
            plugin: "com.example.quiet-hours".to_string(),
            point: "com.example.owner/hook".to_string(),
            payload_json,
        }
    }

    /// Checks the mapped base code and the extension code prefix (`<code>: `) of a plugin error.
    fn assert_extension_error(
        error: &PluginError,
        expected_code: ErrorCode,
        expected_prefix: &str,
    ) {
        assert_eq!(
            error.code, expected_code,
            "base error code for message {:?}",
            error.message
        );
        assert!(
            error.message.starts_with(expected_prefix),
            "message {:?} should start with {:?}",
            error.message,
            expected_prefix
        );
    }

    /// REQ: EXT-024
    #[test]
    fn contribute_to_undeclared_point_fails_unknown_point() {
        let mut registry = ExtensionRegistry;
        let result = registry.contribute(contribution_with_payload("{}".to_string()));
        let error = result.expect_err("a point no owner declares must not accept a contribution");
        assert_extension_error(&error, ErrorCode::NotFound, "extension.unknown-point: ");
    }

    /// REQ: EXT-023
    #[test]
    fn contribute_entry_over_16_kib_fails_cap_exceeded() {
        let mut registry = ExtensionRegistry;
        let oversized_entry = json_string_of_bytes(ENTRY_CAP_BYTES + 1);
        let result = registry.contribute(contribution_with_payload(oversized_entry));
        let error = result.expect_err("an entry over 16 KiB must be rejected");
        assert_extension_error(
            &error,
            ErrorCode::InvalidArgument,
            "extension.cap-exceeded: ",
        );
        assert_eq!(
            error.retry_after_ms, None,
            "a size rejection is not retryable"
        );
    }

    /// REQ: EXT-023
    #[test]
    fn contribute_entry_exactly_16_kib_is_not_rejected_for_size() {
        let mut registry = ExtensionRegistry;
        let entry_at_cap = json_string_of_bytes(ENTRY_CAP_BYTES);
        let result = registry.contribute(contribution_with_payload(entry_at_cap));
        // The point is undeclared, so the only expected failure is the point lookup, not the size cap.
        let error = result.expect_err("the point is undeclared, so the call must still fail");
        assert_extension_error(&error, ErrorCode::NotFound, "extension.unknown-point: ");
    }

    /// REQ: EXT-024
    #[test]
    fn contribute_with_empty_point_name_fails_unknown_point() {
        let mut registry = ExtensionRegistry;
        let contribution = Contribution {
            plugin: "com.example.quiet-hours".to_string(),
            point: String::new(),
            payload_json: "{}".to_string(),
        };
        let error = registry
            .contribute(contribution)
            .expect_err("an empty point name is not a declared point");
        assert_extension_error(&error, ErrorCode::NotFound, "extension.unknown-point: ");
    }

    /// REQ: EXT-024
    #[test]
    fn run_chain_on_undeclared_point_fails_unknown_point_in_every_mode() {
        let registry = ExtensionRegistry;
        for mode in [ChainMode::Veto, ChainMode::Transform, ChainMode::Collect] {
            let result = registry.run_chain("x/y", mode, "{}".to_string());
            let error = result.expect_err("an undeclared point must not run any chain");
            assert_extension_error(&error, ErrorCode::NotFound, "extension.unknown-point: ");
        }
    }

    /// REQ: EXT-024
    #[test]
    fn run_chain_with_empty_point_name_fails_unknown_point() {
        let registry = ExtensionRegistry;
        let result = registry.run_chain("", ChainMode::Veto, "{}".to_string());
        let error = result.expect_err("an empty point name is not a declared point");
        assert_extension_error(&error, ErrorCode::NotFound, "extension.unknown-point: ");
    }

    /// REQ: EXT-004
    #[test]
    fn run_chain_payload_over_64_kib_fails_cap_exceeded_before_any_handler() {
        let registry = ExtensionRegistry;
        let oversized_payload = json_string_of_bytes(PAYLOAD_CAP_BYTES + 1);
        let result = registry.run_chain("x/y", ChainMode::Transform, oversized_payload);
        let error = result.expect_err("a payload over 64 KiB must be rejected");
        assert_extension_error(
            &error,
            ErrorCode::InvalidArgument,
            "extension.cap-exceeded: ",
        );
        assert_eq!(
            error.retry_after_ms, None,
            "a size rejection is not retryable"
        );
    }

    /// REQ: EXT-004
    #[test]
    fn run_chain_payload_exactly_64_kib_is_not_rejected_for_size() {
        let registry = ExtensionRegistry;
        let payload_at_cap = json_string_of_bytes(PAYLOAD_CAP_BYTES);
        let result = registry.run_chain("x/y", ChainMode::Transform, payload_at_cap);
        // The point is undeclared, so the only expected failure is the point lookup, not the size cap.
        let error = result.expect_err("the point is undeclared, so the call must still fail");
        assert_extension_error(&error, ErrorCode::NotFound, "extension.unknown-point: ");
    }
}
