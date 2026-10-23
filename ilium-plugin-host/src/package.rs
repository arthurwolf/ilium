//! The `.ilp` package archive: a strict ZIP subset, read and written by hand
//! (reference-packaging.md PKG-100..182).

use ilium_plugin_api::manifest::{Diagnostic, FileEntry, Manifest};

/// One file in a package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveEntry {
    /// Path inside the archive (reference-packaging.md PKG-110).
    pub path: String,
    /// File bytes.
    pub bytes: Vec<u8>,
}

/// The signature document `signature.json` (reference-packaging.md PKG-200).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureDoc {
    /// Key id of the signer.
    pub key_id: String,
    /// Publisher id of the signer.
    pub publisher_id: String,
    /// Signing time, RFC 3339 UTC.
    pub signed_at: String,
    /// Base64 Ed25519 signature over the canonical manifest and file table.
    pub signature: String,
    /// The signed file table.
    pub files: Vec<FileEntry>,
}

/// A parsed, structurally valid package (reference-packaging.md).
#[derive(Debug, Clone)]
pub struct PackageArchive {
    /// The manifest.
    pub manifest: Manifest,
    /// Every other file.
    pub entries: Vec<ArchiveEntry>,
    /// The signature, if the package is signed.
    pub signature: Option<SignatureDoc>,
}

/// Reads package bytes; a structural fault is a `package.*` diagnostic (reference-packaging.md).
pub fn read_package(_bytes: &[u8]) -> Result<PackageArchive, Vec<Diagnostic>> {
    unimplemented!("host::package::read_package")
}

/// Writes files as a package in the accepted ZIP subset (reference-packaging.md).
pub fn write_package(_files: &[(String, Vec<u8>)]) -> Result<Vec<u8>, Diagnostic> {
    unimplemented!("host::package::write_package")
}

#[cfg(test)]
mod tests {
    use super::{read_package, write_package, PackageArchive};
    use ilium_plugin_api::manifest::Diagnostic;

    const MANIFEST_BYTES: &[u8] = include_bytes!("../tests/package/golden/minimal_manifest.json");
    const COMPONENT_HEX: &str = include_str!("../tests/package/golden/minimal_component.hex");
    const UNSIGNED_HEX: &str = include_str!("../tests/package/golden/minimal_unsigned.hex");
    const SIGNED_HEX: &str = include_str!("../tests/package/golden/minimal_signed.hex");

    const LOCAL_HEADER_SIGNATURE_LEN: usize = 30;
    const CENTRAL_HEADER_FIXED_LEN: usize = 46;
    const END_RECORD_LEN: usize = 22;

    fn unhex(text: &str) -> Vec<u8> {
        let digits: Vec<u8> = text
            .bytes()
            .filter(|byte| !byte.is_ascii_whitespace())
            .collect();
        digits
            .chunks(2)
            .map(|pair| {
                let text_pair = std::str::from_utf8(pair).expect("hex digits are ASCII");
                u8::from_str_radix(text_pair, 16).expect("golden fixture holds valid hex")
            })
            .collect()
    }

    fn vector_manifest_and_component() -> Vec<(String, Vec<u8>)> {
        vec![
            ("manifest.json".to_owned(), MANIFEST_BYTES.to_vec()),
            ("server.wasm".to_owned(), unhex(COMPONENT_HEX)),
        ]
    }

    fn vector_unsigned() -> Vec<u8> {
        unhex(UNSIGNED_HEX)
    }

    fn vector_signed() -> Vec<u8> {
        unhex(SIGNED_HEX)
    }

    fn u16_at(bytes: &[u8], position: usize) -> usize {
        usize::from(u16::from_le_bytes([bytes[position], bytes[position + 1]]))
    }

    fn u32_at(bytes: &[u8], position: usize) -> usize {
        let raw = [
            bytes[position],
            bytes[position + 1],
            bytes[position + 2],
            bytes[position + 3],
        ];
        usize::try_from(u32::from_le_bytes(raw)).expect("u32 fits usize on supported targets")
    }

    fn put(bytes: &mut [u8], position: usize, value: &[u8]) {
        bytes[position..position + value.len()].copy_from_slice(value);
    }

    /// Central directory header offsets, found through the end record (PKG-113).
    fn central_offsets(bytes: &[u8]) -> Vec<usize> {
        let end_record = bytes.len() - END_RECORD_LEN;
        let count = u16_at(bytes, end_record + 10);
        let mut offsets = Vec::new();
        let mut position = u32_at(bytes, end_record + 16);
        for _ in 0..count {
            offsets.push(position);
            position += CENTRAL_HEADER_FIXED_LEN
                + u16_at(bytes, position + 28)
                + u16_at(bytes, position + 30)
                + u16_at(bytes, position + 32);
        }
        offsets
    }

    fn local_offset_of(bytes: &[u8], central_position: usize) -> usize {
        u32_at(bytes, central_position + 42)
    }

    /// Byte position of the first data byte of the entry whose central header sits at `central_position`.
    fn data_start_of(bytes: &[u8], central_position: usize) -> usize {
        let local = local_offset_of(bytes, central_position);
        local + LOCAL_HEADER_SIGNATURE_LEN + u16_at(bytes, local + 26) + u16_at(bytes, local + 28)
    }

    /// Sets the given bits in the general-purpose flags of every local and central header.
    fn or_flags_everywhere(bytes: &mut [u8], bits: u8) {
        for central in central_offsets(bytes) {
            let local = local_offset_of(bytes, central);
            bytes[local + 6] |= bits;
            bytes[central + 8] |= bits;
        }
    }

    /// Writes the compression method into every local and central header.
    fn set_method_everywhere(bytes: &mut [u8], method: u8) {
        for central in central_offsets(bytes) {
            let local = local_offset_of(bytes, central);
            put(bytes, local + 8, &[method, 0]);
            put(bytes, central + 10, &[method, 0]);
        }
    }

    fn refusal_codes(outcome: Result<PackageArchive, Vec<Diagnostic>>) -> Vec<String> {
        match outcome {
            Ok(_) => panic!("package was accepted, but a refusal was expected"),
            Err(diagnostics) => diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code.as_str().to_owned())
                .collect(),
        }
    }

    fn assert_refused_with(bytes: &[u8], expected_code: &str) {
        let codes = refusal_codes(read_package(bytes));
        assert!(
            codes.iter().any(|code| code == expected_code),
            "expected refusal {expected_code}, got {codes:?}"
        );
    }

    fn write_refusal_code(files: &[(String, Vec<u8>)]) -> String {
        match write_package(files) {
            Ok(_) => panic!("package was written, but a refusal was expected"),
            Err(diagnostic) => diagnostic.code.as_str().to_owned(),
        }
    }

    // ---------------------------------------------------------------------------------------------
    // Reading valid packages
    // ---------------------------------------------------------------------------------------------

    /// REQ: PKG-100
    #[test]
    fn read_package_accepts_the_unsigned_vector_and_splits_manifest_from_entries() {
        let archive = read_package(&vector_unsigned()).expect("the manual vector is accepted");
        assert_eq!(archive.manifest.id, "example.minimal");
        assert_eq!(archive.entries.len(), 1);
        assert_eq!(archive.entries[0].path, "server.wasm");
        assert_eq!(archive.entries[0].bytes, unhex(COMPONENT_HEX));
        assert!(archive.signature.is_none());
    }

    /// REQ: PKG-205
    #[test]
    fn read_package_parses_the_signature_document_of_the_signed_vector() {
        let archive = read_package(&vector_signed()).expect("the signed vector is accepted");
        let signature = archive
            .signature
            .expect("the signed vector carries a signature");
        assert_eq!(
            signature.key_id,
            "56475aa75463474c0285df5dbf2bcab73da651358839e9b77481b2eab107708c"
        );
        assert_eq!(signature.publisher_id, "example");
        assert_eq!(signature.signed_at, "2026-10-12 10:00:00");
        assert_eq!(
            signature.signature,
            "jv5zcDEiiDljfN-iy3nZLtCgScmbTMJoOv0M3X9r0D9Tm4dvdqE43gpEEAW_cqVod6iYQAjeIR8E0-vFwEplBw"
        );
    }

    /// REQ: PKG-208
    #[test]
    fn read_package_refuses_a_signature_entry_that_is_not_a_signature_document() {
        let files = vec![
            ("manifest.json".to_owned(), MANIFEST_BYTES.to_vec()),
            ("signature".to_owned(), b"not json".to_vec()),
        ];
        let bytes = write_package(&files).expect("the writer accepts any entry bytes");
        assert_refused_with(&bytes, "package.signature-format");
    }

    /// REQ: PKG-131
    #[test]
    fn read_package_refuses_a_signature_entry_over_16_kib() {
        let mut files = vector_manifest_and_component();
        files.push(("signature".to_owned(), vec![b' '; 16 * 1024 + 1]));
        let bytes = write_package(&files).expect("the writer accepts the oversized entry");
        assert_refused_with(&bytes, "package.size-limit");
    }

    /// REQ: PKG-167
    #[test]
    fn read_package_refuses_an_empty_archive_without_a_manifest() {
        let mut empty_archive = vec![0x50, 0x4B, 0x05, 0x06];
        empty_archive.extend_from_slice(&[0u8; 18]);
        assert_refused_with(&empty_archive, "package.manifest-missing");
    }

    /// REQ: PKG-114
    #[test]
    fn read_package_refuses_empty_input_as_zip_invalid() {
        assert_refused_with(&[], "package.zip-invalid");
    }

    // ---------------------------------------------------------------------------------------------
    // Container hostile cases (mutations of the golden vector)
    // ---------------------------------------------------------------------------------------------

    /// REQ: PKG-171
    #[test]
    fn read_package_refuses_bytes_appended_after_the_end_record() {
        let mut bytes = vector_unsigned();
        bytes.extend_from_slice(&[0u8; 16]);
        assert_refused_with(&bytes, "package.zip-trailing");
    }

    /// REQ: PKG-170
    #[test]
    fn read_package_refuses_an_archive_comment() {
        let mut bytes = vector_unsigned();
        let end_record = bytes.len() - END_RECORD_LEN;
        put(&mut bytes, end_record + 20, &[4, 0]);
        bytes.extend_from_slice(b"note");
        assert_refused_with(&bytes, "package.zip-comment");
    }

    /// REQ: PKG-168
    #[test]
    fn read_package_refuses_a_deflate_entry() {
        let mut bytes = vector_unsigned();
        set_method_everywhere(&mut bytes, 8);
        assert_refused_with(&bytes, "package.zip-compressed");
    }

    /// REQ: PKG-169
    #[test]
    fn read_package_refuses_an_encrypted_entry() {
        let mut bytes = vector_unsigned();
        or_flags_everywhere(&mut bytes, 0x01);
        assert_refused_with(&bytes, "package.zip-encrypted");
    }

    /// REQ: PKG-101
    #[test]
    fn read_package_refuses_an_entry_with_a_data_descriptor() {
        let mut bytes = vector_unsigned();
        or_flags_everywhere(&mut bytes, 0x08);
        assert_refused_with(&bytes, "package.zip-descriptor");
    }

    /// REQ: PKG-102
    #[test]
    fn read_package_refuses_zip64_sentinel_sizes() {
        let mut bytes = vector_unsigned();
        let end_record = bytes.len() - END_RECORD_LEN;
        put(&mut bytes, end_record + 12, &[0xFF; 4]);
        assert_refused_with(&bytes, "package.zip64");
    }

    /// REQ: PKG-164
    #[test]
    fn read_package_refuses_a_data_byte_changed_under_an_unchanged_crc() {
        let mut bytes = vector_unsigned();
        let central = central_offsets(&bytes)[0];
        let first_data_byte = data_start_of(&bytes, central);
        bytes[first_data_byte] ^= 0xFF;
        assert_refused_with(&bytes, "package.crc-mismatch");
    }

    /// REQ: PKG-109
    #[test]
    fn read_package_refuses_a_name_that_differs_between_local_and_central_headers() {
        let mut bytes = vector_unsigned();
        let central = central_offsets(&bytes)[0];
        put(&mut bytes, central + 46, b"n");
        assert_refused_with(&bytes, "package.header-mismatch");
    }

    /// REQ: PKG-112
    #[test]
    fn read_package_refuses_a_local_offset_that_points_into_another_entry() {
        let mut bytes = vector_unsigned();
        let second_central = central_offsets(&bytes)[1];
        put(&mut bytes, second_central + 42, &[0, 0, 0, 0]);
        assert_refused_with(&bytes, "package.header-mismatch");
    }

    /// REQ: PKG-113
    #[test]
    fn read_package_refuses_an_end_record_count_that_disagrees_with_the_directory() {
        let mut bytes = vector_unsigned();
        let end_record = bytes.len() - END_RECORD_LEN;
        put(&mut bytes, end_record + 10, &[3, 0]);
        assert_refused_with(&bytes, "package.zip-invalid");
    }

    /// REQ: PKG-165
    #[test]
    fn read_package_refuses_a_truncated_central_directory() {
        let mut bytes = vector_unsigned();
        let half = bytes.len() / 2;
        bytes.truncate(half);
        assert_refused_with(&bytes, "package.zip-invalid");
    }

    /// REQ: PKG-172
    #[test]
    fn read_package_refuses_a_symbolic_link_entry() {
        let mut bytes = vector_unsigned();
        let central = central_offsets(&bytes)[0];
        // External attributes 0xA1FF0000: Unix mode 0120777, a symbolic link.
        put(&mut bytes, central + 38, &[0x00, 0x00, 0xFF, 0xA1]);
        assert_refused_with(&bytes, "package.entry-symlink");
    }

    // ---------------------------------------------------------------------------------------------
    // Paths read from an archive
    // ---------------------------------------------------------------------------------------------

    /// REQ: PKG-116
    #[test]
    fn read_package_refuses_a_path_with_a_leading_slash() {
        let mut bytes = vector_unsigned();
        let central = central_offsets(&bytes)[0];
        let local = local_offset_of(&bytes, central);
        put(&mut bytes, local + 30, b"/");
        put(&mut bytes, central + 46, b"/");
        assert_refused_with(&bytes, "package.path-invalid");
    }

    /// REQ: PKG-115
    #[test]
    fn read_package_refuses_a_path_with_a_backslash() {
        let mut bytes = vector_unsigned();
        let central = central_offsets(&bytes)[0];
        let local = local_offset_of(&bytes, central);
        put(&mut bytes, local + 30 + 8, b"\\");
        put(&mut bytes, central + 46 + 8, b"\\");
        assert_refused_with(&bytes, "package.path-invalid");
    }

    // ---------------------------------------------------------------------------------------------
    // Writing packages
    // ---------------------------------------------------------------------------------------------

    /// REQ: PKG-146
    #[test]
    fn write_package_reproduces_the_unsigned_vector_byte_for_byte() {
        let bytes = write_package(&vector_manifest_and_component()).expect("write the vector");
        assert_eq!(bytes, vector_unsigned());
    }

    /// REQ: PKG-146
    #[test]
    fn write_package_output_reads_back_to_the_same_manifest_and_entries() {
        let bytes = write_package(&vector_manifest_and_component()).expect("write the vector");
        let archive = read_package(&bytes).expect("a written package is readable");
        assert_eq!(archive.manifest.id, "example.minimal");
        assert_eq!(archive.entries.len(), 1);
        assert_eq!(archive.entries[0].path, "server.wasm");
        assert_eq!(archive.entries[0].bytes, unhex(COMPONENT_HEX));
    }

    /// REQ: PKG-122
    #[test]
    fn write_package_sorts_entries_so_input_order_does_not_change_the_bytes() {
        let sorted = write_package(&vector_manifest_and_component()).expect("write sorted");
        let mut reversed_input = vector_manifest_and_component();
        reversed_input.reverse();
        let reversed = write_package(&reversed_input).expect("write reversed");
        assert_eq!(sorted, reversed);
    }

    /// REQ: PKG-147
    #[test]
    fn write_package_is_deterministic_over_identical_inputs() {
        let first = write_package(&vector_manifest_and_component()).expect("first write");
        let second = write_package(&vector_manifest_and_component()).expect("second write");
        assert_eq!(first, second);
    }

    /// REQ: PKG-111
    #[test]
    fn write_package_of_no_files_is_a_bare_end_record_that_reads_as_manifest_missing() {
        let bytes = write_package(&[]).expect("an empty file list writes an archive");
        assert_eq!(bytes.len(), END_RECORD_LEN);
        assert_refused_with(&bytes, "package.manifest-missing");
    }

    /// REQ: PKG-116
    #[test]
    fn write_package_refuses_a_path_that_starts_with_a_slash() {
        let files = vec![("/etc/passwd".to_owned(), vec![0])];
        assert_eq!(write_refusal_code(&files), "package.path-invalid");
    }

    /// REQ: PKG-157
    #[test]
    fn write_package_refuses_a_parent_directory_component() {
        let files = vec![("../evil.wasm".to_owned(), vec![0])];
        assert_eq!(write_refusal_code(&files), "package.path-invalid");
    }

    /// REQ: PKG-116
    #[test]
    fn write_package_refuses_an_empty_path_component() {
        let files = vec![("assets//x.bin".to_owned(), vec![0])];
        assert_eq!(write_refusal_code(&files), "package.path-invalid");
    }

    /// REQ: PKG-115
    #[test]
    fn write_package_refuses_a_backslash_in_a_path() {
        let files = vec![("assets\\x.png".to_owned(), vec![0])];
        assert_eq!(write_refusal_code(&files), "package.path-invalid");
    }

    /// REQ: PKG-117
    #[test]
    fn write_package_refuses_a_hidden_component() {
        let files = vec![("assets/.hidden".to_owned(), vec![0])];
        assert_eq!(write_refusal_code(&files), "package.path-invalid");
    }

    /// REQ: PKG-118
    #[test]
    fn write_package_refuses_a_windows_reserved_name_in_any_case() {
        let files = vec![("assets/CON.bin".to_owned(), vec![0])];
        assert_eq!(write_refusal_code(&files), "package.path-invalid");
    }

    /// REQ: PKG-105
    #[test]
    fn write_package_refuses_a_directory_entry() {
        let files = vec![
            ("manifest.json".to_owned(), MANIFEST_BYTES.to_vec()),
            ("assets/".to_owned(), Vec::new()),
        ];
        assert_eq!(write_refusal_code(&files), "package.entry-directory");
    }

    /// REQ: PKG-162
    #[test]
    fn write_package_refuses_the_same_path_twice() {
        let mut files = vector_manifest_and_component();
        files.push(("server.wasm".to_owned(), vec![0]));
        assert_eq!(write_refusal_code(&files), "package.path-duplicate");
    }

    /// REQ: PKG-120
    #[test]
    fn write_package_refuses_paths_that_collide_after_case_folding() {
        let files = vec![
            ("assets/Server.bin".to_owned(), vec![0]),
            ("assets/server.bin".to_owned(), vec![0]),
        ];
        assert_eq!(write_refusal_code(&files), "package.path-case-collision");
    }

    /// REQ: PKG-126
    #[test]
    fn write_package_refuses_more_than_4096_entries() {
        let mut files = vector_manifest_and_component();
        for index in 1..=4095 {
            files.push((format!("assets/f{index:04}.bin"), Vec::new()));
        }
        assert_eq!(files.len(), 4097);
        assert_eq!(write_refusal_code(&files), "package.entry-limit");
    }

    /// REQ: PKG-126
    #[test]
    fn write_package_accepts_exactly_4096_entries_and_reads_them_back() {
        let mut files = vector_manifest_and_component();
        for index in 1..=4094 {
            files.push((format!("assets/f{index:04}.bin"), Vec::new()));
        }
        assert_eq!(files.len(), 4096);
        let bytes = write_package(&files).expect("4096 entries is the accepted maximum");
        let archive = read_package(&bytes).expect("the maximum-size archive is readable");
        assert_eq!(archive.entries.len(), 4095);
    }

    /// REQ: PKG-127
    #[test]
    fn write_package_refuses_a_manifest_over_1_mib() {
        let files = vec![("manifest.json".to_owned(), vec![b' '; 1_048_577])];
        assert_eq!(write_refusal_code(&files), "package.manifest-size");
    }
}
