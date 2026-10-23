//! Host implementation of `ilium:primitives/editor` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostEditor` trait; hand-owned afterwards.

use ilium_plugin_api::generated::primitives::editor::{
    GutterMark, HostEditor, MinimapMark, RangeDecoration,
};
use ilium_plugin_api::generated::types::editing::{BufferRef, EditorViewport, TextRange};
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::ids::{BufferId, NodeId, RequestId};
use ilium_plugin_api::generated::types::paths::FsPath;

/// Host side of the `editor` interface.
#[derive(Debug)]
pub struct EditorImport;

impl HostEditor for EditorImport {
    fn buffers(&mut self) -> Vec<BufferRef> {
        unimplemented!("host::imports::editor::buffers")
    }

    fn buffer_for_pane(&mut self, _pane: NodeId) -> Option<BufferRef> {
        unimplemented!("host::imports::editor::buffer_for_pane")
    }

    fn read_lines(
        &mut self,
        _buffer: BufferId,
        _first_line: u32,
        _count: u32,
    ) -> Result<Vec<String>, PluginError> {
        unimplemented!("host::imports::editor::read_lines")
    }

    fn selections(&mut self, _buffer: BufferId) -> Vec<TextRange> {
        unimplemented!("host::imports::editor::selections")
    }

    fn set_decorations(
        &mut self,
        _buffer: BufferId,
        _layer: String,
        _items: Vec<RangeDecoration>,
    ) -> Result<(), PluginError> {
        unimplemented!("host::imports::editor::set_decorations")
    }

    fn set_gutter(
        &mut self,
        _buffer: BufferId,
        _layer: String,
        _marks: Vec<GutterMark>,
    ) -> Result<(), PluginError> {
        unimplemented!("host::imports::editor::set_gutter")
    }

    fn create_buffer(
        &mut self,
        _initial: String,
        _language: Option<String>,
    ) -> Result<BufferId, PluginError> {
        unimplemented!("host::imports::editor::create_buffer")
    }

    fn load_file(&mut self, _path: FsPath) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::editor::load_file")
    }

    fn take_buffer(&mut self, _request: RequestId) -> Option<BufferId> {
        unimplemented!("host::imports::editor::take_buffer")
    }

    fn save_buffer(
        &mut self,
        _buffer: BufferId,
        _path: Option<FsPath>,
    ) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::editor::save_buffer")
    }

    fn close_buffer(&mut self, _buffer: BufferId) {
        unimplemented!("host::imports::editor::close_buffer")
    }

    fn viewport(&mut self, _pane: NodeId) -> Option<EditorViewport> {
        unimplemented!("host::imports::editor::viewport")
    }

    fn set_minimap_marks(
        &mut self,
        _buffer: BufferId,
        _layer: String,
        _marks: Vec<MinimapMark>,
    ) -> Result<(), PluginError> {
        unimplemented!("host::imports::editor::set_minimap_marks")
    }
}

#[cfg(test)]
mod tests {
    use super::EditorImport;
    use ilium_plugin_api::generated::primitives::editor::{
        GutterMark, HostEditor, RangeDecoration,
    };
    use ilium_plugin_api::generated::types::editing::{TextPosition, TextRange};
    use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
    use ilium_plugin_api::generated::types::ids::BufferId;
    use ilium_plugin_api::generated::types::text::Style;

    /// Largest `read-lines` count accepted in one call (primitives-editor, "Limits"; PRI-020).
    const MAX_LINES_PER_READ: u32 = 10_000;

    /// Largest text `create-buffer` accepts, in bytes: 16 MiB (primitives-editor, "Limits"; PRI-021).
    const MAX_CREATE_TEXT_BYTES: usize = 16 * 1024 * 1024;

    /// Buffers one plugin may hold at once (primitives-editor, "Limits"; PRI-021).
    const MAX_BUFFERS_PER_PLUGIN: usize = 64;

    /// Decorations or gutter marks accepted per layer in one call (primitives-editor, "Limits"; PRI-024).
    const MAX_ITEMS_PER_LAYER: usize = 10_000;

    /// Asserts that a fallible call failed with the expected error code.
    fn assert_error_code<T: std::fmt::Debug>(result: Result<T, PluginError>, expected: ErrorCode) {
        match result {
            Err(error) => assert_eq!(error.code, expected, "unexpected error: {}", error.message),
            Ok(value) => panic!("expected {expected:?}, got Ok({value:?})"),
        }
    }

    /// Asserts that a fallible call did not fail with the given code. Used at the limit boundary,
    /// where the call is allowed to fail for a different reason (for example a missing buffer).
    fn assert_not_error_code<T: std::fmt::Debug>(
        result: &Result<T, PluginError>,
        rejected: ErrorCode,
    ) {
        if let Err(error) = result {
            assert_ne!(
                error.code, rejected,
                "limit rejected a call at the boundary: {}",
                error.message
            );
        }
    }

    /// A minimal decoration on the first character of line zero.
    fn sample_decoration() -> RangeDecoration {
        RangeDecoration {
            range: TextRange {
                start: TextPosition { line: 0, column: 0 },
                end: TextPosition { line: 0, column: 1 },
            },
            style: Style::default(),
            hover: None,
        }
    }

    /// A minimal gutter mark on line zero.
    fn sample_gutter_mark() -> GutterMark {
        GutterMark {
            line: 0,
            icon: "list-check".to_string(),
            tooltip: None,
            command: None,
        }
    }

    /// REQ: PRI-020
    #[test]
    fn read_lines_rejects_a_count_over_10000_lines() {
        let mut editor = EditorImport;
        let result = editor.read_lines(BufferId(1), 0, MAX_LINES_PER_READ + 1);
        assert_error_code(result, ErrorCode::InvalidArgument);
    }

    /// REQ: PRI-020
    #[test]
    fn read_lines_accepts_a_count_of_exactly_10000_lines() {
        let mut editor = EditorImport;
        let result = editor.read_lines(BufferId(1), 0, MAX_LINES_PER_READ);
        assert_not_error_code(&result, ErrorCode::InvalidArgument);
    }

    /// REQ: PRI-021
    #[test]
    fn create_buffer_accepts_empty_initial_text() {
        let mut editor = EditorImport;
        let result = editor.create_buffer(String::new(), None);
        assert!(
            result.is_ok(),
            "empty text is within the 16 MiB limit, got {result:?}"
        );
    }

    /// REQ: PRI-021
    #[test]
    fn create_buffer_accepts_text_of_exactly_16_mib() {
        let mut editor = EditorImport;
        let text = "a".repeat(MAX_CREATE_TEXT_BYTES);
        let result = editor.create_buffer(text, Some("rust".to_string()));
        assert!(
            result.is_ok(),
            "16 MiB is the accepted maximum, got {result:?}"
        );
    }

    /// REQ: PRI-021
    #[test]
    fn create_buffer_rejects_text_over_16_mib() {
        let mut editor = EditorImport;
        let text = "a".repeat(MAX_CREATE_TEXT_BYTES + 1);
        let result = editor.create_buffer(text, None);
        assert_error_code(result, ErrorCode::InvalidArgument);
    }

    /// REQ: PRI-021
    #[test]
    fn create_buffer_refuses_the_65th_buffer_with_quota_exceeded() {
        let mut editor = EditorImport;
        for index in 0..MAX_BUFFERS_PER_PLUGIN {
            let result = editor.create_buffer(String::new(), None);
            assert!(
                result.is_ok(),
                "buffer {} of {} must be accepted, got {result:?}",
                index + 1,
                MAX_BUFFERS_PER_PLUGIN
            );
        }
        let result = editor.create_buffer(String::new(), None);
        assert_error_code(result, ErrorCode::QuotaExceeded);
    }

    /// REQ: PRI-024
    #[test]
    fn set_decorations_rejects_more_than_10000_items_in_one_layer() {
        let mut editor = EditorImport;
        let items = vec![sample_decoration(); MAX_ITEMS_PER_LAYER + 1];
        let result = editor.set_decorations(BufferId(1), "todo".to_string(), items);
        assert_error_code(result, ErrorCode::InvalidArgument);
    }

    /// REQ: PRI-024
    #[test]
    fn set_decorations_accepts_exactly_10000_items_in_one_layer() {
        let mut editor = EditorImport;
        let items = vec![sample_decoration(); MAX_ITEMS_PER_LAYER];
        let result = editor.set_decorations(BufferId(1), "todo".to_string(), items);
        assert_not_error_code(&result, ErrorCode::InvalidArgument);
    }

    /// REQ: PRI-024
    #[test]
    fn set_decorations_with_an_empty_list_is_within_the_limit() {
        let mut editor = EditorImport;
        let result = editor.set_decorations(BufferId(1), "todo".to_string(), Vec::new());
        assert_not_error_code(&result, ErrorCode::InvalidArgument);
    }

    /// REQ: PRI-024
    #[test]
    fn set_gutter_rejects_more_than_10000_marks_in_one_layer() {
        let mut editor = EditorImport;
        let marks = vec![sample_gutter_mark(); MAX_ITEMS_PER_LAYER + 1];
        let result = editor.set_gutter(BufferId(1), "todo".to_string(), marks);
        assert_error_code(result, ErrorCode::InvalidArgument);
    }

    /// REQ: PRI-024
    #[test]
    fn set_gutter_accepts_exactly_10000_marks_in_one_layer() {
        let mut editor = EditorImport;
        let marks = vec![sample_gutter_mark(); MAX_ITEMS_PER_LAYER];
        let result = editor.set_gutter(BufferId(1), "todo".to_string(), marks);
        assert_not_error_code(&result, ErrorCode::InvalidArgument);
    }

    /// REQ: PRI-024
    #[test]
    fn set_gutter_with_an_empty_list_is_within_the_limit() {
        let mut editor = EditorImport;
        let result = editor.set_gutter(BufferId(1), "todo".to_string(), Vec::new());
        assert_not_error_code(&result, ErrorCode::InvalidArgument);
    }
}
