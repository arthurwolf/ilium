//! Key bindings that plugins contribute in their manifests (reference-manifest.md
//! "Contributions", kind `keybindings`).
//!
//! Only `prefix` bindings without modifiers are honoured so far: the key pressed after the
//! leader runs the plugin's command when no kernel or user binding holds that key. `global`
//! and `pane:<kind>` bindings, chords with modifiers, and the list of losing bindings in
//! Settings > Keys are not consumed yet.

use ilium_plugin_api::generated::types::CommandOrigin;
use serde_json::Value;

use crate::app::App;
use crate::keymap::BindingKey;

/// One contributed `prefix` binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PrefixBinding {
    pub(crate) key: BindingKey,
    /// The global command id, `<plugin>/<command>`.
    pub(crate) command: String,
}

/// The post-leader key a manifest chord names; `None` for a chord with modifiers or a key
/// the leader table cannot hold.
fn chord_key(chord: &str) -> Option<BindingKey> {
    match chord {
        "up" => Some(BindingKey::Up),
        "down" => Some(BindingKey::Down),
        "left" => Some(BindingKey::Left),
        "right" => Some(BindingKey::Right),
        "page-up" => Some(BindingKey::PageUp),
        "page-down" => Some(BindingKey::PageDown),
        "plus" => Some(BindingKey::Character('+')),
        other => {
            let mut characters = other.chars();
            let character = characters.next()?;
            (characters.next().is_none() && !character.is_control())
                .then_some(BindingKey::Character(character))
        }
    }
}

/// The `prefix` bindings of one manifest, in manifest order. A malformed manifest or entry
/// contributes nothing.
pub(crate) fn prefix_bindings(plugin: &str, manifest: &str) -> Vec<PrefixBinding> {
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
        if text("mode") != Some("prefix") {
            continue;
        }
        let (Some(command), Some(key)) = (text("command"), text("chord").and_then(chord_key))
        else {
            continue;
        };
        bindings.push(PrefixBinding {
            key,
            command: format!("{plugin}/{command}"),
        });
    }
    bindings
}

impl App {
    /// Runs the plugin command bound to a post-leader key, if an enabled plugin binds it.
    /// The caller has already found that no kernel or user binding holds the key, which is
    /// the precedence the manual gives. Returns whether a command was run.
    pub(crate) fn run_plugin_prefix_binding(&mut self, key: BindingKey) -> bool {
        let command = self
            .wasm_plugins
            .as_ref()
            .and_then(|plugins| plugins.prefix_binding_command(key));
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
                { "chord": "y", "mode": "prefix" }
            ]
        }
    }"#;

    #[test]
    fn prefix_bindings_without_modifiers_are_read_in_manifest_order() {
        assert_eq!(
            prefix_bindings("ilium.core.worktrees", MANIFEST),
            vec![
                PrefixBinding {
                    key: BindingKey::Character('w'),
                    command: "ilium.core.worktrees/new-agent".to_owned(),
                },
                PrefixBinding {
                    key: BindingKey::PageUp,
                    command: "ilium.core.worktrees/top".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn a_manifest_without_keybindings_or_one_that_does_not_parse_binds_nothing() {
        assert!(prefix_bindings("p", "{}").is_empty());
        assert!(prefix_bindings("p", r#"{"contributes":{"keybindings":"w"}}"#).is_empty());
        assert!(prefix_bindings("p", "not json").is_empty());
    }

    #[test]
    fn a_key_no_plugin_binds_runs_nothing_without_a_plugin_host() {
        let mut app = App::new("test".to_string(), std::env::temp_dir());
        assert!(!app.run_plugin_prefix_binding(BindingKey::Character('w')));
        assert_eq!(app.status_message, None);
    }
}
