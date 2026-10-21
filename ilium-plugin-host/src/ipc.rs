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

    /// The error id a refused submission reports: `ipc.payload-too-large` for a payload over the
    /// cap, refused before it is encoded, none otherwise (HST-124).
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

#[cfg(test)]
mod tests {
    use super::{
        check_payload, encode_checked, DedupVerdict, DetachedQueue, ForwardedDedup, IpcFault,
        OutcomeStore, PayloadKind, PluginRecord, PushOutcome, QueueVerdict, ReceiverConnection,
        StoredOutcome, WriterQueues, Written, CLIENT_TOO_SLOW_BYTES, DEDUP_WINDOW_MS,
        DETACHED_QUEUE_LIMIT, ENVELOPE_MAX_BYTES, EXIT_CODE_PROTOCOL_MISMATCH,
        OUTCOME_GRACE_MS, OUTCOME_RECORD_LIMIT, PLUGIN_QUEUE_MAX_BYTES,
        PLUGIN_QUEUE_MAX_RECORDS, PTY_FRAMES_PER_PLUGIN_ENVELOPE,
    };

    const MIB: usize = 1024 * 1024;
    const KIB: usize = 1024;

    /// Every payload kind with its documented cap in bytes (reference-host-ipc.md, Payload limits).
    fn payload_caps() -> [(PayloadKind, usize); 7] {
        [
            (PayloadKind::MessageBody, MIB),
            (PayloadKind::TopicPayload, 64 * KIB),
            (PayloadKind::UiTree, MIB),
            (PayloadKind::UiTreeT1, 4 * MIB),
            (PayloadKind::SurfaceWrite, 16 * MIB),
            (PayloadKind::Envelope, 17 * MIB),
            (PayloadKind::ContributionDelta, 4 * MIB),
        ]
    }

    fn plugin_record(id: u32, bytes: usize, coalesce_key: Option<&str>) -> PluginRecord {
        PluginRecord {
            id,
            bytes,
            coalesce_key: coalesce_key.map(str::to_owned),
        }
    }

    fn drain_writer(queues: &mut WriterQueues) -> Vec<Written> {
        let mut written = Vec::new();
        while let Some(item) = queues.next() {
            written.push(item);
        }
        written
    }

    // REQ: HST-113
    #[test]
    fn dedup_window_constant_is_ten_minutes() {
        assert_eq!(DEDUP_WINDOW_MS, 600_000);
    }

    // REQ: HST-116
    #[test]
    fn outcome_grace_and_record_limit_constants_match_manual() {
        assert_eq!(OUTCOME_GRACE_MS, 30_000);
        assert_eq!(OUTCOME_RECORD_LIMIT, 1_024);
    }

    // REQ: HST-119
    #[test]
    fn detached_queue_limit_constant_is_256() {
        assert_eq!(DETACHED_QUEUE_LIMIT, 256);
    }

    // REQ: HST-120
    #[test]
    fn pty_frames_per_plugin_envelope_constant_is_8() {
        assert_eq!(PTY_FRAMES_PER_PLUGIN_ENVELOPE, 8);
    }

    // REQ: HST-121
    #[test]
    fn plugin_queue_bounds_are_4mib_and_4096_records() {
        assert_eq!(PLUGIN_QUEUE_MAX_BYTES, 4 * MIB);
        assert_eq!(PLUGIN_QUEUE_MAX_RECORDS, 4_096);
    }

    // REQ: HST-122
    #[test]
    fn client_too_slow_threshold_is_16mib() {
        assert_eq!(CLIENT_TOO_SLOW_BYTES, 16 * MIB);
    }

    // REQ: HST-125
    #[test]
    fn envelope_max_is_17mib() {
        assert_eq!(ENVELOPE_MAX_BYTES, 17 * MIB);
    }

    // REQ: HST-104
    #[test]
    fn protocol_mismatch_exit_code_is_76() {
        assert_eq!(EXIT_CODE_PROTOCOL_MISMATCH, 76);
    }

    // REQ: HST-102
    #[test]
    fn ipc_fault_keeps_its_error_id() {
        assert_eq!(IpcFault::new("ipc.client-too-slow").code(), "ipc.client-too-slow");
        assert_eq!(IpcFault::new("ipc.sequence-gap").code(), "ipc.sequence-gap");
    }

    // REQ: HST-124
    #[test]
    fn completion_outcome_reports_payload_too_large_only() {
        assert_eq!(
            IpcFault::new("ipc.payload-too-large").completion_outcome(),
            Some("ipc.payload-too-large")
        );
        assert_eq!(IpcFault::new("ipc.sequence-gap").completion_outcome(), None);
    }

    // REQ: HST-124
    #[test]
    fn check_payload_accepts_empty_payload_for_every_kind() {
        for (kind, _cap) in payload_caps() {
            assert_eq!(check_payload(kind, 0), Ok(()), "kind {kind:?}");
        }
    }

    // REQ: HST-124
    #[test]
    fn check_payload_accepts_length_exactly_at_each_cap() {
        for (kind, cap) in payload_caps() {
            assert_eq!(check_payload(kind, cap), Ok(()), "kind {kind:?} at cap");
        }
    }

    // REQ: HST-124
    #[test]
    fn check_payload_refuses_one_byte_over_each_cap() {
        for (kind, cap) in payload_caps() {
            assert_eq!(
                check_payload(kind, cap + 1),
                Err(IpcFault::new("ipc.payload-too-large")),
                "kind {kind:?} one over cap"
            );
        }
    }

    // REQ: HST-124
    #[test]
    fn check_payload_refuses_far_over_cap() {
        assert_eq!(
            check_payload(PayloadKind::TopicPayload, usize::MAX),
            Err(IpcFault::new("ipc.payload-too-large"))
        );
    }

    // REQ: HST-124
    #[test]
    fn encode_checked_encodes_payload_within_cap() {
        let mut encode_calls = 0;
        let result = encode_checked(PayloadKind::ContributionDelta, 4 * MIB, || {
            encode_calls += 1;
            vec![7, 8, 9]
        });
        assert_eq!(result, Ok(vec![7, 8, 9]));
        assert_eq!(encode_calls, 1);
    }

    // REQ: HST-124
    #[test]
    fn encode_checked_never_calls_encode_for_oversize_payload() {
        let mut encode_calls = 0;
        let result = encode_checked(PayloadKind::TopicPayload, 64 * KIB + 1, || {
            encode_calls += 1;
            vec![1]
        });
        assert_eq!(result, Err(IpcFault::new("ipc.payload-too-large")));
        assert_eq!(encode_calls, 0);
    }

    // REQ: HST-124
    #[test]
    fn encode_checked_accepts_empty_declared_length() {
        let result = encode_checked(PayloadKind::MessageBody, 0, Vec::new);
        assert_eq!(result, Ok(Vec::new()));
    }

    // REQ: HST-102
    #[test]
    fn receiver_starts_open_with_no_fault() {
        let connection = ReceiverConnection::new();
        assert!(connection.is_open());
        assert_eq!(connection.closed_with(), None);
    }

    // REQ: HST-102
    #[test]
    fn receiver_accepts_consecutive_sequence_numbers_from_one() {
        let mut connection = ReceiverConnection::new();
        assert_eq!(connection.on_envelope(1, 10), Ok(()));
        assert_eq!(connection.on_envelope(2, 10), Ok(()));
        assert_eq!(connection.on_envelope(3, 0), Ok(()));
        assert!(connection.is_open());
        assert_eq!(connection.closed_with(), None);
    }

    // REQ: HST-102
    #[test]
    fn receiver_closes_with_sequence_gap_when_first_seq_is_two() {
        let mut connection = ReceiverConnection::new();
        assert_eq!(
            connection.on_envelope(2, 10),
            Err(IpcFault::new("ipc.sequence-gap"))
        );
        assert!(!connection.is_open());
        assert_eq!(connection.closed_with(), Some("ipc.sequence-gap".to_owned()));
    }

    // REQ: HST-102
    #[test]
    fn receiver_closes_with_sequence_gap_after_accepted_envelopes() {
        let mut connection = ReceiverConnection::new();
        assert_eq!(connection.on_envelope(1, 10), Ok(()));
        assert_eq!(
            connection.on_envelope(3, 10),
            Err(IpcFault::new("ipc.sequence-gap"))
        );
        assert!(!connection.is_open());
        assert_eq!(connection.closed_with(), Some("ipc.sequence-gap".to_owned()));
    }

    // REQ: HST-125
    #[test]
    fn receiver_accepts_envelope_exactly_at_17mib() {
        let mut connection = ReceiverConnection::new();
        assert_eq!(connection.on_envelope(1, ENVELOPE_MAX_BYTES), Ok(()));
        assert!(connection.is_open());
    }

    // REQ: HST-125
    #[test]
    fn receiver_closes_on_envelope_one_byte_over_17mib() {
        let mut connection = ReceiverConnection::new();
        assert_eq!(
            connection.on_envelope(1, ENVELOPE_MAX_BYTES + 1),
            Err(IpcFault::new("ipc.payload-too-large"))
        );
        assert!(!connection.is_open());
        assert_eq!(
            connection.closed_with(),
            Some("ipc.payload-too-large".to_owned())
        );
    }

    // REQ: HST-125
    #[test]
    fn receiver_accepts_empty_envelope() {
        let mut connection = ReceiverConnection::new();
        assert_eq!(connection.on_envelope(1, 0), Ok(()));
        assert!(connection.is_open());
    }

    // REQ: HST-113
    #[test]
    fn dedup_admits_first_request_as_fresh() {
        let mut dedup = ForwardedDedup::new();
        assert_eq!(dedup.admit("plugin.a", "client-1", "key-1", 0), DedupVerdict::Fresh);
    }

    // REQ: HST-113
    #[test]
    fn dedup_refuses_repeat_within_window() {
        let mut dedup = ForwardedDedup::new();
        assert_eq!(dedup.admit("plugin.a", "client-1", "key-1", 0), DedupVerdict::Fresh);
        assert_eq!(
            dedup.admit("plugin.a", "client-1", "key-1", 1),
            DedupVerdict::Duplicate
        );
        assert_eq!(
            dedup.admit("plugin.a", "client-1", "key-1", DEDUP_WINDOW_MS - 1),
            DedupVerdict::Duplicate
        );
    }

    // REQ: HST-113
    #[test]
    fn dedup_key_is_free_again_at_exactly_the_window_end() {
        let mut dedup = ForwardedDedup::new();
        assert_eq!(dedup.admit("plugin.a", "client-1", "key-1", 0), DedupVerdict::Fresh);
        assert_eq!(
            dedup.admit("plugin.a", "client-1", "key-1", 300_000),
            DedupVerdict::Duplicate
        );
        assert_eq!(
            dedup.admit("plugin.a", "client-1", "key-1", DEDUP_WINDOW_MS),
            DedupVerdict::Fresh
        );
    }

    // REQ: HST-113
    #[test]
    fn dedup_scope_is_the_plugin_and_client_pair() {
        let mut dedup = ForwardedDedup::new();
        assert_eq!(dedup.admit("plugin.a", "client-1", "key-1", 0), DedupVerdict::Fresh);
        assert_eq!(dedup.admit("plugin.b", "client-1", "key-1", 0), DedupVerdict::Fresh);
        assert_eq!(dedup.admit("plugin.a", "client-2", "key-1", 0), DedupVerdict::Fresh);
        assert_eq!(dedup.admit("plugin.a", "client-1", "key-2", 0), DedupVerdict::Fresh);
        assert_eq!(
            dedup.admit("plugin.a", "client-1", "key-1", 0),
            DedupVerdict::Duplicate
        );
    }

    // REQ: HST-117
    #[test]
    fn replay_returns_outcomes_after_last_seen_in_order() {
        let mut store = OutcomeStore::new();
        store.record("client-1", 1, "applied", 0);
        store.record("client-1", 2, "refused(busy)", 0);
        store.record("client-1", 3, "cancelled", 0);
        store.on_disconnect("client-1", 0);
        assert_eq!(
            store.replay("client-1", 1, 0),
            vec![
                StoredOutcome {
                    seq: 2,
                    outcome: "refused(busy)".to_owned(),
                },
                StoredOutcome {
                    seq: 3,
                    outcome: "cancelled".to_owned(),
                },
            ]
        );
    }

    // REQ: HST-117
    #[test]
    fn replay_is_empty_when_no_outcome_is_newer_than_last_seen() {
        let mut store = OutcomeStore::new();
        store.record("client-1", 1, "applied", 0);
        store.on_disconnect("client-1", 0);
        assert_eq!(store.replay("client-1", 1, 0), Vec::new());
    }

    // REQ: HST-117
    #[test]
    fn replay_returns_outcome_word_unchanged() {
        let mut store = OutcomeStore::new();
        store.record("client-1", 1, "applied", 0);
        store.on_disconnect("client-1", 0);
        assert_eq!(
            store.replay("client-1", 0, 0),
            vec![StoredOutcome {
                seq: 1,
                outcome: "applied".to_owned(),
            }]
        );
    }

    // REQ: HST-116
    #[test]
    fn replay_is_available_within_the_grace_period() {
        let mut store = OutcomeStore::new();
        store.record("client-1", 1, "applied", 0);
        store.on_disconnect("client-1", 0);
        assert_eq!(store.replay("client-1", 0, 29_999).len(), 1);
    }

    // REQ: HST-116
    #[test]
    fn replay_is_empty_after_the_grace_period() {
        let mut store = OutcomeStore::new();
        store.record("client-1", 1, "applied", 0);
        store.on_disconnect("client-1", 0);
        assert_eq!(store.replay("client-1", 0, 30_001), Vec::new());
    }

    // REQ: HST-117
    #[test]
    fn replay_for_unknown_client_is_empty() {
        let mut store = OutcomeStore::new();
        assert_eq!(store.replay("nobody", 0, 0), Vec::new());
        assert_eq!(store.record_count("nobody", 0), 0);
    }

    // REQ: HST-116
    #[test]
    fn record_count_is_kept_per_client() {
        let mut store = OutcomeStore::new();
        store.record("client-1", 1, "applied", 0);
        store.record("client-2", 1, "applied", 0);
        store.record("client-2", 2, "applied", 0);
        assert_eq!(store.record_count("client-1", 0), 1);
        assert_eq!(store.record_count("client-2", 0), 2);
    }

    // REQ: HST-116
    #[test]
    fn record_count_caps_at_1024_and_drops_oldest() {
        let mut store = OutcomeStore::new();
        for seq in 1..=(OUTCOME_RECORD_LIMIT as u64 + 1) {
            store.record("client-1", seq, "applied", 0);
        }
        store.on_disconnect("client-1", 0);
        assert_eq!(store.record_count("client-1", 0), OUTCOME_RECORD_LIMIT);
        let replayed = store.replay("client-1", 0, 0);
        assert_eq!(replayed.len(), OUTCOME_RECORD_LIMIT);
        assert_eq!(replayed.first().map(|outcome| outcome.seq), Some(2));
    }

    // REQ: HST-119
    #[test]
    fn detached_queue_starts_empty() {
        let queue = DetachedQueue::new();
        assert!(queue.is_empty());
        assert_eq!(queue.len(), 0);
    }

    // REQ: HST-119
    #[test]
    fn detached_queue_accepts_up_to_256_submissions() {
        let mut queue = DetachedQueue::new();
        for index in 0..DETACHED_QUEUE_LIMIT {
            assert_eq!(
                queue.push(&format!("key-{index}")),
                QueueVerdict::Queued,
                "submission {index}"
            );
        }
        assert_eq!(queue.len(), DETACHED_QUEUE_LIMIT);
    }

    // REQ: HST-119
    #[test]
    fn detached_queue_refuses_the_257th_submission_as_busy() {
        let mut queue = DetachedQueue::new();
        for index in 0..DETACHED_QUEUE_LIMIT {
            queue.push(&format!("key-{index}"));
        }
        assert_eq!(queue.push("key-overflow"), QueueVerdict::RefusedBusy);
        assert_eq!(queue.len(), DETACHED_QUEUE_LIMIT);
    }

    // REQ: HST-119
    #[test]
    fn on_client_exit_returns_queued_keys_in_order_and_empties_queue() {
        let mut queue = DetachedQueue::new();
        queue.push("key-a");
        queue.push("key-b");
        queue.push("key-c");
        assert_eq!(
            queue.on_client_exit(),
            vec!["key-a".to_owned(), "key-b".to_owned(), "key-c".to_owned()]
        );
        assert!(queue.is_empty());
    }

    // REQ: HST-119
    #[test]
    fn on_client_exit_on_empty_queue_returns_nothing() {
        let mut queue = DetachedQueue::new();
        assert_eq!(queue.on_client_exit(), Vec::<String>::new());
    }

    // REQ: HST-120
    #[test]
    fn writer_with_nothing_queued_yields_nothing() {
        let mut queues = WriterQueues::new();
        assert_eq!(queues.next(), None);
        assert!(!queues.resync_flagged());
    }

    // REQ: HST-120
    #[test]
    fn writer_sends_pty_frames_only_when_no_plugin_data_is_queued() {
        let mut queues = WriterQueues::new();
        queues.push_pty_frame(1);
        queues.push_pty_frame(2);
        queues.push_pty_frame(3);
        assert_eq!(
            drain_writer(&mut queues),
            vec![Written::Pty(1), Written::Pty(2), Written::Pty(3)]
        );
    }

    // REQ: HST-120
    #[test]
    fn writer_sends_plugin_record_when_no_pty_frames_are_queued() {
        let mut queues = WriterQueues::new();
        assert_eq!(queues.push_plugin(plugin_record(100, 10, None)), PushOutcome::Queued);
        assert_eq!(queues.next(), Some(Written::Plugin(100)));
        assert_eq!(queues.next(), None);
    }

    // REQ: HST-120
    #[test]
    fn writer_interleaves_one_plugin_record_per_eight_pty_frames() {
        let mut queues = WriterQueues::new();
        for frame in 1..=16 {
            queues.push_pty_frame(frame);
        }
        queues.push_plugin(plugin_record(100, 10, None));
        queues.push_plugin(plugin_record(101, 10, None));
        let mut expected: Vec<Written> = (1..=8).map(Written::Pty).collect();
        expected.push(Written::Plugin(100));
        expected.extend((9..=16).map(Written::Pty));
        expected.push(Written::Plugin(101));
        assert_eq!(drain_writer(&mut queues), expected);
    }

    // REQ: HST-121
    #[test]
    fn push_plugin_at_exact_record_cap_is_not_flagged() {
        let mut queues = WriterQueues::new();
        for id in 0..PLUGIN_QUEUE_MAX_RECORDS as u32 {
            assert_eq!(
                queues.push_plugin(plugin_record(id, 1, None)),
                PushOutcome::Queued,
                "record {id}"
            );
        }
        assert_eq!(queues.plugin_queued_records(), PLUGIN_QUEUE_MAX_RECORDS);
        assert!(!queues.resync_flagged());
    }

    // REQ: HST-121
    #[test]
    fn push_plugin_over_record_cap_flags_resync() {
        let mut queues = WriterQueues::new();
        for id in 0..PLUGIN_QUEUE_MAX_RECORDS as u32 {
            queues.push_plugin(plugin_record(id, 1, None));
        }
        let overflow_id = PLUGIN_QUEUE_MAX_RECORDS as u32;
        assert_eq!(
            queues.push_plugin(plugin_record(overflow_id, 1, None)),
            PushOutcome::ResyncFlagged
        );
        assert!(queues.resync_flagged());
    }

    // REQ: HST-121
    #[test]
    fn push_plugin_over_byte_cap_flags_resync() {
        let mut queues = WriterQueues::new();
        assert_eq!(
            queues.push_plugin(plugin_record(1, PLUGIN_QUEUE_MAX_BYTES, None)),
            PushOutcome::Queued
        );
        assert!(!queues.resync_flagged());
        assert_eq!(
            queues.push_plugin(plugin_record(2, 1, None)),
            PushOutcome::ResyncFlagged
        );
        assert!(queues.resync_flagged());
    }

    // REQ: HST-121
    #[test]
    fn overflow_merges_coalescable_record_before_flagging_resync() {
        let mut queues = WriterQueues::new();
        queues.push_plugin(plugin_record(0, 1, Some("pane-title")));
        for id in 1..PLUGIN_QUEUE_MAX_RECORDS as u32 {
            queues.push_plugin(plugin_record(id, 1, None));
        }
        assert_eq!(
            queues.push_plugin(plugin_record(4_096, 1, Some("pane-title"))),
            PushOutcome::Merged
        );
        assert!(!queues.resync_flagged());
        assert_eq!(queues.plugin_queued_records(), PLUGIN_QUEUE_MAX_RECORDS);
    }

    // REQ: HST-121
    #[test]
    fn coalesce_key_replaces_the_queued_record() {
        let mut queues = WriterQueues::new();
        assert_eq!(
            queues.push_plugin(plugin_record(1, 5, Some("status"))),
            PushOutcome::Queued
        );
        assert_eq!(
            queues.push_plugin(plugin_record(2, 7, Some("status"))),
            PushOutcome::Merged
        );
        assert_eq!(queues.plugin_queued_records(), 1);
        assert_eq!(queues.plugin_queued_bytes(), 7);
        assert_eq!(queues.next(), Some(Written::Plugin(2)));
        assert_eq!(queues.next(), None);
    }

    // REQ: HST-121
    #[test]
    fn plugin_queued_bytes_sums_record_sizes() {
        let mut queues = WriterQueues::new();
        assert_eq!(queues.plugin_queued_bytes(), 0);
        queues.push_plugin(plugin_record(1, 100, None));
        queues.push_plugin(plugin_record(2, 200, None));
        assert_eq!(queues.plugin_queued_bytes(), 300);
    }

    // REQ: HST-122
    #[test]
    fn record_exactly_at_16mib_does_not_disconnect_client() {
        let mut queues = WriterQueues::new();
        let outcome = queues.push_plugin(plugin_record(1, CLIENT_TOO_SLOW_BYTES, None));
        assert!(!matches!(outcome, PushOutcome::Disconnect(_)));
    }

    // REQ: HST-122
    #[test]
    fn queued_plugin_data_over_16mib_disconnects_with_client_too_slow() {
        let mut queues = WriterQueues::new();
        assert_eq!(
            queues.push_plugin(plugin_record(1, CLIENT_TOO_SLOW_BYTES + 1, None)),
            PushOutcome::Disconnect(IpcFault::new("ipc.client-too-slow"))
        );
    }

    // REQ: HST-122
    #[test]
    fn cumulative_plugin_data_over_16mib_disconnects_on_the_push_that_crosses_it() {
        let mut queues = WriterQueues::new();
        queues.push_plugin(plugin_record(1, 8 * MIB, None));
        queues.push_plugin(plugin_record(2, 8 * MIB, None));
        assert_eq!(queues.plugin_queued_bytes(), CLIENT_TOO_SLOW_BYTES);
        assert_eq!(
            queues.push_plugin(plugin_record(3, 1, None)),
            PushOutcome::Disconnect(IpcFault::new("ipc.client-too-slow"))
        );
    }
}
