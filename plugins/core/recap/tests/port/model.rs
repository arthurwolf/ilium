//! Twins of ilium-client/src/recap/model.rs: the answer contract and the word limits.

use serde_json::json;

use crate::support::*;

const SHORT_WORD_LIMIT: usize = 40;
const LONG_WORD_LIMIT: usize = 250;

fn outcome(result: Vec<u8>) -> serde_json::Value {
    let (mut host, _, invocation) = start_shell_run(&["$ cargo test"]);
    first_recap(&finish_run(&mut host, &invocation, result))
}

/// REQ: PORT:ilium-client/src/recap/model.rs::parses_both_lengths_inside_a_code_fence
#[test]
fn port_model_parses_both_lengths_inside_a_code_fence() {
    let body = format!("```json\n{}\n```", String::from_utf8(answer(3, 5)).unwrap());
    let record = outcome(body.into_bytes());
    assert_eq!(record["state"], "summarized");
    assert_eq!(record["short"]["text"], words(3));
    assert_eq!(record["long"]["text"], words(5));
}

/// REQ: PORT:ilium-client/src/recap/model.rs::rejects_missing_empty_and_malformed_answers
#[test]
fn port_model_rejects_missing_empty_and_malformed_answers() {
    let bodies = [
        json!({"short": {"text": "only short", "pointers": []}}).to_string(),
        json!({"short": {"text": "   ", "pointers": []}, "long": {"text": "ok", "pointers": []}})
            .to_string(),
        "not json at all".to_string(),
        String::new(),
    ];
    for body in bodies {
        let record = outcome(body.clone().into_bytes());
        assert_eq!(record["state"], "failed", "{body}");
        assert!(record["short"].is_null(), "{body}");
        assert!(
            record["failure"]
                .as_str()
                .unwrap()
                .contains("not the expected JSON"),
            "{body}"
        );
    }
}

/// REQ: PORT:ilium-client/src/recap/model.rs::rejects_text_over_the_word_limit_instead_of_truncating
#[test]
fn port_model_rejects_text_over_the_word_limit_instead_of_truncating() {
    assert_eq!(
        outcome(answer(SHORT_WORD_LIMIT, LONG_WORD_LIMIT))["state"],
        "summarized"
    );
    let short_over = outcome(answer(SHORT_WORD_LIMIT + 1, 5));
    assert_eq!(short_over["state"], "failed");
    assert!(short_over["short"].is_null(), "no truncated text is shown");
    let long_over = outcome(answer(5, LONG_WORD_LIMIT + 1));
    assert_eq!(long_over["state"], "failed");
    assert!(long_over["long"].is_null());
}

/// REQ: PORT:ilium-client/src/recap/model.rs::goal_lines_come_from_recorded_state_only
#[test]
fn port_model_goal_lines_come_from_recorded_state_only() {
    let lines = goal_lines(
        &["user: hello"],
        &[
            ("ship recap".to_string(), "done"),
            ("old goal".to_string(), "unknown"),
        ],
    );
    assert_eq!(
        lines,
        ["Goal done: ship recap", "Goal state unknown: old goal"]
    );
    let (mut host, _, invocation) = start_agent_run(&["user: hello"]);
    let mut text = String::from_utf8(answer(3, 3)).unwrap();
    text = text.replace("word word word", "Goal done: invented by the model");
    let record = first_recap(&finish_run(&mut host, &invocation, text.into_bytes()));
    assert_eq!(
        record["goals"],
        json!([]),
        "model text never becomes a goal line"
    );
}

/// REQ: PORT:ilium-client/src/recap/model.rs::parses_both_lengths_inside_a_code_fence
#[test]
fn port_model_plain_code_fence_without_a_language_tag_is_parsed() {
    let body = format!("```\n{}\n```", String::from_utf8(answer(2, 2)).unwrap());
    assert_eq!(outcome(body.into_bytes())["state"], "summarized");
}

/// REQ: PORT:ilium-client/src/recap/model.rs::rejects_missing_empty_and_malformed_answers
#[test]
fn port_model_answer_missing_the_long_text_is_failed_without_long_text() {
    let record = outcome(
        json!({"short": {"text": "only short", "pointers": []}})
            .to_string()
            .into_bytes(),
    );
    assert_eq!(record["state"], "failed");
    assert!(record["long"].is_null(), "{record}");
}

/// REQ: PORT:ilium-client/src/recap/model.rs::rejects_text_over_the_word_limit_instead_of_truncating
#[test]
fn port_model_over_limit_text_leaves_no_partial_words_in_the_record() {
    let record = outcome(answer(5, LONG_WORD_LIMIT + 1));
    assert!(!record.to_string().contains(&words(5)), "{record}");
}

/// REQ: PORT:ilium-client/src/recap/model.rs::goal_lines_come_from_recorded_state_only
#[test]
fn port_model_goal_words_in_the_transcript_never_become_goal_lines() {
    assert!(goal_lines(&["user: Goal done: typed by the user"], &[]).is_empty());
}
