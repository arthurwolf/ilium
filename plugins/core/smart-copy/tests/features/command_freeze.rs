//! F-066: the `smart-copy` command freezes the pane while the picker is open and unfreezes it on close.

use ilium_plugin_sdk::types::{Intent, Key};

use crate::common::{
    add_terminal_pane, candidates_message, client_host, intents, menu_command, palette_command,
};

fn key(name: &str) -> ilium_plugin_sdk::types::Event {
    ilium_plugin_sdk::types::Event::Key(Key::named(name))
}

/// REQ: F-066
#[test]
fn feature_066_palette_command_with_a_pane_freezes_it() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    let out = intents(&host.deliver_one(palette_command("smart-copy", Some(pane))));
    assert!(out.iter().any(|intent| matches!(intent, Intent::FreezePane(node) if node.pane.0 == pane)), "{out:?}");
}

/// REQ: F-066
#[test]
fn feature_066_menu_command_with_a_pane_freezes_it() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    let out = intents(&host.deliver_one(menu_command("smart-copy", pane, "smart-copy")));
    assert!(out.iter().any(|intent| matches!(intent, Intent::FreezePane(node) if node.pane.0 == pane)), "{out:?}");
}

/// REQ: F-066
#[test]
fn feature_066_command_without_a_pane_does_not_freeze_anything() {
    let mut host = client_host();
    let out = intents(&host.deliver_one(palette_command("smart-copy", None)));
    assert!(!out.iter().any(|intent| matches!(intent, Intent::FreezePane(_))), "{out:?}");
    assert!(host.failures().is_empty());
}

/// REQ: F-066
#[test]
fn feature_066_command_freezes_exactly_once() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    let out = intents(&host.deliver_one(palette_command("smart-copy", Some(pane))));
    assert_eq!(out.iter().filter(|intent| matches!(intent, Intent::FreezePane(_))).count(), 1);
}

/// REQ: F-066
#[test]
fn feature_066_escape_in_the_picker_unfreezes_the_pane() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.deliver_one(palette_command("smart-copy", Some(pane)));
    host.deliver_one(candidates_message(&["src/a.rs"]));
    let out = intents(&host.deliver_one(key("esc")));
    assert!(out.iter().any(|intent| matches!(intent, Intent::UnfreezePane(node) if node.pane.0 == pane)), "{out:?}");
}

/// REQ: F-066
#[test]
fn feature_066_copying_a_candidate_also_unfreezes_the_pane() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.deliver_one(palette_command("smart-copy", Some(pane)));
    host.deliver_one(candidates_message(&["src/a.rs"]));
    let out = intents(&host.deliver_one(key("1")));
    assert!(out.iter().any(|intent| matches!(intent, Intent::UnfreezePane(_))), "{out:?}");
}

/// REQ: F-066
#[test]
fn feature_066_escape_without_an_open_picker_unfreezes_nothing() {
    let mut host = client_host();
    let out = intents(&host.deliver_one(key("esc")));
    assert!(!out.iter().any(|intent| matches!(intent, Intent::UnfreezePane(_))));
}
