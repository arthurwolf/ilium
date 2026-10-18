//! Events handled, commands run and intents emitted by the plugin, and the activation behaviour.

use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::{
    CommandContext, CommandInvokedEvent, CommandOrigin, KernelSettingsEvent,
};
use proptest::prelude::*;

use crate::common::*;

fn invoke(
    host: &mut ilium_plugin_sdk::testing::FakeHost,
    command: &str,
    origin: CommandOrigin,
) -> Vec<Submission> {
    host.deliver_one(Event::CommandInvoked(CommandInvokedEvent {
        command: command.to_string(),
        arguments_json: "{}".to_string(),
        origin,
        context: CommandContext::default(),
    }))
}

fn opened_views(submissions: &[Submission]) -> Vec<ViewTarget> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::OpenView(spec) => Some(spec.view.clone()),
            _ => None,
        })
        .collect()
}

/// REQ: F-010
#[test]
fn events_010_activation_succeeds_with_the_manifest_grants() {
    let mut host = FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("client")
        .build();
    let handle = host.activate::<ilium_plugin_core_keys_and_help::KeysAndHelpClient>();
    assert_eq!(handle.side, "client");
}

use ilium_plugin_sdk::testing::FakeHost;

/// REQ: F-010
#[test]
fn events_010_activation_emits_no_intents_and_no_failures() {
    let host = client_host();
    assert!(host.submitted().is_empty());
    assert!(host.failures().is_empty());
}

/// REQ: F-010
#[test]
fn events_010_show_keys_opens_the_keys_panel_with_focus() {
    let mut host = client_host();
    let submissions = invoke(
        &mut host,
        &format!("{PLUGIN_ID}/show-keys"),
        CommandOrigin::Palette,
    );
    assert_eq!(
        opened_views(&submissions),
        vec![ViewTarget::Panel("keys".to_string())]
    );
    assert!(submissions
        .iter()
        .all(|submission| match &submission.intent {
            Intent::OpenView(spec) => spec.focus,
            _ => true,
        }));
}

/// REQ: F-007, F-010
#[test]
fn events_007_the_prefix_question_mark_keybinding_runs_show_keys() {
    let mut host = client_host();
    let submissions = invoke(
        &mut host,
        &format!("{PLUGIN_ID}/show-keys"),
        CommandOrigin::Keybinding,
    );
    assert_eq!(opened_views(&submissions).len(), 1);
}

/// REQ: F-010
#[test]
fn events_010_show_plugins_opens_the_plugins_panel() {
    let mut host = client_host();
    let submissions = invoke(
        &mut host,
        &format!("{PLUGIN_ID}/show-plugins"),
        CommandOrigin::Palette,
    );
    assert_eq!(
        opened_views(&submissions),
        vec![ViewTarget::Panel("plugins".to_string())]
    );
}

/// REQ: F-010
#[test]
fn events_010_an_unknown_command_of_this_plugin_emits_nothing() {
    let mut host = client_host();
    let submissions = invoke(
        &mut host,
        &format!("{PLUGIN_ID}/does-not-exist"),
        CommandOrigin::Cli,
    );
    assert!(submissions.is_empty());
    assert!(host.failures().is_empty());
}

/// REQ: F-010
#[test]
fn events_010_commands_of_other_plugins_are_ignored() {
    let mut host = client_host();
    assert!(invoke(&mut host, "ilium.core.fleet/open", CommandOrigin::Palette).is_empty());
}

/// REQ: F-010
#[test]
fn events_010_the_bare_local_id_is_not_a_command_of_this_plugin() {
    let mut host = client_host();
    assert!(invoke(&mut host, "show-keys", CommandOrigin::Palette).is_empty());
}

/// REQ: F-010
#[test]
fn events_010_a_batch_of_two_commands_yields_both_panels_in_order() {
    let mut host = client_host();
    let submissions = host.deliver(vec![
        Event::CommandInvoked(CommandInvokedEvent {
            command: format!("{PLUGIN_ID}/show-keys"),
            arguments_json: "{}".to_string(),
            origin: CommandOrigin::Palette,
            context: CommandContext::default(),
        }),
        Event::CommandInvoked(CommandInvokedEvent {
            command: format!("{PLUGIN_ID}/show-plugins"),
            arguments_json: "{}".to_string(),
            origin: CommandOrigin::Palette,
            context: CommandContext::default(),
        }),
    ]);
    assert_eq!(
        opened_views(&submissions),
        vec![
            ViewTarget::Panel("keys".to_string()),
            ViewTarget::Panel("plugins".to_string())
        ]
    );
}

/// REQ: F-010
#[test]
fn events_010_an_empty_batch_yields_no_submissions() {
    let mut host = client_host();
    assert!(host.deliver(Vec::new()).is_empty());
}

/// REQ: F-010
#[test]
fn events_010_the_open_view_correlations_are_distinct_within_a_batch() {
    let mut host = client_host();
    let events: Vec<Event> = (0..2)
        .map(|_| {
            Event::CommandInvoked(CommandInvokedEvent {
                command: format!("{PLUGIN_ID}/show-keys"),
                arguments_json: "{}".to_string(),
                origin: CommandOrigin::Palette,
                context: CommandContext::default(),
            })
        })
        .collect();
    let submissions = host.deliver(events);
    let mut correlations: Vec<u64> = submissions
        .iter()
        .map(|submission| submission.correlation)
        .collect();
    correlations.sort_unstable();
    correlations.dedup();
    assert_eq!(correlations.len(), submissions.len());
}

/// REQ: F-007
#[test]
fn events_007_kernel_settings_changed_of_the_keyboard_tab_does_not_emit_intents() {
    let mut host = seeded_host();
    let submissions = host.deliver_one(Event::KernelSettingsChanged(KernelSettingsEvent {
        tab: "keyboard".to_string(),
        keys: vec!["shortcut_base".to_string()],
    }));
    assert!(submissions.is_empty());
}

/// REQ: F-007
#[test]
fn events_007_kernel_settings_changed_with_no_keys_is_harmless() {
    let mut host = seeded_host();
    host.deliver_one(Event::KernelSettingsChanged(KernelSettingsEvent {
        tab: "keyboard".to_string(),
        keys: Vec::new(),
    }));
    host.settle();
    assert!(host.failures().is_empty());
}

/// REQ: F-010
#[test]
fn events_010_unhandled_events_do_not_fail_the_plugin() {
    let mut host = client_host();
    host.deliver(vec![
        Event::ThemeChanged,
        Event::PaneFocused(None),
        Event::DisplayModeChanged(None),
    ]);
    host.settle();
    assert!(host.failures().is_empty());
    assert!(!host.is_disabled());
}

/// REQ: F-007, F-010
#[test]
fn events_010_deactivation_returns_no_state_to_restore() {
    let mut host = client_host();
    assert!(host.deactivate(DeactivationReason::Shutdown).is_none());
}

proptest! {
    // REQ: F-010
    #[test]
    fn events_010_property_commands_of_other_plugins_never_produce_submissions(suffix in "[a-z]{1,12}") {
        let mut host = client_host();
        let submissions = invoke(&mut host, &format!("com.example.other/{suffix}"), CommandOrigin::Palette);
        prop_assert!(submissions.is_empty());
    }
}
