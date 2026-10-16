//! reference-registry.md PKG-652 (JCS vector, duplicate keys) and PKG-653 (revocations.json
//! signature). Lane `registry-publish`, part 2.

use crate::common::*;
use crate::pages_p2::*;
use serde_json::json;

const PAGE: &str = "reference-registry.md";
const VECTOR_JCS: &str = r#"{"blocked_ids":[],"expires":"2026-10-19 12:00:00","keys":[{"effective":"2026-10-12 10:00:00","key_id":"56475aa75463474c0285df5dbf2bcab73da651358839e9b77481b2eab107708c","publisher_id":"example","reason":"compromised"}],"type":"revocations","version":17}"#;
const VECTOR_SHA: &str = "2ad56689abaf8da83199f2beca98b7342dab7a5fd2269e5a0616acb514b94130";

fn vector() -> serde_json::Value {
    json!({
        "version": 17, "type": "revocations", "expires": "2026-10-19 12:00:00",
        "keys": [{ "publisher_id": "example", "key_id": "56475aa75463474c0285df5dbf2bcab73da651358839e9b77481b2eab107708c", "reason": "compromised", "effective": "2026-10-12 10:00:00" }],
        "blocked_ids": []
    })
}

/// REQ: PKG-652
#[test]
fn doc_pkg_652_the_page_carries_the_jcs_string_and_its_sha256() {
    let text = page_text(PAGE);
    assert!(text.contains(VECTOR_JCS), "JCS vector line missing");
    assert!(text.contains(VECTOR_SHA), "vector hash missing");
    assert_row(PAGE, "PKG-652", &["JCS (RFC 8785)", "refusal of duplicate keys"]);
}

/// REQ: PKG-652
#[test]
fn pkg_652_jcs_of_the_vector_equals_the_published_string() {
    assert_eq!(jcs(&vector()), VECTOR_JCS);
}

/// REQ: PKG-652
#[test]
fn pkg_652_sha256_of_the_jcs_vector_equals_the_published_hash() {
    assert_eq!(sha256_hex(jcs(&vector()).as_bytes()), VECTOR_SHA);
}

/// REQ: PKG-652
#[test]
fn pkg_652_a_document_with_reordered_keys_and_whitespace_still_verifies() {
    let builder = RegistryBuilder::standard();
    let tree = builder.build();
    let original = tree.get("timestamp.json").expect("timestamp").clone();
    let mut doc: serde_json::Value = serde_json::from_slice(&original).expect("json");
    let signed = doc["signed"].clone();
    let signatures = doc["signatures"].clone();
    let mut keys: Vec<(String, serde_json::Value)> = signed.as_object().expect("object").iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    keys.reverse();
    let body: Vec<String> = keys.iter().map(|(k, v)| format!("  \"{k}\"  :   {v}")).collect();
    let text = format!("{{\n \"signatures\" : {signatures},\n \"signed\" : {{\n{}\n }}\n}}\n", body.join(",\n"));
    doc = serde_json::from_str(&text).expect("hand written JSON parses");
    assert_eq!(jcs(&doc["signed"]), jcs(&signed));
    let sandbox = Sandbox::with_registry(&builder);
    sandbox.net.put_file(&format!("{BASE}timestamp.json"), text.into_bytes());
    let output = sandbox.run(&["search"]);
    assert_ok(&output);
}

/// REQ: PKG-652
#[test]
fn pkg_652_a_signed_value_with_a_duplicate_key_is_refused() {
    let builder = RegistryBuilder::standard();
    let tree = builder.build();
    let text = String::from_utf8(tree.get("timestamp.json").expect("timestamp").clone()).expect("utf-8");
    let marker = "\"signed\":";
    let at_signed = text.find(marker).expect("signed key");
    let after = &text[at_signed + marker.len()..];
    let brace = after.find('{').expect("object start");
    let open = at_signed + marker.len() + brace + 1;
    let forged = format!("{}\"version\":1,{}", &text[..open], &text[open..]);
    let sandbox = Sandbox::with_registry(&builder);
    sandbox.net.put_file(&format!("{BASE}timestamp.json"), forged.into_bytes());
    let output = sandbox.run(&["search"]);
    assert_registry_failure(&output);
    assert_no_plugin_records(&output);
}

/// REQ: PKG-652
#[test]
fn pkg_652_a_changed_signed_value_breaks_the_signature() {
    let builder = RegistryBuilder::standard();
    let tree = builder.build();
    let mut doc: serde_json::Value = serde_json::from_slice(tree.get("timestamp.json").expect("timestamp")).expect("json");
    doc["signed"]["version"] = json!(doc["signed"]["version"].as_u64().unwrap_or(1) + 1);
    let sandbox = Sandbox::with_registry(&builder);
    sandbox.net.put_file(&format!("{BASE}timestamp.json"), serde_json::to_vec(&doc).expect("json"));
    assert_code(&sandbox.run(&["search"]), "registry.signature-invalid");
}

/// REQ: PKG-653
#[test]
fn doc_pkg_653_revocations_use_the_wrapper_and_the_timestamp_role_keys() {
    assert_row(PAGE, "PKG-653", &["`signed` and `signatures` wrapper", "`timestamp` role keys", "`registry.signature-invalid`"]);
}

/// REQ: PKG-653
#[test]
fn pkg_653_a_revocations_document_signed_by_the_timestamp_keys_is_accepted() {
    let sandbox = Sandbox::standard();
    let output = sandbox.run(&["search"]);
    assert_ok(&output);
    assert!(!plugin_ids(&output).is_empty());
}

/// REQ: PKG-653
#[test]
fn pkg_653_revocations_without_any_signature_are_refused_signature_invalid() {
    let mut builder = RegistryBuilder::standard();
    builder.overrides.insert("revocations".to_owned(), vec![]);
    let sandbox = Sandbox::with_registry(&builder);
    let output = sandbox.run(&["search"]);
    assert_code(&output, "registry.signature-invalid");
    assert_no_plugin_records(&output);
}

/// REQ: PKG-653
#[test]
fn pkg_653_revocations_signed_by_a_foreign_key_are_refused_signature_invalid() {
    let mut builder = RegistryBuilder::standard();
    let doc = builder.build().get("revocations.json").expect("revocations").clone();
    let forged = edit_signed(&doc, &[KEY_FOREIGN], |_| {});
    builder.overrides.insert("revocations".to_owned(), forged);
    let sandbox = Sandbox::with_registry(&builder);
    assert_code(&sandbox.run(&["search"]), "registry.signature-invalid");
}

/// REQ: PKG-653
#[test]
fn pkg_653_a_threshold_of_two_is_not_met_by_one_signature() {
    let mut builder = RegistryBuilder::standard();
    builder.role_keys = vec![KEY_A, KEY_B];
    builder.threshold = 2;
    builder.signer_count = 2;
    let doc = builder.build().get("revocations.json").expect("revocations").clone();
    let one = edit_signed(&doc, &[KEY_A], |_| {});
    builder.overrides.insert("revocations".to_owned(), one);
    let sandbox = Sandbox::with_registry(&builder);
    assert_code(&sandbox.run(&["search"]), "registry.signature-invalid");
}

/// REQ: PKG-653
#[test]
fn pkg_653_a_threshold_of_two_is_met_by_two_signatures() {
    let mut builder = RegistryBuilder::standard();
    builder.role_keys = vec![KEY_A, KEY_B];
    builder.threshold = 2;
    builder.signer_count = 2;
    let sandbox = Sandbox::with_registry(&builder);
    assert_ok(&sandbox.run(&["search"]));
}

/// REQ: PKG-653
#[test]
fn pkg_653_a_revocations_document_without_the_wrapper_is_refused() {
    let mut builder = RegistryBuilder::standard();
    let bare = serde_json::to_vec(&json!({ "type": "revocations", "version": 1, "keys": [], "blocked_ids": [], "expires": at(86_400) })).expect("json");
    builder.overrides.insert("revocations".to_owned(), bare);
    let sandbox = Sandbox::with_registry(&builder);
    let output = sandbox.run(&["search"]);
    assert_registry_failure(&output);
    assert_no_plugin_records(&output);
}
