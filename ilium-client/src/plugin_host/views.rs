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
use super::settings_rows::{rows_with_kernel, SettingRow, Step};
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
    crate::tree_ui::PluginToolbarButton {
        glyph,
        title,
        caption: item.caption.as_deref().and_then(toolbar_caption),
    }
}

/// A `caption` cut to the cells a toolbar button shows beside its glyph; `None` when nothing
/// printable is left.
fn toolbar_caption(caption: &str) -> Option<String> {
    let limit = usize::from(crate::tree_ui::PLUGIN_TOOLBAR_CAPTION_CELLS);
    let sanitized = sanitize_plugin_text(caption.trim(), limit);
    let mut shown = String::new();
    let mut cells = 0;
    for character in sanitized.chars() {
        let width = unicode_width::UnicodeWidthChar::width(character).unwrap_or(0);
        if cells + width > limit {
            break;
        }
        cells += width;
        shown.push(character);
    }
    let shown = shown.trim().to_owned();
    (!shown.is_empty()).then_some(shown)
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
        let context = self.when_context(Some(target));
        ["context_menu", "tree_toolbar", "row_actions"]
            .into_iter()
            .flat_map(|kind| self.plugin_chrome(kind))
            .filter(|item| self.plugin_item_applies(item, target))
            .filter(|item| super::when::shows(item.when.as_deref(), &context))
            .flat_map(|item| self.expand_dynamic_menu_item(item, target))
            .collect()
    }

    /// A `dynamic` menu entry becomes the entries its plugin's menu provider returns for
    /// `target`; an entry the provider reports as disabled is not offered. Any other entry is
    /// returned unchanged.
    fn expand_dynamic_menu_item(
        &self,
        item: ChromeItem,
        target: ilium_core::NodeId,
    ) -> Vec<ChromeItem> {
        if !item.dynamic {
            return vec![item];
        }
        let Some(plugins) = self.wasm_plugins.as_ref() else {
            return Vec::new();
        };
        let node = ilium_plugin_api::generated::types::NodeId(target.0);
        plugins
            .menu_entries(&item.plugin, "pane", node)
            .into_iter()
            .filter(|entry| !entry.is_submenu && entry.disabled_reason.is_none())
            .map(|entry| ChromeItem {
                id: format!("{}/{}", item.id, entry.id),
                title: entry.label,
                icon: entry.icon,
                dynamic: false,
                menu_item: Some(entry.id),
                ..item.clone()
            })
            .collect()
    }

    /// The values of the `when` context keys with `node` in scope (api-commands.md "Context
    /// expressions"). Without a node, or on a node that is not a pane, `pane.primitive` is
    /// `none` and the pane and agent keys have nothing in scope.
    pub(crate) fn when_context(
        &self,
        node: Option<ilium_core::NodeId>,
    ) -> super::when::WhenContext {
        let view_focused = matches!(self.right_panel_target, RightPanelTarget::PluginView { .. })
            && matches!(self.focus, FocusTarget::Pane);
        let display_mode = "normal";
        let mut context = super::when::WhenContext {
            pane_primitive: Some("none".to_owned()),
            view_focused: Some(view_focused.to_string()),
            display_mode: Some(display_mode.to_owned()),
            ..super::when::WhenContext::default()
        };
        let Some((node_id, found)) = node.and_then(|id| self.tree.get(id).map(|found| (id, found)))
        else {
            return context;
        };
        let (content, status, plugin_pane) = match &found.kind {
            ilium_core::NodeKind::Pane {
                content,
                status,
                plugin_pane,
                ..
            } => (content, status, plugin_pane),
            ilium_core::NodeKind::Folder { .. } => {
                context.node_kind = Some("folder".to_owned());
                return context;
            }
            ilium_core::NodeKind::Container(_) => {
                let kind = if found.is_project() {
                    "project"
                } else {
                    "group"
                };
                context.node_kind = Some(kind.to_owned());
                return context;
            }
        };
        context.node_kind = Some("pane".to_owned());
        let agent = match status {
            ilium_core::PaneStatus::Agent(agent) => Some(agent),
            _ => None,
        };
        let (primitive, kind) = match content {
            ilium_core::PaneContentKind::Terminal if agent.is_some() => ("agent", "terminal"),
            ilium_core::PaneContentKind::Terminal => ("terminal", "terminal"),
            ilium_core::PaneContentKind::Editor => ("editor", "editor"),
            ilium_core::PaneContentKind::Plugin => (
                "basic",
                plugin_pane
                    .as_ref()
                    .map_or("plugin", ilium_core::PluginPaneKind::kind_id),
            ),
        };
        context.pane_primitive = Some(primitive.to_owned());
        context.pane_kind = Some(kind.to_owned());
        if let Some(crate::app::PaneRuntime::Editor(editor)) = self.panes.get(&node_id) {
            context.node_file_extension = editor
                .path
                .as_deref()
                .and_then(std::path::Path::extension)
                .map(|extension| extension.to_string_lossy().into_owned());
        }
        if let Some(agent) = agent {
            context.agent_signature = Some(match &agent.class {
                ilium_core::AgentClass::Claude => "claude".to_owned(),
                ilium_core::AgentClass::Codex => "codex".to_owned(),
                ilium_core::AgentClass::Antigravity => "antigravity".to_owned(),
                ilium_core::AgentClass::Other(name) => name.to_lowercase(),
            });
            context.agent_activity = Some(
                match agent.activity() {
                    ilium_core::AgentActivity::Working
                    | ilium_core::AgentActivity::WaitingBackground
                    | ilium_core::AgentActivity::BackgroundTaskStillRunning => "working",
                    ilium_core::AgentActivity::WaitingApproval => "awaiting-approval",
                    ilium_core::AgentActivity::Done => "done",
                    ilium_core::AgentActivity::Idle => "idle",
                }
                .to_owned(),
            );
        }
        context
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
            "tree-node" => node.is_some(),
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
        let arguments = match (node, item.menu_item.as_deref()) {
            (Some(node), Some(entry)) => {
                serde_json::json!({ "node": node.0, "item": entry }).to_string()
            }
            (Some(node), None) => serde_json::json!({ "node": node.0 }).to_string(),
            (None, _) => "{}".to_owned(),
        };
        self.invoke_plugin_command(command, arguments, origin);
    }

    /// The buttons enabled plugins contribute to the tree toolbar that show for the selected
    /// node, in contribution order. `TreeToolbarAction::Plugin(index)` indexes this list; the
    /// renderer and the click handler both read it, so an index means the same button in
    /// both.
    pub(crate) fn plugin_toolbar_buttons(&self) -> Vec<crate::tree_ui::PluginToolbarButton> {
        self.shown_plugin_toolbar_items()
            .iter()
            .map(toolbar_button)
            .collect()
    }

    /// The contributed tree toolbar entries whose `when` holds for the selected node. A
    /// hidden entry leaves no gap, and both the drawing and the click read this one list.
    fn shown_plugin_toolbar_items(&self) -> Vec<ChromeItem> {
        let items = self.plugin_chrome("tree_toolbar");
        if items.iter().all(|item| item.when.is_none()) {
            return items;
        }
        let context = self.when_context(self.selected_node_id());
        items
            .into_iter()
            .filter(|item| super::when::shows(item.when.as_deref(), &context))
            .collect()
    }

    /// Runs the contributed toolbar button at `index`: its command with the selected node,
    /// or, for a button that runs no command, the popup it names.
    pub(crate) fn run_plugin_toolbar_button(&mut self, index: usize) {
        let Some(item) = self.shown_plugin_toolbar_items().into_iter().nth(index) else {
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
        self.ui_settings
            .plugin_tree_ordering
            .as_ref()
            .is_some_and(|choice| {
                self.plugin_tree_orderings()
                    .iter()
                    .any(|ordering| choice.names(&ordering.plugin, &ordering.id))
            })
    }

    /// The per-node keys of the plugin tree ordering in effect: the sort keys its plugin set
    /// with decorations in the ordering's slot. Empty when no plugin ordering is chosen, or
    /// when the chosen one is no longer contributed, which leaves the manual order.
    pub(crate) fn plugin_tree_order_ranks(
        &self,
    ) -> std::collections::HashMap<ilium_core::NodeId, f64> {
        let (Some(choice), Some(plugins)) = (
            self.ui_settings.plugin_tree_ordering.as_ref(),
            self.wasm_plugins.as_ref(),
        ) else {
            return std::collections::HashMap::new();
        };
        self.plugin_tree_orderings()
            .into_iter()
            .find(|ordering| choice.names(&ordering.plugin, &ordering.id))
            .and_then(|ordering| ordering.slot)
            .map(|slot| plugins.sort_keys(&choice.plugin, &slot))
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
        // A plugin settings page opened from the rail lists that page's rows alone; every
        // row consumer (drawing, keys, clicks, editing) reads this list, so they agree.
        if let Some(open) = self.open_settings_plugin_page() {
            let pages = self.settings_plugin_pages();
            let Some((plugin, page)) = pages.get(open) else {
                return Vec::new();
            };
            return self
                .wasm_plugins
                .as_ref()
                .map(|plugins| {
                    plugins
                        .contributed_settings()
                        .into_iter()
                        .filter(|setting| setting.plugin == *plugin && setting.page == *page)
                        .map(PluginListRow::Setting)
                        .collect()
                })
                .unwrap_or_default();
        }
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
            rows.extend(rows_with_kernel(
                &setting.plugin,
                &manifest,
                &stored,
                &store,
            ));
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

    /// A kernel-bound row of a tab's owner plugin wrote the built-in table of `tab` in
    /// `config.toml`: the client state that mirrors that table is reloaded from the file, so
    /// the workers that read it (titles, restructure, onboarding) see the new value at once.
    fn kernel_setting_changed(&mut self, tab: &str) {
        if tab != "inference" {
            return;
        }
        let Some(directory) = self.config_dir.clone() else {
            return;
        };
        match crate::config::load(&directory) {
            Ok(config) => self.apply_inference_settings_locally(config.inference),
            Err(error) => {
                self.status_message = Some(format!(
                    "The inference settings were saved but not reloaded: {error}"
                ));
            }
        }
    }

    /// Tells the plugin that owns a row that the user changed it in Settings.
    /// Both halves hear it: the client side directly, the server side through the server.
    fn notify_plugin_setting_changed(&mut self, row: &SettingRow) {
        if let Some(binding) = &row.kernel {
            self.kernel_setting_changed(&binding.tab);
        }
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
        // A plugin closing its onboarding step moves the first-run flow to the next one.
        if matches!(view, ViewTarget::OnboardingStep(_)) {
            self.open_next_plugin_onboarding_step();
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
        // An onboarding step the wizard shows lies in the wizard's content frame.
        let area = self
            .plugin_onboarding_step_area()
            .filter(|_| matches!(target, ViewTarget::OnboardingStep(_)))
            .unwrap_or_else(|| inner(self.layout.pane_area));
        let scene = view::compose_with(&tree, area, &self.plugin_view_cursor.states);
        Some((plugin, target, scene))
    }

    fn send_plugin_view_actions(&self, plugin: &str, actions: Vec<UiAction>) {
        if let Some(plugins) = self.wasm_plugins.as_ref() {
            for action in actions {
                plugins.view_action(plugin, action);
            }
        }
    }

    /// Mirrors the key table into the plugin host, so `keymap.bindings` answers with what the
    /// user really has: the leader table first (commands named `ilium/<action>`, context
    /// `leader`), then the enabled plugins' contributed bindings (gap G1). Every chord is the
    /// full grammar text (`shift+c`, `ctrl+shift+f`, `f1`), which `chord_label` shows as
    /// `Shift+C` or `F1`.
    pub(crate) fn mirror_plugin_keymap(&self) {
        let Some(plugins) = self.wasm_plugins.as_ref() else {
            return;
        };
        let mut bindings: Vec<_> = self
            .keybindings
            .iter()
            .map(|binding| ilium_plugin_api::kernel::KeyBinding {
                command: format!("ilium/{}", crate::keymap::action_name(binding.action)),
                chords: vec![super::keybindings::kernel_chord_text(binding.key)],
                context: "leader".to_owned(),
                // The client does not track which bindings the user changed.
                is_default: false,
            })
            .collect();
        bindings.extend(super::keybindings::contributed_keymap(
            &plugins.contributed_bindings(),
        ));
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
        let (slots, contents) =
            self.frame_slots_with_contents(node, |slot| frame_header_rank(slot).is_some())?;
        frame_header_text(&slots, &contents)
    }

    /// What plugins show on the one row of a pane's frame placed `placement` (`banner` above
    /// the terminal, `footer-row` below it): the contents of the slots placed there that
    /// apply to the pane, in slot order. `None` when nothing is set, so the pane keeps its
    /// rows. [`App::pane_viewports`] reserves the row from this, and the frame drawing shows
    /// this, so the terminal size and the drawing never disagree.
    pub(crate) fn plugin_frame_row_text(
        &self,
        node: ilium_core::NodeId,
        placement: FrameRowPlacement,
    ) -> Option<String> {
        let (slots, contents) = self.frame_slots_with_contents(node, |slot| {
            slot.placement.as_deref() == Some(placement.manifest_name())
        })?;
        frame_row_text(&slots, &contents)
    }

    /// The command a click on the footer row of pane `node` runs: the command target of the
    /// first footer-row slot (in slot order) that shows text and whose content carries one.
    /// `None` when the row is absent or nothing on it is clickable.
    pub(crate) fn plugin_footer_command(&self, node: ilium_core::NodeId) -> Option<String> {
        let (slots, contents) = self.frame_slots_with_contents(node, |slot| {
            slot.placement.as_deref() == Some(FrameRowPlacement::FooterRow.manifest_name())
        })?;
        slots.iter().find_map(|slot| {
            contents
                .iter()
                .find(|content| content.plugin == slot.plugin && content.slot == slot.id)
                .filter(|content| !content.text.trim().is_empty())
                .and_then(|content| content.command.clone())
        })
    }

    /// The `pane_frame_slots` entries `keep` selects that apply to pane `node`, with what
    /// plugins set on the pane. `None` when no plugin has frame content or no slot fits.
    fn frame_slots_with_contents(
        &self,
        node: ilium_core::NodeId,
        keep: impl Fn(&ChromeItem) -> bool,
    ) -> Option<(Vec<ChromeItem>, Vec<super::host::TreeDecoration>)> {
        let plugins = self.wasm_plugins.as_ref()?;
        if !plugins.has_frame_slot_contents() {
            return None;
        }
        let slots: Vec<ChromeItem> = self
            .plugin_chrome("pane_frame_slots")
            .into_iter()
            .filter(|slot| keep(slot))
            .filter(|slot| self.frame_slot_applies_to(slot, node))
            .collect();
        if slots.is_empty() {
            return None;
        }
        Some((slots, plugins.frame_slot_contents_of(node)))
    }
}

/// The two pane frame placements that take a whole row of the pane (manual
/// reference-manifest.md `pane_frame_slots`): a banner above the terminal and a footer row
/// below it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FrameRowPlacement {
    Banner,
    FooterRow,
}

impl FrameRowPlacement {
    /// The `placement` value of the manifest.
    pub(crate) const fn manifest_name(self) -> &'static str {
        match self {
            Self::Banner => "banner",
            Self::FooterRow => "footer-row",
        }
    }
}

/// A frame row is at most this many characters before the frame clips it to the pane width.
const FRAME_ROW_LIMIT: usize = 200;

impl crate::app::App {
    /// Whether the pane frame slot `slot` belongs on the frame of pane `node`.
    fn frame_slot_applies_to(&self, slot: &ChromeItem, node: ilium_core::NodeId) -> bool {
        let Some(ilium_core::NodeKind::Pane {
            content,
            status,
            plugin_pane,
            ..
        }) = self.tree.get(node).map(|node| &node.kind)
        else {
            return false;
        };
        let content = match content {
            ilium_core::PaneContentKind::Terminal => "terminal",
            ilium_core::PaneContentKind::Editor => "editor",
            // A plugin pane is `pane:<global kind id>`, as in a `pane:` keybinding.
            ilium_core::PaneContentKind::Plugin => plugin_pane
                .as_ref()
                .map_or("plugin", ilium_core::PluginPaneKind::kind_id),
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

/// The text of one frame row: the contents set in `slots` (all placed on that row), in slot
/// order, sanitized and joined by a middle dot. An icon leads its slot's text.
fn frame_row_text(
    slots: &[ChromeItem],
    contents: &[super::host::TreeDecoration],
) -> Option<String> {
    let mut ordered: Vec<(usize, String)> = contents
        .iter()
        .filter_map(|content| {
            let index = slots
                .iter()
                .position(|slot| slot.plugin == content.plugin && slot.id == content.slot)?;
            let text = match content.icon.as_deref() {
                Some(icon) if !icon.trim().is_empty() => {
                    format!("{} {}", icon.trim(), content.text.trim())
                }
                _ => content.text.trim().to_owned(),
            };
            let shown = sanitize_plugin_text(text.trim(), FRAME_ROW_LIMIT);
            (!shown.trim().is_empty()).then_some((index, shown))
        })
        .collect();
    ordered.sort_by_key(|(index, _)| *index);
    // A slot set both ways (a frame slot and a decoration) shows once.
    ordered.dedup_by_key(|(index, _)| *index);
    let texts: Vec<String> = ordered.into_iter().map(|(_, shown)| shown).collect();
    (!texts.is_empty()).then(|| texts.join(" · "))
}

/// Closes every plugin view the right-hand area no longer shows, so a plugin is not asked to
/// render something nobody sees. Called once per frame; it only reads the list of views that
/// were placed, so a view opened a moment ago is never closed before its first frame. The
/// views of the plugin panes on screen (gap G14b) stay open with the right-hand view.
pub(crate) fn release_hidden(app: &App) {
    let Some(plugins) = app.wasm_plugins.as_ref() else {
        return;
    };
    let shown = app.shown_plugin_view();
    let panes = app.displayed_plugin_pane_views();
    for (plugin, target) in plugins.placed_views() {
        let is_shown = shown.as_ref().is_some_and(|(shown_plugin, shown_view)| {
            *shown_plugin == plugin && shown_view == &target
        }) || panes
            .iter()
            .any(|(pane_plugin, pane_view)| *pane_plugin == plugin && pane_view == &target);
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
    // The wizard draws an onboarding step in its own frame (`onboarding_steps::draw`); drawing
    // it here too would report two sizes for one view.
    if matches!(target, ViewTarget::OnboardingStep(_))
        && app.plugin_onboarding_step_area().is_some()
    {
        return;
    }
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
            when: None,
            caption: None,
            dynamic: false,
            menu_item: None,
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
    fn a_toolbar_caption_is_sanitized_and_cut_to_twelve_cells() {
        let with_caption = |caption: &str| ChromeItem {
            caption: Some(caption.to_owned()),
            ..toolbar_item(Some("✚"), "New note")
        };
        assert_eq!(
            toolbar_button(&with_caption("Compact")).caption.as_deref(),
            Some("Compact")
        );
        // Wide characters count two cells each: six of them fill twelve.
        assert_eq!(
            toolbar_button(&with_caption("漢字漢字漢字漢字"))
                .caption
                .as_deref(),
            Some("漢字漢字漢字")
        );
        assert_eq!(toolbar_button(&with_caption("\u{1b}[31m  ")).caption, None);
        assert_eq!(toolbar_button(&toolbar_item(None, "Go")).caption, None);
        assert_eq!(toolbar_button(&with_caption("Compact")).caption_cells(), 7);
    }

    #[test]
    fn a_hidden_toolbar_entry_leaves_no_gap_in_the_shown_list() {
        // `shown_plugin_toolbar_items` filters with `when::shows`; the shown list is what
        // `TreeToolbarAction::Plugin(index)` indexes for both drawing and clicks.
        let context = super::super::when::WhenContext {
            pane_primitive: Some("none".to_owned()),
            node_kind: Some("group".to_owned()),
            ..super::super::when::WhenContext::default()
        };
        let items = [
            ChromeItem {
                id: "agents-only".to_owned(),
                when: Some(r#"pane.primitive == "agent""#.to_owned()),
                ..toolbar_item(None, "Agents only")
            },
            ChromeItem {
                id: "groups".to_owned(),
                when: Some(r#"node.kind == "group""#.to_owned()),
                ..toolbar_item(None, "Groups")
            },
            toolbar_item(None, "Always"),
        ];
        let shown: Vec<&str> = items
            .iter()
            .filter(|item| super::super::when::shows(item.when.as_deref(), &context))
            .map(|item| item.title.as_str())
            .collect();
        assert_eq!(shown, ["Groups", "Always"]);
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
                command: None,
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
    fn a_frame_row_joins_its_slots_in_slot_order_and_skips_empty_content() {
        let slot = |id: &str| ChromeItem {
            id: id.to_owned(),
            placement: Some(FrameRowPlacement::FooterRow.manifest_name().to_owned()),
            ..toolbar_item(None, id)
        };
        let slots = [slot("recover"), slot("hint")];
        let content =
            |slot: &str, text: &str, icon: Option<&str>| super::super::host::TreeDecoration {
                plugin: "ilium.notes".to_owned(),
                node: ilium_plugin_api::generated::types::NodeId(4),
                slot: slot.to_owned(),
                text: text.to_owned(),
                icon: icon.map(str::to_owned),
                sort_key: 0,
                command: None,
            };
        let contents = [
            content("hint", "press r", None),
            content("recover", "agent stopped", Some("⚠")),
            content("unrelated", "never shown", None),
        ];
        assert_eq!(
            frame_row_text(&slots, &contents).as_deref(),
            Some("⚠ agent stopped · press r")
        );
        // Escape sequences never reach the frame, and blank content takes no row.
        assert_eq!(
            frame_row_text(&slots, &[content("hint", "  \u{1b}[2J ", None)]),
            None
        );
        assert_eq!(frame_row_text(&slots, &[]), None);
        assert_eq!(FrameRowPlacement::Banner.manifest_name(), "banner");
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
