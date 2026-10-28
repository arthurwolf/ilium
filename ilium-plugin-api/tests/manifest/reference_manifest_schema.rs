//! reference-manifest-schema.md: the schema document, the stage mapping and the error catalogue
//! (PKG-560..PKG-577).

use ilium_plugin_api::manifest::{json_schema, CheckMode, Stage};
use ilium_plugin_api::Tier;
use serde_json::{json, Value};

use crate::support::*;

fn shipped_schema() -> Value {
    serde_json::from_str(json_schema()).expect("the shipped schema is JSON")
}

/// REQ: PKG-560
#[test]
fn pkg_560_schema_declares_draft_2020_12_and_the_urn_id() {
    let schema = shipped_schema();
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert_eq!(schema["$id"], "urn:ilium:schema:plugin-manifest:0.1.0");
}

/// REQ: PKG-560
#[test]
fn pkg_560_the_page_copy_declares_the_same_identity() {
    let schema = page_schema();
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert_eq!(schema["$id"], "urn:ilium:schema:plugin-manifest:0.1.0");
}

fn keywords(value: &Value, found: &mut std::collections::BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                found.insert(key.clone());
                keywords(child, found);
            }
        }
        Value::Array(items) => items.iter().for_each(|item| keywords(item, found)),
        _ => {}
    }
}

/// REQ: PKG-560
#[test]
fn pkg_560_schema_uses_no_keyword_outside_the_core_and_validation_vocabularies() {
    const FORBIDDEN: [&str; 8] = [
        "format",
        "contentEncoding",
        "contentMediaType",
        "contentSchema",
        "unevaluatedProperties",
        "$dynamicRef",
        "$vocabulary",
        "x-ilium",
    ];
    let mut found = std::collections::BTreeSet::new();
    keywords(&shipped_schema(), &mut found);
    for keyword in FORBIDDEN {
        // `format` may appear as a property name inside `properties`; the shipped schema has none.
        assert!(
            !found.contains(keyword),
            "keyword {keyword} is outside the allowed vocabularies"
        );
    }
}

/// REQ: PKG-560
#[test]
fn pkg_560_shipped_schema_equals_the_schema_printed_on_the_page() {
    assert_eq!(shipped_schema(), page_schema());
}

fn assert_stage_three_code(value: Value, expected: &str) {
    let found = error_diagnostics(&value);
    assert!(
        found
            .iter()
            .any(|d| d.code.as_str() == expected && d.stage == Some(Stage(3))),
        "expected {expected} at stage 3, got {:?}",
        error_codes(&found)
    );
}

/// REQ: PKG-561
#[test]
fn pkg_561_a_manifest_the_schema_accepts_has_no_stage_three_error() {
    let found = diagnostics(&client(), Tier::T3, CheckMode::Check);
    assert!(found.iter().all(|d| d.stage != Some(Stage(3))));
}

/// REQ: PKG-561
#[test]
fn pkg_561_a_manifest_the_schema_refuses_is_refused_at_stage_three() {
    assert_stage_three_code(set(client(), "/name", json!(7)), "manifest.schema");
    assert_stage_three_code(set(client(), "/zzz", json!(7)), "manifest.unknown-key");
    assert_stage_three_code(unset(client(), "/publisher"), "manifest.key-missing");
}

/// REQ: PKG-561
#[test]
fn pkg_561_self_dependency_uses_catalogued_stage_three() {
    let value = set(
        client(),
        "/requires",
        json!({ "plugins": [{ "id": "example.demo" }] }),
    );
    let found = error_diagnostics(&value);
    // The normative diagnostic catalogue assigns requires-self to stage 3.
    let diagnostic = first_with(&found, "manifest.requires-self");
    assert_eq!(diagnostic.stage, Some(Stage(3)));
}

/// REQ: PKG-562
#[test]
fn pkg_562_each_mapping_row_gives_its_code() {
    assert_has(
        &errors(&set(client(), "/zzz", json!(1))),
        "manifest.unknown-key",
    );
    assert_has(&errors(&unset(client(), "/id")), "manifest.key-missing");
    assert_has(
        &errors(&set(client(), "/id", json!("Bad Id"))),
        "manifest.id-invalid",
    );
    assert_has(
        &errors(&set(client(), "/version", json!("1.0"))),
        "manifest.version-invalid",
    );
    assert_has(
        &errors(&set(client(), "/name", json!("a\nb"))),
        "manifest.display-name",
    );
    assert_has(
        &errors(&set(client(), "/publisher", json!("a\nb"))),
        "manifest.display-name",
    );
    // Descriptions allow LF, unlike single-line names and publishers.
    assert_lacks(
        &errors(&set(client(), "/description", json!("a\nb"))),
        "manifest.display-name",
    );
    assert_has(
        &errors(&set(client(), "/description", json!("a\tb"))),
        "manifest.display-name",
    );
    assert_has(
        &errors(&set(client(), "/data_version", json!(0))),
        "manifest.data-version",
    );
    assert_has(
        &errors(&set(client(), "/homepage", json!("ftp://x"))),
        "manifest.schema",
    );
}

/// REQ: PKG-562
#[test]
fn pkg_562_manifest_errors_follow_bytewise_json_pointer_order() {
    // PKG-578 sorts /name before /zzz, independent of schema traversal.
    let found = errors(&set(set(client(), "/zzz", json!(1)), "/name", json!(5)));
    assert_eq!(found.first().map(String::as_str), Some("manifest.schema"));
    assert_has(&found, "manifest.unknown-key");
}

/// REQ: PKG-562
#[test]
fn pkg_562_data_version_failures_are_never_a_plain_schema_error() {
    for raw in ["0", "-3", "1.5", "4294967296"] {
        let text = with_raw_member(&client(), &format!("\"data_version\":{raw}"));
        let found = text_errors(&text);
        assert_has(&found, "manifest.data-version");
        assert_lacks(&found, "manifest.schema");
    }
}

/// REQ: PKG-563
#[test]
fn pkg_563_the_schema_ships_with_the_crate_and_needs_no_network() {
    let text = json_schema();
    assert!(!text.is_empty());
    assert!(!text.contains("$ref\": \"http"));
    // Calling twice gives the same embedded document.
    assert_eq!(json_schema(), text);
}

/// REQ: PKG-563
#[test]
fn pkg_563_doc_check_print_schema_writes_the_schema_to_standard_output() {
    let text = page("reference-manifest-schema.md");
    assert!(text.contains("ilium plugin check --print-schema"));
}

/// REQ: PKG-564
#[test]
fn pkg_564_reserved_prefixes_refused_id_reserved() {
    for id in ["ilium.demo", "core.demo", "local.demo", "test.demo"] {
        assert_has(
            &errors(&set(client(), "/id", json!(id))),
            "manifest.id-reserved",
        );
    }
}

/// REQ: PKG-564
#[test]
fn pkg_564_ordinary_ids_are_not_reserved() {
    assert_lacks(&errors(&client()), "manifest.id-reserved");
}

/// REQ: PKG-565
#[test]
fn pkg_565_exports_list_must_match_the_real_component() {
    use ilium_plugin_api::manifest::{check_component, ComponentSummary};
    let manifest =
        ilium_plugin_api::manifest::parse_manifest(&client().to_string()).expect("parses");
    let component = ComponentSummary {
        exports: vec!["lifecycle".into(), "handler".into(), "views".into()],
        api_sections: vec![ilium_plugin_api::version::API_VERSION_BYTES.to_vec()],
        ..ComponentSummary::default()
    };
    let found = check_component(
        &manifest,
        ilium_plugin_api::Side::Client,
        &component,
        &context_with(Tier::T3, CheckMode::Install),
    );
    assert_has(&error_codes(&found), "manifest.exports-mismatch");
}

/// REQ: PKG-565
#[test]
fn pkg_565_empty_exports_list_matches_a_component_without_exports() {
    use ilium_plugin_api::manifest::{check_component, ComponentSummary};
    let manifest =
        ilium_plugin_api::manifest::parse_manifest(&client().to_string()).expect("parses");
    let component = ComponentSummary {
        exports: Vec::new(),
        api_sections: vec![ilium_plugin_api::version::API_VERSION_BYTES.to_vec()],
        ..ComponentSummary::default()
    };
    let found = check_component(
        &manifest,
        ilium_plugin_api::Side::Client,
        &component,
        &context_with(Tier::T3, CheckMode::Install),
    );
    assert_lacks(&error_codes(&found), "manifest.exports-mismatch");
}

/// REQ: PKG-566
#[test]
fn pkg_566_unknown_grant_is_refused_on_the_server_side_too() {
    assert_has(
        &errors(&server_with_grants(&["teleport.now"])),
        "manifest.grant-unknown",
    );
}

/// REQ: PKG-566
#[test]
fn pkg_566_grants_must_exist_follow_their_grammar_and_fit_their_side() {
    assert_has(
        &errors(&client_with_grants(&["teleport.now"])),
        "manifest.grant-unknown",
    );
    assert_has(
        &errors(&client_with_grants(&["http"])),
        "manifest.grant-argument-missing",
    );
    assert_has(
        &errors(&client_with_grants(&["tree.read:x"])),
        "manifest.grant-argument",
    );
    assert_has(
        &errors(&client_with_grants(&["sessions.cross"])),
        "manifest.grant-side",
    );
    assert_clean(&client_with_grants(&["tree.read", "http:api.example.com"]));
}

/// REQ: PKG-567
#[test]
fn pkg_567_a_when_expression_that_does_not_parse_refused_when_syntax() {
    let value = set(
        client(),
        "/contributes/commands",
        json!([{ "id": "run", "title": "Run", "when": "pane.kind ==" }]),
    );
    assert_has(&errors(&value), "manifest.when-syntax");
}

/// REQ: PKG-567
#[test]
fn pkg_567_a_valid_when_expression_is_accepted() {
    let value = set(
        client(),
        "/contributes/commands",
        json!([{ "id": "run", "title": "Run", "when": r#"pane.kind == "terminal""# }]),
    );
    assert_lacks(&errors(&value), "manifest.when-syntax");
}

/// REQ: PKG-567
#[test]
fn pkg_567_duplicate_settings_keys_refused_settings_key_clash() {
    let rows = json!([
        { "key": "same", "type": "toggle", "title": "A", "default": false },
        { "key": "same", "type": "toggle", "title": "B", "default": false }
    ]);
    let value = set(
        client(),
        "/contributes/settings",
        json!([
            { "kind": "tab", "id": "main", "title": "Main", "sections": [] },
            { "kind": "section", "id": "one", "tab": "main", "title": "One", "rows": rows }
        ]),
    );
    assert_has(&errors(&value), "manifest.settings-key-clash");
}

/// REQ: PKG-568
#[test]
fn pkg_568_duplicate_ids_in_a_contribution_list_refused() {
    let value = set(
        client_with_grants(&["ui.panels"]),
        "/contributes/panels",
        json!([{ "id": "p", "title": "A" }, { "id": "p", "title": "B" }]),
    );
    let found = errors(&value);
    // manual does not pin the duplicate-id code ("the code of the contribution")
    assert_eq!(found.len(), 1, "exactly one refusal, got {found:?}");
    assert!(
        found[0].starts_with("manifest."),
        "a duplicate id gives a manifest.* code, got {found:?}"
    );
    let diagnostics = error_diagnostics(&value);
    assert!(
        diagnostics[0].pointer.starts_with("/contributes/panels"),
        "pointer names the duplicate: {:?}",
        diagnostics[0].pointer
    );
}

/// REQ: PKG-568
#[test]
fn pkg_568_topics_may_publish_only_under_the_plugin_prefix() {
    let own = set(
        client(),
        "/contributes/topics",
        json!({ "publishes": ["example.demo/ready"] }),
    );
    assert_clean(&own);
    let foreign = set(
        client(),
        "/contributes/topics",
        json!({ "publishes": ["com.other/ready"] }),
    );
    let found = errors(&foreign);
    assert_eq!(
        found.len(),
        1,
        "exactly one refusal for the foreign prefix, got {found:?}"
    );
    // manual does not pin the topic-prefix code
    assert!(
        found[0].starts_with("manifest."),
        "a manifest.* code, got {found:?}"
    );
    let diagnostics = error_diagnostics(&foreign);
    assert!(
        diagnostics[0].pointer.starts_with("/contributes/topics"),
        "pointer names the topic: {:?}",
        diagnostics[0].pointer
    );
}

/// REQ: PKG-568
#[test]
fn pkg_568_commands_global_id_uses_the_plugin_id_prefix() {
    let value = set(
        client(),
        "/contributes/commands",
        json!([{ "id": "run", "title": "Run" }]),
    );
    assert_clean(&value);
    let duplicated = set(
        client(),
        "/contributes/commands",
        json!([{ "id": "run", "title": "Run" }, { "id": "run", "title": "Again" }]),
    );
    let found = errors(&duplicated);
    // manual does not pin the duplicate command id code
    assert_eq!(found.len(), 1, "exactly one refusal, got {found:?}");
    assert!(found[0].starts_with("manifest."), "{found:?}");
}

/// REQ: PKG-569
#[test]
fn pkg_569_requires_ranges_must_parse() {
    for requires in [
        json!({ "host": "garbage" }),
        json!({ "plugins": [{ "id": "com.other.a", "version": "not a range" }] }),
    ] {
        assert_has(
            &errors(&set(client(), "/requires", requires)),
            "manifest.requires-invalid",
        );
    }
}

/// REQ: PKG-569
#[test]
fn pkg_569_duplicate_plugin_requirement_refused_requires_invalid() {
    let value = set(
        client(),
        "/requires",
        json!({ "plugins": [{ "id": "com.other.a" }, { "id": "com.other.a" }] }),
    );
    assert_has(&errors(&value), "manifest.requires-invalid");
}

/// REQ: PKG-569
#[test]
fn pkg_569_requiring_itself_refused_requires_self() {
    let value = set(
        client(),
        "/requires",
        json!({ "plugins": [{ "id": "example.demo" }] }),
    );
    assert_has(&errors(&value), "manifest.requires-self");
}

/// REQ: PKG-570
#[test]
fn pkg_570_each_declared_component_needs_its_file() {
    let mut value = client();
    value["files"] = json!([file_entry("other.bin", 1)]);
    assert_has(&install_errors(&value, Tier::T3), "manifest.file-missing");
}

/// REQ: PKG-570
#[test]
fn pkg_570_help_and_sound_files_named_by_a_contribution_must_exist() {
    let value = set(
        client(),
        "/contributes/help",
        json!([{ "id": "intro", "file": "help/intro.md", "title": "Intro" }]),
    );
    assert_has(
        &install_errors(&packed(value.clone()), Tier::T3),
        "manifest.file-missing",
    );
    assert_lacks(
        &install_errors(&packed_with(value, &["help/intro.md"]), Tier::T3),
        "manifest.file-missing",
    );
}

/// REQ: PKG-571
#[test]
fn pkg_571_catalogue_rows_have_a_stage_number_or_letter_and_an_exit_code() {
    let rows = catalogue();
    assert!(rows.len() > 50, "catalogue rows: {}", rows.len());
    for row in &rows {
        if row.code == "migration.skipped" {
            assert_eq!(row.stage, "-");
            assert_eq!(row.exit, "-");
            assert!(row.template.starts_with("Log line only, not a failure:"));
            continue;
        }
        assert!(!row.stage.is_empty(), "{}", row.code);
        assert!(
            row.exit.chars().all(|c| c.is_ascii_digit()) && !row.exit.is_empty(),
            "{}",
            row.code
        );
    }
}

/// REQ: PKG-571
#[test]
fn pkg_571_a_manifest_error_carries_code_message_stage_and_pointer() {
    let found = error_diagnostics(&set(client(), "/name", json!(5)));
    let diagnostic = first_with(&found, "manifest.schema");
    assert!(!diagnostic.message.is_empty());
    assert_eq!(diagnostic.stage, Some(Stage(3)));
    assert_eq!(diagnostic.pointer, "/name");
}

/// REQ: PKG-572
#[test]
fn pkg_572_a_message_has_no_unreplaced_placeholder() {
    let samples = [
        set(client(), "/id", json!("Bad Id")),
        set(client(), "/version", json!("v1")),
        set(
            client(),
            "/requires",
            json!({ "plugins": [{ "id": "example.demo" }] }),
        ),
        set(client(), "/data_version", json!(0)),
    ];
    for sample in samples {
        let diagnostics = error_diagnostics(&sample);
        assert!(!diagnostics.is_empty(), "each sample is refused");
        for diagnostic in diagnostics {
            // a `{name}` template placeholder, not a brace inside a quoted value
            let message = diagnostic.message.as_str();
            let has_placeholder = message.match_indices('{').any(|(start, _)| {
                let rest = &message[start + 1..];
                rest.find('}').is_some_and(|end| {
                    end > 0
                        && rest[..end]
                            .chars()
                            .all(|c| c.is_ascii_lowercase() || c == '_')
                })
            });
            assert!(!has_placeholder, "unreplaced placeholder in {message:?}");
        }
    }
}

/// REQ: PKG-572
#[test]
fn pkg_572_the_value_of_the_failing_key_appears_in_the_message() {
    let found = error_diagnostics(&set(client(), "/version", json!("v9.9.9")));
    assert!(first_with(&found, "manifest.version-invalid")
        .message
        .contains("v9.9.9"));
}

/// REQ: PKG-572
#[test]
fn pkg_572_doc_templates_name_no_secret_field() {
    for row in catalogue() {
        let lower = row.template.to_lowercase();
        for word in ["{token", "{secret", "{password", "{private_key"] {
            assert!(!lower.contains(word), "{} template holds {word}", row.code);
        }
    }
}

/// REQ: PKG-573
#[test]
fn pkg_573_the_code_is_stable_while_the_message_may_change() {
    let first = error_diagnostics(&set(client(), "/id", json!("Bad")));
    let second = error_diagnostics(&set(client(), "/id", json!("Also Bad")));
    assert_eq!(first[0].code, second[0].code);
    assert!(first[0].message.is_ascii());
}

/// REQ: PKG-574
#[test]
fn pkg_574_the_exit_code_of_the_first_printed_error_is_the_catalogue_exit_of_that_code() {
    let rows = catalogue();
    let found = error_diagnostics(&set(client(), "/zzz", json!(1)));
    let first = &found[0];
    assert_eq!(first.code.as_str(), "manifest.unknown-key");
    let row = rows
        .iter()
        .find(|row| row.code == first.code.as_str())
        .expect("code is in the catalogue");
    assert_eq!(row.exit.parse::<u8>().ok(), Some(1), "unknown-key exits 1");
}

/// REQ: PKG-575
#[test]
fn pkg_575_codes_printed_for_a_batch_of_defects_are_all_in_the_catalogue() {
    let known: Vec<String> = catalogue().into_iter().map(|row| row.code).collect();
    let defects = [
        set(client(), "/zzz", json!(1)),
        set(client(), "/id", json!("ilium.x")),
        set(client(), "/client/entry", json!("x.wasm")),
        client_with_grants(&["fs.pick"]),
        set(client(), "/limits/zzz", json!(1)),
    ];
    for defect in defects {
        for code in errors(&defect) {
            assert!(
                known.contains(&code),
                "{code} printed but not in the catalogue"
            );
        }
    }
}

/// REQ: PKG-575
#[test]
fn pkg_575_catalogue_codes_are_unique_and_namespaced() {
    let rows = catalogue();
    let mut seen = std::collections::BTreeSet::new();
    for row in &rows {
        assert!(row.code.contains('.'), "{}", row.code);
        assert!(seen.insert(row.code.clone()), "duplicate {}", row.code);
    }
}

/// REQ: PKG-576
#[test]
fn pkg_576_doc_every_manifest_code_has_a_stage_in_the_catalogue() {
    for row in catalogue()
        .into_iter()
        .filter(|row| row.code.starts_with("manifest."))
    {
        assert!(
            stage_number(&row.stage).is_some()
                || row.stage.chars().all(|c| c.is_ascii_alphabetic()),
            "{} stage {}",
            row.code,
            row.stage
        );
    }
}

/// REQ: PKG-576
#[test]
fn pkg_576_stage_three_codes_have_the_stage_shown_in_the_catalogue() {
    let rows = catalogue();
    for code in [
        "manifest.schema",
        "manifest.unknown-key",
        "manifest.key-missing",
    ] {
        let row = rows.iter().find(|row| row.code == code).expect(code);
        assert_eq!(row.stage, "3", "{code}");
    }
    for code in ["manifest.json-invalid", "manifest.duplicate-key"] {
        let row = rows.iter().find(|row| row.code == code).expect(code);
        assert_eq!(row.stage, "2", "{code}");
    }
}

/// REQ: PKG-577
#[test]
fn pkg_577_retired_codes_are_never_printed() {
    let retired: Vec<String> = retired_codes().into_iter().map(|(old, _)| old).collect();
    assert!(!retired.is_empty());
    let defects = [
        set(client(), "/zzz", json!(1)),
        unset(client(), "/name"),
        set(client(), "/id", json!("Bad")),
        set(client(), "/api", json!("ilium:plugin@9.9.9")),
        client_with_grants(&["fs.pick"]),
        set(
            client(),
            "/requires",
            json!({ "plugins": [{ "id": "example.demo" }] }),
        ),
    ];
    for defect in defects {
        for code in errors(&defect) {
            assert!(!retired.contains(&code), "retired code {code} printed");
        }
    }
}

/// REQ: PKG-577
#[test]
fn pkg_577_a_retired_code_is_not_a_live_catalogue_entry() {
    let live: Vec<String> = catalogue().into_iter().map(|row| row.code).collect();
    for (old, _) in retired_codes() {
        assert!(!live.contains(&old), "{old} is both retired and live");
    }
}

/// REQ: PKG-573
#[test]
fn pkg_573_the_error_code_is_one_the_catalogue_lists() {
    let found = error_diagnostics(&set(client(), "/id", json!("Bad")));
    let first = &found[0];
    assert!(
        catalogue().iter().any(|row| row.code == first.code.as_str()),
        "tools match the stable code, so every emitted code must be in the catalogue: {}",
        first.code.as_str()
    );
}

/// REQ: PKG-574
#[test]
fn pkg_574_a_clean_manifest_prints_no_error_whose_exit_code_could_win() {
    assert!(
        error_diagnostics(&client()).is_empty(),
        "a clean manifest has no error record, so no exit code is taken from one"
    );
}
