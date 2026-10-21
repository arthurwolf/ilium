//! Host implementation of `ilium:primitives/basic` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostBasic` trait; hand-owned afterwards.

use ilium_plugin_api::generated::primitives::basic::HostBasic;
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::ids::NodeId;
use ilium_plugin_api::generated::types::text::StyledText;
use ilium_plugin_api::generated::types::tree::AgentActivity;

/// Host side of the `basic` interface.
#[derive(Debug)]
pub struct BasicImport;

impl HostBasic for BasicImport {
    fn set_title(&mut self, _pane: NodeId, _title: String) -> Result<(), PluginError> {
        unimplemented!("host::imports::basic::set_title")
    }

    fn set_status(&mut self, _pane: NodeId, _status: StyledText) -> Result<(), PluginError> {
        unimplemented!("host::imports::basic::set_status")
    }

    fn request_focus(&mut self, _pane: NodeId) -> Result<(), PluginError> {
        unimplemented!("host::imports::basic::request_focus")
    }

    fn size(&mut self, _pane: NodeId) -> Option<(u16, u16)> {
        unimplemented!("host::imports::basic::size")
    }

    fn report_activity(
        &mut self,
        _pane: NodeId,
        _activity: AgentActivity,
    ) -> Result<(), PluginError> {
        unimplemented!("host::imports::basic::report_activity")
    }

    fn set_dirty(&mut self, _pane: NodeId, _dirty: bool) -> Result<(), PluginError> {
        unimplemented!("host::imports::basic::set_dirty")
    }
}

#[cfg(test)]
mod tests {
    use super::BasicImport;
    use ilium_plugin_api::generated::primitives::basic::HostBasic;
    use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
    use ilium_plugin_api::generated::types::ids::NodeId;
    use ilium_plugin_api::generated::types::text::{Span, Style, StyledText};
    use ilium_plugin_api::generated::types::tree::AgentActivity;

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

    /// REQ: none
    /// Manual: primitives-basic.md, Interface. A plain title on an own pane is accepted.
    #[test]
    fn set_title_accepts_plain_title_on_own_pane() {
        let mut import = BasicImport;
        let result = import.set_title(plugin_pane(), "Notes".to_owned());
        assert!(result.is_ok(), "expected Ok, got {result:?}");
    }

    /// REQ: none
    /// Manual: primitives-basic.md, Interface ("At most 256 characters"). Zero length is inside the range.
    #[test]
    fn set_title_accepts_empty_title() {
        let mut import = BasicImport;
        let result = import.set_title(plugin_pane(), String::new());
        assert!(result.is_ok(), "expected Ok for empty title, got {result:?}");
    }

    /// REQ: none
    /// Manual: primitives-basic.md, Interface. 256 characters is the inclusive upper bound.
    #[test]
    fn set_title_accepts_exactly_256_characters() {
        let mut import = BasicImport;
        let result = import.set_title(plugin_pane(), "t".repeat(256));
        assert!(result.is_ok(), "expected Ok at 256 characters, got {result:?}");
    }

    /// REQ: none
    /// Manual: primitives-basic.md, Errors and edge cases (`invalid-argument` for a title over 256 characters).
    #[test]
    fn set_title_rejects_257_characters_with_invalid_argument() {
        let mut import = BasicImport;
        let result = import.set_title(plugin_pane(), "t".repeat(257));
        assert_eq!(error_code_of(result), Some(ErrorCode::InvalidArgument));
    }

    /// REQ: none
    /// Manual: primitives-basic.md, Interface. The limit counts characters, not UTF-8 bytes:
    /// 256 two-byte characters are 512 bytes and must still be accepted.
    #[test]
    fn set_title_counts_characters_not_bytes() {
        let mut import = BasicImport;
        let result = import.set_title(plugin_pane(), "é".repeat(256));
        assert!(result.is_ok(), "expected Ok for 256 multi-byte characters, got {result:?}");
    }

    /// REQ: none
    /// Manual: primitives-basic.md, Errors and edge cases (`not-permitted` for a node that is not a pane of the plugin's kinds).
    #[test]
    fn set_title_on_foreign_node_is_not_permitted() {
        let mut import = BasicImport;
        let result = import.set_title(foreign_node(), "Notes".to_owned());
        assert_eq!(error_code_of(result), Some(ErrorCode::NotPermitted));
    }

    /// REQ: none
    /// Manual: primitives-basic.md, Interface. A single plain span is a valid one-line footer.
    #[test]
    fn set_status_accepts_single_plain_span() {
        let mut import = BasicImport;
        let result = import.set_status(plugin_pane(), plain_status("Saved"));
        assert!(result.is_ok(), "expected Ok, got {result:?}");
    }

    /// REQ: none
    /// Manual: primitives-basic.md, Interface. An empty status is inside the 0 to 256 character range.
    #[test]
    fn set_status_accepts_empty_status() {
        let mut import = BasicImport;
        let result = import.set_status(plugin_pane(), StyledText::new());
        assert!(result.is_ok(), "expected Ok for empty status, got {result:?}");
    }

    /// REQ: none
    /// Manual: primitives-basic.md, Interface. 256 characters is the inclusive upper bound.
    #[test]
    fn set_status_accepts_exactly_256_characters() {
        let mut import = BasicImport;
        let result = import.set_status(plugin_pane(), plain_status(&"s".repeat(256)));
        assert!(result.is_ok(), "expected Ok at 256 characters, got {result:?}");
    }

    /// REQ: none
    /// Manual: primitives-basic.md, Errors and edge cases (`invalid-argument` for a status over 256 characters).
    /// The 257 characters are split over two spans to show the limit applies to the whole line.
    #[test]
    fn set_status_rejects_257_characters_across_spans_with_invalid_argument() {
        let mut import = BasicImport;
        let status: StyledText = vec![
            Span { text: "a".repeat(128), style: Style::default() },
            Span { text: "b".repeat(129), style: Style::default() },
        ];
        let result = import.set_status(plugin_pane(), status);
        assert_eq!(error_code_of(result), Some(ErrorCode::InvalidArgument));
    }

    /// REQ: none
    /// Manual: primitives-basic.md, Errors and edge cases (`invalid-argument` for a status with a line break).
    #[test]
    fn set_status_rejects_line_break_with_invalid_argument() {
        let mut import = BasicImport;
        let result = import.set_status(plugin_pane(), plain_status("Saved\nnow"));
        assert_eq!(error_code_of(result), Some(ErrorCode::InvalidArgument));
    }

    /// REQ: none
    /// Manual: primitives-basic.md, Errors and edge cases (`not-permitted` for a node that is not a pane of the plugin's kinds).
    #[test]
    fn set_status_on_foreign_node_is_not_permitted() {
        let mut import = BasicImport;
        let result = import.set_status(foreign_node(), plain_status("Saved"));
        assert_eq!(error_code_of(result), Some(ErrorCode::NotPermitted));
    }

    /// REQ: none
    /// Manual: primitives-basic.md, Interface (`request-focus` asks the kernel to focus the pane).
    #[test]
    fn request_focus_on_own_pane_is_ok() {
        let mut import = BasicImport;
        let result = import.request_focus(plugin_pane());
        assert!(result.is_ok(), "expected Ok, got {result:?}");
    }

    /// REQ: none
    /// Manual: primitives-basic.md, Errors and edge cases (`not-permitted` for a node that is not a pane of the plugin's kinds).
    #[test]
    fn request_focus_on_foreign_node_is_not_permitted() {
        let mut import = BasicImport;
        let result = import.request_focus(foreign_node());
        assert_eq!(error_code_of(result), Some(ErrorCode::NotPermitted));
    }

    /// REQ: none
    /// Manual: primitives-basic.md, Interface. `size` returns the body columns and rows for an own pane.
    #[test]
    fn size_of_own_pane_is_some() {
        let mut import = BasicImport;
        let size = import.size(plugin_pane());
        assert!(size.is_some(), "expected Some((columns, rows)) for an own pane, got {size:?}");
    }

    /// REQ: none
    /// Manual: primitives-basic.md, Interface and Errors and edge cases: `size` returns `None` with no
    /// error code for a node that is not one of the plugin's panes.
    #[test]
    fn size_of_foreign_node_is_none() {
        let mut import = BasicImport;
        assert_eq!(import.size(foreign_node()), None);
    }

    /// REQ: none
    /// Manual: primitives-basic.md, Pane activity and dirty state. `report-activity` drives the activity icon.
    #[test]
    fn report_activity_working_on_own_pane_is_ok() {
        let mut import = BasicImport;
        let result = import.report_activity(plugin_pane(), AgentActivity::Working);
        assert!(result.is_ok(), "expected Ok, got {result:?}");
    }

    /// REQ: none
    /// Manual: primitives-basic.md, Pane activity and dirty state. `none` is a valid activity value.
    #[test]
    fn report_activity_none_on_own_pane_is_ok() {
        let mut import = BasicImport;
        let result = import.report_activity(plugin_pane(), AgentActivity::None);
        assert!(result.is_ok(), "expected Ok, got {result:?}");
    }

    /// REQ: none
    /// Manual: primitives-basic.md, Pane activity and dirty state (`not-permitted` for a node that is not the plugin's kind).
    #[test]
    fn report_activity_on_foreign_node_is_not_permitted() {
        let mut import = BasicImport;
        let result = import.report_activity(foreign_node(), AgentActivity::Working);
        assert_eq!(error_code_of(result), Some(ErrorCode::NotPermitted));
    }

    /// REQ: PRI-031
    /// Manual: primitives-basic.md, Pane activity and dirty state. `set-dirty` with true marks unsaved work.
    #[test]
    fn set_dirty_true_on_own_pane_is_ok() {
        let mut import = BasicImport;
        let result = import.set_dirty(plugin_pane(), true);
        assert!(result.is_ok(), "expected Ok, got {result:?}");
    }

    /// REQ: PRI-031
    /// Manual: primitives-basic.md, Pane activity and dirty state. Clearing the dirty flag is accepted.
    #[test]
    fn set_dirty_false_on_own_pane_is_ok() {
        let mut import = BasicImport;
        let result = import.set_dirty(plugin_pane(), false);
        assert!(result.is_ok(), "expected Ok, got {result:?}");
    }

    /// REQ: none
    /// Manual: primitives-basic.md, Pane activity and dirty state (`not-permitted` for a node that is not the plugin's kind).
    #[test]
    fn set_dirty_on_foreign_node_is_not_permitted() {
        let mut import = BasicImport;
        let result = import.set_dirty(foreign_node(), true);
        assert_eq!(error_code_of(result), Some(ErrorCode::NotPermitted));
    }
}
