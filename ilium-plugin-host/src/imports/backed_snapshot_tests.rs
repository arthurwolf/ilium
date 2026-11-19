//! Production terminal adapter must retain a captured screen independently of later output.

use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use ilium_plugin_api::generated::primitives::terminal::{Cell, CellRows, HostTerminal, ScreenPart, Selection};
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_api::generated::types::ids::NodeId;
use ilium_plugin_api::generated::types::intents::Intent;
use ilium_plugin_api::generated::types::requests::Outcome;
use ilium_plugin_api::kernel::{FrozenTerminal, IntentResult, KernelFacade, Origin, PaneRead, TreeNode, TreeRead};
use ilium_plugin_api::ProcessKind;

use super::KernelTerminal;

struct Kernel(Mutex<String>, Arc<AtomicBool>, AtomicU32);

fn kernel(text: &str) -> Arc<Kernel> {
    Arc::new(Kernel(Mutex::new(text.to_owned()), Arc::new(AtomicBool::new(true)), AtomicU32::new(0)))
}

struct Captured(String, Arc<AtomicBool>);

impl FrozenTerminal for Captured {
    fn retained_bytes(&self) -> usize { self.0.capacity() }
    fn is_live(&self) -> bool { self.1.load(Ordering::SeqCst) }
    fn read_cells(&self, first_row: u32, count: u32) -> Option<CellRows> {
        if !self.is_live() { return None; }
        let rows = if first_row == 0 && count > 0 {
            vec![vec![Cell { text: self.0.clone(), ..Cell::default() }]]
        } else { Vec::new() };
        Some(CellRows { first_row, columns: 1, rows, revision: 0 })
    }
}

impl TreeRead for Kernel {
    fn nodes(&self) -> Vec<TreeNode> { Vec::new() }
    fn node(&self, _id: NodeId) -> Option<TreeNode> { None }
    fn epoch(&self) -> u64 { 0 }
}

impl PaneRead for Kernel {
    fn freeze_cells(&self, pane: NodeId) -> Result<Box<dyn FrozenTerminal>, PluginError> {
        if pane != NodeId(1) {
            return Err(PluginError::new(ErrorCode::NotFound, "pane not found"));
        }
        Ok(Box::new(Captured(self.0.lock().unwrap().clone(), self.1.clone())))
    }
    fn screen_text(&self, pane: NodeId) -> Option<String> {
        (pane == NodeId(1)).then(|| self.0.lock().unwrap().clone())
    }
    fn cells(&self, pane: NodeId, rows: std::ops::Range<u16>) -> Option<Vec<Vec<Cell>>> {
        if pane != NodeId(1) { return None; }
        let text = self.0.lock().unwrap().clone();
        Some(rows.filter(|row| *row == 0).map(|_| vec![Cell { text: text.clone(), ..Cell::default() }]).collect())
    }
    fn history(&self, pane: NodeId, lines: u32) -> Option<Vec<String>> {
        if pane != NodeId(1) { return None; }
        self.2.store(lines, Ordering::SeqCst);
        Some(Vec::new())
    }
    fn selection(&self, pane: NodeId) -> Option<Selection> {
        (pane == NodeId(1)).then(|| Selection {
            first_row: 0, first_column: 0, last_row: 0, last_column: 1,
            text: self.0.lock().unwrap().clone(),
        })
    }
    fn size(&self, pane: NodeId) -> Option<(u16, u16)> {
        (pane == NodeId(1)).then_some((1, 1))
    }
}

#[test]
fn selection_from_the_kernel_is_scoped_to_the_pane_and_grant() {
    let kernel = kernel("selection");
    let mut allowed = KernelTerminal::new(kernel.clone(), &["terminal.read-screen".to_owned()]);
    let mut denied = KernelTerminal::new(kernel, &[]);
    assert_eq!(allowed.get_selection(NodeId(1)).map(|selection| selection.text), Some("selection".to_owned()));
    assert!(allowed.get_selection(NodeId(2)).is_none());
    assert!(denied.get_selection(NodeId(1)).is_none());
}

#[test]
fn live_cell_reads_reject_oversized_and_overflowing_ranges() {
    let mut terminal = KernelTerminal::new(kernel("screen"), &["terminal.read-cells".to_owned()]);
    assert!(terminal.read_cells(NodeId(1), ScreenPart::Visible, 0, 1001).is_none());
    assert!(terminal.read_cells(NodeId(1), ScreenPart::Visible, u32::MAX, 1).is_none());
    assert!(terminal.read_cells(NodeId(1), ScreenPart::Visible, 0, 1000).is_some());
    assert_eq!(terminal.read_cells(NodeId(1), ScreenPart::Visible, 0, 1).unwrap().rows[0][0].text, "screen");
}

#[test]
fn scrollback_screen_read_clamps_before_requesting_history() {
    let kernel = kernel("screen");
    let mut terminal = KernelTerminal::new(kernel.clone(), &["terminal.history".to_owned()]);
    assert!(terminal.read_screen(NodeId(1), ScreenPart::Scrollback, u32::MAX).is_some());
    assert_eq!(kernel.2.load(Ordering::SeqCst), 10_000);
}

impl KernelFacade for Kernel {
    fn apply_intent(&self, _origin: &Origin, _intent: Intent) -> IntentResult { Outcome::Ok }
    fn tree(&self) -> &dyn TreeRead { self }
    fn panes(&self) -> &dyn PaneRead { self }
    fn process_kind(&self) -> ProcessKind { ProcessKind::Client }
}

#[test]
fn snapshot_of_a_real_kernel_adapter_is_stable_and_released() {
    let kernel = kernel("before");
    let mut terminal = KernelTerminal::new(kernel.clone(), &["terminal.read-cells".to_owned()]);
    let snapshot = terminal.freeze_snapshot(NodeId(1)).expect("capture");
    *kernel.0.lock().unwrap() = "after".to_owned();
    let rows = terminal.read_snapshot_cells(snapshot, 0, 1).expect("snapshot rows");
    assert_eq!(rows.rows[0][0].text, "before");
    terminal.drop_snapshot(snapshot);
    assert!(terminal.read_snapshot_cells(snapshot, 0, 1).is_none());
}

#[test]
fn snapshots_require_the_grant_and_are_limited_per_plugin_instance() {
    use ilium_plugin_api::generated::types::errors::ErrorCode;
    let kernel = kernel("screen");
    let mut denied = KernelTerminal::new(kernel.clone(), &[]);
    assert_eq!(denied.freeze_snapshot(NodeId(1)).unwrap_err().code, ErrorCode::NotPermitted);
    let mut allowed = KernelTerminal::new(kernel, &["terminal.read-cells".to_owned()]);
    let snapshots: Vec<_> = (0..4).map(|_| allowed.freeze_snapshot(NodeId(1)).unwrap()).collect();
    assert_eq!(allowed.freeze_snapshot(NodeId(1)).unwrap_err().code, ErrorCode::QuotaExceeded);
    allowed.drop_snapshot(snapshots[0]);
    assert!(allowed.freeze_snapshot(NodeId(1)).is_ok());
    assert!(allowed.read_snapshot_cells(snapshots[1], 0, 1001).is_none());
    assert!(allowed.read_snapshot_cells(snapshots[1], u32::MAX, 1).is_none());
}

#[test]
fn snapshot_handles_are_private_to_the_creating_plugin_instance() {
    let kernel = kernel("private");
    let grant = ["terminal.read-cells".to_owned()];
    let mut owner = KernelTerminal::new(kernel.clone(), &grant);
    let mut other = KernelTerminal::new(kernel, &grant);
    let handle = owner.freeze_snapshot(NodeId(1)).expect("capture");
    assert!(other.read_snapshot_cells(handle, 0, 1).is_none());
    other.drop_snapshot(handle);
    assert_eq!(owner.read_snapshot_cells(handle, 0, 1).unwrap().rows[0][0].text, "private");
}

#[test]
fn removed_pane_invalidates_its_snapshot_and_frees_the_slot() {
    let kernel = kernel("before");
    let mut terminal = KernelTerminal::new(kernel.clone(), &["terminal.read-cells".to_owned()]);
    let snapshot = terminal.freeze_snapshot(NodeId(1)).unwrap();
    kernel.1.store(false, Ordering::SeqCst);
    assert!(terminal.read_snapshot_cells(snapshot, 0, 1).is_none());
    kernel.1.store(true, Ordering::SeqCst);
    assert!(terminal.read_snapshot_cells(snapshot, 0, 1).is_none());
    assert!(terminal.freeze_snapshot(NodeId(1)).is_ok());
}

#[test]
fn capture_rejects_a_pane_removed_before_the_capture_returns() {
    let kernel = kernel("gone");
    kernel.1.store(false, Ordering::SeqCst);
    let mut terminal = KernelTerminal::new(kernel, &["terminal.read-cells".to_owned()]);
    assert_eq!(terminal.freeze_snapshot(NodeId(1)).unwrap_err().code, ErrorCode::NotFound);
}

#[test]
fn memory_quota_refuses_a_new_capture_without_dropping_an_existing_one() {
    struct LargeCapture;
    impl FrozenTerminal for LargeCapture {
        fn retained_bytes(&self) -> usize { 64 * 1024 * 1024 }
        fn is_live(&self) -> bool { true }
        fn read_cells(&self, _first_row: u32, _count: u32) -> Option<CellRows> {
            Some(CellRows::default())
        }
    }
    let mut terminal = KernelTerminal::new(kernel("new"), &["terminal.read-cells".to_owned()]);
    terminal.snapshots.insert(42, Box::new(LargeCapture));
    assert_eq!(terminal.freeze_snapshot(NodeId(1)).unwrap_err().code, ErrorCode::QuotaExceeded);
    assert!(terminal.read_snapshot_cells(42, 0, 1).is_some());
}
