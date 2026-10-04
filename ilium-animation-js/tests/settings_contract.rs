use ilium_animation_js::settings::validate_settings;
use serde_json::json;

#[test]
fn defaults_are_inserted_without_losing_authored_values() {
    let schema = json!({"type":"object","additionalProperties":false,"properties":{
        "speed":{"type":"integer","minimum":0,"maximum":200,"default":100},
        "style":{"type":"string","enum":["classic","rich"],"default":"rich"}
    }});
    assert_eq!(
        validate_settings(&schema, &json!({"speed":75})).unwrap(),
        json!({"speed":75,"style":"rich"})
    );
}
#[test]
fn unknown_or_out_of_range_settings_fail_before_js_execution() {
    let schema = json!({"type":"object","additionalProperties":false,"properties":{
        "seed":{"type":"integer","minimum":0,"maximum":100,"default":1}
    }});
    for value in [
        json!({"seed":101}),
        json!({"seed":0.5}),
        json!({"seed":"2"}),
        json!({"seed":2,"typo":3}),
    ] {
        assert!(validate_settings(&schema, &value).is_err());
    }
}
#[test]
fn invalid_manifest_defaults_and_unsupported_schemas_are_rejected() {
    for schema in [
        json!({"type":"object","properties":{"value":{"type":"integer","minimum":0,"default":-1}}}),
        json!({"type":"array"}),
        json!({"type":"object","properties":{"value":{"type":"object"}}}),
    ] {
        assert!(validate_settings(&schema, &json!({})).is_err());
    }
}
#[test]
fn missing_required_and_oversized_strings_fail() {
    let schema = json!({"type":"object","required":["name"],"properties":{"name":{"type":"string","maxLength":4}}});
    assert!(validate_settings(&schema, &json!({})).is_err());
    assert!(validate_settings(&schema, &json!({"name":"abcde"})).is_err());
    assert!(validate_settings(&schema, &json!({"name":"été"})).is_ok());
}

#[test]
fn catalogue_can_validate_schema_without_authorizing_execution_or_values() {
    let schema = json!({"type":"object","required":["name"],"properties":{"name":{"type":"string","maxLength":4}}});
    assert!(ilium_animation_js::settings::validate_schema(&schema).is_ok());
    assert!(validate_settings(&schema, &json!({})).is_err());
}
