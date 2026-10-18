//! Shared helpers of the imports-ui tests: a rig that wires a harness to a fake kernel, error
//! readers and size constants. Everything outside the skeleton is named in API_GAPS.md.

use std::collections::VecDeque;
use std::fmt::Debug;
use std::sync::{Arc, Mutex};

use ilium_plugin_api::generated::types::events::Event;
use ilium_plugin_api::generated::types::ids::NodeId;
use ilium_plugin_api::generated::types::intents::Submission;
use ilium_plugin_api::{ErrorCode, PluginError, ProcessKind, Side, Tier};
use ilium_plugin_host::calls::Trap;
use ilium_plugin_host::testing::{FakeKernel, HarnessHandle, HostHarness, NativeGuest};

pub const KIB: usize = 1024;
pub const MIB: usize = 1024 * 1024;

/// A live terminal pane in the standard tree of the rig.
pub const PANE: NodeId = NodeId(10);
/// A node id that is not in the tree.
pub const MISSING_PANE: NodeId = NodeId(999);

/// A guest that does nothing; imports are called directly through the host accessors.
pub struct IdleGuest {
    pub seen: Arc<Mutex<Vec<Event>>>,
}

impl NativeGuest for IdleGuest {
    fn on_events(&mut self, batch: Vec<Event>) -> Result<Vec<Submission>, Trap> {
        self.seen.lock().expect("seen lock").extend(batch);
        Ok(Vec::new())
    }
}

/// A harness and its fake kernel.
pub struct Rig {
    pub kernel: Arc<FakeKernel>,
    pub host: HarnessHandle,
}

fn build(side: Side, grants: &[&str], declined: &[&str]) -> Rig {
    let process = match side {
        Side::Server => ProcessKind::Server,
        _ => ProcessKind::Client,
    };
    let kernel = Arc::new(FakeKernel::new(process));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let _queue: VecDeque<Submission> = VecDeque::new();
    let mut builder = HostHarness::new().tier(Tier::T3).side(side).grants(grants);
    if !declined.is_empty() {
        builder = builder.declined(declined);
    }
    let host = builder
        .kernel(kernel.clone())
        .with_native_guest(Box::new(IdleGuest { seen: seen.clone() }))
        .build()
        .expect("the harness builds a host for a native guest");
    Rig { kernel, host }
}

/// A rig for a T3 plugin on `side` with the given grants.
pub fn rig_on(side: Side, grants: &[&str]) -> Rig {
    build(side, grants, &[])
}

/// A client-side rig.
pub fn rig(grants: &[&str]) -> Rig {
    build(Side::Client, grants, &[])
}

/// A server-side rig.
pub fn server_rig(grants: &[&str]) -> Rig {
    build(Side::Server, grants, &[])
}

/// A client-side rig where the plugin requested `requested` but the user declined them.
pub fn declined_rig(requested: &[&str]) -> Rig {
    build(Side::Client, &[], requested)
}

/// The error code of a failed call; panics (red) when the call succeeded.
pub fn code_of<T: Debug>(result: Result<T, PluginError>) -> ErrorCode {
    match result {
        Err(error) => error.code,
        Ok(value) => panic!("expected an error, got Ok({value:?})"),
    }
}

/// A string of `length` ASCII characters.
pub fn text_of(length: usize) -> String {
    "a".repeat(length)
}
