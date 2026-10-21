//! Manifest parsing and validation entry points (reference-manifest-schema.md,
//! packages.md "Install checks").

use super::{Diagnostic, Manifest};
use crate::platform::Platform;
use crate::tier::Tier;

/// Which command runs the checks (reference-check-and-lint-rules.md DEV-200).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CheckMode {
    /// `ilium plugin check` (reference-check-and-lint-rules.md).
    Check,
    /// The install path (reference-manifest-schema.md).
    Install,
    /// `ilium plugin dev` (packages.md).
    Dev,
}

/// What the validator knows about the machine (reference-manifest-schema.md).
#[derive(Debug, Clone)]
pub struct ValidationContext {
    /// Tier of the package under check (security.md).
    pub tier: Tier,
    /// Which command runs the checks.
    pub mode: CheckMode,
    /// Host operating system.
    pub platform: Platform,
    /// Installed plugins, for dependency and conflict rules.
    pub installed: Vec<InstalledSummary>,
    /// Core plugins, for `replaces` (reference-manifest.md PKG-028).
    pub core_plugins: Vec<CorePluginSummary>,
    /// Unstable features this host accepts (reference-abi-evolution.md, ABI-022).
    pub unstable_features: Vec<String>,
}

/// One installed plugin as the validator sees it (reference-dependency-resolution.md).
#[derive(Debug, Clone)]
pub struct InstalledSummary {
    /// Plugin id.
    pub id: String,
    /// Installed version.
    pub version: String,
    /// Services it provides: (name, version).
    pub services: Vec<(String, String)>,
    /// Settings tabs it owns.
    pub settings_owner_tabs: Vec<String>,
}

/// One core plugin as the validator sees it (reference-manifest.md PKG-028, PKG-029).
#[derive(Debug, Clone)]
pub struct CorePluginSummary {
    /// Core plugin id.
    pub id: String,
    /// Whether a third party may replace it.
    pub replaceable: bool,
    /// Its replaceable surface.
    pub surface: Vec<String>,
}

/// Stage 2: parses `manifest.json` text, stopping at the first error (reference-manifest-schema.md).
pub fn parse_manifest(_json: &str) -> Result<Manifest, Vec<Diagnostic>> {
    unimplemented!("api::manifest::parse_manifest")
}

/// Stage 3 and later: every rule of the manifest pages, all errors collected
/// (reference-manifest-schema.md).
pub fn validate(_manifest: &Manifest, _context: &ValidationContext) -> Vec<Diagnostic> {
    unimplemented!("api::manifest::validate")
}

/// Parse plus validate, in install order, one refusal code per package for the API codes
/// (reference-abi-evolution.md, ABI-021).
pub fn validate_json(_json: &str, _context: &ValidationContext) -> Vec<Diagnostic> {
    unimplemented!("api::manifest::validate_json")
}

/// The JSON Schema of reference-manifest-schema.md, embedded as text.
pub fn json_schema() -> &'static str {
    unimplemented!("api::manifest::json_schema")
}

#[cfg(test)]
mod tests {
    use super::{json_schema, parse_manifest, validate, validate_json, CheckMode, ValidationContext};
    use crate::error::Severity;
    use crate::manifest::{Diagnostic, Manifest, Stage};
    use crate::platform::Platform;
    use crate::tier::Tier;
    use serde_json::{json, Value};

    /// The four unstable features of version 0.1.0 (reference-abi-evolution.md, ABI-022).
    const UNSTABLE_FEATURES: [&str; 4] = ["ui-calendar", "plugin-services", "gpu-kernels", "websocket"];

    fn context(tier: Tier, mode: CheckMode) -> ValidationContext {
        ValidationContext {
            tier,
            mode,
            platform: Platform::Linux,
            installed: Vec::new(),
            core_plugins: Vec::new(),
            unstable_features: UNSTABLE_FEATURES.iter().map(|name| (*name).to_owned()).collect(),
        }
    }

    /// `ilium plugin check` runs the rules of a T3 source package.
    fn check_context() -> ValidationContext {
        context(Tier::T3, CheckMode::Check)
    }

    /// A minimal valid source manifest with a server component (reference-manifest.md).
    fn valid_server_manifest() -> Value {
        json!({
            "api": "ilium:plugin@0.1.0",
            "id": "example.demo",
            "name": "Example demo",
            "version": "0.1.0",
            "publisher": "Example Publisher",
            "server": { "entry": "server.wasm", "world": "ilium:server/base@0.1.0" }
        })
    }

    fn with_key(mut value: Value, key: &str, new_value: Value) -> Value {
        value[key] = new_value;
        value
    }

    fn check_text(text: &str) -> Vec<Diagnostic> {
        validate_json(text, &check_context())
    }

    fn check_value(value: &Value) -> Vec<Diagnostic> {
        check_text(&value.to_string())
    }

    fn codes(found: &[Diagnostic]) -> Vec<String> {
        found.iter().map(|diagnostic| diagnostic.code.as_str().to_owned()).collect()
    }

    fn assert_has_code(found: &[Diagnostic], expected_code: &str) {
        let found_codes = codes(found);
        assert!(
            found_codes.iter().any(|code| code == expected_code),
            "expected code {expected_code}, got {found_codes:?}"
        );
    }

    fn assert_lacks_code(found: &[Diagnostic], unexpected_code: &str) {
        let found_codes = codes(found);
        assert!(
            !found_codes.iter().any(|code| code == unexpected_code),
            "code {unexpected_code} must not be reported, got {found_codes:?}"
        );
    }

    fn assert_no_errors(found: &[Diagnostic]) {
        assert!(found.is_empty(), "a valid manifest must have no diagnostics, got {:?}", codes(found));
    }

    fn decode(value: &Value) -> Manifest {
        serde_json::from_value(value.clone()).expect("a manifest with all required keys must deserialize")
    }

    /// REQ: PKG-561
    #[test]
    fn valid_server_manifest_has_no_diagnostics() {
        assert_no_errors(&check_value(&valid_server_manifest()));
    }

    /// REQ: PKG-561
    #[test]
    fn valid_manifest_passes_validate_on_the_decoded_struct() {
        let manifest = decode(&valid_server_manifest());
        assert_no_errors(&validate(&manifest, &check_context()));
    }

    /// REQ: PKG-561
    #[test]
    fn parse_manifest_accepts_well_formed_json() {
        let manifest = parse_manifest(&valid_server_manifest().to_string())
            .expect("well-formed JSON must parse at stage 2");
        assert_eq!(manifest.id, "example.demo");
        assert_eq!(manifest.version, "0.1.0");
    }

    /// REQ: PKG-576
    #[test]
    fn parse_manifest_empty_input_is_json_invalid_at_stage_two() {
        let found = parse_manifest("").expect_err("empty input is not JSON");
        assert_eq!(found.len(), 1, "stage 2 stops at the first error");
        assert_has_code(&found, "manifest.json-invalid");
        assert_eq!(found[0].stage, Some(Stage(2)));
    }

    /// REQ: PKG-576
    #[test]
    fn validate_json_empty_input_is_json_invalid() {
        let found = check_text("");
        assert_has_code(&found, "manifest.json-invalid");
    }

    /// REQ: PKG-576
    #[test]
    fn parse_manifest_rejects_byte_order_mark_as_json_invalid() {
        let text = format!("\u{feff}{}", valid_server_manifest());
        let found = parse_manifest(&text).expect_err("a BOM at the start of the file is refused");
        assert_has_code(&found, "manifest.json-invalid");
    }

    /// REQ: PKG-576
    #[test]
    fn parse_manifest_repeated_key_is_duplicate_key_at_stage_two() {
        let text = r#"{"api": "ilium:plugin@0.1.0", "id": "example.demo", "id": "example.other"}"#;
        let found = parse_manifest(text).expect_err("a repeated key is refused at stage 2");
        assert_eq!(found.len(), 1, "no later stage runs after a duplicate key");
        assert_has_code(&found, "manifest.duplicate-key");
        assert_eq!(found[0].stage, Some(Stage(2)));
    }

    /// REQ: PKG-562
    #[test]
    fn empty_object_reports_each_missing_required_key() {
        let found = check_text("{}");
        for key in ["api", "id", "name", "version", "publisher"] {
            let expected_message_part = format!("\"{key}\"");
            assert!(
                found.iter().any(|diagnostic| diagnostic.code.as_str() == "manifest.key-missing"
                    && diagnostic.message.contains(&expected_message_part)),
                "expected manifest.key-missing for {key}, got {:?}",
                codes(&found)
            );
        }
    }

    /// REQ: PKG-562
    #[test]
    fn empty_object_also_reports_missing_component_as_schema_error() {
        let found = check_text("{}");
        assert_has_code(&found, "manifest.schema");
    }

    /// REQ: PKG-562
    #[test]
    fn json_array_root_is_schema_error_at_the_root_pointer() {
        let found = check_text("[]");
        assert!(
            found
                .iter()
                .any(|diagnostic| diagnostic.code.as_str() == "manifest.schema" && diagnostic.pointer.is_empty()),
            "a JSON array must be refused by manifest.schema at pointer \"\", got {:?}",
            codes(&found)
        );
    }

    /// REQ: PKG-562
    #[test]
    fn unknown_top_level_key_is_unknown_key_at_its_pointer() {
        let manifest = with_key(valid_server_manifest(), "bogus", json!(true));
        let found = check_value(&manifest);
        assert!(
            found
                .iter()
                .any(|diagnostic| diagnostic.code.as_str() == "manifest.unknown-key" && diagnostic.pointer == "/bogus"),
            "expected manifest.unknown-key at /bogus, got {:?}",
            codes(&found)
        );
    }

    /// REQ: PKG-561
    #[test]
    fn manifest_without_any_component_is_refused_as_schema_error() {
        let mut manifest = valid_server_manifest();
        manifest.as_object_mut().expect("object").remove("server");
        let found = check_value(&manifest);
        assert_has_code(&found, "manifest.schema");
    }

    /// REQ: PKG-014
    #[test]
    fn id_with_one_segment_is_invalid_with_schema_stage() {
        let found = check_value(&with_key(valid_server_manifest(), "id", json!("demo")));
        assert_has_code(&found, "manifest.id-invalid");
        let invalid = found
            .iter()
            .find(|diagnostic| diagnostic.code.as_str() == "manifest.id-invalid")
            .expect("id-invalid is present");
        assert_eq!(invalid.stage, Some(Stage(3)));
        assert_eq!(invalid.pointer, "/id");
    }

    /// REQ: PKG-014
    #[test]
    fn id_with_uppercase_letters_is_invalid() {
        let found = check_value(&with_key(valid_server_manifest(), "id", json!("Example.Demo")));
        assert_has_code(&found, "manifest.id-invalid");
    }

    /// REQ: PKG-014
    #[test]
    fn id_with_empty_segment_is_invalid() {
        let found = check_value(&with_key(valid_server_manifest(), "id", json!("example..demo")));
        assert_has_code(&found, "manifest.id-invalid");
    }

    /// REQ: PKG-014
    #[test]
    fn id_segment_starting_with_hyphen_is_invalid() {
        let found = check_value(&with_key(valid_server_manifest(), "id", json!("example.-demo")));
        assert_has_code(&found, "manifest.id-invalid");
    }

    /// REQ: PKG-014
    #[test]
    fn id_with_ilium_prefix_is_reserved() {
        let found = check_value(&with_key(valid_server_manifest(), "id", json!("ilium.demo")));
        assert_has_code(&found, "manifest.id-reserved");
    }

    /// REQ: PKG-014
    #[test]
    fn id_with_eight_segments_of_fifteen_characters_is_the_accepted_maximum() {
        let id = vec!["abcdefghijklmno"; 8].join(".");
        assert_eq!(id.len(), 127, "fixture must stay within the 128 character limit");
        let found = check_value(&with_key(valid_server_manifest(), "id", json!(id)));
        assert_lacks_code(&found, "manifest.id-invalid");
    }

    /// REQ: PKG-014
    #[test]
    fn id_with_nine_segments_is_invalid() {
        let id = vec!["a"; 9].join(".");
        let found = check_value(&with_key(valid_server_manifest(), "id", json!(id)));
        assert_has_code(&found, "manifest.id-invalid");
    }

    /// REQ: PKG-014
    #[test]
    fn id_segment_boundary_is_thirty_two_characters() {
        let accepted = format!("example.{}", "a".repeat(32));
        let refused = format!("example.{}", "a".repeat(33));
        assert_lacks_code(
            &check_value(&with_key(valid_server_manifest(), "id", json!(accepted))),
            "manifest.id-invalid",
        );
        assert_has_code(
            &check_value(&with_key(valid_server_manifest(), "id", json!(refused))),
            "manifest.id-invalid",
        );
    }

    /// REQ: PKG-016
    #[test]
    fn version_with_leading_v_is_invalid() {
        let found = check_value(&with_key(valid_server_manifest(), "version", json!("v1.0.0")));
        assert_has_code(&found, "manifest.version-invalid");
    }

    /// REQ: PKG-016
    #[test]
    fn version_with_two_numeric_parts_is_invalid() {
        let found = check_value(&with_key(valid_server_manifest(), "version", json!("1.0")));
        assert_has_code(&found, "manifest.version-invalid");
    }

    /// REQ: PKG-016
    #[test]
    fn version_with_prerelease_and_build_metadata_is_valid() {
        let found = check_value(&with_key(valid_server_manifest(), "version", json!("1.0.0-rc.1+build.5")));
        assert_lacks_code(&found, "manifest.version-invalid");
    }

    /// REQ: PKG-013
    #[test]
    fn api_outside_the_window_is_api_unsupported() {
        let found = check_value(&with_key(valid_server_manifest(), "api", json!("ilium:plugin@9.0.0")));
        assert_has_code(&found, "package.api-unsupported");
    }

    /// REQ: PKG-013
    #[test]
    fn api_that_is_not_well_formed_is_schema_error() {
        let found = check_value(&with_key(valid_server_manifest(), "api", json!("ilium:plugin")));
        assert_has_code(&found, "manifest.schema");
        assert_lacks_code(&found, "package.api-unsupported");
    }

    /// REQ: PKG-009
    #[test]
    fn name_with_control_character_is_display_name_error_at_its_pointer() {
        let found = check_value(&with_key(valid_server_manifest(), "name", json!("Demo\u{7}")));
        let display = found
            .iter()
            .find(|diagnostic| diagnostic.code.as_str() == "manifest.display-name")
            .expect("a control character in name is refused");
        assert_eq!(display.pointer, "/name");
        assert_eq!(display.severity, Severity::Error);
    }

    /// REQ: PKG-009
    #[test]
    fn publisher_with_bidirectional_override_is_display_name_error() {
        let found = check_value(&with_key(valid_server_manifest(), "publisher", json!("Ex\u{202E}ample")));
        assert_has_code(&found, "manifest.display-name");
    }

    /// REQ: PKG-009
    #[test]
    fn description_with_line_separator_is_display_name_error() {
        let mut manifest = valid_server_manifest();
        manifest["description"] = json!("first\u{2028}second");
        let found = check_value(&manifest);
        assert_has_code(&found, "manifest.display-name");
    }

    /// REQ: LIF-100
    #[test]
    fn absent_data_version_defaults_to_one() {
        let manifest = decode(&valid_server_manifest());
        assert_eq!(manifest.data_version, 1);
        assert_lacks_code(&check_value(&valid_server_manifest()), "manifest.data-version");
    }

    /// REQ: LIF-100
    #[test]
    fn data_version_zero_is_refused() {
        let found = check_value(&with_key(valid_server_manifest(), "data_version", json!(0)));
        assert_has_code(&found, "manifest.data-version");
    }

    /// REQ: LIF-100
    #[test]
    fn data_version_as_string_is_refused() {
        let found = check_value(&with_key(valid_server_manifest(), "data_version", json!("2")));
        assert_has_code(&found, "manifest.data-version");
    }

    /// REQ: LIF-100
    #[test]
    fn data_version_at_the_u32_maximum_is_accepted() {
        let found = check_value(&with_key(valid_server_manifest(), "data_version", json!(4_294_967_295_u64)));
        assert_lacks_code(&found, "manifest.data-version");
    }

    /// REQ: LIF-100
    #[test]
    fn data_version_above_the_u32_maximum_is_refused() {
        let found = check_value(&with_key(valid_server_manifest(), "data_version", json!(4_294_967_296_u64)));
        assert_has_code(&found, "manifest.data-version");
    }

    /// REQ: PKG-323
    #[test]
    fn plugin_requiring_itself_is_requires_self_at_semantic_stage() {
        let manifest = with_key(
            valid_server_manifest(),
            "requires",
            json!({ "plugins": [{ "id": "example.demo", "version": "*" }] }),
        );
        let found = check_value(&manifest);
        let requires_self = found
            .iter()
            .find(|diagnostic| diagnostic.code.as_str() == "manifest.requires-self")
            .expect("a plugin that lists its own id is refused");
        assert_eq!(requires_self.stage, Some(Stage(4)));
    }

    /// REQ: PKG-034
    #[test]
    fn unstable_name_not_known_to_host_is_unstable_unknown() {
        let found = check_value(&with_key(valid_server_manifest(), "unstable", json!(["nope"])));
        assert_has_code(&found, "manifest.unstable-unknown");
    }

    /// REQ: PKG-034
    #[test]
    fn unstable_feature_outside_development_mode_is_unstable_tier() {
        let context = context(Tier::T3, CheckMode::Check);
        let manifest = with_key(valid_server_manifest(), "unstable", json!(["websocket"]));
        let found = validate_json(&manifest.to_string(), &context);
        assert_has_code(&found, "manifest.unstable-tier");
    }

    /// REQ: PKG-034
    #[test]
    fn unstable_feature_in_development_mode_is_allowed_for_t3() {
        let context = context(Tier::T3, CheckMode::Dev);
        let manifest = with_key(valid_server_manifest(), "unstable", json!(["websocket"]));
        let found = validate_json(&manifest.to_string(), &context);
        assert_lacks_code(&found, "manifest.unstable-tier");
    }

    /// REQ: PKG-034
    #[test]
    fn unstable_feature_in_bundled_t1_package_is_allowed_outside_development_mode() {
        let context = context(Tier::T1, CheckMode::Check);
        let manifest = with_key(valid_server_manifest(), "unstable", json!(["websocket"]));
        let found = validate_json(&manifest.to_string(), &context);
        assert_lacks_code(&found, "manifest.unstable-tier");
    }

    /// REQ: PKG-562
    #[test]
    fn several_stage_three_failures_are_all_collected() {
        let mut manifest = valid_server_manifest();
        manifest["id"] = json!("demo");
        manifest["version"] = json!("1.0");
        manifest["data_version"] = json!(0);
        let found = check_value(&manifest);
        assert_has_code(&found, "manifest.id-invalid");
        assert_has_code(&found, "manifest.version-invalid");
        assert_has_code(&found, "manifest.data-version");
    }

    /// REQ: PKG-571
    #[test]
    fn every_diagnostic_is_an_error_with_a_stage_and_pointer_where_a_place_exists() {
        let found = check_value(&with_key(valid_server_manifest(), "name", json!("Demo\u{7}")));
        assert!(!found.is_empty());
        for diagnostic in &found {
            assert_eq!(diagnostic.severity, Severity::Error, "{diagnostic}");
            assert!(diagnostic.stage.is_some(), "every catalogue error has a stage: {diagnostic}");
        }
    }

    /// REQ: PKG-560
    #[test]
    fn json_schema_declares_draft_2020_12_and_its_urn_id() {
        let schema: Value = serde_json::from_str(json_schema()).expect("the embedded schema is JSON");
        assert_eq!(schema["$schema"], "https://json-schema.org/draft/2020-12/schema");
        assert_eq!(schema["$id"], "urn:ilium:schema:plugin-manifest:0.1.0");
    }

    /// REQ: PKG-561
    #[test]
    fn json_schema_requires_the_five_identity_keys() {
        let schema: Value = serde_json::from_str(json_schema()).expect("the embedded schema is JSON");
        let required: Vec<&str> = schema["required"]
            .as_array()
            .expect("required is an array")
            .iter()
            .filter_map(Value::as_str)
            .collect();
        for key in ["api", "id", "name", "version", "publisher"] {
            assert!(required.contains(&key), "schema must require {key}, got {required:?}");
        }
    }

    /// REQ: PKG-563
    #[test]
    fn json_schema_is_the_same_embedded_text_on_every_call() {
        assert_eq!(json_schema(), json_schema());
    }
}
