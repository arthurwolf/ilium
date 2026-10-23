//! reference-manifest.md: `requires.plugins` and `requires.services` hold at most 64 entries each
//! (PKG-805). A longer list is refused with `manifest.limit` naming the key.

use ilium_plugin_api::manifest::parse_manifest;
use serde_json::{json, Value};

use crate::support::*;

/// A manifest whose `requires.plugins` and `requires.services` hold the given numbers of entries.
fn requires_manifest(plugin_count: usize, service_count: usize) -> Value {
    let plugins: Vec<String> = (0..plugin_count)
        .map(|index| format!("dep{index:02}@1.0.0"))
        .collect();
    let services: Vec<String> = (0..service_count)
        .map(|index| format!("svc{index:02}@1.0.0"))
        .collect();
    let mut value = base_manifest();
    value["requires"] = json!({ "plugins": plugins, "services": services });
    value
}

/// REQ: PKG-805
#[test]
fn pkg_805_sixty_five_entries_under_each_key_are_refused_naming_each_key() {
    let findings = manifest_findings(&requires_manifest(65, 65));
    let limits: Vec<_> = findings
        .iter()
        .filter(|diagnostic| diagnostic.code.as_str() == "manifest.limit")
        .collect();
    assert!(
        limits.iter().any(|diagnostic| {
            diagnostic.pointer.contains("plugins") || diagnostic.message.contains("plugins")
        }),
        "a 65-entry requires.plugins is refused with manifest.limit naming the plugins key"
    );
    assert!(
        limits.iter().any(|diagnostic| {
            diagnostic.pointer.contains("services") || diagnostic.message.contains("services")
        }),
        "a 65-entry requires.services is refused with manifest.limit naming the services key"
    );
}

/// REQ: PKG-805
#[test]
fn pkg_805_sixty_four_entries_under_each_key_install() {
    parse_manifest(&requires_manifest(64, 64).to_string())
        .expect("a manifest with 64 entries under each key installs");
}
