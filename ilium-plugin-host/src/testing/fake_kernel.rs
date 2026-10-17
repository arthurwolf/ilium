//! An in-memory kernel behind `KernelFacade` (test plan section 1.3 `testing`).
//!
//! Beyond the facade, the fake kernel exposes setters and observers the conformance lanes use to
//! stage panes, agents, editors, host services and failure injection. The manual does not
//! document these hooks; each names the requirement family of its callers. Bodies are
//! `unimplemented!()` placeholders.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::Duration;

use ilium_plugin_api::generated::host::audio::AudioDevice;
use ilium_plugin_api::generated::host::keymap::Binding;
use ilium_plugin_api::generated::host::media::VideoInfo;
use ilium_plugin_api::generated::host::sessions::LiveSession;
use ilium_plugin_api::generated::primitives::agent::{SessionRef, SignatureSpec, TranscriptTurn};
use ilium_plugin_api::generated::primitives::terminal::{Selection, TerminalMode};
use ilium_plugin_api::generated::types::editing::TextRange;
use ilium_plugin_api::generated::types::events::Event;
use ilium_plugin_api::generated::types::geo::GeoPlace;
use ilium_plugin_api::generated::types::ids::{BufferId, ClientId, NodeId, RequestId};
use ilium_plugin_api::generated::types::tree::{AgentActivity, AgentModel};
use ilium_plugin_api::generated::types::ui::{
    CatalogItem, Density, IconMode, PopupOptions, WidgetState,
};
use ilium_plugin_api::generated::types::Intent;
use ilium_plugin_api::kernel::{
    Cell, IntentResult, KernelFacade, Origin, PaneRead, TreeNode, TreeRead,
};
use ilium_plugin_api::{ErrorCode, PluginError, ProcessKind, Side};

/// The screen of one fake pane.
#[derive(Debug, Clone, Default)]
pub struct FakePaneScreen {
    /// Visible text, one entry per row.
    pub rows: Vec<String>,
    /// Lines of history, oldest first.
    pub history: Vec<String>,
}

/// The answer the fake kernel produced for one HTTP API call (WIT:ilium:host/http-api).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HttpAnswer {
    /// HTTP status code.
    pub status: u16,
    /// Response headers in order.
    pub headers: Vec<(String, String)>,
    /// Response body.
    pub body: Vec<u8>,
}

/// An in-memory tree and pane screens; every applied intent is recorded.
#[derive(Debug)]
pub struct FakeKernel {
    process_kind: ProcessKind,
    state: Mutex<FakeKernelState>,
}

#[derive(Debug, Default)]
struct FakeKernelState {
    nodes: Vec<TreeNode>,
    epoch: u64,
    screens: BTreeMap<u64, FakePaneScreen>,
    applied: Vec<(Origin, Intent)>,
}

impl FakeKernel {
    /// An empty kernel running as the given process.
    pub fn new(process_kind: ProcessKind) -> FakeKernel {
        FakeKernel {
            process_kind,
            state: Mutex::new(FakeKernelState::default()),
        }
    }

    /// Replaces the tree and bumps the epoch.
    pub fn set_tree(&self, nodes: Vec<TreeNode>) {
        unimplemented!("host::testing::FakeKernel::set_tree")
    }

    /// Sets the screen of a pane.
    pub fn set_screen(&self, pane: NodeId, screen: FakePaneScreen) {
        unimplemented!("host::testing::FakeKernel::set_screen")
    }

    /// Every intent applied so far, with its origin, in order.
    pub fn applied_intents(&self) -> Vec<(Origin, Intent)> {
        unimplemented!("host::testing::FakeKernel::applied_intents")
    }

    // ---- events and intent plumbing (HST-49x events and intents)

    /// Emits a kernel event to the plugins.
    pub fn emit_event(&self, event: Event) {
        unimplemented!("host::testing::FakeKernel::emit_event")
    }

    /// Marks a pane as restored from a snapshot (or not).
    pub fn set_pane_restored(&self, pane: NodeId, restored: bool) {
        unimplemented!("host::testing::FakeKernel::set_pane_restored")
    }

    /// The ordered log of request steps (`journal`, `pane-write`, ...).
    pub fn step_log(&self) -> Vec<String> {
        unimplemented!("host::testing::FakeKernel::step_log")
    }

    /// Interrupts the server right after this pane's journal entry with this key.
    pub fn interrupt_after_journal(&self, pane: NodeId, key: &str) {
        unimplemented!("host::testing::FakeKernel::interrupt_after_journal")
    }

    /// Holds the next request open until the test completes it (CAP revocation mid-request).
    pub fn hold_next_request(&self) {
        unimplemented!("host::testing::FakeKernel::hold_next_request")
    }

    /// Fails the next pane write with this error code.
    pub fn fail_next_write(&self, code: ErrorCode) {
        unimplemented!("host::testing::FakeKernel::fail_next_write")
    }

    /// Vetoes the next hook with this reason.
    pub fn veto_next_hook(&self, reason: &str) {
        unimplemented!("host::testing::FakeKernel::veto_next_hook")
    }

    /// Sides that applied the named intent case (HST-490).
    pub fn sides_that_applied(&self, case: &str) -> Vec<Side> {
        unimplemented!("host::testing::FakeKernel::sides_that_applied")
    }

    /// Freezes server-side tree reads so client reads must come from the mirror (HST-49x).
    pub fn freeze_server_tree_reads(&self) {
        unimplemented!("host::testing::FakeKernel::freeze_server_tree_reads")
    }

    /// Raw bytes written to panes so far (HST-4xx).
    pub fn raw_bytes_sent(&self) -> u64 {
        unimplemented!("host::testing::FakeKernel::raw_bytes_sent")
    }

    // ---- terminal primitives (PRI terminal)

    /// Appends lines to a pane's screen.
    pub fn emit_lines(&self, pane: NodeId, lines: Vec<String>) {
        unimplemented!("host::testing::FakeKernel::emit_lines")
    }

    /// Marks a pane's screen as changed.
    pub fn change_screen(&self, pane: NodeId) {
        unimplemented!("host::testing::FakeKernel::change_screen")
    }

    /// Sets or clears a pane's selection.
    pub fn set_selection(&self, pane: NodeId, selection: Option<Selection>) {
        unimplemented!("host::testing::FakeKernel::set_selection")
    }

    /// Sets a pane's terminal modes.
    pub fn set_modes(&self, pane: NodeId, modes: TerminalMode) {
        unimplemented!("host::testing::FakeKernel::set_modes")
    }

    /// Sets a pane's foreground process name and arguments.
    pub fn set_foreground(&self, pane: NodeId, name: &str, arguments: &[&str]) {
        unimplemented!("host::testing::FakeKernel::set_foreground")
    }

    /// Sets a pane's size in columns and rows.
    pub fn set_pane_size(&self, pane: NodeId, columns: u16, rows: u16) {
        unimplemented!("host::testing::FakeKernel::set_pane_size")
    }

    /// Removes a node from the tree.
    pub fn remove_node(&self, node: NodeId) {
        unimplemented!("host::testing::FakeKernel::remove_node")
    }

    /// Whether a node exists.
    pub fn node_exists(&self, node: NodeId) -> bool {
        unimplemented!("host::testing::FakeKernel::node_exists")
    }

    /// The pane with focus.
    pub fn focused_pane(&self) -> Option<NodeId> {
        unimplemented!("host::testing::FakeKernel::focused_pane")
    }

    // ---- agent primitives (PRI agent)

    /// A user-defined signature that wins over plugin signatures (PRI-011).
    pub fn set_user_signature(&self, signature: SignatureSpec) {
        unimplemented!("host::testing::FakeKernel::set_user_signature")
    }

    /// Declares a signature id in the plugin manifest.
    pub fn contribute_signature_in_manifest(&self, id: &str) {
        unimplemented!("host::testing::FakeKernel::contribute_signature_in_manifest")
    }

    /// The signature id that identifies this process name, if any.
    pub fn identify_process_name(&self, name: &str) -> Option<String> {
        unimplemented!("host::testing::FakeKernel::identify_process_name")
    }

    /// Declares a toolbar button of the harness plugin.
    pub fn contribute_toolbar_button(&self, name: &str) {
        unimplemented!("host::testing::FakeKernel::contribute_toolbar_button")
    }

    /// Declares a toolbar button of another plugin.
    pub fn contribute_toolbar_button_for(&self, plugin: &str, name: &str) {
        unimplemented!("host::testing::FakeKernel::contribute_toolbar_button_for")
    }

    /// The (enabled, label) state of a toolbar button, globally or for one pane.
    pub fn toolbar_state(
        &self,
        name: &str,
        pane: Option<NodeId>,
    ) -> Option<(bool, Option<String>)> {
        unimplemented!("host::testing::FakeKernel::toolbar_state")
    }

    /// Attaches an agent session to a pane.
    pub fn set_agent_session(&self, pane: NodeId, session: SessionRef) {
        unimplemented!("host::testing::FakeKernel::set_agent_session")
    }

    /// Whether the pane's agent session file has a known format.
    pub fn set_agent_session_format(&self, pane: NodeId, known: bool) {
        unimplemented!("host::testing::FakeKernel::set_agent_session_format")
    }

    /// The last prompt of a pane's agent.
    pub fn set_last_prompt(&self, pane: NodeId, prompt: &str) {
        unimplemented!("host::testing::FakeKernel::set_last_prompt")
    }

    /// Bytes the user's keyboard writes to the pane, as one chunk of terminal input. The kernel
    /// reconstructs the agent composer text from them; an Enter that follows exact text records
    /// that text as the pane's last prompt, an Enter that follows text the kernel cannot vouch
    /// for records the prompt as unavailable (`last-prompt` returns none).
    pub fn user_types_into(&self, pane: NodeId, bytes: &[u8]) {
        unimplemented!("host::testing::FakeKernel::user_types_into")
    }

    /// Bytes written to the pane by something other than the user (a plugin or an automation).
    /// They edit the composer behind the user's back, so the prompt they belong to is unavailable.
    pub fn automation_writes_into(&self, pane: NodeId, bytes: &[u8]) {
        unimplemented!("host::testing::FakeKernel::automation_writes_into")
    }

    /// The prompt epoch of a pane's agent.
    pub fn set_prompt_epoch(&self, pane: NodeId, epoch: u64) {
        unimplemented!("host::testing::FakeKernel::set_prompt_epoch")
    }

    /// The model of a pane's agent.
    pub fn set_agent_model(&self, pane: NodeId, model: AgentModel) {
        unimplemented!("host::testing::FakeKernel::set_agent_model")
    }

    /// The transcript of a pane's agent.
    pub fn set_agent_transcript(&self, pane: NodeId, turns: Vec<TranscriptTurn>) {
        unimplemented!("host::testing::FakeKernel::set_agent_transcript")
    }

    /// The activity of a pane as the kernel reports it.
    pub fn pane_activity(&self, pane: NodeId) -> Option<AgentActivity> {
        unimplemented!("host::testing::FakeKernel::pane_activity")
    }

    /// The activity a pane had before any detector ran.
    pub fn initial_activity(&self, pane: NodeId) -> Option<AgentActivity> {
        unimplemented!("host::testing::FakeKernel::initial_activity")
    }

    // ---- editor primitives (PRI editor)

    /// Attaches a buffer with this text to a pane and returns its id.
    pub fn attach_buffer(&self, pane: NodeId, text: &str) -> BufferId {
        unimplemented!("host::testing::FakeKernel::attach_buffer")
    }

    /// Sets the editor viewport (first row, first column, columns, rows).
    pub fn set_editor_viewport(
        &self,
        pane: NodeId,
        first_row: u32,
        first_column: u32,
        columns: u32,
        rows: u32,
    ) {
        unimplemented!("host::testing::FakeKernel::set_editor_viewport")
    }

    /// Sets a buffer's selections.
    pub fn set_selections(&self, buffer: BufferId, ranges: Vec<TextRange>) {
        unimplemented!("host::testing::FakeKernel::set_selections")
    }

    /// Applies the n-th scripted edit to a buffer.
    pub fn edit_buffer(&self, buffer: BufferId, index: u32) {
        unimplemented!("host::testing::FakeKernel::edit_buffer")
    }

    /// How often the annotator export ran for a buffer.
    pub fn annotator_calls(&self, buffer: BufferId) -> u32 {
        unimplemented!("host::testing::FakeKernel::annotator_calls")
    }

    /// Writes a file into a named root (`project`, ...).
    pub fn put_file(&self, root: &str, path: &str, bytes: &[u8]) {
        unimplemented!("host::testing::FakeKernel::put_file")
    }

    /// Reads a file from a named root.
    pub fn file(&self, root: &str, path: &str) -> Option<Vec<u8>> {
        unimplemented!("host::testing::FakeKernel::file")
    }

    // ---- basic primitives (PRI basic)

    /// Whether a pane is marked dirty.
    pub fn pane_is_dirty(&self, pane: NodeId) -> bool {
        unimplemented!("host::testing::FakeKernel::pane_is_dirty")
    }

    /// Whether a close confirmation is pending for a pane.
    pub fn pending_confirmation(&self, pane: NodeId) -> bool {
        unimplemented!("host::testing::FakeKernel::pending_confirmation")
    }

    /// A pane's title.
    pub fn pane_title(&self, pane: NodeId) -> Option<String> {
        unimplemented!("host::testing::FakeKernel::pane_title")
    }

    /// Sets a pane's title as the user would.
    pub fn set_manual_title(&self, pane: NodeId, title: &str) {
        unimplemented!("host::testing::FakeKernel::set_manual_title")
    }

    /// A pane's status text.
    pub fn pane_status_text(&self, pane: NodeId) -> Option<String> {
        unimplemented!("host::testing::FakeKernel::pane_status_text")
    }

    // ---- catalogs, audio, workspace, http-api (WIT:ilium:host/*)

    /// Replaces the items of a catalog.
    pub fn set_catalog(&self, catalog: &str, items: Vec<CatalogItem>) {
        unimplemented!("host::testing::FakeKernel::set_catalog")
    }

    /// Makes a catalog's provider answer after this delay.
    pub fn set_catalog_provider_delay(&self, catalog: &str, delay: Duration) {
        unimplemented!("host::testing::FakeKernel::set_catalog_provider_delay")
    }

    /// The audio devices present.
    pub fn set_audio_devices(&self, devices: Vec<AudioDevice>) {
        unimplemented!("host::testing::FakeKernel::set_audio_devices")
    }

    /// Ids of the open audio captures.
    pub fn open_captures(&self) -> Vec<u64> {
        unimplemented!("host::testing::FakeKernel::open_captures")
    }

    /// Whether loopback capture is available.
    pub fn set_loopback_available(&self, available: bool) {
        unimplemented!("host::testing::FakeKernel::set_loopback_available")
    }

    /// Sample buffers played so far.
    pub fn played_audio(&self) -> Vec<Vec<i16>> {
        unimplemented!("host::testing::FakeKernel::played_audio")
    }

    /// Marks a path in a root as a git repository.
    pub fn set_git_repository(&self, root: &str, path: &str) {
        unimplemented!("host::testing::FakeKernel::set_git_repository")
    }

    /// Marks a registered workspace as in use.
    pub fn set_workspace_in_use(&self, workspace: u64, in_use: bool) {
        unimplemented!("host::testing::FakeKernel::set_workspace_in_use")
    }

    /// Starts an HTTP API call and returns its id.
    pub fn start_http_call(&self, method: &str, path: &str) -> u64 {
        unimplemented!("host::testing::FakeKernel::start_http_call")
    }

    /// The answer delivered for an HTTP API call.
    pub fn http_response(&self, call: u64) -> Option<HttpAnswer> {
        unimplemented!("host::testing::FakeKernel::http_response")
    }

    // ---- ui-host (WIT:ilium:host/ui-host)

    /// Declares a panel of the harness plugin.
    pub fn declare_panel(&self, panel: &str) {
        unimplemented!("host::testing::FakeKernel::declare_panel")
    }

    /// Commits a view tree of (widget kind, key) pairs and popup keys for a panel.
    pub fn commit_view_tree(&self, panel: &str, widgets: &[(&str, &str)], popups: &[&str]) {
        unimplemented!("host::testing::FakeKernel::commit_view_tree")
    }

    /// Keys of the open panels.
    pub fn open_panels(&self) -> Vec<String> {
        unimplemented!("host::testing::FakeKernel::open_panels")
    }

    /// Keys of the open popups.
    pub fn open_popups(&self) -> Vec<String> {
        unimplemented!("host::testing::FakeKernel::open_popups")
    }

    /// Options of an open popup.
    pub fn popup_options(&self, popup: &str) -> Option<PopupOptions> {
        unimplemented!("host::testing::FakeKernel::popup_options")
    }

    /// How often a panel was rendered.
    pub fn render_count(&self, panel: &str) -> u32 {
        unimplemented!("host::testing::FakeKernel::render_count")
    }

    /// Sets the state of a widget in a panel.
    pub fn set_widget_state(&self, panel: &str, widget: &str, state: WidgetState) {
        unimplemented!("host::testing::FakeKernel::set_widget_state")
    }

    /// Sets the theme accent colour and dark flag.
    pub fn set_theme(&self, accent_rgb: u32, dark: bool) {
        unimplemented!("host::testing::FakeKernel::set_theme")
    }

    /// Sets the icon mode.
    pub fn set_icon_mode(&self, mode: IconMode) {
        unimplemented!("host::testing::FakeKernel::set_icon_mode")
    }

    /// Sets the density.
    pub fn set_density(&self, density: Density) {
        unimplemented!("host::testing::FakeKernel::set_density")
    }

    /// Sets reduced motion.
    pub fn set_reduced_motion(&self, reduced: bool) {
        unimplemented!("host::testing::FakeKernel::set_reduced_motion")
    }

    /// Pixel rows of a surface widget (first row, row count).
    pub fn surface_rows(&self, panel: &str, surface: &str, first_row: u32, rows: u32) -> Vec<u8> {
        unimplemented!("host::testing::FakeKernel::surface_rows")
    }

    // ---- cli (WIT:ilium:host/cli)

    /// Starts a CLI invocation (command, output format) and returns its id.
    pub fn start_cli_invocation(&self, command: &str, format: &str) -> u64 {
        unimplemented!("host::testing::FakeKernel::start_cli_invocation")
    }

    /// Starts a CLI invocation with this stdin.
    pub fn start_cli_invocation_with_stdin(
        &self,
        command: &str,
        format: &str,
        stdin: &[u8],
    ) -> u64 {
        unimplemented!("host::testing::FakeKernel::start_cli_invocation_with_stdin")
    }

    /// Starts a CLI invocation with stdin closed.
    pub fn start_cli_invocation_without_stdin(&self, command: &str, format: &str) -> u64 {
        unimplemented!("host::testing::FakeKernel::start_cli_invocation_without_stdin")
    }

    /// Starts a CLI invocation with (stdin, stdout) tty flags.
    pub fn start_cli_invocation_with_tty(
        &self,
        command: &str,
        format: &str,
        tty: (bool, bool),
    ) -> u64 {
        unimplemented!("host::testing::FakeKernel::start_cli_invocation_with_tty")
    }

    /// Starts a CLI invocation from inside a pane.
    pub fn start_cli_invocation_from_pane(&self, command: &str, format: &str, pane: NodeId) -> u64 {
        unimplemented!("host::testing::FakeKernel::start_cli_invocation_from_pane")
    }

    /// Starts a CLI invocation with selected panes and a recursive flag.
    pub fn start_cli_invocation_with_selection(
        &self,
        command: &str,
        format: &str,
        panes: Vec<NodeId>,
        recursive: bool,
    ) -> u64 {
        unimplemented!("host::testing::FakeKernel::start_cli_invocation_with_selection")
    }

    /// Stdout lines of a CLI invocation.
    pub fn cli_stdout(&self, invocation: u64) -> Vec<String> {
        unimplemented!("host::testing::FakeKernel::cli_stdout")
    }

    /// Stdout of a CLI invocation as text.
    pub fn cli_stdout_text(&self, invocation: u64) -> String {
        unimplemented!("host::testing::FakeKernel::cli_stdout_text")
    }

    /// Stderr of a CLI invocation as text.
    pub fn cli_stderr_text(&self, invocation: u64) -> String {
        unimplemented!("host::testing::FakeKernel::cli_stderr_text")
    }

    /// Error messages of a CLI invocation.
    pub fn cli_errors(&self, invocation: u64) -> Vec<String> {
        unimplemented!("host::testing::FakeKernel::cli_errors")
    }

    /// Exit code of a finished CLI invocation.
    pub fn cli_exit_code(&self, invocation: u64) -> Option<i32> {
        unimplemented!("host::testing::FakeKernel::cli_exit_code")
    }

    // ---- sounds, clipboard, sessions, geocode, inference, vector, settings, keymap

    /// Sound names the harness plugin ships.
    pub fn set_plugin_sounds(&self, names: &[&str]) {
        unimplemented!("host::testing::FakeKernel::set_plugin_sounds")
    }

    /// Sound names another plugin ships.
    pub fn set_other_plugin_sounds(&self, plugin: &str, names: &[&str]) {
        unimplemented!("host::testing::FakeKernel::set_other_plugin_sounds")
    }

    /// Sets or clears the clipboard text.
    pub fn set_clipboard(&self, text: Option<String>) {
        unimplemented!("host::testing::FakeKernel::set_clipboard")
    }

    /// The clipboard text.
    pub fn clipboard(&self) -> Option<String> {
        unimplemented!("host::testing::FakeKernel::clipboard")
    }

    /// The live sessions.
    pub fn set_live_sessions(&self, sessions: Vec<LiveSession>) {
        unimplemented!("host::testing::FakeKernel::set_live_sessions")
    }

    /// Commands sent to a session so far.
    pub fn session_commands(&self, session: &str) -> Vec<String> {
        unimplemented!("host::testing::FakeKernel::session_commands")
    }

    /// The reply a session gives to its next command.
    pub fn set_session_reply(&self, session: &str, reply_json: &str) {
        unimplemented!("host::testing::FakeKernel::set_session_reply")
    }

    /// Places the geocoder returns.
    pub fn set_geocode_places(&self, places: Vec<GeoPlace>) {
        unimplemented!("host::testing::FakeKernel::set_geocode_places")
    }

    /// Makes the geocoder answer nothing.
    pub fn set_geocode_silent(&self, silent: bool) {
        unimplemented!("host::testing::FakeKernel::set_geocode_silent")
    }

    /// Selects the inference provider plugin.
    pub fn select_inference_provider(&self, plugin: &str) {
        unimplemented!("host::testing::FakeKernel::select_inference_provider")
    }

    /// Sets the embedder's dimension, or removes it.
    pub fn set_embedder(&self, dimension: Option<u32>) {
        unimplemented!("host::testing::FakeKernel::set_embedder")
    }

    /// The vector index entry limit.
    pub fn set_vector_entry_limit(&self, limit: u32) {
        unimplemented!("host::testing::FakeKernel::set_vector_entry_limit")
    }

    /// A kernel settings tab's JSON.
    pub fn set_kernel_tab(&self, tab: &str, json: &str) {
        unimplemented!("host::testing::FakeKernel::set_kernel_tab")
    }

    /// A secret value in a kernel settings tab.
    pub fn set_kernel_tab_secret(&self, tab: &str, key: &str, value: &str) {
        unimplemented!("host::testing::FakeKernel::set_kernel_tab_secret")
    }

    /// The keymap bindings.
    pub fn set_keymap(&self, bindings: Vec<Binding>) {
        unimplemented!("host::testing::FakeKernel::set_keymap")
    }

    // ---- client requests, gpu, location, keys, help, plugins, media

    /// Registers a client that answers these request kinds.
    pub fn register_client(&self, client: ClientId, kinds: &[&str]) {
        unimplemented!("host::testing::FakeKernel::register_client")
    }

    /// (kind, payload) requests waiting at a client.
    pub fn client_inbox(&self, client: ClientId) -> Vec<(String, String)> {
        unimplemented!("host::testing::FakeKernel::client_inbox")
    }

    /// A client answers a request.
    pub fn client_answers(
        &self,
        client: ClientId,
        request: RequestId,
        answer: Result<String, PluginError>,
    ) {
        unimplemented!("host::testing::FakeKernel::client_answers")
    }

    /// Starts a client-request call to the plugin's export and returns its id.
    pub fn start_client_request_call(&self, kind: &str, payload: &str) -> u64 {
        unimplemented!("host::testing::FakeKernel::start_client_request_call")
    }

    /// The answer the plugin gave to a client-request call.
    pub fn client_request_answer(&self, call: u64) -> Option<Result<String, PluginError>> {
        unimplemented!("host::testing::FakeKernel::client_request_answer")
    }

    /// Whether a GPU is available.
    pub fn set_gpu_available(&self, available: bool) {
        unimplemented!("host::testing::FakeKernel::set_gpu_available")
    }

    /// The device location as (latitude, longitude), or none.
    pub fn set_location(&self, location: Option<(f64, f64)>) {
        unimplemented!("host::testing::FakeKernel::set_location")
    }

    /// Ids of the active key grabs.
    pub fn active_grabs(&self) -> Vec<u64> {
        unimplemented!("host::testing::FakeKernel::active_grabs")
    }

    /// Another plugin holds this chord.
    pub fn hold_chord_for(&self, plugin: &str, chord: &str) {
        unimplemented!("host::testing::FakeKernel::hold_chord_for")
    }

    /// Help topics as (id, markdown).
    pub fn set_help_topics(&self, topics: &[(&str, &str)]) {
        unimplemented!("host::testing::FakeKernel::set_help_topics")
    }

    /// The installed plugin ids.
    pub fn set_installed_plugins(&self, plugins: &[&str]) {
        unimplemented!("host::testing::FakeKernel::set_installed_plugins")
    }

    /// Whether a plugin is enabled.
    pub fn plugin_enabled(&self, plugin: &str) -> bool {
        unimplemented!("host::testing::FakeKernel::plugin_enabled")
    }

    /// A video file in a root with this info.
    pub fn set_video(&self, root: &str, path: &str, info: VideoInfo) {
        unimplemented!("host::testing::FakeKernel::set_video")
    }

    /// Whether ffmpeg is available.
    pub fn set_ffmpeg_available(&self, available: bool) {
        unimplemented!("host::testing::FakeKernel::set_ffmpeg_available")
    }

    /// The info of videos opened from bytes.
    pub fn set_bytes_video(&self, info: VideoInfo) {
        unimplemented!("host::testing::FakeKernel::set_bytes_video")
    }

    /// The playback position of an open video in milliseconds.
    pub fn video_position(&self, video: u64) -> Option<u64> {
        unimplemented!("host::testing::FakeKernel::video_position")
    }
}

impl TreeRead for FakeKernel {
    fn nodes(&self) -> Vec<TreeNode> {
        unimplemented!("host::testing::FakeKernel::nodes")
    }

    fn node(&self, id: NodeId) -> Option<TreeNode> {
        unimplemented!("host::testing::FakeKernel::node")
    }

    fn epoch(&self) -> u64 {
        unimplemented!("host::testing::FakeKernel::epoch")
    }
}

impl PaneRead for FakeKernel {
    fn screen_text(&self, pane: NodeId) -> Option<String> {
        unimplemented!("host::testing::FakeKernel::screen_text")
    }

    fn cells(&self, pane: NodeId, rows: std::ops::Range<u16>) -> Option<Vec<Vec<Cell>>> {
        unimplemented!("host::testing::FakeKernel::cells")
    }

    fn history(&self, pane: NodeId, lines: u32) -> Option<Vec<String>> {
        unimplemented!("host::testing::FakeKernel::history")
    }
}

impl KernelFacade for FakeKernel {
    fn apply_intent(&self, origin: &Origin, intent: Intent) -> IntentResult {
        unimplemented!("host::testing::FakeKernel::apply_intent")
    }

    fn tree(&self) -> &dyn TreeRead {
        self
    }

    fn panes(&self) -> &dyn PaneRead {
        self
    }

    fn process_kind(&self) -> ProcessKind {
        self.process_kind
    }
}
