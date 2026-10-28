//! Imports forwarded from the helper to the host (reference-host-architecture.md).

use crate::protocol::HelperReply;
use crate::HelperError;

/// Sends one import call to the host and waits for its answer.
///
/// The call is validated first (empty names are protocol violations). The answer travels on the
/// channel owned by [`crate::serve`]; outside a served session no host channel exists, which is
/// reported as [`HelperError::Channel`].
pub fn forward_import(
    interface: &str,
    function: &str,
    payload: &[u8],
) -> Result<Vec<u8>, HelperError> {
    import_request(interface, function, payload)?;
    Err(HelperError::Channel(
        "no host channel is open outside a served session".to_owned(),
    ))
}

/// Forwards one import over an open host channel: writes the `ForwardImport` frame, then reads
/// the host's `Ok` (the import result) or `Failed` reply.
pub fn forward_import_over(
    input: &mut impl std::io::Read,
    output: &mut impl std::io::Write,
    interface: &str,
    function: &str,
    payload: &[u8],
) -> Result<Vec<u8>, HelperError> {
    let request = import_request(interface, function, payload)?;
    crate::framing::write_frame(output, &request)?;
    match crate::framing::read_frame::<HelperReply>(input)? {
        Some(HelperReply::Ok { payload }) => Ok(payload),
        Some(HelperReply::Failed { code, message }) => {
            Err(HelperError::Protocol(format!("host refused import {code}: {message}")))
        }
        Some(HelperReply::ForwardImport { .. }) => Err(HelperError::Protocol(
            "host answered an import with another import".to_owned(),
        )),
        None => Err(HelperError::Channel("host closed the channel mid-import".to_owned())),
    }
}

/// Builds the `ForwardImport` frame body for one import call.
pub(crate) fn import_request(
    interface: &str,
    function: &str,
    payload: &[u8],
) -> Result<HelperReply, HelperError> {
    if interface.is_empty() {
        return Err(HelperError::Protocol("import interface name is empty".to_owned()));
    }
    if function.is_empty() {
        return Err(HelperError::Protocol("import function name is empty".to_owned()));
    }
    Ok(HelperReply::ForwardImport {
        interface: interface.to_owned(),
        function: function.to_owned(),
        payload: payload.to_vec(),
    })
}

#[cfg(test)]
mod spec_tests {
    use super::forward_import;
    use crate::HelperError;

    /// REQ: HST-159
    #[test]
    fn forwards_host_state_import_and_returns_host_reply() {
        let reply = forward_import("ilium:storage", "get", b"settings/theme");
        assert!(
            reply.is_ok(),
            "a forwarded storage import must return the host's reply"
        );
    }

    /// REQ: HST-159
    #[test]
    fn forwards_import_with_empty_payload() {
        let reply = forward_import("ilium:clipboard", "clear", &[]);
        assert!(
            reply.is_ok(),
            "an import with no argument bytes must still be forwarded"
        );
    }

    /// REQ: HST-155
    #[test]
    fn empty_interface_name_is_a_protocol_violation() {
        let outcome = forward_import("", "get", b"");
        assert!(
            matches!(outcome, Err(HelperError::Protocol(_))),
            "an empty interface name must be reported as HelperError::Protocol"
        );
    }

    /// REQ: HST-155
    #[test]
    fn empty_function_name_is_a_protocol_violation() {
        let outcome = forward_import("ilium:storage", "", b"");
        assert!(
            matches!(outcome, Err(HelperError::Protocol(_))),
            "an empty function name must be reported as HelperError::Protocol"
        );
    }

    /// REQ: HST-039
    #[test]
    fn closed_host_channel_is_reported_as_channel_error() {
        let outcome = forward_import("ilium:timers", "sleep", b"\x01\x00\x00\x00");
        assert!(
            matches!(outcome, Err(HelperError::Channel(_))),
            "a closed host channel must surface as HelperError::Channel"
        );
    }
}
