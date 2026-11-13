//! Manifest helpers of the port twins (lane core-ui, part ports).
use serde_json::Value;

pub fn manifest_path() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json")
}

pub fn manifest_json() -> Value {
    serde_json::from_str(&std::fs::read_to_string(manifest_path()).expect("manifest file"))
        .expect("manifest json")
}

pub fn manifest_row(key: &str) -> Value {
    manifest_json()["contributes"]["settings"][0]["sections"][0]["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .find(|row| row["key"] == key)
        .cloned()
        .unwrap_or_else(|| panic!("no row {key}"))
}
