//! Single source of truth for illium's leader-key bindings.
//!
//! illium's prefix key is `Ctrl+A` (like screen's classic prefix),
//! followed by a single letter. `Ctrl+:` is also recognized as an
//! alternate leader for terminals that can disambiguate it (see
//! `is_leader_key`), but `Ctrl+A` is the documented default since it needs
//! no special terminal protocol support — it works identically in a plain
//! xterm, VS Code's integrated terminal, tmux, everywhere. Both the
//! input-dispatch logic (`app.rs`) and the help screen render straight
//! from `LEADER_BINDINGS`, so the two can't drift out of sync — add a new
//! leader shortcut here and both consumers pick it up automatically.

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
        description: "Toggle move mode for the selected node (then use arrow keys)",
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

/// Looks up the action bound to a letter pressed right after the leader.
pub fn action_for(letter: char) -> Option<Action> {
    LEADER_BINDINGS
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
}
