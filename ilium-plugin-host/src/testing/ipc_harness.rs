//! An in-process server plus client halves around a `FakeKernel`, driven by hand, for the
//! IPC rules of reference-host-ipc.md (HST-100..125). The grant `panes.create` gates the intent
//! `create-pane` (outcome `ok-node`).

use std::time::Duration;

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
pub struct IpcHarness;

impl IpcHarness {
    /// An empty server with no plugins and no connections.
    pub fn new() -> IpcHarness {
        unimplemented!("host::testing::ipc_harness::IpcHarness::new")
    }

    /// Installs a plugin on the server.
    pub fn install_server_plugin(&mut self, _id: &str, _version: &str, _content_hash: [u8; 32]) {
        unimplemented!("host::testing::ipc_harness::IpcHarness::install_server_plugin")
    }

    /// Grants a plugin a grant.
    pub fn grant(&mut self, _plugin: &str, _grant: &str) {
        unimplemented!("host::testing::ipc_harness::IpcHarness::grant")
    }

    /// Revokes a grant of a plugin.
    pub fn revoke(&mut self, _plugin: &str, _grant: &str) {
        unimplemented!("host::testing::ipc_harness::IpcHarness::revoke")
    }

    /// The hello of a client that matches the server: protocol 1, contract `0.1.0`, the
    /// server's plugin set (HST-103).
    pub fn matching_hello(&self) -> HelloSpec {
        unimplemented!("host::testing::ipc_harness::IpcHarness::matching_hello")
    }

    /// Connects a client.
    pub fn connect(&mut self, _client_id: &str, _interactive: bool, _hello: HelloSpec) -> ConnId {
        unimplemented!("host::testing::ipc_harness::IpcHarness::connect")
    }

    /// Reattaches a client that last saw outcome `last_seen_seq` (HST-116).
    pub fn reattach(&mut self, _client_id: &str, _last_seen_seq: u64, _hello: HelloSpec) -> ConnId {
        unimplemented!("host::testing::ipc_harness::IpcHarness::reattach")
    }

    /// Drops a connection.
    pub fn disconnect(&mut self, _conn: ConnId) {
        unimplemented!("host::testing::ipc_harness::IpcHarness::disconnect")
    }

    /// Moves the clock.
    pub fn advance(&mut self, _by: Duration) {
        unimplemented!("host::testing::ipc_harness::IpcHarness::advance")
    }

    /// Runs server and client halves until idle or `max_rounds`.
    pub fn pump(&mut self, _max_rounds: u32) {
        unimplemented!("host::testing::ipc_harness::IpcHarness::pump")
    }

    /// Everything the server sent to the connection, in order.
    pub fn wire(&self, _conn: ConnId) -> Vec<WireItem> {
        unimplemented!("host::testing::ipc_harness::IpcHarness::wire")
    }

    /// What the client half did, in order.
    pub fn deliveries(&self, _conn: ConnId) -> Vec<ClientDelivery> {
        unimplemented!("host::testing::ipc_harness::IpcHarness::deliveries")
    }

    /// Whether the connection is open.
    pub fn is_open(&self, _conn: ConnId) -> bool {
        unimplemented!("host::testing::ipc_harness::IpcHarness::is_open")
    }

    /// The error id the connection was closed with.
    pub fn close_code(&self, _conn: ConnId) -> Option<String> {
        unimplemented!("host::testing::ipc_harness::IpcHarness::close_code")
    }

    /// How the client process exited, when it did (HST-104).
    pub fn client_exit(&self, _conn: ConnId) -> Option<ClientExit> {
        unimplemented!("host::testing::ipc_harness::IpcHarness::client_exit")
    }

    /// The client half of a plugin on a connection (HST-105).
    pub fn client_half(&self, _conn: ConnId, _plugin: &str) -> ClientHalf {
        unimplemented!("host::testing::ipc_harness::IpcHarness::client_half")
    }

    /// Whether the server half of a plugin runs.
    pub fn server_half_running(&self, _plugin: &str) -> bool {
        unimplemented!("host::testing::ipc_harness::IpcHarness::server_half_running")
    }

    /// Open connections.
    pub fn connection_count(&self) -> usize {
        unimplemented!("host::testing::ipc_harness::IpcHarness::connection_count")
    }

    /// Nodes in the server tree.
    pub fn server_tree_len(&self) -> usize {
        unimplemented!("host::testing::ipc_harness::IpcHarness::server_tree_len")
    }

    /// Local effects the client applied before the server confirmed (HST-112).
    pub fn client_local_effects(&self, _conn: ConnId) -> usize {
        unimplemented!("host::testing::ipc_harness::IpcHarness::client_local_effects")
    }

    /// Submits the client sent.
    pub fn submits_sent(&self, _conn: ConnId) -> usize {
        unimplemented!("host::testing::ipc_harness::IpcHarness::submits_sent")
    }

    /// Resync requests the client sent (HST-108).
    pub fn resync_requests(&self, _conn: ConnId) -> usize {
        unimplemented!("host::testing::ipc_harness::IpcHarness::resync_requests")
    }

    /// The contribution ids in the client's mirror.
    pub fn client_mirror(&self, _conn: ConnId) -> Vec<String> {
        unimplemented!("host::testing::ipc_harness::IpcHarness::client_mirror")
    }

    /// The contribution ids on the server.
    pub fn server_contributions(&self) -> Vec<String> {
        unimplemented!("host::testing::ipc_harness::IpcHarness::server_contributions")
    }

    /// The final outcome of a submission, when it completed.
    pub fn final_outcome(
        &self,
        _client_id: &str,
        _plugin: &str,
        _dedup_key: &str,
    ) -> Option<String> {
        unimplemented!("host::testing::ipc_harness::IpcHarness::final_outcome")
    }

    /// PTY frames written to the connection.
    pub fn pty_frames_written(&self, _conn: ConnId) -> usize {
        unimplemented!("host::testing::ipc_harness::IpcHarness::pty_frames_written")
    }

    /// PTY frames dropped on the connection.
    pub fn pty_frames_dropped(&self, _conn: ConnId) -> usize {
        unimplemented!("host::testing::ipc_harness::IpcHarness::pty_frames_dropped")
    }

    /// The client submits an intent of a plugin.
    pub fn submit(&mut self, _conn: ConnId, _plugin: &str, _dedup_key: &str, _intent: &str) {
        unimplemented!("host::testing::ipc_harness::IpcHarness::submit")
    }

    /// Pauses or resumes the server's apply step.
    pub fn pause_apply(&mut self, _paused: bool) {
        unimplemented!("host::testing::ipc_harness::IpcHarness::pause_apply")
    }

    /// The server-side plugin puts a contribution of the given encoded size.
    pub fn put_contribution(&mut self, _plugin: &str, _id: &str, _entry_bytes: usize) {
        unimplemented!("host::testing::ipc_harness::IpcHarness::put_contribution")
    }

    /// Loses the next increment sent to the connection (HST-108).
    pub fn drop_next_increment(&mut self, _conn: ConnId) {
        unimplemented!("host::testing::ipc_harness::IpcHarness::drop_next_increment")
    }

    /// The client sends an envelope with an explicit sequence number (HST-102).
    pub fn send_client_envelope(&mut self, _conn: ConnId, _seq: u64) {
        unimplemented!("host::testing::ipc_harness::IpcHarness::send_client_envelope")
    }

    /// Floods plugin data toward a connection whose client reader is stalled (HST-121..123).
    pub fn flood_plugin_data(&mut self, _conn: ConnId, _record_bytes: usize, _records: usize) {
        unimplemented!("host::testing::ipc_harness::IpcHarness::flood_plugin_data")
    }

    /// Queues PTY frames toward a connection (HST-120).
    pub fn push_pty_frames(&mut self, _conn: ConnId, _count: usize) {
        unimplemented!("host::testing::ipc_harness::IpcHarness::push_pty_frames")
    }
}
