//! An in-memory kernel behind `KernelFacade` (test plan section 1.3 `testing`).
//!
//! Beyond the facade, the fake kernel exposes setters and observers the conformance lanes use to
//! stage panes, agents, editors, host services and failure injection. The manual does not
//! document these hooks; each names the requirement family of its callers. Bodies are
//! represented by typed fixture state; external effects require a harness adapter.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Mutex, MutexGuard};
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
use ilium_plugin_api::generated::types::intents::{Delivery, ExpandTarget, RestructureOp};
use ilium_plugin_api::generated::types::requests::Outcome;
use ilium_plugin_api::generated::types::tree::{AgentActivity, AgentModel};
use ilium_plugin_api::generated::types::tree::{NodeClass, Placement, TitleSource};
use ilium_plugin_api::generated::types::ui::ViewTarget;
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

/// Toolbar owner, contribution name and optional pane scope.
pub type ToolbarKey = (String, String, Option<NodeId>);
/// Enabled flag and optional label override.
pub type ToolbarState = (bool, Option<String>);
/// Committed widget kind/key pairs and declared popup keys.
pub type FakeViewTree = (Vec<(String, String)>, Vec<String>);

/// Typed fixture state shared with the conformance harness adapters. No provider work is
/// performed merely by configuring a fixture; adapters must consume requests and publish replies.
#[derive(Debug, Clone, Default)]
pub struct FakeKernelState {
    pub nodes: Vec<TreeNode>,
    pub epoch: u64,
    pub clock: Duration,
    pub current_plugin: String,
    pub rate_windows: BTreeMap<(String, String), VecDeque<Duration>>,
    pub undo: BTreeMap<u64, (u64, Vec<TreeNode>)>,
    pub screens: BTreeMap<NodeId, FakePaneScreen>,
    pub applied: Vec<(Origin, Intent)>,
    pub events: VecDeque<Event>,
    pub restored: BTreeSet<NodeId>,
    pub steps: Vec<String>,
    pub interrupt: Option<(NodeId, String)>,
    pub hold_next: bool,
    pub write_error: Option<ErrorCode>,
    pub veto: Option<String>,
    pub frozen_reads: bool,
    pub raw_bytes: u64,
    pub pane_writes: Vec<(NodeId, Vec<u8>)>,
    pub current_submission_key: Option<String>,
    pub journal: BTreeSet<(NodeId, String)>,
    pub uncertain_journal: BTreeSet<(NodeId, String)>,
    pub selections: BTreeMap<NodeId, Option<Selection>>,
    pub modes: BTreeMap<NodeId, TerminalMode>,
    pub foreground: BTreeMap<NodeId, (String, Vec<String>)>,
    pub sizes: BTreeMap<NodeId, (u16, u16)>,
    pub screen_revisions: BTreeMap<NodeId, u64>,
    pub focus: Option<NodeId>,
    pub user_signatures: Vec<SignatureSpec>,
    pub manifest_signatures: BTreeSet<String>,
    pub toolbars: BTreeMap<ToolbarKey, ToolbarState>,
    pub agent_sessions: BTreeMap<NodeId, SessionRef>,
    pub known_sessions: BTreeMap<NodeId, bool>,
    pub last_prompts: BTreeMap<NodeId, String>,
    pub composers: BTreeMap<NodeId, Vec<u8>>,
    pub uncertain_composers: BTreeSet<NodeId>,
    pub prompt_epochs: BTreeMap<NodeId, u64>,
    pub models: BTreeMap<NodeId, AgentModel>,
    pub transcripts: BTreeMap<NodeId, Vec<TranscriptTurn>>,
    pub initial_activities: BTreeMap<NodeId, AgentActivity>,
    pub buffers: BTreeMap<BufferId, String>,
    pub pane_buffers: BTreeMap<NodeId, BufferId>,
    pub buffer_revisions: BTreeMap<BufferId, u64>,
    pub viewports: BTreeMap<NodeId, (u32, u32, u32, u32)>,
    pub scripted_edits:
        BTreeMap<BufferId, Vec<ilium_plugin_api::generated::types::editing::TextEdit>>,
    pub buffer_selections: BTreeMap<BufferId, Vec<TextRange>>,
    pub annotator_counts: BTreeMap<BufferId, u32>,
    pub files: BTreeMap<(String, String), Vec<u8>>,
    pub dirty: BTreeSet<NodeId>,
    pub confirmations: BTreeSet<NodeId>,
    pub manual_titles: BTreeSet<NodeId>,
    pub status: BTreeMap<NodeId, String>,
    pub styled_status: BTreeMap<NodeId, ilium_plugin_api::generated::types::text::StyledText>,
    pub basic_activities: BTreeMap<NodeId, AgentActivity>,
    pub pane_kind_registry: BTreeMap<
        String,
        (
            String,
            ilium_plugin_api::generated::types::tree::PrimitiveKind,
        ),
    >,
    pub catalogs: BTreeMap<String, Vec<CatalogItem>>,
    pub catalog_delays: BTreeMap<String, Duration>,
    pub audio_devices: Vec<AudioDevice>,
    pub captures: BTreeSet<u64>,
    pub loopback_available: bool,
    pub played_audio: Vec<Vec<i16>>,
    pub git_repositories: BTreeSet<(String, String)>,
    pub workspaces_in_use: BTreeSet<u64>,
    pub http_calls: BTreeMap<u64, (String, String)>,
    pub http_answers: BTreeMap<u64, HttpAnswer>,
    pub panels: BTreeSet<String>,
    pub view_trees: BTreeMap<String, FakeViewTree>,
    pub open_panels: BTreeSet<String>,
    pub popups: BTreeMap<String, PopupOptions>,
    pub render_counts: BTreeMap<String, u32>,
    pub widget_states: BTreeMap<(String, String), WidgetState>,
    pub theme: Option<(u32, bool)>,
    pub icon_mode: Option<IconMode>,
    pub density: Option<Density>,
    pub reduced_motion: bool,
    pub surface_row_bytes: BTreeMap<(String, String), usize>,
    pub surfaces: BTreeMap<(String, String), Vec<u8>>,
    pub cli: BTreeMap<u64, FakeCliInvocation>,
    pub sounds: BTreeMap<String, BTreeSet<String>>,
    pub clipboard: Option<String>,
    pub live_sessions: Vec<LiveSession>,
    pub session_commands: BTreeMap<String, Vec<String>>,
    pub session_replies: BTreeMap<String, String>,
    pub geocode_places: Vec<GeoPlace>,
    pub geocode_silent: bool,
    pub inference_provider: Option<String>,
    pub embedder_dimension: Option<u32>,
    pub vector_entry_limit: u32,
    pub kernel_tabs: BTreeMap<String, String>,
    pub kernel_secrets: BTreeMap<(String, String), String>,
    pub keymap: Vec<Binding>,
    pub clients: BTreeMap<ClientId, BTreeSet<String>>,
    pub client_inboxes: BTreeMap<ClientId, Vec<(String, String)>>,
    pub client_answers: BTreeMap<(ClientId, RequestId), Result<String, PluginError>>,
    pub client_calls: BTreeMap<u64, (String, String)>,
    pub client_call_answers: BTreeMap<u64, Result<String, PluginError>>,
    pub gpu_available: bool,
    pub location: Option<(f64, f64)>,
    pub grabs: BTreeSet<u64>,
    pub held_chords: BTreeMap<String, String>,
    pub help_topics: BTreeMap<String, String>,
    pub plugins: BTreeMap<String, bool>,
    pub videos: BTreeMap<(String, String), VideoInfo>,
    pub ffmpeg_available: bool,
    pub bytes_video: Option<VideoInfo>,
    pub video_positions: BTreeMap<u64, u64>,
    pub next_id: u64,
    pub next_node_id: u64,
    pub node_paths: BTreeMap<NodeId, ilium_plugin_api::generated::types::paths::FsPath>,
    pub bookmarks: BTreeSet<NodeId>,
    pub expanded: BTreeMap<NodeId, bool>,
    pub attention: BTreeMap<NodeId, Option<String>>,
    pub icons: BTreeMap<NodeId, Option<String>>,
    pub frozen_panes: BTreeSet<NodeId>,
    pub interrupt_next_pane_write: bool,
    pub proposal_dialog: Option<ProposalDialog>,
    pub pending_proposal: Option<PendingProposal>,
    pub proposal_completion: Option<Result<ProposalCompletion, PluginError>>,
    pub fail_next_rename: bool,
    pub consents_json: Option<Vec<u8>>,
    pub consents_at_start: Option<Vec<u8>>,
    /// Audit records as (event kind, record), in write order.
    pub audit: Vec<(String, serde_json::Value)>,
}

/// A CLI invocation and its actual collected output, owned by the harness CLI adapter.
#[derive(Debug, Clone, Default)]
pub struct FakeCliInvocation {
    pub command: String,
    pub format: String,
    pub stdin: Option<Vec<u8>>,
    pub tty: (bool, bool),
    pub pane: Option<NodeId>,
    pub selection: Vec<NodeId>,
    pub recursive: bool,
    pub stdout: String,
    pub stderr: String,
    pub errors: Vec<String>,
    pub exit_code: Option<i32>,
}

/// An in-memory tree and pane screens; every applied intent is recorded.
#[derive(Debug)]
pub struct FakeKernel {
    process_kind: ProcessKind,
    state: Mutex<FakeKernelState>,
}

impl FakeKernel {
    /// Accesses typed fixtures for a host-import adapter. A guard must never span a guest call.
    pub fn state(&self) -> MutexGuard<'_, FakeKernelState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Transfers emitted events to the harness dispatcher, preserving emission order.
    pub fn take_events(&self) -> Vec<Event> {
        self.state().events.drain(..).collect()
    }

    /// Applies the complete submission envelope, retaining the dedup key needed by the
    /// journal boundary. A plain KernelFacade intent has no key and cannot journal input.
    pub fn apply_submission(
        &self,
        origin: &Origin,
        submission: ilium_plugin_api::generated::types::intents::Submission,
    ) -> IntentResult {
        let mut state = self.state();
        state.current_plugin = match origin {
            Origin::Plugin { plugin, .. } => plugin.0.clone(),
            _ => String::new(),
        };
        state.current_submission_key = submission.dedup_key;
        state
            .applied
            .push((origin.clone(), submission.intent.clone()));
        let outcome = apply(&mut state, submission.intent);
        state.current_submission_key = None;
        state.current_plugin.clear();
        outcome
    }

    /// A request adapter consumes this one-shot flag before starting async work.
    pub fn take_hold_next_request(&self) -> bool {
        std::mem::take(&mut self.state().hold_next)
    }

    /// Recovery transfers each interrupted journal entry to one uncertainty event and never
    /// replays its bytes. The harness restart adapter calls this once per simulated restart.
    pub fn recover_journaled_inputs(&self) {
        let mut state = self.state();
        let uncertain = std::mem::take(&mut state.uncertain_journal);
        for (pane, dedup_key) in uncertain {
            state.events.push_back(Event::InputUncertain(
                ilium_plugin_api::generated::types::events::InputUncertainEvent { pane, dedup_key },
            ));
        }
    }

    /// An empty kernel running as the given process.
    pub fn new(process_kind: ProcessKind) -> FakeKernel {
        FakeKernel {
            process_kind,
            state: Mutex::new(FakeKernelState::default()),
        }
    }

    /// Replaces the tree and bumps the epoch.
    pub fn set_tree(&self, nodes: Vec<TreeNode>) {
        let mut state = self.state();
        state.epoch = state.epoch.saturating_add(1);
        state.initial_activities = nodes
            .iter()
            .filter_map(|node| node.agent.as_ref().map(|agent| (node.id, agent.activity)))
            .collect();
        state.next_node_id = state
            .next_node_id
            .max(nodes.iter().map(|node| node.id.0).max().unwrap_or(0));
        state.nodes = nodes;
        state.focus = state
            .nodes
            .iter()
            .find(|node| node.class == ilium_plugin_api::generated::types::tree::NodeClass::Pane)
            .map(|node| node.id);
    }

    /// Sets the screen of a pane.
    pub fn set_screen(&self, pane: NodeId, screen: FakePaneScreen) {
        self.state().screens.insert(pane, screen);
    }

    /// Every intent passed to the facade, with its origin, in order (including refusals).
    pub fn applied_intents(&self) -> Vec<(Origin, Intent)> {
        self.state().applied.clone()
    }

    // ---- events and intent plumbing (HST-49x events and intents)

    /// Emits a kernel event to the plugins.
    pub fn emit_event(&self, event: Event) {
        self.state().events.push_back(event);
    }

    /// Marks a pane as restored from a snapshot (or not).
    pub fn set_pane_restored(&self, pane: NodeId, restored: bool) {
        let mut state = self.state();
        if restored {
            state.restored.insert(pane);
        } else {
            state.restored.remove(&pane);
        }
    }

    /// The ordered log of request steps (`journal`, `pane-write`, ...).
    pub fn step_log(&self) -> Vec<String> {
        self.state().steps.clone()
    }

    /// Interrupts the server right after this pane's journal entry with this key.
    pub fn interrupt_after_journal(&self, pane: NodeId, key: &str) {
        self.state().interrupt = Some((pane, key.into()));
    }

    /// Holds the next request open until the test completes it (CAP revocation mid-request).
    pub fn hold_next_request(&self) {
        self.state().hold_next = true;
    }

    /// Fails the next pane write with this error code.
    pub fn fail_next_write(&self, code: ErrorCode) {
        self.state().write_error = Some(code);
    }

    /// Interrupts the next pane write part-way (API fs interruption).
    pub fn interrupt_next_write(&self) {
        self.state().interrupt_next_pane_write = true;
    }

    /// Vetoes the next hook with this reason.
    pub fn veto_next_hook(&self, reason: &str) {
        self.state().veto = Some(reason.into());
    }

    /// Sides that applied the named intent case (HST-490).
    pub fn sides_that_applied(&self, case: &str) -> Vec<Side> {
        let mut sides = Vec::new();
        for (origin, intent) in &self.state().applied {
            if intent.case_name() != case {
                continue;
            }
            let side = match origin {
                Origin::Plugin { side, .. } => *side,
                _ => {
                    if self.process_kind == ProcessKind::Client {
                        Side::Client
                    } else {
                        Side::Server
                    }
                }
            };
            if !sides.contains(&side) {
                sides.push(side);
            }
        }
        sides
    }

    /// Freezes server-side tree reads so client reads must come from the mirror (HST-49x).
    pub fn freeze_server_tree_reads(&self) {
        self.state().frozen_reads = true;
    }

    /// Raw bytes written to panes so far (HST-4xx).
    pub fn raw_bytes_sent(&self) -> u64 {
        self.state().raw_bytes
    }

    // ---- terminal primitives (PRI terminal)

    /// Appends lines to a pane's screen.
    pub fn emit_lines(&self, pane: NodeId, lines: Vec<String>) {
        let mut state = self.state();
        state.screens.entry(pane).or_default().rows.extend(lines);
        *state.screen_revisions.entry(pane).or_default() += 1;
    }

    /// Marks a pane's screen as changed.
    pub fn change_screen(&self, pane: NodeId) {
        *self.state().screen_revisions.entry(pane).or_default() += 1;
    }

    /// Sets or clears a pane's selection.
    pub fn set_selection(&self, pane: NodeId, selection: Option<Selection>) {
        self.state().selections.insert(pane, selection);
    }

    /// Sets a pane's terminal modes.
    pub fn set_modes(&self, pane: NodeId, modes: TerminalMode) {
        self.state().modes.insert(pane, modes);
    }

    /// Sets a pane's foreground process name and arguments.
    pub fn set_foreground(&self, pane: NodeId, name: &str, arguments: &[&str]) {
        self.state().foreground.insert(
            pane,
            (
                name.into(),
                arguments.iter().map(|value| (*value).into()).collect(),
            ),
        );
    }

    /// Sets a pane's size in columns and rows.
    pub fn set_pane_size(&self, pane: NodeId, columns: u16, rows: u16) {
        self.state().sizes.insert(pane, (columns, rows));
    }

    /// Removes a node from the tree.
    pub fn remove_node(&self, node: NodeId) {
        remove_subtree(&mut self.state(), node);
    }

    /// Whether a node exists.
    pub fn node_exists(&self, node: NodeId) -> bool {
        self.state().nodes.iter().any(|entry| entry.id == node)
    }

    /// The pane with focus.
    pub fn focused_pane(&self) -> Option<NodeId> {
        self.state().focus
    }

    // ---- agent primitives (PRI agent)

    /// A user-defined signature that wins over plugin signatures (PRI-011).
    pub fn set_user_signature(&self, signature: SignatureSpec) {
        let mut state = self.state();
        state.user_signatures.retain(|item| item.id != signature.id);
        state.user_signatures.push(signature);
    }

    /// Declares a signature id in the plugin manifest.
    pub fn contribute_signature_in_manifest(&self, id: &str) {
        self.state().manifest_signatures.insert(id.into());
    }

    /// The signature id that identifies this process name, if any.
    pub fn identify_process_name(&self, name: &str) -> Option<String> {
        self.state()
            .user_signatures
            .iter()
            .find(|signature| {
                signature
                    .process_names
                    .iter()
                    .chain(&signature.aliases)
                    .any(|candidate| candidate == name)
            })
            .map(|signature| signature.id.clone())
    }

    /// Declares a toolbar button of the harness plugin.
    pub fn contribute_toolbar_button(&self, name: &str) {
        self.contribute_toolbar_button_for("harness", name);
    }

    /// Declares a toolbar button of another plugin.
    pub fn contribute_toolbar_button_for(&self, plugin: &str, name: &str) {
        self.state()
            .toolbars
            .insert((plugin.into(), name.into(), None), (false, None));
    }

    /// The (enabled, label) state of a toolbar button, globally or for one pane.
    pub fn toolbar_state(
        &self,
        name: &str,
        pane: Option<NodeId>,
    ) -> Option<(bool, Option<String>)> {
        let state = self.state();
        state
            .toolbars
            .iter()
            .find(|((_, key, node), _)| key == name && *node == pane)
            .or_else(|| {
                state
                    .toolbars
                    .iter()
                    .find(|((_, key, node), _)| key == name && node.is_none())
            })
            .map(|(_, value)| value.clone())
    }

    /// Attaches an agent session to a pane.
    pub fn set_agent_session(&self, pane: NodeId, session: SessionRef) {
        self.state().agent_sessions.insert(pane, session);
    }

    /// Whether the pane's agent session file has a known format.
    pub fn set_agent_session_format(&self, pane: NodeId, known: bool) {
        self.state().known_sessions.insert(pane, known);
    }

    /// The last prompt of a pane's agent.
    pub fn set_last_prompt(&self, pane: NodeId, prompt: &str) {
        self.state().last_prompts.insert(pane, prompt.into());
    }

    /// Bytes the user's keyboard writes to the pane, as one chunk of terminal input. The kernel
    /// reconstructs the agent composer text from them; an Enter that follows exact text records
    /// that text as the pane's last prompt, an Enter that follows text the kernel cannot vouch
    /// for records the prompt as unavailable (`last-prompt` returns none).
    pub fn user_types_into(&self, pane: NodeId, bytes: &[u8]) {
        let mut state = self.state();
        for byte in bytes {
            match *byte {
                b'\r' | b'\n' => {
                    let text = state.composers.remove(&pane).unwrap_or_default();
                    if state.uncertain_composers.remove(&pane) {
                        state.last_prompts.remove(&pane);
                    } else if let Ok(text) = String::from_utf8(text) {
                        state.last_prompts.insert(pane, text);
                    } else {
                        state.last_prompts.remove(&pane);
                    }
                    *state.prompt_epochs.entry(pane).or_default() += 1;
                }
                8 | 127 => {
                    let composer = state.composers.entry(pane).or_default();
                    if let Ok(text) = std::str::from_utf8(composer) {
                        let length = text
                            .char_indices()
                            .next_back()
                            .map(|(offset, _)| offset)
                            .unwrap_or(0);
                        composer.truncate(length);
                    } else {
                        composer.clear();
                        state.uncertain_composers.insert(pane);
                    }
                }
                0..=31 => {
                    state.uncertain_composers.insert(pane);
                }
                _ => {
                    state.composers.entry(pane).or_default().push(*byte);
                }
            }
        }
        state.raw_bytes = state.raw_bytes.saturating_add(bytes.len() as u64);
    }

    /// Bytes written to the pane by something other than the user (a plugin or an automation).
    /// They edit the composer behind the user's back, so the prompt they belong to is unavailable.
    pub fn automation_writes_into(&self, pane: NodeId, bytes: &[u8]) {
        let mut state = self.state();
        state.uncertain_composers.insert(pane);
        state.raw_bytes = state.raw_bytes.saturating_add(bytes.len() as u64);
    }

    /// The prompt epoch of a pane's agent.
    pub fn set_prompt_epoch(&self, pane: NodeId, epoch: u64) {
        self.state().prompt_epochs.insert(pane, epoch);
    }

    /// The model of a pane's agent.
    pub fn set_agent_model(&self, pane: NodeId, model: AgentModel) {
        self.state().models.insert(pane, model);
    }

    /// The transcript of a pane's agent.
    pub fn set_agent_transcript(&self, pane: NodeId, turns: Vec<TranscriptTurn>) {
        self.state().transcripts.insert(pane, turns);
    }

    /// The activity of a pane as the kernel reports it.
    pub fn pane_activity(&self, pane: NodeId) -> Option<AgentActivity> {
        let state = self.state();
        state.basic_activities.get(&pane).copied().or_else(|| {
            state
                .nodes
                .iter()
                .find(|node| node.id == pane)
                .and_then(|node| node.agent.as_ref().map(|agent| agent.activity))
        })
    }

    /// The activity a pane had before any detector ran.
    pub fn initial_activity(&self, pane: NodeId) -> Option<AgentActivity> {
        self.state().initial_activities.get(&pane).copied()
    }

    // ---- editor primitives (PRI editor)

    /// Attaches a buffer with this text to a pane and returns its id.
    pub fn attach_buffer(&self, pane: NodeId, text: &str) -> BufferId {
        let mut state = self.state();
        state.next_id += 1;
        let id = BufferId(state.next_id);
        state.buffers.insert(id, text.into());
        state.pane_buffers.insert(pane, id);
        state.buffer_revisions.insert(id, 0);
        id
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
        self.state()
            .viewports
            .insert(pane, (first_row, first_column, columns, rows));
    }

    /// Sets a buffer's selections.
    pub fn set_selections(&self, buffer: BufferId, ranges: Vec<TextRange>) {
        self.state().buffer_selections.insert(buffer, ranges);
    }

    /// Applies the n-th scripted edit to a buffer.
    pub fn edit_buffer(&self, buffer: BufferId, index: u32) {
        let mut state = self.state();
        let edit = state
            .scripted_edits
            .get(&buffer)
            .and_then(|edits| edits.get(index as usize))
            .cloned();
        if let Some(edit) = edit {
            let _ = apply_edits(&mut state, buffer, &[edit]);
        }
    }

    /// How often the annotator export ran for a buffer.
    pub fn annotator_calls(&self, buffer: BufferId) -> u32 {
        self.state()
            .annotator_counts
            .get(&buffer)
            .copied()
            .unwrap_or(0)
    }

    /// Writes a file into a named root (`project`, ...).
    pub fn put_file(&self, root: &str, path: &str, bytes: &[u8]) {
        self.state()
            .files
            .insert((root.into(), path.into()), bytes.to_vec());
    }

    /// Reads a file from a named root.
    pub fn file(&self, root: &str, path: &str) -> Option<Vec<u8>> {
        self.state().files.get(&(root.into(), path.into())).cloned()
    }

    // ---- basic primitives (PRI basic)

    /// Whether a pane is marked dirty.
    pub fn pane_is_dirty(&self, pane: NodeId) -> bool {
        self.state().dirty.contains(&pane)
    }

    /// Whether a close confirmation is pending for a pane.
    pub fn pending_confirmation(&self, pane: NodeId) -> bool {
        self.state().confirmations.contains(&pane)
    }

    /// A pane's title.
    pub fn pane_title(&self, pane: NodeId) -> Option<String> {
        self.state()
            .nodes
            .iter()
            .find(|node| node.id == pane)
            .map(|node| node.title.clone())
    }

    /// Sets a pane's title as the user would.
    pub fn set_manual_title(&self, pane: NodeId, title: &str) {
        let mut state = self.state();
        if let Some(node) = state.nodes.iter_mut().find(|node| node.id == pane) {
            node.title = title.into();
            node.revision = node.revision.saturating_add(1);
            state.manual_titles.insert(pane);
            state.epoch = state.epoch.saturating_add(1);
        }
    }

    /// Seeds the authoritative registry with an admitted manifest's exact kind and owner.
    /// Merely giving a node a namespaced content kind grants no primitive authority.
    pub fn register_pane_kind(
        &self,
        plugin: &str,
        kind: &str,
        primitive: ilium_plugin_api::generated::types::tree::PrimitiveKind,
    ) {
        self.state()
            .pane_kind_registry
            .insert(kind.into(), (plugin.into(), primitive));
    }

    /// Complete footer spans, including styles supplied by the plugin.
    pub fn pane_status_spans(
        &self,
        pane: NodeId,
    ) -> Option<ilium_plugin_api::generated::types::text::StyledText> {
        self.state().styled_status.get(&pane).cloned()
    }

    /// A pane's status text.
    pub fn pane_status_text(&self, pane: NodeId) -> Option<String> {
        self.state().status.get(&pane).cloned()
    }

    // ---- catalogs, audio, workspace, http-api (WIT:ilium:host/*)

    /// Replaces the items of a catalog.
    pub fn set_catalog(&self, catalog: &str, items: Vec<CatalogItem>) {
        self.state().catalogs.insert(catalog.into(), items);
    }

    /// Makes a catalog's provider answer after this delay.
    pub fn set_catalog_provider_delay(&self, catalog: &str, delay: Duration) {
        self.state().catalog_delays.insert(catalog.into(), delay);
    }

    /// The audio devices present.
    pub fn set_audio_devices(&self, devices: Vec<AudioDevice>) {
        self.state().audio_devices = devices;
    }

    /// Ids of the open audio captures.
    pub fn open_captures(&self) -> Vec<u64> {
        self.state().captures.iter().copied().collect()
    }

    /// Whether loopback capture is available.
    pub fn set_loopback_available(&self, available: bool) {
        self.state().loopback_available = available;
    }

    /// Sample buffers played so far.
    pub fn played_audio(&self) -> Vec<Vec<i16>> {
        self.state().played_audio.clone()
    }

    /// Marks a path in a root as a git repository.
    pub fn set_git_repository(&self, root: &str, path: &str) {
        self.state()
            .git_repositories
            .insert((root.into(), path.into()));
    }

    /// Marks a registered workspace as in use.
    pub fn set_workspace_in_use(&self, workspace: u64, in_use: bool) {
        let mut state = self.state();
        if in_use {
            state.workspaces_in_use.insert(workspace);
        } else {
            state.workspaces_in_use.remove(&workspace);
        }
    }

    /// Starts an HTTP API call and returns its id.
    pub fn start_http_call(&self, method: &str, path: &str) -> u64 {
        let mut state = self.state();
        state.next_id += 1;
        let id = state.next_id;
        state.http_calls.insert(id, (method.into(), path.into()));
        id
    }

    /// The answer delivered for an HTTP API call.
    pub fn http_response(&self, call: u64) -> Option<HttpAnswer> {
        self.state().http_answers.get(&call).cloned()
    }

    // ---- ui-host (WIT:ilium:host/ui-host)

    /// Declares a panel of the harness plugin.
    pub fn declare_panel(&self, panel: &str) {
        self.state().panels.insert(panel.into());
    }

    /// Commits a view tree of (widget kind, key) pairs and popup keys for a panel.
    pub fn commit_view_tree(&self, panel: &str, widgets: &[(&str, &str)], popups: &[&str]) {
        self.state().view_trees.insert(
            panel.into(),
            (
                widgets
                    .iter()
                    .map(|(kind, key)| ((*kind).into(), (*key).into()))
                    .collect(),
                popups.iter().map(|key| (*key).into()).collect(),
            ),
        );
    }

    /// Keys of the open panels.
    pub fn open_panels(&self) -> Vec<String> {
        self.state().open_panels.iter().cloned().collect()
    }

    /// Keys of the open popups.
    pub fn open_popups(&self) -> Vec<String> {
        self.state().popups.keys().cloned().collect()
    }

    /// Options of an open popup.
    pub fn popup_options(&self, popup: &str) -> Option<PopupOptions> {
        self.state().popups.get(popup).cloned()
    }

    /// How often a panel was rendered.
    pub fn render_count(&self, panel: &str) -> u32 {
        self.state().render_counts.get(panel).copied().unwrap_or(0)
    }

    /// Sets the state of a widget in a panel.
    pub fn set_widget_state(&self, panel: &str, widget: &str, state: WidgetState) {
        self.state()
            .widget_states
            .insert((panel.into(), widget.into()), state);
    }

    /// Sets the theme accent colour and dark flag.
    pub fn set_theme(&self, accent_rgb: u32, dark: bool) {
        self.state().theme = Some((accent_rgb, dark));
    }

    /// Sets the icon mode.
    pub fn set_icon_mode(&self, mode: IconMode) {
        self.state().icon_mode = Some(mode);
    }

    /// Sets the density.
    pub fn set_density(&self, density: Density) {
        self.state().density = Some(density);
    }

    /// Sets reduced motion.
    pub fn set_reduced_motion(&self, reduced: bool) {
        self.state().reduced_motion = reduced;
    }

    /// Pixel rows of a surface widget (first row, row count).
    pub fn surface_rows(&self, panel: &str, surface: &str, first_row: u32, rows: u32) -> Vec<u8> {
        let state = self.state();
        let key = (panel.into(), surface.into());
        let bytes = state.surfaces.get(&key);
        let stride = state.surface_row_bytes.get(&key).copied().unwrap_or(0);
        let first = (first_row as usize).saturating_mul(stride);
        let end = first.saturating_add((rows as usize).saturating_mul(stride));
        bytes
            .map(|bytes| {
                bytes
                    .get(first..end.min(bytes.len()))
                    .unwrap_or_default()
                    .to_vec()
            })
            .unwrap_or_default()
    }

    // ---- cli (WIT:ilium:host/cli)

    /// Starts a CLI invocation (command, output format) and returns its id.
    pub fn start_cli_invocation(&self, command: &str, format: &str) -> u64 {
        let mut state = self.state();
        state.next_id += 1;
        let id = state.next_id;
        state.cli.insert(
            id,
            FakeCliInvocation {
                command: command.into(),
                format: format.into(),
                ..FakeCliInvocation::default()
            },
        );
        id
    }

    /// Starts a CLI invocation with this stdin.
    pub fn start_cli_invocation_with_stdin(
        &self,
        command: &str,
        format: &str,
        stdin: &[u8],
    ) -> u64 {
        let id = self.start_cli_invocation(command, format);
        if let Some(entry) = self.state().cli.get_mut(&id) {
            entry.stdin = Some(stdin.to_vec());
        }
        id
    }

    /// Starts a CLI invocation with stdin closed.
    pub fn start_cli_invocation_without_stdin(&self, command: &str, format: &str) -> u64 {
        let id = self.start_cli_invocation(command, format);
        if let Some(entry) = self.state().cli.get_mut(&id) {
            entry.stdin = None;
        }
        id
    }

    /// Starts a CLI invocation with (stdin, stdout) tty flags.
    pub fn start_cli_invocation_with_tty(
        &self,
        command: &str,
        format: &str,
        tty: (bool, bool),
    ) -> u64 {
        let id = self.start_cli_invocation(command, format);
        if let Some(entry) = self.state().cli.get_mut(&id) {
            entry.tty = tty;
        }
        id
    }

    /// Starts a CLI invocation from inside a pane.
    pub fn start_cli_invocation_from_pane(&self, command: &str, format: &str, pane: NodeId) -> u64 {
        let id = self.start_cli_invocation(command, format);
        if let Some(entry) = self.state().cli.get_mut(&id) {
            entry.pane = Some(pane);
        }
        id
    }

    /// Starts a CLI invocation with selected panes and a recursive flag.
    pub fn start_cli_invocation_with_selection(
        &self,
        command: &str,
        format: &str,
        panes: Vec<NodeId>,
        recursive: bool,
    ) -> u64 {
        let id = self.start_cli_invocation(command, format);
        if let Some(entry) = self.state().cli.get_mut(&id) {
            entry.selection = panes;
            entry.recursive = recursive;
        }
        id
    }

    /// Stdout lines of a CLI invocation.
    pub fn cli_stdout(&self, invocation: u64) -> Vec<String> {
        self.cli_stdout_text(invocation)
            .lines()
            .map(str::to_owned)
            .collect()
    }

    /// Stdout of a CLI invocation as text.
    pub fn cli_stdout_text(&self, invocation: u64) -> String {
        self.state()
            .cli
            .get(&invocation)
            .map(|entry| entry.stdout.clone())
            .unwrap_or_default()
    }

    /// Stderr of a CLI invocation as text.
    pub fn cli_stderr_text(&self, invocation: u64) -> String {
        self.state()
            .cli
            .get(&invocation)
            .map(|entry| entry.stderr.clone())
            .unwrap_or_default()
    }

    /// Error messages of a CLI invocation.
    pub fn cli_errors(&self, invocation: u64) -> Vec<String> {
        self.state()
            .cli
            .get(&invocation)
            .map(|entry| entry.errors.clone())
            .unwrap_or_default()
    }

    /// Exit code of a finished CLI invocation.
    pub fn cli_exit_code(&self, invocation: u64) -> Option<i32> {
        self.state()
            .cli
            .get(&invocation)
            .and_then(|entry| entry.exit_code)
    }

    // ---- sounds, clipboard, sessions, geocode, inference, vector, settings, keymap

    /// Sound names the harness plugin ships.
    pub fn set_plugin_sounds(&self, names: &[&str]) {
        self.set_other_plugin_sounds("harness", names);
    }

    /// Sound names another plugin ships.
    pub fn set_other_plugin_sounds(&self, plugin: &str, names: &[&str]) {
        self.state().sounds.insert(
            plugin.into(),
            names.iter().map(|name| (*name).into()).collect(),
        );
    }

    /// Sets or clears the clipboard text.
    pub fn set_clipboard(&self, text: Option<String>) {
        self.state().clipboard = text;
    }

    /// The clipboard text.
    pub fn clipboard(&self) -> Option<String> {
        self.state().clipboard.clone()
    }

    /// The live sessions.
    pub fn set_live_sessions(&self, sessions: Vec<LiveSession>) {
        self.state().live_sessions = sessions;
    }

    /// Commands sent to a session so far.
    pub fn session_commands(&self, session: &str) -> Vec<String> {
        self.state()
            .session_commands
            .get(session)
            .cloned()
            .unwrap_or_default()
    }

    /// The reply a session gives to its next command.
    pub fn set_session_reply(&self, session: &str, reply_json: &str) {
        self.state()
            .session_replies
            .insert(session.into(), reply_json.into());
    }

    /// Places the geocoder returns.
    pub fn set_geocode_places(&self, places: Vec<GeoPlace>) {
        self.state().geocode_places = places;
    }

    /// Makes the geocoder answer nothing.
    pub fn set_geocode_silent(&self, silent: bool) {
        self.state().geocode_silent = silent;
    }

    /// Selects the inference provider plugin.
    pub fn select_inference_provider(&self, plugin: &str) {
        self.state().inference_provider = Some(plugin.into());
    }

    /// Sets the embedder's dimension, or removes it.
    pub fn set_embedder(&self, dimension: Option<u32>) {
        self.state().embedder_dimension = dimension;
    }

    /// The vector index entry limit.
    pub fn set_vector_entry_limit(&self, limit: u32) {
        self.state().vector_entry_limit = limit;
    }

    /// A kernel settings tab's JSON.
    pub fn set_kernel_tab(&self, tab: &str, json: &str) {
        self.state().kernel_tabs.insert(tab.into(), json.into());
    }

    /// A secret value in a kernel settings tab.
    pub fn set_kernel_tab_secret(&self, tab: &str, key: &str, value: &str) {
        self.state()
            .kernel_secrets
            .insert((tab.into(), key.into()), value.into());
    }

    /// The keymap bindings.
    pub fn set_keymap(&self, bindings: Vec<Binding>) {
        self.state().keymap = bindings;
    }

    // ---- client requests, gpu, location, keys, help, plugins, media

    /// Registers a client that answers these request kinds.
    pub fn register_client(&self, client: ClientId, kinds: &[&str]) {
        self.state()
            .clients
            .insert(client, kinds.iter().map(|kind| (*kind).into()).collect());
    }

    /// (kind, payload) requests waiting at a client.
    pub fn client_inbox(&self, client: ClientId) -> Vec<(String, String)> {
        self.state()
            .client_inboxes
            .get(&client)
            .cloned()
            .unwrap_or_default()
    }

    /// A client answers a request.
    pub fn client_answers(
        &self,
        client: ClientId,
        request: RequestId,
        answer: Result<String, PluginError>,
    ) {
        self.state()
            .client_answers
            .insert((client, request), answer);
    }

    /// Starts a client-request call to the plugin's export and returns its id.
    pub fn start_client_request_call(&self, kind: &str, payload: &str) -> u64 {
        let mut state = self.state();
        state.next_id += 1;
        let id = state.next_id;
        state.client_calls.insert(id, (kind.into(), payload.into()));
        id
    }

    /// The answer the plugin gave to a client-request call.
    pub fn client_request_answer(&self, call: u64) -> Option<Result<String, PluginError>> {
        self.state().client_call_answers.get(&call).cloned()
    }

    /// Whether a GPU is available.
    pub fn set_gpu_available(&self, available: bool) {
        self.state().gpu_available = available;
    }

    /// The device location as (latitude, longitude), or none.
    pub fn set_location(&self, location: Option<(f64, f64)>) {
        self.state().location = location;
    }

    /// Ids of the active key grabs.
    pub fn active_grabs(&self) -> Vec<u64> {
        self.state().grabs.iter().copied().collect()
    }

    /// Another plugin holds this chord.
    pub fn hold_chord_for(&self, plugin: &str, chord: &str) {
        self.state().held_chords.insert(chord.into(), plugin.into());
    }

    /// Help topics as (id, markdown).
    pub fn set_help_topics(&self, topics: &[(&str, &str)]) {
        self.state().help_topics = topics
            .iter()
            .map(|(id, markdown)| ((*id).into(), (*markdown).into()))
            .collect();
    }

    /// The installed plugin ids.
    pub fn set_installed_plugins(&self, plugins: &[&str]) {
        self.state().plugins = plugins
            .iter()
            .map(|plugin| ((*plugin).into(), true))
            .collect();
    }

    /// Whether a plugin is enabled.
    pub fn plugin_enabled(&self, plugin: &str) -> bool {
        self.state().plugins.get(plugin).copied().unwrap_or(false)
    }

    /// A video file in a root with this info.
    pub fn set_video(&self, root: &str, path: &str, info: VideoInfo) {
        self.state().videos.insert((root.into(), path.into()), info);
    }

    /// Whether ffmpeg is available.
    pub fn set_ffmpeg_available(&self, available: bool) {
        self.state().ffmpeg_available = available;
    }

    /// The info of videos opened from bytes.
    pub fn set_bytes_video(&self, info: VideoInfo) {
        self.state().bytes_video = Some(info);
    }

    /// The playback position of an open video in milliseconds.
    pub fn video_position(&self, video: u64) -> Option<u64> {
        self.state().video_positions.get(&video).copied()
    }
}

impl TreeRead for FakeKernel {
    fn nodes(&self) -> Vec<TreeNode> {
        let state = self.state();
        if state.frozen_reads {
            Vec::new()
        } else {
            state.nodes.clone()
        }
    }

    fn node(&self, id: NodeId) -> Option<TreeNode> {
        let state = self.state();
        if state.frozen_reads {
            None
        } else {
            state.nodes.iter().find(|node| node.id == id).cloned()
        }
    }

    fn epoch(&self) -> u64 {
        self.state().epoch
    }
}

impl PaneRead for FakeKernel {
    fn screen_text(&self, pane: NodeId) -> Option<String> {
        self.state()
            .screens
            .get(&pane)
            .map(|screen| screen.rows.join("\n"))
    }

    fn cells(&self, pane: NodeId, rows: std::ops::Range<u16>) -> Option<Vec<Vec<Cell>>> {
        self.state().screens.get(&pane).map(|screen| {
            rows.filter_map(|row| screen.rows.get(row as usize))
                .map(|text| {
                    text.chars()
                        .map(|character| Cell {
                            text: character.to_string(),
                            ..Cell::default()
                        })
                        .collect()
                })
                .collect()
        })
    }

    fn history(&self, pane: NodeId, lines: u32) -> Option<Vec<String>> {
        self.state().screens.get(&pane).map(|screen| {
            screen.history[screen.history.len().saturating_sub(lines as usize)..].to_vec()
        })
    }
}

impl KernelFacade for FakeKernel {
    fn apply_intent(&self, origin: &Origin, intent: Intent) -> IntentResult {
        let mut state = self.state();
        state.applied.push((origin.clone(), intent.clone()));
        state.current_plugin = match origin {
            Origin::Plugin { plugin, .. } => plugin.0.clone(),
            _ => String::new(),
        };
        let outcome = apply(&mut state, intent);
        state.current_plugin.clear();
        outcome
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

/// One whole-file change of a proposal, as the plugin sent it (CAP-886).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentConfigChange {
    /// Path relative to the agent's configuration folder.
    pub path: String,
    /// The whole new file; `None` removes it.
    pub content: Option<Vec<u8>>,
    /// The revision the plugin read; `None` means the file must not exist yet.
    pub revision: Option<String>,
    /// One line shown beside the file in the dialog.
    pub summary: String,
}

/// A proposal waiting for the user's answer in its dialog (one per plugin, CAP-890).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingProposal {
    /// Plugin id, the audit actor.
    pub plugin: String,
    pub agent: String,
    /// Request id; names the history entry.
    pub request: u64,
    /// One entry per dialog row, in row order.
    pub changes: Vec<AgentConfigChange>,
}

/// The completed result of a proposal: the applied and rejected paths and the history path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposalCompletion {
    pub applied: Vec<String>,
    pub rejected: Vec<String>,
    pub history: String,
}

fn agent_config_key(agent: &str, path: &str) -> (String, String) {
    (format!("agent-config:{agent}"), path.into())
}

/// Equal tokens mean equal content; the token of a missing file is `absent`.
fn revision_token(content: Option<&[u8]>) -> String {
    match content {
        None => "absent".into(),
        Some(bytes) => format!("bytes:{}", to_hex(bytes)),
    }
}

fn revision_matches(expected: Option<&str>, current: Option<&[u8]>) -> bool {
    match (expected, current) {
        (None, None) => true,
        (Some(token), Some(bytes)) => token == revision_token(Some(bytes)),
        _ => false,
    }
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The dialog text: each change as a header plus its removed and added lines.
fn proposal_full_diff(state: &FakeKernelState, agent: &str, changes: &[AgentConfigChange]) -> String {
    let mut diff = String::new();
    for change in changes {
        let previous = state.files.get(&agent_config_key(agent, &change.path));
        let previous_text = previous.map_or_else(String::new, |b| String::from_utf8_lossy(b).into_owned());
        let next_text = change
            .content
            .as_deref()
            .map_or_else(String::new, |b| String::from_utf8_lossy(b).into_owned());
        diff.push_str(&format!("--- {agent}/{}\n+++ {agent}/{}\n", change.path, change.path));
        for line in previous_text.lines() {
            diff.push_str(&format!("-{line}\n"));
        }
        for line in next_text.lines() {
            diff.push_str(&format!("+{line}\n"));
        }
    }
    diff
}

/// Completes the open proposal. Checked rows are applied all or none (CAP-887); with
/// `reject_all` every row counts as unchecked and the proposal is declined (CAP-890).
fn resolve_agent_config_proposal(state: &mut FakeKernelState, reject_all: bool) {
    let Some(pending) = state.pending_proposal.take() else {
        return;
    };
    let dialog = state.proposal_dialog.take();
    let checked: Vec<bool> = dialog
        .map(|dialog| dialog.rows.iter().map(|row| row.checked && !reject_all).collect())
        .unwrap_or_else(|| vec![false; pending.changes.len()]);
    let agent = pending.agent.clone();
    let revisions_hold = pending.changes.iter().all(|change| {
        let current = state.files.get(&agent_config_key(&agent, &change.path));
        revision_matches(change.revision.as_deref(), current.map(Vec::as_slice))
    });
    if !revisions_hold {
        state.proposal_completion = Some(Err(PluginError::new(
            ErrorCode::Conflict,
            "a file changed since the plugin read it",
        )));
        return;
    }
    if !checked.iter().any(|&is_checked| is_checked) {
        state.proposal_completion = Some(Err(PluginError::new(
            ErrorCode::NotPermitted,
            "declined: the user checked no file",
        )));
        return;
    }
    let history = format!("agent-config-history/{agent}/{}.json", pending.request);
    let entries: Vec<serde_json::Value> = pending
        .changes
        .iter()
        .zip(&checked)
        .filter(|(_, is_checked)| **is_checked)
        .map(|(change, _)| {
            let previous = state.files.get(&agent_config_key(&agent, &change.path));
            serde_json::json!({
                "path": change.path,
                "previous": previous.map(|bytes| to_hex(bytes)),
            })
        })
        .collect();
    state.files.insert(
        ("data".into(), history.clone()),
        serde_json::to_vec(&entries).unwrap_or_default(),
    );
    let mut replaced: Vec<((String, String), Option<Vec<u8>>)> = Vec::new();
    for (change, _) in pending.changes.iter().zip(&checked).filter(|(_, is_checked)| **is_checked) {
        let key = agent_config_key(&agent, &change.path);
        if std::mem::take(&mut state.fail_next_rename) {
            for (restored_key, previous) in replaced.into_iter().rev() {
                match previous {
                    Some(bytes) => state.files.insert(restored_key, bytes),
                    None => state.files.remove(&restored_key),
                };
            }
            state.proposal_completion = Some(Err(PluginError::new(
                ErrorCode::Io,
                "rename failed; replaced files were restored",
            )));
            return;
        }
        replaced.push((key.clone(), state.files.get(&key).cloned()));
        match &change.content {
            Some(bytes) => state.files.insert(key, bytes.clone()),
            None => state.files.remove(&key),
        };
    }
    let mut applied = Vec::new();
    let mut rejected = Vec::new();
    for (change, is_checked) in pending.changes.iter().zip(&checked) {
        if !is_checked {
            rejected.push(change.path.clone());
            continue;
        }
        applied.push(change.path.clone());
        state.audit.push((
            "write-outside-data".into(),
            serde_json::json!({
                "actor": pending.plugin,
                "grant": format!("agent-config.propose:{agent}"),
                "target": format!("{agent}/{}", change.path),
            }),
        ));
    }
    state.proposal_completion = Some(Ok(ProposalCompletion {
        applied,
        rejected,
        history,
    }));
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
    pub fn agent_config_file_exists(&self, agent: &str, path: &str) -> bool {
        self.state()
            .files
            .contains_key(&agent_config_key(agent, path))
    }

    /// The open proposal dialog, if any.
    pub fn open_proposal_dialog(&self) -> Option<ProposalDialog> {
        self.state().proposal_dialog.clone()
    }

    /// Makes the next atomic rename fail with an I/O error.
    pub fn fail_next_rename(&self) {
        self.state().fail_next_rename = true;
    }

    /// Checks one dialog row and presses Apply.
    pub fn check_row_and_apply(&self, row: usize) {
        let mut state = self.state();
        if let Some(entry) = state
            .proposal_dialog
            .as_mut()
            .and_then(|dialog| dialog.rows.get_mut(row))
        {
            entry.checked = true;
        }
        resolve_agent_config_proposal(&mut state, false);
    }

    /// Presses Reject all.
    pub fn reject_all(&self) {
        resolve_agent_config_proposal(&mut self.state(), true);
    }

    /// Writes an agent config file directly, as the user editing it.
    pub fn write_agent_config_file(&self, agent: &str, path: &str, content: &[u8]) {
        self.state()
            .files
            .insert(agent_config_key(agent, path), content.to_vec());
    }

    /// The current revision token of an agent config file.
    pub fn agent_config_revision(&self, agent: &str, path: &str) -> String {
        let state = self.state();
        revision_token(
            state
                .files
                .get(&agent_config_key(agent, path))
                .map(Vec::as_slice),
        )
    }

    /// The bytes of an agent config file.
    pub fn read_agent_config_file(&self, agent: &str, path: &str) -> Vec<u8> {
        self.state()
            .files
            .get(&agent_config_key(agent, path))
            .cloned()
            .unwrap_or_default()
    }

    /// Whether `consents.json` is byte-identical to its state at rig start.
    pub fn consents_json_is_unchanged(&self) -> bool {
        let state = self.state();
        state.consents_json == state.consents_at_start
    }

    /// The bytes of a file under the plugin's data directory.
    pub fn plugin_data_file(&self, path: &str) -> Vec<u8> {
        self.file("data", path).unwrap_or_default()
    }

    /// The audit records of one event kind.
    pub fn audit_records(&self, event: &str) -> Vec<serde_json::Value> {
        self.state()
            .audit
            .iter()
            .filter(|(kind, _)| kind == event)
            .map(|(_, record)| record.clone())
            .collect()
    }

    /// Opens the kernel's dialog for a plugin's `propose-agent-config` call (CAP-887). A changed
    /// revision completes the proposal as conflict before any dialog opens (CAP-889), and a second
    /// proposal while one is pending leaves the first dialog open (CAP-890).
    pub fn open_agent_config_proposal(
        &self,
        plugin: &str,
        agent: &str,
        changes: Vec<AgentConfigChange>,
    ) {
        let mut state = self.state();
        if state.pending_proposal.is_some() {
            return;
        }
        state.proposal_completion = None;
        let revisions_hold = changes.iter().all(|change| {
            let current = state.files.get(&agent_config_key(agent, &change.path));
            revision_matches(change.revision.as_deref(), current.map(Vec::as_slice))
        });
        if !revisions_hold {
            state.proposal_completion = Some(Err(PluginError::new(
                ErrorCode::Conflict,
                "a file changed since the plugin read it",
            )));
            return;
        }
        let request = state.next_id;
        state.next_id = state.next_id.saturating_add(1);
        state.proposal_dialog = Some(ProposalDialog {
            rows: changes
                .iter()
                .map(|_| ProposalRow { checked: false })
                .collect(),
            full_diff: proposal_full_diff(&state, agent, &changes),
        });
        state.pending_proposal = Some(PendingProposal {
            plugin: plugin.into(),
            agent: agent.into(),
            request,
            changes,
        });
    }
}

fn refused(code: ErrorCode, message: &str) -> Outcome {
    Outcome::Refused(PluginError::new(code, message))
}
fn failed(code: ErrorCode, message: &str) -> Outcome {
    Outcome::Failed(PluginError::new(code, message))
}
fn changed(state: &mut FakeKernelState, nodes: Vec<NodeId>) {
    state.epoch = state.epoch.saturating_add(1);
    state.events.push_back(Event::TreeChanged(nodes));
}
fn subtree(state: &FakeKernelState, root: NodeId) -> BTreeSet<NodeId> {
    let mut ids = BTreeSet::from([root]);
    loop {
        let before = ids.len();
        for node in &state.nodes {
            if node.parent.is_some_and(|parent| ids.contains(&parent)) {
                ids.insert(node.id);
            }
        }
        if ids.len() == before {
            return ids;
        }
    }
}
fn remove_subtree(state: &mut FakeKernelState, root: NodeId) {
    if !state.nodes.iter().any(|node| node.id == root) {
        return;
    }
    let ids = subtree(state, root);
    let removed: Vec<_> = state
        .nodes
        .iter()
        .filter(|node| ids.contains(&node.id))
        .map(|node| node.id)
        .collect();
    state.nodes.retain(|node| !ids.contains(&node.id));
    for id in removed.into_iter().rev() {
        state.screens.remove(&id);
        state.pane_buffers.remove(&id);
        state.restored.remove(&id);
        state.events.push_back(Event::NodeRemoved(id));
    }
    if state.focus.is_some_and(|id| ids.contains(&id)) {
        state.focus = None;
    }
    state.epoch = state.epoch.saturating_add(1);
}
fn create_node(
    state: &mut FakeKernelState,
    parent: Option<NodeId>,
    title: String,
    class: NodeClass,
    content_kind: Option<String>,
    cwd: Option<String>,
) -> Outcome {
    if let Some(parent) = parent {
        let Some(node) = state.nodes.iter().find(|node| node.id == parent) else {
            return refused(ErrorCode::NotFound, "parent does not exist");
        };
        if node.class == NodeClass::Pane {
            return refused(ErrorCode::InvalidArgument, "parent is a pane");
        }
    }
    let Some(next) = state.next_node_id.checked_add(1) else {
        return failed(ErrorCode::Internal, "node ids exhausted");
    };
    state.next_node_id = next;
    let id = NodeId(next);
    let index = parent
        .and_then(|parent| {
            let children = subtree(state, parent);
            state
                .nodes
                .iter()
                .rposition(|node| children.contains(&node.id))
                .map(|index| index + 1)
        })
        .unwrap_or(state.nodes.len());
    state.nodes.insert(
        index,
        TreeNode {
            id,
            parent,
            class,
            content_kind,
            title,
            cwd,
            agent: None,
            locked: false,
            revision: 0,
        },
    );
    changed(state, vec![id]);
    Outcome::OkNode(id)
}
fn move_node(
    state: &mut FakeKernelState,
    node: NodeId,
    target: NodeId,
    placement: Placement,
) -> Outcome {
    if !state.nodes.iter().any(|entry| entry.id == node)
        || !state.nodes.iter().any(|entry| entry.id == target)
    {
        return refused(ErrorCode::NotFound, "node or target does not exist");
    }
    let ids = subtree(state, node);
    if ids.contains(&target) {
        return refused(ErrorCode::InvalidArgument, "target lies inside moved node");
    }
    let target_node = state.nodes.iter().find(|entry| entry.id == target).cloned();
    let Some(target_node) = target_node else {
        return refused(ErrorCode::NotFound, "target does not exist");
    };
    let inside = matches!(placement, Placement::InsideFirst | Placement::InsideLast);
    if inside && target_node.class == NodeClass::Pane {
        return refused(ErrorCode::InvalidArgument, "cannot move inside a pane");
    }
    let mut moved: Vec<_> = state
        .nodes
        .iter()
        .filter(|entry| ids.contains(&entry.id))
        .cloned()
        .collect();
    state.nodes.retain(|entry| !ids.contains(&entry.id));
    if let Some(root) = moved.first_mut() {
        root.parent = if inside {
            Some(target)
        } else {
            target_node.parent
        };
        root.revision = root.revision.saturating_add(1);
    }
    let target_ids = subtree(state, target);
    let position = match placement {
        Placement::Before => state.nodes.iter().position(|entry| entry.id == target),
        Placement::InsideFirst => state
            .nodes
            .iter()
            .position(|entry| entry.id == target)
            .map(|index| index + 1),
        Placement::After | Placement::InsideLast => state
            .nodes
            .iter()
            .rposition(|entry| target_ids.contains(&entry.id))
            .map(|index| index + 1),
    }
    .unwrap_or(state.nodes.len());
    state.nodes.splice(position..position, moved);
    changed(state, vec![node, target]);
    Outcome::Ok
}
fn position_offset(
    text: &str,
    position: &ilium_plugin_api::generated::types::editing::TextPosition,
) -> Option<usize> {
    let mut offset = 0;
    for (line, contents) in text.split('\n').enumerate() {
        if line == position.line as usize {
            let column = position.column as usize;
            return if column == contents.chars().count() {
                Some(offset + contents.len())
            } else {
                contents
                    .char_indices()
                    .nth(column)
                    .map(|(column, _)| offset + column)
            };
        }
        offset += contents.len() + 1;
    }
    None
}
fn apply_edits(
    state: &mut FakeKernelState,
    buffer: BufferId,
    edits: &[ilium_plugin_api::generated::types::editing::TextEdit],
) -> Outcome {
    let Some(text) = state.buffers.get(&buffer) else {
        return refused(ErrorCode::NotFound, "buffer does not exist");
    };
    let mut ranges = Vec::new();
    for edit in edits {
        let Some(start) = position_offset(text, &edit.range.start) else {
            return refused(ErrorCode::InvalidArgument, "invalid edit start");
        };
        let Some(end) = position_offset(text, &edit.range.end) else {
            return refused(ErrorCode::InvalidArgument, "invalid edit end");
        };
        if start > end {
            return refused(ErrorCode::InvalidArgument, "reversed edit range");
        }
        ranges.push((start, end, edit.replacement.as_str()));
    }
    ranges.sort_by_key(|range| (range.0, range.1));
    if ranges.windows(2).any(|pair| pair[0].1 > pair[1].0) {
        return refused(ErrorCode::InvalidArgument, "overlapping edits");
    }
    let mut text = text.clone();
    for (start, end, replacement) in ranges.into_iter().rev() {
        text.replace_range(start..end, replacement);
    }
    state.buffers.insert(buffer, text);
    let revision = state.buffer_revisions.entry(buffer).or_default();
    *revision += 1;
    state.events.push_back(Event::EditorChanged(
        ilium_plugin_api::generated::types::events::EditorChangedEvent {
            buffer,
            revision: *revision,
            first_line: edits
                .iter()
                .map(|edit| edit.range.start.line)
                .min()
                .unwrap_or(0),
            last_line: edits
                .iter()
                .map(|edit| edit.range.end.line)
                .max()
                .unwrap_or(0),
        },
    ));
    for (pane, id) in &state.pane_buffers {
        if *id == buffer {
            state.dirty.insert(*pane);
        }
    }
    Outcome::Ok
}
fn apply(state: &mut FakeKernelState, intent: Intent) -> Outcome {
    match intent {
        Intent::RenameNode(spec) => {
            let Some(index) = state.nodes.iter().position(|node| node.id == spec.node) else {
                return failed(ErrorCode::NotFound, "node does not exist");
            };
            if spec
                .expected_revision
                .is_some_and(|revision| revision != state.nodes[index].revision)
            {
                return refused(ErrorCode::Conflict, "stale node revision");
            }
            if spec.source == TitleSource::Inferred && state.manual_titles.contains(&spec.node) {
                return refused(ErrorCode::Conflict, "manual title wins");
            }
            state.nodes[index].title = spec.title;
            state.nodes[index].revision = state.nodes[index].revision.saturating_add(1);
            changed(state, vec![spec.node]);
            Outcome::Ok
        }
        Intent::MoveNode(spec) => move_node(state, spec.node, spec.target, spec.placement),
        Intent::CloseNode(node) => {
            let Some(entry) = state.nodes.iter().find(|entry| entry.id == node) else {
                return refused(ErrorCode::NotFound, "node does not exist");
            };
            if entry.locked {
                return refused(ErrorCode::Conflict, "node is locked");
            }
            remove_subtree(state, node);
            Outcome::Ok
        }
        Intent::SetNodeLocked(spec) => {
            let Some(node) = state.nodes.iter_mut().find(|node| node.id == spec.node) else {
                return refused(ErrorCode::NotFound, "node does not exist");
            };
            node.locked = spec.locked;
            node.revision = node.revision.saturating_add(1);
            changed(state, vec![spec.node]);
            Outcome::Ok
        }
        Intent::CreatePane(spec) => {
            let result = create_node(
                state,
                spec.parent,
                spec.title,
                NodeClass::Pane,
                Some(spec.content_kind),
                spec.cwd.map(|directory| directory.path),
            );
            if let Outcome::OkNode(node) = result {
                state.screens.insert(node, FakePaneScreen::default());
                if let Some(command) = spec.command {
                    if let Some(name) = command.first() {
                        state
                            .foreground
                            .insert(node, (name.clone(), command[1..].to_vec()));
                    }
                }
                if let Some(input) = spec.initial_input {
                    state.raw_bytes = state.raw_bytes.saturating_add(input.len() as u64);
                }
            }
            result
        }
        Intent::SendInput(spec) => {
            if !state
                .nodes
                .iter()
                .any(|node| node.id == spec.pane && node.class == NodeClass::Pane)
            {
                return refused(ErrorCode::NotFound, "pane does not exist");
            }
            if state.restored.contains(&spec.pane) {
                return refused(ErrorCode::PaneNotReady, "pane is restored");
            }
            if spec.prompt_epoch.is_some_and(|epoch| {
                epoch != state.prompt_epochs.get(&spec.pane).copied().unwrap_or(0)
            }) {
                return refused(ErrorCode::Conflict, "stale prompt epoch");
            }
            if spec.stages.len() > 16
                || spec
                    .stages
                    .iter()
                    .map(|stage| u64::from(stage.delay_ms))
                    .sum::<u64>()
                    > 10_000
                || spec.raw.as_ref().is_some_and(|bytes| bytes.len() > 65_536)
            {
                return refused(
                    ErrorCode::InvalidArgument,
                    "input exceeds stage or byte limit",
                );
            }
            if spec.mouse.as_ref().is_some_and(|events| !events.is_empty()) {
                return refused(
                    ErrorCode::Unsupported,
                    "mouse encoding requires a terminal adapter",
                );
            }
            if let Some(condition) = &spec.if_current {
                let activity = state
                    .nodes
                    .iter()
                    .find(|node| node.id == spec.pane)
                    .and_then(|node| node.agent.as_ref().map(|agent| agent.activity));
                if condition
                    .activity
                    .is_some_and(|expected| Some(expected) != activity)
                    || condition.screen_revision.is_some_and(|expected| {
                        expected != state.screen_revisions.get(&spec.pane).copied().unwrap_or(0)
                    })
                    || condition.foreground.as_ref().is_some_and(|expected| {
                        state
                            .foreground
                            .get(&spec.pane)
                            .is_none_or(|(name, _)| name != expected)
                    })
                {
                    return refused(ErrorCode::Conflict, "input precondition changed");
                }
            }
            if spec.journal {
                let Some(key) = state.current_submission_key.clone() else {
                    return refused(
                        ErrorCode::Unsupported,
                        "journaled input needs submission envelope",
                    );
                };
                state.journal.insert((spec.pane, key.clone()));
                state.steps.push(format!("journal:{key}"));
                if state.interrupt.as_ref() == Some(&(spec.pane, key.clone())) {
                    state.interrupt = None;
                    state.uncertain_journal.insert((spec.pane, key));
                    return failed(
                        ErrorCode::Internal,
                        "interrupted after journal before pane write",
                    );
                }
            }
            if let Some(code) = state.write_error.take() {
                return failed(code, "injected pane write failure");
            }
            let mut bytes = spec.raw.clone().unwrap_or_default();
            let bracketed = state
                .modes
                .get(&spec.pane)
                .is_some_and(|mode| mode.contains(TerminalMode::BRACKETED_PASTE));
            let mut submitted = 0u64;
            if spec.raw.is_none() {
                if spec.stages.is_empty() {
                    append_input(&mut bytes, &spec.text, spec.delivery, bracketed);
                    submitted = u64::from(spec.delivery == Delivery::Submitted);
                } else {
                    for stage in &spec.stages {
                        append_input(&mut bytes, &stage.text, stage.delivery, bracketed);
                        submitted += u64::from(stage.delivery == Delivery::Submitted);
                    }
                }
            }
            if std::mem::take(&mut state.interrupt_next_pane_write) {
                // The interrupted write keeps the bytes that reached the pane before the cut.
                bytes.truncate(bytes.len() / 2);
                state.pane_writes.push((spec.pane, bytes.clone()));
                state.raw_bytes = state.raw_bytes.saturating_add(bytes.len() as u64);
                state.uncertain_composers.insert(spec.pane);
                return failed(ErrorCode::Io, "pane write interrupted part-way");
            }
            state.steps.push(format!("pane-write:{}", spec.pane.0));
            state.pane_writes.push((spec.pane, bytes.clone()));
            state.raw_bytes = state.raw_bytes.saturating_add(bytes.len() as u64);
            state.uncertain_composers.insert(spec.pane);
            let agent = state
                .nodes
                .iter()
                .find(|node| node.id == spec.pane)
                .is_some_and(|node| node.agent.is_some());
            if agent && submitted > 0 {
                let epoch = state.prompt_epochs.entry(spec.pane).or_default();
                *epoch = epoch.saturating_add(submitted);
                Outcome::OkEpoch(*epoch)
            } else {
                Outcome::Ok
            }
        }
        Intent::AgentAction(_) => refused(
            ErrorCode::Unsupported,
            "agent action requires signature input adapter",
        ),
        Intent::SignalPane(spec) => {
            if !state.nodes.iter().any(|node| node.id == spec.pane) {
                return refused(ErrorCode::NotFound, "pane does not exist");
            }
            if state.restored.contains(&spec.pane) {
                return refused(ErrorCode::PaneNotReady, "pane is restored");
            }
            refused(ErrorCode::Unsupported, "signals require a process adapter")
        }
        Intent::FreezePane(spec) => {
            if !state.nodes.iter().any(|node| node.id == spec.pane) {
                return refused(ErrorCode::NotFound, "pane does not exist");
            }
            if !state.frozen_panes.insert(spec.pane) {
                return refused(ErrorCode::Conflict, "pane is already frozen");
            }
            Outcome::Ok
        }
        Intent::UnfreezePane(spec) => {
            if !state.frozen_panes.remove(&spec.pane) {
                refused(ErrorCode::Conflict, "pane is not frozen")
            } else {
                Outcome::Ok
            }
        }
        Intent::ResizePane(spec) => {
            if !state.nodes.iter().any(|node| node.id == spec.pane) {
                return refused(ErrorCode::NotFound, "pane does not exist");
            }
            let old = state.sizes.get(&spec.pane).copied().unwrap_or((80, 24));
            let size = (spec.columns.unwrap_or(old.0), spec.rows.unwrap_or(old.1));
            if size.0 == 0 || size.1 == 0 {
                return refused(
                    ErrorCode::InvalidArgument,
                    "pane dimensions must be positive",
                );
            }
            state.sizes.insert(spec.pane, size);
            Outcome::Ok
        }
        Intent::CreateGroup(spec) => {
            if spec
                .children
                .iter()
                .any(|id| !state.nodes.iter().any(|node| node.id == *id))
            {
                return refused(ErrorCode::NotFound, "child does not exist");
            }
            if spec.children.iter().copied().collect::<BTreeSet<_>>().len() != spec.children.len() {
                return refused(ErrorCode::InvalidArgument, "duplicate group child");
            }
            let mut working = state.clone();
            let parent = match (spec.parent, spec.placement) {
                (Some(target), Placement::Before | Placement::After) => working
                    .nodes
                    .iter()
                    .find(|node| node.id == target)
                    .and_then(|node| node.parent),
                _ => spec.parent,
            };
            let result = create_node(
                &mut working,
                parent,
                spec.title,
                NodeClass::Container,
                None,
                None,
            );
            if let Outcome::OkNode(id) = result {
                if let Some(target) = spec.parent {
                    let placement = move_node(&mut working, id, target, spec.placement);
                    if !matches!(placement, Outcome::Ok) {
                        return placement;
                    }
                }
                for child in spec.children {
                    let result = move_node(&mut working, child, id, Placement::InsideLast);
                    if !matches!(result, Outcome::Ok) {
                        return result;
                    }
                }
                *state = working;
            }
            result
        }
        Intent::CreateFolder(spec) => {
            let outcome = create_node(
                state,
                spec.parent,
                spec.title,
                NodeClass::Folder,
                None,
                None,
            );
            if let Outcome::OkNode(id) = outcome {
                if let Some(path) = spec.path {
                    state.node_paths.insert(id, path);
                }
            }
            outcome
        }
        Intent::CreateProject(spec) => {
            let outcome = create_node(state, None, spec.title, NodeClass::Folder, None, None);
            if let Outcome::OkNode(id) = outcome {
                state.node_paths.insert(id, spec.root);
            }
            outcome
        }
        Intent::SetBookmarked(spec) => {
            if !state.nodes.iter().any(|node| node.id == spec.node) {
                return refused(ErrorCode::NotFound, "node does not exist");
            }
            if spec.bookmarked {
                state.bookmarks.insert(spec.node);
            } else {
                state.bookmarks.remove(&spec.node);
            }
            Outcome::Ok
        }
        Intent::SetExpanded(spec) => {
            match spec.target {
                ExpandTarget::Node(node) => {
                    if !state.nodes.iter().any(|entry| entry.id == node) {
                        return refused(ErrorCode::NotFound, "node does not exist");
                    }
                    state.expanded.insert(node, spec.expanded);
                }
                ExpandTarget::VirtualRow(_) => {
                    return refused(
                        ErrorCode::Unsupported,
                        "virtual row ownership requires contribution adapter",
                    )
                }
            };
            Outcome::Ok
        }
        Intent::MarkAttention(spec) => {
            if spec.reason.chars().count() > 80 {
                return refused(
                    ErrorCode::InvalidArgument,
                    "attention reason exceeds 80 characters",
                );
            }
            if !state.nodes.iter().any(|node| node.id == spec.node) {
                return refused(ErrorCode::NotFound, "node does not exist");
            }
            if !admit_rate(
                state,
                format!("attention:{}", spec.node.0),
                10,
                Duration::from_secs(1),
            ) {
                return refused(ErrorCode::RateLimited, "attention rate limit reached");
            }
            state.attention.insert(spec.node, Some(spec.reason));
            Outcome::Ok
        }
        Intent::AckAttention(node) => {
            if !state.nodes.iter().any(|entry| entry.id == node) {
                return refused(ErrorCode::NotFound, "node does not exist");
            }
            if !admit_rate(
                state,
                format!("attention:{}", node.0),
                10,
                Duration::from_secs(1),
            ) {
                return refused(ErrorCode::RateLimited, "attention rate limit reached");
            }
            state.attention.remove(&node);
            Outcome::Ok
        }
        Intent::SetInferredIcon(spec) => {
            if !state.nodes.iter().any(|node| node.id == spec.node) {
                return refused(ErrorCode::NotFound, "node does not exist");
            }
            state.icons.insert(spec.node, spec.icon);
            Outcome::Ok
        }
        Intent::EditorApplyEdits(spec) => {
            if !state.buffers.contains_key(&spec.buffer) {
                return refused(ErrorCode::NotFound, "buffer does not exist");
            }
            if state.buffer_revisions.get(&spec.buffer).copied() != Some(spec.base_revision) {
                return refused(ErrorCode::Conflict, "stale buffer revision");
            }
            apply_edits(state, spec.buffer, &spec.edits)
        }
        Intent::OpenView(spec) => match spec.view {
            ViewTarget::Panel(panel) => {
                if !state.panels.contains(&panel) {
                    return refused(ErrorCode::NotFound, "panel is undeclared");
                }
                state.open_panels.insert(panel);
                Outcome::Ok
            }
            ViewTarget::Pane(node) => {
                if !state.nodes.iter().any(|entry| entry.id == node) {
                    return refused(ErrorCode::NotFound, "pane does not exist");
                }
                if spec.focus {
                    state.focus = Some(node);
                }
                Outcome::Ok
            }
            _ => refused(ErrorCode::Unsupported, "view requires UI adapter"),
        },
        Intent::CloseView(view) => match view {
            ViewTarget::Panel(panel) => {
                state.open_panels.remove(&panel);
                Outcome::Ok
            }
            _ => refused(ErrorCode::Unsupported, "view requires UI adapter"),
        },
        Intent::Restructure(spec) => {
            if spec.operations.len() > 256 {
                return refused(
                    ErrorCode::InvalidArgument,
                    "too many restructure operations",
                );
            }
            let before = state.nodes.clone();
            let mut working = state.clone();
            for (index, operation) in spec.operations.into_iter().enumerate() {
                let intent = match operation {
                    RestructureOp::Move(spec) => Intent::MoveNode(spec),
                    RestructureOp::CreateGroup(spec) => Intent::CreateGroup(spec),
                    RestructureOp::Rename(spec) => Intent::RenameNode(spec),
                    RestructureOp::Ungroup(_) => {
                        return refused(ErrorCode::Unsupported, "ungroup requires layout adapter")
                    }
                };
                let outcome = apply(&mut working, intent);
                if !matches!(outcome, Outcome::Ok | Outcome::OkNode(_)) {
                    return refused(
                        ErrorCode::Conflict,
                        &format!("restructure operation {index} failed"),
                    );
                }
            }
            working.next_id = working.next_id.saturating_add(1);
            let token = working.next_id;
            working.undo.insert(token, (working.epoch, before));
            *state = working;
            Outcome::OkNode(NodeId(token))
        }
        Intent::RevertRestructure(token) => {
            let Some((epoch, nodes)) = state.undo.get(&token).cloned() else {
                return refused(ErrorCode::NotFound, "undo token does not exist");
            };
            if epoch != state.epoch {
                return refused(ErrorCode::Conflict, "tree changed after restructure");
            }
            state.nodes = nodes;
            state.undo.remove(&token);
            let ids = state.nodes.iter().map(|node| node.id).collect();
            changed(state, ids);
            Outcome::Ok
        }
        _ => refused(
            ErrorCode::Unsupported,
            "intent requires a host-service or layout adapter",
        ),
    }
}

fn append_input(bytes: &mut Vec<u8>, text: &str, delivery: Delivery, bracketed: bool) {
    let wrap = delivery == Delivery::Pasted && bracketed;
    if wrap {
        bytes.extend_from_slice(b"\x1b[200~");
    }
    bytes.extend_from_slice(text.as_bytes());
    if wrap {
        bytes.extend_from_slice(b"\x1b[201~");
    }
    if delivery == Delivery::Submitted {
        bytes.push(b'\r');
    }
}

fn admit_rate(state: &mut FakeKernelState, key: String, maximum: usize, window: Duration) -> bool {
    let times = state
        .rate_windows
        .entry((state.current_plugin.clone(), key))
        .or_default();
    while times
        .front()
        .is_some_and(|at| state.clock.saturating_sub(*at) >= window)
    {
        times.pop_front();
    }
    if times.len() >= maximum {
        return false;
    }
    times.push_back(state.clock);
    true
}

impl crate::imports::basic::BasicBackend for FakeKernel {
    fn apply_basic(
        &self,
        plugin: &str,
        pane: NodeId,
        change: crate::imports::basic::BasicChange,
    ) -> Result<(), PluginError> {
        use crate::imports::basic::BasicChange;
        let mut state = self.state();
        let Some(index) = state
            .nodes
            .iter()
            .position(|node| node.id == pane && basic_kind_owned(&state, node, plugin))
        else {
            return Err(PluginError::new(
                ErrorCode::NotPermitted,
                "pane is not owned by this plugin",
            ));
        };
        match change {
            BasicChange::Title(title) => {
                if state.manual_titles.contains(&pane) {
                    return Err(PluginError::new(
                        ErrorCode::Conflict,
                        "manual title takes precedence",
                    ));
                }
                state.nodes[index].title = title;
                state.nodes[index].revision = state.nodes[index].revision.saturating_add(1);
            }
            BasicChange::Status(status) => {
                state
                    .status
                    .insert(pane, status.iter().map(|span| span.text.as_str()).collect());
                state.styled_status.insert(pane, status);
            }
            BasicChange::Focus => state.focus = Some(pane),
            BasicChange::Activity(activity) => {
                state.basic_activities.insert(pane, activity);
            }
            BasicChange::Dirty(dirty) => {
                if dirty {
                    state.dirty.insert(pane);
                } else {
                    state.dirty.remove(&pane);
                }
            }
        }
        state.epoch = state.epoch.saturating_add(1);
        Ok(())
    }
    fn basic_size(&self, plugin: &str, pane: NodeId) -> Option<(u16, u16)> {
        let state = self.state();
        state
            .nodes
            .iter()
            .find(|node| node.id == pane && basic_kind_owned(&state, node, plugin))?;
        state.sizes.get(&pane).copied()
    }
}

fn basic_kind_owned(state: &FakeKernelState, node: &TreeNode, plugin: &str) -> bool {
    node.class == NodeClass::Pane
        && node
            .content_kind
            .as_ref()
            .and_then(|kind| state.pane_kind_registry.get(kind))
            .is_some_and(|(owner, primitive)| {
                owner == plugin
                    && *primitive == ilium_plugin_api::generated::types::tree::PrimitiveKind::Basic
            })
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
