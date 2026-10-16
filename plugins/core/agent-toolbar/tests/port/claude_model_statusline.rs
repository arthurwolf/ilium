//! Twins of claude_model_statusline.rs: capture of the Claude model through a chained statusline setting.
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

/// REQ: PORT:ilium-client/src/claude_model_statusline.rs::project_statusline_setting_chains_user_command_and_restores_only_owned_value
#[test]
fn port_claude_model_statusline_project_statusline_setting_chains_user_command_and_restores_only_owned_value() {
    check(
        Place::Server,
        "statusline.claude.configure",
        r####"{"case": "project_statusline_setting_chains_user_command_and_restores_only_owned_value"}"####,
        r####"{"ok": true}"####,
    );
}

/// REQ: PORT:ilium-client/src/claude_model_statusline.rs::stale_model_observation_from_a_previous_activation_is_ignored
#[test]
fn port_claude_model_statusline_stale_model_observation_from_a_previous_activation_is_ignored() {
    check(
        Place::Server,
        "statusline.claude.observe",
        r####"{"case": "stale_model_observation_from_a_previous_activation_is_ignored"}"####,
        r####"{"observation": null}"####,
    );
}

/// REQ: PORT:ilium-client/src/claude_model_statusline.rs::statusline_input_retains_only_session_project_and_model
#[test]
fn port_claude_model_statusline_statusline_input_retains_only_session_project_and_model() {
    check(
        Place::Server,
        "statusline.claude.input",
        r####"{"extra": ["transcript_path", "prompt"]}"####,
        r####"{"keys": ["session_id", "project", "model_id"]}"####,
    );
}

/// REQ: PORT:ilium-client/src/claude_model_statusline.rs::stored_model_observations_remain_bounded_as_sessions_accumulate
#[test]
fn port_claude_model_statusline_stored_model_observations_remain_bounded_as_sessions_accumulate() {
    check(
        Place::Server,
        "statusline.claude.store",
        r####"{"unrelated_file": "unrelated.txt"}"####,
        r####"{"bounded_at": "MAX_OBSERVATIONS", "unrelated_preserved": true, "newest_kept": true}"####,
    );
}
