//! The server's plugin host: boot order and the `KernelFacade` over the real tree
//! (reference-host-architecture.md). Skeleton: nothing calls it yet, so no existing code path
//! changes behaviour.
//!
//! The `for_parity_world*` constructors and the drivers and observers below exist for the
//! plugin-parity tests (`ilium-server/tests/plugin_parity`, parity-and-coverage.md): they build a
//! kernel over a temporary project in a start mode and let a test drive it like a running server.

// Not wired into the server yet; the implementation phase calls it from `run()`.
#![allow(dead_code, unused_variables)]

use std::net::SocketAddr;
use std::path::PathBuf;

use ilium_plugin_api::generated::types::{Intent, NodeId};
use ilium_plugin_api::kernel::{
    Cell, IntentResult, KernelFacade, Origin, PaneRead, TreeNode, TreeRead,
};
use ilium_plugin_api::ProcessKind;
use ilium_plugin_host::lifecycle::{ShutdownReport, StartMode, StartReport};

/// Read and write access to the real tree and panes for plugins.
#[derive(Debug)]
pub struct ServerKernel {
    start_mode: StartMode,
}

impl ServerKernel {
    /// A kernel over a fresh temporary project started in `mode` with this `config.toml`
    /// (parity-and-coverage.md COR-010).
    pub fn for_parity_world(mode: StartMode, config_toml: &str) -> ServerKernel {
        unimplemented!("server::plugin_host::ServerKernel::for_parity_world")
    }

    /// Like `for_parity_world`, owning the named session (parity-and-coverage.md COR-010).
    pub fn for_parity_world_named(mode: StartMode, config_toml: &str, session: &str) -> ServerKernel {
        unimplemented!("server::plugin_host::ServerKernel::for_parity_world_named")
    }

    // ---- Drivers (parity-and-coverage.md) -------------------------------------------------

    /// Runs every pending task until the server is idle.
    pub fn settle(&self) {
        unimplemented!("server::plugin_host::ServerKernel::settle")
    }

    /// Advances the server's clock.
    pub fn advance_clock_ms(&self, ms: u64) {
        unimplemented!("server::plugin_host::ServerKernel::advance_clock_ms")
    }

    /// Feeds bytes to a pane as if its process wrote them.
    pub fn feed_output(&self, pane: NodeId, bytes: &[u8]) {
        unimplemented!("server::plugin_host::ServerKernel::feed_output")
    }

    /// Clicks in a pane (`button` 0 left, 1 middle, 2 right).
    pub fn mouse_click(&self, pane: NodeId, column: u16, row: u16, button: u8) {
        unimplemented!("server::plugin_host::ServerKernel::mouse_click")
    }

    /// Scrolls the wheel in a pane.
    pub fn mouse_wheel(&self, pane: NodeId, column: u16, row: u16, up: bool) {
        unimplemented!("server::plugin_host::ServerKernel::mouse_wheel")
    }

    /// Marks the tree changed without changing it (snapshot scheduling, COR-120).
    pub fn touch_tree(&self) {
        unimplemented!("server::plugin_host::ServerKernel::touch_tree")
    }

    /// A new kernel over the same project, restored from disk as after a restart (COR-130).
    pub fn restart_from_disk(&self) -> ServerKernel {
        unimplemented!("server::plugin_host::ServerKernel::restart_from_disk")
    }

    /// Re-reads `config.toml` as the file watcher would.
    pub fn reload_config(&self) {
        unimplemented!("server::plugin_host::ServerKernel::reload_config")
    }

    /// Replaces `config.toml`.
    pub fn write_config_file(&self, text: &str) {
        unimplemented!("server::plugin_host::ServerKernel::write_config_file")
    }

    /// Writes a file under the project root.
    pub fn write_project_file(&self, path: &str, text: &str) {
        unimplemented!("server::plugin_host::ServerKernel::write_project_file")
    }

    /// Sets an environment variable the server sees.
    pub fn set_environment(&self, name: &str, value: &str) {
        unimplemented!("server::plugin_host::ServerKernel::set_environment")
    }

    /// Starts a new debug-logging session (COR-180).
    pub fn restart_logging_session(&self) {
        unimplemented!("server::plugin_host::ServerKernel::restart_logging_session")
    }

    /// Completes onboarding, with AI features on or off.
    pub fn finish_onboarding(&self, enable_ai: bool) {
        unimplemented!("server::plugin_host::ServerKernel::finish_onboarding")
    }

    /// Types text into an editor pane.
    pub fn editor_type(&self, pane: NodeId, text: &str) {
        unimplemented!("server::plugin_host::ServerKernel::editor_type")
    }

    /// Saves an editor pane.
    pub fn editor_save(&self, pane: NodeId) {
        unimplemented!("server::plugin_host::ServerKernel::editor_save")
    }

    /// Adds a card to a board column.
    pub fn board_add_card(&self, pane: NodeId, column: &str, text: &str) {
        unimplemented!("server::plugin_host::ServerKernel::board_add_card")
    }

    /// Creates a board pane with a storage kind at a path.
    pub fn create_board(&self, storage: &str, path: &str) -> NodeId {
        unimplemented!("server::plugin_host::ServerKernel::create_board")
    }

    // ---- Observers (parity-and-coverage.md) -----------------------------------------------

    /// The start report of the boot (reference-host-architecture.md).
    pub fn start_report(&self) -> StartReport {
        unimplemented!("server::plugin_host::ServerKernel::start_report")
    }

    /// Every `(section, key)` the settings surface knows.
    pub fn settings_keys(&self) -> Vec<(String, String)> {
        unimplemented!("server::plugin_host::ServerKernel::settings_keys")
    }

    /// Every `(section, key, default)` of the settings surface.
    pub fn settings_defaults(&self) -> Vec<(String, String, String)> {
        unimplemented!("server::plugin_host::ServerKernel::settings_defaults")
    }

    /// The effective value of a setting.
    pub fn setting(&self, section: &str, key: &str) -> Option<String> {
        unimplemented!("server::plugin_host::ServerKernel::setting")
    }

    /// Applies a setting as the settings dialog would.
    pub fn apply_setting(&self, section: &str, key: &str, value: &str) -> Result<(), String> {
        unimplemented!("server::plugin_host::ServerKernel::apply_setting")
    }

    /// The current `config.toml` text.
    pub fn config_text(&self) -> String {
        unimplemented!("server::plugin_host::ServerKernel::config_text")
    }

    /// The configuration directory.
    pub fn config_dir(&self) -> PathBuf {
        unimplemented!("server::plugin_host::ServerKernel::config_dir")
    }

    /// The project root.
    pub fn project_root(&self) -> String {
        unimplemented!("server::plugin_host::ServerKernel::project_root")
    }

    /// Every byte written to a pane's process.
    pub fn pane_input_log(&self, pane: NodeId) -> Vec<u8> {
        unimplemented!("server::plugin_host::ServerKernel::pane_input_log")
    }

    /// The exit code of a pane's process, once it ended.
    pub fn pane_exit_code(&self, pane: NodeId) -> Option<i32> {
        unimplemented!("server::plugin_host::ServerKernel::pane_exit_code")
    }

    /// Names of the events emitted for a node, in order.
    pub fn event_names_for(&self, node: NodeId) -> Vec<String> {
        unimplemented!("server::plugin_host::ServerKernel::event_names_for")
    }

    /// Sequence numbers of the journal entries written.
    pub fn journal_sequences(&self) -> Vec<u64> {
        unimplemented!("server::plugin_host::ServerKernel::journal_sequences")
    }

    /// The rectangles `(x, y, width, height)` of a split's children at a terminal size.
    pub fn split_rects(&self, split: NodeId, columns: u16, rows: u16) -> Vec<(u16, u16, u16, u16)> {
        unimplemented!("server::plugin_host::ServerKernel::split_rects")
    }

    /// The rectangle `(x, y, width, height)` of a node at a terminal size.
    pub fn layout_rect(&self, node: NodeId, columns: u16, rows: u16) -> (u16, u16, u16, u16) {
        unimplemented!("server::plugin_host::ServerKernel::layout_rect")
    }

    /// The action a key sequence resolves to.
    pub fn resolve_key(&self, keys: &[&str]) -> Option<String> {
        unimplemented!("server::plugin_host::ServerKernel::resolve_key")
    }

    /// A file under the project root.
    pub fn read_project_file(&self, path: &str) -> Option<String> {
        unimplemented!("server::plugin_host::ServerKernel::read_project_file")
    }

    /// Whether a file exists under the project root.
    pub fn project_file_exists(&self, path: &str) -> bool {
        unimplemented!("server::plugin_host::ServerKernel::project_file_exists")
    }

    /// Whether a directory exists under the project root.
    pub fn project_dir_exists(&self, path: &str) -> bool {
        unimplemented!("server::plugin_host::ServerKernel::project_dir_exists")
    }

    /// The permission bits of a file under the project root.
    pub fn project_file_mode(&self, path: &str) -> Option<u32> {
        unimplemented!("server::plugin_host::ServerKernel::project_file_mode")
    }

    /// How many session snapshots were written (COR-120).
    pub fn snapshot_write_count(&self) -> usize {
        unimplemented!("server::plugin_host::ServerKernel::snapshot_write_count")
    }

    /// Names of the backup files.
    pub fn backup_files(&self) -> Vec<String> {
        unimplemented!("server::plugin_host::ServerKernel::backup_files")
    }

    /// Names of the backup files younger than `ms`.
    pub fn backup_files_younger_than_ms(&self, ms: u64) -> Vec<String> {
        unimplemented!("server::plugin_host::ServerKernel::backup_files_younger_than_ms")
    }

    /// The number of panes a pending restore prompt offers, if one is pending (COR-130).
    pub fn pending_restore_prompt(&self) -> Option<usize> {
        unimplemented!("server::plugin_host::ServerKernel::pending_restore_prompt")
    }

    /// Names of the debug log files (COR-180).
    pub fn debug_log_files(&self) -> Vec<String> {
        unimplemented!("server::plugin_host::ServerKernel::debug_log_files")
    }

    /// The debug log directory (COR-180).
    pub fn debug_log_root(&self) -> String {
        unimplemented!("server::plugin_host::ServerKernel::debug_log_root")
    }

    /// The cursor line of an editor pane.
    pub fn editor_cursor_line(&self, pane: NodeId) -> Option<u32> {
        unimplemented!("server::plugin_host::ServerKernel::editor_cursor_line")
    }

    /// The cards of a board column.
    pub fn board_column(&self, pane: NodeId, column: &str) -> Vec<String> {
        unimplemented!("server::plugin_host::ServerKernel::board_column")
    }

    /// Whether an AI request of this kind passed admission.
    pub fn ai_request_admitted(&self, kind: &str) -> bool {
        unimplemented!("server::plugin_host::ServerKernel::ai_request_admitted")
    }

    // ---- HTTP and jobs (parity-and-coverage.md) --------------------------------------------

    /// A POST to the local HTTP listener: `(status, body)`.
    pub fn http_post(&self, path: &str, body: serde_json::Value) -> (u16, serde_json::Value) {
        unimplemented!("server::plugin_host::ServerKernel::http_post")
    }

    /// A GET from the local HTTP listener: `(status, body)`.
    pub fn http_get(&self, path: &str) -> (u16, serde_json::Value) {
        unimplemented!("server::plugin_host::ServerKernel::http_get")
    }

    /// The address of the local HTTP listener, if it is up.
    pub fn http_listener_address(&self) -> Option<SocketAddr> {
        unimplemented!("server::plugin_host::ServerKernel::http_listener_address")
    }

    /// Runs a job on a bank and returns its job id.
    pub fn run_finite_job(&self, bank: &str, job: Box<dyn FnOnce() -> u64 + Send>) -> u64 {
        unimplemented!("server::plugin_host::ServerKernel::run_finite_job")
    }

    /// Runs a job that fails on a bank and returns its job id.
    pub fn run_failing_job(&self, bank: &str) -> u64 {
        unimplemented!("server::plugin_host::ServerKernel::run_failing_job")
    }

    /// The result of a job, once it ended.
    pub fn job_result(&self, job: u64) -> Option<u64> {
        unimplemented!("server::plugin_host::ServerKernel::job_result")
    }

    /// Worker threads of a job bank.
    pub fn bank_worker_count(&self, bank: &str) -> usize {
        unimplemented!("server::plugin_host::ServerKernel::bank_worker_count")
    }

    /// Peak concurrency a job bank reached.
    pub fn bank_peak_concurrency(&self, bank: &str) -> usize {
        unimplemented!("server::plugin_host::ServerKernel::bank_peak_concurrency")
    }
}

impl TreeRead for ServerKernel {
    fn nodes(&self) -> Vec<TreeNode> {
        unimplemented!("server::plugin_host::ServerKernel::nodes")
    }

    fn node(&self, id: NodeId) -> Option<TreeNode> {
        unimplemented!("server::plugin_host::ServerKernel::node")
    }

    fn epoch(&self) -> u64 {
        unimplemented!("server::plugin_host::ServerKernel::epoch")
    }
}

impl PaneRead for ServerKernel {
    fn screen_text(&self, pane: NodeId) -> Option<String> {
        unimplemented!("server::plugin_host::ServerKernel::screen_text")
    }

    fn cells(&self, pane: NodeId, rows: std::ops::Range<u16>) -> Option<Vec<Vec<Cell>>> {
        unimplemented!("server::plugin_host::ServerKernel::cells")
    }

    fn history(&self, pane: NodeId, lines: u32) -> Option<Vec<String>> {
        unimplemented!("server::plugin_host::ServerKernel::history")
    }
}

impl KernelFacade for ServerKernel {
    fn apply_intent(&self, origin: &Origin, intent: Intent) -> IntentResult {
        unimplemented!("server::plugin_host::ServerKernel::apply_intent")
    }

    fn tree(&self) -> &dyn TreeRead {
        self
    }

    fn panes(&self) -> &dyn PaneRead {
        self
    }

    fn process_kind(&self) -> ProcessKind {
        ProcessKind::Server
    }
}

/// Owns the plugin host inside the server process.
#[derive(Debug)]
pub struct ServerPluginHost {
    start_mode: StartMode,
}

impl ServerPluginHost {
    /// A host that will start in the given mode.
    pub fn new(start_mode: StartMode) -> ServerPluginHost {
        ServerPluginHost { start_mode }
    }

    /// Boot order: engine, registries, core plugins, then the rest (reference-host-architecture.md).
    pub fn start(&self) -> StartReport {
        unimplemented!("server::plugin_host::ServerPluginHost::start")
    }

    /// Reverse order within the shutdown budget.
    pub fn shutdown(&self) -> ShutdownReport {
        unimplemented!("server::plugin_host::ServerPluginHost::shutdown")
    }
}
