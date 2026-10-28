//! Decoding of wire error code strings into the SDK view (API-088).
//!
//! A plugin treats a code it does not know as a failed call of unspecified cause. The SDKs
//! expose such a code as an `Unknown` variant that carries the code text, and never match on
//! the message. This stub models that decoding on the host side until the SDK decoder exists.

use ilium_plugin_api::generated::types::ErrorCode;

/// A decoded error code: a code of the contract version the decoder knows, or an unknown code
/// kept as its text.
#[derive(Debug, Clone, PartialEq)]
pub enum DecodedErrorCode {
    /// A code the decoder knows.
    Known(ErrorCode),
    /// A code the decoder does not know; the text is kept unchanged.
    Unknown(String),
}

/// Decodes one wire error code string. Unknown text never fails the decode.
pub fn decode_error_code(text: &str) -> DecodedErrorCode {
    use ErrorCode::*;
    let known = [
        NotPermitted,
        NotFound,
        InvalidArgument,
        Conflict,
        QuotaExceeded,
        RateLimited,
        Busy,
        TimedOut,
        Cancelled,
        Unsupported,
        PaneNotReady,
        Revoked,
        Io,
        Network,
        Internal,
    ];
    match known.into_iter().find(|code| code.as_str() == text) {
        Some(code) => DecodedErrorCode::Known(code),
        None => DecodedErrorCode::Unknown(text.to_owned()),
    }
}
