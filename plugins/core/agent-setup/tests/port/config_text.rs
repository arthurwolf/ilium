//! Twins of the pure text-edit tests of ilium-client/src/agent_config_writer.rs (F-037): JSON and TOML
//! patching of one top-level key, byte-exact. The plugin exposes the edit as
//! `ilium_plugin_core_agent_setup::config_writer` (API_GAPS_port.md).

use ilium_plugin_core_agent_setup::config_writer::{
    edit_config_text, edit_json_text, parse_setting, parse_toml_setting, ConfigTarget, KeyChange,
};

const CLAUDE: ConfigTarget = ConfigTarget::ClaudeAutoCompactWindow;
const CODEX: ConfigTarget = ConfigTarget::CodexAutoCompactTokenLimit;

pub const CODEX_SAMPLE: &str = "# Codex config\nmodel = \"gpt-6\"   # keep me\n\n# Trust prompts\n[projects.\"/home/a\"]\ntrust_level = \"trusted\"\nmodel_auto_compact_token_limit = 7\n\n[projects.\"/home/b\"]\ntrust_level = \"untrusted\"\n";

fn json_set(text: &str, value: u64) -> String {
    edit_config_text(CLAUDE, text, KeyChange::Set(value)).expect("json set")
}

fn toml_set(text: &str, value: u64) -> String {
    edit_config_text(CODEX, text, KeyChange::Set(value)).expect("toml set")
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::json_replaces_the_value_and_keeps_every_other_byte
#[test]
fn port_cw_01_json_replaces_the_value_and_keeps_every_other_byte() {
    let original = "{\n  \"theme\": \"dark\",\n  \"autoCompactWindow\":   567000 ,\n  \"name\": \"Zoë 😀\",\n  \"list\": [1,  2]\n}\n";
    assert_eq!(
        json_set(original, 250_000),
        "{\n  \"theme\": \"dark\",\n  \"autoCompactWindow\":   250000 ,\n  \"name\": \"Zoë 😀\",\n  \"list\": [1,  2]\n}\n"
    );
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::json_inserts_first_key_with_tab_indentation_and_crlf
#[test]
fn port_cw_02_json_inserts_first_key_with_tab_indentation_and_crlf() {
    let edited = json_set("{\r\n\t\"alpha\": 1,\r\n\t\"beta\": [1, 2]\r\n}", 300_000);
    assert_eq!(
        edited,
        "{\r\n\t\"autoCompactWindow\": 300000,\r\n\t\"alpha\": 1,\r\n\t\"beta\": [1, 2]\r\n}"
    );
    assert!(
        !edited.ends_with('\n'),
        "trailing newline presence is preserved"
    );
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::json_inserts_first_key_with_two_space_indentation_and_trailing_newline
#[test]
fn port_cw_03_json_inserts_first_key_with_two_space_indentation_and_trailing_newline() {
    assert_eq!(
        json_set(
            "{\n  \"alpha\": {\"x\": 1},\n  \"beta\": true\n}\n",
            250_000
        ),
        "{\n  \"autoCompactWindow\": 250000,\n  \"alpha\": {\"x\": 1},\n  \"beta\": true\n}\n"
    );
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::json_inserts_into_compact_and_spaced_objects
#[test]
fn port_cw_04_json_inserts_into_compact_and_spaced_objects() {
    assert_eq!(
        json_set("{\"a\":1}", 200_000),
        "{\"autoCompactWindow\":200000,\"a\":1}"
    );
    assert_eq!(
        json_set("{ \"a\" : 1 }", 200_000),
        "{ \"autoCompactWindow\" : 200000, \"a\" : 1 }"
    );
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::json_inserts_into_empty_objects
#[test]
fn port_cw_05_json_inserts_into_empty_objects() {
    assert_eq!(json_set("{}", 200_000), "{\"autoCompactWindow\": 200000}");
    assert_eq!(
        json_set("{\n}\n", 200_000),
        "{\n  \"autoCompactWindow\": 200000\n}\n"
    );
    assert_eq!(
        json_set("{\r\n}", 200_000),
        "{\r\n  \"autoCompactWindow\": 200000\r\n}"
    );
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::json_nested_same_named_key_is_not_the_top_level_key
#[test]
fn port_cw_06_json_nested_same_named_key_is_not_the_top_level_key() {
    let original = "{\n  \"nested\": {\"autoCompactWindow\": 5},\n  \"other\": 1\n}\n";
    assert_eq!(parse_setting(CLAUDE, original), Ok(None));
    let edited = json_set(original, 250_000);
    assert!(edited.contains("\"nested\": {\"autoCompactWindow\": 5}"));
    assert_eq!(parse_setting(CLAUDE, &edited), Ok(Some(250_000)));
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::json_values_with_braces_and_escapes_do_not_confuse_the_scanner
#[test]
fn port_cw_07_json_values_with_braces_and_escapes_do_not_confuse_the_scanner() {
    let original = "{\n  \"cmd\": \"echo \\\"}\\\" , {\",\n  \"arr\": [\"]\", {\"k\": \"}\"}],\n  \"autoCompactWindow\": 400000\n}\n";
    assert_eq!(
        json_set(original, 250_000),
        original.replace("400000", "250000")
    );
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::json_insert_then_remove_restores_the_original_bytes
#[test]
fn port_cw_08_json_insert_then_remove_restores_the_original_bytes() {
    let shapes = [
        "{\n  \"a\": 1,\n  \"b\": {\"c\": [1, 2, 3]}\n}\n",
        "{\r\n\t\"a\": 1\r\n}",
        "{\"a\":1,\"b\":2}",
        "{ \"a\": 1, \"b\": 2 }\n",
        "{\n  \"only\": \"é\"\n}",
    ];
    for original in shapes {
        let inserted = json_set(original, 250_000);
        let removed = edit_config_text(CLAUDE, &inserted, KeyChange::Remove).expect("remove");
        assert_eq!(removed, original, "shape {original:?}");
    }
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::json_removal_handles_first_middle_last_and_only_members
#[test]
fn port_cw_09_json_removal_handles_first_middle_last_and_only_members() {
    let key = "autoCompactWindow";
    assert_eq!(
        edit_json_text(
            "{\n  \"autoCompactWindow\": 1,\n  \"b\": 2\n}\n",
            key,
            KeyChange::Remove
        )
        .unwrap(),
        "{\n  \"b\": 2\n}\n"
    );
    assert_eq!(
        edit_json_text(
            "{\n  \"a\": 1,\n  \"autoCompactWindow\": 1,\n  \"b\": 2\n}\n",
            key,
            KeyChange::Remove
        )
        .unwrap(),
        "{\n  \"a\": 1,\n  \"b\": 2\n}\n"
    );
    assert_eq!(
        edit_json_text(
            "{\n  \"a\": 1,\n  \"autoCompactWindow\": 1\n}\n",
            key,
            KeyChange::Remove
        )
        .unwrap(),
        "{\n  \"a\": 1\n}\n"
    );
    assert_eq!(
        edit_json_text("{\"autoCompactWindow\": 1}", key, KeyChange::Remove).unwrap(),
        "{}"
    );
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::json_invalid_duplicate_and_mistyped_input_is_refused
#[test]
fn port_cw_10_json_invalid_duplicate_and_mistyped_input_is_refused_as_invalid_argument_text() {
    let cases = [
        (
            "{\"autoCompactWindow\": 1, \"autoCompactWindow\": 2}",
            "more than once",
        ),
        ("{\"a\": 1,}", "invalid JSON"),
        ("{\"a\": 1", "invalid JSON"),
        ("// comment\n{\"a\": 1}", "invalid JSON"),
        ("[1, 2]", "not a JSON object"),
        ("", "invalid JSON"),
        (
            "{\"autoCompactWindow\": \"250000\"}",
            "not a non-negative integer",
        ),
        (
            "{\"autoCompactWindow\": 250000.5}",
            "not a non-negative integer",
        ),
        (
            "{\"autoCompactWindow\": null}",
            "not a non-negative integer",
        ),
        ("{\"autoCompactWindow\": -4}", "not a non-negative integer"),
    ];
    for (input, expected) in cases {
        let error = edit_config_text(CLAUDE, input, KeyChange::Set(250_000)).unwrap_err();
        assert!(error.contains(expected), "{input:?} -> {error}");
    }
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::toml_inserts_before_the_first_table_and_leaves_table_keys_alone
#[test]
fn port_cw_11_toml_inserts_before_the_first_table_and_leaves_table_keys_alone() {
    assert_eq!(
        toml_set(CODEX_SAMPLE, 250_000),
        "# Codex config\nmodel = \"gpt-6\"   # keep me\nmodel_auto_compact_token_limit = 250000\n\n# Trust prompts\n[projects.\"/home/a\"]\ntrust_level = \"trusted\"\nmodel_auto_compact_token_limit = 7\n\n[projects.\"/home/b\"]\ntrust_level = \"untrusted\"\n"
    );
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::toml_replaces_an_existing_top_level_value_keeping_its_comment
#[test]
fn port_cw_12_toml_replaces_an_existing_top_level_value_keeping_its_comment() {
    let original =
        "a = 1\nmodel_auto_compact_token_limit = 100_000   # tuned by hand\n\n[t]\nx = 1\n";
    assert_eq!(
        toml_set(original, 250_000),
        "a = 1\nmodel_auto_compact_token_limit = 250000   # tuned by hand\n\n[t]\nx = 1\n"
    );
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::toml_insert_then_remove_restores_the_original_bytes
#[test]
fn port_cw_13_toml_insert_then_remove_restores_the_original_bytes() {
    let shapes = [
        CODEX_SAMPLE,
        "# only a comment\n\n[t]\nx = 1\n",
        "[t]\nx = 1\n",
        "a = 1\nb = 2",
        "a = 1\r\nb = 2\r\n\r\n[t]\r\nx = 1\r\n",
        "a = 1",
        "",
        "# header only\n",
        "a.b = 1\nc = [\n  1,\n  2,\n]\n[t]\n",
        "title = \"é 😀\"\n[t]\nx = 1\n",
    ];
    for original in shapes {
        let inserted = toml_set(original, 250_000);
        assert_eq!(
            parse_setting(CODEX, &inserted),
            Ok(Some(250_000)),
            "{original:?}"
        );
        let removed = edit_config_text(CODEX, &inserted, KeyChange::Remove).expect("remove");
        assert_eq!(removed, original, "shape {original:?}");
    }
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::toml_insertion_follows_the_file_line_ending
#[test]
fn port_cw_14_toml_insertion_follows_the_file_line_ending() {
    assert_eq!(
        toml_set("a = 1\r\n\r\n[t]\r\nx = 1\r\n", 250_000),
        "a = 1\r\nmodel_auto_compact_token_limit = 250000\r\n\r\n[t]\r\nx = 1\r\n"
    );
    assert_eq!(
        toml_set("a = 1", 250_000),
        "a = 1\nmodel_auto_compact_token_limit = 250000"
    );
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::toml_parse_errors_never_echo_source_lines
#[test]
fn port_cw_15_toml_parse_errors_never_echo_source_lines() {
    let text = "a = 1\nSECRET_KEY = \"sk-SECRET123\" oops\n";
    let error = parse_toml_setting(text, "model_auto_compact_token_limit").unwrap_err();
    assert!(error.starts_with("invalid TOML"), "{error}");
    assert!(!error.contains("SECRET"), "{error}");
    assert!(error.contains("line 2"), "{error}");
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::toml_invalid_duplicate_and_mistyped_input_is_refused
#[test]
fn port_cw_16_toml_invalid_duplicate_and_mistyped_input_is_refused() {
    let cases = [
        (
            "model_auto_compact_token_limit = 1\nmodel_auto_compact_token_limit = 2\n",
            "invalid TOML",
        ),
        ("a = \n", "invalid TOML"),
        ("[t\nx = 1\n", "invalid TOML"),
        (
            "model_auto_compact_token_limit = \"250000\"\n",
            "not an integer",
        ),
        ("model_auto_compact_token_limit = 2.5\n", "not an integer"),
        (
            "model_auto_compact_token_limit = -3\n",
            "not a non-negative integer",
        ),
    ];
    for (input, expected) in cases {
        let error = edit_config_text(CODEX, input, KeyChange::Set(250_000)).unwrap_err();
        assert!(error.contains(expected), "{input:?} -> {error}");
    }
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::toml_removal_refuses_a_line_that_gained_a_comment
#[test]
fn port_cw_17_toml_removal_refuses_a_line_that_gained_a_comment() {
    let error = edit_config_text(
        CODEX,
        "a = 1\nmodel_auto_compact_token_limit = 5 # mine\n",
        KeyChange::Remove,
    )
    .unwrap_err();
    assert!(error.contains("comment"), "{error}");
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::toml_large_file_with_many_tables_changes_only_one_line
#[test]
fn port_cw_18_toml_large_file_with_many_tables_changes_only_one_line() {
    let mut original = String::from("# header\nmodel = \"gpt-6\"\n\n");
    for index in 0..1_488 {
        original.push_str(&format!(
            "# project {index}\n[projects.\"/home/arthur/dev/p{index}\"]\ntrust_level = \"trusted\"\nmodel_auto_compact_token_limit = {index}\n\n"
        ));
    }
    let edited = toml_set(&original, 250_000);
    assert_eq!(
        edited.replacen("model_auto_compact_token_limit = 250000\n", "", 1),
        original
    );
    assert!(edited.starts_with(
        "# header\nmodel = \"gpt-6\"\nmodel_auto_compact_token_limit = 250000\n\n# project 0\n"
    ));
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::json_replaces_the_value_and_keeps_every_other_byte
#[test]
fn port_cw_01b_json_replace_keeps_crlf_and_spacing_around_the_value() {
    let original = "{\r\n  \"autoCompactWindow\":   567000 ,\r\n  \"b\": 1\r\n}";
    assert_eq!(
        json_set(original, 250_000),
        "{\r\n  \"autoCompactWindow\":   250000 ,\r\n  \"b\": 1\r\n}"
    );
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::json_inserts_first_key_with_tab_indentation_and_crlf
#[test]
fn port_cw_02b_json_second_set_replaces_instead_of_duplicating_the_key() {
    let first = json_set("{\r\n\t\"alpha\": 1\r\n}", 300_000);
    let second = json_set(&first, 260_000);
    assert_eq!(second.matches("autoCompactWindow").count(), 1, "{second:?}");
    assert_eq!(parse_setting(CLAUDE, &second), Ok(Some(260_000)));
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::json_inserts_first_key_with_two_space_indentation_and_trailing_newline
#[test]
fn port_cw_03b_json_insert_without_trailing_newline_stays_without_one() {
    assert_eq!(
        json_set("{\n  \"alpha\": 1\n}", 250_000),
        "{\n  \"autoCompactWindow\": 250000,\n  \"alpha\": 1\n}"
    );
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::json_inserts_into_compact_and_spaced_objects
#[test]
fn port_cw_04b_json_insert_keeps_array_valued_members_intact() {
    assert_eq!(
        json_set("{\"a\":[1,2]}", 250_000),
        "{\"autoCompactWindow\":250000,\"a\":[1,2]}"
    );
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::json_inserts_into_empty_objects
#[test]
fn port_cw_05b_json_insert_into_empty_object_variants_parses_back() {
    for original in ["{}", "{\n}\n", "{\r\n}", "{ }"] {
        let edited = json_set(original, 250_000);
        assert_eq!(
            parse_setting(CLAUDE, &edited),
            Ok(Some(250_000)),
            "{original:?} -> {edited:?}"
        );
    }
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::json_invalid_duplicate_and_mistyped_input_is_refused
#[test]
fn port_cw_10b_json_read_and_remove_refuse_invalid_input_too() {
    let duplicate_error =
        parse_setting(CLAUDE, "{\"autoCompactWindow\": 1, \"autoCompactWindow\": 2}")
            .unwrap_err();
    assert!(duplicate_error.contains("more than once"), "{duplicate_error}");
    assert!(
        edit_config_text(CLAUDE, "{\"a\": 1,}", KeyChange::Remove).is_err(),
        "removal must refuse invalid JSON"
    );
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::json_nested_same_named_key_is_not_the_top_level_key
#[test]
fn port_cw_06b_json_remove_takes_only_the_top_level_key() {
    let original = "{\n  \"nested\": {\"autoCompactWindow\": 5},\n  \"autoCompactWindow\": 9\n}\n";
    let removed = edit_config_text(CLAUDE, original, KeyChange::Remove).expect("remove");
    assert!(removed.contains("\"nested\": {\"autoCompactWindow\": 5}"), "{removed}");
    assert_eq!(parse_setting(CLAUDE, &removed), Ok(None));
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::json_values_with_braces_and_escapes_do_not_confuse_the_scanner
#[test]
fn port_cw_07b_json_string_value_imitating_the_key_is_not_edited() {
    let original = "{\n  \"note\": \"\\\"autoCompactWindow\\\": 1\",\n  \"autoCompactWindow\": 400000\n}\n";
    let edited = json_set(original, 250_000);
    assert_eq!(parse_setting(CLAUDE, &edited), Ok(Some(250_000)));
    assert!(edited.contains("\"note\": \"\\\"autoCompactWindow\\\": 1\""), "{edited}");
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::json_insert_then_remove_restores_the_original_bytes
#[test]
fn port_cw_08b_json_restore_survives_a_value_change_in_between() {
    let original = "{\r\n\t\"a\": 1\r\n}";
    let inserted = json_set(original, 250_000);
    let changed = json_set(&inserted, 300_000);
    let removed = edit_config_text(CLAUDE, &changed, KeyChange::Remove).expect("remove");
    assert_eq!(removed, original);
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::json_removal_handles_first_middle_last_and_only_members
#[test]
fn port_cw_09b_json_removal_of_first_compact_member_keeps_the_rest_compact() {
    assert_eq!(
        edit_json_text(
            "{\"autoCompactWindow\":1,\"b\":2}",
            "autoCompactWindow",
            KeyChange::Remove
        )
        .unwrap(),
        "{\"b\":2}"
    );
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::toml_inserts_before_the_first_table_and_leaves_table_keys_alone
#[test]
fn port_cw_11b_toml_insert_without_tables_appends_at_the_end() {
    assert_eq!(
        toml_set("a = 1\n", 250_000),
        "a = 1\nmodel_auto_compact_token_limit = 250000\n"
    );
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::toml_replaces_an_existing_top_level_value_keeping_its_comment
#[test]
fn port_cw_12b_toml_table_scoped_key_is_not_read_as_the_top_level_setting() {
    let text = "[t]\nmodel_auto_compact_token_limit = 9\n";
    assert_eq!(parse_setting(CODEX, text), Ok(None));
    assert_eq!(
        toml_set(text, 250_000),
        "model_auto_compact_token_limit = 250000\n[t]\nmodel_auto_compact_token_limit = 9\n"
    );
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::toml_insert_then_remove_restores_the_original_bytes
#[test]
fn port_cw_13b_toml_remove_leaves_a_comment_only_line_in_place() {
    let text = "model_auto_compact_token_limit = 5\n# keep this\n";
    assert_eq!(
        edit_config_text(CODEX, text, KeyChange::Remove).expect("remove"),
        "# keep this\n"
    );
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::toml_insertion_follows_the_file_line_ending
#[test]
fn port_cw_14b_toml_crlf_file_without_trailing_newline_gets_no_bare_lf() {
    let edited = toml_set("a = 1\r\n[t]\r\nx = 1", 250_000);
    assert_eq!(parse_setting(CODEX, &edited), Ok(Some(250_000)));
    assert!(!edited.replace("\r\n", "").contains('\n'), "{edited:?}");
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::toml_parse_errors_never_echo_source_lines
#[test]
fn port_cw_15b_toml_set_parse_error_never_echoes_the_value_line() {
    let error = edit_config_text(
        CODEX,
        "a = \"SECRET9\"\nbad line here\n",
        KeyChange::Set(250_000),
    )
    .unwrap_err();
    assert!(error.starts_with("invalid TOML"), "{error}");
    assert!(!error.contains("SECRET9"), "{error}");
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::toml_invalid_duplicate_and_mistyped_input_is_refused
#[test]
fn port_cw_16b_toml_removal_refuses_unparseable_input() {
    let error = edit_config_text(CODEX, "a = \n", KeyChange::Remove).unwrap_err();
    assert!(error.contains("invalid TOML"), "{error}");
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::toml_removal_refuses_a_line_that_gained_a_comment
#[test]
fn port_cw_17b_toml_comment_on_another_line_does_not_block_removal() {
    assert_eq!(
        edit_config_text(
            CODEX,
            "# note\na = 1\nmodel_auto_compact_token_limit = 5\n",
            KeyChange::Remove
        )
        .expect("remove"),
        "# note\na = 1\n"
    );
}

/// REQ: PORT:ilium-client/src/agent_config_writer.rs::toml_large_file_with_many_tables_changes_only_one_line
#[test]
fn port_cw_18b_toml_large_file_without_top_level_key_reads_none_then_inserts_after_model() {
    let mut original = String::from("# header\nmodel = \"gpt-6\"\n\n");
    for index in 0..1_488 {
        original.push_str(&format!(
            "[projects.\"/home/arthur/dev/p{index}\"]\nmodel_auto_compact_token_limit = {index}\n\n"
        ));
    }
    assert_eq!(parse_setting(CODEX, &original), Ok(None));
    let edited = toml_set(&original, 250_000);
    assert!(
        edited.starts_with("# header\nmodel = \"gpt-6\"\nmodel_auto_compact_token_limit = 250000\n"),
        "{:?}",
        &edited[..120.min(edited.len())]
    );
    assert_eq!(parse_setting(CODEX, &edited), Ok(Some(250_000)));
}
