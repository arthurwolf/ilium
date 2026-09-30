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
            format!("{:x}", Sha256::digest(source.as_bytes())),
            case["sha256"].as_str().unwrap(),
            "{name}"
        );
    }
}
