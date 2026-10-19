//! Secret redaction and the encodings it must recognise (reference-secrets.md, reference-audit-log.md).

/// Replaces every secret, in every encoding the host recognises, with the redaction marker.
pub fn redact(_text: &str, _secrets: &[String]) -> String {
    unimplemented!("host::secrets::redact")
}

/// Standard base64 with padding.
pub fn base64_standard(_bytes: &[u8]) -> String {
    unimplemented!("host::secrets::base64_standard")
}

/// URL-safe base64 without padding.
pub fn base64_url(_bytes: &[u8]) -> String {
    unimplemented!("host::secrets::base64_url")
}

/// Percent-encoding of every byte outside the unreserved set.
pub fn percent_encode(_text: &str) -> String {
    unimplemented!("host::secrets::percent_encode")
}

/// Lower-case hex.
pub fn hex_lower(_bytes: &[u8]) -> String {
    unimplemented!("host::secrets::hex_lower")
}

/// The masked display row of a secret, keeping `visible` trailing characters.
pub fn masked_row(_secret: &str, _visible: u8) -> String {
    unimplemented!("host::secrets::masked_row")
}
