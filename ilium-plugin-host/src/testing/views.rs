//! A harness over the view and popup lifecycle of plugin UI trees (reference-host-rendering.md).

use ilium_plugin_api::PluginError;

/// Opens views, their popups and commits trees, recording the close events the host emits.
#[derive(Debug, Default)]
pub struct ViewHarness {
    views: Vec<(String, Vec<String>)>,
    open_popups: Vec<String>,
    closed: Vec<String>,
}

impl ViewHarness {
    /// An empty harness.
    pub fn new() -> ViewHarness {
        unimplemented!("host::testing::views::ViewHarness::new")
    }

    /// Opens a view declaring `popups`.
    pub fn open_view(&mut self, view: &str, popups: &[String]) {
        unimplemented!("host::testing::views::ViewHarness::open_view")
    }

    /// Opens a declared popup; `not-found` for an unknown id.
    pub fn open_popup(&mut self, popup: &str) -> Result<(), PluginError> {
        unimplemented!("host::testing::views::ViewHarness::open_popup")
    }

    /// Closes an open popup.
    pub fn close_popup(&mut self, popup: &str) -> Result<(), PluginError> {
        unimplemented!("host::testing::views::ViewHarness::close_popup")
    }

    /// Commits a new tree of a view declaring `popups`; popups no longer declared are closed.
    pub fn commit_tree(&mut self, view: &str, popups: &[String]) {
        unimplemented!("host::testing::views::ViewHarness::commit_tree")
    }

    /// Whether a popup is open.
    pub fn is_open(&self, popup: &str) -> bool {
        unimplemented!("host::testing::views::ViewHarness::is_open")
    }

    /// The popup-closed events emitted so far, in order.
    pub fn closed_events(&self) -> Vec<String> {
        unimplemented!("host::testing::views::ViewHarness::closed_events")
    }
}
