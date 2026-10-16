//! Markdown source and rendered views (F-021, src/docs/editors-and-boards.md "Markdown: Source and
//! Rendered views", core-plugin-editor.md "Markdown rendered view").

use crate::support::{
    active_host, buffer_ref, direct, direct_open, nodes_where, open_file, project_path, render_pane,
    seed_text, NOTES_MD,
};
use ilium_plugin_sdk::generated::types::{
    BufferId, Direction, Event, EditorPointerEvent, Intent, NodeId, PointerButton, PointerKind,
    PointerModifiers, TextPosition, ViewPointer, ViewTarget, Widget,
};
use ilium_plugin_sdk::{Handler, Views};
use serde_json::json;

const OVER_LIMITS: &str = "document exceeds preparation limits; showing source";

fn rendered_host(text: &str) -> ilium_plugin_sdk::testing::host::FakeHost {
    let mut host = active_host(&[("markdown_rendered_by_default", json!(true)), ("show_minimap", json!(false))]);
    open_file(&mut host, 1, "notes.md", Some("markdown"), text);
    host
}

fn click(line: u32, column: u32) -> Event {
    Event::EditorPointer(EditorPointerEvent {
        buffer: BufferId(1),
        pane: NodeId(7),
        position: TextPosition { line, column },
        pointer: ViewPointer {
            kind: PointerKind::Down,
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

fn markdown_host(text: &str) -> ilium_plugin_sdk::testing::host::FakeHost {
    let mut host = active_host(&[("show_minimap", json!(false))]);
    open_file(&mut host, 1, "notes.md", Some("markdown"), text);
    host
}

/// REQ: F-021
#[test]
fn f_021_source_view_shows_the_raw_markdown() {
    let mut host = markdown_host(NOTES_MD);
    let text = render_pane(&mut host, 60, 8).text();
    assert!(text.contains("# Plan"), "{text}");
    assert!(text.contains("- [ ] ship it"), "{text}");
}

/// REQ: F-021
#[test]
fn f_021_rendered_view_shows_styled_text_without_markup() {
    let mut host = rendered_host(NOTES_MD);
    let text = render_pane(&mut host, 60, 8).text();
    assert!(text.contains("Plan"), "{text}");
    assert!(!text.contains("# Plan"), "{text}");
    assert!(text.contains("[ ] ship it"), "{text}");
    assert!(text.contains("TODO write docs"), "{text}");
}

/// REQ: F-021
#[test]
fn f_021_rendered_view_is_the_default_only_when_the_setting_is_on() {
    let mut host = active_host(&[("markdown_rendered_by_default", json!(false)), ("show_minimap", json!(false))]);
    open_file(&mut host, 1, "notes.md", Some("markdown"), NOTES_MD);
    assert!(render_pane(&mut host, 60, 8).text().contains("# Plan"));
}

/// REQ: F-021
#[test]
fn f_021_toolbar_offers_source_and_rendered_for_md_files() {
    let mut host = markdown_host(NOTES_MD);
    let text = render_pane(&mut host, 80, 8).text();
    assert!(text.contains("Source"), "{text}");
    assert!(text.contains("Rendered"), "{text}");
}

/// REQ: F-021
#[test]
fn f_021_toolbar_offers_source_and_rendered_for_markdown_extension() {
    let mut host = active_host(&[("show_minimap", json!(false))]);
    open_file(&mut host, 1, "notes.markdown", Some("markdown"), NOTES_MD);
    let text = render_pane(&mut host, 80, 8).text();
    assert!(text.contains("Source") && text.contains("Rendered"), "{text}");
}

/// REQ: F-021
#[test]
fn f_021_toolbar_hides_the_view_switch_for_other_files() {
    let mut host = active_host(&[("show_minimap", json!(false))]);
    open_file(&mut host, 1, "main.rs", Some("rust"), "fn main() {}\n");
    let text = render_pane(&mut host, 80, 8).text();
    assert!(!text.contains("Rendered"), "{text}");
}

/// REQ: F-021
#[test]
fn f_021_markdown_split_has_source_beside_the_rendered_view() {
    let mut editor = direct("{}");
    direct_open(&mut editor, 1, Some("markdown"), 3);
    let tree = Views::render(&mut editor, ViewTarget::Panel("markdown".to_string()), ilium_plugin_sdk::generated::types::Area { columns: 80, rows: 12 });
    let root = &tree.nodes[tree.root as usize];
    assert_eq!(root.key.as_deref(), Some("markdown-split"));
    let Widget::Split(split) = &root.widget else { panic!("the root is a split") };
    assert_eq!(split.direction, Direction::Vertical);
    assert_eq!(split.position, 500);
    assert_eq!(root.children.len(), 2);
    assert_eq!(tree.nodes[root.children[0] as usize].key.as_deref(), Some("source"));
    assert_eq!(tree.nodes[root.children[1] as usize].key.as_deref(), Some("rendered"));
    assert!(matches!(tree.nodes[root.children[0] as usize].widget, Widget::CodeView(_)));
    assert!(matches!(tree.nodes[root.children[1] as usize].widget, Widget::Markdown(_)));
}

/// REQ: F-021
#[test]
fn f_021_code_view_shows_the_same_buffer_as_the_markdown_widget() {
    let mut editor = direct("{}");
    direct_open(&mut editor, 4, Some("markdown"), 3);
    seed_text(&mut editor, 4, NOTES_MD);
    let tree = Views::render(&mut editor, ViewTarget::Panel("markdown".to_string()), ilium_plugin_sdk::generated::types::Area { columns: 80, rows: 12 });
    let code = nodes_where(&tree, |widget| matches!(widget, Widget::CodeView(_)));
    let markdown = nodes_where(&tree, |widget| matches!(widget, Widget::Markdown(_)));
    assert_eq!(code.len(), 1);
    assert_eq!(markdown.len(), 1);
    if let Widget::CodeView(spec) = &code[0].widget {
        assert_eq!(spec.buffer, BufferId(4));
    }
    if let Widget::Markdown(spec) = &markdown[0].widget {
        assert_eq!(spec.source, NOTES_MD);
    }
}

/// REQ: F-021
#[test]
fn f_021_markdown_widget_follows_edits_of_the_buffer() {
    let mut editor = direct("{}");
    direct_open(&mut editor, 1, Some("markdown"), 1);
    seed_text(&mut editor, 1, "# One");
    crate::support::call_buffers(
        &mut editor,
        "replace-range",
        json!({"buffer": 1, "base-revision": 2, "edits": [{
            "range": {"start": {"line": 0, "column": 2}, "end": {"line": 0, "column": 5}},
            "replacement": "Two",
        }]}),
    );
    let tree = Views::render(&mut editor, ViewTarget::Panel("markdown".to_string()), ilium_plugin_sdk::generated::types::Area { columns: 80, rows: 12 });
    let markdown = nodes_where(&tree, |widget| matches!(widget, Widget::Markdown(_)));
    if let Widget::Markdown(spec) = &markdown[0].widget {
        assert_eq!(spec.source, "# Two");
    } else {
        panic!("a markdown widget");
    }
}

/// REQ: F-021
#[test]
fn f_021_images_resolve_relative_to_the_markdown_file() {
    let mut editor = direct("{}");
    Handler::on_events(&mut editor, vec![Event::EditorOpened(buffer_ref(1, Some("docs/plan.md"), Some("markdown"), 1))]);
    let tree = Views::render(&mut editor, ViewTarget::Panel("markdown".to_string()), ilium_plugin_sdk::generated::types::Area { columns: 80, rows: 12 });
    let markdown = nodes_where(&tree, |widget| matches!(widget, Widget::Markdown(_)));
    if let Widget::Markdown(spec) = &markdown[0].widget {
        assert_eq!(spec.base, Some(project_path("docs")));
    } else {
        panic!("a markdown widget");
    }
}

/// REQ: F-021
#[test]
fn f_021_clicking_an_open_task_checkbox_checks_it() {
    let mut host = markdown_host(NOTES_MD);
    let submissions = host.deliver_one(click(1, 3));
    assert_eq!(submissions.len(), 1);
    let Intent::EditorApplyEdits(spec) = &submissions[0].intent else { panic!("editor-apply-edits") };
    assert_eq!(spec.buffer, BufferId(1));
    assert_eq!(spec.base_revision, 1);
    assert_eq!(spec.edits.len(), 1);
    assert_eq!(spec.edits[0].range.start, TextPosition { line: 1, column: 3 });
    assert_eq!(spec.edits[0].range.end, TextPosition { line: 1, column: 4 });
    assert_eq!(spec.edits[0].replacement, "x");
}

/// REQ: F-021
#[test]
fn f_021_clicking_a_checked_task_checkbox_unchecks_it() {
    let mut host = markdown_host("- [x] done\n");
    let submissions = host.deliver_one(click(0, 3));
    assert_eq!(submissions.len(), 1);
    let Intent::EditorApplyEdits(spec) = &submissions[0].intent else { panic!("editor-apply-edits") };
    assert_eq!(spec.edits[0].replacement, " ");
}

/// REQ: F-021
#[test]
fn f_021_clicking_outside_the_checkbox_edits_nothing() {
    let mut host = markdown_host(NOTES_MD);
    assert!(host.deliver_one(click(1, 8)).is_empty());
    assert!(host.deliver_one(click(0, 3)).is_empty());
}

/// REQ: F-021
#[test]
fn f_021_rendered_view_within_the_line_limit_shows_the_document() {
    let text = "a\n".repeat(32_768);
    let mut host = rendered_host(&text);
    assert!(!render_pane(&mut host, 40, 6).text().contains(OVER_LIMITS));
}

/// REQ: F-021
#[test]
fn f_021_rendered_view_over_the_line_limit_falls_back_to_source() {
    let text = "a\n".repeat(32_769);
    let mut host = rendered_host(&text);
    assert!(render_pane(&mut host, 80, 6).text().contains(OVER_LIMITS));
}

/// REQ: F-021
#[test]
fn f_021_rendered_view_with_a_64_kib_line_shows_the_document() {
    let text = format!("{}\n", "a".repeat(65_536));
    let mut host = rendered_host(&text);
    assert!(!render_pane(&mut host, 80, 6).text().contains(OVER_LIMITS));
}

/// REQ: F-021
#[test]
fn f_021_rendered_view_with_a_line_over_64_kib_falls_back_to_source() {
    let text = format!("{}\n", "a".repeat(65_537));
    let mut host = rendered_host(&text);
    assert!(render_pane(&mut host, 80, 6).text().contains(OVER_LIMITS));
}

/// REQ: F-021
#[test]
fn f_021_rendered_view_at_2_mib_shows_the_document() {
    let text = format!("{}\n", "a".repeat(1023)).repeat(2048);
    assert_eq!(text.len(), 2 * 1024 * 1024);
    let mut host = rendered_host(&text);
    assert!(!render_pane(&mut host, 80, 6).text().contains(OVER_LIMITS));
}

/// REQ: F-021
#[test]
fn f_021_rendered_view_over_2_mib_falls_back_to_source() {
    let text = format!("{}\n", "a".repeat(1023)).repeat(2048) + "b\n";
    let mut host = rendered_host(&text);
    assert!(render_pane(&mut host, 80, 6).text().contains(OVER_LIMITS));
}
