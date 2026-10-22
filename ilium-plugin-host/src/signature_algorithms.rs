//! Closed, versioned allow-list of signature and digest algorithms compiled into the binary
//! (SEC-459). An unknown algorithm in a key, signature or digest entry is a verification
//! failure; a retired algorithm is refused for metadata or packages issued after its retirement.

use ilium_plugin_api::manifest::Diagnostic;

/// The algorithm names this build accepts, in allow-list order (SEC-459, PKG-205).
pub fn accepted_algorithms() -> &'static [&'static str] {
    unimplemented!("host::signature_algorithms::accepted_algorithms")
}

/// Accepts `algorithm` only when it is on the compiled-in allow-list; any other name is an error.
pub fn check_algorithm(algorithm: &str) -> Result<(), Diagnostic> {
    unimplemented!("host::signature_algorithms::check_algorithm: {algorithm}")
}
