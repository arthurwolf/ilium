//! Shared helpers of the screens lane. The rig types below do not exist in the skeleton yet and
//! are listed in API_GAPS.md.

pub use ilium_client::plugin_host::screens_rig::{RowState, ScreensRig};

/// True when some row of `screen` contains `needle` after the box-drawing frame is stripped.
pub fn has_row_with(screen: &str, needle: &str) -> bool {
    screen.lines().any(|line| line.contains(needle))
}

/// The first row of `screen` that contains `needle`.
pub fn row_with(screen: &str, needle: &str) -> String {
    screen
        .lines()
        .find(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("no row contains {needle:?} in:\n{screen}"))
        .to_string()
}
