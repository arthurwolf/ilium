use serde_json::Value;
use sha2::{Digest, Sha256};

#[test]
fn extracted_sources_match_frozen_original_bytes_and_format_skeletons() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/source-hashes.json")).unwrap();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let source = ilium_prompts::catalog()
            .find(|&&(catalog_name, _)| catalog_name == name)
            .unwrap()
            .1;
        // Semantic intentionally extends the restructure contract. Retain the
        // original extraction hashes as history; its current behavior is
        // checked by semantic_restructure_contract and the client parser tests.
        if matches!(
            name,
            "naming/restructure"
                | "naming/restructure-label-example"
                | "naming/restructure-summary-example"
        ) {
            continue;
        }
        assert_eq!(
            format!(
                "{:x}",
                Sha256::digest(baseline_source(name, source).as_bytes())
            ),
            case["sha256"].as_str().unwrap(),
            "{name}"
        );
    }
}

#[test]
fn semantic_restructure_contract_keeps_catalog_and_required_pointers() {
    let context = serde_json::json!({
        "animation_catalog": "COMPLETE_CATALOG_SENTINEL",
        "fixed_groups": "FIXED_GROUP_SENTINEL",
        "resource_capabilities": "AUTHORED_CAPABILITY_SENTINEL",
    });
    let prompt = ilium_prompts::render("naming/restructure", &context).unwrap();
    for value in [
        "COMPLETE_CATALOG_SENTINEL",
        "FIXED_GROUP_SENTINEL",
        "AUTHORED_CAPABILITY_SENTINEL",
        "<animation-catalog>",
        "\"animations\"",
        "\"project\"",
        "name-fixed",
        "Protected splits",
    ] {
        assert!(prompt.contains(value), "missing {value}");
    }
    for name in [
        "naming/restructure-label-example",
        "naming/restructure-summary-example",
    ] {
        let example = ilium_prompts::render(name, &serde_json::json!({})).unwrap();
        let value: Value = serde_json::from_str(&example).unwrap();
        assert!(value["animations"]["definitions"].is_array());
        assert!(value["animations"]["project"].is_string());
        fn check_nodes(nodes: &[Value]) {
            for node in nodes {
                assert!(node["animation"].is_string());
                if let Some(children) = node["children"].as_array() {
                    check_nodes(children);
                }
            }
        }
        check_nodes(value["children"].as_array().unwrap());
    }
}

// Keep the immutable extraction hashes: only the explicitly authorized additive
// instruction blocks may differ from the frozen baseline source.
fn baseline_source(name: &str, source: &str) -> String {
    // Onboarding deliberately expands the sound-source registry. Normalize
    // only its exact authorized message, retaining the immutable extraction
    // hash and still rejecting any other unexpected text change.
    if name == "voice/settings/sound-source-must-be-system-beep-or"
        && source
            == "sound.source must be system_beep, sound_file, bundled_chirping, generated, or muted"
    {
        return "sound source must be system_beep or sound_file".to_owned();
    }
    let fields: &[&str] = match name {
        "agent/ask-for-update" => &["custom_instructions"],
        "naming/project-name" => &["project_naming", "naming_and_organization"],
        "naming/session-title" | "naming/terminal-title" => {
            &["entry_naming", "naming_and_organization"]
        }
        "naming/restructure" => &["entry_naming", "organization", "naming_and_organization"],
        "naming/smart-copy-system" => &["smart_copy"],
        _ => &[],
    };
    let mut baseline = source.to_owned();
    for field in fields {
        let purpose = if *field == "custom_instructions" {
            "ask-for-update"
        } else {
            field
        };
        let block = format!("{{{{#if {field}}}}}\n<custom-instructions purpose=\"{purpose}\">\n{{{{{field}}}}}\n</custom-instructions>{{{{/if}}}}");
        baseline = baseline.replace(&block, "");
    }
    baseline
}
