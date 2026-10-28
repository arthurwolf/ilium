//! Twins of `ilium-client/src/agent_message_dialog.rs`. The old widget had a recipient checklist,
//! a press-Enter option and mouse hit-testing. The plugin page specifies only the target summary
//! line, the message field and the Send and Cancel buttons, so only that part has a twin.

use ilium_plugin_sdk::types::ViewTarget;

use crate::common::*;

fn open() -> ilium_plugin_sdk::testing::FakeHost {
    let mut host = client_host();
    host.deliver_one(ilium_plugin_sdk::types::Event::CommandInvoked(
        ilium_plugin_sdk::types::CommandInvokedEvent {
            command: "ask-for-update".to_string(),
            arguments_json: "{}".to_string(),
            origin: ilium_plugin_sdk::types::CommandOrigin::Palette,
            context: ilium_plugin_sdk::types::CommandContext::default(),
        },
    ));
    host.settle();
    host
}

/// REQ: PORT:ilium-client/src/agent_message_dialog.rs::all_recipients_start_selected_and_plain_enter_edits_message
#[test]
fn agent_message_dialog_all_recipients_start_selected_and_message_field_is_present() {
    let mut host = open();
    let text = host
        .render(ViewTarget::Popup("ask-for-update".to_string()), 80, 24)
        .text();
    assert!(text.contains("All agents"), "{text}");
    assert!(text.contains("Message"), "{text}");
}

/// REQ: PORT:ilium-client/src/agent_message_dialog.rs::all_recipients_start_selected_and_plain_enter_edits_message
#[test]
fn agent_message_dialog_offers_send_and_cancel_buttons() {
    let mut host = open();
    let text = host
        .render(ViewTarget::Popup("ask-for-update".to_string()), 80, 24)
        .text();
    assert!(text.contains("Send"), "{text}");
    assert!(text.contains("Cancel"), "{text}");
}
