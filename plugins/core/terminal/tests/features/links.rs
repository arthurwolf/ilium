//! F-015 clickable links: the `path-line` detector regex of the manifest and the page sample
//! "Clickable links" (`link-activated` becomes `editor-open` with `placement: split`).

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{
    EditorOpenSpec, EditorPlacement, Event, FsPath, Intent, LinkActivatedEvent, NodeId, PaneTitleChangedEvent, Submission, TitleOrigin,
};
use proptest::prelude::*;
use regex::Regex;

use crate::common::{client_host, manifest_json};

fn detector_regex() -> Regex {
    let manifest = manifest_json();
    let source = manifest["contributes"]["link_detectors"][0]["regex"].as_str().expect("regex string").to_string();
    Regex::new(&source).expect("detector regex compiles")
}

fn first_match(text: &str) -> Option<String> {
    detector_regex().find(text).map(|found| found.as_str().to_string())
}

fn link(node: u64, detector_id: &str, text: &str) -> Event {
    Event::LinkActivated(LinkActivatedEvent { node: NodeId(node), detector_id: detector_id.to_string(), text: text.to_string() })
}

fn open_spec(path: &str, line: Option<u32>) -> Intent {
    Intent::EditorOpen(EditorOpenSpec {
        path: FsPath { scope: "project".to_string(), path: path.to_string() },
        line,
        placement: EditorPlacement::Split,
    })
}

fn activate(host: &mut FakeHost, events: Vec<Event>) -> Vec<Submission> {
    host.deliver(events)
}

/// REQ: F-015
#[test]
fn feature_015_detector_regex_matches_the_page_example_path() {
    assert_eq!(first_match("--> src/server/routes.rs:42"), Some("src/server/routes.rs:42".to_string()));
}

/// REQ: F-015
#[test]
fn feature_015_detector_regex_matches_relative_paths_with_dots_dashes_and_underscores() {
    assert_eq!(first_match("./a/b-c_d.rs:7"), Some("./a/b-c_d.rs:7".to_string()));
    assert_eq!(first_match("../x.toml:1"), Some("../x.toml:1".to_string()));
}

/// REQ: F-015
#[test]
fn feature_015_detector_regex_needs_a_line_number() {
    assert_eq!(first_match("routes.rs"), None);
    assert_eq!(first_match("routes.rs:"), None);
    assert_eq!(first_match("routes.rs:abc"), None);
}

/// REQ: F-015
#[test]
fn feature_015_detector_regex_needs_a_lower_case_extension() {
    assert_eq!(first_match("MAIN.RS:3"), None);
    assert_eq!(first_match("Makefile:12"), None);
    assert_eq!(first_match("main.rs1:3"), None);
}

/// REQ: F-015
#[test]
fn feature_015_detector_regex_stops_at_the_first_line_number() {
    assert_eq!(first_match("foo.rs:12:5"), Some("foo.rs:12".to_string()));
}

/// REQ: F-015
#[test]
fn feature_015_detector_regex_takes_the_multi_digit_line_whole() {
    assert_eq!(first_match("see lib.rs:10234 now"), Some("lib.rs:10234".to_string()));
}

/// REQ: F-015
#[test]
fn feature_015_detector_regex_finds_each_link_in_a_line_in_order() {
    let found: Vec<String> = detector_regex().find_iter("a.rs:1 and b/c.ts:22").map(|m| m.as_str().to_string()).collect();
    assert_eq!(found, ["a.rs:1", "b/c.ts:22"]);
}

proptest! {
    // REQ: F-015
    #[test]
    fn feature_015_detector_regex_matches_every_generated_path_line_whole(
        path in "[./A-Za-z0-9_-]{1,12}", extension in "[a-z]{1,5}", line in 0u32..100000
    ) {
        let text = format!("{path}.{extension}:{line}");
        let found = detector_regex().find(&text).expect("matches");
        prop_assert_eq!(found.as_str(), text.as_str());
    }
}

/// REQ: F-015
#[test]
fn feature_015_activating_a_path_line_link_opens_the_editor_in_a_split() {
    let mut host = client_host();
    let submissions = activate(&mut host, vec![link(7, "path-line", "src/server/routes.rs:42")]);
    assert_eq!(submissions.len(), 1);
    assert_eq!(submissions[0].intent, open_spec("src/server/routes.rs", Some(41)));
}

/// REQ: F-015
#[test]
fn feature_015_the_editor_line_is_zero_based() {
    let mut host = client_host();
    let submissions = activate(&mut host, vec![link(7, "path-line", "a.rs:1")]);
    assert_eq!(intents_of(&submissions), vec![open_spec("a.rs", Some(0))]);
}

/// REQ: F-015
#[test]
fn feature_015_line_zero_saturates_at_the_first_line() {
    let mut host = client_host();
    let submissions = activate(&mut host, vec![link(7, "path-line", "a.rs:0")]);
    assert_eq!(intents_of(&submissions), vec![open_spec("a.rs", Some(0))]);
}

/// REQ: F-015
#[test]
fn feature_015_the_path_uses_the_project_scope() {
    let mut host = client_host();
    let submissions = activate(&mut host, vec![link(7, "path-line", "docs/x.md:3")]);
    match &submissions[0].intent {
        Intent::EditorOpen(spec) => {
            assert_eq!(spec.path.scope, "project");
            assert_eq!(spec.path.path, "docs/x.md");
            assert_eq!(spec.placement, EditorPlacement::Split);
        }
        other => panic!("unexpected intent {other:?}"),
    }
}

/// REQ: F-015
#[test]
fn feature_015_a_link_of_another_detector_is_ignored() {
    let mut host = client_host();
    assert!(activate(&mut host, vec![link(7, "url", "a.rs:1")]).is_empty());
}

/// REQ: F-015
#[test]
fn feature_015_a_link_without_a_numeric_line_is_ignored() {
    let mut host = client_host();
    assert!(activate(&mut host, vec![link(7, "path-line", "a.rs:x")]).is_empty());
    assert!(activate(&mut host, vec![link(7, "path-line", "a.rs")]).is_empty());
    assert!(activate(&mut host, vec![link(7, "path-line", "")]).is_empty());
}

/// REQ: F-015
#[test]
fn feature_015_a_line_number_beyond_u32_is_ignored() {
    let mut host = client_host();
    assert!(activate(&mut host, vec![link(7, "path-line", "a.rs:4294967296")]).is_empty());
}

/// REQ: F-015
#[test]
fn feature_015_the_largest_line_number_opens_one_line_before_it() {
    let mut host = client_host();
    let submissions = activate(&mut host, vec![link(7, "path-line", "a.rs:4294967295")]);
    assert_eq!(intents_of(&submissions), vec![open_spec("a.rs", Some(4294967294))]);
}

/// REQ: F-015
#[test]
fn feature_015_two_links_in_one_batch_open_in_order() {
    let mut host = client_host();
    let submissions = activate(&mut host, vec![link(7, "path-line", "a.rs:2"), link(8, "path-line", "b.rs:3")]);
    assert_eq!(intents_of(&submissions), vec![open_spec("a.rs", Some(1)), open_spec("b.rs", Some(2))]);
}

/// REQ: F-015
#[test]
fn feature_015_other_events_in_the_batch_submit_nothing() {
    let mut host = client_host();
    let other = Event::PaneTitleChanged(PaneTitleChangedEvent { node: NodeId(7), title: "x".to_string(), source: TitleOrigin::Osc });
    assert!(activate(&mut host, vec![other]).is_empty());
}

/// REQ: F-015
#[test]
fn feature_015_an_empty_batch_submits_nothing() {
    let mut host = client_host();
    assert!(activate(&mut host, Vec::new()).is_empty());
}

/// REQ: F-015
#[test]
fn feature_015_submissions_carry_distinct_increasing_correlations_and_no_dedup_key() {
    let mut host = client_host();
    let submissions = activate(&mut host, vec![link(7, "path-line", "a.rs:2"), link(7, "path-line", "a.rs:2")]);
    assert_eq!(submissions.len(), 2);
    assert!(submissions[0].correlation >= 1u64 << 63);
    assert!(submissions[1].correlation > submissions[0].correlation);
    assert!(submissions.iter().all(|submission| submission.dedup_key.is_none()));
}

/// REQ: F-015
#[test]
fn feature_015_submitted_log_of_the_host_matches_the_returned_submissions() {
    let mut host = client_host();
    let returned = activate(&mut host, vec![link(7, "path-line", "a.rs:9")]);
    assert_eq!(host.submitted(), returned);
}

proptest! {
    // REQ: F-015
    #[test]
    fn feature_015_any_path_and_line_open_one_line_before(path in "[a-z][a-z0-9_/]{0,20}\\.[a-z]{1,4}", line in 1u32..1_000_000) {
        let mut host = client_host();
        let submissions = host.deliver(vec![link(3, "path-line", &format!("{path}:{line}"))]);
        prop_assert_eq!(submissions.len(), 1);
        prop_assert_eq!(&submissions[0].intent, &open_spec(&path, Some(line - 1)));
    }
}

fn intents_of(submissions: &[Submission]) -> Vec<Intent> {
    crate::common::intents(submissions)
}
