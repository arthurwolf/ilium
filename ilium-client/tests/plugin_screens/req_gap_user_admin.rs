//! Gap requirements ACC-000 and ACC-023 .. ACC-035 (accessibility of plugin views as a user and an
//! administrator meets them). Checks run against the accessible plugin-view harness
//! (`ilium_plugin_host::a11y`), the view validator (`ilium_plugin_host::view`), the manifest lint
//! (`ilium_plugin_host::devtools`) and the screens rig. Each requirement has two tests: `a` is the
//! specified behaviour, `b` the refusal or edge case. Several are red until the harness gains the
//! behaviour the requirement names.

use ilium_client::plugin_host::screens_rig::ScreensRig;
use ilium_plugin_host::a11y::{A11yScreen, A11ySettings, Key};
use ilium_plugin_host::devtools::lint;
use ilium_plugin_host::view::validate_view;

fn reader_settings() -> A11ySettings {
    A11ySettings {
        screen_reader: true,
        ..A11ySettings::default()
    }
}

fn open_view(view_json: &str) -> A11yScreen {
    A11yScreen::open(view_json, &A11ySettings::default())
}

fn open_reader_view(view_json: &str) -> A11yScreen {
    A11yScreen::open(view_json, &reader_settings())
}

fn row_index(rows: &[String], needle: &str) -> usize {
    rows.iter()
        .position(|row| row.contains(needle))
        .unwrap_or_else(|| panic!("no row contains {needle:?}: {rows:?}"))
}

fn has_braille_cell(text: &str) -> bool {
    text.chars().any(|cell| ('\u{2800}'..='\u{28FF}').contains(&cell))
}

fn numbered_buttons(count: usize) -> String {
    let widgets: Vec<String> = (1..=count)
        .map(|number| {
            format!(
                r#"{{"id":"item{number}","kind":"button","label":"Item {number}","row":{},"col":0}}"#,
                number - 1
            )
        })
        .collect();
    format!(r#"{{"widgets":[{}]}}"#, widgets.join(","))
}

fn lint_project_with_manifest(manifest_json: &str) -> Vec<String> {
    let project = tempfile::tempdir().expect("create a temporary plugin project");
    std::fs::write(project.path().join("manifest.json"), manifest_json)
        .expect("write the fixture manifest");
    lint(project.path())
        .iter()
        .map(|finding| format!("{finding:?}"))
        .collect()
}

const HOVER_ROW_VIEW: &str = r#"{"widgets":[
  {"id":"row","kind":"list_row","label":"Build 42","selected":true,
   "decorations":[{"visibility":"on-hover","text":"Open log"}]},
  {"id":"other","kind":"button","label":"Other"}]}"#;

const TABLE_VIEW: &str = r#"{"widgets":[
  {"id":"builds","kind":"table",
   "columns":[{"id":"name","title":"Name"},{"id":"size","title":"Size"},{"id":"owner","title":"Owner"}],
   "rows":[{"id":"r1","cells":{"name":"alpha","size":"4 KB","owner":"ann"}}]}]}"#;

const TABLE_EMPTY_TITLE_VIEW: &str = r#"{"widgets":[
  {"id":"builds","kind":"table",
   "columns":[{"id":"name","title":"Name"},{"id":"size","title":""}],
   "rows":[{"id":"r1","cells":{"name":"alpha","size":"4 KB"}}]}]}"#;

const TWO_BLOCKS_VIEW: &str = r#"{"widgets":[
  {"id":"install","kind":"block","title":"Install","children":[
    {"id":"yes","kind":"button","label":"Yes"},{"id":"later","kind":"button","label":"Later"}]},
  {"id":"remove","kind":"block","title":"Remove","children":[
    {"id":"confirm","kind":"button","label":"Confirm"},{"id":"cancel","kind":"button","label":"Cancel"}]}]}"#;

const BLOCK_WITHOUT_TITLE_VIEW: &str = r#"{"widgets":[
  {"id":"untitled","kind":"block","children":[{"id":"go","kind":"button","label":"Go"}]}]}"#;

const NOTICE_GAUGE_BUTTON_VIEW: &str = r#"{"widgets":[
  {"id":"notice","kind":"notice","text":"Heads up: low disk space"},
  {"id":"disk","kind":"gauge","label":"Disk","value":0.37},
  {"id":"ok","kind":"button","label":"OK"}]}"#;

const TREE_VIEW: &str = r#"{"widgets":[
  {"id":"projects","kind":"tree_item","label":"Projects","level":1,"expandable":true,"expanded":false},
  {"id":"notes","kind":"tree_item","label":"Notes","level":2}]}"#;

const TREE_EXPANDED_VIEW: &str = r#"{"widgets":[
  {"id":"projects","kind":"tree_item","label":"Projects","level":1,"expandable":true,"expanded":true},
  {"id":"notes","kind":"tree_item","label":"Notes","level":2}]}"#;

const SECRET_INPUT_VIEW: &str = r#"{"widgets":[
  {"id":"password","kind":"input","label":"Password","secret":true,"value":"hunter2"}]}"#;

const CURSOR_VIEW: &str = r#"{"widgets":[
  {"id":"first","kind":"button","label":"First"},
  {"id":"second","kind":"button","label":"Second"},
  {"id":"third","kind":"button","label":"Third"}]}"#;

const CURSOR_WITH_HEADER_VIEW: &str = r#"{"widgets":[
  {"id":"header","kind":"label","text":"Header"},
  {"id":"first","kind":"button","label":"First"},
  {"id":"second","kind":"button","label":"Second"},
  {"id":"third","kind":"button","label":"Third"}]}"#;

const CONFIRM_MODAL_VIEW: &str = r#"{"modal":true,"widgets":[
  {"id":"name","kind":"input","label":"File name"},
  {"id":"ok","kind":"button","label":"OK"}]}"#;

const NO_FOCUS_VIEW: &str = r#"{"widgets":[
  {"id":"note","kind":"label","text":"Nothing to do"}]}"#;

const REVEAL_ON_FOCUS_VIEW: &str = r#"{"widgets":[
  {"id":"node","kind":"button","label":"Details"},
  {"id":"other","kind":"button","label":"Other"},
  {"id":"preview","kind":"label","text":"Preview: build log","shown_when_focused":"node"}]}"#;

const SPINNER_LABELLED_VIEW: &str = r#"{"widgets":[
  {"id":"indexing","kind":"spinner","label":"Indexing"}]}"#;

const SPINNER_UNLABELLED_VIEW: &str = r#"{"widgets":[
  {"id":"spinner-1","kind":"spinner"}]}"#;

const CONFIRM_DIALOG_VIEW: &str = r#"{"modal":true,"widgets":[
  {"id":"keep","kind":"button","label":"Keep"},
  {"id":"delete","kind":"button","label":"Delete"}]}"#;

/// REQ: ACC-000
#[test]
fn req_acc_000_hover_decoration_shows_on_keyboard_focused_row_a() {
    let screen = open_view(HOVER_ROW_VIEW);
    assert_eq!(screen.focused_id(), "row");
    let rows = screen.frame().rows();
    assert!(
        rows.iter().any(|row| row.contains("Open log")),
        "the on-hover decoration is missing on the keyboard-focused row: {rows:?}"
    );
}

/// REQ: ACC-000
#[test]
fn req_acc_000_hover_decoration_is_gone_when_focus_leaves_row_b() {
    let mut screen = open_view(HOVER_ROW_VIEW);
    screen.press(Key::Tab);
    assert_eq!(screen.focused_id(), "other");
    let rows = screen.frame().rows();
    assert!(
        !rows.iter().any(|row| row.contains("Open log")),
        "the on-hover decoration stayed after focus moved away: {rows:?}"
    );
}

/// REQ: ACC-023
#[test]
fn req_acc_023_table_row_reads_column_title_before_cell_value_a() {
    let screen = open_reader_view(TABLE_VIEW);
    let rows = screen.frame().rows();
    let row = rows
        .iter()
        .find(|row| row.contains("alpha"))
        .unwrap_or_else(|| panic!("no table row shows alpha: {rows:?}"));
    assert!(
        row.trim_start().starts_with("Name: alpha"),
        "the first column is not read as its title then its value: {row:?}"
    );
    assert!(
        row.contains("Size: 4 KB") && row.contains("Owner: ann"),
        "later columns are not read as title: value: {row:?}"
    );
}

/// REQ: ACC-023
#[test]
fn req_acc_023_table_column_without_title_fails_validation_b() {
    let error = validate_view(TABLE_EMPTY_TITLE_VIEW)
        .expect_err("a table column with an empty title must fail the view validator");
    let message = format!("{error}");
    assert!(
        message.contains("ui.label-missing") && message.contains("size"),
        "the refusal must be ui.label-missing naming the column id: {message}"
    );
}

/// REQ: ACC-024
#[test]
fn req_acc_024_block_title_is_read_on_its_own_line_before_its_buttons_a() {
    let screen = open_reader_view(TWO_BLOCKS_VIEW);
    let rows = screen.frame().rows();
    let install_title = row_index(&rows, "Install");
    let yes_button = row_index(&rows, "Yes");
    assert_eq!(
        rows[install_title].trim(),
        "Install",
        "the block title is not on its own line: {rows:?}"
    );
    assert_eq!(
        yes_button,
        install_title + 1,
        "the first button does not follow the block title directly: {rows:?}"
    );
    let remove_title = row_index(&rows, "Remove");
    assert_eq!(row_index(&rows, "Confirm"), remove_title + 1, "{rows:?}");
}

/// REQ: ACC-024
#[test]
fn req_acc_024_block_with_focusable_child_and_no_title_fails_validation_b() {
    let error = validate_view(BLOCK_WITHOUT_TITLE_VIEW)
        .expect_err("a block with a focusable child and no title must be refused");
    let message = format!("{error}");
    assert!(
        message.contains("block"),
        "the refusal must name the untitled block: {message}"
    );
}

/// REQ: ACC-025
#[test]
fn req_acc_025_text_widgets_are_plain_lines_and_gauge_reads_as_percent_a() {
    let screen = open_reader_view(NOTICE_GAUGE_BUTTON_VIEW);
    let rows = screen.frame().rows();
    let notice = row_index(&rows, "Heads up: low disk space");
    let button = row_index(&rows, "[ OK ]");
    assert!(
        notice < button,
        "the notice must print before the focusable button: {rows:?}"
    );
    let gauge = rows
        .iter()
        .find(|row| row.contains("Disk"))
        .unwrap_or_else(|| panic!("no gauge line: {rows:?}"));
    assert!(
        gauge.trim_end().ends_with("37 percent"),
        "the gauge must end with its percent as digits and the word percent: {gauge:?}"
    );
}

/// REQ: ACC-025
#[test]
fn req_acc_025_gauge_is_not_read_as_block_glyphs_b() {
    let screen = open_reader_view(NOTICE_GAUGE_BUTTON_VIEW);
    let text = screen.frame().rows().join("\n");
    assert!(
        !text.contains('\u{2588}') && !text.contains('\u{2591}'),
        "the gauge bar cells were read aloud: {text}"
    );
}

/// REQ: ACC-026
#[test]
fn req_acc_026_tree_rows_read_level_and_position_a() {
    let screen = open_reader_view(TREE_VIEW);
    let rows = screen.frame().rows();
    let parent = rows
        .iter()
        .find(|row| row.contains("Projects"))
        .unwrap_or_else(|| panic!("no tree row for Projects: {rows:?}"));
    assert!(
        parent.trim_end().ends_with("1 of 2, level 1, collapsed"),
        "the collapsed parent must read position, level and state: {parent:?}"
    );
    let leaf = rows
        .iter()
        .find(|row| row.contains("Notes"))
        .unwrap_or_else(|| panic!("no tree row for Notes: {rows:?}"));
    assert!(leaf.contains("level 2"), "the leaf must read its level: {leaf:?}");
}

/// REQ: ACC-026
#[test]
fn req_acc_026_expanded_parent_reads_expanded_b() {
    let screen = open_reader_view(TREE_EXPANDED_VIEW);
    let rows = screen.frame().rows();
    let parent = rows
        .iter()
        .find(|row| row.contains("Projects"))
        .unwrap_or_else(|| panic!("no tree row for Projects: {rows:?}"));
    assert!(
        parent.trim_end().ends_with("expanded"),
        "an expanded parent must end with the word expanded: {parent:?}"
    );
}

/// REQ: ACC-027
#[test]
fn req_acc_027_secret_field_reads_filled_without_its_value_a() {
    let screen = open_reader_view(SECRET_INPUT_VIEW);
    let rows = screen.frame().rows();
    assert!(
        rows.iter()
            .any(|row| row.contains("Password, text field, filled")),
        "the secret field must read as label, text field, filled: {rows:?}"
    );
    let text = rows.join("\n");
    assert!(
        !text.contains("hun"),
        "a character of the secret value was printed: {text}"
    );
}

/// REQ: ACC-027
#[test]
fn req_acc_027_secret_value_is_not_printed_in_plain_view_b() {
    let screen = open_view(SECRET_INPUT_VIEW);
    let text = screen.frame().rows().join("\n");
    assert!(
        !text.contains("hunter2"),
        "the secret value is printed on screen: {text}"
    );
}

/// REQ: ACC-028
#[test]
fn req_acc_028_cursor_stays_on_focused_element_after_redraw_a() {
    let mut screen = open_reader_view(CURSOR_VIEW);
    screen.press(Key::Tab);
    screen.press(Key::Tab);
    assert_eq!(screen.focused_id(), "third");
    let rows = screen.frame().rows();
    assert_eq!(
        usize::from(screen.cursor_position().1),
        row_index(&rows, "[ Third ]"),
        "the cursor must sit on the row of the focused element, not the first line"
    );
    assert_eq!(screen.cursor_position(), screen.focused_cell());
}

/// REQ: ACC-028
#[test]
fn req_acc_028_cursor_follows_focused_element_when_a_row_is_added_above_b() {
    let mut screen = open_reader_view(CURSOR_WITH_HEADER_VIEW);
    screen.press(Key::Tab);
    screen.press(Key::Tab);
    assert_eq!(screen.focused_id(), "third");
    let rows = screen.frame().rows();
    assert_eq!(
        usize::from(screen.cursor_position().1),
        row_index(&rows, "[ Third ]"),
        "the cursor did not follow the focused element after a row was added above it"
    );
}

/// REQ: ACC-029
#[test]
fn req_acc_029_opened_modal_places_focus_on_first_focusable_node_a() {
    let screen = open_view(CONFIRM_MODAL_VIEW);
    let order = screen.focus_order();
    assert_eq!(order.first().map(String::as_str), Some("name"));
    assert_eq!(
        screen.focused_id(),
        order[0],
        "focus must land on the first node of the focus order without a click"
    );
}

/// REQ: ACC-029
#[test]
fn req_acc_029_view_without_focusable_node_keeps_focus_and_sends_nothing_b() {
    let screen = open_view(NO_FOCUS_VIEW);
    assert_eq!(screen.focused_id(), "");
    assert!(!screen.is_closed(), "the view must not close by itself");
    assert!(
        screen.log_codes().is_empty(),
        "no action should be logged for a view with nothing to focus: {:?}",
        screen.log_codes()
    );
}

/// REQ: ACC-030
#[test]
fn req_acc_030_focus_moves_into_view_of_scroll_container_a() {
    let mut screen = open_view(&numbered_buttons(30));
    for _ in 0..19 {
        screen.press(Key::Tab);
    }
    assert_eq!(screen.focused_id(), "item20");
    let rows = screen.frame().rows();
    assert!(
        rows.iter().any(|row| row.contains("[ Item 20 ]")),
        "the focused button is not scrolled into view: {rows:?}"
    );
}

/// REQ: ACC-030
#[test]
fn req_acc_030_focus_back_to_first_button_scrolls_it_back_into_view_b() {
    let mut screen = open_view(&numbered_buttons(30));
    for _ in 0..30 {
        screen.press(Key::Tab);
    }
    assert_eq!(screen.focused_id(), "item1");
    let rows = screen.frame().rows();
    assert!(
        rows.iter().any(|row| row.contains("[ Item 1 ]")),
        "the first button is not visible after focus returned to it: {rows:?}"
    );
}

/// REQ: ACC-031
#[test]
fn req_acc_031_labelled_spinner_reads_label_then_busy_a() {
    let screen = open_reader_view(SPINNER_LABELLED_VIEW);
    let rows = screen.frame().rows();
    let row = rows
        .iter()
        .find(|row| row.contains("Indexing"))
        .unwrap_or_else(|| panic!("no spinner line: {rows:?}"));
    assert_eq!(
        row.trim(),
        "Indexing, busy",
        "the spinner must read as its label then busy"
    );
    assert!(
        !has_braille_cell(row),
        "a Braille spinner frame glyph was drawn: {row:?}"
    );
}

/// REQ: ACC-031
#[test]
fn req_acc_031_unlabelled_spinner_fails_validation_b() {
    let error = validate_view(SPINNER_UNLABELLED_VIEW)
        .expect_err("a spinner without a label must be refused by the view validator");
    let message = format!("{error}");
    assert!(
        message.contains("ui.label-missing") && message.contains("spinner-1"),
        "the refusal must be ui.label-missing naming the node: {message}"
    );
}

/// REQ: ACC-032
#[test]
fn req_acc_032_content_shown_on_focus_appears_when_node_takes_focus_a() {
    let screen = open_view(REVEAL_ON_FOCUS_VIEW);
    assert_eq!(screen.focused_id(), "node");
    let rows = screen.frame().rows();
    assert!(
        rows.iter().any(|row| row.contains("Preview: build log")),
        "the preview shown on focus is missing while its node has focus: {rows:?}"
    );
}

/// REQ: ACC-032
#[test]
fn req_acc_032_content_shown_on_focus_is_hidden_when_focus_leaves_b() {
    let mut screen = open_view(REVEAL_ON_FOCUS_VIEW);
    screen.press(Key::Tab);
    assert_eq!(screen.focused_id(), "other");
    let rows = screen.frame().rows();
    assert!(
        !rows.iter().any(|row| row.contains("Preview: build log")),
        "the preview stayed after focus left its node: {rows:?}"
    );
}

/// REQ: ACC-033
#[test]
fn req_acc_033_flipping_screen_reader_redraws_open_plugin_views_a() {
    let mut rig = ScreensRig::new(80, 24);
    rig.start_client();
    rig.open_plugins_tab();
    let before = rig.screen_text();
    rig.set_screen_reader(true);
    let after = rig.screen_text();
    assert_ne!(
        before, after,
        "the open view was not drawn again in screen reader mode"
    );
    assert!(
        !rig.speech_lines().is_empty(),
        "the screen reader mode must speak the view without a restart"
    );
}

/// REQ: ACC-033
#[test]
fn req_acc_033_returning_to_visual_mode_restores_the_first_drawing_b() {
    let mut rig = ScreensRig::new(80, 24);
    rig.start_client();
    rig.open_plugins_tab();
    let before = rig.screen_text();
    rig.set_screen_reader(true);
    rig.set_screen_reader(false);
    assert_eq!(
        rig.screen_text(),
        before,
        "switching back to the visual mode must restore the same drawing"
    );
}

/// REQ: ACC-034
#[test]
fn req_acc_034_awaiting_dialog_stays_open_and_sends_nothing_on_timer_a() {
    let mut rig = ScreensRig::new(80, 24);
    rig.start_client();
    rig.open_trust_prompt("Example Publisher", "ab:cd:ef");
    let default_before = rig.default_button_label();
    rig.advance_ms(120_000);
    assert!(
        rig.screen_text().contains("Example Publisher"),
        "the decision dialog closed on a timer: {}",
        rig.screen_text()
    );
    assert!(
        rig.trust_answers().is_empty(),
        "an answer was sent without a user action"
    );
    assert_eq!(rig.default_button_label(), default_before);
}

/// REQ: ACC-034
#[test]
fn req_acc_034_dialog_default_button_does_not_change_on_timer_b() {
    let mut rig = ScreensRig::new(80, 24);
    rig.start_client();
    rig.open_trust_prompt("Example Publisher", "ab:cd:ef");
    let default_before = rig.default_button_label();
    assert!(default_before.is_some(), "the trust prompt has no default button");
    rig.advance_ms(120_000);
    assert_eq!(
        rig.default_button_label(),
        default_before,
        "the default button changed while the dialog waited for a decision"
    );
    assert!(
        rig.replace_answers().is_empty(),
        "a decision was recorded without a user action"
    );
}

/// REQ: ACC-035
#[test]
fn req_acc_035_manifest_binding_f1_fails_lint_a() {
    let findings = lint_project_with_manifest(
        r#"{"id":"example.plugin","keybindings":{"F1":"example.command"}}"#,
    );
    assert!(
        findings.iter().any(|finding| finding.contains("F1")),
        "lint did not refuse a manifest that binds F1: {findings:?}"
    );
}

/// REQ: ACC-035
#[test]
fn req_acc_035_manifest_binding_shift_f10_or_menu_fails_lint_b() {
    let findings = lint_project_with_manifest(
        r#"{"id":"example.plugin","keybindings":{"Shift+F10":"example.command","Menu":"example.other"}}"#,
    );
    assert!(
        findings
            .iter()
            .any(|finding| finding.contains("Shift+F10") || finding.contains("Menu")),
        "lint did not refuse Shift+F10 or Menu bindings: {findings:?}"
    );
}

