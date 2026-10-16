//! Twins of antigravity_model_statusline.rs: capture of the Antigravity model through the statusline setting, with byte-exact restore.
//! Request/reply shape: see API_GAPS_ports.md.
use serde_json::Value;
use crate::common::{ask, Place};

fn check(place: Place, op: &str, request: &str, expected: &str) {
    let request: Value = serde_json::from_str(request).expect("request literal parses");
    let expected: Value = serde_json::from_str(expected).expect("expected literal parses");
    let reply = ask(place, op, request);
    let fields = expected.as_object().expect("expected is an object");
    for (name, value) in fields {
        assert_eq!(&reply[name.as_str()], value, "field {name} of reply {reply}");
    }
}

/// REQ: PORT:ilium-client/src/antigravity_model_statusline.rs::conversation_ids_cannot_escape_the_observation_directory
#[test]
fn port_antigravity_model_statusline_conversation_ids_cannot_escape_the_observation_directory() {
    check(
        Place::Server,
        "statusline.antigravity.case",
        r####"{"conversation_id": "../../settings.json"}"####,
        r####"{"valid": false}"####,
    );
}

/// REQ: PORT:ilium-client/src/antigravity_model_statusline.rs::disable_restores_dropped_fields_when_statusline_off_keeps_extra_metadata
#[test]
fn port_antigravity_model_statusline_disable_restores_dropped_fields_when_statusline_off_keeps_extra_metadata() {
    check(
        Place::Server,
        "statusline.antigravity.case",
        r####"{"case": "disable_restores_dropped_fields_when_statusline_off_keeps_extra_metadata"}"####,
        r####"{"ok": true}"####,
    );
}

/// REQ: PORT:ilium-client/src/antigravity_model_statusline.rs::disable_restores_enabled_saved_statusline_after_cli_normalizes_it
#[test]
fn port_antigravity_model_statusline_disable_restores_enabled_saved_statusline_after_cli_normalizes_it() {
    check(
        Place::Server,
        "statusline.antigravity.case",
        r####"{"case": "disable_restores_enabled_saved_statusline_after_cli_normalizes_it"}"####,
        r####"{"ok": true}"####,
    );
}

/// REQ: PORT:ilium-client/src/antigravity_model_statusline.rs::disable_restores_saved_fields_after_statusline_off_drops_them
#[test]
fn port_antigravity_model_statusline_disable_restores_saved_fields_after_statusline_off_drops_them() {
    check(
        Place::Server,
        "statusline.antigravity.case",
        r####"{"case": "disable_restores_saved_fields_after_statusline_off_drops_them"}"####,
        r####"{"ok": true}"####,
    );
}

/// REQ: PORT:ilium-client/src/antigravity_model_statusline.rs::disable_restores_saved_statusline_json_after_live_command_switch
#[test]
fn port_antigravity_model_statusline_disable_restores_saved_statusline_json_after_live_command_switch() {
    check(
        Place::Server,
        "statusline.antigravity.case",
        r####"{"case": "disable_restores_saved_statusline_json_after_live_command_switch"}"####,
        r####"{"ok": true}"####,
    );
}

/// REQ: PORT:ilium-client/src/antigravity_model_statusline.rs::disabling_capture_preserves_statusline_changes_made_by_the_user
#[test]
fn port_antigravity_model_statusline_disabling_capture_preserves_statusline_changes_made_by_the_user() {
    check(
        Place::Server,
        "statusline.antigravity.case",
        r####"{"case": "disabling_capture_preserves_statusline_changes_made_by_the_user"}"####,
        r####"{"ok": true}"####,
    );
}

/// REQ: PORT:ilium-client/src/antigravity_model_statusline.rs::live_statusline_actions_preserve_enabled_prior_commands_without_reactivating_disabled_ones
#[test]
fn port_antigravity_model_statusline_live_statusline_actions_preserve_enabled_prior_commands_without_reactivating_disabled_ones() {
    check(
        Place::Server,
        "statusline.antigravity.case",
        r####"{"case": "live_statusline_actions_preserve_enabled_prior_commands_without_reactivating_disabled_ones"}"####,
        r####"{"ok": true}"####,
    );
}

/// REQ: PORT:ilium-client/src/antigravity_model_statusline.rs::observations_are_scoped_to_conversation_and_project
#[test]
fn port_antigravity_model_statusline_observations_are_scoped_to_conversation_and_project() {
    check(
        Place::Server,
        "statusline.antigravity.case",
        r####"{"model": "Gemini 3.5 Flash (High)", "case": "observations_are_scoped_to_conversation_and_project"}"####,
        r####"{"model_for_same_scope": "Gemini 3.5 Flash (High)", "model_for_other_project": null}"####,
    );
}

/// REQ: PORT:ilium-client/src/antigravity_model_statusline.rs::old_observation_is_rejected_after_model_icons_are_disabled_and_reenabled
#[test]
fn port_antigravity_model_statusline_old_observation_is_rejected_after_model_icons_are_disabled_and_reenabled() {
    check(
        Place::Server,
        "statusline.antigravity.case",
        r####"{"case": "old_observation_is_rejected_after_model_icons_are_disabled_and_reenabled"}"####,
        r####"{"stale_observation": null}"####,
    );
}

/// REQ: PORT:ilium-client/src/antigravity_model_statusline.rs::statusline_configuration_chains_and_restores_an_existing_command
#[test]
fn port_antigravity_model_statusline_statusline_configuration_chains_and_restores_an_existing_command() {
    check(
        Place::Server,
        "statusline.antigravity.case",
        r####"{"case": "statusline_configuration_chains_and_restores_an_existing_command"}"####,
        r####"{"ok": true}"####,
    );
}

/// REQ: PORT:ilium-client/src/antigravity_model_statusline.rs::statusline_configuration_restores_previously_disabled_command_byte_for_byte
#[test]
fn port_antigravity_model_statusline_statusline_configuration_restores_previously_disabled_command_byte_for_byte() {
    check(
        Place::Server,
        "statusline.antigravity.case",
        r####"{"case": "statusline_configuration_restores_previously_disabled_command_byte_for_byte"}"####,
        r####"{"ok": true}"####,
    );
}

/// REQ: PORT:ilium-client/src/antigravity_model_statusline.rs::statusline_configuration_stacks_with_default_and_removes_its_own_key
#[test]
fn port_antigravity_model_statusline_statusline_configuration_stacks_with_default_and_removes_its_own_key() {
    check(
        Place::Server,
        "statusline.antigravity.case",
        r####"{"case": "statusline_configuration_stacks_with_default_and_removes_its_own_key"}"####,
        r####"{"ok": true}"####,
    );
}

/// REQ: PORT:ilium-client/src/antigravity_model_statusline.rs::statusline_helper_does_not_capture_a_model_while_setting_is_disabled
#[test]
fn port_antigravity_model_statusline_statusline_helper_does_not_capture_a_model_while_setting_is_disabled() {
    check(
        Place::Server,
        "statusline.antigravity.case",
        r####"{"case": "statusline_helper_does_not_capture_a_model_while_setting_is_disabled"}"####,
        r####"{"captured": false}"####,
    );
}

/// REQ: PORT:ilium-client/src/antigravity_model_statusline.rs::statusline_payload_rejects_conflicting_session_aliases
#[test]
fn port_antigravity_model_statusline_statusline_payload_rejects_conflicting_session_aliases() {
    check(
        Place::Server,
        "statusline.antigravity.case",
        r####"{"case": "statusline_payload_rejects_conflicting_session_aliases"}"####,
        r####"{"rejected": true}"####,
    );
}

/// REQ: PORT:ilium-client/src/antigravity_model_statusline.rs::statusline_payload_retains_only_project_conversation_and_model
#[test]
fn port_antigravity_model_statusline_statusline_payload_retains_only_project_conversation_and_model() {
    check(
        Place::Server,
        "statusline.antigravity.case",
        r####"{"case": "statusline_payload_retains_only_project_conversation_and_model"}"####,
        r####"{"keys": ["project", "conversation", "model"]}"####,
    );
}
