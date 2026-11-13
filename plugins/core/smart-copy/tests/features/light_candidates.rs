//! F-065: the server side recognises paths, commands and hashes from the visible cells and sends
//! the candidates to the client side as a `candidates` message (page sample "Smart Copy light",
//! smart-copy.md "What Smart Copy recognises"), plus the `recognisers` collect point.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{Event, NodeId, ScreenChangedEvent, Submission};
use proptest::prelude::*;

use crate::common::{add_terminal_pane, candidate_lines, candidate_payloads, server_host};

/// A server host whose pane shows these rows; returns the host and the pane id.
fn host_showing(rows: &[&str]) -> (FakeHost, u64) {
    let mut host = server_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.terminal().set_rows(pane, rows);
    (host, pane)
}

fn screen_changed(pane: u64, first_row: u32, last_row: u32) -> Event {
    Event::ScreenChanged(ScreenChangedEvent {
        pane: NodeId(pane),
        first_row,
        last_row,
        revision: 1,
    })
}

fn scan(rows: &[&str]) -> Vec<String> {
    let (mut host, pane) = host_showing(rows);
    let last = rows.len().saturating_sub(1) as u32;
    candidate_lines(&host.deliver_one(screen_changed(pane, 0, last)))
}

fn lines_of(submissions: &[Submission]) -> Vec<String> {
    candidate_lines(submissions)
}

/// REQ: F-065
#[test]
fn feature_065_a_file_path_on_the_screen_is_a_candidate() {
    let lines = scan(&["$ git status", "  modified: src/lib.rs"]);
    assert!(lines.contains(&"src/lib.rs".to_string()), "{lines:?}");
}

/// REQ: F-065
#[test]
fn feature_065_a_seven_character_hash_is_a_candidate() {
    let lines = scan(&["commit 9f3c2ab"]);
    assert!(lines.contains(&"9f3c2ab".to_string()), "{lines:?}");
}

/// REQ: F-065
#[test]
fn feature_065_a_full_forty_character_git_sha_is_a_candidate() {
    let sha = "9f3c2ab9f3c2ab9f3c2ab9f3c2ab9f3c2ab9f3c2";
    assert_eq!(sha.len(), 40);
    let lines = scan(&[&format!("commit {sha}")]);
    assert!(lines.contains(&sha.to_string()), "{lines:?}");
}

/// REQ: F-065
#[test]
fn feature_065_a_six_character_hex_word_is_not_a_hash_candidate() {
    // Boundary n-1 of the page sample rule "word.len() >= 7".
    let lines = scan(&["see 9f3c2a here"]);
    assert!(!lines.contains(&"9f3c2a".to_string()), "{lines:?}");
}

/// REQ: F-065
#[test]
fn feature_065_a_seven_character_word_with_a_non_hex_letter_is_not_a_hash_candidate() {
    let lines = scan(&["see 9f3c2az here"]);
    assert!(!lines.contains(&"9f3c2az".to_string()), "{lines:?}");
}

/// REQ: F-065
#[test]
fn feature_065_a_command_in_an_inline_code_span_is_a_candidate_without_the_backticks() {
    let lines = scan(&["run `cargo test -p ilium-server` now"]);
    assert!(
        lines.contains(&"cargo test -p ilium-server".to_string()),
        "{lines:?}"
    );
}

/// REQ: F-065
#[test]
fn feature_065_the_page_figure_example_offers_path_command_and_hash() {
    let lines = scan(&[
        "src/server/routes.rs",
        "`cargo test -p ilium-server`",
        "9f3c2ab",
    ]);
    for expected in [
        "src/server/routes.rs",
        "cargo test -p ilium-server",
        "9f3c2ab",
    ] {
        assert!(
            lines.contains(&expected.to_string()),
            "{expected} missing from {lines:?}"
        );
    }
}

/// REQ: F-065
#[test]
fn feature_065_candidates_keep_reading_order_top_to_bottom() {
    let lines = scan(&["first src/a.rs", "then 9f3c2ab", "last src/z.rs"]);
    let position = |text: &str| {
        lines
            .iter()
            .position(|line| line == text)
            .unwrap_or_else(|| panic!("{text} missing"))
    };
    assert!(position("src/a.rs") < position("9f3c2ab"));
    assert!(position("9f3c2ab") < position("src/z.rs"));
}

/// REQ: F-065
#[test]
fn feature_065_candidates_keep_reading_order_left_to_right() {
    let lines = scan(&["src/left.rs and src/right.rs"]);
    let left = lines
        .iter()
        .position(|line| line == "src/left.rs")
        .expect("left");
    let right = lines
        .iter()
        .position(|line| line == "src/right.rs")
        .expect("right");
    assert!(left < right);
}

/// REQ: F-065
#[test]
fn feature_065_a_path_with_non_ascii_letters_stays_intact() {
    let lines = scan(&["open src/héllo.rs please"]);
    assert!(lines.contains(&"src/héllo.rs".to_string()), "{lines:?}");
}

/// REQ: F-065
#[test]
fn feature_065_only_the_changed_rows_are_scanned() {
    let (mut host, pane) = host_showing(&["old src/a.rs", "mid 9f3c2ab", "new src/c.rs"]);
    let lines = lines_of(&host.deliver_one(screen_changed(pane, 2, 2)));
    assert!(lines.contains(&"src/c.rs".to_string()), "{lines:?}");
    assert!(!lines.contains(&"src/a.rs".to_string()), "{lines:?}");
    assert!(!lines.contains(&"9f3c2ab".to_string()), "{lines:?}");
}

/// REQ: F-065
#[test]
fn feature_065_an_empty_screen_offers_no_candidates() {
    let lines = scan(&["", ""]);
    assert!(lines.is_empty(), "{lines:?}");
}

/// REQ: F-065
#[test]
fn feature_065_the_message_goes_to_the_client_side_of_this_plugin_on_channel_candidates() {
    let (mut host, pane) = host_showing(&["src/lib.rs"]);
    let submissions = host.deliver_one(screen_changed(pane, 0, 0));
    assert_eq!(
        candidate_payloads(&submissions),
        vec!["src/lib.rs".to_string()]
    );
}

/// REQ: F-065
#[test]
fn feature_065_candidate_payload_is_newline_joined_utf8_without_a_trailing_newline() {
    let (mut host, pane) = host_showing(&["a src/a.rs", "b src/b.rs"]);
    let submissions = host.deliver_one(screen_changed(pane, 0, 1));
    let payload = candidate_payloads(&submissions).remove(0);
    assert!(!payload.ends_with('\n'));
    assert_eq!(payload.lines().count(), payload.split('\n').count());
}

/// REQ: F-065
#[test]
fn feature_065_light_never_calls_the_inference_provider() {
    // smart-copy.md "Rules": light mode never calls an inference provider.
    let (mut host, pane) = host_showing(&["src/lib.rs"]);
    host.deliver_one(screen_changed(pane, 0, 0));
    host.settle();
    assert!(host.inference().requests().is_empty());
}

/// REQ: F-065
#[test]
fn feature_065_built_in_candidates_are_capped_at_1024() {
    // smart-copy.md "What the model sees and returns": at most 1024 candidates come from the built-in detectors.
    let rows: Vec<String> = (0..20)
        .map(|row| {
            (0..100)
                .map(|column| format!("d{row}x{column}/f.rs"))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect();
    let borrowed: Vec<&str> = rows.iter().map(String::as_str).collect();
    let lines = scan(&borrowed);
    assert!(lines.len() <= 1024, "{} candidates", lines.len());
    assert!(
        lines.len() >= 1000,
        "the cap must not drop far below the limit: {}",
        lines.len()
    );
}

/// REQ: F-065
#[test]
fn feature_065_a_screen_change_for_a_pane_that_is_gone_fails_nothing() {
    let mut host = server_host();
    let submissions = host.deliver_one(screen_changed(4242, 0, 0));
    assert!(candidate_lines(&submissions).is_empty());
    assert!(host.failures().is_empty());
}

/// REQ: F-065
#[test]
fn feature_065_a_repeated_path_is_offered_once_or_twice_but_never_dropped() {
    // The manual does not say whether duplicates collapse; both outcomes are allowed (MAY).
    let lines = scan(&["src/lib.rs", "src/lib.rs"]);
    let count = lines
        .iter()
        .filter(|line| line.as_str() == "src/lib.rs")
        .count();
    assert!((1..=2).contains(&count), "{lines:?}");
}

/// REQ: F-065
#[test]
fn feature_065_a_collect_contribution_to_recognisers_adds_matches() {
    let (mut host, pane) = host_showing(&["fix ABC-123 today"]);
    host.contribute_entry(
        "ilium.core.smart-copy/recognisers",
        "jira-key",
        r#"{"id":"jira-key","kind":"ticket","regex":"[A-Z]+-[0-9]+","multiline":false,"label":"Jira key"}"#,
    );
    let lines = lines_of(&host.deliver_one(screen_changed(pane, 0, 0)));
    assert!(lines.contains(&"ABC-123".to_string()), "{lines:?}");
}

/// REQ: F-065
#[test]
fn feature_065_every_match_of_a_recogniser_is_offered_in_reading_order() {
    let (mut host, pane) = host_showing(&["ABC-1 then ABC-22", "and DEF-3"]);
    host.contribute_entry(
        "ilium.core.smart-copy/recognisers",
        "jira-key",
        r#"{"id":"jira-key","kind":"ticket","regex":"[A-Z]+-[0-9]+","multiline":false,"label":"Jira key"}"#,
    );
    let lines = lines_of(&host.deliver_one(screen_changed(pane, 0, 1)));
    let tickets: Vec<&String> = lines
        .iter()
        .filter(|line| line.contains('-') && !line.contains('/'))
        .collect();
    assert_eq!(tickets, ["ABC-1", "ABC-22", "DEF-3"]);
}

/// REQ: F-065
#[test]
fn feature_065_a_recogniser_with_an_invalid_regex_is_ignored_and_the_rest_still_work() {
    let (mut host, pane) = host_showing(&["src/lib.rs"]);
    host.contribute_entry(
        "ilium.core.smart-copy/recognisers",
        "broken",
        r#"{"id":"broken","kind":"ticket","regex":"([unclosed","multiline":false,"label":"Broken"}"#,
    );
    let lines = lines_of(&host.deliver_one(screen_changed(pane, 0, 0)));
    assert!(lines.contains(&"src/lib.rs".to_string()), "{lines:?}");
    assert!(host.failures().is_empty());
}

/// REQ: F-065
#[test]
fn feature_065_two_contributors_with_the_same_entry_id_both_contribute() {
    // EXT-049: equal ids from two plugins both stay.
    let (mut host, pane) = host_showing(&["AAA-1 and bbb#2"]);
    host.contribute_entry(
        "ilium.core.smart-copy/recognisers",
        "ocean",
        r#"{"id":"ocean","kind":"a","regex":"[A-Z]{3}-[0-9]","multiline":false,"label":"A"}"#,
    );
    host.contribute_entry(
        "ilium.core.smart-copy/recognisers",
        "ocean",
        r#"{"id":"ocean","kind":"b","regex":"[a-z]{3}#[0-9]","multiline":false,"label":"B"}"#,
    );
    let lines = lines_of(&host.deliver_one(screen_changed(pane, 0, 0)));
    assert!(lines.contains(&"AAA-1".to_string()), "{lines:?}");
    assert!(lines.contains(&"bbb#2".to_string()), "{lines:?}");
}

proptest! {
    // REQ: F-065
    #[test]
    fn feature_065_any_hex_word_of_seven_to_forty_characters_with_a_letter_is_a_hash_candidate(
        hash in "[0-9a-f]{6}[a-f][0-9a-f]{0,33}"
    ) {
        let row = format!("commit {hash}");
        let lines = scan(&[row.as_str()]);
        prop_assert!(lines.contains(&hash), "{:?}", lines);
    }

    // REQ: F-065
    #[test]
    fn feature_065_any_directory_slash_name_dot_extension_word_is_a_path_candidate(
        path in "[a-z]{1,8}/[a-z]{1,8}\\.[a-z]{1,3}"
    ) {
        let row = format!("edited {path} today");
        let lines = scan(&[row.as_str()]);
        prop_assert!(lines.contains(&path), "{:?}", lines);
    }
}
