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
        'i' => warning(
            "is indistinguishable from Tab in ordinary terminals, so every Tab becomes the shortcut base",
        ),
        'j' => warning("is normally line-feed/newline in terminal control encoding"),
        'k' => warning("commonly cuts from the cursor to the end of the line"),
        'l' => warning("commonly clears or redraws the terminal"),
        'm' => warning(
            "is indistinguishable from Enter in ordinary terminals, so every Enter becomes the shortcut base",
        ),
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
    FocusNextPane,
    FocusPreviousPane,
    FocusPaneLeft,
    FocusPaneRight,
    FocusPaneUp,
    FocusPaneDown,
    ScrollbackUp,
    ScrollbackDown,
    Save,
    RunCommand,
    Search,
    Help,
    Detach,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyBinding {
    pub letter: char,
    pub action: Action,
    pub description: &'static str,
}

/// One memorable word a user can associate with a possible key for an
/// action. The word deliberately begins with its key, so the Keyboard
/// settings table can explain a remap without inventing a second shortcut
/// language. Punctuation has no useful word mnemonic and is therefore not
/// represented here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionMnemonic {
    pub key: char,
    pub word: &'static str,
}

/// Every printable key that can safely be used as the second key in an
/// ilium leader sequence. Keeping this finite, explicit catalogue shared by
/// the settings table and the visual picker means a binding cannot be saved
/// for a key that the dispatcher could never receive.
pub const BINDABLE_KEYS: &[char] = &[
    'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's',
    't', 'u', 'v', 'w', 'x', 'y', 'z', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'A', 'B',
    'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O', 'P', 'Q', 'R', 'S', 'T', 'U',
    'V', 'W', 'X', 'Y', 'Z', '`', '-', '=', '[', ']', '\\', ';', '\'', ',', '.', '/', '!', '@',
    '#', '$', '%', '^', '&', '*', '(', ')', '_', '+', '{', '}', '|', ':', '\"', '<', '>', '?', '~',
    ' ',
];

/// Human-readable cap label for a printable leader key. Space is printable
/// but invisible in a table, so it must never be rendered as an empty box.
pub fn key_label(key: char) -> String {
    match key {
        ' ' => "Space".to_string(),
        _ => key.to_string(),
    }
}

/// A complete preset, not merely a prefix. The Screen and tmux presets use
/// their documented defaults where ilium has an equivalent action, while
/// retaining ilium-only editor/tree actions on free keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeymapPreset {
    Screen,
    Tmux,
}

impl KeymapPreset {
    pub const ALL: [Self; 2] = [Self::Screen, Self::Tmux];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Screen => "GNU Screen",
            Self::Tmux => "tmux",
        }
    }

    pub const fn shortcut_base(self) -> ShortcutBase {
        match self {
            Self::Screen => ShortcutBase::A,
            Self::Tmux => ShortcutBase::B,
        }
    }
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
        letter: 'o',
        action: Action::FocusNextPane,
        description: "Focus the next visible pane",
    },
    KeyBinding {
        letter: ';',
        action: Action::FocusPreviousPane,
        description: "Focus the previous visible pane",
    },
    KeyBinding {
        letter: 'h',
        action: Action::FocusPaneLeft,
        description: "Focus the visible pane to the left",
    },
    KeyBinding {
        letter: 'l',
        action: Action::FocusPaneRight,
        description: "Focus the visible pane to the right",
    },
    KeyBinding {
        letter: 'k',
        action: Action::FocusPaneUp,
        description: "Focus the visible pane above",
    },
    KeyBinding {
        letter: 'j',
        action: Action::FocusPaneDown,
        description: "Focus the visible pane below",
    },
    KeyBinding {
        letter: '[',
        action: Action::ScrollbackUp,
        description: "Scroll the focused terminal one page up",
    },
    KeyBinding {
        letter: ']',
        action: Action::ScrollbackDown,
        description: "Scroll the focused terminal one page down",
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
        letter: '/',
        action: Action::Search,
        description: "Search terminal history and open editor buffers",
    },
    KeyBinding {
        letter: 'v',
        action: Action::ToggleEditorViewMode,
        description: "Toggle the focused editor pane between Source and Rendered (markdown files only)",
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
        letter: 'd',
        action: Action::Detach,
        description: "Detach this client and leave the session running",
    },
    KeyBinding {
        letter: 'Q',
        action: Action::Quit,
        description: "Kill this project session and disconnect every client",
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
        Action::FocusNextPane => "focus_next_pane",
        Action::FocusPreviousPane => "focus_previous_pane",
        Action::FocusPaneLeft => "focus_pane_left",
        Action::FocusPaneRight => "focus_pane_right",
        Action::FocusPaneUp => "focus_pane_up",
        Action::FocusPaneDown => "focus_pane_down",
        Action::ScrollbackUp => "scrollback_up",
        Action::ScrollbackDown => "scrollback_down",
        Action::Save => "save",
        Action::RunCommand => "run_command",
        Action::Search => "search",
        Action::Help => "help",
        Action::Detach => "detach",
        Action::Quit => "quit",
        Action::ToggleEditorViewMode => "toggle_editor_view_mode",
        Action::ToggleLineNumbers => "toggle_line_numbers",
        Action::ToggleMinimap => "toggle_minimap",
        Action::ToggleAutosave => "toggle_autosave",
        Action::Settings => "settings",
    }
}

/// Compact table label for the Keyboard settings view. The detailed
/// `KeyBinding::description` remains the Help text; this label keeps the
/// three-column editor usable in an 80-column terminal.
pub const fn action_label(action: Action) -> &'static str {
    match action {
        Action::NewTerminal => "New terminal",
        Action::NewEditor => "New editor",
        Action::NewBoard => "New board",
        Action::ClosePane => "Close selection",
        Action::NewGroup => "New group",
        Action::NewSplitView => "New split view",
        Action::NewFolder => "Open folder",
        Action::Rename => "Rename selection",
        Action::ToggleMove => "Move mode",
        Action::FocusTree => "Focus tree",
        Action::FocusPane => "Focus active pane",
        Action::FocusNextPane => "Focus next pane",
        Action::FocusPreviousPane => "Focus previous pane",
        Action::FocusPaneLeft => "Focus pane left",
        Action::FocusPaneRight => "Focus pane right",
        Action::FocusPaneUp => "Focus pane above",
        Action::FocusPaneDown => "Focus pane below",
        Action::ScrollbackUp => "Scrollback up",
        Action::ScrollbackDown => "Scrollback down",
        Action::Save => "Save editor",
        Action::RunCommand => "Run command",
        Action::Search => "Workspace search",
        Action::Help => "Help",
        Action::Detach => "Detach client",
        Action::Quit => "Kill session",
        Action::ToggleEditorViewMode => "Toggle editor view",
        Action::ToggleLineNumbers => "Toggle line numbers",
        Action::ToggleMinimap => "Toggle minimap",
        Action::ToggleAutosave => "Toggle autosave",
        Action::Settings => "Open settings",
    }
}

/// Returns the action-owned mnemonic candidates for every letter where a
/// useful association exists. They are intentionally a data registry rather
/// than UI text: the Settings table, future key pickers, and presets can all
/// consult the same vocabulary after a remap.
pub const fn action_mnemonics(action: Action) -> &'static [ActionMnemonic] {
    use ActionMnemonic as Mnemonic;

    match action {
        Action::NewTerminal => &[
            Mnemonic {
                key: 'c',
                word: "create",
            },
            Mnemonic {
                key: 'n',
                word: "new",
            },
            Mnemonic {
                key: 'o',
                word: "open",
            },
            Mnemonic {
                key: 't',
                word: "terminal",
            },
        ],
        Action::NewEditor => &[
            Mnemonic {
                key: 'c',
                word: "compose",
            },
            Mnemonic {
                key: 'e',
                word: "edit",
            },
            Mnemonic {
                key: 'n',
                word: "new",
            },
            Mnemonic {
                key: 'w',
                word: "write",
            },
        ],
        Action::NewBoard => &[
            Mnemonic {
                key: 'b',
                word: "board",
            },
            Mnemonic {
                key: 'c',
                word: "canvas",
            },
            Mnemonic {
                key: 'n',
                word: "new",
            },
        ],
        Action::ClosePane => &[
            Mnemonic {
                key: 'c',
                word: "close",
            },
            Mnemonic {
                key: 'd',
                word: "dismiss",
            },
            Mnemonic {
                key: 'k',
                word: "kill",
            },
            Mnemonic {
                key: 'x',
                word: "xout",
            },
        ],
        Action::NewGroup => &[
            Mnemonic {
                key: 'b',
                word: "bundle",
            },
            Mnemonic {
                key: 'c',
                word: "cluster",
            },
            Mnemonic {
                key: 'g',
                word: "group",
            },
            Mnemonic {
                key: 'n',
                word: "new",
            },
        ],
        Action::NewSplitView => &[
            Mnemonic {
                key: 'd',
                word: "divide",
            },
            Mnemonic {
                key: 'p',
                word: "partition",
            },
            Mnemonic {
                key: 's',
                word: "split",
            },
            Mnemonic {
                key: 'w',
                word: "window",
            },
        ],
        Action::NewFolder => &[
            Mnemonic {
                key: 'b',
                word: "browse",
            },
            Mnemonic {
                key: 'd',
                word: "directory",
            },
            Mnemonic {
                key: 'f',
                word: "folder",
            },
            Mnemonic {
                key: 'o',
                word: "open",
            },
        ],
        Action::Rename => &[
            Mnemonic {
                key: 'a',
                word: "alter",
            },
            Mnemonic {
                key: 'l',
                word: "label",
            },
            Mnemonic {
                key: 'n',
                word: "name",
            },
            Mnemonic {
                key: 'r',
                word: "rename",
            },
        ],
        Action::ToggleMove => &[
            Mnemonic {
                key: 'm',
                word: "move",
            },
            Mnemonic {
                key: 'o',
                word: "organize",
            },
            Mnemonic {
                key: 'r',
                word: "reorder",
            },
            Mnemonic {
                key: 's',
                word: "shift",
            },
        ],
        Action::FocusTree => &[
            Mnemonic {
                key: 'b',
                word: "branches",
            },
            Mnemonic {
                key: 'f',
                word: "focus",
            },
            Mnemonic {
                key: 'n',
                word: "navigation",
            },
            Mnemonic {
                key: 't',
                word: "tree",
            },
        ],
        Action::FocusPane => &[
            Mnemonic {
                key: 'a',
                word: "active",
            },
            Mnemonic {
                key: 'f',
                word: "focus",
            },
            Mnemonic {
                key: 'p',
                word: "pane",
            },
        ],
        Action::FocusNextPane => &[
            Mnemonic {
                key: 'c',
                word: "cycle",
            },
            Mnemonic {
                key: 'n',
                word: "next",
            },
            Mnemonic {
                key: 'o',
                word: "onward",
            },
        ],
        Action::FocusPreviousPane => &[
            Mnemonic {
                key: 'b',
                word: "back",
            },
            Mnemonic {
                key: 'p',
                word: "previous",
            },
            Mnemonic {
                key: 'r',
                word: "reverse",
            },
        ],
        Action::FocusPaneLeft => &[
            Mnemonic {
                key: 'h',
                word: "hither",
            },
            Mnemonic {
                key: 'l',
                word: "left",
            },
            Mnemonic {
                key: 'w',
                word: "west",
            },
        ],
        Action::FocusPaneRight => &[
            Mnemonic {
                key: 'e',
                word: "east",
            },
            Mnemonic {
                key: 'l',
                word: "lateral",
            },
            Mnemonic {
                key: 'r',
                word: "right",
            },
        ],
        Action::FocusPaneUp => &[
            Mnemonic {
                key: 'a',
                word: "above",
            },
            Mnemonic {
                key: 'u',
                word: "up",
            },
        ],
        Action::FocusPaneDown => &[
            Mnemonic {
                key: 'd',
                word: "down",
            },
            Mnemonic {
                key: 'j',
                word: "jump",
            },
            Mnemonic {
                key: 's',
                word: "south",
            },
        ],
        Action::ScrollbackUp => &[
            Mnemonic {
                key: 'b',
                word: "back",
            },
            Mnemonic {
                key: 'p',
                word: "previous",
            },
            Mnemonic {
                key: 'u',
                word: "up",
            },
        ],
        Action::ScrollbackDown => &[
            Mnemonic {
                key: 'd',
                word: "down",
            },
            Mnemonic {
                key: 'f',
                word: "forward",
            },
            Mnemonic {
                key: 'n',
                word: "next",
            },
        ],
        Action::Save => &[
            Mnemonic {
                key: 'c',
                word: "commit",
            },
            Mnemonic {
                key: 'p',
                word: "persist",
            },
            Mnemonic {
                key: 's',
                word: "save",
            },
            Mnemonic {
                key: 'w',
                word: "write",
            },
        ],
        Action::RunCommand => &[
            Mnemonic {
                key: 'c',
                word: "command",
            },
            Mnemonic {
                key: 'e',
                word: "execute",
            },
            Mnemonic {
                key: 'r',
                word: "run",
            },
        ],
        Action::Search => &[
            Mnemonic {
                key: 'f',
                word: "find",
            },
            Mnemonic {
                key: 'l',
                word: "look",
            },
            Mnemonic {
                key: 'q',
                word: "query",
            },
            Mnemonic {
                key: 's',
                word: "search",
            },
        ],
        Action::Help => &[
            Mnemonic {
                key: 'a',
                word: "aid",
            },
            Mnemonic {
                key: 'g',
                word: "guide",
            },
            Mnemonic {
                key: 'h',
                word: "help",
            },
        ],
        Action::Detach => &[
            Mnemonic {
                key: 'd',
                word: "detach",
            },
            Mnemonic {
                key: 'l',
                word: "leave",
            },
            Mnemonic {
                key: 'u',
                word: "unplug",
            },
        ],
        Action::Quit => &[
            Mnemonic {
                key: 'e',
                word: "end",
            },
            Mnemonic {
                key: 'q',
                word: "quit",
            },
            Mnemonic {
                key: 's',
                word: "stop",
            },
        ],
        Action::ToggleEditorViewMode => &[
            Mnemonic {
                key: 'm',
                word: "mode",
            },
            Mnemonic {
                key: 'r',
                word: "render",
            },
            Mnemonic {
                key: 'v',
                word: "view",
            },
        ],
        Action::ToggleLineNumbers => &[
            Mnemonic {
                key: 'l',
                word: "lines",
            },
            Mnemonic {
                key: 'n',
                word: "numbers",
            },
            Mnemonic {
                key: 'o',
                word: "ordinal",
            },
        ],
        Action::ToggleMinimap => &[
            Mnemonic {
                key: 'b',
                word: "birdseye",
            },
            Mnemonic {
                key: 'm',
                word: "minimap",
            },
            Mnemonic {
                key: 'o',
                word: "overview",
            },
        ],
        Action::ToggleAutosave => &[
            Mnemonic {
                key: 'a',
                word: "automatic",
            },
            Mnemonic {
                key: 'p',
                word: "persist",
            },
            Mnemonic {
                key: 's',
                word: "save",
            },
        ],
        Action::Settings => &[
            Mnemonic {
                key: 'c',
                word: "configure",
            },
            Mnemonic {
                key: 'o',
                word: "options",
            },
            Mnemonic {
                key: 's',
                word: "settings",
            },
        ],
    }
}

/// Looks up the mnemonic for an action's currently assigned key. Uppercase
/// bindings share their lowercase word, while punctuation intentionally
/// returns `None` because it has no letter-led word equivalent.
pub fn mnemonic_for(action: Action, key: char) -> Option<&'static str> {
    let letter = key.to_ascii_lowercase();
    action_mnemonics(action)
        .iter()
        .find(|mnemonic| mnemonic.key == letter)
        .map(|mnemonic| mnemonic.word)
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

/// Replaces one action's binding only when `key` is currently free. This is
/// the single collision gate used by both the visual picker and preset load;
/// callers can report a refusal without ever transiently installing an
/// ambiguous map.
pub fn assign_key(bindings: &mut [KeyBinding], action: Action, key: char) -> Result<(), Action> {
    if !BINDABLE_KEYS.contains(&key) {
        return Err(action);
    }
    if let Some(existing) = bindings.iter().find(|binding| binding.letter == key) {
        if existing.action != action {
            return Err(existing.action);
        }
        return Ok(());
    }
    if let Some(binding) = bindings.iter_mut().find(|binding| binding.action == action) {
        binding.letter = key;
    }
    Ok(())
}

/// Returns the free keys in physical-keyboard order for the settings table
/// and picker. A row's own current key is deliberately excluded: selecting a
/// key is a reassignment, never an accidental no-op.
pub fn available_keys(bindings: &[KeyBinding]) -> Vec<char> {
    BINDABLE_KEYS
        .iter()
        .copied()
        .filter(|key| !bindings.iter().any(|binding| binding.letter == *key))
        .collect()
}

/// Builds the full documented default map for one established multiplexer.
/// ilium has no flat windows or paste buffer, so the closest tree/pane action
/// is used only where the interaction is genuinely equivalent.
pub fn preset_bindings(preset: KeymapPreset) -> Vec<KeyBinding> {
    let mut bindings = LEADER_BINDINGS.to_vec();
    let assignments: &[(Action, char)] = match preset {
        KeymapPreset::Screen => &[
            (Action::NewTerminal, 'c'),
            (Action::ClosePane, 'k'),
            (Action::Rename, 'A'),
            (Action::NewSplitView, 'S'),
            (Action::FocusPane, 'P'),
            (Action::FocusNextPane, 'n'),
            (Action::FocusPreviousPane, 'p'),
            (Action::FocusPaneLeft, 'H'),
            (Action::FocusPaneRight, 'L'),
            (Action::FocusPaneUp, 'K'),
            (Action::FocusPaneDown, 'J'),
            (Action::ScrollbackUp, '['),
            (Action::ScrollbackDown, ']'),
            (Action::Help, '?'),
            (Action::Detach, 'd'),
            (Action::Quit, '\\'),
            (Action::Settings, 'o'),
            (Action::NewGroup, 'g'),
            (Action::NewEditor, 'e'),
            (Action::NewBoard, 'B'),
            (Action::NewFolder, 'F'),
            (Action::ToggleMove, 'm'),
            (Action::FocusTree, 't'),
            (Action::Save, 's'),
            (Action::RunCommand, ':'),
            (Action::Search, '/'),
            (Action::ToggleEditorViewMode, 'v'),
            (Action::ToggleLineNumbers, 'N'),
            (Action::ToggleMinimap, 'b'),
            (Action::ToggleAutosave, 'a'),
        ],
        KeymapPreset::Tmux => &[
            (Action::NewTerminal, 'c'),
            (Action::ClosePane, 'x'),
            (Action::Rename, ','),
            (Action::NewSplitView, '"'),
            (Action::FocusPane, 'P'),
            (Action::FocusNextPane, 'o'),
            (Action::FocusPreviousPane, ';'),
            (Action::FocusPaneLeft, 'h'),
            (Action::FocusPaneRight, 'l'),
            (Action::FocusPaneUp, 'k'),
            (Action::FocusPaneDown, 'j'),
            (Action::ScrollbackUp, '['),
            (Action::ScrollbackDown, ']'),
            (Action::Help, '?'),
            (Action::Detach, 'd'),
            (Action::Quit, '&'),
            (Action::Settings, ':'),
            (Action::NewGroup, 'g'),
            (Action::NewEditor, 'e'),
            (Action::NewBoard, 'B'),
            (Action::NewFolder, 'F'),
            (Action::ToggleMove, 'm'),
            (Action::FocusTree, 't'),
            (Action::Save, 's'),
            (Action::RunCommand, '!'),
            (Action::Search, 'f'),
            (Action::ToggleEditorViewMode, 'v'),
            (Action::ToggleLineNumbers, 'n'),
            (Action::ToggleMinimap, 'b'),
            (Action::ToggleAutosave, 'a'),
        ],
    };
    for (action, key) in assignments {
        // Preset literals are maintained alongside the bindable catalogue and
        // every map starts one-action-per-row, so this cannot conflict.
        let binding = bindings
            .iter_mut()
            .find(|binding| binding.action == *action)
            .expect("every preset action is registered in LEADER_BINDINGS");
        binding.letter = *key;
    }
    bindings
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
        assert_eq!(action_for('d'), Some(Action::Detach));
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

    #[test]
    fn mnemonic_words_start_with_their_registered_letters() {
        for binding in LEADER_BINDINGS {
            for mnemonic in action_mnemonics(binding.action) {
                assert_eq!(
                    mnemonic
                        .word
                        .chars()
                        .next()
                        .map(|letter| letter.to_ascii_lowercase()),
                    Some(mnemonic.key),
                    "{} mnemonic {:?} must start with {:?}",
                    action_name(binding.action),
                    mnemonic.word,
                    mnemonic.key,
                );
            }
        }
    }

    #[test]
    fn documented_preset_letters_have_mnemonics_except_explicitly_unmnemonicable_keys() {
        let maps = [
            LEADER_BINDINGS.to_vec(),
            preset_bindings(KeymapPreset::Screen),
            preset_bindings(KeymapPreset::Tmux),
        ];
        for bindings in maps {
            for binding in bindings {
                let has_no_honest_word = matches!(
                    (binding.action, binding.letter.to_ascii_lowercase()),
                    (Action::FocusPaneUp, 'k')
                );
                if binding.letter.is_ascii_alphabetic() && !has_no_honest_word {
                    assert!(
                        mnemonic_for(binding.action, binding.letter).is_some(),
                        "{} on {:?} needs a mnemonic",
                        action_name(binding.action),
                        binding.letter,
                    );
                }
            }
        }
        assert_eq!(mnemonic_for(Action::Search, '/'), None);
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

    #[test]
    fn assignment_refuses_a_key_already_owned_by_another_action() {
        let mut bindings = LEADER_BINDINGS.to_vec();
        assert_eq!(
            assign_key(&mut bindings, Action::Quit, 'c'),
            Err(Action::NewTerminal)
        );
        assert_eq!(action_for_table(&bindings, 'Q'), Some(Action::Quit));
    }

    #[test]
    fn presets_have_their_documented_prefixes_and_no_collisions() {
        for preset in KeymapPreset::ALL {
            let bindings = preset_bindings(preset);
            let unique = bindings
                .iter()
                .map(|binding| binding.letter)
                .collect::<std::collections::HashSet<_>>();
            assert_eq!(unique.len(), bindings.len());
        }
        assert_eq!(KeymapPreset::Screen.shortcut_base(), ShortcutBase::A);
        assert_eq!(KeymapPreset::Tmux.shortcut_base(), ShortcutBase::B);
        assert_eq!(
            action_for_table(&preset_bindings(KeymapPreset::Tmux), 'x'),
            Some(Action::ClosePane)
        );
    }
}
