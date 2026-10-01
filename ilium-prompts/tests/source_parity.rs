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

// Keep the immutable extraction hashes: only the explicitly authorized additive
// instruction blocks may differ from the frozen baseline source.
fn baseline_source(name: &str, source: &str) -> String {
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
