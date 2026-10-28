//! Test-support rig that draws the user-admin screens (Plugins tab, Capabilities tab, Extensions
//! tab, recovery page, install dialogs, footer and notices) into a screen buffer and feeds keys
//! back. State-driven model of the screens; spec in `tests/plugin_screens/API_GAPS.md`.

/// Requirement family USR-014..USR-214: the state shown in a Plugins tab row.
#[derive(Clone, Debug, PartialEq)]
pub enum RowState {
    /// Running normally.
    On,
    /// Running but slow.
    OnSlow,
    /// Being restarted.
    Restarting,
    /// Stopped after failures.
    Failed,
    /// Switched off.
    Off,
    /// Waiting for the named owner plugin.
    WaitingFor(String),
    /// Blocked by system policy.
    BlockedByPolicy,
    /// Refused to load, with the reason.
    Refused(String),
}

use std::collections::BTreeMap;

/// The first-start prompt asks for these bundled plugins until it is answered or declined three times.
const FIRST_START_LIMIT: u32 = 3;
/// How long a clipboard footer indicator stays (USR-087).
const CLIPBOARD_INDICATOR_MS: u64 = 3_000;
/// How long a plugin input lock lasts at most (USR-088).
const INPUT_LOCK_MS: u64 = 2_000;
/// How long a toast stays on screen.
const TOAST_LIFETIME_MS: u64 = 5_000;
/// Toasts per plugin inside the rate window (USR-088).
const TOASTS_PER_WINDOW: usize = 3;
/// The rate window of the toast limit.
const TOAST_WINDOW_MS: u64 = 10_000;
/// Failures that stop a plugin or an extension (USR-053, USR-130).
const FAILURE_LIMIT: u32 = 3;
/// The danger colour of the client theme, used for stop notices (USR-130).
const THEME_DANGER: ratatui::style::Color = ratatui::style::Color::Red;
/// The safe-mode banner of every screen (USR-031).
const SAFE_MODE_BANNER: &str = "Safe mode: third-party plugins are off.";
/// Text of the administrator lock (USR-214).
const ADMIN_LOCK_TEXT: &str = "Blocked by your administrator";

/// The thirty bundled core plugins of a fresh profile (USR-162).
const CORE_PLUGINS: [&str; 30] = [
    "ilium.core.tree",
    "ilium.core.titles",
    "ilium.core.settings",
    "ilium.core.notifications",
    "ilium.core.smart-copy",
    "ilium.core.agent-setup",
    "ilium.core.cost",
    "ilium.core.sessions",
    "ilium.core.worktrees",
    "ilium.core.layout",
    "ilium.core.terminal",
    "ilium.core.fleet",
    "ilium.core.diagnostics",
    "ilium.core.inference",
    "ilium.core.editor",
    "ilium.core.chatroom",
    "ilium.core.text-triggers",
    "ilium.core.scheduler",
    "ilium.core.ambient",
    "ilium.core.keys-and-help",
    "ilium.core.kanban",
    "ilium.core.search",
    "ilium.core.progress",
    "ilium.core.session-convert",
    "ilium.core.recap",
    "ilium.core.agent-detect",
    "ilium.core.scenes",
    "ilium.core.voice",
    "ilium.core.compaction",
    "ilium.core.agent-toolbar",
];

/// Core plugins that the Plugins tab never switches off (USR-041).
const PROTECTED_CORE: [&str; 4] = [
    "ilium.core.tree",
    "ilium.core.terminal",
    "ilium.core.layout",
    "ilium.core.keys-and-help",
];

/// Plain wording and risk of each permission name (user-permissions-explained.md).
const GRANT_TABLE: [(&str, &str, bool); 61] = [
    (
        "tree.read",
        "See the groups, panes and their names in your project tree",
        false,
    ),
    (
        "tree.write",
        "Add, rename or move items you did not ask it to touch",
        false,
    ),
    ("tree.metadata", "Attach its own notes to tree items", false),
    ("tree.decorate", "Add badges and colors to tree rows", false),
    (
        "tree.restructure",
        "Reorganize the tree on a large scale",
        true,
    ),
    (
        "tree.attention",
        "Mark items as needing your attention",
        false,
    ),
    ("panes.read", "See pane sizes, titles and focus", false),
    ("panes.create", "Open new panes", false),
    (
        "panes.signal",
        "Send signals to the programs in panes",
        true,
    ),
    ("panes.lifecycle", "Close, restart or kill panes", true),
    (
        "panes.provide-kind",
        "Offer its own kind of pane, like a notes pane",
        false,
    ),
    (
        "terminal.read-screen",
        "Read the text you see in a terminal pane",
        false,
    ),
    (
        "terminal.read-cells",
        "Read the text with its colors and styles",
        false,
    ),
    (
        "terminal.history",
        "Read scrollback, the older lines that scrolled out of view",
        true,
    ),
    (
        "terminal.output-filter",
        "Change terminal output before you see it, such as hiding noise",
        false,
    ),
    ("terminal.write", "Type into a terminal as if you did", true),
    (
        "terminal.input-lock",
        "Block your keyboard for a moment while it acts",
        true,
    ),
    (
        "terminal.raw-input",
        "Receive every key you press in a terminal",
        true,
    ),
    (
        "terminal.pointer",
        "See mouse movement over a terminal",
        false,
    ),
    (
        "agent.read",
        "See which coding agents run, and whether each is working or idle",
        false,
    ),
    (
        "agent.read-prompts",
        "Read what you typed to an agent",
        true,
    ),
    ("agent.control", "Send commands to an agent", true),
    (
        "agent.signatures",
        "Add rules that tell Ilium how to recognize an agent",
        false,
    ),
    (
        "agent.transcript",
        "Read the saved conversation of an agent",
        true,
    ),
    ("editor.read", "Read the text of open editor panes", false),
    (
        "editor.edit",
        "Change the text of open editor panes",
        false,
    ),
    (
        "editor.provide",
        "Offer its own editor, for a new file type",
        true,
    ),
    (
        "ui.panels",
        "Show its own panels and settings pages",
        false,
    ),
    (
        "ui.chrome",
        "Add items to menus, toolbars, status bar and tree rows",
        false,
    ),
    (
        "keys.intercept",
        "See your keys before Ilium does and swallow some",
        true,
    ),
    ("keymap.read", "Read your key bindings", false),
    ("commands.invoke", "Run Ilium commands", false),
    ("fs.read", "Read files in one named place", false),
    ("fs.write", "Create and change files in one named place", true),
    (
        "fs.watch",
        "Be told when files in a place change",
        false,
    ),
    (
        "fs.pick",
        "Ask you to choose a file or folder, then read that one only",
        false,
    ),
    (
        "agent-config.propose",
        "Suggest changes to one agent's settings files",
        false,
    ),
    ("http", "Talk to one named website or server", false),
    ("jobs.run", "Run work in the background", false),
    (
        "process.run",
        "Run one named program. Never a wildcard",
        true,
    ),
    (
        "process.long-lived",
        "Keep a started program running in the background",
        true,
    ),
    (
        "inference",
        "Use your configured AI providers to answer a prompt",
        false,
    ),
    ("inference.provide", "Offer itself as an AI provider", false),
    (
        "inference.embed",
        "Turn text into embeddings with your AI providers",
        false,
    ),
    ("services.use", "Call a named service of another plugin", false),
    (
        "services.provide",
        "Offer a named service to other plugins",
        false,
    ),
    (
        "messages.peer",
        "Exchange messages with one named other plugin",
        false,
    ),
    (
        "topics.subscribe",
        "Listen to a named stream of events",
        false,
    ),
    (
        "journal.read",
        "Read the host's record of recent events",
        false,
    ),
    (
        "extensions.provide",
        "Offer its own extension point",
        false,
    ),
    (
        "extensions.contribute",
        "Fill another plugin's extension point",
        false,
    ),
    ("sound.play", "Play sounds", false),
    ("notify.desktop", "Send desktop notifications", false),
    ("notify.toast", "Show small messages inside Ilium", false),
    (
        "open-external",
        "Ask to open a link; you confirm each site",
        false,
    ),
    ("clipboard.read", "Read what you copied", true),
    ("clipboard.write", "Put text on the clipboard", false),
    ("audio.capture", "Use the microphone", true),
    (
        "audio.loopback",
        "Record the sound your computer plays",
        true,
    ),
    ("audio.playback", "Play audio streams", false),
    ("location.observer", "Know your approximate location for scenes", true),
];

/// Plain wording and risk of a permission string such as `http:api.example.com`.
fn grant_info(grant: &str) -> (String, bool) {
    let base = grant.split(':').next().unwrap_or(grant);
    let argument = grant.split_once(':').map(|(_, rest)| rest);
    let bare_star_is_high = matches!(base, "commands.invoke" | "topics.subscribe") && argument == Some("*");
    match GRANT_TABLE.iter().find(|(name, _, _)| *name == base) {
        Some((_, wording, high)) => ((*wording).to_string(), *high || bare_star_is_high),
        None => (format!("Use the {base} permission"), false),
    }
}

/// Splits `text` at whitespace into lines of at most `columns` characters; a word longer than a
/// line is broken hard so that nothing is cut off.
fn wrap_text(text: &str, columns: usize) -> Vec<String> {
    let columns = columns.max(1);
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let mut rest: Vec<char> = word.chars().collect();
        while rest.len() > columns {
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
            let tail = rest.split_off(columns);
            lines.push(rest.into_iter().collect());
            rest = tail;
        }
        let word: String = rest.into_iter().collect();
        if current.is_empty() {
            current = word;
        } else if current.chars().count() + 1 + word.chars().count() <= columns {
            current.push(' ');
            current.push_str(&word);
        } else {
            lines.push(std::mem::take(&mut current));
            current = word;
        }
    }
    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }
    lines
}

/// A fingerprint without the grouping spaces.
fn compact_fingerprint(fingerprint: &str) -> String {
    fingerprint.split_whitespace().collect()
}

/// The first `count` characters of `text`.
fn head_chars(text: &str, count: usize) -> String {
    text.chars().take(count).collect()
}

/// The last `count` characters of `text`.
fn tail_chars(text: &str, count: usize) -> String {
    let length = text.chars().count();
    text.chars().skip(length.saturating_sub(count)).collect()
}

/// One drawn line; `wrap` marks prose that may be re-flowed to the screen width.
#[derive(Clone, Debug)]
struct DrawnLine {
    text: String,
    wrap: bool,
}

/// A plugin as the Plugins tab knows it.
#[derive(Clone, Debug)]
struct Plugin {
    id: String,
    name: String,
    tier: String,
    core: bool,
    state: RowState,
    failures: u32,
    stop_notice: Option<String>,
    grants: BTreeMap<String, String>,
    settings: BTreeMap<String, String>,
    project_settings: BTreeMap<String, String>,
    trap_reason: Option<String>,
}

impl Plugin {
    fn new(id: &str, name: &str, tier: &str, core: bool) -> Plugin {
        Plugin {
            id: id.to_string(),
            name: name.to_string(),
            tier: tier.to_string(),
            core,
            state: RowState::On,
            failures: 0,
            stop_notice: None,
            grants: BTreeMap::new(),
            settings: BTreeMap::new(),
            project_settings: BTreeMap::new(),
            trap_reason: None,
        }
    }

    fn is_protected(&self) -> bool {
        self.core && PROTECTED_CORE.contains(&self.id.as_str())
    }
}

/// An extension contribution of one plugin to one extension point (USR-050).
#[derive(Clone, Debug)]
struct Contribution {
    plugin: String,
    owner: String,
    point: String,
    hook: String,
    enabled: bool,
    breaches: u32,
}

impl Contribution {
    fn is_stopped(&self) -> bool {
        self.breaches >= FAILURE_LIMIT
    }
}

/// The bundled-plugin first-start prompt state (USR-014).
#[derive(Clone, Debug)]
struct FirstStart {
    plugin: String,
    grants: Vec<String>,
    not_now: u32,
    allowed: bool,
}

/// One row of a review or first-start dialog.
#[derive(Clone, Debug)]
struct ReviewRow {
    grant: String,
    checked: bool,
    locked: bool,
}

/// What a confirmation question will do on Yes.
#[derive(Clone, Debug)]
enum ConfirmKind {
    ForgetSettings,
    Uninstall,
}

/// A modal dialog.
#[derive(Clone, Debug)]
enum Dialog {
    FirstStart {
        plugin: String,
        rows: Vec<ReviewRow>,
    },
    Review {
        plugin: String,
        rows: Vec<ReviewRow>,
    },
    Trust {
        publisher: String,
        fingerprint: String,
    },
    KeyChange {
        plugin: String,
        old: String,
        new: String,
    },
    Replace {
        plugin: String,
        replaced: String,
    },
    SupportBundle {
        plugin: String,
    },
    Confirm {
        kind: ConfirmKind,
        plugin: String,
    },
}

/// The full screen the user looks at below any dialog.
#[derive(Clone, Debug)]
enum View {
    Tree,
    Settings,
    PluginsTab,
    Details(String),
    Capabilities(String),
    Recovery,
    ExtensionsOwner(String),
    ExtensionsContributor(String),
}

/// A selectable row of the current view or dialog.
#[derive(Clone, Debug)]
struct Item {
    key: String,
    label: String,
}

/// A toast alive on screen.
#[derive(Clone, Debug)]
struct Toast {
    plugin: String,
    text: String,
    until: u64,
}

/// Requirement family USR-014..USR-214: the client screens test rig.
///
/// The rig keeps the state of the user-admin screens (plugin rows, grants, extension orders,
/// dialogs, footer indicators) in memory, draws every screen as text and feeds keys into the same
/// state. It draws the whole screen content without clipping to the row count, so a test can read
/// a long list in one call.
#[derive(Debug)]
pub struct ScreensRig {
    columns: u16,
    rows: u16,
    now_ms: u64,
    started: bool,
    safe_mode: bool,
    screen_reader: bool,
    view: View,
    dialog: Option<Dialog>,
    selection: usize,
    plugins: Vec<Plugin>,
    contributions: Vec<Contribution>,
    extension_points: Vec<(String, String)>,
    extension_orders: BTreeMap<(String, String), Vec<String>>,
    priority_table: BTreeMap<(String, String, String), i32>,
    first_starts: Vec<FirstStart>,
    policy_denied: Vec<String>,
    policy_file: Option<(String, u32)>,
    policy_notice: Option<String>,
    policy_blocks_third_party: bool,
    pending_failures: Vec<(u64, String)>,
    last_notice: Option<String>,
    clipboard_indicator: Option<(String, u64)>,
    toasts: Vec<Toast>,
    toast_times: BTreeMap<String, Vec<u64>>,
    input_locked_until: Option<u64>,
    delivered_keys: Vec<char>,
    dropped_keys: Vec<char>,
    trust_answers: Vec<bool>,
    replace_answers: Vec<bool>,
    editor_paths: Vec<String>,
    safe_mode_restart_requested: bool,
    install_error: Option<String>,
}

impl ScreensRig {
    /// USR-014..USR-214: a rig with a screen of the given size.
    pub fn new(columns: u16, rows: u16) -> ScreensRig {
        let mut plugins: Vec<Plugin> = CORE_PLUGINS
            .iter()
            .map(|id| Plugin::new(id, id, "T1", true))
            .collect();
        for plugin in &mut plugins {
            plugin.state = RowState::On;
        }
        ScreensRig {
            columns,
            rows,
            now_ms: 0,
            started: true,
            safe_mode: false,
            screen_reader: false,
            view: View::Tree,
            dialog: None,
            selection: 0,
            plugins,
            contributions: Vec::new(),
            extension_points: Vec::new(),
            extension_orders: BTreeMap::new(),
            priority_table: BTreeMap::new(),
            first_starts: vec![FirstStart {
                plugin: "ilium.core.voice".to_string(),
                grants: vec!["audio.capture".to_string()],
                not_now: 0,
                allowed: false,
            }],
            policy_denied: Vec::new(),
            policy_file: None,
            policy_notice: None,
            policy_blocks_third_party: false,
            pending_failures: Vec::new(),
            last_notice: None,
            clipboard_indicator: None,
            toasts: Vec::new(),
            toast_times: BTreeMap::new(),
            input_locked_until: None,
            delivered_keys: Vec::new(),
            dropped_keys: Vec::new(),
            trust_answers: Vec::new(),
            replace_answers: Vec::new(),
            editor_paths: Vec::new(),
            safe_mode_restart_requested: false,
            install_error: None,
        }
    }

    // ---- screen reads ----

    /// USR-014..USR-214 screen reads.
    pub fn screen_text(&self) -> String {
        self.draw(true)
    }

    /// USR-014..USR-214 screen reads: the content with every row at its full length, as a
    /// horizontally scrolled view would show it.
    pub fn scrolled_text(&mut self) -> String {
        self.draw(false)
    }

    /// USR-014..USR-214 screen reads.
    pub fn footer_text(&self) -> String {
        self.flatten(self.footer_lines(), true)
    }

    /// USR-014..USR-214 screen reads.
    pub fn footer_notice_foreground(&self) -> Option<String> {
        let has_stop_notice = self
            .plugins
            .iter()
            .any(|plugin| plugin.stop_notice.is_some());
        has_stop_notice.then(|| self.theme_danger_colour())
    }

    /// USR-014..USR-214 screen reads.
    pub fn theme_danger_colour(&self) -> String {
        format!("{THEME_DANGER:?}")
    }

    /// USR-014..USR-214 screen reads.
    pub fn last_notice(&self) -> Option<String> {
        self.last_notice.clone()
    }

    /// USR-014..USR-214 time: moves the rig clock and runs what falls due.
    pub fn advance_ms(&mut self, ms: u64) {
        let target = self.now_ms.saturating_add(ms);
        loop {
            let next = self
                .pending_failures
                .iter()
                .enumerate()
                .filter(|(_, (due, _))| *due <= target)
                .min_by_key(|(_, (due, _))| *due)
                .map(|(index, (due, id))| (index, *due, id.clone()));
            let Some((index, due, plugin_id)) = next else {
                break;
            };
            self.pending_failures.remove(index);
            self.now_ms = self.now_ms.max(due);
            self.apply_failure(&plugin_id);
        }
        self.now_ms = target;
    }

    // ---- input ----

    /// USR-014..USR-214 input.
    pub fn press_key(&mut self, key: char) {
        self.last_notice = None;
        if self.dialog.is_some() {
            self.dialog_key(key);
            return;
        }
        match self.view {
            View::Recovery => self.recovery_key(key),
            View::Tree => self.pane_key(key),
            _ => {}
        }
    }

    /// USR-014..USR-214 input.
    pub fn press_key_named(&mut self, name: &str) {
        match name {
            "Up" | "Down" => {
                self.last_notice = None;
                let up = name == "Up";
                if self.dialog.is_none() && matches!(self.view, View::ExtensionsOwner(_)) {
                    self.move_extension_row(up);
                } else {
                    self.move_selection(up);
                }
            }
            "Enter" => self.press_enter(),
            "Space" => self.press_space(),
            "Escape" | "Esc" => self.press_escape(),
            _ => {}
        }
    }

    /// USR-014..USR-214 input.
    pub fn press_space(&mut self) {
        self.last_notice = None;
        if let Some(Dialog::Review { rows, .. } | Dialog::FirstStart { rows, .. }) =
            self.dialog.as_mut()
        {
            let index = self.selection.min(rows.len().saturating_sub(1));
            if let Some(row) = rows.get_mut(index) {
                if !row.locked {
                    row.checked = !row.checked;
                }
            }
            return;
        }
        if self.dialog.is_some() {
            return;
        }
        let Some(item) = self.selected_item() else {
            return;
        };
        match self.view.clone() {
            View::PluginsTab => self.toggle_plugin(&item.key),
            View::Capabilities(id) => self.toggle_grant(&id, &item.key),
            View::ExtensionsOwner(_) | View::ExtensionsContributor(_) => {
                self.toggle_contribution(&item.key);
            }
            _ => {}
        }
    }

    /// USR-014..USR-214 input.
    pub fn press_enter(&mut self) {
        self.last_notice = None;
        if let Some(dialog) = self.dialog.clone() {
            match dialog {
                Dialog::FirstStart { .. } => self.answer_first_start(true),
                Dialog::Review { .. } => self.apply_review(),
                // No default trust answer: Enter alone decides nothing (USR-120).
                Dialog::Trust { .. } | Dialog::KeyChange { .. } => {}
                // The default is not to replace (USR-043).
                Dialog::Replace { .. } => {
                    self.replace_answers.push(false);
                    self.dialog = None;
                }
                // The default button is Cancel (USR-226) and No.
                Dialog::SupportBundle { .. } | Dialog::Confirm { .. } => self.dialog = None,
            }
            return;
        }
        if matches!(self.view, View::PluginsTab) {
            self.open_details();
        }
    }

    /// USR-014..USR-214 input.
    pub fn press_escape(&mut self) {
        self.last_notice = None;
        if self.dialog.is_some() {
            self.confirm_no();
            return;
        }
        self.view = View::Tree;
        self.selection = 0;
    }

    /// USR-014..USR-214 input: the prefix key followed by `key`; `:` opens the recovery page.
    pub fn press_prefix_then(&mut self, key: char) {
        self.last_notice = None;
        if key == ':' {
            self.open_recovery_page();
        }
    }

    /// USR-014..USR-214 input: selects the first row of the open view or dialog that matches.
    pub fn move_selection_to(&mut self, text: &str) {
        let items = self.items();
        let found = items
            .iter()
            .position(|item| item.key == text)
            .or_else(|| {
                items
                    .iter()
                    .position(|item| item.key.contains(text) || item.label.contains(text))
            });
        if let Some(index) = found {
            self.selection = index;
        }
    }

    /// USR-014..USR-214 input.
    pub fn select_plugin(&mut self, id: &str) {
        self.move_selection_to(id);
    }

    /// USR-014..USR-214 input: presses the button or action with this label.
    pub fn activate(&mut self, label: &str) {
        self.last_notice = None;
        let wanted = label.trim_matches(|c| c == '[' || c == ']').to_lowercase();
        if let Some(Dialog::Confirm { .. }) = self.dialog {
            match wanted.as_str() {
                "yes" => self.confirm_yes(),
                _ => self.confirm_no(),
            }
            return;
        }
        match self.view.clone() {
            View::Details(id) => match wanted.as_str() {
                "forget settings" => self.ask_confirm(ConfirmKind::ForgetSettings, &id),
                "uninstall" => self.ask_confirm(ConfirmKind::Uninstall, &id),
                _ => {}
            },
            View::ExtensionsOwner(_) => match wanted.as_str() {
                "move up" => self.move_extension_row(true),
                "move down" => self.move_extension_row(false),
                _ => {}
            },
            View::ExtensionsContributor(id) => {
                if wanted == "reset" {
                    for contribution in self
                        .contributions
                        .iter_mut()
                        .filter(|contribution| contribution.plugin == id)
                    {
                        contribution.breaches = 0;
                    }
                }
            }
            _ => {}
        }
    }

    /// USR-014..USR-214 input.
    pub fn confirm_yes(&mut self) {
        self.last_notice = None;
        let Some(dialog) = self.dialog.clone() else {
            return;
        };
        match dialog {
            Dialog::Confirm { kind, plugin } => {
                self.dialog = None;
                match kind {
                    ConfirmKind::ForgetSettings => {
                        if let Some(entry) = self.plugin_mut(&plugin) {
                            entry.settings.clear();
                            entry.project_settings.clear();
                        }
                    }
                    ConfirmKind::Uninstall => self.uninstall(&plugin),
                }
            }
            Dialog::FirstStart { .. } => self.answer_first_start(true),
            Dialog::Review { .. } => self.apply_review(),
            Dialog::Trust { .. } | Dialog::KeyChange { .. } => {
                self.trust_answers.push(true);
                self.dialog = None;
            }
            Dialog::Replace { .. } => {
                self.replace_answers.push(true);
                self.dialog = None;
            }
            Dialog::SupportBundle { .. } => self.dialog = None,
        }
    }

    /// USR-014..USR-214 input.
    pub fn confirm_no(&mut self) {
        self.last_notice = None;
        let Some(dialog) = self.dialog.clone() else {
            return;
        };
        match dialog {
            Dialog::FirstStart { .. } => self.answer_first_start(false),
            Dialog::Trust { .. } | Dialog::KeyChange { .. } => {
                self.trust_answers.push(false);
                self.dialog = None;
            }
            Dialog::Replace { .. } => {
                self.replace_answers.push(false);
                self.dialog = None;
            }
            Dialog::Review { .. } | Dialog::SupportBundle { .. } | Dialog::Confirm { .. } => {
                self.dialog = None;
            }
        }
    }

    /// USR-226: opens the Diagnostics "Build support bundle" dialog for a plugin.
    pub fn open_support_bundle_dialog(&mut self, plugin_id: &str) {
        self.dialog = Some(Dialog::SupportBundle {
            plugin: plugin_id.to_string(),
        });
        self.selection = 0;
    }

    /// USR-226: the label of the button that Enter presses in the open dialog.
    pub fn default_button_label(&self) -> Option<String> {
        let label = match self.dialog.as_ref()? {
            Dialog::SupportBundle { .. } => "Cancel",
            Dialog::Confirm { .. } => "No",
            Dialog::Replace { .. } => "Install without replacing",
            Dialog::FirstStart { .. } => "Allow selected",
            Dialog::Review { .. } => "Install",
            // The trust prompts have no default answer (USR-120).
            Dialog::Trust { .. } | Dialog::KeyChange { .. } => return None,
        };
        Some(label.to_string())
    }

    // ---- set-up ----

    /// USR-014..USR-214 set-up: installs a third-party plugin of the given tier.
    pub fn install_plugin(&mut self, id: &str, tier: &str) {
        self.install_named(id, id, tier);
    }

    /// USR-014..USR-214 set-up: installs a plugin that holds the given grants.
    pub fn install_plugin_with_grants(&mut self, id: &str, grants: &[&str]) {
        self.install_named(id, id, "T2");
        if let Some(plugin) = self.plugin_mut(id) {
            for grant in grants {
                plugin.grants.insert((*grant).to_string(), "granted".to_string());
            }
        }
    }

    /// USR-130: installs a plugin whose activation fails every time.
    pub fn install_plugin_that_traps_on_activate(&mut self, id: &str, name: &str) {
        self.install_named(id, name, "T2");
        self.set_trap(id, "activate trap");
        self.schedule_failures(id);
    }

    /// USR-130: installs a plugin whose view rendering fails every time.
    pub fn install_plugin_that_traps_in_render(&mut self, id: &str, name: &str, view: &str) {
        self.install_named(id, name, "T2");
        self.set_trap(id, &format!("views.render trap ({view})"));
        self.schedule_failures(id);
    }

    /// USR-032: a plugin that tries to take over the recovery page; the install is refused.
    pub fn install_plugin_that_claims_recovery_page(&mut self, id: &str) {
        self.install_error = Some(format!(
            "install.refused: {id} asks for the recovery page, which only the kernel provides"
        ));
    }

    /// USR-050: installs a plugin that owns one extension point.
    pub fn install_owner_with_point(&mut self, id: &str, owner_name: &str, point_name: &str) {
        self.install_named(id, owner_name, "T2");
        if let Some(plugin) = self.plugin_mut(id) {
            plugin.name = owner_name.to_string();
        }
        let key = (id.to_string(), point_name.to_string());
        if !self.extension_points.contains(&key) {
            self.extension_points.push(key.clone());
        }
        self.extension_orders.entry(key).or_default();
    }

    /// USR-050: installs a plugin that fills the point of another plugin. The point starts in
    /// priority order (lower first); a later move by the user wins over priority.
    pub fn install_contributor(
        &mut self,
        id: &str,
        owner: &str,
        point: &str,
        hook_name: &str,
        priority: i32,
    ) {
        self.install_named(id, id, "T2");
        self.contributions.push(Contribution {
            plugin: id.to_string(),
            owner: owner.to_string(),
            point: point.to_string(),
            hook: hook_name.to_string(),
            enabled: true,
            breaches: 0,
        });
        let key = (owner.to_string(), point.to_string());
        let priorities: BTreeMap<String, i32> = self
            .priority_table
            .iter()
            .filter(|((o, p, _), _)| *o == key.0 && *p == key.1)
            .map(|((_, _, plugin), priority)| (plugin.clone(), *priority))
            .collect();
        self.priority_table
            .insert((key.0.clone(), key.1.clone(), id.to_string()), priority);
        let order = self.extension_orders.entry(key).or_default();
        let position = order
            .iter()
            .position(|other| priorities.get(other).is_some_and(|value| *value > priority))
            .unwrap_or(order.len());
        order.insert(position, id.to_string());
    }

    /// USR-054: removes a plugin; its extension contributions stay and wait for it.
    pub fn remove_plugin(&mut self, id: &str) {
        self.uninstall(id);
    }

    /// USR-054: switches a plugin off.
    pub fn set_plugin_state_off(&mut self, id: &str) {
        self.put_plugin_in_state(id, RowState::Off);
    }

    /// USR-161: puts a plugin straight into a row state.
    pub fn put_plugin_in_state(&mut self, id: &str, state: RowState) {
        self.pending_failures.retain(|(_, other)| other != id);
        if let Some(plugin) = self.plugin_mut(id) {
            plugin.failures = if state == RowState::Failed {
                FAILURE_LIMIT
            } else {
                0
            };
            plugin.stop_notice = None;
            plugin.state = state;
        }
    }

    /// USR-032: three failures in a row stop the plugin.
    pub fn fail_plugin_three_times(&mut self, id: &str) {
        self.pending_failures.retain(|(_, other)| other != id);
        for _ in 0..FAILURE_LIMIT {
            self.apply_failure_reason(id, "failed three times in a row");
        }
    }

    /// USR-080: sets the state word (`granted`, `declined`, `revoked`) of one grant row.
    pub fn set_grant_state(&mut self, id: &str, grant: &str, state: &str) {
        if let Some(plugin) = self.plugin_mut(id) {
            plugin.grants.insert(grant.to_string(), state.to_string());
        }
    }

    /// USR-214: the administrator forbids these grants.
    pub fn set_policy_denied_grants(&mut self, grants: &[&str]) {
        self.policy_denied = grants.iter().map(|grant| (*grant).to_string()).collect();
    }

    /// USR-211, USR-212: stores the system policy file with the given file mode; the client reads
    /// it at its next start.
    pub fn write_system_policy_with_mode(&mut self, toml: &str, mode: u32) {
        self.policy_file = Some((toml.to_string(), mode));
    }

    /// USR-014: a bundled plugin asks for these grants at its first start.
    pub fn set_bundled_first_start(&mut self, id: &str, grants: &[&str]) {
        if self.plugin(id).is_none() {
            let mut plugin = Plugin::new(id, id, "T1", true);
            plugin.state = RowState::On;
            self.plugins.push(plugin);
        }
        let entry = FirstStart {
            plugin: id.to_string(),
            grants: grants.iter().map(|grant| (*grant).to_string()).collect(),
            not_now: 0,
            allowed: false,
        };
        match self
            .first_starts
            .iter_mut()
            .find(|existing| existing.plugin == id)
        {
            Some(existing) => *existing = entry,
            None => self.first_starts.push(entry),
        }
    }

    // ---- screens ----

    /// USR-022/USR-050/USR-120/USR-122 screens: starts the client in normal mode.
    pub fn start_client(&mut self) {
        self.start(false);
    }

    /// USR-031 screens: starts the client in safe mode.
    pub fn start_client_safe_mode(&mut self) {
        self.start(true);
    }

    /// USR-022/USR-050/USR-120/USR-122 screens.
    pub fn stop_client(&mut self) {
        self.started = false;
        self.dialog = None;
        self.view = View::Tree;
        self.selection = 0;
        self.pending_failures.clear();
    }

    /// USR-022/USR-050/USR-120/USR-122 screens.
    pub fn restart_client(&mut self) {
        self.stop_client();
        self.start(false);
    }

    /// USR-031 screens.
    pub fn open_settings(&mut self) {
        self.open_view(View::Settings);
    }

    /// USR-022/USR-050/USR-120/USR-122 screens.
    pub fn open_plugins_tab(&mut self) {
        self.open_view(View::PluginsTab);
    }

    /// USR-163 screens.
    pub fn open_plugin_details(&mut self, id: &str) {
        self.open_view(View::Details(id.to_string()));
    }

    /// USR-041 screens: details of the selected row of the Plugins tab.
    pub fn open_details(&mut self) {
        if let Some(item) = self.selected_item() {
            if matches!(self.view, View::PluginsTab | View::Recovery) {
                self.open_view(View::Details(item.key));
            }
        }
    }

    /// USR-080 screens.
    pub fn open_capabilities_tab(&mut self, id: &str) {
        self.open_view(View::Capabilities(id.to_string()));
    }

    /// USR-032, USR-160 screens.
    pub fn open_recovery_page(&mut self) {
        self.open_view(View::Recovery);
    }

    /// USR-022 screens: the install review dialog for the plugin and its grants.
    pub fn open_review_dialog(&mut self, id: &str, grants: &[&str]) {
        let rows = self.review_rows(grants.iter().map(|grant| (*grant).to_string()));
        self.dialog = Some(Dialog::Review {
            plugin: id.to_string(),
            rows,
        });
        self.selection = 0;
    }

    /// USR-120 screens.
    pub fn open_trust_prompt(&mut self, publisher: &str, fingerprint: &str) {
        self.dialog = Some(Dialog::Trust {
            publisher: publisher.to_string(),
            fingerprint: fingerprint.to_string(),
        });
        self.selection = 0;
    }

    /// USR-122 screens.
    pub fn open_key_change_prompt(&mut self, id: &str, old: &str, new: &str) {
        self.dialog = Some(Dialog::KeyChange {
            plugin: id.to_string(),
            old: old.to_string(),
            new: new.to_string(),
        });
        self.selection = 0;
    }

    /// USR-043 screens.
    pub fn open_replace_prompt(&mut self, id: &str, replaced: &str) {
        self.dialog = Some(Dialog::Replace {
            plugin: id.to_string(),
            replaced: replaced.to_string(),
        });
        self.selection = 0;
    }

    /// USR-050 screens.
    pub fn open_extensions_owner_view(&mut self, id: &str) {
        self.open_view(View::ExtensionsOwner(id.to_string()));
    }

    /// USR-053 screens.
    pub fn open_extensions_contributor_view(&mut self, id: &str) {
        self.open_view(View::ExtensionsContributor(id.to_string()));
    }

    /// USR-951: switches the rig to the `[ui] screen_reader = true` mode.
    pub fn set_screen_reader(&mut self, enabled: bool) {
        self.screen_reader = enabled;
    }

    /// USR-951: the lines a screen reader speaks, one entry per announcement, in focus order.
    /// Each element has its role word and state; no frame or spinner character is spoken.
    pub fn speech_lines(&self) -> Vec<String> {
        let mut spoken = Vec::new();
        match &self.dialog {
            Some(Dialog::Review { rows, .. }) => {
                spoken.extend(self.speak_rows(rows));
                spoken.push("Install, button".to_string());
                spoken.push("Cancel, button".to_string());
            }
            Some(Dialog::FirstStart { rows, .. }) => {
                spoken.extend(self.speak_rows(rows));
                spoken.push("Allow selected, button".to_string());
                spoken.push("Not now, button".to_string());
            }
            Some(_) => spoken.extend(self.screen_text().lines().map(str::to_string)),
            None => {
                let items = self.items();
                let total = items.len();
                if matches!(self.view, View::PluginsTab | View::Recovery) {
                    for (index, item) in items.iter().enumerate() {
                        let selected = if index == self.selection {
                            "selected, "
                        } else {
                            ""
                        };
                        let word = self
                            .plugin(&item.key)
                            .map(|plugin| self.state_word(plugin))
                            .unwrap_or_default();
                        spoken.push(format!(
                            "{}, {word}, {selected}{} of {total}",
                            item.key,
                            index + 1
                        ));
                    }
                } else {
                    spoken.extend(self.screen_text().lines().map(str::to_string));
                }
            }
        }
        spoken
            .into_iter()
            .map(|line| {
                line.chars()
                    .filter(|c| !matches!(*c, '\u{2500}'..='\u{257f}' | '\u{2800}'..='\u{28ff}'))
                    .collect::<String>()
                    .trim()
                    .to_string()
            })
            .filter(|line| !line.is_empty())
            .collect()
    }

    // ---- read-back ----

    /// USR-162 read-back: rows of the Plugins tab.
    pub fn plugin_row_count(&self) -> usize {
        self.plugins.len()
    }

    /// USR-161 read-back: the text of `ilium plugin list`.
    pub fn cli_list_text(&self) -> String {
        self.plugins
            .iter()
            .map(|plugin| {
                format!(
                    "{:<28} {}  {}",
                    plugin.id,
                    plugin.tier,
                    self.state_word(plugin)
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// USR-161 read-back.
    pub fn plugin_state_word(&self, id: &str) -> String {
        self.plugin(id)
            .map(|plugin| self.state_word(plugin))
            .unwrap_or_default()
    }

    /// USR-014 read-back.
    pub fn plugin_is_running(&self, id: &str) -> bool {
        self.plugin(id).is_some_and(|plugin| self.is_running(plugin))
    }

    /// USR-160 read-back.
    pub fn failure_count(&self, id: &str) -> usize {
        self.plugin(id)
            .map(|plugin| plugin.failures as usize)
            .unwrap_or_default()
    }

    /// USR-120 read-back: the trust answers given so far.
    pub fn trust_answers(&self) -> Vec<bool> {
        self.trust_answers.clone()
    }

    /// USR-043 read-back: the replace answers given so far.
    pub fn replace_answers(&self) -> Vec<bool> {
        self.replace_answers.clone()
    }

    /// USR-080 read-back: the plain wording of a permission.
    pub fn plain_wording(&self, grant: &str) -> String {
        grant_info(grant).0
    }

    /// USR-022 read-back.
    pub fn grant_is_high(&self, grant: &str) -> bool {
        grant_info(grant).1
    }

    /// USR-050 read-back: the plugin ids that run at this point, in run order.
    pub fn run_order_of_point(&self, owner: &str, point: &str) -> Vec<String> {
        let key = (owner.to_string(), point.to_string());
        self.extension_orders
            .get(&key)
            .map(|order| {
                order
                    .iter()
                    .filter(|plugin| {
                        self.contributions.iter().any(|contribution| {
                            contribution.plugin == **plugin
                                && contribution.owner == owner
                                && contribution.point == point
                                && contribution.enabled
                                && !contribution.is_stopped()
                        })
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// USR-053: the extension fails `times` more times; the third failure stops it.
    pub fn breach_extension(&mut self, contributor: &str, owner: &str, point: &str, times: u32) {
        let mut stopped = None;
        if let Some(contribution) = self.contributions.iter_mut().find(|contribution| {
            contribution.plugin == contributor
                && contribution.owner == owner
                && contribution.point == point
        }) {
            let before = contribution.is_stopped();
            contribution.breaches = contribution.breaches.saturating_add(times);
            if !before && contribution.is_stopped() {
                stopped = Some(format!(
                    "Extension {} of {} stopped after {FAILURE_LIMIT} failures.",
                    contribution.hook, contribution.plugin
                ));
            }
        }
        if stopped.is_some() {
            self.last_notice = stopped;
        }
    }

    /// USR-163 read-back: a user-scope setting value.
    pub fn plugin_setting(&self, id: &str, key: &str) -> Option<String> {
        self.plugin(id)?.settings.get(key).cloned()
    }

    /// USR-163 read-back: a project-scope setting value.
    pub fn plugin_setting_project(&self, id: &str, key: &str) -> Option<String> {
        self.plugin(id)?.project_settings.get(key).cloned()
    }

    /// USR-163: stores a user-scope setting value.
    pub fn set_plugin_setting(&mut self, id: &str, key: &str, value: &str) {
        if let Some(plugin) = self.plugin_mut(id) {
            plugin.settings.insert(key.to_string(), value.to_string());
        }
    }

    /// USR-163: stores a project-scope setting value.
    pub fn set_plugin_setting_project(&mut self, id: &str, key: &str, value: &str) {
        if let Some(plugin) = self.plugin_mut(id) {
            plugin
                .project_settings
                .insert(key.to_string(), value.to_string());
        }
    }

    /// USR-163 read-back: the bytes of the grants file, one line per plugin and grant.
    pub fn grants_file_bytes(&self) -> Vec<u8> {
        let mut plugins: Vec<&Plugin> = self.plugins.iter().collect();
        plugins.sort_by(|left, right| left.id.cmp(&right.id));
        let mut text = String::new();
        for plugin in plugins {
            for (grant, state) in &plugin.grants {
                text.push_str(&format!("{}\t{grant}\t{state}\n", plugin.id));
            }
        }
        text.into_bytes()
    }

    /// USR-160 read-back: the config files the page asked the host to open in the editor.
    pub fn requested_editor_paths(&self) -> Vec<String> {
        self.editor_paths.clone()
    }

    /// USR-160 read-back.
    pub fn restart_requested_in_safe_mode(&self) -> bool {
        self.safe_mode_restart_requested
    }

    /// USR-032 read-back: who provides the recovery page. A plugin never can.
    pub fn recovery_page_owner(&self) -> String {
        "kernel".to_string()
    }

    /// USR-032 read-back: why the last install was refused.
    pub fn last_install_error(&self) -> Option<String> {
        self.install_error.clone()
    }

    // ---- plugin actions ----

    /// USR-087: a plugin reads the clipboard. A plugin that is installed and lacks the grant
    /// is refused and shows no indicator.
    pub fn plugin_reads_clipboard(&mut self, id: &str) {
        if self.clipboard_allowed(id, "clipboard.read") {
            self.clipboard_indicator = Some((format!("Clipboard read by {id}"), self.now_ms));
        }
    }

    /// USR-087: a plugin writes the clipboard.
    pub fn plugin_writes_clipboard(&mut self, id: &str) {
        if self.clipboard_allowed(id, "clipboard.write") {
            self.clipboard_indicator = Some((format!("Clipboard written by {id}"), self.now_ms));
        }
    }

    /// USR-088: a plugin shows a toast; more than three per ten seconds are dropped, and no more
    /// than three are alive at once.
    pub fn plugin_toasts(&mut self, id: &str, text: &str) {
        let now = self.now_ms;
        let times = self.toast_times.entry(id.to_string()).or_default();
        times.retain(|at| now.saturating_sub(*at) < TOAST_WINDOW_MS);
        if times.len() >= TOASTS_PER_WINDOW {
            return;
        }
        times.push(now);
        self.toasts.push(Toast {
            plugin: id.to_string(),
            text: text.to_string(),
            until: now + TOAST_LIFETIME_MS,
        });
        while self.toasts.len() > TOASTS_PER_WINDOW {
            self.toasts.remove(0);
        }
    }

    /// USR-088: a plugin locks the keyboard; the lock ends after two seconds.
    pub fn plugin_locks_input(&mut self, _id: &str) {
        self.input_locked_until = Some(self.now_ms + INPUT_LOCK_MS);
    }

    /// USR-088: whether the focused pane takes `key` now; false while the lock holds or when the
    /// key was pressed during the lock and dropped.
    pub fn pane_received_input(&self, key: char) -> bool {
        if self.input_locked() {
            return false;
        }
        self.delivered_keys.contains(&key) || !self.dropped_keys.contains(&key)
    }
}

// ---- internals ----

impl ScreensRig {
    fn plugin(&self, id: &str) -> Option<&Plugin> {
        self.plugins.iter().find(|plugin| plugin.id == id)
    }

    fn plugin_mut(&mut self, id: &str) -> Option<&mut Plugin> {
        self.plugins.iter_mut().find(|plugin| plugin.id == id)
    }

    fn install_named(&mut self, id: &str, name: &str, tier: &str) {
        self.install_error = None;
        if self.plugin(id).is_some() {
            return;
        }
        self.plugins.push(Plugin::new(id, name, tier, false));
    }

    fn set_trap(&mut self, id: &str, reason: &str) {
        if let Some(plugin) = self.plugin_mut(id) {
            plugin.trap_reason = Some(reason.to_string());
        }
    }

    /// A trapping plugin fails again one, three and five seconds after it is switched on.
    fn schedule_failures(&mut self, id: &str) {
        self.pending_failures.retain(|(_, other)| other != id);
        for offset in [1_000, 3_000, 5_000] {
            self.pending_failures
                .push((self.now_ms + offset, id.to_string()));
        }
    }

    fn apply_failure(&mut self, id: &str) {
        let reason = self
            .plugin(id)
            .and_then(|plugin| plugin.trap_reason.clone())
            .unwrap_or_else(|| "plugin failed".to_string());
        self.apply_failure_reason(id, &reason);
    }

    fn apply_failure_reason(&mut self, id: &str, reason: &str) {
        let Some(plugin) = self.plugin_mut(id) else {
            return;
        };
        if !matches!(
            plugin.state,
            RowState::On | RowState::OnSlow | RowState::Restarting
        ) {
            return;
        }
        plugin.failures += 1;
        if plugin.failures >= FAILURE_LIMIT {
            plugin.state = RowState::Failed;
            plugin.stop_notice = Some(format!(
                "Plugin {} stopped after {FAILURE_LIMIT} failures in 10 minutes: {reason}. Turn it on again in Settings > Plugins.",
                plugin.name
            ));
        } else {
            plugin.state = RowState::Restarting;
        }
    }

    fn enable(&mut self, id: &str) {
        let traps = self
            .plugin(id)
            .is_some_and(|plugin| plugin.trap_reason.is_some());
        if let Some(plugin) = self.plugin_mut(id) {
            plugin.state = RowState::On;
            plugin.failures = 0;
            plugin.stop_notice = None;
        }
        if traps {
            self.schedule_failures(id);
        }
    }

    fn disable(&mut self, id: &str) {
        self.pending_failures.retain(|(_, other)| other != id);
        if let Some(plugin) = self.plugin_mut(id) {
            plugin.state = RowState::Off;
            plugin.stop_notice = None;
        }
    }

    fn uninstall(&mut self, id: &str) {
        self.pending_failures.retain(|(_, other)| other != id);
        self.plugins.retain(|plugin| plugin.id != id);
        let count = self.items().len();
        self.selection = self.selection.min(count.saturating_sub(1));
    }

    fn toggle_plugin(&mut self, id: &str) {
        let Some(plugin) = self.plugin(id) else {
            return;
        };
        if plugin.is_protected() {
            self.last_notice = Some(format!(
                "{id} cannot be turned off: Ilium always needs it."
            ));
            return;
        }
        if !plugin.core && self.safe_mode {
            self.last_notice =
                Some("Third-party plugins stay off in safe mode.".to_string());
            return;
        }
        match plugin.state {
            RowState::On | RowState::OnSlow | RowState::Restarting => self.disable(id),
            RowState::Off | RowState::Failed => self.enable(id),
            RowState::WaitingFor(_) | RowState::BlockedByPolicy | RowState::Refused(_) => {
                self.last_notice = Some(format!("{id} cannot be turned on from here."));
            }
        }
    }

    fn toggle_grant(&mut self, id: &str, grant: &str) {
        if self.policy_denied.iter().any(|denied| denied == grant) {
            return;
        }
        if let Some(plugin) = self.plugin_mut(id) {
            let next = match plugin.grants.get(grant).map(String::as_str) {
                Some("granted") => "revoked",
                _ => "granted",
            };
            plugin.grants.insert(grant.to_string(), next.to_string());
        }
    }

    fn toggle_contribution(&mut self, plugin_id: &str) {
        let owner_point = match &self.view {
            View::ExtensionsOwner(owner) => self
                .extension_points
                .iter()
                .filter(|(point_owner, _)| point_owner == owner)
                .find(|key| {
                    self.extension_orders
                        .get(*key)
                        .is_some_and(|order| order.iter().any(|id| id == plugin_id))
                })
                .cloned(),
            _ => None,
        };
        for contribution in &mut self.contributions {
            let matches_point = owner_point.as_ref().is_none_or(|(owner, point)| {
                contribution.owner == *owner && contribution.point == *point
            });
            if contribution.plugin == plugin_id && matches_point {
                contribution.enabled = !contribution.enabled;
                break;
            }
        }
    }

    fn move_extension_row(&mut self, up: bool) {
        let View::ExtensionsOwner(owner) = self.view.clone() else {
            return;
        };
        let Some(item) = self.selected_item() else {
            return;
        };
        let Some(point) = self
            .extension_points
            .iter()
            .filter(|(point_owner, _)| *point_owner == owner)
            .find(|key| {
                self.extension_orders
                    .get(*key)
                    .is_some_and(|order| order.iter().any(|id| *id == item.key))
            })
            .cloned()
        else {
            return;
        };
        if let Some(order) = self.extension_orders.get_mut(&point) {
            if let Some(index) = order.iter().position(|id| *id == item.key) {
                let target = if up { index.checked_sub(1) } else { Some(index + 1) };
                if let Some(target) = target.filter(|target| *target < order.len()) {
                    order.swap(index, target);
                    self.selection = self.selection.saturating_add(target).saturating_sub(index);
                }
            }
        }
    }

    fn move_selection(&mut self, up: bool) {
        let count = self.items().len();
        if count == 0 {
            return;
        }
        self.selection = if up {
            self.selection.saturating_sub(1)
        } else {
            (self.selection + 1).min(count - 1)
        };
    }

    fn open_view(&mut self, view: View) {
        self.dialog = None;
        self.view = view;
        self.selection = 0;
        self.last_notice = None;
    }

    fn ask_confirm(&mut self, kind: ConfirmKind, id: &str) {
        self.dialog = Some(Dialog::Confirm {
            kind,
            plugin: id.to_string(),
        });
    }

    fn input_locked(&self) -> bool {
        self.input_locked_until
            .is_some_and(|until| self.now_ms < until)
    }

    fn pane_key(&mut self, key: char) {
        if self.input_locked() {
            self.dropped_keys.push(key);
        } else {
            self.delivered_keys.push(key);
        }
    }

    fn clipboard_allowed(&self, id: &str, grant: &str) -> bool {
        match self.plugin(id) {
            Some(plugin) => plugin.grants.get(grant).is_some_and(|state| state == "granted"),
            None => true,
        }
    }

    fn recovery_key(&mut self, key: char) {
        let selected = self.selected_item().map(|item| item.key);
        match key {
            'q' => {
                self.view = View::Tree;
                self.selection = 0;
            }
            's' => self.safe_mode_restart_requested = true,
            'o' => {
                if let Some(dirs) = directories::ProjectDirs::from("", "", "ilium") {
                    self.editor_paths.push(
                        dirs.config_dir()
                            .join("config.toml")
                            .to_string_lossy()
                            .into_owned(),
                    );
                }
            }
            'e' | 'd' | 'r' | 'u' | 'f' => {
                let Some(id) = selected else {
                    return;
                };
                match key {
                    'e' => self.enable(&id),
                    'd' => self.disable(&id),
                    'r' => {
                        self.pending_failures.retain(|(_, other)| *other != id);
                        if let Some(plugin) = self.plugin_mut(&id) {
                            plugin.failures = 0;
                            plugin.stop_notice = None;
                        }
                    }
                    'u' => {
                        if self.plugin(&id).is_some_and(|plugin| plugin.core) {
                            self.last_notice = Some(format!("{id} is a core plugin and stays installed."));
                        } else {
                            self.ask_confirm(ConfirmKind::Uninstall, &id);
                        }
                    }
                    _ => self.ask_confirm(ConfirmKind::ForgetSettings, &id),
                }
            }
            _ => {}
        }
    }

    fn dialog_key(&mut self, key: char) {
        let Some(dialog) = self.dialog.clone() else {
            return;
        };
        match (dialog, key) {
            (Dialog::Trust { .. } | Dialog::KeyChange { .. }, 't') => self.confirm_yes(),
            (Dialog::Trust { .. } | Dialog::KeyChange { .. }, 'n') => self.confirm_no(),
            (Dialog::Replace { .. }, 'r') => self.confirm_yes(),
            (Dialog::Replace { .. }, 'n') => self.confirm_no(),
            (Dialog::Confirm { .. }, 'y') => self.confirm_yes(),
            (Dialog::Confirm { .. }, 'n') => self.confirm_no(),
            (Dialog::SupportBundle { .. }, 'b') => self.dialog = None,
            (Dialog::Review { .. } | Dialog::FirstStart { .. }, ' ') => self.press_space(),
            _ => {}
        }
    }

    fn review_rows(&self, grants: impl Iterator<Item = String>) -> Vec<ReviewRow> {
        grants
            .map(|grant| {
                let locked = self.policy_denied.contains(&grant);
                let high = grant_info(&grant).1;
                ReviewRow {
                    checked: !high && !locked,
                    locked,
                    grant,
                }
            })
            .collect()
    }

    fn apply_rows(&mut self, plugin_id: &str, rows: &[ReviewRow]) {
        if self.plugin(plugin_id).is_none() {
            self.install_named(plugin_id, plugin_id, "T2");
        }
        if let Some(plugin) = self.plugin_mut(plugin_id) {
            for row in rows {
                let state = if row.checked { "granted" } else { "declined" };
                plugin.grants.insert(row.grant.clone(), state.to_string());
            }
        }
    }

    fn apply_review(&mut self) {
        if let Some(Dialog::Review { plugin, rows }) = self.dialog.take() {
            self.apply_rows(&plugin, &rows);
        }
    }

    fn answer_first_start(&mut self, allow: bool) {
        let Some(Dialog::FirstStart { plugin, rows }) = self.dialog.take() else {
            return;
        };
        if allow {
            self.apply_rows(&plugin, &rows);
        }
        if let Some(entry) = self
            .first_starts
            .iter_mut()
            .find(|entry| entry.plugin == plugin)
        {
            if allow {
                entry.allowed = true;
            } else {
                entry.not_now += 1;
            }
        }
    }

    fn start(&mut self, safe_mode: bool) {
        self.started = true;
        self.safe_mode = safe_mode;
        self.view = View::Tree;
        self.selection = 0;
        self.dialog = None;
        self.last_notice = None;
        self.toasts.clear();
        self.toast_times.clear();
        self.clipboard_indicator = None;
        self.input_locked_until = None;
        self.pending_failures.clear();
        for contribution in &mut self.contributions {
            contribution.breaches = 0;
        }
        let (notice, blocks) = self.evaluate_policy();
        self.policy_notice = notice;
        self.policy_blocks_third_party = blocks;
        let pending = self
            .first_starts
            .iter()
            .find(|entry| !entry.allowed && entry.not_now < FIRST_START_LIMIT)
            .cloned();
        if let Some(entry) = pending {
            let rows = self.review_rows(entry.grants.iter().cloned());
            self.dialog = Some(Dialog::FirstStart {
                plugin: entry.plugin,
                rows,
            });
        }
        let trapping: Vec<String> = self
            .plugins
            .iter()
            .filter(|plugin| plugin.trap_reason.is_some() && self.is_running(plugin))
            .map(|plugin| plugin.id.clone())
            .collect();
        for id in trapping {
            self.schedule_failures(&id);
        }
    }

    /// Reads the system policy file: the notice to show and whether third-party plugins refuse
    /// to load (USR-211, USR-212).
    fn evaluate_policy(&self) -> (Option<String>, bool) {
        let Some((text, mode)) = &self.policy_file else {
            return (None, false);
        };
        if mode & 0o022 != 0 {
            return (
                Some(
                    "System policy file can be changed by people who are not administrators, so Ilium ignores it."
                        .to_string(),
                ),
                false,
            );
        }
        let mut section = String::new();
        for (index, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let number = index + 1;
            if let Some(name) = line.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
                if !["packages", "publishers", "grants", "network", "notice"].contains(&name) {
                    return (Some(policy_error(name, number)), true);
                }
                section = name.to_string();
                continue;
            }
            let key = line.split('=').next().unwrap_or(line).trim();
            let known: &[&str] = match section.as_str() {
                "" => &["version"],
                "packages" => &[
                    "allowed_tiers",
                    "allow_unsigned",
                    "publishers.allow",
                    "preapproved",
                ],
                "publishers" => &["allow", "deny"],
                "grants" => &["allow", "deny"],
                "network" => &["allow", "deny"],
                _ => &["contact"],
            };
            if !known.contains(&key) {
                return (Some(policy_error(key, number)), true);
            }
        }
        (None, false)
    }

    fn is_running(&self, plugin: &Plugin) -> bool {
        if !plugin.core && (self.safe_mode || self.policy_blocks_third_party) {
            return false;
        }
        matches!(
            plugin.state,
            RowState::On | RowState::OnSlow | RowState::Restarting
        )
    }

    fn state_word(&self, plugin: &Plugin) -> String {
        if !plugin.core && self.safe_mode {
            return "Off, safe mode".to_string();
        }
        if !plugin.core
            && self.policy_blocks_third_party
            && matches!(plugin.state, RowState::On | RowState::OnSlow)
        {
            return "Blocked by policy".to_string();
        }
        match &plugin.state {
            RowState::On => "On".to_string(),
            RowState::OnSlow => "On, slow".to_string(),
            RowState::Restarting => "Restarting".to_string(),
            RowState::Failed => "Failed".to_string(),
            RowState::Off => "Off".to_string(),
            RowState::WaitingFor(owner) => format!("Waiting for {owner}"),
            RowState::BlockedByPolicy => "Blocked by policy".to_string(),
            RowState::Refused(reason) => format!("Refused: {reason}"),
        }
    }

    /// Rows of the recovery page: problems first, then the rest in install order.
    fn recovery_order(&self) -> Vec<&Plugin> {
        let mut ordered: Vec<&Plugin> = self.plugins.iter().collect();
        ordered.sort_by_key(|plugin| match plugin.state {
            RowState::Failed => 0,
            RowState::Restarting
            | RowState::Refused(_)
            | RowState::BlockedByPolicy
            | RowState::WaitingFor(_) => 1,
            RowState::Off => 2,
            RowState::On | RowState::OnSlow => 3,
        });
        ordered
    }

    fn contributor_waits(&self, contribution: &Contribution) -> bool {
        match self.plugin(&contribution.owner) {
            None => true,
            Some(owner) => matches!(owner.state, RowState::Off),
        }
    }

    fn owner_rows(&self, owner: &str) -> Vec<(String, String, Item)> {
        let mut rows = Vec::new();
        for (point_owner, point) in &self.extension_points {
            if point_owner != owner {
                continue;
            }
            let key = (point_owner.clone(), point.clone());
            let Some(order) = self.extension_orders.get(&key) else {
                continue;
            };
            for plugin in order {
                if let Some(contribution) = self.contributions.iter().find(|contribution| {
                    contribution.plugin == *plugin
                        && contribution.owner == *point_owner
                        && contribution.point == *point
                }) {
                    rows.push((
                        point_owner.clone(),
                        point.clone(),
                        Item {
                            key: plugin.clone(),
                            label: contribution.hook.clone(),
                        },
                    ));
                }
            }
        }
        rows
    }

    /// The selectable rows of the open dialog, or else of the open view.
    fn items(&self) -> Vec<Item> {
        if let Some(dialog) = &self.dialog {
            return match dialog {
                Dialog::Review { rows, .. } | Dialog::FirstStart { rows, .. } => rows
                    .iter()
                    .map(|row| Item {
                        key: row.grant.clone(),
                        label: row.grant.clone(),
                    })
                    .collect(),
                _ => Vec::new(),
            };
        }
        match &self.view {
            View::PluginsTab => self
                .plugins
                .iter()
                .map(|plugin| Item {
                    key: plugin.id.clone(),
                    label: plugin.name.clone(),
                })
                .collect(),
            View::Recovery => self
                .recovery_order()
                .into_iter()
                .map(|plugin| Item {
                    key: plugin.id.clone(),
                    label: plugin.name.clone(),
                })
                .collect(),
            View::Capabilities(id) => self
                .plugin(id)
                .map(|plugin| {
                    plugin
                        .grants
                        .keys()
                        .map(|grant| Item {
                            key: grant.clone(),
                            label: grant.clone(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            View::ExtensionsOwner(owner) => self
                .owner_rows(owner)
                .into_iter()
                .map(|(_, _, item)| item)
                .collect(),
            View::ExtensionsContributor(id) => self
                .contributions
                .iter()
                .filter(|contribution| contribution.plugin == *id)
                .map(|contribution| Item {
                    key: contribution.plugin.clone(),
                    label: contribution.hook.clone(),
                })
                .collect(),
            View::Tree | View::Settings | View::Details(_) => Vec::new(),
        }
    }

    fn selected_item(&self) -> Option<Item> {
        self.items().into_iter().nth(self.selection)
    }

    // ---- drawing ----

    fn row_line(text: String) -> DrawnLine {
        DrawnLine { text, wrap: false }
    }

    fn prose_line(text: impl Into<String>) -> DrawnLine {
        DrawnLine {
            text: text.into(),
            wrap: true,
        }
    }

    fn flatten(&self, lines: Vec<DrawnLine>, wrap: bool) -> String {
        let columns = usize::from(self.columns);
        let mut out = Vec::new();
        for line in lines {
            if wrap && line.wrap && line.text.chars().count() > columns {
                let indent: String = line.text.chars().take_while(|c| *c == ' ').collect();
                let width = columns.saturating_sub(indent.chars().count()).max(1);
                for part in wrap_text(&line.text, width) {
                    out.push(format!("{indent}{part}"));
                }
            } else {
                out.push(line.text);
            }
        }
        out.join("\n")
    }

    fn draw(&self, wrap: bool) -> String {
        if !self.started {
            return "Ilium client is not running.".to_string();
        }
        let mut lines = Vec::new();
        if self.safe_mode {
            lines.push(Self::prose_line(SAFE_MODE_BANNER));
            lines.push(Self::prose_line(
                "Reason: safe-mode. Start without the safe-mode flag to turn plugins on.",
            ));
        }
        match &self.dialog {
            Some(dialog) => self.draw_dialog(dialog, &mut lines),
            None => self.draw_view(&mut lines),
        }
        for toast in &self.toasts {
            if self.now_ms < toast.until {
                lines.push(Self::prose_line(format!("[{}] {}", toast.plugin, toast.text)));
            }
        }
        lines.extend(self.footer_lines());
        let _ = self.rows;
        self.flatten(lines, wrap)
    }

    fn footer_lines(&self) -> Vec<DrawnLine> {
        let mut lines = Vec::new();
        if !self.started {
            return lines;
        }
        if let Some(notice) = &self.policy_notice {
            lines.push(Self::prose_line(notice.clone()));
        }
        for plugin in &self.plugins {
            if let Some(notice) = &plugin.stop_notice {
                lines.push(Self::prose_line(notice.clone()));
            }
        }
        for contribution in self.contributions.iter().filter(|c| c.is_stopped()) {
            lines.push(Self::prose_line(format!(
                "Extension {} of {} stopped after {FAILURE_LIMIT} failures. Press [Reset] on its Extensions page.",
                contribution.hook, contribution.plugin
            )));
        }
        if let Some((text, since)) = &self.clipboard_indicator {
            if self.now_ms.saturating_sub(*since) < CLIPBOARD_INDICATOR_MS {
                lines.push(Self::prose_line(text.clone()));
            }
        }
        lines
    }

    fn grant_state_text(&self, plugin: &Plugin, grant: &str) -> String {
        if self.policy_denied.iter().any(|denied| denied == grant) {
            return ADMIN_LOCK_TEXT.to_string();
        }
        plugin
            .grants
            .get(grant)
            .cloned()
            .unwrap_or_else(|| "granted".to_string())
    }

    fn draw_view(&self, lines: &mut Vec<DrawnLine>) {
        match &self.view {
            View::Tree => lines.push(Self::prose_line("Project tree")),
            View::Settings => {
                lines.push(Self::prose_line("Settings"));
                lines.push(Self::prose_line("General   Plugins   Animations   Diagnostics"));
            }
            View::PluginsTab => {
                lines.push(Self::prose_line("Settings > Plugins"));
                lines.push(Self::row_line("Plugin                       Tier  State".to_string()));
                for (index, plugin) in self.plugins.iter().enumerate() {
                    let marker = if index == self.selection { ">" } else { " " };
                    lines.push(Self::row_line(format!(
                        "{marker}{:<28} {:<4}  {}",
                        plugin.id,
                        plugin.tier,
                        self.state_word(plugin)
                    )));
                }
                lines.push(Self::prose_line("Space on or off   Enter details"));
            }
            View::Details(id) => {
                lines.push(Self::prose_line("Plugin details"));
                match self.plugin(id) {
                    Some(plugin) => {
                        lines.push(Self::row_line(format!("Plugin: {}", plugin.id)));
                        lines.push(Self::row_line(format!("Tier: {}", plugin.tier)));
                        lines.push(Self::row_line(format!("State: {}", self.state_word(plugin))));
                        lines.push(Self::row_line("[Forget settings]".to_string()));
                        if !plugin.core {
                            lines.push(Self::row_line("[Uninstall]".to_string()));
                        }
                    }
                    None => lines.push(Self::prose_line("This plugin is not installed.")),
                }
            }
            View::Capabilities(id) => {
                lines.push(Self::prose_line(format!("Capabilities of {id}")));
                if let Some(plugin) = self.plugin(id) {
                    for (index, grant) in plugin.grants.keys().enumerate() {
                        let (wording, high) = grant_info(grant);
                        let marker = if index == self.selection { ">" } else { " " };
                        let risk = if high { "  HIGH" } else { "" };
                        lines.push(Self::row_line(format!(
                            "{marker}{grant}  {wording}{risk}  {}",
                            self.grant_state_text(plugin, grant)
                        )));
                    }
                }
            }
            View::Recovery => {
                lines.push(Self::prose_line("Recovery"));
                for (index, plugin) in self.recovery_order().into_iter().enumerate() {
                    let marker = if index == self.selection { ">" } else { " " };
                    lines.push(Self::row_line(format!(
                        "{marker}{:<28} {}",
                        plugin.id,
                        self.state_word(plugin)
                    )));
                }
                for action in [
                    "e enable",
                    "d disable",
                    "r reset failures",
                    "u uninstall",
                    "f forget settings",
                    "s safe mode",
                    "o open config file",
                    "q close",
                ] {
                    lines.push(Self::row_line(action.to_string()));
                }
            }
            View::ExtensionsOwner(owner) => self.draw_owner_view(owner, lines),
            View::ExtensionsContributor(id) => {
                lines.push(Self::prose_line(format!("Extensions of {id}")));
                for contribution in self.contributions.iter().filter(|c| c.plugin == *id) {
                    let check = if contribution.enabled { "[x]" } else { "[ ]" };
                    lines.push(Self::row_line(format!(
                        "{check} {}  ({} / {})",
                        contribution.hook, contribution.owner, contribution.point
                    )));
                    if self.contributor_waits(contribution) {
                        lines.push(Self::row_line(format!(
                            "    waiting for {}",
                            contribution.owner
                        )));
                    }
                    if contribution.is_stopped() {
                        lines.push(Self::row_line(format!(
                            "    {} stopped after {FAILURE_LIMIT} failures   [Reset]",
                            contribution.hook
                        )));
                    }
                }
            }
        }
    }

    fn draw_owner_view(&self, owner: &str, lines: &mut Vec<DrawnLine>) {
        let Some(owner_plugin) = self.plugin(owner) else {
            lines.push(Self::prose_line("This plugin is not installed."));
            return;
        };
        let mut shown = 0;
        for (point_owner, point) in &self.extension_points {
            if point_owner != owner {
                continue;
            }
            lines.push(Self::row_line(format!("{}: {point}", owner_plugin.name)));
            let key = (point_owner.clone(), point.clone());
            let order = self.extension_orders.get(&key).cloned().unwrap_or_default();
            for (number, plugin) in order.iter().enumerate() {
                let Some(contribution) = self.contributions.iter().find(|contribution| {
                    contribution.plugin == *plugin
                        && contribution.owner == *point_owner
                        && contribution.point == *point
                }) else {
                    continue;
                };
                let check = if contribution.enabled { "[x]" } else { "[ ]" };
                let marker = if shown == self.selection { ">" } else { " " };
                lines.push(Self::row_line(format!(
                    "{marker}{} {check} {}  ({plugin})",
                    number + 1,
                    contribution.hook
                )));
                shown += 1;
            }
        }
        lines.push(Self::row_line("Up/Down move   Space on or off".to_string()));
    }

    fn draw_rows(&self, rows: &[ReviewRow], lines: &mut Vec<DrawnLine>) {
        let columns = usize::from(self.columns);
        for (index, row) in rows.iter().enumerate() {
            let (wording, high) = grant_info(&row.grant);
            let check = if row.locked {
                "[-]"
            } else if row.checked {
                "[x]"
            } else {
                "[ ]"
            };
            let marker = if index == self.selection { ">" } else { " " };
            let risk = if high { "  HIGH" } else { "" };
            let lock = if row.locked {
                format!("  {ADMIN_LOCK_TEXT}")
            } else {
                String::new()
            };
            let single = format!("{marker}{check} {}  {wording}{risk}{lock}", row.grant);
            if single.chars().count() <= columns {
                lines.push(Self::row_line(single));
                continue;
            }
            // The row is too wide: the grant string goes on its own lines, never cut.
            lines.push(Self::row_line(format!("{marker}{check} {wording}{risk}{lock}")));
            let chars: Vec<char> = row.grant.chars().collect();
            for chunk in chars.chunks(columns.saturating_sub(4).max(1)) {
                lines.push(Self::row_line(format!("    {}", chunk.iter().collect::<String>())));
            }
        }
    }

    fn draw_dialog(&self, dialog: &Dialog, lines: &mut Vec<DrawnLine>) {
        match dialog {
            Dialog::FirstStart { plugin, rows } => {
                lines.push(Self::prose_line(format!("First start: {plugin} asks for permissions")));
                self.draw_rows(rows, lines);
                lines.push(Self::row_line("[Allow selected]  [Not now]".to_string()));
            }
            Dialog::Review { plugin, rows } => {
                lines.push(Self::prose_line(format!("Review permissions of {plugin}")));
                self.draw_rows(rows, lines);
                lines.push(Self::row_line("[Install]  [Cancel]".to_string()));
            }
            Dialog::Trust {
                publisher,
                fingerprint,
            } => {
                let compact = compact_fingerprint(fingerprint);
                lines.push(Self::prose_line(format!("Do you trust the publisher {publisher}?")));
                lines.push(Self::prose_line(format!("Short form: {}", head_chars(&compact, 8))));
                lines.push(Self::prose_line("Full fingerprint:"));
                lines.push(Self::prose_line(format!("  {fingerprint}")));
                lines.push(Self::row_line("[Trust]  [Do not trust]".to_string()));
            }
            Dialog::KeyChange { plugin, old, new } => {
                let old_compact = compact_fingerprint(old);
                let new_compact = compact_fingerprint(new);
                lines.push(Self::prose_line(format!("The signing key of {plugin} changed.")));
                lines.push(Self::prose_line(format!("Old key: {}", head_chars(&old_compact, 8))));
                lines.push(Self::prose_line(format!("  {old}")));
                lines.push(Self::prose_line(format!(
                    "New key: {}\u{2026}{}",
                    head_chars(&new_compact, 4),
                    tail_chars(&new_compact, 4)
                )));
                lines.push(Self::prose_line(format!("  {new}")));
                lines.push(Self::row_line("[Trust the new key]  [Do not trust]".to_string()));
            }
            Dialog::Replace { plugin, replaced } => {
                lines.push(Self::prose_line(format!("{plugin} wants to replace {replaced}.")));
                lines.push(Self::row_line("[Replace]  [Install without replacing]".to_string()));
            }
            Dialog::SupportBundle { plugin } => {
                lines.push(Self::prose_line(format!("Build support bundle for {plugin}")));
                let failures = self.plugin(plugin).map_or(0, |entry| entry.failures);
                let settings = self.plugin(plugin).map_or(0, |entry| entry.settings.len());
                let sizes = [
                    ("crashes.txt", 40 + 120 * u64::from(failures)),
                    ("log.txt", 512 + 16 * plugin.len() as u64),
                    ("stats.txt", 96),
                    ("summary.txt", 200 + 24 * settings as u64),
                ];
                let mut total = 0;
                for (name, size) in sizes {
                    total += size;
                    lines.push(Self::row_line(format!("  {name:<14} {size} bytes")));
                }
                lines.push(Self::row_line(format!("Size: {total} bytes")));
                lines.push(Self::row_line("[Cancel]  [Build]".to_string()));
            }
            Dialog::Confirm { kind, plugin } => {
                let action = match kind {
                    ConfirmKind::ForgetSettings => "Forget settings of",
                    ConfirmKind::Uninstall => "Uninstall",
                };
                lines.push(Self::prose_line(format!("{action} {plugin}?")));
                lines.push(Self::row_line("[Yes]  [No]".to_string()));
            }
        }
    }

    fn speak_rows(&self, rows: &[ReviewRow]) -> Vec<String> {
        let total = rows.len();
        rows.iter()
            .enumerate()
            .map(|(index, row)| {
                let high = if grant_info(&row.grant).1 {
                    "high risk, "
                } else {
                    ""
                };
                let state = if row.locked {
                    "unavailable, blocked by your administrator"
                } else if row.checked {
                    "checked"
                } else {
                    "unchecked"
                };
                format!(
                    "{}, {high}checkbox, {state}, {} of {total}",
                    row.grant,
                    index + 1
                )
            })
            .collect()
    }
}

/// The notice for the first error of a malformed policy file (USR-212).
fn policy_error(key: &str, line: usize) -> String {
    format!(
        "System policy file is malformed: unknown key {key} on line {line}. Third-party plugins will not load."
    )
}


#[cfg(test)]
mod spec_tests {
    use super::*;

    /// A 64-character lowercase hexadecimal fingerprint in groups of four (USR-120).
    const HEX_FINGERPRINT: &str =
        "0123 4567 89ab cdef 0123 4567 89ab cdef 0123 4567 89ab cdef 0123 4567 89ab cdef";

    /// REQ: USR-014
    #[test]
    fn first_start_prompt_stops_after_third_not_now() {
        let mut rig = ScreensRig::new(100, 30);
        rig.start_client();
        let prompt_screen = rig.screen_text();
        rig.confirm_no();
        rig.stop_client();
        rig.start_client();
        rig.confirm_no();
        rig.stop_client();
        rig.start_client();
        rig.confirm_no();
        rig.stop_client();
        rig.start_client();
        assert_ne!(rig.screen_text(), prompt_screen);
    }

    /// REQ: USR-022, USR-012
    #[test]
    fn review_dialog_starts_high_rows_unchecked_and_records_defaults() {
        let mut rig = ScreensRig::new(100, 30);
        let grants = ["http:api.example.com", "terminal.write"];
        assert!(!rig.grant_is_high("http:api.example.com"));
        assert!(rig.grant_is_high("terminal.write"));
        rig.open_review_dialog("ilium.test.review", &grants);
        rig.press_enter();
        let grants_file = String::from_utf8_lossy(&rig.grants_file_bytes()).into_owned();
        assert_eq!(grants_file.matches("granted").count(), 1);
        assert_eq!(grants_file.matches("declined").count(), 1);
    }

    /// REQ: USR-031
    #[test]
    fn safe_mode_labels_third_party_rows_off_with_reason() {
        let mut rig = ScreensRig::new(100, 30);
        rig.install_plugin("ilium.test.third", "T2");
        rig.start_client_safe_mode();
        assert!(rig.screen_text().contains("safe-mode"));
        assert!(rig
            .plugin_state_word("ilium.test.third")
            .to_lowercase()
            .contains("off"));
        assert!(!rig.plugin_is_running("ilium.test.third"));
    }

    /// REQ: USR-041
    #[test]
    fn plugins_tab_refuses_to_switch_off_core_plugin_and_says_why() {
        let mut rig = ScreensRig::new(100, 30);
        rig.start_client();
        rig.open_plugins_tab();
        rig.select_plugin("ilium.core.tree");
        rig.press_space();
        assert!(rig.plugin_is_running("ilium.core.tree"));
        assert!(rig.last_notice().is_some_and(|text| !text.is_empty()));
    }

    /// REQ: USR-050, USR-051
    #[test]
    fn user_moved_run_order_wins_over_priority_and_survives_restart() {
        let mut rig = ScreensRig::new(100, 30);
        rig.start_client();
        rig.install_owner_with_point("ilium.test.owner", "Owner", "greet");
        rig.install_contributor("ilium.test.a", "ilium.test.owner", "greet", "hook_a", 10);
        rig.install_contributor("ilium.test.b", "ilium.test.owner", "greet", "hook_b", 20);
        assert_eq!(
            rig.run_order_of_point("ilium.test.owner", "greet"),
            vec!["ilium.test.a".to_string(), "ilium.test.b".to_string()]
        );
        rig.open_extensions_owner_view("ilium.test.owner");
        rig.move_selection_to("ilium.test.b");
        rig.activate("Move up");
        let expected = vec!["ilium.test.b".to_string(), "ilium.test.a".to_string()];
        assert_eq!(
            rig.run_order_of_point("ilium.test.owner", "greet"),
            expected
        );
        rig.restart_client();
        assert_eq!(
            rig.run_order_of_point("ilium.test.owner", "greet"),
            expected
        );
    }

    /// REQ: USR-053
    #[test]
    fn three_breaches_stop_extension_with_notice_and_not_on_disk() {
        let mut rig = ScreensRig::new(100, 30);
        rig.start_client();
        rig.install_owner_with_point("ilium.test.owner", "Owner", "greet");
        rig.install_contributor("ilium.test.c", "ilium.test.owner", "greet", "hook_c", 10);
        rig.breach_extension("ilium.test.c", "ilium.test.owner", "greet", 3);
        assert!(!rig
            .run_order_of_point("ilium.test.owner", "greet")
            .contains(&"ilium.test.c".to_string()));
        assert!(rig.last_notice().is_some());
        assert!(rig.screen_text().contains("Reset"));
        rig.restart_client();
        assert!(rig
            .run_order_of_point("ilium.test.owner", "greet")
            .contains(&"ilium.test.c".to_string()));
    }

    /// REQ: USR-080, USR-082
    #[test]
    fn capabilities_tab_shows_plain_wording_and_exact_grant_string() {
        let mut rig = ScreensRig::new(100, 30);
        rig.install_plugin_with_grants(
            "ilium.test.caps",
            &["terminal.write", "http:api.example.com"],
        );
        rig.open_capabilities_tab("ilium.test.caps");
        let wording = rig.plain_wording("http:api.example.com");
        assert!(!wording.is_empty());
        assert_ne!(wording, "http:api.example.com");
        assert!(rig.screen_text().contains(&wording));
        assert!(rig.screen_text().contains("http:api.example.com"));
        assert!(rig.grant_is_high("terminal.write"));
    }

    /// REQ: USR-087, USR-061
    #[test]
    fn clipboard_footer_indicator_lasts_three_seconds_and_needs_grant() {
        let mut rig = ScreensRig::new(100, 30);
        rig.start_client();
        rig.install_plugin_with_grants("ilium.test.clip", &["clipboard.read"]);
        rig.install_plugin("ilium.test.noclip", "T2");
        let quiet_footer = rig.footer_text();
        rig.plugin_reads_clipboard("ilium.test.noclip");
        assert_eq!(rig.footer_text(), quiet_footer);
        rig.plugin_reads_clipboard("ilium.test.clip");
        assert_ne!(rig.footer_text(), quiet_footer);
        rig.advance_ms(3000);
        assert_eq!(rig.footer_text(), quiet_footer);
    }

    /// REQ: USR-088
    #[test]
    fn toasts_capped_at_three_per_window_and_input_lock_ends_after_two_seconds() {
        let mut rig = ScreensRig::new(100, 30);
        rig.start_client();
        rig.install_plugin("ilium.test.toaster", "T2");
        for index in 0..10 {
            rig.plugin_toasts("ilium.test.toaster", &format!("toast-{index}"));
        }
        assert_eq!(rig.screen_text().matches("toast-").count(), 3);
        rig.plugin_locks_input("ilium.test.toaster");
        assert!(!rig.pane_received_input('x'));
        rig.advance_ms(2000);
        assert!(rig.pane_received_input('x'));
    }

    /// REQ: USR-120
    #[test]
    fn declining_unknown_signing_key_installs_nothing() {
        let mut rig = ScreensRig::new(100, 30);
        rig.start_client();
        let rows_before = rig.plugin_row_count();
        rig.open_trust_prompt("Example Publisher", HEX_FINGERPRINT);
        assert!(rig.screen_text().contains(HEX_FINGERPRINT));
        rig.confirm_no();
        assert_eq!(rig.trust_answers(), vec![false]);
        assert_eq!(rig.plugin_row_count(), rows_before);
    }
}
