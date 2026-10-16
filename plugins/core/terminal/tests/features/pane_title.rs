//! F-018 pane title from the terminal: the page sample "Pane title from the terminal".
//! OSC titles rename the pane with `source: inferred`; a manual name always wins.

use ilium_plugin_sdk::types::{Event, Intent, NodeId, PaneTitleChangedEvent, RenameNodeSpec, TitleOrigin, TitleSource};
use proptest::prelude::*;

use crate::common::{intents, server_host};

fn title_event(node: u64, title: &str, source: TitleOrigin) -> Event {
    Event::PaneTitleChanged(PaneTitleChangedEvent { node: NodeId(node), title: title.to_string(), source })
}

fn rename(node: u64, title: &str) -> Intent {
    Intent::RenameNode(RenameNodeSpec { node: NodeId(node), title: title.to_string(), source: TitleSource::Inferred, expected_revision: None })
}

/// REQ: F-018
#[test]
fn feature_018_an_osc_title_renames_the_pane_as_inferred() {
    let mut host = server_host();
    let submissions = host.deliver(vec![title_event(7, "cargo test", TitleOrigin::Osc)]);
    assert_eq!(intents(&submissions), vec![rename(7, "cargo test")]);
}

/// REQ: F-018
#[test]
fn feature_018_the_rename_is_never_marked_as_a_plugin_name() {
    let mut host = server_host();
    let submissions = host.deliver(vec![title_event(7, "vim", TitleOrigin::Osc)]);
    match &submissions[0].intent {
        Intent::RenameNode(spec) => assert_eq!(spec.source, TitleSource::Inferred),
        other => panic!("unexpected intent {other:?}"),
    }
}

/// REQ: F-018
#[test]
fn feature_018_the_rename_carries_no_expected_revision() {
    let mut host = server_host();
    let submissions = host.deliver(vec![title_event(7, "vim", TitleOrigin::Osc)]);
    match &submissions[0].intent {
        Intent::RenameNode(spec) => assert_eq!(spec.expected_revision, None),
        other => panic!("unexpected intent {other:?}"),
    }
}

/// REQ: F-018
#[test]
fn feature_018_a_manual_title_is_never_renamed() {
    let mut host = server_host();
    assert!(host.deliver(vec![title_event(7, "my build", TitleOrigin::Manual)]).is_empty());
}

/// REQ: F-018
#[test]
fn feature_018_an_inferred_title_is_never_renamed_again() {
    let mut host = server_host();
    assert!(host.deliver(vec![title_event(7, "guess", TitleOrigin::Inferred)]).is_empty());
}

/// REQ: F-018
#[test]
fn feature_018_the_rename_is_keyed_by_the_pane_so_repeats_collapse() {
    let mut host = server_host();
    let submissions = host.deliver(vec![title_event(7, "a", TitleOrigin::Osc), title_event(7, "b", TitleOrigin::Osc)]);
    assert_eq!(submissions.len(), 2);
    assert_eq!(submissions[0].dedup_key.as_deref(), Some("osc-title-7"));
    assert_eq!(submissions[1].dedup_key.as_deref(), Some("osc-title-7"));
}

/// REQ: F-018
#[test]
fn feature_018_different_panes_get_different_keys() {
    let mut host = server_host();
    let submissions = host.deliver(vec![title_event(7, "a", TitleOrigin::Osc), title_event(8, "a", TitleOrigin::Osc)]);
    assert_eq!(submissions[0].dedup_key.as_deref(), Some("osc-title-7"));
    assert_eq!(submissions[1].dedup_key.as_deref(), Some("osc-title-8"));
}

/// REQ: F-018
#[test]
fn feature_018_titles_of_several_panes_rename_in_event_order() {
    let mut host = server_host();
    let submissions = host.deliver(vec![
        title_event(9, "nine", TitleOrigin::Osc),
        title_event(2, "two", TitleOrigin::Manual),
        title_event(4, "four", TitleOrigin::Osc),
    ]);
    assert_eq!(intents(&submissions), vec![rename(9, "nine"), rename(4, "four")]);
}

/// REQ: F-018
#[test]
fn feature_018_an_empty_osc_title_is_passed_on_unchanged() {
    let mut host = server_host();
    let submissions = host.deliver(vec![title_event(7, "", TitleOrigin::Osc)]);
    assert_eq!(intents(&submissions), vec![rename(7, "")]);
}

/// REQ: F-018
#[test]
fn feature_018_unicode_titles_survive_unchanged() {
    let mut host = server_host();
    let title = "build \u{2714} \u{65e5}\u{672c}\u{8a9e}";
    let submissions = host.deliver(vec![title_event(7, title, TitleOrigin::Osc)]);
    assert_eq!(intents(&submissions), vec![rename(7, title)]);
}

/// REQ: F-018
#[test]
fn feature_018_an_empty_batch_submits_nothing() {
    let mut host = server_host();
    assert!(host.deliver(Vec::new()).is_empty());
}

proptest! {
    // REQ: F-018
    #[test]
    fn feature_018_every_osc_title_becomes_one_inferred_rename_with_the_same_text(node in 1u64..10_000, title in ".{0,80}") {
        let mut host = server_host();
        let submissions = host.deliver(vec![title_event(node, &title, TitleOrigin::Osc)]);
        prop_assert_eq!(intents(&submissions), vec![rename(node, &title)]);
    }
}
