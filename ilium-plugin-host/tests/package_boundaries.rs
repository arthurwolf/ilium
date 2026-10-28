//! Actual-byte tests of the strict stored ZIP subset.
use ilium_plugin_host::package::{read_package, write_package};

fn vector(signed: bool) -> Vec<u8> {
    let text = if signed {
        include_str!("package/golden/minimal_signed.hex")
    } else {
        include_str!("package/golden/minimal_unsigned.hex")
    };
    let digits: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    (0..digits.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&digits[i..i + 2], 16).unwrap())
        .collect()
}
fn code(bytes: &[u8]) -> String {
    read_package(bytes).unwrap_err()[0].code.as_str().to_owned()
}

#[test]
fn canonical_writer_reproduces_the_actual_manual_vector() {
    let bytes = vector(false);
    let parsed = read_package(&bytes).unwrap();
    let files: Vec<_> = parsed
        .entries
        .into_iter()
        .map(|entry| (entry.path, entry.bytes))
        .collect();
    assert_eq!(write_package(&files).unwrap(), bytes);
    let mut reversed = files.clone();
    reversed.reverse();
    assert_eq!(write_package(&reversed).unwrap(), bytes);
}

#[test]
fn signature_and_manifest_bytes_remain_exact() {
    let bytes = vector(true);
    let parsed = read_package(&bytes).unwrap();
    assert!(parsed.signature.is_some());
    assert_eq!(
        parsed
            .entries
            .iter()
            .find(|entry| entry.path == "manifest.json")
            .unwrap()
            .bytes
            .as_slice(),
        include_bytes!("package/golden/minimal_manifest.json")
    );
    let files: Vec<_> = parsed
        .entries
        .into_iter()
        .map(|entry| (entry.path, entry.bytes))
        .collect();
    assert_eq!(write_package(&files).unwrap(), bytes);
}

#[test]
fn every_truncated_vector_is_refused_without_panicking() {
    let bytes = vector(false);
    for length in 0..bytes.len() {
        assert!(read_package(&bytes[..length]).is_err(), "prefix {length}");
    }
}

#[test]
fn crc_is_checked_before_manifest_json() {
    let mut bytes = vector(false);
    let length = u16::from_le_bytes([bytes[26], bytes[27]]) as usize;
    bytes[30 + length] = b'#';
    assert_eq!(code(&bytes), "package.crc-mismatch");
}

#[test]
fn altered_offsets_and_suffix_bytes_are_rejected() {
    let original = vector(false);
    let mut bytes = original.clone();
    let end = bytes.len() - 22;
    let directory = u32::from_le_bytes(bytes[end + 16..end + 20].try_into().unwrap()) as usize;
    bytes[directory + 42..directory + 46].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(code(&bytes), "package.header-mismatch");
    let mut trailing = original;
    trailing.extend_from_slice(b"junk");
    assert_eq!(code(&trailing), "package.zip-trailing");
}

#[test]
fn writer_refuses_cross_platform_path_collisions() {
    let manifest = include_bytes!("package/golden/minimal_manifest.json").to_vec();
    for (paths, expected) in [
        (vec!["assets/x", "assets/x"], "package.path-duplicate"),
        (vec!["assets/x", "assets/X"], "package.path-case-collision"),
        (vec!["assets/x", "assets/x/child"], "package.path-invalid"),
        (vec!["assets/CON.txt"], "package.path-invalid"),
        (vec!["assets/../x"], "package.path-invalid"),
    ] {
        let mut files = vec![("manifest.json".into(), manifest.clone())];
        files.extend(paths.into_iter().map(|path| (path.into(), Vec::new())));
        assert_eq!(write_package(&files).unwrap_err().code, expected);
    }
}

/// SEC-151 checks the declared ZIP lengths before attempting any decompression.
fn ratio_fixture(compressed: u32, uncompressed: u32) -> Vec<u8> {
    let mut bytes = write_package(&[
        ("assets/zeros".into(), vec![0xd7, 0x01]),
        (
            "manifest.json".into(),
            include_bytes!("package/golden/minimal_manifest.json").to_vec(),
        ),
    ])
    .unwrap();
    let end = bytes.len() - 22;
    let central = u32::from_le_bytes(bytes[end + 16..end + 20].try_into().unwrap()) as usize;
    assert_eq!(&bytes[central + 46..central + 58], b"assets/zeros");
    let local = u32::from_le_bytes(bytes[central + 42..central + 46].try_into().unwrap()) as usize;
    bytes[central + 10..central + 12].copy_from_slice(&8u16.to_le_bytes());
    bytes[local + 8..local + 10].copy_from_slice(&8u16.to_le_bytes());
    bytes[central + 20..central + 24].copy_from_slice(&compressed.to_le_bytes());
    bytes[central + 24..central + 28].copy_from_slice(&uncompressed.to_le_bytes());
    bytes[local + 18..local + 22].copy_from_slice(&compressed.to_le_bytes());
    bytes[local + 22..local + 26].copy_from_slice(&uncompressed.to_le_bytes());
    bytes
}

#[test]
fn expansion_ratio_above_200_is_diagnosed_before_unsupported_compression() {
    let errors =
        ilium_plugin_host::package::read_archive_entries(&ratio_fixture(2, 401)).unwrap_err();
    let ratio = errors
        .iter()
        .position(|error| error.code == "package.zip-invalid" && error.message.contains("ratio"))
        .unwrap();
    let compressed = errors
        .iter()
        .position(|error| error.code == "package.zip-compressed")
        .unwrap();
    assert!(ratio < compressed);
}

#[test]
fn expansion_ratio_exactly_200_remains_compressed_and_forbidden_without_ratio_error() {
    let errors =
        ilium_plugin_host::package::read_archive_entries(&ratio_fixture(2, 400)).unwrap_err();
    assert!(errors
        .iter()
        .any(|error| error.code == "package.zip-compressed"));
    assert!(!errors
        .iter()
        .any(|error| error.code == "package.zip-invalid" && error.message.contains("ratio")));
}

#[test]
fn zero_compressed_length_cannot_hide_nonempty_expansion_or_divide_by_zero() {
    let errors =
        ilium_plugin_host::package::read_archive_entries(&ratio_fixture(0, 1)).unwrap_err();
    assert!(errors
        .iter()
        .any(|error| error.code == "package.zip-invalid" && error.message.contains("ratio")));
    let empty = ilium_plugin_host::package::read_archive_entries(&ratio_fixture(0, 0)).unwrap_err();
    assert!(empty
        .iter()
        .any(|error| error.code == "package.zip-compressed"));
    assert!(!empty
        .iter()
        .any(|error| error.code == "package.zip-invalid" && error.message.contains("ratio")));
}
