//! Unit tests for public SDK items that had no coverage. Expectations follow the
//! documented behaviour (plugins manual, WIT reference, limits). Items that are
//! still stubs (`unimplemented!`) are expected to fail until they are built.

use std::cell::Cell;
use std::fmt::Debug;
use std::rc::Rc;

use ilium_plugin_sdk_core::generated::host::Level;
use ilium_plugin_sdk_core::generated::types::*;
use ilium_plugin_sdk_core::{dedup, events, intents, log, plugin, request, task, ui};

/// The SDK correlation range starts at 2^63; values below belong to the plugin.
const SDK_CORRELATION_FLOOR: u64 = 1 << 63;
const MAX_ERROR_MESSAGE_CHARS: usize = 256;
const MAX_LOG_BYTES: usize = 16 * 1024;
const MAX_UI_NODES: usize = 4096;
const MAX_UI_DEPTH: u32 = 32;
const MAX_DISABLED_REASON_CHARS: usize = 40;

fn check_intent<T: Clone + PartialEq + Debug>(
    build: fn(T) -> Intent,
    wrap: fn(T) -> Intent,
    payload: T,
) {
    assert_eq!(build(payload.clone()), wrap(payload));
}

fn check_event<T: Clone + PartialEq + Debug>(
    build: fn(T) -> Event,
    wrap: fn(T) -> Event,
    payload: T,
) {
    assert_eq!(build(payload.clone()), wrap(payload));
}

fn check_widget<T: Clone + PartialEq + Debug>(
    build: fn(T) -> Widget,
    wrap: fn(T) -> Widget,
    payload: T,
) {
    assert_eq!(build(payload.clone()), wrap(payload));
}

fn expect_finished(result: Result<UiTree, PluginError>, context: &str) -> UiTree {
    match result {
        Ok(tree) => tree,
        Err(error) => panic!("{context}: finish refused the tree with {error:?}"),
    }
}

fn chain_tree(levels: u32) -> ui::build::Tree {
    let mut tree = ui::build::Tree::new();
    for index in 0..levels {
        let mut builder = tree.node(ui::widgets::clear());
        if index + 1 < levels {
            builder = builder.child(index + 1);
        }
        builder.done();
    }
    tree
}

fn sample_rename() -> RenameNodeSpec {
    RenameNodeSpec {
        node: NodeId(1),
        title: "build".to_string(),
        source: TitleSource::Plugin,
        expected_revision: None,
    }
}

fn sample_move() -> MoveNodeSpec {
    MoveNodeSpec {
        node: NodeId(1),
        target: NodeId(2),
        placement: Placement::After,
    }
}

fn sample_signal() -> SignalPaneSpec {
    SignalPaneSpec {
        pane: NodeId(1),
        signal: Signal::Interrupt,
    }
}

// The generated spec and event types carry no Default: several fields name ids or enums with no
// empty value. These builders hold the same empty-or-first-variant values the old defaults meant.
fn sample_send_input() -> SendInputSpec {
    SendInputSpec {
        pane: NodeId(1),
        text: String::new(),
        delivery: Delivery::Typed,
        prompt_epoch: None,
        stages: Vec::new(),
        raw: None,
        mouse: None,
        if_current: None,
        lock_user_input: false,
        journal: false,
    }
}

fn sample_create_group() -> CreateGroupSpec {
    CreateGroupSpec {
        parent: None,
        title: String::new(),
        placement: Placement::After,
        children: Vec::new(),
    }
}

fn sample_create_split() -> CreateSplitSpec {
    CreateSplitSpec {
        panes: Vec::new(),
        direction: SplitDirection::Horizontal,
    }
}

fn sample_title_changed() -> PaneTitleChangedEvent {
    PaneTitleChangedEvent {
        node: NodeId(1),
        title: String::new(),
        source: TitleOrigin::Manual,
    }
}

fn sample_agent_activity() -> AgentActivityEvent {
    AgentActivityEvent {
        node: NodeId(1),
        previous: AgentActivity::None,
        current: AgentActivity::Working,
        revision: 0,
    }
}

fn sample_editor_pointer() -> EditorPointerEvent {
    EditorPointerEvent {
        buffer: BufferId(1),
        pane: NodeId(1),
        position: TextPosition { line: 0, column: 0 },
        pointer: ViewPointer {
            kind: PointerKind::Down,
            button: PointerButton::Left,
            modifiers: PointerModifiers(0),
            column: 0,
            row: 0,
            sub_x: 0,
            sub_y: 0,
            clicks: 1,
        },
    }
}

fn sample_message_event() -> MessageEvent {
    MessageEvent {
        from: PluginId("ilium.core.tree".to_string()),
        from_side: PluginSide::Server,
        from_client: None,
        channel: String::new(),
        payload: Vec::new(),
    }
}

fn sample_pane_lifecycle() -> PaneLifecycleEvent {
    PaneLifecycleEvent {
        pane: NodeId(1),
        state: PaneLifecycleState::Running,
        by: None,
    }
}

fn sample_decoration() -> SetDecorationSpec {
    SetDecorationSpec {
        node: NodeId(1),
        slot: "badge".to_string(),
        content: Vec::new(),
        icon: None,
        sort_key: None,
        persist: false,
        spans: None,
        tooltip: None,
        frames: Vec::new(),
        frame_ms: 0,
        trailing: false,
        visibility: DecorationVisibility::Always,
        tone: None,
    }
}

fn sample_notify() -> NotifySpec {
    NotifySpec {
        title: "Done".to_string(),
        body: None,
        urgency: Urgency::Normal,
        sound: None,
        coalesce_key: None,
        coalesce_window_ms: 0,
    }
}

fn sample_open_view() -> OpenViewSpec {
    OpenViewSpec {
        view: ViewTarget::Panel("status".to_string()),
        focus: true,
    }
}

fn sample_message() -> MessageSpec {
    MessageSpec {
        to: MessageTarget {
            plugin: PluginId("other.plugin".to_string()),
            side: PluginSide::Server,
            client: None,
        },
        channel: "ping".to_string(),
        payload: vec![1, 2, 3],
    }
}

fn sample_editor_open() -> EditorOpenSpec {
    EditorOpenSpec {
        path: FsPath::default(),
        line: Some(3),
        placement: EditorPlacement::Split,
    }
}

fn sample_set_expanded() -> SetExpandedSpec {
    SetExpandedSpec {
        target: ExpandTarget::Node(NodeId(1)),
        expanded: true,
    }
}

fn sample_open_external() -> OpenExternalSpec {
    OpenExternalSpec {
        target: ExternalTarget::Url("https://example.test/".to_string()),
    }
}

fn sample_toast() -> ToastSpec {
    ToastSpec {
        id: None,
        tone: Tone::Info,
        title: "Saved".to_string(),
        body: None,
        actions: Vec::new(),
        duration_ms: None,
    }
}

// ---------------------------------------------------------------------------
// intents: Submission, PluginError, Outcome
// ---------------------------------------------------------------------------

#[test]
fn submission_new_assigns_sdk_correlation_and_no_dedup_key() {
    let intent = Intent::CloseNode(NodeId(9));
    let first = Submission::new(intent.clone());
    let second = Submission::new(intent.clone());
    assert_eq!(first.intent, intent);
    assert!(
        first.correlation >= SDK_CORRELATION_FLOOR,
        "SDK correlations start at 2^63, got {}",
        first.correlation
    );
    assert_eq!(first.dedup_key, None);
    assert_ne!(first.correlation, second.correlation);
}

#[test]
fn submission_keyed_keeps_dedup_key_and_sdk_correlation() {
    let intent = Intent::CloseNode(NodeId(9));
    let submission = Submission::keyed(intent.clone(), "freeze:7");
    assert_eq!(submission.intent, intent);
    assert_eq!(submission.dedup_key, Some("freeze:7".to_string()));
    assert!(submission.correlation >= SDK_CORRELATION_FLOOR);
}

#[test]
fn outcome_is_ok_only_for_success_variants() {
    assert!(Outcome::Ok.is_ok());
    assert!(Outcome::OkNode(NodeId(1)).is_ok());
    assert!(Outcome::OkEpoch(3).is_ok());
    assert!(!Outcome::Cancelled.is_ok());
    assert!(!Outcome::Refused(PluginError::not_permitted("no")).is_ok());
    assert!(!Outcome::Failed(PluginError::not_found("gone")).is_ok());
}

#[test]
fn plugin_error_new_cuts_long_message_to_256_chars() {
    let error = PluginError::new(ErrorCode::Busy, "x".repeat(300));
    assert_eq!(error.code, ErrorCode::Busy);
    assert_eq!(error.message.chars().count(), MAX_ERROR_MESSAGE_CHARS);
    assert_eq!(error.retry_after_ms, None);
}

#[test]
fn plugin_error_new_keeps_short_message_whole() {
    let error = PluginError::new(ErrorCode::NotFound, "gone");
    assert_eq!(error.code, ErrorCode::NotFound);
    assert_eq!(error.message, "gone");
}

#[test]
fn plugin_error_named_constructors_set_their_codes() {
    assert_eq!(PluginError::timed_out("slow").code, ErrorCode::TimedOut);
    assert_eq!(
        PluginError::invalid_argument("bad").code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(PluginError::not_found("missing").code, ErrorCode::NotFound);
    assert_eq!(
        PluginError::not_permitted("denied").code,
        ErrorCode::NotPermitted
    );
    assert_eq!(PluginError::timed_out("slow").message, "slow");
}

#[test]
fn plugin_error_busy_sets_code_message_and_retry_delay() {
    let error = PluginError::busy("pane is starting", 250);
    assert_eq!(error.code, ErrorCode::Busy);
    assert_eq!(error.message, "pane is starting");
    assert_eq!(error.retry_after_ms, Some(250));
}

// ---------------------------------------------------------------------------
// intents: Intent constructors wrap their payloads in the matching variant
// ---------------------------------------------------------------------------

#[test]
fn intent_pane_and_node_constructors_wrap_payloads() {
    check_intent(intents::rename_node, Intent::RenameNode, sample_rename());
    check_intent(intents::move_node, Intent::MoveNode, sample_move());
    check_intent(intents::close_node, Intent::CloseNode, NodeId(4));
    check_intent(
        intents::set_node_locked,
        Intent::SetNodeLocked,
        SetNodeLockedSpec::default(),
    );
    check_intent(
        intents::create_pane,
        Intent::CreatePane,
        CreatePaneSpec::default(),
    );
    check_intent(intents::signal_pane, Intent::SignalPane, sample_signal());
    check_intent(
        intents::send_input,
        Intent::SendInput,
        sample_send_input(),
    );
    check_intent(
        intents::agent_action,
        Intent::AgentAction,
        AgentActionSpec::default(),
    );
    check_intent(
        intents::set_decoration,
        Intent::SetDecoration,
        sample_decoration(),
    );
    check_intent(
        intents::clear_decoration,
        Intent::ClearDecoration,
        DecorationKey::default(),
    );
    check_intent(
        intents::set_virtual_rows,
        Intent::SetVirtualRows,
        SetVirtualRowsSpec::default(),
    );
}

#[test]
fn intent_sound_view_command_and_editor_constructors_wrap_payloads() {
    check_intent(intents::play_sound, Intent::PlaySound, "chime".to_string());
    check_intent(intents::notify, Intent::Notify, sample_notify());
    check_intent(intents::open_view, Intent::OpenView, sample_open_view());
    check_intent(
        intents::close_view,
        Intent::CloseView,
        ViewTarget::Popup("help".to_string()),
    );
    check_intent(
        intents::invoke_command,
        Intent::InvokeCommand,
        InvokeCommandSpec::default(),
    );
    check_intent(intents::message, Intent::Message, sample_message());
    check_intent(
        intents::editor_apply_edits,
        Intent::EditorApplyEdits,
        EditorApplyEditsSpec::default(),
    );
    check_intent(
        intents::editor_open,
        Intent::EditorOpen,
        sample_editor_open(),
    );
}

#[test]
fn intent_pane_process_constructors_wrap_payloads() {
    check_intent(
        intents::freeze_pane,
        Intent::FreezePane,
        FreezePaneSpec::default(),
    );
    check_intent(
        intents::unfreeze_pane,
        Intent::UnfreezePane,
        UnfreezePaneSpec::default(),
    );
    check_intent(
        intents::replace_pane_command,
        Intent::ReplacePaneCommand,
        ReplacePaneCommandSpec::default(),
    );
    check_intent(
        intents::terminate_pane_process,
        Intent::TerminatePaneProcess,
        TerminatePaneProcessSpec::default(),
    );
    check_intent(
        intents::resize_pane,
        Intent::ResizePane,
        ResizePaneSpec::default(),
    );
}

#[test]
fn intent_group_split_and_attention_constructors_wrap_payloads() {
    check_intent(
        intents::create_group,
        Intent::CreateGroup,
        sample_create_group(),
    );
    check_intent(
        intents::create_folder,
        Intent::CreateFolder,
        CreateFolderSpec::default(),
    );
    check_intent(
        intents::create_project,
        Intent::CreateProject,
        CreateProjectSpec::default(),
    );
    check_intent(
        intents::create_split,
        Intent::CreateSplit,
        sample_create_split(),
    );
    check_intent(
        intents::set_bookmarked,
        Intent::SetBookmarked,
        SetBookmarkedSpec::default(),
    );
    check_intent(
        intents::set_expanded,
        Intent::SetExpanded,
        sample_set_expanded(),
    );
    check_intent(
        intents::restructure,
        Intent::Restructure,
        RestructureSpec::default(),
    );
    check_intent(intents::revert_restructure, Intent::RevertRestructure, 12);
    check_intent(
        intents::set_inferred_icon,
        Intent::SetInferredIcon,
        SetInferredIconSpec::default(),
    );
    check_intent(
        intents::mark_attention,
        Intent::MarkAttention,
        MarkAttentionSpec::default(),
    );
    check_intent(intents::ack_attention, Intent::AckAttention, NodeId(5));
    check_intent(
        intents::open_external,
        Intent::OpenExternal,
        sample_open_external(),
    );
}

#[test]
fn intent_status_frame_and_toast_constructors_wrap_payloads() {
    check_intent(
        intents::set_status_segment,
        Intent::SetStatusSegment,
        SetStatusSegmentSpec::default(),
    );
    check_intent(
        intents::clear_status_segment,
        Intent::ClearStatusSegment,
        "branch".to_string(),
    );
    check_intent(
        intents::set_frame_slot,
        Intent::SetFrameSlot,
        SetFrameSlotSpec::default(),
    );
    check_intent(
        intents::clear_frame_slot,
        Intent::ClearFrameSlot,
        ClearFrameSlotSpec::default(),
    );
    check_intent(intents::toast, Intent::Toast, sample_toast());
    check_intent(
        intents::dismiss_toast,
        Intent::DismissToast,
        "saved-toast".to_string(),
    );
}

// ---------------------------------------------------------------------------
// events: builders
// ---------------------------------------------------------------------------

#[test]
fn activity_builder_maps_node_number_and_both_states() {
    let event = events::activity(7, AgentActivity::Idle, AgentActivity::Working);
    match event {
        Event::AgentActivityChanged(payload) => {
            assert_eq!(payload.node, NodeId(7));
            assert_eq!(payload.previous, AgentActivity::Idle);
            assert_eq!(payload.current, AgentActivity::Working);
        }
        other => panic!("expected AgentActivityChanged, got {other:?}"),
    }
}

#[test]
fn command_builder_wraps_name_and_arguments_json() {
    let event = events::command("palette.open", "{\"query\":\"log\"}");
    match event {
        Event::CommandInvoked(payload) => {
            assert_eq!(payload.command, "palette.open");
            assert_eq!(payload.arguments_json, "{\"query\":\"log\"}");
        }
        other => panic!("expected CommandInvoked, got {other:?}"),
    }
}

#[test]
fn event_tree_pane_and_agent_constructors_wrap_payloads() {
    check_event(
        events::tree_changed,
        Event::TreeChanged,
        vec![NodeId(1), NodeId(2)],
    );
    check_event(events::node_removed, Event::NodeRemoved, NodeId(3));
    check_event(
        events::pane_created,
        Event::PaneCreated,
        PaneInfo {
            node: NodeId(1),
            content_kind: "terminal".to_string(),
            primitive: PrimitiveKind::Terminal,
            columns: 80,
            rows: 24,
            focused: true,
            title: "shell".to_string(),
            is_restored: false,
        },
    );
    check_event(
        events::pane_exited,
        Event::PaneExited,
        PaneExitedEvent::default(),
    );
    check_event(events::pane_focused, Event::PaneFocused, Some(NodeId(3)));
    check_event(
        events::pane_resized,
        Event::PaneResized,
        PaneResizedEvent::default(),
    );
    check_event(
        events::pane_title_changed,
        Event::PaneTitleChanged,
        sample_title_changed(),
    );
    check_event(
        events::agent_identified,
        Event::AgentIdentified,
        AgentIdentifiedEvent::default(),
    );
    check_event(
        events::agent_activity_changed,
        Event::AgentActivityChanged,
        sample_agent_activity(),
    );
    check_event(
        events::agent_goal_changed,
        Event::AgentGoalChanged,
        AgentGoalEvent::default(),
    );
}

#[test]
fn event_input_output_pointer_and_editor_constructors_wrap_payloads() {
    check_event(
        events::input_submitted,
        Event::InputSubmitted,
        InputSubmittedEvent {
            node: NodeId(1),
            text: "ls".to_string(),
            source: InputSource::User,
            epoch: 1,
        },
    );
    check_event(
        events::output_matched,
        Event::OutputMatched,
        OutputMatchedEvent::default(),
    );
    check_event(
        events::link_activated,
        Event::LinkActivated,
        LinkActivatedEvent::default(),
    );
    check_event(
        events::pointer,
        Event::Pointer,
        PointerEvent {
            node: NodeId(1),
            row: 2,
            column: 3,
            kind: PointerKind::Down,
            button: PointerButton::Left,
            modifiers: PointerModifiers(0),
            sub_x: 0,
            sub_y: 0,
        },
    );
    check_event(
        events::editor_opened,
        Event::EditorOpened,
        BufferRef::default(),
    );
    check_event(
        events::editor_closed,
        Event::EditorClosed,
        BufferRef::default(),
    );
    check_event(
        events::editor_changed,
        Event::EditorChanged,
        EditorChangedEvent::default(),
    );
    check_event(
        events::editor_saved,
        Event::EditorSaved,
        EditorSavedEvent::default(),
    );
    check_event(
        events::editor_pointer,
        Event::EditorPointer,
        sample_editor_pointer(),
    );
    check_event(
        events::editor_viewport_changed,
        Event::EditorViewportChanged,
        EditorViewport::default(),
    );
}

#[test]
fn event_settings_request_timer_job_and_topic_constructors_wrap_payloads() {
    check_event(
        events::settings_changed,
        Event::SettingsChanged,
        SettingsChange {
            keys: vec!["theme".to_string()],
            origin: SettingsOrigin::User,
            side: PluginSide::Server,
        },
    );
    check_event(events::timer_fired, Event::TimerFired, TimerId(4));
    check_event(
        events::request_completed,
        Event::RequestCompleted,
        RequestResult {
            request: RequestRef::Submission(5),
            outcome: Outcome::Ok,
        },
    );
    check_event(
        events::job_progress,
        Event::JobProgress,
        JobProgressEvent::default(),
    );
    check_event(events::message, Event::Message, sample_message_event());
    check_event(
        events::topic_published,
        Event::TopicPublished,
        TopicPublishedEvent::default(),
    );
    check_event(
        events::durable_timer_fired,
        Event::DurableTimerFired,
        DurableTimerEvent::default(),
    );
    check_event(
        events::input_uncertain,
        Event::InputUncertain,
        InputUncertainEvent::default(),
    );
}

#[test]
fn event_command_ui_keyboard_and_view_constructors_wrap_payloads() {
    check_event(
        events::command_invoked,
        Event::CommandInvoked,
        CommandInvokedEvent {
            command: "palette.open".to_string(),
            arguments_json: "{}".to_string(),
            origin: CommandOrigin::Palette,
            context: CommandContext::default(),
        },
    );
    check_event(
        events::ui_action,
        Event::UiAction,
        UiAction {
            view: ViewTarget::Panel("status".to_string()),
            key: "save".to_string(),
            action_id: "activate".to_string(),
            kind: ActionKind::Activated,
            value: None,
        },
    );
    check_event(events::key_pressed, Event::KeyPressed, KeyEvent::default());
    check_event(
        events::key_released,
        Event::KeyReleased,
        KeyEvent::default(),
    );
    check_event(
        events::view_visibility,
        Event::ViewVisibility,
        ViewVisibilityEvent {
            view: ViewTarget::Panel("status".to_string()),
            visible: true,
        },
    );
}

#[test]
fn event_fs_audio_cli_and_http_constructors_wrap_payloads() {
    check_event(
        events::fs_changed,
        Event::FsChanged,
        FsChangedEvent::default(),
    );
    check_event(
        events::audio_frames,
        Event::AudioFrames,
        AudioFramesEvent::default(),
    );
    check_event(
        events::cli_invoked,
        Event::CliInvoked,
        CliInvokedEvent::default(),
    );
    check_event(events::cli_cancelled, Event::CliCancelled, 7);
    check_event(
        events::http_called,
        Event::HttpCalled,
        HttpCalledEvent::default(),
    );
    check_event(
        events::http_stream_data,
        Event::HttpStreamData,
        HttpStreamDataEvent::default(),
    );
    check_event(
        events::http_stream_closed,
        Event::HttpStreamClosed,
        HttpStreamClosedEvent {
            stream: StreamId::default(),
            outcome: StreamOutcome::Ended,
        },
    );
}

#[test]
fn event_client_screen_signal_and_model_constructors_wrap_payloads() {
    check_event(events::client_attached, Event::ClientAttached, ClientId(1));
    check_event(events::client_detached, Event::ClientDetached, ClientId(1));
    check_event(
        events::screen_changed,
        Event::ScreenChanged,
        ScreenChangedEvent::default(),
    );
    check_event(
        events::user_line_submitted,
        Event::UserLineSubmitted,
        UserLineEvent::default(),
    );
    check_event(
        events::signal_changed,
        Event::SignalChanged,
        SignalChangedEvent::default(),
    );
    check_event(
        events::agent_model_changed,
        Event::AgentModelChanged,
        AgentModelEvent {
            pane: NodeId(1),
            model: AgentModel {
                model: "model-a".to_string(),
                provider: None,
                context_window: Some(8192),
                source: "statusline".to_string(),
            },
        },
    );
    check_event(
        events::attention_changed,
        Event::AttentionChanged,
        AttentionEvent::default(),
    );
    check_event(
        events::client_request,
        Event::ClientRequest,
        ClientRequestEvent::default(),
    );
    check_event(
        events::kernel_settings_changed,
        Event::KernelSettingsChanged,
        KernelSettingsEvent::default(),
    );
}

#[test]
fn event_child_output_exit_and_lifecycle_constructors_wrap_payloads() {
    check_event(
        events::child_output,
        Event::ChildOutput,
        ChildOutputEvent {
            child: ChildId::default(),
            stream: OutputStream::Stdout,
            bytes: b"hi".to_vec(),
        },
    );
    check_event(
        events::child_exited,
        Event::ChildExited,
        ChildExitedEvent {
            child: ChildId::default(),
            status: Some(0),
            signal: None,
        },
    );
    check_event(
        events::pane_lifecycle_changed,
        Event::PaneLifecycleChanged,
        sample_pane_lifecycle(),
    );
}

#[test]
fn event_unit_and_list_constructors_produce_their_variants() {
    assert_eq!(events::theme_changed(), Event::ThemeChanged);
    assert_eq!(events::keymap_changed(), Event::KeymapChanged);
    check_event(
        events::events_dropped,
        Event::EventsDropped,
        vec![DroppedCount::default()],
    );
    check_event(
        events::display_mode_changed,
        Event::DisplayModeChanged,
        Some("compact".to_string()),
    );
    check_event(
        events::display_mode_changed,
        Event::DisplayModeChanged,
        None,
    );
}

// ---------------------------------------------------------------------------
// dedup, task, log, request, plugin
// ---------------------------------------------------------------------------

#[test]
fn dedup_key_joins_parts_with_colon() {
    assert_eq!(dedup::key_for(&["freeze", "7"]), "freeze:7");
    assert_eq!(dedup::key_for(&["a", "b", "c"]), "a:b:c");
}

#[test]
fn dedup_single_part_is_unchanged() {
    assert_eq!(dedup::key_for(&["only"]), "only");
}

#[test]
fn dedup_key_distinguishes_parts_that_contain_the_separator() {
    // Concern: the manual defines no escaping for ':', so this may collide until one is added.
    assert_ne!(dedup::key_for(&["a:b", "c"]), dedup::key_for(&["a", "b:c"]));
}

#[test]
fn task_spawn_does_not_run_future_inline() {
    let ran = Rc::new(Cell::new(false));
    let ran_in_task = Rc::clone(&ran);
    task::spawn(async move {
        ran_in_task.set(true);
    });
    assert!(!ran.get(), "spawn must queue the future, not run it inline");
}

#[test]
fn log_accepts_message_over_16_kib_without_panicking() {
    let message = "a".repeat(MAX_LOG_BYTES + 4096);
    log::log(Level::Warn, &message);
}

#[test]
fn log_accepts_multibyte_message_over_limit_without_splitting_a_char() {
    let message = "\u{e9}".repeat(MAX_LOG_BYTES);
    log::log(Level::Error, &message);
}

#[test]
fn pending_request_take_from_removes_and_returns_matching_outcome() {
    let pending = request::PendingRequest {
        request: RequestRef::Submission(5),
    };
    let mut batch = vec![
        RequestResult {
            request: RequestRef::Submission(4),
            outcome: Outcome::Cancelled,
        },
        RequestResult {
            request: RequestRef::Submission(5),
            outcome: Outcome::Ok,
        },
    ];
    assert_eq!(pending.take_from(&mut batch), Some(Outcome::Ok));
    assert_eq!(
        batch.len(),
        1,
        "the matching completion must leave the batch"
    );
    assert_eq!(batch[0].request, RequestRef::Submission(4));
}

#[test]
fn pending_request_take_from_returns_none_when_absent() {
    let pending = request::PendingRequest {
        request: RequestRef::Submission(9),
    };
    let mut batch = vec![RequestResult {
        request: RequestRef::Submission(1),
        outcome: Outcome::Ok,
    }];
    assert_eq!(pending.take_from(&mut batch), None);
    assert_eq!(batch.len(), 1);
}

struct FakeHost;

#[test]
fn set_host_then_host_returns_a_handle() {
    plugin::set_host(Box::new(FakeHost));
    let _handle = plugin::host();
}

#[test]
fn set_host_twice_replaces_the_handle_without_panicking() {
    plugin::set_host(Box::new(FakeHost));
    plugin::set_host(Box::new(FakeHost));
    let _handle = plugin::host();
}

// ---------------------------------------------------------------------------
// ui: widget constructors
// ---------------------------------------------------------------------------

#[test]
fn widget_layout_to_dialog_constructors_wrap_specs() {
    check_widget(
        ui::widgets::layout,
        Widget::Layout,
        LayoutSpec {
            direction: Direction::Vertical,
            constraints: vec![Constraint::Min(1)],
            flex: Flex::Start,
            spacing: Spacing::Space(0),
            margin: Margin {
                horizontal: 0,
                vertical: 0,
            },
        },
    );
    check_widget(
        ui::widgets::block,
        Widget::Block,
        BlockSpec {
            borders: Borders(0),
            border_type: BorderType::Plain,
            titles: Vec::new(),
            padding: Padding::default(),
            style: Style::default(),
            border_style: Style::default(),
        },
    );
    check_widget(
        ui::widgets::popup,
        Widget::Popup,
        PopupSpec {
            id: "help".to_string(),
            title: None,
            modal: true,
            width: Constraint::Length(40),
            height: Constraint::Length(10),
        },
    );
    check_widget(
        ui::widgets::scrollbar,
        Widget::Scrollbar,
        ScrollbarSpec {
            orientation: ScrollbarOrientation::VerticalRight,
            target_key: "list".to_string(),
        },
    );
    check_widget(
        ui::widgets::split,
        Widget::Split,
        SplitSpec {
            direction: Direction::Horizontal,
            position: 10,
            min_first: 1,
            min_second: 1,
            resizable: true,
        },
    );
    check_widget(
        ui::widgets::separator,
        Widget::Separator,
        SeparatorSpec {
            direction: Direction::Vertical,
            label: None,
            style: Style::default(),
        },
    );
    check_widget(
        ui::widgets::dialog,
        Widget::Dialog,
        DialogSpec {
            title: Vec::new(),
            kind: DialogKind::Confirm,
            buttons: Vec::new(),
            width: Constraint::Percentage(50),
            height: Constraint::Percentage(30),
            message: None,
        },
    );
}

#[test]
fn widget_paragraph_to_diff_and_list_constructors_wrap_specs() {
    check_widget(
        ui::widgets::paragraph,
        Widget::Paragraph,
        ParagraphSpec {
            lines: Vec::new(),
            rich: None,
            wrap: None,
            alignment: Alignment::Left,
            style: Style::default(),
            scrollable: false,
        },
    );
    check_widget(
        ui::widgets::diff_view,
        Widget::DiffView,
        DiffViewSpec {
            hunks: Vec::new(),
            layout: DiffLayout::Unified,
            language: None,
            word_diff: false,
        },
    );
    check_widget(
        ui::widgets::list,
        Widget::List,
        ListSpec {
            items: Vec::new(),
            highlight_symbol: String::new(),
            highlight_style: Style::default(),
            highlight_spacing: HighlightSpacing::Never,
            repeat_highlight_symbol: false,
            direction: ListDirection::TopToBottom,
            filter: false,
            filter_placeholder: None,
            multi_select: false,
            follow_tail: false,
            empty: None,
            item_tooltip: false,
        },
    );
    check_widget(
        ui::widgets::table,
        Widget::Table,
        TableSpec {
            columns: Vec::new(),
            rows: Vec::new(),
            sort: None,
            multi_select: false,
            follow_tail: false,
            column_spacing: 1,
            highlight_symbol: String::new(),
            highlight_spacing: HighlightSpacing::Never,
        },
    );
}

#[test]
fn widget_gauge_chart_and_canvas_constructors_wrap_specs() {
    check_widget(
        ui::widgets::line_gauge,
        Widget::LineGauge,
        LineGaugeSpec {
            ratio: 0.5,
            label: None,
            filled_style: Style::default(),
            unfilled_style: Style::default(),
            line_set: LineSet::Normal,
        },
    );
    check_widget(
        ui::widgets::sparkline,
        Widget::Sparkline,
        SparklineSpec {
            data: vec![Some(1), None, Some(3)],
            max: None,
            direction: RenderDirection::LeftToRight,
            style: Style::default(),
        },
    );
    check_widget(
        ui::widgets::bar_chart,
        Widget::BarChart,
        BarChartSpec {
            groups: Vec::new(),
            bar_width: 1,
            bar_gap: 0,
            group_gap: 0,
            direction: BarDirection::Vertical,
            max: None,
        },
    );
    check_widget(
        ui::widgets::canvas,
        Widget::Canvas,
        CanvasSpec {
            x_bounds: (0.0, 1.0),
            y_bounds: (0.0, 1.0),
            marker: Marker::Braille,
            shapes: Vec::new(),
        },
    );
    check_widget(
        ui::widgets::progress_row,
        Widget::ProgressRow,
        ProgressRowSpec {
            state: ProgressState::Running,
            title: Vec::new(),
            percent: Some(50.0),
            message: None,
            details: None,
        },
    );
    check_widget(
        ui::widgets::meter,
        Widget::Meter,
        MeterSpec {
            segments: Vec::new(),
            max: 100.0,
            label: None,
            show_values: false,
            orientation: Orientation::Horizontal,
        },
    );
}

#[test]
fn widget_input_and_media_constructors_wrap_specs() {
    check_widget(
        ui::widgets::button,
        Widget::Button,
        ButtonSpec {
            label: "Save".to_string(),
            icon: None,
            enabled: true,
            role: ButtonRole::Primary,
        },
    );
    check_widget(
        ui::widgets::toggle,
        Widget::Toggle,
        ToggleSpec {
            label: "Enabled".to_string(),
            value: false,
            style: ToggleStyle::Switch,
            tri_state: false,
            icon_on: None,
            icon_off: None,
        },
    );
    check_widget(
        ui::widgets::choice,
        Widget::Choice,
        ChoiceSpec {
            label: "Mode".to_string(),
            options: vec!["a".to_string()],
            selected: 0,
            style: ChoiceStyle::Stepper,
            items: Vec::new(),
            source: None,
            selected_id: None,
            filter: false,
            view_modes: Vec::new(),
            disabled_marker: DisabledMarker::Reason,
            allow_custom: false,
        },
    );
    check_widget(
        ui::widgets::number,
        Widget::Number,
        NumberSpec {
            label: "Size".to_string(),
            value: 1.0,
            minimum: 0.0,
            maximum: 10.0,
            step: 1.0,
            unit: None,
            style: NumberStyle::Slider,
            steps: Vec::new(),
            special_values: Vec::new(),
            scale: 1,
            debounce_ms: 0,
        },
    );
    check_widget(
        ui::widgets::path_field,
        Widget::PathField,
        PathFieldSpec {
            kind: PathKind::OpenFile,
            value: None,
            display: String::new(),
            extensions: Vec::new(),
            editable: false,
        },
    );
    check_widget(
        ui::widgets::badge,
        Widget::Badge,
        BadgeSpec {
            label: None,
            count: Some(3),
            dot: false,
            tone: Tone::Info,
        },
    );
    check_widget(
        ui::widgets::spinner,
        Widget::Spinner,
        SpinnerSpec {
            frames: SpinnerFrames::Braille,
            interval_ms: 80,
            label: None,
        },
    );
    check_widget(
        ui::widgets::notice,
        Widget::Notice,
        NoticeSpec {
            tone: Tone::Warning,
            title: None,
            text: Vec::new(),
            actions: Vec::new(),
            dismissible: false,
        },
    );
    check_widget(
        ui::widgets::image,
        Widget::Image,
        ImageSpec {
            source: ImageSource::Asset("logo".to_string()),
            fit: MediaFit::Contain,
        },
    );
    check_widget(
        ui::widgets::surface,
        Widget::Surface,
        SurfaceSpec {
            format: PixelFormat::Rgba8,
            min_columns: 1,
            min_rows: 1,
            glyph_plane: false,
            owner_ids: false,
        },
    );
    check_widget(
        ui::widgets::video,
        Widget::Video,
        VideoSpec {
            source: VideoSource::Asset("clip".to_string()),
            autoplay: false,
            looped: false,
            muted: true,
            controls: true,
            fit: MediaFit::Cover,
        },
    );
}

#[test]
fn widget_default_spec_constructors_wrap_specs() {
    check_widget(ui::widgets::tabs, Widget::Tabs, TabsSpec::default());
    check_widget(
        ui::widgets::scroll_view,
        Widget::ScrollView,
        ScrollViewSpec::default(),
    );
    check_widget(
        ui::widgets::markdown,
        Widget::Markdown,
        MarkdownSpec::default(),
    );
    check_widget(
        ui::widgets::leader_row,
        Widget::LeaderRow,
        LeaderSpec::default(),
    );
    check_widget(
        ui::widgets::tree_list,
        Widget::TreeList,
        TreeListSpec::default(),
    );
    check_widget(ui::widgets::menu, Widget::Menu, MenuSpec::default());
    check_widget(
        ui::widgets::check_list,
        Widget::CheckList,
        CheckListSpec::default(),
    );
    check_widget(
        ui::widgets::list_editor,
        Widget::ListEditor,
        ListEditorSpec::default(),
    );
    check_widget(ui::widgets::gauge, Widget::Gauge, GaugeSpec::default());
    check_widget(ui::widgets::chart, Widget::Chart, ChartSpec::default());
    check_widget(
        ui::widgets::calendar,
        Widget::Calendar,
        CalendarSpec::default(),
    );
    check_widget(
        ui::widgets::minimap,
        Widget::Minimap,
        MinimapSpec::default(),
    );
    check_widget(
        ui::widgets::text_input,
        Widget::TextInput,
        TextInputSpec::default(),
    );
    check_widget(
        ui::widgets::text_area,
        Widget::TextArea,
        TextAreaSpec::default(),
    );
    check_widget(
        ui::widgets::schema_form,
        Widget::SchemaForm,
        SchemaFormSpec::default(),
    );
    check_widget(
        ui::widgets::key_capture,
        Widget::KeyCapture,
        KeyCaptureSpec::default(),
    );
    check_widget(
        ui::widgets::color_picker,
        Widget::ColorPicker,
        ColorPickerSpec::default(),
    );
    check_widget(
        ui::widgets::location_picker,
        Widget::LocationPicker,
        LocationPickerSpec::default(),
    );
    check_widget(ui::widgets::icon, Widget::Icon, IconSpec::default());
    check_widget(
        ui::widgets::code_view,
        Widget::CodeView,
        CodeViewSpec::default(),
    );
    check_widget(
        ui::widgets::terminal_view,
        Widget::TerminalView,
        TerminalViewSpec::default(),
    );
}

#[test]
fn widget_clear_and_key_hints_constructors_produce_their_variants() {
    assert_eq!(ui::widgets::clear(), Widget::Clear);
    check_widget(
        ui::widgets::key_hints,
        Widget::KeyHints,
        vec![KeyHint {
            keys: "q".to_string(),
            label: "quit".to_string(),
            command: None,
        }],
    );
}

// ---------------------------------------------------------------------------
// ui: tree and node builders
// ---------------------------------------------------------------------------

#[test]
fn ui_tree_new_starts_empty() {
    let tree = ui::build::Tree::new();
    assert!(tree.is_empty());
    assert_eq!(tree.len(), 0);
}

#[test]
fn ui_node_builder_reports_its_index_and_tree_length() {
    let mut tree = ui::build::Tree::new();
    let builder = tree.node(ui::widgets::clear());
    assert_eq!(builder.index(), 0);
    assert_eq!(builder.tree_len(), 1);
    builder.done();
}

#[test]
fn ui_tree_finish_links_children_and_keeps_root() {
    let mut tree = ui::build::Tree::new();
    tree.node(ui::widgets::clear()).child(1).done();
    tree.node(ui::widgets::clear()).done();
    let built = expect_finished(tree.finish(0), "valid two-node tree");
    assert_eq!(built.root, 0);
    assert_eq!(built.nodes.len(), 2);
    assert_eq!(built.nodes[0].children, vec![1]);
}

#[test]
fn ui_tree_finish_rejects_empty_tree() {
    // Concern: the manual does not state this case; an empty tree has no root 0.
    assert!(ui::build::Tree::new().finish(0).is_err());
}

#[test]
fn ui_tree_finish_rejects_root_out_of_range() {
    let mut tree = ui::build::Tree::new();
    tree.node(ui::widgets::clear()).done();
    assert!(tree.finish(3).is_err());
}

#[test]
fn ui_tree_finish_rejects_child_index_out_of_range() {
    let mut tree = ui::build::Tree::new();
    tree.node(ui::widgets::clear()).child(9).done();
    assert!(tree.finish(0).is_err());
}

#[test]
fn ui_tree_finish_rejects_backward_child_link() {
    let mut tree = ui::build::Tree::new();
    tree.node(ui::widgets::clear()).child(1).done();
    tree.node(ui::widgets::clear()).child(0).done();
    assert!(tree.finish(0).is_err());
}

#[test]
fn ui_tree_finish_rejects_duplicate_key() {
    let mut tree = ui::build::Tree::new();
    tree.node(ui::widgets::clear()).key("dup").done();
    tree.node(ui::widgets::clear()).key("dup").done();
    assert!(tree.finish(0).is_err());
}

#[test]
fn ui_tree_depth_limit_is_32_levels() {
    assert!(chain_tree(MAX_UI_DEPTH).finish(0).is_ok());
    assert!(chain_tree(MAX_UI_DEPTH + 1).finish(0).is_err());
}

#[test]
fn ui_tree_node_limit_is_4096() {
    let mut at_limit = ui::build::Tree::new();
    for _ in 0..MAX_UI_NODES {
        at_limit.node(ui::widgets::clear()).done();
    }
    assert!(at_limit.finish(0).is_ok());

    let mut over_limit = ui::build::Tree::new();
    for _ in 0..=MAX_UI_NODES {
        over_limit.node(ui::widgets::clear()).done();
    }
    assert!(over_limit.finish(0).is_err());
}

#[test]
fn ui_node_builder_stores_key_flags_tooltip_and_tab_index() {
    let mut tree = ui::build::Tree::new();
    tree.node(ui::widgets::clear())
        .key("save")
        .flags(NodeFlags::FOCUSABLE)
        .tooltip(Tooltip::default())
        .tab_index(2)
        .done();
    let built = expect_finished(tree.finish(0), "single keyed node");
    let node = &built.nodes[0];
    assert_eq!(node.key, Some("save".to_string()));
    assert_eq!(node.flags, NodeFlags::FOCUSABLE);
    assert_eq!(node.tooltip, Some(Tooltip::default()));
    assert_eq!(node.tab_index, Some(2));
}

#[test]
fn ui_disabled_reason_is_capped_at_40_chars_or_refused() {
    let mut tree = ui::build::Tree::new();
    tree.node(ui::widgets::clear())
        .disabled_reason("x".repeat(60))
        .done();
    match tree.finish(0) {
        Err(_) => {}
        Ok(built) => {
            let reason = built.nodes[0].disabled_reason.clone().unwrap_or_default();
            assert!(
                reason.chars().count() <= MAX_DISABLED_REASON_CHARS,
                "disabled reason kept {} chars",
                reason.chars().count()
            );
        }
    }
}

// Not covered: `environment::{Side, Tier, Platform}` (`src/environment.rs`) is not
// declared in `src/lib.rs`, so these items are unreachable from integration tests.
