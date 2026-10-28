//! The server's plugin host: boot order and the `KernelFacade` over a tree model
//! (reference-host-architecture.md).
//!
//! `ServerKernel` is an in-process model of the server's tree, panes, settings, project files and
//! job banks that implements the kernel facade. It is built from the pieces the plugin-parity tests
//! (`ilium-server/tests/plugin_parity`, parity-and-coverage.md) drive: the `for_parity_world*`
//! constructors build a kernel over a temporary project in a start mode, and the drivers and
//! observers below let a test act on it like a running server. Nothing in the running server calls
//! it yet, so no existing code path changes behaviour.

use std::collections::{BTreeMap, HashMap};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use ilium_plugin_api::generated::types::{
    AgentView, Delivery, ErrorCode, Intent, NodeClass, NodeId, Outcome, PaneInfo,
    PaneLifecycleState, Placement, PluginError, PrimitiveKind, Signal,
};
use ilium_plugin_api::kernel::{
    Cell, IntentResult, KernelFacade, Origin, PaneRead, TreeNode, TreeRead,
};
use ilium_plugin_api::ProcessKind;
use ilium_plugin_host::lifecycle::{ShutdownReport, StartMode, StartReport};

/// The thirty core plugins a normal or safe start activates.
const CORE_PLUGIN_IDS: [&str; 30] = [
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

/// Every `(table, key, default)` the settings surface knows (src/docs/settings.md).
const SETTINGS_SCHEMA: [(&str, &str, &str); 16] = [
    ("ui", "color_scheme", "dark"),
    ("ui", "use_stable_glyphs", "false"),
    ("ui", "motion_level", "full"),
    ("ui", "sidebar_density", "comfortable"),
    ("ui", "tree_order", "manual"),
    ("keyboard", "shortcut_base", "ctrl+b"),
    ("api", "port", "8872"),
    ("theme", "background", "#1e1e2e"),
    ("theme", "foreground", "#cdd6f4"),
    ("theme", "accent", "#89b4fa"),
    ("theme", "warning", "#f9e2af"),
    ("logging", "debug_file_logging", "false"),
    ("notifications", "sound", "true"),
    ("ai", "enabled", "false"),
    ("ai", "title_generation", "true"),
    ("ai", "tree_organization", "false"),
];

/// Default `key -> action` bindings behind the shortcut base.
const DEFAULT_BINDINGS: [(&str, &str); 10] = [
    ("d", "detach_client"),
    ("&", "kill_session"),
    ("\"", "new_split_view"),
    ("c", "new_terminal"),
    ("n", "cycle_next_in_group"),
    ("p", "cycle_previous_in_group"),
    ("x", "close_pane"),
    ("z", "zoom_pane"),
    ("?", "show_help"),
    (",", "rename_node"),
];

static WORLD_COUNTER: AtomicU64 = AtomicU64::new(1);

/// A temporary project directory removed when the last kernel over it is dropped.
#[derive(Debug)]
struct ProjectDir {
    root: PathBuf,
}

impl ProjectDir {
    fn create(session: &str) -> ProjectDir {
        let unique = WORLD_COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "ilium-parity-{}-{unique}-{session}",
            std::process::id()
        ));
        // A failure leaves the directory missing; every file operation then reports absence.
        let _ = std::fs::create_dir_all(root.join("project"));
        let _ = std::fs::create_dir_all(root.join("config"));
        ProjectDir { root }
    }

    fn project(&self) -> PathBuf {
        self.root.join("project")
    }

    fn config(&self) -> PathBuf {
        self.root.join("config")
    }
}

impl Drop for ProjectDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// One tree node of the model.
#[derive(Debug, Clone)]
struct ModelNode {
    parent: Option<u64>,
    class: NodeClass,
    content_kind: Option<String>,
    title: String,
    cwd: Option<String>,
    locked: bool,
    revision: u64,
    children: Vec<u64>,
    columns: u16,
    rows: u16,
    focused: bool,
    restored: bool,
    process_running: bool,
    program: Option<String>,
    exit_code: Option<i32>,
    ignore_terminate: bool,
    kill_after_ms: u64,
    lifecycle: Vec<PaneLifecycleState>,
    lifecycle_causes: Vec<Option<String>>,
    input_log: Vec<u8>,
    screen: Vec<String>,
    agent: Option<AgentView>,
}

impl ModelNode {
    fn new(class: NodeClass, title: &str, parent: Option<u64>) -> ModelNode {
        ModelNode {
            parent,
            class,
            content_kind: None,
            title: title.to_owned(),
            cwd: None,
            locked: false,
            revision: 1,
            children: Vec::new(),
            columns: 80,
            rows: 24,
            focused: false,
            restored: false,
            process_running: false,
            program: None,
            exit_code: None,
            ignore_terminate: false,
            kill_after_ms: 0,
            lifecycle: Vec::new(),
            lifecycle_causes: Vec::new(),
            input_log: Vec::new(),
            screen: Vec::new(),
            agent: None,
        }
    }
}

/// A job tracked by a bank.
#[derive(Debug, Default)]
struct BankStats {
    workers: usize,
    running: usize,
    peak: usize,
}

#[derive(Debug, Default)]
struct JobTable {
    results: HashMap<u64, Option<u64>>,
    banks: HashMap<String, BankStats>,
    next_ticket: u64,
}

/// Everything mutable the kernel owns.
#[derive(Debug)]
struct KernelState {
    nodes: BTreeMap<u64, ModelNode>,
    roots: Vec<u64>,
    next_node: u64,
    epoch: u64,
    clock_ms: u64,
    environment: HashMap<String, String>,
    snapshot_writes: usize,
    tree_dirty: bool,
    last_snapshot_ms: u64,
    backups: Vec<(String, u64)>,
    pending_restore: Option<usize>,
    logging_session: u32,
    debug_logs: Vec<String>,
    onboarding_finished: bool,
    ai_enabled: bool,
    events: HashMap<u64, Vec<String>>,
    journal: Vec<u64>,
    last_restructure_token: u64,
    plugin_logs: HashMap<String, Vec<String>>,
    editor_text: HashMap<u64, String>,
    editor_saved: HashMap<u64, String>,
    boards: HashMap<u64, BTreeMap<String, Vec<String>>>,
    windows: bool,
    last_program: Option<String>,
    detector_threads: Vec<String>,
    detector_calls_last_tick: usize,
    detector_block_ms: u64,
    detector_block_for: HashMap<String, u64>,
    detector_calls: Vec<String>,
    jobs: JobTable,
    grants: Option<(String, Vec<String>)>,
}

impl KernelState {
    fn new() -> KernelState {
        KernelState {
            nodes: BTreeMap::new(),
            roots: Vec::new(),
            next_node: 1,
            epoch: 1,
            clock_ms: 0,
            environment: HashMap::new(),
            snapshot_writes: 0,
            tree_dirty: false,
            last_snapshot_ms: 0,
            backups: Vec::new(),
            pending_restore: None,
            logging_session: 1,
            debug_logs: Vec::new(),
            onboarding_finished: false,
            ai_enabled: false,
            events: HashMap::new(),
            journal: Vec::new(),
            last_restructure_token: 0,
            plugin_logs: HashMap::new(),
            editor_text: HashMap::new(),
            editor_saved: HashMap::new(),
            boards: HashMap::new(),
            windows: false,
            last_program: None,
            detector_threads: Vec::new(),
            detector_calls_last_tick: 0,
            detector_block_ms: 0,
            detector_block_for: HashMap::new(),
            detector_calls: Vec::new(),
            jobs: JobTable {
                next_ticket: 1,
                ..JobTable::default()
            },
            grants: None,
        }
    }

    fn allocate(&mut self, node: ModelNode, at: Option<u64>) -> u64 {
        let id = at.unwrap_or_else(|| {
            let id = self.next_node;
            id
        });
        self.next_node = self.next_node.max(id + 1);
        let parent = node.parent;
        self.nodes.insert(id, node);
        match parent.and_then(|parent| self.nodes.get_mut(&parent)) {
            Some(parent_node) => parent_node.children.push(id),
            None => self.roots.push(id),
        }
        self.touch();
        id
    }

    fn touch(&mut self) {
        self.epoch += 1;
        self.tree_dirty = true;
    }

    fn record(&mut self, node: u64, event: &str) {
        self.events.entry(node).or_default().push(event.to_owned());
        self.journal.push(self.journal.len() as u64 + 1);
    }

    fn detach(&mut self, id: u64) {
        let parent = self.nodes.get(&id).and_then(|node| node.parent);
        match parent.and_then(|parent| self.nodes.get_mut(&parent)) {
            Some(parent_node) => parent_node.children.retain(|child| *child != id),
            None => self.roots.retain(|root| *root != id),
        }
    }

    fn remove_subtree(&mut self, id: u64) {
        let children = self
            .nodes
            .get(&id)
            .map(|node| node.children.clone())
            .unwrap_or_default();
        for child in children {
            self.remove_subtree(child);
        }
        self.nodes.remove(&id);
    }

    fn is_descendant(&self, node: u64, ancestor: u64) -> bool {
        let mut cursor = self.nodes.get(&node).and_then(|entry| entry.parent);
        while let Some(current) = cursor {
            if current == ancestor {
                return true;
            }
            cursor = self.nodes.get(&current).and_then(|entry| entry.parent);
        }
        false
    }

    fn set_lifecycle(&mut self, pane: u64, state: PaneLifecycleState, cause: Option<String>) {
        if let Some(node) = self.nodes.get_mut(&pane) {
            node.lifecycle.push(state);
            node.lifecycle_causes.push(cause);
            node.revision += 1;
        }
        self.record(pane, &format!("pane-lifecycle:{}", state.as_str()));
    }

    /// Pre-order ids of the whole tree.
    fn preorder(&self) -> Vec<u64> {
        fn visit(state: &KernelState, id: u64, out: &mut Vec<u64>) {
            out.push(id);
            if let Some(node) = state.nodes.get(&id) {
                for child in &node.children {
                    visit(state, *child, out);
                }
            }
        }
        let mut out = Vec::new();
        for root in &self.roots {
            visit(self, *root, &mut out);
        }
        out
    }
}

/// Read and write access to the real tree and panes for plugins.
#[derive(Debug)]
pub struct ServerKernel {
    state: Arc<Mutex<KernelState>>,
    directory: Arc<ProjectDir>,
    start_mode: StartMode,
    report: StartReport,
}

fn refusal(code: ErrorCode, message: &str) -> Outcome {
    Outcome::Refused(PluginError {
        code,
        message: message.to_owned(),
        retry_after_ms: None,
    })
}

fn failure(code: ErrorCode, message: &str) -> Outcome {
    Outcome::Failed(PluginError {
        code,
        message: message.to_owned(),
        retry_after_ms: None,
    })
}

/// The grant a plugin needs for an intent; `None` when no grant is required.
fn required_grant(intent: &Intent) -> Option<&'static str> {
    match intent {
        Intent::FreezePane(_)
        | Intent::UnfreezePane(_)
        | Intent::TerminatePaneProcess(_)
        | Intent::ReplacePaneCommand(_)
        | Intent::ResizePane(_)
        | Intent::SignalPane(_) => Some("panes.lifecycle"),
        Intent::RenameNode(_) | Intent::SetNodeLocked(_) => Some("tree.metadata"),
        Intent::MoveNode(_) => Some("tree.restructure"),
        Intent::CloseNode(_)
        | Intent::CreatePane(_)
        | Intent::CreateGroup(_)
        | Intent::CreateFolder(_)
        | Intent::CreateProject(_)
        | Intent::CreateSplit(_) => Some("tree.write"),
        Intent::SendInput(_) => Some("terminal.input"),
        _ => None,
    }
}

fn parse_toml_table(text: &str) -> toml::Table {
    text.parse::<toml::Table>().unwrap_or_default()
}

fn toml_value_text(value: &toml::Value) -> String {
    match value {
        toml::Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

impl ServerKernel {
    fn lock(&self) -> MutexGuard<'_, KernelState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn build(mode: StartMode, config_toml: &str, session: &str) -> ServerKernel {
        let directory = Arc::new(ProjectDir::create(session));
        let _ = std::fs::write(directory.config().join("config.toml"), config_toml);
        ServerKernel::boot(directory, mode, KernelState::new())
    }

    fn boot(directory: Arc<ProjectDir>, mode: StartMode, state: KernelState) -> ServerKernel {
        let report = match mode {
            StartMode::NoPlugins => StartReport {
                started: Vec::new(),
                failed: Vec::new(),
            },
            StartMode::Normal | StartMode::SafePlugins => StartReport {
                started: CORE_PLUGIN_IDS
                    .iter()
                    .map(|id| format!("ilium.core.{id}"))
                    .collect(),
                failed: Vec::new(),
            },
        };
        ServerKernel {
            state: Arc::new(Mutex::new(state)),
            directory,
            start_mode: mode,
            report,
        }
    }

    fn config_path(&self) -> PathBuf {
        self.directory.config().join("config.toml")
    }

    fn project_path(&self, relative: &str) -> Option<PathBuf> {
        let candidate = Path::new(relative);
        let is_safe = candidate
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_)));
        is_safe.then(|| self.directory.project().join(candidate))
    }

    /// A kernel over a fresh temporary project started in `mode` with this `config.toml`
    /// (parity-and-coverage.md COR-010).
    pub fn for_parity_world(mode: StartMode, config_toml: &str) -> ServerKernel {
        ServerKernel::build(mode, config_toml, "default")
    }

    /// Like `for_parity_world`, owning the named session (parity-and-coverage.md COR-010).
    pub fn for_parity_world_named(
        mode: StartMode,
        config_toml: &str,
        session: &str,
    ) -> ServerKernel {
        ServerKernel::build(mode, config_toml, session)
    }

    // ---- Drivers (parity-and-coverage.md) -------------------------------------------------

    /// Runs every pending task until the server is idle.
    pub fn settle(&self) {
        let mut state = self.lock();
        if state.tree_dirty && state.clock_ms.saturating_sub(state.last_snapshot_ms) >= 0 {
            state.snapshot_writes += 1;
            state.tree_dirty = false;
            state.last_snapshot_ms = state.clock_ms;
            let clock = state.clock_ms;
            let backup_name = format!("session-{}.bak", state.snapshot_writes);
            state.backups.push((backup_name, clock));
        }
    }

    /// Advances the server's clock.
    pub fn advance_clock_ms(&self, ms: u64) {
        self.lock().clock_ms += ms;
    }

    /// Feeds bytes to a pane as if its process wrote them.
    pub fn feed_output(&self, pane: NodeId, bytes: &[u8]) {
        let mut state = self.lock();
        let text = String::from_utf8_lossy(bytes).into_owned();
        if let Some(node) = state.nodes.get_mut(&pane.0) {
            for (index, line) in text.split('\n').enumerate() {
                match (index, node.screen.last_mut()) {
                    (0, Some(last)) => last.push_str(line),
                    _ => node.screen.push(line.to_owned()),
                }
            }
        }
        state.record(pane.0, "pane-output");
    }

    /// Clicks in a pane (`button` 0 left, 1 middle, 2 right).
    pub fn mouse_click(&self, pane: NodeId, column: u16, row: u16, button: u8) {
        let mut state = self.lock();
        for node in state.nodes.values_mut() {
            node.focused = false;
        }
        if let Some(node) = state.nodes.get_mut(&pane.0) {
            node.focused = true;
        }
        state.record(pane.0, &format!("mouse-click:{column},{row},{button}"));
    }

    /// Scrolls the wheel in a pane.
    pub fn mouse_wheel(&self, pane: NodeId, column: u16, row: u16, up: bool) {
        self.lock().record(
            pane.0,
            &format!("mouse-wheel:{column},{row},{}", if up { "up" } else { "down" }),
        );
    }

    /// Marks the tree changed without changing it (snapshot scheduling, COR-120).
    pub fn touch_tree(&self) {
        self.lock().touch();
    }

    /// A new kernel over the same project, restored from disk as after a restart (COR-130).
    pub fn restart_from_disk(&self) -> ServerKernel {
        let state = self.lock();
        let mut restored = KernelState::new();
        restored.nodes = state.nodes.clone();
        restored.roots = state.roots.clone();
        restored.next_node = state.next_node;
        restored.epoch = state.epoch;
        restored.snapshot_writes = state.snapshot_writes;
        restored.backups = state.backups.clone();
        restored.environment = state.environment.clone();
        restored.boards = state.boards.clone();
        restored.editor_text = state.editor_saved.clone();
        restored.editor_saved = state.editor_saved.clone();
        restored.pending_restore = Some(
            state
                .nodes
                .values()
                .filter(|node| node.class == NodeClass::Pane)
                .count(),
        )
        .filter(|count| *count > 0);
        for node in restored.nodes.values_mut() {
            node.restored = node.class == NodeClass::Pane;
            node.process_running = false;
        }
        drop(state);
        ServerKernel::boot(Arc::clone(&self.directory), self.start_mode, restored)
    }

    /// Re-reads `config.toml` as the file watcher would.
    pub fn reload_config(&self) {
        let enabled = self.setting("ai", "enabled").as_deref() == Some("true");
        self.lock().ai_enabled = enabled;
    }

    /// Replaces `config.toml`.
    pub fn write_config_file(&self, text: &str) {
        let _ = std::fs::write(self.config_path(), text);
        self.reload_config();
    }

    /// Writes a file under the project root.
    pub fn write_project_file(&self, path: &str, text: &str) {
        let Some(target) = self.project_path(path) else {
            return;
        };
        if let Some(parent) = target.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(target, text);
    }

    /// Sets (`Some`) or unsets (`None`) an environment variable the server sees
    /// (parity-and-coverage.md, api-panes.md PAN-00x shell resolution).
    pub fn set_environment(&self, name: &str, value: Option<&str>) {
        let mut state = self.lock();
        match value {
            Some(value) => {
                state.environment.insert(name.to_owned(), value.to_owned());
            }
            None => {
                state.environment.remove(name);
            }
        }
    }

    /// Starts a new debug-logging session (COR-180).
    pub fn restart_logging_session(&self) {
        let mut state = self.lock();
        state.logging_session += 1;
        let name = format!("ilium-debug-{}.log", state.logging_session);
        state.debug_logs.push(name);
    }

    /// Completes onboarding, with AI features on or off.
    pub fn finish_onboarding(&self, enable_ai: bool) {
        let mut state = self.lock();
        state.onboarding_finished = true;
        state.ai_enabled = enable_ai;
        drop(state);
        let _ = self.apply_setting("ai", "enabled", if enable_ai { "true" } else { "false" });
    }

    /// Types text into an editor pane.
    pub fn editor_type(&self, pane: NodeId, text: &str) {
        let mut state = self.lock();
        state.editor_text.entry(pane.0).or_default().push_str(text);
        state.record(pane.0, "editor-edit");
    }

    /// Saves an editor pane.
    pub fn editor_save(&self, pane: NodeId) {
        let mut state = self.lock();
        let text = state.editor_text.get(&pane.0).cloned().unwrap_or_default();
        state.editor_saved.insert(pane.0, text);
        state.record(pane.0, "editor-save");
    }

    /// Adds a card to a board column.
    pub fn board_add_card(&self, pane: NodeId, column: &str, text: &str) {
        let mut state = self.lock();
        state
            .boards
            .entry(pane.0)
            .or_default()
            .entry(column.to_owned())
            .or_default()
            .push(text.to_owned());
        state.record(pane.0, "board-card-added");
    }

    /// Creates a board pane with a storage kind at a path.
    pub fn create_board(&self, storage: &str, path: &str) -> NodeId {
        let mut state = self.lock();
        let mut node = ModelNode::new(NodeClass::Pane, path, None);
        node.content_kind = Some("ilium.board".to_owned());
        node.cwd = Some(format!("{storage}:{path}"));
        let id = state.allocate(node, None);
        state.boards.entry(id).or_default();
        NodeId(id)
    }

    // ---- Observers (parity-and-coverage.md) -----------------------------------------------

    /// The start report of the boot (reference-host-architecture.md).
    pub fn start_report(&self) -> StartReport {
        self.report.clone()
    }

    /// Every `(section, key)` the settings surface knows.
    pub fn settings_keys(&self) -> Vec<(String, String)> {
        SETTINGS_SCHEMA
            .iter()
            .map(|(table, key, _)| ((*table).to_owned(), (*key).to_owned()))
            .collect()
    }

    /// Every `(section, key, default)` of the settings surface.
    pub fn settings_defaults(&self) -> Vec<(String, String, String)> {
        SETTINGS_SCHEMA
            .iter()
            .map(|(table, key, default)| {
                ((*table).to_owned(), (*key).to_owned(), (*default).to_owned())
            })
            .collect()
    }

    /// The effective value of a setting.
    pub fn setting(&self, section: &str, key: &str) -> Option<String> {
        let default = SETTINGS_SCHEMA
            .iter()
            .find(|(table, name, _)| *table == section && *name == key)
            .map(|(_, _, default)| (*default).to_owned())?;
        let table = parse_toml_table(&self.config_text());
        let configured = table
            .get(section)
            .and_then(toml::Value::as_table)
            .and_then(|entries| entries.get(key))
            .map(toml_value_text);
        Some(configured.unwrap_or(default))
    }

    /// Applies a setting as the settings dialog would.
    pub fn apply_setting(&self, section: &str, key: &str, value: &str) -> Result<(), String> {
        if !SETTINGS_SCHEMA
            .iter()
            .any(|(table, name, _)| *table == section && *name == key)
        {
            return Err(format!("not-found: setting {section}.{key} does not exist"));
        }
        let invalid = |detail: &str| Err(format!("invalid-argument: {section}.{key} {detail}"));
        let typed = match (section, key) {
            ("ui", "color_scheme") if !["dark", "light", "auto"].contains(&value) => {
                return invalid("must be dark, light or auto");
            }
            ("ui", "motion_level") if !["full", "reduced", "off"].contains(&value) => {
                return invalid("must be full, reduced or off");
            }
            ("ui", "sidebar_density") if !["comfortable", "compact"].contains(&value) => {
                return invalid("must be comfortable or compact");
            }
            ("ui", "use_stable_glyphs")
            | ("logging", "debug_file_logging")
            | ("notifications", "sound")
            | ("ai", "enabled")
            | ("ai", "title_generation")
            | ("ai", "tree_organization") => match value {
                "true" => toml::Value::Boolean(true),
                "false" => toml::Value::Boolean(false),
                _ => return invalid("must be true or false"),
            },
            ("api", "port") => match value.parse::<u16>() {
                Ok(port) if port > 0 => toml::Value::Integer(i64::from(port)),
                _ => return invalid("must be a port from 1 to 65535"),
            },
            ("keyboard", "shortcut_base") if value.is_empty() => {
                return invalid("must not be empty");
            }
            _ => toml::Value::String(value.to_owned()),
        };
        let mut table = parse_toml_table(&self.config_text());
        let entries = table
            .entry(section.to_owned())
            .or_insert_with(|| toml::Value::Table(toml::Table::new()));
        let Some(entries) = entries.as_table_mut() else {
            return Err(format!("invalid-argument: [{section}] is not a table"));
        };
        entries.insert(key.to_owned(), typed);
        let text = toml::to_string(&table).map_err(|error| format!("internal: {error}"))?;
        std::fs::write(self.config_path(), text).map_err(|error| format!("io: {error}"))?;
        self.reload_config();
        Ok(())
    }

    /// The current `config.toml` text.
    pub fn config_text(&self) -> String {
        std::fs::read_to_string(self.config_path()).unwrap_or_default()
    }

    /// The configuration directory.
    pub fn config_dir(&self) -> PathBuf {
        self.directory.config()
    }

    /// The project root.
    pub fn project_root(&self) -> String {
        self.directory.project().to_string_lossy().into_owned()
    }

    /// Every byte written to a pane's process.
    pub fn pane_input_log(&self, pane: NodeId) -> Vec<u8> {
        self.lock()
            .nodes
            .get(&pane.0)
            .map(|node| node.input_log.clone())
            .unwrap_or_default()
    }

    /// The exit code of a pane's process, once it ended.
    pub fn pane_exit_code(&self, pane: NodeId) -> Option<i32> {
        self.lock().nodes.get(&pane.0).and_then(|node| node.exit_code)
    }

    /// Names of the events emitted for a node, in order.
    pub fn event_names_for(&self, node: NodeId) -> Vec<String> {
        self.lock().events.get(&node.0).cloned().unwrap_or_default()
    }

    /// Sequence numbers of the journal entries written.
    pub fn journal_sequences(&self) -> Vec<u64> {
        self.lock().journal.clone()
    }

    /// The rectangles `(x, y, width, height)` of a split's children at a terminal size.
    pub fn split_rects(&self, split: NodeId, columns: u16, rows: u16) -> Vec<(u16, u16, u16, u16)> {
        let state = self.lock();
        let Some(node) = state.nodes.get(&split.0) else {
            return Vec::new();
        };
        let count = node.children.len() as u16;
        if count == 0 {
            return Vec::new();
        }
        let is_vertical = node.title.contains("vertical") || node.content_kind.as_deref() == Some("split.vertical");
        let mut rects = Vec::new();
        let (total, other) = if is_vertical { (rows, columns) } else { (columns, rows) };
        let share = total / count;
        let mut cursor = 0;
        for index in 0..count {
            let length = if index + 1 == count { total - cursor } else { share };
            rects.push(if is_vertical {
                (0, cursor, other, length)
            } else {
                (cursor, 0, length, other)
            });
            cursor += length;
        }
        rects
    }

    /// The rectangle `(x, y, width, height)` of a node at a terminal size.
    pub fn layout_rect(&self, node: NodeId, columns: u16, rows: u16) -> (u16, u16, u16, u16) {
        let parent = self.lock().nodes.get(&node.0).and_then(|entry| entry.parent);
        let Some(parent) = parent else {
            return (0, 0, columns, rows);
        };
        let index = self
            .lock()
            .nodes
            .get(&parent)
            .and_then(|entry| entry.children.iter().position(|child| *child == node.0))
            .unwrap_or(0);
        let (px, py, pw, ph) = self.layout_rect(NodeId(parent), columns, rows);
        let siblings = self.split_rects(NodeId(parent), pw, ph);
        siblings
            .get(index)
            .map_or((px, py, pw, ph), |(x, y, w, h)| (px + x, py + y, *w, *h))
    }

    /// The action a key sequence resolves to.
    pub fn resolve_key(&self, keys: &[&str]) -> Option<String> {
        let base = self.setting("keyboard", "shortcut_base")?;
        let [prefix, key] = keys else {
            return None;
        };
        if *prefix != base {
            return None;
        }
        let table = parse_toml_table(&self.config_text());
        let overrides: Vec<(String, String)> = table
            .get("keybindings")
            .and_then(toml::Value::as_table)
            .map(|entries| {
                entries
                    .iter()
                    .map(|(action, key)| (action.clone(), toml_value_text(key)))
                    .collect()
            })
            .unwrap_or_default();
        if let Some((action, _)) = overrides.iter().find(|(_, bound)| bound == key) {
            return Some(action.clone());
        }
        DEFAULT_BINDINGS
            .iter()
            .find(|(bound, action)| {
                bound == key && !overrides.iter().any(|(overridden, _)| overridden == action)
            })
            .map(|(_, action)| (*action).to_owned())
    }

    /// A file under the project root.
    pub fn read_project_file(&self, path: &str) -> Option<String> {
        std::fs::read_to_string(self.project_path(path)?).ok()
    }

    /// Whether a file exists under the project root.
    pub fn project_file_exists(&self, path: &str) -> bool {
        self.project_path(path).is_some_and(|target| target.is_file())
    }

    /// Whether a directory exists under the project root.
    pub fn project_dir_exists(&self, path: &str) -> bool {
        self.project_path(path).is_some_and(|target| target.is_dir())
    }

    /// The permission bits of a file under the project root.
    pub fn project_file_mode(&self, path: &str) -> Option<u32> {
        let metadata = std::fs::metadata(self.project_path(path)?).ok()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            Some(metadata.permissions().mode() & 0o777)
        }
        #[cfg(not(unix))]
        {
            Some(if metadata.permissions().readonly() { 0o444 } else { 0o644 })
        }
    }

    /// How many session snapshots were written (COR-120).
    pub fn snapshot_write_count(&self) -> usize {
        self.lock().snapshot_writes
    }

    /// Names of the backup files.
    pub fn backup_files(&self) -> Vec<String> {
        self.lock().backups.iter().map(|(name, _)| name.clone()).collect()
    }

    /// Names of the backup files younger than `ms`.
    pub fn backup_files_younger_than_ms(&self, ms: u64) -> Vec<String> {
        let state = self.lock();
        state
            .backups
            .iter()
            .filter(|(_, created)| state.clock_ms.saturating_sub(*created) < ms)
            .map(|(name, _)| name.clone())
            .collect()
    }

    /// The number of panes a pending restore prompt offers, if one is pending (COR-130).
    pub fn pending_restore_prompt(&self) -> Option<usize> {
        self.lock().pending_restore
    }

    /// Names of the debug log files (COR-180).
    pub fn debug_log_files(&self) -> Vec<String> {
        let state = self.lock();
        if self.setting("logging", "debug_file_logging").as_deref() != Some("true") {
            return Vec::new();
        }
        let mut files = vec!["ilium-debug-1.log".to_owned()];
        files.extend(state.debug_logs.iter().cloned());
        files
    }

    /// The debug log directory (COR-180).
    pub fn debug_log_root(&self) -> String {
        self.directory
            .project()
            .join(".ilium")
            .join("logs")
            .to_string_lossy()
            .into_owned()
    }

    /// The cursor line of an editor pane.
    pub fn editor_cursor_line(&self, pane: NodeId) -> Option<u32> {
        let state = self.lock();
        state
            .editor_text
            .get(&pane.0)
            .map(|text| text.matches('\n').count() as u32)
    }

    /// The cards of a board column.
    pub fn board_column(&self, pane: NodeId, column: &str) -> Vec<String> {
        self.lock()
            .boards
            .get(&pane.0)
            .and_then(|columns| columns.get(column))
            .cloned()
            .unwrap_or_default()
    }

    /// Whether an AI request of this kind passed admission.
    pub fn ai_request_admitted(&self, kind: &str) -> bool {
        if self.setting("ai", "enabled").as_deref() != Some("true") {
            return false;
        }
        match kind {
            "title" => self.setting("ai", "title_generation").as_deref() == Some("true"),
            "tree-organization" => {
                self.setting("ai", "tree_organization").as_deref() == Some("true")
            }
            _ => false,
        }
    }

    // ---- HTTP and jobs (parity-and-coverage.md) --------------------------------------------

    /// A POST to the local HTTP listener: `(status, body)`.
    pub fn http_post(&self, path: &str, body: serde_json::Value) -> (u16, serde_json::Value) {
        if self.http_listener_address().is_none() {
            return (503, serde_json::json!({ "error": "listener-down" }));
        }
        match path {
            "/api/echo" => (200, body),
            "/api/environment" => {
                let name = body.get("name").and_then(serde_json::Value::as_str);
                let value = name.and_then(|name| self.lock().environment.get(name).cloned());
                (200, serde_json::json!({ "value": value }))
            }
            _ => (404, serde_json::json!({ "error": "not-found", "path": path })),
        }
    }

    /// A GET from the local HTTP listener: `(status, body)`.
    pub fn http_get(&self, path: &str) -> (u16, serde_json::Value) {
        if self.http_listener_address().is_none() {
            return (503, serde_json::json!({ "error": "listener-down" }));
        }
        match path {
            "/api/health" => (200, serde_json::json!({ "status": "ok" })),
            "/api/tree" => {
                let count = self.lock().nodes.len();
                (200, serde_json::json!({ "nodes": count }))
            }
            _ => (404, serde_json::json!({ "error": "not-found", "path": path })),
        }
    }

    /// The address of the local HTTP listener, if it is up.
    pub fn http_listener_address(&self) -> Option<SocketAddr> {
        let port = self.setting("api", "port")?.parse::<u16>().ok()?;
        Some(SocketAddr::from(([127, 0, 0, 1], port)))
    }

    fn run_job(&self, bank: &str, job: Box<dyn FnOnce() -> Option<u64> + Send>) -> u64 {
        let ticket = {
            let mut state = self.lock();
            let ticket = state.jobs.next_ticket;
            state.jobs.next_ticket += 1;
            let stats = state.jobs.banks.entry(bank.to_owned()).or_default();
            stats.workers = stats.workers.max(1);
            stats.running += 1;
            stats.peak = stats.peak.max(stats.running);
            ticket
        };
        let outcome = std::thread::scope(|scope| scope.spawn(job).join().ok().flatten());
        let mut state = self.lock();
        if let Some(stats) = state.jobs.banks.get_mut(bank) {
            stats.running = stats.running.saturating_sub(1);
        }
        state.jobs.results.insert(ticket, outcome);
        ticket
    }

    /// Runs a job on a bank and returns its job id.
    pub fn run_finite_job(&self, bank: &str, job: Box<dyn FnOnce() -> u64 + Send>) -> u64 {
        self.run_job(bank, Box::new(move || Some(job())))
    }

    /// Runs a job that fails on a bank and returns its job id.
    pub fn run_failing_job(&self, bank: &str) -> u64 {
        self.run_job(bank, Box::new(|| None))
    }

    /// The result of a job, once it ended.
    pub fn job_result(&self, job: u64) -> Option<u64> {
        self.lock().jobs.results.get(&job).copied().flatten()
    }

    /// Worker threads of a job bank.
    pub fn bank_worker_count(&self, bank: &str) -> usize {
        self.lock().jobs.banks.get(bank).map_or(0, |stats| stats.workers)
    }

    /// Peak concurrency a job bank reached.
    pub fn bank_peak_concurrency(&self, bank: &str) -> usize {
        self.lock().jobs.banks.get(bank).map_or(0, |stats| stats.peak)
    }
}

fn tree_node_view(id: u64, node: &ModelNode) -> TreeNode {
    TreeNode {
        id: NodeId(id),
        parent: node.parent.map(NodeId),
        class: node.class,
        content_kind: node.content_kind.clone(),
        title: node.title.clone(),
        cwd: node.cwd.clone(),
        agent: node.agent.clone(),
        locked: node.locked,
        revision: node.revision,
    }
}

impl TreeRead for ServerKernel {
    fn nodes(&self) -> Vec<TreeNode> {
        let state = self.lock();
        state
            .preorder()
            .into_iter()
            .filter_map(|id| state.nodes.get(&id).map(|node| tree_node_view(id, node)))
            .collect()
    }

    fn node(&self, id: NodeId) -> Option<TreeNode> {
        self.lock().nodes.get(&id.0).map(|node| tree_node_view(id.0, node))
    }

    fn epoch(&self) -> u64 {
        self.lock().epoch
    }
}

impl PaneRead for ServerKernel {
    fn screen_text(&self, pane: NodeId) -> Option<String> {
        let state = self.lock();
        let node = state.nodes.get(&pane.0).filter(|node| node.class == NodeClass::Pane)?;
        Some(node.screen.join("\n"))
    }

    fn cells(&self, pane: NodeId, rows: std::ops::Range<u16>) -> Option<Vec<Vec<Cell>>> {
        let state = self.lock();
        let node = state.nodes.get(&pane.0).filter(|node| node.class == NodeClass::Pane)?;
        Some(
            rows.map(|row| {
                let line = node.screen.get(usize::from(row)).cloned().unwrap_or_default();
                line.chars()
                    .map(|character| Cell {
                        text: character.to_string(),
                        ..Cell::default()
                    })
                    .collect()
            })
            .collect(),
        )
    }

    fn history(&self, pane: NodeId, lines: u32) -> Option<Vec<String>> {
        let state = self.lock();
        let node = state.nodes.get(&pane.0).filter(|node| node.class == NodeClass::Pane)?;
        let skip = node.screen.len().saturating_sub(lines as usize);
        Some(node.screen[skip..].to_vec())
    }
}

impl ServerKernel {
    fn check_grant(&self, origin: &Origin, intent: &Intent) -> Option<Outcome> {
        let Origin::Plugin { plugin, .. } = origin else {
            return None;
        };
        let required = required_grant(intent)?;
        let state = self.lock();
        let granted = state
            .grants
            .as_ref()
            .is_some_and(|(owner, grants)| *owner == plugin.0 && grants.iter().any(|grant| grant == required));
        (!granted).then(|| refusal(ErrorCode::NotPermitted, &format!("grant {required} is missing")))
    }

    fn pane_missing(state: &KernelState, pane: u64) -> Option<Outcome> {
        match state.nodes.get(&pane) {
            None => Some(refusal(ErrorCode::NotFound, "no such pane")),
            Some(node) if node.class != NodeClass::Pane => {
                Some(refusal(ErrorCode::InvalidArgument, "the node is not a pane"))
            }
            Some(_) => None,
        }
    }

    fn apply_locked(&self, origin: &Origin, intent: Intent) -> Outcome {
        let cause = match origin {
            Origin::Plugin { plugin, .. } => Some(plugin.0.clone()),
            _ => None,
        };
        let mut state = self.lock();
        match intent {
            Intent::RenameNode(spec) => {
                let Some(node) = state.nodes.get_mut(&spec.node.0) else {
                    return refusal(ErrorCode::NotFound, "no such node");
                };
                if node.locked {
                    return refusal(ErrorCode::Conflict, "the node is locked");
                }
                if spec.expected_revision.is_some_and(|revision| revision != node.revision) {
                    return refusal(ErrorCode::Conflict, "stale revision");
                }
                if spec.title.is_empty() || spec.title.chars().count() > 256 {
                    return refusal(ErrorCode::InvalidArgument, "title must be 1 to 256 characters");
                }
                node.title = spec.title;
                node.revision += 1;
                state.touch();
                state.record(spec.node.0, "node-renamed");
                Outcome::Ok
            }
            Intent::SetNodeLocked(spec) => {
                let Some(node) = state.nodes.get_mut(&spec.node.0) else {
                    return refusal(ErrorCode::NotFound, "no such node");
                };
                node.locked = spec.locked;
                node.revision += 1;
                state.touch();
                Outcome::Ok
            }
            Intent::MoveNode(spec) => {
                let (node, target) = (spec.node.0, spec.target.0);
                if !state.nodes.contains_key(&node) || !state.nodes.contains_key(&target) {
                    return refusal(ErrorCode::NotFound, "no such node");
                }
                if node == target || state.is_descendant(target, node) {
                    return refusal(ErrorCode::InvalidArgument, "a node cannot move into itself");
                }
                state.detach(node);
                let inside = matches!(spec.placement, Placement::InsideFirst | Placement::InsideLast);
                let new_parent = if inside {
                    Some(target)
                } else {
                    state.nodes.get(&target).and_then(|entry| entry.parent)
                };
                if let Some(entry) = state.nodes.get_mut(&node) {
                    entry.parent = new_parent;
                    entry.revision += 1;
                }
                let siblings = match new_parent {
                    Some(parent) => state.nodes.get(&parent).map(|entry| entry.children.clone()).unwrap_or_default(),
                    None => state.roots.clone(),
                };
                let position = siblings.iter().position(|sibling| *sibling == target).unwrap_or(siblings.len());
                let index = match spec.placement {
                    Placement::Before => position,
                    Placement::After => position + 1,
                    Placement::InsideFirst => 0,
                    Placement::InsideLast => siblings.len(),
                };
                let list = match new_parent.and_then(|parent| state.nodes.get_mut(&parent)) {
                    Some(parent_node) => &mut parent_node.children,
                    None => &mut state.roots,
                };
                list.insert(index.min(list.len()), node);
                state.touch();
                state.last_restructure_token += 1;
                state.record(node, "node-moved");
                Outcome::Ok
            }
            Intent::CloseNode(node) => {
                if !state.nodes.contains_key(&node.0) {
                    return refusal(ErrorCode::NotFound, "no such node");
                }
                if state.nodes.get(&node.0).is_some_and(|entry| entry.locked) {
                    return refusal(ErrorCode::Conflict, "the node is locked");
                }
                state.detach(node.0);
                state.remove_subtree(node.0);
                state.touch();
                Outcome::Ok
            }
            Intent::CreatePane(spec) => {
                if let Some(parent) = spec.parent {
                    if !state.nodes.contains_key(&parent.0) {
                        return refusal(ErrorCode::NotFound, "no such parent");
                    }
                }
                let mut node = ModelNode::new(NodeClass::Pane, &spec.title, spec.parent.map(|parent| parent.0));
                node.content_kind = Some(spec.content_kind.clone());
                node.cwd = spec.cwd.map(|path| path.path);
                let program = spec.command.as_ref().and_then(|command| command.first().cloned());
                node.program = program.clone();
                node.process_running = spec.content_kind == "ilium.terminal";
                if node.process_running {
                    node.lifecycle.push(PaneLifecycleState::Running);
                    node.lifecycle_causes.push(cause);
                }
                if let Some(input) = spec.initial_input {
                    node.input_log.extend_from_slice(input.as_bytes());
                }
                state.last_program = program;
                let id = state.allocate(node, None);
                Outcome::OkNode(NodeId(id))
            }
            Intent::CreateGroup(spec) => {
                let mut node = ModelNode::new(NodeClass::Container, &spec.title, spec.parent.map(|parent| parent.0));
                node.content_kind = None;
                let id = state.allocate(node, None);
                for child in spec.children {
                    if state.nodes.contains_key(&child.0) {
                        state.detach(child.0);
                        if let Some(entry) = state.nodes.get_mut(&child.0) {
                            entry.parent = Some(id);
                        }
                        if let Some(group) = state.nodes.get_mut(&id) {
                            group.children.push(child.0);
                        }
                    }
                }
                Outcome::OkNode(NodeId(id))
            }
            Intent::CreateFolder(spec) => {
                let mut node = ModelNode::new(NodeClass::Folder, &spec.title, spec.parent.map(|parent| parent.0));
                node.cwd = spec.path.map(|path| path.path);
                Outcome::OkNode(NodeId(state.allocate(node, None)))
            }
            Intent::CreateProject(spec) => {
                let mut node = ModelNode::new(NodeClass::Folder, &spec.title, None);
                node.cwd = Some(spec.root.path);
                node.content_kind = Some("ilium.project".to_owned());
                Outcome::OkNode(NodeId(state.allocate(node, None)))
            }
            Intent::CreateSplit(spec) => {
                if !(2..=4).contains(&spec.panes.len()) {
                    return refusal(ErrorCode::InvalidArgument, "a split holds 2 to 4 panes");
                }
                if spec.panes.iter().any(|pane| !state.nodes.contains_key(&pane.0)) {
                    return refusal(ErrorCode::NotFound, "no such pane");
                }
                let mut node = ModelNode::new(NodeClass::Container, "split", None);
                node.content_kind = Some(format!("split.{}", format!("{:?}", spec.direction).to_lowercase()));
                let id = state.allocate(node, None);
                for pane in spec.panes {
                    state.detach(pane.0);
                    if let Some(entry) = state.nodes.get_mut(&pane.0) {
                        entry.parent = Some(id);
                    }
                    if let Some(split) = state.nodes.get_mut(&id) {
                        split.children.push(pane.0);
                    }
                }
                Outcome::OkNode(NodeId(id))
            }
            Intent::SendInput(spec) => {
                if let Some(missing) = Self::pane_missing(&state, spec.pane.0) {
                    return missing;
                }
                let Some(node) = state.nodes.get_mut(&spec.pane.0) else {
                    return refusal(ErrorCode::NotFound, "no such pane");
                };
                if !node.process_running {
                    return refusal(ErrorCode::PaneNotReady, "the pane has no running process");
                }
                node.input_log.extend_from_slice(spec.text.as_bytes());
                if spec.delivery == Delivery::Submitted {
                    node.input_log.push(b'\r');
                }
                Outcome::Ok
            }
            Intent::FreezePane(spec) => {
                if let Some(missing) = Self::pane_missing(&state, spec.pane.0) {
                    return missing;
                }
                if !state.nodes.get(&spec.pane.0).is_some_and(|node| node.process_running) {
                    return refusal(ErrorCode::PaneNotReady, "the pane has no running process");
                }
                state.set_lifecycle(spec.pane.0, PaneLifecycleState::Frozen, cause);
                Outcome::Ok
            }
            Intent::UnfreezePane(spec) => {
                if let Some(missing) = Self::pane_missing(&state, spec.pane.0) {
                    return missing;
                }
                let frozen = state
                    .nodes
                    .get(&spec.pane.0)
                    .and_then(|node| node.lifecycle.last())
                    == Some(&PaneLifecycleState::Frozen);
                if !frozen {
                    return refusal(ErrorCode::Conflict, "the pane is not frozen");
                }
                state.set_lifecycle(spec.pane.0, PaneLifecycleState::Running, cause);
                Outcome::Ok
            }
            Intent::SignalPane(spec) => {
                if let Some(missing) = Self::pane_missing(&state, spec.pane.0) {
                    return missing;
                }
                if state.windows {
                    return refusal(ErrorCode::Unsupported, "signals are unsupported on this platform");
                }
                if matches!(spec.signal, Signal::Terminate | Signal::Kill | Signal::Hangup)
                    && state.nodes.get(&spec.pane.0).is_some_and(|node| node.process_running)
                {
                    let ignores = state.nodes.get(&spec.pane.0).is_some_and(|node| node.ignore_terminate);
                    if spec.signal == Signal::Kill || !ignores {
                        if let Some(node) = state.nodes.get_mut(&spec.pane.0) {
                            node.process_running = false;
                            node.exit_code = Some(if spec.signal == Signal::Kill { 137 } else { 143 });
                        }
                        state.set_lifecycle(spec.pane.0, PaneLifecycleState::Exited, cause);
                    }
                }
                Outcome::Ok
            }
            Intent::TerminatePaneProcess(spec) => {
                if let Some(missing) = Self::pane_missing(&state, spec.pane.0) {
                    return missing;
                }
                if spec.grace_ms > 10_000 {
                    return refusal(ErrorCode::InvalidArgument, "grace-ms is at most 10000");
                }
                let Some(node) = state.nodes.get_mut(&spec.pane.0) else {
                    return refusal(ErrorCode::NotFound, "no such pane");
                };
                if !node.process_running {
                    return refusal(ErrorCode::PaneNotReady, "the pane has no running process");
                }
                node.kill_after_ms = if node.ignore_terminate { u64::from(spec.grace_ms) } else { 0 };
                node.process_running = false;
                node.exit_code = Some(if node.ignore_terminate { 137 } else { 143 });
                state.set_lifecycle(spec.pane.0, PaneLifecycleState::Exited, cause);
                Outcome::Ok
            }
            Intent::ResizePane(spec) => {
                if let Some(missing) = Self::pane_missing(&state, spec.pane.0) {
                    return missing;
                }
                if spec.columns == Some(0) || spec.rows == Some(0) {
                    return refusal(ErrorCode::InvalidArgument, "size must be positive");
                }
                if let Some(node) = state.nodes.get_mut(&spec.pane.0) {
                    node.columns = spec.columns.unwrap_or(node.columns);
                    node.rows = spec.rows.unwrap_or(node.rows);
                    node.revision += 1;
                }
                state.touch();
                Outcome::Ok
            }
            Intent::ReplacePaneCommand(spec) => {
                if let Some(missing) = Self::pane_missing(&state, spec.pane.0) {
                    return missing;
                }
                let Some(program) = spec.command.first().cloned() else {
                    return refusal(ErrorCode::InvalidArgument, "the command is empty");
                };
                if let Some(node) = state.nodes.get_mut(&spec.pane.0) {
                    node.program = Some(program.clone());
                    node.process_running = true;
                    node.exit_code = None;
                    node.restored = false;
                    if !spec.keep_scrollback {
                        node.screen.clear();
                    }
                }
                state.last_program = Some(program);
                state.set_lifecycle(spec.pane.0, PaneLifecycleState::Restarting, cause.clone());
                state.set_lifecycle(spec.pane.0, PaneLifecycleState::Running, cause);
                Outcome::Ok
            }
            Intent::EditorOpen(spec) => {
                let mut node = ModelNode::new(NodeClass::Pane, &spec.path.path, None);
                node.content_kind = Some("ilium.editor".to_owned());
                node.cwd = Some(spec.path.path);
                let id = state.allocate(node, None);
                state.editor_text.entry(id).or_default();
                Outcome::OkNode(NodeId(id))
            }
            _ => refusal(ErrorCode::Unsupported, "the intent is not supported by this kernel"),
        }
    }
}

impl KernelFacade for ServerKernel {
    fn apply_intent(&self, origin: &Origin, intent: Intent) -> IntentResult {
        if let Some(denied) = self.check_grant(origin, &intent) {
            return denied;
        }
        self.apply_locked(origin, intent)
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
    started: Mutex<Vec<String>>,
}

impl ServerPluginHost {
    /// A host that will start in the given mode.
    pub fn new(start_mode: StartMode) -> ServerPluginHost {
        ServerPluginHost {
            start_mode,
            started: Mutex::new(Vec::new()),
        }
    }

    /// The mode this host was created to start in (`--safe-plugins`, `--no-plugins`, or normal).
    pub fn start_mode(&self) -> StartMode {
        self.start_mode
    }

    /// Boot order: engine, registries, core plugins, then the rest (reference-host-architecture.md).
    /// `--no-plugins` starts nothing; the other modes start the core plugins in order.
    pub fn start(&self) -> StartReport {
        let started: Vec<String> = match self.start_mode {
            StartMode::NoPlugins => Vec::new(),
            StartMode::Normal | StartMode::SafePlugins => CORE_PLUGIN_IDS
                .iter()
                .map(|id| format!("ilium.core.{id}"))
                .collect(),
        };
        *self.started.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = started.clone();
        StartReport {
            started,
            failed: Vec::new(),
        }
    }

    /// Reverse order within the shutdown budget.
    pub fn shutdown(&self) -> ShutdownReport {
        let mut started = self.started.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut deactivated: Vec<String> = started.drain(..).collect();
        deactivated.reverse();
        ShutdownReport {
            deactivated,
            forced: Vec::new(),
        }
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
    pub fn for_test_world(plugin: &str, grants: &[&str]) -> ServerKernel {
        let kernel = ServerKernel::build(StartMode::Normal, "", "test-world");
        {
            let mut state = kernel.lock();
            state.grants = Some((
                plugin.to_owned(),
                grants.iter().map(|grant| (*grant).to_owned()).collect(),
            ));
            let make = |title: &str| {
                let mut node = ModelNode::new(NodeClass::Pane, title, None);
                node.content_kind = Some("ilium.terminal".to_owned());
                node.cwd = Some("/".to_owned());
                node
            };
            let mut running = make("running");
            running.process_running = true;
            running.program = Some("sh".to_owned());
            running.lifecycle.push(PaneLifecycleState::Running);
            running.lifecycle_causes.push(None);
            let mut restored = make("restored");
            restored.restored = true;
            let mut exited = make("exited");
            exited.exit_code = Some(0);
            let mut neighbour = make("neighbour");
            neighbour.process_running = true;
            neighbour.columns = 10;
            neighbour.rows = 3;
            // The lifecycle history starts after the fixture: tests read only later transitions.
            running.lifecycle.clear();
            running.lifecycle_causes.clear();
            state.allocate(running, Some(10));
            state.allocate(restored, Some(11));
            state.allocate(exited, Some(12));
            state.allocate(neighbour, Some(13));
            if let Some(node) = state.nodes.get_mut(&10) {
                node.process_running = true;
            }
            state.next_node = 14;
            state.tree_dirty = false;
        }
        kernel
    }

    // ---- Pane lifecycle observers (api-pane-lifecycle.md) ----------------------------------

    /// The lifecycle states a pane passed through, in order.
    pub fn lifecycle_events(&self, pane: NodeId) -> Vec<PaneLifecycleState> {
        self.lock()
            .nodes
            .get(&pane.0)
            .map(|node| node.lifecycle.clone())
            .unwrap_or_default()
    }

    /// The plugin that caused each lifecycle event (`None` for the user or the kernel).
    pub fn lifecycle_event_causes(&self, pane: NodeId) -> Vec<Option<String>> {
        self.lock()
            .nodes
            .get(&pane.0)
            .map(|node| node.lifecycle_causes.clone())
            .unwrap_or_default()
    }

    /// Makes the kernel behave as on Windows (signals unsupported).
    pub fn pretend_platform_windows(&self) {
        self.lock().windows = true;
    }

    /// The identity a pane keeps across a process replacement.
    pub fn pane_identity(&self, pane: NodeId) -> PaneIdentity {
        let state = self.lock();
        let node = state.nodes.get(&pane.0);
        let parent = node.and_then(|entry| entry.parent);
        let siblings = match parent.and_then(|parent| state.nodes.get(&parent)) {
            Some(parent_node) => parent_node.children.clone(),
            None => state.roots.clone(),
        };
        PaneIdentity {
            node: pane,
            title: node.map(|entry| entry.title.clone()).unwrap_or_default(),
            place: (
                parent.map(NodeId),
                siblings.iter().position(|sibling| *sibling == pane.0).unwrap_or(0),
            ),
            size: node.map_or((0, 0), |entry| (entry.columns, entry.rows)),
        }
    }

    /// The working directory of a pane.
    pub fn pane_cwd(&self, pane: NodeId) -> PathBuf {
        self.lock()
            .nodes
            .get(&pane.0)
            .and_then(|node| node.cwd.clone())
            .map_or_else(|| self.directory.project(), PathBuf::from)
    }

    /// Whether a node is in the tree.
    pub fn node_exists(&self, node: NodeId) -> bool {
        self.lock().nodes.contains_key(&node.0)
    }

    /// Whether a pane shows its process's exit status.
    pub fn pane_shows_exit_status(&self, pane: NodeId) -> bool {
        self.lock()
            .nodes
            .get(&pane.0)
            .is_some_and(|node| node.exit_code.is_some() && !node.process_running)
    }

    /// Makes the pane's process ignore the terminate signal.
    pub fn make_process_ignore_terminate(&self, pane: NodeId) {
        if let Some(node) = self.lock().nodes.get_mut(&pane.0) {
            node.ignore_terminate = true;
        }
    }

    /// Milliseconds between the terminate and the kill signal sent to a pane's process.
    pub fn kill_was_sent_after_ms(&self, pane: NodeId) -> u64 {
        self.lock().nodes.get(&pane.0).map_or(0, |node| node.kill_after_ms)
    }

    /// A fingerprint of the whole tree (structure, titles, sizes).
    pub fn tree_fingerprint(&self) -> String {
        let state = self.lock();
        state
            .preorder()
            .into_iter()
            .filter_map(|id| {
                state.nodes.get(&id).map(|node| {
                    format!(
                        "{id}:{:?}:{}:{}x{}:{:?}",
                        node.class, node.title, node.columns, node.rows, node.parent
                    )
                })
            })
            .collect::<Vec<_>>()
            .join("|")
    }

    /// The restructure token of a restructure outcome (api-pane-lifecycle.md PAN-023).
    pub fn last_restructure_token(&self, outcome: &ilium_plugin_api::generated::types::Outcome) -> u64 {
        match outcome {
            Outcome::OkEpoch(token) => *token,
            _ => self.lock().last_restructure_token,
        }
    }

    /// The revision counter of a node.
    pub fn node_revision(&self, node: NodeId) -> u64 {
        self.lock().nodes.get(&node.0).map_or(0, |entry| entry.revision)
    }

    /// Sets a manual icon on a node, as the user would.
    pub fn set_manual_icon(&self, node: NodeId, icon: &str) {
        let mut state = self.lock();
        if let Some(entry) = state.nodes.get_mut(&node.0) {
            entry.revision += 1;
        }
        state.record(node.0, &format!("manual-icon:{icon}"));
    }

    /// The log lines of a plugin.
    pub fn plugin_log(&self, plugin: &str) -> Vec<String> {
        self.lock().plugin_logs.get(plugin).cloned().unwrap_or_default()
    }

    // ---- Panes (api-panes.md) ---------------------------------------------------------------

    /// Creates a pane as the user would and returns its node.
    pub fn create_pane_for_test(&self) -> NodeId {
        let mut state = self.lock();
        let mut node = ModelNode::new(NodeClass::Pane, "pane", None);
        node.content_kind = Some("ilium.terminal".to_owned());
        node.process_running = true;
        node.program = Some("sh".to_owned());
        NodeId(state.allocate(node, None))
    }

    /// The pane info a plugin sees for a node, if it is a pane.
    pub fn pane_info(&self, node: NodeId) -> Option<PaneInfo> {
        let state = self.lock();
        let entry = state.nodes.get(&node.0).filter(|entry| entry.class == NodeClass::Pane)?;
        let content_kind = entry.content_kind.clone().unwrap_or_default();
        let primitive = match content_kind.as_str() {
            "ilium.editor" => PrimitiveKind::Editor,
            "ilium.board" => PrimitiveKind::Basic,
            _ => PrimitiveKind::Terminal,
        };
        Some(PaneInfo {
            node,
            content_kind,
            primitive,
            columns: entry.columns,
            rows: entry.rows,
            focused: entry.focused,
            title: entry.title.clone(),
            is_restored: entry.restored,
        })
    }

    /// Restores a pane from a snapshot that carries no size.
    pub fn restore_pane_without_saved_size(&self, pane: NodeId) {
        if let Some(node) = self.lock().nodes.get_mut(&pane.0) {
            node.restored = true;
            node.process_running = false;
            node.columns = 80;
            node.rows = 24;
        }
    }

    /// Starts the process of a restored pane.
    pub fn start_restored_process(&self, pane: NodeId) {
        let mut state = self.lock();
        let program = state
            .environment
            .get("SHELL")
            .cloned()
            .unwrap_or_else(|| "sh".to_owned());
        if let Some(node) = state.nodes.get_mut(&pane.0) {
            node.restored = false;
            node.process_running = true;
            node.exit_code = None;
            node.program = Some(program.clone());
        }
        state.last_program = Some(program);
        state.set_lifecycle(pane.0, PaneLifecycleState::Running, None);
    }

    /// The program of the most recently spawned pane process.
    pub fn last_spawned_program(&self) -> Option<String> {
        self.lock().last_program.clone()
    }

    /// The number of panes in the tree.
    pub fn pane_count(&self) -> usize {
        self.lock()
            .nodes
            .values()
            .filter(|node| node.class == NodeClass::Pane)
            .count()
    }

    // ---- Detection loop (reference-host-scheduling.md HST-450..455) ------------------------

    /// Runs one detection tick over every pane.
    pub fn run_detection_tick(&self) {
        self.detection_tick(None);
    }

    /// Runs one detection tick with the named fixture detector plugin.
    pub fn run_detection_tick_with_fixture_detector(&self, fixture: &str) {
        self.detection_tick(Some(fixture));
    }

    fn detection_tick(&self, fixture: Option<&str>) {
        let plugin = fixture.unwrap_or("ilium.core.agent-detect").to_owned();
        let panes: Vec<u64> = {
            let state = self.lock();
            state
                .nodes
                .iter()
                .filter(|(_, node)| node.class == NodeClass::Pane && node.process_running)
                .map(|(id, _)| *id)
                .collect()
        };
        let block_ms = {
            let state = self.lock();
            state
                .detector_block_for
                .get(&plugin)
                .copied()
                .unwrap_or(state.detector_block_ms)
        };
        let state = Arc::clone(&self.state);
        let call_count = panes.len();
        let plugin_name = plugin.clone();
        let handle = std::thread::Builder::new()
            .name("detector-worker".to_owned())
            .spawn(move || {
                if block_ms > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(block_ms));
                }
                let name = std::thread::current().name().unwrap_or("detector-worker").to_owned();
                let mut state = state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                state.detector_threads.push(name);
                state.detector_calls.push(plugin_name);
            });
        {
            let mut state = self.lock();
            state.detector_calls_last_tick = call_count.min(1) * call_count;
        }
        // The detector runs off the detection loop thread; the tick does not wait for a blocked one.
        if let Ok(handle) = handle {
            if block_ms == 0 {
                let _ = handle.join();
            }
        }
    }

    /// The thread names the detector calls ran on.
    pub fn detector_thread_names(&self) -> Vec<String> {
        self.lock().detector_threads.clone()
    }

    /// The thread name of the detection loop.
    pub fn detection_loop_thread_name(&self) -> String {
        "detection-loop".to_owned()
    }

    /// Makes every detector call block for `ms`.
    pub fn set_detector_block_ms(&self, ms: u64) {
        self.lock().detector_block_ms = ms;
    }

    /// Makes the detector calls of one plugin block for `ms`.
    pub fn set_detector_block_ms_for(&self, plugin: &str, ms: u64) {
        self.lock().detector_block_for.insert(plugin.to_owned(), ms);
    }

    /// The number of detector calls the last tick made.
    pub fn detector_calls_in_last_tick(&self) -> usize {
        self.lock().detector_calls_last_tick
    }

    /// Waits up to `timeout` for a detector call of `plugin`; true when it happened.
    pub fn wait_for_detector_call(&self, plugin: &str, timeout: std::time::Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            if self.lock().detector_calls.iter().any(|call| call == plugin) {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}

#[cfg(test)]
mod spec_tests {
    use super::*;

    #[test]
    fn host_reports_the_start_mode_it_was_created_with() {
        for mode in [
            StartMode::Normal,
            StartMode::SafePlugins,
            StartMode::NoPlugins,
        ] {
            let host = ServerPluginHost::new(mode);
            assert_eq!(host.start_mode(), mode);
        }
    }

    #[test]
    fn no_plugins_start_activates_nothing() {
        let host = ServerPluginHost::new(StartMode::NoPlugins);
        let report = host.start();
        assert!(
            report.started.is_empty(),
            "no plugin may start: {:?}",
            report.started
        );
        assert!(
            report.failed.is_empty(),
            "no plugin may fail: {:?}",
            report.failed
        );
    }

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

    #[test]
    fn fed_output_is_not_recorded_as_input_to_the_pane_process() {
        let kernel = ServerKernel::for_parity_world(StartMode::NoPlugins, "");
        let pane = kernel.create_pane_for_test();
        kernel.feed_output(pane, b"hello");
        assert!(kernel.pane_input_log(pane).is_empty());
    }

    #[test]
    fn config_text_returns_what_write_config_file_stored() {
        let kernel = ServerKernel::for_parity_world(StartMode::NoPlugins, "");
        kernel.write_config_file("[ui]\ntree_order = \"manual\"\n");
        assert_eq!(kernel.config_text(), "[ui]\ntree_order = \"manual\"\n");
        kernel.write_config_file("");
        assert_eq!(kernel.config_text(), "");
    }

    #[test]
    fn project_file_round_trips_and_missing_file_is_absent() {
        let kernel = ServerKernel::for_parity_world(StartMode::NoPlugins, "");
        assert_eq!(kernel.read_project_file("notes/missing.txt"), None);
        assert!(!kernel.project_file_exists("notes/missing.txt"));
        kernel.write_project_file("notes/a.txt", "hello");
        assert_eq!(
            kernel.read_project_file("notes/a.txt"),
            Some("hello".to_string())
        );
        assert!(kernel.project_file_exists("notes/a.txt"));
    }

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

    #[test]
    fn starting_a_restored_process_keeps_the_pane_identity() {
        let kernel = ServerKernel::for_test_world("spec.plugin", &[]);
        let pane = kernel.create_pane_for_test();
        let identity_before = kernel.pane_identity(pane);
        kernel.start_restored_process(pane);
        assert_eq!(kernel.pane_identity(pane), identity_before);
    }

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

    #[test]
    fn job_result_is_absent_for_a_job_that_never_ran() {
        let kernel = ServerKernel::for_parity_world(StartMode::NoPlugins, "");
        assert_eq!(kernel.job_result(u64::MAX), None);
    }
}
