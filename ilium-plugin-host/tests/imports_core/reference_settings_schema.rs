//! reference-settings-schema.md: limits and install error codes of `contributes.settings`,
//! swept at their boundaries (n-1, n, n+1) and by property tests.
//!
//! These rows are checked by `ilium_plugin_api::manifest::validate_json`, the same function
//! `ilium plugin check` and the installer run (SET-001..SET-064). They fail red while the
//! skeleton body is `unimplemented!()`.

use ilium_plugin_api::Tier;
use proptest::prelude::*;
use serde_json::{json, Value};

use crate::support::*;

const LIMIT: &str = "manifest.settings-limit";

fn has(manifest: &Value, tier: Tier, code: &str) -> bool {
    install_codes(manifest, tier)
        .iter()
        .any(|found| found == code)
}

fn group(title: &str, children: Vec<Value>) -> Value {
    json!({ "type": "group", "title": title, "rows": children })
}

fn tab(id: &str, title: &str) -> Value {
    json!({
        "kind": "tab", "id": id, "title": title,
        "sections": [{ "id": "s", "title": "S", "rows": [toggle(&format!("{id}_on"), true)] }]
    })
}

fn section(id: &str, rows: Vec<Value>) -> Value {
    json!({ "id": id, "title": "Section", "rows": rows })
}

fn rows_with_prefix(count: usize, prefix: &str) -> Vec<Value> {
    many_toggles(count, prefix)
}

// ---- SET-001: tabs, sections, rows, group children -------------------------------------------

/// REQ: SET-001
#[test]
fn set_001_two_tabs_accepted() {
    let manifest = manifest_with_settings_entries(vec![tab("alpha", "Alpha"), tab("beta", "Beta")]);
    assert!(!has(&manifest, Tier::T2, LIMIT));
}

/// REQ: SET-001
#[test]
fn set_001_three_tabs_refused_manifest_settings_limit() {
    let manifest = manifest_with_settings_entries(vec![
        tab("alpha", "Alpha"),
        tab("beta", "Beta"),
        tab("gamma", "Gamma"),
    ]);
    assert!(has(&manifest, Tier::T2, LIMIT));
}

/// REQ: SET-001
#[test]
fn set_001_one_tab_accepted() {
    let manifest = manifest_with_settings_entries(vec![tab("alpha", "Alpha")]);
    assert!(!has(&manifest, Tier::T2, LIMIT));
}

/// REQ: SET-001
#[test]
fn set_001_eight_sections_in_a_tab_accepted() {
    let sections = (0..8)
        .map(|index| {
            section(
                &format!("s{index}"),
                vec![toggle(&format!("k{index}"), true)],
            )
        })
        .collect();
    assert!(!has(&manifest_with_tab(sections), Tier::T2, LIMIT));
}

/// REQ: SET-001
#[test]
fn set_001_nine_sections_in_a_tab_refused_manifest_settings_limit() {
    let sections = (0..9)
        .map(|index| {
            section(
                &format!("s{index}"),
                vec![toggle(&format!("k{index}"), true)],
            )
        })
        .collect();
    assert!(has(&manifest_with_tab(sections), Tier::T2, LIMIT));
}

/// REQ: SET-001
#[test]
fn set_001_seven_sections_in_a_tab_accepted() {
    let sections = (0..7)
        .map(|index| {
            section(
                &format!("s{index}"),
                vec![toggle(&format!("k{index}"), true)],
            )
        })
        .collect();
    let manifest = manifest_with_tab(sections);
    assert!(!has(&manifest, Tier::T2, LIMIT));
    assert!(install_codes(&manifest, Tier::T2).is_empty());
}

/// REQ: SET-001
#[test]
fn set_001_twenty_three_rows_accepted() {
    assert!(!has(
        &manifest_with_rows(rows_with_prefix(23, "r")),
        Tier::T2,
        LIMIT
    ));
}

/// REQ: SET-001
#[test]
fn set_001_twenty_four_rows_accepted() {
    assert!(!has(
        &manifest_with_rows(rows_with_prefix(24, "r")),
        Tier::T2,
        LIMIT
    ));
}

/// REQ: SET-001
#[test]
fn set_001_twenty_five_rows_refused_manifest_settings_limit() {
    assert!(has(
        &manifest_with_rows(rows_with_prefix(25, "r")),
        Tier::T2,
        LIMIT
    ));
}

/// REQ: SET-001
#[test]
fn set_001_group_counts_as_one_row_plus_children_at_the_limit() {
    // 1 group row + 12 children = 13, plus 11 toggles = 24.
    let mut rows = vec![group("G", rows_with_prefix(12, "child"))];
    rows.extend(rows_with_prefix(11, "plain"));
    assert!(!has(&manifest_with_rows(rows), Tier::T2, LIMIT));
}

/// REQ: SET-001
#[test]
fn set_001_group_counts_as_one_row_plus_children_over_the_limit_manifest_settings_limit() {
    // 1 group row + 12 children = 13, plus 12 toggles = 25.
    let mut rows = vec![group("G", rows_with_prefix(12, "child"))];
    rows.extend(rows_with_prefix(12, "plain"));
    assert!(has(&manifest_with_rows(rows), Tier::T2, LIMIT));
}

/// REQ: SET-001
#[test]
fn set_001_twelve_group_children_accepted() {
    let rows = vec![group("G", rows_with_prefix(12, "child"))];
    assert!(!has(&manifest_with_rows(rows), Tier::T2, LIMIT));
}

/// REQ: SET-001
#[test]
fn set_001_thirteen_group_children_refused_manifest_settings_limit() {
    let rows = vec![group("G", rows_with_prefix(13, "child"))];
    assert!(has(&manifest_with_rows(rows), Tier::T2, LIMIT));
}

// ---- SET-002: value keys ---------------------------------------------------------------------

fn sections_of_keys(total: usize) -> Vec<Value> {
    // 8 sections of at most 24 rows hold up to 192 keys; spread them evenly.
    let per_section = total.div_ceil(8).max(1);
    let mut remaining = total;
    let mut sections = Vec::new();
    let mut index = 0;
    while remaining > 0 {
        let take = remaining.min(per_section);
        let rows: Vec<Value> = (0..take)
            .map(|offset| toggle(&format!("k{}", index * 100 + offset), false))
            .collect();
        sections.push(section(&format!("s{index}"), rows));
        remaining -= take;
        index += 1;
    }
    sections
}

/// REQ: SET-002
#[test]
fn set_002_128_value_keys_accepted() {
    assert!(!has(
        &manifest_with_tab(sections_of_keys(128)),
        Tier::T2,
        LIMIT
    ));
}

/// REQ: SET-002
#[test]
fn set_002_129_value_keys_refused_manifest_settings_limit() {
    assert!(has(
        &manifest_with_tab(sections_of_keys(129)),
        Tier::T2,
        LIMIT
    ));
}

/// REQ: SET-002
#[test]
fn set_002_127_value_keys_accepted() {
    assert!(!has(
        &manifest_with_tab(sections_of_keys(127)),
        Tier::T2,
        LIMIT
    ));
}

/// REQ: SET-002
#[test]
fn set_002_duplicate_key_in_one_section_refused_manifest_settings_key_clash() {
    let manifest = manifest_with_rows(vec![toggle("a", true), toggle("a", false)]);
    assert!(has(&manifest, Tier::T2, "manifest.settings-key-clash"));
}

/// REQ: SET-002
#[test]
fn set_002_duplicate_key_across_sections_refused_manifest_settings_key_clash() {
    let manifest = manifest_with_tab(vec![
        section("one", vec![toggle("a", true)]),
        section("two", vec![toggle("a", true)]),
    ]);
    assert!(has(&manifest, Tier::T2, "manifest.settings-key-clash"));
}

/// REQ: SET-002
#[test]
fn set_002_distinct_keys_accepted_without_clash() {
    let manifest = manifest_with_rows(vec![toggle("a", true), toggle("b", false)]);
    assert!(!has(&manifest, Tier::T2, "manifest.settings-key-clash"));
}

// ---- SET-003: title and text lengths ---------------------------------------------------------

fn row_with_title(length: usize) -> Value {
    json!({ "type": "toggle", "key": "k", "title": ascii(length), "default": true })
}

/// REQ: SET-003
#[test]
fn set_003_row_title_48_accepted() {
    assert!(!has(
        &manifest_with_rows(vec![row_with_title(48)]),
        Tier::T2,
        LIMIT
    ));
}

/// REQ: SET-003
#[test]
fn set_003_row_title_49_refused_manifest_settings_limit() {
    assert!(has(
        &manifest_with_rows(vec![row_with_title(49)]),
        Tier::T2,
        LIMIT
    ));
}

/// REQ: SET-003
#[test]
fn set_003_row_title_47_accepted() {
    assert!(!has(
        &manifest_with_rows(vec![row_with_title(47)]),
        Tier::T2,
        LIMIT
    ));
}

/// REQ: SET-003
#[test]
fn set_003_section_title_40_accepted() {
    let manifest = manifest_with_tab(vec![json!({
        "id": "s", "title": ascii(40), "rows": [toggle("k", true)]
    })]);
    assert!(!has(&manifest, Tier::T2, LIMIT));
}

/// REQ: SET-003
#[test]
fn set_003_section_title_41_refused_manifest_settings_limit() {
    let manifest = manifest_with_tab(vec![json!({
        "id": "s", "title": ascii(41), "rows": [toggle("k", true)]
    })]);
    assert!(has(&manifest, Tier::T2, LIMIT));
}

/// REQ: SET-003
#[test]
fn set_003_tab_title_32_accepted() {
    let manifest = manifest_with_settings_entries(vec![tab("alpha", &ascii(32))]);
    assert!(!has(&manifest, Tier::T2, LIMIT));
}

/// REQ: SET-003
#[test]
fn set_003_tab_title_33_refused_manifest_settings_limit() {
    let manifest = manifest_with_settings_entries(vec![tab("alpha", &ascii(33))]);
    assert!(has(&manifest, Tier::T2, LIMIT));
}

/// REQ: SET-003
#[test]
fn set_003_description_200_accepted() {
    let row = json!({ "type": "toggle", "key": "k", "title": "T", "description": ascii(200), "default": true });
    assert!(!has(&manifest_with_rows(vec![row]), Tier::T2, LIMIT));
}

/// REQ: SET-003
#[test]
fn set_003_description_201_refused_manifest_settings_limit() {
    let row = json!({ "type": "toggle", "key": "k", "title": "T", "description": ascii(201), "default": true });
    assert!(has(&manifest_with_rows(vec![row]), Tier::T2, LIMIT));
}

/// REQ: SET-003
#[test]
fn set_003_info_text_120_accepted() {
    let row = json!({ "type": "info", "id": "i", "title": "I", "text": ascii(120) });
    assert!(!has(&manifest_with_rows(vec![row]), Tier::T2, LIMIT));
}

/// REQ: SET-003
#[test]
fn set_003_info_text_121_refused_manifest_settings_limit() {
    let row = json!({ "type": "info", "id": "i", "title": "I", "text": ascii(121) });
    assert!(has(&manifest_with_rows(vec![row]), Tier::T2, LIMIT));
}

/// REQ: SET-003
#[test]
fn set_003_button_label_20_accepted() {
    let row =
        json!({ "type": "button", "id": "b", "title": "B", "label": ascii(20), "command": "go" });
    let mut manifest = manifest_with_rows(vec![row]);
    manifest["contributes"]["commands"] = json!([{ "id": "go", "title": "Go" }]);
    assert!(!has(&manifest, Tier::T2, LIMIT));
}

/// REQ: SET-003
#[test]
fn set_003_button_label_21_refused_manifest_settings_limit() {
    let row =
        json!({ "type": "button", "id": "b", "title": "B", "label": ascii(21), "command": "go" });
    let mut manifest = manifest_with_rows(vec![row]);
    manifest["contributes"]["commands"] = json!([{ "id": "go", "title": "Go" }]);
    assert!(has(&manifest, Tier::T2, LIMIT));
}

// ---- SET-004: first-party caps ---------------------------------------------------------------

fn tabs(count: usize) -> Vec<Value> {
    (0..count)
        .map(|index| tab(&format!("tab{index}"), &format!("Tab number {index}")))
        .collect()
}

/// REQ: SET-004
#[test]
fn set_004_nine_tabs_as_t2_refused_manifest_settings_limit() {
    assert!(has(
        &manifest_with_settings_entries(tabs(9)),
        Tier::T2,
        LIMIT
    ));
}

/// REQ: SET-004
#[test]
fn set_004_nine_tabs_as_t3_refused_manifest_settings_limit() {
    assert!(has(
        &manifest_with_settings_entries(tabs(9)),
        Tier::T3,
        LIMIT
    ));
}

/// REQ: SET-004
#[test]
fn set_004_limits_tier_first_party_does_not_raise_caps_for_t2_manifest_settings_limit() {
    let mut manifest = manifest_with_settings_entries(tabs(3));
    manifest["limits_tier"] = json!("first-party");
    assert!(has(&manifest, Tier::T2, LIMIT));
}

/// REQ: SET-004 MAY
#[test]
fn set_004_first_party_bundled_plugin_may_exceed_the_default_tab_cap() {
    let mut manifest = manifest_with_settings_entries(tabs(3));
    manifest["limits_tier"] = json!("first-party");
    assert!(!has(&manifest, Tier::T1, LIMIT));
}

// ---- SET-010: defaults and options checked at install ----------------------------------------

fn settings_error_codes(manifest: &Value) -> Vec<String> {
    install_codes(manifest, Tier::T2)
        .into_iter()
        .filter(|code| code.starts_with("manifest.settings-"))
        .collect()
}

/// REQ: SET-010
#[test]
fn set_010_number_default_above_maximum_refused_manifest_settings_default() {
    let manifest = manifest_with_rows(vec![number("n", 0, 10, 11)]);
    assert!(settings_error_codes(&manifest).contains(&"manifest.settings-default".to_owned()));
}

/// REQ: SET-010
#[test]
fn set_010_number_default_at_maximum_accepted() {
    let manifest = manifest_with_rows(vec![number("n", 0, 10, 10)]);
    assert!(settings_error_codes(&manifest).is_empty());
}

/// REQ: SET-010
#[test]
fn set_010_number_default_below_minimum_refused_manifest_settings_default() {
    let manifest = manifest_with_rows(vec![number("n", 0, 10, -1)]);
    assert!(settings_error_codes(&manifest).contains(&"manifest.settings-default".to_owned()));
}

/// REQ: SET-010
#[test]
fn set_010_number_default_at_minimum_accepted() {
    let manifest = manifest_with_rows(vec![number("n", 0, 10, 0)]);
    assert!(settings_error_codes(&manifest).is_empty());
}

/// REQ: SET-010
#[test]
fn set_010_choice_default_not_an_option_refused_manifest_settings_default() {
    let mut row = choice("c", &["a", "b"]);
    row["default"] = json!("zzz");
    assert!(settings_error_codes(&manifest_with_rows(vec![row]))
        .contains(&"manifest.settings-default".to_owned()));
}

/// REQ: SET-010
#[test]
fn set_010_text_default_longer_than_max_length_refused_manifest_settings_default() {
    let manifest = manifest_with_rows(vec![text("t", 5, "abcdef")]);
    assert!(settings_error_codes(&manifest).contains(&"manifest.settings-default".to_owned()));
}

/// REQ: SET-010
#[test]
fn set_010_text_default_exactly_max_length_accepted() {
    let manifest = manifest_with_rows(vec![text("t", 5, "abcde")]);
    assert!(settings_error_codes(&manifest).is_empty());
}

/// REQ: SET-010
#[test]
fn set_010_toggle_default_of_the_wrong_type_refused_manifest_settings_default() {
    let row = json!({ "type": "toggle", "key": "k", "title": "T", "default": "yes" });
    assert!(settings_error_codes(&manifest_with_rows(vec![row]))
        .contains(&"manifest.settings-default".to_owned()));
}

/// REQ: SET-010
#[test]
fn set_010_color_default_not_a_hex_colour_refused_manifest_settings_default() {
    let row = json!({ "type": "color", "key": "c", "title": "C", "default": "red" });
    assert!(settings_error_codes(&manifest_with_rows(vec![row]))
        .contains(&"manifest.settings-default".to_owned()));
}

// ---- SET-060: option and item counts ---------------------------------------------------------

fn choice_with_options(count: usize) -> Value {
    let values: Vec<String> = (0..count).map(|index| format!("o{index}")).collect();
    let refs: Vec<&str> = values.iter().map(String::as_str).collect();
    choice("c", &refs)
}

/// REQ: SET-060
#[test]
fn set_060_choice_with_64_options_accepted() {
    assert!(!has(
        &manifest_with_rows(vec![choice_with_options(64)]),
        Tier::T2,
        LIMIT
    ));
}

/// REQ: SET-060
#[test]
fn set_060_choice_with_65_options_refused_manifest_settings_limit() {
    assert!(has(
        &manifest_with_rows(vec![choice_with_options(65)]),
        Tier::T2,
        LIMIT
    ));
}

/// REQ: SET-060
#[test]
fn set_060_choice_with_63_options_accepted() {
    assert!(!has(
        &manifest_with_rows(vec![choice_with_options(63)]),
        Tier::T2,
        LIMIT
    ));
}

/// REQ: SET-060
#[test]
fn set_060_list_max_items_64_accepted() {
    let row = json!({ "type": "list", "key": "l", "title": "L", "item": { "type": "string" },
        "max_items": 64, "default": [] });
    assert!(!has(&manifest_with_rows(vec![row]), Tier::T2, LIMIT));
}

/// REQ: SET-060
#[test]
fn set_060_list_max_items_65_refused_manifest_settings_limit() {
    let row = json!({ "type": "list", "key": "l", "title": "L", "item": { "type": "string" },
        "max_items": 65, "default": [] });
    assert!(has(&manifest_with_rows(vec![row]), Tier::T2, LIMIT));
}

/// REQ: SET-060
#[test]
fn set_060_map_max_entries_64_accepted() {
    let row = json!({ "type": "map", "key": "m", "title": "M", "value_type": "string",
        "max_entries": 64, "default": {} });
    assert!(!has(&manifest_with_rows(vec![row]), Tier::T2, LIMIT));
}

/// REQ: SET-060
#[test]
fn set_060_map_max_entries_65_refused_manifest_settings_limit() {
    let row = json!({ "type": "map", "key": "m", "title": "M", "value_type": "string",
        "max_entries": 65, "default": {} });
    assert!(has(&manifest_with_rows(vec![row]), Tier::T2, LIMIT));
}

/// REQ: SET-060
#[test]
fn set_060_meter_with_8_segments_accepted() {
    let segments: Vec<Value> = (0..8)
        .map(|index| json!({ "label": format!("s{index}"), "value": 0.1, "tone": "info" }))
        .collect();
    let row = json!({ "type": "meter", "id": "m", "title": "M", "segments": segments });
    assert!(!has(&manifest_with_rows(vec![row]), Tier::T2, LIMIT));
}

/// REQ: SET-060
#[test]
fn set_060_meter_with_9_segments_refused_manifest_settings_limit() {
    let segments: Vec<Value> = (0..9)
        .map(|index| json!({ "label": format!("s{index}"), "value": 0.1, "tone": "info" }))
        .collect();
    let row = json!({ "type": "meter", "id": "m", "title": "M", "segments": segments });
    assert!(has(&manifest_with_rows(vec![row]), Tier::T2, LIMIT));
}

/// REQ: SET-060
#[test]
fn set_060_notice_text_200_accepted() {
    let row = json!({ "type": "notice", "id": "n", "title": "N", "text": ascii(200) });
    assert!(!has(&manifest_with_rows(vec![row]), Tier::T2, LIMIT));
}

/// REQ: SET-060
#[test]
fn set_060_notice_text_201_refused_manifest_settings_limit() {
    let row = json!({ "type": "notice", "id": "n", "title": "N", "text": ascii(201) });
    assert!(has(&manifest_with_rows(vec![row]), Tier::T2, LIMIT));
}

// ---- SET-061..064: error codes ---------------------------------------------------------------

/// REQ: SET-061
#[test]
fn set_061_depends_on_missing_key_refused_manifest_settings_depends() {
    let mut row = toggle("a", true);
    row["depends_on"] = json!({ "key": "nope", "equals": true });
    assert!(has(
        &manifest_with_rows(vec![row]),
        Tier::T2,
        "manifest.settings-depends"
    ));
}

/// REQ: SET-061
#[test]
fn set_061_depends_on_existing_key_accepted() {
    let mut row = toggle("a", true);
    row["depends_on"] = json!({ "key": "b", "equals": true });
    let manifest = manifest_with_rows(vec![row, toggle("b", true)]);
    assert!(!has(&manifest, Tier::T2, "manifest.settings-depends"));
}

/// REQ: SET-061
#[test]
fn set_061_depends_on_cycle_refused_manifest_settings_depends() {
    let mut first = toggle("a", true);
    first["depends_on"] = json!({ "key": "b", "equals": true });
    let mut second = toggle("b", true);
    second["depends_on"] = json!({ "key": "a", "equals": true });
    assert!(has(
        &manifest_with_rows(vec![first, second]),
        Tier::T2,
        "manifest.settings-depends"
    ));
}

/// REQ: SET-061
#[test]
fn set_061_depends_on_itself_refused_manifest_settings_depends() {
    let mut row = toggle("a", true);
    row["depends_on"] = json!({ "key": "a", "equals": true });
    assert!(has(
        &manifest_with_rows(vec![row]),
        Tier::T2,
        "manifest.settings-depends"
    ));
}

/// REQ: SET-062
#[test]
fn set_062_key_with_space_and_capital_refused_manifest_settings_key() {
    let manifest = manifest_with_rows(vec![toggle("A b", true)]);
    assert!(has(&manifest, Tier::T2, "manifest.settings-key"));
}

/// REQ: SET-062
#[test]
fn set_062_key_64_characters_accepted() {
    let key = format!("a{}", ascii(63));
    assert!(!has(
        &manifest_with_rows(vec![toggle(&key, true)]),
        Tier::T2,
        "manifest.settings-key"
    ));
}

/// REQ: SET-062
#[test]
fn set_062_key_65_characters_refused_manifest_settings_key() {
    let key = format!("a{}", ascii(64));
    assert!(has(
        &manifest_with_rows(vec![toggle(&key, true)]),
        Tier::T2,
        "manifest.settings-key"
    ));
}

/// REQ: SET-062
#[test]
fn set_062_key_starting_with_a_digit_refused_manifest_settings_key() {
    assert!(has(
        &manifest_with_rows(vec![toggle("1abc", true)]),
        Tier::T2,
        "manifest.settings-key"
    ));
}

/// REQ: SET-062
#[test]
fn set_062_key_with_a_dash_refused_manifest_settings_key() {
    assert!(has(
        &manifest_with_rows(vec![toggle("a-b", true)]),
        Tier::T2,
        "manifest.settings-key"
    ));
}

/// REQ: SET-062
#[test]
fn set_062_empty_key_refused_manifest_settings_key() {
    assert!(has(
        &manifest_with_rows(vec![toggle("", true)]),
        Tier::T2,
        "manifest.settings-key"
    ));
}

/// REQ: SET-063
#[test]
fn set_063_button_with_unknown_command_refused_manifest_settings_command() {
    let row =
        json!({ "type": "button", "id": "b", "title": "B", "label": "Go", "command": "ghost" });
    assert!(has(
        &manifest_with_rows(vec![row]),
        Tier::T2,
        "manifest.settings-command"
    ));
}

/// REQ: SET-063
#[test]
fn set_063_button_with_a_contributed_command_accepted() {
    let row = json!({ "type": "button", "id": "b", "title": "B", "label": "Go", "command": "go" });
    let mut manifest = manifest_with_rows(vec![row]);
    manifest["contributes"]["commands"] = json!([{ "id": "go", "title": "Go" }]);
    assert!(!has(&manifest, Tier::T2, "manifest.settings-command"));
}

/// REQ: SET-063
#[test]
fn set_063_keybinding_row_with_unknown_command_refused_manifest_settings_command() {
    let row = json!({ "type": "keybinding", "key": "k", "title": "K", "command": "ghost", "default": "ctrl+alt+v" });
    assert!(has(
        &manifest_with_rows(vec![row]),
        Tier::T2,
        "manifest.settings-command"
    ));
}

/// REQ: SET-063
#[test]
fn set_063_glyph_of_three_graphemes_refused_manifest_settings_glyph() {
    let mut entry = tab("alpha", "Alpha");
    entry["glyph"] = json!("abc");
    assert!(has(
        &manifest_with_settings_entries(vec![entry]),
        Tier::T2,
        "manifest.settings-glyph"
    ));
}

/// REQ: SET-063
#[test]
fn set_063_glyph_of_one_grapheme_accepted() {
    let mut entry = tab("alpha", "Alpha");
    entry["glyph"] = json!("\u{29c9}");
    assert!(!has(
        &manifest_with_settings_entries(vec![entry]),
        Tier::T2,
        "manifest.settings-glyph"
    ));
}

/// REQ: SET-063
#[test]
fn set_063_help_id_not_contributed_refused_manifest_settings_help() {
    let mut row = toggle("k", true);
    row["help"] = json!("no-such-topic");
    assert!(has(
        &manifest_with_rows(vec![row]),
        Tier::T2,
        "manifest.settings-help"
    ));
}

/// REQ: SET-063
#[test]
fn set_063_tab_after_a_never_anchor_refused_manifest_settings_tab_anchor() {
    for anchor in ["debug", "api", "about", "setup"] {
        let mut entry = tab("alpha", "Alpha");
        entry["after"] = json!(anchor);
        assert!(
            has(
                &manifest_with_settings_entries(vec![entry]),
                Tier::T2,
                "manifest.settings-tab-anchor"
            ),
            "after {anchor} must be refused"
        );
    }
}

/// REQ: SET-063
#[test]
fn set_063_tab_after_an_existing_tab_accepted() {
    let mut entry = tab("alpha", "Alpha");
    entry["after"] = json!("terminal");
    assert!(!has(
        &manifest_with_settings_entries(vec![entry]),
        Tier::T2,
        "manifest.settings-tab-anchor"
    ));
}

/// REQ: SET-064
#[test]
fn set_064_one_of_in_an_arguments_schema_refused_manifest_schema_keyword() {
    let mut manifest = minimal_manifest();
    manifest["contributes"] = json!({ "commands": [{
        "id": "go", "title": "Go",
        "arguments_schema": { "type": "object", "oneOf": [{ "type": "string" }] }
    }] });
    assert!(has(&manifest, Tier::T2, "manifest.schema-keyword"));
}

/// REQ: SET-064
#[test]
fn set_064_allowed_keywords_only_accepted() {
    let mut manifest = minimal_manifest();
    manifest["contributes"] = json!({ "commands": [{
        "id": "go", "title": "Go",
        "arguments_schema": { "type": "object", "properties": {
            "name": { "type": "string", "maxLength": 10, "title": "Name" } }, "required": ["name"] }
    }] });
    assert!(!has(&manifest, Tier::T2, "manifest.schema-keyword"));
}

/// REQ: SET-064
#[test]
fn set_064_each_unsupported_keyword_refused_manifest_schema_keyword() {
    for keyword in [
        "anyOf",
        "allOf",
        "not",
        "$ref",
        "if",
        "patternProperties",
        "format",
    ] {
        let mut schema = json!({ "type": "object", "properties": { "a": { "type": "string" } } });
        schema[keyword] = json!({});
        let mut manifest = minimal_manifest();
        manifest["contributes"] =
            json!({ "commands": [{ "id": "go", "title": "Go", "arguments_schema": schema }] });
        assert!(
            has(&manifest, Tier::T2, "manifest.schema-keyword"),
            "keyword {keyword}"
        );
    }
}

// ---- property tests --------------------------------------------------------------------------

proptest! {
    /// REQ: SET-062
    #[test]
    fn set_062_every_key_matching_the_grammar_is_accepted(key in "[a-z][a-z0-9_]{0,63}") {
        let manifest = manifest_with_rows(vec![toggle(&key, true)]);
        prop_assert!(!has(&manifest, Tier::T2, "manifest.settings-key"));
    }

    /// REQ: SET-062
    #[test]
    fn set_062_every_key_starting_with_a_capital_or_space_is_refused(key in "[A-Z ][a-zA-Z0-9_ ]{0,10}") {
        let manifest = manifest_with_rows(vec![toggle(&key, true)]);
        prop_assert!(has(&manifest, Tier::T2, "manifest.settings-key"));
    }

    /// REQ: SET-001
    #[test]
    fn set_001_row_count_refused_exactly_above_24(count in 1usize..40) {
        let refused = has(&manifest_with_rows(rows_with_prefix(count, "r")), Tier::T2, LIMIT);
        prop_assert_eq!(refused, count > 24);
    }

    /// REQ: SET-003
    #[test]
    fn set_003_row_title_refused_exactly_above_48(length in 1usize..80) {
        let refused = has(&manifest_with_rows(vec![row_with_title(length)]), Tier::T2, LIMIT);
        prop_assert_eq!(refused, length > 48);
    }

    /// REQ: SET-060
    #[test]
    fn set_060_option_count_refused_exactly_above_64(count in 2usize..100) {
        let refused = has(&manifest_with_rows(vec![choice_with_options(count)]), Tier::T2, LIMIT);
        prop_assert_eq!(refused, count > 64);
    }
}
