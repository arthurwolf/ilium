//! api-tree.md, capability-tree-read.md and capability-tree-metadata.md: `get`, `children`,
//! `roots`, `find`, `metadata` and `set-metadata` of `ilium:host/tree`. Reads need `tree.read`,
//! the agent field and the `signature` filter need `agent.read`, and the metadata calls need
//! `tree.metadata`.
//!
//! Tree building helpers on the fixture are listed in API_GAPS_p2.md.

use ilium_plugin_api::generated::host::HostTree;
use ilium_plugin_api::generated::types::{ErrorCode, NodeClass, NodeId, NodeQuery};
use ilium_plugin_api::Tier;
use ilium_plugin_host::testing::imports_fixture::ImportsFixture;
use proptest::prelude::*;

use crate::support::*;

struct Nodes {
    /// Group at the root.
    group: NodeId,
    /// Folder inside `group`, holding `claude_pane` and `plain_pane`.
    folder: NodeId,
    claude_pane: NodeId,
    plain_pane: NodeId,
    /// Locked folder at the root holding `codex_pane`.
    locked_folder: NodeId,
    codex_pane: NodeId,
}

fn tree_fixture(grants: &[&str]) -> (ImportsFixture, Nodes) {
    let mut fixture = fixture_t2(grants);
    let group = fixture.add_group("Work");
    let folder = fixture.add_group_under(group, "Backend");
    let claude_pane = fixture.add_terminal_pane_under(folder, "claude", 80, 24);
    let plain_pane = fixture.add_terminal_pane_under(folder, "shell", 80, 24);
    fixture.set_agent(claude_pane, "claude");
    let locked_folder = fixture.add_group("Locked");
    let codex_pane = fixture.add_terminal_pane_under(locked_folder, "codex", 80, 24);
    fixture.set_agent(codex_pane, "codex");
    fixture.lock_node(locked_folder);
    (
        fixture,
        Nodes {
            group,
            folder,
            claude_pane,
            plain_pane,
            locked_folder,
            codex_pane,
        },
    )
}

const READ: &[&str] = &["tree.read"];
const READ_AGENT: &[&str] = &["tree.read", "agent.read"];
const META: &[&str] = &["tree.read", "tree.metadata"];

// ---- get ---------------------------------------------------------------------------------------

/// REQ: WIT:ilium:host/tree.get
#[test]
fn tree_get_returns_the_view_of_a_node() {
    let (mut fixture, nodes) = tree_fixture(READ);
    let view = fixture.tree().get(nodes.plain_pane).expect("node exists");
    assert_eq!(view.id, nodes.plain_pane);
    assert_eq!(view.parent, Some(nodes.folder));
    assert_eq!(view.class, NodeClass::Pane);
    assert_eq!(view.title, "shell");
    assert!(!view.locked);
}

/// REQ: WIT:ilium:host/tree.get
#[test]
fn tree_get_of_a_terminal_pane_names_its_content_kind() {
    let (mut fixture, nodes) = tree_fixture(READ);
    let view = fixture.tree().get(nodes.plain_pane).expect("node exists");
    assert_eq!(view.content_kind.as_deref(), Some("ilium.terminal"));
}

/// REQ: WIT:ilium:host/tree.get
#[test]
fn tree_get_of_a_group_is_not_a_pane() {
    let (mut fixture, nodes) = tree_fixture(READ);
    let view = fixture.tree().get(nodes.folder).expect("node exists");
    // A group nested inside another group is a folder (api-tree.md: container | folder | pane).
    assert_eq!(view.class, NodeClass::Folder);
    assert_eq!(view.content_kind, None);
}

/// REQ: WIT:ilium:host/tree.get
#[test]
fn tree_get_of_a_root_node_has_no_parent() {
    let (mut fixture, nodes) = tree_fixture(READ);
    let view = fixture.tree().get(nodes.group).expect("node exists");
    assert_eq!(view.parent, None);
}

/// REQ: WIT:ilium:host/tree.get
#[test]
fn tree_get_of_a_locked_node_reports_locked() {
    let (mut fixture, nodes) = tree_fixture(READ);
    assert!(
        fixture
            .tree()
            .get(nodes.locked_folder)
            .expect("node exists")
            .locked
    );
}

/// REQ: WIT:ilium:host/tree.get
#[test]
fn tree_get_of_an_unknown_node_is_none() {
    let (mut fixture, _nodes) = tree_fixture(READ);
    assert_eq!(fixture.tree().get(NodeId(9_999_999)), None);
}

/// REQ: WIT:ilium:host/tree.get
#[test]
fn tree_get_agent_is_none_without_the_agent_read_grant() {
    let (mut fixture, nodes) = tree_fixture(READ);
    let view = fixture.tree().get(nodes.claude_pane).expect("node exists");
    assert_eq!(view.agent, None);
}

/// REQ: WIT:ilium:host/tree.get
#[test]
fn tree_get_agent_is_present_with_the_agent_read_grant() {
    let (mut fixture, nodes) = tree_fixture(READ_AGENT);
    let view = fixture.tree().get(nodes.claude_pane).expect("node exists");
    assert_eq!(
        view.agent.map(|agent| agent.signature),
        Some("claude".to_owned())
    );
    assert_eq!(
        fixture
            .tree()
            .get(nodes.plain_pane)
            .expect("node exists")
            .agent,
        None
    );
}

/// REQ: WIT:ilium:host/tree.get
#[test]
fn tree_get_without_tree_read_is_not_permitted() {
    let (mut fixture, nodes) = tree_fixture(&[]);
    assert_eq!(fixture.tree().get(nodes.plain_pane), None);
    assert_eq!(
        fixture.last_import_error_code(),
        Some(ErrorCode::NotPermitted)
    );
}

/// REQ: WIT:ilium:host/tree.get
#[test]
fn tree_get_a_title_change_moves_the_revision() {
    let (mut fixture, nodes) = tree_fixture(READ);
    let before = fixture
        .tree()
        .get(nodes.plain_pane)
        .expect("node exists")
        .revision;
    fixture.rename_node(nodes.plain_pane, "renamed");
    let after = fixture.tree().get(nodes.plain_pane).expect("node exists");
    assert_eq!(after.title, "renamed");
    assert!(after.revision > before);
}

// ---- children ----------------------------------------------------------------------------------

/// REQ: WIT:ilium:host/tree.children
#[test]
fn tree_children_returns_the_direct_children_in_tree_order() {
    let (mut fixture, nodes) = tree_fixture(READ);
    assert_eq!(
        fixture.tree().children(nodes.folder),
        vec![nodes.claude_pane, nodes.plain_pane]
    );
}

/// REQ: WIT:ilium:host/tree.children
#[test]
fn tree_children_lists_only_direct_children_not_deeper_nodes() {
    let (mut fixture, nodes) = tree_fixture(READ);
    assert_eq!(fixture.tree().children(nodes.group), vec![nodes.folder]);
}

/// REQ: WIT:ilium:host/tree.children
#[test]
fn tree_children_of_a_pane_is_empty() {
    let (mut fixture, nodes) = tree_fixture(READ);
    assert!(fixture.tree().children(nodes.plain_pane).is_empty());
}

/// REQ: WIT:ilium:host/tree.children
#[test]
fn tree_children_of_an_unknown_node_is_empty() {
    let (mut fixture, _nodes) = tree_fixture(READ);
    assert!(fixture.tree().children(NodeId(9_999_999)).is_empty());
}

/// REQ: WIT:ilium:host/tree.children
#[test]
fn tree_children_without_tree_read_is_not_permitted() {
    let (mut fixture, nodes) = tree_fixture(&[]);
    assert!(fixture.tree().children(nodes.folder).is_empty());
    assert_eq!(
        fixture.last_import_error_code(),
        Some(ErrorCode::NotPermitted)
    );
}

/// REQ: WIT:ilium:host/tree.children
#[test]
fn tree_children_keeps_the_order_of_siblings_added_later() {
    let (mut fixture, nodes) = tree_fixture(READ);
    let third = fixture.add_terminal_pane_under(nodes.folder, "third", 80, 24);
    assert_eq!(
        fixture.tree().children(nodes.folder),
        vec![nodes.claude_pane, nodes.plain_pane, third]
    );
}

// ---- roots -------------------------------------------------------------------------------------

/// REQ: WIT:ilium:host/tree.roots
#[test]
fn tree_roots_returns_the_top_level_nodes_in_tree_order() {
    let (mut fixture, nodes) = tree_fixture(READ);
    assert_eq!(
        fixture.tree().roots(),
        vec![nodes.group, nodes.locked_folder]
    );
}

/// REQ: WIT:ilium:host/tree.roots
#[test]
fn tree_roots_of_an_empty_tree_is_empty() {
    let mut fixture = fixture_t2(READ);
    assert!(fixture.tree().roots().is_empty());
}

/// REQ: WIT:ilium:host/tree.roots
#[test]
fn tree_roots_does_not_list_nested_nodes() {
    let (mut fixture, nodes) = tree_fixture(READ);
    let roots = fixture.tree().roots();
    assert!(!roots.contains(&nodes.folder));
    assert!(!roots.contains(&nodes.claude_pane));
}

/// REQ: WIT:ilium:host/tree.roots
#[test]
fn tree_roots_without_tree_read_is_not_permitted() {
    let (mut fixture, _nodes) = tree_fixture(&[]);
    assert!(fixture.tree().roots().is_empty());
    assert_eq!(
        fixture.last_import_error_code(),
        Some(ErrorCode::NotPermitted)
    );
}

// ---- find --------------------------------------------------------------------------------------

/// REQ: WIT:ilium:host/tree.find
#[test]
fn tree_find_with_every_field_unset_returns_every_node_parent_before_children() {
    let (mut fixture, nodes) = tree_fixture(READ);
    let found = fixture.tree().find(NodeQuery::default());
    assert_eq!(found.len(), 6);
    let position = |id: NodeId| {
        found
            .iter()
            .position(|candidate| *candidate == id)
            .expect("listed")
    };
    assert!(position(nodes.group) < position(nodes.folder));
    assert!(position(nodes.folder) < position(nodes.claude_pane));
    assert!(position(nodes.folder) < position(nodes.plain_pane));
    assert!(position(nodes.locked_folder) < position(nodes.codex_pane));
}

/// REQ: WIT:ilium:host/tree.find
#[test]
fn tree_find_by_content_kind_matches_the_exact_string() {
    let (mut fixture, nodes) = tree_fixture(READ);
    let found = fixture.tree().find(NodeQuery {
        content_kind: Some("ilium.terminal".to_owned()),
        ..NodeQuery::default()
    });
    assert_eq!(
        found,
        vec![nodes.claude_pane, nodes.plain_pane, nodes.codex_pane]
    );
    let none = fixture.tree().find(NodeQuery {
        content_kind: Some("ilium.termin".to_owned()),
        ..NodeQuery::default()
    });
    assert!(none.is_empty());
}

/// REQ: WIT:ilium:host/tree.find
#[test]
fn tree_find_by_signature_matches_agent_panes_with_agent_read() {
    let (mut fixture, nodes) = tree_fixture(READ_AGENT);
    let found = fixture.tree().find(NodeQuery {
        signature: Some("claude".to_owned()),
        ..NodeQuery::default()
    });
    assert_eq!(found, vec![nodes.claude_pane]);
}

/// REQ: WIT:ilium:host/tree.find
#[test]
fn tree_find_by_signature_matches_nothing_without_agent_read() {
    let (mut fixture, _nodes) = tree_fixture(READ);
    let found = fixture.tree().find(NodeQuery {
        signature: Some("claude".to_owned()),
        ..NodeQuery::default()
    });
    assert!(found.is_empty());
}

/// REQ: WIT:ilium:host/tree.find
#[test]
fn tree_find_signature_is_an_exact_match_not_a_prefix() {
    let (mut fixture, _nodes) = tree_fixture(READ_AGENT);
    let found = fixture.tree().find(NodeQuery {
        signature: Some("clau".to_owned()),
        ..NodeQuery::default()
    });
    assert!(found.is_empty());
}

/// REQ: WIT:ilium:host/tree.find
#[test]
fn tree_find_under_matches_strict_descendants_at_any_depth() {
    let (mut fixture, nodes) = tree_fixture(READ);
    let found = fixture.tree().find(NodeQuery {
        under: Some(nodes.group),
        ..NodeQuery::default()
    });
    assert_eq!(
        found,
        vec![nodes.folder, nodes.claude_pane, nodes.plain_pane]
    );
}

/// REQ: WIT:ilium:host/tree.find
#[test]
fn tree_find_under_excludes_the_node_itself() {
    let (mut fixture, nodes) = tree_fixture(READ);
    let found = fixture.tree().find(NodeQuery {
        under: Some(nodes.folder),
        ..NodeQuery::default()
    });
    assert!(!found.contains(&nodes.folder));
}

/// REQ: WIT:ilium:host/tree.find
#[test]
fn tree_find_under_a_pane_is_empty() {
    let (mut fixture, nodes) = tree_fixture(READ);
    let found = fixture.tree().find(NodeQuery {
        under: Some(nodes.plain_pane),
        ..NodeQuery::default()
    });
    assert!(found.is_empty());
}

/// REQ: WIT:ilium:host/tree.find
#[test]
fn tree_find_combines_its_fields_with_and() {
    let (mut fixture, nodes) = tree_fixture(READ_AGENT);
    let found = fixture.tree().find(NodeQuery {
        content_kind: Some("ilium.terminal".to_owned()),
        signature: Some("codex".to_owned()),
        under: Some(nodes.group),
    });
    assert!(found.is_empty());
    let found = fixture.tree().find(NodeQuery {
        content_kind: Some("ilium.terminal".to_owned()),
        signature: Some("codex".to_owned()),
        under: Some(nodes.locked_folder),
    });
    assert_eq!(found, vec![nodes.codex_pane]);
}

/// REQ: WIT:ilium:host/tree.find
#[test]
fn tree_find_under_an_unknown_node_is_empty() {
    let (mut fixture, _nodes) = tree_fixture(READ);
    let found = fixture.tree().find(NodeQuery {
        under: Some(NodeId(9_999_999)),
        ..NodeQuery::default()
    });
    assert!(found.is_empty());
}

/// REQ: WIT:ilium:host/tree.find
#[test]
fn tree_find_returns_at_most_4096_ids() {
    let mut fixture = fixture_t2(READ);
    let group = fixture.add_group("Many");
    for index in 0..4100 {
        fixture.add_terminal_pane_under(group, &format!("p{index}"), 20, 5);
    }
    assert_eq!(fixture.tree().find(NodeQuery::default()).len(), 4096);
}

/// REQ: WIT:ilium:host/tree.find
#[test]
fn tree_find_without_tree_read_is_not_permitted() {
    let (mut fixture, _nodes) = tree_fixture(&[]);
    assert!(fixture.tree().find(NodeQuery::default()).is_empty());
    assert_eq!(
        fixture.last_import_error_code(),
        Some(ErrorCode::NotPermitted)
    );
}

proptest! {
    /// REQ: WIT:ilium:host/tree.find
    #[test]
    fn tree_find_under_never_returns_the_node_or_a_node_outside_its_subtree(index in 0usize..6) {
        let (mut fixture, nodes) = tree_fixture(READ);
        let all = [nodes.group, nodes.folder, nodes.claude_pane, nodes.plain_pane, nodes.locked_folder, nodes.codex_pane];
        let found = fixture.tree().find(NodeQuery { under: Some(all[index]), ..NodeQuery::default() });
        prop_assert!(!found.contains(&all[index]));
        // Expected result: every descendant, computed by walking `children`.
        let mut expected = Vec::new();
        let mut pending = fixture.tree().children(all[index]);
        while let Some(next) = pending.pop() {
            expected.push(next);
            pending.extend(fixture.tree().children(next));
        }
        let mut sorted_found = found.clone();
        sorted_found.sort_by_key(|id| format!("{id:?}"));
        expected.sort_by_key(|id| format!("{id:?}"));
        prop_assert_eq!(sorted_found, expected);
        for id in found {
            let mut cursor = fixture.tree().get(id).and_then(|view| view.parent);
            let mut inside = false;
            while let Some(parent) = cursor {
                if parent == all[index] { inside = true; break; }
                cursor = fixture.tree().get(parent).and_then(|view| view.parent);
            }
            prop_assert!(inside);
        }
    }
}

// ---- metadata and set-metadata -----------------------------------------------------------------

/// REQ: WIT:ilium:host/tree.metadata
#[test]
fn tree_metadata_is_none_when_nothing_is_stored() {
    let (mut fixture, nodes) = tree_fixture(META);
    assert_eq!(fixture.tree().metadata(nodes.plain_pane), None);
}

/// REQ: WIT:ilium:host/tree.metadata, WIT:ilium:host/tree.set-metadata
#[test]
fn tree_metadata_returns_the_bytes_that_set_metadata_stored() {
    let (mut fixture, nodes) = tree_fixture(META);
    expect_ok(fixture.tree().set_metadata(nodes.plain_pane, vec![1, 2, 3]));
    assert_eq!(
        fixture.tree().metadata(nodes.plain_pane),
        Some(vec![1, 2, 3])
    );
}

/// REQ: WIT:ilium:host/tree.metadata
#[test]
fn tree_metadata_is_kept_per_node() {
    let (mut fixture, nodes) = tree_fixture(META);
    expect_ok(fixture.tree().set_metadata(nodes.plain_pane, vec![1]));
    assert_eq!(fixture.tree().metadata(nodes.claude_pane), None);
}

/// REQ: WIT:ilium:host/tree.metadata
#[test]
fn tree_metadata_is_per_plugin_another_plugin_sees_no_entry() {
    let (mut fixture, nodes) = tree_fixture(META);
    expect_ok(fixture.tree().set_metadata(nodes.plain_pane, vec![1]));
    assert_eq!(fixture.other_plugin_metadata(nodes.plain_pane), None);
}

/// REQ: WIT:ilium:host/tree.metadata
#[test]
fn tree_metadata_survives_a_rename_of_the_node() {
    let (mut fixture, nodes) = tree_fixture(META);
    expect_ok(fixture.tree().set_metadata(nodes.plain_pane, vec![5]));
    fixture.rename_node(nodes.plain_pane, "renamed");
    assert_eq!(fixture.tree().metadata(nodes.plain_pane), Some(vec![5]));
}

/// REQ: WIT:ilium:host/tree.metadata
#[test]
fn tree_metadata_without_the_grant_is_not_permitted() {
    let (mut fixture, nodes) = tree_fixture(READ);
    assert_eq!(fixture.tree().metadata(nodes.plain_pane), None);
    assert_eq!(
        fixture.last_import_error_code(),
        Some(ErrorCode::NotPermitted)
    );
}

/// REQ: WIT:ilium:host/tree.metadata
#[test]
fn tree_metadata_of_an_unknown_node_is_none() {
    let (mut fixture, _nodes) = tree_fixture(META);
    assert_eq!(fixture.tree().metadata(NodeId(9_999_999)), None);
}

/// REQ: WIT:ilium:host/tree.set-metadata
#[test]
fn tree_set_metadata_replaces_the_previous_bytes() {
    let (mut fixture, nodes) = tree_fixture(META);
    expect_ok(fixture.tree().set_metadata(nodes.plain_pane, vec![1, 2, 3]));
    expect_ok(fixture.tree().set_metadata(nodes.plain_pane, vec![9]));
    assert_eq!(fixture.tree().metadata(nodes.plain_pane), Some(vec![9]));
}

/// REQ: WIT:ilium:host/tree.set-metadata
#[test]
fn tree_set_metadata_empty_write_leaves_some_empty_not_none() {
    let (mut fixture, nodes) = tree_fixture(META);
    expect_ok(fixture.tree().set_metadata(nodes.plain_pane, vec![1]));
    expect_ok(fixture.tree().set_metadata(nodes.plain_pane, Vec::new()));
    assert_eq!(fixture.tree().metadata(nodes.plain_pane), Some(Vec::new()));
}

/// REQ: WIT:ilium:host/tree.set-metadata
#[test]
fn tree_set_metadata_exactly_65536_bytes_accepted() {
    let (mut fixture, nodes) = tree_fixture(META);
    expect_ok(
        fixture
            .tree()
            .set_metadata(nodes.plain_pane, vec![7; 65_536]),
    );
}

/// REQ: WIT:ilium:host/tree.set-metadata
#[test]
fn tree_set_metadata_65537_bytes_quota_exceeded() {
    let (mut fixture, nodes) = tree_fixture(META);
    let code = expect_code(
        fixture
            .tree()
            .set_metadata(nodes.plain_pane, vec![7; 65_537]),
    );
    assert_eq!(code, ErrorCode::QuotaExceeded);
}

/// REQ: WIT:ilium:host/tree.set-metadata
#[test]
fn tree_set_metadata_refused_write_keeps_the_previous_bytes() {
    let (mut fixture, nodes) = tree_fixture(META);
    expect_ok(fixture.tree().set_metadata(nodes.plain_pane, vec![1]));
    let _ = fixture
        .tree()
        .set_metadata(nodes.plain_pane, vec![7; 65_537]);
    assert_eq!(fixture.tree().metadata(nodes.plain_pane), Some(vec![1]));
}

/// REQ: WIT:ilium:host/tree.set-metadata
#[test]
fn tree_set_metadata_t2_manifest_request_above_the_64_kib_ceiling_is_clamped() {
    let mut fixture = ImportsFixture::builder()
        .tier(Tier::T2)
        .grants(META)
        .limit("node_metadata_kib", 256)
        .build();
    let node = fixture.add_group("Node");
    let code = expect_code(fixture.tree().set_metadata(node, vec![0; 65_537]));
    assert_eq!(code, ErrorCode::QuotaExceeded);
}

/// REQ: WIT:ilium:host/tree.set-metadata
#[test]
fn tree_set_metadata_t1_accepts_up_to_the_256_kib_ceiling() {
    let mut fixture = ImportsFixture::builder()
        .tier(Tier::T1)
        .grants(META)
        .limit("node_metadata_kib", 256)
        .build();
    let node = fixture.add_group("Node");
    expect_ok(fixture.tree().set_metadata(node, vec![0; 262_144]));
    let code = expect_code(fixture.tree().set_metadata(node, vec![0; 262_145]));
    assert_eq!(code, ErrorCode::QuotaExceeded);
}

/// REQ: WIT:ilium:host/tree.set-metadata
#[test]
fn tree_set_metadata_unknown_node_not_found() {
    let (mut fixture, _nodes) = tree_fixture(META);
    let code = expect_code(fixture.tree().set_metadata(NodeId(9_999_999), vec![1]));
    assert_eq!(code, ErrorCode::NotFound);
}

/// REQ: WIT:ilium:host/tree.set-metadata
#[test]
fn tree_set_metadata_without_the_grant_not_permitted() {
    let (mut fixture, nodes) = tree_fixture(READ);
    let code = expect_code(fixture.tree().set_metadata(nodes.plain_pane, vec![1]));
    assert_eq!(code, ErrorCode::NotPermitted);
}

/// REQ: WIT:ilium:host/tree.set-metadata
#[test]
fn tree_set_metadata_changes_neither_the_revision_nor_the_events() {
    let (mut fixture, nodes) = tree_fixture(META);
    // Positive control: a rename must produce at least one event in this fixture.
    let _ = fixture.events();
    fixture.rename_node(nodes.plain_pane, "renamed");
    fixture.pump();
    assert!(!fixture.events().is_empty());
    let _ = fixture.events();
    let before = fixture
        .tree()
        .get(nodes.plain_pane)
        .expect("node exists")
        .revision;
    expect_ok(fixture.tree().set_metadata(nodes.plain_pane, vec![1, 2]));
    fixture.pump();
    assert_eq!(
        fixture
            .tree()
            .get(nodes.plain_pane)
            .expect("node exists")
            .revision,
        before
    );
    assert!(fixture.events().is_empty());
}

proptest! {
    /// REQ: WIT:ilium:host/tree.set-metadata
    #[test]
    fn tree_set_metadata_accepts_exactly_up_to_65536_bytes(length in 65_000usize..66_100) {
        let (mut fixture, nodes) = tree_fixture(META);
        let result = fixture.tree().set_metadata(nodes.plain_pane, vec![0; length]);
        if length <= 65_536 {
            prop_assert!(result.is_ok());
        } else {
            prop_assert_eq!(result.err().map(|error| error.code), Some(ErrorCode::QuotaExceeded));
        }
    }
}
