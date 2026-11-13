//! Twins of the pure parts of ilium-client/src/search_ui.rs, run against the plugin's
//! `matching` module (terminal control stripping, case-insensitive matching, context slicing).

use ilium_plugin_core_search::matching::{
    context_parts, find_terminal_results, find_text_results, last_command_from_text,
    result_from_source, search_workspace, strip_terminal_controls, strip_terminal_controls_bounded,
    truncate, Content, Location, ObjectKind, Request, SearchResult, Source, TextFragment,
    CONTEXT_CHARS, MAX_RESULTS,
};

fn result(
    kind: ObjectKind,
    pane_id: u64,
    before: &str,
    matched: &str,
    after: &str,
    location: Location,
) -> SearchResult {
    SearchResult {
        pane_id,
        kind,
        object_name: "name".to_string(),
        automatic_title: None,
        path: None,
        last_command: None,
        before: before.to_string(),
        matched: matched.to_string(),
        after: after.to_string(),
        location,
    }
}

fn source(kind: ObjectKind, content: Content) -> Source {
    Source {
        pane_id: 4,
        kind,
        object_name: "note.md".to_string(),
        automatic_title: None,
        path: None,
        last_command: None,
        content,
    }
}

/// REQ: PORT:ilium-client/src/search_ui.rs::terminal_search_ignores_ansi_and_preserves_a_raw_jump_offset
#[test]
fn port_terminal_search_ignores_ansi_and_preserves_a_raw_jump_offset() {
    let mut results = Vec::new();
    let history = b"before \x1b[31mneedle\x1b[0m after";
    let searchable = strip_terminal_controls(history);
    find_terminal_results(
        &searchable,
        history.len(),
        "NEEDLE",
        |before, matched, after, history_end_byte| {
            result(
                ObjectKind::Shell,
                1,
                before,
                matched,
                after,
                Location::Terminal { history_end_byte },
            )
        },
        &mut results,
        &|| false,
    );
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].matched, "needle");
    // Byte 17 is the closing `e` of "needle"; the exclusive raw end is 18, not the fallback 28.
    assert_eq!(
        results[0].location,
        Location::Terminal {
            history_end_byte: 18
        }
    );
}

/// REQ: PORT:ilium-client/src/search_ui.rs::stripping_collapses_a_crlf_pair_into_a_single_newline
#[test]
fn port_stripping_collapses_a_crlf_pair_into_a_single_newline() {
    let stripped = strip_terminal_controls(b"a\r\nb");
    assert_eq!(stripped.text, "a\nb");
    assert_eq!(stripped.raw_ends.len(), stripped.text.len());
}

/// REQ: PORT:ilium-client/src/search_ui.rs::stripping_keeps_text_that_precedes_an_invalid_utf8_byte
#[test]
fn port_stripping_keeps_text_that_precedes_an_invalid_utf8_byte() {
    let stripped = strip_terminal_controls(b"needle \xff tail");
    assert!(stripped.text.contains("needle"));
    assert!(stripped.text.contains("tail"));
    assert_eq!(stripped.raw_ends.len(), stripped.text.len());
}

/// REQ: PORT:ilium-client/src/search_ui.rs::local_search_is_case_insensitive_and_keeps_the_line_target
#[test]
fn port_local_search_is_case_insensitive_and_keeps_the_line_target() {
    let mut results = Vec::new();
    find_text_results(
        "A Needle appears",
        "needle",
        |before, matched, after, line| {
            result(
                ObjectKind::File,
                2,
                before,
                matched,
                after,
                Location::Editor { line },
            )
        },
        4,
        &mut results,
    );
    assert_eq!(results[0].matched, "Needle");
    assert_eq!(results[0].location, Location::Editor { line: 4 });
}

/// REQ: PORT:ilium-client/src/search_ui.rs::result_from_source_collapses_embedded_newlines_in_context_text
#[test]
fn port_result_from_source_collapses_embedded_newlines_in_context_text() {
    let note = source(ObjectKind::File, Content::Text(Vec::new()));
    let found = result_from_source(
        &note,
        "first line\nsecond ",
        "needle",
        " third\nline",
        Location::Editor { line: 0 },
        None,
    );
    assert_eq!(found.before, "first line second ");
    assert_eq!(found.after, " third line");
    assert!(!found.before.contains('\n'));
    assert!(!found.after.contains('\n'));
}

/// REQ: PORT:ilium-client/src/search_ui.rs::cancelled_terminal_preparation_does_not_publish_partial_matches
#[test]
fn port_cancelled_terminal_preparation_does_not_publish_partial_matches() {
    let history = vec![b'x'; 1024 * 1024];
    let checks = std::cell::Cell::new(0);
    let stripped = strip_terminal_controls_bounded(&history, &|| {
        checks.set(checks.get() + 1);
        checks.get() > 2
    });
    assert!(stripped.is_none());
    assert_eq!(checks.get(), 3);
}

#[test]
fn a_cancelled_scan_over_a_terminal_source_reports_nothing() {
    let request = Request {
        revision: 1,
        query: "x".to_string(),
        sources: vec![source(
            ObjectKind::Shell,
            Content::Terminal(vec![b'x'; 10_000]),
        )],
    };
    let calls = std::cell::Cell::new(0);
    let found = search_workspace(&request, || {
        calls.set(calls.get() + 1);
        calls.get() > 1
    });
    assert!(found.is_empty());
}

#[test]
fn an_empty_query_matches_nothing() {
    let mut results = Vec::new();
    find_text_results(
        "abc",
        "",
        |b, m, a, l| result(ObjectKind::File, 1, b, m, a, Location::Editor { line: l }),
        0,
        &mut results,
    );
    assert!(results.is_empty());
}

#[test]
fn results_stop_at_the_cap_and_one_past_it_is_never_reported() {
    let text = "a ".repeat(MAX_RESULTS + 5);
    let mut results = Vec::new();
    find_text_results(
        &text,
        "a",
        |b, m, a, l| result(ObjectKind::File, 1, b, m, a, Location::Editor { line: l }),
        0,
        &mut results,
    );
    assert_eq!(results.len(), MAX_RESULTS);
}

#[test]
fn a_scan_over_text_and_terminal_sources_reports_each_with_its_own_location() {
    let request = Request {
        revision: 3,
        query: "needle".to_string(),
        sources: vec![
            Source {
                last_command: Some("fallback".to_string()),
                ..source(
                    ObjectKind::File,
                    Content::Text(vec![TextFragment {
                        text: "a NEEDLE here".to_string(),
                        location: Location::Editor { line: 7 },
                    }]),
                )
            },
            source(
                ObjectKind::Shell,
                Content::Terminal(b"$ make needle\r\nok\r\n".to_vec()),
            ),
        ],
    };
    let found = search_workspace(&request, || false);
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].location, Location::Editor { line: 7 });
    assert_eq!(found[0].last_command.as_deref(), Some("fallback"));
    assert_eq!(found[1].last_command.as_deref(), Some("make needle"));
    assert!(matches!(found[1].location, Location::Terminal { .. }));
}

#[test]
fn context_is_cut_to_seventy_two_characters_each_side_on_character_boundaries() {
    let text = format!("{}needle{}", "é".repeat(100), "ü".repeat(100));
    let start = text.find("needle").expect("needle present");
    let (before, matched, after) = context_parts(&text, start, start + 6);
    assert_eq!(before.chars().count(), CONTEXT_CHARS);
    assert_eq!(matched, "needle");
    assert_eq!(after.chars().count(), CONTEXT_CHARS);
}

#[test]
fn the_last_command_is_the_newest_prompt_line_without_its_prompt() {
    assert_eq!(
        last_command_from_text("$ one\n❯ two\nnoise"),
        Some("two".to_string())
    );
    assert_eq!(last_command_from_text("no prompt here"), None);
    assert_eq!(last_command_from_text("$ \n"), None);
    let long = format!("$ {}", "c".repeat(100));
    let command = last_command_from_text(&long).expect("a command");
    assert_eq!(command.chars().count(), 73);
    assert!(command.ends_with('…'));
}

#[test]
fn truncate_appends_an_ellipsis_only_when_it_cut() {
    assert_eq!(truncate("abc", 3), "abc");
    assert_eq!(truncate("abcd", 3), "abc…");
}

#[test]
fn escape_strings_are_skipped_whole() {
    let stripped = strip_terminal_controls(b"a\x1b]0;title\x07b\x1bPsixel\x1b\\c\x1b[1;31md");
    assert_eq!(stripped.text, "abcd");
}

#[test]
fn object_kinds_round_trip_their_wire_names() {
    for kind in [
        ObjectKind::Agent,
        ObjectKind::Shell,
        ObjectKind::File,
        ObjectKind::Board,
    ] {
        assert_eq!(ObjectKind::from_wire_name(kind.wire_name()), Some(kind));
    }
    assert_eq!(ObjectKind::from_wire_name("nope"), None);
}
