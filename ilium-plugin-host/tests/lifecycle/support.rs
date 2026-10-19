//! Shared helpers over the `testing::life` gap harness (see API_GAPS.md).

pub use ilium_plugin_api::generated::host::plugins::{HealthState, InstanceState, PackageState};
pub use ilium_plugin_api::lifecycle::DeactivationReason;
pub use ilium_plugin_api::{Side, Tier};
pub use ilium_plugin_host::lifecycle::StartMode;
pub use ilium_plugin_host::testing::life::{FailureKind, LifeHost, PluginSpec, SafeMode};

pub const A: &str = "com.test.alpha";
pub const B: &str = "com.test.beta";
pub const C: &str = "com.test.gamma";
pub const SERVER: &str = "server";

/// A host with one lazy T3 server plugin, started.
pub fn one_plugin() -> LifeHost {
    let mut host = LifeHost::new();
    host.install(PluginSpec::new(A).sides(&[Side::Server]).startup());
    host.start(StartMode::Normal);
    host
}

/// A host with one plugin that has a server and a client side, one client attached.
pub fn two_sided() -> (LifeHost, String) {
    let mut host = LifeHost::new();
    host.install(
        PluginSpec::new(A)
            .sides(&[Side::Server, Side::Client])
            .startup(),
    );
    host.start(StartMode::Normal);
    let scope = host.attach_client("client-1");
    (host, scope)
}

/// Fails the server side of `plugin` once with a trap.
pub fn trap(host: &mut LifeHost, plugin: &str) {
    host.fail(plugin, Side::Server, SERVER, FailureKind::Trap);
}

pub fn state(host: &LifeHost, plugin: &str) -> InstanceState {
    host.side(plugin, Side::Server, SERVER).state
}

pub fn failures(host: &LifeHost, plugin: &str) -> u32 {
    host.side(plugin, Side::Server, SERVER).failures_in_window
}

/// Milliseconds until the scheduled restart, read from `restart_at_unix_ms`.
pub fn restart_in_ms(host: &LifeHost, plugin: &str) -> Option<u64> {
    host.side(plugin, Side::Server, SERVER)
        .restart_at_unix_ms
        .map(|at| at - host.unix_ms())
}

/// Parsed `ilium plugin ...` JSONL records of the given type.
pub fn records(host: &mut LifeHost, args: &[&str], kind: &str) -> Vec<serde_json::Value> {
    host.cli(args)
        .stdout_jsonl
        .into_iter()
        .filter(|line| line["type"] == kind)
        .collect()
}
