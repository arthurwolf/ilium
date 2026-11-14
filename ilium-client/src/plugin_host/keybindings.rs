//! Key bindings that plugins contribute in their manifests (reference-manifest.md
//! "Contributions", kind `keybindings`, and "Keybinding chords"), and the key grabs of the
//! `keys` import (api-keys.md), on the client's live key path (gap G1).
//!
//! A binding's `chord` follows the grammar `*(modifier "+") key` ([`parse_chord`]); its `mode`
//! is `global` (the default), `prefix` (the key after the leader) or `pane:<kind>` (while a
//! pane of that content kind has focus). The precedence of the manual holds: the user's remap
//! and the kernel's own keys are looked up first by the caller, then plugins in catalogue
//! order, the first plugin winning a clash. A binding whose chord or mode does not parse is
//! skipped.
//!
//! In normal mode, after the kernel's leader keys, a pressed key goes first to the plugin key
//! grab that takes it (the plugin hears `key-pressed` and the pane never sees the key), then to
//! a `pane:<kind>` binding of the focused pane, then to a `global` binding; otherwise it
//! reaches the pane as before.

use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ilium_plugin_api::generated::types::CommandOrigin;
pub(crate) use ilium_plugin_host::imports::keys::parse_chord;
use ilium_plugin_host::imports::keys::{Chord, ChordKey};
use serde_json::Value;

use crate::app::{App, FocusTarget};

/// When a contributed binding applies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BindingMode {
    /// In normal mode, whatever has focus.
    Global,
    /// The key pressed after the leader.
    Prefix,
    /// While a pane of this content kind has focus.
    Pane(String),
}

impl BindingMode {
    fn parse(text: Option<&str>) -> Option<Self> {
        match text {
            None | Some("global") => Some(Self::Global),
            Some("prefix") => Some(Self::Prefix),
            Some(other) => other
                .strip_prefix("pane:")
                .filter(|kind| !kind.is_empty())
                .map(|kind| Self::Pane(kind.to_owned())),
        }
    }
}

/// One contributed binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PluginBinding {
    pub(crate) mode: BindingMode,
    pub(crate) chord: Chord,
    /// The global command id, `<plugin>/<command>`.
    pub(crate) command: String,
}

/// The bindings of one manifest, in manifest order. A malformed manifest or entry
/// contributes nothing.
pub(crate) fn plugin_bindings(plugin: &str, manifest: &str) -> Vec<PluginBinding> {
    let Ok(manifest) = serde_json::from_str::<Value>(manifest) else {
        return Vec::new();
    };
    let entries = manifest
        .get("contributes")
        .and_then(|contributes| contributes.get("keybindings"))
        .and_then(Value::as_array);
    let mut bindings = Vec::new();
    for entry in entries.into_iter().flatten() {
        let text = |name: &str| entry.get(name).and_then(Value::as_str);
        let (Some(mode), Some(command), Some(chord)) = (
            BindingMode::parse(text("mode")),
            text("command"),
            text("chord").and_then(parse_chord),
        ) else {
            continue;
        };
        bindings.push(PluginBinding {
            mode,
            chord,
            command: format!("{plugin}/{command}"),
        });
    }
    bindings
}

/// The bindings of every enabled plugin, in catalogue order, built once per enabled set.
#[derive(Debug, Default)]
pub(crate) struct BindingCache {
    built: Option<(Vec<String>, Arc<[PluginBinding]>)>,
}

impl BindingCache {
    /// The bindings of `manifests` (`(plugin, manifest)` pairs), rebuilt when the set of
    /// plugin ids changes.
    pub(crate) fn bindings<'a>(
        &mut self,
        manifests: impl Iterator<Item = (&'a str, &'a str)> + Clone,
    ) -> Arc<[PluginBinding]> {
        let ids: Vec<String> = manifests.clone().map(|(id, _)| id.to_owned()).collect();
        match &self.built {
            Some((built_for, bindings)) if *built_for == ids => Arc::clone(bindings),
            _ => {
                let bindings: Arc<[PluginBinding]> = manifests
                    .flat_map(|(id, manifest)| plugin_bindings(id, manifest))
                    .collect::<Vec<_>>()
                    .into();
                self.built = Some((ids, Arc::clone(&bindings)));
                bindings
            }
        }
    }
}

/// The chord of a pressed key, or `None` for a key the grammar cannot name. An upper case
/// letter reads as `shift` plus the lower case letter; for other characters the shift is
/// already in the character and is dropped.
pub(crate) fn chord_of_key(key: &KeyEvent) -> Option<Chord> {
    let modifiers = key.modifiers;
    let mut shift = modifiers.contains(KeyModifiers::SHIFT);
    let key_of = match key.code {
        KeyCode::Char(character) => {
            if character.is_uppercase() {
                shift = true;
                ChordKey::Character(character.to_lowercase().next()?)
            } else if character.is_alphabetic() {
                ChordKey::Character(character)
            } else {
                shift = false;
                // A control character the terminal did not decode is not a chord.
                if character.is_control() || character.is_whitespace() {
                    return None;
                }
                ChordKey::Character(character)
            }
        }
        KeyCode::Enter => ChordKey::Enter,
        KeyCode::Esc => ChordKey::Escape,
        KeyCode::Tab => ChordKey::Tab,
        KeyCode::BackTab => {
            shift = true;
            ChordKey::Tab
        }
        KeyCode::Backspace => ChordKey::Backspace,
        KeyCode::Delete => ChordKey::Delete,
        KeyCode::Insert => ChordKey::Insert,
        KeyCode::Home => ChordKey::Home,
        KeyCode::End => ChordKey::End,
        KeyCode::PageUp => ChordKey::PageUp,
        KeyCode::PageDown => ChordKey::PageDown,
        KeyCode::Up => ChordKey::Up,
        KeyCode::Down => ChordKey::Down,
        KeyCode::Left => ChordKey::Left,
        KeyCode::Right => ChordKey::Right,
        KeyCode::F(number) if (1..=24).contains(&number) => ChordKey::Function(number),
        _ => return None,
    };
    Some(Chord {
        ctrl: modifiers.contains(KeyModifiers::CONTROL),
        alt: modifiers.contains(KeyModifiers::ALT),
        shift,
        super_key: modifiers.contains(KeyModifiers::SUPER),
        key: key_of,
    })
}

/// The first binding of `bindings` for `chord` that `accepts` allows.
pub(crate) fn find_binding<'a>(
    bindings: &'a [PluginBinding],
    chord: &Chord,
    accepts: impl Fn(&BindingMode) -> bool,
) -> Option<&'a PluginBinding> {
    bindings
        .iter()
        .find(|binding| binding.chord == *chord && accepts(&binding.mode))
}

impl App {
    /// Runs the plugin command bound to the key pressed after the leader, if an enabled plugin
    /// binds it with mode `prefix`. The caller has already found that no kernel or user binding
    /// holds the key, which is the precedence the manual gives. Returns whether a command ran.
    pub(crate) fn run_plugin_prefix_binding(&mut self, key: &KeyEvent) -> bool {
        let Some(chord) = chord_of_key(key) else {
            return false;
        };
        self.run_plugin_binding(&chord, |mode| *mode == BindingMode::Prefix)
    }

    /// Offers a normal-mode key to the plugins (gap G1): a key grab first, then a
    /// `pane:<kind>` binding of the focused pane, then a `global` binding. Returns whether a
    /// plugin took the key; the caller then keeps it from the pane.
    pub(crate) fn route_plugin_key(&mut self, key: &KeyEvent) -> bool {
        if self.wasm_plugins.is_none() {
            return false;
        }
        let Some(chord) = chord_of_key(key) else {
            return false;
        };
        let focused = self.focused_pane_id();
        let repeat = key.kind == KeyEventKind::Repeat;
        if self
            .wasm_plugins
            .as_ref()
            .is_some_and(|plugins| plugins.route_grab(&chord, focused, repeat))
        {
            return true;
        }
        let pane_kind = (self.focus == FocusTarget::Pane)
            .then(|| focused.and_then(|pane| self.pane_content_kind_name(pane)))
            .flatten();
        if let Some(kind) = pane_kind {
            if self.run_plugin_binding(
                &chord,
                |mode| matches!(mode, BindingMode::Pane(wanted) if *wanted == kind),
            ) {
                return true;
            }
        }
        self.run_plugin_binding(&chord, |mode| *mode == BindingMode::Global)
    }

    /// The content kind a `pane:<kind>` binding names for a pane. A plugin pane is named by
    /// its global pane kind id, such as `ilium.core.kanban.board`.
    fn pane_content_kind_name(&self, pane: ilium_core::NodeId) -> Option<String> {
        match &self.tree.get(pane)?.kind {
            ilium_core::NodeKind::Pane {
                content,
                plugin_pane,
                ..
            } => Some(match content {
                ilium_core::PaneContentKind::Terminal => "terminal".to_owned(),
                ilium_core::PaneContentKind::Editor => "editor".to_owned(),
                ilium_core::PaneContentKind::Board => "board".to_owned(),
                ilium_core::PaneContentKind::Plugin => plugin_pane.as_ref()?.kind_id().to_owned(),
            }),
            _ => None,
        }
    }

    fn run_plugin_binding(
        &mut self,
        chord: &Chord,
        accepts: impl Fn(&BindingMode) -> bool,
    ) -> bool {
        let command = self
            .wasm_plugins
            .as_ref()
            .and_then(|plugins| plugins.binding_command(chord, accepts));
        let Some(command) = command else {
            return false;
        };
        self.invoke_plugin_command(&command, "{}".to_owned(), CommandOrigin::Keybinding);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = r#"{
        "contributes": {
            "keybindings": [
                { "command": "new-agent", "chord": "w", "mode": "prefix" },
                { "command": "find", "chord": "ctrl+f", "mode": "global" },
                { "command": "top", "chord": "page-up", "mode": "prefix" },
                { "command": "held", "chord": "ctrl+w", "mode": "prefix" },
                { "command": "bare", "chord": "x" },
                { "command": "fold", "chord": "alt+z", "mode": "pane:editor" },
                { "command": "bad", "chord": "Ctrl+Q", "mode": "global" },
                { "command": "odd", "chord": "q", "mode": "pane:" },
                { "chord": "y", "mode": "prefix" }
            ]
        }
    }"#;

    fn chord(text: &str) -> Chord {
        parse_chord(text).unwrap_or_else(|| panic!("{text} is a chord"))
    }

    #[test]
    fn bindings_of_every_mode_and_modifier_are_read_in_manifest_order() {
        let bindings = plugin_bindings("acme", MANIFEST);
        let read: Vec<(BindingMode, String, &str)> = bindings
            .iter()
            .map(|binding| {
                (
                    binding.mode.clone(),
                    binding.chord.canonical(),
                    binding.command.as_str(),
                )
            })
            .collect();
        assert_eq!(
            read,
            vec![
                (BindingMode::Prefix, "w".to_owned(), "acme/new-agent"),
                (BindingMode::Global, "ctrl+f".to_owned(), "acme/find"),
                (BindingMode::Prefix, "page-up".to_owned(), "acme/top"),
                (BindingMode::Prefix, "ctrl+w".to_owned(), "acme/held"),
                (BindingMode::Global, "x".to_owned(), "acme/bare"),
                (
                    BindingMode::Pane("editor".to_owned()),
                    "alt+z".to_owned(),
                    "acme/fold"
                ),
            ]
        );
    }

    #[test]
    fn a_manifest_without_keybindings_or_one_that_does_not_parse_binds_nothing() {
        assert!(plugin_bindings("p", "{}").is_empty());
        assert!(plugin_bindings("p", r#"{"contributes":{"keybindings":"w"}}"#).is_empty());
        assert!(plugin_bindings("p", "not json").is_empty());
    }

    #[test]
    fn pressed_keys_read_as_chords() {
        let press = |code, modifiers| KeyEvent::new(code, modifiers);
        assert_eq!(
            chord_of_key(&press(KeyCode::Char('W'), KeyModifiers::SHIFT)),
            Some(chord("shift+w"))
        );
        assert_eq!(
            chord_of_key(&press(KeyCode::Char('w'), KeyModifiers::CONTROL)),
            Some(chord("ctrl+w"))
        );
        assert_eq!(
            chord_of_key(&press(KeyCode::Char('+'), KeyModifiers::SHIFT)),
            Some(chord("plus"))
        );
        assert_eq!(
            chord_of_key(&press(KeyCode::BackTab, KeyModifiers::SHIFT)),
            Some(chord("shift+tab"))
        );
        assert_eq!(
            chord_of_key(&press(KeyCode::F(5), KeyModifiers::ALT)),
            Some(chord("alt+f5"))
        );
        assert_eq!(
            chord_of_key(&press(KeyCode::Null, KeyModifiers::NONE)),
            None
        );
    }

    #[test]
    fn the_first_binding_of_the_wanted_mode_wins() {
        let mut bindings = plugin_bindings("first", MANIFEST);
        bindings.extend(plugin_bindings("second", MANIFEST));
        let found = find_binding(&bindings, &chord("ctrl+w"), |mode| {
            *mode == BindingMode::Prefix
        });
        assert_eq!(
            found.map(|binding| binding.command.as_str()),
            Some("first/held")
        );
        assert!(find_binding(&bindings, &chord("ctrl+w"), |mode| *mode
            == BindingMode::Global)
        .is_none());
    }

    #[test]
    fn the_cache_rebuilds_only_when_the_plugin_set_changes() {
        let mut cache = BindingCache::default();
        let one = [("acme", MANIFEST)];
        let first = cache.bindings(one.iter().copied());
        let again = cache.bindings(one.iter().copied());
        assert!(Arc::ptr_eq(&first, &again));
        let none: [(&str, &str); 0] = [];
        assert!(cache.bindings(none.iter().copied()).is_empty());
    }

    #[test]
    fn a_key_no_plugin_binds_runs_nothing_without_a_plugin_host() {
        let mut app = App::new("test".to_string(), std::env::temp_dir());
        let key = KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE);
        assert!(!app.run_plugin_prefix_binding(&key));
        assert!(!app.route_plugin_key(&key));
        assert_eq!(app.status_message, None);
    }
}
