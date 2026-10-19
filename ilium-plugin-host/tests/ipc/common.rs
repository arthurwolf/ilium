//! Shared helpers of the host-ipc lane.

use std::path::PathBuf;
use std::time::Duration;

use ilium_plugin_host::testing::ipc_harness::{ConnId, IpcHarness, WireItem};

pub const PLUGIN: &str = "acme.demo";
pub const OTHER_PLUGIN: &str = "acme.other";
pub const MIB: usize = 1024 * 1024;
/// Upper bound of pump rounds in every test, so no test can spin.
pub const ROUNDS: u32 = 64;

/// A server with `acme.demo` installed and granted `panes.create`.
pub fn server_with_demo() -> IpcHarness {
    let mut harness = IpcHarness::new();
    harness.install_server_plugin(PLUGIN, "1.0.0", [1; 32]);
    harness.grant(PLUGIN, "panes.create");
    harness
}

/// A server with `acme.demo` and one attached matching client.
pub fn attached() -> (IpcHarness, ConnId) {
    let mut harness = server_with_demo();
    let hello = harness.matching_hello();
    let conn = harness.connect("client-1", true, hello);
    harness.pump(ROUNDS);
    (harness, conn)
}

pub fn seconds(count: u64) -> Duration {
    Duration::from_secs(count)
}

pub fn position(wire: &[WireItem], wanted: impl Fn(&WireItem) -> bool) -> Option<usize> {
    wire.iter().position(wanted)
}

pub fn is_outcome(item: &WireItem) -> bool {
    matches!(item, WireItem::Outcome { .. })
}

pub fn is_snapshot(item: &WireItem) -> bool {
    matches!(item, WireItem::Snapshot { .. })
}

pub fn is_tree_change(item: &WireItem) -> bool {
    matches!(item, WireItem::TreeChange { .. })
}

/// Directory of the manual pages (override `ILIUM_PLUGIN_MANUAL_DIR`).
pub fn manual_pages_dir() -> PathBuf {
    match std::env::var_os("ILIUM_PLUGIN_MANUAL_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../docs/plugins-manual/pages"),
    }
}
