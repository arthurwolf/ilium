//! The server-side rules of plugin IPC as pure state machines (reference-host-ipc.md HST-100..125):
//! envelope sequencing, payload caps, forwarded-request dedup, outcome replay, the detached queue
//! and the writer queues. No I/O; `ilium-server` drives these from its connection tasks.

use std::collections::VecDeque;

/// A forwarded request's dedup key is remembered this long, milliseconds (HST-113).
pub const DEDUP_WINDOW_MS: u64 = 600_000;

/// Outcomes of a disconnected client are kept this long for replay, milliseconds (HST-116).
pub const OUTCOME_GRACE_MS: u64 = 30_000;

/// Outcome records kept per client id; oldest dropped beyond this (HST-117).
pub const OUTCOME_RECORD_LIMIT: usize = 1_024;

/// Submissions a detached client may have queued (HST-119).
pub const DETACHED_QUEUE_LIMIT: usize = 256;

/// PTY frames written per plugin envelope when both queues are busy (HST-120).
pub const PTY_FRAMES_PER_PLUGIN_ENVELOPE: u32 = 8;

/// Bytes the plugin queue of one connection may hold before a resync is flagged (HST-121).
pub const PLUGIN_QUEUE_MAX_BYTES: usize = 4 * 1024 * 1024;

/// Records the plugin queue of one connection may hold before a resync is flagged (HST-121).
pub const PLUGIN_QUEUE_MAX_RECORDS: usize = 4_096;

/// Queued bytes at which a client is too slow and is disconnected (HST-122).
pub const CLIENT_TOO_SLOW_BYTES: usize = 16 * 1024 * 1024;

/// The largest envelope a connection accepts (HST-125).
pub const ENVELOPE_MAX_BYTES: usize = 17 * 1024 * 1024;

/// Exit code of a client whose plugin protocol does not match the server's (HST-104).
pub const EXIT_CODE_PROTOCOL_MISMATCH: i32 = 76;

/// Why a connection was closed or a payload refused (reference-host-ipc.md errors table).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IpcFault {
    code: String,
}

impl IpcFault {
    /// A fault with the given error id, such as `ipc.sequence-gap`.
    pub fn new(code: &str) -> IpcFault {
        IpcFault {
            code: code.to_owned(),
        }
    }

    /// The error id (`ipc.sequence-gap`, `ipc.payload-too-large`, `ipc.client-too-slow`,
    /// `ipc.protocol-mismatch`) (HST-102, HST-124).
    pub fn code(&self) -> &str {
        &self.code
    }

    /// The completion outcome a refused submission gets: `failed(too-large)` for a payload over
    /// the cap, none otherwise (HST-124).
    pub fn completion_outcome(&self) -> Option<&'static str> {
        unimplemented!("host::ipc::IpcFault::completion_outcome")
    }
}

/// The kinds of payload with their own cap (HST-109, HST-124).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PayloadKind {
    /// A message body, 1 MiB.
    MessageBody,
    /// A topic payload, 64 KiB.
    TopicPayload,
    /// A UI tree, 1 MiB.
    UiTree,
    /// A UI tree of a T1 plugin, 4 MiB.
    UiTreeT1,
    /// A surface write, 16 MiB.
    SurfaceWrite,
    /// A whole envelope, 17 MiB.
    Envelope,
    /// A contribution delta, 4 MiB.
    ContributionDelta,
}

/// Refuses a payload longer than its kind's cap (HST-124).
pub fn check_payload(_kind: PayloadKind, _len: usize) -> Result<(), IpcFault> {
    unimplemented!("host::ipc::check_payload")
}

/// Encodes a payload only when its declared length is within the cap; `encode` is never called
/// for an oversize payload (HST-124).
pub fn encode_checked<F: FnOnce() -> Vec<u8>>(
    _kind: PayloadKind,
    _declared_len: usize,
    _encode: F,
) -> Result<Vec<u8>, IpcFault> {
    unimplemented!("host::ipc::encode_checked")
}

/// The receiving side of one connection: envelope sequence numbers start at 1 and increase by
/// one; a gap or an oversize envelope closes the connection (HST-102, HST-125).
#[derive(Debug, Default)]
pub struct ReceiverConnection;

impl ReceiverConnection {
    /// An open connection expecting sequence 1.
    pub fn new() -> ReceiverConnection {
        ReceiverConnection
    }

    /// Accepts one envelope; the fault closes the connection.
    pub fn on_envelope(&mut self, _seq: u64, _encoded_len: usize) -> Result<(), IpcFault> {
        unimplemented!("host::ipc::ReceiverConnection::on_envelope")
    }

    /// Whether the connection is still open.
    pub fn is_open(&self) -> bool {
        unimplemented!("host::ipc::ReceiverConnection::is_open")
    }

    /// The error id the connection was closed with, when closed.
    pub fn closed_with(&self) -> Option<String> {
        unimplemented!("host::ipc::ReceiverConnection::closed_with")
    }
}

/// Whether a forwarded request is new within the dedup window (HST-113).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DedupVerdict {
    /// Not seen within the window.
    Fresh,
    /// Seen within the window.
    Duplicate,
}

/// Remembers forwarded requests per plugin and client for `DEDUP_WINDOW_MS` (HST-113).
#[derive(Debug, Default)]
pub struct ForwardedDedup;

impl ForwardedDedup {
    /// An empty window.
    pub fn new() -> ForwardedDedup {
        ForwardedDedup
    }

    /// Admits a request once per window; a key is free again at exactly `DEDUP_WINDOW_MS` after
    /// it was accepted.
    pub fn admit(
        &mut self,
        _plugin: &str,
        _client_id: &str,
        _dedup_key: &str,
        _now_ms: u64,
    ) -> DedupVerdict {
        unimplemented!("host::ipc::ForwardedDedup::admit")
    }
}

/// One outcome kept for replay (HST-116).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredOutcome {
    /// The outcome's sequence number.
    pub seq: u64,
    /// The outcome word.
    pub outcome: String,
}

/// Outcomes kept per client id for replay after a reattach (HST-116, HST-117).
#[derive(Debug, Default)]
pub struct OutcomeStore;

impl OutcomeStore {
    /// An empty store.
    pub fn new() -> OutcomeStore {
        OutcomeStore
    }

    /// Records an outcome; the oldest beyond `OUTCOME_RECORD_LIMIT` per client is dropped.
    pub fn record(&mut self, _client_id: &str, _seq: u64, _outcome: &str, _now_ms: u64) {
        unimplemented!("host::ipc::OutcomeStore::record")
    }

    /// Starts the grace period of a disconnected client.
    pub fn on_disconnect(&mut self, _client_id: &str, _now_ms: u64) {
        unimplemented!("host::ipc::OutcomeStore::on_disconnect")
    }

    /// The outcomes after `last_seen_seq` for a reattaching client, in order; empty once the
    /// grace period has elapsed.
    pub fn replay(
        &mut self,
        _client_id: &str,
        _last_seen_seq: u64,
        _now_ms: u64,
    ) -> Vec<StoredOutcome> {
        unimplemented!("host::ipc::OutcomeStore::replay")
    }

    /// The records currently kept for a client.
    pub fn record_count(&mut self, _client_id: &str, _now_ms: u64) -> usize {
        unimplemented!("host::ipc::OutcomeStore::record_count")
    }
}

/// Whether a detached submission was queued (HST-119).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueVerdict {
    /// Queued.
    Queued,
    /// Refused: the queue holds `DETACHED_QUEUE_LIMIT` submissions.
    RefusedBusy,
}

/// Submissions a detached client has not seen complete (HST-119).
#[derive(Debug, Default)]
pub struct DetachedQueue {
    keys: VecDeque<String>,
}

impl DetachedQueue {
    /// An empty queue.
    pub fn new() -> DetachedQueue {
        DetachedQueue::default()
    }

    /// Queues one submission by dedup key.
    pub fn push(&mut self, _dedup_key: &str) -> QueueVerdict {
        unimplemented!("host::ipc::DetachedQueue::push")
    }

    /// Queued submissions.
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Whether nothing is queued.
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// The keys cancelled when the client exits, in queue order.
    pub fn on_client_exit(&mut self) -> Vec<String> {
        unimplemented!("host::ipc::DetachedQueue::on_client_exit")
    }
}

/// One plugin record waiting in the writer queue (HST-121).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginRecord {
    /// The record id (test handle).
    pub id: u32,
    /// Encoded size.
    pub bytes: usize,
    /// Records with the same key replace each other while queued.
    pub coalesce_key: Option<String>,
}

/// What the writer sent next (HST-120).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Written {
    /// A PTY frame.
    Pty(u32),
    /// A plugin record.
    Plugin(u32),
}

/// What happened to a pushed plugin record (HST-121..123).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PushOutcome {
    /// Queued as is.
    Queued,
    /// Replaced the queued record with the same coalesce key.
    Merged,
    /// Queued, and the queue is over its soft cap: a resync is flagged.
    ResyncFlagged,
    /// The client is too slow; the connection is closed with the fault.
    Disconnect(IpcFault),
}

/// The two writer queues of one connection: PTY frames and plugin records, with the fairness and
/// back-pressure rules of HST-120..123.
#[derive(Debug, Default)]
pub struct WriterQueues {
    plugin: VecDeque<PluginRecord>,
    resync_flagged: bool,
}

impl WriterQueues {
    /// Empty queues.
    pub fn new() -> WriterQueues {
        WriterQueues::default()
    }

    /// Queues a PTY frame.
    pub fn push_pty_frame(&mut self, _id: u32) {
        unimplemented!("host::ipc::WriterQueues::push_pty_frame")
    }

    /// Queues a plugin record.
    pub fn push_plugin(&mut self, _record: PluginRecord) -> PushOutcome {
        unimplemented!("host::ipc::WriterQueues::push_plugin")
    }

    /// Whether the plugin queue overflowed its soft cap.
    pub fn resync_flagged(&self) -> bool {
        self.resync_flagged
    }

    /// Bytes queued in the plugin queue.
    pub fn plugin_queued_bytes(&self) -> usize {
        unimplemented!("host::ipc::WriterQueues::plugin_queued_bytes")
    }

    /// Records queued in the plugin queue.
    pub fn plugin_queued_records(&self) -> usize {
        self.plugin.len()
    }
}

impl Iterator for WriterQueues {
    type Item = Written;

    /// The next item to write: `PTY_FRAMES_PER_PLUGIN_ENVELOPE` PTY frames per plugin record.
    fn next(&mut self) -> Option<Written> {
        unimplemented!("host::ipc::WriterQueues::next")
    }
}
