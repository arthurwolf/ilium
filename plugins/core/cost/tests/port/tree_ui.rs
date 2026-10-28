//! Twin of the cost test in ilium-client/src/tree_ui.rs: cost ordering of the tree rows.

use serde_json::json;

use crate::common::client;

/// REQ: PORT:ilium-client/src/tree_ui.rs::cost_order_lists_the_most_expensive_agent_first
#[test]
fn port_tree_ui_cost_order_lists_the_most_expensive_agent_first() {
    let rows = json!([{ "name": "shell", "level": 0, "amount": 1.0 }, { "name": "second_shell", "level": 3, "amount": 30.0 }]);
    let manual = client(
        "tree-ui.render-order",
        json!({ "rows": rows.clone(), "order": "manual" }),
    );
    assert!(manual["row_of"]["shell"].as_u64() < manual["row_of"]["second_shell"].as_u64());
    let by_cost = client(
        "tree-ui.render-order",
        json!({ "rows": rows, "order": "cost_descending" }),
    );
    assert!(by_cost["row_of"]["second_shell"].as_u64() < by_cost["row_of"]["shell"].as_u64());
}

/// REQ: PORT:ilium-client/src/tree_ui.rs::cost_order_lists_the_most_expensive_agent_first
#[test]
fn port_tree_ui_cost_order_puts_the_costliest_of_three_rows_first() {
    let rows = json!([
        { "name": "cheap", "level": 0, "amount": 1.0 },
        { "name": "mid", "level": 1, "amount": 5.0 },
        { "name": "costly", "level": 2, "amount": 30.0 }
    ]);
    let by_cost = client(
        "tree-ui.render-order",
        json!({ "rows": rows, "order": "cost_descending" }),
    );
    assert!(by_cost["row_of"]["costly"].as_u64() < by_cost["row_of"]["mid"].as_u64());
    assert!(by_cost["row_of"]["mid"].as_u64() < by_cost["row_of"]["cheap"].as_u64());
}
