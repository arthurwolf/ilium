use ilium_prompts::{catalog, render, render_source};
use serde_json::json;

#[test]
fn every_embedded_template_is_registered_and_parses() {
    for (name, _) in catalog() {
        render(name, &json!({})).expect("embedded template renders with optional empty data");
    }
}

#[test]
fn inserted_user_text_is_literal_and_not_html_escaped() {
    let text = "{{> nonexistent}} <xml>& \"quoted\" 🦀";
    assert_eq!(
        render_source("test", "{{value}}", &json!({"value": text})).unwrap(),
        text
    );
}

#[test]
fn invalid_template_and_unknown_catalog_name_fail() {
    assert!(render_source("test", "{{#if flag}}", &json!({})).is_err());
    assert!(render("unknown/template", &json!({})).is_err());
}

#[test]
fn embedded_fragments_can_be_used_as_named_partials() {
    assert_eq!(
        render_source("test-partial", "{{> naming/json-only}}", &json!({})).unwrap(),
        ilium_prompts::naming::JSON_ONLY
    );
}

#[test]
fn status_update_instructions_are_additive_literal_and_optional() {
    let original = "please remind me, in a very compact way, what you were doing, what I asked you to do, how it went, etc, remind me what's going on";
    assert_eq!(
        render("agent/ask-for-update", &json!({})).unwrap(),
        original
    );
    assert_eq!(
        render("agent/ask-for-update", &json!({"custom_instructions": ""})).unwrap(),
        original
    );
    let extra = "Mention blockers. {{> nonexistent}} <xml>& 🦀\nThen next action.";
    let prompt = render(
        "agent/ask-for-update",
        &json!({"custom_instructions": extra}),
    )
    .unwrap();
    assert!(prompt.starts_with(original));
    assert!(prompt.contains(extra));
    assert_eq!(prompt.matches(extra).count(), 1);
}

#[test]
fn instruction_templates_ignore_preferences_for_other_tasks() {
    let all = json!({
        "entry_naming": "ENTRY_ONLY",
        "organization": "ORG_ONLY",
        "naming_and_organization": "SHARED_ONLY",
        "project_naming": "PROJECT_ONLY",
        "smart_copy": "COPY_ONLY",
        "custom_instructions": "UPDATE_ONLY",
    });
    for (name, expected) in [
        ("naming/session-title", vec!["ENTRY_ONLY", "SHARED_ONLY"]),
        ("naming/terminal-title", vec!["ENTRY_ONLY", "SHARED_ONLY"]),
        ("naming/project-name", vec!["PROJECT_ONLY", "SHARED_ONLY"]),
        (
            "naming/restructure",
            vec!["ENTRY_ONLY", "ORG_ONLY", "SHARED_ONLY"],
        ),
        ("naming/smart-copy-system", vec!["COPY_ONLY"]),
        ("agent/ask-for-update", vec!["UPDATE_ONLY"]),
    ] {
        let prompt = render(name, &all).unwrap();
        for marker in [
            "ENTRY_ONLY",
            "ORG_ONLY",
            "SHARED_ONLY",
            "PROJECT_ONLY",
            "COPY_ONLY",
            "UPDATE_ONLY",
        ] {
            assert_eq!(
                prompt.contains(marker),
                expected.contains(&marker),
                "{name}: {marker}"
            );
        }
    }
}
