//! Twins of ilium-client/src/session_stats_popover.rs: the statistics icon, hover preview, pin and tab handling.
//! Scenario ops `popover.<name-with-dashes>` replay the mouse sequence of the original and answer the key
//! observable values it asserted (see ../API_GAPS_ports.md).

use serde_json::{json, Value};

use crate::common::client;

fn scenario(name: &str, request: Value) -> Value {
    client(&format!("popover.{name}"), request)
}

/// REQ: PORT:ilium-client/src/session_stats_popover.rs::a_preview_survives_the_pointer_moving_onto_the_popover
#[test]
fn port_session_stats_popover_a_preview_survives_the_pointer_moving_onto_the_popover() {
    let reply = scenario(
        "a-preview-survives-the-pointer-moving-onto-the-popover",
        json!({ "events": ["hover_icon", "move_inside_popover"] }),
    );
    assert_eq!(reply["last_event_claimed"], json!(true));
    assert_eq!(reply["popover_open"], json!(true));
}

/// REQ: PORT:ilium-client/src/session_stats_popover.rs::agents_without_a_readable_transcript_are_unsupported
#[test]
fn port_session_stats_popover_agents_without_a_readable_transcript_are_unsupported() {
    let reply = scenario(
        "agents-without-a-readable-transcript-are-unsupported",
        json!({ "agents": ["with_transcript", "without_transcript"] }),
    );
    assert_eq!(reply["supported"], json!([true, false]));
    assert_eq!(reply["refresh_request_for_unsupported"], Value::Null);
}

/// REQ: PORT:ilium-client/src/session_stats_popover.rs::clicking_a_preview_pins_it_and_tabs_switch
#[test]
fn port_session_stats_popover_clicking_a_preview_pins_it_and_tabs_switch() {
    let reply = scenario(
        "clicking-a-preview-pins-it-and-tabs-switch",
        json!({ "events": ["hover_icon", "click_popover", "click_tab_tokens"] }),
    );
    assert_eq!(reply["pinned"], json!(true));
    assert_eq!(reply["tab"], json!("tokens"));
}

/// REQ: PORT:ilium-client/src/session_stats_popover.rs::clicking_the_icon_pins_and_clicking_again_or_close_unpins
#[test]
fn port_session_stats_popover_clicking_the_icon_pins_and_clicking_again_or_close_unpins() {
    let reply = scenario(
        "clicking-the-icon-pins-and-clicking-again-or-close-unpins",
        json!({ "events": ["click_icon", "click_close", "click_icon", "click_icon"] }),
    );
    assert_eq!(reply["pinned_after_first_click"], json!(true));
    assert_eq!(reply["open_after_first_click"], json!(true));
    assert_eq!(
        reply["open_after_close"],
        json!(false),
        "the close control closes it"
    );
    assert_eq!(reply["open_after_reopen"], json!(true));
    assert_eq!(
        reply["open_after_icon_toggle"],
        json!(false),
        "the icon toggles it off"
    );
}

/// REQ: PORT:ilium-client/src/session_stats_popover.rs::crash_keeps_original_stats_after_live_session_map_is_cleared
#[test]
fn port_session_stats_popover_crash_keeps_original_stats_after_live_session_map_is_cleared() {
    let reply = scenario(
        "crash-keeps-original-stats-after-live-session-map-is-cleared",
        json!({ "session_id": "original-session", "clear_live_session_map": true }),
    );
    assert_eq!(reply["history_context_known"], json!(true));
    assert_eq!(reply["snapshot_same_before_and_after_clear"], json!(true));
    assert_eq!(reply["tick_changed"], json!(false));
}

/// REQ: PORT:ilium-client/src/session_stats_popover.rs::different_verified_session_hides_old_snapshot_immediately
#[test]
fn port_session_stats_popover_different_verified_session_hides_old_snapshot_immediately() {
    let reply = scenario(
        "different-verified-session-hides-old-snapshot-immediately",
        json!({ "replacement_session": "replacement-session" }),
    );
    assert_eq!(reply["current_entry"], Value::Null);
    assert_eq!(reply["store_entry"], Value::Null);
}

/// REQ: PORT:ilium-client/src/session_stats_popover.rs::events_over_the_popover_never_reach_the_pane
#[test]
fn port_session_stats_popover_events_over_the_popover_never_reach_the_pane() {
    let reply = scenario(
        "events-over-the-popover-never-reach-the-pane",
        json!({ "kinds": ["down", "up", "moved", "scroll_up", "scroll_down", "drag"], "also_outside": true }),
    );
    assert_eq!(
        reply["claimed_inside"],
        json!([true, true, true, true, true, true]),
        "every event kind must be claimed"
    );
    assert_eq!(reply["outside_claimed"], json!(false));
}

/// REQ: PORT:ilium-client/src/session_stats_popover.rs::full_click_sequence_through_the_top_level_dispatcher_pins
#[test]
fn port_session_stats_popover_full_click_sequence_through_the_top_level_dispatcher_pins() {
    let reply = scenario(
        "full-click-sequence-through-the-top-level-dispatcher-pins",
        json!({ "events": ["down", "up"], "through": "top_level_dispatcher" }),
    );
    assert_eq!(
        reply["pinned"],
        json!(true),
        "the dispatcher sequence must leave a pinned popover"
    );
}

/// REQ: PORT:ilium-client/src/session_stats_popover.rs::hovering_the_second_icon_previews_and_leaving_it_closes
#[test]
fn port_session_stats_popover_hovering_the_second_icon_previews_and_leaving_it_closes() {
    let reply = scenario(
        "hovering-the-second-icon-previews-and-leaving-it-closes",
        json!({ "events": ["hover_second_icon", "leave"] }),
    );
    assert_eq!(reply["pane_is_second"], json!(true));
    assert_eq!(reply["pinned_while_hovering"], json!(false));
    assert_eq!(
        reply["open_after_leave"],
        json!(false),
        "leaving closes a preview"
    );
}

/// REQ: PORT:ilium-client/src/session_stats_popover.rs::modal_dialogs_suspend_the_popover_interaction
#[test]
fn port_session_stats_popover_modal_dialogs_suspend_the_popover_interaction() {
    let reply = scenario(
        "modal-dialogs-suspend-the-popover-interaction",
        json!({ "modal_open": true, "pinned_before": true }),
    );
    assert_eq!(reply["event_claimed"], json!(false));
    assert_eq!(
        reply["popover_kept"],
        json!(true),
        "state is kept for when the dialog closes"
    );
}

/// REQ: PORT:ilium-client/src/session_stats_popover.rs::plain_shells_have_no_stats_icon
#[test]
fn port_session_stats_popover_plain_shells_have_no_stats_icon() {
    let reply = scenario(
        "plain-shells-have-no-stats-icon",
        json!({ "pane": "shell" }),
    );
    assert_eq!(reply["icon_pane_at"], Value::Null);
    assert_eq!(reply["event_claimed"], json!(false));
}

/// REQ: PORT:ilium-client/src/session_stats_popover.rs::stopped_agent_stats_dot_opens_pinned_retained_metrics_in_actual_renderer
#[test]
fn port_session_stats_popover_stopped_agent_stats_dot_opens_pinned_retained_metrics_in_actual_renderer(
) {
    let reply = scenario(
        "stopped-agent-stats-dot-opens-pinned-retained-metrics-in-actual-renderer",
        json!({ "provider": "synthetic-retained-stats", "session_id": "original-session", "output_tokens": 991, "stopped": true }),
    );
    assert_eq!(reply["icon_symbol"], json!("●"));
    assert_eq!(reply["click_claimed"], json!(true));
    assert_eq!(reply["pinned"], json!(true));
    assert_eq!(reply["rendered_contains_provider"], json!(true));
    assert_eq!(reply["rendered_contains_991"], json!(true));
    assert_eq!(reply["rendered_waiting_text"], json!(false));
    assert_eq!(
        reply["after_owner_change"]["rendered_contains_provider"],
        json!(false)
    );
    assert_eq!(
        reply["after_owner_change"]["rendered_contains_991"],
        json!(false)
    );
    assert_eq!(
        reply["after_owner_change"]["rendered_waiting_text"],
        json!("Waiting for the agent's session")
    );
}

/// REQ: PORT:ilium-client/src/session_stats_popover.rs::the_tick_closes_a_popover_whose_pane_stopped_being_an_agent
#[test]
fn port_session_stats_popover_the_tick_closes_a_popover_whose_pane_stopped_being_an_agent() {
    let reply = scenario(
        "the-tick-closes-a-popover-whose-pane-stopped-being-an-agent",
        json!({ "pane_becomes_shell": true }),
    );
    assert_eq!(reply["tick_changed"], json!(true));
    assert_eq!(reply["popover_open"], json!(false));
}

/// REQ: PORT:ilium-client/src/session_stats_popover.rs::unresolved_replacement_hides_old_stats_before_tick_and_drops_cache_without_popover
#[test]
fn port_session_stats_popover_unresolved_replacement_hides_old_stats_before_tick_and_drops_cache_without_popover(
) {
    let reply = scenario(
        "unresolved-replacement-hides-old-stats-before-tick-and-drops-cache-without-popover",
        json!({ "replacement_session": "unresolved", "popover": false }),
    );
    assert_eq!(reply["current_entry_before_tick"], Value::Null);
    assert_eq!(reply["tick_changed"], json!(true));
    assert_eq!(reply["store_entry_after_tick"], Value::Null);
}

/// REQ: PORT:ilium-client/src/session_stats_popover.rs::a_preview_survives_the_pointer_moving_onto_the_popover
#[test]
fn port_session_stats_popover_a_preview_closes_when_the_pointer_leaves_the_popover() {
    let reply = scenario(
        "a-preview-survives-the-pointer-moving-onto-the-popover",
        json!({ "events": ["hover_icon", "move_inside_popover", "leave_popover"] }),
    );
    assert_eq!(reply["popover_open"], json!(false));
}

/// REQ: PORT:ilium-client/src/session_stats_popover.rs::agents_without_a_readable_transcript_are_unsupported
#[test]
fn port_session_stats_popover_agents_with_a_transcript_are_all_supported() {
    let reply = scenario(
        "agents-without-a-readable-transcript-are-unsupported",
        json!({ "agents": ["with_transcript", "with_transcript"] }),
    );
    assert_eq!(reply["supported"], json!([true, true]));
}

/// REQ: PORT:ilium-client/src/session_stats_popover.rs::clicking_a_preview_pins_it_and_tabs_switch
#[test]
fn port_session_stats_popover_pinned_popover_stays_open_when_the_pointer_leaves() {
    let reply = scenario(
        "clicking-a-preview-pins-it-and-tabs-switch",
        json!({ "events": ["hover_icon", "click_popover", "leave"] }),
    );
    assert_eq!(reply["popover_open"], json!(true));
}

/// REQ: PORT:ilium-client/src/session_stats_popover.rs::clicking_the_icon_pins_and_clicking_again_or_close_unpins
#[test]
fn port_session_stats_popover_hovering_after_a_pin_does_not_unpin_it() {
    let reply = scenario(
        "clicking-the-icon-pins-and-clicking-again-or-close-unpins",
        json!({ "events": ["click_icon", "hover_second_icon"] }),
    );
    assert_eq!(reply["pinned"], json!(true));
}

/// REQ: PORT:ilium-client/src/session_stats_popover.rs::crash_keeps_original_stats_after_live_session_map_is_cleared
#[test]
fn port_session_stats_popover_snapshot_is_kept_when_the_live_map_is_not_cleared() {
    let reply = scenario(
        "crash-keeps-original-stats-after-live-session-map-is-cleared",
        json!({ "session_id": "original-session", "clear_live_session_map": false }),
    );
    assert_eq!(reply["history_context_known"], json!(true));
    assert_eq!(reply["snapshot_same_before_and_after_clear"], json!(true));
}

/// REQ: PORT:ilium-client/src/session_stats_popover.rs::different_verified_session_hides_old_snapshot_immediately
#[test]
fn port_session_stats_popover_same_verified_session_keeps_its_snapshot() {
    let reply = scenario(
        "different-verified-session-hides-old-snapshot-immediately",
        json!({ "replacement_session": "original-session" }),
    );
    assert_ne!(reply["current_entry"], Value::Null);
}

/// REQ: PORT:ilium-client/src/session_stats_popover.rs::events_over_the_popover_never_reach_the_pane
#[test]
fn port_session_stats_popover_scroll_outside_the_popover_is_not_claimed() {
    let reply = scenario(
        "events-over-the-popover-never-reach-the-pane",
        json!({ "kinds": ["scroll_up"], "popover_open": false }),
    );
    assert_eq!(reply["claimed_inside"], json!([false]));
}

/// REQ: PORT:ilium-client/src/session_stats_popover.rs::full_click_sequence_through_the_top_level_dispatcher_pins
#[test]
fn port_session_stats_popover_press_without_release_does_not_pin() {
    let reply = scenario(
        "full-click-sequence-through-the-top-level-dispatcher-pins",
        json!({ "events": ["down"], "through": "top_level_dispatcher" }),
    );
    assert_eq!(reply["pinned"], json!(false));
}

/// REQ: PORT:ilium-client/src/session_stats_popover.rs::hovering_the_second_icon_previews_and_leaving_it_closes
#[test]
fn port_session_stats_popover_hovering_the_second_icon_opens_a_preview() {
    let reply = scenario(
        "hovering-the-second-icon-previews-and-leaving-it-closes",
        json!({ "events": ["hover_second_icon"] }),
    );
    assert_eq!(reply["open_after_hover"], json!(true));
}

/// REQ: PORT:ilium-client/src/session_stats_popover.rs::modal_dialogs_suspend_the_popover_interaction
#[test]
fn port_session_stats_popover_without_a_modal_the_click_is_claimed() {
    let reply = scenario(
        "modal-dialogs-suspend-the-popover-interaction",
        json!({ "modal_open": false, "pinned_before": true }),
    );
    assert_eq!(reply["event_claimed"], json!(true));
}

/// REQ: PORT:ilium-client/src/session_stats_popover.rs::plain_shells_have_no_stats_icon
#[test]
fn port_session_stats_popover_hovering_a_shell_opens_no_preview() {
    let reply = scenario(
        "plain-shells-have-no-stats-icon",
        json!({ "pane": "shell", "events": ["hover_icon"] }),
    );
    assert_eq!(reply["popover_open"], json!(false));
}

/// REQ: PORT:ilium-client/src/session_stats_popover.rs::stopped_agent_stats_dot_opens_pinned_retained_metrics_in_actual_renderer
#[test]
fn port_session_stats_popover_zero_output_is_not_rendered_as_the_retained_figure() {
    let reply = scenario(
        "stopped-agent-stats-dot-opens-pinned-retained-metrics-in-actual-renderer",
        json!({ "provider": "synthetic-retained-stats", "session_id": "original-session", "output_tokens": 0, "stopped": true }),
    );
    assert_eq!(reply["rendered_contains_991"], json!(false));
}

/// REQ: PORT:ilium-client/src/session_stats_popover.rs::the_tick_closes_a_popover_whose_pane_stopped_being_an_agent
#[test]
fn port_session_stats_popover_tick_keeps_a_popover_whose_pane_is_still_an_agent() {
    let reply = scenario(
        "the-tick-closes-a-popover-whose-pane-stopped-being-an-agent",
        json!({ "pane_becomes_shell": false }),
    );
    assert_eq!(reply["tick_changed"], json!(false));
    assert_eq!(reply["popover_open"], json!(true));
}

/// REQ: PORT:ilium-client/src/session_stats_popover.rs::unresolved_replacement_hides_old_stats_before_tick_and_drops_cache_without_popover
#[test]
fn port_session_stats_popover_unresolved_replacement_drops_the_cache_even_with_a_popover_open() {
    let reply = scenario(
        "unresolved-replacement-hides-old-stats-before-tick-and-drops-cache-without-popover",
        json!({ "replacement_session": "unresolved", "popover": true }),
    );
    assert_eq!(reply["store_entry_after_tick"], Value::Null);
}
