//! Host implementation of `ilium:primitives/terminal` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostTerminal` trait; hand-owned afterwards.

use ilium_plugin_api::generated::primitives::terminal::{
    CellRegion, CellRows, ForegroundProcess, HistoryMatch, HostTerminal, OutputFilter, Screen,
    ScreenPart, Selection, TerminalMode,
};
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::ids::{NodeId, RequestId};
use ilium_plugin_api::generated::types::ui::PointerModifiers;

/// Host side of the `terminal` interface.
#[derive(Debug)]
pub struct TerminalImport;

impl HostTerminal for TerminalImport {
    fn read_screen(&mut self, _pane: NodeId, _part: ScreenPart, _max_rows: u32) -> Option<Screen> {
        unimplemented!("host::imports::terminal::read_screen")
    }

    fn selection(&mut self, _pane: NodeId) -> Option<Selection> {
        unimplemented!("host::imports::terminal::selection")
    }

    fn add_output_filter(
        &mut self,
        _pane: NodeId,
        _filter: OutputFilter,
    ) -> Result<(), PluginError> {
        unimplemented!("host::imports::terminal::add_output_filter")
    }

    fn remove_output_filter(&mut self, _pane: NodeId, _filter_id: String) {
        unimplemented!("host::imports::terminal::remove_output_filter")
    }

    fn claim_pointer(&mut self, _pane: NodeId) -> Result<(), PluginError> {
        unimplemented!("host::imports::terminal::claim_pointer")
    }

    fn claim_pointer_gesture(
        &mut self,
        _pane: NodeId,
        _modifiers: PointerModifiers,
    ) -> Result<(), PluginError> {
        unimplemented!("host::imports::terminal::claim_pointer_gesture")
    }

    fn release_pointer(&mut self, _pane: NodeId) {
        unimplemented!("host::imports::terminal::release_pointer")
    }

    fn set_highlights(
        &mut self,
        _pane: NodeId,
        _layer: String,
        _regions: Vec<CellRegion>,
    ) -> Result<(), PluginError> {
        unimplemented!("host::imports::terminal::set_highlights")
    }

    fn clear_highlights(&mut self, _pane: NodeId, _layer: String) {
        unimplemented!("host::imports::terminal::clear_highlights")
    }

    fn read_cells(
        &mut self,
        _pane: NodeId,
        _part: ScreenPart,
        _first_row: u32,
        _count: u32,
    ) -> Option<CellRows> {
        unimplemented!("host::imports::terminal::read_cells")
    }

    fn modes(&mut self, _pane: NodeId) -> Option<TerminalMode> {
        unimplemented!("host::imports::terminal::modes")
    }

    fn foreground_process(&mut self, _pane: NodeId) -> Option<ForegroundProcess> {
        unimplemented!("host::imports::terminal::foreground_process")
    }

    fn freeze_snapshot(&mut self, _pane: NodeId) -> Result<u64, PluginError> {
        unimplemented!("host::imports::terminal::freeze_snapshot")
    }

    fn read_snapshot_cells(
        &mut self,
        _snapshot: u64,
        _first_row: u32,
        _count: u32,
    ) -> Option<CellRows> {
        unimplemented!("host::imports::terminal::read_snapshot_cells")
    }

    fn drop_snapshot(&mut self, _snapshot: u64) {
        unimplemented!("host::imports::terminal::drop_snapshot")
    }

    fn history_text(&mut self, _pane: NodeId, _max_bytes: u32) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::terminal::history_text")
    }

    fn take_history(&mut self, _request: RequestId) -> Option<String> {
        unimplemented!("host::imports::terminal::take_history")
    }

    fn search_history(
        &mut self,
        _pane: NodeId,
        _regex: String,
        _max_matches: u16,
    ) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::terminal::search_history")
    }

    fn take_matches(&mut self, _request: RequestId) -> Option<Vec<HistoryMatch>> {
        unimplemented!("host::imports::terminal::take_matches")
    }

    fn lock_input(&mut self, _pane: NodeId, _millis: u16) -> Result<(), PluginError> {
        unimplemented!("host::imports::terminal::lock_input")
    }

    fn unlock_input(&mut self, _pane: NodeId) {
        unimplemented!("host::imports::terminal::unlock_input")
    }
}
