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
