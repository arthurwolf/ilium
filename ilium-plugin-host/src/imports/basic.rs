//! Checked host implementation of `ilium:primitives/basic`.
use std::sync::{Arc, Mutex, MutexGuard};
use ilium_plugin_api::generated::primitives::basic::HostBasic;
use ilium_plugin_api::generated::types::{errors::PluginError, ids::NodeId, text::StyledText, tree::AgentActivity};
use ilium_plugin_api::{ErrorCode, grants::Grant};

/// A backend must resolve the content kind through its authoritative kind registry,
/// require its exact owner and Basic primitive, and mutate under the same transaction.
/// The caller holds the shared grant gate through the complete backend operation.
/// Runtime kernels must supply an adapter; implementing this trait does not run a guest.
pub trait BasicBackend: Send + Sync {
    fn apply_basic(&self, plugin: &str, pane: NodeId, change: BasicChange) -> Result<(), PluginError>;
    fn basic_size(&self, plugin: &str, pane: NodeId) -> Option<(u16, u16)>;
}
pub enum BasicChange { Title(String), Status(StyledText), Focus, Activity(AgentActivity), Dirty(bool) }

pub struct BasicImport<'a> {
    backend: Arc<dyn BasicBackend>,
    plugin: String,
    // Every grant mutation must use this same gate. Lock order is authorization,
    // then backend transaction; a backend must never acquire this gate itself.
    grants: Arc<Mutex<Vec<Grant>>>,
    refusal: Box<dyn Fn(&str) + 'a>,
}
impl<'a> BasicImport<'a> {
    pub fn new(backend: Arc<dyn BasicBackend>, plugin: String, grants: Arc<Mutex<Vec<Grant>>>, refusal: impl Fn(&str) + 'a) -> Self {
        Self { backend, plugin, grants, refusal: Box::new(refusal) }
    }
    fn authorized(&self, function: &str) -> Result<MutexGuard<'_, Vec<Grant>>, PluginError> {
        let grants = self.grants.lock().map_err(|_| PluginError::new(ErrorCode::Internal, "authorization gate poisoned"))?;
        for grant in ["panes.provide-kind", "tree.metadata"] {
            if !Grant::parse(grant).is_ok_and(|needed| grants.iter().any(|current| current.covers(&needed))) {
                (self.refusal)(&format!("refused {grant} by basic.{function} (not-permitted)"));
                return Err(PluginError::new(ErrorCode::NotPermitted, "missing basic primitive grant"));
            }
        }
        Ok(grants)
    }
    fn apply(&self, pane: NodeId, function: &str, change: BasicChange) -> Result<(), PluginError> {
        let _authorization = self.authorized(function)?;
        self.apply_backend(pane, function, change)
    }
    fn apply_backend(&self, pane: NodeId, function: &str, change: BasicChange) -> Result<(), PluginError> {
        let result = self.backend.apply_basic(&self.plugin, pane, change);
        if result.as_ref().is_err_and(|error| error.code == ErrorCode::NotPermitted) {
            (self.refusal)(&format!("refused basic.{function}: not-permitted"));
        }
        result
    }
}
impl HostBasic for BasicImport<'_> {
    fn set_title(&mut self, pane: NodeId, title: String) -> Result<(), PluginError> {
        let _authorization = self.authorized("set-title")?;
        if title.chars().count() > 256 { return Err(PluginError::new(ErrorCode::InvalidArgument, "title exceeds 256 characters")); }
        self.apply_backend(pane, "set-title", BasicChange::Title(title))
    }
    fn set_status(&mut self, pane: NodeId, status: StyledText) -> Result<(), PluginError> {
        let _authorization = self.authorized("set-status")?;
        if status.iter().map(|span| span.text.chars().count()).sum::<usize>() > 256 || status.iter().any(|span| span.text.chars().any(|c| matches!(c, '\n' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}'))) {
            return Err(PluginError::new(ErrorCode::InvalidArgument, "status must be one line of at most 256 characters"));
        }
        self.apply_backend(pane, "set-status", BasicChange::Status(status))
    }
    fn request_focus(&mut self, pane: NodeId) -> Result<(), PluginError> { self.apply(pane, "request-focus", BasicChange::Focus) }
    fn size(&mut self, pane: NodeId) -> Option<(u16,u16)> { let _authorization = self.authorized("size").ok()?; self.backend.basic_size(&self.plugin,pane) }
    fn report_activity(&mut self,pane: NodeId,activity: AgentActivity)->Result<(),PluginError>{ self.apply(pane,"report-activity",BasicChange::Activity(activity)) }
    fn set_dirty(&mut self,pane: NodeId,dirty:bool)->Result<(),PluginError>{self.apply(pane,"set-dirty",BasicChange::Dirty(dirty))}
}

#[cfg(test)]
mod tests {
    use super::{BasicBackend, BasicChange, BasicImport};
    use ilium_plugin_api::grants::Grant;
    use std::sync::{Arc, Mutex};
    use ilium_plugin_api::generated::primitives::basic::HostBasic;
    use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
    use ilium_plugin_api::generated::types::ids::NodeId;
    use ilium_plugin_api::generated::types::text::{Span, Style, StyledText};
    use ilium_plugin_api::generated::types::tree::AgentActivity;

    /// Accepts changes on the plugin's own pane and refuses every other node.
    struct FakeBackend;

    impl BasicBackend for FakeBackend {
        fn apply_basic(&self, _plugin: &str, pane: NodeId, _change: BasicChange) -> Result<(), PluginError> {
            if pane == plugin_pane() {
                Ok(())
            } else {
                Err(PluginError::new(ErrorCode::NotPermitted, "not a pane of the plugin's kinds"))
            }
        }
        fn basic_size(&self, _plugin: &str, pane: NodeId) -> Option<(u16, u16)> {
            (pane == plugin_pane()).then_some((80, 24))
        }
    }

    /// An import holding both grants the basic primitive requires.
    fn granted_import() -> BasicImport<'static> {
        let grants = ["panes.provide-kind", "tree.metadata"]
            .iter()
            .map(|text| Grant::parse(text).expect("valid grant"))
            .collect();
        BasicImport::new(
            Arc::new(FakeBackend),
            "example.basic-test".to_owned(),
            Arc::new(Mutex::new(grants)),
            |_| {},
        )
    }

    /// Node of a pane that the plugin's basic kind owns (the host's pane registry decides this).
    fn plugin_pane() -> NodeId {
        NodeId(1)
    }

    /// Node that is not a pane of the plugin's kinds.
    fn foreign_node() -> NodeId {
        NodeId(2)
    }

    fn plain_status(text: &str) -> StyledText {
        vec![Span {
            text: text.to_owned(),
            style: Style::default(),
        }]
    }

    fn error_code_of(result: Result<(), PluginError>) -> Option<ErrorCode> {
        result.err().map(|error| error.code)
    }

    /// Manual: primitives-basic.md, Interface. A plain title on an own pane is accepted.
    #[test]
    fn set_title_accepts_plain_title_on_own_pane() {
        let mut import = granted_import();
        let result = import.set_title(plugin_pane(), "Notes".to_owned());
        assert!(result.is_ok(), "expected Ok, got {result:?}");
    }

    /// Manual: primitives-basic.md, Interface ("At most 256 characters"). Zero length is inside the range.
    #[test]
    fn set_title_accepts_empty_title() {
        let mut import = granted_import();
        let result = import.set_title(plugin_pane(), String::new());
        assert!(
            result.is_ok(),
            "expected Ok for empty title, got {result:?}"
        );
    }

    /// Manual: primitives-basic.md, Interface. 256 characters is the inclusive upper bound.
    #[test]
    fn set_title_accepts_exactly_256_characters() {
        let mut import = granted_import();
        let result = import.set_title(plugin_pane(), "t".repeat(256));
        assert!(
            result.is_ok(),
            "expected Ok at 256 characters, got {result:?}"
        );
    }

    /// Manual: primitives-basic.md, Errors and edge cases (`invalid-argument` for a title over 256 characters).
    #[test]
    fn set_title_rejects_257_characters_with_invalid_argument() {
        let mut import = granted_import();
        let result = import.set_title(plugin_pane(), "t".repeat(257));
        assert_eq!(error_code_of(result), Some(ErrorCode::InvalidArgument));
    }

    /// Manual: primitives-basic.md, Interface. The limit counts characters, not UTF-8 bytes:
    /// 256 two-byte characters are 512 bytes and must still be accepted.
    #[test]
    fn set_title_counts_characters_not_bytes() {
        let mut import = granted_import();
        let result = import.set_title(plugin_pane(), "é".repeat(256));
        assert!(
            result.is_ok(),
            "expected Ok for 256 multi-byte characters, got {result:?}"
        );
    }

    /// Manual: primitives-basic.md, Errors and edge cases (`not-permitted` for a node that is not a pane of the plugin's kinds).
    #[test]
    fn set_title_on_foreign_node_is_not_permitted() {
        let mut import = granted_import();
        let result = import.set_title(foreign_node(), "Notes".to_owned());
        assert_eq!(error_code_of(result), Some(ErrorCode::NotPermitted));
    }

    /// Manual: primitives-basic.md, Interface. A single plain span is a valid one-line footer.
    #[test]
    fn set_status_accepts_single_plain_span() {
        let mut import = granted_import();
        let result = import.set_status(plugin_pane(), plain_status("Saved"));
        assert!(result.is_ok(), "expected Ok, got {result:?}");
    }

    /// Manual: primitives-basic.md, Interface. An empty status is inside the 0 to 256 character range.
    #[test]
    fn set_status_accepts_empty_status() {
        let mut import = granted_import();
        let result = import.set_status(plugin_pane(), StyledText::new());
        assert!(
            result.is_ok(),
            "expected Ok for empty status, got {result:?}"
        );
    }

    /// Manual: primitives-basic.md, Interface. 256 characters is the inclusive upper bound.
    #[test]
    fn set_status_accepts_exactly_256_characters() {
        let mut import = granted_import();
        let result = import.set_status(plugin_pane(), plain_status(&"s".repeat(256)));
        assert!(
            result.is_ok(),
            "expected Ok at 256 characters, got {result:?}"
        );
    }

    /// Manual: primitives-basic.md, Errors and edge cases (`invalid-argument` for a status over 256 characters).
    /// The 257 characters are split over two spans to show the limit applies to the whole line.
    #[test]
    fn set_status_rejects_257_characters_across_spans_with_invalid_argument() {
        let mut import = granted_import();
        let status: StyledText = vec![
            Span {
                text: "a".repeat(128),
                style: Style::default(),
            },
            Span {
                text: "b".repeat(129),
                style: Style::default(),
            },
        ];
        let result = import.set_status(plugin_pane(), status);
        assert_eq!(error_code_of(result), Some(ErrorCode::InvalidArgument));
    }

    /// Manual: primitives-basic.md, Errors and edge cases (`invalid-argument` for a status with a line break).
    #[test]
    fn set_status_rejects_line_break_with_invalid_argument() {
        let mut import = granted_import();
        let result = import.set_status(plugin_pane(), plain_status("Saved\nnow"));
        assert_eq!(error_code_of(result), Some(ErrorCode::InvalidArgument));
    }

    /// Manual: primitives-basic.md, Errors and edge cases (`not-permitted` for a node that is not a pane of the plugin's kinds).
    #[test]
    fn set_status_on_foreign_node_is_not_permitted() {
        let mut import = granted_import();
        let result = import.set_status(foreign_node(), plain_status("Saved"));
        assert_eq!(error_code_of(result), Some(ErrorCode::NotPermitted));
    }

    /// Manual: primitives-basic.md, Interface (`request-focus` asks the kernel to focus the pane).
    #[test]
    fn request_focus_on_own_pane_is_ok() {
        let mut import = granted_import();
        let result = import.request_focus(plugin_pane());
        assert!(result.is_ok(), "expected Ok, got {result:?}");
    }

    /// Manual: primitives-basic.md, Errors and edge cases (`not-permitted` for a node that is not a pane of the plugin's kinds).
    #[test]
    fn request_focus_on_foreign_node_is_not_permitted() {
        let mut import = granted_import();
        let result = import.request_focus(foreign_node());
        assert_eq!(error_code_of(result), Some(ErrorCode::NotPermitted));
    }

    /// Manual: primitives-basic.md, Interface. `size` returns the body columns and rows for an own pane.
    #[test]
    fn size_of_own_pane_is_some() {
        let mut import = granted_import();
        let size = import.size(plugin_pane());
        assert!(
            size.is_some(),
            "expected Some((columns, rows)) for an own pane, got {size:?}"
        );
    }

    /// Manual: primitives-basic.md, Interface and Errors and edge cases: `size` returns `None` with no
    /// error code for a node that is not one of the plugin's panes.
    #[test]
    fn size_of_foreign_node_is_none() {
        let mut import = granted_import();
        assert_eq!(import.size(foreign_node()), None);
    }

    /// Manual: primitives-basic.md, Pane activity and dirty state. `report-activity` drives the activity icon.
    #[test]
    fn report_activity_working_on_own_pane_is_ok() {
        let mut import = granted_import();
        let result = import.report_activity(plugin_pane(), AgentActivity::Working);
        assert!(result.is_ok(), "expected Ok, got {result:?}");
    }

    /// Manual: primitives-basic.md, Pane activity and dirty state. `none` is a valid activity value.
    #[test]
    fn report_activity_none_on_own_pane_is_ok() {
        let mut import = granted_import();
        let result = import.report_activity(plugin_pane(), AgentActivity::None);
        assert!(result.is_ok(), "expected Ok, got {result:?}");
    }

    /// Manual: primitives-basic.md, Pane activity and dirty state (`not-permitted` for a node that is not the plugin's kind).
    #[test]
    fn report_activity_on_foreign_node_is_not_permitted() {
        let mut import = granted_import();
        let result = import.report_activity(foreign_node(), AgentActivity::Working);
        assert_eq!(error_code_of(result), Some(ErrorCode::NotPermitted));
    }

    /// Manual: primitives-basic.md, Pane activity and dirty state. `set-dirty` with true marks unsaved work.
    /// REQ: PRI-031
    #[test]
    fn set_dirty_true_on_own_pane_is_ok() {
        let mut import = granted_import();
        let result = import.set_dirty(plugin_pane(), true);
        assert!(result.is_ok(), "expected Ok, got {result:?}");
    }

    /// Manual: primitives-basic.md, Pane activity and dirty state. Clearing the dirty flag is accepted.
    /// REQ: PRI-031
    #[test]
    fn set_dirty_false_on_own_pane_is_ok() {
        let mut import = granted_import();
        let result = import.set_dirty(plugin_pane(), false);
        assert!(result.is_ok(), "expected Ok, got {result:?}");
    }

    /// Manual: primitives-basic.md, Pane activity and dirty state (`not-permitted` for a node that is not the plugin's kind).
    #[test]
    fn set_dirty_on_foreign_node_is_not_permitted() {
        let mut import = granted_import();
        let result = import.set_dirty(foreign_node(), true);
        assert_eq!(error_code_of(result), Some(ErrorCode::NotPermitted));
    }
}
