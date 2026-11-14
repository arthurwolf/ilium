//! `App` side of the client effects of plugin intents (gap G3, `plugin_host::effects`).
//!
//! A child module of `app`, so it can reach the private external-open queue. Each effect maps
//! onto an existing kernel action: focus goes through `focus_pane`/`select_node`, attention
//! marks through the attention queue's plugin layer, external opens through the bounded
//! external-open service, and Settings pages through the ordinary Settings screen.

use ilium_core::{NodeId, NodeKind};
use ilium_plugin_api::generated::types::intents::EditorPlacement;

use super::{App, Mode, SettingsTab};
use crate::attention::{AttentionItem, AttentionKind, AttentionSource, Briefing};
use crate::plugin_host::effects::ClientEffect;

/// Priority of a plugin's attention mark: above "finished", below questions and blocked goals.
const PLUGIN_ATTENTION_PRIORITY: u8 = 50;

/// The Settings tab a plugin names: its label, case-insensitively, with `-` for spaces.
fn settings_tab_named(page: &str) -> Option<SettingsTab> {
    let wanted = page.trim().replace(['-', '_'], " ").to_ascii_lowercase();
    SettingsTab::ALL
        .into_iter()
        .find(|tab| tab.label().to_ascii_lowercase() == wanted)
}

impl App {
    /// Carries out one client effect a plugin asked for. Returns whether to redraw.
    pub(crate) fn apply_plugin_effect(&mut self, plugin: &str, effect: ClientEffect) -> bool {
        match effect {
            ClientEffect::FocusNode(node) => self.plugin_focus_node(plugin, node),
            ClientEffect::MarkAttention { node, reason } => {
                self.plugin_mark_attention(plugin, node, reason)
            }
            ClientEffect::AckAttention(node) => self.attention.queue.ack_from_plugin(node),
            ClientEffect::OpenExternal(target) => {
                self.queue_external_open(target, None);
                true
            }
            ClientEffect::EditorOpen {
                path,
                line,
                placement,
            } => self.plugin_open_editor(plugin, path, line, placement),
            ClientEffect::OpenSettings(page) => {
                if !matches!(self.mode, Mode::Normal) {
                    // Another screen has the user: a plugin never replaces it.
                    return false;
                }
                self.action_open_settings();
                if let (Some(tab), Mode::Settings(state)) =
                    (settings_tab_named(&page), &mut self.mode)
                {
                    state.tab = tab;
                }
                true
            }
            ClientEffect::DismissToast(id) => {
                let is_shown = self
                    .plugin_toast
                    .as_ref()
                    .is_some_and(|(owner, shown)| owner == plugin && *shown == id);
                if is_shown {
                    self.plugin_toast = None;
                    self.status_message = None;
                }
                is_shown
            }
        }
    }

    /// Opens a file a plugin named in a new editor pane. `split` opens it beside the focused
    /// pane, in the same group; `new-pane` and `replace` open it where a new pane of the user
    /// would go (the kernel has no in-place replacement of a pane).
    fn plugin_open_editor(
        &mut self,
        plugin: &str,
        path: std::path::PathBuf,
        line: Option<u32>,
        placement: EditorPlacement,
    ) -> bool {
        let beside_focused = match placement {
            EditorPlacement::Split => self
                .focused_pane_id()
                .and_then(|pane| self.tree.parent_of(pane)),
            EditorPlacement::NewPane | EditorPlacement::Replace => None,
        };
        let parent_group = match beside_focused {
            Some(group) => group,
            None => self.group_for_new_node(),
        };
        if !self.request_new_editor_at(parent_group, path.clone(), line, None) {
            self.status_message = Some(format!("{plugin}: {} could not open", path.display()));
            return true;
        }
        self.status_message = Some(match line {
            Some(line) => format!("Opening {} at line {line}", path.display()),
            None => format!("Opening {}", path.display()),
        });
        true
    }

    fn plugin_focus_node(&mut self, plugin: &str, node: NodeId) -> bool {
        let Some(found) = self.tree.get(node) else {
            self.status_message = Some(format!("{plugin}: node {} is gone", node.0));
            return true;
        };
        if matches!(found.kind, NodeKind::Pane { .. }) {
            self.focus_pane(node);
        } else {
            self.select_node(node);
        }
        true
    }

    fn plugin_mark_attention(&mut self, plugin: &str, node: NodeId, reason: String) -> bool {
        let Some(found) = self.tree.get(node) else {
            return false;
        };
        let title = found.name.clone();
        let now_ms = crate::scheduled_input::unix_millis_now();
        let item = AttentionItem {
            pane_id: node,
            project_id: self.tree.project_ancestor(node),
            title,
            kind: AttentionKind::Changed,
            reason: format!("{plugin}: {reason}"),
            question: None,
            waiting_on: None,
            briefing: Briefing::default(),
            priority: PLUGIN_ATTENTION_PRIORITY,
            source: AttentionSource::Plugin,
            fingerprint: crate::attention::fingerprint_parts(&[
                plugin.as_bytes(),
                reason.as_bytes(),
            ]),
            generation: self.attention.queue.generation(),
            created_ms: now_ms,
        };
        self.attention.queue.mark_from_plugin(item);
        true
    }

    /// Lets the plugin host report focus and `config.toml` changes (gap G2); every tick. A
    /// plugin's write to the `animations` kernel tab is applied to the live animation here.
    pub(crate) fn tick_wasm_plugin_feed(&mut self) {
        let focused = self.focused_pane_id();
        let animations = self
            .wasm_plugins
            .as_mut()
            .and_then(|plugins| plugins.observe(focused));
        if let Some(table) = animations {
            self.apply_plugin_animation_table(&table);
        }
    }

    /// Applies the `[animations]` table a plugin wrote through `kernel-settings` (the ambient
    /// plugin's `set-setting` service): the scene it names, or the semantic animation. Goes
    /// through the one save path of the Animations tab, so a refused save reports the same
    /// status message and changes nothing.
    pub(crate) fn apply_plugin_animation_table(&mut self, table: &toml::Table) {
        let Some(kind) = crate::plugin_host::animation_table::requested_kind(
            table,
            self.animation_settings.kind,
        ) else {
            return;
        };
        self.settings_select_animation_scene(kind);
    }

    /// Drops plugin attention marks whose node left the tree.
    pub(crate) fn prune_plugin_attention(&mut self) {
        let tree = &self.tree;
        self.attention
            .queue
            .retain_plugin_marks(|node| tree.get(node).is_some());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_pages_match_tab_labels() {
        let first = SettingsTab::ALL[0];
        let slug = first.label().to_ascii_lowercase().replace(' ', "-");
        assert_eq!(settings_tab_named(&slug), Some(first));
        assert_eq!(settings_tab_named("no such page"), None);
    }
}
