//! Port twins of the tests of `inference_test.rs`.

use crate::common::*;

/// REQ: PORT:ilium-client/src/inference_test.rs::parser_accepts_fenced_json_and_trailing_model_text
#[test]
fn port_043_fenced_replies_are_tolerated() {
    let host = organised_host(&format!("```json\n{}\n```", plan_reply()));
    assert_eq!(restructures(&host.submitted()).len(), 1);
    let host = organised_host(&format!("```\n{}\n```", plan_reply()));
    assert_eq!(restructures(&host.submitted()).len(), 1);
}

/// REQ: PORT:ilium-client/src/inference_test.rs::normalization_bounds_labels
#[test]
fn port_041_out_of_bounds_titles_are_not_applied() {
    for reply in [
        "",
        "one two three four five six seven eight nine ten eleven twelve thirteen",
    ] {
        let mut host = activated_server();
        title_for_prompt(&mut host, 3, "Fix the login bug", reply);
        assert!(renames(&host.submitted()).is_empty());
    }
}

/// REQ: PORT:ilium-client/src/inference_test.rs::parser_accepts_fenced_json_and_trailing_model_text
#[test]
fn port_043_trailing_prose_after_a_fence_is_ignored() {
    let host = organised_host(&format!(
        "```json\n{}\n```\nLet me know if you want changes.",
        plan_reply()
    ));
    assert_eq!(restructures(&host.submitted()).len(), 1);
}

/// REQ: PORT:ilium-client/src/inference_test.rs::normalization_bounds_labels
#[test]
fn port_041_four_word_title_is_applied() {
    let mut host = activated_server();
    title_for_prompt(&mut host, 3, "Fix the login bug", "one two three four");
    let applied = renames(&host.submitted());
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0].title, "one two three four");
}
