//! Component-model text to component bytes, so host tests need no wasm target (test plan 0.5).

/// Compiles component-model text (`(component ...)`) to a binary component.
pub fn wat_component(_text: &str) -> Vec<u8> {
    unimplemented!("host::testing::wat_component")
}
