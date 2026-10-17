//! reference-package-format.md, hostile-archive matrix (PKG-157..182). Each row builds the
//! archive named by the row and asserts the exact ordered list of error codes.

use super::support::*;

fn only(code: &str) -> Vec<String> {
    strings(&[code])
}

fn sorted(mut entries: Vec<Z>) -> Vec<Z> {
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    entries
}

fn vector_entries_z() -> Vec<Z> {
    vec![
        Z::new("manifest.json", &vector_manifest()),
        Z::new("server.wasm", &vector_component()),
    ]
}

/// REQ: PKG-157
#[test]
fn pkg_157_zip_slip_entry_refused_path_invalid() {
    let mut entries = vector_entries_z();
    entries.insert(0, Z::new("../evil.wasm", b"x"));
    assert_eq!(
        check_package_codes(&build(&entries, &Tail::default())),
        only("package.path-invalid")
    );
}

/// REQ: PKG-157
#[test]
fn pkg_157_zip_slip_in_a_folder_refused_path_invalid() {
    let entries = sorted({
        let mut v = vector_entries_z();
        v.push(Z::new("assets/../../evil", b"x"));
        v
    });
    assert_eq!(
        check_package_codes(&build(&entries, &Tail::default())),
        only("package.path-invalid")
    );
}

/// REQ: PKG-158
#[test]
fn pkg_158_absolute_path_entry_refused_path_invalid() {
    let mut entries = vector_entries_z();
    entries.insert(0, Z::new("/etc/passwd", b"x"));
    assert_eq!(
        check_package_codes(&build(&entries, &Tail::default())),
        only("package.path-invalid")
    );
}

/// REQ: PKG-159
#[test]
fn pkg_159_backslash_entry_refused_path_invalid() {
    let entries = sorted({
        let mut v = vector_entries_z();
        v.push(Z::new("assets\\x.png", b"x"));
        v
    });
    assert_eq!(
        check_package_codes(&build(&entries, &Tail::default())),
        only("package.path-invalid")
    );
}

/// REQ: PKG-160
#[test]
fn pkg_160_nul_byte_entry_refused_path_invalid() {
    let entries = sorted({
        let mut v = vector_entries_z();
        v.push(Z::raw(b"assets/a\x00b.png", b"x"));
        v
    });
    assert_eq!(
        check_package_codes(&build(&entries, &Tail::default())),
        only("package.path-invalid")
    );
}

/// REQ: PKG-161
#[test]
fn pkg_161_server_and_capital_server_refused_unexpected_then_case_collision() {
    let entries = vec![
        Z::new("Server.wasm", &vector_component()),
        Z::new("manifest.json", &vector_manifest()),
        Z::new("server.wasm", &vector_component()),
    ];
    let mut sorted_entries = entries;
    sorted_entries.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(
        stage1_codes(&build(&sorted_entries, &Tail::default())),
        strings(&["package.entry-unexpected", "package.path-case-collision"])
    );
}

/// REQ: PKG-162
#[test]
fn pkg_162_same_name_twice_refused_path_duplicate() {
    let mut entries = vector_entries_z();
    entries.push(Z::new("server.wasm", &vector_component()));
    assert_eq!(
        stage1_codes(&build(&entries, &Tail::default())),
        only("package.path-duplicate")
    );
}

/// REQ: PKG-163
#[test]
fn pkg_163_central_offset_into_another_entry_refused_header_mismatch() {
    let mut entries = vector_entries_z();
    entries[1].offset = Some(5);
    assert_eq!(
        stage1_codes(&build(&entries, &Tail::default())),
        only("package.header-mismatch")
    );
}

/// REQ: PKG-164
#[test]
fn pkg_164_one_data_byte_changed_with_crc_unchanged_refused_crc_mismatch() {
    let bytes = patch_data(&vector_unsigned(), "server.wasm", 12, 0x7F, false);
    assert_eq!(stage1_codes(&bytes), only("package.crc-mismatch"));
}

/// REQ: PKG-165
#[test]
fn pkg_165_central_directory_cut_in_half_refused_zip_invalid() {
    let bytes = build(
        &vector_entries_z(),
        &Tail {
            cut_cd: true,
            ..Tail::default()
        },
    );
    assert_eq!(stage1_codes(&bytes), only("package.zip-invalid"));
}

/// REQ: PKG-166
#[test]
fn pkg_166_empty_manifest_refused_manifest_json_invalid() {
    let bytes = zip_of(&[
        ("manifest.json", Vec::new()),
        ("server.wasm", vector_component()),
    ]);
    assert_eq!(check_codes(&bytes), only("manifest.json-invalid"));
}

/// REQ: PKG-167
#[test]
fn pkg_167_archive_without_manifest_refused_manifest_missing() {
    let bytes = zip_of(&[("server.wasm", vector_component())]);
    assert_eq!(check_codes(&bytes), only("package.manifest-missing"));
}

/// REQ: PKG-168
#[test]
fn pkg_168_method_8_refused_zip_compressed() {
    let mut entries = vector_entries_z();
    entries[1].method = 8;
    assert_eq!(
        stage1_codes(&build(&entries, &Tail::default())),
        only("package.zip-compressed")
    );
}

/// REQ: PKG-169
#[test]
fn pkg_169_bit_0_set_refused_zip_encrypted() {
    let mut entries = vector_entries_z();
    entries[1].flags = 0x0801;
    assert_eq!(
        stage1_codes(&build(&entries, &Tail::default())),
        only("package.zip-encrypted")
    );
}

/// REQ: PKG-170
#[test]
fn pkg_170_archive_comment_of_4_bytes_refused_zip_comment() {
    let bytes = build(
        &vector_entries_z(),
        &Tail {
            comment: b"abcd".to_vec(),
            ..Tail::default()
        },
    );
    assert_eq!(stage1_codes(&bytes), only("package.zip-comment"));
}

/// REQ: PKG-171
#[test]
fn pkg_171_16_bytes_after_the_end_record_refused_zip_trailing() {
    let bytes = build(
        &vector_entries_z(),
        &Tail {
            trailing: vec![0u8; 16],
            ..Tail::default()
        },
    );
    assert_eq!(stage1_codes(&bytes), only("package.zip-trailing"));
}

/// REQ: PKG-172
#[test]
fn pkg_172_symbolic_link_mode_refused_entry_symlink() {
    let mut entries = vector_entries_z();
    entries[1].external = 0xA1FF_0000;
    assert_eq!(
        stage1_codes(&build(&entries, &Tail::default())),
        only("package.entry-symlink")
    );
}

/// REQ: PKG-173
#[test]
fn pkg_173_directory_entry_refused_entry_directory() {
    let mut entries = vector_entries_z();
    entries.insert(0, Z::new("assets/", b""));
    assert_eq!(
        stage1_codes(&build(&entries, &Tail::default())),
        only("package.entry-directory")
    );
}

/// REQ: PKG-174
#[test]
fn pkg_174_unlisted_top_level_file_refused_entry_unexpected() {
    let entries = sorted({
        let mut v = vector_entries_z();
        v.push(Z::new("extra.txt", b"x"));
        v
    });
    assert_eq!(
        stage1_codes(&build(&entries, &Tail::default())),
        only("package.entry-unexpected")
    );
}

/// REQ: PKG-175
#[test]
fn pkg_175_manifest_and_server_swapped_refused_entry_order() {
    let entries = vec![
        Z::new("server.wasm", &vector_component()),
        Z::new("manifest.json", &vector_manifest()),
    ];
    assert_eq!(
        stage1_codes(&build(&entries, &Tail::default())),
        only("package.entry-order")
    );
}

/// REQ: PKG-176
#[test]
fn pkg_176_4097_entries_refused_entry_limit() {
    let mut entries = vector_entries_z();
    for index in 0..4095 {
        entries.push(Z::new(&format!("assets/f{index:04}.bin"), b""));
    }
    let entries = sorted(entries);
    assert_eq!(entries.len(), 4097);
    assert_eq!(
        stage1_codes(&build(&entries, &Tail::default())),
        only("package.entry-limit")
    );
}

/// REQ: PKG-177
#[test]
fn pkg_177_core_module_instead_of_component_refused_component_invalid() {
    let bytes = package_with_component(&core_module_with(&[api_section(&[0, 0, 0, 1, 0, 0])]));
    assert_eq!(check_codes(&bytes), only("package.component-invalid"));
}

/// REQ: PKG-178
#[test]
fn pkg_178_duplicated_api_section_refused_api_section_repeated() {
    let section = api_section(&[0, 0, 0, 1, 0, 0]);
    let bytes = package_with_component(&component_with(&[section.clone(), section]));
    assert_eq!(check_codes(&bytes), only("package.api-section-repeated"));
}

/// REQ: PKG-179
#[test]
fn pkg_179_five_byte_payload_refused_api_section_invalid() {
    let bytes = package_with_component(&component_with(&[api_section(&[0, 0, 0, 1, 0])]));
    assert_eq!(check_codes(&bytes), only("package.api-section-invalid"));
}

/// REQ: PKG-180
#[test]
fn pkg_180_payload_0_0_2_0_refused_api_version() {
    let bytes = package_with_component(&component_with(&[api_section(&[0, 0, 0, 2, 0, 0])]));
    assert_eq!(check_codes(&bytes), only("manifest.api-version"));
}

/// REQ: PKG-181
#[test]
fn pkg_181_changed_component_byte_with_matching_crc_refused_file_hash() {
    let bytes = patch_data(&vector_unsigned(), "server.wasm", 33, 0x01, true);
    assert_eq!(check_codes(&bytes), only("manifest.file-hash"));
}

/// REQ: PKG-182
#[test]
fn pkg_182_manifest_letter_changed_in_a_signed_package_refused_signature_invalid() {
    let signed = vector_signed();
    let manifest = data_of(&signed, "manifest.json");
    let at = String::from_utf8(manifest)
        .expect("utf8")
        .find("\"Minimal\"")
        .expect("name")
        + 1;
    let bytes = patch_data(&signed, "manifest.json", at, b'N', true);
    let report = check(&bytes);
    assert_eq!(
        error_codes(&report.diagnostics),
        only("manifest.signature-invalid")
    );
    assert_eq!(report.last_stage, 7);
}

/// REQ: PKG-182
#[test]
fn pkg_182_the_unchanged_signed_vector_passes_stage_7() {
    let report = check(&vector_signed());
    assert!(!error_codes(&report.diagnostics).contains(&"manifest.signature-invalid".to_string()));
}

/// REQ: PKG-182
#[test]
fn pkg_182_changed_manifest_without_the_crc_repair_stops_at_stage_1() {
    let signed = vector_signed();
    let bytes = patch_data(&signed, "manifest.json", 20, b'Z', false);
    assert_eq!(check_codes(&bytes), only("package.crc-mismatch"));
}
