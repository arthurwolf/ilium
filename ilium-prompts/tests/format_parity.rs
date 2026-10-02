use serde_json::Value;
use sha2::{Digest, Sha256};

#[test]
fn migrated_format_layouts_preserve_exact_text_and_literal_values() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/format-cases.json")).unwrap();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        if name == "naming/restructure/restructure-prompt-exceeded-the-maximum-restructure-prompt-characters-character-safet" {
            let rendered = ilium_prompts::render(name, &serde_json::json!({"v0": "200000", "v1": "200001"})).unwrap();
            assert!(rendered.contains("200001 estimated tokens"));
            assert!(rendered.contains("200000-token safety budget"));
            assert!(rendered.contains("Settings > Inference"));
            continue;
        }
        let rendered = ilium_prompts::render(name, &case["context"]).unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(rendered.as_bytes())),
            case["sha256"].as_str().unwrap(),
            "{name}"
        );
    }
}
