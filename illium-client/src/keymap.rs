//! Single source of truth for illium's leader-key bindings.
//!
//! illium's prefix key is `Ctrl+A` (like screen's classic prefix),
//! followed by a single letter. `Ctrl+:` is also recognized as an
//! alternate leader for terminals that can disambiguate it (see
//! `is_leader_key`), but `Ctrl+A` is the documented default since it needs
//! no special terminal protocol support — it works identically in a plain
//! xterm, VS Code's integrated terminal, tmux, everywhere. Both the
//! input-dispatch logic (`app.rs`) and the help screen render from
//! [`effective_bindings`] (defaulting to [`LEADER_BINDINGS`] until
//! [`init_effective_bindings`] runs at startup), so the two can't drift out
//! of sync — add a new leader shortcut to `LEADER_BINDINGS` and both
//! consumers pick it up automatically.
//!
//! A user may remap which letter triggers an existing [`Action`] via
//! `config.toml`'s `[keybindings]` table (`crate::config`) — see
//! [`action_name`]/[`action_from_name`] for the stable string each
//! `Action` is addressed by in that config, and [`init_effective_bindings`]
//! for how the override table replaces [`LEADER_BINDINGS`] as the table
//! `action_for` searches. Deliberately scoped to *remapping* only: `config.toml`
//! cannot define a brand-new action, only change which letter dispatches
//! one of the actions already listed here.

use std::sync::OnceLock;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// One leader-key action. Extend this enum (and `LEADER_BINDINGS` below)
/// together when adding a new shortcut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    NewTerminal,
    NewEditor,
    ClosePane,
    NewGroup,
    Rename,
    ToggleMove,
    FocusTree,
    FocusPane,
    Save,
    RunCommand,
    Help,
    Quit,
    ToggleEditorViewMode,
    ToggleLineNumbers,
    ToggleMinimap,
    ToggleAutosave,
}

/// A single letter -> action mapping, plus the human-readable description
/// shown on the help screen.
#[derive(Debug, Clone, Copy)]
pub struct KeyBinding {
    pub letter: char,
    pub action: Action,
    pub description: &'static str,
}

/// All leader-key bindings, in display order. `action_for` and the help
/// screen both walk this table, so it is the only place a binding needs to
/// be registered.
pub const LEADER_BINDINGS: &[KeyBinding] = &[
    KeyBinding {
        letter: 'c',
        action: Action::NewTerminal,
        description: "New terminal pane in the selected group",
    },
    KeyBinding {
        letter: 'e',
        action: Action::NewEditor,
        description: "New editor pane (opens a file picker)",
    },
    KeyBinding {
        letter: 'x',
        action: Action::ClosePane,
        description: "Close the selected pane or group",
    },
    KeyBinding {
        letter: 'g',
        action: Action::NewGroup,
        description: "New group (choose where in a dialog)",
    },
    KeyBinding {
        letter: 'r',
        action: Action::Rename,
        description: "Rename the selected node",
    },
    KeyBinding {
        letter: 'm',
        action: Action::ToggleMove,
        description: "Toggle move mode for the selected node (up/down to reorder, left/right to outdent/indent)",
    },
    KeyBinding {
        letter: 't',
        action: Action::FocusTree,
        description: "Focus the tree panel",
    },
    KeyBinding {
        letter: 'p',
        action: Action::FocusPane,
        description: "Focus the active pane",
    },
    KeyBinding {
        letter: 's',
        action: Action::Save,
        description: "Save the focused editor pane",
    },
    KeyBinding {
        letter: '!',
        action: Action::RunCommand,
        description: "Prompt for a command, run it in a new terminal pane in the selected group",
    },
    KeyBinding {
        letter: 'v',
        action: Action::ToggleEditorViewMode,
        description:
            "Toggle the focused editor pane between Source and Rendered (markdown files only)",
    },
    KeyBinding {
        letter: 'n',
        action: Action::ToggleLineNumbers,
        description: "Toggle line numbers in the focused editor pane",
    },
    KeyBinding {
        letter: 'b',
        action: Action::ToggleMinimap,
        description: "Toggle the minimap in the focused editor pane",
    },
    KeyBinding {
        letter: 'a',
        action: Action::ToggleAutosave,
        description: "Toggle autosave (debounced ~1s after each edit) in the focused editor pane",
    },
    KeyBinding {
        letter: '?',
        action: Action::Help,
        description: "Show or hide this help screen",
    },
    KeyBinding {
        letter: 'q',
        action: Action::Quit,
        description: "Quit illium",
    },
];

/// The stable, `snake_case` config-file name for each [`Action`] --
/// `config.toml`'s `[keybindings]` table keys are these names, e.g.
/// `new_terminal = "c"`. Kept as one explicit match (rather than deriving
/// a name from `Debug`) so renaming an `Action` variant for readability
/// doesn't silently rename what users type in their config file.
pub fn action_name(action: Action) -> &'static str {
    match action {
        Action::NewTerminal => "new_terminal",
        Action::NewEditor => "new_editor",
        Action::ClosePane => "close_pane",
        Action::NewGroup => "new_group",
        Action::Rename => "rename",
        Action::ToggleMove => "toggle_move",
        Action::FocusTree => "focus_tree",
        Action::FocusPane => "focus_pane",
        Action::Save => "save",
        Action::RunCommand => "run_command",
        Action::Help => "help",
        Action::Quit => "quit",
        Action::ToggleEditorViewMode => "toggle_editor_view_mode",
        Action::ToggleLineNumbers => "toggle_line_numbers",
        Action::ToggleMinimap => "toggle_minimap",
        Action::ToggleAutosave => "toggle_autosave",
    }
}

/// The inverse of [`action_name`]: looks up the `Action` a config-file
/// `[keybindings]` key refers to. `None` for any name that isn't exactly
/// one of [`action_name`]'s outputs -- `crate::config`'s loader turns that
/// into a clear "unknown action" error rather than silently ignoring a
/// typo.
pub fn action_from_name(name: &str) -> Option<Action> {
    LEADER_BINDINGS
        .iter()
        .map(|binding| binding.action)
        .find(|&action| action_name(action) == name)
}

/// The table [`action_for`] and the help screen actually search: either
/// the startup-computed override table (see [`init_effective_bindings`])
/// or, absent that (no config file, or a test that never calls it),
/// [`LEADER_BINDINGS`] unchanged.
static EFFECTIVE_BINDINGS: OnceLock<Vec<KeyBinding>> = OnceLock::new();

/// Installs the effective leader-key binding table for the rest of the
/// process's lifetime -- called once at client startup (`crate::run`)
/// after merging `config.toml`'s `[keybindings]` overrides onto
/// [`LEADER_BINDINGS`] (see `crate::config::load`). A second call is a
/// no-op: there is no "reload config" request yet, so nothing should ever
/// attempt one.
pub fn init_effective_bindings(bindings: Vec<KeyBinding>) {
    let _ = EFFECTIVE_BINDINGS.set(bindings);
}

/// The binding table currently in effect -- what [`action_for`] and the
/// help screen (`crate::help`) search.
pub fn effective_bindings() -> &'static [KeyBinding] {
    EFFECTIVE_BINDINGS
        .get()
        .map(Vec::as_slice)
        .unwrap_or(LEADER_BINDINGS)
}

/// True if this key event is the leader key itself: `Ctrl+A` (the
/// documented default — a classic Ctrl+letter combo, encoded identically
/// as a single control byte on every terminal, no protocol negotiation
/// needed), or `Ctrl+:` for terminals that can disambiguate a shifted
/// symbol from Ctrl (requires the Kitty keyboard protocol or xterm's
/// `modifyOtherKeys`; most terminals, including VS Code's integrated
/// terminal, don't disambiguate this without extra configuration, which is
/// why `Ctrl+A` is the default rather than `Ctrl+:` alone).
///
/// Terminals disagree on how they report `Ctrl` + a shifted symbol like
/// `:`; crossterm may see it as `Char(':')` with `CONTROL` set, or (more
/// commonly, since `:` is shift+`;` on a US layout) as `Char(';')` with
/// both `CONTROL` and `SHIFT` set. Both patterns are matched.
pub fn is_leader_key(key: &KeyEvent) -> bool {
    match key.code {
        // The documented default: Ctrl+A, a plain control byte.
        KeyCode::Char('a' | 'A') => key.modifiers.contains(KeyModifiers::CONTROL),
        // Direct report of the shifted symbol with just Ctrl held.
        KeyCode::Char(':') => key.modifiers.contains(KeyModifiers::CONTROL),
        // Unshifted base char reported alongside both Ctrl and Shift.
        KeyCode::Char(';') => {
            key.modifiers.contains(KeyModifiers::CONTROL)
                && key.modifiers.contains(KeyModifiers::SHIFT)
        }
        _ => false,
    }
}

/// Looks up the action bound to a letter pressed right after the leader,
/// searching [`effective_bindings`] (the possibly-user-remapped table)
/// rather than [`LEADER_BINDINGS`] directly.
pub fn action_for(letter: char) -> Option<Action> {
    action_for_table(effective_bindings(), letter)
}

/// The lookup [`action_for`] runs, parameterized over an explicit table
/// rather than the global [`effective_bindings`] -- lets `crate::config`'s
/// tests exercise a merged table's lookup behavior without touching
/// [`EFFECTIVE_BINDINGS`]' process-lifetime `OnceLock`.
pub fn action_for_table(bindings: &[KeyBinding], letter: char) -> Option<Action> {
    bindings
        .iter()
        .find(|binding| binding.letter == letter)
        .map(|binding| binding.action)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a bare `KeyEvent` (no kind/state distinctions needed for
    /// these tests) for the given code and modifiers.
    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn is_leader_key_matches_ctrl_a() {
        let event = key(KeyCode::Char('a'), KeyModifiers::CONTROL);
        assert!(is_leader_key(&event));
    }

    #[test]
    fn is_leader_key_rejects_plain_a() {
        let event = key(KeyCode::Char('a'), KeyModifiers::NONE);
        assert!(!is_leader_key(&event));
    }

    #[test]
    fn is_leader_key_matches_ctrl_colon() {
        let event = key(KeyCode::Char(':'), KeyModifiers::CONTROL);
        assert!(is_leader_key(&event));
    }

    #[test]
    fn is_leader_key_matches_ctrl_shift_semicolon() {
        let event = key(
            KeyCode::Char(';'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        );
        assert!(is_leader_key(&event));
    }

    #[test]
    fn is_leader_key_rejects_plain_colon() {
        let event = key(KeyCode::Char(':'), KeyModifiers::NONE);
        assert!(!is_leader_key(&event));
    }

    #[test]
    fn is_leader_key_rejects_semicolon_without_shift() {
        // Ctrl alone on ';' is a distinct, unrelated combo (not the leader).
        let event = key(KeyCode::Char(';'), KeyModifiers::CONTROL);
        assert!(!is_leader_key(&event));
    }

    #[test]
    fn is_leader_key_rejects_unrelated_key() {
        let event = key(KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert!(!is_leader_key(&event));
    }

    #[test]
    fn action_for_known_letter() {
        assert_eq!(action_for('q'), Some(Action::Quit));
        assert_eq!(action_for('c'), Some(Action::NewTerminal));
        assert_eq!(action_for('!'), Some(Action::RunCommand));
    }

    #[test]
    fn action_for_unknown_letter() {
        assert_eq!(action_for('z'), None);
    }

    /// Every binding's `action_name` round-trips back through
    /// `action_from_name` -- the two are meant to be exact inverses of
    /// each other over every action `LEADER_BINDINGS` actually lists.
    #[test]
    fn action_name_round_trips_through_action_from_name_for_every_binding() {
        for binding in LEADER_BINDINGS {
            let name = action_name(binding.action);
            assert_eq!(action_from_name(name), Some(binding.action));
        }
    }

    #[test]
    fn action_from_name_rejects_an_unknown_name() {
        assert_eq!(action_from_name("not_a_real_action"), None);
    }

    /// `effective_bindings` falls back to `LEADER_BINDINGS` unchanged in
    /// this test binary, since nothing here ever calls
    /// `init_effective_bindings` (a deliberately untested global -- see
    /// `crate::config`'s tests for the pure merge logic that would feed
    /// it, kept separate from this `OnceLock` so tests never race on
    /// shared global state).
    #[test]
    fn effective_bindings_defaults_to_leader_bindings() {
        assert_eq!(effective_bindings().len(), LEADER_BINDINGS.len());
    }
}
