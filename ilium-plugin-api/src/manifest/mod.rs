//! The manifest model, its validation and its diagnostics (reference-manifest.md,
//! reference-manifest-schema.md).
//!
//! JSON keys are snake_case as in the manual. Every optional key has a serde default so a
//! minimal manifest deserializes; the checks of the manual live in [`validate`], not in serde.

mod component;
mod diagnostic;
mod syntax;
mod validate;

pub use component::{check_component, ComponentSummary};
pub use diagnostic::{Diagnostic, Stage};
pub use syntax::{
    parse_activation, parse_chord, parse_when, ActivationTrigger, Chord, WhenExpr, ACTIVATION_KINDS,
};
pub use validate::{
    json_schema, parse_manifest, validate, validate_json, CheckMode, CorePluginSummary,
    InstalledSummary, ValidationContext,
};

use serde::{Deserialize, Serialize};

use crate::grants::Grant;
use crate::limits::Limits;

/// A parsed `manifest.json` (reference-manifest.md "Top-level keys").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    /// Contract identity, exactly `ilium:plugin@0.1.0` (reference-manifest.md PKG-013).
    pub api: String,
    /// Plugin id, 2 to 8 `.`-separated segments (reference-manifest.md PKG-014).
    pub id: String,
    /// Display name (reference-manifest.md PKG-015).
    pub name: String,
    /// SemVer 2.0.0 version (reference-manifest.md PKG-016).
    pub version: String,
    /// Publisher display name (reference-manifest.md PKG-017).
    pub publisher: String,
    /// One-line description (reference-manifest.md PKG-018).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// SPDX licence expression (reference-manifest.md PKG-019).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// Tabler icon name (reference-manifest.md PKG-020).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Registry categories (reference-manifest.md PKG-021).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub categories: Vec<String>,
    /// Search keywords (reference-manifest.md PKG-022).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
    /// `https://` home page (reference-manifest.md PKG-023).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    /// `https://` source repository (reference-manifest.md PKG-023).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    /// The server component (reference-manifest.md PKG-024).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<ComponentDecl>,
    /// The client component (reference-manifest.md PKG-024).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<ComponentDecl>,
    /// The scene component (reference-manifest.md PKG-024).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene: Option<ComponentDecl>,
    /// One grant list per side (reference-manifest.md PKG-025).
    #[serde(default)]
    pub capabilities: Capabilities,
    /// Activation triggers; empty means lazy (reference-manifest.md PKG-026).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub activation: Vec<String>,
    /// Host range, services and plugins (reference-dependency-resolution.md).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires: Option<Requires>,
    /// Replaced core plugins (reference-manifest.md PKG-028).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub replaces: Vec<String>,
    /// Plugin ids allowed to send `messages.peer` messages (reference-manifest.md).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accepts_messages_from: Vec<String>,
    /// Core plugin may be replaced (reference-manifest.md PKG-029).
    #[serde(default)]
    pub replaceable: bool,
    /// Replaceable surface of a core plugin (reference-manifest.md PKG-029).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub surface: Vec<String>,
    /// Static contributions (reference-manifest.md PKG-030).
    #[serde(default)]
    pub contributes: Contributes,
    /// Requested limits, clamped to the tier ceiling (reference-manifest.md PKG-031).
    #[serde(default)]
    pub limits: Limits,
    /// Data version, default 1 (reference-manifest.md PKG-032).
    #[serde(default = "default_data_version")]
    pub data_version: u32,
    /// `standard` or `first-party` (reference-manifest.md PKG-033).
    #[serde(default)]
    pub limits_tier: LimitsTier,
    /// Unstable features used (reference-manifest.md PKG-034).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unstable: Vec<String>,
    /// File inventory of a packed package (reference-package-format.md "The inventory").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<FileEntry>,
}

/// The default of `data_version` (reference-manifest.md PKG-032).
pub const DEFAULT_DATA_VERSION: u32 = 1;

fn default_data_version() -> u32 {
    DEFAULT_DATA_VERSION
}

/// One side's component: `{ "entry", "world", "exports" }` (reference-manifest.md
/// "Components and exports"). The Rust field `component` is the JSON key `entry`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentDecl {
    /// File name of the component, JSON key `entry` (reference-manifest.md PKG-040).
    #[serde(rename = "entry")]
    pub component: String,
    /// The world the component targets (reference-manifest.md PKG-041).
    pub world: String,
    /// Optional exports the component has; required ones are never listed (reference-manifest.md PKG-042).
    #[serde(default)]
    pub exports: Vec<String>,
}

/// One grant list per side (reference-manifest.md "The capabilities object").
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Capabilities {
    /// Grants of the server side (reference-capabilities.md).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub server: Vec<Grant>,
    /// Grants of the client side (reference-capabilities.md).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub client: Vec<Grant>,
    /// Grants of the scene side (reference-capabilities.md).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scene: Vec<Grant>,
}

/// The `requires` object (reference-dependency-resolution.md "The requires object").
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Requires {
    /// Host version range (reference-dependency-resolution.md).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// Services as `<interface>@<range>` strings; the range defaults to `*`
    /// (reference-dependency-resolution.md). Parse one with [`ServiceRequirement::parse`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub services: Vec<String>,
    /// Required plugins (reference-dependency-resolution.md).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub plugins: Vec<PluginRequirement>,
}

/// One parsed `requires.services` entry (reference-dependency-resolution.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceRequirement {
    /// Service interface name (reference-dependency-resolution.md).
    pub name: String,
    /// Version range, `*` when omitted (reference-dependency-resolution.md).
    pub version: String,
}

impl ServiceRequirement {
    /// Parses `<interface>@<range>`; a bad entry is `manifest.requires-invalid`
    /// (reference-dependency-resolution.md).
    pub fn parse(_text: &str) -> Result<ServiceRequirement, Diagnostic> {
        unimplemented!("api::manifest::ServiceRequirement::parse")
    }
}

/// One `requires.plugins` entry (reference-dependency-resolution.md).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginRequirement {
    /// Required plugin id (reference-dependency-resolution.md).
    pub id: String,
    /// Version range, default `*` (reference-dependency-resolution.md).
    #[serde(default = "any_version")]
    pub version: String,
    /// Whether the dependency never blocks activation, default `false` (reference-dependency-resolution.md).
    #[serde(default)]
    pub optional: bool,
}

/// The range that matches every version (reference-dependency-resolution.md).
pub const ANY_VERSION: &str = "*";

fn any_version() -> String {
    ANY_VERSION.to_owned()
}

/// Every key of reference-manifest.md "Contributes keys" and "New contributes keys", typed as
/// `serde_json::Value` per key in the skeleton; the implementation replaces each with a typed record.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Contributes(pub std::collections::BTreeMap<String, serde_json::Value>);

/// The limits ceiling class (reference-manifest.md PKG-033).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LimitsTier {
    /// Third-party ceilings (reference-manifest.md PKG-033).
    #[default]
    Standard,
    /// T1 ceilings (reference-manifest.md PKG-033).
    FirstParty,
}

/// One inventory entry of a packed package (reference-package-format.md "The inventory").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    /// Path inside the package (reference-package-format.md).
    pub path: String,
    /// Length in bytes (reference-package-format.md).
    pub length: u64,
    /// Lower-case hex sha256 (reference-package-format.md).
    pub sha256: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{Code, Severity};
    use crate::platform::Platform;
    use crate::side::Side;
    use crate::tier::Tier;
    use serde_json::{json, Value};

    fn context(mode: CheckMode) -> ValidationContext {
        ValidationContext {
            tier: Tier::T2,
            mode,
            platform: Platform::Linux,
            installed: Vec::new(),
            core_plugins: Vec::new(),
            unstable_features: Vec::new(),
        }
    }

    fn base_manifest() -> Value {
        json!({
            "api": "ilium:plugin@0.1.0",
            "id": "example.demo",
            "name": "Demo",
            "version": "1.0.0",
            "publisher": "Example Labs",
            "client": { "entry": "client.wasm", "world": "ilium:client/base@0.1.0" }
        })
    }

    fn with_key(mut value: Value, key: &str, new_value: Value) -> Value {
        value[key] = new_value;
        value
    }

    fn check_value(value: &Value) -> Vec<Diagnostic> {
        validate_json(&value.to_string(), &context(CheckMode::Check))
    }

    fn check_text(text: &str) -> Vec<Diagnostic> {
        validate_json(text, &context(CheckMode::Check))
    }

    fn codes(diagnostics: &[Diagnostic]) -> Vec<&str> {
        diagnostics.iter().map(|diagnostic| diagnostic.code.as_str()).collect()
    }

    fn has_code(diagnostics: &[Diagnostic], expected: &str) -> bool {
        diagnostics.iter().any(|diagnostic| diagnostic.code.as_str() == expected)
    }

    fn has_code_at(diagnostics: &[Diagnostic], expected: &str, pointer: &str) -> bool {
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code.as_str() == expected && diagnostic.pointer == pointer)
    }

    // ----- Parsing a manifest and the top-level document rules -----

    /// REQ: PKG-024
    #[test]
    fn valid_minimal_manifest_has_no_diagnostics() {
        let diagnostics = check_value(&base_manifest());
        assert!(diagnostics.is_empty(), "expected no diagnostics, got {:?}", codes(&diagnostics));
    }

    /// REQ: PKG-032
    #[test]
    fn parse_minimal_manifest_applies_documented_defaults() {
        let manifest = parse_manifest(&base_manifest().to_string()).expect("minimal manifest parses");
        assert_eq!(manifest.data_version, DEFAULT_DATA_VERSION);
        assert_eq!(manifest.data_version, 1);
        assert_eq!(manifest.limits_tier, LimitsTier::Standard);
        assert!(manifest.activation.is_empty(), "absent activation means lazy");
        assert!(!manifest.replaceable);
        assert!(manifest.capabilities.server.is_empty());
    }

    /// REQ: PKG-166
    #[test]
    fn parse_manifest_refuses_empty_text_as_json_invalid() {
        match parse_manifest("") {
            Ok(_) => panic!("an empty manifest must be refused"),
            Err(diagnostics) => assert!(has_code(&diagnostics, "manifest.json-invalid")),
        }
    }

    /// REQ: PKG-002
    #[test]
    fn top_level_array_is_refused_as_schema_error_at_root() {
        let diagnostics = check_text("[]");
        assert!(has_code_at(&diagnostics, "manifest.schema", ""), "got {:?}", codes(&diagnostics));
    }

    /// REQ: PKG-004
    #[test]
    fn trailing_comma_is_refused_as_json_invalid() {
        let diagnostics = check_text("{\"api\": \"ilium:plugin@0.1.0\",}");
        assert_eq!(codes(&diagnostics), vec!["manifest.json-invalid"]);
    }

    /// REQ: PKG-003
    #[test]
    fn duplicate_key_at_nested_depth_stops_validation() {
        let text = "{\"api\":\"ilium:plugin@0.1.0\",\"id\":\"demo\",\"name\":\"Demo\",\"version\":\"1.0.0\",\
            \"publisher\":\"Example Labs\",\"limits\":{\"memory_mib\":1,\"memory_mib\":2},\
            \"client\":{\"entry\":\"client.wasm\",\"world\":\"ilium:client/base@0.1.0\"}}";
        let diagnostics = check_text(text);
        assert_eq!(codes(&diagnostics), vec!["manifest.duplicate-key"]);
    }

    /// REQ: PKG-076
    #[test]
    fn empty_object_reports_each_missing_required_key() {
        let diagnostics = check_text("{}");
        let missing = diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code.as_str() == "manifest.key-missing")
            .count();
        assert_eq!(missing, 5, "api, id, name, version and publisher are required; got {:?}", codes(&diagnostics));
        assert!(has_code(&diagnostics, "manifest.schema"), "a missing component is a schema error");
    }

    /// REQ: PKG-005
    #[test]
    fn unknown_top_level_key_is_refused_with_pointer() {
        let diagnostics = check_value(&with_key(base_manifest(), "bogus", json!(true)));
        assert!(has_code_at(&diagnostics, "manifest.unknown-key", "/bogus"), "got {:?}", codes(&diagnostics));
    }

    /// REQ: PKG-024
    #[test]
    fn manifest_without_any_component_is_refused_as_schema_error() {
        let mut value = base_manifest();
        value.as_object_mut().expect("object").remove("client");
        let diagnostics = check_value(&value);
        assert!(has_code(&diagnostics, "manifest.schema"), "got {:?}", codes(&diagnostics));
    }

    // ----- Identity and display fields -----

    /// REQ: PKG-013
    #[test]
    fn api_contract_string_other_than_exact_text_is_schema_error() {
        let diagnostics = check_value(&with_key(base_manifest(), "api", json!("ilium:plugin@0.1")));
        assert!(has_code_at(&diagnostics, "manifest.schema", "/api"), "got {:?}", codes(&diagnostics));
    }

    /// REQ: PKG-013
    #[test]
    fn well_formed_api_outside_host_window_is_unsupported() {
        let diagnostics = check_value(&with_key(base_manifest(), "api", json!("ilium:plugin@9.0.0")));
        assert!(has_code(&diagnostics, "package.api-unsupported"), "got {:?}", codes(&diagnostics));
    }

    /// REQ: PKG-014
    #[test]
    fn id_with_one_segment_is_invalid() {
        let diagnostics = check_value(&with_key(base_manifest(), "id", json!("demo")));
        assert!(has_code_at(&diagnostics, "manifest.id-invalid", "/id"), "got {:?}", codes(&diagnostics));
    }

    /// REQ: PKG-014
    #[test]
    fn id_with_uppercase_or_empty_or_leading_hyphen_segments_is_invalid() {
        for id in ["Example.Demo", "example..demo", "example.-demo"] {
            let diagnostics = check_value(&with_key(base_manifest(), "id", json!(id)));
            assert!(
                has_code_at(&diagnostics, "manifest.id-invalid", "/id"),
                "id {id:?} should be refused, got {:?}",
                codes(&diagnostics)
            );
        }
    }

    /// REQ: PKG-014
    #[test]
    fn id_boundary_eight_segments_accepted_nine_refused() {
        let eight = check_value(&with_key(base_manifest(), "id", json!("a.b.c.d.e.f.g.h")));
        assert!(!has_code(&eight, "manifest.id-invalid"), "eight segments are the maximum");
        let nine = check_value(&with_key(base_manifest(), "id", json!("a.b.c.d.e.f.g.h.i")));
        assert!(has_code(&nine, "manifest.id-invalid"), "nine segments exceed the maximum");
    }

    /// REQ: PKG-014
    #[test]
    fn id_in_reserved_namespace_is_refused_as_reserved() {
        for id in ["ilium.demo", "core.demo", "local.demo", "test.demo"] {
            let diagnostics = check_value(&with_key(base_manifest(), "id", json!(id)));
            assert!(
                has_code(&diagnostics, "manifest.id-reserved"),
                "id {id:?} should be reserved, got {:?}",
                codes(&diagnostics)
            );
        }
    }

    /// REQ: PKG-016
    #[test]
    fn version_with_leading_v_is_invalid() {
        let diagnostics = check_value(&with_key(base_manifest(), "version", json!("v1.0.0")));
        assert!(has_code_at(&diagnostics, "manifest.version-invalid", "/version"), "got {:?}", codes(&diagnostics));
    }

    /// REQ: PKG-016
    #[test]
    fn version_with_prerelease_and_build_is_valid() {
        let diagnostics = check_value(&with_key(base_manifest(), "version", json!("1.0.0-rc.1+build.5")));
        assert!(!has_code(&diagnostics, "manifest.version-invalid"), "got {:?}", codes(&diagnostics));
    }

    /// REQ: PKG-009
    #[test]
    fn name_with_bidirectional_override_is_display_name_error() {
        let diagnostics = check_value(&with_key(base_manifest(), "name", json!("Demo\u{202E}")));
        assert!(has_code_at(&diagnostics, "manifest.display-name", "/name"), "got {:?}", codes(&diagnostics));
    }

    /// REQ: PKG-015
    #[test]
    fn empty_name_is_refused_as_schema_error() {
        let diagnostics = check_value(&with_key(base_manifest(), "name", json!("")));
        assert!(has_code(&diagnostics, "manifest.schema"), "got {:?}", codes(&diagnostics));
    }

    /// REQ: PKG-017
    #[test]
    fn empty_publisher_is_refused_as_schema_error() {
        let diagnostics = check_value(&with_key(base_manifest(), "publisher", json!("")));
        assert!(has_code(&diagnostics, "manifest.schema"), "got {:?}", codes(&diagnostics));
    }

    /// REQ: PKG-018
    #[test]
    fn description_with_line_break_is_display_name_error() {
        let diagnostics = check_value(&with_key(base_manifest(), "description", json!("line one\nline two")));
        assert!(has_code_at(&diagnostics, "manifest.display-name", "/description"), "got {:?}", codes(&diagnostics));
    }

    /// REQ: PKG-018
    #[test]
    fn description_over_500_characters_is_schema_error() {
        let long = "x".repeat(501);
        let diagnostics = check_value(&with_key(base_manifest(), "description", json!(long)));
        assert!(has_code(&diagnostics, "manifest.schema"), "got {:?}", codes(&diagnostics));
    }

    /// REQ: PKG-019
    #[test]
    fn empty_license_is_refused_as_schema_error() {
        let diagnostics = check_value(&with_key(base_manifest(), "license", json!("")));
        assert!(has_code(&diagnostics, "manifest.schema"), "got {:?}", codes(&diagnostics));
    }

    /// REQ: PKG-020
    #[test]
    fn unknown_icon_name_is_only_a_warning_in_check_mode() {
        let diagnostics = check_value(&with_key(base_manifest(), "icon", json!("Not A Valid Icon")));
        assert!(
            diagnostics.iter().all(|diagnostic| diagnostic.severity != Severity::Error),
            "an icon name never fails the check, got {:?}",
            codes(&diagnostics)
        );
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.pointer == "/icon" && diagnostic.severity == Severity::Warning),
            "expected a warning at /icon"
        );
    }

    /// REQ: PKG-023
    #[test]
    fn plain_http_homepage_is_refused_as_schema_error() {
        let diagnostics = check_value(&with_key(base_manifest(), "homepage", json!("http://example.com")));
        assert!(has_code_at(&diagnostics, "manifest.schema", "/homepage"), "got {:?}", codes(&diagnostics));
    }

    /// REQ: PKG-022
    #[test]
    fn keywords_boundary_eight_accepted_nine_refused() {
        let eight: Vec<String> = (0..8).map(|index| format!("kw{index}")).collect();
        let nine: Vec<String> = (0..9).map(|index| format!("kw{index}")).collect();
        let accepted = check_value(&with_key(base_manifest(), "keywords", json!(eight)));
        assert!(!has_code(&accepted, "manifest.schema"), "got {:?}", codes(&accepted));
        let refused = check_value(&with_key(base_manifest(), "keywords", json!(nine)));
        assert!(has_code(&refused, "manifest.schema"), "got {:?}", codes(&refused));
    }

    /// REQ: PKG-022
    #[test]
    fn keyword_longer_than_24_characters_is_refused() {
        let diagnostics = check_value(&with_key(base_manifest(), "keywords", json!(["k".repeat(25)])));
        assert!(has_code(&diagnostics, "manifest.schema"), "got {:?}", codes(&diagnostics));
    }

    // ----- Tier, replacement and data version -----

    /// REQ: PKG-029
    #[test]
    fn replaceable_true_in_third_party_package_is_refused() {
        let value = with_key(base_manifest(), "replaceable", json!(true));
        let diagnostics = check_value(&with_key(value, "surface", json!(["services:ilium.x/y"])));
        assert!(
            has_code(&diagnostics, "manifest.limits-tier") || has_code(&diagnostics, "manifest.schema"),
            "got {:?}",
            codes(&diagnostics)
        );
    }

    /// REQ: PKG-028
    #[test]
    fn replaces_with_more_than_four_entries_is_schema_error() {
        let replaces = json!(["a.one", "a.two", "a.three", "a.four", "a.five"]);
        let diagnostics = check_value(&with_key(base_manifest(), "replaces", replaces));
        assert!(has_code(&diagnostics, "manifest.schema"), "got {:?}", codes(&diagnostics));
    }

    /// REQ: PKG-033
    #[test]
    fn first_party_limits_tier_on_non_ilium_id_is_refused() {
        let diagnostics = check_value(&with_key(base_manifest(), "limits_tier", json!("first-party")));
        assert!(has_code(&diagnostics, "manifest.limits-tier"), "got {:?}", codes(&diagnostics));
    }

    /// REQ: PKG-033
    #[test]
    fn unknown_limits_tier_value_is_schema_error() {
        let diagnostics = check_value(&with_key(base_manifest(), "limits_tier", json!("gold")));
        assert!(has_code(&diagnostics, "manifest.schema"), "got {:?}", codes(&diagnostics));
    }

    /// REQ: PKG-032
    #[test]
    fn data_version_zero_is_refused() {
        let diagnostics = check_value(&with_key(base_manifest(), "data_version", json!(0)));
        assert!(has_code_at(&diagnostics, "manifest.data-version", "/data_version"), "got {:?}", codes(&diagnostics));
    }

    /// REQ: PKG-032
    #[test]
    fn data_version_negative_is_refused() {
        let diagnostics = check_value(&with_key(base_manifest(), "data_version", json!(-1)));
        assert!(has_code(&diagnostics, "manifest.data-version"), "got {:?}", codes(&diagnostics));
    }

    /// REQ: PKG-032
    #[test]
    fn data_version_range_boundaries_are_one_and_u32_max() {
        let minimum = check_value(&with_key(base_manifest(), "data_version", json!(1)));
        assert!(!has_code(&minimum, "manifest.data-version"), "1 is the minimum");
        let maximum = check_value(&with_key(base_manifest(), "data_version", json!(4_294_967_295u64)));
        assert!(!has_code(&maximum, "manifest.data-version"), "4,294,967,295 is the maximum");
        let above = check_value(&with_key(base_manifest(), "data_version", json!(4_294_967_296u64)));
        assert!(has_code(&above, "manifest.data-version"), "above the maximum is refused");
    }

    /// REQ: PKG-034
    #[test]
    fn unstable_name_not_a_host_feature_is_unknown() {
        let value = with_key(base_manifest(), "unstable", json!(["ui-calendar"]));
        let diagnostics = validate_json(&value.to_string(), &context(CheckMode::Dev));
        assert!(has_code(&diagnostics, "manifest.unstable-unknown"), "got {:?}", codes(&diagnostics));
    }

    /// REQ: PKG-034
    #[test]
    fn unstable_feature_needs_development_mode_for_third_party_package() {
        let value = with_key(base_manifest(), "unstable", json!(["ui-calendar"]));
        let mut check = context(CheckMode::Check);
        check.unstable_features = vec!["ui-calendar".to_owned()];
        let in_check = validate_json(&value.to_string(), &check);
        assert!(has_code(&in_check, "manifest.unstable-tier"), "got {:?}", codes(&in_check));

        let mut dev = context(CheckMode::Dev);
        dev.unstable_features = vec!["ui-calendar".to_owned()];
        let in_dev = validate_json(&value.to_string(), &dev);
        assert!(!has_code(&in_dev, "manifest.unstable-tier"), "development mode allows it, got {:?}", codes(&in_dev));
    }

    // ----- Activation triggers -----

    /// REQ: PKG-059
    #[test]
    fn activation_thirty_two_triggers_accepted_thirty_three_refused() {
        let thirty_two: Vec<String> = (0..32).map(|index| format!("event:e{index}")).collect();
        let thirty_three: Vec<String> = (0..33).map(|index| format!("event:e{index}")).collect();
        let accepted = check_value(&with_key(base_manifest(), "activation", json!(thirty_two)));
        assert!(!has_code(&accepted, "manifest.schema"), "got {:?}", codes(&accepted));
        let refused = check_value(&with_key(base_manifest(), "activation", json!(thirty_three)));
        assert!(has_code(&refused, "manifest.schema"), "got {:?}", codes(&refused));
    }

    /// REQ: PKG-060
    #[test]
    fn activation_naming_missing_own_command_is_contribution_target_error() {
        let value = with_key(base_manifest(), "activation", json!(["command:example.demo/show"]));
        let diagnostics = check_value(&value);
        assert!(has_code(&diagnostics, "manifest.contribution-target"), "got {:?}", codes(&diagnostics));
    }

    /// REQ: PKG-059
    #[test]
    fn activation_with_unknown_kind_is_schema_error() {
        let diagnostics = check_value(&with_key(base_manifest(), "activation", json!(["bogus:x"])));
        assert!(has_code(&diagnostics, "manifest.schema"), "got {:?}", codes(&diagnostics));
    }

    /// REQ: PKG-059
    #[test]
    fn activation_with_name_containing_space_is_schema_error() {
        let diagnostics = check_value(&with_key(base_manifest(), "activation", json!(["event:demo tick"])));
        assert!(has_code(&diagnostics, "manifest.schema"), "got {:?}", codes(&diagnostics));
    }

    /// REQ: PKG-059
    #[test]
    fn parse_activation_no_name_triggers() {
        assert_eq!(parse_activation("startup"), Ok(ActivationTrigger::Startup));
        assert_eq!(parse_activation("durable-timer"), Ok(ActivationTrigger::DurableTimer));
    }

    /// REQ: PKG-059
    #[test]
    fn parse_activation_named_kinds_map_to_their_variant() {
        assert_eq!(parse_activation("event:demo.tick"), Ok(ActivationTrigger::Event("demo.tick".to_owned())));
        assert_eq!(
            parse_activation("command:example.demo/show"),
            Ok(ActivationTrigger::Command("example.demo/show".to_owned()))
        );
        assert_eq!(parse_activation("view:main"), Ok(ActivationTrigger::View("main".to_owned())));
        assert_eq!(parse_activation("pane-kind:notes"), Ok(ActivationTrigger::PaneKind("notes".to_owned())));
        assert_eq!(parse_activation("scene:aurora"), Ok(ActivationTrigger::Scene("aurora".to_owned())));
        assert_eq!(parse_activation("cli:hello"), Ok(ActivationTrigger::Cli("hello".to_owned())));
        assert_eq!(parse_activation("http:/api/ping"), Ok(ActivationTrigger::Http("/api/ping".to_owned())));
        assert_eq!(
            parse_activation("service:ilium:inference/provider"),
            Ok(ActivationTrigger::Service("ilium:inference/provider".to_owned()))
        );
        assert_eq!(parse_activation("topic:tree.changed"), Ok(ActivationTrigger::Topic("tree.changed".to_owned())));
        assert_eq!(parse_activation("settings:general"), Ok(ActivationTrigger::Settings("general".to_owned())));
        assert_eq!(parse_activation("catalog:tools"), Ok(ActivationTrigger::Catalog("tools".to_owned())));
        assert_eq!(parse_activation("menu:context"), Ok(ActivationTrigger::Menu("context".to_owned())));
        assert_eq!(parse_activation("virtual-rows:files"), Ok(ActivationTrigger::VirtualRows("files".to_owned())));
        assert_eq!(parse_activation("language:rust"), Ok(ActivationTrigger::Language("rust".to_owned())));
        assert_eq!(
            parse_activation("client-request:ping"),
            Ok(ActivationTrigger::ClientRequest("ping".to_owned()))
        );
    }

    /// REQ: PKG-059
    #[test]
    fn parse_activation_covers_every_documented_kind() {
        assert_eq!(ACTIVATION_KINDS.len(), 15);
        for kind in ACTIVATION_KINDS {
            let trigger = format!("{kind}:name");
            assert!(parse_activation(&trigger).is_ok(), "documented kind {kind} must parse");
        }
    }

    /// REQ: PKG-059
    #[test]
    fn parse_activation_refuses_unknown_kind_and_malformed_forms() {
        for trigger in ["bogus:x", "command", "startup:x", "durable-timer:x", "command:demo show", ""] {
            let refusal = parse_activation(trigger);
            assert!(
                matches!(&refusal, Err(diagnostic) if diagnostic.code.as_str() == "manifest.schema"),
                "trigger {trigger:?} must be refused with manifest.schema, got {refusal:?}"
            );
        }
    }

    // ----- when expressions -----

    /// REQ: PKG-567
    #[test]
    fn parse_when_accepts_comparisons_with_boolean_operators() {
        assert!(parse_when("pane.primitive == \"terminal\"").is_ok());
        assert!(parse_when("pane.primitive == \"terminal\" && !(pane.primitive == \"none\")").is_ok());
        assert!(parse_when("pane.primitive != \"agent\" || pane.primitive == \"editor\"").is_ok());
    }

    /// REQ: PKG-567
    #[test]
    fn parse_when_unknown_key_is_when_key_error() {
        let refusal = parse_when("pane.colour == \"red\"");
        assert!(
            matches!(&refusal, Err(diagnostic) if diagnostic.code.as_str() == "manifest.when-key"),
            "got {refusal:?}"
        );
    }

    /// REQ: PKG-567
    #[test]
    fn parse_when_unknown_value_is_when_syntax_error() {
        let refusal = parse_when("pane.primitive == \"bogus\"");
        assert!(
            matches!(&refusal, Err(diagnostic) if diagnostic.code.as_str() == "manifest.when-syntax"),
            "got {refusal:?}"
        );
    }

    /// REQ: PKG-567
    #[test]
    fn parse_when_bad_syntax_is_when_syntax_error() {
        for expression in [
            "pane.primitive ==",
            "pane.primitive == \"terminal\" &&",
            "(pane.primitive == \"terminal\"",
            "pane.primitive",
        ] {
            let refusal = parse_when(expression);
            assert!(
                matches!(&refusal, Err(diagnostic) if diagnostic.code.as_str() == "manifest.when-syntax"),
                "expression {expression:?} must be refused with manifest.when-syntax, got {refusal:?}"
            );
        }
    }

    /// REQ: PKG-567
    #[test]
    fn manifest_when_expression_with_unknown_key_is_reported_in_commands() {
        let value = with_key(
            base_manifest(),
            "contributes",
            json!({ "commands": [{ "id": "show", "title": "Show", "when": "pane.colour == \"red\"" }] }),
        );
        let diagnostics = check_value(&value);
        assert!(has_code(&diagnostics, "manifest.when-key"), "got {:?}", codes(&diagnostics));
    }

    // ----- Keybinding chords -----
    // The manual's "Keybinding chords" section has no requirement id; these tests cite none.

    /// REQ: none
    #[test]
    fn parse_chord_splits_modifiers_from_key_in_written_order() {
        assert_eq!(
            parse_chord("ctrl+shift+p"),
            Ok(Chord(vec!["ctrl".to_owned(), "shift".to_owned(), "p".to_owned()]))
        );
        assert_eq!(parse_chord("shift+a"), Ok(Chord(vec!["shift".to_owned(), "a".to_owned()])));
    }

    /// REQ: none
    #[test]
    fn parse_chord_accepts_named_keys_and_function_key_boundaries() {
        for chord in ["enter", "escape", "page-up", "super+f1", "alt+f24", "ctrl+plus", "ctrl+alt+delete"] {
            assert!(parse_chord(chord).is_ok(), "chord {chord:?} is documented and must parse");
        }
    }

    /// REQ: none
    #[test]
    fn parse_chord_refuses_uppercase_empty_and_bare_plus() {
        for chord in ["Ctrl+P", "ctrl+", "+", "ctrl++", "f25", "", "ctrl+shift+"] {
            assert!(parse_chord(chord).is_err(), "chord {chord:?} must be refused");
        }
    }

    // ----- Requires -----

    /// REQ: PKG-327
    #[test]
    fn service_requirement_full_version_splits_name_and_version() {
        let parsed = ServiceRequirement::parse("ilium:inference/provider@0.1.0").expect("full version parses");
        assert_eq!(parsed.name, "ilium:inference/provider");
        assert_eq!(parsed.version, "0.1.0");
    }

    /// REQ: PKG-327
    #[test]
    fn service_requirement_short_or_range_version_is_service_version_error() {
        for text in ["ilium:inference/provider@0.1", "ilium:inference/provider@^0.1.0", "x/y@~1.2.0", "x/y@>=1.2.0"] {
            let refusal = ServiceRequirement::parse(text);
            assert!(
                matches!(&refusal, Err(diagnostic) if diagnostic.code.as_str() == "manifest.service-version"),
                "{text:?} must be refused with manifest.service-version, got {refusal:?}"
            );
        }
    }

    /// REQ: PKG-327
    #[test]
    fn service_requirement_without_version_is_refused() {
        let refusal = ServiceRequirement::parse("com.example.git/status");
        assert!(
            matches!(&refusal, Err(diagnostic) if diagnostic.code.as_str() == "manifest.requires-invalid"),
            "a full version is required, got {refusal:?}"
        );
    }

    /// REQ: PKG-569
    #[test]
    fn service_requirement_empty_text_is_requires_invalid() {
        let refusal = ServiceRequirement::parse("");
        assert!(
            matches!(&refusal, Err(diagnostic) if diagnostic.code.as_str() == "manifest.requires-invalid"),
            "got {refusal:?}"
        );
    }

    /// REQ: PKG-327
    #[test]
    fn manifest_requires_caret_service_range_is_service_version_error() {
        let value = with_key(
            base_manifest(),
            "requires",
            json!({ "services": ["ilium:inference/provider@^0.1.0"] }),
        );
        let diagnostics = check_value(&value);
        assert!(has_code(&diagnostics, "manifest.service-version"), "got {:?}", codes(&diagnostics));
    }

    /// REQ: PKG-569
    #[test]
    fn manifest_requires_bad_service_entry_is_requires_invalid() {
        let value = with_key(base_manifest(), "requires", json!({ "services": ["not a service"] }));
        let diagnostics = check_value(&value);
        assert!(has_code(&diagnostics, "manifest.requires-invalid"), "got {:?}", codes(&diagnostics));
    }

    /// REQ: PKG-569
    #[test]
    fn manifest_requires_duplicate_plugin_is_requires_invalid() {
        let plugins = json!([
            { "id": "com.example.git", "version": "^1.2" },
            { "id": "com.example.git", "version": "^1.3" }
        ]);
        let diagnostics = check_value(&with_key(base_manifest(), "requires", json!({ "plugins": plugins })));
        assert!(has_code(&diagnostics, "manifest.requires-invalid"), "got {:?}", codes(&diagnostics));
    }

    /// REQ: PKG-569
    #[test]
    fn manifest_requires_itself_is_requires_self() {
        let plugins = json!([{ "id": "example.demo" }]);
        let diagnostics = check_value(&with_key(base_manifest(), "requires", json!({ "plugins": plugins })));
        assert!(has_code(&diagnostics, "manifest.requires-self"), "got {:?}", codes(&diagnostics));
    }

    /// REQ: PKG-326
    #[test]
    fn manifest_requires_wait_ms_above_five_thousand_is_refused() {
        let value = with_key(base_manifest(), "requires", json!({ "wait_ms": 5001 }));
        let diagnostics = check_value(&value);
        assert!(has_code(&diagnostics, "manifest.requires-invalid"), "got {:?}", codes(&diagnostics));
    }

    // ----- Capabilities and contributes -----

    /// REQ: CAP-004
    #[test]
    fn grant_without_argument_given_one_is_grant_argument_error() {
        let value = with_key(base_manifest(), "capabilities", json!({ "client": ["tree.read:x"] }));
        let diagnostics = check_value(&value);
        assert!(has_code(&diagnostics, "manifest.grant-argument"), "got {:?}", codes(&diagnostics));
    }

    /// REQ: CAP-004
    #[test]
    fn grant_needing_argument_without_one_is_grant_argument_missing() {
        let value = with_key(base_manifest(), "capabilities", json!({ "client": ["http"] }));
        let diagnostics = check_value(&value);
        assert!(has_code(&diagnostics, "manifest.grant-argument-missing"), "got {:?}", codes(&diagnostics));
    }

    /// REQ: PKG-051
    #[test]
    fn misspelt_grant_is_grant_unknown_with_list_pointer() {
        let value = with_key(base_manifest(), "capabilities", json!({ "client": ["tree.raed"] }));
        let diagnostics = check_value(&value);
        assert!(
            has_code_at(&diagnostics, "manifest.grant-unknown", "/capabilities/client/0"),
            "got {:?}",
            codes(&diagnostics)
        );
    }

    /// REQ: PKG-051
    #[test]
    fn known_grant_on_its_side_is_accepted() {
        let value = with_key(base_manifest(), "capabilities", json!({ "client": ["tree.read"] }));
        let diagnostics = check_value(&value);
        assert!(!has_code(&diagnostics, "manifest.grant-unknown"), "got {:?}", codes(&diagnostics));
    }

    /// REQ: PKG-025
    #[test]
    fn grant_list_for_side_without_component_is_grant_side_error() {
        let value = with_key(base_manifest(), "capabilities", json!({ "server": ["tree.read"] }));
        let diagnostics = check_value(&value);
        assert!(has_code(&diagnostics, "manifest.grant-side"), "got {:?}", codes(&diagnostics));
    }

    /// REQ: PKG-030
    #[test]
    fn unknown_contributes_key_is_unknown_key() {
        let diagnostics = check_value(&with_key(base_manifest(), "contributes", json!({ "widgets": [] })));
        assert!(has_code(&diagnostics, "manifest.unknown-key"), "got {:?}", codes(&diagnostics));
    }

    /// REQ: PKG-064
    #[test]
    fn scenes_contribution_without_scene_component_is_contribution_target_error() {
        let diagnostics = check_value(&with_key(base_manifest(), "contributes", json!({ "scenes": [] })));
        assert!(has_code(&diagnostics, "manifest.contribution-target"), "got {:?}", codes(&diagnostics));
    }

    /// REQ: PKG-072
    #[test]
    fn unknown_limits_key_is_unknown_key() {
        let diagnostics = check_value(&with_key(base_manifest(), "limits", json!({ "memory_mibs": 4 })));
        assert!(has_code(&diagnostics, "manifest.unknown-key"), "got {:?}", codes(&diagnostics));
    }

    /// REQ: PKG-070
    #[test]
    fn negative_limit_value_is_schema_error() {
        let diagnostics = check_value(&with_key(base_manifest(), "limits", json!({ "memory_mib": -1 })));
        assert!(has_code(&diagnostics, "manifest.schema"), "got {:?}", codes(&diagnostics));
    }

    /// REQ: PKG-031
    #[test]
    fn limit_above_tier_ceiling_is_clamped_not_refused() {
        let diagnostics = check_value(&with_key(base_manifest(), "limits", json!({ "memory_mib": 4096 })));
        assert!(
            diagnostics.iter().all(|diagnostic| diagnostic.severity != Severity::Error),
            "a request above the ceiling is clamped, got {:?}",
            codes(&diagnostics)
        );
    }

    /// REQ: PKG-071
    #[test]
    fn limit_of_zero_is_accepted_as_default() {
        let diagnostics = check_value(&with_key(base_manifest(), "limits", json!({ "memory_mib": 0 })));
        assert!(!has_code(&diagnostics, "manifest.schema"), "got {:?}", codes(&diagnostics));
    }

    // ----- Components and exports -----

    /// REQ: PKG-040
    #[test]
    fn server_entry_other_than_server_wasm_is_entry_invalid() {
        let server = json!({ "entry": "server.js", "world": "ilium:server/base@0.1.0" });
        let diagnostics = check_value(&with_key(base_manifest(), "server", server));
        assert!(has_code_at(&diagnostics, "manifest.entry-invalid", "/server/entry"), "got {:?}", codes(&diagnostics));
    }

    /// REQ: PKG-041
    #[test]
    fn component_world_mismatch_is_schema_error() {
        let client = json!({ "entry": "client.wasm", "world": "ilium:server/base@0.1.0" });
        let diagnostics = check_value(&with_key(base_manifest(), "client", client));
        assert!(has_code(&diagnostics, "manifest.schema"), "got {:?}", codes(&diagnostics));
    }

    /// REQ: PKG-042
    #[test]
    fn listing_required_export_is_schema_error() {
        let client = json!({ "entry": "client.wasm", "world": "ilium:client/base@0.1.0", "exports": ["lifecycle"] });
        let diagnostics = check_value(&with_key(base_manifest(), "client", client));
        assert!(has_code(&diagnostics, "manifest.schema"), "got {:?}", codes(&diagnostics));
    }

    fn client_summary(exports: &[&str]) -> ComponentSummary {
        ComponentSummary {
            imports: Vec::new(),
            exports: exports.iter().map(|name| (*name).to_owned()).collect(),
            api_sections: vec![vec![0, 0, 0, 1, 0, 0]],
            unstable_imports: Vec::new(),
        }
    }

    fn manifest_with_client_exports(exports: &[&str]) -> Manifest {
        let client = json!({
            "entry": "client.wasm",
            "world": "ilium:client/base@0.1.0",
            "exports": exports
        });
        parse_manifest(&with_key(base_manifest(), "client", client).to_string())
            .expect("manifest with optional exports parses")
    }

    /// REQ: PKG-043
    #[test]
    fn component_export_missing_from_manifest_is_exports_mismatch() {
        let manifest = manifest_with_client_exports(&[]);
        let diagnostics = check_component(
            &manifest,
            Side::Client,
            &client_summary(&["detector"]),
            &context(CheckMode::Install),
        );
        assert!(has_code(&diagnostics, "manifest.exports-mismatch"), "got {:?}", codes(&diagnostics));
    }

    /// REQ: PKG-043
    #[test]
    fn manifest_export_missing_from_component_is_exports_mismatch() {
        let manifest = manifest_with_client_exports(&["detector"]);
        let diagnostics = check_component(&manifest, Side::Client, &client_summary(&[]), &context(CheckMode::Install));
        assert!(has_code(&diagnostics, "manifest.exports-mismatch"), "got {:?}", codes(&diagnostics));
    }

    /// REQ: PKG-043
    #[test]
    fn matching_exports_and_api_section_produce_no_exports_mismatch() {
        let manifest = manifest_with_client_exports(&["detector"]);
        let diagnostics = check_component(
            &manifest,
            Side::Client,
            &client_summary(&["detector"]),
            &context(CheckMode::Install),
        );
        assert!(!has_code(&diagnostics, "manifest.exports-mismatch"), "got {:?}", codes(&diagnostics));
    }

    /// REQ: PKG-140
    #[test]
    fn api_section_for_other_version_is_api_version_error() {
        let manifest = manifest_with_client_exports(&[]);
        let mut summary = client_summary(&[]);
        summary.api_sections = vec![vec![0, 0, 0, 2, 0, 0]];
        let diagnostics = check_component(&manifest, Side::Client, &summary, &context(CheckMode::Install));
        assert!(has_code(&diagnostics, "manifest.api-version"), "got {:?}", codes(&diagnostics));
    }

    /// REQ: PKG-138
    #[test]
    fn component_without_api_section_is_api_section_missing() {
        let manifest = manifest_with_client_exports(&[]);
        let mut summary = client_summary(&[]);
        summary.api_sections = Vec::new();
        let diagnostics = check_component(&manifest, Side::Client, &summary, &context(CheckMode::Install));
        assert!(has_code(&diagnostics, "package.api-section-missing"), "got {:?}", codes(&diagnostics));
    }

    // ----- Error collection, ordering and stage rules -----

    /// REQ: PKG-075
    #[test]
    fn two_defects_in_one_stage_both_reported() {
        let value = with_key(with_key(base_manifest(), "id", json!("demo")), "version", json!("v1.0.0"));
        let diagnostics = check_value(&value);
        assert!(has_code(&diagnostics, "manifest.id-invalid"), "got {:?}", codes(&diagnostics));
        assert!(has_code(&diagnostics, "manifest.version-invalid"), "got {:?}", codes(&diagnostics));
    }

    /// REQ: PKG-075
    #[test]
    fn defect_in_earlier_stage_hides_later_stage_defects() {
        let text = "{\"api\":\"ilium:plugin@0.1.0\",\"id\":\"demo\",\"name\":\"A\",\"name\":\"B\",\
            \"version\":\"1.0.0\",\"publisher\":\"Example Labs\",\
            \"client\":{\"entry\":\"client.wasm\",\"world\":\"ilium:client/base@0.1.0\"}}";
        let diagnostics = check_text(text);
        assert_eq!(codes(&diagnostics), vec!["manifest.duplicate-key"]);
    }

    /// REQ: PKG-075
    #[test]
    fn error_collection_stops_at_one_hundred_findings() {
        let mut value = base_manifest();
        for index in 0..120 {
            value[format!("bogus{index}")] = json!(true);
        }
        let diagnostics = check_value(&value);
        assert_eq!(diagnostics.len(), 100, "errors are collected up to 100");
    }

    /// REQ: PKG-077
    #[test]
    fn validate_struct_reports_same_codes_as_validate_json() {
        let text = with_key(base_manifest(), "id", json!("demo")).to_string();
        let context = context(CheckMode::Check);
        let manifest = parse_manifest(&text).expect("syntactically valid manifest parses");
        assert_eq!(codes(&validate(&manifest, &context)), codes(&validate_json(&text, &context)));
    }

    /// REQ: PKG-076
    #[test]
    fn diagnostic_display_includes_pointer_when_present() {
        let diagnostic = Diagnostic::error(Code::from_static("manifest.schema"), "/id", "bad value");
        assert_eq!(diagnostic.to_string(), "manifest.schema /id: bad value");
    }

    /// REQ: PKG-076
    #[test]
    fn diagnostic_display_omits_pointer_when_empty() {
        let diagnostic = Diagnostic::error(Code::from_static("manifest.schema"), "", "bad value");
        assert_eq!(diagnostic.to_string(), "manifest.schema: bad value");
    }

    /// REQ: PKG-560
    #[test]
    fn json_schema_declares_draft_2020_12_and_contract_id() {
        let schema: Value = serde_json::from_str(json_schema()).expect("embedded schema is JSON");
        assert_eq!(schema["$schema"], "https://json-schema.org/draft/2020-12/schema");
        assert_eq!(schema["$id"], "urn:ilium:schema:plugin-manifest:0.1.0");
    }
}
