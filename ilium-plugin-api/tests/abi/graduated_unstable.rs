//! ABI-083: a manifest that still lists a graduated unstable feature loads and gets one
//! `deprecated` record instead of a `manifest.unstable-unknown` refusal.

use ilium_plugin_api::deprecation::{deprecation_records, MarkedFeature};
use ilium_plugin_api::manifest::{validate_json, CheckMode, ValidationContext};
use ilium_plugin_api::{Platform, Tier};

const GRADUATED: &str = "websocket";
const PLUGIN: &str = "example.notes";

/// The host after `websocket` graduated: it is no longer in the unstable list.
fn host_after_graduation() -> ValidationContext {
    ValidationContext {
        tier: Tier::T3,
        mode: CheckMode::Check,
        platform: Platform::Linux,
        installed: Vec::new(),
        core_plugins: Vec::new(),
        unstable_features: ["ui-calendar", "plugin-services", "gpu-kernels"]
            .iter()
            .map(|name| (*name).to_owned())
            .collect(),
    }
}

/// REQ: ABI-083
#[test]
fn abi_083_graduated_unstable_name_is_not_refused_as_unknown() {
    let manifest = format!(
        r#"{{"id":"{PLUGIN}","version":"1.0.0","api":"0.1.0","tier":"T3","unstable":["{GRADUATED}"]}}"#
    );
    let found = validate_json(&manifest, &host_after_graduation());
    let codes: Vec<&str> = found.iter().map(|diagnostic| diagnostic.code.as_str()).collect();
    assert!(!codes.contains(&"manifest.unstable-unknown"), "{codes:?}");
}

/// REQ: ABI-083
#[test]
fn abi_083_graduated_unstable_name_writes_one_deprecated_record() {
    let marked = [MarkedFeature {
        name: GRADUATED.to_owned(),
        marked_in_minor: 1,
        removal_minor: 2,
    }];
    let records = deprecation_records(PLUGIN, &[GRADUATED], &marked, 2);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].feature, GRADUATED);
}
