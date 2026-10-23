//! One builder per `Intent` case and `Submission` constructors (rust-sdk.md "Intents").
use crate::generated::types::*;

impl Submission {
    /// A submission without a dedup key; the SDK numbers `correlation` from 2^63 (rust-sdk.md "Submissions").
    pub fn new(_intent: Intent) -> Submission {
        unimplemented!("rust-sdk.submission-correlation")
    }

    /// A submission with an explicit dedup key (rust-sdk.md "Submissions").
    pub fn keyed(_intent: Intent, _dedup_key: impl Into<String>) -> Submission {
        unimplemented!("rust-sdk.submission-correlation")
    }
}

impl Outcome {
    /// True for `Ok`, `OkNode` and `OkEpoch` (rust-sdk.md "Outcomes").
    pub fn is_ok(&self) -> bool {
        matches!(self, Outcome::Ok | Outcome::OkNode(_) | Outcome::OkEpoch(_))
    }
}

/// Longest `PluginError` message in characters (reference-wit.md section 4.3).
pub const MAX_ERROR_MESSAGE_CHARS: usize = 256;

impl PluginError {
    /// Builds an error with the message cut to 256 characters.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> PluginError {
        let message: String = message
            .into()
            .chars()
            .take(MAX_ERROR_MESSAGE_CHARS)
            .collect();
        PluginError {
            code,
            message,
            retry_after_ms: None,
        }
    }

    /// `timed-out` error (rust-sdk.md "Errors").
    pub fn timed_out(message: impl Into<String>) -> PluginError {
        PluginError::new(ErrorCode::TimedOut, message)
    }

    /// `invalid-argument` error.
    pub fn invalid_argument(message: impl Into<String>) -> PluginError {
        PluginError::new(ErrorCode::InvalidArgument, message)
    }

    /// `not-found` error.
    pub fn not_found(message: impl Into<String>) -> PluginError {
        PluginError::new(ErrorCode::NotFound, message)
    }

    /// `not-permitted` error.
    pub fn not_permitted(message: impl Into<String>) -> PluginError {
        PluginError::new(ErrorCode::NotPermitted, message)
    }

    /// `busy` error with a retry hint.
    pub fn busy(message: impl Into<String>, retry_after_ms: u32) -> PluginError {
        PluginError {
            retry_after_ms: Some(retry_after_ms),
            ..PluginError::new(ErrorCode::Busy, message)
        }
    }
}

/// Builds `Intent::RenameNode` (rust-sdk.md "Intents").
pub fn rename_node(payload: RenameNodeSpec) -> Intent {
    Intent::RenameNode(payload)
}

/// Builds `Intent::MoveNode` (rust-sdk.md "Intents").
pub fn move_node(payload: MoveNodeSpec) -> Intent {
    Intent::MoveNode(payload)
}

/// Builds `Intent::CloseNode` (rust-sdk.md "Intents").
pub fn close_node(payload: NodeId) -> Intent {
    Intent::CloseNode(payload)
}

/// Builds `Intent::SetNodeLocked` (rust-sdk.md "Intents").
pub fn set_node_locked(payload: SetNodeLockedSpec) -> Intent {
    Intent::SetNodeLocked(payload)
}

/// Builds `Intent::CreatePane` (rust-sdk.md "Intents").
pub fn create_pane(payload: CreatePaneSpec) -> Intent {
    Intent::CreatePane(payload)
}

/// Builds `Intent::SignalPane` (rust-sdk.md "Intents").
pub fn signal_pane(payload: SignalPaneSpec) -> Intent {
    Intent::SignalPane(payload)
}

/// Builds `Intent::SendInput` (rust-sdk.md "Intents").
pub fn send_input(payload: SendInputSpec) -> Intent {
    Intent::SendInput(payload)
}

/// Builds `Intent::AgentAction` (rust-sdk.md "Intents").
pub fn agent_action(payload: AgentActionSpec) -> Intent {
    Intent::AgentAction(payload)
}

/// Builds `Intent::SetDecoration` (rust-sdk.md "Intents").
pub fn set_decoration(payload: SetDecorationSpec) -> Intent {
    Intent::SetDecoration(payload)
}

/// Builds `Intent::ClearDecoration` (rust-sdk.md "Intents").
pub fn clear_decoration(payload: DecorationKey) -> Intent {
    Intent::ClearDecoration(payload)
}

/// Builds `Intent::SetVirtualRows` (rust-sdk.md "Intents").
pub fn set_virtual_rows(payload: SetVirtualRowsSpec) -> Intent {
    Intent::SetVirtualRows(payload)
}

/// Builds `Intent::PlaySound` (rust-sdk.md "Intents").
pub fn play_sound(payload: String) -> Intent {
    Intent::PlaySound(payload)
}

/// Builds `Intent::Notify` (rust-sdk.md "Intents").
pub fn notify(payload: NotifySpec) -> Intent {
    Intent::Notify(payload)
}

/// Builds `Intent::OpenView` (rust-sdk.md "Intents").
pub fn open_view(payload: OpenViewSpec) -> Intent {
    Intent::OpenView(payload)
}

/// Builds `Intent::CloseView` (rust-sdk.md "Intents").
pub fn close_view(payload: ViewTarget) -> Intent {
    Intent::CloseView(payload)
}

/// Builds `Intent::InvokeCommand` (rust-sdk.md "Intents").
pub fn invoke_command(payload: InvokeCommandSpec) -> Intent {
    Intent::InvokeCommand(payload)
}

/// Builds `Intent::Message` (rust-sdk.md "Intents").
pub fn message(payload: MessageSpec) -> Intent {
    Intent::Message(payload)
}

/// Builds `Intent::EditorApplyEdits` (rust-sdk.md "Intents").
pub fn editor_apply_edits(payload: EditorApplyEditsSpec) -> Intent {
    Intent::EditorApplyEdits(payload)
}

/// Builds `Intent::EditorOpen` (rust-sdk.md "Intents").
pub fn editor_open(payload: EditorOpenSpec) -> Intent {
    Intent::EditorOpen(payload)
}

/// Builds `Intent::FreezePane` (rust-sdk.md "Intents").
pub fn freeze_pane(payload: FreezePaneSpec) -> Intent {
    Intent::FreezePane(payload)
}

/// Builds `Intent::UnfreezePane` (rust-sdk.md "Intents").
pub fn unfreeze_pane(payload: UnfreezePaneSpec) -> Intent {
    Intent::UnfreezePane(payload)
}

/// Builds `Intent::ReplacePaneCommand` (rust-sdk.md "Intents").
pub fn replace_pane_command(payload: ReplacePaneCommandSpec) -> Intent {
    Intent::ReplacePaneCommand(payload)
}

/// Builds `Intent::TerminatePaneProcess` (rust-sdk.md "Intents").
pub fn terminate_pane_process(payload: TerminatePaneProcessSpec) -> Intent {
    Intent::TerminatePaneProcess(payload)
}

/// Builds `Intent::ResizePane` (rust-sdk.md "Intents").
pub fn resize_pane(payload: ResizePaneSpec) -> Intent {
    Intent::ResizePane(payload)
}

/// Builds `Intent::CreateGroup` (rust-sdk.md "Intents").
pub fn create_group(payload: CreateGroupSpec) -> Intent {
    Intent::CreateGroup(payload)
}

/// Builds `Intent::CreateFolder` (rust-sdk.md "Intents").
pub fn create_folder(payload: CreateFolderSpec) -> Intent {
    Intent::CreateFolder(payload)
}

/// Builds `Intent::CreateProject` (rust-sdk.md "Intents").
pub fn create_project(payload: CreateProjectSpec) -> Intent {
    Intent::CreateProject(payload)
}

/// Builds `Intent::CreateSplit` (rust-sdk.md "Intents").
pub fn create_split(payload: CreateSplitSpec) -> Intent {
    Intent::CreateSplit(payload)
}

/// Builds `Intent::SetBookmarked` (rust-sdk.md "Intents").
pub fn set_bookmarked(payload: SetBookmarkedSpec) -> Intent {
    Intent::SetBookmarked(payload)
}

/// Builds `Intent::SetExpanded` (rust-sdk.md "Intents").
pub fn set_expanded(payload: SetExpandedSpec) -> Intent {
    Intent::SetExpanded(payload)
}

/// Builds `Intent::Restructure` (rust-sdk.md "Intents").
pub fn restructure(payload: RestructureSpec) -> Intent {
    Intent::Restructure(payload)
}

/// Builds `Intent::RevertRestructure` (rust-sdk.md "Intents").
pub fn revert_restructure(payload: u64) -> Intent {
    Intent::RevertRestructure(payload)
}

/// Builds `Intent::SetInferredIcon` (rust-sdk.md "Intents").
pub fn set_inferred_icon(payload: SetInferredIconSpec) -> Intent {
    Intent::SetInferredIcon(payload)
}

/// Builds `Intent::MarkAttention` (rust-sdk.md "Intents").
pub fn mark_attention(payload: MarkAttentionSpec) -> Intent {
    Intent::MarkAttention(payload)
}

/// Builds `Intent::AckAttention` (rust-sdk.md "Intents").
pub fn ack_attention(payload: NodeId) -> Intent {
    Intent::AckAttention(payload)
}

/// Builds `Intent::OpenExternal` (rust-sdk.md "Intents").
pub fn open_external(payload: OpenExternalSpec) -> Intent {
    Intent::OpenExternal(payload)
}

/// Builds `Intent::SetStatusSegment` (rust-sdk.md "Intents").
pub fn set_status_segment(payload: SetStatusSegmentSpec) -> Intent {
    Intent::SetStatusSegment(payload)
}

/// Builds `Intent::ClearStatusSegment` (rust-sdk.md "Intents").
pub fn clear_status_segment(payload: String) -> Intent {
    Intent::ClearStatusSegment(payload)
}

/// Builds `Intent::SetFrameSlot` (rust-sdk.md "Intents").
pub fn set_frame_slot(payload: SetFrameSlotSpec) -> Intent {
    Intent::SetFrameSlot(payload)
}

/// Builds `Intent::ClearFrameSlot` (rust-sdk.md "Intents").
pub fn clear_frame_slot(payload: ClearFrameSlotSpec) -> Intent {
    Intent::ClearFrameSlot(payload)
}

/// Builds `Intent::Toast` (rust-sdk.md "Intents").
pub fn toast(payload: ToastSpec) -> Intent {
    Intent::Toast(payload)
}

/// Builds `Intent::DismissToast` (rust-sdk.md "Intents").
pub fn dismiss_toast(payload: String) -> Intent {
    Intent::DismissToast(payload)
}

#[cfg(test)]
mod spec_tests {
    use super::*;

    /// Manual: Submission::new has no dedup key
    #[test]
    fn plain_submission_has_no_dedup_key_and_keeps_intent() {
        let submission = Submission::new(close_node(NodeId(7)));
        assert_eq!(submission.dedup_key, None);
        assert_eq!(submission.intent, Intent::CloseNode(NodeId(7)));
    }

    /// Manual: SDK numbers correlations from 2^63 upward
    #[test]
    fn plain_submissions_get_distinct_correlations_from_two_pow_63() {
        let first = Submission::new(close_node(NodeId(1)));
        let second = Submission::new(close_node(NodeId(2)));
        assert!(first.correlation >= 1u64 << 63);
        assert!(second.correlation >= 1u64 << 63);
        assert_ne!(first.correlation, second.correlation);
    }

    /// Manual: Submission::keyed sets the key
    #[test]
    fn keyed_submission_stores_dedup_key_and_correlation_from_two_pow_63() {
        let submission = Submission::keyed(play_sound("chime".to_string()), "toast.done");
        assert_eq!(submission.dedup_key.as_deref(), Some("toast.done"));
        assert_eq!(submission.intent, Intent::PlaySound("chime".to_string()));
        assert!(submission.correlation >= 1u64 << 63);
    }

    /// Manual: empty input: a message may be empty
    #[test]
    fn plugin_error_with_empty_message_keeps_code_and_no_retry_hint() {
        let error = PluginError::new(ErrorCode::Conflict, "");
        assert_eq!(error.code, ErrorCode::Conflict);
        assert_eq!(error.message, "");
        assert_eq!(error.retry_after_ms, None);
    }

    #[test]
    fn plugin_error_message_is_cut_to_256_characters_not_bytes() {
        let long_ascii = PluginError::new(ErrorCode::Internal, "a".repeat(300));
        assert_eq!(long_ascii.message.chars().count(), MAX_ERROR_MESSAGE_CHARS);
        let exact_multibyte = PluginError::new(ErrorCode::Internal, "é".repeat(256));
        assert_eq!(exact_multibyte.message, "é".repeat(256));
        let long_multibyte = PluginError::new(ErrorCode::Internal, "é".repeat(300));
        assert_eq!(long_multibyte.message.chars().count(), 256);
    }

    /// Manual: named constructors map to their error codes
    #[test]
    fn named_error_constructors_map_to_their_codes() {
        assert_eq!(PluginError::timed_out("slow").code, ErrorCode::TimedOut);
        assert_eq!(
            PluginError::invalid_argument("bad").code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(PluginError::not_found("gone").code, ErrorCode::NotFound);
        assert_eq!(
            PluginError::not_permitted("no").code,
            ErrorCode::NotPermitted
        );
        assert_eq!(PluginError::timed_out("slow").retry_after_ms, None);
    }

    /// Manual: busy error carries the retry hint
    #[test]
    fn busy_error_carries_retry_hint_in_milliseconds() {
        let error = PluginError::busy("pending requests full", 250);
        assert_eq!(error.code, ErrorCode::Busy);
        assert_eq!(error.retry_after_ms, Some(250));
        assert_eq!(error.message, "pending requests full");
    }

    #[test]
    fn is_ok_is_true_for_applied_outcomes_and_false_for_refused_failed_duplicate_cancelled() {
        assert!(Outcome::Ok.is_ok());
        assert!(Outcome::OkNode(NodeId(3)).is_ok());
        assert!(Outcome::OkEpoch(9).is_ok());
        assert!(!Outcome::Duplicate(RequestRef::Submission(5)).is_ok());
        assert!(!Outcome::Refused(PluginError::not_permitted("no")).is_ok());
        assert!(!Outcome::Failed(PluginError::timed_out("late")).is_ok());
        assert!(!Outcome::Cancelled.is_ok());
    }
}
