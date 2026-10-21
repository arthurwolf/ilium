//! The server's plugin host: boot order and the `KernelFacade` over the real tree
//! (reference-host-architecture.md). Skeleton: nothing calls it yet, so no existing code path
//! changes behaviour.
//!
//! The `for_parity_world*` constructors and the drivers and observers below exist for the
//! plugin-parity tests (`ilium-server/tests/plugin_parity`, parity-and-coverage.md): they build a
//! kernel over a temporary project in a start mode and let a test drive it like a running server.

// Not wired into the server yet; the implementation phase calls it from `run()`.

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
pub struct ServerKernel;

impl ServerKernel {
    /// A kernel over a fresh temporary project started in `mode` with this `config.toml`
    /// (parity-and-coverage.md COR-010).
    pub fn for_parity_world(_mode: StartMode, _config_toml: &str) -> ServerKernel {
        unimplemented!("server::plugin_host::ServerKernel::for_parity_world")
    }

    /// Like `for_parity_world`, owning the named session (parity-and-coverage.md COR-010).
    pub fn for_parity_world_named(
        _mode: StartMode,
        _config_toml: &str,
        _session: &str,
    ) -> ServerKernel {
        unimplemented!("server::plugin_host::ServerKernel::for_parity_world_named")
    }

    // ---- Drivers (parity-and-coverage.md) -------------------------------------------------

    /// Runs every pending task until the server is idle.
    pub fn settle(&self) {
        unimplemented!("server::plugin_host::ServerKernel::settle")
    }

    /// Advances the server's clock.
    pub fn advance_clock_ms(&self, _ms: u64) {
        unimplemented!("server::plugin_host::ServerKernel::advance_clock_ms")
    }

    /// Feeds bytes to a pane as if its process wrote them.
    pub fn feed_output(&self, _pane: NodeId, _bytes: &[u8]) {
        unimplemented!("server::plugin_host::ServerKernel::feed_output")
    }

    /// Clicks in a pane (`button` 0 left, 1 middle, 2 right).
    pub fn mouse_click(&self, _pane: NodeId, _column: u16, _row: u16, _button: u8) {
        unimplemented!("server::plugin_host::ServerKernel::mouse_click")
    }

    /// Scrolls the wheel in a pane.
    pub fn mouse_wheel(&self, _pane: NodeId, _column: u16, _row: u16, _up: bool) {
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
    pub fn write_config_file(&self, _text: &str) {
        unimplemented!("server::plugin_host::ServerKernel::write_config_file")
    }

    /// Writes a file under the project root.
    pub fn write_project_file(&self, _path: &str, _text: &str) {
        unimplemented!("server::plugin_host::ServerKernel::write_project_file")
    }

    /// Sets (`Some`) or unsets (`None`) an environment variable the server sees
    /// (parity-and-coverage.md, api-panes.md PAN-00x shell resolution).
    pub fn set_environment(&self, _name: &str, _value: Option<&str>) {
        unimplemented!("server::plugin_host::ServerKernel::set_environment")
    }

    /// Starts a new debug-logging session (COR-180).
    pub fn restart_logging_session(&self) {
        unimplemented!("server::plugin_host::ServerKernel::restart_logging_session")
    }

    /// Completes onboarding, with AI features on or off.
    pub fn finish_onboarding(&self, _enable_ai: bool) {
        unimplemented!("server::plugin_host::ServerKernel::finish_onboarding")
    }

    /// Types text into an editor pane.
    pub fn editor_type(&self, _pane: NodeId, _text: &str) {
        unimplemented!("server::plugin_host::ServerKernel::editor_type")
    }

    /// Saves an editor pane.
    pub fn editor_save(&self, _pane: NodeId) {
        unimplemented!("server::plugin_host::ServerKernel::editor_save")
    }

    /// Adds a card to a board column.
    pub fn board_add_card(&self, _pane: NodeId, _column: &str, _text: &str) {
        unimplemented!("server::plugin_host::ServerKernel::board_add_card")
    }

    /// Creates a board pane with a storage kind at a path.
    pub fn create_board(&self, _storage: &str, _path: &str) -> NodeId {
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
    pub fn setting(&self, _section: &str, _key: &str) -> Option<String> {
        unimplemented!("server::plugin_host::ServerKernel::setting")
    }

    /// Applies a setting as the settings dialog would.
    pub fn apply_setting(&self, _section: &str, _key: &str, _value: &str) -> Result<(), String> {
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
    pub fn pane_input_log(&self, _pane: NodeId) -> Vec<u8> {
        unimplemented!("server::plugin_host::ServerKernel::pane_input_log")
    }

    /// The exit code of a pane's process, once it ended.
    pub fn pane_exit_code(&self, _pane: NodeId) -> Option<i32> {
        unimplemented!("server::plugin_host::ServerKernel::pane_exit_code")
    }

    /// Names of the events emitted for a node, in order.
    pub fn event_names_for(&self, _node: NodeId) -> Vec<String> {
        unimplemented!("server::plugin_host::ServerKernel::event_names_for")
    }

    /// Sequence numbers of the journal entries written.
    pub fn journal_sequences(&self) -> Vec<u64> {
        unimplemented!("server::plugin_host::ServerKernel::journal_sequences")
    }

    /// The rectangles `(x, y, width, height)` of a split's children at a terminal size.
    pub fn split_rects(
        &self,
        _split: NodeId,
        _columns: u16,
        _rows: u16,
    ) -> Vec<(u16, u16, u16, u16)> {
        unimplemented!("server::plugin_host::ServerKernel::split_rects")
    }

    /// The rectangle `(x, y, width, height)` of a node at a terminal size.
    pub fn layout_rect(&self, _node: NodeId, _columns: u16, _rows: u16) -> (u16, u16, u16, u16) {
        unimplemented!("server::plugin_host::ServerKernel::layout_rect")
    }

    /// The action a key sequence resolves to.
    pub fn resolve_key(&self, _keys: &[&str]) -> Option<String> {
        unimplemented!("server::plugin_host::ServerKernel::resolve_key")
    }

    /// A file under the project root.
    pub fn read_project_file(&self, _path: &str) -> Option<String> {
        unimplemented!("server::plugin_host::ServerKernel::read_project_file")
    }

    /// Whether a file exists under the project root.
    pub fn project_file_exists(&self, _path: &str) -> bool {
        unimplemented!("server::plugin_host::ServerKernel::project_file_exists")
    }

    /// Whether a directory exists under the project root.
    pub fn project_dir_exists(&self, _path: &str) -> bool {
        unimplemented!("server::plugin_host::ServerKernel::project_dir_exists")
    }

    /// The permission bits of a file under the project root.
    pub fn project_file_mode(&self, _path: &str) -> Option<u32> {
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
    pub fn backup_files_younger_than_ms(&self, _ms: u64) -> Vec<String> {
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
    pub fn editor_cursor_line(&self, _pane: NodeId) -> Option<u32> {
        unimplemented!("server::plugin_host::ServerKernel::editor_cursor_line")
    }

    /// The cards of a board column.
    pub fn board_column(&self, _pane: NodeId, _column: &str) -> Vec<String> {
        unimplemented!("server::plugin_host::ServerKernel::board_column")
    }

    /// Whether an AI request of this kind passed admission.
    pub fn ai_request_admitted(&self, _kind: &str) -> bool {
        unimplemented!("server::plugin_host::ServerKernel::ai_request_admitted")
    }

    // ---- HTTP and jobs (parity-and-coverage.md) --------------------------------------------

    /// A POST to the local HTTP listener: `(status, body)`.
    pub fn http_post(&self, _path: &str, _body: serde_json::Value) -> (u16, serde_json::Value) {
        unimplemented!("server::plugin_host::ServerKernel::http_post")
    }

    /// A GET from the local HTTP listener: `(status, body)`.
    pub fn http_get(&self, _path: &str) -> (u16, serde_json::Value) {
        unimplemented!("server::plugin_host::ServerKernel::http_get")
    }

    /// The address of the local HTTP listener, if it is up.
    pub fn http_listener_address(&self) -> Option<SocketAddr> {
        unimplemented!("server::plugin_host::ServerKernel::http_listener_address")
    }

    /// Runs a job on a bank and returns its job id.
    pub fn run_finite_job(&self, _bank: &str, _job: Box<dyn FnOnce() -> u64 + Send>) -> u64 {
        unimplemented!("server::plugin_host::ServerKernel::run_finite_job")
    }

    /// Runs a job that fails on a bank and returns its job id.
    pub fn run_failing_job(&self, _bank: &str) -> u64 {
        unimplemented!("server::plugin_host::ServerKernel::run_failing_job")
    }

    /// The result of a job, once it ended.
    pub fn job_result(&self, _job: u64) -> Option<u64> {
        unimplemented!("server::plugin_host::ServerKernel::job_result")
    }

    /// Worker threads of a job bank.
    pub fn bank_worker_count(&self, _bank: &str) -> usize {
        unimplemented!("server::plugin_host::ServerKernel::bank_worker_count")
    }

    /// Peak concurrency a job bank reached.
    pub fn bank_peak_concurrency(&self, _bank: &str) -> usize {
        unimplemented!("server::plugin_host::ServerKernel::bank_peak_concurrency")
    }
}

impl TreeRead for ServerKernel {
    fn nodes(&self) -> Vec<TreeNode> {
        unimplemented!("server::plugin_host::ServerKernel::nodes")
    }

    fn node(&self, _id: NodeId) -> Option<TreeNode> {
        unimplemented!("server::plugin_host::ServerKernel::node")
    }

    fn epoch(&self) -> u64 {
        unimplemented!("server::plugin_host::ServerKernel::epoch")
    }
}

impl PaneRead for ServerKernel {
    fn screen_text(&self, _pane: NodeId) -> Option<String> {
        unimplemented!("server::plugin_host::ServerKernel::screen_text")
    }

    fn cells(&self, _pane: NodeId, _rows: std::ops::Range<u16>) -> Option<Vec<Vec<Cell>>> {
        unimplemented!("server::plugin_host::ServerKernel::cells")
    }

    fn history(&self, _pane: NodeId, _lines: u32) -> Option<Vec<String>> {
        unimplemented!("server::plugin_host::ServerKernel::history")
    }
}

impl KernelFacade for ServerKernel {
    fn apply_intent(&self, _origin: &Origin, _intent: Intent) -> IntentResult {
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

    /// The mode this host was created to start in (`--safe-plugins`, `--no-plugins`, or normal).
    pub fn start_mode(&self) -> StartMode {
        self.start_mode
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

/// What identifies a pane across a process replacement: id, title, place and size
/// (api-pane-lifecycle.md PAN-00x).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneIdentity {
    /// The node id.
    pub node: NodeId,
    /// The title.
    pub title: String,
    /// The parent node and index.
    pub place: (Option<NodeId>, usize),
    /// Columns and rows.
    pub size: (u16, u16),
}

impl ServerKernel {
    /// A kernel over a test tree holding a running, a restored, an exited and a neighbour pane,
    /// with `plugin` holding `grants` (api-panes.md, api-pane-lifecycle.md).
    pub fn for_test_world(_plugin: &str, _grants: &[&str]) -> ServerKernel {
        unimplemented!("server::plugin_host::ServerKernel::for_test_world")
    }

    // ---- Pane lifecycle observers (api-pane-lifecycle.md) ----------------------------------

    /// The lifecycle states a pane passed through, in order.
    pub fn lifecycle_events(
        &self,
        _pane: NodeId,
    ) -> Vec<ilium_plugin_api::generated::types::PaneLifecycleState> {
        unimplemented!("server::plugin_host::ServerKernel::lifecycle_events")
    }

    /// The plugin that caused each lifecycle event (`None` for the user or the kernel).
    pub fn lifecycle_event_causes(&self, _pane: NodeId) -> Vec<Option<String>> {
        unimplemented!("server::plugin_host::ServerKernel::lifecycle_event_causes")
    }

    /// Makes the kernel behave as on Windows (signals unsupported).
    pub fn pretend_platform_windows(&self) {
        unimplemented!("server::plugin_host::ServerKernel::pretend_platform_windows")
    }

    /// The identity a pane keeps across a process replacement.
    pub fn pane_identity(&self, _pane: NodeId) -> PaneIdentity {
        unimplemented!("server::plugin_host::ServerKernel::pane_identity")
    }

    /// The working directory of a pane.
    pub fn pane_cwd(&self, _pane: NodeId) -> PathBuf {
        unimplemented!("server::plugin_host::ServerKernel::pane_cwd")
    }

    /// Whether a node is in the tree.
    pub fn node_exists(&self, _node: NodeId) -> bool {
        unimplemented!("server::plugin_host::ServerKernel::node_exists")
    }

    /// Whether a pane shows its process's exit status.
    pub fn pane_shows_exit_status(&self, _pane: NodeId) -> bool {
        unimplemented!("server::plugin_host::ServerKernel::pane_shows_exit_status")
    }

    /// Makes the pane's process ignore the terminate signal.
    pub fn make_process_ignore_terminate(&self, _pane: NodeId) {
        unimplemented!("server::plugin_host::ServerKernel::make_process_ignore_terminate")
    }

    /// Milliseconds between the terminate and the kill signal sent to a pane's process.
    pub fn kill_was_sent_after_ms(&self, _pane: NodeId) -> u64 {
        unimplemented!("server::plugin_host::ServerKernel::kill_was_sent_after_ms")
    }

    /// A fingerprint of the whole tree (structure, titles, sizes).
    pub fn tree_fingerprint(&self) -> String {
        unimplemented!("server::plugin_host::ServerKernel::tree_fingerprint")
    }

    /// The restructure token of a restructure outcome (api-pane-lifecycle.md PAN-023).
    pub fn last_restructure_token(
        &self,
        _outcome: &ilium_plugin_api::generated::types::Outcome,
    ) -> u64 {
        unimplemented!("server::plugin_host::ServerKernel::last_restructure_token")
    }

    /// The revision counter of a node.
    pub fn node_revision(&self, _node: NodeId) -> u64 {
        unimplemented!("server::plugin_host::ServerKernel::node_revision")
    }

    /// Sets a manual icon on a node, as the user would.
    pub fn set_manual_icon(&self, _node: NodeId, _icon: &str) {
        unimplemented!("server::plugin_host::ServerKernel::set_manual_icon")
    }

    /// The log lines of a plugin.
    pub fn plugin_log(&self, _plugin: &str) -> Vec<String> {
        unimplemented!("server::plugin_host::ServerKernel::plugin_log")
    }

    // ---- Panes (api-panes.md) ---------------------------------------------------------------

    /// Creates a pane as the user would and returns its node.
    pub fn create_pane_for_test(&self) -> NodeId {
        unimplemented!("server::plugin_host::ServerKernel::create_pane_for_test")
    }

    /// The pane info a plugin sees for a node, if it is a pane.
    pub fn pane_info(&self, _node: NodeId) -> Option<ilium_plugin_api::generated::types::PaneInfo> {
        unimplemented!("server::plugin_host::ServerKernel::pane_info")
    }

    /// Restores a pane from a snapshot that carries no size.
    pub fn restore_pane_without_saved_size(&self, _pane: NodeId) {
        unimplemented!("server::plugin_host::ServerKernel::restore_pane_without_saved_size")
    }

    /// Starts the process of a restored pane.
    pub fn start_restored_process(&self, _pane: NodeId) {
        unimplemented!("server::plugin_host::ServerKernel::start_restored_process")
    }

    /// The program of the most recently spawned pane process.
    pub fn last_spawned_program(&self) -> Option<String> {
        unimplemented!("server::plugin_host::ServerKernel::last_spawned_program")
    }

    /// The number of panes in the tree.
    pub fn pane_count(&self) -> usize {
        unimplemented!("server::plugin_host::ServerKernel::pane_count")
    }

    // ---- Detection loop (reference-host-scheduling.md HST-450..455) ------------------------

    /// Runs one detection tick over every pane.
    pub fn run_detection_tick(&self) {
        unimplemented!("server::plugin_host::ServerKernel::run_detection_tick")
    }

    /// Runs one detection tick with the named fixture detector plugin.
    pub fn run_detection_tick_with_fixture_detector(&self, _fixture: &str) {
        unimplemented!(
            "server::plugin_host::ServerKernel::run_detection_tick_with_fixture_detector"
        )
    }

    /// The thread names the detector calls ran on.
    pub fn detector_thread_names(&self) -> Vec<String> {
        unimplemented!("server::plugin_host::ServerKernel::detector_thread_names")
    }

    /// The thread name of the detection loop.
    pub fn detection_loop_thread_name(&self) -> String {
        unimplemented!("server::plugin_host::ServerKernel::detection_loop_thread_name")
    }

    /// Makes every detector call block for `ms`.
    pub fn set_detector_block_ms(&self, _ms: u64) {
        unimplemented!("server::plugin_host::ServerKernel::set_detector_block_ms")
    }

    /// Makes the detector calls of one plugin block for `ms`.
    pub fn set_detector_block_ms_for(&self, _plugin: &str, _ms: u64) {
        unimplemented!("server::plugin_host::ServerKernel::set_detector_block_ms_for")
    }

    /// The number of detector calls the last tick made.
    pub fn detector_calls_in_last_tick(&self) -> usize {
        unimplemented!("server::plugin_host::ServerKernel::detector_calls_in_last_tick")
    }

    /// Waits up to `timeout` for a detector call of `plugin`; true when it happened.
    pub fn wait_for_detector_call(&self, _plugin: &str, _timeout: std::time::Duration) -> bool {
        unimplemented!("server::plugin_host::ServerKernel::wait_for_detector_call")
    }
}

#[cfg(test)]
mod spec_tests {
    use super::*;

    /// REQ: [HST-033] (`--safe-plugins` and `--no-plugins` select the start mode) and reference-host-architecture.md "Safe start".
    #[test]
    fn host_reports_the_start_mode_it_was_created_with() {
        for mode in [StartMode::Normal, StartMode::SafePlugins, StartMode::NoPlugins] {
            let host = ServerPluginHost::new(mode);
            assert_eq!(host.start_mode(), mode);
        }
    }

    /// REQ: [HST-033] and [HST-034] (`--no-plugins` starts no plugin host; the session still boots).
    #[test]
    fn no_plugins_start_activates_nothing() {
        let host = ServerPluginHost::new(StartMode::NoPlugins);
        let report = host.start();
        assert!(report.started.is_empty(), "no plugin may start: {:?}", report.started);
        assert!(report.failed.is_empty(), "no plugin may fail: {:?}", report.failed);
    }

    /// REQ: reference-host-architecture.md "Server shutdown" step 3 (each active plugin receives deactivate; plugins are deactivated or dropped at the budget).
    #[test]
    fn shutdown_accounts_for_every_started_plugin_exactly_once() {
        let host = ServerPluginHost::new(StartMode::Normal);
        let start_report = host.start();
        let shutdown_report = host.shutdown();
        let mut accounted: Vec<String> = shutdown_report
            .deactivated
            .iter()
            .chain(shutdown_report.forced.iter())
            .cloned()
            .collect();
        let mut started = start_report.started.clone();
        accounted.sort();
        started.sort();
        assert_eq!(accounted, started);
    }

    /// REQ: parity-and-coverage.md driver contract for `feed_output` and `pane_input_log` (output is what the process writes; input is what is written to the process).
    #[test]
    fn fed_output_is_not_recorded_as_input_to_the_pane_process() {
        let kernel = ServerKernel::for_parity_world(StartMode::NoPlugins, "");
        let pane = kernel.create_pane_for_test();
        kernel.feed_output(pane, b"hello");
        assert!(kernel.pane_input_log(pane).is_empty());
    }

    /// REQ: parity-and-coverage.md driver contract for `write_config_file` and `config_text` (replace, then read back, including empty text).
    #[test]
    fn config_text_returns_what_write_config_file_stored() {
        let kernel = ServerKernel::for_parity_world(StartMode::NoPlugins, "");
        kernel.write_config_file("[ui]\ntree_order = \"manual\"\n");
        assert_eq!(kernel.config_text(), "[ui]\ntree_order = \"manual\"\n");
        kernel.write_config_file("");
        assert_eq!(kernel.config_text(), "");
    }

    /// REQ: parity-and-coverage.md driver contract for `write_project_file` and `read_project_file`; a missing file is `None` and does not exist.
    #[test]
    fn project_file_round_trips_and_missing_file_is_absent() {
        let kernel = ServerKernel::for_parity_world(StartMode::NoPlugins, "");
        assert_eq!(kernel.read_project_file("notes/missing.txt"), None);
        assert!(!kernel.project_file_exists("notes/missing.txt"));
        kernel.write_project_file("notes/a.txt", "hello");
        assert_eq!(kernel.read_project_file("notes/a.txt"), Some("hello".to_string()));
        assert!(kernel.project_file_exists("notes/a.txt"));
    }

    /// REQ: [HST-026] (a restart restores the tree from the snapshot, and the restored tree is unchanged) and reference-host-architecture.md "Server start" steps 2 and 7; COR-130 per the `restart_from_disk` doc.
    #[test]
    fn restart_from_disk_keeps_the_tree_structure() {
        let kernel = ServerKernel::for_parity_world(StartMode::NoPlugins, "");
        let _pane = kernel.create_pane_for_test();
        let fingerprint_before = kernel.tree_fingerprint();
        let pane_count_before = kernel.pane_count();
        let restarted = kernel.restart_from_disk();
        assert_eq!(restarted.tree_fingerprint(), fingerprint_before);
        assert_eq!(restarted.pane_count(), pane_count_before);
    }

    /// REQ: [PAN-004] (a process replacement keeps node id, title, place and size) and the `pane_identity` doc in api-pane-lifecycle.md.
    #[test]
    fn starting_a_restored_process_keeps_the_pane_identity() {
        let kernel = ServerKernel::for_test_world("spec.plugin", &[]);
        let pane = kernel.create_pane_for_test();
        let identity_before = kernel.pane_identity(pane);
        kernel.start_restored_process(pane);
        assert_eq!(kernel.pane_identity(pane), identity_before);
    }

    /// REQ: [HST-450] (detector calls run on the plugin executor, never on the detection loop thread) and [HST-451] (one detector call per pane, at most one call per pane per tick).
    #[test]
    fn detector_calls_stay_off_the_detection_loop_thread_and_respect_one_per_pane() {
        let kernel = ServerKernel::for_test_world("spec.plugin", &[]);
        kernel.run_detection_tick();
        let loop_thread = kernel.detection_loop_thread_name();
        for thread_name in kernel.detector_thread_names() {
            assert_ne!(thread_name, loop_thread);
        }
        assert!(kernel.detector_calls_in_last_tick() <= kernel.pane_count());
    }

    /// REQ: invalid input: a job id that never ran has no result (`job_result` reports a result only once a job ended).
    #[test]
    fn job_result_is_absent_for_a_job_that_never_ran() {
        let kernel = ServerKernel::for_parity_world(StartMode::NoPlugins, "");
        assert_eq!(kernel.job_result(u64::MAX), None);
    }
}
