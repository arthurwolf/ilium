//! Compaction-lane hooks shared by the port and feature test targets.
//!
//! Mounted by both targets, so only methods that both targets call live here.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::ViewTarget;
use serde_json::Value;

pub trait CompactionSharedHostExt {
    fn scans_started(&self) -> Vec<Value> {
        unimplemented!("compaction.scans-started")
    }
    fn recommended_tokens(&self) -> u64 {
        unimplemented!("compaction.recommended-tokens")
    }
    fn select_agent_target(&mut self, _target: &str) {
        unimplemented!("compaction.select-agent-target")
    }
    fn agent_file_writes(&self) -> Vec<String> {
        unimplemented!("compaction.agent-file-writes")
    }
    fn open_view(&mut self, _target: ViewTarget) {
        unimplemented!("compaction.open-view")
    }
    fn view_closed(&self, _target: ViewTarget) -> bool {
        unimplemented!("compaction.view-closed")
    }
}

impl CompactionSharedHostExt for FakeHost {}
