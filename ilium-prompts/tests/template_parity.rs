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
        // The new mandatory Semantic contract changes this prompt's output.
        // Keep its old fixture/hash intact as provenance; source_parity's
        // semantic contract and client regressions verify the current shape.
        if name != "naming/restructure" {
            assert_eq!(
                format!("{:x}", Sha256::digest(rendered.as_bytes())),
                case["sha256"].as_str().unwrap(),
                "{}",
                case["case"]
            );
        }
    }
}

#[test]
fn custom_guidance_is_literal_and_isolated_to_its_template() {
    let literal = "Prefer French {{> nonexistent}} <x>& 🦀";
    for (name, fields) in [
        (
            "naming/session-title",
            vec!["entry_naming", "naming_and_organization"],
        ),
        (
            "naming/terminal-title",
            vec!["entry_naming", "naming_and_organization"],
        ),
        (
            "naming/project-name",
            vec!["project_naming", "naming_and_organization"],
        ),
        (
            "naming/restructure",
            vec!["entry_naming", "organization", "naming_and_organization"],
        ),
        ("naming/smart-copy-system", vec!["smart_copy"]),
    ] {
        let baseline = ilium_prompts::render(name, &serde_json::json!({})).unwrap();
        let mut context = serde_json::json!({});
        for field in &fields {
            context[*field] = Value::String(literal.to_owned());
        }
        let rendered = ilium_prompts::render(name, &context).unwrap();
        assert_eq!(rendered.matches(literal).count(), fields.len(), "{name}");
        assert!(rendered.contains("custom-instructions"), "{name}");
        for field in fields {
            context[field] = Value::String(String::new());
        }
        assert_eq!(ilium_prompts::render(name, &context).unwrap(), baseline);
    }
}
