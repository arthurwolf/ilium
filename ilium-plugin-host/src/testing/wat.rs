//! Parse component-model text into actual WebAssembly component bytes for tests.

/// Test fixture parsing. Invalid WAT is a fixture error, not a host runtime result.
pub fn wat_component(text: &str) -> Vec<u8> {
    wat::parse_str(text).unwrap_or_else(|error| panic!("invalid component fixture: {error}"))
}

/// A genuine component for install tests: it exports the required `lifecycle` and `handler`
/// interfaces plus the given optional exports, and carries the `ilium:api-version` custom
/// section for API 0.1.0 (reference-manifest.md, PKG-042 and PKG-043).
pub fn plugin_component(optional_exports: &[&str]) -> Vec<u8> {
    let mut text = String::from("(component\n");
    for (index, name) in ["lifecycle", "handler"]
        .iter()
        .chain(optional_exports.iter())
        .enumerate()
    {
        text.push_str(&format!(
            "  (instance $e{index})\n  (export \"ilium:exports/{name}@0.1.0\" (instance $e{index}))\n"
        ));
    }
    text.push(')');
    let mut bytes = wat_component(&text);
    // Custom section: id 0, size, name length, name, then major.minor.patch as three big-endian u16.
    let name = b"ilium:api-version";
    let payload = [0u8, 0, 0, 1, 0, 0];
    bytes.push(0);
    bytes.push((1 + name.len() + payload.len()) as u8);
    bytes.push(name.len() as u8);
    bytes.extend_from_slice(name);
    bytes.extend_from_slice(&payload);
    bytes
}

/// The `files` inventory of a package: length and SHA-256 of every entry except `manifest.json`
/// and `signature`, bytewise sorted by path as the manifest reference requires.
pub fn files_inventory(files: &[(String, Vec<u8>)]) -> serde_json::Value {
    use sha2::{Digest, Sha256};
    let mut listed: Vec<_> = files
        .iter()
        .filter(|(path, _)| path != "manifest.json" && path != "signature")
        .collect();
    listed.sort_by(|left, right| left.0.cmp(&right.0));
    serde_json::Value::Array(
        listed
            .into_iter()
            .map(|(path, bytes)| {
                serde_json::json!({
                    "path": path,
                    "length": bytes.len(),
                    "sha256": format!("{:x}", Sha256::digest(bytes)),
                })
            })
            .collect(),
    )
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
