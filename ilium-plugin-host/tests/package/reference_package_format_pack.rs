//! reference-package-format.md, part 2: the inventory (PKG-133..137), the component preamble and
//! api-version section (PKG-138..142), pack and sign (PKG-143..148), the pipeline (PKG-149..152)
//! and the `check` and `verify` commands (PKG-153..156).

use super::support::*;
use ilium_plugin_host::package::{read_package, write_package};
use ilium_plugin_host::signing::{sign, SignedAt, SigningKey};
use proptest::prelude::*;

fn manifest_with_files(files: &[Listing]) -> Vec<u8> {
    manifest_from_listing("", "server", "server.wasm", files)
}

fn package_of_manifest(manifest: Vec<u8>) -> Vec<u8> {
    zip_of(&[
        ("manifest.json", manifest),
        ("server.wasm", vector_component()),
    ])
}

fn component_listing() -> Listing {
    let component = vector_component();
    (
        "server.wasm".to_string(),
        component.len(),
        sha256_hex(&component),
    )
}

fn replaced(from: &str, to: &str) -> Vec<u8> {
    let source = String::from_utf8(vector_manifest()).expect("utf8");
    assert!(source.contains(from), "vector manifest lacks {from}");
    package_of_manifest(source.replace(from, to).into_bytes())
}

fn has(codes: &[String], code: &str) -> bool {
    codes.iter().any(|candidate| candidate == code)
}

fn signing_key() -> SigningKey {
    SigningKey(vector_seed())
}

// ---------------------------------------------------------------------------------------------
// Inventory
// ---------------------------------------------------------------------------------------------

/// REQ: PKG-133
#[test]
fn pkg_133_vector_files_item_is_accepted() {
    assert!(check_codes(&vector_unsigned()).is_empty());
}

/// REQ: PKG-133
#[test]
fn pkg_133_extra_key_in_a_files_item_refused_key_unknown() {
    let bytes = replaced("\"length\": 34,", "\"length\": 34,\n      \"mode\": 420,");
    assert!(has(&check_codes(&bytes), "manifest.key-unknown"));
}

/// REQ: PKG-133
#[test]
fn pkg_133_missing_key_in_a_files_item_refused_key_missing() {
    let bytes = replaced("      \"length\": 34,\n", "");
    assert!(has(&check_codes(&bytes), "manifest.key-missing"));
}

/// REQ: PKG-133
#[test]
fn pkg_133_length_above_64_mib_refused_value_out_of_range() {
    let mut listing = component_listing();
    listing.1 = 67_108_865;
    let codes = check_codes(&package_of_manifest(manifest_with_files(&[listing])));
    assert!(has(&codes, "manifest.value-out-of-range"), "{codes:?}");
}

/// REQ: PKG-133
#[test]
fn pkg_133_length_of_exactly_64_mib_is_in_range() {
    let mut listing = component_listing();
    listing.1 = 67_108_864;
    let codes = check_codes(&package_of_manifest(manifest_with_files(&[listing])));
    assert!(!has(&codes, "manifest.value-out-of-range"), "{codes:?}");
}

/// REQ: PKG-133
#[test]
fn pkg_133_uppercase_hash_refused_pattern_mismatch() {
    let mut listing = component_listing();
    listing.2 = listing.2.to_uppercase();
    let codes = check_codes(&package_of_manifest(manifest_with_files(&[listing])));
    assert!(has(&codes, "manifest.pattern-mismatch"), "{codes:?}");
}

/// REQ: PKG-133
#[test]
fn pkg_133_hash_of_63_characters_refused_pattern_mismatch() {
    let mut listing = component_listing();
    listing.2.pop();
    let codes = check_codes(&package_of_manifest(manifest_with_files(&[listing])));
    assert!(has(&codes, "manifest.pattern-mismatch"), "{codes:?}");
}

/// REQ: PKG-133
#[test]
fn pkg_133_hash_of_65_characters_refused_pattern_mismatch() {
    let mut listing = component_listing();
    listing.2.push('0');
    let codes = check_codes(&package_of_manifest(manifest_with_files(&[listing])));
    assert!(has(&codes, "manifest.pattern-mismatch"), "{codes:?}");
}

/// REQ: PKG-133
#[test]
fn pkg_133_negative_length_refused_value_out_of_range() {
    let bytes = replaced("\"length\": 34", "\"length\": -1");
    let codes = check_codes(&bytes);
    assert!(
        has(&codes, "manifest.value-out-of-range") || has(&codes, "manifest.type-mismatch"),
        "{codes:?}"
    );
}

/// REQ: PKG-134
#[test]
fn pkg_134_files_in_archive_order_are_accepted() {
    assert!(!has(
        &check_codes(&package_with(&[("assets/a.png", b"1")])),
        "manifest.files-order"
    ));
}

/// REQ: PKG-134
#[test]
fn pkg_134_unsorted_files_refused_files_order() {
    let component = vector_component();
    let listing = vec![
        component_listing(),
        ("assets/a.png".to_string(), 1, sha256_hex(b"1")),
    ];
    let manifest = manifest_with_files(&listing);
    let bytes = zip_of(&[
        ("assets/a.png", b"1".to_vec()),
        ("manifest.json", manifest),
        ("server.wasm", component),
    ]);
    assert!(has(&check_codes(&bytes), "manifest.files-order"));
}

/// REQ: PKG-134
#[test]
fn pkg_134_path_listed_twice_refused() {
    let listing = vec![component_listing(), component_listing()];
    let codes = check_codes(&package_of_manifest(manifest_with_files(&listing)));
    assert!(
        has(&codes, "manifest.files-order") || has(&codes, "manifest.file-duplicate"),
        "{codes:?}"
    );
}

/// REQ: PKG-135
#[test]
fn pkg_135_archive_entry_not_in_files_refused_file_unlisted() {
    let bytes = vector_plus_unlisted(vec![Z::new("assets/x.png", b"1")]);
    assert_eq!(check_codes(&bytes), strings(&["manifest.file-unlisted"]));
}

/// REQ: PKG-135
#[test]
fn pkg_135_files_item_without_an_entry_refused_file_missing() {
    let listing = vec![
        ("assets/gone.png".to_string(), 1, sha256_hex(b"1")),
        component_listing(),
    ];
    let codes = check_codes(&package_of_manifest(manifest_with_files(&listing)));
    assert_eq!(codes, strings(&["manifest.file-missing"]));
}

/// REQ: PKG-135
#[test]
fn pkg_135_files_listing_manifest_json_refused_file_self() {
    let manifest = vector_manifest();
    let listing = vec![
        component_listing(),
        (
            "manifest.json".to_string(),
            manifest.len(),
            sha256_hex(&manifest),
        ),
    ];
    let mut sorted = listing;
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let codes = check_codes(&package_of_manifest(manifest_with_files(&sorted)));
    assert!(has(&codes, "manifest.file-self"), "{codes:?}");
}

/// REQ: PKG-135
#[test]
fn pkg_135_files_listing_signature_refused_file_self() {
    let listing = vec![
        component_listing(),
        ("signature".to_string(), 1, sha256_hex(b"1")),
    ];
    let mut sorted = listing;
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let codes = check_codes(&package_of_manifest(manifest_with_files(&sorted)));
    assert!(has(&codes, "manifest.file-self"), "{codes:?}");
}

/// REQ: PKG-136
#[test]
fn pkg_136_length_one_less_refused_file_length() {
    let mut listing = component_listing();
    listing.1 -= 1;
    let codes = check_codes(&package_of_manifest(manifest_with_files(&[listing])));
    assert_eq!(codes, strings(&["manifest.file-length"]));
}

/// REQ: PKG-136
#[test]
fn pkg_136_length_one_more_refused_file_length() {
    let mut listing = component_listing();
    listing.1 += 1;
    let codes = check_codes(&package_of_manifest(manifest_with_files(&[listing])));
    assert_eq!(codes, strings(&["manifest.file-length"]));
}

/// REQ: PKG-136
#[test]
fn pkg_136_wrong_hash_refused_file_hash() {
    let mut listing = component_listing();
    listing.2 = "0".repeat(64);
    let codes = check_codes(&package_of_manifest(manifest_with_files(&[listing])));
    assert_eq!(codes, strings(&["manifest.file-hash"]));
}

/// REQ: PKG-136
#[test]
fn pkg_136_exact_length_and_hash_are_accepted() {
    let codes = check_codes(&package_of_manifest(manifest_with_files(&[
        component_listing(),
    ])));
    assert!(codes.is_empty());
}

/// REQ: PKG-137
#[test]
fn pkg_137_packed_manifest_without_files_refused_key_missing() {
    let source = String::from_utf8(vector_manifest()).expect("utf8");
    let cut = source.find(",\n  \"files\"").expect("files key");
    let without = format!("{}\n}}\n", &source[..cut]);
    let report = check(&package_of_manifest(without.into_bytes()));
    let hit = report
        .diagnostics
        .iter()
        .find(|d| d.code.as_str() == "manifest.key-missing")
        .expect("key-missing reported");
    assert_eq!(hit.pointer, "/files");
}

/// REQ: PKG-137
#[test]
fn pkg_137_empty_files_list_is_a_present_key() {
    let codes = check_codes(&package_of_manifest(manifest_with_files(&[])));
    assert!(!has(&codes, "manifest.key-missing"));
    assert!(has(&codes, "manifest.file-unlisted"));
}

// ---------------------------------------------------------------------------------------------
// Components
// ---------------------------------------------------------------------------------------------

const API_010: [u8; 6] = [0, 0, 0, 1, 0, 0];

fn component_codes(component: &[u8]) -> Vec<String> {
    check_codes(&package_with_component(component))
}

/// REQ: PKG-138
#[test]
fn pkg_138_exactly_one_section_is_accepted() {
    assert!(component_codes(&component_with(&[api_section(&API_010)])).is_empty());
}

/// REQ: PKG-138, PKG-178
#[test]
fn pkg_138_no_section_refused_api_section_missing() {
    assert_eq!(
        component_codes(&component_with(&[])),
        strings(&["package.api-section-missing"])
    );
}

/// REQ: PKG-138, PKG-178
#[test]
fn pkg_138_second_section_refused_api_section_repeated() {
    let bytes = component_with(&[api_section(&API_010), api_section(&API_010)]);
    assert_eq!(
        component_codes(&bytes),
        strings(&["package.api-section-repeated"])
    );
}

/// REQ: PKG-138
#[test]
fn pkg_138_other_custom_sections_do_not_count() {
    let bytes = component_with(&[custom_section("name", b"x"), api_section(&API_010)]);
    assert!(component_codes(&bytes).is_empty());
}

/// REQ: PKG-138
#[test]
fn pkg_138_section_with_a_similar_name_does_not_count() {
    let bytes = component_with(&[custom_section("ilium:api-versions", &API_010)]);
    assert_eq!(
        component_codes(&bytes),
        strings(&["package.api-section-missing"])
    );
}

/// REQ: PKG-139
#[test]
fn pkg_139_payload_of_6_bytes_is_accepted() {
    assert!(component_codes(&component_with(&[api_section(&[0, 0, 0, 1, 0, 0])])).is_empty());
}

/// REQ: PKG-139
#[test]
fn pkg_139_payload_of_5_bytes_refused_api_section_invalid() {
    let bytes = component_with(&[api_section(&[0, 0, 0, 1, 0])]);
    assert_eq!(
        component_codes(&bytes),
        strings(&["package.api-section-invalid"])
    );
}

/// REQ: PKG-139
#[test]
fn pkg_139_payload_of_7_bytes_refused_api_section_invalid() {
    let bytes = component_with(&[api_section(&[0, 0, 0, 1, 0, 0, 0])]);
    assert_eq!(
        component_codes(&bytes),
        strings(&["package.api-section-invalid"])
    );
}

/// REQ: PKG-139
#[test]
fn pkg_139_empty_payload_refused_api_section_invalid() {
    let bytes = component_with(&[api_section(&[])]);
    assert_eq!(
        component_codes(&bytes),
        strings(&["package.api-section-invalid"])
    );
}

/// REQ: PKG-139
#[test]
fn pkg_139_little_endian_payload_does_not_match_the_manifest_version() {
    let bytes = component_with(&[api_section(&[0, 0, 1, 0, 0, 0])]);
    assert_eq!(component_codes(&bytes), strings(&["manifest.api-version"]));
}

/// REQ: PKG-140
#[test]
fn pkg_140_matching_version_is_accepted() {
    assert!(!has(
        &component_codes(&component_with(&[api_section(&API_010)])),
        "manifest.api-version"
    ));
}

/// REQ: PKG-140, PKG-180
#[test]
fn pkg_140_major_differs_refused_api_version() {
    let bytes = component_with(&[api_section(&[0, 1, 0, 1, 0, 0])]);
    assert_eq!(component_codes(&bytes), strings(&["manifest.api-version"]));
}

/// REQ: PKG-140, PKG-180
#[test]
fn pkg_140_minor_differs_refused_api_version() {
    let bytes = component_with(&[api_section(&[0, 0, 0, 2, 0, 0])]);
    assert_eq!(component_codes(&bytes), strings(&["manifest.api-version"]));
}

/// REQ: PKG-140
#[test]
fn pkg_140_patch_differs_refused_api_version() {
    let bytes = component_with(&[api_section(&[0, 0, 0, 1, 0, 1])]);
    assert_eq!(component_codes(&bytes), strings(&["manifest.api-version"]));
}

/// REQ: PKG-141
#[test]
fn pkg_141_layer_1_is_a_component() {
    assert!(!has(
        &component_codes(&component_with(&[api_section(&API_010)])),
        "package.component-invalid"
    ));
}

/// REQ: PKG-141, PKG-177
#[test]
fn pkg_141_layer_0_core_module_refused_component_invalid() {
    let bytes = core_module_with(&[api_section(&API_010)]);
    assert_eq!(
        component_codes(&bytes),
        strings(&["package.component-invalid"])
    );
}

/// REQ: PKG-141
#[test]
fn pkg_141_wrong_magic_refused_component_invalid() {
    let mut bytes = component_with(&[api_section(&API_010)]);
    bytes[1] = 0x62;
    assert_eq!(
        component_codes(&bytes),
        strings(&["package.component-invalid"])
    );
}

/// REQ: PKG-141
#[test]
fn pkg_141_file_shorter_than_the_preamble_refused_component_invalid() {
    for length in [0usize, 1, 7] {
        let bytes = COMPONENT_PREAMBLE[..length].to_vec();
        assert!(
            has(&component_codes(&bytes), "package.component-invalid"),
            "length {length}"
        );
    }
}

/// REQ: PKG-141
#[test]
fn pkg_141_unknown_layer_refused_component_invalid() {
    let mut bytes = component_with(&[api_section(&API_010)]);
    bytes[6] = 2;
    assert_eq!(
        component_codes(&bytes),
        strings(&["package.component-invalid"])
    );
}

/// REQ: PKG-142
#[test]
fn pkg_142_truncated_component_body_refused_component_invalid() {
    let mut bytes = component_with(&[api_section(&API_010)]);
    bytes.extend_from_slice(&[0x01, 0xFF]);
    assert!(has(&component_codes(&bytes), "package.component-invalid"));
}

/// REQ: PKG-142
#[test]
fn pkg_142_exports_not_matching_the_manifest_refused_exports_mismatch() {
    let source = String::from_utf8(vector_manifest()).expect("utf8");
    let declared = source.replace(
        "\"exports\": []",
        "\"exports\": [\n      \"ilium:server/missing@0.1.0\"\n    ]",
    );
    let manifest = declared;
    let bytes = package_of_manifest(manifest.into_bytes());
    assert!(has(&check_codes(&bytes), "manifest.exports-mismatch"));
}

proptest! {
    // REQ: PKG-139, PKG-138
    #[test]
    fn pkg_139_any_payload_length_other_than_6_is_refused(length in 0usize..16) {
        prop_assume!(length != 6);
        let bytes = component_with(&[api_section(&vec![0u8; length])]);
        prop_assert_eq!(component_codes(&bytes), strings(&["package.api-section-invalid"]));
    }

    // REQ: PKG-140
    #[test]
    fn pkg_140_any_version_other_than_0_1_0_is_refused(major in 0u16..4, minor in 0u16..4, patch in 0u16..4) {
        prop_assume!((major, minor, patch) != (0, 1, 0));
        let mut payload = Vec::new();
        for part in [major, minor, patch] {
            payload.extend_from_slice(&part.to_be_bytes());
        }
        let bytes = component_with(&[api_section(&payload)]);
        prop_assert_eq!(component_codes(&bytes), strings(&["manifest.api-version"]));
    }
}

// ---------------------------------------------------------------------------------------------
// pack, determinism, sign
// ---------------------------------------------------------------------------------------------

fn pack_in(fixture: &CliFixture, extra: &[&str]) -> (CliOutput, Vec<u8>) {
    let mut args = vec!["pack", "--allow-debug"];
    args.extend_from_slice(extra);
    let output = run_cli(fixture, &args);
    let path = fixture
        .work()
        .join("dist")
        .join("example.minimal-0.1.0.iliumpkg");
    let bytes = std::fs::read(path).unwrap_or_default();
    (output, bytes)
}

use ilium_plugin_host::cli::CliOutput;

/// REQ: PKG-143
#[test]
fn pkg_143_pack_includes_source_manifest_and_built_component() {
    let fixture = cli_fixture();
    write_pack_project(&fixture.work());
    let (output, bytes) = pack_in(&fixture, &[]);
    assert_eq!(output.exit_code, 0, "{:?}", output.stdout_jsonl);
    assert_eq!(names_of(&bytes), vec!["manifest.json", "server.wasm"]);
}

/// REQ: PKG-143
#[test]
fn pkg_143_pack_includes_assets_help_sounds_screenshots_and_documents() {
    let fixture = cli_fixture();
    write_pack_project(&fixture.work());
    for path in [
        "assets/a.png",
        "help/t.md",
        "sounds/s.wav",
        "screenshots/p.png",
        "README.md",
        "CHANGELOG.md",
        "LICENSE",
        "NOTICE",
    ] {
        write_file(&fixture.work().join(path), b"x");
    }
    let (output, bytes) = pack_in(&fixture, &[]);
    assert_eq!(output.exit_code, 0, "{:?}", output.stdout_jsonl);
    let names = names_of(&bytes);
    for expected in [
        "assets/a.png",
        "help/t.md",
        "sounds/s.wav",
        "screenshots/p.png",
        "README.md",
        "CHANGELOG.md",
        "LICENSE",
        "NOTICE",
    ] {
        assert!(
            names.iter().any(|n| n == expected),
            "{expected} missing from {names:?}"
        );
    }
}

/// REQ: PKG-143
#[test]
fn pkg_143_pack_leaves_out_src_without_include_src() {
    let fixture = cli_fixture();
    write_pack_project(&fixture.work());
    write_file(&fixture.work().join("src/lib.rs"), b"x");
    let (_, without) = pack_in(&fixture, &[]);
    assert!(!names_of(&without).iter().any(|n| n.starts_with("src/")));
    let (_, with) = pack_in(&fixture, &["--include-src"]);
    assert!(names_of(&with).iter().any(|n| n == "src/lib.rs"));
}

/// REQ: PKG-143
#[test]
fn pkg_143_pack_never_includes_files_outside_the_listed_inputs() {
    let fixture = cli_fixture();
    write_pack_project(&fixture.work());
    write_file(&fixture.work().join("notes.txt"), b"x");
    write_file(&fixture.work().join("other/a.png"), b"x");
    let (_, bytes) = pack_in(&fixture, &[]);
    assert_eq!(names_of(&bytes), vec!["manifest.json", "server.wasm"]);
}

/// REQ: PKG-144
#[test]
fn pkg_144_pack_skips_ignored_names_with_a_warning() {
    let fixture = cli_fixture();
    write_pack_project(&fixture.work());
    for path in [
        "assets/.hidden",
        "assets/a~",
        "assets/a.swp",
        "assets/a.tmp",
        "assets/.git/x",
        "assets/target/x",
        "assets/node_modules/x",
        "assets/dist/x",
    ] {
        write_file(&fixture.work().join(path), b"x");
    }
    write_file(&fixture.work().join("assets/keep.png"), b"x");
    let (output, bytes) = pack_in(&fixture, &[]);
    assert_eq!(output.exit_code, 0, "{:?}", output.stdout_jsonl);
    assert_eq!(
        names_of(&bytes),
        vec!["assets/keep.png", "manifest.json", "server.wasm"]
    );
    assert!(records(&output, "warning").len() >= 8);
}

/// REQ: PKG-144
#[test]
fn pkg_144_pack_stops_on_a_remaining_file_that_breaks_a_path_rule() {
    let fixture = cli_fixture();
    write_pack_project(&fixture.work());
    write_file(&fixture.work().join("assets/a b.png"), b"x");
    let (output, bytes) = pack_in(&fixture, &[]);
    assert_ne!(output.exit_code, 0);
    assert!(bytes.is_empty());
    assert!(record_codes(&output, "error")
        .iter()
        .any(|c| c == "package.path-invalid"));
}

/// REQ: PKG-145
#[test]
fn pkg_145_packed_manifest_has_the_documented_serialisation() {
    let fixture = cli_fixture();
    write_pack_project(&fixture.work());
    let (_, bytes) = pack_in(&fixture, &[]);
    assert_eq!(data_of(&bytes, "manifest.json"), vector_manifest());
}

/// REQ: PKG-145
#[test]
fn pkg_145_manifest_ends_with_exactly_one_lf_and_has_no_cr() {
    let manifest = vector_manifest();
    assert!(manifest.ends_with(b"}\n") && !manifest.ends_with(b"\n\n"));
    assert!(!manifest.contains(&b'\r'));
    let fixture = cli_fixture();
    write_pack_project(&fixture.work());
    let (_, bytes) = pack_in(&fixture, &[]);
    let packed = data_of(&bytes, "manifest.json");
    assert!(packed.ends_with(b"}\n") && !packed.ends_with(b"\n\n") && !packed.contains(&b'\r'));
}

/// REQ: PKG-145
#[test]
fn pkg_145_files_key_is_last_and_empty_collections_stay_on_the_key_line() {
    let fixture = cli_fixture();
    write_pack_project(&fixture.work());
    let (_, bytes) = pack_in(&fixture, &[]);
    let text = String::from_utf8(data_of(&bytes, "manifest.json")).expect("utf8");
    assert!(text.contains("\"exports\": []"));
    let files_at = text.find("\"files\"").expect("files key");
    assert!(!text[files_at..].contains("\"server\""));
}

/// REQ: PKG-145
#[test]
fn pkg_145_source_manifest_written_in_other_style_is_reserialised() {
    let fixture = cli_fixture();
    write_pack_project(&fixture.work());
    let compact: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture.work().join("manifest.json")).expect("read"))
            .expect("json");
    write_file(
        &fixture.work().join("manifest.json"),
        serde_json::to_string(&compact).expect("compact").as_bytes(),
    );
    let (output, bytes) = pack_in(&fixture, &[]);
    assert_eq!(output.exit_code, 0, "{:?}", output.stdout_jsonl);
    assert_eq!(data_of(&bytes, "manifest.json"), vector_manifest());
}

/// REQ: PKG-145
#[test]
fn pkg_145_control_characters_in_strings_are_escaped_as_u00xx() {
    let fixture = cli_fixture();
    write_pack_project(&fixture.work());
    let source = std::fs::read_to_string(fixture.work().join("manifest.json")).expect("read");
    let changed = source.replace("\"Minimal\"", "\"Min\\u0001imal\\\\ \\\"q\\\"\"");
    write_file(&fixture.work().join("manifest.json"), changed.as_bytes());
    let (_, bytes) = pack_in(&fixture, &[]);
    let text = String::from_utf8(data_of(&bytes, "manifest.json")).expect("utf8");
    assert!(
        text.contains("\"name\": \"Min\\u0001imal\\\\ \\\"q\\\"\""),
        "{text}"
    );
}

/// REQ: PKG-146
#[test]
fn pkg_146_pack_output_has_sorted_entries_and_fixed_headers() {
    let fixture = cli_fixture();
    write_pack_project(&fixture.work());
    write_file(&fixture.work().join("assets/b.png"), b"b");
    write_file(&fixture.work().join("assets/A.png"), b"a");
    let (_, bytes) = pack_in(&fixture, &[]);
    assert_eq!(
        names_of(&bytes),
        vec![
            "assets/A.png",
            "assets/b.png",
            "manifest.json",
            "server.wasm"
        ]
    );
    for fields in central_fields(&bytes) {
        assert_eq!(
            (fields.made, fields.needed, fields.flags, fields.method),
            (0x0314, 20, 0x0800, 0)
        );
        assert_eq!(
            (fields.time, fields.date, fields.external),
            (0, 0x0021, 0x81A4_0000)
        );
    }
}

/// REQ: PKG-146
#[test]
fn pkg_146_write_package_reproduces_the_unsigned_vector() {
    let files = vector_entries();
    let bytes = write_package(&files).expect("write");
    assert_eq!(sha256_hex(&bytes), VECTOR_UNSIGNED_SHA256);
    assert_eq!(bytes, vector_unsigned());
}

/// REQ: PKG-147
#[test]
fn pkg_147_two_pack_runs_give_identical_bytes() {
    let fixture = cli_fixture();
    write_pack_project(&fixture.work());
    let (_, first) = pack_in(&fixture, &[]);
    std::fs::remove_file(fixture.work().join("dist/example.minimal-0.1.0.iliumpkg"))
        .expect("remove first output");
    let (_, second) = pack_in(&fixture, &[]);
    assert!(!first.is_empty());
    assert_eq!(sha256_hex(&first), sha256_hex(&second));
}

/// REQ: PKG-147
#[test]
fn pkg_147_pack_output_does_not_depend_on_the_clock_or_the_folder() {
    let first_fixture = cli_fixture();
    let mut second_fixture = cli_fixture();
    second_fixture.env.now += std::time::Duration::from_secs(86_400);
    write_pack_project(&first_fixture.work());
    write_pack_project(&second_fixture.work());
    let (_, first) = pack_in(&first_fixture, &[]);
    let (_, second) = pack_in(&second_fixture, &[]);
    assert_eq!(sha256_hex(&first), sha256_hex(&second));
}

/// REQ: PKG-147
#[test]
fn pkg_147_pack_matches_the_manual_vector_bytes() {
    let fixture = cli_fixture();
    write_pack_project(&fixture.work());
    let (_, bytes) = pack_in(&fixture, &[]);
    assert_eq!(sha256_hex(&bytes), VECTOR_UNSIGNED_SHA256);
}

proptest! {
    // REQ: PKG-147, PKG-146
    #[test]
    fn pkg_147_write_package_is_independent_of_input_order(rotation in 0usize..4) {
        let mut files = vec![
            ("assets/b.png".to_string(), b"b".to_vec()),
            ("assets/A.png".to_string(), b"a".to_vec()),
            ("manifest.json".to_string(), vector_manifest()),
            ("server.wasm".to_string(), vector_component()),
        ];
        let reference = write_package(&files).expect("write");
        files.rotate_left(rotation);
        let rotated = write_package(&files).expect("write rotated");
        prop_assert_eq!(reference, rotated);
    }
}

/// REQ: PKG-148
#[test]
fn pkg_148_sign_adds_the_signature_entry_in_sorted_position() {
    let signed = sign(
        &vector_unsigned(),
        &signing_key(),
        SignedAt(VECTOR_SIGNED_AT.to_string()),
    )
    .expect("sign");
    assert_eq!(
        names_of(&signed),
        vec!["manifest.json", "server.wasm", "signature"]
    );
}

/// REQ: PKG-148
#[test]
fn pkg_148_sign_leaves_unsigned_entries_byte_identical() {
    let unsigned = vector_unsigned();
    let signed = sign(
        &unsigned,
        &signing_key(),
        SignedAt(VECTOR_SIGNED_AT.to_string()),
    )
    .expect("sign");
    for name in ["manifest.json", "server.wasm"] {
        assert_eq!(data_of(&signed, name), data_of(&unsigned, name));
    }
}

/// REQ: PKG-148
#[test]
fn pkg_148_sign_reproduces_the_signed_vector() {
    let signed = sign(
        &vector_unsigned(),
        &signing_key(),
        SignedAt(VECTOR_SIGNED_AT.to_string()),
    )
    .expect("sign");
    assert_eq!(sha256_hex(&signed), VECTOR_SIGNED_SHA256);
    assert_eq!(signed, vector_signed());
}

/// REQ: PKG-148
#[test]
fn pkg_148_signed_vector_is_a_valid_archive_in_central_order() {
    let archive = read_package(&vector_signed()).expect("read signed");
    assert!(archive.signature.is_some());
    assert_eq!(
        names_of(&vector_signed()),
        vec!["manifest.json", "server.wasm", "signature"]
    );
}

/// REQ: PKG-148
#[test]
fn pkg_148_signing_a_package_with_sounds_puts_signature_before_sounds() {
    let package = package_with(&[("sounds/a.wav", b"w")]);
    let signed = sign(
        &package,
        &signing_key(),
        SignedAt(VECTOR_SIGNED_AT.to_string()),
    )
    .expect("sign");
    let names = names_of(&signed);
    let signature = names
        .iter()
        .position(|n| n == "signature")
        .expect("signature");
    let sounds = names
        .iter()
        .position(|n| n == "sounds/a.wav")
        .expect("sounds");
    let server = names
        .iter()
        .position(|n| n == "server.wasm")
        .expect("server");
    assert!(server < signature && signature < sounds, "{names:?}");
}

// ---------------------------------------------------------------------------------------------
// Pipeline
// ---------------------------------------------------------------------------------------------

/// REQ: PKG-149
#[test]
fn pkg_149_a_valid_package_reaches_stage_6_in_check() {
    assert!(check(&vector_unsigned()).last_stage >= 6);
}

/// REQ: PKG-149
#[test]
fn pkg_149_stage_1_error_stops_every_later_stage() {
    let bytes = changed_entry_order();
    let report = check(&bytes);
    assert_eq!(report.last_stage, 1);
    assert!(error_codes(&report.diagnostics)
        .iter()
        .all(|c| c.starts_with("package.")));
}

fn changed_entry_order() -> Vec<u8> {
    build(
        &[
            Z::new("server.wasm", &vector_component()),
            Z::new("manifest.json", b"not json"),
        ],
        &Tail::default(),
    )
}

/// REQ: PKG-149
#[test]
fn pkg_149_stage_2_error_stops_before_the_inventory_and_components() {
    let bytes = zip_of(&[
        ("manifest.json", b"{".to_vec()),
        ("server.wasm", b"junk".to_vec()),
    ]);
    let report = check(&bytes);
    assert_eq!(report.last_stage, 2);
    assert_eq!(
        error_codes(&report.diagnostics),
        strings(&["manifest.json-invalid"])
    );
}

/// REQ: PKG-149
#[test]
fn pkg_149_stage_5_error_stops_before_the_components() {
    let report = check(&vector_plus_unlisted(vec![Z::new("assets/x.png", b"1")]));
    assert_eq!(report.last_stage, 5);
}

/// REQ: PKG-149
#[test]
fn pkg_149_stage_6_error_is_reported_without_stage_7_codes() {
    let report = check(&package_with_component(&component_with(&[])));
    assert_eq!(report.last_stage, 6);
    assert!(error_codes(&report.diagnostics)
        .iter()
        .all(|c| c != "manifest.signature-invalid"));
}

/// REQ: PKG-150
#[test]
fn pkg_150_100_errors_in_a_stage_are_all_reported_without_a_warning() {
    let extras: Vec<Z> = (0..100)
        .map(|i| Z::new(&format!("assets/f{i:03}.bin"), b"x"))
        .collect();
    let report = check(&vector_plus_unlisted(extras));
    assert_eq!(error_codes(&report.diagnostics).len(), 100);
    assert!(warning_diagnostics(&report.diagnostics).is_empty());
}

/// REQ: PKG-150
#[test]
fn pkg_150_the_101st_error_becomes_one_warning_with_the_count() {
    let extras: Vec<Z> = (0..101)
        .map(|i| Z::new(&format!("assets/f{i:03}.bin"), b"x"))
        .collect();
    let report = check(&vector_plus_unlisted(extras));
    assert_eq!(error_codes(&report.diagnostics).len(), 100);
    let warnings = warning_diagnostics(&report.diagnostics);
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].message.contains('1'), "{}", warnings[0].message);
}

/// REQ: PKG-150
#[test]
fn pkg_150_the_warning_counts_all_the_remaining_errors() {
    let extras: Vec<Z> = (0..130)
        .map(|i| Z::new(&format!("assets/f{i:03}.bin"), b"x"))
        .collect();
    let report = check(&vector_plus_unlisted(extras));
    assert_eq!(error_codes(&report.diagnostics).len(), 100);
    let warnings = warning_diagnostics(&report.diagnostics);
    assert_eq!(warnings.len(), 1);
    assert!(
        warnings[0].message.contains("30"),
        "{}",
        warnings[0].message
    );
}

/// REQ: PKG-150
#[test]
fn pkg_150_99_errors_are_all_reported() {
    let extras: Vec<Z> = (0..99)
        .map(|i| Z::new(&format!("assets/f{i:03}.bin"), b"x"))
        .collect();
    let report = check(&vector_plus_unlisted(extras));
    assert_eq!(error_codes(&report.diagnostics).len(), 99);
}

/// REQ: PKG-151
#[test]
fn pkg_151_a_full_check_instantiates_no_component() {
    let (_dir, installer) = installer();
    let _ = installer.check(
        &vector_signed(),
        ilium_plugin_api::manifest::CheckMode::Install,
    );
    assert_eq!(installer.instantiated_count(), 0);
}

/// REQ: PKG-151
#[test]
fn pkg_151_a_refused_package_instantiates_no_component() {
    let (_dir, installer) = installer();
    let _ = installer.check(
        &package_with_component(&component_with(&[])),
        ilium_plugin_api::manifest::CheckMode::Install,
    );
    assert_eq!(installer.instantiated_count(), 0);
}

/// REQ: PKG-152
#[test]
fn pkg_152_a_tier_key_in_the_manifest_is_not_a_source_of_tier() {
    let source = String::from_utf8(vector_manifest()).expect("utf8");
    let with_tier = source.replace("  \"server\"", "  \"tier\": \"T3\",\n  \"server\"");
    let report = check(&package_of_manifest(with_tier.into_bytes()));
    assert!(report.last_stage <= 3);
    assert!(!error_codes(&report.diagnostics).is_empty());
}

/// REQ: PKG-152
#[test]
fn pkg_152_an_unsigned_package_is_untrusted_whatever_the_manifest_says() {
    let fixture = cli_fixture();
    let path = fixture.work().join("p.iliumpkg");
    write_file(&path, &vector_unsigned());
    let output = run_cli(&fixture, &["verify", path.to_str().expect("utf8")]);
    let result = records(&output, "result");
    assert_eq!(result[0]["tier"], "untrusted");
}

// ---------------------------------------------------------------------------------------------
// check and verify
// ---------------------------------------------------------------------------------------------

fn state_files(fixture: &CliFixture) -> Vec<String> {
    let mut found = Vec::new();
    fn walk(dir: &std::path::Path, out: &mut Vec<String>) {
        if let Ok(read) = std::fs::read_dir(dir) {
            for entry in read.flatten() {
                let path = entry.path();
                out.push(path.display().to_string());
                if path.is_dir() {
                    walk(&path, out);
                }
            }
        }
    }
    walk(&fixture.state(), &mut found);
    walk(&fixture.root.path().join("data"), &mut found);
    found.sort();
    found
}

/// REQ: PKG-153
#[test]
fn pkg_153_check_of_a_source_manifest_needs_no_archive() {
    let fixture = cli_fixture();
    write_pack_project(&fixture.work());
    let output = run_cli(&fixture, &["check"]);
    assert_eq!(output.exit_code, 0, "{:?}", output.stdout_jsonl);
}

/// REQ: PKG-153
#[test]
fn pkg_153_check_of_a_package_runs_through_stage_6() {
    let fixture = cli_fixture();
    let path = fixture.work().join("p.iliumpkg");
    write_file(&path, &package_with_component(&component_with(&[])));
    let output = run_cli(&fixture, &["check", path.to_str().expect("utf8")]);
    assert_eq!(output.exit_code, 3);
    assert!(record_codes(&output, "error")
        .iter()
        .any(|c| c == "package.api-section-missing"));
}

/// REQ: PKG-153
#[test]
fn pkg_153_check_never_contacts_the_network_and_never_writes_state() {
    let fixture = cli_fixture();
    let path = fixture.work().join("p.iliumpkg");
    write_file(&path, &vector_signed());
    let before = state_files(&fixture);
    let _ = run_cli(&fixture, &["check", path.to_str().expect("utf8")]);
    assert_eq!(fixture.network_calls(), 0);
    assert_eq!(state_files(&fixture), before);
}

/// REQ: PKG-153
#[test]
fn pkg_153_check_of_a_refused_source_manifest_exits_3() {
    let fixture = cli_fixture();
    write_pack_project(&fixture.work());
    write_file(&fixture.work().join("manifest.json"), b"{}");
    let output = run_cli(&fixture, &["check"]);
    assert_eq!(output.exit_code, 3);
}

fn verify_output(bytes: &[u8], trust: Option<(&str, &str)>) -> CliOutput {
    let fixture = cli_fixture();
    if let Some((status, revoked)) = trust {
        write_trust_store(&fixture, status, revoked);
    }
    let path = fixture.work().join("p.iliumpkg");
    write_file(&path, bytes);
    run_cli(&fixture, &["verify", path.to_str().expect("utf8")])
}

/// REQ: PKG-154
#[test]
fn pkg_154_verify_prints_one_progress_line_per_stage() {
    let output = verify_output(&vector_unsigned(), None);
    let stages: Vec<i64> = records(&output, "progress")
        .iter()
        .filter_map(|r| r["stage"].as_i64())
        .collect();
    assert_eq!(stages, (1..=8).collect::<Vec<i64>>());
    assert!(records(&output, "progress")
        .iter()
        .all(|r| r["name"].is_string()));
}

/// REQ: PKG-154
#[test]
fn pkg_154_verify_prints_errors_then_exactly_one_result_last() {
    let output = verify_output(&package_with_component(&component_with(&[])), None);
    let kinds: Vec<&str> = output
        .stdout_jsonl
        .iter()
        .filter_map(|r| r["type"].as_str())
        .collect();
    assert_eq!(kinds.iter().filter(|k| **k == "result").count(), 1);
    let last_error = kinds
        .iter()
        .rposition(|k| *k == "error")
        .expect("an error line");
    let result = kinds.iter().position(|k| *k == "result").expect("result");
    assert!(last_error < result);
    assert!(kinds.iter().filter(|k| **k == "progress").count() < 8);
}

/// REQ: PKG-155
#[test]
fn pkg_155_result_line_has_the_documented_fields_for_a_signed_package() {
    let output = verify_output(&vector_signed(), Some(("trusted", "null")));
    let result = records(&output, "result")[0].clone();
    assert_eq!(result["ok"], true);
    assert_eq!(result["id"], "example.minimal");
    assert_eq!(result["version"], "0.1.0");
    assert_eq!(result["sha256"], VECTOR_SIGNED_SHA256);
    assert_eq!(result["manifest_sha256"], VECTOR_MANIFEST_SHA256);
    assert_eq!(result["tier"], "T2");
    assert_eq!(result["signer_key_id"], VECTOR_KEY_ID);
    assert_eq!(result["publisher_id"], "example");
}

/// REQ: PKG-155
#[test]
fn pkg_155_unsigned_package_has_null_signer_and_untrusted_tier() {
    let output = verify_output(&vector_unsigned(), None);
    let result = records(&output, "result")[0].clone();
    assert_eq!(result["tier"], "untrusted");
    assert!(result["signer_key_id"].is_null());
    assert!(result["publisher_id"].is_null());
    assert_eq!(result["sha256"], VECTOR_UNSIGNED_SHA256);
}

/// REQ: PKG-156
#[test]
fn pkg_156_verify_exits_0_when_stages_1_to_7_pass() {
    assert_eq!(verify_output(&vector_unsigned(), None).exit_code, 0);
    assert_eq!(
        verify_output(&vector_signed(), Some(("trusted", "null"))).exit_code,
        0
    );
}

/// REQ: PKG-156
#[test]
fn pkg_156_verify_exits_1_when_a_stage_fails() {
    let output = verify_output(&package_with_component(&component_with(&[])), None);
    assert_eq!(output.exit_code, 1);
    assert_eq!(records(&output, "result")[0]["ok"], false);
}

/// REQ: PKG-156
#[test]
fn pkg_156_unknown_key_is_a_warning_with_tier_untrusted_and_exit_0() {
    let output = verify_output(&vector_signed(), None);
    assert_eq!(output.exit_code, 0);
    assert!(record_codes(&output, "warning")
        .iter()
        .any(|c| c == "trust.key-unknown"));
    assert_eq!(records(&output, "result")[0]["tier"], "untrusted");
}

/// REQ: PKG-156
#[test]
fn pkg_156_revoked_key_is_an_error_and_exit_1() {
    let output = verify_output(
        &vector_signed(),
        Some((
            "revoked",
            "{\"reason\":\"compromised\",\"at\":\"2026-10-12 11:00:00\"}",
        )),
    );
    assert_eq!(output.exit_code, 1);
    assert!(record_codes(&output, "error")
        .iter()
        .any(|c| c == "trust.key-revoked"));
}

/// REQ: PKG-156
#[test]
fn pkg_156_install_of_an_unknown_key_without_a_prompt_answer_exits_1() {
    let fixture = cli_fixture();
    let path = fixture.work().join("p.iliumpkg");
    write_file(&path, &vector_signed());
    let output = run_cli(&fixture, &["install", path.to_str().expect("utf8")]);
    assert_eq!(output.exit_code, 1);
}
