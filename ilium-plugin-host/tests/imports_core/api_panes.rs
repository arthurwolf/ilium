//! api-panes.md: `panes.info` and `panes.focused`, with the `panes.read` grant and pane lifecycle.

use ilium_plugin_api::generated::host::HostPanes;
use ilium_plugin_api::generated::types::NodeId;
use ilium_plugin_api::Tier;

use crate::support::*;

const READ: &str = "panes.read";

/// REQ: WIT:ilium:host/panes.info
#[test]
fn panes_info_returns_the_pane_description() {
    let mut fixture = fixture(Tier::T2, &[READ]);
    let pane = fixture.add_terminal_pane("build", 80, 24);
    let info = fixture.panes().info(pane).expect("pane info");
    assert_eq!(info.node, pane);
    assert_eq!((info.columns, info.rows), (80, 24));
    assert_eq!(info.title, "build");
    assert!(!info.is_restored);
}

/// REQ: WIT:ilium:host/panes.info
#[test]
fn panes_info_unknown_node_is_none() {
    let mut fixture = fixture(Tier::T2, &[READ]);
    assert!(fixture.panes().info(NodeId(9_999_999)).is_none());
}

/// REQ: WIT:ilium:host/panes.info
#[test]
fn panes_info_node_that_is_not_a_pane_is_none() {
    let mut fixture = fixture(Tier::T2, &[READ]);
    let group = fixture.add_group("folder");
    assert!(fixture.panes().info(group).is_none());
}

/// REQ: WIT:ilium:host/panes.info
#[test]
fn panes_info_without_the_grant_is_none_and_logs_one_warning_naming_the_grant() {
    let mut fixture = fixture(Tier::T2, &[]);
    let pane = fixture.add_terminal_pane("build", 80, 24);
    assert!(fixture.panes().info(pane).is_none());
    assert!(fixture.panes().info(pane).is_none());
    let warnings: Vec<_> = fixture
        .logs()
        .into_iter()
        .filter(|line| line.level == "warn" && line.message.contains(READ))
        .collect();
    assert_eq!(warnings.len(), 1, "only the first refused call is logged");
    assert!(warnings[0].message.contains("declined"));
}

/// REQ: WIT:ilium:host/panes.info
#[test]
fn panes_info_an_exited_pane_keeps_its_last_title_and_size() {
    let mut fixture = fixture(Tier::T2, &[READ]);
    let pane = fixture.add_terminal_pane("job", 100, 30);
    fixture.exit_pane(pane);
    let info = fixture
        .panes()
        .info(pane)
        .expect("an exited pane is still described");
    assert_eq!(
        (info.columns, info.rows, info.title.as_str()),
        (100, 30, "job")
    );
    assert!(!info.is_restored);
}

/// REQ: WIT:ilium:host/panes.info
#[test]
fn panes_info_a_restored_pane_is_flagged_and_has_a_non_zero_size() {
    let mut fixture = fixture(Tier::T2, &[READ]);
    let pane = fixture.add_restored_pane("old", 90, 20);
    let info = fixture.panes().info(pane).expect("restored pane");
    assert!(info.is_restored);
    assert!(info.columns > 0 && info.rows > 0);
}

/// REQ: WIT:ilium:host/panes.info
#[test]
fn panes_info_focused_flag_follows_the_focused_pane() {
    let mut fixture = fixture(Tier::T2, &[READ]);
    let first = fixture.add_terminal_pane("a", 80, 24);
    let second = fixture.add_terminal_pane("b", 80, 24);
    fixture.focus_pane(second);
    assert!(!fixture.panes().info(first).unwrap().focused);
    assert!(fixture.panes().info(second).unwrap().focused);
}

/// REQ: WIT:ilium:host/panes.focused
#[test]
fn panes_focused_returns_the_focused_pane() {
    let mut fixture = fixture(Tier::T2, &[READ]);
    let pane = fixture.add_terminal_pane("a", 80, 24);
    fixture.focus_pane(pane);
    assert_eq!(fixture.panes().focused(), Some(pane));
}

/// REQ: WIT:ilium:host/panes.focused
#[test]
fn panes_focused_is_none_when_nothing_is_focused() {
    let mut fixture = fixture(Tier::T2, &[READ]);
    assert_eq!(fixture.panes().focused(), None);
}

/// REQ: WIT:ilium:host/panes.focused
#[test]
fn panes_focused_without_the_grant_is_none_and_logs_a_warning() {
    let mut fixture = fixture(Tier::T2, &[]);
    let pane = fixture.add_terminal_pane("a", 80, 24);
    fixture.focus_pane(pane);
    assert_eq!(fixture.panes().focused(), None);
    assert!(fixture
        .logs()
        .iter()
        .any(|line| line.level == "warn" && line.message.contains(READ)));
}

/// REQ: WIT:ilium:host/panes.focused
#[test]
fn panes_focused_moves_when_the_focus_moves() {
    let mut fixture = fixture(Tier::T2, &[READ]);
    let first = fixture.add_terminal_pane("a", 80, 24);
    let second = fixture.add_terminal_pane("b", 80, 24);
    fixture.focus_pane(first);
    assert_eq!(fixture.panes().focused(), Some(first));
    fixture.focus_pane(second);
    assert_eq!(fixture.panes().focused(), Some(second));
}
