//! Closed, versioned allow-list of signature and digest algorithms compiled into the binary
//! (SEC-459). An unknown algorithm in a key, signature or digest entry is a verification
//! failure; a retired algorithm is refused for metadata or packages issued after its retirement.

use ilium_plugin_api::manifest::Diagnostic;

/// The algorithm names this build accepts, in allow-list order (SEC-459, PKG-205).
pub fn accepted_algorithms() -> &'static [&'static str] {
    &["ed25519"]
}

/// Accepts `algorithm` only when it is on the compiled-in allow-list; any other name is an error.
pub fn check_algorithm(algorithm: &str) -> Result<(), Diagnostic> {
    if accepted_algorithms().contains(&algorithm) {
        return Ok(());
    }
    Err(Diagnostic::error(
        ilium_plugin_api::Code::from_static(ilium_plugin_api::codes::manifest::SIGNATURE_INVALID),
        "/signatures",
        format!("signature algorithm `{algorithm}` is not on the compiled-in allow-list"),
    ))
}
