//! F-076: detach and reattach (page sample "Detach and reattach"): the plugin counts clients from
//! `client-attached` and `client-detached` and slows its periodic work down while none is attached.

use crate::common::*;

const FAST: (u32, Option<u32>) = (ATTACHED_POLL_MS, Some(ATTACHED_POLL_MS));
const SLOW: (u32, Option<u32>) = (DETACHED_POLL_MS, Some(DETACHED_POLL_MS));

/// REQ: F-076
#[test]
fn detach_076_poll_periods_are_one_and_thirty_seconds() {
    assert_eq!(ATTACHED_POLL_MS, 1_000);
    assert_eq!(DETACHED_POLL_MS, 30_000);
}

/// REQ: F-076
#[test]
fn detach_076_first_attach_polls_every_second() {
    let mut host = activated_server();
    host.deliver(vec![attached(1)]);
    assert_eq!(active_timers(&host), [FAST]);
}

/// REQ: F-076
#[test]
fn detach_076_last_detach_slows_the_poll_to_thirty_seconds() {
    let mut host = activated_server();
    host.deliver(vec![attached(1)]);
    host.deliver(vec![detached(1)]);
    assert_eq!(active_timers(&host), [SLOW]);
}

/// REQ: F-076
#[test]
fn detach_076_second_attach_replaces_the_timer_instead_of_stacking() {
    let mut host = activated_server();
    host.deliver(vec![attached(1), attached(2)]);
    assert_eq!(active_timers(&host), [FAST]);
}

/// REQ: F-076
#[test]
fn detach_076_one_of_two_clients_leaving_keeps_the_fast_poll() {
    let mut host = activated_server();
    host.deliver(vec![attached(1), attached(2)]);
    host.deliver(vec![detached(1)]);
    assert_eq!(active_timers(&host), [FAST]);
}

/// REQ: F-076
#[test]
fn detach_076_both_clients_leaving_slows_the_poll() {
    let mut host = activated_server();
    host.deliver(vec![attached(1), attached(2)]);
    host.deliver(vec![detached(1), detached(2)]);
    assert_eq!(active_timers(&host), [SLOW]);
}

/// REQ: F-076
#[test]
fn detach_076_reattach_after_a_full_detach_returns_to_the_fast_poll() {
    let mut host = activated_server();
    host.deliver(vec![attached(1)]);
    host.deliver(vec![detached(1)]);
    host.deliver(vec![attached(3)]);
    assert_eq!(active_timers(&host), [FAST]);
}

/// REQ: F-076
#[test]
fn detach_076_three_attached_two_detached_is_still_fast() {
    let mut host = activated_server();
    host.deliver(vec![attached(1), attached(2), attached(3)]);
    host.deliver(vec![detached(1), detached(2)]);
    assert_eq!(active_timers(&host), [FAST]);
}

/// REQ: F-076
#[test]
fn detach_076_three_attached_three_detached_is_slow() {
    let mut host = activated_server();
    host.deliver(vec![attached(1), attached(2), attached(3)]);
    host.deliver(vec![detached(1), detached(2), detached(3)]);
    assert_eq!(active_timers(&host), [SLOW]);
}

/// REQ: F-076
#[test]
fn detach_076_detach_without_any_attach_saturates_at_zero() {
    let mut host = activated_server();
    host.deliver(vec![detached(9)]);
    assert!(host.failures().is_empty());
    assert_eq!(active_timers(&host), [SLOW]);
}

/// REQ: F-076
#[test]
fn detach_076_an_extra_detach_does_not_make_the_next_attach_look_detached() {
    let mut host = activated_server();
    host.deliver(vec![attached(1), detached(1), detached(1)]);
    host.deliver(vec![attached(2)]);
    assert_eq!(active_timers(&host), [FAST]);
}

/// REQ: F-076
#[test]
fn detach_076_the_clock_fires_the_repeating_poll_without_stacking_timers() {
    let mut host = activated_server();
    host.deliver(vec![attached(1)]);
    host.advance_clock(3 * ATTACHED_POLL_MS as u64);
    host.settle();
    assert_eq!(active_timers(&host), [FAST]);
    assert!(host.failures().is_empty());
}

/// REQ: F-076
#[test]
fn detach_076_panes_keep_running_while_detached() {
    let mut host = activated_server();
    let pane = add_shell(&mut host, "shell");
    host.deliver(vec![attached(1)]);
    host.deliver(vec![detached(1)]);
    assert!(terminations(&host.submitted()).is_empty());
    assert!(host.tree().get(pane).is_some());
}
