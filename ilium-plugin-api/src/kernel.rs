//! The object-safe kernel facade the plugin host calls (reference-host-architecture.md).
//! Traits only; the server implements them.

pub use crate::generated::primitives::terminal::Cell;
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
}

/// What the plugin host needs from the kernel (reference-host-architecture.md).
pub trait KernelFacade: Send + Sync {
    /// Applies one intent for an origin (reference-events-and-intents.md).
    fn apply_intent(&self, origin: &Origin, intent: Intent) -> IntentResult;
    /// Tree reader.
    fn tree(&self) -> &dyn TreeRead;
    /// Pane reader.
    fn panes(&self) -> &dyn PaneRead;
    /// Which process this kernel runs in.
    fn process_kind(&self) -> ProcessKind;
}
