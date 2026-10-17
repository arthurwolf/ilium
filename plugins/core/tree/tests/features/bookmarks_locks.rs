//! F-006 bookmarks and locks (page sample "Bookmarks and locks", capability tree.metadata).
//!
//! The bookmark flag is one metadata byte: `1` is bookmarked, empty is not.

use ilium_plugin_sdk::testing::FakeSession;
use ilium_plugin_sdk::types::{Intent, NodeId, SetBookmarkedSpec};

use crate::common::{add_node, workspace_root};
use ilium_plugin_sdk::types::NodeClass;

const SERVICE: &str = "ilium.core.tree/bookmarks";

fn session() -> FakeSession {
    let mut session = FakeSession::new();
    session
        .install(workspace_root().join("plugins/core/tree"))
        .start();
    session
}

fn call(
    session: &mut FakeSession,
    request: serde_json::Value,
) -> Result<String, ilium_plugin_sdk::types::PluginError> {
    use ilium_plugin_sdk::exports::ServiceReply;
    match session.call_service("ilium.core.tree", SERVICE, "^0.1", &request.to_string())? {
        ServiceReply::Done(json) => Ok(json),
        ServiceReply::Deferred => panic!("the bookmarks service answers at once"),
    }
}

/// REQ: F-006
#[test]
fn feature_006_adding_a_bookmark_sets_the_metadata_byte_to_one() {
    let mut session = session();
    let node = add_node(
        session.server("ilium.core.tree"),
        None,
        "api-deploy",
        NodeClass::Pane,
    );
    call(&mut session, serde_json::json!({"op": "add", "node": node})).expect("add accepted");
    session.pump().expect("pump");
    assert_eq!(
        session.server("ilium.core.tree").tree().metadata(node),
        Some(vec![1])
    );
}

/// REQ: F-006
#[test]
fn feature_006_adding_a_bookmark_mirrors_it_with_set_bookmarked_true() {
    let mut session = session();
    let node = add_node(
        session.server("ilium.core.tree"),
        None,
        "api-deploy",
        NodeClass::Pane,
    );
    call(&mut session, serde_json::json!({"op": "add", "node": node})).expect("add accepted");
    session.pump().expect("pump");
    let intents: Vec<Intent> = session
        .server("ilium.core.tree")
        .submitted()
        .into_iter()
        .map(|s| s.intent)
        .collect();
    assert!(intents.contains(&Intent::SetBookmarked(SetBookmarkedSpec {
        node: NodeId(node),
        bookmarked: true
    })));
}

/// REQ: F-006
#[test]
fn feature_006_removing_a_bookmark_clears_the_metadata() {
    let mut session = session();
    let node = add_node(
        session.server("ilium.core.tree"),
        None,
        "api-deploy",
        NodeClass::Pane,
    );
    call(&mut session, serde_json::json!({"op": "add", "node": node})).expect("add accepted");
    call(
        &mut session,
        serde_json::json!({"op": "remove", "node": node}),
    )
    .expect("remove accepted");
    session.pump().expect("pump");
    let metadata = session.server("ilium.core.tree").tree().metadata(node);
    assert!(metadata.is_none() || metadata == Some(vec![]));
}

/// REQ: F-006
#[test]
fn feature_006_removing_a_bookmark_mirrors_set_bookmarked_false() {
    let mut session = session();
    let node = add_node(
        session.server("ilium.core.tree"),
        None,
        "api-deploy",
        NodeClass::Pane,
    );
    call(&mut session, serde_json::json!({"op": "add", "node": node})).expect("add accepted");
    call(
        &mut session,
        serde_json::json!({"op": "remove", "node": node}),
    )
    .expect("remove accepted");
    session.pump().expect("pump");
    let intents: Vec<Intent> = session
        .server("ilium.core.tree")
        .submitted()
        .into_iter()
        .map(|s| s.intent)
        .collect();
    assert_eq!(
        intents.last(),
        Some(&Intent::SetBookmarked(SetBookmarkedSpec {
            node: NodeId(node),
            bookmarked: false
        }))
    );
}

/// REQ: F-006
#[test]
fn feature_006_the_list_operation_returns_the_bookmarked_nodes_as_json() {
    let mut session = session();
    let first = add_node(
        session.server("ilium.core.tree"),
        None,
        "a",
        NodeClass::Pane,
    );
    let second = add_node(
        session.server("ilium.core.tree"),
        None,
        "b",
        NodeClass::Pane,
    );
    call(
        &mut session,
        serde_json::json!({"op": "add", "node": first}),
    )
    .expect("add");
    call(
        &mut session,
        serde_json::json!({"op": "add", "node": second}),
    )
    .expect("add");
    let listed = call(&mut session, serde_json::json!({"op": "list"})).expect("list");
    let value: serde_json::Value = serde_json::from_str(&listed).expect("list answers JSON");
    let items = value
        .as_array()
        .or_else(|| value.get("bookmarks").and_then(|items| items.as_array()))
        .unwrap_or_else(|| {
            panic!("list answers an array of node ids (bare or under `bookmarks`): {value}")
        });
    let mut ids: Vec<u64> = items
        .iter()
        .map(|item| {
            item.as_u64()
                .or_else(|| item.get("node").and_then(|node| node.as_u64()))
                .unwrap_or(u64::MAX)
        })
        .collect();
    ids.sort_unstable();
    let mut expected = vec![first, second];
    expected.sort_unstable();
    assert_eq!(ids, expected, "exactly the two bookmarked nodes: {value}");
}

/// REQ: F-006
#[test]
fn feature_006_the_list_is_empty_before_any_bookmark() {
    let mut session = session();
    let listed = call(&mut session, serde_json::json!({"op": "list"})).expect("list");
    let value: serde_json::Value = serde_json::from_str(&listed).expect("JSON");
    let is_empty = value.as_array().is_some_and(|items| items.is_empty())
        || value
            .get("bookmarks")
            .and_then(|items| items.as_array())
            .is_some_and(|items| items.is_empty());
    assert!(is_empty, "list: {value}");
}

/// REQ: F-006
#[test]
fn feature_006_adding_a_bookmark_for_a_missing_node_fails_with_not_found() {
    use ilium_plugin_sdk::types::ErrorCode;
    let mut session = session();
    let error = call(
        &mut session,
        serde_json::json!({"op": "add", "node": 424_242u64}),
    )
    .expect_err("no such node");
    assert_eq!(error.code, ErrorCode::NotFound);
}

/// REQ: F-006
#[test]
fn feature_006_bookmarks_survive_a_restart_of_the_server_side() {
    let mut session = session();
    let node = add_node(
        session.server("ilium.core.tree"),
        None,
        "api-deploy",
        NodeClass::Pane,
    );
    call(&mut session, serde_json::json!({"op": "add", "node": node})).expect("add");
    session.pump().expect("pump");
    let saved = session
        .server("ilium.core.tree")
        .deactivate(ilium_plugin_sdk::types::DeactivationReason::Shutdown);
    let metadata_before = session.server("ilium.core.tree").tree().metadata(node);
    // The flag lives in tree metadata, so the plugin may save no state of its own (None) or some bytes.
    let saved_bytes = match saved {
        Some(bytes) => bytes,
        None => Vec::new(),
    };
    session.server("ilium.core.tree").restart_with(saved_bytes);
    assert_eq!(
        session.server("ilium.core.tree").tree().metadata(node),
        metadata_before
    );
    assert_eq!(metadata_before, Some(vec![1]));
}
