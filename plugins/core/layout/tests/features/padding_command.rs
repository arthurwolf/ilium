//! The Layout plugin owns the panel padding and hands the kernel its whole table.

use ilium_plugin_core_layout::padding::{PaddingMode, PaddingSettings};
use ilium_plugin_core_layout::LayoutClient;
use ilium_plugin_sdk::types::{
    Event, Intent, InvokeCommandSpec, PluginSide, SettingsChange, SettingsOrigin, Submission,
};

use crate::common::*;

fn set_padding(submissions: &[Submission]) -> Vec<PaddingSettings> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::InvokeCommand(InvokeCommandSpec {
                command,
                arguments_json,
            }) if command == "kernel.set-padding" => {
                Some(serde_json::from_str(arguments_json).expect("a padding table"))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn the_first_batch_sends_the_stored_padding_to_the_kernel_once() {
    let mut host = client_host().with_settings(serde_json::json!({
        "mode": "enabled",
        "tree_all": 2,
    }));
    host.activate::<LayoutClient>();
    let first = set_padding(&host.deliver_one(ignored_event()));
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].mode, PaddingMode::Enabled);
    assert_eq!(first[0].tree.all, 2);
    assert!(set_padding(&host.deliver_one(ignored_event())).is_empty());
}

#[test]
fn an_edit_of_a_padding_row_sends_the_new_table() {
    let mut host = activated_client();
    host.seed_settings_unchecked("mode", serde_json::json!("width_dependent"));
    let sent = set_padding(&host.deliver_one(Event::SettingsChanged(SettingsChange {
        keys: vec!["mode".to_owned()],
        origin: SettingsOrigin::User,
        side: PluginSide::Client,
    })));
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].mode, PaddingMode::WidthDependent);
    assert_eq!(sent[0].validate(), Ok(()));
}
