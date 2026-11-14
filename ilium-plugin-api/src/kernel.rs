//! The object-safe kernel facade the plugin host calls (reference-host-architecture.md).
//! Traits only; the server implements them.

pub use crate::generated::primitives::terminal::Cell;
pub use crate::generated::primitives::terminal::ForegroundProcess;
pub use crate::generated::types::tree::NodeView as TreeNode;

use crate::generated::types::{Intent, NodeId, Outcome, PluginId};
use crate::platform::ProcessKind;
use crate::side::Side;

/// Who asked for an intent (reference-host-architecture.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// A plugin side instance (reference-host-architecture.md).
    Plugin {
        /// The plugin.
        plugin: PluginId,
        /// Its side.
        side: Side,
        /// The instance number, new after each restart.
        instance: u64,
    },
    /// The kernel itself.
    Kernel,
    /// A user action in the client.
    User,
    /// The `ilium` CLI.
    Cli,
}

/// The result of an applied intent is its outcome (reference-wit.md `ilium:types/requests`).
pub type IntentResult = Outcome;

/// Read access to the tree (reference-host-architecture.md).
pub trait TreeRead: Send + Sync {
    /// Every node in pre-order (reference-events-and-intents.md ABI-047).
    fn nodes(&self) -> Vec<TreeNode>;
    /// One node, if it exists.
    fn node(&self, id: NodeId) -> Option<TreeNode>;
    /// The tree revision (reference-wit.md `ok-epoch`).
    fn epoch(&self) -> u64;
}

/// Read access to pane content (reference-host-architecture.md).
pub trait PaneRead: Send + Sync {
    /// The visible screen as text.
    fn screen_text(&self, pane: NodeId) -> Option<String>;
    /// Styled cells of the given rows.
    fn cells(&self, pane: NodeId, rows: std::ops::Range<u16>) -> Option<Vec<Vec<Cell>>>;
    /// The last `lines` lines of history.
    fn history(&self, pane: NodeId, lines: u32) -> Option<Vec<String>>;
    /// The size of the pane in columns and rows, when this process knows it.
    fn size(&self, _pane: NodeId) -> Option<(u16, u16)> {
        None
    }
    /// The cursor and the revision of the visible screen, when this process knows them. The
    /// revision grows with every output the screen took in, so a plugin that classifies the
    /// screen (agent detection) can tell a new screen from one it already read.
    fn screen_state(&self, _pane: NodeId) -> Option<ScreenState> {
        None
    }
    /// The process that runs in the pane: the agent the kernel detected there, else the pane's
    /// own process. `None` when this process cannot inspect processes.
    fn foreground_process(&self, _pane: NodeId) -> Option<ForegroundProcess> {
        None
    }
}

/// The cursor and revision of a pane's visible screen (`ilium:primitives/terminal` `screen`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScreenState {
    /// The cursor row, from 0.
    pub cursor_row: u16,
    /// The cursor column, from 0.
    pub cursor_column: u16,
    /// Grows with every output the screen took in.
    pub revision: u64,
}

/// What the plugin host needs from the kernel (reference-host-architecture.md).
/// What plugins attach to tree nodes. Every entry belongs to one plugin: a plugin never reads
/// or replaces the bytes of another.
pub trait TreeAnnotations: Send + Sync {
    /// The bytes `plugin` keeps on `node`, or none.
    fn metadata(&self, plugin: &str, node: NodeId) -> Option<Vec<u8>>;
    /// Replaces the bytes `plugin` keeps on `node`; empty bytes remove the entry. The caller
    /// has checked the grant and the size limit; the store refuses an unknown node.
    fn set_metadata(&self, plugin: &str, node: NodeId, bytes: Vec<u8>) -> Result<(), FacadeError>;
}

pub trait KernelFacade: Send + Sync {
    /// Applies one intent for an origin (reference-events-and-intents.md).
    fn apply_intent(&self, origin: &Origin, intent: Intent) -> IntentResult;
    /// Tree reader.
    fn tree(&self) -> &dyn TreeRead;
    /// Pane reader.
    fn panes(&self) -> &dyn PaneRead;
    /// Which process this kernel runs in.
    fn process_kind(&self) -> ProcessKind;

    /// Kernel settings tables, where this process has them.
    fn settings(&self) -> Option<&dyn KernelSettings> {
        None
    }
    /// Terminal input and control; server only.
    fn terminal(&self) -> Option<&dyn TerminalAccess> {
        None
    }
    /// Agent detection state.
    fn agents(&self) -> Option<&dyn AgentRead> {
        None
    }
    /// Session snapshots; server only.
    fn sessions(&self) -> Option<&dyn SessionAccess> {
        None
    }
    /// The project workspace; server only.
    fn workspace(&self) -> Option<&dyn WorkspaceAccess> {
        None
    }
    /// Panels, toasts and redraw requests; client only.
    fn ui(&self) -> Option<&dyn UiSink> {
        None
    }
    /// The resolved key map; client only.
    fn keymap(&self) -> Option<&dyn KeymapRead> {
        None
    }
    /// The help catalogue; client only.
    fn help(&self) -> Option<&dyn HelpCatalog> {
        None
    }
    /// The store of what plugins attach to tree nodes; `None` on a side that keeps none.
    fn annotations(&self) -> Option<&dyn TreeAnnotations> {
        None
    }
    /// The clipboard; client only.
    fn clipboard(&self) -> Option<&dyn ClipboardAccess> {
        None
    }
    /// The pane that has focus, if this process knows it.
    fn focused_pane(&self) -> Option<NodeId> {
        None
    }
}

/// A kernel-side failure of a facade call, mapped to a `plugin-error` by the import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FacadeError {
    /// The error class.
    pub code: crate::generated::types::ErrorCode,
    /// A short message for the plugin log.
    pub message: String,
}

impl FacadeError {
    /// An error of a class with a message.
    pub fn new(code: crate::generated::types::ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// Kernel settings tables (`ilium:host/kernel-settings`). Values are JSON text.
pub trait KernelSettings: Send + Sync {
    /// The value of a key, if set.
    fn get(&self, key: &str) -> Option<String>;
    /// Writes a key; the kernel validates and persists it.
    fn set(&self, key: &str, value_json: &str) -> Result<(), FacadeError>;
    /// The revision of the settings tables; it grows with every accepted write.
    fn revision(&self) -> u64;
}

/// Terminal input and control of a pane (`ilium:primitives/terminal`). The PTY stays owned
/// by the server; this trait only forwards requests to its owner.
pub trait TerminalAccess: Send + Sync {
    /// Writes bytes to the pane's input.
    fn write_input(&self, pane: NodeId, bytes: &[u8]) -> Result<(), FacadeError>;
    /// Resizes the pane's terminal.
    fn resize(&self, pane: NodeId, columns: u16, rows: u16) -> Result<(), FacadeError>;
    /// Sends a signal by name (for example `interrupt`) to the pane's foreground process.
    fn signal(&self, pane: NodeId, signal: &str) -> Result<(), FacadeError>;
}

/// Agent detection state (`ilium:primitives/agent`).
pub trait AgentRead: Send + Sync {
    /// The agent of a pane as JSON of the `agent-view` record, if one was identified.
    fn agent_json(&self, pane: NodeId) -> Option<String>;
}

/// Session snapshots (`ilium:host/sessions`).
pub trait SessionAccess: Send + Sync {
    /// The names of the stored sessions of this project.
    fn names(&self) -> Vec<String>;
    /// The stored snapshot of a session as JSON text.
    fn snapshot_json(&self, name: &str) -> Option<String>;
    /// Asks the kernel to write the current session snapshot.
    fn save(&self) -> Result<(), FacadeError>;
}

/// The project workspace (`ilium:host/workspace`).
pub trait WorkspaceAccess: Send + Sync {
    /// The canonical project directory.
    fn project_dir(&self) -> Option<String>;
    /// The session name.
    fn session_name(&self) -> Option<String>;
}

/// What the client draws for plugins (`ilium:host/ui-host`). Trees and toasts are JSON of
/// their mirror types, so this trait does not depend on the renderer.
pub trait UiSink: Send + Sync {
    /// Shows or replaces a panel of a plugin.
    fn put_panel(&self, plugin: &PluginId, panel: &str, tree_json: &str)
        -> Result<(), FacadeError>;
    /// Removes a panel of a plugin.
    fn remove_panel(&self, plugin: &PluginId, panel: &str);
    /// Shows a toast.
    fn toast(&self, plugin: &PluginId, toast_json: &str) -> Result<(), FacadeError>;
    /// Asks for a redraw of a view of a plugin.
    fn invalidate(&self, plugin: &PluginId, view: &str);
}

/// One resolved binding of the key map: a command with every chord that runs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyBinding {
    /// The command id.
    pub command: String,
    /// The chords bound to it, in the chord syntax of the manual.
    pub chords: Vec<String>,
    /// Where the binding applies (`global`, `leader`, ...).
    pub context: String,
    /// Whether this is the binding the kernel ships, not one the user changed.
    pub is_default: bool,
}

/// The resolved key map (`ilium:host/keymap`).
pub trait KeymapRead: Send + Sync {
    /// Every resolved binding. A kernel that cannot list its key map gives none.
    fn bindings(&self) -> Vec<KeyBinding> {
        Vec::new()
    }

    /// The chords bound to a command id, in display form.
    fn chords_of(&self, command: &str) -> Vec<String>;
    /// The command id bound to a chord.
    fn command_of(&self, chord: &str) -> Option<String>;
}

/// The help catalogue (`ilium:host/help`).
pub trait HelpCatalog: Send + Sync {
    /// The markdown of a topic.
    fn topic(&self, id: &str) -> Option<String>;
    /// Every topic id.
    fn topic_ids(&self) -> Vec<String>;
}

/// The clipboard (`ilium:host/clipboard`).
pub trait ClipboardAccess: Send + Sync {
    /// Replaces the clipboard text.
    fn write_text(&self, text: &str) -> Result<(), FacadeError>;
    /// The clipboard text, when the platform lets the client read it.
    fn read_text(&self) -> Result<Option<String>, FacadeError>;
}
