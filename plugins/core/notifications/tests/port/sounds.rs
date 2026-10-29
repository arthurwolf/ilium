//! Ports of the sound tests of ilium-server/src/sounds.rs and of the notification setting rows of
//! ilium-client/src/app.rs, observed through the plugin's settings and `play-sound` submissions.
//!
//! Skipped as host internals with no plugin-API channel (see the result notes): the bounded
//! execution bank, admission and shutdown behaviour, playback receipts, the client outbox, preview
//! requests and their status line, and the tree and tooltip renderings.

use ilium_plugin_sdk::types::AgentActivity;
use serde_json::json;

use crate::common::*;

/// REQ: PORT:ilium-server/src/sounds.rs::actor_forwards_requests_in_order_without_real_audio
#[test]
fn sounds_port_alerts_are_played_in_the_order_they_were_raised() {
    let mut host = server_host();
    set_setting(&mut host, "sound.events.approval_required", json!(true));
    let first = add_pane(&mut host, "one", Some(AgentActivity::Working));
    let second = add_pane(&mut host, "two", Some(AgentActivity::Working));
    let mut played = sounds(&finish_turn(&mut host, first, 1));
    played.extend(sounds(&transition(
        &mut host,
        second,
        AgentActivity::Working,
        AgentActivity::AwaitingApproval,
        2,
    )));
    assert_eq!(played, ["finished", "ask"]);
}

/// REQ: PORT:ilium-server/src/sounds.rs::config_watcher_updates_an_already_running_server
#[test]
fn sounds_port_a_changed_sound_table_applies_to_the_running_plugin() {
    let mut host = server_host();
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    assert!(sounds(&transition(
        &mut host,
        node,
        AgentActivity::Idle,
        AgentActivity::AwaitingApproval,
        1
    ))
    .is_empty());
    set_setting(&mut host, "sound.events.approval_required", json!(true));
    let played = sounds(&transition(
        &mut host,
        node,
        AgentActivity::Idle,
        AgentActivity::AwaitingApproval,
        2,
    ));
    assert_eq!(played, ["ask"]);
}

/// REQ: PORT:ilium-client/src/app.rs::notification_rows_persist_every_toggle_and_stepper_without_an_ipc_update
#[test]
fn sounds_port_every_notification_row_persists_and_reads_back() {
    let mut host = server_host();
    assert_eq!(
        host.settings().get("notifications.task_succeeded"),
        Some(json!(false))
    );
    for key in [
        "notifications.task_succeeded",
        "notifications.task_failed",
        "notifications.agent_finished",
        "notifications.approval_required",
        "notifications.suppress_redundant_task_outcomes",
        "notifications.enabled",
    ] {
        let before = host
            .settings()
            .get(key)
            .and_then(|value| value.as_bool())
            .expect(key);
        set_setting(&mut host, key, json!(!before));
        assert_eq!(host.settings().get(key), Some(json!(!before)), "{key}");
    }
    // Final state of the original: task_succeeded on, every other row flipped off.
    assert_eq!(
        host.settings().get("notifications.task_succeeded"),
        Some(json!(true))
    );
    assert_eq!(
        host.settings().get("notifications.task_failed"),
        Some(json!(false))
    );
    assert_eq!(
        host.settings().get("notifications.agent_finished"),
        Some(json!(false))
    );
    assert_eq!(
        host.settings().get("notifications.approval_required"),
        Some(json!(false))
    );
    assert_eq!(
        host.settings()
            .get("notifications.suppress_redundant_task_outcomes"),
        Some(json!(false))
    );
    assert_eq!(
        host.settings().get("notifications.enabled"),
        Some(json!(false))
    );
}

/// REQ: PORT:ilium-client/src/app.rs::notification_rows_persist_every_toggle_and_stepper_without_an_ipc_update
#[test]
fn sounds_port_the_merge_window_steps_by_ten_seconds_from_thirty() {
    let mut host = server_host();
    assert_eq!(
        host.settings().get("notifications.task_coalesce_seconds"),
        Some(json!(30))
    );
    for step in [40, 50, 40] {
        set_setting(
            &mut host,
            "notifications.task_coalesce_seconds",
            json!(step),
        );
        assert_eq!(
            host.settings().get("notifications.task_coalesce_seconds"),
            Some(json!(step))
        );
    }
}

/// REQ: PORT:ilium-client/src/app.rs::sound_changes_persist_and_queue_one_live_server_update
#[test]
fn sounds_port_a_sound_file_choice_persists_and_reaches_the_running_plugin() {
    let mut host = server_host();
    set_setting(
        &mut host,
        "sound.file",
        json!("/usr/share/sounds/complete.oga"),
    );
    set_setting(&mut host, "sound.source", json!("sound_file"));
    assert_eq!(
        host.settings().get("sound.source"),
        Some(json!("sound_file"))
    );
    assert_eq!(
        host.settings().get("sound.file"),
        Some(json!("/usr/share/sounds/complete.oga"))
    );
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    assert_eq!(sounds(&finish_turn(&mut host, node, 1)).len(), 1);
}

/// REQ: PORT:ilium-client/src/app.rs::sound_preview_muted_does_not_publish_a_request
#[test]
fn sounds_port_a_muted_source_plays_nothing() {
    let mut host = server_host();
    set_setting(&mut host, "sound.source", json!("muted"));
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    assert!(sounds(&finish_turn(&mut host, node, 1)).is_empty());
}

/// REQ: PORT:ilium-client/src/render_cache.rs::agent_finished_trigger_uses_the_sound_transition_classifier
#[test]
fn sounds_port_a_busy_agent_reaching_done_is_agent_finished_and_busy_to_idle_is_not() {
    let mut host = server_host();
    let node = add_pane(&mut host, "codex", Some(AgentActivity::Working));
    assert_eq!(
        sounds(&transition(
            &mut host,
            node,
            AgentActivity::Working,
            AgentActivity::Done,
            1
        )),
        ["finished"]
    );
    let other = add_pane_signature(&mut host, "codex-2", "codex", AgentActivity::Working);
    assert!(sounds(&transition(
        &mut host,
        other,
        AgentActivity::Working,
        AgentActivity::Idle,
        2
    ))
    .is_empty());
}
