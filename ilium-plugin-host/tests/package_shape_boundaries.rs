use ilium_plugin_host::package::{read_package, write_package};
use serde_json::{json, Value};

fn manifest() -> Vec<u8> {
    serde_json::to_vec(&json!({
        "api":"ilium:plugin@0.1.0", "id":"example.shape", "name":"Shape",
        "version":"1.0.0", "publisher":"Example",
        "client":{"entry":"client.wasm", "world":"ilium:client/base@0.1.0"}
    }))
    .unwrap()
}
fn envelope() -> Value {
    json!({"format":"ilium-sig/1", "signatures":[{
        "algorithm":"ed25519", "key_id":"0".repeat(64), "publisher_id":"example",
        "signature":"A".repeat(86), "signed_at":"2026-10-22 12:00:00"
    }]})
}
fn package(signature: Vec<u8>) -> Vec<u8> {
    write_package(&[
        ("manifest.json".into(), manifest()),
        ("signature".into(), signature),
    ])
    .unwrap()
}
fn first_code(bytes: &[u8]) -> String {
    read_package(bytes).unwrap_err()[0].code.as_str().into()
}
fn central_offset(bytes: &[u8]) -> usize {
    let end = bytes.len() - 22;
    u32::from_le_bytes(bytes[end + 16..end + 20].try_into().unwrap()) as usize
}
fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

#[test]
fn shape_validation_preserves_exact_bytes_without_claiming_crypto_verification() {
    // Zero signature bytes have valid shape but do not authenticate the package.
    let mut raw = serde_json::to_vec_pretty(&envelope()).unwrap();
    raw.extend_from_slice(b"\n \n");
    let archive = read_package(&package(raw.clone())).unwrap();
    assert_eq!(
        archive
            .entries
            .iter()
            .find(|entry| entry.path == "signature")
            .unwrap()
            .bytes,
        raw
    );
    assert_eq!(archive.signature.unwrap().publisher_id, "example");
    assert!(
        read_package(&write_package(&[("manifest.json".into(), manifest())]).unwrap())
            .unwrap()
            .signature
            .is_none()
    );
}

#[test]
fn malformed_envelopes_never_become_unsigned_packages() {
    let mut extra = envelope();
    extra["unexpected"] = json!(true);
    let mut missing = envelope();
    missing.as_object_mut().unwrap().remove("format");
    let mut nested_extra = envelope();
    nested_extra["signatures"][0]["unexpected"] = json!(true);
    let mut bad_second = envelope();
    bad_second["signatures"]
        .as_array_mut()
        .unwrap()
        .push(json!({}));
    for document in [
        json!({}),
        json!({"format":"ilium-sig/1", "signatures":[]}),
        extra,
        missing,
        nested_extra,
        bad_second,
    ] {
        assert_eq!(
            first_code(&package(serde_json::to_vec(&document).unwrap())),
            "package.signature-format"
        );
    }
    for raw in [b"not JSON".as_slice(), b"null", b"[]"] {
        assert_eq!(
            first_code(&package(raw.to_vec())),
            "package.signature-format"
        );
    }
}

#[test]
fn duplicate_json_members_are_rejected_before_value_projection() {
    let raw = serde_json::to_string(&envelope()).unwrap();
    let duplicate = raw.replacen(
        "\"format\":\"ilium-sig/1\"",
        "\"format\":\"ilium-sig/1\",\"format\":\"ilium-sig/1\"",
        1,
    );
    assert_eq!(
        first_code(&package(duplicate.into_bytes())),
        "package.signature-format"
    );
}

#[test]
fn signature_byte_cap_is_inclusive_and_has_signature_format_diagnostic() {
    let mut raw = serde_json::to_vec(&envelope()).unwrap();
    raw.resize(16 * 1024, b' ');
    let bytes = package(raw.clone());
    assert!(read_package(&bytes).is_ok());
    raw.push(b' ');
    assert_eq!(
        write_package(&[
            ("manifest.json".into(), manifest()),
            ("signature".into(), raw)
        ])
        .unwrap_err()
        .code
        .as_str(),
        "package.signature-format"
    );
    // Admission checks central declared sizes before reading/allocating the entry.
    let mut oversized = bytes;
    let first = central_offset(&oversized);
    let name_length =
        u16::from_le_bytes(oversized[first + 28..first + 30].try_into().unwrap()) as usize;
    let signature_header = first + 46 + name_length;
    put32(&mut oversized, signature_header + 20, 16 * 1024 + 1);
    put32(&mut oversized, signature_header + 24, 16 * 1024 + 1);
    assert_eq!(first_code(&oversized), "package.signature-format");
}

#[test]
fn container_violations_keep_specific_zip_diagnostics() {
    let original = write_package(&[("manifest.json".into(), manifest())]).unwrap();
    let central = central_offset(&original);
    let mut encrypted = original.clone();
    encrypted[central + 8..central + 10].copy_from_slice(&0x0801u16.to_le_bytes());
    assert_eq!(first_code(&encrypted), "package.zip-encrypted");
    let mut compressed = original.clone();
    compressed[central + 10..central + 12].copy_from_slice(&8u16.to_le_bytes());
    assert_eq!(first_code(&compressed), "package.zip-compressed");
    let mut trailing = original;
    trailing.push(0);
    assert_eq!(first_code(&trailing), "package.zip-trailing");
}

#[test]
fn archive_stage_collects_distinct_violations_without_parsing_content() {
    let mut bytes = write_package(&[("manifest.json".into(), b"not JSON".to_vec())]).unwrap();
    let central = central_offset(&bytes);
    bytes[central + 8..central + 10].copy_from_slice(&0x0801u16.to_le_bytes());
    bytes[central + 10..central + 12].copy_from_slice(&8u16.to_le_bytes());
    let errors = read_package(&bytes).unwrap_err();
    let codes: Vec<_> = errors.iter().map(|error| error.code.as_str()).collect();
    assert_eq!(codes, ["package.zip-encrypted", "package.zip-compressed"]);
}

#[test]
fn archive_only_reader_leaves_manifest_and_signature_validation_to_later_stages() {
    use ilium_plugin_host::package::read_archive_entries;
    let bytes = write_package(&[
        ("manifest.json".into(), b"not JSON".to_vec()),
        ("signature".into(), b"not JSON".to_vec()),
    ])
    .unwrap();
    let entries = read_archive_entries(&bytes).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].bytes, b"not JSON");
    assert!(read_package(&bytes).is_err());
    let bytes = package(b"not JSON".to_vec());
    assert!(read_archive_entries(&bytes).is_ok());
    assert_eq!(first_code(&bytes), "package.signature-format");
}

#[test]
fn declared_expansion_limits_are_checked_before_entry_data() {
    use ilium_plugin_host::package::read_archive_entries;
    let files: Vec<_> = (0..8)
        .map(|index| (format!("assets/{index}"), Vec::new()))
        .chain(std::iter::once(("manifest.json".into(), vec![b'x'])))
        .collect();
    let base = write_package(&files).unwrap();
    for (extra, exceeds) in [(0, false), (1, true)] {
        let mut bytes = base.clone();
        let mut cursor = central_offset(&bytes);
        for index in 0..8 {
            let length =
                64 * 1024 * 1024 - u32::from(index == 0) + if index == 0 { extra } else { 0 };
            put32(&mut bytes, cursor + 20, length);
            put32(&mut bytes, cursor + 24, length);
            let name_len =
                u16::from_le_bytes(bytes[cursor + 28..cursor + 30].try_into().unwrap()) as usize;
            cursor += 46 + name_len;
        }
        let errors = read_archive_entries(&bytes).unwrap_err();
        let expansion_error = errors
            .iter()
            .any(|error| error.message.contains("total uncompressed size"));
        assert_eq!(expansion_error, exceeds);
    }
    for (length, exceeds) in [(200, false), (201, true)] {
        let mut bytes = base.clone();
        let cursor = central_offset(&bytes);
        put32(&mut bytes, cursor + 20, 1);
        put32(&mut bytes, cursor + 24, length);
        let errors = read_archive_entries(&bytes).unwrap_err();
        assert_eq!(
            errors
                .iter()
                .any(|error| error.message.contains("compression ratio")),
            exceeds
        );
    }
}
