//! Standalone plugin records carried between `ilium-server` and `ilium-client`
//! (reference-host-architecture.md, reference-extension-points.md).
//!
//! Skeleton state: the records are complete and not yet appended to [`crate::ClientRequest`] or
//! [`crate::ServerEvent`], because a new variant forces new match arms in existing server and
//! client code. The implementation appends them (append-only rule). The records carry plain
//! strings and bytes so this crate keeps no edge to the plugin crates.

use serde::{Deserialize, Serialize};

/// A plugin side changed state (server to client).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginStateChanged {
    /// The plugin id.
    pub plugin: String,
    /// `server`, `client` or `scene`.
    pub side: String,
    /// The new instance state, for example `active`.
    pub state: String,
    /// The instance number, new after each restart.
    pub instance: u64,
}

/// A contribution (command, setting, key, view, ...) was added or removed (server to client).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginContributionDelta {
    /// The contributing plugin.
    pub plugin: String,
    /// The extension point id.
    pub extension_point: String,
    /// Contribution ids added, as JSON documents.
    pub added_json: Vec<String>,
    /// Contribution ids removed.
    pub removed: Vec<String>,
    /// The contribution store revision after this delta.
    pub revision: u64,
}

/// A rendered view tree for one plugin view (client side render to server cache and back).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginViewTree {
    /// The plugin id.
    pub plugin: String,
    /// The view id.
    pub view: String,
    /// The ui tree as JSON.
    pub tree_json: String,
    /// Revision of the view, counted per view.
    pub revision: u64,
}

/// A batch of submissions from a plugin instance (client to server).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginSubmission {
    /// The plugin id.
    pub plugin: String,
    /// The instance number the submissions came from.
    pub instance: u64,
    /// The submissions as JSON documents, in order.
    pub submissions_json: Vec<String>,
}

/// The outcome of one submission (server to client).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginOutcome {
    /// The plugin id.
    pub plugin: String,
    /// The request number the plugin was given.
    pub request: u64,
    /// The outcome as JSON.
    pub outcome_json: String,
}

/// An `ilium plugin ...` command run through the server (CLI to server).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginCliRequest {
    /// The arguments after `ilium plugin`.
    pub args: Vec<String>,
}
