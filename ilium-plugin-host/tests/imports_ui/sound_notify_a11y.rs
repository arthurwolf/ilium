//! Sound and notices are never the only signal (CAP-880..B06).
//!
//! `plan_signals` is the fan-out rule these requirements describe: every available channel is
//! kept, and one visible in-app text always accompanies the event.

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

/// The fan-out rule for one event: each channel that is neither muted nor failed keeps its
/// signal, and exactly one in-app text always accompanies the event. The text is persistent when
/// the event needs action or when a desktop notice (which can vanish) is part of the event.
fn plan_signals(event: &AlertEvent) -> Result<Vec<Signal>, String> {
    let mut signals: Vec<Signal> = event
        .channels
        .iter()
        .filter(|(channel, state)| {
            *channel != Channel::InAppText && *state == ChannelState::Available
        })
        .map(|(channel, _)| Signal { channel: *channel, is_persistent: false })
        .collect();
    let has_desktop_notice = event
        .channels
        .iter()
        .any(|(channel, _)| *channel == Channel::DesktopNotice);
    signals.push(Signal {
        channel: Channel::InAppText,
        is_persistent: event.needs_action || has_desktop_notice,
    });
    Ok(signals)
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

/// REQ: CAP-880
#[test]
fn a_sound_event_plans_the_sound_next_to_its_visible_text() {
    let event = event_with(false, &[(Channel::Sound, ChannelState::Available)]);
    let signals = plan_signals(&event).expect("a sound event plans");
    assert!(
        signals.iter().any(|signal| signal.channel == Channel::Sound),
        "the sound must stay in the plan"
    );
    assert!(has_text(&signals), "the paired visible text is missing");
}

/// REQ: CAP-881
#[test]
fn a_muted_sound_setting_keeps_the_visible_signal() {
    let event = event_with(
        false,
        &[
            (Channel::Sound, ChannelState::Muted),
            (Channel::InAppText, ChannelState::Available),
        ],
    );
    let signals = plan_signals(&event).expect("a muted sound still plans");
    assert!(
        has_text(&signals),
        "a muted sound must not suppress the visible signal"
    );
}

/// REQ: CAP-882
#[test]
fn a_failed_desktop_notice_still_has_its_persistent_in_app_twin() {
    let event = event_with(false, &[(Channel::DesktopNotice, ChannelState::Failed)]);
    let signals = plan_signals(&event).expect("a failed desktop notice plans");
    assert!(
        signals
            .iter()
            .any(|signal| signal.channel == Channel::InAppText && signal.is_persistent),
        "the in-app twin must survive a failed desktop notice"
    );
}

/// REQ: CAP-883
#[test]
fn a_muted_toast_on_an_action_needed_event_still_has_a_persistent_twin() {
    let event = event_with(true, &[(Channel::Toast, ChannelState::Muted)]);
    let signals = plan_signals(&event).expect("a muted toast plans");
    assert!(
        signals.iter().any(|signal| signal.is_persistent),
        "an action-needed event needs a persistent signal even when its toast is muted"
    );
}

/// REQ: CAP-884
#[test]
fn text_is_present_when_every_other_channel_is_muted() {
    let muted = event_with(
        true,
        &[
            (Channel::Sound, ChannelState::Muted),
            (Channel::DesktopNotice, ChannelState::Muted),
            (Channel::Toast, ChannelState::Muted),
        ],
    );
    let signals = plan_signals(&muted).expect("muted plans");
    assert!(has_text(&signals), "the text signal must remain");
}

/// REQ: CAP-885
#[test]
fn a_failed_desktop_channel_does_not_drop_the_sound() {
    let event = event_with(
        false,
        &[
            (Channel::Sound, ChannelState::Available),
            (Channel::DesktopNotice, ChannelState::Failed),
        ],
    );
    let signals = plan_signals(&event).expect("partial failure plans");
    assert!(
        signals.iter().any(|signal| signal.channel == Channel::Sound),
        "a failed desktop channel must not remove the sound"
    );
}
