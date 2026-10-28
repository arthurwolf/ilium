//! Host implementation of `ilium:primitives/terminal`.
//!
//! Hand-owned. The import carries no session pane table yet, so no pane is known: reads answer
//! `none` and operations that need a pane answer `not-found` after their arguments validate.
//! Input locks are real and tracked per thread (one plugin instance per thread).

use ilium_plugin_api::generated::primitives::terminal::{
    CellRegion, CellRows, ForegroundProcess, HistoryMatch, HostTerminal, OutputFilter, Screen,
    ScreenPart, Selection, TerminalMode,
};
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_api::generated::types::ids::{NodeId, RequestId};
use ilium_plugin_api::generated::types::ui::PointerModifiers;
use regex::Regex;
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Longest output-filter or history-search regex, in bytes (API-061).
const REGEX_LIMIT_BYTES: usize = 512;

/// Highest output-filter rate, in matches per second (API-061).
const FILTER_RATE_LIMIT: u16 = 100;

/// Most rows one `read-cells` call may return (PRI-001).
const READ_CELLS_ROW_LIMIT: u32 = 1_000;

/// Longest highlight label, in characters (API-061).
const HIGHLIGHT_LABEL_LIMIT_CHARS: usize = 128;

/// Longest highlight layer name, in characters (API-061).
const HIGHLIGHT_LAYER_LIMIT_CHARS: usize = 32;

/// Longest input lock, in milliseconds (PRI-005).
const INPUT_LOCK_LIMIT_MILLIS: u16 = 2_000;

/// Most matches one history search may request (API-061).
const SEARCH_MATCH_LIMIT: u16 = 10_000;

thread_local! {
    /// Panes whose input is locked, with the instant the lock lapses.
    static INPUT_LOCKS: RefCell<HashMap<u64, Instant>> = RefCell::new(HashMap::new());
}

/// Builds a terminal error with no retry delay.
fn terminal_error(code: ErrorCode, message: &str) -> PluginError {
    PluginError {
        code,
        message: message.to_string(),
        retry_after_ms: None,
    }
}

/// The error for a pane the import does not know.
fn pane_not_found() -> PluginError {
    terminal_error(ErrorCode::NotFound, "the pane does not exist")
}

/// Checks the length and syntax of a regex argument.
fn validate_regex(regex: &str) -> Result<(), PluginError> {
    if regex.len() > REGEX_LIMIT_BYTES {
        return Err(terminal_error(
            ErrorCode::InvalidArgument,
            "the regex is longer than 512 bytes",
        ));
    }
    Regex::new(regex)
        .map(|_| ())
        .map_err(|_| terminal_error(ErrorCode::InvalidArgument, "the regex does not compile"))
}

/// A layer name is 1 to 32 characters of lowercase letters, digits, `-` and `_`.
fn is_valid_layer_name(layer: &str) -> bool {
    let length = layer.chars().count();
    (1..=HIGHLIGHT_LAYER_LIMIT_CHARS).contains(&length)
        && layer.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '-' | '_')
        })
}

/// Host side of the `terminal` interface.
#[derive(Debug)]
pub struct TerminalImport;

impl HostTerminal for TerminalImport {
    fn read_screen(&mut self, _pane: NodeId, _part: ScreenPart, _max_rows: u32) -> Option<Screen> {
        None
    }

    fn selection(&mut self, _pane: NodeId) -> Option<Selection> {
        None
    }

    fn add_output_filter(
        &mut self,
        _pane: NodeId,
        filter: OutputFilter,
    ) -> Result<(), PluginError> {
        if filter.id.is_empty() {
            return Err(terminal_error(
                ErrorCode::InvalidArgument,
                "the filter id is empty",
            ));
        }
        if filter.rate_per_second == 0 || filter.rate_per_second > FILTER_RATE_LIMIT {
            return Err(terminal_error(
                ErrorCode::InvalidArgument,
                "rate-per-second must be 1 to 100",
            ));
        }
        validate_regex(&filter.regex)?;
        Err(pane_not_found())
    }

    fn remove_output_filter(&mut self, _pane: NodeId, _filter_id: String) {}

    fn claim_pointer(&mut self, _pane: NodeId) -> Result<(), PluginError> {
        Err(pane_not_found())
    }

    fn claim_pointer_gesture(
        &mut self,
        _pane: NodeId,
        modifiers: PointerModifiers,
    ) -> Result<(), PluginError> {
        if modifiers.0 == 0 {
            return Err(terminal_error(
                ErrorCode::InvalidArgument,
                "a gesture claim needs at least one modifier",
            ));
        }
        Err(pane_not_found())
    }

    fn release_pointer(&mut self, _pane: NodeId) {}

    fn set_highlights(
        &mut self,
        _pane: NodeId,
        layer: String,
        regions: Vec<CellRegion>,
    ) -> Result<(), PluginError> {
        if !is_valid_layer_name(&layer) {
            return Err(terminal_error(
                ErrorCode::InvalidArgument,
                "the layer name must be 1 to 32 characters of a-z, 0-9, - and _",
            ));
        }
        for region in &regions {
            if region.first_row > region.last_row {
                return Err(terminal_error(
                    ErrorCode::InvalidArgument,
                    "first-row is after last-row",
                ));
            }
            let label_too_long = region
                .label
                .as_ref()
                .is_some_and(|label| label.chars().count() > HIGHLIGHT_LABEL_LIMIT_CHARS);
            if label_too_long {
                return Err(terminal_error(
                    ErrorCode::InvalidArgument,
                    "a highlight label is longer than 128 characters",
                ));
            }
        }
        Err(pane_not_found())
    }

    fn clear_highlights(&mut self, _pane: NodeId, _layer: String) {}

    fn read_cells(
        &mut self,
        _pane: NodeId,
        _part: ScreenPart,
        _first_row: u32,
        count: u32,
    ) -> Option<CellRows> {
        if count > READ_CELLS_ROW_LIMIT {
            return None;
        }
        None
    }

    fn modes(&mut self, _pane: NodeId) -> Option<TerminalMode> {
        None
    }

    fn foreground_process(&mut self, _pane: NodeId) -> Option<ForegroundProcess> {
        None
    }

    fn freeze_snapshot(&mut self, _pane: NodeId) -> Result<u64, PluginError> {
        Err(pane_not_found())
    }

    fn read_snapshot_cells(
        &mut self,
        _snapshot: u64,
        _first_row: u32,
        _count: u32,
    ) -> Option<CellRows> {
        None
    }

    fn drop_snapshot(&mut self, _snapshot: u64) {}

    fn history_text(&mut self, _pane: NodeId, _max_bytes: u32) -> Result<RequestId, PluginError> {
        Err(pane_not_found())
    }

    fn take_history(&mut self, _request: RequestId) -> Option<String> {
        None
    }

    fn search_history(
        &mut self,
        _pane: NodeId,
        regex: String,
        max_matches: u16,
    ) -> Result<RequestId, PluginError> {
        if max_matches == 0 || max_matches > SEARCH_MATCH_LIMIT {
            return Err(terminal_error(
                ErrorCode::InvalidArgument,
                "max-matches must be 1 to 10000",
            ));
        }
        validate_regex(&regex)?;
        Err(pane_not_found())
    }

    fn take_matches(&mut self, _request: RequestId) -> Option<Vec<HistoryMatch>> {
        None
    }

    fn lock_input(&mut self, pane: NodeId, millis: u16) -> Result<(), PluginError> {
        if millis > INPUT_LOCK_LIMIT_MILLIS {
            return Err(terminal_error(
                ErrorCode::InvalidArgument,
                "an input lock lasts at most 2000 ms",
            ));
        }
        let now = Instant::now();
        INPUT_LOCKS.with(|cell| {
            let mut locks = cell.borrow_mut();
            if locks.get(&pane.0).is_some_and(|expiry| *expiry > now) {
                return Err(terminal_error(
                    ErrorCode::Conflict,
                    "the pane input is already locked",
                ));
            }
            locks.insert(pane.0, now + Duration::from_millis(u64::from(millis)));
            Ok(())
        })
    }

    fn unlock_input(&mut self, pane: NodeId) {
        INPUT_LOCKS.with(|cell| {
            cell.borrow_mut().remove(&pane.0);
        });
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
        OutputFilter {
            id: id.to_string(),
            regex: regex.to_string(),
            rate_per_second,
        }
    }

    /// Builds a highlight region spanning the given rows, with default style and no label.
    fn region(first_row: u32, last_row: u32) -> CellRegion {
        CellRegion {
            first_row,
            first_column: 0,
            last_row,
            last_column: 0,
            ..Default::default()
        }
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
        assert!(terminal
            .read_screen(missing_pane(), ScreenPart::Visible, 24)
            .is_none());
    }

    /// REQ: PRI-001
    #[test]
    fn read_screen_scrollback_of_unknown_pane_returns_none() {
        let mut terminal = TerminalImport;
        assert!(terminal
            .read_screen(missing_pane(), ScreenPart::Scrollback, 10_000)
            .is_none());
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
        assert!(terminal
            .read_cells(pane(), ScreenPart::Visible, 0, 1_001)
            .is_none());
    }

    /// REQ: PRI-001
    #[test]
    fn read_cells_of_scrollback_above_1000_rows_returns_none() {
        let mut terminal = TerminalImport;
        assert!(terminal
            .read_cells(pane(), ScreenPart::Scrollback, 0, u32::MAX)
            .is_none());
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
        assert_eq!(
            error_code_of(terminal.freeze_snapshot(missing_pane())),
            ErrorCode::NotFound
        );
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
        let result =
            terminal.add_output_filter(missing_pane(), filter("rate-limit", "rate limit", 1));
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
        assert_eq!(
            error_code_of(terminal.claim_pointer(missing_pane())),
            ErrorCode::NotFound
        );
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
        let result =
            terminal.set_highlights(missing_pane(), "search".to_string(), vec![region(0, 0)]);
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
        assert_eq!(
            error_code_of(terminal.lock_input(pane(), 500)),
            ErrorCode::Conflict
        );
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
