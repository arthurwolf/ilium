//! Plugin panes on the client (gap G14b).
//!
//! A plugin pane is a tree node of `PaneContentKind::Plugin` with no process behind it. Its
//! owner plugin draws the body as the view `ViewTarget::Pane(<node>)`; the client draws the
//! frame and the title, places the view in the pane's content area and routes the keyboard,
//! the pointer and pastes to it, exactly as it does for a plugin panel. Each pane keeps its
//! own view cursor, so two plugin panes of a split view never share a focus or a text input.
//! When the owner is disabled or missing, an "off" frame stands in until it comes back.

use std::collections::HashMap;
use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, MouseButton, MouseEvent, MouseEventKind};
use ilium_core::{NodeId, PluginPaneKind, Tree};
use ilium_plugin_api::generated::types::ui::UiAction;
use ilium_plugin_api::generated::types::ViewTarget;
use ratatui::layout::Position;
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Wrap};
use ratatui::Frame;

use super::view;
use crate::app::{App, FocusTarget};
use crate::split_layout::PaneViewport;
use crate::theme;

/// The view cursors of the plugin panes, by pane.
pub(crate) type PaneCursors = HashMap<NodeId, view::ViewCursor>;

/// The view a plugin draws for pane `pane`.
pub(crate) fn pane_target(pane: NodeId) -> ViewTarget {
    ViewTarget::Pane(ilium_plugin_api::generated::types::NodeId(pane.0))
}

/// The plugin that owns pane `pane`, when it is a plugin pane.
pub(crate) fn owner(tree: &Tree, pane: NodeId) -> Option<&str> {
    tree.plugin_pane_kind(pane).map(PluginPaneKind::plugin)
}

/// What the body of a plugin pane shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Presence {
    /// The owner runs: its view fills the body.
    Drawn { plugin: String },
    /// Plugins are off, or the owner is disabled or not installed.
    Off { plugin: String },
}

/// The text of the frame that stands in for a plugin pane whose plugin is off.
pub(crate) fn off_text(plugin: &str) -> String {
    format!(
        "The plugin {plugin} is off, so this pane is not drawn.\n\
         It comes back when the plugin is turned on again (Settings, Plugins)."
    )
}

impl App {
    /// Whether `pane` is a plugin pane.
    pub(crate) fn is_plugin_pane(&self, pane: NodeId) -> bool {
        owner(&self.tree, pane).is_some()
    }

    /// What plugin pane `pane` shows; `None` when it is not a plugin pane.
    pub(crate) fn plugin_pane_presence(&self, pane: NodeId) -> Option<Presence> {
        let plugin = owner(&self.tree, pane)?.to_owned();
        let runs = self
            .wasm_plugins
            .as_ref()
            .is_some_and(|plugins| plugins.runs_plugin(&plugin));
        Some(if runs {
            Presence::Drawn { plugin }
        } else {
            Presence::Off { plugin }
        })
    }

    /// The views of the plugin panes on screen, as owner and target. `release_hidden` keeps
    /// these open; every other placed pane view is closed.
    pub(crate) fn displayed_plugin_pane_views(&self) -> Vec<(String, ViewTarget)> {
        self.pane_viewports()
            .into_iter()
            .filter_map(|viewport| {
                owner(&self.tree, viewport.pane_id)
                    .map(|plugin| (plugin.to_owned(), pane_target(viewport.pane_id)))
            })
            .collect()
    }

    /// The view of plugin pane `pane` laid out for its content area, when its plugin runs and
    /// has drawn it.
    fn compose_plugin_pane(&self, pane: NodeId) -> Option<(String, ViewTarget, view::Scene)> {
        let Presence::Drawn { plugin } = self.plugin_pane_presence(pane)? else {
            return None;
        };
        let viewport = self.pane_viewport(pane)?;
        let target = pane_target(pane);
        let tree = self
            .wasm_plugins
            .as_ref()
            .and_then(|plugins| plugins.view_tree(&plugin, &target))?;
        let states = self
            .plugin_pane_cursors
            .get(&pane)
            .map(|cursor| &cursor.states);
        let empty = super::ui_state::WidgetStates::default();
        let scene = view::compose_with(&tree, viewport.content_area, states.unwrap_or(&empty));
        Some((plugin, target, scene))
    }

    /// Keyboard in a focused plugin pane: Esc returns to the tree (unless a number entry takes
    /// it); the rest moves the view's cursor, edits its focused input or sends its action.
    pub(crate) fn handle_plugin_pane_key(&mut self, pane: NodeId, key: KeyEvent) {
        if key.kind == KeyEventKind::Release {
            return;
        }
        let captures_escape = self
            .plugin_pane_cursors
            .get(&pane)
            .is_some_and(view::captures_escape);
        if key.code == KeyCode::Esc && !captures_escape {
            self.leave_pane_focus();
            return;
        }
        let Some((plugin, target, scene)) = self.compose_plugin_pane(pane) else {
            return;
        };
        let cursor = self.plugin_pane_cursors.entry(pane).or_default();
        let actions = view::key(&scene, &target, cursor, key, Instant::now());
        self.send_plugin_pane_actions(&plugin, actions);
    }

    /// Pointer over a plugin pane: a click focuses the pane and sends the action under it,
    /// the wheel scrolls, a drag moves a scrollbar, a divider or a slider.
    pub(crate) fn handle_plugin_pane_mouse(
        &mut self,
        viewport: &PaneViewport,
        mouse: MouseEvent,
        position: Position,
    ) {
        let pane = viewport.pane_id;
        if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
            && !(matches!(self.focus, FocusTarget::Pane) && self.active_pane_id() == Some(pane))
        {
            self.focus_pane(pane);
        }
        if !viewport.content_area.contains(position)
            && mouse.kind != MouseEventKind::Up(MouseButton::Left)
        {
            return;
        }
        let Some((plugin, target, scene)) = self.compose_plugin_pane(pane) else {
            return;
        };
        let cursor = self.plugin_pane_cursors.entry(pane).or_default();
        let actions = match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                view::click(&scene, &target, cursor, position, Instant::now())
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                let mut moved = Vec::new();
                view::drag(&target, cursor, position, &mut moved);
                moved
            }
            MouseEventKind::Up(MouseButton::Left) => view::release(&target, cursor),
            MouseEventKind::ScrollUp => view::wheel(&scene, &target, cursor, position, -1),
            MouseEventKind::ScrollDown => view::wheel(&scene, &target, cursor, position, 1),
            _ => Vec::new(),
        };
        self.send_plugin_pane_actions(&plugin, actions);
    }

    /// A paste goes to the focused text input of the focused plugin pane. Returns whether the
    /// pane took it.
    pub(crate) fn handle_plugin_pane_paste(&mut self, pasted: &str) -> bool {
        if !matches!(self.focus, FocusTarget::Pane) {
            return false;
        }
        let Some(pane) = self
            .active_pane_id()
            .filter(|pane| self.is_plugin_pane(*pane))
        else {
            return false;
        };
        let Some((plugin, target, scene)) = self.compose_plugin_pane(pane) else {
            return false;
        };
        let cursor = self.plugin_pane_cursors.entry(pane).or_default();
        let actions = view::paste(&scene, &target, cursor, pasted, Instant::now());
        let taken = actions.is_some();
        self.send_plugin_pane_actions(&plugin, actions.unwrap_or_default());
        taken
    }

    /// Sends the plugin pane edits whose debounce ran out and forgets the cursors of panes
    /// that are gone. Returns whether anything was sent.
    pub(crate) fn tick_plugin_pane_edits(&mut self, now: Instant) -> bool {
        let tree = &self.tree;
        self.plugin_pane_cursors
            .retain(|pane, _| owner(tree, *pane).is_some());
        let due: Vec<NodeId> = self
            .plugin_pane_cursors
            .iter()
            .filter(|(_, cursor)| view::next_due(cursor).is_some_and(|due| due <= now))
            .map(|(pane, _)| *pane)
            .collect();
        let mut sent = false;
        for pane in due {
            let Some(plugin) = owner(&self.tree, pane).map(str::to_owned) else {
                continue;
            };
            let target = pane_target(pane);
            let Some(cursor) = self.plugin_pane_cursors.get_mut(&pane) else {
                continue;
            };
            let actions = view::tick(&target, cursor, now);
            sent |= !actions.is_empty();
            self.send_plugin_pane_actions(&plugin, actions);
        }
        sent
    }

    /// When the next unsent plugin pane edit falls due.
    pub(crate) fn plugin_pane_edit_due(&self) -> Option<Instant> {
        self.plugin_pane_cursors
            .values()
            .filter_map(view::next_due)
            .min()
    }

    fn send_plugin_pane_actions(&self, plugin: &str, actions: Vec<UiAction>) {
        if let Some(plugins) = self.wasm_plugins.as_ref() {
            for action in actions {
                plugins.view_action(plugin, action);
            }
        }
    }
}

/// Draws plugin pane `viewport.pane_id`: the frame with `title`, then the owner's view in the
/// content area, or the "off" text when the owner does not run. Returns false, drawing
/// nothing, when the pane is not a plugin pane.
pub(crate) fn draw(
    frame: &mut Frame,
    app: &App,
    viewport: &PaneViewport,
    focused: bool,
    title: &str,
) -> bool {
    let Some(presence) = app.plugin_pane_presence(viewport.pane_id) else {
        return false;
    };
    frame.render_widget(
        theme::block(focused).title(theme::chrome_title(title)),
        viewport.outer_area,
    );
    let body = viewport.content_area;
    match presence {
        Presence::Off { plugin } => {
            frame.render_widget(
                Paragraph::new(off_text(&plugin))
                    .wrap(Wrap { trim: true })
                    .style(Style::new().add_modifier(Modifier::DIM)),
                body,
            );
        }
        Presence::Drawn { plugin } => {
            let Some(plugins) = app.wasm_plugins.as_ref() else {
                return true;
            };
            let target = pane_target(viewport.pane_id);
            plugins.ensure_pane_view(&plugin, &target);
            plugins.set_view_area(&plugin, &target, (body.width, body.height));
            let cursor = app
                .plugin_pane_cursors
                .get(&viewport.pane_id)
                .cloned()
                .unwrap_or_default();
            match plugins.view_tree(&plugin, &target) {
                Some(tree) => {
                    let scene = view::compose_with(&tree, body, &cursor.states);
                    view::draw(frame, &scene, &cursor, focused);
                }
                // The plugin has not drawn the pane yet.
                None => frame.render_widget(Paragraph::new(Line::from("…")), body),
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_core::ROOT_ID;

    fn tree_with_plugin_pane() -> (Tree, NodeId, NodeId) {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let kind = PluginPaneKind::new("ilium.core.kanban.board").unwrap();
        let plugin_pane = tree.add_plugin_pane(group, "Sprint", kind).unwrap();
        let terminal = tree
            .add_pane(group, "shell", ilium_core::PaneContentKind::Terminal)
            .unwrap();
        (tree, plugin_pane, terminal)
    }

    #[test]
    fn a_plugin_pane_is_owned_by_the_plugin_its_kind_names() {
        let (tree, plugin_pane, terminal) = tree_with_plugin_pane();
        assert_eq!(owner(&tree, plugin_pane), Some("ilium.core.kanban"));
        assert_eq!(owner(&tree, terminal), None);
        assert_eq!(owner(&tree, NodeId(999_999)), None);
    }

    #[test]
    fn a_plugin_pane_is_drawn_as_the_pane_view_of_its_node() {
        let (_, plugin_pane, _) = tree_with_plugin_pane();
        assert_eq!(
            pane_target(plugin_pane),
            ViewTarget::Pane(ilium_plugin_api::generated::types::NodeId(plugin_pane.0))
        );
    }

    #[test]
    fn the_off_frame_names_the_plugin_and_how_to_bring_the_pane_back() {
        let text = off_text("ilium.core.kanban");
        assert!(text.contains("ilium.core.kanban is off"));
        assert!(text.contains("turned on again"));
    }
}
