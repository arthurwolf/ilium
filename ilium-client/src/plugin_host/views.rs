//! Where the client shows what plugins contribute (foundation contract section 19.1).
//!
//! A panel, a settings page or an onboarding step of a plugin is shown in the right-hand area,
//! like the chatroom and the attention list: it owns no PTY and no split-view slot. The tree is
//! rendered by the plugin on the host thread; this module only places it, draws it with
//! `view`, and turns keys and clicks into `ui-action`s. Status segments and tree decorations
//! are read here for the status bar and the tree.

use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, MouseButton, MouseEvent, MouseEventKind};
use ilium_plugin_api::generated::types::ui::UiAction;
use ilium_plugin_api::generated::types::ViewTarget;
use ilium_plugin_host::render::sanitize_plugin_text;
use ratatui::layout::{Position, Rect};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use super::host::ChromeItem;
use super::settings_rows::{rows_for, SettingRow, Step};
use super::view;
use crate::app::{App, FocusTarget, RightPanelTarget};
use crate::theme;

/// Longest status segment and decoration text, in characters.
const SEGMENT_LIMIT: usize = 48;

/// One row of the Settings plugin list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PluginListRow {
    /// A plugin with its on/off switch.
    Plugin(ilium_plugin_host::wiring::PluginRow),
    /// A contributed panel or onboarding step; activating the row opens it.
    View {
        plugin: String,
        kind: PluginViewKind,
        name: String,
        title: String,
    },
    /// A settings row a plugin contributes, shown with its default value.
    Setting(super::host::ContributedSetting),
}

/// Which kind of plugin view the right-hand area shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PluginViewKind {
    Panel,
    SettingsPage,
    OnboardingStep,
}

impl PluginViewKind {
    /// The kind and name of a view target; a popup or a pane view has no place here.
    pub(crate) fn of(view: &ViewTarget) -> Option<(Self, &str)> {
        match view {
            ViewTarget::Panel(name) => Some((Self::Panel, name)),
            ViewTarget::SettingsPage(name) => Some((Self::SettingsPage, name)),
            ViewTarget::OnboardingStep(name) => Some((Self::OnboardingStep, name)),
            ViewTarget::Popup(_) | ViewTarget::Pane(_) => None,
        }
    }

    /// The view target of this kind with a name.
    pub(crate) fn target(self, name: &str) -> ViewTarget {
        match self {
            Self::Panel => ViewTarget::Panel(name.to_owned()),
            Self::SettingsPage => ViewTarget::SettingsPage(name.to_owned()),
            Self::OnboardingStep => ViewTarget::OnboardingStep(name.to_owned()),
        }
    }

    /// The manifest contribution kind that declares views of this kind.
    pub(crate) const fn contribution(self) -> &'static str {
        match self {
            Self::Panel => "panels",
            Self::SettingsPage => "settings",
            Self::OnboardingStep => "onboarding_steps",
        }
    }

    /// The name used in control snapshots.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Panel => "panel",
            Self::SettingsPage => "settings-page",
            Self::OnboardingStep => "onboarding-step",
        }
    }
}

/// The cells a plugin view gets inside the right-hand area: the area without its frame.
fn inner(area: Rect) -> Rect {
    Rect::new(
        area.x.saturating_add(2),
        area.y.saturating_add(1),
        area.width.saturating_sub(4),
        area.height.saturating_sub(2),
    )
}

impl App {
    /// The plugin view the right-hand area shows, if it shows one.
    pub(crate) fn shown_plugin_view(&self) -> Option<(&str, ViewTarget)> {
        match &self.right_panel_target {
            RightPanelTarget::PluginView { plugin, kind, name } => {
                Some((plugin.as_str(), kind.target(name)))
            }
            _ => None,
        }
    }

    /// The entries enabled plugins contribute of one manifest kind; empty without plugins.
    pub(crate) fn plugin_chrome(&self, kind: &str) -> Vec<ChromeItem> {
        self.wasm_plugins
            .as_ref()
            .map(|plugins| plugins.contributed(kind))
            .unwrap_or_default()
    }

    /// The contributed context-menu entries that apply to a tree node (contract section
    /// 20.7). An entry that names node kinds (`applies_to`, `target`) shows only on a
    /// matching node; `terminal` entries belong to the terminal pane's own menu and never
    /// show in the tree menu.
    pub(crate) fn plugin_context_items(&self, target: ilium_core::NodeId) -> Vec<ChromeItem> {
        // Contributed tree toolbar buttons and row actions have no drawn button yet, so the
        // menu is where they are reached: toolbar entries on every node, row actions on the
        // nodes they apply to.
        ["context_menu", "tree_toolbar", "row_actions"]
            .into_iter()
            .flat_map(|kind| self.plugin_chrome(kind))
            .filter(|item| self.plugin_item_applies(item, target))
            .collect()
    }

    /// Whether a contributed entry applies to a node, by the node kinds it names.
    pub(crate) fn plugin_item_applies(
        &self,
        item: &ChromeItem,
        target: ilium_core::NodeId,
    ) -> bool {
        let Some(kinds) = item.target.as_deref() else {
            return true;
        };
        let node = self.tree.get(target);
        let is_pane =
            node.is_some_and(|node| matches!(node.kind, ilium_core::NodeKind::Pane { .. }));
        let is_agent = node.is_some_and(|node| {
            matches!(
                node.kind,
                ilium_core::NodeKind::Pane {
                    status: ilium_core::PaneStatus::Agent(_),
                    ..
                }
            )
        });
        let is_project = node.is_some_and(ilium_core::Node::is_project);
        kinds.split(',').map(str::trim).any(|kind| match kind {
            "pane" => is_pane,
            "agent" => is_agent,
            "group" | "folder" => node.is_some() && !is_pane && target != ilium_core::ROOT_ID,
            "project" => is_project,
            "tree" | "root" => target == ilium_core::ROOT_ID,
            _ => false,
        })
    }

    /// Runs the command of a contributed toolbar button, menu entry or row action. The
    /// arguments name the node the entry was used on, when there is one.
    pub(crate) fn run_plugin_chrome(
        &mut self,
        item: &ChromeItem,
        node: Option<ilium_core::NodeId>,
        origin: ilium_plugin_api::generated::types::CommandOrigin,
    ) {
        let Some(command) = item.command.as_deref() else {
            return;
        };
        let arguments = match node {
            Some(node) => serde_json::json!({ "node": node.0 }).to_string(),
            None => "{}".to_owned(),
        };
        self.invoke_plugin_command(command, arguments, origin);
    }

    /// The rows of the Settings plugin list (contract section 20.6): one switch per plugin,
    /// then one row per contributed panel and onboarding step that opens it, then the
    /// settings rows plugins contribute. Drawing, row counting, keys and clicks all read
    /// this one list, so a row index means the same thing everywhere.
    pub(crate) fn plugin_list_rows(&self) -> Vec<PluginListRow> {
        let mut rows: Vec<PluginListRow> = self
            .wasm_plugin_rows()
            .into_iter()
            .map(PluginListRow::Plugin)
            .collect();
        let Some(plugins) = self.wasm_plugins.as_ref() else {
            return rows;
        };
        for kind in [PluginViewKind::Panel, PluginViewKind::OnboardingStep] {
            rows.extend(
                plugins
                    .contributed(kind.contribution())
                    .into_iter()
                    .map(|item| PluginListRow::View {
                        plugin: item.plugin,
                        kind,
                        name: item.id,
                        title: item.title,
                    }),
            );
        }
        rows.extend(
            plugins
                .contributed_settings()
                .into_iter()
                .map(PluginListRow::Setting),
        );
        rows
    }

    /// Acts on one row of the Settings plugin list: flips a plugin, opens a view, or says
    /// where a contributed setting is edited. Returns true when Settings must close, because
    /// the row opened a view in the right-hand area.
    pub(crate) fn settings_activate_plugin_row(&mut self, row: usize) -> bool {
        match self.plugin_list_rows().into_iter().nth(row) {
            Some(PluginListRow::Plugin(_)) => {
                // Plugin rows come first, so the index is the plugin's own.
                self.settings_toggle_wasm_plugin(row);
                false
            }
            Some(PluginListRow::View {
                plugin, kind, name, ..
            }) => {
                self.show_plugin_view(&plugin, kind, &name);
                true
            }
            Some(PluginListRow::Setting(_)) => {
                self.settings_step_plugin_row(row, Step::Next);
                false
            }
            None => false,
        }
    }

    /// Every contributed settings row with the value in effect (the stored value over the
    /// default). The user's `config.toml` is read once per plugin, not once per row.
    pub(crate) fn plugin_setting_rows(&self) -> Vec<SettingRow> {
        let Some(plugins) = self.wasm_plugins.as_ref() else {
            return Vec::new();
        };
        let store = ilium_plugin_host::config_settings::PluginSettingsStore::of_user();
        let mut rows = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for setting in plugins.contributed_settings() {
            if !seen.insert(setting.plugin.clone()) {
                continue;
            }
            let Some(manifest) = plugins
                .enabled_manifests()
                .into_iter()
                .find_map(|(id, text)| (id == setting.plugin).then_some(text))
            else {
                continue;
            };
            let stored = store.read(&setting.plugin).unwrap_or_default();
            rows.extend(rows_for(&setting.plugin, manifest, &stored));
        }
        rows
    }

    /// Moves the contributed setting under a row of the plugin list one step: a switch flips,
    /// a choice cycles, a number steps inside its range. The value is checked against the
    /// row's schema and written to `config.toml`; a refusal is shown in the status line and
    /// nothing is stored. Returns whether the stored value changed.
    pub(crate) fn settings_step_plugin_row(&mut self, row: usize, step: Step) -> bool {
        let Some(PluginListRow::Setting(setting)) = self.plugin_list_rows().into_iter().nth(row)
        else {
            return false;
        };
        let Some(mut edited) = self
            .plugin_setting_rows()
            .into_iter()
            .find(|candidate| candidate.plugin == setting.plugin && candidate.key == setting.key)
        else {
            return false;
        };
        let store = ilium_plugin_host::config_settings::PluginSettingsStore::of_user();
        let changed = edited.step_and_commit(&store, step);
        self.status_message = Some(match &edited.validation {
            Some(reason) => format!("{}: {reason}", edited.title),
            None => format!("{} is now {}", edited.title, edited.display()),
        });
        changed
    }

    /// Shows a contributed view on the user's request (a menu entry, a key, the plugin list).
    pub(crate) fn show_plugin_view(&mut self, plugin: &str, kind: PluginViewKind, name: &str) {
        let Some(plugins) = self.wasm_plugins.as_ref() else {
            self.status_message = Some(format!(
                "{plugin} is not running, so this view is unavailable"
            ));
            return;
        };
        // The host answers with `ViewOpened`, which places the view.
        plugins.show_view(plugin, kind.target(name));
    }

    /// A plugin view was opened: it takes the right-hand area, and the keyboard if asked.
    pub(crate) fn plugin_view_opened(&mut self, plugin: &str, view: &ViewTarget, focus: bool) {
        let Some((kind, name)) = PluginViewKind::of(view) else {
            return;
        };
        if focus {
            // Leaving a pane for the view must release the pane's focus first.
            self.leave_pane_focus();
        }
        self.plugin_view_cursor = view::ViewCursor::default();
        self.right_panel_target = RightPanelTarget::PluginView {
            plugin: plugin.to_owned(),
            kind,
            name: name.to_owned(),
        };
        if focus {
            self.focus = FocusTarget::Pane;
        }
    }

    /// A plugin view went away: the right-hand area is emptied if it showed that view.
    pub(crate) fn plugin_view_closed(&mut self, plugin: &str, view: &ViewTarget) {
        let shown = self
            .shown_plugin_view()
            .is_some_and(|(shown_plugin, shown_view)| {
                shown_plugin == plugin && &shown_view == view
            });
        if shown {
            self.right_panel_target = RightPanelTarget::Empty;
            self.focus = FocusTarget::Tree;
        }
    }

    /// Keyboard in a plugin view: Esc returns to the tree (unless it closes a number entry), a
    /// focused text input or number edits, the rest moves the cursor or sends the action under it.
    pub(crate) fn handle_plugin_view_key(&mut self, key: KeyEvent) {
        if key.kind == KeyEventKind::Release {
            return;
        }
        if key.code == KeyCode::Esc && !view::captures_escape(&self.plugin_view_cursor) {
            self.leave_pane_focus();
            return;
        }
        let Some((plugin, target, scene)) = self.compose_shown_plugin_view() else {
            return;
        };
        let mut cursor = std::mem::take(&mut self.plugin_view_cursor);
        let actions = view::key(&scene, &target, &mut cursor, key, Instant::now());
        self.plugin_view_cursor = cursor;
        self.send_plugin_view_actions(&plugin, actions);
    }

    /// Pointer in a plugin view: a click gives it the keyboard and sends the action under it;
    /// the wheel scrolls, a drag moves a scrollbar, a divider or a slider, and the release ends it.
    pub(crate) fn handle_plugin_view_mouse(&mut self, mouse: MouseEvent, position: Position) {
        if !matches!(
            mouse.kind,
            MouseEventKind::Down(MouseButton::Left)
                | MouseEventKind::Drag(MouseButton::Left)
                | MouseEventKind::Up(MouseButton::Left)
                | MouseEventKind::ScrollUp
                | MouseEventKind::ScrollDown
        ) {
            return;
        }
        let Some((plugin, target, scene)) = self.compose_shown_plugin_view() else {
            return;
        };
        let mut cursor = std::mem::take(&mut self.plugin_view_cursor);
        let actions = match mouse.kind {
            MouseEventKind::Down(_) => {
                self.focus = FocusTarget::Pane;
                view::click(&scene, &target, &mut cursor, position, Instant::now())
            }
            MouseEventKind::Drag(_) => {
                let mut moved = Vec::new();
                view::drag(&target, &mut cursor, position, &mut moved);
                moved
            }
            MouseEventKind::Up(_) => view::release(&target, &mut cursor),
            MouseEventKind::ScrollUp => view::wheel(&scene, &target, &mut cursor, position, -1),
            _ => view::wheel(&scene, &target, &mut cursor, position, 1),
        };
        self.plugin_view_cursor = cursor;
        self.send_plugin_view_actions(&plugin, actions);
    }

    /// A paste goes to the text input of the shown plugin view that has the keyboard. Returns
    /// whether the view took it.
    pub(crate) fn handle_plugin_view_paste(&mut self, pasted: &str) -> bool {
        if !matches!(self.focus, FocusTarget::Pane) {
            return false;
        }
        let Some((plugin, target, scene)) = self.compose_shown_plugin_view() else {
            return false;
        };
        let mut cursor = std::mem::take(&mut self.plugin_view_cursor);
        let actions = view::paste(&scene, &target, &mut cursor, pasted, Instant::now());
        self.plugin_view_cursor = cursor;
        let taken = actions.is_some();
        self.send_plugin_view_actions(&plugin, actions.unwrap_or_default());
        taken
    }

    /// Sends the plugin view edits whose debounce ran out. Returns whether anything was sent.
    pub(crate) fn tick_plugin_view_edits(&mut self, now: Instant) -> bool {
        let Some((plugin, target)) = self
            .shown_plugin_view()
            .map(|(plugin, target)| (plugin.to_owned(), target))
        else {
            return false;
        };
        if view::next_due(&self.plugin_view_cursor).is_none() {
            return false;
        }
        let actions = view::tick(&target, &mut self.plugin_view_cursor, now);
        let sent = !actions.is_empty();
        self.send_plugin_view_actions(&plugin, actions);
        sent
    }

    /// The shown plugin view with its tree laid out for the right-hand area.
    fn compose_shown_plugin_view(&self) -> Option<(String, ViewTarget, view::Scene)> {
        let (plugin, target) = self
            .shown_plugin_view()
            .map(|(plugin, target)| (plugin.to_owned(), target))?;
        let tree = self
            .wasm_plugins
            .as_ref()
            .and_then(|plugins| plugins.view_tree(&plugin, &target))?;
        let scene = view::compose_with(
            &tree,
            inner(self.layout.pane_area),
            &self.plugin_view_cursor.states,
        );
        Some((plugin, target, scene))
    }

    fn send_plugin_view_actions(&self, plugin: &str, actions: Vec<UiAction>) {
        if let Some(plugins) = self.wasm_plugins.as_ref() {
            for action in actions {
                plugins.view_action(plugin, action);
            }
        }
    }

    /// Mirrors the leader key table into the plugin host, so `keymap.bindings` answers with
    /// what the user really has. Commands are named `ilium/<action>`; chords are the config
    /// spelling of the key after the leader.
    pub(crate) fn mirror_plugin_keymap(&self) {
        let Some(plugins) = self.wasm_plugins.as_ref() else {
            return;
        };
        let bindings = self
            .keybindings
            .iter()
            .map(|binding| ilium_plugin_api::kernel::KeyBinding {
                command: format!("ilium/{}", crate::keymap::action_name(binding.action)),
                chords: vec![binding.key.config_value()],
                context: "leader".to_owned(),
                // The client does not track which bindings the user changed.
                is_default: false,
            })
            .collect();
        plugins.sync_keymap(bindings);
    }

    /// The status segments of plugins as one text for the status bar; `None` when no plugin
    /// set one. Each segment is sanitized and cut to a fixed length.
    pub(crate) fn plugin_status_text(&self) -> Option<String> {
        let segments = self.wasm_plugins.as_ref()?.status_segments();
        let texts: Vec<String> = segments
            .iter()
            .map(|segment| sanitize_plugin_text(&segment.text, SEGMENT_LIMIT))
            .filter(|text| !text.trim().is_empty())
            .collect();
        (!texts.is_empty()).then(|| texts.join(" · "))
    }

    /// What client-side plugins attach to one tree row, as one sanitized text; `None` when
    /// nothing decorates the node.
    pub(crate) fn plugin_decoration_text(&self, node: ilium_core::NodeId) -> Option<String> {
        let plugins = self.wasm_plugins.as_ref()?;
        if !plugins.has_decorations() {
            return None;
        }
        let texts: Vec<String> = plugins
            .decorations_of(node)
            .iter()
            .map(|decoration| sanitize_plugin_text(&decoration.text, SEGMENT_LIMIT))
            .filter(|text| !text.trim().is_empty())
            .collect();
        (!texts.is_empty()).then(|| texts.join(" "))
    }
}

/// Closes every plugin view the right-hand area no longer shows, so a plugin is not asked to
/// render something nobody sees. Called once per frame; it only reads the list of views that
/// were placed, so a view opened a moment ago is never closed before its first frame.
pub(crate) fn release_hidden(app: &App) {
    let Some(plugins) = app.wasm_plugins.as_ref() else {
        return;
    };
    let shown = app.shown_plugin_view();
    for (plugin, target) in plugins.placed_views() {
        let is_shown = shown.as_ref().is_some_and(|(shown_plugin, shown_view)| {
            *shown_plugin == plugin && shown_view == &target
        });
        if !is_shown {
            plugins.dismiss_view(&plugin, &target);
        }
    }
}

/// Draws the plugin view of the right-hand area and tells the host how many cells it has.
pub(crate) fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let Some((plugin, target)) = app.shown_plugin_view() else {
        return;
    };
    let Some(plugins) = app.wasm_plugins.as_ref() else {
        return;
    };
    let focused = matches!(app.focus, FocusTarget::Pane);
    let title = PluginViewKind::of(&target)
        .and_then(|(kind, name)| {
            plugins
                .contributed(kind.contribution())
                .into_iter()
                .find(|item| item.plugin == plugin && item.id == name)
        })
        .map_or_else(|| plugin.to_owned(), |item| item.title);
    frame.render_widget(
        theme::block(focused).title(theme::chrome_title(&sanitize_plugin_text(&title, 80))),
        area,
    );
    let body = inner(area);
    plugins.set_view_area(plugin, &target, (body.width, body.height));
    match plugins.view_tree(plugin, &target) {
        Some(tree) => {
            let scene = view::compose_with(&tree, body, &app.plugin_view_cursor.states);
            view::draw(frame, &scene, &app.plugin_view_cursor, focused);
        }
        // The plugin has not rendered yet.
        None => frame.render_widget(Paragraph::new(Line::from("…")), body),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_view_kind_round_trips_through_its_target() {
        for kind in [
            PluginViewKind::Panel,
            PluginViewKind::SettingsPage,
            PluginViewKind::OnboardingStep,
        ] {
            let target = kind.target("name");
            assert_eq!(PluginViewKind::of(&target), Some((kind, "name")));
        }
        assert_eq!(PluginViewKind::of(&ViewTarget::Popup("p".to_owned())), None);
    }

    #[test]
    fn a_view_keeps_a_margin_inside_its_frame() {
        assert_eq!(inner(Rect::new(10, 2, 40, 12)), Rect::new(12, 3, 36, 10));
        assert_eq!(inner(Rect::new(0, 0, 3, 1)).width, 0);
    }
}
