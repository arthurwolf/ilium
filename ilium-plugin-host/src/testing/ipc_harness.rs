//! An in-process server plus client halves around a `FakeKernel`, driven by hand, for the
//! IPC rules of reference-host-ipc.md (HST-100..125). The grant `panes.create` gates the intent
//! `create-pane` (outcome `ok-node`).

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use crate::ipc::{
    DedupVerdict, DetachedQueue, ForwardedDedup, OutcomeStore, PluginRecord, PushOutcome,
    QueueVerdict, ReceiverConnection, WriterQueues, Written, EXIT_CODE_PROTOCOL_MISMATCH,
    OUTCOME_GRACE_MS,
};

/// A connection handle of the harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConnId(pub u32);

/// What a client announces in its hello (HST-103, HST-105).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelloSpec {
    /// The plugin protocol version the client speaks.
    pub plugin_protocol: u16,
    /// The API contract version the client was built against.
    pub contract: String,
    /// The plugins the client has installed.
    pub installed: Vec<InstalledSpec>,
}

/// One plugin as the hello lists it (HST-105).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledSpec {
    /// The plugin id.
    pub id: String,
    /// The version.
    pub version: String,
    /// The package content hash.
    pub content_hash: [u8; 32],
}

/// One item the server sent to a connection (HST-106..118).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireItem {
    /// The attach reply with the server's plugin set.
    Attached {
        /// The plugin ids.
        plugin_set: Vec<String>,
    },
    /// A full contribution snapshot.
    Snapshot {
        /// Its sequence number.
        sequence: u64,
        /// Its encoded size.
        bytes: usize,
    },
    /// A contribution increment.
    Increment {
        /// Its sequence number.
        sequence: u64,
        /// Its encoded size.
        bytes: usize,
    },
    /// A tree change.
    TreeChange {
        /// The node id.
        node: String,
    },
    /// A submission outcome.
    Outcome {
        /// The outcome sequence number.
        seq: u64,
        /// The plugin.
        plugin: String,
        /// The dedup key.
        dedup_key: String,
        /// The outcome word.
        outcome: String,
    },
    /// A rejected message.
    Rejected {
        /// The error id.
        code: String,
    },
}

/// What a client half did, in order (HST-110, HST-111).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientDelivery {
    /// The mirror was updated.
    MirrorUpdated {
        /// The node id.
        node: String,
    },
    /// A tree-changed event was delivered to the client-side plugin.
    TreeChangedEvent {
        /// The node id.
        node: String,
    },
    /// A request completed.
    RequestCompleted {
        /// The dedup key.
        dedup_key: String,
    },
}

/// How a client half exited (HST-104).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientExit {
    /// The process exit code.
    pub code: i32,
    /// The message printed.
    pub message: String,
    /// Whether the interface was drawn before exiting.
    pub interface_drawn: bool,
}

/// The state of one plugin's client half (HST-105).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientHalf {
    /// Whether the client-side instance runs.
    pub running: bool,
    /// The error id it was disabled with, when disabled.
    pub disabled_code: Option<String>,
}

/// The in-process IPC harness (HST-100..125).
#[derive(Debug, Default)]
pub struct IpcHarness {
    clock_ms: u64,
    apply_paused: bool,
    plugins: BTreeMap<String, ServerPlugin>,
    /// Contribution id to encoded size.
    contributions: BTreeMap<String, usize>,
    tree: Vec<String>,
    connections: Vec<Connection>,
    clients: BTreeMap<String, ClientState>,
    /// Submissions the server has received but not yet admitted.
    inbox: Vec<Submission>,
    /// Admitted submissions waiting for the apply step.
    unapplied: Vec<Submission>,
    dedup: ForwardedDedup,
    outcomes: OutcomeStore,
    /// (client id, outcome seq) to (plugin, dedup key), for replay.
    outcome_details: BTreeMap<(String, u64), (String, String)>,
    /// (client id, plugin, dedup key) to (outcome seq, outcome word).
    completed: BTreeMap<(String, String, String), (u64, String)>,
}

#[derive(Debug, Default)]
struct ServerPlugin {
    version: String,
    content_hash: [u8; 32],
    grants: BTreeSet<String>,
}

#[derive(Debug, Default)]
struct ClientState {
    conn: Option<ConnId>,
    disconnected_at: Option<u64>,
    outcome_seq: u64,
    detached: DetachedQueue,
    queued: Vec<Submission>,
}

#[derive(Debug)]
struct Submission {
    client_id: String,
    plugin: String,
    dedup_key: String,
    intent: String,
}

/// One encoded slice of a contribution, waiting in the writer queue.
#[derive(Debug)]
struct Delta {
    sequence: u64,
    contribution: String,
    bytes: usize,
    /// Lost on the wire: the sequence is used up, nothing reaches the client.
    lost: bool,
}

#[derive(Debug, Default)]
struct Connection {
    client_id: String,
    open: bool,
    close_code: Option<String>,
    exit: Option<ClientExit>,
    halves: BTreeMap<String, ClientHalf>,
    wire: Vec<WireItem>,
    deliveries: Vec<ClientDelivery>,
    mirror: BTreeSet<String>,
    receiver: ReceiverConnection,
    writer: WriterQueues,
    /// Contribution deltas by writer record id; plugin records without an entry are data only.
    outbound: BTreeMap<u32, Delta>,
    next_record_id: u32,
    next_pty_id: u32,
    /// The sequence the server gives the next contribution delta.
    next_out_seq: u64,
    /// The sequence the client half expects next.
    expected_in_seq: u64,
    drop_next: bool,
    overflow_resync: bool,
    resync_requests: usize,
    submits_sent: usize,
    pty_written: usize,
    pty_dropped: usize,
}

impl Connection {
    fn new_delta(&mut self, contribution: &str, bytes: usize) -> Delta {
        let sequence = self.next_out_seq;
        self.next_out_seq += 1;
        Delta {
            sequence,
            contribution: contribution.to_owned(),
            bytes,
            lost: std::mem::take(&mut self.drop_next),
        }
    }

    /// The mirror takes the node before the client half sees the tree event (HST-111).
    fn tree_changed(&mut self, node: &str) {
        self.wire.push(WireItem::TreeChange {
            node: node.to_owned(),
        });
        self.mirror.insert(node.to_owned());
        self.deliveries.push(ClientDelivery::MirrorUpdated {
            node: node.to_owned(),
        });
        self.deliveries.push(ClientDelivery::TreeChangedEvent {
            node: node.to_owned(),
        });
    }

    fn deliver(&mut self, delta: Delta, server_ids: &[String], snapshot_bytes: usize) {
        if delta.lost {
            return;
        }
        if delta.sequence != self.expected_in_seq {
            self.resync(server_ids, snapshot_bytes);
            return;
        }
        self.expected_in_seq += 1;
        self.wire.push(WireItem::Increment {
            sequence: delta.sequence,
            bytes: delta.bytes,
        });
        self.mirror.insert(delta.contribution);
    }

    /// The client discards its mirror; the server answers with a full snapshot (HST-108).
    fn resync(&mut self, server_ids: &[String], snapshot_bytes: usize) {
        self.resync_requests += 1;
        self.mirror = server_ids.iter().cloned().collect();
        self.wire.push(WireItem::Snapshot {
            sequence: 1,
            bytes: snapshot_bytes,
        });
        self.expected_in_seq = 2;
        self.next_out_seq = 2;
        self.outbound.clear();
    }
}

const PLUGIN_PROTOCOL: u16 = 1;
const CONTRACT: &str = "0.1.0";
const PROTOCOL_MISMATCH: &str = "ipc.protocol-mismatch";
const PROTOCOL_MISMATCH_MESSAGE: &str =
    "client and server speak different plugin protocols; start the matching client";
const VERSION_SKEW: &str = "plugin.version-skew";
const SERVER_MISSING: &str = "plugin.server-missing";
const PANE_GRANT: &str = "panes.create";
const CREATE_PANE_INTENT: &str = "create-pane";
const OK_OUTCOME: &str = "ok-node";
const REVOKED_OUTCOME: &str = "revoked";
const CANCELLED_OUTCOME: &str = "cancelled";
const REFUSED_BUSY_OUTCOME: &str = "refused(busy)";
const UNKNOWN_INTENT_OUTCOME: &str = "refused(unknown-intent)";
const CONTRIBUTION_DELTA_MAX_BYTES: usize = 4 * 1024 * 1024;
/// Envelope sizes are not modelled; the receiver only checks sequence numbers here.
const ENVELOPE_LEN: usize = 0;
const NO_HALF: ClientHalf = ClientHalf {
    running: false,
    disabled_code: None,
};

fn conn_index(conn: ConnId) -> usize {
    conn.0 as usize
}

/// Removes the submissions of one client from a list, keeping the others in order.
fn split_off_client(items: &mut Vec<Submission>, client_id: &str) -> Vec<Submission> {
    let (matching, kept): (Vec<Submission>, Vec<Submission>) = std::mem::take(items)
        .into_iter()
        .partition(|submission| submission.client_id == client_id);
    *items = kept;
    matching
}

impl IpcHarness {
    /// An empty server with no plugins and no connections.
    pub fn new() -> IpcHarness {
        IpcHarness::default()
    }

    /// Installs a plugin on the server.
    pub fn install_server_plugin(&mut self, id: &str, version: &str, content_hash: [u8; 32]) {
        let server_plugin = self.plugins.entry(id.to_owned()).or_default();
        server_plugin.version = version.to_owned();
        server_plugin.content_hash = content_hash;
    }

    /// Grants a plugin a grant.
    pub fn grant(&mut self, plugin: &str, grant: &str) {
        if let Some(server_plugin) = self.plugins.get_mut(plugin) {
            server_plugin.grants.insert(grant.to_owned());
        }
    }

    /// Revokes a grant of a plugin.
    pub fn revoke(&mut self, plugin: &str, grant: &str) {
        if let Some(server_plugin) = self.plugins.get_mut(plugin) {
            server_plugin.grants.remove(grant);
        }
    }

    /// The hello of a client that matches the server: protocol 1, contract `0.1.0`, the
    /// server's plugin set (HST-103).
    pub fn matching_hello(&self) -> HelloSpec {
        HelloSpec {
            plugin_protocol: PLUGIN_PROTOCOL,
            contract: CONTRACT.to_owned(),
            installed: self
                .plugins
                .iter()
                .map(|(id, server_plugin)| InstalledSpec {
                    id: id.clone(),
                    version: server_plugin.version.clone(),
                    content_hash: server_plugin.content_hash,
                })
                .collect(),
        }
    }

    /// Connects a client.
    pub fn connect(&mut self, client_id: &str, _interactive: bool, hello: HelloSpec) -> ConnId {
        self.open_connection(client_id, hello, None)
    }

    /// Reattaches a client that last saw outcome `last_seen_seq` (HST-116).
    pub fn reattach(&mut self, client_id: &str, last_seen_seq: u64, hello: HelloSpec) -> ConnId {
        self.expire_grace();
        self.open_connection(client_id, hello, Some(last_seen_seq))
    }

    /// Drops a connection.
    pub fn disconnect(&mut self, conn: ConnId) {
        let index = conn_index(conn);
        if !self.connections[index].open {
            return;
        }
        self.connections[index].open = false;
        self.detach_client(index);
    }

    /// Moves the clock.
    pub fn advance(&mut self, by: Duration) {
        let millis = u64::try_from(by.as_millis()).unwrap_or(u64::MAX);
        self.clock_ms = self.clock_ms.saturating_add(millis);
    }

    /// Runs server and client halves until idle or `max_rounds`.
    pub fn pump(&mut self, max_rounds: u32) {
        for _ in 0..max_rounds {
            if !self.run_round() {
                break;
            }
        }
    }

    /// Everything the server sent to the connection, in order.
    pub fn wire(&self, conn: ConnId) -> Vec<WireItem> {
        self.connections[conn_index(conn)].wire.clone()
    }

    /// What the client half did, in order.
    pub fn deliveries(&self, conn: ConnId) -> Vec<ClientDelivery> {
        self.connections[conn_index(conn)].deliveries.clone()
    }

    /// Whether the connection is open.
    pub fn is_open(&self, conn: ConnId) -> bool {
        self.connections[conn_index(conn)].open
    }

    /// The error id the connection was closed with.
    pub fn close_code(&self, conn: ConnId) -> Option<String> {
        self.connections[conn_index(conn)].close_code.clone()
    }

    /// How the client process exited, when it did (HST-104).
    pub fn client_exit(&self, conn: ConnId) -> Option<ClientExit> {
        self.connections[conn_index(conn)].exit.clone()
    }

    /// The client half of a plugin on a connection (HST-105).
    pub fn client_half(&self, conn: ConnId, plugin: &str) -> ClientHalf {
        self.connections[conn_index(conn)]
            .halves
            .get(plugin)
            .cloned()
            .unwrap_or(NO_HALF)
    }

    /// Whether the server half of a plugin runs.
    pub fn server_half_running(&self, plugin: &str) -> bool {
        self.plugins.contains_key(plugin)
    }

    /// Open connections.
    pub fn connection_count(&self) -> usize {
        self.connections.iter().filter(|conn| conn.open).count()
    }

    /// Nodes in the server tree.
    pub fn server_tree_len(&self) -> usize {
        self.tree.len()
    }

    /// Local effects the client applied before the server confirmed (HST-112). The client
    /// forwards every submission and never applies its effects itself.
    pub fn client_local_effects(&self, _conn: ConnId) -> usize {
        0
    }

    /// Submits the client sent.
    pub fn submits_sent(&self, conn: ConnId) -> usize {
        self.connections[conn_index(conn)].submits_sent
    }

    /// Resync requests the client sent (HST-108).
    pub fn resync_requests(&self, conn: ConnId) -> usize {
        self.connections[conn_index(conn)].resync_requests
    }

    /// The contribution ids in the client's mirror.
    pub fn client_mirror(&self, conn: ConnId) -> Vec<String> {
        self.connections[conn_index(conn)]
            .mirror
            .iter()
            .cloned()
            .collect()
    }

    /// The contribution ids on the server.
    pub fn server_contributions(&self) -> Vec<String> {
        self.contributions.keys().cloned().collect()
    }

    /// The final outcome of a submission, when it completed.
    pub fn final_outcome(&self, client_id: &str, plugin: &str, dedup_key: &str) -> Option<String> {
        self.completed
            .get(&(
                client_id.to_owned(),
                plugin.to_owned(),
                dedup_key.to_owned(),
            ))
            .map(|(_, outcome)| outcome.clone())
    }

    /// PTY frames written to the connection.
    pub fn pty_frames_written(&self, conn: ConnId) -> usize {
        self.connections[conn_index(conn)].pty_written
    }

    /// PTY frames dropped on the connection.
    pub fn pty_frames_dropped(&self, conn: ConnId) -> usize {
        self.connections[conn_index(conn)].pty_dropped
    }

    /// The client submits an intent of a plugin.
    pub fn submit(&mut self, conn: ConnId, plugin: &str, dedup_key: &str, intent: &str) {
        let index = conn_index(conn);
        let connection = &mut self.connections[index];
        connection.submits_sent += 1;
        let submission = Submission {
            client_id: connection.client_id.clone(),
            plugin: plugin.to_owned(),
            dedup_key: dedup_key.to_owned(),
            intent: intent.to_owned(),
        };
        if connection.open {
            self.inbox.push(submission);
            return;
        }
        self.queue_detached(submission);
    }

    /// Pauses or resumes the server's apply step.
    pub fn pause_apply(&mut self, paused: bool) {
        self.apply_paused = paused;
    }

    /// The server-side plugin puts a contribution of the given encoded size.
    pub fn put_contribution(&mut self, _plugin: &str, id: &str, entry_bytes: usize) {
        self.contributions.insert(id.to_owned(), entry_bytes);
        for index in 0..self.connections.len() {
            if !self.connections[index].open {
                continue;
            }
            let mut remaining = entry_bytes;
            loop {
                let chunk = remaining.min(CONTRIBUTION_DELTA_MAX_BYTES);
                let delta = self.connections[index].new_delta(id, chunk);
                self.queue_plugin_record(index, chunk, Some(delta));
                remaining -= chunk;
                if remaining == 0 {
                    break;
                }
            }
        }
    }

    /// Loses the next increment sent to the connection (HST-108).
    pub fn drop_next_increment(&mut self, conn: ConnId) {
        self.connections[conn_index(conn)].drop_next = true;
    }

    /// The client sends an envelope with an explicit sequence number (HST-102).
    pub fn send_client_envelope(&mut self, conn: ConnId, seq: u64) {
        let index = conn_index(conn);
        if !self.connections[index].open {
            return;
        }
        let result = self.connections[index]
            .receiver
            .on_envelope(seq, ENVELOPE_LEN);
        if let Err(fault) = result {
            self.close_connection(index, fault.code());
        }
    }

    /// Floods plugin data toward a connection whose client reader is stalled (HST-121..123).
    pub fn flood_plugin_data(&mut self, conn: ConnId, record_bytes: usize, records: usize) {
        let index = conn_index(conn);
        for _ in 0..records {
            if !self.connections[index].open {
                break;
            }
            self.queue_plugin_record(index, record_bytes, None);
        }
    }

    /// Queues PTY frames toward a connection (HST-120).
    pub fn push_pty_frames(&mut self, conn: ConnId, count: usize) {
        let connection = &mut self.connections[conn_index(conn)];
        if !connection.open {
            connection.pty_dropped += count;
            return;
        }
        for _ in 0..count {
            let frame_id = connection.next_pty_id;
            connection.next_pty_id = connection.next_pty_id.wrapping_add(1);
            connection.writer.push_pty_frame(frame_id);
        }
    }
}

impl IpcHarness {
    fn run_round(&mut self) -> bool {
        self.expire_grace();
        let received = self.receive_submissions();
        let applied = self.apply_submissions();
        let written = self.drain_writers();
        received || applied || written
    }

    fn open_connection(
        &mut self,
        client_id: &str,
        hello: HelloSpec,
        last_seen_seq: Option<u64>,
    ) -> ConnId {
        let conn = ConnId(self.connections.len() as u32);
        let mut connection = Connection {
            client_id: client_id.to_owned(),
            ..Connection::default()
        };
        if hello.plugin_protocol != PLUGIN_PROTOCOL || hello.contract != CONTRACT {
            connection.close_code = Some(PROTOCOL_MISMATCH.to_owned());
            connection.wire.push(WireItem::Rejected {
                code: PROTOCOL_MISMATCH.to_owned(),
            });
            connection.exit = Some(ClientExit {
                code: EXIT_CODE_PROTOCOL_MISMATCH,
                message: PROTOCOL_MISMATCH_MESSAGE.to_owned(),
                interface_drawn: false,
            });
            self.connections.push(connection);
            return conn;
        }
        let replay = match last_seen_seq {
            Some(last_seen) => self.replay_outcomes(client_id, last_seen),
            None => Vec::new(),
        };
        let queued = self.resume_client(client_id, conn);
        connection.open = true;
        connection.halves = self.client_halves(&hello);
        connection.next_out_seq = 2;
        connection.expected_in_seq = 2;
        connection.wire.push(WireItem::Attached {
            plugin_set: self.plugins.keys().cloned().collect(),
        });
        connection.wire.extend(replay);
        let (server_ids, snapshot_bytes) = self.snapshot();
        connection.wire.push(WireItem::Snapshot {
            sequence: 1,
            bytes: snapshot_bytes,
        });
        connection.mirror = server_ids.into_iter().collect();
        self.connections.push(connection);
        self.inbox.extend(queued);
        conn
    }

    /// Points the client at its new connection and takes back its queued submissions.
    fn resume_client(&mut self, client_id: &str, conn: ConnId) -> Vec<Submission> {
        let client = self.clients.entry(client_id.to_owned()).or_default();
        client.conn = Some(conn);
        client.disconnected_at = None;
        client.detached = DetachedQueue::new();
        std::mem::take(&mut client.queued)
    }

    fn replay_outcomes(&mut self, client_id: &str, last_seen_seq: u64) -> Vec<WireItem> {
        let stored = self
            .outcomes
            .replay(client_id, last_seen_seq, self.clock_ms);
        stored
            .into_iter()
            .filter_map(|record| {
                let (plugin, dedup_key) = self
                    .outcome_details
                    .get(&(client_id.to_owned(), record.seq))?
                    .clone();
                Some(WireItem::Outcome {
                    seq: record.seq,
                    plugin,
                    dedup_key,
                    outcome: record.outcome,
                })
            })
            .collect()
    }

    /// Plugin set skew disables only the client half of the affected plugin (HST-105).
    fn client_halves(&self, hello: &HelloSpec) -> BTreeMap<String, ClientHalf> {
        let mut halves = BTreeMap::new();
        for (plugin_id, server_plugin) in &self.plugins {
            let half = match hello.installed.iter().find(|item| item.id == *plugin_id) {
                None => NO_HALF,
                Some(installed)
                    if installed.version != server_plugin.version
                        || installed.content_hash != server_plugin.content_hash =>
                {
                    ClientHalf {
                        running: false,
                        disabled_code: Some(VERSION_SKEW.to_owned()),
                    }
                }
                Some(_) => ClientHalf {
                    running: true,
                    disabled_code: None,
                },
            };
            halves.insert(plugin_id.clone(), half);
        }
        for installed in &hello.installed {
            if !self.plugins.contains_key(&installed.id) {
                halves.insert(
                    installed.id.clone(),
                    ClientHalf {
                        running: false,
                        disabled_code: Some(SERVER_MISSING.to_owned()),
                    },
                );
            }
        }
        halves
    }

    fn snapshot(&self) -> (Vec<String>, usize) {
        (
            self.contributions.keys().cloned().collect(),
            self.contributions.values().sum(),
        )
    }

    fn detach_client(&mut self, index: usize) {
        let client_id = self.connections[index].client_id.clone();
        let conn = ConnId(index as u32);
        let now = self.clock_ms;
        let Some(client) = self.clients.get_mut(&client_id) else {
            return;
        };
        if client.conn != Some(conn) {
            return;
        }
        client.conn = None;
        client.disconnected_at = Some(now);
        self.outcomes.on_disconnect(&client_id, now);
    }

    fn close_connection(&mut self, index: usize, code: &str) {
        let connection = &mut self.connections[index];
        connection.open = false;
        connection.close_code = Some(code.to_owned());
        connection.wire.push(WireItem::Rejected {
            code: code.to_owned(),
        });
        self.detach_client(index);
    }

    /// Frees the clients whose grace period has run out: their unanswered submissions are
    /// cancelled (HST-118, HST-119).
    fn expire_grace(&mut self) {
        let now = self.clock_ms;
        let expired: Vec<String> = self
            .clients
            .iter()
            .filter(|(_, client)| {
                client.conn.is_none()
                    && client
                        .disconnected_at
                        .is_some_and(|at| now >= at.saturating_add(OUTCOME_GRACE_MS))
            })
            .map(|(client_id, _)| client_id.clone())
            .collect();
        for client_id in expired {
            let queued = match self.clients.get_mut(&client_id) {
                Some(client) => {
                    client.disconnected_at = None;
                    client.detached.on_client_exit();
                    std::mem::take(&mut client.queued)
                }
                None => continue,
            };
            let mut stranded = split_off_client(&mut self.inbox, &client_id);
            stranded.extend(split_off_client(&mut self.unapplied, &client_id));
            for submission in stranded.into_iter().chain(queued) {
                self.complete(
                    &submission.client_id,
                    &submission.plugin,
                    &submission.dedup_key,
                    CANCELLED_OUTCOME,
                );
            }
        }
    }

    fn queue_detached(&mut self, submission: Submission) {
        let client = self
            .clients
            .entry(submission.client_id.clone())
            .or_default();
        match client.detached.push(&submission.dedup_key) {
            QueueVerdict::Queued => client.queued.push(submission),
            QueueVerdict::RefusedBusy => {
                self.complete(
                    &submission.client_id,
                    &submission.plugin,
                    &submission.dedup_key,
                    REFUSED_BUSY_OUTCOME,
                );
            }
        }
    }

    /// Admits the received submissions; a repeat within the window is answered from the table.
    fn receive_submissions(&mut self) -> bool {
        let received = std::mem::take(&mut self.inbox);
        let progressed = !received.is_empty();
        for submission in received {
            let verdict = self.dedup.admit(
                &submission.plugin,
                &submission.client_id,
                &submission.dedup_key,
                self.clock_ms,
            );
            match verdict {
                DedupVerdict::Fresh => self.unapplied.push(submission),
                DedupVerdict::Duplicate => self.replay_known_outcome(&submission),
            }
        }
        progressed
    }

    fn replay_known_outcome(&mut self, submission: &Submission) {
        let key = (
            submission.client_id.clone(),
            submission.plugin.clone(),
            submission.dedup_key.clone(),
        );
        let Some((seq, outcome)) = self.completed.get(&key).cloned() else {
            return;
        };
        let item = WireItem::Outcome {
            seq,
            plugin: submission.plugin.clone(),
            dedup_key: submission.dedup_key.clone(),
            outcome,
        };
        self.send_to_client(&submission.client_id, item);
    }

    fn send_to_client(&mut self, client_id: &str, item: WireItem) {
        let Some(conn) = self.clients.get(client_id).and_then(|client| client.conn) else {
            return;
        };
        self.connections[conn_index(conn)].wire.push(item);
    }

    fn apply_submissions(&mut self) -> bool {
        if self.apply_paused {
            return false;
        }
        let pending = std::mem::take(&mut self.unapplied);
        let progressed = !pending.is_empty();
        for submission in pending {
            self.apply(submission);
        }
        progressed
    }

    /// Applies one submission. The grant is checked now, not when it was sent (HST-114).
    fn apply(&mut self, submission: Submission) {
        if submission.intent != CREATE_PANE_INTENT {
            self.complete(
                &submission.client_id,
                &submission.plugin,
                &submission.dedup_key,
                UNKNOWN_INTENT_OUTCOME,
            );
            return;
        }
        let granted = self
            .plugins
            .get(&submission.plugin)
            .is_some_and(|server_plugin| server_plugin.grants.contains(PANE_GRANT));
        if !granted {
            self.complete(
                &submission.client_id,
                &submission.plugin,
                &submission.dedup_key,
                REVOKED_OUTCOME,
            );
            return;
        }
        let node = format!("pane-{}", self.tree.len() + 1);
        self.tree.push(node.clone());
        for connection in self.connections.iter_mut().filter(|conn| conn.open) {
            connection.tree_changed(&node);
        }
        self.complete(
            &submission.client_id,
            &submission.plugin,
            &submission.dedup_key,
            OK_OUTCOME,
        );
    }

    /// Records the outcome, then sends it and the request-completed event to the client if
    /// it is attached (HST-115).
    fn complete(&mut self, client_id: &str, plugin: &str, dedup_key: &str, outcome: &str) {
        let client = self.clients.entry(client_id.to_owned()).or_default();
        client.outcome_seq += 1;
        let seq = client.outcome_seq;
        let conn = client.conn;
        self.outcomes.record(client_id, seq, outcome, self.clock_ms);
        self.outcome_details.insert(
            (client_id.to_owned(), seq),
            (plugin.to_owned(), dedup_key.to_owned()),
        );
        self.completed.insert(
            (
                client_id.to_owned(),
                plugin.to_owned(),
                dedup_key.to_owned(),
            ),
            (seq, outcome.to_owned()),
        );
        let Some(conn) = conn else {
            return;
        };
        let connection = &mut self.connections[conn_index(conn)];
        connection.wire.push(WireItem::Outcome {
            seq,
            plugin: plugin.to_owned(),
            dedup_key: dedup_key.to_owned(),
            outcome: outcome.to_owned(),
        });
        connection
            .deliveries
            .push(ClientDelivery::RequestCompleted {
                dedup_key: dedup_key.to_owned(),
            });
    }

    /// Queues a plugin record on one connection and applies the back-pressure outcome
    /// (HST-121, HST-122).
    fn queue_plugin_record(&mut self, index: usize, bytes: usize, delta: Option<Delta>) {
        let (server_ids, snapshot_bytes) = self.snapshot();
        let connection = &mut self.connections[index];
        let record_id = connection.next_record_id;
        connection.next_record_id = connection.next_record_id.wrapping_add(1);
        if let Some(delta) = delta {
            connection.outbound.insert(record_id, delta);
        }
        let outcome = connection.writer.push_plugin(PluginRecord {
            id: record_id,
            bytes,
            coalesce_key: None,
        });
        match outcome {
            PushOutcome::Queued | PushOutcome::Merged => {}
            PushOutcome::ResyncFlagged => {
                if !connection.overflow_resync {
                    connection.overflow_resync = true;
                    connection.resync(&server_ids, snapshot_bytes);
                }
            }
            PushOutcome::Disconnect(fault) => self.close_connection(index, fault.code()),
        }
    }

    /// Writes PTY frames before plugin records, then delivers the contribution deltas.
    fn drain_writers(&mut self) -> bool {
        let (server_ids, snapshot_bytes) = self.snapshot();
        let mut progressed = false;
        for connection in self.connections.iter_mut() {
            if !connection.open {
                continue;
            }
            while let Some(written) = connection.writer.next() {
                progressed = true;
                match written {
                    Written::Pty(_) => connection.pty_written += 1,
                    Written::Plugin(record_id) => {
                        if let Some(delta) = connection.outbound.remove(&record_id) {
                            connection.deliver(delta, &server_ids, snapshot_bytes);
                        }
                    }
                }
            }
            if connection.writer.plugin_queued_records() == 0 {
                connection.overflow_resync = false;
            }
        }
        progressed
    }
}

#[cfg(test)]
mod tests {
    use super::{ClientDelivery, ConnId, HelloSpec, InstalledSpec, IpcHarness, WireItem};
    use std::time::Duration;

    const MIB: usize = 1024 * 1024;
    const PLUGIN: &str = "pdf";
    const PANE_GRANT: &str = "panes.create";
    const CREATE_PANE_INTENT: &str = "create-pane";
    const OK_OUTCOME: &str = "ok-node";
    const PROTOCOL_MISMATCH: &str = "ipc.protocol-mismatch";
    const SEQUENCE_GAP: &str = "ipc.sequence-gap";
    const CLIENT_TOO_SLOW: &str = "ipc.client-too-slow";
    const VERSION_SKEW: &str = "plugin.version-skew";
    const SERVER_MISSING: &str = "plugin.server-missing";
    const MAX_DELTA_BYTES: usize = 4 * MIB;
    const PLUGIN_QUEUE_RECORD_LIMIT: usize = 4096;
    const DETACHED_QUEUE_LIMIT: usize = 256;
    const OUTCOME_RECORD_LIMIT: usize = 1024;
    const GRACE_PERIOD_MS: u64 = 30_000;
    const DEDUP_WINDOW_MS: u64 = 600_000;
    const CLIENT_ID: &str = "client-a";

    fn harness_with_plugin() -> IpcHarness {
        let mut harness = IpcHarness::new();
        harness.install_server_plugin(PLUGIN, "1.0.0", [7; 32]);
        harness.grant(PLUGIN, PANE_GRANT);
        harness
    }

    fn connect_matching(harness: &mut IpcHarness, client_id: &str) -> ConnId {
        let hello = harness.matching_hello();
        harness.connect(client_id, true, hello)
    }

    fn hello_with_installed(harness: &IpcHarness, installed: Vec<InstalledSpec>) -> HelloSpec {
        let mut hello = harness.matching_hello();
        hello.installed = installed;
        hello
    }

    fn installed_spec(id: &str, version: &str, content_hash: [u8; 32]) -> InstalledSpec {
        InstalledSpec {
            id: id.to_string(),
            version: version.to_string(),
            content_hash,
        }
    }

    fn outcome_seqs(items: &[WireItem]) -> Vec<u64> {
        items
            .iter()
            .filter_map(|item| match item {
                WireItem::Outcome { seq, .. } => Some(*seq),
                _ => None,
            })
            .collect()
    }

    fn tree_node_ids(items: &[WireItem]) -> Vec<String> {
        items
            .iter()
            .filter_map(|item| match item {
                WireItem::TreeChange { node } => Some(node.clone()),
                _ => None,
            })
            .collect()
    }

    fn increment_sequences(items: &[WireItem]) -> Vec<u64> {
        items
            .iter()
            .filter_map(|item| match item {
                WireItem::Increment { sequence, .. } => Some(*sequence),
                _ => None,
            })
            .collect()
    }

    fn increment_bytes(items: &[WireItem]) -> Vec<usize> {
        items
            .iter()
            .filter_map(|item| match item {
                WireItem::Increment { bytes, .. } => Some(*bytes),
                _ => None,
            })
            .collect()
    }

    fn snapshot_count(items: &[WireItem]) -> usize {
        items
            .iter()
            .filter(|item| matches!(item, WireItem::Snapshot { .. }))
            .count()
    }

    fn outcome_count(items: &[WireItem]) -> usize {
        items
            .iter()
            .filter(|item| matches!(item, WireItem::Outcome { .. }))
            .count()
    }

    #[test]
    fn new_harness_has_no_connections_tree_or_contributions() {
        let harness = IpcHarness::new();
        assert_eq!(harness.connection_count(), 0);
        assert_eq!(harness.server_tree_len(), 0);
        assert!(harness.server_contributions().is_empty());
    }

    /// REQ: HST-103
    #[test]
    fn matching_hello_speaks_protocol_one_and_contract_0_1_0() {
        let harness = harness_with_plugin();
        let hello = harness.matching_hello();
        assert_eq!(hello.plugin_protocol, 1);
        assert_eq!(hello.contract, "0.1.0");
        assert_eq!(
            hello.installed,
            vec![installed_spec(PLUGIN, "1.0.0", [7; 32])]
        );
    }

    /// REQ: HST-103
    #[test]
    fn matching_hello_of_empty_server_lists_no_plugins() {
        let harness = IpcHarness::new();
        assert!(harness.matching_hello().installed.is_empty());
    }

    /// REQ: HST-103
    #[test]
    fn matching_client_is_attached_with_server_plugin_set() {
        let mut harness = harness_with_plugin();
        let conn = connect_matching(&mut harness, CLIENT_ID);
        harness.pump(8);
        assert!(harness.is_open(conn));
        assert_eq!(harness.close_code(conn), None);
        assert_eq!(
            harness.wire(conn).first(),
            Some(&WireItem::Attached {
                plugin_set: vec![PLUGIN.to_string()],
            })
        );
    }

    /// REQ: HST-103
    #[test]
    fn attach_to_empty_server_reports_empty_plugin_set() {
        let mut harness = IpcHarness::new();
        let conn = connect_matching(&mut harness, CLIENT_ID);
        harness.pump(8);
        assert_eq!(
            harness.wire(conn).first(),
            Some(&WireItem::Attached {
                plugin_set: Vec::new(),
            })
        );
    }

    /// REQ: HST-103
    #[test]
    fn protocol_version_mismatch_is_rejected_and_closed() {
        let mut harness = harness_with_plugin();
        let mut hello = harness.matching_hello();
        hello.plugin_protocol = 2;
        let conn = harness.connect(CLIENT_ID, true, hello);
        harness.pump(8);
        assert!(!harness.is_open(conn));
        assert_eq!(harness.close_code(conn).as_deref(), Some(PROTOCOL_MISMATCH));
        assert!(harness.wire(conn).iter().any(|item| {
            matches!(item, WireItem::Rejected { code } if code.as_str() == PROTOCOL_MISMATCH)
        }));
    }

    /// REQ: HST-103
    #[test]
    fn contract_mismatch_is_rejected_and_closed() {
        let mut harness = harness_with_plugin();
        let mut hello = harness.matching_hello();
        hello.contract = "0.2.0".to_string();
        let conn = harness.connect(CLIENT_ID, true, hello);
        harness.pump(8);
        assert!(!harness.is_open(conn));
        assert_eq!(harness.close_code(conn).as_deref(), Some(PROTOCOL_MISMATCH));
    }

    /// REQ: HST-104
    #[test]
    fn protocol_mismatch_exits_with_code_76_before_drawing() {
        let mut harness = harness_with_plugin();
        let mut hello = harness.matching_hello();
        hello.plugin_protocol = 2;
        let conn = harness.connect(CLIENT_ID, true, hello);
        harness.pump(8);
        let exit = harness
            .client_exit(conn)
            .expect("a mismatched client exits");
        assert_eq!(exit.code, 76);
        assert!(!exit.interface_drawn);
        assert!(!exit.message.is_empty());
    }

    /// REQ: HST-104
    #[test]
    fn matching_client_has_no_exit_record() {
        let mut harness = harness_with_plugin();
        let conn = connect_matching(&mut harness, CLIENT_ID);
        harness.pump(8);
        assert_eq!(harness.client_exit(conn), None);
    }

    /// REQ: HST-105
    #[test]
    fn version_skew_disables_only_the_client_half_of_that_plugin() {
        let mut harness = harness_with_plugin();
        harness.install_server_plugin("notes", "1.0.0", [9; 32]);
        let hello = hello_with_installed(
            &harness,
            vec![
                installed_spec(PLUGIN, "1.0.1", [7; 32]),
                installed_spec("notes", "1.0.0", [9; 32]),
            ],
        );
        let conn = harness.connect(CLIENT_ID, true, hello);
        harness.pump(8);
        let skewed = harness.client_half(conn, PLUGIN);
        assert!(!skewed.running);
        assert_eq!(skewed.disabled_code.as_deref(), Some(VERSION_SKEW));
        assert!(harness.client_half(conn, "notes").running);
        assert!(harness.server_half_running(PLUGIN));
        assert!(harness.is_open(conn));
    }

    /// REQ: HST-105
    #[test]
    fn hash_skew_with_equal_version_also_disables_client_half() {
        let mut harness = harness_with_plugin();
        let hello = hello_with_installed(&harness, vec![installed_spec(PLUGIN, "1.0.0", [8; 32])]);
        let conn = harness.connect(CLIENT_ID, true, hello);
        harness.pump(8);
        let half = harness.client_half(conn, PLUGIN);
        assert!(!half.running);
        assert_eq!(half.disabled_code.as_deref(), Some(VERSION_SKEW));
    }

    /// REQ: HST-105
    #[test]
    fn client_only_plugin_is_disabled_as_server_missing() {
        let mut harness = harness_with_plugin();
        let hello = hello_with_installed(
            &harness,
            vec![
                installed_spec(PLUGIN, "1.0.0", [7; 32]),
                installed_spec("ghost", "0.1.0", [1; 32]),
            ],
        );
        let conn = harness.connect(CLIENT_ID, true, hello);
        harness.pump(8);
        let ghost = harness.client_half(conn, "ghost");
        assert!(!ghost.running);
        assert_eq!(ghost.disabled_code.as_deref(), Some(SERVER_MISSING));
        assert!(harness.server_half_running(PLUGIN));
    }

    /// REQ: HST-105
    #[test]
    fn server_only_plugin_keeps_running_without_client_half() {
        let mut harness = harness_with_plugin();
        harness.install_server_plugin("tray", "1.0.0", [4; 32]);
        let hello = hello_with_installed(&harness, vec![installed_spec(PLUGIN, "1.0.0", [7; 32])]);
        let conn = harness.connect(CLIENT_ID, true, hello);
        harness.pump(8);
        assert!(harness.is_open(conn));
        assert!(harness.server_half_running("tray"));
        let tray = harness.client_half(conn, "tray");
        assert!(!tray.running);
        assert_eq!(tray.disabled_code, None);
    }

    /// REQ: HST-100
    #[test]
    fn plugin_traffic_reuses_the_attached_connection() {
        let mut harness = harness_with_plugin();
        let conn = connect_matching(&mut harness, CLIENT_ID);
        harness.submit(conn, PLUGIN, "key-1", CREATE_PANE_INTENT);
        harness.pump(8);
        assert_eq!(harness.connection_count(), 1);
        assert!(!harness.wire(conn).is_empty());
    }

    /// REQ: HST-102
    #[test]
    fn consecutive_client_envelopes_keep_the_connection_open() {
        let mut harness = harness_with_plugin();
        let conn = connect_matching(&mut harness, CLIENT_ID);
        harness.send_client_envelope(conn, 1);
        harness.send_client_envelope(conn, 2);
        harness.pump(8);
        assert!(harness.is_open(conn));
        assert_eq!(harness.close_code(conn), None);
    }

    /// REQ: HST-102
    #[test]
    fn envelope_sequence_gap_closes_with_sequence_gap() {
        let mut harness = harness_with_plugin();
        let conn = connect_matching(&mut harness, CLIENT_ID);
        harness.send_client_envelope(conn, 1);
        harness.send_client_envelope(conn, 2);
        harness.pump(8);
        harness.send_client_envelope(conn, 4);
        harness.pump(8);
        assert!(!harness.is_open(conn));
        assert_eq!(harness.close_code(conn).as_deref(), Some(SEQUENCE_GAP));
    }

    /// REQ: HST-102
    #[test]
    fn first_envelope_must_have_sequence_one() {
        let mut harness = harness_with_plugin();
        let conn = connect_matching(&mut harness, CLIENT_ID);
        harness.send_client_envelope(conn, 2);
        harness.pump(8);
        assert_eq!(harness.close_code(conn).as_deref(), Some(SEQUENCE_GAP));
    }

    /// REQ: HST-107
    #[test]
    fn first_contribution_after_attach_is_snapshot_with_sequence_one() {
        let mut harness = harness_with_plugin();
        harness.put_contribution(PLUGIN, "status-1", 64);
        let conn = connect_matching(&mut harness, CLIENT_ID);
        harness.pump(8);
        let items = harness.wire(conn);
        let first_contribution = items
            .iter()
            .find(|item| matches!(item, WireItem::Snapshot { .. } | WireItem::Increment { .. }));
        assert!(matches!(
            first_contribution,
            Some(WireItem::Snapshot { sequence: 1, .. })
        ));
    }

    /// REQ: HST-107
    #[test]
    fn later_contributions_are_increments_with_consecutive_sequences() {
        let mut harness = harness_with_plugin();
        let conn = connect_matching(&mut harness, CLIENT_ID);
        harness.pump(8);
        harness.put_contribution(PLUGIN, "c1", 32);
        harness.put_contribution(PLUGIN, "c2", 32);
        harness.pump(8);
        let sequences = increment_sequences(&harness.wire(conn));
        assert!(!sequences.is_empty());
        assert_eq!(sequences[0], 2);
        assert!(sequences.windows(2).all(|pair| pair[1] == pair[0] + 1));
    }

    /// REQ: HST-108
    #[test]
    fn sequence_gap_in_deltas_triggers_resync_with_full_snapshot() {
        let mut harness = harness_with_plugin();
        let conn = connect_matching(&mut harness, CLIENT_ID);
        harness.pump(8);
        harness.drop_next_increment(conn);
        harness.put_contribution(PLUGIN, "c1", 32);
        harness.put_contribution(PLUGIN, "c2", 32);
        harness.pump(8);
        assert_eq!(harness.resync_requests(conn), 1);
        assert!(snapshot_count(&harness.wire(conn)) >= 2);
        let mirror = harness.client_mirror(conn);
        assert!(mirror.contains(&"c1".to_string()));
        assert!(mirror.contains(&"c2".to_string()));
    }

    /// REQ: HST-109
    #[test]
    fn oversized_contribution_is_split_into_deltas_of_at_most_4_mib() {
        let mut harness = harness_with_plugin();
        let conn = connect_matching(&mut harness, CLIENT_ID);
        harness.pump(8);
        harness.put_contribution(PLUGIN, "big", 9 * MIB);
        harness.pump(8);
        let bytes = increment_bytes(&harness.wire(conn));
        assert!(bytes.len() >= 3);
        assert!(bytes.iter().all(|size| *size <= MAX_DELTA_BYTES));
    }

    /// REQ: HST-109
    #[test]
    fn contribution_of_exactly_4_mib_is_one_delta() {
        let mut harness = harness_with_plugin();
        let conn = connect_matching(&mut harness, CLIENT_ID);
        harness.pump(8);
        harness.put_contribution(PLUGIN, "edge", MAX_DELTA_BYTES);
        harness.pump(8);
        assert_eq!(increment_bytes(&harness.wire(conn)), vec![MAX_DELTA_BYTES]);
    }

    /// REQ: HST-110
    #[test]
    fn tree_change_is_sent_before_the_outcome_of_its_submission() {
        let mut harness = harness_with_plugin();
        let conn = connect_matching(&mut harness, CLIENT_ID);
        harness.submit(conn, PLUGIN, "key-1", CREATE_PANE_INTENT);
        harness.pump(8);
        let items = harness.wire(conn);
        let tree_index = items
            .iter()
            .position(|item| matches!(item, WireItem::TreeChange { .. }))
            .expect("the created pane sends a tree change");
        let outcome_index = items
            .iter()
            .position(|item| matches!(item, WireItem::Outcome { .. }))
            .expect("the submission sends an outcome");
        assert!(tree_index < outcome_index);
    }

    /// REQ: HST-111
    #[test]
    fn mirror_update_precedes_tree_event_which_precedes_request_completed() {
        let mut harness = harness_with_plugin();
        let conn = connect_matching(&mut harness, CLIENT_ID);
        harness.submit(conn, PLUGIN, "key-1", CREATE_PANE_INTENT);
        harness.pump(8);
        let deliveries = harness.deliveries(conn);
        let mirror_index = deliveries
            .iter()
            .position(|item| matches!(item, ClientDelivery::MirrorUpdated { .. }))
            .expect("the mirror is updated");
        let event_index = deliveries
            .iter()
            .position(|item| matches!(item, ClientDelivery::TreeChangedEvent { .. }))
            .expect("the plugin receives tree-changed");
        let completed_index = deliveries
            .iter()
            .position(|item| matches!(item, ClientDelivery::RequestCompleted { .. }))
            .expect("the request completes");
        assert!(mirror_index < event_index);
        assert!(event_index < completed_index);
    }

    /// REQ: HST-111
    #[test]
    fn request_completed_is_delivered_only_after_the_created_node_is_mirrored() {
        let mut harness = harness_with_plugin();
        let conn = connect_matching(&mut harness, CLIENT_ID);
        harness.submit(conn, PLUGIN, "key-1", CREATE_PANE_INTENT);
        harness.pump(8);
        let nodes = tree_node_ids(&harness.wire(conn));
        let node = nodes
            .first()
            .expect("the created pane is sent as a tree change");
        assert!(harness.client_mirror(conn).contains(node));
    }

    /// REQ: HST-112
    #[test]
    fn client_forwards_submission_and_applies_no_local_effect() {
        let mut harness = harness_with_plugin();
        let conn = connect_matching(&mut harness, CLIENT_ID);
        harness.submit(conn, PLUGIN, "key-1", CREATE_PANE_INTENT);
        assert_eq!(harness.submits_sent(conn), 1);
        assert_eq!(harness.client_local_effects(conn), 0);
        harness.pump(8);
        assert_eq!(harness.client_local_effects(conn), 0);
        assert_eq!(harness.server_tree_len(), 1);
    }

    /// REQ: HST-115
    #[test]
    fn each_submit_is_answered_by_exactly_one_outcome() {
        let mut harness = harness_with_plugin();
        let conn = connect_matching(&mut harness, CLIENT_ID);
        for key in ["key-1", "key-2", "key-3"] {
            harness.submit(conn, PLUGIN, key, CREATE_PANE_INTENT);
        }
        harness.pump(16);
        assert_eq!(outcome_count(&harness.wire(conn)), 3);
        for key in ["key-1", "key-2", "key-3"] {
            assert_eq!(
                harness.final_outcome(CLIENT_ID, PLUGIN, key).as_deref(),
                Some(OK_OUTCOME)
            );
        }
    }

    /// REQ: HST-113
    #[test]
    fn same_key_within_dedup_window_is_answered_from_the_table() {
        let mut harness = harness_with_plugin();
        let conn = connect_matching(&mut harness, CLIENT_ID);
        harness.submit(conn, PLUGIN, "key-1", CREATE_PANE_INTENT);
        harness.pump(8);
        harness.advance(Duration::from_millis(DEDUP_WINDOW_MS - 1));
        harness.submit(conn, PLUGIN, "key-1", CREATE_PANE_INTENT);
        harness.pump(8);
        assert_eq!(harness.server_tree_len(), 1);
    }

    /// REQ: HST-113
    #[test]
    fn key_is_free_again_at_exactly_600_000_ms() {
        let mut harness = harness_with_plugin();
        let conn = connect_matching(&mut harness, CLIENT_ID);
        harness.submit(conn, PLUGIN, "key-1", CREATE_PANE_INTENT);
        harness.pump(8);
        harness.advance(Duration::from_millis(DEDUP_WINDOW_MS));
        harness.submit(conn, PLUGIN, "key-1", CREATE_PANE_INTENT);
        harness.pump(8);
        assert_eq!(harness.server_tree_len(), 2);
    }

    /// REQ: HST-113
    #[test]
    fn same_key_from_two_clients_is_two_submissions() {
        let mut harness = harness_with_plugin();
        let first = connect_matching(&mut harness, "client-a");
        let second = connect_matching(&mut harness, "client-b");
        harness.submit(first, PLUGIN, "key-1", CREATE_PANE_INTENT);
        harness.submit(second, PLUGIN, "key-1", CREATE_PANE_INTENT);
        harness.pump(8);
        assert_eq!(harness.server_tree_len(), 2);
        assert_eq!(
            harness
                .final_outcome("client-a", PLUGIN, "key-1")
                .as_deref(),
            Some(OK_OUTCOME)
        );
        assert_eq!(
            harness
                .final_outcome("client-b", PLUGIN, "key-1")
                .as_deref(),
            Some(OK_OUTCOME)
        );
    }

    /// REQ: HST-114
    #[test]
    fn grant_revoked_before_application_completes_as_revoked() {
        let mut harness = harness_with_plugin();
        let conn = connect_matching(&mut harness, CLIENT_ID);
        harness.submit(conn, PLUGIN, "key-1", CREATE_PANE_INTENT);
        harness.revoke(PLUGIN, PANE_GRANT);
        harness.pump(8);
        assert_eq!(
            harness.final_outcome(CLIENT_ID, PLUGIN, "key-1").as_deref(),
            Some("revoked")
        );
        assert_eq!(harness.server_tree_len(), 0);
    }

    /// REQ: HST-117
    #[test]
    fn reattach_replays_outcomes_newer_than_last_seen_then_snapshot() {
        let mut harness = harness_with_plugin();
        let conn = connect_matching(&mut harness, CLIENT_ID);
        harness.submit(conn, PLUGIN, "key-1", CREATE_PANE_INTENT);
        harness.pump(8);
        harness.disconnect(conn);
        let hello = harness.matching_hello();
        let reattached = harness.reattach(CLIENT_ID, 0, hello);
        harness.pump(8);
        let items = harness.wire(reattached);
        assert_eq!(outcome_seqs(&items), vec![1]);
        let outcome_index = items
            .iter()
            .position(|item| matches!(item, WireItem::Outcome { .. }))
            .expect("the outcome is replayed");
        let snapshot_index = items
            .iter()
            .position(|item| matches!(item, WireItem::Snapshot { .. }))
            .expect("a snapshot follows the replay");
        assert!(outcome_index < snapshot_index);
    }

    /// REQ: HST-117
    #[test]
    fn reattach_skips_outcomes_the_client_already_saw() {
        let mut harness = harness_with_plugin();
        let conn = connect_matching(&mut harness, CLIENT_ID);
        harness.submit(conn, PLUGIN, "key-1", CREATE_PANE_INTENT);
        harness.submit(conn, PLUGIN, "key-2", CREATE_PANE_INTENT);
        harness.pump(8);
        harness.disconnect(conn);
        let hello = harness.matching_hello();
        let reattached = harness.reattach(CLIENT_ID, 1, hello);
        harness.pump(8);
        assert_eq!(outcome_seqs(&harness.wire(reattached)), vec![2]);
    }

    /// REQ: HST-116
    #[test]
    fn outcome_records_survive_inside_the_30_second_grace_period() {
        let mut harness = harness_with_plugin();
        let conn = connect_matching(&mut harness, CLIENT_ID);
        harness.submit(conn, PLUGIN, "key-1", CREATE_PANE_INTENT);
        harness.pump(8);
        harness.disconnect(conn);
        harness.advance(Duration::from_millis(GRACE_PERIOD_MS - 1_000));
        let hello = harness.matching_hello();
        let reattached = harness.reattach(CLIENT_ID, 0, hello);
        harness.pump(8);
        assert_eq!(outcome_seqs(&harness.wire(reattached)), vec![1]);
    }

    /// REQ: HST-116
    #[test]
    fn replay_is_capped_at_1024_outcome_records() {
        let mut harness = harness_with_plugin();
        let conn = connect_matching(&mut harness, CLIENT_ID);
        for index in 0..=OUTCOME_RECORD_LIMIT {
            harness.submit(conn, PLUGIN, &format!("key-{index}"), CREATE_PANE_INTENT);
        }
        harness.pump(4096);
        harness.disconnect(conn);
        let hello = harness.matching_hello();
        let reattached = harness.reattach(CLIENT_ID, 0, hello);
        harness.pump(8);
        assert_eq!(
            outcome_count(&harness.wire(reattached)),
            OUTCOME_RECORD_LIMIT
        );
    }

    /// REQ: HST-118
    #[test]
    fn unapplied_submission_is_cancelled_after_grace_period() {
        let mut harness = harness_with_plugin();
        harness.pause_apply(true);
        let conn = connect_matching(&mut harness, CLIENT_ID);
        harness.submit(conn, PLUGIN, "key-1", CREATE_PANE_INTENT);
        harness.disconnect(conn);
        harness.advance(Duration::from_millis(GRACE_PERIOD_MS + 1));
        harness.pause_apply(false);
        let hello = harness.matching_hello();
        let _reattached = harness.reattach(CLIENT_ID, 0, hello);
        harness.pump(8);
        assert_eq!(
            harness.final_outcome(CLIENT_ID, PLUGIN, "key-1").as_deref(),
            Some("cancelled")
        );
        assert_eq!(harness.server_tree_len(), 0);
    }

    /// REQ: HST-119
    #[test]
    fn detached_client_refuses_the_257th_queued_submission_at_once() {
        let mut harness = harness_with_plugin();
        let conn = connect_matching(&mut harness, CLIENT_ID);
        harness.disconnect(conn);
        for index in 0..=DETACHED_QUEUE_LIMIT {
            harness.submit(conn, PLUGIN, &format!("key-{index}"), CREATE_PANE_INTENT);
        }
        harness.pump(8);
        assert_eq!(
            harness
                .final_outcome(CLIENT_ID, PLUGIN, &format!("key-{DETACHED_QUEUE_LIMIT}"))
                .as_deref(),
            Some("refused(busy)")
        );
        assert_eq!(harness.final_outcome(CLIENT_ID, PLUGIN, "key-0"), None);
    }

    /// REQ: HST-120
    #[test]
    fn pty_frames_are_all_written_while_plugin_data_is_queued() {
        let mut harness = harness_with_plugin();
        let conn = connect_matching(&mut harness, CLIENT_ID);
        harness.pump(8);
        harness.put_contribution(PLUGIN, "status-1", 64);
        harness.push_pty_frames(conn, 8);
        harness.pump(8);
        assert_eq!(harness.pty_frames_written(conn), 8);
        assert_eq!(harness.pty_frames_dropped(conn), 0);
        assert!(!increment_sequences(&harness.wire(conn)).is_empty());
    }

    /// REQ: HST-121
    #[test]
    fn plugin_queue_at_4096_records_does_not_force_resync() {
        let mut harness = harness_with_plugin();
        let conn = connect_matching(&mut harness, CLIENT_ID);
        harness.pump(8);
        harness.flood_plugin_data(conn, 1, PLUGIN_QUEUE_RECORD_LIMIT);
        harness.pump(8);
        assert_eq!(harness.resync_requests(conn), 0);
        assert!(harness.is_open(conn));
    }

    /// REQ: HST-121
    #[test]
    fn plugin_queue_over_4096_records_flags_resync() {
        let mut harness = harness_with_plugin();
        let conn = connect_matching(&mut harness, CLIENT_ID);
        harness.pump(8);
        harness.flood_plugin_data(conn, 1, PLUGIN_QUEUE_RECORD_LIMIT + 1);
        harness.pump(8);
        assert!(harness.resync_requests(conn) >= 1);
        assert!(harness.is_open(conn));
    }

    /// REQ: HST-121
    #[test]
    fn plugin_queue_over_4_mib_flags_resync_without_disconnect() {
        let mut harness = harness_with_plugin();
        let conn = connect_matching(&mut harness, CLIENT_ID);
        harness.pump(8);
        harness.flood_plugin_data(conn, MIB, 5);
        harness.pump(8);
        assert!(harness.resync_requests(conn) >= 1);
        assert!(harness.is_open(conn));
    }

    /// REQ: HST-122
    #[test]
    fn client_with_more_than_16_mib_queued_is_disconnected_too_slow() {
        let mut harness = harness_with_plugin();
        let conn = connect_matching(&mut harness, CLIENT_ID);
        harness.pump(8);
        harness.flood_plugin_data(conn, MIB, 17);
        harness.pump(8);
        assert!(!harness.is_open(conn));
        assert_eq!(harness.close_code(conn).as_deref(), Some(CLIENT_TOO_SLOW));
    }

    /// REQ: HST-122
    #[test]
    fn client_with_exactly_16_mib_queued_stays_connected() {
        let mut harness = harness_with_plugin();
        let conn = connect_matching(&mut harness, CLIENT_ID);
        harness.pump(8);
        harness.flood_plugin_data(conn, MIB, 16);
        harness.pump(8);
        assert!(harness.is_open(conn));
        assert_eq!(harness.close_code(conn), None);
    }

    /// REQ: HST-123
    #[test]
    fn plugin_burst_does_not_delay_or_drop_a_pty_frame() {
        let mut harness = harness_with_plugin();
        let conn = connect_matching(&mut harness, CLIENT_ID);
        harness.pump(8);
        harness.flood_plugin_data(conn, MIB, 4);
        harness.push_pty_frames(conn, 1);
        harness.pump(8);
        assert_eq!(harness.pty_frames_written(conn), 1);
        assert_eq!(harness.pty_frames_dropped(conn), 0);
    }
}
