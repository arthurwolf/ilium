use serde_json::Value;
use sha2::{Digest, Sha256};

#[test]
fn full_prompt_templates_preserve_frozen_original_output() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/template-cases.json")).unwrap();
    for mut case in cases {
        let name = case["name"].as_str().unwrap().to_owned();
        let labeling = case["context"]["is_labeling"].as_bool().unwrap_or(false);
        let instructions = if labeling {
            ilium_prompts::naming::LABEL_INSTRUCTIONS
        } else {
            match name.as_str() {
                "naming/session-title" => ilium_prompts::naming::SESSION_SUMMARY,
                "naming/terminal-title" => ilium_prompts::naming::TERMINAL_SUMMARY,
                "naming/restructure" => ilium_prompts::naming::RESTRUCTURE_SUMMARY,
                _ => "",
            }
        };
        let field = if name == "naming/restructure" {
            "title_instructions"
        } else {
            "style_instructions"
        };
        case["context"][field] = Value::String(instructions.to_owned());
        let rendered = ilium_prompts::render(&name, &case["context"]).unwrap();
        let source = ilium_prompts::catalog()
            .find(|&&(catalog_name, _)| catalog_name == name)
            .unwrap()
            .1;
        let legacy_rendered = ilium_prompts::render_source(
            name.strip_prefix("naming/").unwrap(),
            source,
            &case["context"],
        )
        .unwrap();
        assert_eq!(legacy_rendered, rendered, "legacy label: {}", case["case"]);
        assert_eq!(
            format!("{:x}", Sha256::digest(rendered.as_bytes())),
            case["sha256"].as_str().unwrap(),
            "{}",
            case["case"]
        );
    }
}
