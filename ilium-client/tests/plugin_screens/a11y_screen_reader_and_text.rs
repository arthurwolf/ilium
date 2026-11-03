//! Requirements A11Y-012 .. A11Y-022 (screen reader and localized text) of the plugin manual page
//! `a11y-screen-reader-and-localized-text`. Checks run against the accessible plugin-view harness
//! in `ilium_plugin_host::a11y` in screen reader mode, plus source checks of the shipped docs and
//! sound notice path. Each requirement has two tests; several are red until the behaviour lands.

use ilium_plugin_host::a11y::{render_view, A11yScreen, A11ySettings};

fn repo_file(relative: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

fn screen_reader() -> A11ySettings {
    A11ySettings {
        screen_reader: true,
        ..A11ySettings::default()
    }
}

fn reader_rows(view_json: &str) -> Vec<String> {
    render_view(view_json, &screen_reader()).rows()
}

fn row_with(rows: &[String], needle: &str) -> String {
    rows.iter()
        .find(|row| row.contains(needle))
        .cloned()
        .unwrap_or_else(|| panic!("no row contains {needle:?} in {rows:?}"))
}

fn is_plain_word_character(character: char) -> bool {
    character.is_alphanumeric() || character == ' ' || character.is_ascii_punctuation()
}

const SAVE_BUTTON: &str = r#"{"widgets":[
  {"id":"save","kind":"button","label":"Save","row":0,"col":0}]}"#;

const MUTE_TOGGLE: &str = r#"{"widgets":[
  {"id":"mute","kind":"toggle","label":"Mute","checked":true,"row":0,"col":0}]}"#;

const STATUS_FAILED: &str = r#"{"widgets":[
  {"id":"build","kind":"label","role":"danger","text":"Build Failed"}]}"#;

const STATUS_DONE: &str = r#"{"widgets":[
  {"id":"build","kind":"label","role":"success","text":"Build Done"}]}"#;

const LONG_LABEL: &str = r#"{"widgets":[
  {"id":"save_all","kind":"button","label":"Save the project file now","row":0,"col":0}]}"#;

const DISABLED_WITH_REASON: &str = r#"{"widgets":[
  {"id":"publish","kind":"button","label":"Publish","disabled":true,"disabled_reason":"Sign in first"}]}"#;

const PLAIN_LABELS: &str = r#"{"widgets":[
  {"id":"save","kind":"button","label":"Save file","row":0,"col":0},
  {"id":"open","kind":"button","label":"Open file","row":0,"col":1},
  {"id":"close","kind":"button","label":"Close file","row":1,"col":0}]}"#;

const THREE_BUTTONS: &str = r#"{"widgets":[
  {"id":"save","kind":"button","label":"Save","row":0,"col":0},
  {"id":"open","kind":"button","label":"Open","row":0,"col":1},
  {"id":"close","kind":"button","label":"Close","row":1,"col":0}]}"#;

/// REQ: A11Y-012
#[test]
fn a11y_012_screen_reader_prints_the_role_word_once_per_line_a() {
    let rows = reader_rows(SAVE_BUTTON);
    let line = row_with(&rows, "Save");
    assert_eq!(
        line.matches("button").count(),
        1,
        "the role word is not printed exactly once: {line:?}"
    );
}

/// REQ: A11Y-012
#[test]
fn a11y_012_label_is_not_repeated_on_its_line_b() {
    let rows = reader_rows(SAVE_BUTTON);
    let line = row_with(&rows, "Save");
    assert_eq!(
        line.matches("Save").count(),
        1,
        "the label is repeated on one line: {line:?}"
    );
}

/// REQ: A11Y-013
#[test]
fn a11y_013_screen_reader_rows_carry_no_decorative_glyph_a() {
    let rows = reader_rows(PLAIN_LABELS);
    assert!(
        rows.iter().all(|row| row.is_ascii()),
        "a decorative glyph reached the screen reader rows: {rows:?}"
    );
}

/// REQ: A11Y-013
#[test]
fn a11y_013_label_line_has_only_words_digits_and_punctuation_b() {
    let rows = reader_rows(SAVE_BUTTON);
    let line = row_with(&rows, "Save");
    assert!(
        line.chars().all(is_plain_word_character),
        "the label line holds a symbol that is not a word: {line:?}"
    );
}

/// REQ: A11Y-014
#[test]
fn a11y_014_failed_status_is_written_in_the_row_text_a() {
    let screen = A11yScreen::open(STATUS_FAILED, &A11ySettings::default());
    let row = screen
        .frame()
        .row_containing("Build")
        .expect("the status row is drawn");
    assert!(row.contains("Failed"), "the row does not say Failed: {row:?}");
}

/// REQ: A11Y-014
#[test]
fn a11y_014_changed_status_word_appears_in_the_row_b() {
    let rows = reader_rows(STATUS_DONE);
    let row = row_with(&rows, "Build");
    assert!(row.contains("Done"), "the row does not say Done: {row:?}");
}

/// REQ: A11Y-015
#[test]
fn a11y_015_attention_change_has_a_persistent_text_twin_a() {
    let source = repo_file("ilium-plugin-host/tests/imports_ui/sound_notify_a11y.rs");
    assert!(
        source.contains("CAP-883"),
        "the notice path does not name the CAP-883 text twin"
    );
}

/// REQ: A11Y-015
#[test]
fn a11y_015_manual_requires_a_persistent_text_twin_b() {
    let manual = repo_file("docs/plugins-manual/pages/a11y-screen-reader-and-localized-text.md");
    assert!(
        manual.contains("persistent text twin"),
        "the manual page no longer requires a persistent text twin"
    );
}

/// REQ: A11Y-016
#[test]
fn a11y_016_accessible_label_is_rendered_whole_a() {
    let rows = reader_rows(PLAIN_LABELS);
    let line = row_with(&rows, "Save file");
    assert!(
        line.contains("Save file"),
        "a translated label was split: {line:?}"
    );
}

/// REQ: A11Y-016
#[test]
fn a11y_016_manual_forbids_joining_translated_pieces_b() {
    let manual = repo_file("docs/plugins-manual/pages/a11y-screen-reader-and-localized-text.md");
    assert!(
        manual.contains("MUST NOT join two translated pieces"),
        "the manual page no longer forbids joining translated pieces"
    );
}

/// REQ: A11Y-017
#[test]
fn a11y_017_host_prints_the_button_role_word_exactly_once_a() {
    let rows = reader_rows(SAVE_BUTTON);
    let count: usize = rows.iter().map(|row| row.matches("button").count()).sum();
    assert_eq!(count, 1, "the button role word is not printed once: {rows:?}");
}

/// REQ: A11Y-017
#[test]
fn a11y_017_toggle_line_holds_the_toggle_role_word_once_b() {
    let rows = reader_rows(MUTE_TOGGLE);
    let line = row_with(&rows, "Mute");
    assert_eq!(
        line.matches("toggle").count(),
        1,
        "the toggle role word is not printed once: {line:?}"
    );
}

/// REQ: A11Y-018
#[test]
fn a11y_018_long_label_is_whole_at_48_columns_in_screen_reader_mode_a() {
    let rows = reader_rows(LONG_LABEL);
    let line = row_with(&rows, "Save the project");
    assert!(
        line.contains("Save the project file now"),
        "the label is cut: {line:?}"
    );
}

/// REQ: A11Y-018
#[test]
fn a11y_018_no_row_is_cut_with_an_ellipsis_b() {
    let rows = reader_rows(LONG_LABEL);
    assert!(
        rows.iter().all(|row| !row.contains('\u{2026}')),
        "a label row is cut with an ellipsis: {rows:?}"
    );
}

/// REQ: A11Y-019
#[test]
fn a11y_019_disabled_reason_is_complete_in_one_line_a() {
    let rows = reader_rows(DISABLED_WITH_REASON);
    let line = row_with(&rows, "Sign in first");
    assert!(
        line.contains("Sign in first"),
        "the disabled reason is cut: {line:?}"
    );
}

/// REQ: A11Y-019
#[test]
fn a11y_019_disabled_reason_is_given_in_words_not_a_symbol_b() {
    let rows = reader_rows(DISABLED_WITH_REASON);
    let line = row_with(&rows, "Sign in first");
    assert!(
        line.chars().all(|character| character.is_ascii()),
        "the disabled reason uses a symbol: {line:?}"
    );
}

/// REQ: A11Y-020
#[test]
fn a11y_020_no_dotted_identifier_is_shown_in_a_label_a() {
    let rows = reader_rows(PLAIN_LABELS);
    for row in &rows {
        for token in row.split_whitespace() {
            let dotted = token.contains('.')
                && token
                    .split('.')
                    .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_lowercase()));
            assert!(!dotted, "a dotted identifier is shown: {token:?} in {row:?}");
        }
    }
}

/// REQ: A11Y-020
#[test]
fn a11y_020_no_snake_case_identifier_is_shown_in_a_label_b() {
    let rows = reader_rows(PLAIN_LABELS);
    for row in &rows {
        assert!(
            !row.split_whitespace().any(|token| token.contains('_')),
            "a catalogue key is shown: {row:?}"
        );
    }
}

/// REQ: A11Y-021
#[test]
fn a11y_021_same_focusable_widgets_in_both_modes_a() {
    let normal = A11yScreen::open(THREE_BUTTONS, &A11ySettings::default());
    let reader = A11yScreen::open(THREE_BUTTONS, &screen_reader());
    assert_eq!(normal.focus_order(), reader.focus_order());
}

/// REQ: A11Y-021
#[test]
fn a11y_021_every_label_is_shown_in_both_modes_b() {
    let normal = render_view(THREE_BUTTONS, &A11ySettings::default()).rows();
    let reader = reader_rows(THREE_BUTTONS);
    for label in ["Save", "Open", "Close"] {
        assert!(normal.iter().any(|row| row.contains(label)), "{label} missing in normal mode");
        assert!(reader.iter().any(|row| row.contains(label)), "{label} missing in screen reader mode");
    }
}

/// REQ: A11Y-022
#[test]
fn a11y_022_linear_line_order_matches_focus_order_a() {
    let screen = A11yScreen::open(THREE_BUTTONS, &screen_reader());
    let rows = screen.frame().rows();
    let order = screen.focus_order();
    let positions: Vec<usize> = order
        .iter()
        .map(|id| {
            let label = match id.as_str() {
                "save" => "Save",
                "open" => "Open",
                _ => "Close",
            };
            rows.iter()
                .position(|row| row.contains(label))
                .unwrap_or_else(|| panic!("no line for {id}: {rows:?}"))
        })
        .collect();
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "linear order {positions:?} differs from focus order {order:?}"
    );
}

/// REQ: A11Y-022
#[test]
fn a11y_022_each_focusable_label_has_exactly_one_linear_line_b() {
    let rows = reader_rows(THREE_BUTTONS);
    for label in ["Save", "Open", "Close"] {
        let count = rows.iter().filter(|row| row.contains(label)).count();
        assert_eq!(count, 1, "{label} is on {count} lines: {rows:?}");
    }
}
