//! Twins of the search tests of ilium-client/src/keys.rs.

use crate::common::*;

/// REQ: F-007, PORT:ilium-client/src/keys.rs::search_typing_updates_the_visible_prompt_without_running_a_scan
#[test]
fn port_001_typing_updates_the_field_and_runs_no_query_until_filter_changed() {
    let mut host = activated_client();
    host.deliver(vec![open_command_event()]);
    let typing = host.deliver(vec![text_changed("n")]);
    assert_eq!(query_messages(&typing), Vec::<serde_json::Value>::new());
    let screen = host.render(panel(), 48, 12).text();
    assert!(!screen.contains("deploy"), "no result rows before a query answers");
    let settled = host.deliver(vec![filter_changed("n")]);
    let messages = query_messages(&settled);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["query"], "n");
    assert_eq!(messages[0]["revision"], 1);
}
