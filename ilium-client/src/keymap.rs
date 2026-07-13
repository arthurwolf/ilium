//! Single source of truth for ilium's leader-key bindings.
//!
//! ilium's shortcut base defaults to `Ctrl+A` (like screen's classic
//! prefix), followed by a single letter. The base is configurable to any
//! ASCII letter through the Keyboard settings tab; `Ctrl+B` is the other
//! recommended preset because it is tmux's established default. Both the
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

/// A validated `Ctrl+letter` shortcut base. Keeping the letter private makes
/// invalid states (symbols, whitespace, multi-character strings) impossible
/// after config parsing or settings interaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShortcutBase(char);

impl ShortcutBase {
    pub const A: Self = Self('a');
    pub const B: Self = Self('b');

    /// Accepts one ASCII letter, case-insensitively.
    pub fn parse(value: &str) -> Option<Self> {
        let mut characters = value.chars();
        let letter = characters.next()?;
        if characters.next().is_some() || !letter.is_ascii_alphabetic() {
            return None;
        }
        Some(Self(letter.to_ascii_lowercase()))
    }

    pub const fn letter(self) -> char {
        self.0
    }

    /// Cycles through all allowed bases, wrapping from A to Z and vice versa.
    pub fn stepped(self, direction: i32) -> Self {
        let zero_based = i32::from(self.0 as u8 - b'a');
        let next = (zero_based + direction.signum()).rem_euclid(26) as u8;
        Self((b'a' + next) as char)
    }

    pub fn label(self) -> String {
        format!("Ctrl+{}", self.0.to_ascii_uppercase())
    }
}

impl Default for ShortcutBase {
    fn default() -> Self {
        Self::A
    }
}

/// The two broad, established presets presented before the custom A-Z
/// selector: GNU Screen's `Ctrl+A` and tmux's `Ctrl+B`.
pub const SHORTCUT_BASE_PRESETS: [ShortcutBase; 2] = [ShortcutBase::A, ShortcutBase::B];

/// User-facing guidance for one allowed shortcut base. Every non-recommended
/// letter has a concrete warning instead of a generic "may conflict" label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShortcutBaseAdvice {
    pub is_recommended: bool,
    pub explanation: &'static str,
}

/// Explains the terminal/shell convention a `Ctrl+letter` base shadows.
pub const fn shortcut_base_advice(base: ShortcutBase) -> ShortcutBaseAdvice {
    match base.letter() {
        'a' => ShortcutBaseAdvice {
            is_recommended: true,
            explanation: "GNU Screen's established prefix; it shadows shell beginning-of-line while ilium is active.",
        },
        'b' => ShortcutBaseAdvice {
            is_recommended: true,
            explanation: "tmux's established prefix; it shadows shell backward-character while ilium is active.",
        },
        'c' => warning("commonly sends interrupt/SIGINT and is often used for terminal copy"),
        'd' => warning("commonly sends EOF or logs out of an interactive shell"),
        'e' => warning("commonly moves the cursor to the end of the line"),
        'f' => warning("commonly moves forward one character or opens search"),
        'g' => warning("commonly cancels the current operation or rings the terminal bell"),
        'h' => warning("is normally Backspace in terminal control encoding"),
        'i' => warning("is indistinguishable from Tab in ordinary terminals, so every Tab becomes the shortcut base"),
        'j' => warning("is normally line-feed/newline in terminal control encoding"),
        'k' => warning("commonly cuts from the cursor to the end of the line"),
        'l' => warning("commonly clears or redraws the terminal"),
        'm' => warning("is indistinguishable from Enter in ordinary terminals, so every Enter becomes the shortcut base"),
        'n' => warning("commonly selects the next history entry or search result"),
        'o' => warning("commonly accepts the current line and fetches the next history entry"),
        'p' => warning("commonly selects the previous history entry"),
        'q' => warning("commonly resumes software flow control"),
        'r' => warning("commonly starts reverse history search"),
        's' => warning("commonly pauses software flow control and is also used for Save"),
        't' => warning("commonly transposes characters and is often used for a new terminal tab"),
        'u' => warning("commonly erases from the cursor to the start of the line"),
        'v' => warning("commonly quotes the next key and is often used for terminal paste"),
        'w' => warning("commonly erases the previous word and is often used to close a tab"),
        'x' => warning("is an Emacs command prefix and is often used for Cut"),
        'y' => warning("commonly yanks previously cut text"),
        'z' => warning("commonly suspends the foreground process and is often used for Undo"),
        _ => warning("is not representable as a portable Ctrl+letter terminal shortcut"),
    }
}

const fn warning(explanation: &'static str) -> ShortcutBaseAdvice {
    ShortcutBaseAdvice {
        is_recommended: false,
        explanation,
    }
}

/// One leader-key action. Extend this enum (and `LEADER_BINDINGS` below)
/// together when adding a new shortcut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    NewTerminal,
    NewEditor,
    NewBoard,
    ClosePane,
    NewGroup,
    NewSplitView,
    NewFolder,
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
    /// Opens the full-screen settings view (`crate::app::Mode::Settings`) --
    /// also reachable via a right-click in the tree panel
    /// (`ContextMenuAction::Settings`), which never goes through this table.
    Settings,
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
        letter: 'B',
        action: Action::NewBoard,
        description: "New board (choose storage format and location)",
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
        letter: 'W',
        action: Action::NewSplitView,
        description: "New vertical or horizontal split view",
    },
    KeyBinding {
        letter: 'f',
        action: Action::NewFolder,
        description: "Open a folder in the sidebar",
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
        letter: 'S',
        action: Action::Settings,
        description: "Open settings (also: use the tree footer gear)",
    },
    KeyBinding {
        letter: '?',
        action: Action::Help,
        description: "Show or hide this help screen",
    },
    KeyBinding {
        letter: 'q',
        action: Action::Quit,
        description: "Quit ilium",
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
        Action::NewBoard => "new_board",
        Action::ClosePane => "close_pane",
        Action::NewGroup => "new_group",
        Action::NewSplitView => "new_split_view",
        Action::NewFolder => "new_folder",
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
        Action::Settings => "settings",
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

/// True if this key event matches the currently configured `Ctrl+letter`
/// shortcut base. ASCII control-letter events are portable across ordinary
/// terminals without requiring an enhanced keyboard protocol.
pub fn is_leader_key(key: &KeyEvent, base: ShortcutBase) -> bool {
    match (base.letter(), key.code) {
        // Raw terminal input cannot distinguish Ctrl+I from Tab or Ctrl+M
        // from Enter. These bases remain available as requested, with an
        // explicit severe warning in the settings screen.
        ('i', KeyCode::Tab) | ('m', KeyCode::Enter) => true,
        (_, KeyCode::Char(letter)) => {
            letter.eq_ignore_ascii_case(&base.letter())
                && key.modifiers.contains(KeyModifiers::CONTROL)
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
    fn shortcut_base_accepts_exactly_one_ascii_letter() {
        assert_eq!(ShortcutBase::parse("A"), Some(ShortcutBase::A));
        assert_eq!(ShortcutBase::parse("b"), Some(ShortcutBase::B));
        assert_eq!(ShortcutBase::parse(""), None);
        assert_eq!(ShortcutBase::parse("ab"), None);
        assert_eq!(ShortcutBase::parse("?"), None);
        assert_eq!(ShortcutBase::parse("é"), None);
    }

    #[test]
    fn shortcut_base_steps_through_all_letters_with_wrapping() {
        assert_eq!(ShortcutBase::A.stepped(1), ShortcutBase::B);
        assert_eq!(ShortcutBase::A.stepped(-1).letter(), 'z');
        assert_eq!(
            ShortcutBase::parse("z").unwrap().stepped(1),
            ShortcutBase::A
        );
    }

    #[test]
    fn every_allowed_letter_has_specific_advice() {
        for letter in 'a'..='z' {
            let base = ShortcutBase::parse(&letter.to_string()).unwrap();
            assert!(!shortcut_base_advice(base).explanation.is_empty());
        }
        assert!(shortcut_base_advice(ShortcutBase::A).is_recommended);
        assert!(shortcut_base_advice(ShortcutBase::B).is_recommended);
        assert!(!shortcut_base_advice(ShortcutBase::parse("c").unwrap()).is_recommended);
    }

    #[test]
    fn is_leader_key_matches_configured_ctrl_a() {
        let event = key(KeyCode::Char('a'), KeyModifiers::CONTROL);
        assert!(is_leader_key(&event, ShortcutBase::A));
    }

    #[test]
    fn is_leader_key_rejects_plain_a() {
        let event = key(KeyCode::Char('a'), KeyModifiers::NONE);
        assert!(!is_leader_key(&event, ShortcutBase::A));
    }

    #[test]
    fn is_leader_key_changes_with_the_configured_base() {
        let ctrl_a = key(KeyCode::Char('a'), KeyModifiers::CONTROL);
        let ctrl_b = key(
            KeyCode::Char('B'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        );
        assert!(!is_leader_key(&ctrl_a, ShortcutBase::B));
        assert!(is_leader_key(&ctrl_b, ShortcutBase::B));
    }

    #[test]
    fn ctrl_i_and_ctrl_m_terminal_ambiguities_remain_selectable() {
        let tab = key(KeyCode::Tab, KeyModifiers::NONE);
        let enter = key(KeyCode::Enter, KeyModifiers::NONE);
        assert!(is_leader_key(&tab, ShortcutBase::parse("i").unwrap()));
        assert!(is_leader_key(&enter, ShortcutBase::parse("m").unwrap()));
        assert!(!is_leader_key(&tab, ShortcutBase::A));
        assert!(!is_leader_key(&enter, ShortcutBase::A));
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
