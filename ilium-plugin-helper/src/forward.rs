//! Imports forwarded from the helper to the host (reference-host-architecture.md).

use crate::HelperError;

/// Sends one import call to the host and waits for its answer.
pub fn forward_import(
    _interface: &str,
    _function: &str,
    _payload: &[u8],
) -> Result<Vec<u8>, HelperError> {
    unimplemented!("helper::forward::forward_import")
}

#[cfg(test)]
mod spec_tests {
    use super::forward_import;
    use crate::HelperError;

    /// REQ: HST-159 (every import that is not pure compute is forwarded to the host)
    #[test]
    fn forwards_host_state_import_and_returns_host_reply() {
        let reply = forward_import("ilium:storage", "get", b"settings/theme");
        assert!(reply.is_ok(), "a forwarded storage import must return the host's reply");
    }

    /// REQ: HST-159 (an empty payload is a valid argument encoding and is still forwarded)
    #[test]
    fn forwards_import_with_empty_payload() {
        let reply = forward_import("ilium:clipboard", "clear", &[]);
        assert!(reply.is_ok(), "an import with no argument bytes must still be forwarded");
    }

    /// REQ: HST-155 (a schema violation ends the helper and counts as a failure)
    #[test]
    fn empty_interface_name_is_a_protocol_violation() {
        let outcome = forward_import("", "get", b"");
        assert!(
            matches!(outcome, Err(HelperError::Protocol(_))),
            "an empty interface name must be reported as HelperError::Protocol"
        );
    }

    /// REQ: HST-155 (a schema violation ends the helper and counts as a failure)
    #[test]
    fn empty_function_name_is_a_protocol_violation() {
        let outcome = forward_import("ilium:storage", "", b"");
        assert!(
            matches!(outcome, Err(HelperError::Protocol(_))),
            "an empty function name must be reported as HelperError::Protocol"
        );
    }

    /// REQ: HST-039 (a helper channel failure is reported as an error, never a hang or crash)
    #[test]
    fn closed_host_channel_is_reported_as_channel_error() {
        let outcome = forward_import("ilium:timers", "sleep", b"\x01\x00\x00\x00");
        assert!(
            matches!(outcome, Err(HelperError::Channel(_))),
            "a closed host channel must surface as HelperError::Channel"
        );
    }
}
