use ilium_plugin_api::grants::Grant;
use ilium_plugin_host::settings::{
    kernel_tab_ids, may_write_kernel_tab, validate_settings, KERNEL_TAB_COUNT,
};
use serde_json::{json, Value};

fn validate(schema: Value, value: Value) -> bool {
    validate_settings(&schema.to_string(), &value.to_string()).is_ok()
}
#[test]
fn frozen_tabs_are_unique_in_rail_order() {
    let tabs = kernel_tab_ids();
    assert_eq!(tabs.len(), KERNEL_TAB_COUNT);
    assert_eq!(
        tabs.iter().collect::<std::collections::BTreeSet<_>>().len(),
        25
    );
    assert_eq!(tabs.first(), Some(&"user-interface"));
    assert_eq!(&tabs[21..], &["debug", "api", "about", "setup"]);
}
#[test]
fn kernel_write_requires_exact_current_grant_and_known_tab() {
    let grants = vec![
        Grant::parse("settings.write-kernel:terminal").unwrap(),
        Grant::parse("settings.read-kernel:editor").unwrap(),
    ];
    assert!(may_write_kernel_tab(&grants, "terminal"));
    assert!(!may_write_kernel_tab(&grants, "editor"));
    assert!(!may_write_kernel_tab(&grants, "not-a-tab"));
    assert!(!may_write_kernel_tab(&[], "terminal"));
}
#[test]
fn complete_values_enforce_types_ranges_required_and_unknown_keys() {
    let schema = json!({"type":"object", "properties": {"enabled":{"type":"boolean"}, "count":{"type":"integer","minimum":0,"maximum":4}}, "required":["enabled"]});
    assert!(validate(schema.clone(), json!({"enabled":true,"count":4})));
    for value in [
        json!({}),
        json!({"enabled":1}),
        json!({"enabled":true,"count":4.5}),
        json!({"enabled":true,"count":5}),
        json!({"enabled":true,"unknown":1}),
        json!({"enabled":null}),
    ] {
        assert!(!validate(schema.clone(), value));
    }
}
#[test]
fn unicode_length_and_pattern_are_real_schema_constraints() {
    let schema = json!({"type":"object","properties":{"name":{"type":"string","maxLength":2,"pattern":"^[éa]+$"}}});
    assert!(validate(schema.clone(), json!({"name":"éé"})));
    assert!(!validate(schema.clone(), json!({"name":"ééé"})));
    assert!(!validate(schema, json!({"name":"ab"})));
}
#[test]
fn nested_lists_validate_every_item_and_report_escaped_pointer() {
    let schema = json!({"type":"object","properties":{"a/b~c":{"type":"array","maxItems":2,"items":{"type":"object","properties":{"n":{"type":"integer"}},"required":["n"]}}}});
    assert!(validate(schema.clone(), json!({"a/b~c":[{"n":1},{"n":2}]})));
    let errors = validate_settings(
        &schema.to_string(),
        &json!({"a/b~c":[{"n":"bad"}]}).to_string(),
    )
    .unwrap_err();
    assert_eq!(errors[0].pointer, "/a~1b~0c/0/n");
    assert!(!validate(
        schema,
        json!({"a/b~c":[{"n":1},{"n":2},{"n":3}]})
    ));
}
#[test]
fn visibility_and_control_annotations_never_exempt_values_from_validation() {
    let schema = json!({"type":"object","properties":{"count":{"type":"integer","minimum":0,"maximum":10,"step":2,"x-visible-when":"false","x-scope":"project","x-applies":"restart"}}});
    assert!(validate(schema.clone(), json!({"count":3})));
    assert!(!validate(schema, json!({"count":-1})));
}
#[test]
fn unsupported_or_malformed_schema_fails_even_when_property_is_absent() {
    for property in [
        json!({"type":"string","pattern":"["}),
        json!({"type":"string","minLength":1}),
        json!({"type":"integer","minimum":"zero"}),
        json!({"type":"array"}),
        json!({"type":"string","enum":[]}),
    ] {
        assert!(!validate(
            json!({"type":"object","properties":{"unused":property}}),
            json!({})
        ));
    }
}
#[test]
fn malformed_values_and_oversize_values_are_refused() {
    let schema = r#"{"type":"object","properties":{}}"#;
    assert!(validate_settings(schema, "[]").is_err());
    assert!(validate_settings(schema, "{").is_err());
    assert!(validate_settings("{", "{}").is_err());
    let value = json!({"x":"x".repeat(256*1024)}).to_string();
    assert!(validate_settings(schema, &value).is_err());
}

#[test]
fn map_keys_and_values_are_checked_without_accepting_unknown_root_keys() {
    let schema = json!({"type":"object","properties":{"environment":{"type":"object","maxProperties":2,"propertyNames":{"pattern":"^[A-Z]+$"},"additionalProperties":{"type":"string"}}}});
    assert!(validate(
        schema.clone(),
        json!({"environment":{"PATH":"bin"}})
    ));
    for value in [
        json!({"environment":{"bad key":"bin"}}),
        json!({"environment":{"PATH":42}}),
        json!({"environment":{"A":"a","B":"b","C":"c"}}),
        json!({"other":{}}),
    ] {
        assert!(!validate(schema.clone(), value));
    }
}
#[test]
fn multi_choice_enforces_membership_uniqueness_and_both_item_bounds() {
    let schema = json!({"type":"object","properties":{"kinds":{"type":"array","items":{"enum":["text","images"]},"uniqueItems":true,"minItems":1,"maxItems":2}}});
    assert!(validate(schema.clone(), json!({"kinds":["text","images"]})));
    for value in [
        json!({"kinds":[]}),
        json!({"kinds":["other"]}),
        json!({"kinds":["text","text"]}),
    ] {
        assert!(!validate(schema.clone(), value));
    }
}
