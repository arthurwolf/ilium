//! Parse component-model text into actual WebAssembly component bytes for tests.

/// Test fixture parsing. Invalid WAT is a fixture error, not a host runtime result.
pub fn wat_component(text: &str) -> Vec<u8> {
    wat::parse_str(text).unwrap_or_else(|error| panic!("invalid component fixture: {error}"))
}

#[cfg(test)]
mod tests {
    use super::wat_component;

    /// The 8-byte component preamble: magic `\0asm`, then layer 1 (component) with version 0x0d.
    const COMPONENT_PREAMBLE: [u8; 8] = [0x00, 0x61, 0x73, 0x6D, 0x0D, 0x00, 0x01, 0x00];

    /// REQ: PKG-141
    #[test]
    fn empty_component_text_compiles_to_component_preamble() {
        let bytes = wat_component("(component)");
        assert_eq!(bytes, COMPONENT_PREAMBLE.to_vec());
    }

    /// REQ: PKG-141
    #[test]
    fn output_starts_with_layer_one_component_magic() {
        let bytes = wat_component("(component)");
        assert!(
            bytes.len() >= 8,
            "output shorter than the preamble: {} bytes",
            bytes.len()
        );
        assert_eq!(&bytes[0..4], &[0x00, 0x61, 0x73, 0x6D]);
        assert_eq!(bytes[4], 0x0D, "layer field must mark a component");
    }

    #[test]
    fn same_text_compiles_to_identical_bytes() {
        let text = "(component (core module $m (func (export \"f\"))))";
        let first = wat_component(text);
        let second = wat_component(text);
        assert_eq!(first, second);
    }

    #[test]
    fn component_with_core_module_is_longer_than_empty_component() {
        let empty = wat_component("(component)");
        let with_module = wat_component("(component (core module $m (func (export \"f\"))))");
        assert!(
            with_module.len() > empty.len(),
            "module-bearing component ({} bytes) must exceed empty component ({} bytes)",
            with_module.len(),
            empty.len()
        );
    }

    #[test]
    fn whitespace_around_component_text_does_not_change_output() {
        let compact = wat_component("(component)");
        let padded = wat_component("  \n(component)\n  ");
        assert_eq!(compact, padded);
    }

    #[test]
    fn empty_input_does_not_yield_a_bare_component_preamble() {
        // The doc comment defines the input as `(component ...)` text. Empty text is not that
        // form, so it must not compile to the bare 8-byte empty component.
        let bytes = wat_component("");
        assert_ne!(bytes, COMPONENT_PREAMBLE.to_vec());
    }
}
