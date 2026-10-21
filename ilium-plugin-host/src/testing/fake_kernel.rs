//! An in-memory kernel behind `KernelFacade` (test plan section 1.3 `testing`).
//!
//! Beyond the facade, the fake kernel exposes setters and observers the conformance lanes use to
//! stage panes, agents, editors, host services and failure injection. The manual does not
//! document these hooks; each names the requirement family of its callers. Bodies are
//! `unimplemented!()` placeholders.

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
}

impl FakeKernel {
    /// An empty kernel running as the given process.
    pub fn new(process_kind: ProcessKind) -> FakeKernel {
        FakeKernel { process_kind }
    }

    /// Replaces the tree and bumps the epoch.
    pub fn set_tree(&self, _nodes: Vec<TreeNode>) {
        unimplemented!("host::testing::FakeKernel::set_tree")
    }

    /// Sets the screen of a pane.
    pub fn set_screen(&self, _pane: NodeId, _screen: FakePaneScreen) {
        unimplemented!("host::testing::FakeKernel::set_screen")
    }

    /// Every intent applied so far, with its origin, in order.
    pub fn applied_intents(&self) -> Vec<(Origin, Intent)> {
        unimplemented!("host::testing::FakeKernel::applied_intents")
    }

    // ---- events and intent plumbing (HST-49x events and intents)

    /// Emits a kernel event to the plugins.
    pub fn emit_event(&self, _event: Event) {
        unimplemented!("host::testing::FakeKernel::emit_event")
    }

    /// Marks a pane as restored from a snapshot (or not).
    pub fn set_pane_restored(&self, _pane: NodeId, _restored: bool) {
        unimplemented!("host::testing::FakeKernel::set_pane_restored")
    }

    /// The ordered log of request steps (`journal`, `pane-write`, ...).
    pub fn step_log(&self) -> Vec<String> {
        unimplemented!("host::testing::FakeKernel::step_log")
    }

    /// Interrupts the server right after this pane's journal entry with this key.
    pub fn interrupt_after_journal(&self, _pane: NodeId, _key: &str) {
        unimplemented!("host::testing::FakeKernel::interrupt_after_journal")
    }

    /// Holds the next request open until the test completes it (CAP revocation mid-request).
    pub fn hold_next_request(&self) {
        unimplemented!("host::testing::FakeKernel::hold_next_request")
    }

    /// Fails the next pane write with this error code.
    pub fn fail_next_write(&self, _code: ErrorCode) {
        unimplemented!("host::testing::FakeKernel::fail_next_write")
    }

    /// Vetoes the next hook with this reason.
    pub fn veto_next_hook(&self, _reason: &str) {
        unimplemented!("host::testing::FakeKernel::veto_next_hook")
    }

    /// Sides that applied the named intent case (HST-490).
    pub fn sides_that_applied(&self, _case: &str) -> Vec<Side> {
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
    pub fn emit_lines(&self, _pane: NodeId, _lines: Vec<String>) {
        unimplemented!("host::testing::FakeKernel::emit_lines")
    }

    /// Marks a pane's screen as changed.
    pub fn change_screen(&self, _pane: NodeId) {
        unimplemented!("host::testing::FakeKernel::change_screen")
    }

    /// Sets or clears a pane's selection.
    pub fn set_selection(&self, _pane: NodeId, _selection: Option<Selection>) {
        unimplemented!("host::testing::FakeKernel::set_selection")
    }

    /// Sets a pane's terminal modes.
    pub fn set_modes(&self, _pane: NodeId, _modes: TerminalMode) {
        unimplemented!("host::testing::FakeKernel::set_modes")
    }

    /// Sets a pane's foreground process name and arguments.
    pub fn set_foreground(&self, _pane: NodeId, _name: &str, _arguments: &[&str]) {
        unimplemented!("host::testing::FakeKernel::set_foreground")
    }

    /// Sets a pane's size in columns and rows.
    pub fn set_pane_size(&self, _pane: NodeId, _columns: u16, _rows: u16) {
        unimplemented!("host::testing::FakeKernel::set_pane_size")
    }

    /// Removes a node from the tree.
    pub fn remove_node(&self, _node: NodeId) {
        unimplemented!("host::testing::FakeKernel::remove_node")
    }

    /// Whether a node exists.
    pub fn node_exists(&self, _node: NodeId) -> bool {
        unimplemented!("host::testing::FakeKernel::node_exists")
    }

    /// The pane with focus.
    pub fn focused_pane(&self) -> Option<NodeId> {
        unimplemented!("host::testing::FakeKernel::focused_pane")
    }

    // ---- agent primitives (PRI agent)

    /// A user-defined signature that wins over plugin signatures (PRI-011).
    pub fn set_user_signature(&self, _signature: SignatureSpec) {
        unimplemented!("host::testing::FakeKernel::set_user_signature")
    }

    /// Declares a signature id in the plugin manifest.
    pub fn contribute_signature_in_manifest(&self, _id: &str) {
        unimplemented!("host::testing::FakeKernel::contribute_signature_in_manifest")
    }

    /// The signature id that identifies this process name, if any.
    pub fn identify_process_name(&self, _name: &str) -> Option<String> {
        unimplemented!("host::testing::FakeKernel::identify_process_name")
    }

    /// Declares a toolbar button of the harness plugin.
    pub fn contribute_toolbar_button(&self, _name: &str) {
        unimplemented!("host::testing::FakeKernel::contribute_toolbar_button")
    }

    /// Declares a toolbar button of another plugin.
    pub fn contribute_toolbar_button_for(&self, _plugin: &str, _name: &str) {
        unimplemented!("host::testing::FakeKernel::contribute_toolbar_button_for")
    }

    /// The (enabled, label) state of a toolbar button, globally or for one pane.
    pub fn toolbar_state(
        &self,
        _name: &str,
        _pane: Option<NodeId>,
    ) -> Option<(bool, Option<String>)> {
        unimplemented!("host::testing::FakeKernel::toolbar_state")
    }

    /// Attaches an agent session to a pane.
    pub fn set_agent_session(&self, _pane: NodeId, _session: SessionRef) {
        unimplemented!("host::testing::FakeKernel::set_agent_session")
    }

    /// Whether the pane's agent session file has a known format.
    pub fn set_agent_session_format(&self, _pane: NodeId, _known: bool) {
        unimplemented!("host::testing::FakeKernel::set_agent_session_format")
    }

    /// The last prompt of a pane's agent.
    pub fn set_last_prompt(&self, _pane: NodeId, _prompt: &str) {
        unimplemented!("host::testing::FakeKernel::set_last_prompt")
    }

    /// Bytes the user's keyboard writes to the pane, as one chunk of terminal input. The kernel
    /// reconstructs the agent composer text from them; an Enter that follows exact text records
    /// that text as the pane's last prompt, an Enter that follows text the kernel cannot vouch
    /// for records the prompt as unavailable (`last-prompt` returns none).
    pub fn user_types_into(&self, _pane: NodeId, _bytes: &[u8]) {
        unimplemented!("host::testing::FakeKernel::user_types_into")
    }

    /// Bytes written to the pane by something other than the user (a plugin or an automation).
    /// They edit the composer behind the user's back, so the prompt they belong to is unavailable.
    pub fn automation_writes_into(&self, _pane: NodeId, _bytes: &[u8]) {
        unimplemented!("host::testing::FakeKernel::automation_writes_into")
    }

    /// The prompt epoch of a pane's agent.
    pub fn set_prompt_epoch(&self, _pane: NodeId, _epoch: u64) {
        unimplemented!("host::testing::FakeKernel::set_prompt_epoch")
    }

    /// The model of a pane's agent.
    pub fn set_agent_model(&self, _pane: NodeId, _model: AgentModel) {
        unimplemented!("host::testing::FakeKernel::set_agent_model")
    }

    /// The transcript of a pane's agent.
    pub fn set_agent_transcript(&self, _pane: NodeId, _turns: Vec<TranscriptTurn>) {
        unimplemented!("host::testing::FakeKernel::set_agent_transcript")
    }

    /// The activity of a pane as the kernel reports it.
    pub fn pane_activity(&self, _pane: NodeId) -> Option<AgentActivity> {
        unimplemented!("host::testing::FakeKernel::pane_activity")
    }

    /// The activity a pane had before any detector ran.
    pub fn initial_activity(&self, _pane: NodeId) -> Option<AgentActivity> {
        unimplemented!("host::testing::FakeKernel::initial_activity")
    }

    // ---- editor primitives (PRI editor)

    /// Attaches a buffer with this text to a pane and returns its id.
    pub fn attach_buffer(&self, _pane: NodeId, _text: &str) -> BufferId {
        unimplemented!("host::testing::FakeKernel::attach_buffer")
    }

    /// Sets the editor viewport (first row, first column, columns, rows).
    pub fn set_editor_viewport(
        &self,
        _pane: NodeId,
        _first_row: u32,
        _first_column: u32,
        _columns: u32,
        _rows: u32,
    ) {
        unimplemented!("host::testing::FakeKernel::set_editor_viewport")
    }

    /// Sets a buffer's selections.
    pub fn set_selections(&self, _buffer: BufferId, _ranges: Vec<TextRange>) {
        unimplemented!("host::testing::FakeKernel::set_selections")
    }

    /// Applies the n-th scripted edit to a buffer.
    pub fn edit_buffer(&self, _buffer: BufferId, _index: u32) {
        unimplemented!("host::testing::FakeKernel::edit_buffer")
    }

    /// How often the annotator export ran for a buffer.
    pub fn annotator_calls(&self, _buffer: BufferId) -> u32 {
        unimplemented!("host::testing::FakeKernel::annotator_calls")
    }

    /// Writes a file into a named root (`project`, ...).
    pub fn put_file(&self, _root: &str, _path: &str, _bytes: &[u8]) {
        unimplemented!("host::testing::FakeKernel::put_file")
    }

    /// Reads a file from a named root.
    pub fn file(&self, _root: &str, _path: &str) -> Option<Vec<u8>> {
        unimplemented!("host::testing::FakeKernel::file")
    }

    // ---- basic primitives (PRI basic)

    /// Whether a pane is marked dirty.
    pub fn pane_is_dirty(&self, _pane: NodeId) -> bool {
        unimplemented!("host::testing::FakeKernel::pane_is_dirty")
    }

    /// Whether a close confirmation is pending for a pane.
    pub fn pending_confirmation(&self, _pane: NodeId) -> bool {
        unimplemented!("host::testing::FakeKernel::pending_confirmation")
    }

    /// A pane's title.
    pub fn pane_title(&self, _pane: NodeId) -> Option<String> {
        unimplemented!("host::testing::FakeKernel::pane_title")
    }

    /// Sets a pane's title as the user would.
    pub fn set_manual_title(&self, _pane: NodeId, _title: &str) {
        unimplemented!("host::testing::FakeKernel::set_manual_title")
    }

    /// A pane's status text.
    pub fn pane_status_text(&self, _pane: NodeId) -> Option<String> {
        unimplemented!("host::testing::FakeKernel::pane_status_text")
    }

    // ---- catalogs, audio, workspace, http-api (WIT:ilium:host/*)

    /// Replaces the items of a catalog.
    pub fn set_catalog(&self, _catalog: &str, _items: Vec<CatalogItem>) {
        unimplemented!("host::testing::FakeKernel::set_catalog")
    }

    /// Makes a catalog's provider answer after this delay.
    pub fn set_catalog_provider_delay(&self, _catalog: &str, _delay: Duration) {
        unimplemented!("host::testing::FakeKernel::set_catalog_provider_delay")
    }

    /// The audio devices present.
    pub fn set_audio_devices(&self, _devices: Vec<AudioDevice>) {
        unimplemented!("host::testing::FakeKernel::set_audio_devices")
    }

    /// Ids of the open audio captures.
    pub fn open_captures(&self) -> Vec<u64> {
        unimplemented!("host::testing::FakeKernel::open_captures")
    }

    /// Whether loopback capture is available.
    pub fn set_loopback_available(&self, _available: bool) {
        unimplemented!("host::testing::FakeKernel::set_loopback_available")
    }

    /// Sample buffers played so far.
    pub fn played_audio(&self) -> Vec<Vec<i16>> {
        unimplemented!("host::testing::FakeKernel::played_audio")
    }

    /// Marks a path in a root as a git repository.
    pub fn set_git_repository(&self, _root: &str, _path: &str) {
        unimplemented!("host::testing::FakeKernel::set_git_repository")
    }

    /// Marks a registered workspace as in use.
    pub fn set_workspace_in_use(&self, _workspace: u64, _in_use: bool) {
        unimplemented!("host::testing::FakeKernel::set_workspace_in_use")
    }

    /// Starts an HTTP API call and returns its id.
    pub fn start_http_call(&self, _method: &str, _path: &str) -> u64 {
        unimplemented!("host::testing::FakeKernel::start_http_call")
    }

    /// The answer delivered for an HTTP API call.
    pub fn http_response(&self, _call: u64) -> Option<HttpAnswer> {
        unimplemented!("host::testing::FakeKernel::http_response")
    }

    // ---- ui-host (WIT:ilium:host/ui-host)

    /// Declares a panel of the harness plugin.
    pub fn declare_panel(&self, _panel: &str) {
        unimplemented!("host::testing::FakeKernel::declare_panel")
    }

    /// Commits a view tree of (widget kind, key) pairs and popup keys for a panel.
    pub fn commit_view_tree(&self, _panel: &str, _widgets: &[(&str, &str)], _popups: &[&str]) {
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
    pub fn popup_options(&self, _popup: &str) -> Option<PopupOptions> {
        unimplemented!("host::testing::FakeKernel::popup_options")
    }

    /// How often a panel was rendered.
    pub fn render_count(&self, _panel: &str) -> u32 {
        unimplemented!("host::testing::FakeKernel::render_count")
    }

    /// Sets the state of a widget in a panel.
    pub fn set_widget_state(&self, _panel: &str, _widget: &str, _state: WidgetState) {
        unimplemented!("host::testing::FakeKernel::set_widget_state")
    }

    /// Sets the theme accent colour and dark flag.
    pub fn set_theme(&self, _accent_rgb: u32, _dark: bool) {
        unimplemented!("host::testing::FakeKernel::set_theme")
    }

    /// Sets the icon mode.
    pub fn set_icon_mode(&self, _mode: IconMode) {
        unimplemented!("host::testing::FakeKernel::set_icon_mode")
    }

    /// Sets the density.
    pub fn set_density(&self, _density: Density) {
        unimplemented!("host::testing::FakeKernel::set_density")
    }

    /// Sets reduced motion.
    pub fn set_reduced_motion(&self, _reduced: bool) {
        unimplemented!("host::testing::FakeKernel::set_reduced_motion")
    }

    /// Pixel rows of a surface widget (first row, row count).
    pub fn surface_rows(
        &self,
        _panel: &str,
        _surface: &str,
        _first_row: u32,
        _rows: u32,
    ) -> Vec<u8> {
        unimplemented!("host::testing::FakeKernel::surface_rows")
    }

    // ---- cli (WIT:ilium:host/cli)

    /// Starts a CLI invocation (command, output format) and returns its id.
    pub fn start_cli_invocation(&self, _command: &str, _format: &str) -> u64 {
        unimplemented!("host::testing::FakeKernel::start_cli_invocation")
    }

    /// Starts a CLI invocation with this stdin.
    pub fn start_cli_invocation_with_stdin(
        &self,
        _command: &str,
        _format: &str,
        _stdin: &[u8],
    ) -> u64 {
        unimplemented!("host::testing::FakeKernel::start_cli_invocation_with_stdin")
    }

    /// Starts a CLI invocation with stdin closed.
    pub fn start_cli_invocation_without_stdin(&self, _command: &str, _format: &str) -> u64 {
        unimplemented!("host::testing::FakeKernel::start_cli_invocation_without_stdin")
    }

    /// Starts a CLI invocation with (stdin, stdout) tty flags.
    pub fn start_cli_invocation_with_tty(
        &self,
        _command: &str,
        _format: &str,
        _tty: (bool, bool),
    ) -> u64 {
        unimplemented!("host::testing::FakeKernel::start_cli_invocation_with_tty")
    }

    /// Starts a CLI invocation from inside a pane.
    pub fn start_cli_invocation_from_pane(
        &self,
        _command: &str,
        _format: &str,
        _pane: NodeId,
    ) -> u64 {
        unimplemented!("host::testing::FakeKernel::start_cli_invocation_from_pane")
    }

    /// Starts a CLI invocation with selected panes and a recursive flag.
    pub fn start_cli_invocation_with_selection(
        &self,
        _command: &str,
        _format: &str,
        _panes: Vec<NodeId>,
        _recursive: bool,
    ) -> u64 {
        unimplemented!("host::testing::FakeKernel::start_cli_invocation_with_selection")
    }

    /// Stdout lines of a CLI invocation.
    pub fn cli_stdout(&self, _invocation: u64) -> Vec<String> {
        unimplemented!("host::testing::FakeKernel::cli_stdout")
    }

    /// Stdout of a CLI invocation as text.
    pub fn cli_stdout_text(&self, _invocation: u64) -> String {
        unimplemented!("host::testing::FakeKernel::cli_stdout_text")
    }

    /// Stderr of a CLI invocation as text.
    pub fn cli_stderr_text(&self, _invocation: u64) -> String {
        unimplemented!("host::testing::FakeKernel::cli_stderr_text")
    }

    /// Error messages of a CLI invocation.
    pub fn cli_errors(&self, _invocation: u64) -> Vec<String> {
        unimplemented!("host::testing::FakeKernel::cli_errors")
    }

    /// Exit code of a finished CLI invocation.
    pub fn cli_exit_code(&self, _invocation: u64) -> Option<i32> {
        unimplemented!("host::testing::FakeKernel::cli_exit_code")
    }

    // ---- sounds, clipboard, sessions, geocode, inference, vector, settings, keymap

    /// Sound names the harness plugin ships.
    pub fn set_plugin_sounds(&self, _names: &[&str]) {
        unimplemented!("host::testing::FakeKernel::set_plugin_sounds")
    }

    /// Sound names another plugin ships.
    pub fn set_other_plugin_sounds(&self, _plugin: &str, _names: &[&str]) {
        unimplemented!("host::testing::FakeKernel::set_other_plugin_sounds")
    }

    /// Sets or clears the clipboard text.
    pub fn set_clipboard(&self, _text: Option<String>) {
        unimplemented!("host::testing::FakeKernel::set_clipboard")
    }

    /// The clipboard text.
    pub fn clipboard(&self) -> Option<String> {
        unimplemented!("host::testing::FakeKernel::clipboard")
    }

    /// The live sessions.
    pub fn set_live_sessions(&self, _sessions: Vec<LiveSession>) {
        unimplemented!("host::testing::FakeKernel::set_live_sessions")
    }

    /// Commands sent to a session so far.
    pub fn session_commands(&self, _session: &str) -> Vec<String> {
        unimplemented!("host::testing::FakeKernel::session_commands")
    }

    /// The reply a session gives to its next command.
    pub fn set_session_reply(&self, _session: &str, _reply_json: &str) {
        unimplemented!("host::testing::FakeKernel::set_session_reply")
    }

    /// Places the geocoder returns.
    pub fn set_geocode_places(&self, _places: Vec<GeoPlace>) {
        unimplemented!("host::testing::FakeKernel::set_geocode_places")
    }

    /// Makes the geocoder answer nothing.
    pub fn set_geocode_silent(&self, _silent: bool) {
        unimplemented!("host::testing::FakeKernel::set_geocode_silent")
    }

    /// Selects the inference provider plugin.
    pub fn select_inference_provider(&self, _plugin: &str) {
        unimplemented!("host::testing::FakeKernel::select_inference_provider")
    }

    /// Sets the embedder's dimension, or removes it.
    pub fn set_embedder(&self, _dimension: Option<u32>) {
        unimplemented!("host::testing::FakeKernel::set_embedder")
    }

    /// The vector index entry limit.
    pub fn set_vector_entry_limit(&self, _limit: u32) {
        unimplemented!("host::testing::FakeKernel::set_vector_entry_limit")
    }

    /// A kernel settings tab's JSON.
    pub fn set_kernel_tab(&self, _tab: &str, _json: &str) {
        unimplemented!("host::testing::FakeKernel::set_kernel_tab")
    }

    /// A secret value in a kernel settings tab.
    pub fn set_kernel_tab_secret(&self, _tab: &str, _key: &str, _value: &str) {
        unimplemented!("host::testing::FakeKernel::set_kernel_tab_secret")
    }

    /// The keymap bindings.
    pub fn set_keymap(&self, _bindings: Vec<Binding>) {
        unimplemented!("host::testing::FakeKernel::set_keymap")
    }

    // ---- client requests, gpu, location, keys, help, plugins, media

    /// Registers a client that answers these request kinds.
    pub fn register_client(&self, _client: ClientId, _kinds: &[&str]) {
        unimplemented!("host::testing::FakeKernel::register_client")
    }

    /// (kind, payload) requests waiting at a client.
    pub fn client_inbox(&self, _client: ClientId) -> Vec<(String, String)> {
        unimplemented!("host::testing::FakeKernel::client_inbox")
    }

    /// A client answers a request.
    pub fn client_answers(
        &self,
        _client: ClientId,
        _request: RequestId,
        _answer: Result<String, PluginError>,
    ) {
        unimplemented!("host::testing::FakeKernel::client_answers")
    }

    /// Starts a client-request call to the plugin's export and returns its id.
    pub fn start_client_request_call(&self, _kind: &str, _payload: &str) -> u64 {
        unimplemented!("host::testing::FakeKernel::start_client_request_call")
    }

    /// The answer the plugin gave to a client-request call.
    pub fn client_request_answer(&self, _call: u64) -> Option<Result<String, PluginError>> {
        unimplemented!("host::testing::FakeKernel::client_request_answer")
    }

    /// Whether a GPU is available.
    pub fn set_gpu_available(&self, _available: bool) {
        unimplemented!("host::testing::FakeKernel::set_gpu_available")
    }

    /// The device location as (latitude, longitude), or none.
    pub fn set_location(&self, _location: Option<(f64, f64)>) {
        unimplemented!("host::testing::FakeKernel::set_location")
    }

    /// Ids of the active key grabs.
    pub fn active_grabs(&self) -> Vec<u64> {
        unimplemented!("host::testing::FakeKernel::active_grabs")
    }

    /// Another plugin holds this chord.
    pub fn hold_chord_for(&self, _plugin: &str, _chord: &str) {
        unimplemented!("host::testing::FakeKernel::hold_chord_for")
    }

    /// Help topics as (id, markdown).
    pub fn set_help_topics(&self, _topics: &[(&str, &str)]) {
        unimplemented!("host::testing::FakeKernel::set_help_topics")
    }

    /// The installed plugin ids.
    pub fn set_installed_plugins(&self, _plugins: &[&str]) {
        unimplemented!("host::testing::FakeKernel::set_installed_plugins")
    }

    /// Whether a plugin is enabled.
    pub fn plugin_enabled(&self, _plugin: &str) -> bool {
        unimplemented!("host::testing::FakeKernel::plugin_enabled")
    }

    /// A video file in a root with this info.
    pub fn set_video(&self, _root: &str, _path: &str, _info: VideoInfo) {
        unimplemented!("host::testing::FakeKernel::set_video")
    }

    /// Whether ffmpeg is available.
    pub fn set_ffmpeg_available(&self, _available: bool) {
        unimplemented!("host::testing::FakeKernel::set_ffmpeg_available")
    }

    /// The info of videos opened from bytes.
    pub fn set_bytes_video(&self, _info: VideoInfo) {
        unimplemented!("host::testing::FakeKernel::set_bytes_video")
    }

    /// The playback position of an open video in milliseconds.
    pub fn video_position(&self, _video: u64) -> Option<u64> {
        unimplemented!("host::testing::FakeKernel::video_position")
    }
}

impl TreeRead for FakeKernel {
    fn nodes(&self) -> Vec<TreeNode> {
        unimplemented!("host::testing::FakeKernel::nodes")
    }

    fn node(&self, _id: NodeId) -> Option<TreeNode> {
        unimplemented!("host::testing::FakeKernel::node")
    }

    fn epoch(&self) -> u64 {
        unimplemented!("host::testing::FakeKernel::epoch")
    }
}

impl PaneRead for FakeKernel {
    fn screen_text(&self, _pane: NodeId) -> Option<String> {
        unimplemented!("host::testing::FakeKernel::screen_text")
    }

    fn cells(&self, _pane: NodeId, _rows: std::ops::Range<u16>) -> Option<Vec<Vec<Cell>>> {
        unimplemented!("host::testing::FakeKernel::cells")
    }

    fn history(&self, _pane: NodeId, _lines: u32) -> Option<Vec<String>> {
        unimplemented!("host::testing::FakeKernel::history")
    }
}

impl KernelFacade for FakeKernel {
    fn apply_intent(&self, _origin: &Origin, _intent: Intent) -> IntentResult {
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

/// One file row of an open agent-config proposal dialog (CAP-887).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposalRow {
    /// Whether the user has checked this row.
    pub checked: bool,
}

/// The kernel-drawn dialog of an `agent-config.propose` call (CAP-887).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposalDialog {
    /// One row per proposed file.
    pub rows: Vec<ProposalRow>,
    /// The complete diff of every proposed change.
    pub full_diff: String,
}

/// Agent-config proposal hooks (CAP-887..891).
impl FakeKernel {
    /// Whether the agent's config file exists.
    pub fn agent_config_file_exists(&self, _agent: &str, _path: &str) -> bool {
        unimplemented!("host::testing::FakeKernel::agent_config_file_exists")
    }

    /// The open proposal dialog, if any.
    pub fn open_proposal_dialog(&self) -> Option<ProposalDialog> {
        unimplemented!("host::testing::FakeKernel::open_proposal_dialog")
    }

    /// Makes the next atomic rename fail with an I/O error.
    pub fn fail_next_rename(&self) {
        unimplemented!("host::testing::FakeKernel::fail_next_rename")
    }

    /// Checks one dialog row and presses Apply.
    pub fn check_row_and_apply(&self, _row: usize) {
        unimplemented!("host::testing::FakeKernel::check_row_and_apply")
    }

    /// Presses Reject all.
    pub fn reject_all(&self) {
        unimplemented!("host::testing::FakeKernel::reject_all")
    }

    /// Writes an agent config file directly, as the user editing it.
    pub fn write_agent_config_file(&self, _agent: &str, _path: &str, _content: &[u8]) {
        unimplemented!("host::testing::FakeKernel::write_agent_config_file")
    }

    /// The current revision token of an agent config file.
    pub fn agent_config_revision(&self, _agent: &str, _path: &str) -> String {
        unimplemented!("host::testing::FakeKernel::agent_config_revision")
    }

    /// The bytes of an agent config file.
    pub fn read_agent_config_file(&self, _agent: &str, _path: &str) -> Vec<u8> {
        unimplemented!("host::testing::FakeKernel::read_agent_config_file")
    }

    /// Whether `consents.json` is byte-identical to its state at rig start.
    pub fn consents_json_is_unchanged(&self) -> bool {
        unimplemented!("host::testing::FakeKernel::consents_json_is_unchanged")
    }

    /// The bytes of a file under the plugin's data directory.
    pub fn plugin_data_file(&self, _path: &str) -> Vec<u8> {
        unimplemented!("host::testing::FakeKernel::plugin_data_file")
    }

    /// The audit records of one event kind.
    pub fn audit_records(&self, _event: &str) -> Vec<serde_json::Value> {
        unimplemented!("host::testing::FakeKernel::audit_records")
    }
}

#[cfg(test)]
mod tests {
    use super::{FakeKernel, FakePaneScreen};
    use ilium_plugin_api::generated::types::ids::{BufferId, ClientId, NodeId};
    use ilium_plugin_api::kernel::KernelFacade;
    use ilium_plugin_api::{ErrorCode, ProcessKind, Side};
    use std::time::Duration;

    fn fresh_kernel() -> FakeKernel {
        FakeKernel::new(ProcessKind::Server)
    }

    fn screen_with_rows(rows: &[&str]) -> FakePaneScreen {
        FakePaneScreen {
            rows: rows.iter().map(|row| row.to_string()).collect(),
            history: Vec::new(),
        }
    }

    fn screen_with_history(history: &[&str]) -> FakePaneScreen {
        FakePaneScreen {
            rows: Vec::new(),
            history: history.iter().map(|line| line.to_string()).collect(),
        }
    }

    #[test]
    fn new_keeps_the_process_kind_for_every_kind() {
        for kind in ProcessKind::ALL {
            assert_eq!(FakeKernel::new(kind).process_kind(), kind);
        }
    }

    #[test]
    fn new_kernel_has_no_applied_intents() {
        let kernel = fresh_kernel();
        assert!(kernel.applied_intents().is_empty());
    }

    #[test]
    fn new_kernel_has_an_empty_step_log() {
        let kernel = fresh_kernel();
        assert!(kernel.step_log().is_empty());
    }

    #[test]
    fn new_kernel_has_no_tree_nodes_and_no_focus() {
        let kernel = fresh_kernel();
        assert!(kernel.tree().nodes().is_empty());
        assert!(kernel.focused_pane().is_none());
        assert!(!kernel.node_exists(NodeId(1)));
    }

    #[test]
    fn set_tree_with_empty_input_leaves_no_addressable_node() {
        let kernel = fresh_kernel();
        kernel.set_tree(Vec::new());
        assert!(kernel.tree().nodes().is_empty());
        assert!(kernel.tree().node(NodeId(1)).is_none());
    }

    #[test]
    fn set_tree_bumps_the_epoch() {
        let kernel = fresh_kernel();
        let epoch_before = kernel.tree().epoch();
        kernel.set_tree(Vec::new());
        assert!(kernel.tree().epoch() > epoch_before);
    }

    #[test]
    fn node_lookup_of_an_absent_id_is_none() {
        let kernel = fresh_kernel();
        assert!(kernel.tree().node(NodeId(0)).is_none());
        assert!(kernel.tree().node(NodeId(u64::MAX)).is_none());
    }

    #[test]
    fn set_screen_makes_its_rows_visible_as_screen_text() {
        let kernel = fresh_kernel();
        let pane = NodeId(1);
        kernel.set_screen(pane, screen_with_rows(&["ready", "prompt"]));
        let text = kernel.panes().screen_text(pane).unwrap_or_default();
        assert!(text.contains("ready"));
        assert!(text.contains("prompt"));
    }

    #[test]
    fn set_screen_with_an_empty_screen_gives_blank_text() {
        let kernel = fresh_kernel();
        let pane = NodeId(1);
        kernel.set_screen(pane, FakePaneScreen::default());
        assert_eq!(kernel.panes().screen_text(pane).as_deref(), Some(""));
    }

    #[test]
    fn screen_text_of_an_unknown_pane_is_none() {
        let kernel = fresh_kernel();
        assert!(kernel.panes().screen_text(NodeId(9)).is_none());
    }

    #[test]
    fn emit_lines_with_no_lines_keeps_the_screen() {
        let kernel = fresh_kernel();
        let pane = NodeId(1);
        kernel.set_screen(pane, screen_with_rows(&["keep"]));
        kernel.emit_lines(pane, Vec::new());
        let text = kernel.panes().screen_text(pane).unwrap_or_default();
        assert!(text.contains("keep"));
    }

    #[test]
    fn history_with_zero_lines_is_empty() {
        let kernel = fresh_kernel();
        let pane = NodeId(1);
        kernel.set_screen(pane, screen_with_history(&["old"]));
        assert_eq!(kernel.panes().history(pane, 0), Some(Vec::new()));
    }

    #[test]
    fn history_beyond_the_stored_lines_returns_all_of_them_oldest_first() {
        let kernel = fresh_kernel();
        let pane = NodeId(1);
        kernel.set_screen(pane, screen_with_history(&["first", "second"]));
        assert_eq!(
            kernel.panes().history(pane, 10),
            Some(vec!["first".to_string(), "second".to_string()])
        );
    }

    #[test]
    fn history_of_an_unknown_pane_is_none() {
        let kernel = fresh_kernel();
        assert!(kernel.panes().history(NodeId(9), 5).is_none());
    }

    #[test]
    fn raw_bytes_sent_starts_at_zero() {
        let kernel = fresh_kernel();
        assert_eq!(kernel.raw_bytes_sent(), 0);
    }

    #[test]
    fn pane_flags_and_titles_of_an_unknown_pane_are_empty() {
        let kernel = fresh_kernel();
        let pane = NodeId(9);
        assert!(!kernel.pane_is_dirty(pane));
        assert!(!kernel.pending_confirmation(pane));
        assert!(kernel.pane_title(pane).is_none());
        assert!(kernel.pane_status_text(pane).is_none());
    }

    #[test]
    fn identify_process_name_without_signatures_is_none() {
        let kernel = fresh_kernel();
        assert!(kernel.identify_process_name("claude").is_none());
        assert!(kernel.identify_process_name("").is_none());
    }

    #[test]
    fn a_contributed_toolbar_button_has_a_reported_state() {
        let kernel = fresh_kernel();
        kernel.contribute_toolbar_button("save");
        assert!(kernel.toolbar_state("save", None).is_some());
    }

    #[test]
    fn pane_activity_of_an_unknown_pane_is_none() {
        let kernel = fresh_kernel();
        assert!(kernel.pane_activity(NodeId(9)).is_none());
        assert!(kernel.initial_activity(NodeId(9)).is_none());
    }

    #[test]
    fn annotator_calls_are_zero_before_any_edit_of_an_empty_buffer() {
        let kernel = fresh_kernel();
        let buffer = kernel.attach_buffer(NodeId(1), "");
        assert_eq!(kernel.annotator_calls(buffer), 0);
    }

    #[test]
    fn annotator_calls_of_an_unseen_buffer_are_zero() {
        let kernel = fresh_kernel();
        assert_eq!(kernel.annotator_calls(BufferId(1)), 0);
    }

    #[test]
    fn put_file_with_empty_bytes_is_distinct_from_a_missing_file() {
        let kernel = fresh_kernel();
        kernel.put_file("project", "empty.txt", b"");
        assert_eq!(kernel.file("project", "empty.txt"), Some(Vec::new()));
        assert!(kernel.file("project", "missing.txt").is_none());
    }

    #[test]
    fn open_captures_and_played_audio_are_empty_on_a_fresh_kernel() {
        let kernel = fresh_kernel();
        assert!(kernel.open_captures().is_empty());
        assert!(kernel.played_audio().is_empty());
    }

    #[test]
    fn an_unknown_http_call_has_no_response() {
        let kernel = fresh_kernel();
        assert!(kernel.http_response(999).is_none());
    }

    #[test]
    fn each_started_http_call_gets_its_own_id() {
        let kernel = fresh_kernel();
        let first_call = kernel.start_http_call("GET", "/a");
        let second_call = kernel.start_http_call("GET", "/b");
        assert_ne!(first_call, second_call);
    }

    #[test]
    fn surface_rows_with_zero_rows_are_empty() {
        let kernel = fresh_kernel();
        assert!(kernel.surface_rows("harness", "surface", 0, 0).is_empty());
    }

    #[test]
    fn a_declared_panel_is_listed_as_open() {
        let kernel = fresh_kernel();
        kernel.declare_panel("harness");
        assert!(kernel.open_panels().contains(&"harness".to_string()));
    }

    #[test]
    fn a_view_tree_without_popups_opens_no_popup() {
        let kernel = fresh_kernel();
        kernel.commit_view_tree("harness", &[], &[]);
        assert!(kernel.open_popups().is_empty());
    }

    #[test]
    fn a_popup_key_in_the_view_tree_opens_that_popup() {
        let kernel = fresh_kernel();
        kernel.commit_view_tree("harness", &[], &["menu"]);
        assert_eq!(kernel.open_popups(), vec!["menu".to_string()]);
    }

    #[test]
    fn popup_options_of_an_unopened_popup_are_none() {
        let kernel = fresh_kernel();
        assert!(kernel.popup_options("missing").is_none());
    }

    #[test]
    fn a_panel_never_rendered_has_zero_renders() {
        let kernel = fresh_kernel();
        assert_eq!(kernel.render_count("harness"), 0);
    }

    #[test]
    fn a_running_cli_invocation_has_no_exit_code_yet() {
        let kernel = fresh_kernel();
        let invocation = kernel.start_cli_invocation_without_stdin("version", "json");
        assert!(kernel.cli_exit_code(invocation).is_none());
    }

    #[test]
    fn an_unknown_cli_invocation_has_no_output_or_errors() {
        let kernel = fresh_kernel();
        assert!(kernel.cli_stdout(999).is_empty());
        assert!(kernel.cli_errors(999).is_empty());
    }

    #[test]
    fn a_registered_client_starts_with_an_empty_inbox() {
        let kernel = fresh_kernel();
        kernel.register_client(ClientId(1), &[]);
        assert!(kernel.client_inbox(ClientId(1)).is_empty());
    }

    #[test]
    fn a_client_request_call_has_no_answer_until_the_client_answers() {
        let kernel = fresh_kernel();
        let call = kernel.start_client_request_call("ping", "{}");
        assert!(kernel.client_request_answer(call).is_none());
    }

    #[test]
    fn clipboard_is_empty_by_default() {
        let kernel = fresh_kernel();
        assert!(kernel.clipboard().is_none());
    }

    #[test]
    fn clipboard_keeps_an_empty_string_distinct_from_none() {
        let kernel = fresh_kernel();
        kernel.set_clipboard(Some(String::new()));
        assert_eq!(kernel.clipboard(), Some(String::new()));
    }

    #[test]
    fn a_session_reply_is_not_recorded_as_a_command() {
        let kernel = fresh_kernel();
        kernel.set_session_reply("session-a", "{}");
        assert!(kernel.session_commands("session-a").is_empty());
    }

    #[test]
    fn video_position_of_an_unknown_video_is_none() {
        let kernel = fresh_kernel();
        assert!(kernel.video_position(7).is_none());
    }

    #[test]
    fn sides_that_applied_an_unknown_case_is_empty() {
        let kernel = fresh_kernel();
        let sides: Vec<Side> = kernel.sides_that_applied("never-applied");
        assert!(sides.is_empty());
    }

    #[test]
    fn agent_config_file_written_empty_exists_and_reads_back_empty() {
        let kernel = fresh_kernel();
        assert!(!kernel.agent_config_file_exists("claude", "settings.json"));
        kernel.write_agent_config_file("claude", "settings.json", b"");
        assert!(kernel.agent_config_file_exists("claude", "settings.json"));
        assert_eq!(
            kernel.read_agent_config_file("claude", "settings.json"),
            Vec::<u8>::new()
        );
    }

    #[test]
    fn no_proposal_dialog_is_open_on_a_fresh_kernel() {
        let kernel = fresh_kernel();
        assert!(kernel.open_proposal_dialog().is_none());
    }

    /// REQ: CAP-890
    #[test]
    fn consents_json_is_unchanged_before_any_prompt() {
        let kernel = fresh_kernel();
        assert!(kernel.consents_json_is_unchanged());
    }

    /// REQ: CAP-891
    #[test]
    fn no_audit_records_exist_before_any_write() {
        let kernel = fresh_kernel();
        assert!(kernel.audit_records("write-outside-data").is_empty());
    }

    #[test]
    fn no_key_grabs_or_popups_are_active_on_a_fresh_kernel() {
        let kernel = fresh_kernel();
        assert!(kernel.active_grabs().is_empty());
        assert!(kernel.open_popups().is_empty());
    }

    #[test]
    fn a_failing_write_is_not_recorded_as_an_applied_intent() {
        let kernel = fresh_kernel();
        kernel.fail_next_write(ErrorCode::Busy);
        assert!(kernel.applied_intents().is_empty());
    }

    #[test]
    fn location_can_be_cleared_and_set_to_the_origin() {
        let kernel = fresh_kernel();
        kernel.set_location(None);
        kernel.set_location(Some((0.0, 0.0)));
        assert!(kernel.active_grabs().is_empty());
    }

    #[test]
    fn a_zero_catalog_provider_delay_is_accepted() {
        let kernel = fresh_kernel();
        kernel.set_catalog_provider_delay("commands", Duration::from_millis(0));
        assert!(kernel.open_panels().is_empty());
    }
}
