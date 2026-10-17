//! Twins of the sound preview tests of `ilium-client/src/app.rs`. The plugin API has no preview
//! request; what survives is the contract the previews rely on: the selected source and its design
//! are kept, and the server does not refuse a source whose file is missing.

use ilium_plugin_sdk::types::AgentActivity;
use serde_json::json;

use crate::common::*;

/// REQ: PORT:ilium-client/src/app.rs::sound_preview_design_publishers_preserve_refusal_and_selected_source
#[test]
fn port_004_the_generated_source_and_its_design_are_kept_and_play_one_sound() {
    let mut host = server_host();
    set_setting(&mut host, "sound.source", json!("generated"));
    set_setting(&mut host, "sound.design.waveform", json!("square"));
    assert_eq!(host.settings().get("sound.source"), Some(json!("generated")));
    assert_eq!(host.settings().get("sound.design.waveform"), Some(json!("square")));
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    assert_eq!(sounds(&finish_turn(&mut host, node, 1)).len(), 1);
}

/// REQ: PORT:ilium-client/src/app.rs::sound_preview_sends_missing_file_validation_to_server
#[test]
fn port_006_a_sound_file_that_does_not_exist_is_still_passed_on_to_play() {
    let mut host = server_host();
    set_setting(&mut host, "sound.source", json!("sound_file"));
    set_setting(&mut host, "sound.file", json!("/nonexistent/ilium-missing.oga"));
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    assert_eq!(sounds(&finish_turn(&mut host, node, 1)), ["finished"]);
}
