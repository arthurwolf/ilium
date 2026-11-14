//! The Settings navigation rail is data: the built-in tabs, then one entry per settings page
//! that an enabled plugin contributes (`contributes.settings`). A plugin page is the Plugins
//! tab narrowed to that page's rows, so it reuses the Plugins tab's row drawing, keys, mouse
//! hits and editing; the rail entry only decides which rows the Plugins tab lists.
//!
//! Drawing and hit testing both read [`App::settings_rail`] and
//! [`App::settings_rail_active`], so a click always lands on the entry drawn under it.

use crate::app::{App, SettingsState, SettingsTab};

/// One entry of the Settings rail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsRailEntry {
    /// A built-in tab.
    Tab(SettingsTab),
    /// The plugin settings page at this index of [`App::settings_plugin_pages`].
    PluginPage(usize),
}

/// What the rail draws for one entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RailLabel {
    pub icon: &'static str,
    pub label: String,
    /// A shorter label for a narrow rail; empty when there is none.
    pub compact: String,
}

/// The rail icon of a plugin settings page: the Plugins tab's icon.
pub(crate) const PLUGIN_PAGE_ICON: &str = "⬡";

/// The pages of `settings`, `(plugin, page title)`, in first-seen order, without pages whose
/// title is empty or names a built-in tab: a plugin page never poses as a built-in one.
pub(crate) fn plugin_pages(
    settings: &[super::host::ContributedSetting],
    built_in_labels: &[&str],
) -> Vec<(String, String)> {
    let mut pages: Vec<(String, String)> = Vec::new();
    for setting in settings {
        let title = setting.page.trim();
        if title.is_empty()
            || built_in_labels
                .iter()
                .any(|label| label.eq_ignore_ascii_case(title))
        {
            continue;
        }
        let page = (setting.plugin.clone(), setting.page.clone());
        if !pages.contains(&page) {
            pages.push(page);
        }
    }
    pages
}

/// The rail entries: every built-in tab, then `plugin_pages` plugin pages.
pub(crate) fn rail_entries(plugin_pages: usize) -> Vec<SettingsRailEntry> {
    SettingsTab::ALL
        .into_iter()
        .map(SettingsRailEntry::Tab)
        .chain((0..plugin_pages).map(SettingsRailEntry::PluginPage))
        .collect()
}

/// The rail index of the entry in effect: the open plugin page while the Plugins tab shows
/// one, else the tab's own entry.
pub(crate) fn active_index(tab: SettingsTab, open_page: Option<usize>) -> usize {
    match (tab, open_page) {
        (SettingsTab::Plugins, Some(page)) => SettingsTab::ALL.len() + page,
        _ => SettingsTab::ALL
            .iter()
            .position(|known| *known == tab)
            .unwrap_or(0),
    }
}

impl App {
    /// The settings pages enabled plugins contribute, `(plugin, page title)`, in rail order.
    pub(crate) fn settings_plugin_pages(&self) -> Vec<(String, String)> {
        let Some(plugins) = self.wasm_plugins.as_ref() else {
            return Vec::new();
        };
        let labels: Vec<&str> = SettingsTab::ALL.iter().map(|tab| tab.label()).collect();
        plugin_pages(&plugins.contributed_settings(), &labels)
    }

    /// The index of the open plugin settings page, if one is open and still contributed.
    pub(crate) fn open_settings_plugin_page(&self) -> Option<usize> {
        let open = self.settings_plugin_page.as_ref()?;
        self.settings_plugin_pages()
            .iter()
            .position(|page| page == open)
    }

    /// The rail entries Settings draws and hit-tests.
    pub(crate) fn settings_rail(&self) -> Vec<SettingsRailEntry> {
        rail_entries(self.settings_plugin_pages().len())
    }

    /// The rail index of the entry in effect for `tab`.
    pub(crate) fn settings_rail_active(&self, tab: SettingsTab) -> usize {
        active_index(tab, self.open_settings_plugin_page())
    }

    /// What the rail shows for each entry, in rail order.
    pub(crate) fn settings_rail_labels(
        &self,
        built_in: impl Fn(SettingsTab) -> RailLabel,
    ) -> Vec<RailLabel> {
        let pages = self.settings_plugin_pages();
        self.settings_rail()
            .into_iter()
            .map(|entry| match entry {
                SettingsRailEntry::Tab(tab) => built_in(tab),
                SettingsRailEntry::PluginPage(index) => {
                    let title = pages
                        .get(index)
                        .map(|(_, page)| page.clone())
                        .unwrap_or_default();
                    // A narrow rail shows the title cut with `…`, never a bare icon that
                    // would look like the Plugins tab.
                    RailLabel {
                        icon: PLUGIN_PAGE_ICON,
                        label: sanitize_page_title(&title, PAGE_TITLE_LIMIT),
                        compact: sanitize_page_title(&title, COMPACT_PAGE_TITLE_LIMIT),
                    }
                }
            })
            .collect()
    }

    /// Opens a rail entry: a built-in tab closes any plugin page; a plugin page shows the
    /// Plugins tab narrowed to its rows. The row selection and scroll start over whenever the
    /// entry changes.
    pub(crate) fn select_settings_rail_entry(
        &mut self,
        state: &mut SettingsState,
        entry: SettingsRailEntry,
    ) {
        let before = self.settings_rail_active(state.tab);
        match entry {
            SettingsRailEntry::Tab(tab) => {
                self.settings_plugin_page = None;
                state.tab = tab;
            }
            SettingsRailEntry::PluginPage(index) => {
                self.settings_plugin_page = self.settings_plugin_pages().get(index).cloned();
                state.tab = SettingsTab::Plugins;
            }
        }
        if self.settings_rail_active(state.tab) != before {
            state.selected_row = 0;
            state.trigger_action_cursor = 0;
            state.scroll = 0;
        }
    }

    /// Tab and Shift+Tab: the next or previous rail entry, wrapping at either end.
    pub(crate) fn step_settings_rail(&mut self, state: &mut SettingsState, forward: bool) {
        let rail = self.settings_rail();
        if rail.is_empty() {
            return;
        }
        let current = self.settings_rail_active(state.tab).min(rail.len() - 1);
        let next = if forward {
            (current + 1) % rail.len()
        } else {
            (current + rail.len() - 1) % rail.len()
        };
        self.select_settings_rail_entry(state, rail[next]);
        // Stepping always starts the entry at its top, even onto the same tab.
        state.selected_row = 0;
        state.trigger_action_cursor = 0;
        state.scroll = 0;
    }
}

/// The longest page title the rail draws, in characters.
const PAGE_TITLE_LIMIT: usize = 24;
/// The page title a narrow rail draws, ending in `…` when cut.
const COMPACT_PAGE_TITLE_LIMIT: usize = 10;

/// A page title as the rail draws it: control and escape sequences removed, at most `limit`
/// characters, the last one `…` when cut.
fn sanitize_page_title(title: &str, limit: usize) -> String {
    ilium_plugin_host::render::sanitize_plugin_text(title.trim(), limit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin_host::host::ContributedSetting;

    fn setting(plugin: &str, page: &str, key: &str) -> ContributedSetting {
        ContributedSetting {
            plugin: plugin.to_owned(),
            page: page.to_owned(),
            key: key.to_owned(),
            title: key.to_owned(),
            row_type: "toggle".to_owned(),
            default: "off".to_owned(),
        }
    }

    #[test]
    fn each_contributed_page_is_one_rail_entry_after_the_built_in_tabs() {
        let settings = [
            setting("com.example.notes", "Notes", "autosave"),
            setting("com.example.notes", "Notes", "font"),
            setting("com.example.notes", "Sync", "server"),
            setting("com.example.other", "Notes", "colour"),
            // A page named like a built-in tab is not a rail entry of its own.
            setting("com.example.other", "appearance", "theme"),
            setting("com.example.other", "  ", "blank"),
        ];
        let pages = plugin_pages(&settings, &["Appearance", "Plugins"]);
        assert_eq!(
            pages,
            [
                ("com.example.notes".to_owned(), "Notes".to_owned()),
                ("com.example.notes".to_owned(), "Sync".to_owned()),
                ("com.example.other".to_owned(), "Notes".to_owned()),
            ]
        );
        let rail = rail_entries(pages.len());
        assert_eq!(rail.len(), SettingsTab::ALL.len() + 3);
        assert_eq!(rail[0], SettingsRailEntry::Tab(SettingsTab::ALL[0]));
        assert_eq!(
            rail[SettingsTab::ALL.len()],
            SettingsRailEntry::PluginPage(0)
        );
    }

    #[test]
    fn the_active_entry_is_the_open_page_only_on_the_plugins_tab() {
        let plugins_index = SettingsTab::ALL
            .iter()
            .position(|tab| *tab == SettingsTab::Plugins)
            .unwrap_or(0);
        assert_eq!(active_index(SettingsTab::Plugins, None), plugins_index);
        assert_eq!(
            active_index(SettingsTab::Plugins, Some(1)),
            SettingsTab::ALL.len() + 1
        );
        assert_eq!(active_index(SettingsTab::ALL[0], Some(1)), 0);
    }

    #[test]
    fn a_page_title_is_sanitized_for_the_rail() {
        assert_eq!(
            sanitize_page_title("\u{1b}[31mNotes ", PAGE_TITLE_LIMIT),
            "Notes"
        );
        let compact = sanitize_page_title("Synchronisation options", COMPACT_PAGE_TITLE_LIMIT);
        assert_eq!(compact.chars().count(), COMPACT_PAGE_TITLE_LIMIT);
        assert!(compact.ends_with('…'));
    }
}
