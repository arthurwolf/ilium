//! Pure manifest boundaries; component bytes and package custody belong to the host.
use ilium_plugin_api::manifest::{
    CheckMode, ComponentSummary, ServiceRequirement, ValidationContext, check_component,
    parse_activation, parse_chord, parse_manifest, parse_when, validate_json,
};
use ilium_plugin_api::{Platform, Side, Tier};
use serde_json::{Value, json};

fn context(mode: CheckMode) -> ValidationContext {
    ValidationContext {
        tier: Tier::T3,
        mode,
        platform: Platform::Linux,
        installed: vec![],
        core_plugins: vec![],
        unstable_features: vec![],
    }
}
fn source() -> Value {
    json!({"api":"ilium:plugin@0.1.0","id":"example.boundaries","name":"Boundaries",
        "version":"1.0.0","publisher":"Example",
        "client":{"entry":"client.wasm","world":"ilium:client/base@0.1.0"}})
}
fn codes(value: &Value, mode: CheckMode) -> Vec<String> {
    validate_json(&value.to_string(), &context(mode))
        .iter()
        .map(|error| error.code.as_str().to_owned())
        .collect()
}

#[test]
fn source_and_packed_manifest_have_distinct_inventory_requirements() {
    assert!(codes(&source(), CheckMode::Check).is_empty());
    assert!(codes(&source(), CheckMode::Install).contains(&"manifest.key-missing".into()));
}
#[test]
fn duplicate_members_precede_schema_validation() {
    let errors = validate_json(
        r#"{"name":"a","name":"b","unknown":true}"#,
        &context(CheckMode::Check),
    );
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code.as_str(), "manifest.duplicate-key");
    assert_eq!(errors[0].stage.unwrap().0, 2);
}
#[test]
fn nested_unknown_members_and_lexical_fractional_integers_are_refused() {
    let mut value = source();
    value["client"]["surprise"] = json!(true);
    assert!(!codes(&value, CheckMode::Check).is_empty());
    let text = source()
        .to_string()
        .replace("\"publisher\":", "\"data_version\":1.0,\"publisher\":");
    assert!(parse_manifest(&text).is_err());
}
#[test]
fn schema_errors_are_sorted_by_pointer_bytes() {
    let mut value = source();
    value["z_unknown"] = json!(false);
    value["a_unknown"] = json!(false);
    let errors = validate_json(&value.to_string(), &context(CheckMode::Check));
    assert!(
        errors
            .windows(2)
            .all(|pair| pair[0].pointer.as_bytes() <= pair[1].pointer.as_bytes())
    );
}
#[test]
fn service_minimum_is_a_full_stable_version() {
    assert!(ServiceRequirement::parse("example.service@1.2.3").is_ok());
    for text in [
        "example.service@1",
        "example.service@^1.2.3",
        "example.service@1.2.3-beta",
        "example.service@1.2.3+build",
    ] {
        assert!(ServiceRequirement::parse(text).is_err(), "{text}");
    }
}
#[test]
fn dependency_self_reference_and_ranges_are_checked() {
    let mut value = source();
    value["requires"] = json!({"plugins":[{"id":"example.boundaries","version":"*"}]});
    assert!(codes(&value, CheckMode::Check).contains(&"manifest.requires-self".into()));
    value["requires"] = json!({"host":">=1.0.0 || <2.0.0"});
    assert!(codes(&value, CheckMode::Check).contains(&"manifest.requires-invalid".into()));
}
#[test]
fn data_use_and_activation_wait_survive_parsing() {
    let mut value = source();
    value["data_use"] = json!([{"what":"Name","where":"Local","purpose":"Display","opt_in":true}]);
    value["requires"] = json!({"wait_ms":5000});
    let manifest = parse_manifest(&value.to_string()).unwrap();
    assert_eq!(manifest.requires.unwrap().wait_ms, Some(5000));
    assert_eq!(manifest.data_use[0].where_, "Local");
}
#[test]
fn component_section_validation_precedes_export_comparison() {
    let manifest = parse_manifest(&source().to_string()).unwrap();
    let errors = check_component(
        &manifest,
        Side::Client,
        &ComponentSummary::default(),
        &context(CheckMode::Install),
    );
    assert_eq!(errors[0].code.as_str(), "package.api-section-missing");
}
#[test]
fn scene_world_requires_lifecycle_and_renderer_without_handler() {
    let mut value = source();
    value.as_object_mut().unwrap().remove("client");
    value["scene"] = json!({"entry":"scene.wasm","world":"ilium:anim/base@0.1.0"});
    let manifest = parse_manifest(&value.to_string()).unwrap();
    let summary = ComponentSummary {
        api_sections: vec![vec![0, 0, 0, 1, 0, 0]],
        exports: vec!["lifecycle".into(), "renderer".into()],
        imports: vec!["ilium:anim/surface@0.1.0".into()],
        ..Default::default()
    };
    assert!(
        check_component(
            &manifest,
            Side::Scene,
            &summary,
            &context(CheckMode::Install)
        )
        .is_empty()
    );
}
#[test]
fn small_manifest_languages_reject_ambiguous_or_unknown_forms() {
    assert!(parse_activation("event:tree-changed").is_ok());
    assert!(parse_activation("unknown:x").is_err());
    assert!(parse_chord("ctrl+ctrl+a").is_err());
    assert!(parse_when("unknown_key").is_err());
}
#[test]
fn settings_rows_reject_wrong_fields_and_defaults() {
    let mut value = source();
    value["contributes"] = json!({"settings":[{"kind":"tab","id":"main","title":"Example","sections":[{"id":"main","rows":[{"type":"toggle","key":"enabled","title":"Enabled","default":"yes","minimum":0}]}]}]});
    let errors = codes(&value, CheckMode::Check);
    assert!(errors.contains(&"manifest.settings-default".into()));
    assert!(errors.contains(&"manifest.settings-row-field".into()));
}

#[test]
fn specialized_schema_failures_keep_their_catalogue_code_and_message() {
    let mut value = source();
    value["version"] = json!("v1.2.3");
    let diagnostics = validate_json(&value.to_string(), &context(CheckMode::Check));
    assert_eq!(diagnostics[0].code.as_str(), "manifest.version-invalid");
    assert_eq!(
        diagnostics[0].message,
        "The version \"v1.2.3\" is not a valid semantic version."
    );
    value = source();
    value["data_version"] = json!(0);
    let diagnostics = validate_json(&value.to_string(), &context(CheckMode::Check));
    assert_eq!(
        diagnostics[0].message,
        "data_version must be an integer from 1 to 4294967295. Found 0."
    );
    value = source();
    value["id"] = json!(format!(
        "{}.{}.{}.{}",
        "a".repeat(32),
        "b".repeat(32),
        "c".repeat(32),
        "d".repeat(30)
    ));
    assert_eq!(codes(&value, CheckMode::Check), ["manifest.schema"]);
}

#[test]
fn requires_shape_and_bounds_have_schema_diagnostics() {
    for requires in [
        json!(false),
        json!({"wait_ms":5001}),
        json!({"plugins":"wrong"}),
    ] {
        let mut value = source();
        value["requires"] = requires;
        assert_eq!(codes(&value, CheckMode::Check), ["manifest.schema"]);
    }
}

#[test]
fn kernel_subcommands_cannot_be_plugin_cli_names() {
    for name in ["plugin", "session", "kill-session", "help", "version"] {
        let mut value = source();
        value["contributes"] = json!({"cli":[{"name":name,"summary":"Tool"}]});
        assert!(codes(&value, CheckMode::Check).contains(&"manifest.cli-name-reserved".into()));
    }
}

#[test]
fn extension_point_cross_fields_follow_mode_and_risk() {
    for extra in [
        json!({"on_failure":"pass"}),
        json!({"final_check":true}),
        json!({"reply_schema":{"type":"object"}}),
    ] {
        let mut point = json!({"id":"before","kind":"hook","mode":"observe","payload_schema":{"type":"object"}});
        point
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let mut value = source();
        value["contributes"] = json!({"extension_points":[point]});
        assert!(
            codes(&value, CheckMode::Check).contains(&"manifest.extension-point-invalid".into())
        );
    }
}

#[test]
fn malformed_grants_have_capability_diagnostics_instead_of_generic_schema() {
    let mut value = source();
    value["capabilities"] = json!({"client":["Tree.Read"]});
    let errors = codes(&value, CheckMode::Check);
    assert!(
        errors
            .iter()
            .any(|code| code.starts_with("manifest.grant-"))
    );
    assert!(!errors.contains(&"manifest.schema".into()));
}

#[test]
fn settings_keys_are_unique_across_inline_sections() {
    let mut value = source();
    value["contributes"] = json!({"settings":[{"kind":"tab","id":"main","title":"Example","sections":[
        {"id":"first","rows":[{"type":"toggle","key":"enabled","title":"Enabled","default":true}]},
        {"id":"second","rows":[{"type":"toggle","key":"enabled","title":"Again","default":false}]}
    ]}]});
    assert!(codes(&value, CheckMode::Check).contains(&"manifest.settings-key-clash".into()));
}

#[test]
fn string_length_failures_are_schema_errors_even_for_named_patterns() {
    for (field, text) in [
        ("name", "n".repeat(65)),
        ("publisher", "p".repeat(65)),
        ("description", "d".repeat(501)),
        ("version", format!("1.0.0+{}", "a".repeat(59))),
    ] {
        let mut value = source();
        value[field] = json!(text);
        assert_eq!(
            codes(&value, CheckMode::Check),
            ["manifest.schema"],
            "{field}"
        );
    }
}

#[test]
fn description_allows_empty_text_and_line_feeds() {
    for text in ["", "first\nsecond"] {
        let mut value = source();
        value["description"] = json!(text);
        assert!(codes(&value, CheckMode::Check).is_empty());
    }
    let mut value = source();
    value["description"] = json!("bad\u{0009}tab");
    assert!(codes(&value, CheckMode::Check).contains(&"manifest.display-name".into()));
}

#[test]
fn requires_wrong_type_is_a_schema_failure() {
    let mut value = source();
    value["requires"] = Value::Null;
    assert_eq!(codes(&value, CheckMode::Check), ["manifest.schema"]);
}

#[test]
fn grant_side_validation_precedes_tier_admission() {
    let mut value = source();
    value["capabilities"] = json!({"client":["sessions.cross"]});
    let errors = validate_json(&value.to_string(), &context(CheckMode::Check));
    assert!(
        errors.iter().any(
            |error| error.code.as_str() == "manifest.grant-side" && error.stage.unwrap().0 == 4
        )
    );
}

#[test]
fn command_creation_and_long_lived_processes_require_base_grants() {
    for (grant, prerequisite) in [
        ("panes.create-command:htop", "panes.create"),
        ("process.long-lived", "process.run:tail"),
    ] {
        let mut value = source();
        value["capabilities"] = json!({"client":[grant]});
        assert!(codes(&value, CheckMode::Check).contains(&"manifest.grant-dependency".into()));
        value["capabilities"] = json!({"client":[grant, prerequisite]});
        assert!(codes(&value, CheckMode::Check).is_empty());
    }
}

#[test]
fn published_topics_use_the_owning_plugin_id_and_slash() {
    let mut value = source();
    value["contributes"] = json!({"topics":{"publishes":["other.plugin/ready"]}});
    assert!(codes(&value, CheckMode::Check).contains(&"manifest.contribution-target".into()));
    value["contributes"]["topics"]["publishes"] = json!(["example.boundaries/ready"]);
    assert!(codes(&value, CheckMode::Check).is_empty());
}
