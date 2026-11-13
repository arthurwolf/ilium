//! Sound and notices are never the only signal (CAP-880..B06).
//!
//! Red phase: the skeleton `plan_signals` is not implemented yet.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Channel {
    Sound,
    DesktopNotice,
    Toast,
    InAppText,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ChannelState {
    Available,
    Muted,
    Failed,
}

struct AlertEvent {
    needs_action: bool,
    channels: Vec<(Channel, ChannelState)>,
}

#[derive(Debug)]
struct Signal {
    channel: Channel,
    is_persistent: bool,
}

/// Skeleton of the plugin-check and fan-out rule for one event.
fn plan_signals(event: &AlertEvent) -> Result<Vec<Signal>, String> {
    let _inputs = (event.needs_action, &event.channels);
    unimplemented!("alert-redundancy fan-out is not implemented")
}

fn event_with(needs_action: bool, channels: &[(Channel, ChannelState)]) -> AlertEvent {
    AlertEvent {
        needs_action,
        channels: channels.to_vec(),
    }
}

fn has_text(signals: &[Signal]) -> bool {
    signals
        .iter()
        .any(|signal| signal.channel == Channel::InAppText)
}

/// REQ: CAP-880
#[test]
fn sound_is_always_paired_with_visible_text() {
    let event = event_with(false, &[(Channel::Sound, ChannelState::Available)]);
    let signals = plan_signals(&event).expect("a sound event plans");
    assert!(has_text(&signals));
}

/// REQ: CAP-881
#[test]
fn failed_sound_keeps_the_visible_signal() {
    let event = event_with(
        false,
        &[
            (Channel::Sound, ChannelState::Failed),
            (Channel::InAppText, ChannelState::Available),
        ],
    );
    let signals = plan_signals(&event).expect("a failed sound still plans");
    assert!(has_text(&signals));
}

/// REQ: CAP-882
#[test]
fn desktop_notice_has_a_persistent_in_app_twin() {
    let event = event_with(false, &[(Channel::DesktopNotice, ChannelState::Available)]);
    let signals = plan_signals(&event).expect("a desktop event plans");
    assert!(signals
        .iter()
        .any(|signal| signal.channel == Channel::InAppText && signal.is_persistent));
}

/// REQ: CAP-883
#[test]
fn action_needed_toast_has_a_persistent_twin() {
    let event = event_with(true, &[(Channel::Toast, ChannelState::Available)]);
    let signals = plan_signals(&event).expect("a toast event plans");
    assert!(signals.iter().any(|signal| signal.is_persistent));
}

/// REQ: CAP-884
#[test]
fn muting_every_other_channel_leaves_the_text_unchanged() {
    let muted = event_with(
        true,
        &[
            (Channel::Sound, ChannelState::Muted),
            (Channel::DesktopNotice, ChannelState::Muted),
            (Channel::Toast, ChannelState::Muted),
        ],
    );
    let open = event_with(
        true,
        &[
            (Channel::Sound, ChannelState::Available),
            (Channel::DesktopNotice, ChannelState::Available),
            (Channel::Toast, ChannelState::Available),
        ],
    );
    let muted_text = plan_signals(&muted).expect("muted plans");
    let open_text = plan_signals(&open).expect("open plans");
    assert_eq!(
        muted_text
            .iter()
            .filter(|s| s.channel == Channel::InAppText)
            .count(),
        open_text
            .iter()
            .filter(|s| s.channel == Channel::InAppText)
            .count()
    );
}

/// REQ: CAP-885
#[test]
fn a_failed_channel_does_not_drop_the_others() {
    let event = event_with(
        false,
        &[
            (Channel::Sound, ChannelState::Failed),
            (Channel::DesktopNotice, ChannelState::Available),
        ],
    );
    let signals = plan_signals(&event).expect("partial failure plans");
    assert!(has_text(&signals));
    assert!(signals
        .iter()
        .any(|signal| signal.channel == Channel::DesktopNotice));
}
