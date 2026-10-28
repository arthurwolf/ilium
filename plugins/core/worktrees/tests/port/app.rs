//! Twins of ilium-client/src/app.rs worktree behaviour: the close offer and the unowned guard.

use ilium_plugin_sdk::types::{Event, NodeId};
use serde_json::json;

use crate::common::*;
use crate::support::*;

/// REQ: F-070, F-072, PORT:ilium-client/src/app.rs::interactive_worktree_close_uses_correlated_offer_and_keeps_by_default
#[test]
fn port_070_closing_an_owned_pane_offers_nothing_by_default() {
    let mut host = server_host_at("/projects/acme");
    create_worktree(&mut host, &request("fix-auth"));
    host.deliver_one(pane_created(14, "fix-auth"));
    host.deliver_one(Event::NodeRemoved(NodeId(14)));
    host.settle();
    assert!(host.offered_actions().is_empty());
    assert!(removal_calls(&mut host).is_empty());
}

/// REQ: F-070, F-072, PORT:ilium-client/src/app.rs::interactive_worktree_close_uses_correlated_offer_and_keeps_by_default
#[test]
fn port_070_the_offer_names_the_worktree_of_the_closed_pane() {
    let mut host = server_host_at("/projects/acme");
    set_git(
        &mut host,
        "default_close_policy",
        json!("offer_removal_when_safe"),
    )
    .expect("valid");
    create_worktree(&mut host, &request("fix-auth"));
    host.deliver_one(pane_created(14, "fix-auth"));
    host.deliver_one(Event::NodeRemoved(NodeId(14)));
    host.settle();
    let offers = host.offered_actions();
    assert_eq!(offers.len(), 1);
    assert!(
        format!("{:?}", offers[0]).contains("fix-auth"),
        "{:?}",
        offers[0]
    );
    assert!(
        removal_calls(&mut host).is_empty(),
        "an offer removes nothing by itself"
    );
}

/// REQ: F-070, PORT:ilium-client/src/app.rs::worktree_menu_uses_server_resolved_cwd_and_guards_unowned_removal
#[test]
fn port_070_a_worktree_not_owned_by_the_plugin_is_not_removed() {
    let mut host = server_host_at("/projects/acme");
    host.workspace()
        .add_foreign("/projects/other.worktrees/x", "x");
    remove_request(
        &mut host,
        json!({"path": "/projects/other.worktrees/x", "branch": "x"}),
    );
    assert!(removal_calls(&mut host).is_empty());
}

/// REQ: F-067, PORT:ilium-client/src/app.rs::worktree_menu_uses_server_resolved_cwd_and_guards_unowned_removal
#[test]
fn port_067_the_agent_pane_starts_in_the_server_resolved_worktree_folder() {
    let mut host = server_host_at("/projects/acme");
    let submitted = create_worktree(&mut host, &request("agent/fix-login"));
    let panes = create_panes(&submitted);
    assert_eq!(panes.len(), 1);
    assert!(
        cwd_text(&panes[0]).contains("/projects/acme.worktrees/agent-fix-login"),
        "{}",
        cwd_text(&panes[0])
    );
}

/// REQ: F-067, F-072, PORT:ilium-client/src/app.rs::new_agent_submenu_starts_with_here_and_updates_worktree_availability
#[test]
fn port_072_the_default_where_starts_with_here_and_the_three_values_are_accepted() {
    let mut host = client_host();
    assert_eq!(
        host.kernel_settings().get("git", "default_where"),
        Some(json!("here"))
    );
    for value in ["new_worktree", "existing_worktree", "here"] {
        set_git(&mut host, "default_where", json!(value)).expect("valid");
        assert_eq!(
            host.kernel_settings().get("git", "default_where"),
            Some(json!(value))
        );
    }
}

/// REQ: F-067, F-072, PORT:ilium-client/src/app.rs::new_agent_submenu_starts_with_here_and_updates_worktree_availability
#[test]
fn port_072_a_where_value_outside_the_three_choices_is_refused() {
    let mut host = client_host();
    let refused = set_git(&mut host, "default_where", json!("elsewhere"));
    assert!(refused.is_err());
    assert_eq!(
        host.kernel_settings().get("git", "default_where"),
        Some(json!("here"))
    );
}
