//! Requirements ACC-001 .. ACC-011 (keyboard operation and focus) of the plugin manual page
//! `a11y-keyboard-and-focus`. The checks run against the accessible plugin-view harness in
//! `ilium_plugin_host::a11y`. Each requirement has two tests; several are red until the
//! harness or the plugin-view path gains the behaviour (see the report of this lane).

use ilium_plugin_host::a11y::{render_view, A11yScreen, A11ySettings, CellStyle, Key};

fn open_view(view_json: &str) -> A11yScreen {
    A11yScreen::open(view_json, &A11ySettings::default())
}

fn repo_file(relative: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

fn is_focus_attribute(style: CellStyle) -> bool {
    style.bold || style.reverse || style.underline
}

const THREE_BUTTONS: &str = r#"{"widgets":[
  {"id":"save","kind":"button","label":"Save","row":0,"col":0},
  {"id":"open","kind":"button","label":"Open","row":0,"col":1},
  {"id":"close","kind":"button","label":"Close","row":1,"col":0}]}"#;

const TWO_ROWS: &str = r#"{"widgets":[
  {"id":"first","kind":"list_row","label":"Alpha","selected":false},
  {"id":"second","kind":"list_row","label":"Beta","selected":false}]}"#;

const DISABLED_PUBLISH: &str = r#"{"widgets":[
  {"id":"publish","kind":"button","label":"Publish","disabled":true,"disabled_reason":"Sign in first"}]}"#;

const DRAG_ROW_WITH_MOVE: &str = r#"{"widgets":[
  {"id":"draft","kind":"list_row","label":"Draft","draggable":true},
  {"id":"up","kind":"button","label":"Move draft up"}]}"#;

const MODAL_WITH_INPUT: &str = r#"{"modal":true,"widgets":[
  {"id":"name","kind":"input","label":"File name"},
  {"id":"ok","kind":"button","label":"OK"}]}"#;

const MODAL_DIALOG: &str = r#"{"modal":true,"widgets":[
  {"id":"ok","kind":"button","label":"OK"}]}"#;

const INSTRUCTION_VIEW: &str = r#"{"widgets":[
  {"id":"instruction","kind":"label","text":"Press Enter to finish the install"},
  {"id":"next","kind":"button","label":"Next"}]}"#;

/// REQ: ACC-001
#[test]
fn a11y_001_every_click_action_has_a_focusable_widget_a() {
    let screen = open_view(THREE_BUTTONS);
    let order = screen.focus_order();
    for id in ["save", "open", "close"] {
        assert!(
            order.iter().any(|item| item == id),
            "no keyboard path to {id}: {order:?}"
        );
    }
}

/// REQ: ACC-001
#[test]
fn a11y_001_tab_reaches_the_last_action_b() {
    let mut screen = open_view(THREE_BUTTONS);
    let count = screen.focus_order().len();
    for _ in 1..count {
        screen.press(Key::Tab);
    }
    assert_eq!(screen.focused_id(), "close");
}

/// REQ: ACC-002
#[test]
fn a11y_002_tree_order_without_tab_index_gives_the_focus_order_a() {
    let screen = open_view(THREE_BUTTONS);
    assert_eq!(screen.focus_order(), vec!["save", "open", "close"]);
}

/// REQ: ACC-002
#[test]
fn a11y_002_focus_order_follows_the_rendered_reading_order_b() {
    let screen = open_view(THREE_BUTTONS);
    let rows = screen.frame().rows();
    let positions: Vec<usize> = ["Save", "Open", "Close"]
        .iter()
        .map(|label| {
            rows.iter()
                .position(|row| row.contains(label))
                .unwrap_or_else(|| panic!("no row shows {label}: {rows:?}"))
        })
        .collect();
    assert!(
        positions.windows(2).all(|pair| pair[0] <= pair[1]),
        "rendered order {positions:?} differs from focus order"
    );
}

/// REQ: ACC-003
#[test]
fn a11y_003_tab_visits_each_focusable_node_once_in_order_a() {
    let mut screen = open_view(THREE_BUTTONS);
    let order = screen.focus_order();
    let mut visited = Vec::new();
    for _ in 0..order.len() {
        visited.push(screen.focused_id().to_string());
        screen.press(Key::Tab);
    }
    assert_eq!(visited, order);
}

/// REQ: ACC-003
#[test]
fn a11y_003_tab_after_the_last_node_returns_to_the_first_b() {
    let mut screen = open_view(THREE_BUTTONS);
    let order = screen.focus_order();
    for _ in 0..order.len() {
        screen.press(Key::Tab);
    }
    assert_eq!(screen.focused_id(), order[0]);
}

/// REQ: ACC-004
#[test]
fn a11y_004_disabled_node_shows_its_reason_in_the_screen_text_a() {
    let screen = open_view(DISABLED_PUBLISH);
    let rows = screen.frame().rows();
    assert!(
        rows.iter().any(|row| row.contains("Sign in first")),
        "the disabled reason is not on screen: {rows:?}"
    );
}

/// REQ: ACC-004
#[test]
fn a11y_004_disabled_reason_is_visible_in_screen_reader_mode_b() {
    let settings = A11ySettings {
        screen_reader: true,
        ..A11ySettings::default()
    };
    let frame = render_view(DISABLED_PUBLISH, &settings);
    assert!(
        frame.row_containing("Sign in first").is_some(),
        "the disabled reason is missing in screen reader mode"
    );
}

/// REQ: ACC-005
#[test]
fn a11y_005_unfocused_row_has_no_plugin_written_focus_marker_a() {
    let screen = open_view(TWO_ROWS);
    let row = screen
        .frame()
        .row_containing("Beta")
        .expect("the Beta row is drawn");
    assert!(
        !row.trim_start().starts_with('>'),
        "the plugin drew its own focus glyph: {row:?}"
    );
}

/// REQ: ACC-005
#[test]
fn a11y_005_focused_row_after_tab_has_no_plugin_written_focus_marker_b() {
    let mut screen = open_view(TWO_ROWS);
    screen.press(Key::Tab);
    let row = screen
        .frame()
        .row_containing("Beta")
        .expect("the Beta row is drawn");
    assert!(
        !row.trim_start().starts_with('>'),
        "the focused row carries a plugin-written marker: {row:?}"
    );
}

/// REQ: ACC-006
#[test]
fn a11y_006_focused_widget_has_an_attribute_besides_colour_a() {
    let screen = open_view(THREE_BUTTONS);
    let style = screen.frame().style_of("Save");
    assert!(
        is_focus_attribute(style),
        "the focused button has no bold, underline or reverse cell: {style:?}"
    );
}

/// REQ: ACC-006
#[test]
fn a11y_006_unfocused_widget_has_no_focus_attribute_b() {
    let screen = open_view(THREE_BUTTONS);
    let style = screen.frame().style_of("Open");
    assert!(
        !is_focus_attribute(style),
        "an unfocused button carries a focus attribute: {style:?}"
    );
}

/// REQ: ACC-007
#[test]
fn a11y_007_focused_key_removed_by_new_tree_is_not_kept_a() {
    let mut screen = open_view(THREE_BUTTONS);
    screen.press(Key::Tab);
    assert_eq!(screen.focused_id(), "open");
    let rebuilt = r#"{"widgets":[
      {"id":"save","kind":"button","label":"Save","row":0,"col":0},
      {"id":"close","kind":"button","label":"Close","row":1,"col":0}]}"#;
    let new_screen = open_view(rebuilt);
    assert_ne!(new_screen.focused_id(), "open");
    assert!(!new_screen.focused_id().is_empty());
}

/// REQ: ACC-007
#[test]
fn a11y_007_rebuilt_tree_has_no_focus_on_the_removed_key_b() {
    let rebuilt = r#"{"widgets":[
      {"id":"save","kind":"button","label":"Save","row":0,"col":0}]}"#;
    let screen = open_view(rebuilt);
    assert!(
        !screen.focus_order().iter().any(|id| id == "open"),
        "the removed key is still focusable"
    );
}

/// REQ: ACC-008
#[test]
fn a11y_008_escape_closes_a_modal_dialog_a() {
    let mut screen = open_view(MODAL_DIALOG);
    screen.press(Key::Esc);
    assert!(screen.is_closed(), "Escape did not close the dialog");
}

/// REQ: ACC-008
#[test]
fn a11y_008_escape_on_a_plain_view_does_not_close_it_b() {
    let mut screen = open_view(THREE_BUTTONS);
    screen.press(Key::Esc);
    assert!(!screen.is_closed(), "Escape closed a view that is not a dialog");
}

/// REQ: ACC-009
#[test]
fn a11y_009_tab_moves_focus_even_with_an_input_focused_a() {
    let mut screen = open_view(MODAL_WITH_INPUT);
    assert_eq!(screen.focused_id(), "name");
    screen.press(Key::Tab);
    assert_eq!(screen.focused_id(), "ok", "Tab did not reach the next node");
}

/// REQ: ACC-009
#[test]
fn a11y_009_escape_closes_the_modal_while_an_input_is_focused_b() {
    let mut screen = open_view(MODAL_WITH_INPUT);
    screen.press(Key::Esc);
    assert!(screen.is_closed(), "the view kept Escape for itself");
}

/// REQ: ACC-010
#[test]
fn a11y_010_drag_row_has_a_focusable_keyboard_move_action_a() {
    let screen = open_view(DRAG_ROW_WITH_MOVE);
    assert!(
        screen.focus_order().iter().any(|id| id == "up"),
        "no keyboard move action for the draggable row: {:?}",
        screen.focus_order()
    );
}

/// REQ: ACC-010
#[test]
fn a11y_010_keyboard_move_action_is_reached_by_tab_b() {
    let mut screen = open_view(DRAG_ROW_WITH_MOVE);
    let count = screen.focus_order().len();
    let mut reached = false;
    for _ in 0..count {
        if screen.focused_id() == "up" {
            reached = true;
            break;
        }
        screen.press(Key::Tab);
    }
    assert!(reached, "Tab never reaches the move action");
}

/// REQ: ACC-011
#[test]
fn a11y_011_instruction_text_is_on_screen_without_focus_a() {
    let screen = open_view(INSTRUCTION_VIEW);
    assert!(
        screen.frame().row_containing("Press Enter").is_some(),
        "the view that needs the user has no instruction text"
    );
}

/// REQ: ACC-011
#[test]
fn a11y_011_focus_is_unchanged_when_the_view_was_never_opened_b() {
    let screen = open_view(INSTRUCTION_VIEW);
    assert_eq!(
        screen.focused_id(),
        "next",
        "focus moved into a view the user has not opened"
    );
    let manual = repo_file("docs/plugins-manual/pages/a11y-keyboard-and-focus.md");
    assert!(manual.contains("ACC-011"), "the manual page lost ACC-011");
}
