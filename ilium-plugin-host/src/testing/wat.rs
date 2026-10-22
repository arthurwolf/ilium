//! Parse component-model text into actual WebAssembly component bytes for tests.

/// Test fixture parsing. Invalid WAT is a fixture error, not a host runtime result.
pub fn wat_component(text: &str) -> Vec<u8> {
    wat::parse_str(text).unwrap_or_else(|error| panic!("invalid component fixture: {error}"))
}
