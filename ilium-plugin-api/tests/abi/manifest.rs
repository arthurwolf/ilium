//! Manifest and component checks of the ABI rows: ABI-004, ABI-006, ABI-007, ABI-018,
//! ABI-021..ABI-024. Serde tests are pure data; the rest call stubs.

use ilium_plugin_api::abi::UNSTABLE_FEATURES;
use ilium_plugin_api::codes;
use ilium_plugin_api::generated;
use ilium_plugin_api::manifest::{
    check_component, validate, validate_json, ComponentSummary, LimitsTier, Manifest,
    DEFAULT_DATA_VERSION,
};
use ilium_plugin_api::version::{
    ApiVersion, API_VERSION_BYTES, API_VERSION_SECTION, CONTRACT, CURRENT, REFUSAL_ORDER,
};
use ilium_plugin_api::{Side, Tier};
use proptest::prelude::*;
use serde_json::json;

use crate::support::{codes, context, minimal_manifest};

fn manifest_from(value: serde_json::Value) -> Manifest {
    serde_json::from_value(value).expect("manifest deserializes")
}

fn codes_of_json(value: &serde_json::Value, tier: Tier) -> Vec<String> {
    codes(&validate_json(&value.to_string(), &context(tier)))
}

/// A server component that matches the minimal manifest exactly.
fn matching_server() -> ComponentSummary {
    ComponentSummary {
        imports: Vec::new(),
        exports: vec!["lifecycle".to_owned(), "handler".to_owned()],
        api_sections: vec![API_VERSION_BYTES.to_vec()],
        unstable_imports: Vec::new(),
    }
}

fn component_codes(manifest: &serde_json::Value, component: &ComponentSummary) -> Vec<String> {
    codes(&check_component(
        &manifest_from(manifest.clone()),
        Side::Server,
        component,
        &context(Tier::T3),
    ))
}

/// REQ: ABI-018
#[test]
fn abi_018_minimal_manifest_deserializes_with_defaults() {
    let manifest = manifest_from(minimal_manifest());
    assert_eq!(manifest.api, CONTRACT);
    assert_eq!(manifest.data_version, DEFAULT_DATA_VERSION);
    assert_eq!(manifest.limits_tier, LimitsTier::Standard);
    let server = manifest.server.expect("server component");
    assert_eq!(server.component, "server.wasm", "JSON key `entry`");
    assert!(server.exports.is_empty());
}

/// REQ: ABI-018
#[test]
fn abi_018_component_serializes_entry_not_component() {
    let manifest = manifest_from(minimal_manifest());
    let value = serde_json::to_value(&manifest).expect("serialize");
    assert_eq!(value["server"]["entry"], "server.wasm");
    assert!(value["server"].get("component").is_none());
}

/// REQ: ABI-018
#[test]
fn abi_018_minimal_manifest_validates_clean() {
    assert_eq!(
        codes_of_json(&minimal_manifest(), Tier::T3),
        Vec::<String>::new()
    );
}

/// REQ: ABI-018
#[test]
fn abi_018_listing_lifecycle_is_a_schema_error() {
    let mut value = minimal_manifest();
    value["server"]["exports"] = json!(["lifecycle"]);
    assert!(codes_of_json(&value, Tier::T3).contains(&codes::manifest::SCHEMA.to_owned()));
}

/// REQ: ABI-018
#[test]
fn abi_018_listing_handler_is_a_schema_error() {
    let mut value = minimal_manifest();
    value["server"]["exports"] = json!(["handler"]);
    assert!(codes_of_json(&value, Tier::T3).contains(&codes::manifest::SCHEMA.to_owned()));
}

/// REQ: ABI-018
#[test]
fn abi_018_listing_renderer_on_a_scene_is_a_schema_error() {
    let mut value = minimal_manifest();
    value["scene"] = json!({
        "entry": "scene.wasm",
        "world": "ilium:anim/base@0.1.0",
        "exports": ["renderer"]
    });
    assert!(codes_of_json(&value, Tier::T3).contains(&codes::manifest::SCHEMA.to_owned()));
}

/// REQ: ABI-018
#[test]
fn abi_018_listing_an_optional_export_is_accepted() {
    let mut value = minimal_manifest();
    value["server"]["exports"] = json!(["detector"]);
    assert_eq!(codes_of_json(&value, Tier::T3), Vec::<String>::new());
}

/// REQ: ABI-006
#[test]
fn abi_006_equal_export_lists_pass() {
    let mut value = minimal_manifest();
    value["server"]["exports"] = json!(["detector"]);
    let mut component = matching_server();
    component.exports.push("detector".to_owned());
    assert_eq!(component_codes(&value, &component), Vec::<String>::new());
}

/// REQ: ABI-006
#[test]
fn abi_006_an_unlisted_extra_export_is_refused() {
    let mut component = matching_server();
    component.exports.push("detector".to_owned());
    assert_eq!(
        component_codes(&minimal_manifest(), &component),
        vec![codes::manifest::EXPORTS_MISMATCH.to_owned()]
    );
}

/// REQ: ABI-007
#[test]
fn abi_007_a_listed_missing_export_is_refused() {
    let mut value = minimal_manifest();
    value["server"]["exports"] = json!(["detector"]);
    assert_eq!(
        component_codes(&value, &matching_server()),
        vec![codes::manifest::EXPORTS_MISMATCH.to_owned()]
    );
}

/// REQ: ABI-004
#[test]
fn abi_004_section_bytes_are_00_00_00_01_00_00() {
    assert_eq!(API_VERSION_SECTION, "ilium:api-version");
    assert_eq!(API_VERSION_BYTES, [0x00, 0x00, 0x00, 0x01, 0x00, 0x00]);
    assert_eq!(CURRENT.to_string(), "0.1.0");
}

/// REQ: ABI-004
#[test]
fn abi_004_section_decodes_to_the_current_version() {
    assert_eq!(
        ApiVersion::from_custom_section(&API_VERSION_BYTES),
        Ok(CURRENT)
    );
}

/// REQ: ABI-004
#[test]
fn abi_004_section_of_another_length_is_refused() {
    let error = ApiVersion::from_custom_section(&[0, 0, 0, 1, 0]).expect_err("five bytes");
    assert_eq!(error.code.as_str(), codes::package::API_SECTION_INVALID);
}

/// REQ: ABI-004
#[test]
fn abi_004_component_without_section_is_refused() {
    let mut component = matching_server();
    component.api_sections.clear();
    assert!(component_codes(&minimal_manifest(), &component)
        .contains(&codes::package::API_SECTION_MISSING.to_owned()));
}

/// REQ: ABI-004, ABI-021
#[test]
fn abi_004_section_that_differs_from_the_manifest_is_api_version() {
    let mut component = matching_server();
    component.api_sections = vec![vec![0, 0, 0, 2, 0, 0]];
    assert!(component_codes(&minimal_manifest(), &component)
        .contains(&codes::manifest::API_VERSION.to_owned()));
}

/// REQ: ABI-021
#[test]
fn abi_021_contract_string_parses() {
    assert_eq!(ApiVersion::parse(CONTRACT), Ok(CURRENT));
}

/// REQ: ABI-021
#[test]
fn abi_021_malformed_contract_string_is_schema() {
    let error = ApiVersion::parse("ilium:plugin@zero").expect_err("malformed");
    assert_eq!(error.code.as_str(), codes::manifest::SCHEMA);
}

/// REQ: ABI-021
#[test]
fn abi_021_host_window_is_current_and_previous_minor() {
    let window = ApiVersion::host_window();
    assert!(window.contains(&CURRENT));
    assert!(window.contains(&ApiVersion {
        major: 0,
        minor: 0,
        patch: 0
    }));
    assert!(!window.contains(&ApiVersion {
        major: 0,
        minor: 2,
        patch: 0
    }));
    assert!(!window.contains(&ApiVersion {
        major: 0,
        minor: 3,
        patch: 0
    }));
}

fn refusal_codes(value: &serde_json::Value) -> Vec<String> {
    codes_of_json(value, Tier::T3)
        .into_iter()
        .filter(|code| REFUSAL_ORDER.contains(&code.as_str()))
        .collect()
}

/// REQ: ABI-021
#[test]
fn abi_021_newer_minor_is_api_unsupported() {
    let mut value = minimal_manifest();
    value["api"] = json!("ilium:plugin@0.3.0");
    assert_eq!(
        refusal_codes(&value),
        vec![codes::package::API_UNSUPPORTED.to_owned()]
    );
}

/// REQ: ABI-021
#[test]
fn abi_021_one_refusal_code_per_package_first_failing_check() {
    let mut value = minimal_manifest();
    value["api"] = json!("ilium:plugin@0.3.0");
    value["capabilities"] = json!({ "server": ["teleport"] });
    assert_eq!(
        refusal_codes(&value),
        vec![codes::package::API_UNSUPPORTED.to_owned()],
        "grant-unknown must not be reported beside api-unsupported"
    );
}

/// REQ: ABI-021
#[test]
fn abi_021_malformed_api_beats_every_later_refusal() {
    let mut value = minimal_manifest();
    value["api"] = json!("ilium-plugin-0.1");
    value["capabilities"] = json!({ "server": ["teleport"] });
    assert_eq!(
        refusal_codes(&value),
        vec![codes::manifest::SCHEMA.to_owned()]
    );
}

/// REQ: ABI-022
#[test]
fn abi_022_exactly_four_unstable_features() {
    assert_eq!(
        UNSTABLE_FEATURES,
        ["ui-calendar", "plugin-services", "gpu-kernels", "websocket"]
    );
}

/// REQ: ABI-022
#[test]
fn abi_022_wit_markers_use_only_the_four_features() {
    let mut used: Vec<&str> = generated::UNSTABLE_ITEMS
        .iter()
        .map(|(feature, _)| *feature)
        .collect();
    used.sort_unstable();
    used.dedup();
    for feature in &used {
        assert!(UNSTABLE_FEATURES.contains(feature), "{feature}");
    }
    assert!(!generated::UNSTABLE_ITEMS.is_empty());
    for feature in UNSTABLE_FEATURES {
        assert!(used.contains(&feature), "no WIT item is marked {feature}");
    }
}

/// REQ: ABI-022
#[test]
fn abi_022_unknown_feature_is_unstable_unknown() {
    let mut value = minimal_manifest();
    value["unstable"] = json!(["teleport"]);
    assert!(codes_of_json(&value, Tier::T1).contains(&codes::manifest::UNSTABLE_UNKNOWN.to_owned()));
}

/// REQ: ABI-022
#[test]
fn abi_022_third_party_feature_outside_dev_mode_is_unstable_tier() {
    let mut value = minimal_manifest();
    value["unstable"] = json!(["websocket"]);
    value["files"] = json!([{
        "path": "server.wasm", "length": 0,
        "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    }]);
    let mut admission = context(Tier::T2);
    admission.mode = ilium_plugin_api::manifest::CheckMode::Install;
    assert!(codes(&validate_json(&value.to_string(), &admission))
        .contains(&codes::manifest::UNSTABLE_TIER.to_owned()));
}

/// REQ: ABI-022
#[test]
fn abi_022_unstable_import_without_the_feature_is_unstable_missing() {
    let mut component = matching_server();
    component.unstable_imports.push("websocket".to_owned());
    assert!(component_codes(&minimal_manifest(), &component)
        .contains(&codes::manifest::UNSTABLE_MISSING.to_owned()));
}

/// REQ: ABI-023
#[test]
fn abi_023_unknown_grant_is_grant_unknown() {
    let mut value = minimal_manifest();
    value["capabilities"] = json!({ "server": ["teleport"] });
    assert!(codes_of_json(&value, Tier::T3).contains(&codes::manifest::GRANT_UNKNOWN.to_owned()));
}

/// REQ: ABI-023
#[test]
fn abi_023_import_without_grant_is_import_not_requested() {
    let mut component = matching_server();
    component.imports.push("ilium:host/http@0.1.0".to_owned());
    assert_eq!(
        component_codes(&minimal_manifest(), &component),
        vec![codes::manifest::IMPORT_NOT_REQUESTED.to_owned()]
    );
}

/// REQ: ABI-023
#[test]
fn abi_023_import_with_its_grant_passes() {
    let mut value = minimal_manifest();
    value["capabilities"] = json!({ "server": ["http:api.example.com"] });
    let mut component = matching_server();
    component.imports.push("ilium:host/http@0.1.0".to_owned());
    assert_eq!(component_codes(&value, &component), Vec::<String>::new());
}

/// REQ: ABI-024
#[test]
fn abi_024_wasi_sockets_import_is_refused() {
    let mut component = matching_server();
    component.imports.push("wasi:sockets/tcp@0.2.0".to_owned());
    assert!(component_codes(&minimal_manifest(), &component)
        .contains(&codes::component::WASI_FORBIDDEN.to_owned()));
}

/// REQ: ABI-024
#[test]
fn abi_024_wasi_http_import_is_refused() {
    let mut component = matching_server();
    component
        .imports
        .push("wasi:http/outgoing-handler@0.2.0".to_owned());
    assert!(component_codes(&minimal_manifest(), &component)
        .contains(&codes::component::WASI_FORBIDDEN.to_owned()));
}

/// REQ: ABI-024
#[test]
fn abi_024_wasi_clocks_import_is_allowed() {
    let mut component = matching_server();
    component
        .imports
        .push("wasi:clocks/wall-clock@0.2.0".to_owned());
    assert!(!component_codes(&minimal_manifest(), &component)
        .contains(&codes::component::WASI_FORBIDDEN.to_owned()));
}

/// REQ: ABI-018
#[test]
fn abi_018_validate_agrees_with_validate_json() {
    let value = minimal_manifest();
    let manifest = manifest_from(value.clone());
    assert_eq!(
        codes(&validate(&manifest, &context(Tier::T3))),
        codes_of_json(&value, Tier::T3)
    );
}

const REQUIRED_EXPORTS: [&str; 3] = ["lifecycle", "handler", "renderer"];

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    // REQ: ABI-018
    #[test]
    fn abi_018_any_listed_required_export_is_a_schema_error(index in 0usize..3, extra in proptest::bool::ANY) {
        let mut value = minimal_manifest();
        let mut exports = vec![REQUIRED_EXPORTS[index]];
        if extra {
            exports.push("detector");
        }
        value["server"]["exports"] = json!(exports);
        prop_assert!(codes_of_json(&value, Tier::T3).contains(&codes::manifest::SCHEMA.to_owned()));
    }

    // REQ: ABI-021
    #[test]
    fn abi_021_minor_outside_the_window_gets_exactly_one_refusal(minor in 2u16..1000, patch in 0u16..100) {
        let mut value = minimal_manifest();
        value["api"] = json!(format!("ilium:plugin@0.{minor}.{patch}"));
        prop_assert_eq!(refusal_codes(&value), vec![codes::package::API_UNSUPPORTED.to_owned()]);
    }

    // REQ: ABI-004
    #[test]
    fn abi_004_any_section_length_but_six_is_refused(bytes in proptest::collection::vec(any::<u8>(), 0..32)) {
        prop_assume!(bytes.len() != 6);
        prop_assert!(ApiVersion::from_custom_section(&bytes).is_err());
    }
}
