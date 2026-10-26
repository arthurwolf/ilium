//! reference-manifest.md: document rules (PKG-001..PKG-012) and the top-level keys
//! (PKG-013..PKG-035). Expected values come from the rule tables of that page and from the
//! mapping of reference-manifest-schema.md (PKG-562).

use ilium_plugin_api::manifest::{
    parse_manifest, validate_json, CheckMode, Manifest, Stage, DEFAULT_DATA_VERSION,
};
use ilium_plugin_api::Tier;
use proptest::prelude::*;
use serde_json::{json, Value};

use crate::support::*;

const ONE_MIB: usize = 1_048_576;

fn parse_codes(text: &str) -> Vec<String> {
    match parse_manifest(text) {
        Ok(_) => Vec::new(),
        Err(found) => found
            .iter()
            .map(|diagnostic| diagnostic.code.as_str().to_owned())
            .collect(),
    }
}

/// A client manifest text padded with trailing spaces (valid JSON whitespace) to `size` bytes.
fn padded_text(size: usize) -> String {
    let mut text = client().to_string();
    assert!(text.len() <= size);
    text.push_str(&" ".repeat(size - text.len()));
    text
}

// ---------------------------------------------------------------------------------------------
// PKG-001 .. PKG-012 document rules
// ---------------------------------------------------------------------------------------------

/// REQ: PKG-001
#[test]
fn pkg_001_manifest_of_exactly_one_mib_is_accepted() {
    let text = padded_text(ONE_MIB);
    assert_eq!(text.len(), 1_048_576);
    assert!(parse_manifest(&text).is_ok());
}

/// REQ: PKG-001
#[test]
fn pkg_001_manifest_one_byte_over_one_mib_refused_manifest_size() {
    let text = padded_text(ONE_MIB + 1);
    assert_has(&parse_codes(&text), "package.manifest-size");
}

/// REQ: PKG-001
#[test]
fn pkg_001_manifest_one_byte_under_one_mib_is_accepted() {
    let text = padded_text(ONE_MIB - 1);
    assert!(parse_manifest(&text).is_ok());
}

/// REQ: PKG-001
#[test]
fn pkg_001_byte_order_mark_refused_json_invalid() {
    let text = format!("\u{feff}{}", client());
    assert_codes(&parse_codes(&text), &["manifest.json-invalid"]);
}

/// REQ: PKG-001
#[test]
fn pkg_001_truncated_json_refused_json_invalid_at_stage_two() {
    let found = diagnostics_of_text("{\"api\":", Tier::T3, CheckMode::Check);
    assert_eq!(
        error_codes(&found),
        vec!["manifest.json-invalid".to_owned()]
    );
    assert_eq!(found[0].stage, Some(Stage(2)));
}

/// REQ: PKG-001
#[test]
fn pkg_001_empty_text_refused_json_invalid() {
    assert_codes(&text_errors(""), &["manifest.json-invalid"]);
}

/// REQ: PKG-001
#[test]
fn pkg_001_source_manifest_has_the_same_form_and_parses() {
    let text = client().to_string();
    let parsed = parse_manifest(&text).expect("a source manifest parses");
    assert_eq!(parsed.id, "example.demo");
    assert!(parsed.files.is_empty());
    assert_eq!(parsed.api, "ilium:plugin@0.1.0");
}

/// REQ: PKG-002
#[test]
fn pkg_002_non_object_top_level_refused_schema_at_root() {
    for text in ["[]", "[{}]", "\"text\"", "1", "true", "false", "null"] {
        let found = diagnostics_of_text(text, Tier::T3, CheckMode::Check);
        assert_eq!(
            error_codes(&found),
            vec!["manifest.schema".to_owned()],
            "top-level value {text}"
        );
        assert_eq!(found[0].pointer, "", "root pointer for {text}");
    }
}

/// REQ: PKG-002
#[test]
fn pkg_002_object_top_level_is_accepted() {
    assert_clean(&client());
}

/// REQ: PKG-003
#[test]
fn pkg_003_duplicate_key_at_top_level_refused_duplicate_key() {
    let text = r#"{"api":"ilium:plugin@0.1.0","api":"ilium:plugin@0.1.0"}"#;
    assert_codes(&text_errors(text), &["manifest.duplicate-key"]);
}

/// REQ: PKG-003
#[test]
fn pkg_003_duplicate_key_in_nested_object_refused_duplicate_key() {
    let text = r#"{"client":{"entry":"client.wasm","entry":"client.wasm","world":"ilium:client/base@0.1.0"}}"#;
    assert_codes(&text_errors(text), &["manifest.duplicate-key"]);
}

/// REQ: PKG-003
#[test]
fn pkg_003_duplicate_key_inside_array_element_refused_duplicate_key() {
    let text = r#"{"contributes":{"commands":[{"id":"a","id":"b"}]}}"#;
    assert_codes(&text_errors(text), &["manifest.duplicate-key"]);
}

/// REQ: PKG-003
#[test]
fn pkg_003_escaped_spelling_of_the_same_name_is_a_duplicate() {
    let text = r#"{"api":"x","api":"y"}"#;
    assert_codes(&text_errors(text), &["manifest.duplicate-key"]);
}

/// REQ: PKG-003
#[test]
fn pkg_003_duplicate_key_stops_validation_at_stage_two() {
    // The text also has an unknown key and a bad id; none of them is reported.
    let text = r#"{"id":"Bad","id":"Bad","unknown":1}"#;
    let found = diagnostics_of_text(text, Tier::T3, CheckMode::Check);
    assert_eq!(
        error_codes(&found),
        vec!["manifest.duplicate-key".to_owned()]
    );
    assert_eq!(found[0].stage, Some(Stage(2)));
}

/// REQ: PKG-003
#[test]
fn pkg_003_parse_manifest_reports_duplicate_key() {
    let text = r#"{"api":"a","api":"a"}"#;
    assert_codes(&parse_codes(text), &["manifest.duplicate-key"]);
}

/// REQ: PKG-004
#[test]
fn pkg_004_non_json_forms_refused_json_invalid() {
    let valid = client().to_string();
    let without_last = &valid[..valid.len() - 1];
    let cases = [
        ("line comment", format!("// note\n{valid}")),
        ("block comment", format!("/* note */{valid}")),
        ("trailing comma", format!("{without_last},}}")),
        ("single quotes", "{'api':'ilium:plugin@0.1.0'}".to_owned()),
        ("unquoted key", "{api:\"ilium:plugin@0.1.0\"}".to_owned()),
        ("trailing garbage", format!("{valid} x")),
    ];
    for (name, text) in cases {
        let found = diagnostics_of_text(&text, Tier::T3, CheckMode::Check);
        assert_eq!(
            error_codes(&found),
            vec!["manifest.json-invalid".to_owned()],
            "{name}"
        );
        assert_eq!(found[0].stage, Some(Stage(2)), "{name}");
    }
}

/// REQ: PKG-004
#[test]
fn pkg_004_parse_manifest_refuses_comments_and_trailing_commas() {
    assert_codes(&parse_codes("{/* c */}"), &["manifest.json-invalid"]);
    assert_codes(&parse_codes("{\"a\":1,}"), &["manifest.json-invalid"]);
}

/// REQ: PKG-005
#[test]
fn pkg_005_unknown_key_at_top_level_refused_unknown_key() {
    let found = errors(&set(client(), "/extra", json!(1)));
    assert_codes(&found, &["manifest.unknown-key"]);
}

/// REQ: PKG-005
#[test]
fn pkg_005_unknown_key_in_nested_objects_refused_unknown_key() {
    for pointer in [
        "/client/extra",
        "/capabilities/extra",
        "/requires/extra",
        "/contributes/extra",
    ] {
        let found = errors(&set(client(), pointer, json!([])));
        assert_has(&found, "manifest.unknown-key");
    }
}

/// REQ: PKG-005
#[test]
fn pkg_005_unknown_key_in_array_element_refused_unknown_key() {
    let value = set(
        client(),
        "/contributes/commands",
        json!([{ "id": "run", "title": "Run", "bogus": true }]),
    );
    assert_has(&errors(&value), "manifest.unknown-key");
}

/// REQ: PKG-005
#[test]
fn pkg_005_unknown_key_in_files_item_refused_unknown_key() {
    let mut value = packed(client());
    value["files"][0]["extra"] = json!(1);
    assert_has(&install_errors(&value, Tier::T3), "manifest.unknown-key");
}

/// REQ: PKG-005
#[test]
fn pkg_005_json_schema_subset_objects_accept_other_names() {
    let value = set(
        client(),
        "/contributes/commands",
        json!([{
            "id": "run",
            "title": "Run",
            "arguments_schema": {
                "type": "object",
                "properties": { "any_free_name": { "type": "string" } }
            }
        }]),
    );
    assert_lacks(&errors(&value), "manifest.unknown-key");
}

/// REQ: PKG-005
#[test]
fn pkg_005_key_names_are_case_sensitive() {
    let found = errors(&set(client(), "/Name", json!("x")));
    assert_has(&found, "manifest.unknown-key");
}

/// REQ: PKG-006
#[test]
fn pkg_006_null_values_refused_schema() {
    for pointer in [
        "/description",
        "/license",
        "/icon",
        "/client/exports",
        "/requires",
    ] {
        let found = errors(&set(client(), pointer, Value::Null));
        assert_has(&found, "manifest.schema");
    }
}

/// REQ: PKG-006
#[test]
fn pkg_006_null_required_value_refused_without_being_a_missing_key() {
    let found = errors(&set(client(), "/name", Value::Null));
    assert_has(&found, "manifest.schema");
}

/// REQ: PKG-006
#[test]
fn pkg_006_omitting_unneeded_keys_is_accepted() {
    assert_clean(&client());
}

/// REQ: PKG-007
#[test]
fn pkg_007_integer_forms_in_a_limit_value() {
    let accepted = ["10", "0", "4294967295"];
    for number in accepted {
        let text = with_raw_member(
            &client(),
            &format!("\"limits\":{{\"memory_mib\":{number}}}"),
        );
        assert_lacks(&text_errors(&text), "manifest.schema");
    }
    let refused = ["10.0", "1e1", "1E1", "10.5", "-1", "4294967296"];
    for number in refused {
        let text = with_raw_member(
            &client(),
            &format!("\"limits\":{{\"memory_mib\":{number}}}"),
        );
        assert_has(&text_errors(&text), "manifest.schema");
    }
}

/// REQ: PKG-007
#[test]
fn pkg_007_fraction_in_integer_key_refused_schema_not_rounded() {
    let text = with_raw_member(&client(), "\"limits\":{\"event_queue\":1024.0}");
    assert_has(&text_errors(&text), "manifest.schema");
}

/// REQ: PKG-007
#[test]
fn pkg_007_string_where_an_integer_is_required_refused_schema() {
    let value = set(client(), "/limits/memory_mib", json!("10"));
    assert_has(&errors(&value), "manifest.schema");
}

/// REQ: PKG-008
#[test]
fn pkg_008_nesting_of_exactly_32_levels_is_accepted() {
    // Levels: root 1, contributes 2, commands 3, entry 4, arguments_schema 5; every wrapper adds
    // `properties` and the property schema (2 levels). 13 wrappers put the innermost object at 31
    // and its `enum` array at 32.
    let value = set(
        client(),
        "/contributes/commands",
        json!([{ "id": "run", "title": "Run", "arguments_schema": nested_schema(13, true) }]),
    );
    let found = errors(&value);
    assert_lacks(&found, "manifest.schema");
    assert_lacks(&found, "manifest.unknown-key");
}

/// REQ: PKG-008
#[test]
fn pkg_008_nesting_of_33_levels_refused_schema() {
    let value = set(
        client(),
        "/contributes/commands",
        json!([{ "id": "run", "title": "Run", "arguments_schema": nested_schema(14, false) }]),
    );
    assert_has(&errors(&value), "manifest.schema");
}

/// REQ: PKG-008
#[test]
fn pkg_008_very_deep_nesting_refused_schema_and_does_not_crash() {
    let value = set(
        client(),
        "/contributes/commands",
        json!([{ "id": "run", "title": "Run", "arguments_schema": nested_schema(200, false) }]),
    );
    assert_has(&errors(&value), "manifest.schema");
}

fn nested_schema(wrappers: usize, with_enum: bool) -> Value {
    let mut current = if with_enum {
        json!({ "type": "string", "enum": ["a"] })
    } else {
        json!({ "type": "string" })
    };
    for _ in 0..wrappers {
        current = json!({ "type": "object", "properties": { "p": current } });
    }
    current
}

const FORBIDDEN_DISPLAY_CHARS: [char; 17] = [
    '\u{0000}', '\u{0007}', '\u{0009}', '\u{000a}', '\u{000d}', '\u{001f}', '\u{007f}', '\u{2028}',
    '\u{2029}', '\u{202a}', '\u{202b}', '\u{202c}', '\u{202d}', '\u{202e}', '\u{2066}', '\u{2067}',
    '\u{2069}',
];

/// REQ: PKG-009
#[test]
fn pkg_009_forbidden_characters_in_name_refused_display_name() {
    for character in FORBIDDEN_DISPLAY_CHARS {
        let value = set(client(), "/name", json!(format!("Ex{character}ample")));
        assert_has(&errors(&value), "manifest.display-name");
    }
}

/// REQ: PKG-009
#[test]
fn pkg_009_forbidden_characters_in_publisher_refused_display_name() {
    for character in FORBIDDEN_DISPLAY_CHARS {
        let value = set(
            client(),
            "/publisher",
            json!(format!("Pub{character}lisher")),
        );
        assert_has(&errors(&value), "manifest.display-name");
    }
}

/// REQ: PKG-009
#[test]
fn pkg_009_forbidden_characters_in_description_refused_display_name() {
    for character in [
        '\u{0007}', '\u{001f}', '\u{007f}', '\u{2028}', '\u{2029}', '\u{202e}', '\u{2066}',
    ] {
        let value = set(
            client(),
            "/description",
            json!(format!("Text{character}more")),
        );
        assert_has(&errors(&value), "manifest.display-name");
    }
}

/// REQ: PKG-009
#[test]
fn pkg_009_forbidden_characters_in_a_title_refused_display_name_with_pointer() {
    let value = set(
        client(),
        "/contributes/commands",
        json!([{ "id": "run", "title": "Ru\u{202e}n" }]),
    );
    let found = error_diagnostics(&value);
    let diagnostic = first_with(&found, "manifest.display-name");
    assert_eq!(diagnostic.pointer, "/contributes/commands/0/title");
}

/// REQ: PKG-009
#[test]
fn pkg_009_characters_next_to_the_forbidden_ranges_are_accepted() {
    // U+202F, U+2065, U+206A and U+200E are outside the listed ranges.
    for character in ['\u{202f}', '\u{2065}', '\u{206a}', '\u{200e}', 'é', '漢'] {
        let value = set(client(), "/name", json!(format!("Name{character}x")));
        assert_lacks(&errors(&value), "manifest.display-name");
    }
}

/// REQ: PKG-009
#[test]
fn pkg_009_all_four_isolate_characters_refused() {
    for character in ['\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}'] {
        let value = set(client(), "/name", json!(format!("a{character}b")));
        assert_has(&errors(&value), "manifest.display-name");
    }
}

/// REQ: PKG-010
#[test]
fn pkg_010_order_of_keys_does_not_change_the_meaning() {
    let value = set(client(), "/description", json!("A description."));
    let forward = value.to_string();
    let pairs: Vec<String> = value
        .as_object()
        .expect("object")
        .iter()
        .rev()
        .map(|(key, member)| format!("{}:{}", Value::String(key.clone()), member))
        .collect();
    let reversed = format!("{{{}}}", pairs.join(","));
    assert_ne!(forward, reversed);
    let first: Manifest = parse_manifest(&forward).expect("forward parses");
    let second: Manifest = parse_manifest(&reversed).expect("reversed parses");
    assert_eq!(first, second);
    assert_eq!(text_errors(&forward), text_errors(&reversed));
}

/// REQ: PKG-010
#[test]
fn pkg_010_every_rotation_of_the_top_level_keys_parses_to_the_same_manifest() {
    let value = set(client(), "/license", json!("MIT"));
    let entries: Vec<(String, Value)> = value
        .as_object()
        .expect("object")
        .iter()
        .map(|(key, member)| (key.clone(), member.clone()))
        .collect();
    let expected = parse_manifest(&value.to_string()).expect("parses");
    for shift in 0..entries.len() {
        let rotated: Vec<String> = (0..entries.len())
            .map(|offset| {
                let (key, member) = &entries[(offset + shift) % entries.len()];
                format!("{}:{}", Value::String(key.clone()), member)
            })
            .collect();
        let text = format!("{{{}}}", rotated.join(","));
        assert_eq!(parse_manifest(&text).expect("rotation parses"), expected);
    }
}

/// REQ: PKG-010
#[test]
fn pkg_010_serialised_keys_follow_the_table_order_with_files_last() {
    let value = json!({
        "api": "ilium:plugin@0.1.0", "id": "example.demo", "name": "N", "version": "0.1.0",
        "publisher": "P", "description": "D", "license": "MIT", "icon": "puzzle",
        "categories": ["tools"], "keywords": ["k"], "homepage": "https://example.com",
        "repository": "https://example.com/r",
        "client": { "entry": "client.wasm", "world": "ilium:client/base@0.1.0" },
        "activation": ["startup"], "replaces": ["ilium.core.titles"],
        "unstable": ["websocket"],
        "files": [{ "path": "client.wasm", "length": 1, "sha256": SAMPLE_HASH }]
    });
    let manifest: Manifest = serde_json::from_value(value).expect("deserializes");
    let text = serde_json::to_string(&manifest).expect("serializes");
    let table_order = [
        "api",
        "id",
        "name",
        "version",
        "publisher",
        "description",
        "license",
        "icon",
        "categories",
        "keywords",
        "homepage",
        "repository",
        "client",
        "activation",
        "replaces",
        "unstable",
        "files",
    ];
    let mut last = 0usize;
    for key in table_order {
        let position = text
            .find(&format!("\"{key}\":"))
            .unwrap_or_else(|| panic!("key {key} missing in {text}"));
        assert!(position >= last, "{key} is out of table order in {text}");
        last = position;
    }
    assert!(
        text.rfind("\"files\":").expect("files") > text.rfind("\"unstable\":").expect("unstable")
    );
}

/// REQ: PKG-011
#[test]
fn pkg_011_source_manifest_may_omit_files() {
    let found = error_codes(&diagnostics(&client(), Tier::T3, CheckMode::Check));
    assert_lacks(&found, "manifest.key-missing");
}

/// REQ: PKG-011
#[test]
fn pkg_011_packed_manifest_without_files_refused_key_missing() {
    let found = install_errors(&client(), Tier::T3);
    assert_has(&found, "manifest.key-missing");
}

/// REQ: PKG-011
#[test]
fn pkg_011_packed_manifest_with_files_has_no_key_missing() {
    let found = install_errors(&packed(client()), Tier::T3);
    assert_lacks(&found, "manifest.key-missing");
}

/// REQ: PKG-011
#[test]
fn pkg_011_check_ignores_files_in_a_source_manifest() {
    // A source manifest with an unsorted, wrong inventory is not refused by `check`.
    let mut value = client();
    value["files"] = json!([file_entry("zzz.png", 1), file_entry("client.wasm", 1)]);
    let found = errors(&value);
    assert_lacks(&found, "manifest.files-order");
}

/// REQ: PKG-012
#[test]
fn pkg_012_manifest_without_any_component_is_refused() {
    let value = unset(client(), "/client");
    let found = errors(&value);
    assert!(!found.is_empty());
    for code in &found {
        assert!(
            code == "manifest.key-missing" || code == "manifest.schema",
            "unexpected {code}"
        );
    }
}

/// REQ: PKG-012
#[test]
fn pkg_012_each_single_side_is_accepted() {
    assert_clean(&server());
    assert_clean(&client());
    assert_clean(&scene());
}

/// REQ: PKG-012
#[test]
fn pkg_012_empty_object_lists_all_missing_requirements_in_pointer_order() {
    let found = text_errors("{}");
    assert_codes(
        &found,
        &[
            "manifest.schema",
            "manifest.key-missing",
            "manifest.key-missing",
            "manifest.key-missing",
            "manifest.key-missing",
            "manifest.key-missing",
        ],
    );
}

/// REQ: PKG-012
#[test]
fn pkg_012_two_sides_are_accepted() {
    let value = set(
        server(),
        "/client",
        json!({ "entry": "client.wasm", "world": "ilium:client/base@0.1.0" }),
    );
    assert_clean(&value);
}

// ---------------------------------------------------------------------------------------------
// PKG-013 .. PKG-035 top-level keys
// ---------------------------------------------------------------------------------------------

/// REQ: PKG-013
#[test]
fn pkg_013_exact_contract_string_is_accepted() {
    assert_clean(&set(client(), "/api", json!("ilium:plugin@0.1.0")));
}

/// REQ: PKG-013
#[test]
fn pkg_013_well_formed_version_outside_the_window_refused_api_unsupported() {
    for text in [
        "ilium:plugin@9.9.9",
        "ilium:plugin@10.0.0",
        "ilium:plugin@1.0.0",
    ] {
        let found = error_diagnostics(&set(client(), "/api", json!(text)));
        assert_eq!(
            found
                .iter()
                .map(|d| d.code.as_str().to_owned())
                .collect::<Vec<_>>(),
            vec!["package.api-unsupported".to_owned()],
            "{text}"
        );
        assert_eq!(found[0].stage, Some(Stage(3)), "{text}");
    }
}

/// REQ: PKG-013
#[test]
fn pkg_013_any_other_string_is_a_schema_error() {
    for text in [
        "",
        "ilium:plugin@0.1",
        "ilium:plugin",
        "ilium:plugin@0.1.0 ",
        " ilium:plugin@0.1.0",
        "ILIUM:plugin@0.1.0",
        "ilium:plugin@v0.1.0",
        "ilium:plugin@0.1.0-rc1",
        "ilium:host@0.1.0",
        "ilium:plugin@0.1.x",
    ] {
        let found = errors(&set(client(), "/api", json!(text)));
        assert_has(&found, "manifest.schema");
        assert_lacks(&found, "package.api-unsupported");
    }
}

/// REQ: PKG-013
#[test]
fn pkg_013_non_string_api_is_a_schema_error() {
    assert_has(&errors(&set(client(), "/api", json!(1))), "manifest.schema");
    assert_has(
        &errors(&set(client(), "/api", json!(["ilium:plugin@0.1.0"]))),
        "manifest.schema",
    );
}

fn is_well_formed_api(text: &str) -> bool {
    match text.strip_prefix("ilium:plugin@") {
        Some(rest) => {
            let parts: Vec<&str> = rest.split('.').collect();
            parts.len() == 3
                && parts
                    .iter()
                    .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
        }
        None => false,
    }
}

proptest! {
    // REQ: PKG-013
    #[test]
    fn pkg_013_random_text_that_is_not_a_well_formed_contract_is_a_schema_error(
        text in "[a-z:@0-9. ]{0,24}"
    ) {
        prop_assume!(!is_well_formed_api(&text));
        let found = errors(&set(client(), "/api", json!(text)));
        prop_assert!(found.iter().any(|code| code == "manifest.schema"));
    }
}

fn id_codes(id: &str) -> Vec<String> {
    errors(&set(client(), "/id", json!(id)))
}

/// REQ: PKG-014
#[test]
fn pkg_014_valid_ids_are_accepted() {
    let long_segment = repeat('a', 32);
    let eight_segments = "a.b.c.d.e.f.g.h".to_owned();
    for id in [
        "example.demo",
        "com.acme.git-tools",
        "a.b",
        "1a.2b",
        "a-b.c-d",
        eight_segments.as_str(),
        &format!("{long_segment}.x"),
    ] {
        assert_clean(&set(client(), "/id", json!(id)));
    }
}

/// REQ: PKG-014
#[test]
fn pkg_014_invalid_ids_refused_id_invalid() {
    let segment_33 = repeat('a', 33);
    let nine_segments = "a.b.c.d.e.f.g.h.i".to_owned();
    for id in [
        "demo",
        "Example.Demo",
        "example..demo",
        "example.-demo",
        "example.demo-",
        "-example.demo",
        "example.demo.",
        ".example.demo",
        "ex_ample.demo",
        "example.de mo",
        "example.dé",
        nine_segments.as_str(),
        &format!("{segment_33}.x"),
    ] {
        assert_has(&id_codes(id), "manifest.id-invalid");
    }
}

/// REQ: PKG-014
#[test]
fn pkg_014_segment_length_boundary_32_and_33() {
    assert_lacks(
        &id_codes(&format!("{}.x", repeat('a', 32))),
        "manifest.id-invalid",
    );
    assert_has(
        &id_codes(&format!("{}.x", repeat('a', 33))),
        "manifest.id-invalid",
    );
    assert_lacks(
        &id_codes(&format!("x.{}", repeat('a', 1))),
        "manifest.id-invalid",
    );
}

/// REQ: PKG-014
#[test]
fn pkg_014_segment_count_boundary_8_and_9() {
    assert_lacks(&id_codes("a.b.c.d.e.f.g.h"), "manifest.id-invalid");
    assert_has(&id_codes("a.b.c.d.e.f.g.h.i"), "manifest.id-invalid");
    assert_has(&id_codes("a"), "manifest.id-invalid");
}

/// REQ: PKG-014
#[test]
fn pkg_014_whole_id_of_128_characters_is_accepted() {
    // 7 segments of 16 characters and one of 9: 112 + 9 + 7 dots = 128.
    let mut segments: Vec<String> = (0..7).map(|_| repeat('a', 16)).collect();
    segments.push(repeat('b', 9));
    let id = segments.join(".");
    assert_eq!(id.len(), 128);
    assert_clean(&set(client(), "/id", json!(id)));
}

/// REQ: PKG-014
#[test]
fn pkg_014_whole_id_of_129_characters_is_refused() {
    let mut segments: Vec<String> = (0..7).map(|_| repeat('a', 16)).collect();
    segments.push(repeat('b', 10));
    let id = segments.join(".");
    assert_eq!(id.len(), 129);
    let found = id_codes(&id);
    // The segment grammar is valid; only the independent maxLength fails (PKG-562).
    assert_codes(&found, &["manifest.schema"]);
}

/// REQ: PKG-014
#[test]
fn pkg_014_reserved_first_segments_refused_id_reserved() {
    for id in [
        "ilium.demo",
        "core.demo",
        "local.demo",
        "test.demo",
        "ilium.core.titles",
    ] {
        assert_has(&id_codes(id), "manifest.id-reserved");
    }
}

/// REQ: PKG-014
#[test]
fn pkg_014_names_that_only_start_with_a_reserved_word_are_not_reserved() {
    for id in [
        "coreutils.demo",
        "testing.demo",
        "locale.demo",
        "my.core.demo",
        "iliumx.demo",
    ] {
        let found = id_codes(id);
        assert_lacks(&found, "manifest.id-reserved");
        assert_lacks(&found, "manifest.id-invalid");
    }
}

/// REQ: PKG-014
#[test]
fn pkg_014_ilium_prefix_is_allowed_for_the_release_itself() {
    let value = set(packed(client()), "/id", json!("ilium.core.demo"));
    assert_lacks(&install_errors(&value, Tier::T1), "manifest.id-reserved");
    assert_has(&install_errors(&value, Tier::T3), "manifest.id-reserved");
}

proptest! {
    // REQ: PKG-014
    #[test]
    fn pkg_014_ids_built_from_the_grammar_pass_the_id_rules(
        segments in prop::collection::vec("[a-z0-9]([a-z0-9-]{0,30}[a-z0-9])?", 2..=8)
    ) {
        let id = segments.join(".");
        prop_assume!(id.len() <= 128);
        prop_assume!(!["ilium", "core", "local", "test"].contains(&segments[0].as_str()));
        let found = id_codes(&id);
        prop_assert!(!found.iter().any(|code| code == "manifest.id-invalid"));
        prop_assert!(!found.iter().any(|code| code == "manifest.id-reserved"));
    }

    // REQ: PKG-014
    #[test]
    fn pkg_014_an_upper_case_letter_in_any_id_is_invalid(
        head in "[a-z0-9]{1,8}", upper in "[A-Z]", tail in "[a-z0-9]{0,8}"
    ) {
        let id = format!("{head}.{upper}{tail}");
        prop_assert!(id_codes(&id).iter().any(|code| code == "manifest.id-invalid"));
    }
}

/// REQ: PKG-015
#[test]
fn pkg_015_name_boundaries_64_and_65_characters() {
    assert_clean(&set(client(), "/name", json!(repeat('n', 64))));
    assert_has(
        &errors(&set(client(), "/name", json!(repeat('n', 65)))),
        "manifest.schema",
    );
}

/// REQ: PKG-015
#[test]
fn pkg_015_name_follows_the_display_text_rule() {
    assert_has(
        &errors(&set(client(), "/name", json!("A\nB"))),
        "manifest.display-name",
    );
}

/// REQ: PKG-015
#[test]
fn pkg_015_missing_name_refused_key_missing() {
    assert_has(&errors(&unset(client(), "/name")), "manifest.key-missing");
}

/// REQ: PKG-015
#[test]
fn pkg_015_name_of_a_wrong_type_refused_schema() {
    assert_has(
        &errors(&set(client(), "/name", json!(5))),
        "manifest.schema",
    );
}

fn version_codes(version: &str) -> Vec<String> {
    errors(&set(client(), "/version", json!(version)))
}

/// REQ: PKG-016
#[test]
fn pkg_016_valid_semver_strings_are_accepted() {
    for version in [
        "0.0.0",
        "1.2.3",
        "10.20.30",
        "1.0.0-alpha",
        "1.0.0-alpha.1",
        "1.0.0-0.3.7",
        "1.0.0+build.5",
        "1.0.0-rc.1+build.5",
        "1.0.0-x-y-z",
        "1.0.0-0",
    ] {
        assert_lacks(&version_codes(version), "manifest.version-invalid");
    }
}

/// REQ: PKG-016
#[test]
fn pkg_016_invalid_strings_refused_version_invalid() {
    for version in [
        "v1.0.0",
        "1.0",
        "1",
        "1.0.0.0",
        "01.0.0",
        "1.00.0",
        "1.0.00",
        "1.0.0-",
        "1.0.0-01",
        "1.0.0+",
        "1.0.0-alpha..1",
        "",
        " 1.0.0",
        "1.0.0 ",
        "1.0.0-alpha_1",
        "1.-1.0",
        "x.y.z",
    ] {
        assert_has(&version_codes(version), "manifest.version-invalid");
    }
}

/// REQ: PKG-016
#[test]
fn pkg_016_leading_v_refused_with_the_exact_message() {
    let found = error_diagnostics(&set(client(), "/version", json!("v1.2.3")));
    let diagnostic = first_with(&found, "manifest.version-invalid");
    assert_eq!(
        diagnostic.message,
        "The version \"v1.2.3\" is not a valid semantic version."
    );
}

/// REQ: PKG-016
#[test]
fn pkg_016_length_boundary_64_and_65_characters() {
    let at_limit = format!("1.0.0-{}", repeat('a', 58));
    assert_eq!(at_limit.len(), 64);
    assert_clean(&set(client(), "/version", json!(at_limit)));
    let over = format!("1.0.0-{}", repeat('a', 59));
    assert_eq!(over.len(), 65);
    assert_has(&version_codes(&over), "manifest.schema");
}

proptest! {
    // REQ: PKG-016
    #[test]
    fn pkg_016_plain_three_part_versions_are_accepted(
        major in 0u32..1000, minor in 0u32..1000, patch in 0u32..1000
    ) {
        let found = version_codes(&format!("{major}.{minor}.{patch}"));
        prop_assert!(!found.iter().any(|code| code == "manifest.version-invalid"));
    }

    // REQ: PKG-016
    #[test]
    fn pkg_016_a_leading_v_is_always_refused(
        major in 0u32..1000, minor in 0u32..1000, patch in 0u32..1000
    ) {
        let found = version_codes(&format!("v{major}.{minor}.{patch}"));
        prop_assert!(found.iter().any(|code| code == "manifest.version-invalid"));
    }
}

/// REQ: PKG-017
#[test]
fn pkg_017_publisher_boundaries_64_and_65_characters() {
    assert_clean(&set(client(), "/publisher", json!(repeat('p', 64))));
    assert_has(
        &errors(&set(client(), "/publisher", json!(repeat('p', 65)))),
        "manifest.schema",
    );
}

/// REQ: PKG-017
#[test]
fn pkg_017_publisher_is_display_text() {
    assert_has(
        &errors(&set(client(), "/publisher", json!("P\u{202e}q"))),
        "manifest.display-name",
    );
    assert_has(
        &errors(&unset(client(), "/publisher")),
        "manifest.key-missing",
    );
}

/// REQ: PKG-017
#[test]
fn pkg_017_publisher_is_not_an_identity_any_text_is_accepted_in_a_source_manifest() {
    assert_clean(&set(client(), "/publisher", json!("Anyone At All (Ltd.)")));
}

/// REQ: PKG-017
#[test]
fn pkg_017_doc_publisher_mismatch_is_a_stage_eight_trust_check() {
    let row = catalogue()
        .into_iter()
        .find(|row| row.code == "manifest.publisher-mismatch")
        .expect("publisher-mismatch is in the catalogue");
    assert_eq!(row.stage, "8");
}

/// REQ: PKG-018
#[test]
fn pkg_018_description_boundaries_500_and_501_characters() {
    assert_clean(&set(client(), "/description", json!(repeat('d', 500))));
    assert_has(
        &errors(&set(client(), "/description", json!(repeat('d', 501)))),
        "manifest.schema",
    );
}

/// REQ: PKG-018
#[test]
fn pkg_018_description_with_a_control_character_refused_display_name() {
    assert_has(
        &errors(&set(
            client(),
            "/description",
            json!("one\u{0007}paragraph"),
        )),
        "manifest.display-name",
    );
}

/// REQ: PKG-018
#[test]
fn pkg_018_description_may_be_omitted_or_empty() {
    assert_clean(&unset(client(), "/description"));
    assert_clean(&set(client(), "/description", json!("")));
    let boundary = errors(&set(client(), "/description", json!(repeat('d', 500))));
    assert_eq!(
        boundary,
        Vec::<String>::new(),
        "500 characters is the accepted maximum"
    );
    assert_lacks(
        &errors(&set(client(), "/description", json!(repeat('d', 501)))),
        "manifest.display-name",
    );
}

/// REQ: PKG-019
#[test]
fn pkg_019_license_length_boundaries_1_128_129() {
    assert_clean(&set(client(), "/license", json!("M")));
    assert_clean(&set(client(), "/license", json!(repeat('l', 128))));
    assert_has(
        &errors(&set(client(), "/license", json!(repeat('l', 129)))),
        "manifest.schema",
    );
    assert_has(
        &errors(&set(client(), "/license", json!(""))),
        "manifest.schema",
    );
}

/// REQ: PKG-019
#[test]
fn pkg_019_host_checks_only_the_length_of_the_license() {
    // Not a known SPDX expression, still accepted by the host.
    let odd = set(client(), "/license", json!("Not-An-SPDX-Id OR whatever"));
    assert_eq!(errors(&odd), Vec::<String>::new());
    assert_clean(&set(client(), "/license", json!(repeat('x', 128))));
    assert_has(
        &errors(&set(client(), "/license", json!(repeat('x', 129)))),
        "manifest.schema",
    );
}

/// REQ: PKG-020
#[test]
fn pkg_020_icon_pattern_and_length() {
    assert_clean(&set(client(), "/icon", json!("puzzle")));
    assert_clean(&set(client(), "/icon", json!("a")));
    assert_clean(&set(client(), "/icon", json!(repeat('i', 64))));
    for icon in [
        "Puzzle",
        "-puzzle",
        "puzzle-",
        "pu zzle",
        "pu_zzle",
        "",
        &repeat('i', 65),
    ] {
        assert_has(
            &errors(&set(client(), "/icon", json!(icon))),
            "manifest.schema",
        );
    }
}

/// REQ: PKG-020
#[test]
fn pkg_020_unknown_icon_name_is_never_an_install_error() {
    let value = set(
        packed(client()),
        "/icon",
        json!("no-such-icon-in-the-client"),
    );
    assert!(install_errors(&value, Tier::T3).is_empty());
}

fn categories_codes(value: Value) -> Vec<String> {
    errors(&set(client(), "/categories", value))
}

/// REQ: PKG-021
#[test]
fn pkg_021_one_to_three_distinct_known_categories_are_accepted() {
    assert_clean(&set(client(), "/categories", json!(["tools"])));
    assert_clean(&set(client(), "/categories", json!(["tools", "git"])));
    assert_clean(&set(
        client(),
        "/categories",
        json!(["agents", "git", "voice"]),
    ));
}

/// REQ: PKG-021
#[test]
fn pkg_021_every_category_of_the_list_is_accepted() {
    for category in [
        "agents",
        "appearance",
        "scenes",
        "editors",
        "git",
        "inference",
        "notifications",
        "search",
        "terminal",
        "tools",
        "voice",
    ] {
        assert_clean(&set(client(), "/categories", json!([category])));
    }
}

/// REQ: PKG-021
#[test]
fn pkg_021_empty_four_duplicate_and_unknown_categories_refused_schema() {
    assert_has(&categories_codes(json!([])), "manifest.schema");
    assert_has(
        &categories_codes(json!(["tools", "git", "voice", "agents"])),
        "manifest.schema",
    );
    assert_has(
        &categories_codes(json!(["tools", "tools"])),
        "manifest.schema",
    );
    assert_has(&categories_codes(json!(["bogus"])), "manifest.schema");
    assert_has(&categories_codes(json!("tools")), "manifest.schema");
}

/// REQ: PKG-021
#[test]
fn pkg_021_a_value_outside_the_list_is_refused_at_publish_not_with_a_registry_code_here() {
    let found = categories_codes(json!(["bogus"]));
    assert_lacks(&found, "registry.category-unknown");
}

/// REQ: PKG-022
#[test]
fn pkg_022_keywords_count_and_length_boundaries() {
    let eight: Vec<String> = (0..8).map(|index| format!("k{index}")).collect();
    let nine: Vec<String> = (0..9).map(|index| format!("k{index}")).collect();
    assert_clean(&set(client(), "/keywords", json!(eight)));
    assert_has(
        &errors(&set(client(), "/keywords", json!(nine))),
        "manifest.schema",
    );
    assert_clean(&set(client(), "/keywords", json!([repeat('w', 24)])));
    assert_has(
        &errors(&set(client(), "/keywords", json!([repeat('w', 25)]))),
        "manifest.schema",
    );
}

/// REQ: PKG-022
#[test]
fn pkg_022_keywords_without_spaces_distinct_and_non_empty() {
    assert_has(
        &errors(&set(client(), "/keywords", json!(["two words"]))),
        "manifest.schema",
    );
    assert_has(
        &errors(&set(client(), "/keywords", json!([""]))),
        "manifest.schema",
    );
    assert_has(
        &errors(&set(client(), "/keywords", json!(["dup", "dup"]))),
        "manifest.schema",
    );
}

fn link_codes(key: &str, value: &str) -> Vec<String> {
    errors(&set(client(), &format!("/{key}"), json!(value)))
}

/// REQ: PKG-023
#[test]
fn pkg_023_https_addresses_are_accepted_for_homepage_and_repository() {
    for key in ["homepage", "repository"] {
        assert_lacks(
            &link_codes(key, "https://example.com/plugin"),
            "manifest.schema",
        );
    }
}

/// REQ: PKG-023
#[test]
fn pkg_023_other_schemes_and_spaces_refused_schema() {
    for key in ["homepage", "repository"] {
        for value in [
            "http://example.com",
            "ftp://example.com",
            "example.com",
            "https://a b",
            "https://",
        ] {
            assert_has(&link_codes(key, value), "manifest.schema");
        }
    }
}

/// REQ: PKG-023
#[test]
fn pkg_023_length_boundary_256_and_257_characters() {
    let at_limit = format!("https://{}", repeat('h', 256 - "https://".len()));
    assert_eq!(at_limit.len(), 256);
    assert_lacks(&link_codes("homepage", &at_limit), "manifest.schema");
    let over = format!("{at_limit}h");
    assert_has(&link_codes("repository", &over), "manifest.schema");
}

/// REQ: PKG-024
#[test]
fn pkg_024_packed_manifest_inventory_lists_the_declared_component() {
    let found = install_errors(&packed(client()), Tier::T3);
    assert_lacks(&found, "manifest.file-missing");
}

/// REQ: PKG-024
#[test]
fn pkg_024_declared_component_missing_from_the_inventory_refused_file_missing() {
    let mut value = packed(client());
    value["files"] = json!([file_entry("assets/a.png", 1)]);
    assert_has(&install_errors(&value, Tier::T3), "manifest.file-missing");
}

/// REQ: PKG-024
#[test]
fn pkg_024_every_declared_side_needs_its_component() {
    let mut value = set(
        client(),
        "/server",
        json!({ "entry": "server.wasm", "world": "ilium:server/base@0.1.0" }),
    );
    value["files"] = json!([file_entry("client.wasm", 1)]);
    let found = install_errors(&value, Tier::T3);
    assert_has(&found, "manifest.file-missing");
}

/// REQ: PKG-025 MAY
#[test]
fn pkg_025_capabilities_may_be_present_or_omitted() {
    assert_clean(&client());
    assert_clean(&client_with_grants(&["tree.read"]));
    assert_clean(&set(client(), "/capabilities", json!({})));
}

/// REQ: PKG-025
#[test]
fn pkg_025_only_server_client_and_scene_keys_are_allowed() {
    let value = set(client(), "/capabilities/browser", json!(["tree.read"]));
    assert_has(&errors(&value), "manifest.unknown-key");
}

/// REQ: PKG-025
#[test]
fn pkg_025_list_for_a_side_without_a_component_refused_grant_side() {
    let value = set(client(), "/capabilities/server", json!(["tree.read"]));
    assert_has(&errors(&value), "manifest.grant-side");
}

/// REQ: PKG-025
#[test]
fn pkg_025_list_boundary_256_and_257_distinct_grants() {
    let grants = |count: usize| -> Vec<String> {
        (0..count)
            .map(|index| format!("topics.subscribe:topic-{index}"))
            .collect()
    };
    let at_limit = set(server(), "/capabilities/server", json!(grants(256)));
    assert_lacks(&errors(&at_limit), "manifest.schema");
    let over = set(server(), "/capabilities/server", json!(grants(257)));
    assert_has(&errors(&over), "manifest.schema");
}

/// REQ: PKG-025
#[test]
fn pkg_025_duplicate_grants_in_one_list_are_refused() {
    // The single grant alone is clean, so the refusal below comes from the duplicate.
    assert_clean(&server_with_grants(&["tree.read"]));
    let found = errors(&server_with_grants(&["tree.read", "tree.read"]));
    // manual does not pin the code for a duplicate grant
    assert!(!found.is_empty(), "duplicate grants must be refused");
    assert_lacks(&found, "manifest.key-missing");
}

/// REQ: PKG-026
#[test]
fn pkg_026_activation_boundary_32_and_33_triggers() {
    let triggers = |count: usize| -> Vec<String> {
        (0..count)
            .map(|index| format!("event:ev-{index}"))
            .collect()
    };
    assert_lacks(
        &errors(&set(client(), "/activation", json!(triggers(32)))),
        "manifest.schema",
    );
    assert_has(
        &errors(&set(client(), "/activation", json!(triggers(33)))),
        "manifest.schema",
    );
}

/// REQ: PKG-026
#[test]
fn pkg_026_duplicate_triggers_refused_schema() {
    let value = set(client(), "/activation", json!(["startup", "startup"]));
    assert_has(&errors(&value), "manifest.schema");
}

/// REQ: PKG-026
#[test]
fn pkg_026_unknown_trigger_kind_refused_schema() {
    let value = set(client(), "/activation", json!(["bogus:thing"]));
    assert_has(&errors(&value), "manifest.schema");
}

/// REQ: PKG-026
#[test]
fn pkg_026_startup_and_durable_timer_triggers_are_accepted() {
    assert_clean(&set(
        client(),
        "/activation",
        json!(["startup", "durable-timer"]),
    ));
}

/// REQ: PKG-027
#[test]
fn pkg_027_plugin_listing_itself_refused_requires_self() {
    let value = set(
        client(),
        "/requires",
        json!({ "plugins": [{ "id": "example.demo" }] }),
    );
    let found = error_diagnostics(&value);
    let diagnostic = first_with(&found, "manifest.requires-self");
    assert_eq!(
        diagnostic.message,
        "The plugin example.demo lists itself in requires.plugins."
    );
}

/// REQ: PKG-027
#[test]
fn pkg_027_malformed_requires_values_respect_schema_before_semantics() {
    for (requires, code) in [
        (
            json!({ "host": "not a range" }),
            "manifest.requires-invalid",
        ),
        (
            json!({ "plugins": [{ "id": "Bad Id" }] }),
            "manifest.schema",
        ),
        (
            json!({ "plugins": [{ "id": "com.other.a" }, { "id": "com.other.a" }] }),
            "manifest.requires-invalid",
        ),
        (json!({ "wait_ms": 5001 }), "manifest.schema"),
        (json!({ "services": [""] }), "manifest.schema"),
    ] {
        let found = errors(&set(client(), "/requires", requires));
        assert_has(&found, code);
    }
}

/// REQ: PKG-027
#[test]
fn pkg_027_wait_ms_boundaries_0_5000_5001() {
    for wait_ms in [0, 5000] {
        assert_clean(&set(client(), "/requires", json!({ "wait_ms": wait_ms })));
    }
    assert_codes(
        &errors(&set(client(), "/requires", json!({ "wait_ms": 5001 }))),
        &["manifest.schema"],
    );
}

/// REQ: PKG-027
#[test]
fn pkg_027_a_well_formed_requires_object_is_accepted() {
    let requires = json!({
        "host": ">=0.1.0",
        "services": ["ilium:inference/provider@0.1.0"],
        "plugins": [
            { "id": "com.example.git", "version": "^1.2", "optional": false },
            { "id": "com.example.theme-pack", "version": "*", "optional": true }
        ],
        "wait_ms": 2000
    });
    let found = errors(&set(client(), "/requires", requires));
    assert_lacks(&found, "manifest.requires-invalid");
    assert_lacks(&found, "manifest.requires-self");
    assert_lacks(&found, "manifest.unknown-key");
}

/// REQ: PKG-027
#[test]
fn pkg_027_plugin_requirement_defaults_are_any_version_and_not_optional() {
    let parsed: ilium_plugin_api::manifest::PluginRequirement =
        serde_json::from_value(json!({ "id": "com.example.git" })).expect("deserializes");
    assert_eq!(parsed.version, "*");
    assert!(!parsed.optional);
}

/// REQ: PKG-027
#[test]
fn pkg_027_service_requirement_parses_name_and_range() {
    use ilium_plugin_api::manifest::ServiceRequirement;
    let parsed = ServiceRequirement::parse("com.example.git/status@1.2.3").expect("parses");
    assert_eq!(parsed.name, "com.example.git/status");
    assert_eq!(parsed.version, "1.2.3");
    let kernel = ServiceRequirement::parse("ilium:inference/provider@0.1.0").expect("parses");
    assert_eq!(kernel.name, "ilium:inference/provider");
    assert_eq!(kernel.version, "0.1.0");
}

/// REQ: PKG-027
#[test]
fn pkg_027_service_requirement_requires_a_full_contract_version() {
    use ilium_plugin_api::manifest::ServiceRequirement;
    let refused =
        ServiceRequirement::parse("com.example.git/status").expect_err("version required");
    assert_eq!(refused.code.as_str(), "manifest.service-version");
}

/// REQ: PKG-027
#[test]
fn pkg_027_bad_service_requirement_refused_service_version() {
    use ilium_plugin_api::manifest::ServiceRequirement;
    for text in ["", "@1.0.0", "name@", "bad name@1.0.0"] {
        let refused = ServiceRequirement::parse(text).expect_err(text);
        assert_eq!(refused.code.as_str(), "manifest.service-version", "{text}");
    }
}

/// REQ: PKG-027
#[test]
fn pkg_027_list_boundaries_32_and_33_for_services_and_plugins() {
    let services = |count: usize| -> Vec<String> {
        (0..count)
            .map(|index| format!("com.example.s{index}/api@1.0.0"))
            .collect()
    };
    let plugins = |count: usize| -> Vec<Value> {
        (0..count)
            .map(|index| json!({ "id": format!("com.other.p{index}") }))
            .collect()
    };
    {
        let count = 32usize;
        assert_clean(&set(
            client(),
            "/requires",
            json!({ "services": services(count) }),
        ));
        assert_clean(&set(
            client(),
            "/requires",
            json!({ "plugins": plugins(count) }),
        ));
    }
    assert_has(
        &errors(&set(
            client(),
            "/requires",
            json!({ "services": services(33) }),
        )),
        "manifest.schema",
    );
    assert_has(
        &errors(&set(
            client(),
            "/requires",
            json!({ "plugins": plugins(33) }),
        )),
        "manifest.schema",
    );
}

fn replaces_errors(replaces: Value, tier: Tier) -> Vec<String> {
    install_errors(&set(packed(client()), "/replaces", replaces), tier)
}

/// REQ: PKG-028
#[test]
fn pkg_028_replacing_a_known_replaceable_core_plugin_is_accepted_for_a_bundled_package() {
    let found = replaces_errors(json!(["ilium.core.titles"]), Tier::T1);
    assert_lacks(&found, "manifest.replaces-unknown");
    assert_lacks(&found, "manifest.replaces-not-replaceable");
    assert_lacks(&found, "manifest.replaces-tier");
}

/// REQ: PKG-028
#[test]
fn pkg_028_unknown_core_plugin_refused_replaces_unknown() {
    let found = replaces_errors(json!(["ilium.core.nothing"]), Tier::T1);
    assert_has(&found, "manifest.replaces-unknown");
}

/// REQ: PKG-028
#[test]
fn pkg_028_core_plugin_that_is_not_replaceable_refused_not_replaceable() {
    let found = replaces_errors(json!(["ilium.core.sessions"]), Tier::T1);
    assert_has(&found, "manifest.replaces-not-replaceable");
}

/// REQ: PKG-028
#[test]
fn pkg_028_list_longer_than_four_refused_schema() {
    let four = json!(["a.one", "a.two", "a.three", "a.four"]);
    let five = json!(["a.one", "a.two", "a.three", "a.four", "a.five"]);
    assert_lacks(
        &errors(&set(client(), "/replaces", four)),
        "manifest.schema",
    );
    assert_has(
        &errors(&set(client(), "/replaces", five)),
        "manifest.schema",
    );
}

/// REQ: PKG-028
#[test]
fn pkg_028_duplicate_entries_refused_schema() {
    let found = errors(&set(
        client(),
        "/replaces",
        json!(["ilium.core.titles", "ilium.core.titles"]),
    ));
    assert_has(&found, "manifest.schema");
}

/// REQ: PKG-028
#[test]
fn pkg_028_untrusted_package_replacing_a_core_plugin_refused_replaces_tier() {
    let found = replaces_errors(json!(["ilium.core.titles"]), Tier::T3);
    assert_has(&found, "manifest.replaces-tier");
}

/// REQ: PKG-029
#[test]
fn pkg_029_third_party_replaceable_refused() {
    let value = set(
        set(packed(client()), "/replaceable", json!(true)),
        "/surface",
        json!(["services:suggest"]),
    );
    let found = install_errors(&value, Tier::T3);
    assert!(
        found
            .iter()
            .any(|code| code == "manifest.limits-tier" || code == "manifest.schema"),
        "{found:?}"
    );
}

/// REQ: PKG-029
#[test]
fn pkg_029_replaceable_true_needs_a_non_empty_surface() {
    let without = set(client(), "/replaceable", json!(true));
    assert_has(&errors(&without), "manifest.schema");
    let empty = set(without, "/surface", json!([]));
    assert_has(&errors(&empty), "manifest.schema");
}

/// REQ: PKG-029
#[test]
fn pkg_029_surface_without_replaceable_refused_schema() {
    let value = set(client(), "/surface", json!(["services:suggest"]));
    assert_has(&errors(&value), "manifest.schema");
}

/// REQ: PKG-029
#[test]
fn pkg_029_surface_entries_have_a_known_kind() {
    for kind in [
        "services",
        "points",
        "commands",
        "pane-kinds",
        "topics",
        "cli",
    ] {
        let value = set(
            set(client(), "/replaceable", json!(true)),
            "/surface",
            json!([format!("{kind}:name")]),
        );
        assert_lacks(&errors(&value), "manifest.schema");
    }
    let value = set(
        set(client(), "/replaceable", json!(true)),
        "/surface",
        json!(["widgets:name"]),
    );
    assert_has(&errors(&value), "manifest.schema");
}

/// REQ: PKG-030
#[test]
fn pkg_030_unknown_contributes_key_refused_unknown_key() {
    let value = set(client(), "/contributes/themes", json!([]));
    assert_has(&errors(&value), "manifest.unknown-key");
}

/// REQ: PKG-030
#[test]
fn pkg_030_every_documented_contributes_key_is_known() {
    let keys = [
        "commands",
        "keybindings",
        "settings",
        "help",
        "decoration_slots",
        "virtual_rows",
        "toolbar_buttons",
        "context_menu",
        "panels",
        "pane_kinds",
        "scenes",
        "signatures",
        "output_filters",
        "link_detectors",
        "editor_line_actions",
        "sounds",
        "cli",
        "http_routes",
        "services",
        "topics",
        "jobs",
        "onboarding_steps",
        "extension_points",
        "extensions",
        "tree_toolbar",
        "row_actions",
        "tree_orderings",
        "status_segments",
        "pane_frame_slots",
        "display_modes",
        "catalogs",
        "client_requests",
        "languages",
    ];
    for key in keys {
        let value = set(client(), &format!("/contributes/{key}"), json!([]));
        assert_lacks(&errors(&value), "manifest.unknown-key");
    }
}

/// REQ: PKG-031
#[test]
fn pkg_031_unknown_limits_key_refused_unknown_key() {
    let value = set(client(), "/limits/not_a_limit", json!(1));
    assert_has(&errors(&value), "manifest.unknown-key");
}

/// REQ: PKG-031
#[test]
fn pkg_031_request_above_the_ceiling_is_never_refused() {
    let value = set(client(), "/limits/memory_mib", json!(4_294_967_295u64));
    assert_clean(&value);
    assert!(install_errors(&packed(value), Tier::T3).is_empty());
}

/// REQ: PKG-031
#[test]
fn pkg_031_every_limit_key_of_the_table_is_a_known_key() {
    let text = page("reference-manifest.md");
    let table = section(&text, "## Limits object");
    let mut checked = 0;
    for cells in rows_of(&table) {
        if cells.len() == 4
            && cells[0].starts_with('`')
            && cells[1].chars().all(|c| c.is_ascii_digit())
        {
            let key = unquote(&cells[0]);
            let value = set(client(), &format!("/limits/{key}"), json!(1));
            assert_lacks(&errors(&value), "manifest.unknown-key");
            checked += 1;
        }
    }
    assert!(
        checked >= 40,
        "numeric-default rows of the limits table: {checked}"
    );
}

fn data_version_codes(text: &str) -> Vec<String> {
    text_errors(&with_raw_member(
        &client(),
        &format!("\"data_version\":{text}"),
    ))
}

/// REQ: PKG-032
#[test]
fn pkg_032_absent_data_version_means_one() {
    let parsed = parse_manifest(&client().to_string()).expect("parses");
    assert_eq!(parsed.data_version, DEFAULT_DATA_VERSION);
    assert_eq!(DEFAULT_DATA_VERSION, 1);
}

/// REQ: PKG-032
#[test]
fn pkg_032_boundaries_1_and_4294967295_are_accepted() {
    assert_lacks(&data_version_codes("1"), "manifest.data-version");
    assert_lacks(&data_version_codes("2"), "manifest.data-version");
    assert_lacks(&data_version_codes("4294967295"), "manifest.data-version");
}

/// REQ: PKG-032
#[test]
fn pkg_032_zero_negative_and_too_large_refused_data_version() {
    for text in [
        "0",
        "-1",
        "4294967296",
        "1.5",
        "2.0",
        "\"2\"",
        "null",
        "true",
    ] {
        assert_has(&data_version_codes(text), "manifest.data-version");
    }
}

/// REQ: PKG-032
#[test]
fn pkg_032_zero_is_refused_with_the_exact_message() {
    let found = diagnostics_of_text(
        &with_raw_member(&client(), "\"data_version\":0"),
        Tier::T3,
        CheckMode::Check,
    );
    let diagnostic = first_with(&found, "manifest.data-version");
    assert_eq!(
        diagnostic.message,
        "data_version must be an integer from 1 to 4294967295. Found 0."
    );
}

/// REQ: PKG-033
#[test]
fn pkg_033_standard_is_the_default_and_the_only_other_value_is_first_party() {
    let parsed = parse_manifest(&client().to_string()).expect("parses");
    assert_eq!(
        parsed.limits_tier,
        ilium_plugin_api::manifest::LimitsTier::Standard
    );
    assert_has(
        &errors(&set(client(), "/limits_tier", json!("bogus"))),
        "manifest.schema",
    );
    assert_lacks(
        &errors(&set(client(), "/limits_tier", json!("standard"))),
        "manifest.schema",
    );
    assert_lacks(
        &errors(&set(client(), "/limits_tier", json!("first-party"))),
        "manifest.schema",
    );
}

/// REQ: PKG-033
#[test]
fn pkg_033_bundled_ilium_plugin_may_use_first_party() {
    let value = set(
        set(packed(client()), "/id", json!("ilium.core.demo")),
        "/limits_tier",
        json!("first-party"),
    );
    assert_lacks(&install_errors(&value, Tier::T1), "manifest.limits-tier");
}

/// REQ: PKG-033
#[test]
fn pkg_033_any_other_package_refused_limits_tier_at_stage_nine() {
    let value = set(packed(client()), "/limits_tier", json!("first-party"));
    for tier in [Tier::T2, Tier::T3] {
        let found = diagnostics(&value, tier, CheckMode::Install);
        let diagnostic = first_with(&found, "manifest.limits-tier");
        assert_eq!(diagnostic.stage, Some(Stage(9)));
    }
}

/// REQ: PKG-033
#[test]
fn pkg_033_first_party_on_a_bundled_package_without_an_ilium_id_refused() {
    let value = set(packed(client()), "/limits_tier", json!("first-party"));
    assert_has(&install_errors(&value, Tier::T1), "manifest.limits-tier");
}

fn unstable_codes(names: Value, tier: Tier, mode: CheckMode) -> Vec<String> {
    error_codes(&diagnostics(
        &set(packed(client()), "/unstable", names),
        tier,
        mode,
    ))
}

/// REQ: PKG-034
#[test]
fn pkg_034_every_unstable_feature_of_this_host_is_known() {
    for name in ["ui-calendar", "plugin-services", "gpu-kernels", "websocket"] {
        let found = unstable_codes(json!([name]), Tier::T1, CheckMode::Install);
        assert_lacks(&found, "manifest.unstable-unknown");
    }
}

/// REQ: PKG-034
#[test]
fn pkg_034_unknown_unstable_name_refused_unstable_unknown() {
    let found = unstable_codes(json!(["time-travel"]), Tier::T1, CheckMode::Install);
    assert_has(&found, "manifest.unstable-unknown");
}

/// REQ: PKG-034
#[test]
fn pkg_034_third_party_unstable_outside_development_mode_refused_unstable_tier() {
    for tier in [Tier::T2, Tier::T3] {
        let found = unstable_codes(json!(["websocket"]), tier, CheckMode::Install);
        assert_has(&found, "manifest.unstable-tier");
    }
}

/// REQ: PKG-034
#[test]
fn pkg_034_third_party_unstable_in_development_mode_is_accepted() {
    for tier in [Tier::T2, Tier::T3] {
        let found = unstable_codes(json!(["websocket"]), tier, CheckMode::Dev);
        assert_lacks(&found, "manifest.unstable-tier");
        assert_lacks(&found, "manifest.unstable-unknown");
    }
}

/// REQ: PKG-034
#[test]
fn pkg_034_more_than_eight_names_or_duplicates_refused_schema() {
    let duplicate = errors(&set(
        client(),
        "/unstable",
        json!(["websocket", "websocket"]),
    ));
    assert_has(&duplicate, "manifest.schema");
    // Nine entries exceed the cap of eight; manual does not pin whether overflow or an
    // unknown name is reported first, only that the list is refused with manifest.schema.
    let names: Vec<String> = (0..9).map(|index| format!("feature-{index}")).collect();
    let nine = errors(&set(client(), "/unstable", json!(names)));
    assert_has(&nine, "manifest.schema");
}

fn files_codes(files: Vec<Value>) -> Vec<String> {
    let mut value = client();
    value["files"] = Value::Array(files);
    install_errors(&value, Tier::T3)
}

/// REQ: PKG-035
#[test]
fn pkg_035_sorted_inventory_with_no_repeat_is_accepted() {
    let found = files_codes(vec![
        file_entry("assets/a.png", 1),
        file_entry("client.wasm", 1),
    ]);
    assert_lacks(&found, "manifest.files-order");
}

/// REQ: PKG-035
#[test]
fn pkg_035_unsorted_inventory_refused_files_order() {
    let found = files_codes(vec![
        file_entry("client.wasm", 1),
        file_entry("assets/a.png", 1),
    ]);
    assert_has(&found, "manifest.files-order");
}

/// REQ: PKG-035
#[test]
fn pkg_035_archive_order_is_bytewise_so_upper_case_sorts_before_lower_case() {
    let sorted = files_codes(vec![
        file_entry("assets/Z.png", 1),
        file_entry("assets/a.png", 1),
        file_entry("client.wasm", 1),
    ]);
    assert_lacks(&sorted, "manifest.files-order");
    let wrong = files_codes(vec![
        file_entry("assets/a.png", 1),
        file_entry("assets/Z.png", 1),
        file_entry("client.wasm", 1),
    ]);
    assert_has(&wrong, "manifest.files-order");
}

/// REQ: PKG-035
#[test]
fn pkg_035_a_path_listed_twice_refused_files_order() {
    let found = files_codes(vec![
        file_entry("assets/a.png", 1),
        file_entry("assets/a.png", 1),
        file_entry("client.wasm", 1),
    ]);
    assert_has(&found, "manifest.files-order");
}

/// REQ: PKG-035
#[test]
fn pkg_035_manifest_and_signature_must_not_be_listed_refused_file_self() {
    for path in ["manifest.json", "signature"] {
        let mut paths = [path.to_owned(), "client.wasm".to_owned()];
        paths.sort();
        let files: Vec<Value> = paths.iter().map(|entry| file_entry(entry, 1)).collect();
        assert_has(&files_codes(files), "manifest.file-self");
    }
}

/// REQ: PKG-035
#[test]
fn pkg_035_items_need_exactly_path_length_and_sha256() {
    let mut missing_hash = packed(client());
    missing_hash["files"][0]
        .as_object_mut()
        .expect("item")
        .remove("sha256");
    assert_has(
        &install_errors(&missing_hash, Tier::T3),
        "manifest.key-missing",
    );
    let mut extra = packed(client());
    extra["files"][0]["mode"] = json!(420);
    assert_has(&install_errors(&extra, Tier::T3), "manifest.unknown-key");
}

/// REQ: PKG-035
#[test]
fn pkg_035_validate_json_agrees_with_parse_then_validate_for_a_packed_manifest() {
    let value = packed(client());
    let text = value.to_string();
    let direct = validate_json(&text, &context_with(Tier::T3, CheckMode::Install));
    assert!(
        error_codes(&direct).is_empty(),
        "{:?}",
        error_codes(&direct)
    );
}
