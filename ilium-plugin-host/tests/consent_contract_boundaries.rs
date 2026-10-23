use ilium_plugin_host::consent::{ConsentGate, ConsentOutcome, DialogGuard, DialogKind};
use std::time::Duration;

#[test]
fn rolling_hour_limit_has_exact_retry_and_expires_at_boundary() {
    let mut gate = ConsentGate::new();
    for index in 0..6 {
        gate.open("example.plugin", &format!("target-{index}"))
            .unwrap();
        assert!(gate.close_approved());
    }
    let denied = gate.open_detailed("example.plugin", "seventh").unwrap_err();
    assert_eq!(denied.outcome, ConsentOutcome::RateLimited);
    assert_eq!(denied.retry_after, Some(Duration::from_secs(3600)));
    assert_eq!(gate.tick(Duration::from_secs(3599)), None);
    assert_eq!(
        gate.open_detailed("example.plugin", "seventh")
            .unwrap_err()
            .retry_after,
        Some(Duration::from_secs(1))
    );
    assert_eq!(gate.tick(Duration::from_secs(1)), None);
    assert!(gate.open("example.plugin", "seventh").is_ok());
}

#[test]
fn denied_attempts_do_not_consume_shown_prompt_quota_and_plugins_are_independent() {
    let mut gate = ConsentGate::new();
    gate.open("example.first", "one").unwrap();
    for _ in 0..20 {
        let denied = gate.open_detailed("example.second", "one").unwrap_err();
        assert_eq!(denied.outcome, ConsentOutcome::Busy);
        assert_eq!(denied.retry_after, Some(Duration::from_secs(1)));
    }
    gate.close_approved();
    for index in 0..6 {
        gate.open("example.second", &index.to_string()).unwrap();
        gate.close_approved();
    }
    assert!(gate.open("example.first", "two").is_ok());
}

#[test]
fn timeout_occurs_at_120_seconds_once_and_never_records_a_decline() {
    let mut gate = ConsentGate::new();
    for _ in 0..3 {
        gate.open("example.plugin", "target").unwrap();
        assert_eq!(gate.tick(Duration::from_secs(119)), None);
        assert_eq!(
            gate.tick(Duration::from_secs(1)),
            Some(ConsentOutcome::TimedOut)
        );
        assert_eq!(gate.tick(Duration::ZERO), None);
        assert!(!gate.close_approved());
    }
    assert!(gate.open("example.plugin", "target").is_ok());
}

#[test]
fn three_declines_block_only_matching_plugin_and_target_until_reset() {
    let mut gate = ConsentGate::new();
    for _ in 0..3 {
        gate.open("example.plugin", "target").unwrap();
        gate.close(ConsentOutcome::Declined);
    }
    gate.tick(Duration::from_secs(24 * 3600));
    let denied = gate.open_detailed("example.plugin", "target").unwrap_err();
    assert_eq!(denied.outcome, ConsentOutcome::Declined);
    assert_eq!(denied.retry_after, None);
    gate.open("example.other", "target").unwrap();
    gate.close_approved();
    gate.open("example.plugin", "other-target").unwrap();
    gate.close_approved();
    gate.reset_declines("example.plugin", "target");
    gate.open("example.plugin", "target").unwrap();
}

#[test]
fn admission_failures_cannot_close_or_approve_the_pending_prompt() {
    let mut gate = ConsentGate::new();
    gate.open("example.plugin", "target").unwrap();
    gate.close(ConsentOutcome::Busy);
    gate.close(ConsentOutcome::RateLimited);
    assert_eq!(
        gate.open("example.other", "target"),
        Err(ConsentOutcome::Busy)
    );
    assert!(gate.close_approved());
    assert!(!gate.close_approved());
}

#[test]
fn target_reset_does_not_bypass_rolling_prompt_limit() {
    let mut gate = ConsentGate::new();
    for index in 0..6 {
        gate.open("example.plugin", &index.to_string()).unwrap();
        gate.close(ConsentOutcome::Declined);
    }
    gate.reset_declines("example.plugin", "0");
    assert_eq!(
        gate.open("example.plugin", "0"),
        Err(ConsentOutcome::RateLimited)
    );
}

#[test]
fn event_guard_rejects_pre_draw_input_and_repeat_but_allows_release_and_cancel() {
    let mut guard = DialogGuard::opened_at(Duration::from_secs(1));
    assert!(!guard.accepts_key_at("Escape", Duration::from_millis(999), true));
    assert!(guard.accepts_key_at("Escape", Duration::from_millis(1000), true));
    assert!(!guard.accepts_key_at("Enter", Duration::from_millis(1499), true));
    assert!(guard.accepts_key_at("Enter", Duration::from_millis(1500), true));
    assert!(!guard.accepts_key_at("Enter", Duration::from_millis(1579), false));
    assert!(!guard.accepts_key_at("Enter", Duration::from_millis(1580), false));
    assert!(guard.accepts_key_at("Enter", Duration::from_millis(1581), true));
    assert!(guard.accepts_key_at("Space", Duration::from_millis(1582), false));
}

#[test]
fn all_dialog_kinds_name_the_kernel_and_legacy_guard_keeps_grace_boundaries() {
    for kind in [
        DialogKind::Review,
        DialogKind::Consent,
        DialogKind::Update,
        DialogKind::Trust,
    ] {
        assert!(kind.is_kernel_drawn());
        assert_eq!(kind.source_label(), "Ilium");
    }
    assert!(!ConsentGate::new().accepts_plugin_drawn_dialogs());
    let mut guard = DialogGuard::opened_at(Duration::from_secs(1));
    assert!(!guard.accepts_input_at(Duration::from_millis(999)));
    assert!(!guard.accepts_input_at(Duration::from_millis(1499)));
    assert!(guard.accepts_input_at(Duration::from_millis(1500)));
    assert!(!guard.accepts_input_at(Duration::from_millis(1579)));
    assert!(guard.accepts_input_at(Duration::from_millis(1580)));
}
