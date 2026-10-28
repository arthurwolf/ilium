//! Host implementation of `ilium:primitives/editor` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostEditor` trait; hand-owned afterwards.

use ilium_plugin_api::generated::primitives::editor::{
    GutterMark, HostEditor, MinimapMark, RangeDecoration,
};
use ilium_plugin_api::generated::types::editing::{BufferRef, EditorViewport, TextRange};
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use std::collections::BTreeMap;
use ilium_plugin_api::generated::types::ids::{BufferId, NodeId, RequestId};
use ilium_plugin_api::generated::types::paths::FsPath;

/// Largest `read-lines` count accepted in one call (PRI-020).
const MAX_LINES_PER_READ: u32 = 10_000;
/// Largest text `create-buffer` accepts, in bytes: 16 MiB (PRI-021).
const MAX_CREATE_TEXT_BYTES: usize = 16 * 1024 * 1024;
/// Buffers one plugin may hold at once (PRI-021).
const MAX_BUFFERS_PER_PLUGIN: usize = 64;
/// Decorations, gutter marks or minimap marks accepted per layer in one call (PRI-024).
const MAX_ITEMS_PER_LAYER: usize = 10_000;

#[derive(Debug)]
struct Buffer {
    text: String,
    language: Option<String>,
    revision: u64,
}

impl Buffer {
    fn buffer_ref(&self, id: u64) -> BufferRef {
        BufferRef {
            buffer: BufferId(id),
            pane: None,
            path: None,
            language: self.language.clone(),
            revision: self.revision,
            line_count: u32::try_from(self.text.split('\n').count()).unwrap_or(u32::MAX),
            dirty: false,
        }
    }
}

fn invalid(message: &str) -> PluginError {
    PluginError::new(ErrorCode::InvalidArgument, message)
}

fn check_layer(layer: &str, item_count: usize) -> Result<(), PluginError> {
    if layer.is_empty() {
        return Err(invalid("layer must not be empty"));
    }
    if item_count > MAX_ITEMS_PER_LAYER {
        return Err(invalid("a layer holds at most 10000 items"));
    }
    Ok(())
}

/// File access needs a granted file scope, which this import does not resolve itself.
fn no_file_scope(path: &FsPath) -> PluginError {
    PluginError::new(
        ErrorCode::NotPermitted,
        format!("file scope \"{}\" is not granted to the editor import", path.scope),
    )
}

/// Host side of the `editor` interface: the buffers one plugin created through it.
#[derive(Debug, Default)]
pub struct EditorImport {
    buffers: BTreeMap<u64, Buffer>,
    loaded: BTreeMap<u64, BufferId>,
    next_buffer: u64,
}

impl EditorImport {
    fn buffer(&self, id: u64) -> Result<&Buffer, PluginError> {
        self.buffers
            .get(&id)
            .ok_or_else(|| PluginError::new(ErrorCode::NotFound, "no such buffer"))
    }
}

impl HostEditor for EditorImport {
    fn buffers(&mut self) -> Vec<BufferRef> {
        self.buffers.iter().map(|(id, buffer)| buffer.buffer_ref(*id)).collect()
    }

    fn buffer_for_pane(&mut self, _pane: NodeId) -> Option<BufferRef> {
        None
    }

    fn read_lines(
        &mut self,
        buffer: BufferId,
        first_line: u32,
        count: u32,
    ) -> Result<Vec<String>, PluginError> {
        if count > MAX_LINES_PER_READ {
            return Err(invalid("count is above 10000 lines"));
        }
        let text = &self.buffer(buffer.0)?.text;
        Ok(text
            .split('\n')
            .skip(first_line as usize)
            .take(count as usize)
            .map(str::to_owned)
            .collect())
    }

    fn selections(&mut self, _buffer: BufferId) -> Vec<TextRange> {
        Vec::new()
    }

    fn set_decorations(
        &mut self,
        buffer: BufferId,
        layer: String,
        items: Vec<RangeDecoration>,
    ) -> Result<(), PluginError> {
        check_layer(&layer, items.len())?;
        self.buffer(buffer.0).map(|_| ())
    }

    fn set_gutter(
        &mut self,
        buffer: BufferId,
        layer: String,
        marks: Vec<GutterMark>,
    ) -> Result<(), PluginError> {
        check_layer(&layer, marks.len())?;
        self.buffer(buffer.0).map(|_| ())
    }

    fn create_buffer(
        &mut self,
        initial: String,
        language: Option<String>,
    ) -> Result<BufferId, PluginError> {
        if initial.len() > MAX_CREATE_TEXT_BYTES {
            return Err(invalid("initial text is larger than 16 MiB"));
        }
        if self.buffers.len() >= MAX_BUFFERS_PER_PLUGIN {
            return Err(PluginError::new(
                ErrorCode::QuotaExceeded,
                "a plugin may hold at most 64 buffers",
            ));
        }
        self.next_buffer += 1;
        self.buffers.insert(
            self.next_buffer,
            Buffer {
                text: initial,
                language,
                revision: 0,
            },
        );
        Ok(BufferId(self.next_buffer))
    }

    fn load_file(&mut self, path: FsPath) -> Result<RequestId, PluginError> {
        Err(no_file_scope(&path))
    }

    fn take_buffer(&mut self, request: RequestId) -> Option<BufferId> {
        self.loaded.remove(&request.0)
    }

    fn save_buffer(
        &mut self,
        buffer: BufferId,
        path: Option<FsPath>,
    ) -> Result<RequestId, PluginError> {
        self.buffer(buffer.0)?;
        match path {
            Some(path) => Err(no_file_scope(&path)),
            None => Err(PluginError::new(
                ErrorCode::InvalidArgument,
                "the buffer has no path; pass one to save it",
            )),
        }
    }

    fn close_buffer(&mut self, buffer: BufferId) {
        self.buffers.remove(&buffer.0);
    }

    fn viewport(&mut self, _pane: NodeId) -> Option<EditorViewport> {
        None
    }

    fn set_minimap_marks(
        &mut self,
        buffer: BufferId,
        layer: String,
        marks: Vec<MinimapMark>,
    ) -> Result<(), PluginError> {
        check_layer(&layer, marks.len())?;
        self.buffer(buffer.0).map(|_| ())
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
        let mut editor = EditorImport::default();
        let result = editor.read_lines(BufferId(1), 0, MAX_LINES_PER_READ + 1);
        assert_error_code(result, ErrorCode::InvalidArgument);
    }

    /// REQ: PRI-020
    #[test]
    fn read_lines_accepts_a_count_of_exactly_10000_lines() {
        let mut editor = EditorImport::default();
        let result = editor.read_lines(BufferId(1), 0, MAX_LINES_PER_READ);
        assert_not_error_code(&result, ErrorCode::InvalidArgument);
    }

    /// REQ: PRI-021
    #[test]
    fn create_buffer_accepts_empty_initial_text() {
        let mut editor = EditorImport::default();
        let result = editor.create_buffer(String::new(), None);
        assert!(
            result.is_ok(),
            "empty text is within the 16 MiB limit, got {result:?}"
        );
    }

    /// REQ: PRI-021
    #[test]
    fn create_buffer_accepts_text_of_exactly_16_mib() {
        let mut editor = EditorImport::default();
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
        let mut editor = EditorImport::default();
        let text = "a".repeat(MAX_CREATE_TEXT_BYTES + 1);
        let result = editor.create_buffer(text, None);
        assert_error_code(result, ErrorCode::InvalidArgument);
    }

    /// REQ: PRI-021
    #[test]
    fn create_buffer_refuses_the_65th_buffer_with_quota_exceeded() {
        let mut editor = EditorImport::default();
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
        let mut editor = EditorImport::default();
        let items = vec![sample_decoration(); MAX_ITEMS_PER_LAYER + 1];
        let result = editor.set_decorations(BufferId(1), "todo".to_string(), items);
        assert_error_code(result, ErrorCode::InvalidArgument);
    }

    /// REQ: PRI-024
    #[test]
    fn set_decorations_accepts_exactly_10000_items_in_one_layer() {
        let mut editor = EditorImport::default();
        let items = vec![sample_decoration(); MAX_ITEMS_PER_LAYER];
        let result = editor.set_decorations(BufferId(1), "todo".to_string(), items);
        assert_not_error_code(&result, ErrorCode::InvalidArgument);
    }

    /// REQ: PRI-024
    #[test]
    fn set_decorations_with_an_empty_list_is_within_the_limit() {
        let mut editor = EditorImport::default();
        let result = editor.set_decorations(BufferId(1), "todo".to_string(), Vec::new());
        assert_not_error_code(&result, ErrorCode::InvalidArgument);
    }

    /// REQ: PRI-024
    #[test]
    fn set_gutter_rejects_more_than_10000_marks_in_one_layer() {
        let mut editor = EditorImport::default();
        let marks = vec![sample_gutter_mark(); MAX_ITEMS_PER_LAYER + 1];
        let result = editor.set_gutter(BufferId(1), "todo".to_string(), marks);
        assert_error_code(result, ErrorCode::InvalidArgument);
    }

    /// REQ: PRI-024
    #[test]
    fn set_gutter_accepts_exactly_10000_marks_in_one_layer() {
        let mut editor = EditorImport::default();
        let marks = vec![sample_gutter_mark(); MAX_ITEMS_PER_LAYER];
        let result = editor.set_gutter(BufferId(1), "todo".to_string(), marks);
        assert_not_error_code(&result, ErrorCode::InvalidArgument);
    }

    /// REQ: PRI-024
    #[test]
    fn set_gutter_with_an_empty_list_is_within_the_limit() {
        let mut editor = EditorImport::default();
        let result = editor.set_gutter(BufferId(1), "todo".to_string(), Vec::new());
        assert_not_error_code(&result, ErrorCode::InvalidArgument);
    }
}
