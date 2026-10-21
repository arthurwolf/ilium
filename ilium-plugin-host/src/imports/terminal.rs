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

#[cfg(test)]
mod tests {
    use super::TerminalImport;
    use ilium_plugin_api::generated::primitives::terminal::{
        CellRegion, HostTerminal, OutputFilter, ScreenPart,
    };
    use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
    use ilium_plugin_api::generated::types::ids::NodeId;
    use ilium_plugin_api::generated::types::ui::PointerModifiers;

    /// Pane id used for calls whose own rule is under test; the host decides whether it exists.
    fn pane() -> NodeId {
        NodeId(1)
    }

    /// Pane id that no pane can have.
    fn missing_pane() -> NodeId {
        NodeId(u64::MAX)
    }

    /// Builds an output filter with the given id, regex and rate.
    fn filter(id: &str, regex: &str, rate_per_second: u16) -> OutputFilter {
        OutputFilter { id: id.to_string(), regex: regex.to_string(), rate_per_second }
    }

    /// Builds a highlight region spanning the given rows, with default style and no label.
    fn region(first_row: u32, last_row: u32) -> CellRegion {
        CellRegion { first_row, first_column: 0, last_row, last_column: 0, ..Default::default() }
    }

    /// Returns the error code of a failed call; fails the test when the call succeeded.
    fn error_code_of<T>(result: Result<T, PluginError>) -> ErrorCode {
        match result {
            Err(error) => error.code,
            Ok(_) => panic!("expected a plugin error, got Ok"),
        }
    }

    /// REQ: PRI-001
    #[test]
    fn read_screen_of_unknown_pane_returns_none() {
        let mut terminal = TerminalImport;
        assert!(terminal.read_screen(missing_pane(), ScreenPart::Visible, 24).is_none());
    }

    /// REQ: PRI-001
    #[test]
    fn read_screen_scrollback_of_unknown_pane_returns_none() {
        let mut terminal = TerminalImport;
        assert!(terminal.read_screen(missing_pane(), ScreenPart::Scrollback, 10_000).is_none());
    }

    /// REQ: PRI-001
    #[test]
    fn selection_of_unknown_pane_returns_none() {
        let mut terminal = TerminalImport;
        assert!(terminal.selection(missing_pane()).is_none());
    }

    /// REQ: PRI-001
    #[test]
    fn read_cells_above_1000_rows_returns_none() {
        let mut terminal = TerminalImport;
        assert!(terminal.read_cells(pane(), ScreenPart::Visible, 0, 1_001).is_none());
    }

    /// REQ: PRI-001
    #[test]
    fn read_cells_of_scrollback_above_1000_rows_returns_none() {
        let mut terminal = TerminalImport;
        assert!(terminal.read_cells(pane(), ScreenPart::Scrollback, 0, u32::MAX).is_none());
    }

    /// REQ: PRI-001
    #[test]
    fn read_snapshot_cells_of_unknown_snapshot_returns_none() {
        let mut terminal = TerminalImport;
        assert!(terminal.read_snapshot_cells(u64::MAX, 0, 1).is_none());
    }

    /// REQ: PRI-001
    #[test]
    fn freeze_snapshot_of_missing_pane_is_not_found() {
        let mut terminal = TerminalImport;
        assert_eq!(error_code_of(terminal.freeze_snapshot(missing_pane())), ErrorCode::NotFound);
    }

    /// REQ: PRI-001
    #[test]
    fn modes_of_unknown_pane_returns_none() {
        let mut terminal = TerminalImport;
        assert!(terminal.modes(missing_pane()).is_none());
    }

    /// REQ: PRI-001
    #[test]
    fn foreground_process_of_unknown_pane_returns_none() {
        let mut terminal = TerminalImport;
        assert!(terminal.foreground_process(missing_pane()).is_none());
    }

    /// REQ: API-061
    #[test]
    fn add_output_filter_rate_zero_is_invalid_argument() {
        let mut terminal = TerminalImport;
        let result = terminal.add_output_filter(pane(), filter("rate-limit", "rate limit", 0));
        assert_eq!(error_code_of(result), ErrorCode::InvalidArgument);
    }

    /// REQ: API-061
    #[test]
    fn add_output_filter_rate_above_100_is_invalid_argument() {
        let mut terminal = TerminalImport;
        let result = terminal.add_output_filter(pane(), filter("rate-limit", "rate limit", 101));
        assert_eq!(error_code_of(result), ErrorCode::InvalidArgument);
    }

    /// REQ: API-061
    #[test]
    fn add_output_filter_empty_id_is_invalid_argument() {
        let mut terminal = TerminalImport;
        let result = terminal.add_output_filter(pane(), filter("", "rate limit", 1));
        assert_eq!(error_code_of(result), ErrorCode::InvalidArgument);
    }

    /// REQ: API-061
    #[test]
    fn add_output_filter_regex_over_512_bytes_is_invalid_argument() {
        let mut terminal = TerminalImport;
        let long_regex = "a".repeat(513);
        let result = terminal.add_output_filter(pane(), filter("long", &long_regex, 1));
        assert_eq!(error_code_of(result), ErrorCode::InvalidArgument);
    }

    /// REQ: API-002
    #[test]
    fn add_output_filter_regex_that_does_not_compile_is_invalid_argument() {
        let mut terminal = TerminalImport;
        let result = terminal.add_output_filter(pane(), filter("broken", "rate (limit", 1));
        assert_eq!(error_code_of(result), ErrorCode::InvalidArgument);
    }

    /// REQ: PRI-006
    #[test]
    fn add_output_filter_on_missing_pane_is_not_found() {
        let mut terminal = TerminalImport;
        let result = terminal.add_output_filter(missing_pane(), filter("rate-limit", "rate limit", 1));
        assert_eq!(error_code_of(result), ErrorCode::NotFound);
    }

    /// REQ: PRI-006
    #[test]
    fn remove_output_filter_with_unknown_id_returns_without_error() {
        let mut terminal = TerminalImport;
        terminal.remove_output_filter(missing_pane(), "never-added".to_string());
    }

    /// REQ: PRI-007
    #[test]
    fn claim_pointer_on_missing_pane_is_not_found() {
        let mut terminal = TerminalImport;
        assert_eq!(error_code_of(terminal.claim_pointer(missing_pane())), ErrorCode::NotFound);
    }

    /// REQ: PRI-007
    #[test]
    fn claim_pointer_gesture_with_empty_modifiers_is_invalid_argument() {
        let mut terminal = TerminalImport;
        let result = terminal.claim_pointer_gesture(pane(), PointerModifiers(0));
        assert_eq!(error_code_of(result), ErrorCode::InvalidArgument);
    }

    /// REQ: PRI-007
    #[test]
    fn release_pointer_without_a_claim_returns_without_error() {
        let mut terminal = TerminalImport;
        terminal.release_pointer(pane());
    }

    /// REQ: API-061
    #[test]
    fn set_highlights_with_first_row_after_last_row_is_invalid_argument() {
        let mut terminal = TerminalImport;
        let result = terminal.set_highlights(pane(), "search".to_string(), vec![region(5, 2)]);
        assert_eq!(error_code_of(result), ErrorCode::InvalidArgument);
    }

    /// REQ: API-061
    #[test]
    fn set_highlights_label_over_128_characters_is_invalid_argument() {
        let mut terminal = TerminalImport;
        let mut highlight = region(0, 0);
        highlight.label = Some("x".repeat(129));
        let result = terminal.set_highlights(pane(), "search".to_string(), vec![highlight]);
        assert_eq!(error_code_of(result), ErrorCode::InvalidArgument);
    }

    /// REQ: API-061
    #[test]
    fn set_highlights_with_empty_layer_name_is_invalid_argument() {
        let mut terminal = TerminalImport;
        let result = terminal.set_highlights(pane(), String::new(), vec![region(0, 0)]);
        assert_eq!(error_code_of(result), ErrorCode::InvalidArgument);
    }

    /// REQ: API-061
    #[test]
    fn set_highlights_layer_name_of_33_characters_is_invalid_argument() {
        let mut terminal = TerminalImport;
        let result = terminal.set_highlights(pane(), "a".repeat(33), vec![region(0, 0)]);
        assert_eq!(error_code_of(result), ErrorCode::InvalidArgument);
    }

    /// REQ: API-061
    #[test]
    fn set_highlights_layer_name_with_uppercase_is_invalid_argument() {
        let mut terminal = TerminalImport;
        let result = terminal.set_highlights(pane(), "Search".to_string(), vec![region(0, 0)]);
        assert_eq!(error_code_of(result), ErrorCode::InvalidArgument);
    }

    /// REQ: PRI-007
    #[test]
    fn set_highlights_on_missing_pane_is_not_found() {
        let mut terminal = TerminalImport;
        let result = terminal.set_highlights(missing_pane(), "search".to_string(), vec![region(0, 0)]);
        assert_eq!(error_code_of(result), ErrorCode::NotFound);
    }

    /// REQ: API-061
    #[test]
    fn lock_input_longer_than_2000_ms_is_invalid_argument() {
        let mut terminal = TerminalImport;
        assert_eq!(
            error_code_of(terminal.lock_input(pane(), 2_001)),
            ErrorCode::InvalidArgument
        );
    }

    /// REQ: PRI-005
    #[test]
    fn second_input_lock_on_the_same_pane_is_conflict() {
        let mut terminal = TerminalImport;
        let _first = terminal.lock_input(pane(), 500);
        assert_eq!(error_code_of(terminal.lock_input(pane(), 500)), ErrorCode::Conflict);
    }

    /// REQ: PRI-005
    #[test]
    fn input_lock_can_be_taken_again_after_unlock() {
        let mut terminal = TerminalImport;
        let _first = terminal.lock_input(pane(), 500);
        terminal.unlock_input(pane());
        assert_eq!(terminal.lock_input(pane(), 500), Ok(()));
    }

    /// REQ: PRI-005
    #[test]
    fn input_lock_of_exactly_2000_ms_is_accepted() {
        let mut terminal = TerminalImport;
        assert_eq!(terminal.lock_input(pane(), 2_000), Ok(()));
    }

    /// REQ: API-061
    #[test]
    fn search_history_with_zero_max_matches_is_invalid_argument() {
        let mut terminal = TerminalImport;
        let result = terminal.search_history(pane(), "error".to_string(), 0);
        assert_eq!(error_code_of(result), ErrorCode::InvalidArgument);
    }

    /// REQ: API-061
    #[test]
    fn search_history_with_more_than_10000_max_matches_is_invalid_argument() {
        let mut terminal = TerminalImport;
        let result = terminal.search_history(pane(), "error".to_string(), 10_001);
        assert_eq!(error_code_of(result), ErrorCode::InvalidArgument);
    }

    /// REQ: API-002
    #[test]
    fn search_history_with_regex_that_does_not_compile_is_invalid_argument() {
        let mut terminal = TerminalImport;
        let result = terminal.search_history(pane(), r"error\[E\d{4".to_string(), 200);
        assert_eq!(error_code_of(result), ErrorCode::InvalidArgument);
    }

    /// REQ: API-061
    #[test]
    fn search_history_with_regex_over_512_bytes_is_invalid_argument() {
        let mut terminal = TerminalImport;
        let result = terminal.search_history(pane(), "a".repeat(513), 200);
        assert_eq!(error_code_of(result), ErrorCode::InvalidArgument);
    }
}
