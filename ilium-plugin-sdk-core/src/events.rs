//! One builder per `Event` case, plus the test-oriented `activity` and `command` (rust-sdk.md "Events").
use crate::generated::types::*;

/// Event for an agent activity change; the host numbers `revision` +1 per node per host (events manual).
pub fn activity(node: u64, previous: AgentActivity, current: AgentActivity) -> Event {
    let revision = ACTIVITY_REVISIONS.with(|revisions| {
        let mut revisions = revisions.borrow_mut();
        let next = revisions.entry(node).or_insert(0);
        *next += 1;
        *next
    });
    Event::AgentActivityChanged(AgentActivityEvent {
        node: NodeId(node),
        previous,
        current,
        revision,
    })
}

thread_local! {
    /// Last revision handed out per node; the real host numbers +1 per node per host.
    static ACTIVITY_REVISIONS: std::cell::RefCell<std::collections::HashMap<u64, u64>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Event for a command invocation from a JSON arguments string (events manual).
pub fn command(name: &str, arguments_json: &str) -> Event {
    Event::CommandInvoked(CommandInvokedEvent {
        command: name.to_string(),
        arguments_json: arguments_json.to_string(),
        origin: CommandOrigin::Cli,
        context: CommandContext::default(),
    })
}

/// Builds `Event::TreeChanged` (rust-sdk.md "Events").
pub fn tree_changed(payload: Vec<NodeId>) -> Event {
    Event::TreeChanged(payload)
}

/// Builds `Event::NodeRemoved` (rust-sdk.md "Events").
pub fn node_removed(payload: NodeId) -> Event {
    Event::NodeRemoved(payload)
}

/// Builds `Event::PaneCreated` (rust-sdk.md "Events").
pub fn pane_created(payload: PaneInfo) -> Event {
    Event::PaneCreated(payload)
}

/// Builds `Event::PaneExited` (rust-sdk.md "Events").
pub fn pane_exited(payload: PaneExitedEvent) -> Event {
    Event::PaneExited(payload)
}

/// Builds `Event::PaneFocused` (rust-sdk.md "Events").
pub fn pane_focused(payload: Option<NodeId>) -> Event {
    Event::PaneFocused(payload)
}

/// Builds `Event::PaneResized` (rust-sdk.md "Events").
pub fn pane_resized(payload: PaneResizedEvent) -> Event {
    Event::PaneResized(payload)
}

/// Builds `Event::PaneTitleChanged` (rust-sdk.md "Events").
pub fn pane_title_changed(payload: PaneTitleChangedEvent) -> Event {
    Event::PaneTitleChanged(payload)
}

/// Builds `Event::AgentIdentified` (rust-sdk.md "Events").
pub fn agent_identified(payload: AgentIdentifiedEvent) -> Event {
    Event::AgentIdentified(payload)
}

/// Builds `Event::AgentActivityChanged` (rust-sdk.md "Events").
pub fn agent_activity_changed(payload: AgentActivityEvent) -> Event {
    Event::AgentActivityChanged(payload)
}

/// Builds `Event::AgentGoalChanged` (rust-sdk.md "Events").
pub fn agent_goal_changed(payload: AgentGoalEvent) -> Event {
    Event::AgentGoalChanged(payload)
}

/// Builds `Event::InputSubmitted` (rust-sdk.md "Events").
pub fn input_submitted(payload: InputSubmittedEvent) -> Event {
    Event::InputSubmitted(payload)
}

/// Builds `Event::OutputMatched` (rust-sdk.md "Events").
pub fn output_matched(payload: OutputMatchedEvent) -> Event {
    Event::OutputMatched(payload)
}

/// Builds `Event::LinkActivated` (rust-sdk.md "Events").
pub fn link_activated(payload: LinkActivatedEvent) -> Event {
    Event::LinkActivated(payload)
}

/// Builds `Event::Pointer` (rust-sdk.md "Events").
pub fn pointer(payload: PointerEvent) -> Event {
    Event::Pointer(payload)
}

/// Builds `Event::EditorOpened` (rust-sdk.md "Events").
pub fn editor_opened(payload: BufferRef) -> Event {
    Event::EditorOpened(payload)
}

/// Builds `Event::EditorClosed` (rust-sdk.md "Events").
pub fn editor_closed(payload: BufferRef) -> Event {
    Event::EditorClosed(payload)
}

/// Builds `Event::EditorChanged` (rust-sdk.md "Events").
pub fn editor_changed(payload: EditorChangedEvent) -> Event {
    Event::EditorChanged(payload)
}

/// Builds `Event::EditorSaved` (rust-sdk.md "Events").
pub fn editor_saved(payload: EditorSavedEvent) -> Event {
    Event::EditorSaved(payload)
}

/// Builds `Event::SettingsChanged` (rust-sdk.md "Events").
pub fn settings_changed(payload: SettingsChange) -> Event {
    Event::SettingsChanged(payload)
}

/// Builds `Event::TimerFired` (rust-sdk.md "Events").
pub fn timer_fired(payload: TimerId) -> Event {
    Event::TimerFired(payload)
}

/// Builds `Event::RequestCompleted` (rust-sdk.md "Events").
pub fn request_completed(payload: RequestResult) -> Event {
    Event::RequestCompleted(payload)
}

/// Builds `Event::JobProgress` (rust-sdk.md "Events").
pub fn job_progress(payload: JobProgressEvent) -> Event {
    Event::JobProgress(payload)
}

/// Builds `Event::Message` (rust-sdk.md "Events").
pub fn message(payload: MessageEvent) -> Event {
    Event::Message(payload)
}

/// Builds `Event::TopicPublished` (rust-sdk.md "Events").
pub fn topic_published(payload: TopicPublishedEvent) -> Event {
    Event::TopicPublished(payload)
}

/// Builds `Event::CommandInvoked` (rust-sdk.md "Events").
pub fn command_invoked(payload: CommandInvokedEvent) -> Event {
    Event::CommandInvoked(payload)
}

/// Builds `Event::UiAction` (rust-sdk.md "Events").
pub fn ui_action(payload: UiAction) -> Event {
    Event::UiAction(payload)
}

/// Builds `Event::KeyPressed` (rust-sdk.md "Events").
pub fn key_pressed(payload: KeyEvent) -> Event {
    Event::KeyPressed(payload)
}

/// Builds `Event::KeyReleased` (rust-sdk.md "Events").
pub fn key_released(payload: KeyEvent) -> Event {
    Event::KeyReleased(payload)
}

/// Builds `Event::ViewVisibility` (rust-sdk.md "Events").
pub fn view_visibility(payload: ViewVisibilityEvent) -> Event {
    Event::ViewVisibility(payload)
}

/// Builds `Event::FsChanged` (rust-sdk.md "Events").
pub fn fs_changed(payload: FsChangedEvent) -> Event {
    Event::FsChanged(payload)
}

/// Builds `Event::AudioFrames` (rust-sdk.md "Events").
pub fn audio_frames(payload: AudioFramesEvent) -> Event {
    Event::AudioFrames(payload)
}

/// Builds `Event::CliInvoked` (rust-sdk.md "Events").
pub fn cli_invoked(payload: CliInvokedEvent) -> Event {
    Event::CliInvoked(payload)
}

/// Builds `Event::CliCancelled` (rust-sdk.md "Events").
pub fn cli_cancelled(payload: u64) -> Event {
    Event::CliCancelled(payload)
}

/// Builds `Event::HttpCalled` (rust-sdk.md "Events").
pub fn http_called(payload: HttpCalledEvent) -> Event {
    Event::HttpCalled(payload)
}

/// Builds `Event::ClientAttached` (rust-sdk.md "Events").
pub fn client_attached(payload: ClientId) -> Event {
    Event::ClientAttached(payload)
}

/// Builds `Event::ClientDetached` (rust-sdk.md "Events").
pub fn client_detached(payload: ClientId) -> Event {
    Event::ClientDetached(payload)
}

/// Builds `Event::ThemeChanged` (rust-sdk.md "Events").
pub fn theme_changed() -> Event {
    Event::ThemeChanged
}

/// Builds `Event::EventsDropped` (rust-sdk.md "Events").
pub fn events_dropped(payload: Vec<DroppedCount>) -> Event {
    Event::EventsDropped(payload)
}

/// Builds `Event::ScreenChanged` (rust-sdk.md "Events").
pub fn screen_changed(payload: ScreenChangedEvent) -> Event {
    Event::ScreenChanged(payload)
}

/// Builds `Event::UserLineSubmitted` (rust-sdk.md "Events").
pub fn user_line_submitted(payload: UserLineEvent) -> Event {
    Event::UserLineSubmitted(payload)
}

/// Builds `Event::SignalChanged` (rust-sdk.md "Events").
pub fn signal_changed(payload: SignalChangedEvent) -> Event {
    Event::SignalChanged(payload)
}

/// Builds `Event::AgentModelChanged` (rust-sdk.md "Events").
pub fn agent_model_changed(payload: AgentModelEvent) -> Event {
    Event::AgentModelChanged(payload)
}

/// Builds `Event::EditorPointer` (rust-sdk.md "Events").
pub fn editor_pointer(payload: EditorPointerEvent) -> Event {
    Event::EditorPointer(payload)
}

/// Builds `Event::EditorViewportChanged` (rust-sdk.md "Events").
pub fn editor_viewport_changed(payload: EditorViewport) -> Event {
    Event::EditorViewportChanged(payload)
}

/// Builds `Event::HttpStreamData` (rust-sdk.md "Events").
pub fn http_stream_data(payload: HttpStreamDataEvent) -> Event {
    Event::HttpStreamData(payload)
}

/// Builds `Event::HttpStreamClosed` (rust-sdk.md "Events").
pub fn http_stream_closed(payload: HttpStreamClosedEvent) -> Event {
    Event::HttpStreamClosed(payload)
}

/// Builds `Event::ChildOutput` (rust-sdk.md "Events").
pub fn child_output(payload: ChildOutputEvent) -> Event {
    Event::ChildOutput(payload)
}

/// Builds `Event::ChildExited` (rust-sdk.md "Events").
pub fn child_exited(payload: ChildExitedEvent) -> Event {
    Event::ChildExited(payload)
}

/// Builds `Event::PaneLifecycleChanged` (rust-sdk.md "Events").
pub fn pane_lifecycle_changed(payload: PaneLifecycleEvent) -> Event {
    Event::PaneLifecycleChanged(payload)
}

/// Builds `Event::AttentionChanged` (rust-sdk.md "Events").
pub fn attention_changed(payload: AttentionEvent) -> Event {
    Event::AttentionChanged(payload)
}

/// Builds `Event::ClientRequest` (rust-sdk.md "Events").
pub fn client_request(payload: ClientRequestEvent) -> Event {
    Event::ClientRequest(payload)
}

/// Builds `Event::KernelSettingsChanged` (rust-sdk.md "Events").
pub fn kernel_settings_changed(payload: KernelSettingsEvent) -> Event {
    Event::KernelSettingsChanged(payload)
}

/// Builds `Event::KeymapChanged` (rust-sdk.md "Events").
pub fn keymap_changed() -> Event {
    Event::KeymapChanged
}

/// Builds `Event::DurableTimerFired` (rust-sdk.md "Events").
pub fn durable_timer_fired(payload: DurableTimerEvent) -> Event {
    Event::DurableTimerFired(payload)
}

/// Builds `Event::InputUncertain` (rust-sdk.md "Events").
pub fn input_uncertain(payload: InputUncertainEvent) -> Event {
    Event::InputUncertain(payload)
}

/// Builds `Event::DisplayModeChanged` (rust-sdk.md "Events").
pub fn display_mode_changed(payload: Option<String>) -> Event {
    Event::DisplayModeChanged(payload)
}

#[cfg(test)]
mod spec_tests {
    use super::*;

    /// REQ: EVT-044
    #[test]
    fn activity_carries_node_and_both_states() {
        let event = activity(7, AgentActivity::Working, AgentActivity::Idle);
        match event {
            Event::AgentActivityChanged(payload) => {
                assert_eq!(payload.node, NodeId(7));
                assert_eq!(payload.previous, AgentActivity::Working);
                assert_eq!(payload.current, AgentActivity::Idle);
            }
            other => panic!("expected AgentActivityChanged, got {other:?}"),
        }
    }

    /// REQ: EVT-044
    #[test]
    fn activity_accepts_every_agent_activity_state() {
        let all_states = [
            AgentActivity::None,
            AgentActivity::Working,
            AgentActivity::Idle,
            AgentActivity::AwaitingApproval,
            AgentActivity::Done,
            AgentActivity::Menu,
        ];
        for state in all_states {
            match activity(1, AgentActivity::None, state) {
                Event::AgentActivityChanged(payload) => assert_eq!(payload.current, state),
                other => panic!("expected AgentActivityChanged, got {other:?}"),
            }
        }
    }

    #[test]
    fn command_carries_name_and_arguments_json() {
        let event = command("example.plugin/start", r#"{"count":2}"#);
        match event {
            Event::CommandInvoked(payload) => {
                assert_eq!(payload.command, "example.plugin/start");
                assert_eq!(payload.arguments_json, r#"{"count":2}"#);
            }
            other => panic!("expected CommandInvoked, got {other:?}"),
        }
    }

    #[test]
    fn command_passes_empty_and_malformed_arguments_through_verbatim() {
        for arguments in ["", "{not json"] {
            match command("example.plugin/start", arguments) {
                Event::CommandInvoked(payload) => assert_eq!(payload.arguments_json, arguments),
                other => panic!("expected CommandInvoked, got {other:?}"),
            }
        }
    }

    /// REQ: EVT-049
    #[test]
    fn tree_changed_keeps_first_seen_node_order() {
        let event = tree_changed(vec![NodeId(5), NodeId(3)]);
        assert_eq!(event, Event::TreeChanged(vec![NodeId(5), NodeId(3)]));
    }

    /// REQ: EVT-041
    #[test]
    fn node_removed_wraps_the_removed_node_id() {
        assert_eq!(node_removed(NodeId(3)), Event::NodeRemoved(NodeId(3)));
    }

    /// Manual: payload-free events
    #[test]
    fn payload_free_events_have_no_fields() {
        assert_eq!(theme_changed(), Event::ThemeChanged);
        assert_eq!(keymap_changed(), Event::KeymapChanged);
    }

    /// REQ: EVT-044
    #[test]
    fn agent_activity_changed_keeps_payload_unchanged() {
        let payload = AgentActivityEvent {
            node: NodeId(9),
            previous: AgentActivity::Idle,
            current: AgentActivity::AwaitingApproval,
            revision: 42,
        };
        assert_eq!(
            agent_activity_changed(payload.clone()),
            Event::AgentActivityChanged(payload)
        );
    }

    #[test]
    fn command_invoked_builder_keeps_origin_and_context() {
        let payload = CommandInvokedEvent {
            command: "example.plugin/open".to_string(),
            arguments_json: "{}".to_string(),
            origin: CommandOrigin::Palette,
            context: CommandContext::default(),
        };
        assert_eq!(
            command_invoked(payload.clone()),
            Event::CommandInvoked(payload)
        );
    }
}
