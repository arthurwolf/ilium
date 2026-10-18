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

// ---------------------------------------------------------------------------------------------
// Plugin traffic on the attached connection (reference-host-ipc.md, HST-100..HST-109). The
// records below travel inside `ClientRequest::Plugin`, `ServerEvent::Plugin`,
// `ServerEvent::Contributions` and `ServerEvent::Rejected`, and the handshake record inside
// `ClientRequest::Attach`. Variant order is the wire order (bincode variant indices).
// ---------------------------------------------------------------------------------------------

/// The plugin protocol version carried by [`PluginHello`] (HST-106).
pub const PLUGIN_PROTOCOL_VERSION: u16 = 1;

/// Largest encoded [`ContributionDelta`] the server sends (HST-109).
pub const MAX_DELTA_BYTES: usize = 4 * 1024 * 1024;

/// One envelope of plugin traffic in either direction (HST-100).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginEnvelope {
    /// The plugin id.
    pub plugin: String,
    /// Per-connection sequence number, 1 first, then +1.
    pub seq: u64,
    /// What the envelope carries.
    pub body: EnvelopeBody,
}

/// The seven bodies of an envelope, in manual order (HST-101).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EnvelopeBody {
    /// A client submission (client to server).
    Submit(Submission),
    /// The outcome of a submission (server to client).
    Outcome(OutcomeRecord),
    /// A plugin message between the two halves.
    Message(PluginMessage),
    /// A client-request call (server half asks the client half).
    ClientRequest(ClientRequestCall),
    /// The reply to a client-request call.
    ClientReply(ClientReplyBody),
    /// A sourced event forwarded to the other half.
    Event(SourcedEvent),
    /// A control record.
    Control(ControlBody),
}

/// A client submission (HST-112).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Submission {
    /// The submitting client.
    pub client_id: String,
    /// The dedup key of the intent.
    pub dedup_key: String,
    /// The intent as JSON.
    pub intent_json: String,
}

/// The outcome of one submission (HST-115).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutcomeRecord {
    /// The server sequence of the outcome.
    pub seq: u64,
    /// The dedup key of the submission.
    pub dedup_key: String,
    /// The outcome as JSON.
    pub outcome_json: String,
}

/// A message between the plugin halves (HST-100).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginMessage {
    /// The topic.
    pub topic: String,
    /// The payload bytes.
    pub payload: Vec<u8>,
}

/// A client-request call (HST-100).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientRequestCall {
    /// The call id the reply quotes.
    pub call_id: u64,
    /// The request name.
    pub name: String,
    /// The payload bytes.
    pub payload: Vec<u8>,
}

/// The reply to a client-request call (HST-100).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientReplyBody {
    /// The call id of the request.
    pub call_id: u64,
    /// The payload bytes.
    pub payload: Vec<u8>,
}

/// An event with its source topic (HST-100).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourcedEvent {
    /// The topic.
    pub topic: String,
    /// The payload bytes.
    pub payload: Vec<u8>,
}

/// Control records, in manual order (HST-108).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ControlBody {
    /// Subscribe to topics.
    Subscribe {
        /// The topics.
        topics: Vec<String>,
    },
    /// Ask for a fresh contribution snapshot.
    Resync,
    /// Grants changed.
    GrantsChanged,
    /// Ask for statistics.
    Stats,
}

/// The handshake record a client sends on attach (HST-103).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginHello {
    /// The plugin protocol version (`PLUGIN_PROTOCOL_VERSION`).
    pub plugin_protocol: u16,
    /// The contract version, for example `0.1.0`.
    pub contract: String,
    /// The plugins the client has installed.
    pub installed: Vec<InstalledPlugin>,
}

/// One installed plugin named in [`PluginHello`] (HST-103).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledPlugin {
    /// The plugin id.
    pub id: String,
    /// The installed version.
    pub version: String,
    /// The content hash of the installed package.
    pub content_hash: [u8; 32],
}

/// A contribution delta of the server's contribution store (HST-107).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContributionDelta {
    /// The store sequence after this delta.
    pub sequence: u64,
    /// Snapshot or increment.
    pub kind: DeltaKind,
    /// The operations, in order.
    pub ops: Vec<ContributionOp>,
}

/// Whether a delta replaces or extends the mirror (HST-107).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeltaKind {
    /// The full store.
    Snapshot,
    /// Changes since the previous sequence.
    Increment,
}

/// One operation of a contribution delta (HST-107).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContributionOp {
    /// Put or replace an entry.
    Put {
        /// The entry id.
        id: String,
        /// The encoded entry.
        entry: Vec<u8>,
    },
    /// Remove an entry.
    Remove {
        /// The entry id.
        id: String,
    },
    /// Remove every entry of a plugin.
    Clear {
        /// The plugin id.
        plugin: String,
    },
}

/// Why the server refused a connection or an envelope (HST-100, HST-103).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rejection {
    /// The error code, for example `ipc.protocol-mismatch`.
    pub code: String,
    /// Human text.
    pub message: String,
}
