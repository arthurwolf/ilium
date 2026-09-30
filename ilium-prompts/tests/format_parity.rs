use serde_json::Value;
use sha2::{Digest, Sha256};

#[test]
fn migrated_format_layouts_preserve_exact_text_and_literal_values() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/format-cases.json")).unwrap();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let rendered = ilium_prompts::render(name, &case["context"]).unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(rendered.as_bytes())),
            case["sha256"].as_str().unwrap(),
            "{name}"
        );
    }
}
