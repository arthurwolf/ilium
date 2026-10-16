//! F-016: local text selection in terminal panes. The client claims the pointer of terminal panes,
//! tracks `down`, `drag` and `up`, and leaves other pane kinds alone (smart-copy.md "Other ways to
//! copy", page sample "Select regions with the mouse").

use ilium_plugin_sdk::types::{PointerKind, PointerModifiers, PrimitiveKind};
use serde_json::json;

use crate::common::{
    add_editor_pane, add_terminal_pane, client_host, client_host_unactivated, pane_created, pointer,
};
use ilium_plugin_core_smart_copy::SmartCopyClient;

/// Whole-pane claims of a pane: the entries without a modifier set.
fn whole_claims(host: &mut ilium_plugin_sdk::testing::FakeHost, pane: u64) -> usize {
    host.terminal().pointer_claims(pane).into_iter().filter(Option::is_none).count()
}

/// REQ: F-016
#[test]
fn feature_016_a_new_terminal_pane_gets_a_whole_pointer_claim_while_selection_is_on() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.deliver_one(pane_created(pane, "ilium.terminal", PrimitiveKind::Terminal));
    assert_eq!(whole_claims(&mut host, pane), 1);
}

/// REQ: F-016
#[test]
fn feature_016_no_whole_claim_when_terminal_text_selection_is_off() {
    let mut host = client_host();
    host.settings().set("ui.terminal_text_selection_enabled", json!(false)).expect("boolean");
    let pane = add_terminal_pane(&mut host, "api");
    host.deliver_one(pane_created(pane, "ilium.terminal", PrimitiveKind::Terminal));
    assert_eq!(whole_claims(&mut host, pane), 0);
}

/// REQ: F-016
#[test]
fn feature_016_editor_panes_are_never_claimed() {
    let mut host = client_host();
    let pane = add_editor_pane(&mut host, "notes");
    host.deliver_one(pane_created(pane, "ilium.editor", PrimitiveKind::Editor));
    assert!(host.terminal().pointer_claims(pane).is_empty());
}

/// REQ: F-016
#[test]
fn feature_016_terminal_panes_that_exist_at_activation_are_claimed() {
    let mut host = client_host_unactivated();
    let pane = add_terminal_pane(&mut host, "restored");
    host.activate::<SmartCopyClient>();
    assert_eq!(host.terminal().pointer_claims(pane).len(), 1);
    assert_eq!(whole_claims(&mut host, pane), 1);
}

/// REQ: F-016
#[test]
fn feature_016_a_pane_is_claimed_once_when_the_created_event_repeats() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    let created = pane_created(pane, "ilium.terminal", PrimitiveKind::Terminal);
    host.deliver_one(created.clone());
    host.deliver_one(created);
    assert_eq!(whole_claims(&mut host, pane), 1);
}

/// REQ: F-016
#[test]
fn feature_016_turning_selection_off_releases_the_whole_claim() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.deliver_one(pane_created(pane, "ilium.terminal", PrimitiveKind::Terminal));
    host.settings().set("ui.terminal_text_selection_enabled", json!(false)).expect("boolean");
    host.deliver_one(ilium_plugin_sdk::types::Event::KernelSettingsChanged(
        ilium_plugin_sdk::types::KernelSettingsEvent {
            tab: "ui".to_string(),
            keys: vec!["terminal_text_selection_enabled".to_string()],
        },
    ));
    assert_eq!(whole_claims(&mut host, pane), 0);
}

/// REQ: F-016
#[test]
fn feature_016_turning_selection_on_claims_existing_terminal_panes() {
    let mut host = client_host();
    host.settings().set("ui.terminal_text_selection_enabled", json!(false)).expect("boolean");
    let pane = add_terminal_pane(&mut host, "api");
    host.deliver_one(pane_created(pane, "ilium.terminal", PrimitiveKind::Terminal));
    host.settings().set("ui.terminal_text_selection_enabled", json!(true)).expect("boolean");
    host.deliver_one(ilium_plugin_sdk::types::Event::KernelSettingsChanged(
        ilium_plugin_sdk::types::KernelSettingsEvent {
            tab: "ui".to_string(),
            keys: vec!["terminal_text_selection_enabled".to_string()],
        },
    ));
    assert_eq!(whole_claims(&mut host, pane), 1);
}

/// REQ: F-016
#[test]
fn feature_016_down_drag_up_produce_no_submissions() {
    // Page sample: on_events returns an empty list for every pointer kind it tracks.
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.deliver_one(pane_created(pane, "ilium.terminal", PrimitiveKind::Terminal));
    host.terminal().set_rows(pane, &["$ git status", "  modified: src/lib.rs"]);
    for (kind, row, column) in [(PointerKind::Down, 1, 2), (PointerKind::Drag, 1, 9), (PointerKind::Up, 1, 9)] {
        assert!(host.deliver_one(pointer(pane, kind, row, column)).is_empty(), "{kind:?}");
    }
}

/// REQ: F-016
#[test]
fn feature_016_other_pointer_kinds_are_ignored_without_failure() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.deliver_one(pane_created(pane, "ilium.terminal", PrimitiveKind::Terminal));
    for kind in [
        PointerKind::Move,
        PointerKind::WheelUp,
        PointerKind::WheelDown,
        PointerKind::WheelLeft,
        PointerKind::WheelRight,
    ] {
        assert!(host.deliver_one(pointer(pane, kind, 0, 0)).is_empty(), "{kind:?}");
    }
    assert!(host.failures().is_empty());
}

/// REQ: F-016
#[test]
fn feature_016_a_drag_without_a_press_is_ignored_without_failure() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.deliver_one(pane_created(pane, "ilium.terminal", PrimitiveKind::Terminal));
    assert!(host.deliver_one(pointer(pane, PointerKind::Drag, 3, 3)).is_empty());
    assert!(host.deliver_one(pointer(pane, PointerKind::Up, 3, 3)).is_empty());
    assert!(host.failures().is_empty());
}

/// REQ: F-016
#[test]
fn feature_016_the_selection_claim_does_not_use_a_modifier_set() {
    // Ctrl-drag stays free for Smart Copy light: the selection claim is a whole claim, the
    // light claim is a gesture claim (smart-copy.md "Mouse text selection", "Rules").
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.deliver_one(pane_created(pane, "ilium.terminal", PrimitiveKind::Terminal));
    let claims = host.terminal().pointer_claims(pane);
    assert!(claims.contains(&None));
    assert!(claims.contains(&Some(PointerModifiers::CONTROL)));
}
