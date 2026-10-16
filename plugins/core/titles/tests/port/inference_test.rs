//! Port twins of the tests of `inference_test.rs`.
#![allow(unused_imports)]

use ilium_plugin_sdk::types::*;
use serde_json::json;

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
    for reply in ["", "one two three four five six seven eight nine ten eleven twelve thirteen"] {
        let mut host = activated_server();
        title_for_prompt(&mut host, 3, "Fix the login bug", reply);
        assert!(renames(&host.submitted()).is_empty());
    }
}
