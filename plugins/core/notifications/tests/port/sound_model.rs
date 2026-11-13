//! Twins of the kernel tests of `ilium-sound/src/lib.rs` that cover the settings model and the
//! transition-to-event mapping, run against `ilium_plugin_core_notifications::sound`.
//! The kernel built a `PaneStatus` and projected it; the plugin receives the activity through
//! the `ilium.core.agent-detect/activity-changed` topic, so the twins use `Activity` directly.

use ilium_plugin_core_notifications::sound::*;

/// REQ: PORT:ilium-sound/src/lib.rs::defaults_alert_only_when_an_agent_finishes
#[test]
fn defaults_alert_only_when_an_agent_finishes() {
    let settings = SoundSettings::default();
    assert!(settings.events.agent_finished);
    assert!(!settings.events.approval_required);
    assert!(!settings.events.agent_started);
    assert!(!settings.events.waiting_background);
}

/// REQ: PORT:ilium-sound/src/lib.rs::maps_each_supported_transition_once
#[test]
fn maps_each_supported_transition_once() {
    assert_eq!(
        event_for_transition(Some(Activity::Working), Activity::Done),
        Some(SoundEvent::AgentFinished)
    );
    assert_eq!(
        event_for_transition(Some(Activity::Working), Activity::WaitingApproval),
        Some(SoundEvent::ApprovalRequired)
    );
    assert_eq!(
        event_for_transition(Some(Activity::Idle), Activity::Working),
        Some(SoundEvent::AgentStarted)
    );
    assert_eq!(
        event_for_transition(Some(Activity::Working), Activity::WaitingBackground),
        Some(SoundEvent::WaitingBackground)
    );
}

/// REQ: PORT:ilium-sound/src/lib.rs::background_task_still_running_is_treated_symmetrically_with_waiting_background
#[test]
fn background_task_still_running_is_treated_symmetrically_with_waiting_background() {
    // Entering from a non-background-wait state reuses the WaitingBackground chime.
    assert_eq!(
        event_for_transition(
            Some(Activity::Working),
            Activity::BackgroundTaskStillRunning
        ),
        Some(SoundEvent::WaitingBackground)
    );
    // Moving directly between the two background-wait flavors must not refire it.
    assert_eq!(
        event_for_transition(
            Some(Activity::WaitingBackground),
            Activity::BackgroundTaskStillRunning
        ),
        None
    );
    // Leaving it for Done still plays the finished chime.
    assert_eq!(
        event_for_transition(Some(Activity::BackgroundTaskStillRunning), Activity::Done),
        Some(SoundEvent::AgentFinished)
    );
    // Resuming Working from it must not refire AgentStarted: it was already a busy state.
    assert_eq!(
        event_for_transition(
            Some(Activity::BackgroundTaskStillRunning),
            Activity::Working
        ),
        None
    );
}

/// REQ: PORT:ilium-sound/src/lib.rs::ignores_first_poll_and_stable_states
#[test]
fn ignores_first_poll_and_stable_states() {
    assert_eq!(event_for_transition(None, Activity::Done), None);
    assert_eq!(
        event_for_transition(Some(Activity::Done), Activity::Done),
        None
    );
    assert_eq!(
        event_for_transition(Some(Activity::WaitingApproval), Activity::WaitingApproval),
        None
    );
}

/// REQ: PORT:ilium-sound/src/lib.rs::master_notification_switch_overrides_every_event
#[test]
fn master_notification_switch_overrides_every_event() {
    let mut notifications = NotificationSettings {
        enabled: false,
        ..NotificationSettings::default()
    };
    for event in NotificationEvent::ALL {
        assert!(!notifications.is_enabled(event));
    }
    notifications.enabled = true;
    notifications.toggle(NotificationEvent::TaskSucceeded);
    assert!(notifications.is_enabled(NotificationEvent::TaskSucceeded));
    assert!(notifications.event_flag(NotificationEvent::TaskSucceeded));
}

/// REQ: PORT:ilium-sound/src/lib.rs::notification_coalescing_is_clamped
#[test]
fn notification_coalescing_is_clamped() {
    let notifications = NotificationSettings {
        task_coalesce_seconds: u32::MAX,
        ..NotificationSettings::default()
    }
    .normalized();
    assert_eq!(
        notifications.task_coalesce_seconds,
        NotificationSettings::MAX_TASK_COALESCE_SECONDS
    );
}

/// REQ: PORT:ilium-sound/src/lib.rs::task_success_sound_and_notification_default_off_and_failure_on
#[test]
fn task_success_sound_and_notification_default_off_and_failure_on() {
    assert!(!SoundEventSettings::default().task_succeeded);
    assert!(SoundEventSettings::default().task_failed);
    let notifications = NotificationSettings::default();
    assert!(notifications.is_enabled(NotificationEvent::AgentFinished));
    assert!(notifications.is_enabled(NotificationEvent::ApprovalRequired));
    assert!(!notifications.is_enabled(NotificationEvent::TaskSucceeded));
    assert!(notifications.is_enabled(NotificationEvent::TaskFailed));
    assert!(notifications.suppress_redundant_task_outcomes);
    assert_eq!(notifications.task_coalesce_seconds, 30);
}

/// The settings tables keep the kernel's serde shape: a missing key takes its default.
#[test]
fn sound_settings_round_trip_through_json_and_default_missing_keys() {
    let settings: SoundSettings =
        serde_json::from_str(r#"{"source":"muted","events":{"agent_started":true}}"#).unwrap();
    assert_eq!(settings.source, SoundSourceKind::Muted);
    assert!(settings.events.agent_started);
    assert!(settings.events.agent_finished);
    let encoded = serde_json::to_string(&settings).unwrap();
    assert_eq!(
        serde_json::from_str::<SoundSettings>(&encoded).unwrap(),
        settings
    );
}
