//! App-side handling of the Padding settings: applying an edit, keeping the
//! tree's published padding and the displayed panes' sizes in step, and
//! persisting the `[padding]` table. Pure value logic lives in
//! [`crate::padding`].

use ilium_core::{NodeKind, PaneContentKind};
use ilium_ipc::PaneResizeCause;

use crate::app::App;
use crate::filesystem::configuration::ConfigurationChange;
use crate::padding::{
    PaddingMode, PaddingPanel, PaddingSettings, PaddingSides, MAX_PADDING, MAX_WIDTH_THRESHOLD,
    MIN_WIDTH_THRESHOLD,
};

/// One side of a panel, for single-side edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaddingSide {
    Left,
    Right,
    Top,
    Bottom,
}

impl PaddingSide {
    pub const ALL: [Self; 4] = [Self::Left, Self::Right, Self::Top, Self::Bottom];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Left => "Left",
            Self::Right => "Right",
            Self::Top => "Top",
            Self::Bottom => "Bottom",
        }
    }

    pub const fn get(self, sides: PaddingSides) -> u8 {
        match self {
            Self::Left => sides.left,
            Self::Right => sides.right,
            Self::Top => sides.top,
            Self::Bottom => sides.bottom,
        }
    }

    fn set(self, sides: &mut PaddingSides, cells: u8) {
        match self {
            Self::Left => sides.left = cells,
            Self::Right => sides.right = cells,
            Self::Top => sides.top = cells,
            Self::Bottom => sides.bottom = cells,
        }
    }
}

impl App {
    /// The padding `panel` gets at the current terminal width.
    pub fn padding_for(&self, panel: PaddingPanel) -> PaddingSides {
        self.padding_settings
            .sides_for(panel, self.layout.screen_area.width)
    }

    /// The panel kind a pane's content is drawn as.
    pub(crate) fn padding_panel_for_pane(&self, pane_id: ilium_core::NodeId) -> PaddingPanel {
        match self.tree.get(pane_id).map(|node| &node.kind) {
            Some(NodeKind::Pane {
                content: PaneContentKind::Editor,
                ..
            }) => PaddingPanel::Editor,
            Some(NodeKind::Pane {
                content: PaneContentKind::Board,
                ..
            }) => PaddingPanel::Board,
            _ => PaddingPanel::Terminal,
        }
    }

    /// Publishes the tree's current padding. Call after the padding settings
    /// or the terminal width change; the right-hand panels read their padding
    /// straight from the settings when viewports are built.
    pub fn sync_padding(&mut self) {
        crate::padding::set_tree_padding(self.padding_for(PaddingPanel::Tree));
    }

    /// Applies `edit` to the padding settings, then refreshes everything
    /// derived from them and saves the result. A no-op edit saves nothing.
    pub fn edit_padding(&mut self, edit: impl FnOnce(&mut PaddingSettings)) {
        let mut next = self.padding_settings;
        edit(&mut next);
        if next == self.padding_settings {
            return;
        }
        self.padding_settings = next;
        self.sync_padding();
        // Padding changes the interior of every panel, so terminals must be
        // told their new size even though the outer layout is unchanged.
        self.resize_displayed_panes(PaneResizeCause::UserInterfaceSettings);
        self.persist_configuration(
            ConfigurationChange::Padding(self.padding_settings),
            "Padding settings",
        );
    }

    pub fn padding_set_mode(&mut self, mode: PaddingMode) {
        self.edit_padding(|settings| settings.mode = mode);
    }

    /// Sets the width threshold, rejecting values outside the allowed range.
    pub fn padding_set_threshold(&mut self, columns: u16) -> Result<(), String> {
        if !(MIN_WIDTH_THRESHOLD..=MAX_WIDTH_THRESHOLD).contains(&columns) {
            return Err(format!(
                "Enter a width between {MIN_WIDTH_THRESHOLD} and {MAX_WIDTH_THRESHOLD}"
            ));
        }
        self.edit_padding(|settings| settings.width_threshold = columns);
        Ok(())
    }

    /// Moves the threshold by `delta` columns, clamped to the allowed range.
    pub fn padding_step_threshold(&mut self, delta: i32) {
        let stepped = (i32::from(self.padding_settings.width_threshold) + delta).clamp(
            i32::from(MIN_WIDTH_THRESHOLD),
            i32::from(MAX_WIDTH_THRESHOLD),
        ) as u16;
        self.edit_padding(|settings| settings.width_threshold = stepped);
    }

    pub fn padding_set_right_panels_shared(&mut self, shared: bool) {
        self.edit_padding(|settings| settings.right_panels_shared = shared);
    }

    pub fn padding_set_linked(&mut self, panel: PaddingPanel, linked: bool) {
        self.edit_padding(|settings| settings.editable_mut(panel).linked = linked);
    }

    /// Sets the linked value (all four sides).
    pub fn padding_set_all(&mut self, panel: PaddingPanel, cells: u8) {
        let cells = cells.min(MAX_PADDING);
        self.edit_padding(|settings| settings.editable_mut(panel).all = cells);
    }

    pub fn padding_set_side(&mut self, panel: PaddingPanel, side: PaddingSide, cells: u8) {
        let cells = cells.min(MAX_PADDING);
        self.edit_padding(|settings| side.set(&mut settings.editable_mut(panel).sides, cells));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> (App, tempfile::TempDir) {
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::new("padding-app".into(), directory.path().into());
        app.config_dir = Some(directory.path().into());
        (app, directory)
    }

    #[test]
    fn mode_threshold_and_sides_persist_and_reload() {
        let (mut app, directory) = app();
        // Width-dependent with the default 80-column test screen stays
        // inactive, so these edits never publish nonzero tree padding.
        app.padding_set_mode(PaddingMode::WidthDependent);
        app.settle_filesystem_for_test();
        assert_eq!(app.padding_set_threshold(150), Ok(()));
        app.settle_filesystem_for_test();
        assert!(app.padding_set_threshold(10).is_err());
        assert_eq!(app.padding_settings.width_threshold, 150);
        app.padding_set_linked(PaddingPanel::Tree, false);
        app.settle_filesystem_for_test();
        app.padding_set_side(PaddingPanel::Tree, PaddingSide::Top, 3);
        app.settle_filesystem_for_test();
        app.padding_step_threshold(-1000);
        assert_eq!(app.padding_settings.width_threshold, MIN_WIDTH_THRESHOLD);
        app.settle_filesystem_for_test();
        let loaded = crate::config::load(directory.path()).unwrap().padding;
        assert_eq!(loaded, app.padding_settings);
        assert_eq!(loaded.tree.sides.top, 3);
    }

    #[test]
    fn padding_follows_mode_and_terminal_width_per_panel() {
        let (mut app, _directory) = app();
        app.layout =
            crate::layout::UiLayout::from_screen_area(ratatui::layout::Rect::new(0, 0, 120, 40));
        assert_eq!(app.padding_for(PaddingPanel::Tree), PaddingSides::NONE);
        app.padding_settings.mode = PaddingMode::Enabled;
        assert_eq!(
            app.padding_for(PaddingPanel::Tree),
            PaddingSides::uniform(1)
        );
        app.padding_settings.mode = PaddingMode::WidthDependent;
        assert_eq!(app.padding_for(PaddingPanel::Terminal), PaddingSides::NONE);
        app.padding_settings.width_threshold = 120;
        assert_eq!(
            app.padding_for(PaddingPanel::Terminal),
            PaddingSides::uniform(1)
        );
    }
}
