//! Shared helpers of the kernel-boot lane for the server crate.

use std::path::{Path, PathBuf};

use ilium_plugin_api::{ProcessKind, Tier};
use ilium_plugin_host::lifecycle::StartMode;
use ilium_plugin_host::testing::boot::{BootHarness, BootRun, PluginSpec};

/// The workspace root.
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

/// The text of one manual page.
pub fn manual_page(name: &str) -> String {
    let path = workspace_root()
        .join("docs/plugins-manual/pages")
        .join(format!("{name}.md"));
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

/// A core plugin that starts at boot on its server side.
pub fn startup(id: &str) -> PluginSpec {
    PluginSpec::new(id, Tier::T1)
        .core()
        .activation(&["startup"])
}

/// A started server run with the given plugins.
pub fn started(plugins: Vec<PluginSpec>) -> BootRun {
    let mut harness = BootHarness::new(ProcessKind::Server);
    for plugin in plugins {
        harness = harness.plugin(plugin);
    }
    let mut run = harness.build();
    run.start();
    run
}

/// A started server run in a start mode.
pub fn started_in(mode: StartMode, plugins: Vec<PluginSpec>) -> BootRun {
    let mut harness = BootHarness::new(ProcessKind::Server).mode(mode);
    for plugin in plugins {
        harness = harness.plugin(plugin);
    }
    let mut run = harness.build();
    run.start();
    run
}

/// The 30 core plugin names (directory names under `plugins/core`).
pub const CORE_PLUGINS: [&str; 30] = [
    "agent-detect",
    "agent-setup",
    "agent-toolbar",
    "ambient",
    "chatroom",
    "compaction",
    "cost",
    "diagnostics",
    "editor",
    "fleet",
    "inference",
    "kanban",
    "keys-and-help",
    "layout",
    "notifications",
    "progress",
    "recap",
    "scenes",
    "scheduler",
    "search",
    "session-convert",
    "sessions",
    "settings",
    "smart-copy",
    "terminal",
    "text-triggers",
    "titles",
    "tree",
    "voice",
    "worktrees",
];

/// The plugin id of a core plugin name.
pub fn core_id(name: &str) -> String {
    format!("ilium.core.{name}")
}
