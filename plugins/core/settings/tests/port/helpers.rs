//! Helpers of the layout, value-control and onboarding twins.

use ilium_plugin_sdk::testing::{FakeHost, Screen};
use ilium_plugin_sdk::types::{ActionKind, ActionValue, ErrorCode, ViewTarget};
use serde_json::{json, Value};

use crate::common::*;

/// The 26 tab ids in rail order (core-plugin-settings.md, "The 26 tabs").
pub const TABS: [&str; 26] = [
    "user-interface",
    "padding",
    "icons",
    "agent-monitoring",
    "agent-cost",
    "optimization",
    "remote-compaction",
    "keyboard",
    "terminal",
    "editor",
    "session",
    "git",
    "kanban-board",
    "animations",
    "sound",
    "voice-control",
    "reset-planning",
    "inference",
    "llm-instructions",
    "titles",
    "triggers",
    "text-triggers",
    "debug",
    "api",
    "about",
    "setup",
];

/// The acceptance sizes of the original layout tests.
pub const SIZES: [(u16, u16); 4] = [(40, 12), (80, 24), (120, 40), (200, 60)];

/// Render a view and check the frame: requested size, no row wider than the screen.
pub fn render_checked(host: &mut FakeHost, target: ViewTarget, columns: u16, rows: u16) -> Screen {
    let screen = host.render(target, columns, rows);
    assert_eq!((screen.columns, screen.rows), (columns, rows));
    for row in 0..rows {
        let text = screen.row(row);
        assert!(
            text.chars().count() <= usize::from(columns),
            "row {row} wider than {columns}: {text:?}"
        );
    }
    screen
}

/// Render every tab at one size and return the texts in rail order.
pub fn render_every_tab(host: &mut FakeHost, columns: u16, rows: u16) -> Vec<(String, String)> {
    TABS.iter()
        .map(|id| {
            (
                id.to_string(),
                render_checked(host, tab(id), columns, rows).text(),
            )
        })
        .collect()
}

/// Click the first occurrence of a text on a tab; `None` when the text is not on screen.
pub fn click_text(
    host: &mut FakeHost,
    tab_id: &str,
    needle: &str,
    columns: u16,
    rows: u16,
) -> Option<bool> {
    let screen = host.render(tab(tab_id), columns, rows);
    let (row, column) = screen.find(needle)?;
    let handled = host.click(tab(tab_id), column, row);
    host.settle();
    Some(handled)
}

/// Scroll a tab by lines (positive = down).
pub fn scroll(host: &mut FakeHost, tab_id: &str, lines: f64) {
    host.send_action(
        tab(tab_id),
        "document",
        ActionKind::Scrolled,
        ActionValue::Number(lines),
    );
    host.settle();
}

/// Accepted values at `low`, `low+1`, `high-1`, `high`; refused (and unchanged) at `low-1`, `high+1`.
pub fn assert_integer_range(key: &str, low: i64, high: i64) {
    for ok in [low, low + 1, high - 1, high] {
        let mut host = client_host();
        host.settings()
            .set(key, json!(ok))
            .unwrap_or_else(|error| panic!("{key}={ok}: {error:?}"));
        assert_eq!(host.settings().get(key), Some(json!(ok)), "{key}={ok}");
    }
    for bad in [low - 1, high + 1] {
        let mut host = client_host();
        let before = host.settings().get(key);
        let error = host
            .settings()
            .set(key, json!(bad))
            .expect_err(&format!("{key}={bad}"));
        assert_eq!(error.code, ErrorCode::InvalidArgument, "{key}={bad}");
        assert_eq!(
            host.settings().get(key),
            before,
            "{key} unchanged after {bad}"
        );
    }
}

/// A value the setting must refuse, with the stored value left alone.
pub fn assert_refused(key: &str, value: Value) {
    let mut host = client_host();
    let before = host.settings().get(key);
    assert!(
        host.settings().set(key, value.clone()).is_err(),
        "{key}={value}"
    );
    assert_eq!(
        host.settings().get(key),
        before,
        "{key} unchanged after {value}"
    );
}

/// Every text row is bounded and the whole tab list stays stable between two renders.
pub fn assert_render_is_stable(host: &mut FakeHost, tab_id: &str, columns: u16, rows: u16) {
    let first = render_checked(host, tab(tab_id), columns, rows).text();
    let second = render_checked(host, tab(tab_id), columns, rows).text();
    assert_eq!(first, second, "{tab_id} {columns}x{rows}");
}
