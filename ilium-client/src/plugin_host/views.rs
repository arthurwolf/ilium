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

/// The terminal-menu actions of contributed menu entries: those that name `terminal` among
/// their targets and run a command, in contribution order.
fn terminal_menu_actions(
    items: &[ChromeItem],
) -> Vec<crate::terminal_context_menu::TerminalContextAction> {
    items
        .iter()
        .filter(|item| {
            item.target
                .as_deref()
                .is_some_and(|kinds| kinds.split(',').any(|kind| kind.trim() == "terminal"))
        })
        .filter_map(|item| {
            Some(
                crate::terminal_context_menu::TerminalContextAction::Plugin {
                    title: sanitize_plugin_text(&item.title, SEGMENT_LIMIT),
                    command: item.command.clone()?,
                },
            )
        })
        .collect()
}

/// Longest plugin toolbar glyph, in display cells: one toolbar button holds two.
const TOOLBAR_GLYPH_CELLS: usize = 2;

/// What the tree toolbar draws for a contributed button. The manifest's `icon` is used when
/// it is a glyph that fits the button; an icon name such as `plus` does not, so the first
/// letter of the title stands in for it.
fn toolbar_button(item: &ChromeItem) -> crate::tree_ui::PluginToolbarButton {
    let icon = item
        .icon
        .as_deref()
        .map(|icon| sanitize_plugin_text(icon.trim(), TOOLBAR_GLYPH_CELLS))
        .filter(|icon| {
            !icon.is_empty()
                && !icon.chars().any(char::is_alphanumeric)
                && unicode_width::UnicodeWidthStr::width(icon.as_str()) <= TOOLBAR_GLYPH_CELLS
        });
    let title = sanitize_plugin_text(&item.title, SEGMENT_LIMIT);
    let glyph = icon.unwrap_or_else(|| {
        title
            .chars()
            .find(|character| character.is_alphanumeric())
            .map_or_else(|| "◆".to_owned(), |first| first.to_uppercase().collect())
    });
    crate::tree_ui::PluginToolbarButton { glyph, title }
}

/// The setting row a Settings text prompt edits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginSettingTarget {
    pub plugin: String,
    pub key: String,
    /// The row title, shown as the prompt's title.
    pub title: String,
}

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
        // Contributed row actions have no drawn button yet, so the menu is where they are
        // reached, on the nodes they apply to. Toolbar buttons are drawn in the tree toolbar
        // (`plugin_toolbar_buttons`), but a narrow or hidden toolbar drops them first, so
        // the menu keeps them reachable on every node.
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
            "all" => true,
            "pane" | "panes" => is_pane,
            "agent" | "agents" => is_agent,
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

    /// The buttons enabled plugins contribute to the tree toolbar, in contribution order.
    /// `TreeToolbarAction::Plugin(index)` indexes this list; the renderer and the click
    /// handler both read it, so an index means the same button in both.
    pub(crate) fn plugin_toolbar_buttons(&self) -> Vec<crate::tree_ui::PluginToolbarButton> {
        self.plugin_chrome("tree_toolbar")
            .iter()
            .map(toolbar_button)
            .collect()
    }

    /// Runs the contributed toolbar button at `index`: its command with the selected node,
    /// or, for a button that runs no command, the popup it names.
    pub(crate) fn run_plugin_toolbar_button(&mut self, index: usize) {
        let Some(item) = self.plugin_chrome("tree_toolbar").into_iter().nth(index) else {
            return;
        };
        if item.command.is_some() {
            let node = self.selected_node_id();
            self.run_plugin_chrome(
                &item,
                node,
                ilium_plugin_api::generated::types::CommandOrigin::TreeToolbar,
            );
            return;
        }
        match (item.popup_view.as_deref(), self.wasm_plugins.as_ref()) {
            (Some(popup), Some(plugins)) => plugins.show_popup(&item.plugin, popup),
            _ => {
                self.status_message = Some(format!(
                    "{} runs no command and opens no popup",
                    sanitize_plugin_text(&item.title, SEGMENT_LIMIT)
                ));
            }
        }
    }

    /// The tree orderings enabled plugins contribute (`contributes.tree_orderings`), offered
    /// in the tree's "Order by" submenu.
    pub(crate) fn plugin_tree_orderings(&self) -> Vec<ChromeItem> {
        self.plugin_chrome("tree_orderings")
    }

    /// Whether a plugin tree ordering is chosen and its plugin still contributes it. A choice
    /// whose plugin was disabled falls back to the built-in order the user picked.
    pub(crate) fn plugin_tree_ordering_in_effect(&self) -> bool {
        self.plugin_tree_ordering
            .as_ref()
            .is_some_and(|(plugin, id)| {
                self.plugin_tree_orderings()
                    .iter()
                    .any(|ordering| ordering.plugin == *plugin && ordering.id == *id)
            })
    }

    /// The per-node keys of the plugin tree ordering in effect: the sort keys its plugin set
    /// with decorations in the ordering's slot. Empty when no plugin ordering is chosen, or
    /// when the chosen one is no longer contributed, which leaves the manual order.
    pub(crate) fn plugin_tree_order_ranks(
        &self,
    ) -> std::collections::HashMap<ilium_core::NodeId, f64> {
        let (Some((plugin, id)), Some(plugins)) = (
            self.plugin_tree_ordering.as_ref(),
            self.wasm_plugins.as_ref(),
        ) else {
            return std::collections::HashMap::new();
        };
        self.plugin_tree_orderings()
            .into_iter()
            .find(|ordering| ordering.plugin == *plugin && ordering.id == *id)
            .and_then(|ordering| ordering.slot)
            .map(|slot| plugins.sort_keys(plugin, &slot))
            .unwrap_or_default()
    }

    /// The terminal right-click menu entries enabled plugins contribute: `context_menu`
    /// entries whose `target`/`applies_to` names `terminal` and that run a command (G6).
    pub(crate) fn plugin_terminal_menu_actions(
        &self,
    ) -> Vec<crate::terminal_context_menu::TerminalContextAction> {
        terminal_menu_actions(&self.plugin_chrome("context_menu"))
    }

    /// Runs a plugin's terminal menu entry with the pane the menu was opened on.
    pub(crate) fn run_plugin_terminal_entry(&mut self, command: &str, pane: ilium_core::NodeId) {
        let arguments = serde_json::json!({ "node": pane.0 }).to_string();
        self.invoke_plugin_command(
            command,
            arguments,
            ilium_plugin_api::generated::types::CommandOrigin::Menu,
        );
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
        self.settings_plugin_row_action(row, Step::Next, true)
    }

    /// A key on a row of the plugin list. `step` is the way a setting row moves (Left and `h`
    /// step back). `open_prompt` is true for Enter and Space: a row whose value is typed (a
    /// number) then opens a text prompt instead of stepping. Text, path, colour and free-choice
    /// rows open their prompt on any key, since they do not step. Returns whether a plugin view
    /// was opened.
    pub(crate) fn settings_plugin_row_action(
        &mut self,
        row: usize,
        step: Step,
        open_prompt: bool,
    ) -> bool {
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
            Some(PluginListRow::Setting(setting)) => {
                let typed = self
                    .plugin_setting_rows()
                    .into_iter()
                    .find(|candidate| {
                        candidate.plugin == setting.plugin && candidate.key == setting.key
                    })
                    .filter(|candidate| {
                        candidate.takes_text() && (open_prompt || !candidate.is_editable())
                    });
                match typed {
                    Some(candidate) => self.open_plugin_setting_prompt(&candidate),
                    None => {
                        self.settings_step_plugin_row(row, step);
                    }
                }
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
            rows.extend(rows_for(&setting.plugin, &manifest, &stored));
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
        if changed {
            self.notify_plugin_setting_changed(&edited);
        }
        changed
    }

    /// Opens the text prompt of a typed setting row over Settings.
    fn open_plugin_setting_prompt(&mut self, row: &SettingRow) {
        let target = PluginSettingTarget {
            plugin: row.plugin.clone(),
            key: row.key.clone(),
            title: sanitize_plugin_text(&row.title, SEGMENT_LIMIT),
        };
        self.push_modal(crate::app::Mode::PluginSettingPrompt(
            target,
            crate::text_prompt::TextPromptState::new(row.prompt_text()),
        ));
    }

    /// Stores the text typed into a setting prompt. The value is checked against the row's
    /// schema; a refusal is shown in the status line and nothing is stored. A change is told
    /// to the client side of the plugin as `settings-changed`.
    pub(crate) fn settings_commit_plugin_setting(
        &mut self,
        target: &PluginSettingTarget,
        text: &str,
    ) -> bool {
        let Some(mut edited) = self
            .plugin_setting_rows()
            .into_iter()
            .find(|candidate| candidate.plugin == target.plugin && candidate.key == target.key)
        else {
            self.status_message = Some(format!("{} is no longer offered", target.title));
            return false;
        };
        let changed = match edited.typed(text) {
            Ok(value) => {
                let store = ilium_plugin_host::config_settings::PluginSettingsStore::of_user();
                edited.commit(&store, value)
            }
            Err(reason) => {
                edited.validation = Some(reason);
                false
            }
        };
        self.status_message = Some(match &edited.validation {
            Some(reason) => format!("{}: {reason}", edited.title),
            None => format!("{} is now {}", edited.title, edited.display()),
        });
        if changed {
            self.notify_plugin_setting_changed(&edited);
        }
        changed
    }

    /// Tells the plugin that owns a row that the user changed it in Settings.
    /// Both halves hear it: the client side directly, the server side through the server.
    fn notify_plugin_setting_changed(&mut self, row: &SettingRow) {
        let Some(plugins) = self.wasm_plugins.as_ref() else {
            return;
        };
        plugins.settings_changed(&row.plugin, vec![row.key.clone()]);
        let request = plugins.settings_changed_request(&row.plugin, vec![row.key.clone()]);
        // A refused request already told the user why.
        let _ = self.queue_request(request);
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
    pub(crate) fn plugin_decoration_text(
        &self,
        node: ilium_core::NodeId,
        frame_slots: &[ChromeItem],
    ) -> Option<String> {
        let plugins = self.wasm_plugins.as_ref()?;
        if !plugins.has_decorations() {
            return None;
        }
        // A decoration in a pane frame slot (`frame_slots`, read once per frame) belongs to
        // the pane's frame, not the tree row.
        let texts: Vec<String> = plugins
            .decorations_of(node)
            .iter()
            .filter(|decoration| {
                !frame_slots
                    .iter()
                    .any(|slot| slot.plugin == decoration.plugin && slot.id == decoration.slot)
            })
            .map(|decoration| sanitize_plugin_text(&decoration.text, SEGMENT_LIMIT))
            .filter(|text| !text.trim().is_empty())
            .collect();
        (!texts.is_empty()).then(|| texts.join(" "))
    }
}

impl App {
    /// What plugins show in the header of a pane's frame: the decorations they set on the
    /// pane in their `pane_frame_slots` placed `header-icon` or `header-text` that apply to
    /// the pane, icons first, in slot order. `None` when there is nothing to show.
    pub(crate) fn plugin_frame_header_text(&self, node: ilium_core::NodeId) -> Option<String> {
        let plugins = self.wasm_plugins.as_ref()?;
        if !plugins.has_decorations() {
            return None;
        }
        let slots: Vec<ChromeItem> = self
            .plugin_chrome("pane_frame_slots")
            .into_iter()
            .filter(|slot| frame_header_rank(slot).is_some())
            .filter(|slot| self.frame_slot_applies_to(slot, node))
            .collect();
        if slots.is_empty() {
            return None;
        }
        frame_header_text(&slots, &plugins.decorations_of(node))
    }
}

impl crate::app::App {
    /// Whether the pane frame slot `slot` belongs on the frame of pane `node`.
    fn frame_slot_applies_to(&self, slot: &ChromeItem, node: ilium_core::NodeId) -> bool {
        let Some(ilium_core::NodeKind::Pane {
            content, status, ..
        }) = self.tree.get(node).map(|node| &node.kind)
        else {
            return false;
        };
        let content = match content {
            ilium_core::PaneContentKind::Terminal => "terminal",
            ilium_core::PaneContentKind::Editor => "editor",
            ilium_core::PaneContentKind::Board => "board",
        };
        let signature = match status {
            ilium_core::PaneStatus::Agent(state) => Some(match &state.class {
                ilium_core::AgentClass::Claude => "claude".to_owned(),
                ilium_core::AgentClass::Codex => "codex".to_owned(),
                ilium_core::AgentClass::Antigravity => "antigravity".to_owned(),
                ilium_core::AgentClass::Other(name) => name.to_lowercase(),
            }),
            _ => None,
        };
        frame_slot_applies(slot.target.as_deref(), content, signature.as_deref())
    }
}

/// Whether a pane frame slot whose `applies_to` is `applies_to` (comma-joined) matches a
/// pane showing `content` (`terminal`, `editor`, `board`) with agent `signature`, if any.
/// The vocabulary is the manual's: `all`, `agents`, `terminals`, `pane:<kind>`, or agent
/// signature ids. A slot that does not say applies to every pane.
fn frame_slot_applies(applies_to: Option<&str>, content: &str, signature: Option<&str>) -> bool {
    let Some(applies_to) = applies_to else {
        return true;
    };
    applies_to.split(',').map(str::trim).any(|kind| match kind {
        "all" => true,
        "agents" => signature.is_some(),
        "terminals" => content == "terminal",
        _ => match kind.strip_prefix("pane:") {
            Some(pane_kind) => pane_kind == content,
            None => signature.is_some_and(|signature| signature.eq_ignore_ascii_case(kind)),
        },
    })
}

/// 0 for a `header-icon` slot, 1 for a `header-text` slot, `None` for a slot placed
/// elsewhere.
fn frame_header_rank(slot: &ChromeItem) -> Option<u8> {
    match slot.placement.as_deref() {
        Some("header-icon") => Some(0),
        Some("header-text") => Some(1),
        _ => None,
    }
}

/// The header text of the decorations that sit in one of `slots`: icon slots first, then
/// text slots, each in slot order, sanitized.
fn frame_header_text(
    slots: &[ChromeItem],
    decorations: &[super::host::TreeDecoration],
) -> Option<String> {
    let mut ordered: Vec<(u8, usize, String)> = decorations
        .iter()
        .filter_map(|decoration| {
            let (index, slot) = slots
                .iter()
                .enumerate()
                .find(|(_, slot)| slot.plugin == decoration.plugin && slot.id == decoration.slot)?;
            let rank = frame_header_rank(slot)?;
            let shown = if rank == 0 {
                decoration
                    .icon
                    .as_deref()
                    .unwrap_or(decoration.text.as_str())
            } else {
                decoration.text.as_str()
            };
            let shown = sanitize_plugin_text(shown.trim(), SEGMENT_LIMIT);
            (!shown.is_empty()).then_some((rank, index, shown))
        })
        .collect();
    ordered.sort_by_key(|(rank, index, _)| (*rank, *index));
    let texts: Vec<String> = ordered.into_iter().map(|(_, _, shown)| shown).collect();
    (!texts.is_empty()).then(|| texts.join(" "))
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

    fn toolbar_item(icon: Option<&str>, title: &str) -> ChromeItem {
        ChromeItem {
            plugin: "ilium.notes".to_owned(),
            id: "new".to_owned(),
            title: title.to_owned(),
            icon: icon.map(str::to_owned),
            command: Some("ilium.notes/new".to_owned()),
            target: None,
            order: 0,
            popup_view: None,
            slot: None,
            placement: None,
        }
    }

    #[test]
    fn a_toolbar_button_uses_a_glyph_icon_or_the_first_letter_of_its_title() {
        let button = toolbar_button(&toolbar_item(Some("✚"), "New note"));
        assert_eq!(button.glyph, "✚");
        assert_eq!(button.title, "New note");
        // An icon name is no glyph.
        assert_eq!(
            toolbar_button(&toolbar_item(Some("plus"), "new note")).glyph,
            "N"
        );
        // Too wide for a button.
        assert_eq!(toolbar_button(&toolbar_item(Some("→→→"), "Go")).glyph, "G");
        assert_eq!(toolbar_button(&toolbar_item(None, "\u{1b}[31m")).glyph, "◆");
    }

    #[test]
    fn frame_header_text_puts_icons_first_and_skips_other_slots() {
        let slot = |id: &str, placement: &str| ChromeItem {
            id: id.to_owned(),
            placement: Some(placement.to_owned()),
            ..toolbar_item(None, id)
        };
        let slots = [
            slot("branch", "header-text"),
            slot("state", "header-icon"),
            slot("log", "footer-row"),
        ];
        let decoration =
            |slot: &str, text: &str, icon: Option<&str>| super::super::host::TreeDecoration {
                plugin: "ilium.notes".to_owned(),
                node: ilium_plugin_api::generated::types::NodeId(4),
                slot: slot.to_owned(),
                text: text.to_owned(),
                icon: icon.map(str::to_owned),
                sort_key: 0,
            };
        let decorations = [
            decoration("branch", "main", None),
            decoration("state", "ok", Some("✓")),
            decoration("log", "tail", None),
            decoration("other", "x", None),
        ];
        assert_eq!(
            frame_header_text(&slots, &decorations).as_deref(),
            Some("✓ main")
        );
        assert_eq!(frame_header_text(&slots, &decorations[2..]), None);
    }

    #[test]
    fn a_frame_slot_follows_the_applies_to_vocabulary_of_the_manual() {
        assert!(frame_slot_applies(None, "editor", None));
        assert!(frame_slot_applies(Some("all"), "board", None));
        assert!(frame_slot_applies(
            Some("agents"),
            "terminal",
            Some("codex")
        ));
        assert!(!frame_slot_applies(Some("agents"), "terminal", None));
        assert!(frame_slot_applies(Some("terminals"), "terminal", None));
        assert!(!frame_slot_applies(Some("terminals"), "editor", None));
        assert!(frame_slot_applies(Some("pane:editor"), "editor", None));
        assert!(!frame_slot_applies(Some("pane:editor"), "terminal", None));
        assert!(frame_slot_applies(
            Some("claude,codex"),
            "terminal",
            Some("claude")
        ));
        assert!(!frame_slot_applies(
            Some("claude,codex"),
            "terminal",
            Some("antigravity")
        ));
        assert!(!frame_slot_applies(Some("claude,codex"), "terminal", None));
    }

    #[test]
    fn a_frame_slot_reads_its_applies_to_as_a_list_or_one_word() {
        let manifest = r#"{"contributes":{"pane_frame_slots":[
            {"id":"model","placement":"header-text","applies_to":["claude","codex"]},
            {"id":"any","placement":"header-icon","applies_to":"agents"}
        ]}}"#;
        let slots = super::super::host::chrome_items("ilium.notes", manifest, "pane_frame_slots");
        assert_eq!(slots[0].target.as_deref(), Some("claude,codex"));
        assert_eq!(slots[1].target.as_deref(), Some("agents"));
    }

    #[test]
    fn a_view_keeps_a_margin_inside_its_frame() {
        assert_eq!(inner(Rect::new(10, 2, 40, 12)), Rect::new(12, 3, 36, 10));
        assert_eq!(inner(Rect::new(0, 0, 3, 1)).width, 0);
    }

    #[test]
    fn only_terminal_menu_entries_with_a_command_reach_the_terminal_menu() {
        let manifest = r#"{"contributes":{"context_menu":[
            {"id":"explain","title":"Explain output","command":"explain","target":"terminal"},
            {"id":"both","title":"Pin","command":"x.y/pin","applies_to":["pane","terminal"]},
            {"id":"tree","title":"Tree only","command":"tree","target":"pane"},
            {"id":"inert","title":"No command","target":"terminal"}
        ]}}"#;
        let items = super::super::host::chrome_items("example.plugin", manifest, "context_menu");
        let actions = terminal_menu_actions(&items);
        assert_eq!(
            actions,
            vec![
                crate::terminal_context_menu::TerminalContextAction::Plugin {
                    title: "Explain output".to_owned(),
                    command: "example.plugin/explain".to_owned(),
                },
                crate::terminal_context_menu::TerminalContextAction::Plugin {
                    title: "Pin".to_owned(),
                    command: "x.y/pin".to_owned(),
                },
            ]
        );
        assert_eq!(actions[0].label(), "Explain output");
    }
}
