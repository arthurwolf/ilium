//! Twins of `ilium-server/src/sounds.rs` and `sound_settings_storage.rs`. The owned byte bank, the admission
//! request and the playback receipt are host internals; the plugin observes the `play-sound` submission, the
//! outcome the host completes it with and the settings it keeps. See API_GAPS_svc01.md.

use ilium_plugin_sdk::types::{AgentActivity, ErrorCode, Outcome, PluginError};
use serde_json::json;

use crate::common::*;

fn failure(code: ErrorCode, message: &str) -> PluginError {
    PluginError {
        code,
        message: message.to_string(),
        retry_after_ms: None,
    }
}

fn first_sound_request(
    host: &mut ilium_plugin_sdk::testing::FakeHost,
) -> ilium_plugin_sdk::types::RequestRef {
    host.pending_requests().remove(0)
}

/// REQ: PORT:ilium-server/src/sound_settings_storage.rs::closed_bank_returns_the_original_settings_allocation
#[test]
fn port_034_a_saved_sound_file_path_reads_back_exactly() {
    let mut host = server_host();
    set_setting(&mut host, "sound.file", json!("/sounds/finished.oga"));
    assert_eq!(
        host.settings().get("sound.file"),
        Some(json!("/sounds/finished.oga"))
    );
}

/// REQ: PORT:ilium-server/src/sound_settings_storage.rs::replacement_keeps_the_original_path_charged_until_the_last_snapshot_drops
#[test]
fn port_035_replacing_the_sound_file_plays_with_the_new_path_and_keeps_the_old_one_restorable() {
    let mut host = server_host();
    set_setting(&mut host, "sound.source", json!("sound_file"));
    set_setting(&mut host, "sound.file", json!("/sounds/one.oga"));
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    assert_eq!(sounds(&finish_turn(&mut host, node, 1)), ["finished"]);
    set_setting(&mut host, "sound.file", json!("/sounds/two.oga"));
    assert_eq!(
        host.settings().get("sound.file"),
        Some(json!("/sounds/two.oga"))
    );
    set_setting(&mut host, "sound.file", json!("/sounds/one.oga"));
    assert_eq!(
        host.settings().get("sound.file"),
        Some(json!("/sounds/one.oga"))
    );
}

/// REQ: PORT:ilium-server/src/sounds.rs::closed_admission_returns_the_exact_original_request
#[test]
fn port_036_a_sound_refused_while_closed_is_not_replayed_later() {
    let mut host = server_host();
    set_setting(&mut host, "notifications.enabled", json!(false));
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    assert_eq!(sounds(&finish_turn(&mut host, node, 1)).len(), 0);
    set_setting(&mut host, "notifications.enabled", json!(true));
    assert_eq!(
        sounds(&host.submitted()).len(),
        0,
        "nothing is replayed on reopening"
    );
    let other = add_pane(&mut host, "claude-two", Some(AgentActivity::Working));
    assert_eq!(sounds(&finish_turn(&mut host, other, 2)), ["finished"]);
}

/// REQ: PORT:ilium-server/src/sounds.rs::generated_sound_is_prepared_before_ordered_playback
#[test]
fn port_037_generated_sounds_play_in_the_order_the_events_were_raised() {
    let mut host = server_host();
    set_setting(&mut host, "sound.source", json!("generated"));
    set_setting(&mut host, "sound.events.approval_required", json!(true));
    let first = add_pane(&mut host, "one", Some(AgentActivity::Working));
    let second = add_pane(&mut host, "two", Some(AgentActivity::Working));
    let mut played = sounds(&transition(
        &mut host,
        first,
        AgentActivity::Working,
        AgentActivity::AwaitingApproval,
        1,
    ));
    played.extend(sounds(&finish_turn(&mut host, second, 2)));
    assert_eq!(played, ["ask", "finished"]);
}

/// REQ: PORT:ilium-server/src/sounds.rs::playback_callback_remains_admitted_after_actor_cancellation
#[test]
fn port_038_a_cancelled_playback_does_not_stop_later_sounds() {
    let mut host = server_host();
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    finish_turn(&mut host, node, 1);
    let request = first_sound_request(&mut host);
    host.complete(request, Outcome::Cancelled);
    host.settle();
    let other = add_pane(&mut host, "claude-two", Some(AgentActivity::Working));
    assert_eq!(sounds(&finish_turn(&mut host, other, 2)), ["finished"]);
}

/// REQ: PORT:ilium-server/src/sounds.rs::preparation_refusal_preserves_the_borrowed_settings_and_event
#[test]
fn port_039_a_refused_sound_leaves_the_settings_and_the_next_event_intact() {
    let mut host = server_host();
    set_setting(&mut host, "sound.source", json!("sound_file"));
    set_setting(&mut host, "sound.file", json!("/sounds/one.oga"));
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    finish_turn(&mut host, node, 1);
    let request = first_sound_request(&mut host);
    host.complete(
        request,
        Outcome::Refused(failure(ErrorCode::Busy, "preparation refused")),
    );
    host.settle();
    assert_eq!(
        host.settings().get("sound.file"),
        Some(json!("/sounds/one.oga"))
    );
    assert_eq!(
        host.settings().get("sound.source"),
        Some(json!("sound_file"))
    );
    let other = add_pane(&mut host, "claude-two", Some(AgentActivity::Working));
    assert_eq!(sounds(&finish_turn(&mut host, other, 2)), ["finished"]);
}

/// REQ: PORT:ilium-server/src/sounds.rs::preview_completion_follows_the_playback_receipt
#[test]
fn port_040_a_successful_playback_receipt_logs_no_failure() {
    let mut host = server_host();
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    finish_turn(&mut host, node, 1);
    let request = first_sound_request(&mut host);
    host.complete(request, Outcome::Ok);
    host.settle();
    assert_eq!(host.failures().len(), 0);
    assert_eq!(
        host.logs()
            .iter()
            .filter(|line| line.level == "error")
            .count(),
        0
    );
}

/// REQ: PORT:ilium-server/src/sounds.rs::preview_failure_is_sent_after_the_playback_receipt_fails
#[test]
fn port_041_a_failed_playback_receipt_is_logged_after_the_receipt() {
    let mut host = server_host();
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    finish_turn(&mut host, node, 1);
    assert_eq!(
        host.logs()
            .iter()
            .filter(|line| line.contains("sound"))
            .count(),
        0
    );
    let request = first_sound_request(&mut host);
    host.complete(
        request,
        Outcome::Failed(failure(ErrorCode::Io, "no audio device")),
    );
    host.settle();
    assert_eq!(
        host.logs()
            .iter()
            .filter(|line| line.contains("sound"))
            .count(),
        1
    );
}

/// REQ: PORT:ilium-server/src/sounds.rs::queue_refusal_retains_original_bytes_until_the_error_is_disposed
#[test]
fn port_042_a_queue_refusal_keeps_the_configured_sound_for_the_next_event() {
    let mut host = server_host();
    set_setting(&mut host, "sound.source", json!("sound_file"));
    set_setting(&mut host, "sound.file", json!("/sounds/one.oga"));
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    finish_turn(&mut host, node, 1);
    let request = first_sound_request(&mut host);
    host.complete(
        request,
        Outcome::Refused(failure(ErrorCode::QuotaExceeded, "queue full")),
    );
    host.settle();
    assert_eq!(
        host.settings().get("sound.file"),
        Some(json!("/sounds/one.oga"))
    );
    let other = add_pane(&mut host, "claude-two", Some(AgentActivity::Working));
    assert_eq!(sounds(&finish_turn(&mut host, other, 2)), ["finished"]);
}
