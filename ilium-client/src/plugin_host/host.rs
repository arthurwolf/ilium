//! The real plugin host of the client (foundation contract sections 2.4, 2.5 and 12).
//!
//! The client runs the client sides of plugins on the host's own thread. The event loop never
//! waits for that thread: it posts work to it and collects what it produced (requests for the
//! server, toasts, redraws) from an outbox when the host signals. The host thread sees the
//! session only through a mirror the event loop refreshes, never through the `App`, and this
//! module touches no PTY, terminal parser or process table: the client still renders only what
//! the server sends.
//!
//! An intent of a client-side plugin is applied by the server. The facade sends it as a plugin
//! envelope and holds the plugin's call until the outcome envelope returns or a short timeout
//! passes, so a plugin is never told `ok` for something the server did not do.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use ilium_core::{
    ContainerKind as CoreContainerKind, NodeKind, PaneContentKind, SplitOrientation, Tree,
};
use ilium_ipc::plugin::{
    ContributionDelta, ContributionOp as WireOp, ControlBody, DeltaKind, EnvelopeBody,
    InstalledPlugin, PluginEnvelope, PluginHello, PluginMessage, Submission as WireSubmission,
    PLUGIN_PROTOCOL_VERSION,
};
use ilium_ipc::{ClientRequest, ServerEvent};
use ilium_plugin_api::generated::host::log::Level;
use ilium_plugin_api::generated::types::events::MessageEvent;
use ilium_plugin_api::generated::types::tree::{
    AgentActivity, AgentView, ContainerKind, NodeClass, TitleOrigin,
};
use ilium_plugin_api::generated::types::{
    ActionKind, Area, CommandContext, CommandOrigin, ErrorCode, Event, Intent, LinkActivatedEvent,
    NodeId, OpenViewSpec, Outcome, PluginError, PluginId, PluginSide, SettingsChange,
    SettingsOrigin, Tone, UiAction, UiTree, ViewTarget,
};
use ilium_plugin_api::kernel::{
    Cell, FacadeError, HelpCatalog, IntentResult, KernelFacade, KeyBinding, KeymapRead, Origin,
    PaneRead, TreeNode, TreeRead, UiSink, WorkspaceAccess,
};
use ilium_plugin_api::platform::ProcessKind;
use ilium_plugin_api::side::Side;
use ilium_plugin_host::contributions::Entry;
use ilium_plugin_host::engine::{EngineConfig, PluginEngine};
use ilium_plugin_host::imports::backed::LogSink;
use ilium_plugin_host::lifecycle::StartMode;
use ilium_plugin_host::runtime::cross_side::{RemoteMessage, REMOTE_ENVELOPE_PLUGIN};
use ilium_plugin_host::runtime::events_feed;
use ilium_plugin_host::runtime::{
    CataloguePlugin, HostRuntime, RuntimeConfig, RuntimeError, RuntimeThread,
};
use ilium_plugin_host::storage::StorageContext;
use ilium_plugin_host::wiring::{self, PluginRow};

/// How long a client-side plugin waits for the server's outcome of one intent.
const OUTCOME_WAIT: Duration = Duration::from_secs(2);
/// How long plugins get to deactivate when the client exits (contract section 2.4).
const SHUTDOWN_BUDGET: Duration = Duration::from_secs(2);
/// How long the client waits for the host thread to hand back its shutdown report.
const SHUTDOWN_WAIT: Duration = Duration::from_secs(3);
/// Outputs the event loop has not collected yet; beyond this the oldest are dropped.
const OUTBOX_LIMIT: usize = 1024;
/// The epoch tick of the client engine in milliseconds.
const EPOCH_TICK_MS: u64 = 10;

/// What the composition root hands the client to run plugins.
#[derive(Debug, Clone)]
pub struct ClientPluginStart {
    /// The plugins this build ships, manifests and client components.
    pub catalogue: Vec<CataloguePlugin>,
    /// The user data directory (`<data_dir>/plugins` holds the registry, storage and cache).
    pub data_dir: PathBuf,
    /// Normal, safe or no plugins. `NoPlugins` starts no host at all.
    pub mode: StartMode,
    /// The help topics the plugins of the catalogue ship (contract section 20.2).
    pub help: Vec<PluginHelpTopic>,
    /// The schema files of the services the catalogue provides: (service, file text). Every
    /// service call is checked against them (gap G13).
    pub schemas: Vec<(String, String)>,
}

/// One help topic a plugin ships. Its global id is `<plugin>/<id>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginHelpTopic {
    /// The plugin id.
    pub plugin: String,
    /// The topic id inside the plugin.
    pub id: String,
    /// The title of the manifest entry.
    pub title: String,
    /// The Markdown text of the help file.
    pub markdown: String,
}

impl ClientPluginStart {
    /// What the client tells the server about its plugins when it attaches.
    pub fn hello(&self) -> PluginHello {
        PluginHello {
            plugin_protocol: PLUGIN_PROTOCOL_VERSION,
            contract: ilium_plugin_api::CONTRACT.to_string(),
            installed: self
                .catalogue
                .iter()
                .map(|plugin| InstalledPlugin {
                    id: plugin.id.clone(),
                    version: wiring::manifest_version(&plugin.manifest),
                    // The content hash is filled once packages are verified at start
                    // (foundation contract section 12.4); bundled plugins match by build.
                    content_hash: [0; 32],
                })
                .collect(),
        }
    }
}

/// What the host thread asks the event loop to do.
#[derive(Debug)]
pub(crate) enum Output {
    /// Send this to the server.
    Request(ClientRequest),
    /// Show a toast a plugin asked for (toast spec as JSON).
    Toast { plugin: String, toast_json: String },
    /// A plugin view changed.
    Redraw,
    /// A plugin opened a popup; the event loop gives it the screen if no other modal has it.
    PopupOpened { plugin: String, popup: String },
    /// The open popup of a plugin went away (the plugin closed it or its view failed).
    PopupClosed { plugin: String, popup: String },
    /// A plugin opened a panel, a settings page or an onboarding step; `focus` asks for the
    /// keyboard. The event loop decides where the view is shown.
    ViewOpened {
        plugin: String,
        view: ViewTarget,
        focus: bool,
    },
    /// A view of a plugin went away (closed by the plugin, failed to render, or the plugin
    /// was disabled).
    ViewClosed { plugin: String, view: ViewTarget },
    /// A client-local intent of a plugin: focus, attention, an external open, a toast dismissal
    /// (gap G3). The event loop carries it out (`effects_app`).
    Effect {
        plugin: String,
        effect: super::effects::ClientEffect,
    },
    /// A plugin reads or writes the clipboard (`ilium:host/clipboard`); the event loop runs it
    /// on the client's clipboard service and answers on `reply`.
    Clipboard {
        plugin: String,
        request: super::clipboard_backend::PluginClipboardRequest,
        reply: mpsc::Sender<super::clipboard_backend::PluginClipboardReply>,
    },
    /// A plugin asks the user for a path (`fs.pick`); the event loop shows the file picker and
    /// answers on `reply`, `None` when the user dismisses it. Dropping `reply` also dismisses.
    PickPath {
        plugin: String,
        spec: ilium_plugin_api::generated::host::fs::PickSpec,
        reply: mpsc::Sender<Option<PathBuf>>,
    },
}

/// How long a backend polls between checks that the host is still running while it waits for
/// the event loop.
const ASK_POLL: Duration = Duration::from_millis(50);

/// What came of a question a backend put to the event loop ([`OutputPort::ask`]).
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Asked<T> {
    /// The event loop answered.
    Answered(T),
    /// No answer within the time limit.
    TimedOut,
    /// The event loop dropped the question without an answer.
    Dropped,
    /// The host is stopping or the caller gave up.
    Stopped,
}

/// Lets a backend on the plugin runtime thread hand an [`Output`] to the event loop and wait
/// for its answer. Holds the shared state weakly, so a backend kept by an instance never keeps
/// the host alive.
#[derive(Clone)]
pub(crate) struct OutputPort {
    shared: std::sync::Weak<Shared>,
}

impl std::fmt::Debug for OutputPort {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("OutputPort")
    }
}

impl OutputPort {
    /// Whether the host is stopping (or gone).
    pub(crate) fn stopping(&self) -> bool {
        self.shared
            .upgrade()
            .is_none_or(|shared| shared.stopping.load(Ordering::Acquire))
    }

    /// Hands `output` to the event loop; `false` when the host is stopping.
    fn send(&self, output: Output) -> bool {
        match self.shared.upgrade() {
            Some(shared) if !shared.stopping.load(Ordering::Acquire) => {
                shared.push(output);
                true
            }
            _ => false,
        }
    }

    /// Asks the event loop with the output `question` builds around a reply channel, and blocks
    /// until it answers, `limit` passes, the host stops or `gave_up` turns true.
    pub(crate) fn ask<T>(
        &self,
        question: impl FnOnce(mpsc::Sender<T>) -> Output,
        limit: Option<Duration>,
        gave_up: &dyn Fn() -> bool,
    ) -> Asked<T> {
        let (reply, answer) = mpsc::channel();
        if !self.send(question(reply)) {
            return Asked::Stopped;
        }
        let started = std::time::Instant::now();
        loop {
            match answer.recv_timeout(ASK_POLL) {
                Ok(value) => return Asked::Answered(value),
                Err(mpsc::RecvTimeoutError::Disconnected) => return Asked::Dropped,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if self.stopping() || gave_up() {
                return Asked::Stopped;
            }
            if limit.is_some_and(|limit| started.elapsed() >= limit) {
                return Asked::TimedOut;
            }
        }
    }
}

/// A panel, settings page or onboarding step a plugin has open, with the tree the plugin last
/// rendered for it and the cells the client gives it.
#[derive(Debug, Clone)]
struct ViewSlot {
    plugin: String,
    view: ViewTarget,
    tree: Option<UiTree>,
    area: (u16, u16),
}

/// One status-bar segment a plugin set (contract section 19.1). Text is not sanitized here;
/// the drawing code does that.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StatusSegment {
    pub plugin: String,
    pub segment: String,
    pub text: String,
    pub tone: Option<Tone>,
}

/// One decoration a plugin set on a tree node (contract section 19.1).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TreeDecoration {
    pub plugin: String,
    pub node: NodeId,
    pub slot: String,
    pub text: String,
    pub icon: Option<String>,
    pub sort_key: i64,
}

/// One settings row a plugin contributes (contract section 20.6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ContributedSetting {
    pub plugin: String,
    /// The title of the page (`tab` or `section`) the row belongs to.
    pub page: String,
    /// The settings key inside the plugin.
    pub key: String,
    pub title: String,
    /// The row type of the manifest (`toggle`, `number`, `choice`, ...).
    pub row_type: String,
    /// The default value as display text.
    pub default: String,
}

/// The settings rows of one manifest.
pub(crate) fn contributed_settings(plugin: &str, manifest: &str) -> Vec<ContributedSetting> {
    let Ok(manifest) = serde_json::from_str::<serde_json::Value>(manifest) else {
        return Vec::new();
    };
    let text = |entry: &serde_json::Value, field: &str| {
        entry
            .get(field)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    };
    let mut rows = Vec::new();
    let pages = manifest
        .get("contributes")
        .and_then(|contributes| contributes.get("settings"))
        .and_then(serde_json::Value::as_array);
    for page in pages.into_iter().flatten() {
        let page_title = text(page, "title")
            .or_else(|| text(page, "id"))
            .unwrap_or_default();
        // A page holds rows directly, or sections that each hold rows.
        let direct = page.get("rows").and_then(serde_json::Value::as_array);
        let sectioned = page
            .get("sections")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|section| section.get("rows").and_then(serde_json::Value::as_array))
            .flatten();
        for row in direct.into_iter().flatten().chain(sectioned) {
            let Some(key) = text(row, "key") else {
                continue;
            };
            let unit = text(row, "unit").unwrap_or_default();
            let default = match row.get("default") {
                Some(serde_json::Value::Bool(true)) => "on".to_owned(),
                Some(serde_json::Value::Bool(false)) => "off".to_owned(),
                Some(serde_json::Value::String(value)) => value.clone(),
                Some(serde_json::Value::Null) | None => "not set".to_owned(),
                Some(other) => format!("{other} {unit}").trim_end().to_owned(),
            };
            rows.push(ContributedSetting {
                plugin: plugin.to_owned(),
                page: page_title.clone(),
                title: text(row, "title").unwrap_or_else(|| key.clone()),
                row_type: text(row, "type").unwrap_or_else(|| "value".to_owned()),
                key,
                default,
            });
        }
    }
    rows
}

/// One entry of the static chrome a manifest contributes: a toolbar button, a menu entry, a
/// row action, a panel, a settings page or an onboarding step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChromeItem {
    pub plugin: String,
    /// The id inside the plugin.
    pub id: String,
    /// The label or title; the id when the manifest gives none.
    pub title: String,
    pub icon: Option<String>,
    /// The full command id (`<plugin>/<command>`), for entries that run one.
    pub command: Option<String>,
    /// What the entry applies to (`target` or `applies_to` of a menu entry), if stated.
    pub target: Option<String>,
    /// The `order` of an onboarding step; 0 when the manifest gives none.
    pub order: i64,
    /// The popup a toolbar button opens (`popup_view`), if it names one.
    pub popup_view: Option<String>,
    /// The decoration slot a tree ordering sorts by (`slot`), if it names one.
    pub slot: Option<String>,
    /// Where a pane frame slot sits (`placement`: `header-icon`, `header-text`,
    /// `footer-row`, `banner`), if it says.
    pub placement: Option<String>,
    /// The `when` context expression that hides the entry outside its context, if any
    /// (evaluated by `plugin_host::when`).
    pub when: Option<String>,
    /// The short text a tree toolbar button shows beside its icon when the sidebar is wide
    /// (`caption`, at most 12 cells), if it gives one.
    pub caption: Option<String>,
}

/// The entries of one contribution kind in a manifest. A manifest that does not parse, or an
/// entry without an id, contributes nothing.
pub(crate) fn chrome_items(plugin: &str, manifest: &str, kind: &str) -> Vec<ChromeItem> {
    let Ok(manifest) = serde_json::from_str::<serde_json::Value>(manifest) else {
        return Vec::new();
    };
    let text = |entry: &serde_json::Value, field: &str| {
        entry
            .get(field)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    };
    manifest
        .get("contributes")
        .and_then(|contributes| contributes.get(kind))
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let id = text(entry, "id")?;
            Some(ChromeItem {
                plugin: plugin.to_owned(),
                title: text(entry, "title")
                    .or_else(|| text(entry, "label"))
                    .unwrap_or_else(|| id.clone()),
                icon: text(entry, "icon"),
                // A command of the same plugin may be written without the plugin prefix.
                command: text(entry, "command").map(|command| {
                    if command.contains('/') {
                        command
                    } else {
                        format!("{plugin}/{command}")
                    }
                }),
                target: text(entry, "target").or_else(|| {
                    // `applies_to` is a list of node kinds; it is kept as `a,b`.
                    entry
                        .get("applies_to")
                        .and_then(serde_json::Value::as_array)
                        .map(|kinds| {
                            kinds
                                .iter()
                                .filter_map(serde_json::Value::as_str)
                                .collect::<Vec<_>>()
                                .join(",")
                        })
                        // A frame slot may state it as one word (`all`, `agents`, ...).
                        .or_else(|| text(entry, "applies_to"))
                }),
                order: entry
                    .get("order")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(0),
                popup_view: text(entry, "popup_view"),
                slot: text(entry, "slot"),
                placement: text(entry, "placement"),
                when: text(entry, "when"),
                caption: text(entry, "caption"),
                id,
            })
        })
        .collect()
}

/// The one popup a plugin has open, with the view the plugin last rendered for it.
#[derive(Debug, Clone)]
struct PopupSlot {
    plugin: String,
    popup: String,
    tree: Option<UiTree>,
}

/// State the host thread and the event loop share. Every lock is held for a few statements
/// and never across a call into the runtime.
struct Shared {
    nodes: Mutex<Vec<TreeNode>>,
    focused: Mutex<Option<NodeId>>,
    epoch: AtomicU64,
    outbox: Mutex<VecDeque<Output>>,
    notification: Arc<tokio::sync::Notify>,
    waiting: Mutex<HashMap<u64, mpsc::Sender<Outcome>>>,
    next_seq: AtomicU64,
    stopping: AtomicBool,
    panels: Mutex<BTreeMap<(String, String), String>>,
    /// The open plugin popup, if any. One at a time: the kernel has one modal layer.
    popup: Mutex<Option<PopupSlot>>,
    /// The panels, settings pages and onboarding steps plugins have open.
    views: Mutex<Vec<ViewSlot>>,
    /// Status-bar segments, in the order plugins first set them.
    segments: Mutex<Vec<StatusSegment>>,
    /// Tree decorations of client-side plugins.
    decorations: Mutex<Vec<TreeDecoration>>,
    /// Pane frame slot contents client-side plugins set (`set-frame-slot`), one per
    /// (plugin, pane, slot). Kept in the decoration shape: a pane plus a slot id plus text.
    frame_slots: Mutex<Vec<TreeDecoration>>,
    /// The sound catalogue, play rate and mute of client plugins (`play-sound`).
    sounds: Mutex<super::sound_gate::SoundGate>,
    /// The resolved key map, as the event loop last mirrored it.
    keymap: Mutex<Vec<KeyBinding>>,
    /// Columns and rows of the screen, as the event loop last reported them.
    screen: Mutex<(u16, u16)>,
    runtime: Mutex<Option<RuntimeThread>>,
    project_dir: String,
    session_name: String,
    /// The help topics plugins ship, by global id `<plugin>/<topic>`.
    help: BTreeMap<String, PluginHelpTopic>,
    /// The plugins the server last reported as disabled; their help topics are hidden.
    disabled: Mutex<std::collections::BTreeSet<String>>,
    /// The kernel settings tables: the user's `config.toml` (contract section 20.3).
    settings: ilium_plugin_host::config_settings::ConfigFileSettings,
    /// This record itself, so a job posted to the runtime can send what the runtime wants
    /// the server to hear (cross-side topics and service calls, gap G4) when it ends.
    me: std::sync::Weak<Shared>,
    /// The key grabs of the runtime's plugins, shared with it once it runs (gap G1).
    key_grabs: Mutex<Option<ilium_plugin_host::imports::keys::KeyGrabs>>,
}

impl Shared {
    /// Whether the server last reported this plugin as disabled.
    fn is_disabled(&self, plugin: &str) -> bool {
        self.disabled
            .lock()
            .is_ok_and(|disabled| disabled.contains(plugin))
    }

    /// The help topic `<plugin>/<topic>` of an enabled plugin.
    fn plugin_help(&self, id: &str) -> Option<&PluginHelpTopic> {
        self.help
            .get(id)
            .filter(|topic| !self.is_disabled(&topic.plugin))
    }

    fn push(&self, output: Output) {
        if let Ok(mut outbox) = self.outbox.lock() {
            if outbox.len() >= OUTBOX_LIMIT {
                outbox.pop_front();
            }
            outbox.push_back(output);
        }
        self.notification.notify_one();
    }

    fn envelope(&self, plugin: &str, body: EnvelopeBody) -> (u64, ClientRequest) {
        let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);
        (
            seq,
            ClientRequest::Plugin(PluginEnvelope {
                plugin: plugin.to_owned(),
                seq,
                body,
            }),
        )
    }

    fn post(&self, job: impl FnOnce(&mut HostRuntime) + Send + 'static) -> bool {
        let me = self.me.clone();
        self.runtime.lock().ok().is_some_and(|slot| {
            slot.as_ref().is_some_and(|thread| {
                thread.post(move |runtime| {
                    job(runtime);
                    if let Some(shared) = me.upgrade() {
                        shared.forward_remote(runtime);
                    }
                })
            })
        })
    }

    /// Classifies a client-local intent (gap G3) and hands its effect to the event loop.
    /// Gives the intent back (`Err`) when it is not client-local.
    fn apply_client_effect(&self, plugin: &str, intent: Intent) -> Result<Outcome, Intent> {
        let home_dir = directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf());
        match super::effects::classify(
            intent,
            std::path::Path::new(&self.project_dir),
            home_dir.as_deref(),
        ) {
            super::effects::Classified::Effect(effect) => {
                // Client plugins hear about attention marks (gap G2).
                let attention = match &effect {
                    super::effects::ClientEffect::MarkAttention { node, reason } => {
                        Some(events_feed::attention_event(
                            to_plugin(*node),
                            Some(reason.clone()),
                            Some(plugin),
                        ))
                    }
                    super::effects::ClientEffect::AckAttention(node) => Some(
                        events_feed::attention_event(to_plugin(*node), None, Some(plugin)),
                    ),
                    _ => None,
                };
                self.push(Output::Effect {
                    plugin: plugin.to_owned(),
                    effect,
                });
                if let Some(event) = attention {
                    self.post(move |runtime| {
                        log_errors("event delivery", runtime.broadcast(&[event]));
                    });
                }
                Ok(Outcome::Ok)
            }
            super::effects::Classified::Toast(toast_json) => {
                self.push(Output::Toast {
                    plugin: plugin.to_owned(),
                    toast_json,
                });
                Ok(Outcome::Ok)
            }
            super::effects::Classified::Refused(outcome) => Ok(outcome),
            super::effects::Classified::Server(intent) => Err(intent),
        }
    }

    /// Carries out an intent a server plugin forwarded: opens or closes one of its views, or
    /// carries out a client-local effect. Returns whether the screen changed.
    fn apply_forwarded(&self, plugin: &str, intent: Intent) -> bool {
        let intent = match self.apply_client_effect(plugin, intent) {
            Ok(outcome) => {
                if let Outcome::Refused(error) | Outcome::Failed(error) = outcome {
                    tracing::debug!("a forwarded plugin intent was refused: {}", error.message);
                }
                return false;
            }
            Err(intent) => intent,
        };
        match intent {
            Intent::OpenView(OpenViewSpec {
                view: ViewTarget::Popup(popup),
                ..
            }) => {
                self.open_popup(plugin, popup);
                true
            }
            Intent::CloseView(ViewTarget::Popup(popup)) => self.close_popup(plugin, &popup),
            Intent::OpenView(OpenViewSpec { view, focus })
                if !matches!(view, ViewTarget::Pane(_)) =>
            {
                self.open_view(plugin, view, focus);
                true
            }
            Intent::CloseView(view) if !matches!(view, ViewTarget::Pane(_)) => {
                self.close_view(plugin, &view)
            }
            other => {
                tracing::debug!("a forwarded plugin intent is not client-local: {other:?}");
                false
            }
        }
    }

    /// Sends what the client runtime wants the server runtime to hear: topic messages of its
    /// plugins and service calls for providers that live on the server (gap G4).
    fn forward_remote(&self, runtime: &mut HostRuntime) {
        for message in runtime.take_remote_outbound() {
            let body = super::remote::body_of(message.to_wire());
            let (_, request) = self.envelope(REMOTE_ENVELOPE_PLUGIN, body);
            self.push(Output::Request(request));
        }
    }

    /// Records the popup a plugin opened and tells the event loop.
    fn open_popup(&self, plugin: &str, popup: String) {
        if let Ok(mut slot) = self.popup.lock() {
            *slot = Some(PopupSlot {
                plugin: plugin.to_owned(),
                popup: popup.clone(),
                tree: None,
            });
        }
        self.push(Output::PopupOpened {
            plugin: plugin.to_owned(),
            popup,
        });
    }

    /// Forgets the popup if it is the open one. Returns whether it was.
    fn close_popup(&self, plugin: &str, popup: &str) -> bool {
        let was_open = self.popup.lock().is_ok_and(|mut slot| {
            let is_open = slot
                .as_ref()
                .is_some_and(|open| open.plugin == plugin && open.popup == popup);
            if is_open {
                *slot = None;
            }
            is_open
        });
        if was_open {
            self.push(Output::PopupClosed {
                plugin: plugin.to_owned(),
                popup: popup.to_owned(),
            });
        }
        was_open
    }

    /// Records a view a plugin opened (or the kernel opened for it) and tells the event loop.
    /// Opening a view that is already open keeps its tree.
    fn open_view(&self, plugin: &str, view: ViewTarget, focus: bool) {
        if let Ok(mut views) = self.views.lock() {
            if !views
                .iter()
                .any(|open| open.plugin == plugin && open.view == view)
            {
                views.push(ViewSlot {
                    plugin: plugin.to_owned(),
                    view: view.clone(),
                    tree: None,
                    area: (0, 0),
                });
            }
        }
        self.push(Output::ViewOpened {
            plugin: plugin.to_owned(),
            view,
            focus,
        });
    }

    /// Forgets a view if it is open. Returns whether it was.
    fn close_view(&self, plugin: &str, view: &ViewTarget) -> bool {
        let was_open = self.views.lock().is_ok_and(|mut views| {
            let before = views.len();
            views.retain(|open| !(open.plugin == plugin && &open.view == view));
            views.len() != before
        });
        if was_open {
            self.push(Output::ViewClosed {
                plugin: plugin.to_owned(),
                view: view.clone(),
            });
        }
        was_open
    }

    /// Asks the owning plugins for the current trees of every open view that has cells. Runs
    /// on the host thread, never from inside a plugin call. A view that fails to render is
    /// closed, so a broken plugin never leaves a stale picture (COR-218).
    fn refresh_views(&self, runtime: &mut HostRuntime) {
        let open: Vec<(String, ViewTarget, (u16, u16))> = self
            .views
            .lock()
            .map(|views| {
                views
                    .iter()
                    .filter(|slot| slot.area.0 > 0 && slot.area.1 > 0)
                    .map(|slot| (slot.plugin.clone(), slot.view.clone(), slot.area))
                    .collect()
            })
            .unwrap_or_default();
        if open.is_empty() {
            return;
        }
        for (plugin, view, (columns, rows)) in open {
            match runtime.render(&plugin, view.clone(), Area { columns, rows }) {
                Ok(tree) => {
                    if let Ok(mut views) = self.views.lock() {
                        if let Some(slot) = views
                            .iter_mut()
                            .find(|slot| slot.plugin == plugin && slot.view == view)
                        {
                            slot.tree = Some(tree);
                        }
                    }
                }
                Err(error) => {
                    tracing::warn!("view {view:?} of {plugin} did not render: {error}");
                    self.close_view(&plugin, &view);
                }
            }
        }
        self.push(Output::Redraw);
    }

    /// Drops everything a plugin shows: its views, popup, status segments and decorations.
    /// Called when the plugin is disabled, so nothing of it stays on screen.
    fn forget_plugin(&self, plugin: &str) {
        let views: Vec<ViewTarget> = self
            .views
            .lock()
            .map(|views| {
                views
                    .iter()
                    .filter(|slot| slot.plugin == plugin)
                    .map(|slot| slot.view.clone())
                    .collect()
            })
            .unwrap_or_default();
        for view in views {
            self.close_view(plugin, &view);
        }
        let popup = self.popup.lock().ok().and_then(|slot| {
            slot.as_ref()
                .filter(|open| open.plugin == plugin)
                .map(|open| open.popup.clone())
        });
        if let Some(popup) = popup {
            self.close_popup(plugin, &popup);
        }
        if let Ok(mut segments) = self.segments.lock() {
            segments.retain(|segment| segment.plugin != plugin);
        }
        if let Ok(mut decorations) = self.decorations.lock() {
            decorations.retain(|decoration| decoration.plugin != plugin);
        }
        if let Ok(mut frame_slots) = self.frame_slots.lock() {
            frame_slots.retain(|content| content.plugin != plugin);
        }
        if let Ok(mut sounds) = self.sounds.lock() {
            sounds.forget(plugin);
        }
        self.push(Output::Redraw);
    }

    /// Plays one `play-sound` of a client plugin through the client's audio output, after
    /// the catalogue, rate and mute checks of [`super::sound_gate`]. `ok` means the sound
    /// was handed to the audio output: playback runs on its own thread, so a slow player
    /// never holds up the plugin host, and a failing device is only logged.
    fn play_sound(&self, plugin: &str, sound: &str) -> Outcome {
        let decision = match self.sounds.lock() {
            Ok(mut gate) => gate.decide(plugin, sound, std::time::Instant::now()),
            Err(_) => {
                return Outcome::Failed(PluginError::new(
                    ErrorCode::Internal,
                    "the sound gate is unavailable",
                ))
            }
        };
        match decision {
            super::sound_gate::SoundDecision::Play(wav) => {
                let plugin_id = plugin.to_owned();
                let spawned = std::thread::Builder::new()
                    .name("ilium-plugin-sound".to_owned())
                    .spawn(move || {
                        let cancel = ilium_plugin_host::imports::env::CancelFlag::default();
                        if let Err(error) = ilium_plugin_host::imports::audio::AudioBackend::play(
                            &super::audio_backend::ClientAudio,
                            wav,
                            &cancel,
                        ) {
                            tracing::debug!(plugin = %plugin_id, "plugin sound did not play: {}", error.message);
                        }
                    });
                match spawned {
                    Ok(_) => Outcome::Ok,
                    Err(error) => {
                        Outcome::Failed(PluginError::new(ErrorCode::Io, error.to_string()))
                    }
                }
            }
            super::sound_gate::SoundDecision::Muted => Outcome::Ok,
            super::sound_gate::SoundDecision::Refused(error) => Outcome::Refused(error),
            super::sound_gate::SoundDecision::Failed(error) => Outcome::Failed(error),
        }
    }

    fn set_screen(&self, screen: (u16, u16)) {
        if let Ok(mut slot) = self.screen.lock() {
            *slot = screen;
        }
    }

    /// Asks the owning plugin for the current view of the open popup. Runs on the host
    /// thread after anything that may have changed the plugin's state; never called from
    /// inside a plugin call. A view that fails to render closes the popup.
    fn refresh_popup(&self, runtime: &mut HostRuntime) {
        let Some((plugin, popup)) = self.popup.lock().ok().and_then(|slot| {
            slot.as_ref()
                .map(|open| (open.plugin.clone(), open.popup.clone()))
        }) else {
            return;
        };
        let (columns, rows) = self.screen.lock().map(|screen| *screen).unwrap_or((0, 0));
        match runtime.render(
            &plugin,
            ViewTarget::Popup(popup.clone()),
            Area { columns, rows },
        ) {
            Ok(tree) => {
                if let Ok(mut slot) = self.popup.lock() {
                    if let Some(open) = slot
                        .as_mut()
                        .filter(|open| open.plugin == plugin && open.popup == popup)
                    {
                        open.tree = Some(tree);
                    }
                }
                self.push(Output::Redraw);
            }
            Err(error) => {
                tracing::warn!("popup {popup} of {plugin} did not render: {error}");
                self.close_popup(&plugin, &popup);
            }
        }
    }
}

fn failed(code: ErrorCode, message: &str) -> Outcome {
    Outcome::Failed(PluginError::new(code, message))
}

/// The kernel as client-side plugins see it.
struct ClientFacade {
    shared: Arc<Shared>,
}

impl ClientFacade {
    /// Sends an intent to the server and waits for its outcome.
    fn submit(&self, plugin: &str, intent: &Intent) -> Outcome {
        if self.shared.stopping.load(Ordering::Acquire) {
            return Outcome::Cancelled;
        }
        let intent_json = match serde_json::to_string(intent) {
            Ok(text) => text,
            Err(error) => {
                return failed(
                    ErrorCode::Internal,
                    &format!("the intent does not serialize: {error}"),
                );
            }
        };
        let seq = self.shared.next_seq.fetch_add(1, Ordering::Relaxed);
        let (reply, outcome) = mpsc::channel();
        match self.shared.waiting.lock() {
            Ok(mut waiting) => {
                waiting.insert(seq, reply);
            }
            Err(_) => return failed(ErrorCode::Internal, "the plugin host state is poisoned"),
        }
        self.shared
            .push(Output::Request(ClientRequest::Plugin(PluginEnvelope {
                plugin: plugin.to_owned(),
                seq,
                body: EnvelopeBody::Submit(WireSubmission {
                    client_id: String::new(),
                    dedup_key: seq.to_string(),
                    intent_json,
                }),
            })));
        let result = outcome.recv_timeout(OUTCOME_WAIT);
        if let Ok(mut waiting) = self.shared.waiting.lock() {
            waiting.remove(&seq);
        }
        result.unwrap_or_else(|_| {
            failed(
                ErrorCode::TimedOut,
                "the server did not answer the intent in time; it may still be applied",
            )
        })
    }

    fn message(&self, plugin: &PluginId, intent: Intent) -> Outcome {
        let Intent::Message(spec) = intent else {
            return failed(ErrorCode::Internal, "not a message");
        };
        if spec.to.plugin.0 != plugin.0 {
            return Outcome::Refused(PluginError::new(
                ErrorCode::NotPermitted,
                "a message intent reaches only the sender's own other side",
            ));
        }
        match spec.to.side {
            PluginSide::Server => {
                let (_, request) = self.shared.envelope(
                    &plugin.0,
                    EnvelopeBody::Message(PluginMessage {
                        topic: spec.channel,
                        payload: spec.payload,
                    }),
                );
                self.shared.push(Output::Request(request));
                Outcome::Ok
            }
            PluginSide::Client => {
                let target = plugin.0.clone();
                let event = Event::Message(MessageEvent {
                    from: plugin.clone(),
                    from_side: PluginSide::Client,
                    from_client: None,
                    channel: spec.channel,
                    payload: spec.payload,
                });
                // Queued, never delivered inline: this call already runs inside the runtime.
                let posted = self.shared.post(move |runtime| {
                    if let Err(error) = runtime.deliver(&target, vec![event]) {
                        tracing::debug!("plugin message not delivered: {error}");
                    }
                    log_errors("pump", runtime.pump());
                });
                if posted {
                    Outcome::Ok
                } else {
                    failed(ErrorCode::Busy, "the plugin host is stopping")
                }
            }
        }
    }
}

impl KernelFacade for ClientFacade {
    fn apply_intent(&self, origin: &Origin, intent: Intent) -> IntentResult {
        let Origin::Plugin { plugin, .. } = origin else {
            return Outcome::Refused(PluginError::new(
                ErrorCode::NotPermitted,
                "the client kernel applies intents of plugins only",
            ));
        };
        match intent {
            Intent::Message(_) => self.message(plugin, intent),
            // A popup is client-local: the kernel's modal layer shows it, the server never
            // hears of it. Other views still go to the server until their seams exist.
            Intent::OpenView(OpenViewSpec {
                view: ViewTarget::Popup(popup),
                ..
            }) => {
                self.shared.open_popup(&plugin.0, popup);
                Outcome::Ok
            }
            Intent::CloseView(ViewTarget::Popup(popup)) => {
                self.shared.close_popup(&plugin.0, &popup);
                Outcome::Ok
            }
            // Panels, settings pages and onboarding steps of a client side are client-local
            // too (contract section 19.1). A pane view belongs to the tree, so the server
            // decides.
            Intent::OpenView(OpenViewSpec {
                view:
                    view @ (ViewTarget::Panel(_)
                    | ViewTarget::SettingsPage(_)
                    | ViewTarget::OnboardingStep(_)),
                focus,
            }) => {
                self.shared.open_view(&plugin.0, view, focus);
                Outcome::Ok
            }
            Intent::CloseView(
                view @ (ViewTarget::Panel(_)
                | ViewTarget::SettingsPage(_)
                | ViewTarget::OnboardingStep(_)),
            ) => {
                self.shared.close_view(&plugin.0, &view);
                Outcome::Ok
            }
            Intent::SetStatusSegment(spec) => {
                let text: String = spec.content.iter().map(|span| span.text.as_str()).collect();
                if let Ok(mut segments) = self.shared.segments.lock() {
                    match segments.iter_mut().find(|segment| {
                        segment.plugin == plugin.0 && segment.segment == spec.segment
                    }) {
                        Some(segment) => {
                            segment.text = text;
                            segment.tone = spec.tone;
                        }
                        None => segments.push(StatusSegment {
                            plugin: plugin.0.clone(),
                            segment: spec.segment,
                            text,
                            tone: spec.tone,
                        }),
                    }
                }
                self.shared.push(Output::Redraw);
                Outcome::Ok
            }
            Intent::ClearStatusSegment(name) => {
                if let Ok(mut segments) = self.shared.segments.lock() {
                    segments
                        .retain(|segment| !(segment.plugin == plugin.0 && segment.segment == name));
                }
                self.shared.push(Output::Redraw);
                Outcome::Ok
            }
            Intent::SetDecoration(spec) => {
                let decoration = TreeDecoration {
                    plugin: plugin.0.clone(),
                    node: spec.node,
                    slot: spec.slot,
                    text: spec.content.iter().map(|span| span.text.as_str()).collect(),
                    icon: spec.icon,
                    sort_key: spec.sort_key.unwrap_or(0),
                };
                if let Ok(mut decorations) = self.shared.decorations.lock() {
                    decorations.retain(|known| {
                        !(known.plugin == decoration.plugin
                            && known.node == decoration.node
                            && known.slot == decoration.slot)
                    });
                    decorations.push(decoration);
                    decorations.sort_by_key(|known| known.sort_key);
                }
                self.shared.push(Output::Redraw);
                Outcome::Ok
            }
            Intent::ClearDecoration(key) => {
                if let Ok(mut decorations) = self.shared.decorations.lock() {
                    decorations.retain(|known| {
                        !(known.plugin == plugin.0
                            && known.node == key.node
                            && known.slot == key.slot)
                    });
                }
                self.shared.push(Output::Redraw);
                Outcome::Ok
            }
            // A pane frame slot is drawn by this client only, so its content stays here
            // (contract `pane_frame_slots`); the server never draws a frame.
            // A client plugin's sound comes out of this client's audio output; the server
            // never hears of it.
            Intent::PlaySound(sound) => self.shared.play_sound(&plugin.0, &sound),
            Intent::SetFrameSlot(spec) => {
                let content = TreeDecoration {
                    plugin: plugin.0.clone(),
                    node: spec.pane,
                    slot: spec.slot,
                    text: spec.content.iter().map(|span| span.text.as_str()).collect(),
                    icon: spec.icon,
                    sort_key: 0,
                };
                if let Ok(mut frame_slots) = self.shared.frame_slots.lock() {
                    frame_slots.retain(|known| {
                        !(known.plugin == content.plugin
                            && known.node == content.node
                            && known.slot == content.slot)
                    });
                    frame_slots.push(content);
                }
                self.shared.push(Output::Redraw);
                Outcome::Ok
            }
            Intent::ClearFrameSlot(key) => {
                if let Ok(mut frame_slots) = self.shared.frame_slots.lock() {
                    frame_slots.retain(|known| {
                        !(known.plugin == plugin.0
                            && known.node == key.pane
                            && known.slot == key.slot)
                    });
                }
                self.shared.push(Output::Redraw);
                Outcome::Ok
            }
            // Attention, toasts, external opens and kernel commands are client-local (gap
            // G3); everything else goes to the server.
            other => match self.shared.apply_client_effect(&plugin.0, other) {
                Ok(outcome) => outcome,
                Err(intent) => self.submit(&plugin.0, &intent),
            },
        }
    }

    fn tree(&self) -> &dyn TreeRead {
        self
    }

    fn panes(&self) -> &dyn PaneRead {
        self
    }

    fn process_kind(&self) -> ProcessKind {
        ProcessKind::Client
    }

    fn workspace(&self) -> Option<&dyn WorkspaceAccess> {
        Some(self)
    }

    fn ui(&self) -> Option<&dyn UiSink> {
        Some(self)
    }

    fn keymap(&self) -> Option<&dyn KeymapRead> {
        Some(self)
    }

    fn settings(&self) -> Option<&dyn ilium_plugin_api::kernel::KernelSettings> {
        Some(&self.shared.settings)
    }

    fn help(&self) -> Option<&dyn HelpCatalog> {
        Some(self)
    }

    fn focused_pane(&self) -> Option<NodeId> {
        self.shared.focused.lock().ok().and_then(|focused| *focused)
    }
}

/// The prefix of the kernel's own help topics (api-help-and-plugins.md "Help topics").
const KERNEL_HELP_PREFIX: &str = "ilium/";

/// The kernel's bundled help topics. Topics that plugins contribute are not served yet: the
/// client holds a plugin's manifest but not the Markdown files of its package.
impl KeymapRead for ClientFacade {
    fn bindings(&self) -> Vec<KeyBinding> {
        self.shared
            .keymap
            .lock()
            .map(|keymap| keymap.clone())
            .unwrap_or_default()
    }

    fn chords_of(&self, command: &str) -> Vec<String> {
        self.bindings()
            .into_iter()
            .filter(|binding| binding.command == command)
            .flat_map(|binding| binding.chords)
            .collect()
    }

    fn command_of(&self, chord: &str) -> Option<String> {
        self.bindings()
            .into_iter()
            .find(|binding| binding.chords.iter().any(|known| known == chord))
            .map(|binding| binding.command)
    }
}

impl HelpCatalog for ClientFacade {
    fn topic(&self, id: &str) -> Option<String> {
        if let Some(topic) = self.shared.plugin_help(id) {
            return Some(topic.markdown.clone());
        }
        let topic = crate::settings_help::catalog::by_id(id.strip_prefix(KERNEL_HELP_PREFIX)?)?;
        let mut markdown = format!("# {}\n\n{}\n", topic.title, topic.explanation);
        if !topic.caveat.is_empty() {
            markdown.push_str(&format!("\n{}\n", topic.caveat));
        }
        Some(markdown)
    }

    fn topic_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = crate::settings_help::catalog::all()
            .iter()
            .map(|topic| format!("{KERNEL_HELP_PREFIX}{}", topic.id))
            .collect();
        ids.extend(
            self.shared
                .help
                .iter()
                .filter(|(_, topic)| !self.shared.is_disabled(&topic.plugin))
                .map(|(id, _)| id.clone()),
        );
        ids
    }
}

impl TreeRead for ClientFacade {
    fn nodes(&self) -> Vec<TreeNode> {
        self.shared
            .nodes
            .lock()
            .map(|nodes| nodes.clone())
            .unwrap_or_default()
    }

    fn node(&self, id: NodeId) -> Option<TreeNode> {
        self.shared
            .nodes
            .lock()
            .ok()?
            .iter()
            .find(|node| node.id == id)
            .cloned()
    }

    fn epoch(&self) -> u64 {
        self.shared.epoch.load(Ordering::Relaxed)
    }
}

/// Pane content is not mirrored for client plugins yet: the client renders screens from the
/// server's stream and hands none of it to the host (foundation contract section 12.4).
impl PaneRead for ClientFacade {
    fn screen_text(&self, _pane: NodeId) -> Option<String> {
        None
    }

    fn cells(&self, _pane: NodeId, _rows: std::ops::Range<u16>) -> Option<Vec<Vec<Cell>>> {
        None
    }

    fn history(&self, _pane: NodeId, _lines: u32) -> Option<Vec<String>> {
        None
    }
}

impl WorkspaceAccess for ClientFacade {
    fn project_dir(&self) -> Option<String> {
        Some(self.shared.project_dir.clone())
    }

    fn session_name(&self) -> Option<String> {
        Some(self.shared.session_name.clone())
    }
}

impl UiSink for ClientFacade {
    fn put_panel(
        &self,
        plugin: &PluginId,
        panel: &str,
        tree_json: &str,
    ) -> Result<(), FacadeError> {
        let mut panels =
            self.shared.panels.lock().map_err(|_| {
                FacadeError::new(ErrorCode::Internal, "the panel store is poisoned")
            })?;
        panels.insert((plugin.0.clone(), panel.to_owned()), tree_json.to_owned());
        drop(panels);
        self.shared.push(Output::Redraw);
        Ok(())
    }

    fn remove_panel(&self, plugin: &PluginId, panel: &str) {
        if let Ok(mut panels) = self.shared.panels.lock() {
            panels.remove(&(plugin.0.clone(), panel.to_owned()));
        }
        self.shared.push(Output::Redraw);
    }

    fn toast(&self, plugin: &PluginId, toast_json: &str) -> Result<(), FacadeError> {
        self.shared.push(Output::Toast {
            plugin: plugin.0.clone(),
            toast_json: toast_json.to_owned(),
        });
        Ok(())
    }

    fn invalidate(&self, _plugin: &PluginId, _view: &str) {
        self.shared.push(Output::Redraw);
    }
}

/// Plugin log lines join the client log.
struct TracingLog;

impl LogSink for TracingLog {
    fn line(&self, plugin: &str, side: Side, level: Level, message: &str) {
        match level {
            Level::Trace => tracing::trace!(plugin, ?side, "{message}"),
            Level::Debug => tracing::debug!(plugin, ?side, "{message}"),
            Level::Info => tracing::info!(plugin, ?side, "{message}"),
            Level::Warn => tracing::warn!(plugin, ?side, "{message}"),
            Level::Error => tracing::error!(plugin, ?side, "{message}"),
        }
    }
}

fn log_errors(what: &str, errors: Vec<RuntimeError>) {
    for error in errors {
        tracing::warn!("plugin host, {what}: {error}");
    }
}

/// How long a displayed plugin pane waits before its view is asked for again.
const PANE_VIEW_RETRY: Duration = Duration::from_secs(5);

fn to_plugin(node: ilium_core::NodeId) -> NodeId {
    NodeId(node.0)
}

/// The agent of a pane as a plugin sees it. The signature is the provider's lower-case name;
/// the objective text is not part of the tree and stays absent.
fn agent_view(state: &ilium_core::AgentState) -> AgentView {
    let activity = match state.activity() {
        ilium_core::AgentActivity::Working
        | ilium_core::AgentActivity::WaitingBackground
        | ilium_core::AgentActivity::BackgroundTaskStillRunning => AgentActivity::Working,
        ilium_core::AgentActivity::WaitingApproval => AgentActivity::AwaitingApproval,
        ilium_core::AgentActivity::Done => AgentActivity::Done,
        ilium_core::AgentActivity::Idle => AgentActivity::Idle,
    };
    let goal = state.goal.map(|goal| {
        match goal {
            ilium_core::GoalState::Active => "active",
            ilium_core::GoalState::Paused => "paused",
            ilium_core::GoalState::Blocked => "blocked",
            ilium_core::GoalState::UsageLimited => "usage-limited",
            ilium_core::GoalState::Reached => "reached",
        }
        .to_owned()
    });
    AgentView {
        signature: match &state.class {
            ilium_core::AgentClass::Claude => "claude".to_owned(),
            ilium_core::AgentClass::Codex => "codex".to_owned(),
            ilium_core::AgentClass::Antigravity => "antigravity".to_owned(),
            ilium_core::AgentClass::Other(name) => name.to_lowercase(),
        },
        activity,
        objective: None,
        goal,
    }
}

/// One tree node as a plugin sees it. The server facade builds the same view from its tree.
fn node_view(tree: &Tree, id: ilium_core::NodeId) -> Option<TreeNode> {
    let node = tree.get(id)?;
    let container = match &node.kind {
        NodeKind::Container(container) => Some(match &container.kind {
            CoreContainerKind::Project { .. } => ContainerKind::Project,
            CoreContainerKind::Group => ContainerKind::Group,
            CoreContainerKind::SplitView {
                orientation: SplitOrientation::Horizontal,
            } => ContainerKind::SplitHorizontal,
            CoreContainerKind::SplitView {
                orientation: SplitOrientation::Vertical,
            } => ContainerKind::SplitVertical,
            CoreContainerKind::Worktree { .. } => ContainerKind::Worktree,
        }),
        NodeKind::Folder { .. } | NodeKind::Pane { .. } => None,
    };
    let agent = match &node.kind {
        // An agent whose composer is not available keeps its identity and reads with the
        // activity `none`, so a plugin can tell it from a plain shell and from a live agent.
        NodeKind::Pane { status, .. } => status.known_agent_state().map(|state| {
            let mut view = agent_view(state);
            if status.agent_state().is_none() {
                view.activity = AgentActivity::None;
            }
            view
        }),
        NodeKind::Container(_) | NodeKind::Folder { .. } => None,
    };
    let (class, content_kind) = match &node.kind {
        NodeKind::Container(_) => (NodeClass::Container, None),
        NodeKind::Folder { .. } => (NodeClass::Folder, None),
        NodeKind::Pane {
            content,
            plugin_pane,
            ..
        } => (
            NodeClass::Pane,
            Some(match content {
                PaneContentKind::Terminal => "ilium.terminal".to_owned(),
                PaneContentKind::Editor => "ilium.editor".to_owned(),
                PaneContentKind::Board => "ilium.board".to_owned(),
                // The same global kind id the server facade reports for this node.
                PaneContentKind::Plugin => plugin_pane
                    .as_ref()
                    .map_or_else(String::new, |kind| kind.kind_id().to_owned()),
            }),
        ),
    };
    Some(TreeNode {
        id: to_plugin(id),
        parent: node.parent.map(to_plugin),
        class,
        content_kind,
        title: node.name.clone(),
        cwd: tree
            .pane_cwd(id)
            .map(|path| path.to_string_lossy().into_owned()),
        agent,
        // `None` for a pane, which has no closed state to lock.
        locked: node.is_locked_closed().unwrap_or(false),
        revision: node.presentation_revision,
        container,
    })
}

/// Every node id in pre-order: roots by ascending id, children in the order the tree draws
/// them. `TreeRead::nodes` promises this order, and `tree.children` and `tree.roots` inherit
/// their sibling order from it.
fn preorder_ids(tree: &Tree) -> Vec<ilium_core::NodeId> {
    let mut roots: Vec<_> = tree
        .all_ids()
        .filter(|id| tree.parent_of(*id).is_none())
        .collect();
    roots.sort_by_key(|id| id.0);
    let mut ordered = Vec::new();
    let mut stack: Vec<_> = roots.into_iter().rev().collect();
    while let Some(id) = stack.pop() {
        ordered.push(id);
        if let Ok(children) = tree.children_of(id) {
            stack.extend(children.iter().rev().copied());
        }
    }
    // A node no root reaches is still readable by id, so it is still listed, last.
    let mut stray: Vec<_> = tree.all_ids().filter(|id| !ordered.contains(id)).collect();
    stray.sort_by_key(|id| id.0);
    ordered.extend(stray);
    ordered
}

/// A fingerprint of everything a plugin can read about one node, plus its place among its
/// siblings. A node whose fingerprint differs between two snapshots is listed in
/// `tree-changed`, so a rename, a move, a reorder, an agent change and a lock all reach plugins.
fn node_mark(tree: &Tree, id: ilium_core::NodeId) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let position = tree.parent_of(id).and_then(|parent| {
        tree.children_of(parent)
            .ok()
            .and_then(|children| children.iter().position(|child| *child == id))
    });
    position.hash(&mut hasher);
    format!("{:?}", node_view(tree, id)).hash(&mut hasher);
    hasher.finish()
}

/// The fingerprint of every node of a tree, by node id.
fn tree_marks(tree: &Tree) -> BTreeMap<u64, u64> {
    tree.all_ids()
        .map(|id| (id.0, node_mark(tree, id)))
        .collect()
}

/// What changed between two fingerprint maps: one `node-removed` per node that went away, then
/// one `tree-changed` naming every node that is new or reads differently.
fn tree_delta(known: &BTreeMap<u64, u64>, current: &BTreeMap<u64, u64>) -> Vec<Event> {
    let mut batch: Vec<Event> = known
        .keys()
        .filter(|id| !current.contains_key(id))
        .map(|id| Event::NodeRemoved(NodeId(*id)))
        .collect();
    batch.push(Event::TreeChanged(
        current
            .iter()
            .filter(|(id, mark)| known.get(id) != Some(mark))
            .map(|(id, _)| NodeId(*id))
            .collect(),
    ));
    batch
}

/// How often `config.toml` is looked at for kernel settings, theme and keymap changes.
const CONFIG_CHECK_INTERVAL: Duration = Duration::from_secs(2);

/// `config.toml` as the plugins last heard of it (gap G2).
struct KnownConfig {
    path: PathBuf,
    stamp: Option<(std::time::SystemTime, u64)>,
    table: toml::Table,
    checked: std::time::Instant,
}

/// The kernel tab id of the Animations tab, whose `[animations]` table plugins write.
const ANIMATIONS_TAB: &str = "animations";

/// The modification time and length of a file, `None` when it cannot be read.
fn file_stamp(path: &std::path::Path) -> Option<(std::time::SystemTime, u64)> {
    let metadata = std::fs::metadata(path).ok()?;
    Some((metadata.modified().ok()?, metadata.len()))
}

/// Where a pane's title came from: the user's own name, or one the kernel set.
fn title_origin(tree: &Tree, id: NodeId) -> TitleOrigin {
    match tree.get(ilium_core::NodeId(id.0)).map(|node| &node.kind) {
        Some(ilium_core::NodeKind::Pane { title_source, .. })
            if title_source.is_user_specified() =>
        {
            TitleOrigin::Manual
        }
        _ => TitleOrigin::Inferred,
    }
}

/// The running plugin host of one client. Owned by the `App`; every method returns at once.
pub(crate) struct ClientPlugins {
    shared: Arc<Shared>,
    /// Manifests of the catalogue, for the plugin list.
    catalogue: Vec<CataloguePlugin>,
    /// Contributions of the server sides, as the server last reported them.
    server_entries: BTreeMap<String, Entry>,
    /// Plugin states as the server last reported them.
    server_states: BTreeMap<String, String>,
    /// Node ids of the last mirrored tree, to tell plugins what was removed.
    known: BTreeMap<u64, u64>,
    /// The plugin views of the last mirrored tree, for the pane and agent events (gap G2).
    known_views: BTreeMap<u64, TreeNode>,
    /// The focused pane plugins last heard of.
    known_focus: Option<NodeId>,
    /// `config.toml` as plugins last heard of it, with the stamp it was read at.
    known_config: Option<KnownConfig>,
    /// The last refusal of an enable or disable request, for the plugin list.
    last_refusal: Option<String>,
    /// Compiled link detectors of the enabled plugins (G5), rebuilt when that set changes.
    link_cache: Mutex<super::links::DetectorCache>,
    /// Parsed key bindings of the enabled plugins (G1), rebuilt when that set changes.
    binding_cache: Mutex<super::keybindings::BindingCache>,
    /// `contributed(kind)` per kind, so the frame drawing does not parse every manifest on
    /// every frame. Cleared when a plugin's server state changes, which is what can change
    /// the set of enabled plugins; the catalogue itself is fixed for the client's life.
    chrome_cache: Mutex<BTreeMap<String, Vec<ChromeItem>>>,
    /// When the view of each displayed plugin pane was last asked for (gap G14b), so a pane
    /// whose plugin fails to start is not asked again on every frame.
    pane_view_requests: Mutex<Vec<(String, ViewTarget, std::time::Instant)>>,
}

impl std::fmt::Debug for ClientPlugins {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ClientPlugins")
            .field("plugins", &self.catalogue.len())
            .finish_non_exhaustive()
    }
}

impl ClientPlugins {
    /// Starts the host. `None` when plugins are off or the host could not be built; the
    /// client then runs without plugins and says so in its log.
    ///
    /// `paid_proxies` are the Kilo Gateway proxies the client loaded at start
    /// (`proxy_database::load_paid_proxies`); plugin inference goes through them exactly like
    /// the client's own naming workers.
    pub(crate) fn start(
        start: ClientPluginStart,
        session_name: &str,
        session_cwd: &std::path::Path,
        paid_proxies: Vec<ilium_inference::PaidProxy>,
    ) -> Option<Self> {
        if matches!(start.mode, StartMode::NoPlugins) {
            tracing::info!("plugins are off for this client (--no-plugins)");
            return None;
        }
        let cache_dir = start.data_dir.join("plugins").join("cache");
        if let Err(error) = std::fs::create_dir_all(&cache_dir) {
            tracing::warn!(
                "plugin code cache {} is unavailable: {error}",
                cache_dir.display()
            );
        }
        let engine = match PluginEngine::new(EngineConfig {
            epoch_tick_ms: EPOCH_TICK_MS,
            cache_dir: Some(cache_dir),
            process: ProcessKind::Client,
        }) {
            Ok(engine) => Arc::new(engine),
            Err(error) => {
                tracing::error!("plugin engine did not start, plugins are off: {error}");
                return None;
            }
        };
        let shared = Arc::new_cyclic(|me| Shared {
            me: me.clone(),
            nodes: Mutex::new(Vec::new()),
            focused: Mutex::new(None),
            epoch: AtomicU64::new(1),
            outbox: Mutex::new(VecDeque::new()),
            notification: Arc::new(tokio::sync::Notify::new()),
            waiting: Mutex::new(HashMap::new()),
            next_seq: AtomicU64::new(1),
            stopping: AtomicBool::new(false),
            panels: Mutex::new(BTreeMap::new()),
            popup: Mutex::new(None),
            views: Mutex::new(Vec::new()),
            segments: Mutex::new(Vec::new()),
            decorations: Mutex::new(Vec::new()),
            frame_slots: Mutex::new(Vec::new()),
            sounds: Mutex::new(super::sound_gate::SoundGate::new(
                start
                    .catalogue
                    .iter()
                    .map(|plugin| (plugin.id.as_str(), plugin.manifest.as_str())),
            )),
            keymap: Mutex::new(Vec::new()),
            screen: Mutex::new((0, 0)),
            runtime: Mutex::new(None),
            project_dir: session_cwd.to_string_lossy().into_owned(),
            session_name: session_name.to_owned(),
            help: start
                .help
                .iter()
                .map(|topic| (format!("{}/{}", topic.plugin, topic.id), topic.clone()))
                .collect(),
            disabled: Mutex::new(std::collections::BTreeSet::new()),
            settings: ilium_plugin_host::config_settings::ConfigFileSettings::of_user(),
            key_grabs: Mutex::new(None),
        });
        let (disabled, warning) = wiring::disabled_at_start(&start.data_dir);
        if let Some(warning) = warning {
            tracing::warn!("installed plugin registry unreadable, every plugin enabled: {warning}");
        }
        if let Ok(mut hidden) = shared.disabled.lock() {
            hidden.extend(disabled.iter().map(ToString::to_string));
        }
        let log: Arc<dyn LogSink> = Arc::new(TracingLog);
        let port = OutputPort {
            shared: Arc::downgrade(&shared),
        };
        // `ilium:host/inference` answers through the provider selected in Settings, and Kilo
        // Gateway calls go out through the paid proxies the client loaded, never the home IP.
        let inference = ilium_plugin_host::imports::inference_backend::SettingsInference::of_user();
        inference.set_paid_proxies(paid_proxies);
        let provider = wiring::import_provider(wiring::ProcessImports {
            storage_root: wiring::storage_root(&start.data_dir),
            scope: StorageContext {
                project: Some(session_cwd.to_path_buf()),
                session: Some(session_name.to_owned()),
                ..StorageContext::default()
            },
            // Basic panes are a server primitive; the client links no backend for them.
            basic: None,
            log: Arc::clone(&log),
            backends: wiring::wave_two::Backends::client(
                session_cwd.to_path_buf(),
                start.data_dir.clone(),
                wiring::wave_two::ClientBackendInputs {
                    inference: Some(Arc::new(inference)),
                    audio: Some(Arc::new(super::audio_backend::ClientAudio)),
                    // `fs.pick` shows the client's file picker over whatever is on screen.
                    picker: Some(Arc::new(super::file_picker::ClientFilePicker::new(
                        port.clone(),
                    ))),
                    // `clipboard` goes through the client's own clipboard helper process.
                    clipboard: Some(Arc::new(super::clipboard_backend::ClientClipboard::new(
                        port,
                    ))),
                    // `media` decodes with `ffmpeg`, in a child process per open video.
                    media: Some(Arc::new(super::media_backend::FfmpegMedia::new())),
                    agent_scopes: directories::BaseDirs::new()
                        .map(|dirs| wiring::wave_two::agent_scopes_in(dirs.home_dir()))
                        .unwrap_or_default(),
                },
            ),
        });
        let mut runtime = HostRuntime::new(
            engine,
            RuntimeConfig {
                side: Side::Client,
                kernel: Arc::new(ClientFacade {
                    shared: Arc::clone(&shared),
                }),
                log,
                provider: Some(provider),
                disabled,
            },
        );
        // Durable timers survive a client restart as they do a server restart (gap G7).
        runtime.set_durable_timer_file(wiring::durable_timers_path(&start.data_dir, "client"));
        for (service, text) in &start.schemas {
            if let Err(error) = runtime.install_schema(service, text) {
                tracing::warn!(service, "service schema not installed: {error}");
            }
        }
        if let Ok(mut slot) = shared.key_grabs.lock() {
            *slot = Some(runtime.key_grabs());
        }
        let thread = match RuntimeThread::spawn(runtime) {
            Ok(thread) => thread,
            Err(error) => {
                tracing::error!("plugin host thread did not start, plugins are off: {error}");
                return None;
            }
        };
        let manifests = start
            .catalogue
            .iter()
            .map(|plugin| CataloguePlugin {
                id: plugin.id.clone(),
                manifest: plugin.manifest.clone(),
                component: None,
            })
            .collect();
        let catalogue = start.catalogue;
        let wake = Arc::clone(&shared);
        // In the slot before the first job runs, so a plugin can message itself while starting.
        let posted = shared.runtime.lock().ok().is_some_and(|mut slot| {
            slot.insert(thread).post(move |runtime| {
                log_errors("load", runtime.load(catalogue));
                log_errors("startup activation", runtime.activate_startup());
                log_errors("pump", runtime.pump());
                tracing::info!(plugins = runtime.plugin_ids().len(), "plugin host started");
                wake.push(Output::Redraw);
            })
        });
        if !posted {
            tracing::error!("plugin host thread stopped before loading, plugins are off");
            return None;
        }
        Some(Self {
            shared,
            catalogue: manifests,
            server_entries: BTreeMap::new(),
            server_states: BTreeMap::new(),
            known: BTreeMap::new(),
            known_views: BTreeMap::new(),
            known_focus: None,
            known_config: None,
            last_refusal: None,
            link_cache: Mutex::new(super::links::DetectorCache::default()),
            binding_cache: Mutex::new(super::keybindings::BindingCache::default()),
            chrome_cache: Mutex::new(BTreeMap::new()),
            pane_view_requests: Mutex::new(Vec::new()),
        })
    }

    /// Signalled whenever the outbox has something for the event loop.
    pub(crate) fn notification(&self) -> Arc<tokio::sync::Notify> {
        Arc::clone(&self.shared.notification)
    }

    /// Takes what the host produced since the last call.
    pub(crate) fn take_outputs(&self) -> Vec<Output> {
        self.shared
            .outbox
            .lock()
            .map(|mut outbox| outbox.drain(..).collect())
            .unwrap_or_default()
    }

    /// Refreshes the tree mirror after the client applied a tree snapshot and tells the
    /// plugins which nodes appeared, went away or read differently.
    pub(crate) fn sync_tree(&mut self, tree: &Tree, focused: Option<ilium_core::NodeId>) {
        let nodes: Vec<TreeNode> = preorder_ids(tree)
            .into_iter()
            .filter_map(|id| node_view(tree, id))
            .collect();
        let current = tree_marks(tree);
        if let Ok(mut mirror) = self.shared.nodes.lock() {
            *mirror = nodes;
        }
        if let Ok(mut slot) = self.shared.focused.lock() {
            *slot = focused.map(to_plugin);
        }
        self.shared.epoch.fetch_add(1, Ordering::Relaxed);
        let mut batch = tree_delta(&self.known, &current);
        self.known = current;
        // Pane, title and agent events from the same two readings (gap G2).
        let views: BTreeMap<u64, TreeNode> = tree
            .all_ids()
            .filter_map(|id| node_view(tree, id))
            .map(|view| (view.id.0, view))
            .collect();
        let focused = focused.map(to_plugin);
        batch.extend(events_feed::node_events(
            &self.known_views,
            &views,
            focused,
            |id| title_origin(tree, id),
        ));
        self.known_views = views;
        // A grab that names a pane ends when the pane closes.
        if let Ok(grabs) = self.shared.key_grabs.lock() {
            if let Some(grabs) = grabs.as_ref() {
                grabs.forget_closed_panes(|pane| tree.get(ilium_core::NodeId(pane)).is_some());
            }
        }
        batch.extend(events_feed::focus_event(self.known_focus, focused));
        self.known_focus = focused;
        let shared = Arc::clone(&self.shared);
        self.shared.post(move |runtime| {
            log_errors("event delivery", runtime.broadcast(&batch));
            log_errors("pump", runtime.pump());
            shared.refresh_views(runtime);
        });
    }

    /// Tells the plugins what changed outside a tree snapshot (gap G2): the focused pane and,
    /// at most every two seconds, `config.toml` (kernel settings, theme and keymap). Cheap
    /// when nothing changed; the event loop calls it on every tick.
    ///
    /// Returns the new `[animations]` table when this reading found it changed. A plugin
    /// writes it through `kernel-settings` (tab `animations`), and the caller applies it to
    /// the live animation settings, which the plugin host itself does not own.
    pub(crate) fn observe(&mut self, focused: Option<ilium_core::NodeId>) -> Option<toml::Table> {
        let mut batch = Vec::new();
        let mut animations = None;
        let focused = focused.map(to_plugin);
        if let Some(event) = events_feed::focus_event(self.known_focus, focused) {
            batch.push(event);
            self.known_focus = focused;
            if let Ok(mut slot) = self.shared.focused.lock() {
                *slot = focused;
            }
        }
        let now = std::time::Instant::now();
        match &mut self.known_config {
            None => {
                if let Some(path) =
                    ilium_platform::paths::config_dir().map(|dir| dir.join("config.toml"))
                {
                    self.known_config = Some(KnownConfig {
                        stamp: file_stamp(&path),
                        table: events_feed::read_config(&path),
                        path,
                        checked: now,
                    });
                }
            }
            Some(known) if now.duration_since(known.checked) >= CONFIG_CHECK_INTERVAL => {
                known.checked = now;
                let stamp = file_stamp(&known.path);
                if stamp != known.stamp {
                    known.stamp = stamp;
                    let table = events_feed::read_config(&known.path);
                    let events = events_feed::config_events(&known.table, &table);
                    if events.iter().any(|event| {
                        matches!(event, Event::KernelSettingsChanged(change) if change.tab == ANIMATIONS_TAB)
                    }) {
                        animations = Some(
                            ilium_plugin_host::config_settings::table_of_tab(ANIMATIONS_TAB)
                                .and_then(|name| table.get(name))
                                .and_then(toml::Value::as_table)
                                .cloned()
                                .unwrap_or_default(),
                        );
                    }
                    batch.extend(events);
                    known.table = table;
                }
            }
            Some(_) => {}
        }
        if batch.is_empty() {
            return animations;
        }
        self.shared.post(move |runtime| {
            log_errors("event delivery", runtime.broadcast(&batch));
            log_errors("pump", runtime.pump());
        });
        animations
    }

    /// Routes plugin traffic from the server. Returns whether the plugin list changed.
    pub(crate) fn on_server_event(&mut self, event: ServerEvent) -> bool {
        match event {
            ServerEvent::Plugin(envelope) => self.on_envelope(envelope),
            ServerEvent::Contributions(delta) => {
                self.apply_delta(delta);
                true
            }
            ServerEvent::Rejected(rejection) => {
                tracing::warn!(
                    "the server rejected plugin traffic: {} ({})",
                    rejection.message,
                    rejection.code
                );
                false
            }
            _ => false,
        }
    }

    fn on_envelope(&mut self, envelope: PluginEnvelope) -> bool {
        let PluginEnvelope { plugin, seq, body } = envelope;
        match body {
            EnvelopeBody::Outcome(record) => {
                let outcome =
                    serde_json::from_str::<Outcome>(&record.outcome_json).unwrap_or_else(|error| {
                        failed(
                            ErrorCode::Internal,
                            &format!("the server's outcome does not parse: {error}"),
                        )
                    });
                let waiter = self
                    .shared
                    .waiting
                    .lock()
                    .ok()
                    .and_then(|mut waiting| waiting.remove(&seq));
                if let Some(waiter) = waiter {
                    // The plugin may have timed out already; its receiver is then gone.
                    let _ = waiter.send(outcome);
                }
                false
            }
            EnvelopeBody::ForwardedIntent(forwarded) => {
                // An intent a server plugin cannot carry out itself (gap G3).
                match serde_json::from_slice::<Intent>(&forwarded.intent_json) {
                    Ok(intent) => self.shared.apply_forwarded(&forwarded.plugin, intent),
                    Err(error) => {
                        tracing::debug!("a forwarded plugin intent is malformed: {error}");
                        false
                    }
                }
            }
            body @ (EnvelopeBody::Event(_)
            | EnvelopeBody::ClientRequest(_)
            | EnvelopeBody::ClientReply(_))
                if plugin == REMOTE_ENVELOPE_PLUGIN =>
            {
                // A topic message or service traffic of the server runtime (gap G4).
                let Some(message) =
                    super::remote::wire_of(body).and_then(|wire| RemoteMessage::from_wire(&wire))
                else {
                    tracing::debug!("malformed cross-side envelope from the server dropped");
                    return false;
                };
                self.shared.post(move |runtime| {
                    runtime.deliver_remote(vec![message]);
                    log_errors("pump", runtime.pump());
                });
                false
            }
            EnvelopeBody::Message(message) => {
                let event = Event::Message(MessageEvent {
                    from: PluginId(plugin.clone()),
                    from_side: PluginSide::Server,
                    from_client: None,
                    channel: message.topic,
                    payload: message.payload,
                });
                let shared = Arc::clone(&self.shared);
                self.shared.post(move |runtime| {
                    if let Err(error) = runtime.deliver(&plugin, vec![event]) {
                        tracing::debug!("server plugin message not delivered: {error}");
                    }
                    log_errors("pump", runtime.pump());
                    shared.refresh_views(runtime);
                });
                false
            }
            EnvelopeBody::Control(ControlBody::PluginState {
                plugin: target,
                state,
            }) => {
                if let Some(code) = state.strip_prefix("refused:") {
                    self.last_refusal = Some(format!("{target}: {code}"));
                    return true;
                }
                self.last_refusal = None;
                let enabled = !wiring::is_disabled_state(&state);
                let was_enabled = self
                    .server_states
                    .get(&target)
                    .is_none_or(|previous| !wiring::is_disabled_state(previous));
                self.server_states.insert(target.clone(), state);
                if let Ok(mut cache) = self.chrome_cache.lock() {
                    cache.clear();
                }
                if let Ok(mut hidden) = self.shared.disabled.lock() {
                    if enabled {
                        hidden.remove(&target);
                    } else {
                        hidden.insert(target.clone());
                    }
                }
                if enabled != was_enabled {
                    if !enabled {
                        // Nothing of a disabled plugin stays on screen (COR-218).
                        self.shared.forget_plugin(&target);
                    }
                    // The client side of a plugin follows its server side.
                    self.shared.post(move |runtime| {
                        if let Err(error) = runtime.set_enabled(&target, enabled) {
                            tracing::debug!("client side of {target} kept its state: {error}");
                        } else if enabled {
                            log_errors("startup activation", runtime.activate_startup());
                        }
                        log_errors("pump", runtime.pump());
                    });
                }
                true
            }
            other => {
                tracing::debug!(
                    plugin,
                    "plugin envelope not handled by the client: {other:?}"
                );
                false
            }
        }
    }

    fn apply_delta(&mut self, delta: ContributionDelta) {
        if matches!(delta.kind, DeltaKind::Snapshot) {
            self.server_entries.clear();
        }
        for op in delta.ops {
            match op {
                WireOp::Put { id, entry } => match wiring::decode_entry(&entry) {
                    Some(entry) => {
                        self.server_entries.insert(id, entry);
                    }
                    None => tracing::warn!("contribution {id} from the server does not decode"),
                },
                WireOp::Remove { id } => {
                    self.server_entries.remove(&id);
                }
                WireOp::Clear { plugin } => {
                    // A contribution id is `<plugin>/<kind>/<name>`.
                    let prefix = format!("{plugin}/");
                    self.server_entries.retain(|id, _| !id.starts_with(&prefix));
                }
            }
        }
    }

    /// Runs a plugin command (`<plugin id>/<command>`) on the client side of its plugin, with
    /// the focused pane as context. Returns at once; false when no plugin host is running.
    /// A command no enabled plugin contributes becomes a toast, so the user is told.
    pub(crate) fn invoke_command(
        &self,
        command: &str,
        arguments_json: String,
        origin: CommandOrigin,
        screen: (u16, u16),
    ) -> bool {
        self.shared.set_screen(screen);
        let shared = Arc::clone(&self.shared);
        let command = command.to_owned();
        self.shared.post(move |runtime| {
            let context = CommandContext {
                pane: shared.focused.lock().ok().and_then(|focused| *focused),
                ..CommandContext::default()
            };
            if let Err(error) = runtime.invoke_command(&command, &arguments_json, origin, context) {
                let toast = serde_json::json!({
                    "message": format!("{command} is not available: {error}"),
                });
                shared.push(Output::Toast {
                    plugin: "plugins".to_owned(),
                    toast_json: toast.to_string(),
                });
            }
            log_errors("pump", runtime.pump());
            shared.refresh_popup(runtime);
            shared.refresh_views(runtime);
        })
    }

    /// The link detectors of the enabled plugins whose command resolves, compiled once per
    /// set of enabled plugins. The terminal link job of every click matches against them.
    pub(crate) fn link_detectors(&self) -> Arc<[super::links::LinkDetector]> {
        let manifests = self.enabled_manifests();
        match self.link_cache.lock() {
            Ok(mut cache) => cache.detectors(manifests),
            Err(_) => super::links::active_detectors(&manifests).into(),
        }
    }

    /// Activates a link a plugin detector matched: `link-activated` reaches the owner, then
    /// the detector's command runs with the detector id and the matched text as arguments.
    pub(crate) fn activate_link(
        &self,
        found: &super::links::LinkMatch,
        pane: Option<ilium_core::NodeId>,
        screen: (u16, u16),
    ) {
        self.shared.set_screen(screen);
        let shared = Arc::clone(&self.shared);
        let owner = found.plugin.clone();
        let command = found.command.clone();
        let arguments_json = found.arguments_json();
        let activated = Event::LinkActivated(LinkActivatedEvent {
            node: pane.map(to_plugin).unwrap_or_default(),
            detector_id: found.detector_id.clone(),
            text: found.text.clone(),
        });
        self.shared.post(move |runtime| {
            let context = CommandContext {
                pane: pane.map(to_plugin),
                ..CommandContext::default()
            };
            // The WIT has no link origin; a link opens on the Ctrl+O key, a key binding.
            if let Err(error) = runtime.invoke_command(
                &command,
                &arguments_json,
                CommandOrigin::Keybinding,
                context,
            ) {
                let toast = serde_json::json!({
                    "message": format!("{command} is not available: {error}"),
                });
                shared.push(Output::Toast {
                    plugin: "plugins".to_owned(),
                    toast_json: toast.to_string(),
                });
            }
            // After the command, which activates a loaded owner, so the event reaches it.
            if let Err(error) = runtime.deliver(&owner, vec![activated]) {
                tracing::debug!("link-activated not delivered to {owner}: {error}");
            }
            log_errors("pump", runtime.pump());
            shared.refresh_popup(runtime);
            shared.refresh_views(runtime);
        });
    }

    /// Tells the client side of a plugin that the user changed some of its settings in
    /// Settings (`settings-changed`, origin `user`). A plugin that is not active reads the
    /// stored values when it starts, so a refused delivery is only logged.
    pub(crate) fn settings_changed(&self, plugin: &str, keys: Vec<String>) {
        let owner = plugin.to_owned();
        let shared = Arc::clone(&self.shared);
        self.shared.post(move |runtime| {
            let change = Event::SettingsChanged(SettingsChange {
                keys,
                origin: SettingsOrigin::User,
                side: PluginSide::Client,
            });
            if let Err(error) = runtime.deliver(&owner, vec![change]) {
                tracing::debug!("settings-changed not delivered to {owner}: {error}");
            }
            log_errors("pump", runtime.pump());
            shared.refresh_views(runtime);
        });
    }

    /// The view the plugin last rendered for its open popup; `None` before the first one.
    pub(crate) fn popup_tree(&self, plugin: &str, popup: &str) -> Option<UiTree> {
        self.shared.popup.lock().ok().and_then(|slot| {
            slot.as_ref()
                .filter(|open| open.plugin == plugin && open.popup == popup)
                .and_then(|open| open.tree.clone())
        })
    }

    /// Delivers actions of the open popup to its plugin, in order, in one batch (an input's
    /// pending change always arrives before the button that acts on it), then refreshes the
    /// view.
    pub(crate) fn popup_actions(&self, plugin: &str, actions: Vec<UiAction>, screen: (u16, u16)) {
        if actions.is_empty() {
            return;
        }
        self.shared.set_screen(screen);
        let shared = Arc::clone(&self.shared);
        let plugin = plugin.to_owned();
        let events: Vec<Event> = actions.into_iter().map(Event::UiAction).collect();
        self.shared.post(move |runtime| {
            if let Err(error) = runtime.deliver(&plugin, events) {
                tracing::debug!("popup action not delivered to {plugin}: {error}");
            }
            log_errors("pump", runtime.pump());
            shared.refresh_popup(runtime);
        });
    }

    /// Closes the open popup on the kernel's own authority (Escape, a click outside, another
    /// modal taking the screen) and tells the plugin with a `closed` action.
    pub(crate) fn dismiss_popup(&self, plugin: &str, popup: &str) {
        if !self.shared.close_popup(plugin, popup) {
            return;
        }
        let closed = UiAction {
            view: ViewTarget::Popup(popup.to_owned()),
            key: String::new(),
            action_id: String::new(),
            kind: ActionKind::Closed,
            value: None,
        };
        let plugin = plugin.to_owned();
        self.shared.post(move |runtime| {
            if let Err(error) = runtime.deliver(&plugin, vec![Event::UiAction(closed)]) {
                tracing::debug!("popup close not delivered to {plugin}: {error}");
            }
            log_errors("pump", runtime.pump());
        });
    }

    /// Replaces the key map plugins read. Cheap when nothing changed.
    pub(crate) fn sync_keymap(&self, bindings: Vec<KeyBinding>) {
        if let Ok(mut keymap) = self.shared.keymap.lock() {
            if *keymap != bindings {
                *keymap = bindings;
            }
        }
    }

    /// Opens a view on the kernel's own authority (the user picked a contributed panel, a
    /// settings page or an onboarding step).
    /// The first frame that draws the view reports its cells with `set_view_area`.
    pub(crate) fn show_view(&self, plugin: &str, view: ViewTarget) {
        // A view renders only while its plugin is active. The user asked for this view, so
        // the plugin is activated first; activating an active plugin does nothing.
        let shared = Arc::clone(&self.shared);
        let owner = plugin.to_owned();
        let posted = self.shared.post(move |runtime| {
            if let Err(error) = runtime.activate(&owner) {
                tracing::warn!("{owner} did not start, its view stays closed: {error}");
                return;
            }
            log_errors("pump", runtime.pump());
            shared.open_view(&owner, view, true);
        });
        if !posted {
            tracing::debug!("the plugin host is gone, the view of {plugin} stays closed");
        }
    }

    /// Opens a plugin's popup on the kernel's own authority: the user pressed a contributed
    /// toolbar button that names a `popup_view` and runs no command. The plugin is activated
    /// first, like for `show_view`.
    pub(crate) fn show_popup(&self, plugin: &str, popup: &str) {
        let shared = Arc::clone(&self.shared);
        let owner = plugin.to_owned();
        let popup = popup.to_owned();
        let posted = self.shared.post(move |runtime| {
            if let Err(error) = runtime.activate(&owner) {
                tracing::warn!("{owner} did not start, its popup stays closed: {error}");
                return;
            }
            log_errors("pump", runtime.pump());
            shared.open_popup(&owner, popup);
            shared.refresh_popup(runtime);
        });
        if !posted {
            tracing::debug!("the plugin host is gone, the popup of {plugin} stays closed");
        }
    }

    /// Tells the host how many cells a view has. A change asks the plugin for a new tree.
    pub(crate) fn set_view_area(&self, plugin: &str, view: &ViewTarget, area: (u16, u16)) {
        let changed = self.shared.views.lock().is_ok_and(|mut views| {
            views
                .iter_mut()
                .find(|slot| slot.plugin == plugin && &slot.view == view)
                .is_some_and(|slot| {
                    let changed = slot.area != area;
                    slot.area = area;
                    changed
                })
        });
        if changed {
            let shared = Arc::clone(&self.shared);
            self.shared
                .post(move |runtime| shared.refresh_views(runtime));
        }
    }

    /// The tree the plugin last rendered for an open view; `None` before the first one or
    /// when the view is not open.
    pub(crate) fn view_tree(&self, plugin: &str, view: &ViewTarget) -> Option<UiTree> {
        self.shared.views.lock().ok().and_then(|views| {
            views
                .iter()
                .find(|slot| slot.plugin == plugin && &slot.view == view)
                .and_then(|slot| slot.tree.clone())
        })
    }

    /// Every open view that was given cells at least once, as plugin and target. A view the
    /// event loop has not placed yet is left out, so it is never closed before it is shown.
    pub(crate) fn placed_views(&self) -> Vec<(String, ViewTarget)> {
        self.shared
            .views
            .lock()
            .map(|views| {
                views
                    .iter()
                    .filter(|slot| slot.area != (0, 0))
                    .map(|slot| (slot.plugin.clone(), slot.view.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Whether `plugin` is in the catalogue and enabled, so it can draw its plugin panes.
    pub(crate) fn runs_plugin(&self, plugin: &str) -> bool {
        self.catalogue.iter().any(|entry| entry.id == plugin)
            && !self.shared.is_disabled(plugin)
            && !self
                .server_states
                .get(plugin)
                .is_some_and(|state| wiring::is_disabled_state(state))
    }

    /// Opens the view of a displayed plugin pane unless it is open. A request that did not
    /// open the view (the plugin failed to start) is repeated after `PANE_VIEW_RETRY`, not on
    /// every frame.
    pub(crate) fn ensure_pane_view(&self, plugin: &str, view: &ViewTarget) {
        if self.view_is_open(plugin, view) {
            // Opened: the next time this pane is shown after a dismissal, ask again at once.
            if let Ok(mut requests) = self.pane_view_requests.lock() {
                requests.retain(|(owner, target, _)| !(owner == plugin && target == view));
            }
            return;
        }
        let now = std::time::Instant::now();
        let due = self.pane_view_requests.lock().is_ok_and(|mut requests| {
            requests.retain(|(_, _, asked)| now.duration_since(*asked) < PANE_VIEW_RETRY);
            if requests
                .iter()
                .any(|(owner, target, _)| owner == plugin && target == view)
            {
                return false;
            }
            requests.push((plugin.to_owned(), view.clone(), now));
            true
        });
        if due {
            self.show_view(plugin, view.clone());
        }
    }

    /// Whether a view is open.
    pub(crate) fn view_is_open(&self, plugin: &str, view: &ViewTarget) -> bool {
        self.shared.views.lock().is_ok_and(|views| {
            views
                .iter()
                .any(|slot| slot.plugin == plugin && &slot.view == view)
        })
    }

    /// Delivers one action of a view to its plugin and refreshes the open views.
    pub(crate) fn view_action(&self, plugin: &str, action: UiAction) {
        let shared = Arc::clone(&self.shared);
        let plugin = plugin.to_owned();
        self.shared.post(move |runtime| {
            if let Err(error) = runtime.deliver(&plugin, vec![Event::UiAction(action)]) {
                tracing::debug!("view action not delivered to {plugin}: {error}");
            }
            log_errors("pump", runtime.pump());
            shared.refresh_popup(runtime);
            shared.refresh_views(runtime);
        });
    }

    /// Closes a view on the kernel's own authority and tells the plugin with a `closed`
    /// action.
    pub(crate) fn dismiss_view(&self, plugin: &str, view: &ViewTarget) {
        if !self.shared.close_view(plugin, view) {
            return;
        }
        let closed = UiAction {
            view: view.clone(),
            key: String::new(),
            action_id: String::new(),
            kind: ActionKind::Closed,
            value: None,
        };
        let plugin = plugin.to_owned();
        self.shared.post(move |runtime| {
            if let Err(error) = runtime.deliver(&plugin, vec![Event::UiAction(closed)]) {
                tracing::debug!("view close not delivered to {plugin}: {error}");
            }
            log_errors("pump", runtime.pump());
        });
    }

    /// The status-bar segments plugins set, in the order they were first set.
    pub(crate) fn status_segments(&self) -> Vec<StatusSegment> {
        let mut segments = self
            .shared
            .segments
            .lock()
            .map(|segments| segments.clone())
            .unwrap_or_default();
        // Segments of server plugins arrive as `live.status_segment` entries (section 20.4).
        segments.extend(
            self.server_entries
                .values()
                .filter(|entry| entry.kind == wiring::LIVE_SEGMENT_KIND)
                .filter_map(|entry| {
                    let spec: ilium_plugin_api::generated::types::intents::SetStatusSegmentSpec =
                        serde_json::from_value(entry.payload.clone()).ok()?;
                    Some(StatusSegment {
                        plugin: entry.plugin.clone(),
                        segment: spec.segment,
                        text: spec.content.iter().map(|span| span.text.as_str()).collect(),
                        tone: spec.tone,
                    })
                }),
        );
        segments
    }

    /// The decorations server plugins set, decoded from their `live.decoration` entries.
    fn server_decorations(&self) -> impl Iterator<Item = TreeDecoration> + '_ {
        self.server_entries
            .values()
            .filter(|entry| entry.kind == wiring::LIVE_DECORATION_KIND)
            .filter_map(|entry| {
                let spec: ilium_plugin_api::generated::types::intents::SetDecorationSpec =
                    serde_json::from_value(entry.payload.clone()).ok()?;
                Some(TreeDecoration {
                    plugin: entry.plugin.clone(),
                    node: spec.node,
                    slot: spec.slot,
                    text: spec.content.iter().map(|span| span.text.as_str()).collect(),
                    icon: spec.icon,
                    sort_key: spec.sort_key.unwrap_or(0),
                })
            })
    }

    /// The decorations client-side plugins set on one tree node, by sort key.
    pub(crate) fn decorations_of(&self, node: ilium_core::NodeId) -> Vec<TreeDecoration> {
        let node = to_plugin(node);
        let mut found: Vec<TreeDecoration> = self
            .shared
            .decorations
            .lock()
            .map(|decorations| {
                decorations
                    .iter()
                    .filter(|decoration| decoration.node == node)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        found.extend(
            self.server_decorations()
                .filter(|decoration| decoration.node == node),
        );
        found.sort_by_key(|decoration| decoration.sort_key);
        found
    }

    /// The sort keys one plugin set on tree nodes with its decorations in `slot`: what a
    /// tree ordering it contributes (`contributes.tree_orderings`) sorts by. Client-side and
    /// server-side decorations both count.
    pub(crate) fn sort_keys(
        &self,
        plugin: &str,
        slot: &str,
    ) -> std::collections::HashMap<ilium_core::NodeId, f64> {
        let mut keys = std::collections::HashMap::new();
        let mut add = |decoration: &TreeDecoration| {
            if decoration.plugin == plugin && decoration.slot == slot {
                keys.insert(
                    ilium_core::NodeId(decoration.node.0),
                    decoration.sort_key as f64,
                );
            }
        };
        if let Ok(decorations) = self.shared.decorations.lock() {
            decorations.iter().for_each(&mut add);
        }
        self.server_decorations()
            .for_each(|decoration| add(&decoration));
        keys
    }

    /// What plugins put in the frame slots of one pane: the contents they set with
    /// `set-frame-slot`, then decorations they set on the pane under a frame slot's id.
    /// Callers keep only the slots a `pane_frame_slots` entry declares.
    pub(crate) fn frame_slot_contents_of(&self, node: ilium_core::NodeId) -> Vec<TreeDecoration> {
        let pane = to_plugin(node);
        let mut found: Vec<TreeDecoration> = self
            .shared
            .frame_slots
            .lock()
            .map(|frame_slots| {
                frame_slots
                    .iter()
                    .filter(|content| content.node == pane)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        found.extend(self.decorations_of(node));
        found
    }

    /// Mutes or unmutes the `play-sound` intents of client plugins (Settings > Sounds).
    pub(crate) fn set_sounds_muted(&self, muted: bool) {
        if let Ok(mut sounds) = self.shared.sounds.lock() {
            sounds.set_muted(muted);
        }
    }

    /// Whether any plugin may have frame slot content, so frame drawing can skip the lookup.
    pub(crate) fn has_frame_slot_contents(&self) -> bool {
        self.shared
            .frame_slots
            .lock()
            .is_ok_and(|frame_slots| !frame_slots.is_empty())
            || self.has_decorations()
    }

    /// Whether any plugin decorates the tree, so the tree drawing can skip the lookup.
    pub(crate) fn has_decorations(&self) -> bool {
        self.shared
            .decorations
            .lock()
            .is_ok_and(|decorations| !decorations.is_empty())
            || self
                .server_entries
                .values()
                .any(|entry| entry.kind == wiring::LIVE_DECORATION_KIND)
    }

    /// The entries enabled plugins contribute of one manifest kind (`tree_toolbar`,
    /// `context_menu`, `row_actions`, `panels`, `settings`, `onboarding_steps`), in catalogue
    /// order. Onboarding steps are ordered by their `order`.
    pub(crate) fn contributed(&self, kind: &str) -> Vec<ChromeItem> {
        if let Some(items) = self
            .chrome_cache
            .lock()
            .ok()
            .and_then(|cache| cache.get(kind).cloned())
        {
            return items;
        }
        let items = self.contributed_uncached(kind);
        if let Ok(mut cache) = self.chrome_cache.lock() {
            cache.insert(kind.to_owned(), items.clone());
        }
        items
    }

    /// [`Self::contributed`] read from the manifests.
    fn contributed_uncached(&self, kind: &str) -> Vec<ChromeItem> {
        let mut items: Vec<ChromeItem> = self
            .catalogue
            .iter()
            .filter(|plugin| {
                !self
                    .server_states
                    .get(&plugin.id)
                    .is_some_and(|state| wiring::is_disabled_state(state))
            })
            .flat_map(|plugin| chrome_items(&plugin.id, &plugin.manifest, kind))
            .collect();
        items.sort_by_key(|item| item.order);
        items
    }

    /// The settings rows enabled plugins contribute (`contributes.settings[].rows`), in
    /// catalogue order, each with the page it belongs to and its default value.
    pub(crate) fn contributed_settings(&self) -> Vec<ContributedSetting> {
        self.catalogue
            .iter()
            .filter(|plugin| {
                !self
                    .server_states
                    .get(&plugin.id)
                    .is_some_and(|state| wiring::is_disabled_state(state))
            })
            .flat_map(|plugin| contributed_settings(&plugin.id, &plugin.manifest))
            .collect()
    }

    /// `(plugin id, manifest text)` of every enabled plugin, in catalogue order.
    pub(crate) fn enabled_manifests(&self) -> Vec<(String, String)> {
        self.catalogue
            .iter()
            .filter(|plugin| {
                !self
                    .server_states
                    .get(&plugin.id)
                    .is_some_and(|state| wiring::is_disabled_state(state))
            })
            .map(|plugin| (plugin.id.clone(), plugin.manifest.clone()))
            .collect()
    }

    /// The rows of the Settings plugin list.
    pub(crate) fn rows(&self) -> Vec<PluginRow> {
        let disabled = self
            .server_states
            .iter()
            .filter(|(_, state)| wiring::is_disabled_state(state))
            .map(|(id, _)| id.clone())
            .collect();
        wiring::plugin_rows(&self.catalogue, &disabled, &self.server_states)
    }

    /// The command the first enabled plugin binds to a chord in a mode `accepts`, in
    /// catalogue order (gap G1).
    pub(crate) fn binding_command(
        &self,
        chord: &ilium_plugin_host::imports::keys::Chord,
        accepts: impl Fn(&super::keybindings::BindingMode) -> bool,
    ) -> Option<String> {
        let enabled = self
            .catalogue
            .iter()
            .filter(|plugin| {
                !self
                    .server_states
                    .get(&plugin.id)
                    .is_some_and(|state| wiring::is_disabled_state(state))
            })
            .map(|plugin| (plugin.id.as_str(), plugin.manifest.as_str()));
        let bindings = match self.binding_cache.lock() {
            Ok(mut cache) => cache.bindings(enabled),
            Err(_) => enabled
                .flat_map(|(id, manifest)| super::keybindings::plugin_bindings(id, manifest))
                .collect::<Vec<_>>()
                .into(),
        };
        super::keybindings::find_binding(&bindings, chord, accepts)
            .map(|binding| binding.command.clone())
    }

    /// Offers a pressed key to the plugins' key grabs (gap G1). When a grab takes it, its
    /// owner hears `key-pressed` and the method returns true: the key must not reach the pane.
    pub(crate) fn route_grab(
        &self,
        chord: &ilium_plugin_host::imports::keys::Chord,
        focused: Option<ilium_core::NodeId>,
        repeat: bool,
    ) -> bool {
        let routed = self
            .shared
            .key_grabs
            .lock()
            .ok()
            .and_then(|grabs| grabs.as_ref()?.route(chord, focused.map(|pane| pane.0)));
        let Some(routed) = routed else {
            return false;
        };
        let event = Event::KeyPressed(ilium_plugin_api::generated::types::events::KeyEvent {
            grab: ilium_plugin_api::generated::types::GrabId(routed.grab),
            pane: routed.pane.map(NodeId),
            chord: routed.chord,
            repeat,
        });
        let plugin = routed.plugin;
        self.shared.post(move |runtime| {
            if let Err(error) = runtime.deliver(&plugin, vec![event]) {
                tracing::warn!("plugin host, key delivery: {error}");
            }
            log_errors("pump", runtime.pump());
        })
    }

    /// The last refused enable or disable request, if the newest request was refused.
    pub(crate) fn last_refusal(&self) -> Option<&str> {
        self.last_refusal.as_deref()
    }

    /// How many contributions the server sides currently hold.
    pub(crate) fn server_contribution_count(&self) -> usize {
        self.server_entries.len()
    }

    /// The request that asks the server to enable or disable a plugin. The server persists
    /// the choice and broadcasts the new state, which `on_server_event` then applies here.
    pub(crate) fn enable_request(&self, plugin: &str, enabled: bool) -> ClientRequest {
        self.shared
            .envelope(
                plugin,
                EnvelopeBody::Control(ControlBody::SetEnabled {
                    plugin: plugin.to_owned(),
                    enabled,
                }),
            )
            .1
    }

    /// The request that tells the plugin's server side about settings the user changed here;
    /// the server delivers `settings-changed` to it (`ControlBody::SettingsChanged`).
    pub(crate) fn settings_changed_request(
        &self,
        plugin: &str,
        keys: Vec<String>,
    ) -> ClientRequest {
        self.shared
            .envelope(
                plugin,
                EnvelopeBody::Control(ControlBody::SettingsChanged {
                    plugin: plugin.to_owned(),
                    keys,
                }),
            )
            .1
    }

    /// Deactivates every plugin and joins the host thread. Intents submitted while stopping
    /// are cancelled at once, because the event loop no longer carries them to the server.
    pub(crate) async fn stop(&self) {
        self.shared.stopping.store(true, Ordering::Release);
        let thread = self
            .shared
            .runtime
            .lock()
            .ok()
            .and_then(|mut slot| slot.take());
        let Some(thread) = thread else {
            return;
        };
        let join = tokio::task::spawn_blocking(move || thread.shutdown(SHUTDOWN_BUDGET));
        match tokio::time::timeout(SHUTDOWN_WAIT, join).await {
            Ok(Ok(report)) => tracing::info!(
                deactivated = report.deactivated.len(),
                abandoned = report.abandoned.len(),
                "plugin host stopped"
            ),
            Ok(Err(error)) => tracing::warn!("plugin host shutdown task failed: {error}"),
            Err(_) => tracing::warn!("plugin host did not stop in time and was abandoned"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_chrome_of_a_manifest_is_read_with_full_command_ids() {
        let manifest = r#"{"contributes": {"tree_toolbar": [
            {"id": "new", "icon": "plus", "label": "New note", "command": "new-note"},
            {"id": "other", "command": "ilium.other/run"},
            {"label": "no id"}
        ]}}"#;
        let items = chrome_items("ilium.notes", manifest, "tree_toolbar");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, "New note");
        assert_eq!(items[0].icon.as_deref(), Some("plus"));
        assert_eq!(items[0].command.as_deref(), Some("ilium.notes/new-note"));
        assert_eq!(items[1].title, "other");
        assert_eq!(items[1].command.as_deref(), Some("ilium.other/run"));
        assert_eq!(items[0].popup_view, None);
        let popup = r#"{"contributes": {"tree_toolbar": [
            {"id": "pick", "label": "Pick", "popup_view": "picker"}
        ]}}"#;
        let items = chrome_items("ilium.notes", popup, "tree_toolbar");
        assert_eq!(items[0].popup_view.as_deref(), Some("picker"));
        assert_eq!(items[0].command, None);
        assert!(chrome_items("ilium.notes", manifest, "panels").is_empty());
        assert!(chrome_items("ilium.notes", "not json", "panels").is_empty());
    }

    #[test]
    fn a_tree_node_is_mirrored_with_its_class_and_parent() {
        let tree = Tree::new();
        let root = node_view(&tree, ilium_core::ROOT_ID).expect("the root exists");
        assert_eq!(root.id, NodeId(ilium_core::ROOT_ID.0));
        assert_eq!(root.parent, None);
        assert!(matches!(root.class, NodeClass::Container));
        assert_eq!(root.content_kind, None);
    }
}
