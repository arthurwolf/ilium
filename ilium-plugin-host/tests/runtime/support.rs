//! Shared helpers of the host-runtime lane: guests, harness shortcuts, clock steps, manual-page
//! readers. Everything uses only the public surface named in API_GAPS.md or the skeleton.

#![allow(dead_code)]

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use ilium_plugin_api::generated::host::plugins::InstanceState;
use ilium_plugin_api::generated::types::events::Event;
use ilium_plugin_api::generated::types::intents::{Intent, Submission};
use ilium_plugin_api::ids::PluginId;
use ilium_plugin_api::{Code, Tier};
use ilium_plugin_host::calls::Trap;
use ilium_plugin_host::testing::{HarnessHandle, HostHarness, NativeGuest};

/// Plugin id the harness gives its plugin (DEV-420: `test.plugin`).
pub fn plugin_id() -> PluginId {
    PluginId("test.plugin".to_owned())
}

/// One event that makes the harness call `on-events` once.
pub fn one_event() -> Vec<Event> {
    vec![Event::ThemeChanged]
}

/// A trap with the given code, as a native guest returns it.
pub fn trap_with(code: &str) -> Trap {
    Trap {
        code: Code(Cow::Owned(code.to_owned())),
        interface: "ilium:exports/handler".to_owned(),
        function: "on-events".to_owned(),
    }
}

/// A guest whose `on-events` always traps with the given code.
pub struct TrapGuest(pub &'static str);

impl NativeGuest for TrapGuest {
    fn on_events(&mut self, _batch: Vec<Event>) -> Result<Vec<Submission>, Trap> {
        Err(trap_with(self.0))
    }
}

/// A guest that accepts every batch and returns no submission.
pub struct QuietGuest;

impl NativeGuest for QuietGuest {
    fn on_events(&mut self, _batch: Vec<Event>) -> Result<Vec<Submission>, Trap> {
        Ok(Vec::new())
    }
}

/// A guest that counts its `on-events` calls.
pub struct CountGuest(pub Arc<AtomicUsize>);

impl NativeGuest for CountGuest {
    fn on_events(&mut self, _batch: Vec<Event>) -> Result<Vec<Submission>, Trap> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(Vec::new())
    }
}

/// A guest that returns `count` play-sound submissions on every call.
pub struct BatchGuest(pub usize);

impl NativeGuest for BatchGuest {
    fn on_events(&mut self, _batch: Vec<Event>) -> Result<Vec<Submission>, Trap> {
        Ok(sound_submissions(self.0))
    }
}

/// `count` play-sound submissions with correlations 1..=count.
pub fn sound_submissions(count: usize) -> Vec<Submission> {
    (1..=count as u64)
        .map(|correlation| Submission {
            intent: Intent::PlaySound("bell".to_owned()),
            correlation,
            dedup_key: None,
        })
        .collect()
}

/// A T3 server plugin with the given in-process guest.
pub fn host_with(guest: impl NativeGuest + 'static) -> HarnessHandle {
    HostHarness::new()
        .with_native_guest(Box::new(guest))
        .build()
        .expect("the harness builds a host for a native guest")
}

/// Same with a chosen tier.
pub fn host_with_tier(tier: Tier, guest: impl NativeGuest + 'static) -> HarnessHandle {
    HostHarness::new()
        .tier(tier)
        .with_native_guest(Box::new(guest))
        .build()
        .expect("the harness builds a host for a native guest")
}

/// Delivers one event and runs the dispatcher, so a trapping guest fails once.
pub fn fail_once(host: &mut HarnessHandle) {
    host.deliver(one_event());
    host.pump(8);
}

/// Moves the manual clock by whole milliseconds.
pub fn advance_ms(host: &mut HarnessHandle, millis: u64) {
    host.advance_clock(Duration::from_millis(millis));
}

/// Minutes in milliseconds.
pub fn minutes(count: u64) -> u64 {
    count * 60_000
}

/// Fails a trapping host at the given absolute times (ms from the start) and returns the state
/// right after each failure.
pub fn fail_at(host: &mut HarnessHandle, times_ms: &[u64]) -> Vec<InstanceState> {
    let mut now = 0;
    let mut states = Vec::new();
    for time in times_ms {
        advance_ms(host, time - now);
        now = *time;
        fail_once(host);
        states.push(host.state());
    }
    states
}

/// A trapping host that has failed at the given times; returns it with the states.
pub fn trapped_host_at(times_ms: &[u64]) -> (HarnessHandle, Vec<InstanceState>) {
    let mut host = host_with(TrapGuest("trap.panic"));
    let states = fail_at(&mut host, times_ms);
    (host, states)
}

/// All captured log text joined with newlines.
pub fn log_text(host: &HarnessHandle) -> String {
    host.logs()
        .iter()
        .map(|line| format!("{} {} {}", line.plugin, line.level, line.message))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Number of log lines whose message contains the needle.
pub fn log_count(host: &HarnessHandle, needle: &str) -> usize {
    host.logs()
        .iter()
        .filter(|line| line.message.contains(needle))
        .count()
}

/// The code text of a trap.
pub fn code_of(trap: &Trap) -> String {
    trap.code.as_str().to_owned()
}

/// Root of the workspace.
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// The text of a manual page under docs/plugins-manual/pages.
pub fn manual_page(name: &str) -> String {
    let path = workspace_root()
        .join("docs/plugins-manual/pages")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("manual page {name}: {error}"))
}

/// Bytes of a built guest fixture; fails red with `fixture.missing` until the toolchain builds it.
pub fn fixture_wasm(name: &str) -> Vec<u8> {
    let path = workspace_root()
        .join("plugins/fixtures")
        .join(name)
        .join("dist")
        .join(format!("{name}.wasm"));
    assert!(path.exists(), "fixture.missing: {}", path.display());
    std::fs::read(&path).expect("fixture file is readable")
}

/// A host running a built wasm fixture on a given side with a given tier.
pub fn host_with_fixture(name: &str, tier: Tier) -> HarnessHandle {
    let bytes = fixture_wasm(name);
    HostHarness::new()
        .tier(tier)
        .with_wasm_bytes(&bytes)
        .build()
        .expect("the harness builds a host for the fixture component")
}
