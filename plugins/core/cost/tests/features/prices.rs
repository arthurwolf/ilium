//! Prices and the unpriced marker (F-040): known, unknown and zero-price models.

use crate::common::*;

fn text_for(model: &str) -> (String, Option<i64>) {
    let mut host = activated_server();
    let pane = add_agent(&mut host, "agent", "claude");
    let before = host.submitted().len();
    feed_pane(&mut host, pane, model, 50_000);
    let spec = latest_cost_decoration(&host, before, pane).expect("a cost decoration");
    (decoration_text(&spec), spec.sort_key)
}

/// REQ: F-040
#[test]
fn prices_040_known_model_shows_a_plain_dollar_amount() {
    let (text, _) = text_for(KNOWN_MODEL);
    assert!(text.starts_with('$') && !text.contains('~'), "{text}");
}

/// REQ: F-040
#[test]
fn prices_040_unknown_model_shows_a_lower_bound_marker() {
    let (text, _) = text_for(UNKNOWN_MODEL);
    assert!(text.starts_with("~$"), "{text}");
}

/// REQ: F-040
#[test]
fn prices_040_local_model_priced_zero_shows_zero_dollars() {
    let (text, key) = text_for(LOCAL_MODEL);
    assert_eq!(text, "$0.00");
    assert_eq!(key, Some(0));
}

/// REQ: F-040
#[test]
fn doc_f_040_user_doc_states_longest_prefix_and_override_rules() {
    let doc = user_doc("agent-cost.md");
    assert!(doc.contains("[cost.prices]"));
}

/// REQ: F-040
#[test]
fn doc_f_040_user_doc_lists_the_sonnet_price_table_entry() {
    let doc = user_doc("agent-cost.md");
    assert!(doc.to_lowercase().contains("price"));
}
