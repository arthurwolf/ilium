//! Twins of ilium-client/src/agent_freeze.rs: the auto-freeze policy for finished and waiting
//! agents. The policy is the plugin's own pure module (`ilium_plugin_core_agent_toolbar::freeze`),
//! so the twins call it directly.

use ilium_plugin_core_agent_toolbar::freeze::{
    Activity, AutoFreezeSettings, FreezeBook, FreezeClock,
};

fn enabled() -> AutoFreezeSettings {
    AutoFreezeSettings {
        enabled: true,
        ..AutoFreezeSettings::default()
    }
}

/// REQ: PORT:ilium-client/src/agent_freeze.rs::defaults_only_freeze_finished_agents_after_six_hours
#[test]
fn port_agent_freeze_defaults_only_freeze_finished_agents_after_six_hours() {
    let settings = enabled();
    assert!(settings.should_freeze(Activity::Done, 6 * 60 * 60));
    assert!(!settings.should_freeze(Activity::Idle, 99 * 60 * 60));
    assert!(!settings.should_freeze(Activity::Working, 99 * 60 * 60));
}

/// REQ: PORT:ilium-client/src/agent_freeze.rs::unfreeze_resets_even_if_the_agent_stays_done
#[test]
fn port_agent_freeze_unfreeze_resets_even_if_the_agent_stays_done() {
    let clock = FreezeClock::default().observe(20, Activity::Done);
    assert_eq!(clock.reset_on_unfreeze().inactive_seconds(), 0);
}

/// REQ: PORT:ilium-client/src/agent_freeze.rs::waiting_state_options_are_independent
#[test]
fn port_agent_freeze_waiting_state_options_are_independent() {
    let settings = AutoFreezeSettings {
        enabled: true,
        after_seconds: 10,
        freeze_done: false,
        freeze_waiting_for_input: true,
        freeze_waiting_for_approval: false,
        freeze_waiting_for_background: true,
    };
    assert!(settings.should_freeze(Activity::Idle, 10));
    assert!(!settings.should_freeze(Activity::WaitingApproval, 10));
    assert!(settings.should_freeze(Activity::WaitingBackground, 10));
}

/// REQ: PORT:ilium-client/src/agent_freeze.rs::working_activity_clears_elapsed_time
#[test]
fn port_agent_freeze_working_activity_clears_elapsed_time() {
    let clock = FreezeClock::default()
        .observe(20, Activity::Done)
        .observe(1, Activity::Working);
    assert_eq!(clock.inactive_seconds(), 0);
}

/// REQ: PORT:ilium-client/src/agent_freeze.rs::defaults_only_freeze_finished_agents_after_six_hours
#[test]
fn port_agent_freeze_master_switch_off_never_freezes_even_a_finished_agent() {
    assert!(!AutoFreezeSettings::default().should_freeze(Activity::Done, 99 * 60 * 60));
}

/// REQ: PORT:ilium-client/src/agent_freeze.rs::unfreeze_resets_even_if_the_agent_stays_done
#[test]
fn port_agent_freeze_without_unfreeze_reset_the_done_time_keeps_counting() {
    let clock = FreezeClock::default().observe(20, Activity::Done);
    assert_eq!(clock.inactive_seconds(), 20);
}

/// REQ: PORT:ilium-client/src/agent_freeze.rs::waiting_state_options_are_independent
#[test]
fn port_agent_freeze_approval_wait_freezes_only_after_the_threshold() {
    let settings = AutoFreezeSettings {
        enabled: true,
        after_seconds: 10,
        freeze_done: false,
        freeze_waiting_for_input: false,
        freeze_waiting_for_approval: true,
        freeze_waiting_for_background: false,
    };
    assert!(!settings.should_freeze(Activity::WaitingApproval, 9));
    assert!(settings.should_freeze(Activity::WaitingApproval, 10));
}

/// REQ: PORT:ilium-client/src/agent_freeze.rs::working_activity_clears_elapsed_time
#[test]
fn port_agent_freeze_non_working_observations_accumulate() {
    let clock = FreezeClock::default()
        .observe(5, Activity::Done)
        .observe(7, Activity::Idle);
    assert_eq!(clock.inactive_seconds(), 12);
}

/// The kernel-side `tick_auto_freeze` walked the tree every tick; the plugin follows the
/// `activity-detail` signal instead and asks the kernel to freeze each due pane once.
#[test]
fn port_agent_freeze_book_asks_for_each_due_pane_once() {
    let mut book = FreezeBook::default();
    book.observe(3, "done");
    assert!(book.due(0, enabled()).is_empty());
    assert_eq!(book.due(6 * 60 * 60 * 1_000, enabled()), vec![3]);
    assert!(book.due(7 * 60 * 60 * 1_000, enabled()).is_empty());
}
