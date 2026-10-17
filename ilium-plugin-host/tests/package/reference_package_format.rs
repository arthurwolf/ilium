//! reference-package-format.md, part 1: the container (PKG-100..114), paths (PKG-115..122),
//! reserved names and folders (PKG-123..125) and size caps (PKG-126..132).
//!
//! Stage 1 rows use `read_package` and look at the `package.*` codes it prints; rows that need
//! a later stage use the installer check (see the other modules).

use super::support::*;
use ilium_plugin_host::package::{read_package, write_package};
use proptest::prelude::*;

const MIB: u32 = 1024 * 1024;

fn base() -> Vec<Z> {
    vec![
        Z::new("manifest.json", &vector_manifest()),
        Z::new("server.wasm", &vector_component()),
    ]
}

/// The vector with one entry changed by `change`.
fn changed<F: FnOnce(&mut Z)>(index: usize, change: F) -> Vec<u8> {
    let mut entries = base();
    change(&mut entries[index]);
    build(&entries, &Tail::default())
}

fn with_tail(tail: Tail) -> Vec<u8> {
    build(&base(), &tail)
}

fn one_code(code: &str) -> Vec<String> {
    strings(&[code])
}

fn is_refused_as_package_error(bytes: &[u8]) -> bool {
    match read_package(bytes) {
        Ok(_) => false,
        Err(diagnostics) => {
            !diagnostics.is_empty()
                && diagnostics
                    .iter()
                    .all(|diagnostic| diagnostic.code.as_str().starts_with("package."))
        }
    }
}

fn many_assets(count: usize) -> Vec<u8> {
    let mut entries = base();
    for index in 1..=count {
        entries.push(Z::new(&format!("assets/f{index:04}.bin"), b""));
    }
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    build(&entries, &Tail::default())
}

// ---------------------------------------------------------------------------------------------
// The container
// ---------------------------------------------------------------------------------------------

/// REQ: PKG-100, PKG-146
#[test]
fn pkg_100_stored_entries_of_the_vector_are_read() {
    let archive = read_package(&vector_unsigned()).expect("the manual's vector must be accepted");
    assert_eq!(archive.manifest.id.as_str(), "example.minimal");
}

/// REQ: PKG-100
#[test]
fn pkg_100_method_deflate_refused_zip_compressed() {
    let bytes = changed(1, |entry| entry.method = 8);
    assert_eq!(stage1_codes(&bytes), one_code("package.zip-compressed"));
}

/// REQ: PKG-100
#[test]
fn pkg_100_method_other_than_zero_refused_zip_compressed() {
    for method in [1u16, 9, 12, 93] {
        let bytes = changed(0, |entry| entry.method = method);
        assert_eq!(
            stage1_codes(&bytes),
            one_code("package.zip-compressed"),
            "method {method}"
        );
    }
}

/// REQ: PKG-101
#[test]
fn pkg_101_utf8_flag_set_on_every_entry_is_accepted() {
    let bytes = build(&base(), &Tail::default());
    assert_eq!(
        central_fields(&bytes)
            .iter()
            .map(|f| f.flags)
            .collect::<Vec<_>>(),
        vec![0x0800, 0x0800]
    );
    assert!(stage1_codes(&bytes).is_empty());
}

/// REQ: PKG-101
#[test]
fn pkg_101_bit_zero_encryption_refused_zip_encrypted() {
    let bytes = changed(1, |entry| entry.flags = 0x0801);
    assert_eq!(stage1_codes(&bytes), one_code("package.zip-encrypted"));
}

/// REQ: PKG-101
#[test]
fn pkg_101_bit_three_data_descriptor_refused_zip_descriptor() {
    let bytes = changed(1, |entry| entry.flags = 0x0808);
    assert_eq!(stage1_codes(&bytes), one_code("package.zip-descriptor"));
}

/// REQ: PKG-101
#[test]
fn pkg_101_missing_utf8_flag_refused() {
    let bytes = changed(0, |entry| entry.flags = 0x0000);
    assert!(is_refused_as_package_error(&bytes));
}

/// REQ: PKG-101
#[test]
fn pkg_101_any_other_flag_bit_refused() {
    for flags in [0x0802u16, 0x0804, 0x0810, 0x0820, 0x1800, 0x8800] {
        let bytes = changed(0, |entry| entry.flags = flags);
        assert!(is_refused_as_package_error(&bytes), "flags {flags:#06x}");
    }
}

/// REQ: PKG-102
#[test]
fn pkg_102_size_field_ffffffff_refused_zip64() {
    let bytes = changed(1, |entry| {
        entry.size = Some(0xFFFF_FFFF);
        entry.local_size = Some(0xFFFF_FFFF);
    });
    assert_eq!(
        stage1_codes(&bytes).first().map(String::as_str),
        Some("package.zip64")
    );
}

/// REQ: PKG-102
#[test]
fn pkg_102_zip64_locator_refused_zip64() {
    let bytes = with_tail(Tail {
        zip64_locator: true,
        ..Tail::default()
    });
    assert_eq!(
        stage1_codes(&bytes).first().map(String::as_str),
        Some("package.zip64")
    );
}

/// REQ: PKG-103
#[test]
fn pkg_103_local_extra_field_refused_zip_extra() {
    let bytes = changed(0, |entry| entry.local_extra = vec![0xCA, 0xFE, 0x00, 0x00]);
    assert_eq!(stage1_codes(&bytes), one_code("package.zip-extra"));
}

/// REQ: PKG-103
#[test]
fn pkg_103_central_extra_field_refused_zip_extra() {
    let bytes = changed(1, |entry| {
        entry.central_extra = vec![0xCA, 0xFE, 0x00, 0x00]
    });
    assert_eq!(stage1_codes(&bytes), one_code("package.zip-extra"));
}

/// REQ: PKG-103
#[test]
fn pkg_103_extra_length_zero_everywhere_is_accepted() {
    let bytes = build(&base(), &Tail::default());
    assert!(central_fields(&bytes)
        .iter()
        .all(|f| f.extra_len == 0 && f.local_extra_len == 0));
    assert!(stage1_codes(&bytes).is_empty());
}

/// REQ: PKG-104
#[test]
fn pkg_104_end_record_comment_refused_zip_comment() {
    let bytes = with_tail(Tail {
        comment: b"note".to_vec(),
        ..Tail::default()
    });
    assert_eq!(stage1_codes(&bytes), one_code("package.zip-comment"));
}

/// REQ: PKG-104
#[test]
fn pkg_104_entry_comment_refused_zip_comment() {
    let bytes = changed(1, |entry| entry.comment = b"hi".to_vec());
    assert_eq!(stage1_codes(&bytes), one_code("package.zip-comment"));
}

/// REQ: PKG-104
#[test]
fn pkg_104_one_byte_end_record_comment_refused_zip_comment() {
    let bytes = with_tail(Tail {
        comment: b"x".to_vec(),
        ..Tail::default()
    });
    assert_eq!(stage1_codes(&bytes), one_code("package.zip-comment"));
}

/// REQ: PKG-105
#[test]
fn pkg_105_directory_entry_refused_entry_directory() {
    let mut entries = base();
    entries.insert(0, Z::new("assets/", b""));
    let bytes = build(&entries, &Tail::default());
    assert_eq!(stage1_codes(&bytes), one_code("package.entry-directory"));
}

/// REQ: PKG-105
#[test]
fn pkg_105_top_level_directory_entry_refused_entry_directory() {
    let mut entries = base();
    entries.insert(1, Z::new("help/", b""));
    let bytes = build(&entries, &Tail::default());
    assert_eq!(stage1_codes(&bytes), one_code("package.entry-directory"));
}

/// REQ: PKG-106
#[test]
fn pkg_106_written_entries_carry_the_1980_epoch() {
    let bytes = write_package(&vector_entries()).expect("write the vector");
    for fields in central_fields(&bytes) {
        assert_eq!((fields.time, fields.date), (0x0000, 0x0021));
    }
}

/// REQ: PKG-106
#[test]
fn pkg_106_other_date_refused() {
    let bytes = changed(0, |entry| entry.date = 0x5A21);
    assert!(is_refused_as_package_error(&bytes));
}

/// REQ: PKG-106
#[test]
fn pkg_106_other_time_refused() {
    let bytes = changed(1, |entry| entry.time = 0x0001);
    assert!(is_refused_as_package_error(&bytes));
}

/// REQ: PKG-107
#[test]
fn pkg_107_written_entries_have_mode_0644() {
    let bytes = write_package(&vector_entries()).expect("write the vector");
    for fields in central_fields(&bytes) {
        assert_eq!(fields.external, 0x81A4_0000);
    }
}

/// REQ: PKG-107
#[test]
fn pkg_107_symbolic_link_mode_refused_entry_symlink() {
    let bytes = changed(1, |entry| entry.external = 0xA1FF_0000);
    assert_eq!(stage1_codes(&bytes), one_code("package.entry-symlink"));
}

/// REQ: PKG-107
#[test]
fn pkg_107_device_mode_refused_entry_symlink() {
    let bytes = changed(1, |entry| entry.external = 0x61A4_0000);
    assert_eq!(stage1_codes(&bytes), one_code("package.entry-symlink"));
}

/// REQ: PKG-107
#[test]
fn pkg_107_executable_mode_refused_entry_symlink() {
    let bytes = changed(0, |entry| entry.external = 0x81ED_0000);
    assert_eq!(stage1_codes(&bytes), one_code("package.entry-symlink"));
}

/// REQ: PKG-108
#[test]
fn pkg_108_written_headers_use_unix_2_0_values() {
    let bytes = write_package(&vector_entries()).expect("write the vector");
    for fields in central_fields(&bytes) {
        assert_eq!(
            (fields.made, fields.needed, fields.local_needed),
            (0x0314, 20, 20)
        );
        assert_eq!((fields.disk, fields.internal), (0, 0));
    }
}

/// REQ: PKG-108
#[test]
fn pkg_108_other_version_made_by_refused() {
    let bytes = changed(0, |entry| entry.made = 0x031E);
    assert!(is_refused_as_package_error(&bytes));
}

/// REQ: PKG-108
#[test]
fn pkg_108_other_version_needed_refused() {
    let bytes = changed(0, |entry| entry.needed = 21);
    assert!(is_refused_as_package_error(&bytes));
}

/// REQ: PKG-108
#[test]
fn pkg_108_nonzero_disk_number_refused() {
    let bytes = changed(1, |entry| entry.disk = 1);
    assert!(is_refused_as_package_error(&bytes));
}

/// REQ: PKG-108
#[test]
fn pkg_108_nonzero_internal_attributes_refused() {
    let bytes = changed(1, |entry| entry.internal = 1);
    assert!(is_refused_as_package_error(&bytes));
}

/// REQ: PKG-109
#[test]
fn pkg_109_local_name_differs_refused_header_mismatch() {
    let bytes = changed(1, |entry| entry.local_name = Some(b"server.wasn".to_vec()));
    assert_eq!(stage1_codes(&bytes), one_code("package.header-mismatch"));
}

/// REQ: PKG-109
#[test]
fn pkg_109_local_crc_differs_refused_header_mismatch() {
    let bytes = changed(1, |entry| entry.local_crc = Some(0x1234_5678));
    assert_eq!(stage1_codes(&bytes), one_code("package.header-mismatch"));
}

/// REQ: PKG-109
#[test]
fn pkg_109_local_size_differs_refused_header_mismatch() {
    let bytes = changed(1, |entry| entry.local_size = Some(33));
    assert_eq!(stage1_codes(&bytes), one_code("package.header-mismatch"));
}

/// REQ: PKG-109
#[test]
fn pkg_109_central_size_differs_refused_header_mismatch() {
    let bytes = changed(1, |entry| entry.size = Some(35));
    assert_eq!(stage1_codes(&bytes), one_code("package.header-mismatch"));
}

/// REQ: PKG-109
#[test]
fn pkg_109_agreeing_headers_are_accepted() {
    assert!(stage1_codes(&build(&base(), &Tail::default())).is_empty());
}

/// REQ: PKG-110
#[test]
fn pkg_110_wrong_crc_in_both_headers_refused_crc_mismatch() {
    let bytes = patch_data(&vector_unsigned(), "server.wasm", 10, 0x55, false);
    assert_eq!(stage1_codes(&bytes), one_code("package.crc-mismatch"));
}

/// REQ: PKG-110
#[test]
fn pkg_110_crc_is_checked_before_the_content_is_parsed() {
    let mut garbage = vector_manifest();
    garbage[0] = b'#';
    let mut entry = Z::new("manifest.json", &garbage);
    entry.crc = Some(crc32(&vector_manifest()));
    entry.local_crc = Some(crc32(&vector_manifest()));
    let bytes = build(
        &[entry, Z::new("server.wasm", &vector_component())],
        &Tail::default(),
    );
    let codes = check_codes(&bytes);
    assert_eq!(codes, one_code("package.crc-mismatch"));
}

/// REQ: PKG-110
#[test]
fn pkg_110_correct_crc_is_accepted() {
    let bytes = patch_data(&vector_unsigned(), "server.wasm", 10, 0x55, true);
    assert!(stage1_codes(&bytes).is_empty());
}

/// REQ: PKG-111
#[test]
fn pkg_111_bytes_before_the_first_local_header_refused_zip_trailing() {
    let bytes = with_tail(Tail {
        leading: vec![0xAA; 4],
        ..Tail::default()
    });
    assert_eq!(stage1_codes(&bytes), one_code("package.zip-trailing"));
}

/// REQ: PKG-111
#[test]
fn pkg_111_bytes_between_records_refused_zip_trailing() {
    let bytes = changed(0, |entry| entry.gap_after = vec![0x00; 3]);
    assert_eq!(stage1_codes(&bytes), one_code("package.zip-trailing"));
}

/// REQ: PKG-111
#[test]
fn pkg_111_bytes_between_last_entry_and_directory_refused_zip_trailing() {
    let bytes = with_tail(Tail {
        gap_before_cd: vec![0x00; 2],
        ..Tail::default()
    });
    assert_eq!(stage1_codes(&bytes), one_code("package.zip-trailing"));
}

/// REQ: PKG-111
#[test]
fn pkg_111_bytes_after_the_end_record_refused_zip_trailing() {
    let bytes = with_tail(Tail {
        trailing: vec![0x00; 16],
        ..Tail::default()
    });
    assert_eq!(stage1_codes(&bytes), one_code("package.zip-trailing"));
}

/// REQ: PKG-112
#[test]
fn pkg_112_central_offset_into_another_entry_refused_header_mismatch() {
    let bytes = changed(1, |entry| entry.offset = Some(10));
    assert_eq!(stage1_codes(&bytes), one_code("package.header-mismatch"));
}

/// REQ: PKG-112
#[test]
fn pkg_112_central_order_differs_from_local_order_refused_header_mismatch() {
    let bytes = with_tail(Tail {
        cd_order: Some(vec![1, 0]),
        ..Tail::default()
    });
    assert_eq!(
        stage1_codes(&bytes).first().map(String::as_str),
        Some("package.header-mismatch")
    );
}

/// REQ: PKG-112
#[test]
fn pkg_112_offset_one_past_the_next_unread_byte_refused_header_mismatch() {
    let first_len = 30 + "manifest.json".len() + vector_manifest().len();
    let bytes = changed(1, |entry| entry.offset = Some(first_len as u32 + 1));
    assert_eq!(stage1_codes(&bytes), one_code("package.header-mismatch"));
}

/// REQ: PKG-113
#[test]
fn pkg_113_entry_count_too_large_refused_zip_invalid() {
    let bytes = with_tail(Tail {
        count: Some(3),
        ..Tail::default()
    });
    assert_eq!(stage1_codes(&bytes), one_code("package.zip-invalid"));
}

/// REQ: PKG-113
#[test]
fn pkg_113_entry_count_too_small_refused_zip_invalid() {
    let bytes = with_tail(Tail {
        count: Some(1),
        ..Tail::default()
    });
    assert_eq!(stage1_codes(&bytes), one_code("package.zip-invalid"));
}

/// REQ: PKG-113
#[test]
fn pkg_113_directory_size_wrong_refused_zip_invalid() {
    let real = build(&base(), &Tail::default());
    let size = u32::from_le_bytes([
        real[real.len() - 10],
        real[real.len() - 9],
        real[real.len() - 8],
        real[real.len() - 7],
    ]);
    let bytes = with_tail(Tail {
        cd_size: Some(size + 1),
        ..Tail::default()
    });
    assert_eq!(stage1_codes(&bytes), one_code("package.zip-invalid"));
}

/// REQ: PKG-113
#[test]
fn pkg_113_directory_offset_wrong_refused_zip_invalid() {
    let bytes = with_tail(Tail {
        cd_offset: Some(0),
        ..Tail::default()
    });
    assert_eq!(stage1_codes(&bytes), one_code("package.zip-invalid"));
}

/// REQ: PKG-113
#[test]
fn pkg_113_truncated_directory_refused_zip_invalid() {
    let bytes = with_tail(Tail {
        cut_cd: true,
        ..Tail::default()
    });
    assert_eq!(stage1_codes(&bytes), one_code("package.zip-invalid"));
}

/// REQ: PKG-114
#[test]
fn pkg_114_end_record_is_the_last_22_bytes() {
    let bytes = vector_unsigned();
    assert_eq!(&bytes[bytes.len() - 22..bytes.len() - 18], b"PK\x05\x06");
    assert!(stage1_codes(&bytes).is_empty());
}

/// REQ: PKG-114
#[test]
fn pkg_114_wrong_signature_at_minus_22_refused_zip_invalid() {
    let mut bytes = vector_unsigned();
    let at = bytes.len() - 22;
    bytes[at + 3] = 0x07;
    assert_eq!(stage1_codes(&bytes), one_code("package.zip-invalid"));
}

/// REQ: PKG-114
#[test]
fn pkg_114_file_shorter_than_22_bytes_refused_zip_invalid() {
    for length in [0usize, 1, 21] {
        let bytes = vec![0x50u8; length];
        assert_eq!(
            stage1_codes(&bytes),
            one_code("package.zip-invalid"),
            "length {length}"
        );
    }
}

/// REQ: PKG-114
#[test]
fn pkg_114_exactly_22_byte_empty_archive_has_no_end_record_fault() {
    let bytes = build(&[], &Tail::default());
    assert_eq!(bytes.len(), 22);
    assert!(!stage1_codes(&bytes).contains(&"package.zip-invalid".to_string()));
}

/// REQ: PKG-114, PKG-113
#[test]
fn pkg_114_a_lenient_fallback_never_reads_a_shifted_end_record() {
    let mut bytes = vector_unsigned();
    bytes.push(0x00);
    let codes = stage1_codes(&bytes);
    assert!(!codes.is_empty());
}

proptest! {
    // REQ: PKG-114, PKG-113
    #[test]
    fn pkg_114_random_bytes_are_refused_with_a_package_code(bytes in proptest::collection::vec(any::<u8>(), 0..300)) {
        let outcome = read_package(&bytes);
        let diagnostics = match outcome {
            Ok(_) => Vec::new(),
            Err(diagnostics) => diagnostics,
        };
        prop_assert!(!diagnostics.is_empty());
        prop_assert!(diagnostics.iter().all(|d| d.code.as_str().starts_with("package.")));
    }

    // REQ: PKG-110, PKG-109
    #[test]
    fn pkg_110_any_single_byte_flip_of_the_vector_is_refused_without_panic(index in 0usize..658, value in 1u8..=255) {
        let mut bytes = vector_unsigned();
        bytes[index] ^= value;
        let outcome = read_package(&bytes);
        let refused = match outcome {
            Ok(_) => false,
            Err(diagnostics) => !diagnostics.is_empty(),
        };
        prop_assert!(refused);
    }
}

// ---------------------------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------------------------

fn path_codes(path: &[u8]) -> Vec<String> {
    let mut entries = base();
    entries.push(Z::raw(path, b"data"));
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    stage1_codes(&build(&entries, &Tail::default()))
}

fn assert_path_invalid(path: &[u8]) {
    assert_eq!(
        path_codes(path),
        one_code("package.path-invalid"),
        "path {:?}",
        String::from_utf8_lossy(path)
    );
}

fn assert_path_ok(path: &str) {
    let codes = check_package_codes(&package_with(&[(path, b"data")]));
    assert!(codes.is_empty(), "path {path:?} gave {codes:?}");
}

/// REQ: PKG-115
#[test]
fn pkg_115_allowed_characters_are_accepted() {
    assert_path_ok("assets/aZ09._-x.png");
    assert_path_ok("assets/A-b_c.9.png");
}

/// REQ: PKG-115, PKG-159
#[test]
fn pkg_115_backslash_separator_refused_path_invalid() {
    assert_path_invalid(b"assets\\x.png");
}

/// REQ: PKG-115, PKG-160
#[test]
fn pkg_115_nul_byte_refused_path_invalid() {
    assert_path_invalid(b"assets/a\x00b.png");
}

/// REQ: PKG-115
#[test]
fn pkg_115_space_refused_path_invalid() {
    assert_path_invalid(b"assets/a b.png");
}

/// REQ: PKG-115
#[test]
fn pkg_115_control_characters_refused_path_invalid() {
    for control in [0x01u8, 0x09, 0x0A, 0x1F, 0x7F] {
        assert_path_invalid(&[b"assets/a".as_slice(), &[control], b"b.png".as_slice()].concat());
    }
}

/// REQ: PKG-115
#[test]
fn pkg_115_non_ascii_refused_path_invalid() {
    assert_path_invalid("assets/é.png".as_bytes());
    assert_path_invalid("assets/\u{202E}x.png".as_bytes());
}

/// REQ: PKG-115
#[test]
fn pkg_115_other_punctuation_refused_path_invalid() {
    for punctuation in [
        "a:b", "a*b", "a?b", "a<b", "a|b", "a\"b", "a#b", "a%b", "a+b", "a@b",
    ] {
        assert_path_invalid(format!("assets/{punctuation}.png").as_bytes());
    }
}

/// REQ: PKG-116, PKG-157
#[test]
fn pkg_116_dot_dot_component_refused_path_invalid() {
    assert_path_invalid(b"../evil.wasm");
    assert_path_invalid(b"assets/../x.png");
}

/// REQ: PKG-116, PKG-158
#[test]
fn pkg_116_leading_slash_refused_path_invalid() {
    assert_path_invalid(b"/etc/passwd");
}

/// REQ: PKG-116
#[test]
fn pkg_116_empty_component_refused_path_invalid() {
    assert_path_invalid(b"assets//x.png");
}

/// REQ: PKG-116
#[test]
fn pkg_116_single_dot_component_refused_path_invalid() {
    assert_path_invalid(b"assets/./x.png");
}

/// REQ: PKG-117
#[test]
fn pkg_117_component_starting_with_dot_refused_path_invalid() {
    assert_path_invalid(b"assets/.hidden");
    assert_path_invalid(b".env");
    assert_path_invalid(b"assets/.x/y.png");
}

/// REQ: PKG-117
#[test]
fn pkg_117_component_ending_with_dot_refused_path_invalid() {
    assert_path_invalid(b"assets/x.");
    assert_path_invalid(b"assets/x./y.png");
}

/// REQ: PKG-117
#[test]
fn pkg_117_dots_inside_a_component_are_accepted() {
    assert_path_ok("assets/x.y.png");
    assert_path_ok("assets/x..y");
}

/// REQ: PKG-118
#[test]
fn pkg_118_windows_reserved_names_refused_path_invalid() {
    for name in ["CON", "PRN", "AUX", "NUL", "COM1", "COM9", "LPT1", "LPT9"] {
        assert_path_invalid(format!("assets/{name}").as_bytes());
    }
}

/// REQ: PKG-118
#[test]
fn pkg_118_reserved_names_with_extension_refused_path_invalid() {
    assert_path_invalid(b"assets/con.txt");
    assert_path_invalid(b"assets/Nul.png");
    assert_path_invalid(b"assets/aux.tar.gz");
}

/// REQ: PKG-118
#[test]
fn pkg_118_reserved_names_in_any_letter_case_refused_path_invalid() {
    for name in ["con", "Con", "cOn", "com5", "Lpt3", "pRn"] {
        assert_path_invalid(format!("assets/{name}").as_bytes());
    }
}

/// REQ: PKG-118
#[test]
fn pkg_118_reserved_name_as_a_folder_component_refused_path_invalid() {
    assert_path_invalid(b"assets/CON/x.png");
}

/// REQ: PKG-118
#[test]
fn pkg_118_names_next_to_reserved_ones_are_accepted() {
    for name in ["COM0", "COM10", "LPT0", "CONSOLE", "NULL", "AUX1", "PRNT"] {
        assert_path_ok(&format!("assets/{name}.png"));
    }
}

/// REQ: PKG-119
#[test]
fn pkg_119_path_of_255_bytes_is_accepted() {
    let path = format!("assets/{}", "a".repeat(248));
    assert_eq!(path.len(), 255);
    assert_path_ok(&path);
}

/// REQ: PKG-119
#[test]
fn pkg_119_path_of_256_bytes_refused_path_invalid() {
    let path = format!("assets/{}", "a".repeat(249));
    assert_eq!(path.len(), 256);
    assert_path_invalid(path.as_bytes());
}

/// REQ: PKG-119
#[test]
fn pkg_119_path_of_254_bytes_is_accepted() {
    assert_path_ok(&format!("assets/{}", "a".repeat(247)));
}

/// REQ: PKG-119
#[test]
fn pkg_119_sixteen_components_are_accepted() {
    let path = format!("assets/{}x.png", "d/".repeat(14));
    assert_eq!(path.split('/').count(), 16);
    assert_path_ok(&path);
}

/// REQ: PKG-119
#[test]
fn pkg_119_seventeen_components_refused_path_invalid() {
    let path = format!("assets/{}x.png", "d/".repeat(15));
    assert_eq!(path.split('/').count(), 17);
    assert_path_invalid(path.as_bytes());
}

/// REQ: PKG-119
#[test]
fn pkg_119_fifteen_components_are_accepted() {
    assert_path_ok(&format!("assets/{}x.png", "d/".repeat(13)));
}

/// REQ: PKG-120
#[test]
fn pkg_120_identical_paths_refused_path_duplicate() {
    let mut entries = base();
    entries.push(Z::new("server.wasm", &vector_component()));
    let bytes = build(&entries, &Tail::default());
    assert_eq!(stage1_codes(&bytes), one_code("package.path-duplicate"));
}

/// REQ: PKG-120
#[test]
fn pkg_120_case_only_collision_refused_path_case_collision() {
    let mut entries = base();
    entries.push(Z::new("assets/A.png", b"1"));
    entries.push(Z::new("assets/a.png", b"2"));
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    let bytes = build(&entries, &Tail::default());
    assert_eq!(
        stage1_codes(&bytes),
        one_code("package.path-case-collision")
    );
}

/// REQ: PKG-120
#[test]
fn pkg_120_case_collision_in_a_folder_component_refused_path_case_collision() {
    let mut entries = base();
    entries.push(Z::new("assets/Dir/x.png", b"1"));
    entries.push(Z::new("assets/dir/x.png", b"2"));
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    let bytes = build(&entries, &Tail::default());
    assert_eq!(
        stage1_codes(&bytes),
        one_code("package.path-case-collision")
    );
}

/// REQ: PKG-120
#[test]
fn pkg_120_distinct_names_are_not_collisions() {
    assert_path_ok("assets/a.png");
    let codes = check_package_codes(&package_with(&[
        ("assets/a.png", b"1"),
        ("assets/b.png", b"2"),
    ]));
    assert!(codes.is_empty());
}

/// REQ: PKG-121
#[test]
fn pkg_121_file_that_is_also_a_folder_refused_path_invalid() {
    let mut entries = base();
    entries.push(Z::new("assets/x", b"1"));
    entries.push(Z::new("assets/x/y.png", b"2"));
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    let bytes = build(&entries, &Tail::default());
    assert_eq!(stage1_codes(&bytes), one_code("package.path-invalid"));
}

/// REQ: PKG-121
#[test]
fn pkg_121_similar_prefix_without_slash_is_accepted() {
    let codes = check_package_codes(&package_with(&[("assets/x", b"1"), ("assets/x0", b"2")]));
    assert!(codes.is_empty());
}

/// REQ: PKG-122
#[test]
fn pkg_122_ascii_order_is_accepted() {
    let codes = check_package_codes(&package_with(&[
        ("assets/Z.png", b"1"),
        ("assets/a.png", b"2"),
    ]));
    assert!(codes.is_empty());
}

/// REQ: PKG-122, PKG-175
#[test]
fn pkg_122_server_before_manifest_refused_entry_order() {
    let entries = vec![
        Z::new("server.wasm", &vector_component()),
        Z::new("manifest.json", &vector_manifest()),
    ];
    let bytes = build(&entries, &Tail::default());
    assert_eq!(stage1_codes(&bytes), one_code("package.entry-order"));
}

/// REQ: PKG-122
#[test]
fn pkg_122_lowercase_before_uppercase_refused_entry_order() {
    let mut entries = base();
    entries.insert(0, Z::new("assets/a.png", b"2"));
    entries.insert(1, Z::new("assets/Z.png", b"1"));
    let bytes = build(&entries, &Tail::default());
    assert_eq!(stage1_codes(&bytes), one_code("package.entry-order"));
}

/// REQ: PKG-122
#[test]
fn pkg_122_slash_sorts_before_digits() {
    let accepted = check_package_codes(&package_with(&[
        ("assets/a/b.png", b"1"),
        ("assets/a0.png", b"2"),
    ]));
    assert!(accepted.is_empty());
    let mut entries = base();
    entries.insert(0, Z::new("assets/a0.png", b"2"));
    entries.insert(1, Z::new("assets/a/b.png", b"1"));
    assert_eq!(
        stage1_codes(&build(&entries, &Tail::default())),
        one_code("package.entry-order")
    );
}

/// REQ: PKG-122
#[test]
fn pkg_122_written_archive_is_in_bytewise_order() {
    let files = vec![
        ("assets/Z.png".to_string(), b"1".to_vec()),
        ("assets/a.png".to_string(), b"2".to_vec()),
        ("manifest.json".to_string(), vector_manifest()),
        ("server.wasm".to_string(), vector_component()),
    ];
    let bytes = write_package(&files).expect("write sorted files");
    assert_eq!(
        names_of(&bytes),
        vec![
            "assets/Z.png",
            "assets/a.png",
            "manifest.json",
            "server.wasm"
        ]
    );
}

proptest! {
    // REQ: PKG-122, PKG-120
    #[test]
    fn pkg_122_any_two_names_in_bytewise_order_are_accepted_and_reversed_are_refused(
        left in "assets/x[a-zA-Z0-9._-]{0,10}[a-zA-Z0-9_-]",
        right in "assets/x[a-zA-Z0-9._-]{0,10}[a-zA-Z0-9_-]",
    ) {
        prop_assume!(left.to_ascii_lowercase() != right.to_ascii_lowercase());
        let (first, second) = if left.as_bytes() < right.as_bytes() { (left, right) } else { (right, left) };
        let mut ordered = base();
        ordered.insert(0, Z::new(&first, b"1"));
        ordered.insert(1, Z::new(&second, b"2"));
        prop_assert!(!stage1_codes(&build(&ordered, &Tail::default())).contains(&"package.entry-order".to_string()));
        let mut reversed = base();
        reversed.insert(0, Z::new(&second, b"2"));
        reversed.insert(1, Z::new(&first, b"1"));
        prop_assert!(stage1_codes(&build(&reversed, &Tail::default())).contains(&"package.entry-order".to_string()));
    }
}

// ---------------------------------------------------------------------------------------------
// Reserved names and allowed folders
// ---------------------------------------------------------------------------------------------

fn location_codes(path: &str) -> Vec<String> {
    let mut entries = base();
    entries.push(Z::new(path, b"data"));
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    stage1_codes(&build(&entries, &Tail::default()))
}

/// REQ: PKG-123
#[test]
fn pkg_123_listed_names_are_accepted() {
    for path in [
        "README.md",
        "CHANGELOG.md",
        "LICENSE",
        "NOTICE",
        "sbom.json",
        "build-info.json",
        "signature",
        "client.wasm",
        "scene.wasm",
        "assets/a/b/c.bin",
        "src/lib.rs",
        "src/deep/er/x.map",
    ] {
        assert!(location_codes(path).is_empty(), "path {path}");
    }
}

/// REQ: PKG-123, PKG-174
#[test]
fn pkg_123_unlisted_top_level_file_refused_entry_unexpected() {
    assert_eq!(
        location_codes("extra.txt"),
        one_code("package.entry-unexpected")
    );
}

/// REQ: PKG-123
#[test]
fn pkg_123_unlisted_top_level_folder_refused_entry_unexpected() {
    assert_eq!(
        location_codes("docs/readme.md"),
        one_code("package.entry-unexpected")
    );
}

/// REQ: PKG-123
#[test]
fn pkg_123_names_differing_only_in_case_from_listed_ones_refused_entry_unexpected() {
    assert_eq!(
        location_codes("readme.md"),
        one_code("package.entry-unexpected")
    );
    assert_eq!(
        location_codes("Server.wasm"),
        one_code("package.entry-unexpected")
    );
}

/// REQ: PKG-123
#[test]
fn pkg_123_wasm_with_another_name_refused_entry_unexpected() {
    assert_eq!(
        location_codes("plugin.wasm"),
        one_code("package.entry-unexpected")
    );
}

/// REQ: PKG-124
#[test]
fn pkg_124_help_holds_only_markdown() {
    assert!(location_codes("help/topic.md").is_empty());
    assert_eq!(
        location_codes("help/topic.txt"),
        one_code("package.entry-unexpected")
    );
}

/// REQ: PKG-124
#[test]
fn pkg_124_sounds_hold_only_wav_and_ogg() {
    assert!(location_codes("sounds/a.wav").is_empty());
    assert!(location_codes("sounds/a.ogg").is_empty());
    assert_eq!(
        location_codes("sounds/a.mp3"),
        one_code("package.entry-unexpected")
    );
}

/// REQ: PKG-124
#[test]
fn pkg_124_screenshots_hold_only_png() {
    assert!(location_codes("screenshots/a.png").is_empty());
    assert_eq!(
        location_codes("screenshots/a.jpg"),
        one_code("package.entry-unexpected")
    );
}

/// REQ: PKG-124
#[test]
fn pkg_124_flat_folders_refuse_nested_folders() {
    assert_eq!(
        location_codes("help/sub/a.md"),
        one_code("package.entry-unexpected")
    );
    assert_eq!(
        location_codes("sounds/x/y.wav"),
        one_code("package.entry-unexpected")
    );
    assert_eq!(
        location_codes("screenshots/s/a.png"),
        one_code("package.entry-unexpected")
    );
}

/// REQ: PKG-125
#[test]
fn pkg_125_server_wasm_declared_as_the_client_entry_refused_entry_invalid() {
    let component = vector_component();
    let manifest = manifest_text(
        "",
        "client",
        "server.wasm",
        &[("server.wasm", component.as_slice())],
    );
    let bytes = zip_of(&[("manifest.json", manifest), ("server.wasm", component)]);
    assert!(check_codes(&bytes).contains(&"manifest.entry-invalid".to_string()));
}

/// REQ: PKG-125
#[test]
fn pkg_125_client_wasm_declared_as_the_server_entry_refused_entry_invalid() {
    let component = vector_component();
    let manifest = manifest_text(
        "",
        "server",
        "client.wasm",
        &[("client.wasm", component.as_slice())],
    );
    let bytes = zip_of(&[("client.wasm", component), ("manifest.json", manifest)]);
    assert!(check_codes(&bytes).contains(&"manifest.entry-invalid".to_string()));
}

/// REQ: PKG-125
#[test]
fn pkg_125_component_named_after_its_side_is_accepted() {
    let component = vector_component();
    let manifest = manifest_text(
        "",
        "client",
        "client.wasm",
        &[("client.wasm", component.as_slice())],
    );
    let bytes = zip_of(&[("client.wasm", component), ("manifest.json", manifest)]);
    assert!(!check_codes(&bytes).contains(&"manifest.entry-invalid".to_string()));
}

// ---------------------------------------------------------------------------------------------
// Size caps
// ---------------------------------------------------------------------------------------------

fn claiming_codes(name: &str, claimed: u32) -> Vec<String> {
    let mut entries = base();
    entries.push(Z::claiming(name, claimed));
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    stage1_codes(&build(&entries, &Tail::default()))
}

/// REQ: PKG-126, PKG-176
#[test]
fn pkg_126_4097_entries_refused_entry_limit() {
    let bytes = many_assets(4095);
    assert_eq!(names_of(&bytes).len(), 4097);
    assert_eq!(stage1_codes(&bytes), one_code("package.entry-limit"));
}

/// REQ: PKG-126
#[test]
fn pkg_126_4096_entries_are_accepted() {
    let bytes = many_assets(4094);
    assert_eq!(names_of(&bytes).len(), 4096);
    assert!(!stage1_codes(&bytes).contains(&"package.entry-limit".to_string()));
}

/// REQ: PKG-126
#[test]
fn pkg_126_4095_entries_are_accepted() {
    let bytes = many_assets(4093);
    assert!(!stage1_codes(&bytes).contains(&"package.entry-limit".to_string()));
}

/// REQ: PKG-127
#[test]
fn pkg_127_manifest_of_1_mib_is_not_oversize() {
    let bytes = build(
        &[
            Z::new("manifest.json", &vec![b' '; 1_048_576]),
            Z::new("server.wasm", &vector_component()),
        ],
        &Tail::default(),
    );
    assert!(!stage1_codes(&bytes).contains(&"package.manifest-size".to_string()));
}

/// REQ: PKG-127
#[test]
fn pkg_127_manifest_one_byte_over_1_mib_refused_manifest_size() {
    let bytes = build(
        &[
            Z::new("manifest.json", &vec![b' '; 1_048_577]),
            Z::new("server.wasm", &vector_component()),
        ],
        &Tail::default(),
    );
    assert_eq!(stage1_codes(&bytes), one_code("package.manifest-size"));
}

/// REQ: PKG-127
#[test]
fn pkg_127_manifest_one_byte_under_1_mib_is_not_oversize() {
    let bytes = build(
        &[
            Z::new("manifest.json", &vec![b' '; 1_048_575]),
            Z::new("server.wasm", &vector_component()),
        ],
        &Tail::default(),
    );
    assert!(!stage1_codes(&bytes).contains(&"package.manifest-size".to_string()));
}

/// REQ: PKG-127
#[test]
fn pkg_127_manifest_cap_is_checked_from_the_central_directory() {
    let mut oversize = Z::claiming("manifest.json", 1_048_577);
    oversize.data = vector_manifest();
    let bytes = build(
        &[oversize, Z::new("server.wasm", &vector_component())],
        &Tail::default(),
    );
    assert!(stage1_codes(&bytes).contains(&"package.manifest-size".to_string()));
}

/// REQ: PKG-128
#[test]
fn pkg_128_component_over_64_mib_refused_size_limit() {
    let mut entries = base();
    entries[1] = Z::claiming("server.wasm", 64 * MIB + 1);
    let codes = stage1_codes(&build(&entries, &Tail::default()));
    assert!(
        codes.contains(&"package.size-limit".to_string()),
        "{codes:?}"
    );
}

/// REQ: PKG-128
#[test]
fn pkg_128_component_of_exactly_64_mib_is_not_over_the_cap() {
    let mut entries = base();
    entries[1] = Z::claiming("server.wasm", 64 * MIB);
    let codes = stage1_codes(&build(&entries, &Tail::default()));
    assert!(
        !codes.contains(&"package.size-limit".to_string()),
        "{codes:?}"
    );
}

/// REQ: PKG-128
#[test]
fn pkg_128_component_one_byte_under_64_mib_is_not_over_the_cap() {
    let mut entries = base();
    entries[1] = Z::claiming("server.wasm", 64 * MIB - 1);
    assert!(!stage1_codes(&build(&entries, &Tail::default()))
        .contains(&"package.size-limit".to_string()));
}

/// REQ: PKG-129
#[test]
fn pkg_129_non_component_entry_over_64_mib_refused_size_limit() {
    let codes = claiming_codes("src/big.bin", 64 * MIB + 1);
    assert!(
        codes.contains(&"package.size-limit".to_string()),
        "{codes:?}"
    );
}

/// REQ: PKG-129
#[test]
fn pkg_129_non_component_entry_of_exactly_64_mib_is_not_over_the_cap() {
    let codes = claiming_codes("src/big.bin", 64 * MIB);
    assert!(
        !codes.contains(&"package.size-limit".to_string()),
        "{codes:?}"
    );
}

/// REQ: PKG-129
#[test]
fn pkg_129_screenshot_over_64_mib_refused_size_limit() {
    let codes = claiming_codes("screenshots/big.png", 64 * MIB + 1);
    assert!(
        codes.contains(&"package.size-limit".to_string()),
        "{codes:?}"
    );
}

/// REQ: PKG-130
#[test]
fn pkg_130_package_file_over_300_mib_refused_size_limit() {
    let bytes = vec![0u8; 300 * 1024 * 1024 + 1];
    let codes = stage1_codes(&bytes);
    assert_eq!(
        codes.first().map(String::as_str),
        Some("package.size-limit"),
        "{codes:?}"
    );
}

/// REQ: PKG-130
#[test]
fn pkg_130_small_package_is_not_over_the_file_cap() {
    assert!(!stage1_codes(&vector_unsigned()).contains(&"package.size-limit".to_string()));
}

/// REQ: PKG-131
#[test]
fn pkg_131_signature_of_16_kib_is_not_over_the_cap() {
    let mut entries = base();
    entries.push(Z::new("signature", &vec![b' '; 16 * 1024]));
    assert!(!stage1_codes(&build(&entries, &Tail::default()))
        .contains(&"package.size-limit".to_string()));
}

/// REQ: PKG-131
#[test]
fn pkg_131_signature_one_byte_over_16_kib_refused_size_limit() {
    let mut entries = base();
    entries.push(Z::new("signature", &vec![b' '; 16 * 1024 + 1]));
    let codes = stage1_codes(&build(&entries, &Tail::default()));
    assert!(
        codes.contains(&"package.size-limit".to_string()),
        "{codes:?}"
    );
}

/// REQ: PKG-131
#[test]
fn pkg_131_signature_one_byte_under_16_kib_is_not_over_the_cap() {
    let mut entries = base();
    entries.push(Z::new("signature", &vec![b' '; 16 * 1024 - 1]));
    assert!(!stage1_codes(&build(&entries, &Tail::default()))
        .contains(&"package.size-limit".to_string()));
}

/// Two zero-filled asset files whose sizes add up to `second_len + 40 MiB`, with the inventory
/// built from precomputed hashes so the test does not hash 64 MiB.
fn asset_total_package(second_len: usize, second_crc: u32, second_sha: &str) -> Vec<u8> {
    let first_len = 40 * 1024 * 1024;
    let component = vector_component();
    let listing = vec![
        (
            "assets/a.bin".to_string(),
            first_len,
            "80a3721188e40218b08b26776bc53bdae81e4784fff71d71450a197319cba113".to_string(),
        ),
        (
            "assets/b.bin".to_string(),
            second_len,
            second_sha.to_string(),
        ),
        (
            "server.wasm".to_string(),
            component.len(),
            sha256_hex(&component),
        ),
    ];
    let manifest = manifest_from_listing("", "server", "server.wasm", &listing);
    let mut first = Z::new("assets/a.bin", &[]);
    first.data = vec![0u8; first_len];
    first.crc = Some(0xfeeb63ea);
    first.local_crc = Some(0xfeeb63ea);
    let mut second = Z::new("assets/b.bin", &[]);
    second.data = vec![0u8; second_len];
    second.crc = Some(second_crc);
    second.local_crc = Some(second_crc);
    build(
        &[
            first,
            second,
            Z::new("manifest.json", &manifest),
            Z::new("server.wasm", &component),
        ],
        &Tail::default(),
    )
}

/// REQ: PKG-132
#[test]
fn pkg_132_asset_bytes_over_64_mib_refused_asset_size() {
    let bytes = asset_total_package(
        24 * 1024 * 1024 + 1,
        0xf54b7cb8,
        "febff1fa96dfc494fe4b7b61d096df1ea7cb4ca41d3a081f6600c7c935226080",
    );
    assert!(check_codes(&bytes).contains(&"manifest.asset-size".to_string()));
}

/// REQ: PKG-132
#[test]
fn pkg_132_asset_bytes_of_exactly_64_mib_are_accepted() {
    let bytes = asset_total_package(
        24 * 1024 * 1024,
        0x4bd29f71,
        "95aeaae03b56c171cf88753c821630a3c24f1fcf406cec3e17d56781aa3f8369",
    );
    assert!(!check_codes(&bytes).contains(&"manifest.asset-size".to_string()));
}

/// REQ: PKG-132
#[test]
fn pkg_132_components_and_sources_do_not_count_toward_the_asset_total() {
    let mut entries = base();
    entries.push(Z::claiming("src/a.bin", 40 * MIB));
    entries.push(Z::claiming("screenshots/a.png", 40 * MIB));
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    let codes = check_codes(&build(&entries, &Tail::default()));
    assert!(
        !codes.contains(&"manifest.asset-size".to_string()),
        "{codes:?}"
    );
}
