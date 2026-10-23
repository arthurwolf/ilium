//! Twins of ilium-client/src/terminal_links.rs tests. The original resolves a link at a cell
//! column; the plugin exposes the `path-line` detector regex (manifest) and the `link-activated`
//! handler (editor-open, project scope, split, zero-based line).

use ilium_plugin_core_terminal::TerminalClient;
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{
    EditorOpenSpec, EditorPlacement, Event, FsPath, Intent, LinkActivatedEvent, NodeId,
};
use regex::Regex;

fn detector() -> Regex {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/manifest.json"))
        .expect("manifest readable");
    let json: serde_json::Value = serde_json::from_str(&text).expect("manifest json");
    let source = json["contributes"]["link_detectors"][0]["regex"]
        .as_str()
        .expect("regex")
        .to_string();
    Regex::new(&source).expect("regex compiles")
}

fn found(text: &str) -> Option<String> {
    detector().find(text).map(|m| m.as_str().to_string())
}

fn open(text: &str) -> Vec<Intent> {
    let mut host = FakeHost::for_manifest(concat!(env!("CARGO_MANIFEST_DIR"), "/manifest.json"))
        .expect("manifest loads")
        .side("client")
        .build();
    host.activate::<TerminalClient>();
    let event = Event::LinkActivated(LinkActivatedEvent {
        node: NodeId(1),
        detector_id: "path-line".to_string(),
        text: text.to_string(),
    });
    host.deliver(vec![event])
        .into_iter()
        .map(|s| s.intent)
        .collect()
}

fn expect_open(path: &str, line: u32) -> Vec<Intent> {
    vec![Intent::EditorOpen(EditorOpenSpec {
        path: FsPath {
            scope: "project".to_string(),
            path: path.to_string(),
        },
        line: Some(line),
        placement: EditorPlacement::Split,
    })]
}

/// REQ: PORT:ilium-client/src/terminal_links.rs::recognizes_url_and_file_line
#[test]
fn port_links_recognizes_url_and_file_line() {
    assert_eq!(
        found("src/main.rs:42:7"),
        Some("src/main.rs:42".to_string())
    );
    assert_eq!(open("src/main.rs:42"), expect_open("src/main.rs", 41));
    // Bare URLs carry no :line and are outside the path-line detector.
    assert_eq!(found("see https://example.test/a."), None);
}

/// REQ: PORT:ilium-client/src/terminal_links.rs::keeps_relative_references_relative_instead_of_becoming_absolute
#[test]
fn port_links_keeps_relative_references_relative() {
    assert_eq!(
        found("see ../src/main.rs:3 for details"),
        Some("../src/main.rs:3".to_string())
    );
    assert_eq!(open("../src/main.rs:3"), expect_open("../src/main.rs", 2));
}

/// REQ: PORT:ilium-client/src/terminal_links.rs::expands_home_relative_references_against_the_injected_home_dir
#[test]
fn port_links_home_relative_reference_keeps_its_tilde_path() {
    assert_eq!(open("~/.bashrc:3"), expect_open("~/.bashrc", 2));
}

/// REQ: PORT:ilium-client/src/terminal_links.rs::keeps_a_non_numeric_middle_segment_attached_to_the_path
#[test]
fn port_links_non_numeric_middle_segment_stays_in_the_path() {
    assert_eq!(open("a/b:c.txt:42"), expect_open("a/b:c.txt", 41));
}

/// REQ: PORT:ilium-client/src/terminal_links.rs::strips_markdown_backticks_around_a_fenced_path
#[test]
fn port_links_backticks_are_not_part_of_the_match() {
    assert_eq!(
        found("see `docs/guide.md:5` for details"),
        Some("docs/guide.md:5".to_string())
    );
    assert_eq!(open("docs/guide.md:5"), expect_open("docs/guide.md", 4));
}

/// REQ: PORT:ilium-client/src/terminal_links.rs::strips_a_matching_opening_bracket_from_a_wrapped_url
#[test]
fn port_links_wrapped_url_is_not_a_path_line_match() {
    assert_eq!(found("see (https://example.test/docs) for details"), None);
    assert_eq!(
        found("see (src/lib.rs:9) for details"),
        Some("src/lib.rs:9".to_string())
    );
}

/// REQ: PORT:ilium-client/src/terminal_links.rs::recognizes_a_bare_project_local_file_with_trailing_prose_punctuation
#[test]
fn port_links_bare_file_with_prose_punctuation_has_no_line_so_no_match() {
    assert_eq!(found("Read STYLE.md, before continuing"), None);
    assert_eq!(
        found("Read STYLE.md:12, before continuing"),
        Some("STYLE.md:12".to_string())
    );
}

/// REQ: PORT:ilium-client/src/terminal_links.rs::accounts_for_multi_byte_glyphs_when_mapping_a_cell_column
#[test]
fn port_links_multi_byte_glyph_before_the_path_is_not_matched() {
    assert_eq!(
        found("→ src/main.rs:42"),
        Some("src/main.rs:42".to_string())
    );
    assert_eq!(open("src/main.rs:42"), expect_open("src/main.rs", 41));
}

/// REQ: PORT:ilium-client/src/keys.rs::normal_tree_paste_uses_the_retained_bounded_replay
#[test]
fn port_normal_tree_paste_uses_the_retained_bounded_replay() {
    let mut host = FakeHost::for_manifest(concat!(env!("CARGO_MANIFEST_DIR"), "/manifest.json"))
        .expect("manifest loads")
        .side("client")
        .build();
    host.activate::<TerminalClient>();
    // Assumed command `terminal.paste` with the pasted text in its arguments: a paste into the focused pane is
    // one bounded `send-input` with pasted delivery, never one input per key.
    let text = "line one\nline two ".repeat(200);
    let submissions = host.deliver_one(Event::CommandInvoked(
        ilium_plugin_sdk::types::CommandInvokedEvent {
            command: "terminal.paste".to_string(),
            arguments_json: serde_json::json!({ "text": text }).to_string(),
            origin: ilium_plugin_sdk::types::CommandOrigin::Palette,
            context: ilium_plugin_sdk::types::CommandContext {
                pane: Some(NodeId(1)),
                ..Default::default()
            },
        },
    ));
    let sends: Vec<_> = submissions
        .iter()
        .filter_map(|s| match &s.intent {
            Intent::SendInput(spec) => Some(spec.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(sends.len(), 1);
    assert_eq!(sends[0].text, text);
    assert_eq!(sends[0].delivery, ilium_plugin_sdk::types::Delivery::Pasted);
}
