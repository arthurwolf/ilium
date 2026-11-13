//! Events handled and intents emitted: editor-opened/closed/changed/pointer and
//! "Create an agent from a line" (F-023, F-114; core-plugin-editor.md).

use crate::support::{active_host, buffer_ref, open_file};
use ilium_plugin_sdk::generated::types::{
    BufferId, EditorChangedEvent, EditorPointerEvent, Event, Intent, NodeId, PointerButton,
    PointerKind, PointerModifiers, TextPosition, ViewPointer,
};
use serde_json::json;

fn pointer(buffer: u64, line: u32, kind: PointerKind) -> Event {
    Event::EditorPointer(EditorPointerEvent {
        buffer: BufferId(buffer),
        pane: NodeId(7),
        position: TextPosition { line, column: 0 },
        pointer: ViewPointer {
            kind,
            button: PointerButton::Left,
            modifiers: PointerModifiers(0),
            column: 0,
            row: 0,
            sub_x: 128,
            sub_y: 128,
            clicks: 1,
        },
    })
}

fn todo_host(text: &str) -> ilium_plugin_sdk::testing::host::FakeHost {
    let mut host = active_host(&[]);
    open_file(&mut host, 1, "src/todo.rs", Some("rust"), text);
    host
}

/// REQ: F-023, F-114
#[test]
fn f_114_click_on_a_todo_line_creates_a_pane_with_the_line_as_input() {
    let mut host = todo_host("fn a() {}\nTODO write docs\n");
    let submissions = host.deliver_one(pointer(1, 1, PointerKind::Down));
    assert_eq!(submissions.len(), 1);
    let Intent::CreatePane(spec) = &submissions[0].intent else {
        panic!("create-pane")
    };
    assert_eq!(spec.content_kind, "ilium.terminal");
    let input = spec.initial_input.clone().unwrap_or_default();
    assert!(input.contains("TODO write docs"), "{input}");
}

/// REQ: F-023, F-114
#[test]
fn f_114_default_prompt_names_the_file_and_the_one_based_line() {
    let mut host = todo_host("fn a() {}\nTODO write docs\n");
    let submissions = host.deliver_one(pointer(1, 1, PointerKind::Down));
    let Intent::CreatePane(spec) = &submissions[0].intent else {
        panic!("create-pane")
    };
    let input = spec.initial_input.clone().unwrap_or_default();
    assert!(
        input.starts_with("/goal please do the following task: \"TODO write docs\""),
        "{input}"
    );
    assert!(input.contains("src/todo.rs at line 2"), "{input}");
}

/// REQ: F-023
#[test]
fn f_023_line_text_is_kept_verbatim_without_template_interpretation() {
    let mut host = todo_host("TODO <b>{{x}} $HOME \"q\"\n");
    let submissions = host.deliver_one(pointer(1, 0, PointerKind::Down));
    let Intent::CreatePane(spec) = &submissions[0].intent else {
        panic!("create-pane")
    };
    let input = spec.initial_input.clone().unwrap_or_default();
    assert!(input.contains("TODO <b>{{x}} $HOME \"q\""), "{input}");
}

/// REQ: F-023, F-114
#[test]
fn f_114_indented_todo_line_still_counts() {
    let mut host = todo_host("    TODO indented\n");
    assert_eq!(host.deliver_one(pointer(1, 0, PointerKind::Down)).len(), 1);
}

/// REQ: F-023
#[test]
fn f_023_click_on_a_non_todo_line_submits_nothing() {
    let mut host = todo_host("fn a() {}\nTODO write docs\n");
    assert!(host
        .deliver_one(pointer(1, 0, PointerKind::Down))
        .is_empty());
}

/// REQ: F-023
#[test]
fn f_023_todo_in_the_middle_of_a_line_does_not_count() {
    let mut host = todo_host("let x = 1; // TODO later\n");
    assert!(host
        .deliver_one(pointer(1, 0, PointerKind::Down))
        .is_empty());
}

/// REQ: F-023
#[test]
fn f_023_pointer_in_an_unknown_buffer_submits_nothing() {
    let mut host = todo_host("TODO write docs\n");
    assert!(host
        .deliver_one(pointer(9, 0, PointerKind::Down))
        .is_empty());
}

/// REQ: F-023
#[test]
fn f_023_pointer_beyond_the_last_line_submits_nothing() {
    let mut host = todo_host("TODO write docs\n");
    assert!(host
        .deliver_one(pointer(1, 40, PointerKind::Down))
        .is_empty());
}

/// REQ: F-023
#[test]
fn f_023_dedup_key_is_per_buffer_and_line() {
    let mut host = todo_host("TODO a\nTODO b\n");
    let first = host.deliver_one(pointer(1, 0, PointerKind::Down));
    let second = host.deliver_one(pointer(1, 1, PointerKind::Down));
    assert_ne!(first[0].dedup_key, second[0].dedup_key);
    let again = host.deliver_one(pointer(1, 0, PointerKind::Down));
    assert_eq!(first[0].dedup_key, again[0].dedup_key);
}

/// REQ: F-114
#[test]
fn f_114_create_pane_does_not_name_a_parent() {
    let mut host = todo_host("TODO a\n");
    let submissions = host.deliver_one(pointer(1, 0, PointerKind::Down));
    let Intent::CreatePane(spec) = &submissions[0].intent else {
        panic!("create-pane")
    };
    assert!(spec.parent.is_none());
}

/// REQ: F-019
#[test]
fn f_019_editor_opened_and_closed_submit_nothing() {
    let mut host = active_host(&[]);
    assert!(host
        .deliver_one(Event::EditorOpened(buffer_ref(1, None, Some("rust"), 1)))
        .is_empty());
    assert!(host
        .deliver_one(Event::EditorClosed(buffer_ref(1, None, Some("rust"), 1)))
        .is_empty());
}

/// REQ: F-019
#[test]
fn f_019_editor_changed_is_accepted_without_a_submission() {
    let mut host = todo_host("fn a() {}\n");
    let changed = Event::EditorChanged(EditorChangedEvent {
        buffer: BufferId(1),
        first_line: 0,
        last_line: 0,
        revision: 2,
    });
    assert!(host.deliver_one(changed).is_empty());
}

/// REQ: F-019
#[test]
fn f_019_activation_with_garbage_settings_does_not_fail_the_batch() {
    let mut host = active_host(&[("autosave_delay_ms", json!("not a number"))]);
    assert!(host.deliver(Vec::new()).is_empty());
}
