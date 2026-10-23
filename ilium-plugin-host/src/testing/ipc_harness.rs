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
