//! ilium-client: the `ratatui` TUI, a thin renderer/input-dispatcher over
//! `ilium-ipc` -- see `app.rs`'s module docs for the render-cache
//! architecture this crate is built around, and the workspace root
//! `CLAUDE.md` / ARCHITECTURE.md "Process architecture" for how this fits the
//! client/server split as a whole.
//!
//! Module map:
//! - [`app`] -- `App`: render-cache state, input-mode state machine, and
//!   the domain-ish "what happens when X occurs" methods `keys`/`mouse`
//!   dispatch into.
//! - [`config`] -- loads `config.toml`'s client-side `[keybindings]`/
//!   `[theme]` tables and merges them onto `keymap`/`theme`'s defaults;
//!   `run` installs the result once at startup.
//! - [`connection`] -- owns the session's UDS connection (reader/writer
//!   tasks).
//! - [`render_cache`] -- applies incoming `ServerEvent`s to `App`.
//! - [`keys`] / [`mouse`] -- crossterm input dispatch, by `App::mode`.
//! - [`tick`] -- periodic (non-input-driven) maintenance.
//! - [`naming_workers`] -- background provider-neutral title
//!   inference (`std::thread`, bridged into the tokio event loop).
//! - [`run`] -- the actual entry point: terminal lifecycle, connects,
//!   drives the event loop until the user quits or the connection drops.
//!
//! Everything else (`ui`, `tree_ui`, `modal`, `help`, `theme`, `layout`,
//! `settings_ui`, `text_prompt`, `explorer_overlay`, `editor_pane` and its
//! chrome/highlight/toolbar/syntax/minimap helpers, `markdown`, `keymap`,
//! `session_naming`, `project_naming`, `transcript_context`,
//! `project_config`, `workspace_file`, `naming`, `restructure`) is
//! presentation or local-file-I/O logic that doesn't care whether its
//! data came from a local `Tree` or a render-cache mirror of one.

pub mod agent_config_writer;
pub mod agent_debug_export;
pub mod agent_debug_ui;
pub mod agent_feature_setup;
pub mod agent_from_line;
pub mod agent_history_path;
pub mod agent_monitoring;
pub mod agent_prompt_transcript;
pub mod agent_toolbar;
mod animation_hover;
mod animation_plugins;
mod animation_rows;
mod animation_settings_ui;
mod animation_visibility;
pub mod app;
pub mod ascii_chart;
#[cfg(test)]
mod background_acceptance;
pub mod background_animation;
pub mod background_composition;
pub mod board;
pub mod board_ui;
pub mod chatroom;
pub mod chatroom_ui;
pub mod compaction_app;
pub mod compaction_report;
pub mod compaction_scan;
pub mod compaction_ui;
pub mod completed_agent_action;
pub mod config;
pub mod connection;
pub mod control;
pub mod cost_app;
pub mod cost_history;
pub mod cost_model;
pub mod cost_overlay;
pub mod cost_settings;
pub mod cost_settings_ui;
pub mod cost_tracker;
pub mod debug_logging;
pub mod document_preparation;
mod editor_capture_budget;
pub mod editor_chrome;
pub mod editor_highlight;
pub mod editor_line_path;
pub mod editor_pane;
pub mod editor_toolbar;
pub mod error;
pub mod execution;
pub mod explorer_overlay;
mod external_open;
pub mod filesystem;
pub mod help;
pub mod icon_search_workers;
pub mod icon_settings;
mod incoming_projection;
pub mod inference_test;
mod instruction_settings;
mod ipc_preparation;
pub mod keymap;
pub mod keys;
pub mod last_prompt_banner;
pub mod layout;
mod location_picker;
pub mod markdown;
pub mod media_control;
pub mod minimap;
pub mod modal;
mod model_catalog_preparation;
pub mod mouse;
pub mod naming;
pub mod naming_workers;
mod normal_voice;
pub mod onboarding;
pub mod open_target;
pub mod outbound_requests;
pub mod pane_title;
pub mod paths;
pub mod popover;
pub mod presentation;
pub mod progress_bar;
pub mod progress_display;
pub mod project_config;
pub mod project_naming;
pub mod prompt_queue;
mod provider_admission; // Bounded IO preflight retains original request ownership.
mod proxy_database;
pub mod release_animation;
pub mod release_embedding;
pub mod remote_compaction_app;
pub mod remote_compaction_dialog;
pub mod remote_compaction_flow;
pub mod remote_compaction_settings;
pub mod remote_compaction_settings_ui;
pub mod remote_compaction_worker;
pub mod render_cache;
pub mod reset_planning;
pub mod restructure;
pub mod scheduled_input;
pub mod screen_transfer;
pub mod search_ui;
pub mod search_workers;
mod semantic_animation;
pub mod session_conversion;
pub mod session_naming;
pub mod session_stats;
pub mod session_stats_popover;
pub mod session_stats_store;
pub mod session_stats_timeline;
pub mod session_stats_ui;
pub mod settings_help;
pub mod settings_ui;
pub mod setup_prompt;
pub mod smart_copy;
pub mod smart_copy_light;
mod smart_copy_selection;
mod startup_dialog;
/// Typed source custody returned when light-copy shutdown cannot finish.
pub use smart_copy_selection::{
    RestoredSelection, SelectionCompletion, SelectionShutdownCustody, SelectionShutdownErrors,
};
pub mod goal_resume_link;
mod input_backlog;
pub mod smart_copy_tokens;
pub mod smart_copy_workers;
mod source_line_facts;
mod source_stream;
mod source_window_preparation;
mod source_window_surface;
mod source_window_syntax;
mod source_windows;
pub mod split_layout;
pub mod status_icons;
pub mod syntax;
pub mod terminal_activity;
pub mod terminal_clipboard;
pub mod terminal_context_menu;
mod terminal_context_preparation;
pub mod terminal_guard;
mod terminal_input;
mod terminal_input_owner;
pub mod terminal_links;
pub mod terminal_naming;
pub mod terminal_parsing;
pub mod terminal_selection;
pub mod terminal_title_inference;
pub mod terminal_view;
pub mod text_prompt;
pub mod text_trigger_dialog;
pub mod theme;
pub mod tick;
pub mod title_inference;
pub mod transcript_context;
pub mod tree_ordering;
pub mod tree_transitions;
pub mod tree_ui;
pub mod trigger_execution_lease;
pub mod trigger_settings;
pub mod trigger_settings_ui;
pub mod ui;
pub mod value_animation;
pub mod value_config;
pub mod value_control;
pub mod value_cost;
pub mod value_detection;
pub mod value_dialog;
pub mod value_dialog_host;
pub mod value_editor;
pub mod value_effort;
pub mod value_icon;
pub mod value_inference;
pub mod value_keyboard;
pub mod value_manager;
pub mod value_number;
pub mod value_plugin;
pub mod value_scene;
pub mod value_settings;
pub mod value_settings_choice;
pub mod value_voice;
pub mod value_worktree;
mod voice_preparation;
mod voice_presentation;
mod voice_retirement;
pub mod voice_settings;
pub mod workspace_file;
pub mod worktree_dialog;
pub mod worktree_manager;

#[cfg(test)]
mod performance_tests;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::event::Event;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::Terminal;
use tokio::sync::mpsc;

pub use crate::app::ClientExitReason;
pub use crate::execution::{bootstrap_process_quota, bootstrap_runtime_admission};

use crate::app::{App, PaneRuntime};
use crate::connection::Connection;
use crate::error::ClientError;
use crate::icon_search_workers::IconSearchWorkers;
use crate::naming_workers::{NamingWorkerEvent, NamingWorkers};
use crate::search_workers::SearchWorkers;
use crate::smart_copy_workers::{SmartCopyWorkerUpdate, SmartCopyWorkers};
use crate::terminal_guard::TerminalGuard;
use crate::trigger_execution_lease::TriggerExecutionLease;

/// Overrides the home directory whose global agent instruction files
/// (`~/.claude/CLAUDE.md`, `~/.codex/AGENTS.md`) automatic agent setup
/// maintains. Test harnesses point it at a temporary directory so a smoke run
/// never rewrites the developer's real instruction files.
pub const AGENT_SETUP_HOME_ENV: &str = "ILIUM_AGENT_SETUP_HOME";

/// Everything [`run`] needs to attach to one already-running
/// `ilium-server` session and start rendering it.
pub struct RunOptions {
    /// Explicit guided-setup request from `--onboarding`.
    pub onboarding: bool,
    pub session_name: String,
    pub session_cwd: PathBuf,
    /// The CLI resolves the project-scoped session socket before handing
    /// control to the TUI, so a client can never accidentally derive the
    /// machine-wide socket belonging to a different project.
    pub socket_path: PathBuf,
    /// Timestamped path selected once when this detached server process began.
    /// Every attached client appends to the same session-lifetime stream.
    pub log_path: PathBuf,
}

/// Naming-worker results are one-shot per worker (see `naming_workers.rs`),
/// so this only needs enough headroom to never be the bottleneck; it's
/// bounded at all purely for consistency with every other channel in this
/// crate, not because it could plausibly fill up.
const NAMING_EVENTS_CHANNEL_CAPACITY: usize = 16;
/// Semantic icon queries are revisioned, so a tiny bounded channel is enough:
/// the worker drains superseded typing before it runs the next CPU inference.
/// Upper bound for server events applied in one select turn. Terminal output
/// can arrive continuously; yielding after a bounded batch gives pointer and
/// keyboard events a reliable chance to run instead of making hover depend
/// on how chatty the displayed PTY happens to be.
const MAX_SERVER_EVENTS_PER_BATCH: usize = 16;
const MAX_PENDING_SERVER_EVENTS: usize = 256;
/// Byte ceiling for a same-pane raw-output run merged in one client turn.
/// Event count alone is insufficient because a server frame can itself carry
/// a large burst.
#[cfg(test)]
const MAX_MERGED_SCREEN_BYTES_PER_BATCH: usize = 64 * 1024;
/// Input is intentionally favoured over render-cache updates, but it remains
/// bounded so a key-repeat or pointer flood cannot starve incoming terminal
/// frames forever.
const MAX_INPUT_EVENTS_PER_BATCH: usize = 64;
/// Terminal output is visual state, not input acknowledgement. Capping its
/// redraw cadence at 30 Hz leaves CPU for parsing and input while every
/// keyboard/pointer-driven change still bypasses this limit immediately.
const OUTPUT_FRAME_INTERVAL: Duration = Duration::from_millis(33);

/// Remaining delay before output-only damage may trigger another draw.
fn output_redraw_delay(now: Instant, last_draw_at: Instant) -> Duration {
    OUTPUT_FRAME_INTERVAL.saturating_sub(now.saturating_duration_since(last_draw_at))
}

/// Input and semantic state changes bypass output-only frame limiting.
fn output_redraw_is_due(needs_immediate_redraw: bool, now: Instant, last_draw_at: Instant) -> bool {
    needs_immediate_redraw || output_redraw_delay(now, last_draw_at).is_zero()
}

/// Event count and byte count are independent fairness limits.
#[cfg(test)]
fn screen_updates_fit(current_bytes: usize, incoming_bytes: usize) -> bool {
    current_bytes.saturating_add(incoming_bytes) <= MAX_MERGED_SCREEN_BYTES_PER_BATCH
}

/// Records the application status boundary once per visible transition. This
/// captures major actions and every user-visible failure across components
/// without making each board, editor, picker, search, or settings branch own a
/// second logging policy that can drift from what the user actually sees.
fn record_status_message_change(
    status_message: Option<&str>,
    last_recorded_status_message: &mut Option<String>,
) {
    if status_message == last_recorded_status_message.as_deref() {
        return;
    }
    match status_message {
        Some(message) if status_message_is_failure(message) => {
            tracing::error!(status_message = %message, "client action reported a failure");
        }
        Some(message) => {
            tracing::info!(status_message = %message, "client status changed");
        }
        None if last_recorded_status_message.is_some() => {
            tracing::info!("client status cleared");
        }
        None => {}
    }
    *last_recorded_status_message = status_message.map(str::to_owned);
}

fn status_message_is_failure(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    normalized.starts_with("could not ")
        || normalized.starts_with("failed ")
        || normalized.starts_with("save failed")
        || normalized.contains(" error:")
        || normalized.contains(" failed:")
        || normalized.ends_with(" unexpectedly")
        || normalized.contains(" lost its ")
}

/// Records semantic client-surface changes rather than raw key or mouse
/// traffic. Settings includes its active tab so Debug/Inference/Voice visits
/// remain visible while typing and pointer motion stay out of the major-action
/// trail.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ClientSurfaceKey {
    mode_label: &'static str,
    settings_tab_label: Option<&'static str>,
}

/// Damage produced by one bounded server-event batch. Terminal bytes always
/// update their parser cache, but only visible panes damage terminal cells.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ServerEventDamage {
    needs_redraw: bool,
    needs_immediate_redraw: bool,
}

/// Classifies whether one non-output server event can change pixels in the
/// current client. Most semantic events remain immediate; high-frequency
/// activity revisions and debug-log appends are narrowed to their actual
/// visible edge so background panes cannot force redundant full draws.
fn server_event_damage(app: &App, event: &ilium_ipc::ServerEvent) -> ServerEventDamage {
    use ilium_ipc::ServerEvent;

    match event {
        ServerEvent::NodeActivityChanged {
            node_id,
            activity_revision,
        } => {
            let becomes_unread = app.tree.get(*node_id).is_some_and(|node| {
                *activity_revision > node.activity_revision
                    && !node.has_activity_since_focus()
                    && !app.is_pane_displayed(*node_id)
            });
            ServerEventDamage {
                needs_redraw: becomes_unread,
                needs_immediate_redraw: false,
            }
        }
        ServerEvent::NodeFocusCheckpointChanged { node_id, .. } => {
            let clears_unread = app.tree.get(*node_id).is_some_and(|node| {
                node.has_activity_since_focus() && !app.is_pane_displayed(*node_id)
            });
            ServerEventDamage {
                needs_redraw: clears_unread,
                needs_immediate_redraw: false,
            }
        }
        ServerEvent::PaneDebugLogSnapshot { pane_id, .. }
        | ServerEvent::PaneDebugEntryAppended { pane_id, .. } => {
            let is_visible = matches!(
                &app.mode,
                crate::app::Mode::AgentDebugLog(state) if state.pane_id == *pane_id
            );
            ServerEventDamage {
                needs_redraw: is_visible,
                needs_immediate_redraw: is_visible,
            }
        }
        ServerEvent::TerminalReplay { pane_id, .. } => ServerEventDamage {
            needs_redraw: app.is_pane_displayed(*pane_id),
            needs_immediate_redraw: false,
        },
        _ => ServerEventDamage {
            needs_redraw: true,
            needs_immediate_redraw: true,
        },
    }
}

fn client_surface_key(app: &App) -> ClientSurfaceKey {
    ClientSurfaceKey {
        mode_label: crate::control::mode_label(&app.mode),
        settings_tab_label: match &app.mode {
            crate::app::Mode::Settings(settings) => Some(settings.tab.label()),
            _ => None,
        },
    }
}

fn record_client_surface_change(app: &App, last_recorded_surface: &mut Option<ClientSurfaceKey>) {
    let surface = client_surface_key(app);
    if *last_recorded_surface == Some(surface) {
        return;
    }
    let surface_label = match surface.settings_tab_label {
        Some(settings_tab_label) => format!("{}:{settings_tab_label}", surface.mode_label),
        None => surface.mode_label.to_owned(),
    };
    tracing::info!(client_surface = %surface_label, "client surface changed");
    *last_recorded_surface = Some(surface);
}

/// Runs ilium-client until the user quits, requests a client-only restart, or
/// loses the server connection. The typed result lets the CLI wrapper re-exec
/// only for the explicit restart path after this function restores the terminal.
pub async fn run(options: RunOptions) -> Result<ClientExitReason, ClientError> {
    let process_quota = bootstrap_process_quota().map_err(ClientError::ProcessResources)?;
    if !options.session_cwd.is_dir() {
        return Err(ClientError::InvalidSessionCwd(options.session_cwd));
    }

    // One background probe per process decides whether the GPU option of the
    // animation scenes is usable; it never blocks start-up.
    ilium_gpu::start_probe();

    // Resolved and installed once, before the terminal enters raw/
    // alternate-screen mode and before any render call -- see
    // `theme::THEME`'s doc comment on why a one-time `OnceLock` init is only
    // safe this early. `config_dir` is threaded through to `run_inner` too:
    // the settings screen (`crate::app::Mode::Settings`) needs it later to
    // persist a change (`crate::config::save_ui_settings`).
    let config_dir_result = crate::paths::config_dir();
    let config_exists = config_dir_result
        .as_ref()
        .ok()
        .is_some_and(|path| path.join("config.toml").exists());
    let file_logging_enabled_hint = config_dir_result
        .as_ref()
        .ok()
        .and_then(|config_dir| {
            ilium_logging::file_logging_enabled_hint(&config_dir.join("config.toml"))
                .ok()
                .flatten()
        })
        .unwrap_or(false);
    ilium_logging::initialize(
        &options.log_path,
        file_logging_enabled_hint,
        "client",
        &process_quota,
    )?;
    ilium_logging::install_panic_logging();
    let (mut config, config_dir, is_agent_setup_policy_available) =
        init_config(config_dir_result, file_logging_enabled_hint);
    let should_open_onboarding = config
        .onboarding
        .should_open(options.onboarding, config_exists);
    if should_open_onboarding {
        config.onboarding.begin();
    }
    ilium_logging::set_enabled(config.debug.file_logging_enabled)?;
    // Take the terminal before the slow discovery below, so the user sees a
    // progress dialog instead of an unresponsive blank screen. Input is
    // claimed first, as before: the reader owns the image query and all input.
    let input_reservation = terminal_input_owner::InputReservation::prepare(&process_quota)?;
    let terminal_guard = TerminalGuard::enter().map_err(|error| {
        tracing::error!(%error, error_debug = ?error, "failed to enter terminal UI mode");
        error
    })?;
    let guard = InputTerminalGuard {
        _guard: terminal_guard,
        _session: input_reservation.session_claim(),
    };
    let mut startup_dialog = crate::startup_dialog::StartupDialog::new();
    startup_dialog.show("Starting ilium", "Reading settings", Some(0.05));
    if config.inference.kilo_gateway.paid_proxies_enabled
        && !should_open_onboarding
        && config.onboarding.automatic_ai_allowed()
    {
        proxy_database::load_paid_proxies(&mut config.inference.kilo_gateway).await?;
    }
    tracing::info!(
        session_name = options.session_name,
        session_cwd = %options.session_cwd.display(),
        log_path = %options.log_path.display(),
        "ilium-client starting"
    );
    startup_dialog.show("Starting ilium", "Finding system sounds", Some(0.15));
    let sound_discovery = ilium_sound::discover_system_sounds();
    startup_dialog.show(
        "Starting ilium",
        "Detecting audio input devices",
        Some(0.35),
    );
    let voice_input_devices = ilium_voice::available_input_devices().unwrap_or_else(|error| {
        tracing::warn!(%error, "failed to enumerate voice input devices");
        Vec::new()
    });
    startup_dialog.show(
        "Starting ilium",
        "Detecting audio output devices",
        Some(0.55),
    );
    let voice_output_devices = ilium_voice::available_output_devices().unwrap_or_else(|error| {
        tracing::warn!(%error, "failed to enumerate voice output devices");
        Vec::new()
    });

    let result = run_inner(
        &options,
        PreparedStartup {
            config,
            config_dir,
            should_open_onboarding,
            is_agent_setup_policy_available,
            sound_discovery,
            voice_devices: VoiceDeviceCatalog {
                input: voice_input_devices,
                output: voice_output_devices,
            },
            guard,
            input_reservation,
            startup_dialog,
        },
    )
    .await;
    if let Err(error) = &result {
        tracing::error!(%error, error_debug = ?error, "ilium-client exited with an error");
    } else {
        tracing::info!("ilium-client stopped");
    }
    result
}

/// Resolves `config.toml`'s client-side tables and installs the effective
/// theme for the rest of the process's lifetime, also returning the full
/// resolved config (its `[keybindings]` table becomes `App::keybindings` --
/// the single live table both input dispatch and the help screen read, see
/// `keymap`'s module doc -- and its `[ui]` table feeds
/// `App::apply_ui_settings`) and the config directory (for later writes --
/// see `App::config_dir`). A config directory that can't be resolved, or a
/// config file that fails to load, is logged and falls back to defaults
/// rather than refusing to start the client over it -- matches
/// `ilium-server::main`'s own "a bad optional config file is a warning,
/// not a fatal error" policy. An unresolvable config directory means
/// settings changes can still apply live this session, just never persist
/// (see `App::config_dir`'s doc comment).
fn init_config(
    config_dir_result: Result<PathBuf, ClientError>,
    fallback_debug_logging_enabled: bool,
) -> (crate::config::ClientConfig, Option<PathBuf>, bool) {
    let mut is_agent_setup_policy_available = true;
    let config_dir = match config_dir_result {
        Ok(dir) => Some(dir),
        Err(error) => {
            tracing::warn!("failed to resolve config directory, using defaults: {error}");
            is_agent_setup_policy_available = false;
            None
        }
    };
    let config = match &config_dir {
        Some(dir) => match crate::config::load(dir) {
            Ok(config) => config,
            Err(error) => {
                tracing::warn!("failed to load config, using defaults: {error}");
                is_agent_setup_policy_available = false;
                let mut config = crate::config::ClientConfig::default();
                config.debug.file_logging_enabled = fallback_debug_logging_enabled;
                config
            }
        },
        None => {
            let mut config = crate::config::ClientConfig::default();
            config.debug.file_logging_enabled = fallback_debug_logging_enabled;
            config
        }
    };
    crate::theme::init(config.theme);
    (config, config_dir, is_agent_setup_policy_available)
}

struct VoiceDeviceCatalog {
    input: Vec<String>,
    output: Vec<String>,
}

// Field order restores terminal state before releasing the session share,
// including when a failed presenter start drops its cleanup closure.
struct InputTerminalGuard {
    _guard: TerminalGuard,
    _session: terminal_input_owner::InputSession,
}

/// Resources and policy resolved before transferring terminal ownership.
struct PreparedStartup {
    config: crate::config::ClientConfig,
    config_dir: Option<PathBuf>,
    should_open_onboarding: bool,
    is_agent_setup_policy_available: bool,
    sound_discovery: ilium_sound::SoundDiscovery,
    voice_devices: VoiceDeviceCatalog,
    guard: InputTerminalGuard,
    input_reservation: terminal_input_owner::InputReservation,
    startup_dialog: crate::startup_dialog::StartupDialog,
}

async fn run_inner(
    options: &RunOptions,
    startup: PreparedStartup,
) -> Result<ClientExitReason, ClientError> {
    let PreparedStartup {
        config,
        config_dir,
        should_open_onboarding,
        is_agent_setup_policy_available,
        sound_discovery,
        voice_devices,
        guard,
        input_reservation,
        mut startup_dialog,
    } = startup;
    startup_dialog.show("Starting ilium", "Starting background workers", Some(0.7));
    let VoiceDeviceCatalog {
        input: voice_input_devices,
        output: voice_output_devices,
    } = voice_devices;
    // Only the presenter owns terminal diffing, encoding and writes. The UI
    // terminal is an inert composition surface; get_frame does not run a diff.
    let (columns, rows) = crossterm::terminal::size().map_err(ClientError::TerminalSetup)?;
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(columns, rows))
        .unwrap_or_else(|error| match error {});

    let execution = crate::execution::ClientExecution::start_async()
        .await
        .map_err(|error| ClientError::TerminalSetup(std::io::Error::other(error)))?;
    let mut app = App::new(options.session_name.clone(), options.session_cwd.clone());
    let outbound_admission = execution
        .client(crate::ipc_preparation::request_limits())
        .map_err(|error| {
            ClientError::TerminalSetup(std::io::Error::other(format!(
                "outbound admission startup: {error:?}"
            )))
        })?;
    app.outbound_admission = Some(outbound_admission.clone());
    app.location_search_client = Some(execution.location_search());
    let catalogue_client = execution
        .client(ilium_execution::ClientLimits {
            jobs: 1,
            service_jobs: 0,
            input_bytes: 4 * 1024 * 1024,
            result_bytes: 64 * 1024 * 1024,
        })
        .map_err(|error| {
            ClientError::TerminalSetup(std::io::Error::other(format!(
                "animation catalogue startup: {error:?}"
            )))
        })?;
    let catalogue_preparation =
        crate::animation_plugins::preparation::CataloguePreparation::new(catalogue_client);
    let catalogue_notification = catalogue_preparation.notification();
    app.plugin_catalogue_preparation = Some(catalogue_preparation);
    let documents = execution.documents().map_err(|error| {
        ClientError::TerminalSetup(std::io::Error::other(format!(
            "document preparation startup: {error:?}"
        )))
    })?;
    let document_notification = app.configure_document_preparation(documents);
    let context_client = execution
        .client(ilium_execution::ClientLimits {
            jobs: 4,
            service_jobs: 0,
            input_bytes: 384 * 1024 * 1024,
            result_bytes: 256 * 1024 * 1024,
        })
        .map_err(|error| {
            ClientError::TerminalSetup(std::io::Error::other(format!(
                "context preparation startup: {error:?}"
            )))
        })?;
    let context_preparation =
        crate::terminal_context_preparation::TerminalContextPreparation::new(context_client);
    let context_notification = context_preparation.notification();
    app.terminal_context_preparation = Some(context_preparation);
    let external_open_client =
        execution
            .client(crate::external_open::limits())
            .map_err(|error| {
                ClientError::TerminalSetup(std::io::Error::other(format!(
                    "external opening client startup: {error:?}"
                )))
            })?;
    let external_open = crate::external_open::ExternalOpenService::start(external_open_client)
        .map_err(ClientError::TerminalSetup)?;
    let external_open_notification = external_open.notification();
    app.external_open = Some(external_open);
    let clipboard_client = execution
        .client(ilium_execution::ClientLimits {
            jobs: 8,
            service_jobs: 0,
            input_bytes: 64 * 1024 * 1024 + 64 * 1024,
            result_bytes: 128 * 1024 * 1024,
        })
        .map_err(|error| {
            ClientError::TerminalSetup(std::io::Error::other(format!(
                "clipboard client startup: {error:?}"
            )))
        })?;
    let clipboard = crate::terminal_clipboard::ClipboardService::start(clipboard_client)
        .map_err(ClientError::TerminalSetup)?;
    let clipboard_notification = clipboard.notification();
    app.terminal_clipboard = Some(clipboard);
    let selection_client = execution
        .client(crate::smart_copy_selection::limits())
        .map_err(|error| {
            ClientError::TerminalSetup(std::io::Error::other(format!(
                "Light selection admission: {error:?}"
            )))
        })?;
    let selection = crate::smart_copy_selection::SelectionOwner::new(selection_client);
    let selection_notification = selection.notification();
    app.light_copy_selection = Some(selection);
    let parsing = crate::terminal_parsing::TerminalParsing::start(
        execution.terminal_parser().map_err(|error| {
            ClientError::TerminalSetup(std::io::Error::other(format!(
                "terminal parser client: {error:?}"
            )))
        })?,
        app.terminal_settings.engine_memory_budget_mib,
    )
    .map_err(|error| ClientError::TerminalSetup(std::io::Error::other(error)))?;
    let terminal_notification = parsing.notification();
    app.terminal_parsing = Some(parsing);
    let filesystem_client = execution
        .client(ilium_execution::ClientLimits {
            jobs: 16,
            service_jobs: 0,
            input_bytes: 384 * 1024 * 1024,
            result_bytes: 256 * 1024 * 1024,
        })
        .map_err(|error| {
            ClientError::TerminalSetup(std::io::Error::other(format!(
                "filesystem startup: {error:?}"
            )))
        })?;
    let statistics_client = execution
        .client(ilium_execution::ClientLimits {
            jobs: 24,
            service_jobs: 0,
            input_bytes: 384 * 1024 * 1024,
            result_bytes: 384 * 1024 * 1024,
        })
        .map_err(|error| {
            ClientError::TerminalSetup(std::io::Error::other(format!(
                "statistics startup: {error:?}"
            )))
        })?;
    let statistics_notification = std::sync::Arc::new(tokio::sync::Notify::new());
    let statistics_wake = std::sync::Arc::clone(&statistics_notification);
    let statistics_client =
        statistics_client.with_completion_wake(move || statistics_wake.notify_one());
    app.session_stats
        .configure_execution(statistics_client.clone());
    app.compaction_optimizer
        .configure_execution(statistics_client.clone());
    app.cost_tracker.configure_execution(statistics_client);
    let editor_files = crate::filesystem::editors::EditorFiles::new(filesystem_client.clone());
    let filesystem_notification = editor_files.notification();
    let filesystem_admission_notification = crate::execution::admission_notification();
    app.startup_progress.configure(
        filesystem_client.clone(),
        std::sync::Arc::clone(&filesystem_notification),
    );
    app.terminal_baselines = Some(crate::filesystem::transcript_baseline::BaselineFiles::new(
        filesystem_client.clone(),
        std::sync::Arc::clone(&filesystem_notification),
    ));
    app.integration_files = Some(crate::filesystem::integrations::IntegrationFiles::new(
        filesystem_client.clone(),
        std::sync::Arc::clone(&filesystem_notification),
    ));
    app.sidebar_files = Some(crate::filesystem::sidebar::SidebarFiles::new(
        filesystem_client.clone(),
        std::sync::Arc::clone(&filesystem_notification),
    ));
    app.explorer_execution = Some((
        filesystem_client.clone(),
        std::sync::Arc::clone(&filesystem_notification),
    ));
    app.configuration_files = Some(crate::filesystem::configurations::ConfigurationFiles::new(
        filesystem_client.clone(),
        std::sync::Arc::clone(&filesystem_notification),
    ));
    let voice_retirement_client = filesystem_client.clone();
    app.voice_native_retirement = Some(crate::voice_retirement::VoiceRetirement::new(
        voice_retirement_client.clone(),
    ));
    app.voice_preparation
        .configure(voice_retirement_client.clone());
    let normal_voice_preparation_notification = app.voice_preparation.notification();
    app.board_files = Some(crate::filesystem::boards::BoardFiles::new(
        filesystem_client,
        std::sync::Arc::clone(&filesystem_notification),
    ));
    app.editor_files = Some(editor_files);
    app.agent_setup_home_dir = std::env::var_os(AGENT_SETUP_HOME_ENV)
        .map(std::path::PathBuf::from)
        .or_else(|| {
            directories::BaseDirs::new().map(|directories| directories.home_dir().to_path_buf())
        });
    app.is_agent_setup_policy_available = is_agent_setup_policy_available;
    // Keep supervised service owners outside the event-loop future so every
    // error path restores the terminal before awaiting their actual exit.
    let (naming_events_tx, mut naming_events_rx) = mpsc::channel(NAMING_EVENTS_CHANNEL_CAPACITY);
    let provider_notification = std::sync::Arc::new(tokio::sync::Notify::new());
    let provider_client = execution
        .client(ilium_execution::ClientLimits {
            jobs: 16,
            service_jobs: 0,
            input_bytes: 384 * 1024 * 1024,
            result_bytes: 256 * 1024 * 1024,
        })
        .map_err(|error| {
            ClientError::TerminalSetup(std::io::Error::other(format!(
                "provider work startup: {error:?}"
            )))
        })?;
    let provider_wake = std::sync::Arc::clone(&provider_notification);
    let provider_client = provider_client.with_completion_wake(move || provider_wake.notify_one());
    app.model_catalog_preparation
        .configure(provider_client.clone());
    let mut naming_workers =
        NamingWorkers::new(naming_events_tx, &config.inference).map_err(|reason| {
            ClientError::TerminalSetup(std::io::Error::other(format!(
                "naming settings startup: {reason:?}"
            )))
        })?;
    naming_workers.configure_execution(provider_client.clone());
    let (smart_copy_events_tx, mut smart_copy_events_rx) = mpsc::channel(64);
    let mut smart_copy_workers = SmartCopyWorkers::new(smart_copy_events_tx);
    smart_copy_workers.configure_execution(provider_client.clone());
    smart_copy_workers.set_completion_notification(std::sync::Arc::clone(&provider_notification));
    let media_owner =
        crate::media_control::MediaOwner::start().map_err(ClientError::TerminalSetup)?;
    let mut icon_search_workers = IconSearchWorkers::new();
    let ambient_resources = execution.ambient_resources().map_err(|error| {
        ClientError::TerminalSetup(std::io::Error::other(format!(
            "ambient resources startup: {error:?}"
        )))
    })?;
    app.animation_frame.configure_resources(ambient_resources);
    let keyboard_enhancement_pushed = guard._guard.keyboard_enhancement_state();
    // The presenter does not emit a frame until the owned query resolves.
    let backend =
        CrosstermBackend::new(crate::presentation::TerminalOutput::new(std::io::stdout()));
    let mut presenter = crate::presentation::Presenter::start_with_cleanup(
        backend,
        &crate::execution::process_quota(),
        move || drop(guard),
    )
    .map_err(ClientError::TerminalSetup)?;
    let animation_notification = app.animation_frame.notification();
    let animation_admission_notification = app.animation_frame.admission_notification();
    // Single-flight presentation bounds this to one exact frame token. A token
    // remains here until successful emission or an explicit failed-frame exit.
    let mut animation_presentations: std::collections::VecDeque<(
        u64,
        crate::app::EmittedGeometry,
    )> = std::collections::VecDeque::new();
    let mut voice_service = None;
    let mut pending_voice_event = None;
    let mut voice_event_retry_at = None;
    let mut onboarding_voice =
        crate::onboarding::voice_runtime::VoiceDemoRuntime::new(voice_retirement_client.clone());
    let onboarding_voice_notification = onboarding_voice.notification();
    let mut pending_demo_event = None;
    let mut demo_event_retry_at = None;
    let mut demonstration_retirement_failed = false;
    let mut input_owner = None;
    let mut input_failure = None;
    let mut deferred_input = input_backlog::InputBacklog::default();
    let result = async {
    let mut presentation_frame_id = 0_u64;
    let mut presentation_layout_revision = 0_u64;

    app.apply_ui_settings(config.ui);
    match crate::project_config::load(&app.session_cwd) {
        Ok(project_config) => {
            app.ui_settings.show_project_separators = project_config.show_project_separators;
        }
        Err(error) => {
            tracing::warn!(%error, "failed to load project-scoped UI settings");
        }
    }
    // The animation is one global preference, not a per-project one.
    if let Some(home) = crate::project_config::global_animation_home() {
        app.animation_home = home;
    }
    if let Err(error) =
        crate::project_config::seed_global_animation(&app.animation_home, &app.session_cwd)
    {
        tracing::warn!(%error, "could not adopt the launch project's animation as the global one");
    }
    let animation_home = app.animation_home.clone();
    match crate::project_config::load(&animation_home) {
        Ok(animation_config) => {
            app.install_animation_project_settings(animation_home, Ok(animation_config.animation));
        }
        Err(error) => {
            tracing::warn!(%error, "failed to load animation settings");
            app.install_animation_project_settings(
                animation_home,
                Err(format!("Could not load animation settings: {error}")),
            );
        }
    }
    app.keyboard_settings = config.keyboard;
    app.keybindings = config.keybindings;
    app.kanban_board_settings = config.kanban_board;
    app.apply_sound_settings(config.sound);
    app.apply_notification_settings(config.notifications);
    app.apply_inference_settings(config.inference);
    app.apply_trigger_settings(config.triggers);
    app.apply_text_trigger_settings(config.text_triggers);
    app.apply_agent_setup_settings(config.agent_setup);
    app.apply_terminal_settings(config.terminal);
    app.apply_editor_settings(config.editor);
    app.apply_session_settings(config.session);
    app.apply_git_settings(config.git);
    app.apply_voice_settings(config.voice);
    app.apply_reset_planning_settings(config.reset_planning);
    app.apply_cost_settings(config.cost);
    app.apply_remote_compaction_settings(config.remote_compaction);
    app.apply_debug_settings(config.debug);
    app.apply_api_settings(config.api);
    app.request_debug_logging_reconciliation();
    app.request_agent_debug_menu_reconciliation();
    app.sound_discovery = sound_discovery;
    app.voice_input_devices = voice_input_devices;
    app.voice_output_devices = voice_output_devices;
    app.config_dir = config_dir;
    app.onboarding_progress = config.onboarding;
    if should_open_onboarding {
        crate::onboarding::integration::open(&mut app, options.onboarding);
    }
    app.set_screen_area(Rect::new(0, 0, columns, rows));

    startup_dialog.show("Starting ilium", "Connecting to the session server", Some(0.85));
    // Ordered codecs use the existing CPU bank. Their directional groups
    // bypass the independently bounded general aggregate while sharing its root.
    // The server accepts connections only after restoring its session, which
    // can take a long time; keep the dialog current from its progress file.
    let startup_progress_file = ilium_ipc::startup_progress_path(&options.socket_path);
    let connecting = Connection::connect_admitted(
        &options.socket_path,
        options.session_name.clone(),
        &execution,
        outbound_admission,
    );
    tokio::pin!(connecting);
    let mut connecting_tick = tokio::time::interval(Duration::from_millis(100));
    let mut connection = loop {
        tokio::select! {
            result = &mut connecting => break result?,
            _ = connecting_tick.tick() => startup_dialog.show_server_progress(
                ilium_ipc::read_startup_progress(&startup_progress_file).as_ref(),
            ),
        }
    };
    let mut trigger_execution_lease = TriggerExecutionLease::open(&options.socket_path);
    // Tells the server this connection hosts the voice session, so
    // `ilium voice say` can offer typed sentences to it. One-shot CLI
    // connections never send this.
    app.pending_voice_receiver_registration = true;
    reconcile_voice_receiver_registration(&mut app);

    naming_workers.set_automatic_ai_decision(
        app.onboarding_revision,
        app.onboarding.is_none() && app.onboarding_progress.automatic_ai_allowed(),
    );
    let (conversion_events_tx, mut conversion_events_rx) = mpsc::channel(256);
    let mut conversion_workers =
        crate::session_conversion::ConversionWorkers::new(conversion_events_tx);
    let (remote_compaction_events_tx, mut remote_compaction_events_rx) = mpsc::channel(256);
    let mut remote_compaction_workers =
        crate::remote_compaction_worker::RemoteCompactionWorkers::new(remote_compaction_events_tx);
    let search_client = execution.client(ilium_execution::ClientLimits { jobs: 2, service_jobs: 0, input_bytes: 384 * 1024 * 1024, result_bytes: 64 * 1024 * 1024 })
        .map_err(|error| ClientError::TerminalSetup(std::io::Error::other(format!("workspace search startup: {error:?}"))))?;
    let mut search_workers = SearchWorkers::new(search_client);
    let search_notification = search_workers.notification();
    let icon_search_notification = icon_search_workers.notification();
    let (reset_events_tx, mut reset_events_rx) = mpsc::channel(4);
    let (reset_settings_tx, reset_settings_rx) =
        tokio::sync::watch::channel(app.reset_planning_settings.clone());
    let reset_monitor = crate::reset_planning::spawn_monitor(reset_settings_rx, reset_events_tx);
    let control_client = execution.client(ilium_execution::ClientLimits {
        jobs: 20, service_jobs: 0, input_bytes: 384 * 1024 * 1024, result_bytes: 256 * 1024 * 1024,
    }).map_err(|error| ClientError::TerminalSetup(std::io::Error::other(format!("control preparation startup: {error:?}"))))?;
    let mut control_plane = crate::control::ControlPlane::new(control_client);
    let control_notification = control_plane.notification().ok_or_else(|| ClientError::TerminalSetup(std::io::Error::other("control preparation notification unavailable")))?;
    let demonstration_media = media_owner.demonstration();
    voice_service = if app.voice_settings.enabled && app.onboarding.is_none() {
        start_voice_service(&mut app, &control_plane)
    } else {
        None
    };
    control_plane.synchronize_voice_instance(voice_service.as_ref().map(ilium_voice::VoiceService::instance_identity));
    let mut delivered_voice_target_context = voice_service
        .as_ref()
        .map(|_| crate::control::VoiceTargetContext::capture(&app));
    // The media owner retains actual acknowledged player names; this lease
    // publishes only intent, including voice enabled by a persisted setting.
    let mut normal_media = media_owner.normal();
    normal_media.request(voice_service.is_some() && app.voice_settings.pause_media_while_active);
    // Resolved once at startup (cheap: just reads `$HOME`/the platform's
    // equivalent), rather than per pane -- `None` on a platform/environment
    // where it can't be resolved simply disables session-title inference
    // (its transcript lookups are all rooted under the home directory)
    // instead of failing the whole client.
    let home_dir = directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf());
    // Reading the stored project name is synchronous but cheap; inference
    // (a real HTTP call) only runs in the background when nothing is
    // stored yet, so the first frame draws immediately either way.
    match crate::project_naming::load_stored_project_name(&app.session_cwd) {
        Ok(Some(name)) => {
            app.project_name = Some(name);
            app.project_icon = crate::project_naming::load_stored_project_icon(&app.session_cwd)
                .unwrap_or_else(|error| {
                    tracing::warn!(%error, "failed to load stored project icon");
                    None
                });
        }
        Ok(None) if app.onboarding.is_none() && app.onboarding_progress.automatic_ai_allowed() => {
            app.is_project_name_loading = true;
            app.project_name_attempt_revision = Some(app.onboarding_revision);
            handle_naming_admission(naming_workers.spawn_project_name_worker(app.session_cwd.clone()), &mut app);
        }
        Ok(None) => {}
        Err(error) => {
            tracing::error!(
                error = %error,
                error_debug = ?error,
                "failed to load stored project name"
            );
            app.status_message = Some(format!("Could not infer project name: {error}"));
        }
    }

    let (owner, mut input_rx, picker_receiver) =
        input_reservation.start_with_image_probe(keyboard_enhancement_pushed)?;
    input_owner = Some(owner);
    let picker = picker_receiver
        .await
        .map_err(|_| ClientError::TerminalSetup(std::io::Error::other(
            "terminal image probe worker exited before reporting"
        )))?
        .map_err(ClientError::TerminalSetup)?;
    if let Some(picker) = picker {
        app.install_terminal_image_picker(picker);
    }
    // The interface draws its own dialog from here until the server's state
    // has arrived; start from blank cells.
    startup_dialog.clear();
    app.startup_progress_path = Some(ilium_ipc::startup_progress_path(&options.socket_path));

    // Set so the very first pass through the loop always draws (there's
    // nothing on screen yet); every branch below that actually changes
    // visible state re-sets it, and it's cleared right after a draw
    // happens. See the module docs on why an unconditional `terminal.draw`
    // every iteration was wasted work under load.
    let mut needs_redraw = true;
    let mut needs_immediate_redraw = true;
    let mut last_draw_at = Instant::now();
    let mut last_animation_frame_bucket = None;
    let mut last_onboarding_animation_active = false;
    let mut last_recorded_status_message = None;
    let mut last_recorded_surface = None;
    let mut last_streamed_pane_slots: Option<[Option<ilium_core::NodeId>; 4]> = None;

    'event_loop: while app.exit_reason.is_none() {
        app.begin_editor_capture_turn();
        if let Err(failure) = app.retry_native_terminal_paste() {
            input_failure = Some(failure);
            break;
        }
        if app.pending_native_paste.is_none() {
            if let Some(event) = deferred_input.take_front() {
                if let Some(failure) = dispatch_ready_input_events(
                    &mut app, &mut input_rx, &mut naming_workers, &mut icon_search_workers,
                    home_dir.as_deref(), event, &mut deferred_input,
                ) {
                    input_failure = Some(failure);
                    break;
                }
                needs_redraw = true;
                needs_immediate_redraw = true;
            }
        }
        if app.synchronize_animation_project_settings() {
            needs_redraw = true;
            needs_immediate_redraw = true;
        }
        let now = Instant::now();
        if app.refresh_startup_progress() {
            needs_redraw = true;
        }
        let voice_output_owner = voice_service.as_ref().map(ilium_voice::VoiceService::instance_identity);
        control_plane.synchronize_voice_instance(voice_output_owner.clone());
        let mut voice_tool_outputs = Vec::new();
        let maintenance_schedule = app.maintenance_schedule(now);
        let mut tick_delay = maintenance_schedule.delay;
        if let Some(delay) = onboarding_voice.startup_retry_delay(now) {
            tick_delay = tick_delay.min(delay);
        }
        if let Some(delay) = app.voice_preparation.retry_delay(now) {
            tick_delay = tick_delay.min(delay);
        }
        for delay in [naming_workers.retry_delay(now), smart_copy_workers.retry_delay(now)].into_iter().flatten() {
            tick_delay = tick_delay.min(delay);
        }
        if onboarding_voice.pending_work() || app.voice_preparation.is_pending() || app.normal_voice_commands.is_pending()
            || app.voice_shutdown_requested || app.voice_shutdown_batch.is_some()
            || app.pending_normal_voice_outputs.is_some() || app.normal_voice_context_waiting
            || app.voice_native_retirement.as_ref().is_some_and(|owner| owner.is_pending()) {
            tick_delay = tick_delay.min(Duration::from_millis(100));
        }
        if app.is_startup_dialog_visible() {
            tick_delay = tick_delay.min(Duration::from_millis(80));
        }
        if app.is_displayed_terminal_awaiting_engine() {
            tick_delay = tick_delay.min(Duration::from_millis(50));
        }
        if let Some(delay) = app.source_window_retry_delay(now) { tick_delay = tick_delay.min(delay); }
        if let Some(delay) = app.light_copy_selection.as_ref().and_then(|owner| owner.retry_delay()) { tick_delay = tick_delay.min(delay); }
        if !app.light_copy_recovery.is_empty() || !app.light_copy_restored.is_empty() { tick_delay = tick_delay.min(Duration::from_millis(100)); }
        if crate::onboarding::integration::is_animating(&app) || last_onboarding_animation_active {
            tick_delay = tick_delay.min(Duration::from_millis(33));
        }
        if needs_redraw && !needs_immediate_redraw && presenter.outstanding_frames() == 0 {
            tick_delay = tick_delay.min(output_redraw_delay(now, last_draw_at));
        }
        if let Some(interval) = app
            .reset_monitor_state
            .display_tick_interval(&app.reset_planning_settings)
        {
            tick_delay = tick_delay.min(interval);
        }
        if let Some(delay) = crate::background_composition::animation_frame_delay(
            &app,
            now.saturating_duration_since(app.started_at),
        ) {
            tick_delay = tick_delay.min(delay);
        }

        let onboarding_actor_notification = onboarding_voice.actor_notification();
        tokio::select! {
            () = async {
                match &onboarding_actor_notification {
                    Some(notification) => notification.notified().await,
                    None => std::future::pending().await,
                }
            } => {
                onboarding_voice.collect(Instant::now()).await;
                needs_redraw = true;
            }
            _ = control_notification.notified() => { needs_redraw = true; }
            _ = clipboard_notification.notified() => {needs_redraw=true;}
            _ = selection_notification.notified() => {needs_redraw=true;}

            _ = context_notification.notified() => { needs_redraw=true; }
            _ = external_open_notification.notified() => { needs_redraw |= app.collect_external_open(); }
            _ = document_notification.notified() => { needs_redraw = true; }
            _ = catalogue_notification.notified() => { needs_redraw |= app.collect_plugin_catalogue(); }
            _ = statistics_notification.notified() => {
                needs_redraw |= app.session_stats.drain_events();
                needs_redraw |= app.tick_cost(Instant::now());
                needs_redraw |= app.tick_compaction(Instant::now());
            }
            _ = terminal_notification.notified() => { needs_redraw = true; }
            _ = filesystem_notification.notified() => { needs_redraw |= app.collect_editor_files(); }
            _ = async { match &mut app.projection_admission_wake { Some(wake) => wake.as_mut().await, None => std::future::pending().await } } => {
                app.projection_admission_wake = None;
                app.projection_busy_retry_at = None;
            }
            _ = async { match app.projection_busy_retry_at { Some(deadline) => tokio::time::sleep_until(deadline).await, None => std::future::pending().await } } => {
                app.projection_admission_wake = None;
                app.projection_busy_retry_at = None;
            }
            _ = filesystem_admission_notification.notified() => {needs_redraw |= app.collect_editor_files(); naming_workers.collect(); smart_copy_workers.collect(); needs_redraw |= app.collect_model_catalog_preparation();}
            completion = app.debug_logging.next_completion() => {
                apply_debug_logging_completion(&mut app, completion);
                needs_redraw = true;
                needs_immediate_redraw = true;
            }
            acknowledgement = presenter.acknowledgements.recv() => {
                let mut presented = acknowledgement.ok_or_else(|| ClientError::TerminalSetup(std::io::Error::other("presentation owner exited")))?
                    .map_err(ClientError::TerminalSetup)?;
                if let Some((frame_id, geometry)) = animation_presentations.pop_front() {
                    if frame_id != presented.frame.frame_id {
                        return Err(ClientError::TerminalSetup(std::io::Error::other("animation presentation identity mismatch")));
                    }
                    if let Some(reason) = &presented.uncertainty {
                        drop(geometry);
                        if let Some(animation) = presented.frame.take_animation() {
                            app.animation_frame.uncertain_output(animation, reason);
                        }
                        needs_redraw = true;
                    } else if let Some(reason) = &presented.rejection {
                        drop(geometry);
                        drop(presented.frame.take_animation());
                        app.animation_frame.reject_output(reason);
                        needs_redraw = true;
                    } else {
                        needs_redraw |= app.commit_emitted_geometry(geometry);
                        if let Some(press) = app.take_deferred_pointer_press() {
                            crate::mouse::handle_mouse_event(&mut app, press);
                            needs_redraw = true;
                        }
                        if let Some(animation) = presented.frame.take_animation() {
                            let snapshot = animation.snapshot();
                            tracing::trace!(frame_id, animation_revision = snapshot.revision,
                                animation_sequence = snapshot.sequence,
                                composition_to_emission_us = presented.emitted_at.saturating_duration_since(animation.composed_at()).as_micros() as u64,
                                request_to_emission_us = presented.emitted_at.saturating_duration_since(snapshot.requested_at).as_micros() as u64,
                                completed_frame_age_us = presented.emitted_at.saturating_duration_since(snapshot.completed_at).as_micros() as u64,
                                "animation frame emitted");
                            app.animation_frame.acknowledge(animation, presented.flush_proof.take());
                        }
                    }
                }
                if presented.rejection.is_none() && presented.uncertainty.is_none() {
                    tracing::trace!(frame_id = presented.frame.frame_id, layout_revision = presented.frame.layout_revision,
                        frame_age_us = presented.emitted_at.duration_since(presented.frame.prepared_at).as_micros() as u64,
                        emission_us = presented.emission_duration.as_micros() as u64, "terminal frame emitted");
                }
            }
            _ = animation_notification.notified() => {
                needs_redraw |= app.animation_frame.collect();
            }
            _ = animation_admission_notification.notified() => {
                needs_redraw |= app.animation_frame.collect();
            }
            input_event = input_rx.recv(), if app.pending_native_paste.is_none() && deferred_input.is_empty() => {
                match input_event {
                    Some(event) => {
                        if let Some(failure) = dispatch_ready_input_events(
                            &mut app,
                            &mut input_rx,
                            &mut naming_workers,
                            &mut icon_search_workers,
                            home_dir.as_deref(),
                            event,
                            &mut deferred_input,
                        ) {
                            input_failure = Some(failure);
                            break;
                        }
                        needs_redraw = true;
                        needs_immediate_redraw = true;
                    }
                    None => {
                        return Err(ClientError::TerminalSetup(std::io::Error::other(
                            "terminal input owner stopped before a requested shutdown",
                        )));
                    }
                }
            }
            server_event = connection.events.recv(), if app.pending_terminal_events.len() < MAX_PENDING_SERVER_EVENTS => {
                match server_event {
                    Some(event) => {
                        needs_redraw = true;
                        let Some(event) = queue_server_event_in_order(&mut app, event) else { continue; };
                        let damage = apply_server_events(
                            &mut app,
                            &mut connection.events,
                            event,
                            &mut naming_workers,
                            &mut icon_search_workers,
                            &mut trigger_execution_lease,
                            home_dir.as_deref(),
                        );
                        needs_immediate_redraw |= damage.needs_immediate_redraw;
                        needs_redraw |= damage.needs_redraw;
                    }
                    // The reader task ended -- the server is gone or the
                    // connection dropped; nothing left to attach to.
                    None => {
                        tracing::error!("server event channel closed unexpectedly");
                        break;
                    }
                }
            }
            Some(naming_event) = naming_events_rx.recv() => {
                naming_workers.set_automatic_ai_decision(
                    app.onboarding_revision,
                    app.onboarding.is_none() && app.onboarding_progress.automatic_ai_allowed(),
                );
                crate::tick::apply_naming_worker_event(&mut app, &mut naming_workers, naming_event);
                needs_redraw = true;
                needs_immediate_redraw = true;
            }
            Some(conversion_event) = conversion_events_rx.recv() => {
                app.apply_conversion_worker_event(conversion_event);
                needs_redraw = true;
                needs_immediate_redraw = true;
            }
            Some(remote_compaction_event) = remote_compaction_events_rx.recv() => {
                app.apply_remote_compaction_worker_event(remote_compaction_event);
                needs_redraw = true;
                needs_immediate_redraw = true;
            }
            _ = search_notification.notified() => {
                if let Some(search_event) = search_workers.collect() { app.apply_workspace_search_result(search_event); }
                needs_redraw = true;
                needs_immediate_redraw = true;
            }
            _=provider_notification.notified()=>{naming_workers.collect(); smart_copy_workers.collect(); needs_redraw |= app.collect_model_catalog_preparation();},
            Some(smart_copy_event) = smart_copy_events_rx.recv() => {
                let generation = smart_copy_event.generation;
                let is_terminal = matches!(smart_copy_event.update, SmartCopyWorkerUpdate::Finished | SmartCopyWorkerUpdate::Failed(_));
                app.apply_smart_copy_worker_event(smart_copy_event);
                if is_terminal {
                    smart_copy_workers.finish(generation);
                }
                needs_redraw = true;
                needs_immediate_redraw = true;
            }
            _ = icon_search_notification.notified() => {
                if let Some(event) = icon_search_workers.poll() {
                    app.apply_icon_semantic_search_event(event);
                    needs_redraw = true;
                    needs_immediate_redraw = true;
                }
            }
            Some(reset_event) = reset_events_rx.recv() => {
                app.reset_monitor_state.apply(reset_event, &app.reset_planning_settings);
                needs_redraw = true;
                needs_immediate_redraw = true;
            }
            demo_event = next_demo_voice_event(&mut onboarding_voice, &mut pending_demo_event, demo_event_retry_at) => {
                match demo_event {
                    Some(event) => match onboarding_voice.handle_event(event).await {
                        Ok(()) => demo_event_retry_at = None,
                        Err(original) => {
                            pending_demo_event = Some(original);
                            demo_event_retry_at = Some(tokio::time::Instant::now() + Duration::from_millis(20));
                        }
                    },
                    None=>onboarding_voice.channel_closed().await,
                }
                needs_redraw=true;needs_immediate_redraw=true;
            }
            () = normal_voice_preparation_notification.notified() => { needs_redraw=true; }
            () = onboarding_voice_notification.notified() => {
                onboarding_voice.collect(Instant::now()).await;
                needs_redraw = true;
            }
            voice_event = next_voice_event(&mut voice_service, &mut pending_voice_event, voice_event_retry_at), if app.pending_normal_voice_outputs.is_none() && app.normal_voice_commands.available()!=0 => {
                match voice_event {
                    Some(event) => {
                        match handle_voice_event(&mut app, &mut control_plane, event) {
                            Ok(outputs) => {
                                app.voice_event_projection_pending = false;
                                voice_tool_outputs = outputs;
                                voice_event_retry_at = None;
                            }
                            Err((original, reason)) => {
                                app.voice_event_projection_pending = true;
                                pending_voice_event = Some(original);
                                voice_event_retry_at = Some(tokio::time::Instant::now() + Duration::from_millis(20));
                                app.status_message = Some(reason);
                            }
                        }
                    }
                    None if voice_service.is_some() => {
                        tracing::error!("voice service event stream closed");
                        app.voice_shutdown_requested = true;
                        if let Some(service) = &voice_service { service.request_shutdown(); }
                        if should_report_unexpected_voice_stop(
                            app.voice_settings.enabled,
                            &app.voice_connection_state,
                        ) {
                            app.update_voice_connection_state(
                                ilium_voice::VoiceConnectionState::Failed(
                                    "voice service stopped unexpectedly".to_owned(),
                                ),
                            );
                        }
                        // An unexpected stop is still voice mode ending from
                        // the user's perspective -- media paused for it must
                        // not stay paused just because the actor crashed
                        // instead of shutting down through `reconcile_voice_runtime`.
                        normal_media.request(false);
                    }
                    None => {}
                }
                needs_redraw = true;
                needs_immediate_redraw = true;
            }
            () = tokio::time::sleep(tick_delay) => {
                if crate::tick::on_tick(
                    &mut app,
                    Instant::now(),
                    maintenance_schedule.was_animating,
                    &mut search_workers,
                ) {
                    needs_redraw = true;
                }
                if app
                    .reset_monitor_state
                    .display_tick_interval(&app.reset_planning_settings)
                    .is_some()
                {
                    needs_redraw = true;
                }
            }
        }

        if app.pending_normal_voice_outputs.is_none() && app.normal_voice_commands.available()!=0 {
            if let Some(output) = control_plane.collect_prepared(&mut app) {
                voice_tool_outputs.push(output);needs_redraw=true;
            }
        }

        // Continuous IPC/input can starve the sleep branch. Presentation
        // expiry must advance on every pass, including those busy passes.
        needs_redraw |=
            app.tick_completed_progress_display(crate::scheduled_input::unix_millis_now());

        if *reset_settings_tx.borrow() != app.reset_planning_settings {
            let _ = reset_settings_tx.send(app.reset_planning_settings.clone());
        }

        dispatch_pending_app_work(
            &mut app,
            &mut naming_workers,
            &mut icon_search_workers,
            home_dir.as_deref(),
        );
        dispatch_pending_smart_copy_work(&mut app, &mut smart_copy_workers); // Cancellation precedes the dated App-original retry gate.
        smart_copy_workers.collect();
        needs_redraw |= app.collect_terminal_parsing();
        if app.projection_admission_wake.is_none() {
        if let Some(event)=app.pending_terminal_events.pop_front() {
            let damage=apply_server_events(&mut app,&mut connection.events,event,&mut naming_workers,&mut icon_search_workers,&mut trigger_execution_lease,home_dir.as_deref());
            needs_redraw|=damage.needs_redraw;needs_immediate_redraw|=damage.needs_immediate_redraw;
        }
        }
        needs_redraw |= app.collect_document_preparation();
        needs_redraw |= app.collect_terminal_context_preparation();
        needs_redraw |= app.collect_external_open();
        needs_redraw |= app.collect_emitted_smart_copy_capture();
        needs_redraw |= app.collect_clipboard();
        needs_redraw |= app.collect_light_copy_selection();
        needs_redraw |= app.collect_editor_files();
        reconcile_debug_logging(&mut app);
        reconcile_debug_logging_server(&mut app);
        reconcile_voice_receiver_registration(&mut app);
        collect_normal_voice_preparation(&mut app,&mut voice_service,&mut delivered_voice_target_context,&mut normal_media);
        reconcile_agent_debug_menu(&mut app);
        reconcile_progress_monitor_enabled(&mut app);
        // Release the demonstration's devices/media ownership before the
        // saved normal voice preference can acquire those resources again.
        let is_voice_step = app.onboarding.is_some()
            && app.onboarding_progress.wizard.step == crate::onboarding::state::Step::Voice;
        onboarding_voice.set_external_owner_pending(
            app.normal_voice_ownership_pending(voice_service.is_some()),
        );
        onboarding_voice
            .reconcile(&app.voice_settings, is_voice_step)
            .await;
        onboarding_voice.collect(Instant::now()).await;
        needs_redraw |= audit_demo_voice_retirement(
            &mut app, &mut onboarding_voice, &mut demonstration_retirement_failed,
        );
        app.voice_demo_retirement_pending = onboarding_voice.pending_work();
        demonstration_media.request(onboarding_voice.state.can_stop
            && app.voice_settings.pause_media_while_active);
        let is_voice_tool_shutdown = voice_tool_outputs_request_shutdown(&voice_tool_outputs);
        if is_voice_tool_shutdown {
            // A terminating result dominates any parallel tool call that may
            // have touched `voice.enabled` in the same provider response.
            app.stop_voice_control();
        }
        if !is_voice_tool_shutdown {
            reconcile_voice_runtime(
                &mut app,
                &control_plane,
                &mut voice_service,
                &mut normal_media,
            )
            .await;
            reconcile_voice_target_context(
                &mut app,
                &control_plane,
                voice_service.as_ref(),
                &mut delivered_voice_target_context,
            )
            .await;
            deliver_voice_interactions(&mut app, voice_service.as_ref()).await;
            deliver_voice_text_offers(&mut app, voice_service.as_ref()).await;
        }

        // Raw terminal output is useful only for panes occupying this
        // client's right panel. Compare the bounded inline representation on
        // every turn, but allocate/send a protocol vector only on an actual
        // pane or split transition. The server journal repairs a newly
        // visible pane before its live stream resumes.
        let streamed_pane_slots = app.displayed_pane_slots();
        if last_streamed_pane_slots != Some(streamed_pane_slots)
            && app.pending_terminal_discards.is_empty()
            && app.queue_request(ilium_ipc::ClientRequest::SetVisiblePanes {
                pane_ids: streamed_pane_slots.into_iter().flatten().collect(),
            }) {
                last_streamed_pane_slots = Some(streamed_pane_slots);
        }

        if let Some(job) = app.take_pending_conversion_start() {
            conversion_workers.spawn(job);
        }
        if app.take_pending_conversion_cancel() {
            conversion_workers.cancel();
        }
        if app.take_pending_remote_compaction_cancel() {
            remote_compaction_workers.cancel();
        }
        if let Some(job) = app.take_pending_remote_compaction_job() {
            remote_compaction_workers.spawn(job);
        }

        if app.publish_outbound_requests(&connection.requests).is_err() {
            tracing::error!("server request channel closed before request delivery");
            break 'event_loop;
        }

        // A confirmation-required terminal submission queues its visible
        // text as IPC before returning the tool result that makes the voice
        // model ask the question. This preserves the user-facing contract:
        // type first, then ask whether to press Enter.
        let current_voice_owner = voice_service.as_ref().map(ilium_voice::VoiceService::instance_identity);
        let same_owner = match (&voice_output_owner, &current_voice_owner) {
            (Some(original), Some(current)) => std::sync::Arc::ptr_eq(original, current),
            (None, None) => true,
            _ => false,
        };
        let discarded_voice_requests = control_plane.has_pending_preparation() || !voice_tool_outputs.is_empty();
        if control_plane.synchronize_voice_instance(current_voice_owner) && !same_owner && discarded_voice_requests {
            app.status_message = Some("Voice control requests cancelled because their original voice session ended".into());
        }
        if same_owner {
            deliver_voice_tool_outputs(&mut app, &mut voice_service, voice_tool_outputs).await;
        }
        if is_voice_tool_shutdown {
            reconcile_voice_runtime(
                &mut app,
                &control_plane,
                &mut voice_service,
                &mut normal_media,
            )
            .await;
            reconcile_voice_target_context(
                &mut app,
                &control_plane,
                voice_service.as_ref(),
                &mut delivered_voice_target_context,
            )
            .await;
            deliver_voice_interactions(&mut app, voice_service.as_ref()).await;
            deliver_voice_text_offers(&mut app, voice_service.as_ref()).await;
        }

        let demo_action = app
            .onboarding
            .as_mut()
            .and_then(|ui| ui.voice_action.take());
        if is_voice_step {
            if let Some(action) = demo_action {
                use crate::onboarding::voice_ui::VoiceAction;
                let outcome = match action {
                    VoiceAction::Test => {
                        let result = onboarding_voice.start_test(&app.voice_settings).await;
                        if result.is_ok()
                            && app.voice_settings.pause_media_while_active
                            && !demonstration_media.is_requested()
                        {
                            demonstration_media.request(true);
                        }
                        result
                    }
                    VoiceAction::Stop => {
                        onboarding_voice.shutdown().await;
                        Ok(())
                    }
                    VoiceAction::StartPushToTalk => onboarding_voice.push_to_talk(true).await,
                    VoiceAction::StopPushToTalk => onboarding_voice.push_to_talk(false).await,
                    VoiceAction::Configure(_) => Ok(()),
                };
                if let Err(error) = outcome {
                    app.status_message = Some(error);
                }
                needs_redraw = true;
                needs_immediate_redraw = true;
            }
            if let Some(ui) = &mut app.onboarding {
                ui.voice_state = onboarding_voice.state.clone();
            }
        }
        if (!onboarding_voice.state.is_running || !app.voice_settings.pause_media_while_active)
            && demonstration_media.is_requested()
        {
            demonstration_media.request(false);
        }

        if app.synchronize_animation_project_settings() {
            needs_redraw = true;
            needs_immediate_redraw = true;
        }
        record_client_surface_change(&app, &mut last_recorded_surface);
        record_status_message_change(
            app.status_message.as_deref(),
            &mut last_recorded_status_message,
        );

        // Check every branch so ready PTY/input channels cannot postpone a due frame.
        let animation_elapsed = crate::background_composition::quantized_elapsed_at(
            Instant::now().saturating_duration_since(app.started_at),
            app.animation_frames_per_second(),
        );
        let animation_frame_bucket =
            crate::background_composition::animation_frame_bucket(&app, animation_elapsed);
        let onboarding_animation_active = crate::onboarding::integration::is_animating(&app);
        // Draw the settled frame too, so a quiet workspace cannot retain the
        // last green highlight after the finite onboarding animation ends.
        if onboarding_animation_active || last_onboarding_animation_active {
            needs_redraw = true;
        }
        last_onboarding_animation_active = onboarding_animation_active;
        if animation_frame_bucket != last_animation_frame_bucket {
            needs_redraw = true;
        }
        let can_draw = output_redraw_is_due(needs_immediate_redraw, Instant::now(), last_draw_at);
        // Reserve before composition: graphics uploads and exact animation
        // receipts must follow the ordered frame that really reaches output.
        if needs_redraw && can_draw && presenter.outstanding_frames() == 0 {
            let reservation = match presenter.try_reserve() {
                Some(reservation) => app.admit_composition_metadata()
                    .map_err(|error| ClientError::TerminalSetup(std::io::Error::other(error)))?
                    .then_some(reservation),
                None => None,
            };
            if let Some(reservation) = reservation {
                let area = app.layout.screen_area;
                if terminal.current_buffer_mut().area != area {
                    terminal.resize(area).unwrap_or_else(|error| match error {});
                }
                terminal.current_buffer_mut().reset();
                let mut frame = terminal.get_frame();
                let cursor = crate::ui::draw_at_with_cursor(&mut frame, &mut app, animation_elapsed);
                crate::text_trigger_dialog::draw_save_error(&mut frame, &app);
                crate::terminal_guard::skip_bottom_right_cell(&mut frame, cfg!(windows));
                let buffer = frame.buffer_mut().clone();
                presentation_frame_id = presentation_frame_id.checked_add(1)
                    .ok_or_else(|| ClientError::TerminalSetup(std::io::Error::other("presentation frame identity exhausted")))?;
                presentation_layout_revision = presentation_layout_revision.checked_add(1)
                    .ok_or_else(|| ClientError::TerminalSetup(std::io::Error::other("presentation layout revision exhausted")))?;
                let animation = crate::background_composition::capture_final(&buffer, area, &mut app);
                let mut prepared = crate::presentation::PreparedFrame::new(reservation, buffer, cursor,
                    presentation_frame_id, presentation_layout_revision).map_err(ClientError::TerminalSetup)?;
                prepared.attach_animation(animation);
                let geometry = app.capture_emitted_geometry(presentation_layout_revision);
                presenter.submit(prepared).map_err(|failure| ClientError::TerminalSetup(failure.error))?;
                animation_presentations.push_back((presentation_frame_id, geometry));
            needs_redraw = false;
            needs_immediate_redraw = false;
            last_draw_at = Instant::now();
            last_animation_frame_bucket = animation_frame_bucket;
            }
        }
    }
    input_rx.close();
    reset_monitor.abort();

    // Settle accepted state replies and their following semantic actions while
    // their original voice actor and the outbound terminal consumers remain alive.
    let control_drain = async {
        while control_plane.has_pending_preparation() {
            let ready = control_notification.notified();
            let admission = filesystem_admission_notification.notified();
            tokio::pin!(ready, admission);
            ready.as_mut().enable();
            admission.as_mut().enable();
            collect_normal_voice_preparation(&mut app,&mut voice_service,&mut delivered_voice_target_context,&mut normal_media);
            reconcile_voice_target_context(&mut app, &control_plane, voice_service.as_ref(), &mut delivered_voice_target_context).await;
            deliver_voice_tool_outputs(&mut app, &mut voice_service, Vec::new()).await;
            if app.pending_normal_voice_outputs.is_none() && app.normal_voice_commands.available() != 0 {
                if let Some(output) = control_plane.collect_prepared(&mut app) {
                    deliver_voice_tool_outputs(&mut app, &mut voice_service, vec![output]).await;
                    continue;
                }
            }
            if !control_plane.has_pending_preparation() { break; }
            tokio::select! { _ = &mut ready => {}, _ = &mut admission => {}, _ = tokio::time::sleep(Duration::from_millis(20)) => {} }
        }
    };
    if tokio::time::timeout(Duration::from_secs(5), control_drain).await.is_err() {
        control_plane.cancel_pending();
        return Err(ClientError::TerminalSetup(std::io::Error::other("voice control drain deadline expired; preparation cancelled without claiming delivery")));
    }

    app.model_catalog_preparation.close();
    naming_workers.close_finite();
    smart_copy_workers.cancel();
    app.prepare_terminal_shutdown();
    app.cancel_source_windows();
    connection.request_read_shutdown();
    // Detach stops new frames at a transport boundary. Drive all already
    // accepted bytes and input barriers through the same consumers before
    // cancelling the persistent parser. A deadline reports uncertainty.
    let terminal_drain = async {
        loop {
            let notification = terminal_notification.notified();
            let baseline_ready = filesystem_notification.notified();
            let baseline_admission = filesystem_admission_notification.notified();
            tokio::pin!(notification, baseline_ready, baseline_admission);
            notification.as_mut().enable();
            baseline_ready.as_mut().enable();
            baseline_admission.as_mut().enable();
            naming_workers.collect();
            while let Ok(event)=naming_events_rx.try_recv() {
                tick::apply_naming_worker_event(&mut app,&mut naming_workers,event);
            }
            app.collect_terminal_parsing();
            if let Some(failure) = app.take_native_paste_failure() {
                input_failure = Some(terminal_input_owner::InputFailure::combine(input_failure.take(), failure));
                return Err(ClientError::TerminalSetup(std::io::Error::other("terminal Paste publication failed; original retained for caller")));
            }
            app.collect_terminal_context_preparation();
            app.collect_clipboard();
            app.collect_light_copy_selection();
            app.collect_terminal_baselines();
            dispatch_pending_app_work(&mut app, &mut naming_workers, &mut icon_search_workers, home_dir.as_deref());
            if let Some(reason) = app.projection_admission_failure {
                return Err(ClientError::TerminalSetup(std::io::Error::other(format!("unrecoverable incoming projection admission failure: {reason:?}"))));
            }
            let event = if app.projection_admission_wake.is_none() {
                app.pending_terminal_events.pop_front().or_else(|| connection.events.try_recv().ok())
            } else { None };
            if let Some(event) = event {
                apply_server_events(&mut app, &mut connection.events, event,
                    &mut naming_workers, &mut icon_search_workers,
                    &mut trigger_execution_lease, home_dir.as_deref());
            }
            for request in app.take_admitted_outbound_requests() {
                connection.requests.send_admitted(request).await.map_err(|_| ClientError::TerminalSetup(
                    std::io::Error::new(std::io::ErrorKind::BrokenPipe,
                        "accepted terminal input could not drain before detach")))?;
            }
            let (events, intents, owner) = app.terminal_pending_work();
            if events == 0 && intents == 0 && owner == Some((0, 0))
                && animation_presentations.is_empty()
                && !app.has_pending_exact_prompt_reports()
                && !naming_workers.has_pending_exact_delivery() && naming_events_rx.is_empty()
                && !app.terminal_clipboard.as_ref().is_some_and(|clipboard|clipboard.pending())
                && app.terminal_baselines.as_ref().is_none_or(|files| !files.pending())
                && connection.events.is_closed() && connection.events.is_empty() {
                // Observe a final publication queued just before active_bytes
                // cleared, including a barrier which can produce more input.
                app.collect_terminal_parsing();
                app.collect_clipboard();
            app.collect_light_copy_selection();
                app.collect_terminal_baselines();
                if app.terminal_pending_work() == (0, 0, Some((0, 0))) && !naming_workers.has_pending_exact_delivery() && naming_events_rx.is_empty() && !app.has_pending_exact_prompt_reports() && !app.terminal_clipboard.as_ref().is_some_and(|clipboard|clipboard.pending()) && app.terminal_baselines.as_ref().is_none_or(|files| !files.pending()) {
                    break;
                }
                continue;
            }
            if let Some(error) = app.terminal_parsing.as_mut().and_then(|parser| parser.failure()) {
                return Err(ClientError::TerminalSetup(std::io::Error::other(error)));
            }
            if naming_workers.has_pending_exact_delivery() || !naming_events_rx.is_empty() {continue;}
            tokio::select! {
                // Queued complete frames retain parser source leases. Consume
                // actual output receipts during parser drain so those leases
                // cannot prevent the next ordered snapshot publication.
                acknowledgement = presenter.acknowledgements.recv(), if !animation_presentations.is_empty() => {
                    let mut presented = acknowledgement.ok_or_else(|| ClientError::TerminalSetup(
                        std::io::Error::other("presentation owner closed during terminal drain")))?
                        .map_err(ClientError::TerminalSetup)?;
                    let Some((frame_id, geometry)) = animation_presentations.pop_front() else {
                        return Err(ClientError::TerminalSetup(std::io::Error::other("missing terminal drain presentation identity")));
                    };
                    if frame_id != presented.frame.frame_id {
                        return Err(ClientError::TerminalSetup(std::io::Error::other("terminal drain presentation identity mismatch")));
                    }
                    // Interaction has ended; installing geometry would pin the
                    // emitted source again. Scene receipts still apply exactly.
                    drop(geometry);
                    if let Some(reason) = &presented.uncertainty {
                        if let Some(animation) = presented.frame.take_animation() {
                            app.animation_frame.uncertain_output(animation, reason);
                        }
                    } else if let Some(reason) = &presented.rejection {
                        drop(presented.frame.take_animation());
                        app.animation_frame.reject_output(reason);
                    } else if let Some(animation) = presented.frame.take_animation() {
                        app.animation_frame.acknowledge(animation, presented.flush_proof.take());
                    }
                    app.animation_frame.collect();
                },
                _ = notification => {},
                _ = baseline_ready => {},
                _ = baseline_admission => {},
                _ = async { match &mut app.projection_admission_wake { Some(wake) => wake.as_mut().await, None => std::future::pending().await } } => {
                    app.projection_admission_wake = None; app.projection_busy_retry_at = None;
                },
                _ = async { match app.projection_busy_retry_at { Some(deadline) => tokio::time::sleep_until(deadline).await, None => std::future::pending().await } } => {
                    app.projection_admission_wake = None; app.projection_busy_retry_at = None;
                },
                _ = clipboard_notification.notified() => {},
                _ = context_notification.notified() => {},
                event = connection.events.recv(), if app.pending_terminal_events.len() < MAX_PENDING_SERVER_EVENTS && !connection.events.is_closed() => {
                    if let Some(event) = event.and_then(|event| queue_server_event_in_order(&mut app, event)) {
                        apply_server_events(&mut app, &mut connection.events, event,
                            &mut naming_workers, &mut icon_search_workers,
                            &mut trigger_execution_lease, home_dir.as_deref());
                    }
                },
                // A short try_lock failure can unlock without an owner wake.
                _ = tokio::time::sleep(Duration::from_millis(1)), if owner.is_none() || app.has_pending_exact_prompt_reports() || app.terminal_baselines.as_ref().is_some_and(|files| files.needs_retry()) => {},
            }
        }
        connection.finish_read_shutdown().await.map_err(|error|
            ClientError::TerminalSetup(std::io::Error::other(error)))
    };
    tokio::time::timeout(Duration::from_secs(5), terminal_drain).await
        .map_err(|_| ClientError::TerminalSetup(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "terminal detach deadline: accepted bytes or input remain unacknowledged")))??;
    onboarding_voice.shutdown().await;
    demonstration_media.request(false);

    drain_normal_voice_retirement(&mut app, &mut voice_service, &mut pending_voice_event)
        .await.map_err(ClientError::TerminalSetup)?;
    normal_media.request(false);
    media_owner.cancel();
    icon_search_workers.cancel();

    // Release exact emitted scene leases before configuration durability
    // receipts try to admit their ordered semantic animation changes.
    settle_presentation_receipts(&mut app, &mut presenter, &mut animation_presentations).await?;

    // Finish logging transitions while the connection still owns its writer.
    // The local file receipt alone cannot prove the server request was emitted.
    let request_drain = async {
        app.drain_filesystem().await.map_err(|error| crate::connection::ConnectionError::RequestDrain(ilium_ipc::IpcError::Io(error)))?;
        reconcile_debug_logging(&mut app);
        while app.debug_logging.is_pending() {
            let completion = app.debug_logging.next_completion().await;
            apply_debug_logging_completion(&mut app, completion);
        }
        for request in app.take_admitted_outbound_requests() {
            connection.requests.send_admitted(request).await.map_err(|_| {
                crate::connection::ConnectionError::RequestDrain(ilium_ipc::IpcError::Io(
                    std::io::Error::new(std::io::ErrorKind::BrokenPipe, "request writer closed during final drain")))
            })?;
        }
        connection.requests.flush().await.map_err(crate::connection::ConnectionError::RequestDrain)?;
        app.confirm_exact_prompt_reports_flushed();
        Ok::<(),crate::connection::ConnectionError>(())
    };
    tokio::time::timeout(Duration::from_secs(5), request_drain).await
        .map_err(|_| crate::connection::ConnectionError::DrainDeadline)??;

    // Only the explicit Restart menu action can request a process re-exec.
    // Input failures and undispatched originals are returned after cleanup.
    Ok(app.exit_reason.unwrap_or(ClientExitReason::Quit))
    }.await;
    if let Some(pending) = app.pending_native_paste.take() {
        input_failure = Some(terminal_input_owner::InputFailure::undispatched(
            pending.event,
            input_failure.take(),
        ));
    }
    while let Some(event) = deferred_input.take_front() {
        match event {
            Ok(event) => {
                input_failure = Some(terminal_input_owner::InputFailure::undispatched(
                    event,
                    input_failure.take(),
                ))
            }
            Err(error) => {
                input_failure = Some(terminal_input_owner::InputFailure::combine(
                    input_failure.take(),
                    error,
                ))
            }
        }
    }
    if let Some(failure) = app.take_native_paste_failure() {
        input_failure = Some(terminal_input_owner::InputFailure::combine(
            input_failure.take(),
            failure,
        ));
    }
    while let Some(event) = app.take_undelivered_native_paste() {
        input_failure = Some(terminal_input_owner::InputFailure::undispatched(
            event,
            input_failure.take(),
        ));
    }
    let input_retirement = input_owner
        .take()
        .map(terminal_input_owner::InputOwner::stop);
    let input_deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    naming_workers.close_finite();
    naming_workers.collect();
    // Preserve completed exact evidence even when an unrelated error exits the
    // event loop. The independent FIFO keeps bytes and source leases intact.
    while let Ok(event) = naming_events_rx.try_recv() {
        let exact = match &event {
            NamingWorkerEvent::ExactPrepared { .. } => true,
            NamingWorkerEvent::Prepared { event, .. } => {
                matches!(&**event, NamingWorkerEvent::ExactPrepared { .. })
            }
            _ => false,
        };
        if exact {
            tick::apply_naming_worker_event(&mut app, &mut naming_workers, event);
        }
    }
    if result.is_err()
        && (app.unconfirmed_exact_prompt_reports() != 0
            || naming_workers.has_pending_exact_delivery())
    {
        tracing::error!(unconfirmed_exact_transcript_reports=app.unconfirmed_exact_prompt_reports(),
            pending_exact_delivery=naming_workers.has_pending_exact_delivery(),
            "Published exact transcript evidence has no final outbound flush acknowledgement; delivery is uncertain");
    }
    smart_copy_workers.cancel();
    // This runs on every Result exit, including connection and output errors.
    // Restore raw/alternate-screen state only after output has stopped.
    let normal_shutdown_result =
        drain_normal_voice_retirement(&mut app, &mut voice_service, &mut pending_voice_event)
            .await
            .map_err(ClientError::TerminalSetup);
    let demonstration_shutdown_result = drain_demo_voice_retirement(
        &mut app,
        &mut onboarding_voice,
        &mut pending_demo_event,
        &mut demonstration_retirement_failed,
    )
    .await
    .map_err(ClientError::TerminalSetup);
    let external_open_shutdown_result = match app.external_open.take() {
        Some(service) => service.shutdown().await.map_err(ClientError::TerminalSetup),
        None => Ok(()),
    };
    // Terminal interaction has ended. Release the last bounded acknowledgement
    // only after its original display message is destroyed.
    app.status_message = None;
    app.external_open_status_storage = None;
    app.light_copy_shutdown = true;
    app.startup_progress.close();
    if let Some(selection) = &mut app.light_copy_selection {
        selection.close_admission();
    }
    // Bound the normal drain: a blocked native child must reach its existing
    // shutdown/kill path instead of keeping this coordination loop forever.
    let selection_deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while app
        .light_copy_selection
        .as_ref()
        .is_some_and(|selection| selection.pending())
        && tokio::time::Instant::now() < selection_deadline
    {
        app.collect_clipboard();
        app.collect_light_copy_selection();
        if app
            .light_copy_selection
            .as_ref()
            .is_some_and(|selection| selection.pending())
        {
            tokio::select! {
                _ = selection_notification.notified() => {},
                _ = clipboard_notification.notified() => {},
                _ = tokio::time::sleep(Duration::from_millis(100)) => {},
            }
        }
    }
    let clipboard_shutdown_result = match app.terminal_clipboard.take() {
        Some(clipboard) => {
            let (result, acknowledgements) = clipboard.shutdown_with_acknowledgements().await;
            for completion in acknowledgements {
                app.accept_clipboard_completion(completion);
            }
            result.map_err(ClientError::TerminalSetup)
        }
        None => Ok(()),
    };
    if let Some(selection) = &mut app.light_copy_selection {
        selection.native_closed();
    }
    // CPU is still alive. Missing native replies are already marked unknown.
    let recovery_deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while app
        .light_copy_selection
        .as_ref()
        .is_some_and(|selection| selection.pending())
        && tokio::time::Instant::now() < recovery_deadline
    {
        app.collect_light_copy_selection();
        if app
            .light_copy_selection
            .as_ref()
            .is_some_and(|selection| selection.pending())
        {
            tokio::select! {
                _ = selection_notification.notified() => {},
                _ = tokio::time::sleep(Duration::from_millis(100)) => {},
            }
        }
    }
    let selection_shutdown_result = if app
        .light_copy_selection
        .as_ref()
        .is_some_and(|selection| selection.pending())
    {
        match app.light_copy_selection.take() {
            Some(owner) => Err(ClientError::TerminalSetup(std::io::Error::other(
                crate::smart_copy_selection::SelectionShutdownCustody::new(
                    owner,
                    std::mem::take(&mut app.light_copy_recovery),
                    app.smart_copy_preview.take(),
                    std::mem::take(&mut app.light_copy_restored),
                    if app
                        .smart_copy_session
                        .as_ref()
                        .is_some_and(|session| session.is_light)
                    {
                        app.smart_copy_session.take()
                    } else {
                        None
                    },
                ),
            ))),
            None => Err(ClientError::TerminalSetup(std::io::Error::other(
                "Selection custody disappeared",
            ))),
        }
    } else {
        let result = if app.light_copy_recovery.is_empty() && app.light_copy_restored.is_empty() {
            Ok(())
        } else {
            Err(ClientError::TerminalSetup(std::io::Error::other(
                "Selection preparation failed; originals retired during shutdown, no copy success claimed")))
        };
        app.light_copy_recovery.clear(); // Queues entire original disposal on still-live CPU.
        app.light_copy_restored.clear(); // Returned Sessions have their original self-retirement permits.
        if app
            .smart_copy_session
            .as_ref()
            .is_some_and(|session| session.is_light)
        {
            app.smart_copy_session = None;
        }
        app.light_copy_retry_generation = None;
        app.smart_copy_preview = None; // Drop original shared leaves before retirement join.
        app.light_copy_selection = None;
        result
    };
    let logging_result = tokio::time::timeout(Duration::from_secs(5), app.debug_logging.drain())
        .await
        .map_err(|_| ClientError::Logging(ilium_logging::LoggingError::Deadline))
        .and_then(|result| result.map_err(ClientError::Logging));
    app.prepare_terminal_shutdown();
    app.cancel_source_windows();
    if let Some(preparation) = &mut app.document_preparation {
        preparation.cancel();
    }
    if let Some(preparation) = &mut app.terminal_context_preparation {
        preparation.cancel();
    }
    let shutdown_result = presenter
        .shutdown()
        .await
        .map_err(ClientError::TerminalSetup);
    let input_shutdown_result = match input_retirement {
        Some(retirement) => finish_input_retirement(retirement, input_deadline).await,
        None => Ok(()),
    };
    let media_shutdown_result = media_owner
        .shutdown()
        .await
        .map_err(ClientError::TerminalSetup);
    let icon_shutdown_result = icon_search_workers
        .shutdown()
        .await
        .map_err(ClientError::TerminalSetup);
    // Shutdown drains terminal output first; successful receipts still need
    // exact scene credit even when the event loop exited for another error.
    let mut animation_receipt_result = Ok(());
    while let Ok(acknowledgement) = presenter.acknowledgements.try_recv() {
        if let Ok(mut presented) = acknowledgement {
            match animation_presentations.pop_front() {
                Some((frame_id, geometry)) if frame_id == presented.frame.frame_id => {
                    drop(geometry);
                    if let Some(reason) = &presented.uncertainty {
                        if let Some(animation) = presented.frame.take_animation() {
                            app.animation_frame.uncertain_output(animation, reason);
                        }
                    } else if let Some(reason) = &presented.rejection {
                        drop(presented.frame.take_animation());
                        app.animation_frame.reject_output(reason);
                    } else if let Some(animation) = presented.frame.take_animation() {
                        app.animation_frame
                            .acknowledge(animation, presented.flush_proof.take());
                    }
                }
                _ => {
                    animation_receipt_result = Err(ClientError::TerminalSetup(
                        std::io::Error::other("animation shutdown presentation identity mismatch"),
                    ))
                }
            }
        }
    }
    // Remaining tokens correspond to frames without a successful flush.
    animation_presentations.clear();
    // Presenter has joined and every ordered ACK/failed-frame owner was settled.
    // Release its last immutable editor leaves before execution retirement joins.
    app.release_editor_frame_owners();
    let filesystem_result = app
        .drain_filesystem()
        .await
        .map_err(ClientError::TerminalSetup);
    for pane in app.panes.values_mut() {
        if let PaneRuntime::Editor(editor) = pane {
            editor.clear_preparation();
        }
    }
    if let Some(parsing) = &mut app.terminal_parsing {
        parsing.cancel();
    }
    let animation_shutdown_result = app
        .animation_frame
        .shutdown()
        .await
        .map_err(ClientError::TerminalSetup);
    app.session_stats.cancel_pending();
    app.cost_tracker.cancel_pending();
    app.model_catalog_preparation.close();
    let execution_result = execution
        .shutdown()
        .await
        .map_err(ClientError::TerminalSetup);
    let result = shutdown_result
        .and(normal_shutdown_result)
        .and(demonstration_shutdown_result)
        .and(clipboard_shutdown_result)
        .and(external_open_shutdown_result)
        .and(media_shutdown_result)
        .and(icon_shutdown_result)
        .and(animation_receipt_result)
        .and(animation_shutdown_result)
        .and(logging_result)
        .and(filesystem_result)
        .and(execution_result)
        .and(result);
    let result = match selection_shutdown_result {
        Ok(()) => result,
        Err(selection) => Err(crate::smart_copy_selection::preserve_shutdown_error(
            selection,
            result.err(),
        )),
    };
    let retirement = input_shutdown_result.err();
    if input_failure.is_some() || retirement.is_some() {
        Err(ClientError::Input(Box::new(crate::error::InputRunError {
            failure: input_failure,
            retirement,
            other: result.err(),
        })))
    } else {
        result
    }
}

async fn finish_input_retirement(
    mut retirement: terminal_input_owner::InputRetirement,
    deadline: tokio::time::Instant,
) -> Result<(), crate::error::InputRetirementError> {
    loop {
        if let Some(report) = retirement.try_complete() {
            return report
                .into_result()
                .map_err(crate::error::InputRetirementError::Failed);
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(crate::error::InputRetirementError::Deadline(
                retirement.into_deadline(),
            ));
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Drain already-admitted frames while preserving their exact scene leases.
async fn settle_presentation_receipts(
    app: &mut App,
    presenter: &mut crate::presentation::Presenter,
    pending: &mut std::collections::VecDeque<(u64, crate::app::EmittedGeometry)>,
) -> Result<(), ClientError> {
    let drain = async {
        while !pending.is_empty() {
            let mut presented = presenter.acknowledgements.recv().await.ok_or_else(|| {
                std::io::Error::other("presentation owner closed before final acknowledgement")
            })??;
            let Some((frame_id, geometry)) = pending.pop_front() else {
                return Err(std::io::Error::other("missing final presentation identity"));
            };
            if frame_id != presented.frame.frame_id {
                return Err(std::io::Error::other(
                    "final presentation identity mismatch",
                ));
            }
            drop(geometry);
            if let Some(reason) = &presented.uncertainty {
                if let Some(animation) = presented.frame.take_animation() {
                    app.animation_frame.uncertain_output(animation, reason);
                }
            } else if let Some(reason) = &presented.rejection {
                drop(presented.frame.take_animation());
                app.animation_frame.reject_output(reason);
            } else if let Some(animation) = presented.frame.take_animation() {
                app.animation_frame
                    .acknowledge(animation, presented.flush_proof.take());
            }
            app.animation_frame.collect();
        }
        Ok(())
    };
    tokio::time::timeout(Duration::from_secs(5), drain)
        .await
        .map_err(|_| {
            ClientError::TerminalSetup(std::io::Error::other(
                "terminal presentation drain deadline",
            ))
        })?
        .map_err(ClientError::TerminalSetup)
}

/// Applies one coalesced Debug toggle to this client process and queues the
/// matching live update for the detached server. Credentials are never logged;
/// disabling records one final boundary event before closing the local file.
fn reconcile_debug_logging(app: &mut App) {
    let Some(enabled) = app.take_pending_debug_logging_enabled() else {
        return;
    };
    if !enabled {
        tracing::info!("client file logging disable requested");
    }
    match app.debug_logging.request(enabled) {
        Ok(()) => {
            app.debug_settings.file_logging_enabled = app.debug_logging.confirmed_enabled();
            mark_debug_logging_pending(app);
        }
        Err(error) => {
            app.retain_debug_logging_target(enabled);
            app.status_message = Some(format!("Debug logging change queued: {error}"));
        }
    }
}

fn mark_debug_logging_pending(app: &mut App) {
    // A successfully queued runtime transition must not erase a config-save
    // failure or another feature's status with a premature success impression.
    if app.status_message.as_deref().is_none_or(|message| {
        message == "Applying debug logging…" || message == "Debug logging applied"
    }) {
        app.status_message = Some("Applying debug logging…".to_owned());
    }
}

fn apply_debug_logging_completion(app: &mut App, completion: crate::debug_logging::Completion) {
    app.debug_settings.file_logging_enabled = app.debug_logging.confirmed_enabled();
    match completion.result {
        Ok(()) => {
            if completion.origin == crate::debug_logging::Origin::Client {
                app.pending_debug_logging_server_enabled = Some(completion.enabled);
                reconcile_debug_logging_server(app);
            }
            if completion.enabled {
                tracing::info!("client file logging transition acknowledged");
            }
            if !completion.is_pending
                && app.pending_debug_logging_server_enabled.is_none()
                && app.status_message.as_deref() == Some("Applying debug logging…")
            {
                app.status_message = Some("Debug logging applied".to_owned());
            }
        }
        Err(error) => {
            app.status_message = Some(format!("Could not apply debug logging: {error}"));
            tracing::error!(%error, "client logging transition failed");
        }
    }
}

// The local writer transition has already completed. Retrying this complete
// server state must not rerun the writer transition or its filesystem effects.
fn reconcile_debug_logging_server(app: &mut App) {
    let Some(enabled) = app.pending_debug_logging_server_enabled else {
        return;
    };
    if app.queue_request(ilium_ipc::ClientRequest::UpdateDebugLogging { enabled }) {
        app.pending_debug_logging_server_enabled = None;
        if !app.debug_logging.is_pending()
            && app.debug_logging.confirmed_enabled() == enabled
            && app.status_message.as_deref() == Some("Applying debug logging…")
        {
            app.status_message = Some("Debug logging applied".to_owned());
        }
    }
}

fn reconcile_voice_receiver_registration(app: &mut App) {
    if app.pending_voice_receiver_registration
        && app.queue_request(ilium_ipc::ClientRequest::RegisterVoiceTextReceiver)
    {
        app.pending_voice_receiver_registration = false;
    }
}

fn reconcile_agent_debug_menu(app: &mut App) {
    let Some(enabled) = app.take_pending_agent_debug_menu_enabled() else {
        return;
    };
    if !app.queue_request(ilium_ipc::ClientRequest::UpdateAgentDebugMenu { enabled }) {
        app.retain_agent_debug_menu_target(enabled);
    }
}

fn reconcile_progress_monitor_enabled(app: &mut App) {
    let Some(enabled) = app.take_pending_progress_monitor_enabled() else {
        return;
    };
    if !app.queue_request(ilium_ipc::ClientRequest::UpdateProgressMonitorEnabled { enabled }) {
        app.retain_progress_monitor_target(enabled);
    }
}

fn start_voice_service(
    app: &mut App,
    _control_plane: &crate::control::ControlPlane,
) -> Option<ilium_voice::VoiceService> {
    let kind = crate::voice_preparation::PreparationKind::NormalStartup {
        target: crate::control::VoiceTargetContext::capture(app),
    };
    match app.voice_preparation.request(&app.voice_settings, kind) {
        Ok(()) => app.update_voice_connection_state(ilium_voice::VoiceConnectionState::Connecting),
        Err(error) => {
            app.status_message = Some(format!("Voice startup preparation: {error:?}"));
            if error.is_retryable() {
                app.resume_voice_after_onboarding();
            } else {
                app.update_voice_connection_state(ilium_voice::VoiceConnectionState::Failed(
                    "Voice startup source or execution owner unavailable".into(),
                ));
            }
        }
    }
    None
}

/// Builds one Realtime context update only when agent detection changed the
/// semantic destination of an otherwise unqualified utterance. The tool still
/// resolves the exact active pane at execution time, so focus changes between
/// two detected agents do not require a provider update.
#[cfg(test)]
#[derive(Debug)]
struct NormalVoiceContextAllocation {
    _source: std::sync::Arc<ilium_execution::StorageAdmission>,
}
#[cfg(test)]
impl ilium_voice::VoiceTextAllocation for NormalVoiceContextAllocation {}

#[cfg(test)]
fn pending_voice_target_context_update(
    app: &App,
    control_plane: &crate::control::ControlPlane,
    delivered_context: Option<crate::control::VoiceTargetContext>,
) -> Option<(
    crate::control::VoiceTargetContext,
    ilium_voice::VoiceCommand,
)> {
    let current_context = crate::control::VoiceTargetContext::capture(app);
    if delivered_context == Some(current_context) {
        return None;
    }

    // Fixed13 trusted semantic schemas plus <=64KiB custom prompt. Reserve
    // original source, escaped JSON/transport derivatives and fixed scaffolding
    // before constructing either owned definition or rendered instructions.
    if app.voice_settings.custom_prompt.len() > 64 * 1024 {
        return None;
    }
    let context_bytes = crate::normal_voice::capture_bytes(
        &app.voice_settings,
        ilium_prompts::voice::VOICE_MOD_SYSTEM_INSTRUCTIONS.len(),
        include_str!("control/tools.rs").len(),
    )?;
    let context_source = std::sync::Arc::new(
        crate::execution::process_quota()
            .reserve_external_storage(context_bytes)
            .ok()?,
    );
    let instructions =
        crate::control::system_instructions(&app.voice_settings.custom_prompt, current_context);
    let tools = control_plane.tool_definitions();
    let actual = ilium_voice::context_capture_bytes(&instructions, &tools)?
        .checked_add(instructions.capacity())?
        .checked_add(
            tools
                .capacity()
                .checked_mul(std::mem::size_of::<ilium_voice::VoiceToolDefinition>())?,
        )?;
    if actual > context_bytes {
        return None;
    }
    Some((
        current_context,
        ilium_voice::VoiceCommand::UpdateContext(
            ilium_voice::OwnedVoiceContext::charged(
                instructions,
                tools,
                context_bytes,
                std::sync::Arc::new(NormalVoiceContextAllocation {
                    _source: context_source,
                }),
            )
            .ok()?,
        ),
    ))
}
/// Keeps the long-lived Realtime session aligned with client focus and the
/// server's latest agent classification without restarting audio or losing
/// conversation state.
async fn reconcile_voice_target_context(
    app: &mut App,
    _control_plane: &crate::control::ControlPlane,
    service: Option<&ilium_voice::VoiceService>,
    delivered: &mut Option<crate::control::VoiceTargetContext>,
) {
    let Some(service) = service else {
        *delivered = None;
        return;
    };
    if app.voice_shutdown_requested && !app.voice_shutdown_after_delivery {
        return;
    }
    let target = crate::control::VoiceTargetContext::capture(app);
    if *delivered == Some(target) && app.normal_voice_context_slot.is_none() {
        app.normal_voice_context_waiting = false;
        return;
    }
    if app.normal_voice_context_slot.is_some() {
        return;
    }
    let actor = service.instance_identity();
    let slot = match app
        .normal_voice_commands
        .reserve_context(actor.clone(), &crate::execution::process_quota())
    {
        Ok(slot) => slot,
        Err(error) => {
            app.normal_voice_context_waiting = true;
            app.status_message = Some(error);
            return;
        }
    };
    let kind = crate::voice_preparation::PreparationKind::Context {
        target,
        actor: actor.clone(),
    };
    match app.voice_preparation.request(&app.voice_settings, kind) {
        Ok(()) => {
            app.normal_voice_context_slot = Some((slot, target, actor));
            app.normal_voice_context_waiting = true;
        }
        Err(error) => {
            app.normal_voice_commands.cancel_context(&slot);
            app.normal_voice_context_waiting = error.is_retryable();
            app.status_message = Some(format!("Voice context preparation: {error:?}"));
            if !app.normal_voice_context_waiting {
                if !app.voice_shutdown_after_delivery {
                    service.request_shutdown();
                }
                app.voice_shutdown_requested = true;
            }
        }
    }
}

/// Install only an exact current settings/actor result. CPU envelopes release
/// here; the independent captured source follows the actor/context allocation.
fn collect_normal_voice_preparation(
    app: &mut App,
    service: &mut Option<ilium_voice::VoiceService>,
    delivered: &mut Option<crate::control::VoiceTargetContext>,
    media: &mut crate::media_control::MediaLease,
) {
    let Some(prepared) = app.voice_preparation.collect() else {
        return;
    };
    let current_settings = prepared.settings().enabled == app.voice_settings.enabled
        && prepared
            .settings()
            .has_same_runtime_configuration(&app.voice_settings);
    let kind = prepared.kind().clone();
    match kind {
        crate::voice_preparation::PreparationKind::NormalStartup { target } => {
            if !current_settings
                || !app.voice_settings.enabled
                || app.onboarding.is_some()
                || app.voice_demo_retirement_pending
                || service.is_some()
                || app.voice_shutdown_requested
                || app
                    .voice_native_retirement
                    .as_ref()
                    .is_some_and(|owner| owner.is_pending())
            {
                if app.voice_settings.enabled && app.onboarding.is_none() {
                    app.resume_voice_after_onboarding();
                }
                return;
            }
            if !matches!(
                &prepared.value,
                Ok(crate::voice_preparation::PreparedValue::Startup(_))
            ) {
                match prepared.value {
                    Err(error) => app.update_voice_connection_state(
                        ilium_voice::VoiceConnectionState::Failed(error),
                    ),
                    _ => app.normal_voice_retirement_failed = true,
                }
                return;
            }
            let admission =
                match ilium_voice::VoiceService::admit_startup(crate::execution::process_quota()) {
                    Ok(admission) => admission,
                    Err(
                        reason @ (ilium_execution::RejectReason::Busy
                        | ilium_execution::RejectReason::WorkerBytes),
                    ) => {
                        app.voice_preparation.defer_ready(prepared, Instant::now());
                        app.status_message = Some(format!(
                            "Voice startup waiting for actor metadata: {reason:?}"
                        ));
                        return;
                    }
                    Err(reason) => {
                        app.update_voice_connection_state(
                            ilium_voice::VoiceConnectionState::Failed(format!(
                                "Voice startup metadata admission failed: {reason:?}"
                            )),
                        );
                        return;
                    }
                };
            let Ok(crate::voice_preparation::PreparedValue::Startup(startup)) = prepared.value
            else {
                return;
            };
            *service = Some(ilium_voice::VoiceService::start(startup, admission));
            *delivered = Some(target);
            media.request(app.voice_settings.pause_media_while_active);
        }
        crate::voice_preparation::PreparationKind::Context { target, actor } => {
            let Some((slot, expected, owner)) = app.normal_voice_context_slot.take() else {
                return;
            };
            let valid = current_settings
                && target == expected
                && std::sync::Arc::ptr_eq(&actor, &owner)
                && service.as_ref().is_some_and(|service| {
                    std::sync::Arc::ptr_eq(&owner, &service.instance_identity())
                });
            if !valid {
                app.normal_voice_commands.cancel_context(&slot);
                app.normal_voice_context_waiting = false;
                return;
            }
            match prepared.value {
                Ok(crate::voice_preparation::PreparedValue::Context(context)) => {
                    if app
                        .normal_voice_commands
                        .fill_context(&slot, ilium_voice::VoiceCommand::UpdateContext(context))
                        .is_ok()
                    {
                        *delivered = Some(target);
                    }
                    app.normal_voice_context_waiting = false;
                }
                Err(error) => {
                    app.normal_voice_commands.cancel_context(&slot);
                    app.normal_voice_context_waiting = false;
                    app.update_voice_connection_state(ilium_voice::VoiceConnectionState::Failed(
                        error,
                    ));
                    if !app.voice_shutdown_after_delivery {
                        if let Some(service) = service {
                            service.request_shutdown();
                        }
                    }
                    app.voice_shutdown_requested = true;
                }
                _ => {
                    app.normal_voice_commands.cancel_context(&slot);
                    app.normal_voice_retirement_failed = true;
                }
            }
        }
        crate::voice_preparation::PreparationKind::DemoStartup => {
            app.normal_voice_retirement_failed = true;
        }
    }
}
async fn next_demo_voice_event(
    runtime: &mut crate::onboarding::voice_runtime::VoiceDemoRuntime,
    pending: &mut Option<ilium_voice::VoiceEventReceipt>,
    retry_at: Option<tokio::time::Instant>,
) -> Option<ilium_voice::VoiceEventReceipt> {
    if pending.is_some() {
        if let Some(deadline) = retry_at {
            tokio::time::sleep_until(deadline).await;
        }
        return pending.take();
    }
    runtime.next_event().await
}

/// Disposes canceled originals only after recording their terminal disposition.
/// Payload content stays private; counts identify incomplete provider delivery.
fn audit_demo_voice_retirement(
    app: &mut App,
    runtime: &mut crate::onboarding::voice_runtime::VoiceDemoRuntime,
    failed: &mut bool,
) -> bool {
    let mut changed = false;
    if runtime.native_retirement_failed() {
        if !*failed {
            tracing::error!("Onboarding voice native owner joined with a failure");
        }
        *failed = true;
    }
    if let Some((event, allocation)) = runtime.take_cancelled_event() {
        tracing::warn!("Onboarding voice event explicitly canceled during shutdown");
        drop(event);
        drop(allocation);
        changed = true;
    }
    if let Some(commands) = runtime.take_cancelled_commands() {
        let count = commands.count();
        if count != 0 {
            app.status_message = Some(format!(
                "Voice demonstration stopped; {count} queued commands were canceled"
            ));
            tracing::warn!(
                count,
                "Onboarding voice queued commands explicitly canceled"
            );
            changed = true;
        }
        commands.dispose();
    }
    if let Some(outcome) = runtime.take_shutdown_outcome() {
        if matches!(
            &outcome.actor_exit,
            Some(
                ilium_voice::VoiceActorExit::Failed(_)
                    | ilium_voice::VoiceActorExit::Panicked(_)
                    | ilium_voice::VoiceActorExit::Canceled
            )
        ) {
            *failed = true;
            tracing::error!("Onboarding voice actor retired with a failed outcome");
            if !matches!(
                runtime.state.connection.as_ref(),
                ilium_voice::VoiceConnectionState::Failed(_)
            ) {
                app.status_message =
                    Some("Voice demonstration actor stopped with a failure".into());
            }
        }
        let commands = outcome.undelivered_commands.len();
        let outputs = outcome
            .undelivered_stop_outputs
            .as_ref()
            .map_or(0, Vec::len);
        if commands != 0 || outputs != 0 {
            app.status_message = Some(format!(
                "Voice demonstration stopped with {commands} commands and {outputs} tool results undelivered"
            ));
            tracing::warn!(
                commands,
                outputs,
                "Onboarding voice original undelivered values explicitly retired"
            );
        }
        // The original batch metadata remains alive until all its original
        // commands, outputs and actor error are destroyed by this outcome.
        drop(outcome);
        changed = true;
    }
    changed
}

async fn drain_demo_voice_retirement(
    app: &mut App,
    runtime: &mut crate::onboarding::voice_runtime::VoiceDemoRuntime,
    pending: &mut Option<ilium_voice::VoiceEventReceipt>,
    failed: &mut bool,
) -> std::io::Result<()> {
    runtime.shutdown().await;
    let notification = runtime.notification();
    let drain = async {
        loop {
            runtime.collect(Instant::now()).await;
            audit_demo_voice_retirement(app, runtime, failed);
            if !runtime.pending_work() && pending.is_none() {
                return;
            }
            if let Some(original) = pending.take() {
                if let Err(original) = runtime.handle_event(original).await {
                    *pending = Some(original);
                }
            }
            tokio::select! {
                event = runtime.next_event(), if pending.is_none() => {
                    if let Some(event) = event {
                        if let Err(original) = runtime.handle_event(event).await {
                            *pending = Some(original);
                        }
                    } else {
                        runtime.channel_closed().await;
                    }
                }
                () = notification.notified() => {}
                () = tokio::time::sleep(Duration::from_millis(20)) => {}
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(5), drain)
        .await
        .map_err(|_| {
            tracing::error!("Onboarding voice retirement deadline; original pending custody remains owned, native shutdown unverified");
            std::io::Error::new(std::io::ErrorKind::TimedOut,
                "voice demonstration retirement deadline; native shutdown unverified")
        })?;
    if *failed {
        return Err(std::io::Error::other(
            "voice demonstration retired with a failed actor or native outcome",
        ));
    }
    Ok(())
}

async fn next_voice_event(
    voice_service: &mut Option<ilium_voice::VoiceService>,
    pending: &mut Option<ilium_voice::VoiceEventReceipt>,
    retry_at: Option<tokio::time::Instant>,
) -> Option<ilium_voice::VoiceEventReceipt> {
    if pending.is_some() {
        if let Some(deadline) = retry_at {
            tokio::time::sleep_until(deadline).await;
        }
        return pending.take();
    }
    match voice_service {
        Some(service) => {
            let event = service.next_event().await;
            if event.is_none() && !service.actor_is_finished() {
                // Channel closure can precede actual actor retirement. Give
                // other event-loop branches time instead of spinning on None.
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            event
        }
        None => std::future::pending().await,
    }
}

/// A provider failure is delivered immediately before its event channel
/// closes. Preserve that actionable error instead of replacing it with the
/// less useful generic channel-closure message on the next event-loop turn.
fn should_report_unexpected_voice_stop(
    is_voice_enabled: bool,
    state: &ilium_voice::VoiceConnectionState,
) -> bool {
    is_voice_enabled && !matches!(state, ilium_voice::VoiceConnectionState::Failed(_))
}

#[derive(Debug)]
struct VoiceInvocationAllocation {
    _result: Option<std::sync::Arc<dyn std::fmt::Debug + Send + Sync>>,
    _source: std::sync::Arc<ilium_execution::StorageAdmission>,
}

fn handle_voice_event(
    app: &mut App,
    control_plane: &mut crate::control::ControlPlane,
    receipt: ilium_voice::VoiceEventReceipt,
) -> Result<Vec<ilium_voice::VoiceToolOutput>, (ilium_voice::VoiceEventReceipt, String)> {
    if matches!(receipt.event(), ilium_voice::VoiceEvent::ToolInvocations(_)) {
        let (event, source) = receipt.into_parts();
        let ilium_voice::VoiceEvent::ToolInvocations(invocations) = event else {
            unreachable!("checked original event variant")
        };
        let mut outputs = invocations
            .into_iter()
            .filter_map(|invocation| control_plane.begin_invocation(app, invocation))
            .collect::<Vec<_>>();
        for output in &mut outputs {
            output.allocation_hold = Some(std::sync::Arc::new(VoiceInvocationAllocation {
                _result: output.allocation_hold.take(),
                _source: std::sync::Arc::clone(&source),
            }));
        }
        return Ok(outputs);
    }
    project_final_voice_event(app, receipt).map(|()| Vec::new())
}

fn project_final_voice_event(
    app: &mut App,
    receipt: ilium_voice::VoiceEventReceipt,
) -> Result<(), (ilium_voice::VoiceEventReceipt, String)> {
    let derived = match crate::voice_presentation::prepare_derivation(
        &crate::execution::process_quota(),
        receipt.event(),
        app.voice_last_assistant_transcript.as_ref(),
    ) {
        Ok(hold) => hold,
        Err(reason) => {
            return Err((
                receipt,
                format!("Voice event waiting for storage admission: {reason:?}"),
            ))
        }
    };
    if let ilium_voice::VoiceEvent::AssistantTranscript(delta) = receipt.event() {
        if let Err(error) = app
            .voice_last_assistant_transcript
            .get_or_insert_with(String::new)
            .try_reserve_exact(delta.len())
        {
            return Err((
                receipt,
                format!("Voice transcript allocation failed: {error}"),
            ));
        }
    }
    let (event, original_storage) = receipt.into_parts();
    match event {
        ilium_voice::VoiceEvent::StateChanged(state) => {
            let has_error = matches!(&state, ilium_voice::VoiceConnectionState::Failed(_));
            app.update_voice_connection_state(state);
            app.voice_presentation_storage.state = Some(original_storage);
            if has_error {
                app.voice_presentation_storage.status = derived;
            }
        }
        ilium_voice::VoiceEvent::ToolInvocations(invocations) => {
            tracing::warn!(
                count = invocations.len(),
                "Voice invocations explicitly cancelled during original session shutdown"
            );
            app.status_message = Some(format!(
                "Cancelled {} voice tool invocation(s) during shutdown",
                invocations.len()
            ));
        }
        ilium_voice::VoiceEvent::UserTranscript(transcript) => {
            app.voice_last_user_transcript = Some(transcript);
            app.voice_presentation_storage.user = Some(original_storage);
        }
        ilium_voice::VoiceEvent::AssistantTranscript(delta) => {
            app.voice_last_assistant_transcript
                .get_or_insert_with(String::new)
                .push_str(&delta);
            app.voice_presentation_storage.assistant = derived;
        }
        ilium_voice::VoiceEvent::ProviderError(error) => {
            app.status_message = Some(format!("Voice provider error: {error}"));
            app.voice_presentation_storage.status = derived;
        }
    }
    Ok(())
}

/// Returns completed tool calls to the model only after the event loop has
/// dispatched every terminal/UI request produced by their execution.
async fn deliver_voice_tool_outputs(
    app: &mut App,
    voice_service: &mut Option<ilium_voice::VoiceService>,
    mut outputs: Vec<ilium_voice::VoiceToolOutput>,
) {
    let Some(service) = voice_service.as_ref() else {
        if !outputs.is_empty() {
            tracing::warn!(
                count = outputs.len(),
                "Tool outputs explicitly cancelled: original voice actor absent"
            );
            app.status_message =
                Some("Voice tool outputs cancelled because their original session ended".into());
        }
        return;
    };
    let owner = service.instance_identity();
    if let Some((original, pending)) = app.pending_normal_voice_outputs.take() {
        if !std::sync::Arc::ptr_eq(&original, &owner) {
            app.pending_normal_voice_outputs = Some((original, pending));
            app.status_message =
                Some("Earlier voice outputs await original actor cancellation receipt".into());
            if !outputs.is_empty() {
                app.normal_voice_retirement_failed = true;
                tracing::error!(cancelled_outputs=outputs.len(), "Voice output producer crossed actor fence; incoming originals explicitly cancelled");
            }
            return;
        }
        // A single retained head fences both incoming event and CPU collectors.
        if !outputs.is_empty() {
            app.normal_voice_retirement_failed = true;
            tracing::error!(cancelled_outputs=outputs.len(), "Voice output producer crossed retained-head fence; incoming originals explicitly cancelled");
            app.status_message =
                Some("Voice output producer ordering failed; earlier originals retained".into());
            outputs = pending;
        } else {
            outputs = pending;
        }
    }
    if outputs.is_empty() {
        return;
    }
    let terminating = voice_tool_outputs_request_shutdown(&outputs);
    // Preserve staged-stop intent even when context or FIFO admission is busy.
    // An ordinary shutdown signal must not overtake the original final outputs.
    if terminating {
        app.voice_shutdown_after_delivery = true;
        app.voice_shutdown_requested = true;
    }
    if app.normal_voice_context_waiting {
        app.pending_normal_voice_outputs = Some((owner, outputs));
        return;
    }
    let command = if terminating {
        ilium_voice::VoiceCommand::SubmitToolOutputsAndShutdown(outputs)
    } else {
        ilium_voice::VoiceCommand::SubmitToolOutputs(outputs)
    };
    match app.normal_voice_commands.enqueue(
        owner.clone(),
        command,
        &crate::execution::process_quota(),
    ) {
        Ok(()) => {
            if terminating {
                app.voice_shutdown_after_delivery = true;
                app.voice_shutdown_requested = true;
            }
        }
        Err(original) => {
            app.status_message = Some(original.reason);
            let outputs = match original.command {
                ilium_voice::VoiceCommand::SubmitToolOutputs(outputs)
                | ilium_voice::VoiceCommand::SubmitToolOutputsAndShutdown(outputs) => outputs,
                _ => unreachable!("typed tool-output command returns unchanged from admission"),
            };
            app.pending_normal_voice_outputs = Some((owner, outputs));
        }
    }
    if let Err(error) = app.normal_voice_commands.publish(service, Instant::now()) {
        app.status_message = Some(error);
    }
}

/// One terminating result makes shutdown the dominant disposition for the
/// whole parallel function-call batch, while all results are still returned.
fn voice_tool_outputs_request_shutdown(outputs: &[ilium_voice::VoiceToolOutput]) -> bool {
    outputs
        .iter()
        .any(|output| output.terminate_session_after_delivery)
}

/// All Result exits retain the original actor, event head and native custody
/// outside this bounded future. Timing out reports uncertainty, never success.
async fn drain_normal_voice_retirement(
    app: &mut App,
    service: &mut Option<ilium_voice::VoiceService>,
    pending: &mut Option<ilium_voice::VoiceEventReceipt>,
) -> std::io::Result<()> {
    app.voice_preparation.close();
    if let Some((slot, _, _)) = app.normal_voice_context_slot.take() {
        app.normal_voice_commands.cancel_context(&slot);
        app.normal_voice_context_waiting = false;
    }
    let drain = async {
        loop {
            drop(app.voice_preparation.collect());
            if let Some(original) = pending.take() {
                match project_final_voice_event(app, original) {
                    Ok(()) => app.voice_event_projection_pending = false,
                    Err((original, error)) => {
                        *pending = Some(original);
                        app.voice_event_projection_pending = true;
                        app.status_message = Some(error);
                    }
                }
            }
            // Unpublished replaceable context was explicitly cancelled above;
            // original younger semantic commands/final outputs still retire.
            app.normal_voice_context_waiting = false;
            deliver_voice_tool_outputs(app, service, Vec::new()).await;
            if let Some(actor) = service.as_mut() {
                if let Err(error) = app.normal_voice_commands.publish(actor, Instant::now()) {
                    app.status_message = Some(error);
                }
                if !app.voice_shutdown_after_delivery {
                    actor.request_shutdown();
                }
                app.voice_shutdown_requested = true;
                if pending.is_none() {
                    if let Ok(original) = actor.try_next_event() {
                        *pending = Some(original);
                        app.voice_event_projection_pending = true;
                    }
                }
            }
            if pending.is_none() {
                collect_normal_voice_retirement(app, service).await;
            }
            if !app.normal_voice_ownership_pending(service.is_some()) && pending.is_none() {
                return;
            }
            // Dated backpressure retry, not a capacity await on the interactive
            // loop. Core actor completion has a separate event-loop wake.
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(5), drain).await.map_err(|_| {
        tracing::error!("Normal voice retirement deadline; original actor/event/native custody remains owned and delivery is uncertain");
        std::io::Error::new(std::io::ErrorKind::TimedOut,
            "normal voice retirement deadline; command delivery or native shutdown unverified")
    })?;
    if app.normal_voice_retirement_failed
        || app
            .voice_native_retirement
            .as_ref()
            .is_some_and(crate::voice_retirement::VoiceRetirement::has_terminal_failure)
    {
        return Err(std::io::Error::other(
            "normal voice retired with a failed actor, undelivered final output or native outcome",
        ));
    }
    Ok(())
}

/// Observe the SAME actor only after its readiness hint. Finished batches keep
/// original metadata/events/commands alive until each disposition is consumed.
async fn collect_normal_voice_retirement(
    app: &mut App,
    voice_service: &mut Option<ilium_voice::VoiceService>,
) -> bool {
    let now = Instant::now();
    if let Some(retirement) = &mut app.voice_native_retirement {
        retirement.collect(now);
        if let Some(error) = retirement.take_failure() {
            app.status_message = Some(error);
        }
    }
    if app.voice_shutdown_batch.is_none()
        && voice_service
            .as_ref()
            .is_some_and(ilium_voice::VoiceService::actor_is_finished)
    {
        if let Some(service) = voice_service.take() {
            app.voice_shutdown_batch = Some(service.finish_actor_if_ready().await);
        }
    }
    let Some(mut batch) = app.voice_shutdown_batch.take() else {
        return !app.voice_shutdown_requested
            && app
                .voice_native_retirement
                .as_ref()
                .is_none_or(|owner| !owner.is_pending());
    };
    // Project one original per turn; overload keeps exact head and metadata.
    if let Some(receipt) = batch.events.pop_front() {
        if let Err((original, error)) = project_final_voice_event(app, receipt) {
            batch.events.push_front(original);
            app.status_message = Some(error);
        }
        app.voice_shutdown_batch = Some(batch);
        return false;
    }
    let state = std::mem::replace(
        &mut batch.state,
        ilium_voice::VoiceShutdownState::Complete(ilium_voice::VoiceActorExit::Canceled),
    );
    match state {
        ilium_voice::VoiceShutdownState::Pending(service) => {
            // No continue_shutdown await on the UI: restore the original actor
            // and its receiver until its next finished readiness observation.
            *voice_service = Some(*service);
        }
        ilium_voice::VoiceShutdownState::Complete(exit) => {
            batch.state = ilium_voice::VoiceShutdownState::Complete(exit);
            let Some(retirement) = &mut app.voice_native_retirement else {
                app.voice_shutdown_batch = Some(batch);
                app.status_message = Some(
                    "Voice native retirement owner unavailable; original custody retained".into(),
                );
                return false;
            };
            if retirement.retain(batch.audio_custody.clone()).is_err() {
                app.voice_shutdown_batch = Some(batch);
                return false;
            }
            app.voice_preparation.cancel();
            app.normal_voice_context_slot = None;
            let local = app.normal_voice_commands.cancel_after_actor_exit();
            app.normal_voice_context_waiting = false;
            let pending_outputs = app.pending_normal_voice_outputs.take();
            let pending_output_count = pending_outputs
                .as_ref()
                .map_or(0, |(_, outputs)| outputs.len());
            let interactions = app.take_voice_interaction_requests();
            let cancelled = batch.undelivered_commands.len() + local.len() + interactions.len();
            let final_outputs =
                batch.undelivered_stop_outputs.as_ref().map_or(0, Vec::len) + pending_output_count;
            match &batch.state {
                ilium_voice::VoiceShutdownState::Complete(
                    ilium_voice::VoiceActorExit::Completed,
                ) => {}
                ilium_voice::VoiceShutdownState::Complete(exit) => {
                    if matches!(
                        exit,
                        ilium_voice::VoiceActorExit::Failed(_)
                            | ilium_voice::VoiceActorExit::Panicked(_)
                    ) {
                        app.normal_voice_retirement_failed = true;
                    }
                    tracing::error!(
                        cancelled_commands = cancelled,
                        undelivered_stop_outputs = final_outputs,
                        "Original voice actor stopped; consumed provider delivery may be uncertain"
                    );
                    app.status_message = Some(format!("Voice actor stopped; {cancelled} commands and {final_outputs} final outputs undelivered"));
                }
                _ => {}
            }
            if final_outputs != 0 {
                app.normal_voice_retirement_failed = true;
            }
            if cancelled != 0 || final_outputs != 0 {
                tracing::warn!(
                    cancelled_commands = cancelled,
                    undelivered_stop_outputs = final_outputs,
                    "Voice originals explicitly cancelled; never replayed into replacement actor"
                );
                app.status_message = Some(format!("Original voice session cancelled {cancelled} queued commands and {final_outputs} final outputs"));
            }
            app.voice_shutdown_requested = false;
            app.voice_shutdown_after_delivery = false;
            // Batch metadata now remains owned by the same native custody in
            // the finite observer; all original cancellation receipts consumed.
        }
    }
    false
}

/// Reconciles a pending start/stop/reconfigure and, on a real start or stop
/// (not a reconfigure -- that's a live settings change, not the user
/// entering or leaving voice mode), pauses or resumes system media playback
/// through [`crate::media_control`] when
/// `VoiceSettings::pause_media_while_active` is on. Resuming always runs
/// regardless of that setting's *current* value and drains
/// the media owner's acknowledged set, so toggling the setting off requests
/// restoration even when no voice runtime transition is queued.
async fn reconcile_voice_runtime(
    app: &mut App,
    control_plane: &crate::control::ControlPlane,
    voice_service: &mut Option<ilium_voice::VoiceService>,
    normal_media: &mut crate::media_control::MediaLease,
) {
    if !app.voice_settings.pause_media_while_active {
        normal_media.request(false);
    }
    if let Some(service) = voice_service.as_ref() {
        if let Err(error) = app.normal_voice_commands.publish(service, Instant::now()) {
            app.status_message = Some(error);
        }
    }
    deliver_voice_tool_outputs(app, voice_service, Vec::new()).await;
    let onboarding = app.onboarding.is_some();
    if onboarding {
        app.onboarding_voice_suspended |= app.voice_settings.enabled;
    }
    let transition = onboarding || app.pending_voice_runtime_request().is_some();
    if onboarding
        || matches!(
            app.pending_voice_runtime_request(),
            Some(crate::app::VoiceRuntimeRequest::Stop)
        )
    {
        app.voice_preparation.cancel();
        if let Some((slot, _, _)) = app.normal_voice_context_slot.take() {
            app.normal_voice_commands.cancel_context(&slot);
        }
        app.normal_voice_context_waiting = false;
    }
    if transition && voice_service.is_some() && !app.voice_shutdown_requested {
        // Staged self-stop outputs must reach the original actor before close.
        if !app.voice_shutdown_after_delivery {
            if let Some(service) = voice_service.as_ref() {
                service.request_shutdown();
            }
        }
        app.voice_shutdown_requested = true;
        normal_media.request(false);
    }
    if app.voice_event_projection_pending {
        return;
    }
    collect_normal_voice_retirement(app, voice_service).await;
    if app.voice_shutdown_requested
        || app.voice_shutdown_batch.is_some()
        || app
            .voice_native_retirement
            .as_ref()
            .is_some_and(|owner| owner.is_pending())
    {
        return;
    }
    if onboarding {
        app.take_voice_runtime_request();
        normal_media.request(false);
        return;
    }
    // The other role may be off-page while native owners still retire.
    if app.voice_demo_retirement_pending {
        return;
    }
    if std::mem::take(&mut app.onboarding_voice_suspended) {
        app.resume_voice_after_onboarding();
    }
    let Some(request) = app.take_voice_runtime_request() else {
        return;
    };
    if voice_service.is_some() {
        // Never consume an intent while its original actor still owns output.
        app.resume_voice_after_onboarding();
        return;
    }
    match request {
        crate::app::VoiceRuntimeRequest::Stop => {
            app.update_voice_connection_state(ilium_voice::VoiceConnectionState::Disabled);
            normal_media.request(false);
        }
        crate::app::VoiceRuntimeRequest::Start => {
            *voice_service = start_voice_service(app, control_plane);
            normal_media.request(false);
            if voice_service.is_some() && app.voice_settings.pause_media_while_active {
                normal_media.restart();
            }
        }
        crate::app::VoiceRuntimeRequest::Reconfigure => {
            *voice_service = start_voice_service(app, control_plane);
            normal_media
                .request(voice_service.is_some() && app.voice_settings.pause_media_while_active);
        }
    }
}

/// Delivers lossless push-to-talk edges only after start/stop/reconfigure has
/// reconciled the actor for this input batch.
async fn deliver_voice_interactions(
    app: &mut App,
    voice_service: Option<&ilium_voice::VoiceService>,
) {
    if app.normal_voice_context_waiting || app.voice_shutdown_requested {
        return;
    }
    let Some(service) = voice_service else {
        return;
    };
    let mut requests = app.take_voice_interaction_requests().into_iter();
    while let Some(request) = requests.next() {
        let command = match request {
            crate::app::VoiceInteractionRequest::StartPushToTalk => {
                ilium_voice::VoiceCommand::StartPushToTalk
            }
            crate::app::VoiceInteractionRequest::StopPushToTalk => {
                ilium_voice::VoiceCommand::StopPushToTalk
            }
        };
        if let Err(original) = app.normal_voice_commands.enqueue(
            service.instance_identity(),
            command,
            &crate::execution::process_quota(),
        ) {
            app.status_message = Some(original.reason);
            app.restore_voice_interaction_requests(
                std::iter::once(request).chain(requests).collect(),
            );
            break;
        }
    }
    if let Err(error) = app.normal_voice_commands.publish(service, Instant::now()) {
        app.status_message = Some(error);
    }
}

/// Answers every pending `ilium voice say` offer: the sentences go into the
/// live voice session's command queue -- the same `VoiceCommand::SendText`
/// path the accessibility/live-protocol seam uses, so the model receives them
/// as user turns and routes them exactly like recognised speech -- and the
/// server is told what happened. Runs after lifecycle reconciliation, so an
/// offer that asked to start voice finds its session (or its failure).
#[derive(Debug)]
struct VoiceOfferAllocation {
    // Original strings and independently admitted provider/transcript copies.
    _original: Option<crate::connection::EventRetention>,
    _derived: Option<crate::connection::EventRetention>,
}
impl ilium_voice::VoiceTextAllocation for VoiceOfferAllocation {}

async fn deliver_voice_text_offers(
    app: &mut App,
    voice_service: Option<&ilium_voice::VoiceService>,
) {
    if app.normal_voice_context_waiting || app.voice_shutdown_requested {
        return;
    }
    if voice_service.is_none()
        && app.voice_settings.enabled
        && (app.pending_voice_runtime_request().is_some()
            || app
                .voice_native_retirement
                .as_ref()
                .is_some_and(|owner| owner.is_pending()))
    {
        return;
    }
    let commands = voice_service.map(ilium_voice::VoiceService::command_sender);
    let mut fifo = std::mem::take(&mut app.normal_voice_commands);
    let destination = voice_service.map(|service| (&mut fifo, service.instance_identity()));
    deliver_voice_text_offers_inner(app, commands.as_ref(), destination);
    app.normal_voice_commands = fifo;
    if let Some(service) = voice_service {
        if let Err(error) = app.normal_voice_commands.publish(service, Instant::now()) {
            app.status_message = Some(error);
        }
    }
}

#[cfg(test)]
fn deliver_voice_text_offers_to(
    app: &mut App,
    commands: Option<&mpsc::Sender<ilium_voice::VoiceCommand>>,
) {
    deliver_voice_text_offers_inner(app, commands, None);
}

fn deliver_voice_text_offers_inner(
    app: &mut App,
    commands: Option<&mpsc::Sender<ilium_voice::VoiceCommand>>,
    mut fifo: Option<(
        &mut crate::normal_voice::NormalVoiceCommands,
        std::sync::Arc<()>,
    )>,
) {
    let mut offers = app.take_voice_text_offers().into_iter();
    while let Some(mut offer) = offers.next() {
        // Reply admission precedes every actor side effect. This covers the
        // longest possible local rejection, including a failed actor's reason.
        let error_bytes = match &app.voice_connection_state {
            ilium_voice::VoiceConnectionState::Failed(error) => error.capacity(),
            _ => 512,
        };
        let reservation = match app.outbound_admission.as_ref() {
            Some(client) => {
                crate::ipc_preparation::reserve_request(client, error_bytes.saturating_add(4096))
            }
            None => Err(ilium_execution::RejectReason::Closed),
        };
        let reservation = match reservation {
            Ok(reservation) => reservation,
            Err(reason) => {
                if matches!(
                    reason,
                    ilium_execution::RejectReason::Closed
                        | ilium_execution::RejectReason::InvalidCost
                        | ilium_execution::RejectReason::AccountingPoisoned
                ) {
                    tracing::error!(
                        request_id = offer.request_id,
                        ?reason,
                        "voice offer reply admission owner unavailable"
                    );
                    app.status_message =
                        Some(format!("Voice offer cannot be answered: {reason:?}"));
                    app.exit_reason = Some(crate::app::ClientExitReason::Quit);
                }
                app.restore_voice_text_offers(std::iter::once(offer).chain(offers).collect());
                break;
            }
        };
        // Retain the typed placeholder before publishing: no fallible
        // ownership conversion may follow a successful actor acceptance.
        let placeholder = ilium_ipc::ClientRequest::AnswerVoiceText {
            request_id: offer.request_id,
            result: Err(ilium_ipc::VoiceTextRejection::new(
                ilium_ipc::VoiceTextRejectionCode::VoiceUnavailable,
                "",
            )),
        };
        let Ok(admitted) = reservation.retain(placeholder) else {
            app.restore_voice_text_offers(std::iter::once(offer).chain(offers).collect());
            break;
        };
        let (_, reply_retention) = admitted.into_parts();
        let Some(result) = voice_text_offer_result_with_fifo(
            app,
            commands,
            &mut offer,
            fifo.as_mut()
                .map(|(queue, owner)| (&mut **queue, std::sync::Arc::clone(owner))),
        ) else {
            app.restore_voice_text_offers(std::iter::once(offer).chain(offers).collect());
            break;
        };
        app.enqueue_admitted_request(reply_retention.retain(
            ilium_ipc::ClientRequest::AnswerVoiceText {
                request_id: offer.request_id,
                result,
            },
        ));
    }
}

/// `None` is temporary actor/storage pressure. It leaves every original
/// sentence in the offer; acceptance publishes the whole offer atomically.
#[cfg(test)]
fn voice_text_offer_result(
    app: &mut App,
    commands: Option<&mpsc::Sender<ilium_voice::VoiceCommand>>,
    offer: &mut crate::app::VoiceTextOffer,
) -> Option<ilium_ipc::VoiceTextResult> {
    voice_text_offer_result_with_fifo(app, commands, offer, None)
}

fn voice_text_offer_result_with_fifo(
    app: &mut App,
    commands: Option<&mpsc::Sender<ilium_voice::VoiceCommand>>,
    offer: &mut crate::app::VoiceTextOffer,
    fifo: Option<(
        &mut crate::normal_voice::NormalVoiceCommands,
        std::sync::Arc<()>,
    )>,
) -> Option<ilium_ipc::VoiceTextResult> {
    use ilium_ipc::{VoiceTextAccepted, VoiceTextRejection, VoiceTextRejectionCode};
    let Some(commands) = commands else {
        return Some(Err(if app.voice_settings.enabled {
            let reason = match &app.voice_connection_state {
                ilium_voice::VoiceConnectionState::Failed(error) => error.clone(),
                _ => "the voice session is not running".to_owned(),
            };
            VoiceTextRejection::new(VoiceTextRejectionCode::VoiceUnavailable, reason)
        } else {
            VoiceTextRejection::new(
                VoiceTextRejectionCode::VoiceOff,
                "voice control is off; press F8 in the Ilium client or pass --start",
            )
        }));
    };
    if let ilium_voice::VoiceConnectionState::Failed(error) = &app.voice_connection_state {
        return Some(Err(VoiceTextRejection::new(
            VoiceTextRejectionCode::VoiceUnavailable,
            error.clone(),
        )));
    }
    if offer.sentences.len() > commands.max_capacity() {
        return Some(Err(VoiceTextRejection::new(
            VoiceTextRejectionCode::InvalidRequest,
            "the voice offer exceeds the actor queue capacity",
        )));
    }
    let mut reserved_fifo = match fifo {
        Some((fifo, owner)) => match fifo.reserve_batch(
            owner,
            offer.sentences.len(),
            &crate::execution::process_quota(),
        ) {
            Ok(reservation) => Some(reservation),
            Err(error) => {
                app.status_message = Some(error);
                return None;
            }
        },
        None => None,
    };
    let permits = if reserved_fifo.is_none() {
        match commands.try_reserve_many(offer.sentences.len()) {
            Ok(permits) => Some(permits),
            Err(mpsc::error::TrySendError::Full(_)) => return None,
            Err(mpsc::error::TrySendError::Closed(_)) => {
                return Some(Err(VoiceTextRejection::new(
                    VoiceTextRejectionCode::VoiceUnavailable,
                    "the voice session ended before the text could be delivered",
                )))
            }
        }
    } else {
        None
    };
    // Source-pinned serde_json starts with Vec128 and RawVec doubles. Its
    // serialized string and tungstenite's copied output Vec coexist (each
    // <=2*(6*UTF8 bytes + framing/scaffolding)). JSON input, diagnostic clone,
    // and last-user transcript add three unescaped copies. Logging writer
    // buffers have their own pre-growth service reservation.
    let bytes = offer.sentences.iter().try_fold(4096usize, |total, text| {
        let unescaped_copies = text.capacity().checked_mul(3)?;
        let escaped = text.capacity().checked_mul(6)?.checked_add(1024)?;
        let serialized_and_socket = escaped.checked_mul(2)?.checked_mul(2)?;
        total
            .checked_add(unescaped_copies)?
            .checked_add(serialized_and_socket)
    })?;
    if offer._retention.as_ref().is_some_and(|owner| {
        bytes
            .saturating_add(owner.declared_bytes())
            .saturating_add(512)
            > 64 * 1024 * 1024
    }) {
        return Some(Err(VoiceTextRejection::new(
            VoiceTextRejectionCode::VoiceUnavailable,
            "voice text exceeds the bounded provider publication allocation budget",
        )));
    }
    let derived = match &offer._retention {
        Some(owner) => match owner.try_reserve_derived(bytes) {
            Ok(retention) => Some(retention),
            Err(
                error @ (ilium_execution::RejectReason::Closed
                | ilium_execution::RejectReason::InvalidCost
                | ilium_execution::RejectReason::AccountingPoisoned),
            ) => {
                tracing::error!(?error, "voice text publication allocation unavailable");
                return Some(Err(VoiceTextRejection::new(
                    VoiceTextRejectionCode::VoiceUnavailable,
                    "voice text publication allocation owner is unavailable",
                )));
            }
            Err(error) => {
                app.status_message = Some(format!("Voice text delivery queued: {error:?}"));
                return None;
            }
        },
        None => None,
    };
    app.record_typed_voice_text(&offer.sentences);
    app.retain_typed_voice_text(derived.clone());
    let sentence_count = u32::try_from(offer.sentences.len()).unwrap_or(u32::MAX);
    let allocation: std::sync::Arc<dyn ilium_voice::VoiceTextAllocation> =
        std::sync::Arc::new(VoiceOfferAllocation {
            _original: offer._retention.take(),
            _derived: derived,
        });
    if let Some(reserved) = reserved_fifo.as_mut() {
        for sentence in offer.sentences.drain(..) {
            reserved.send(ilium_voice::VoiceCommand::SendText(
                ilium_voice::OwnedVoiceText::charged(sentence, std::sync::Arc::clone(&allocation)),
            ));
        }
    } else if let Some(permits) = permits {
        for (permit, sentence) in permits.zip(offer.sentences.drain(..)) {
            permit.send(ilium_voice::VoiceCommand::SendText(
                ilium_voice::OwnedVoiceText::charged(sentence, std::sync::Arc::clone(&allocation)),
            ));
        }
    }
    Some(Ok(VoiceTextAccepted {
        sentence_count,
        phase: voice_text_phase(&app.voice_connection_state),
        started_voice: offer.started_voice,
    }))
}
fn voice_text_phase(state: &ilium_voice::VoiceConnectionState) -> ilium_ipc::VoiceTextPhase {
    use ilium_ipc::VoiceTextPhase;
    use ilium_voice::VoiceConnectionState;
    match state {
        VoiceConnectionState::Disabled
        | VoiceConnectionState::Connecting
        | VoiceConnectionState::Failed(_) => VoiceTextPhase::Connecting,
        VoiceConnectionState::Listening => VoiceTextPhase::Listening,
        VoiceConnectionState::Recording => VoiceTextPhase::Recording,
        VoiceConnectionState::Thinking => VoiceTextPhase::Thinking,
        VoiceConnectionState::Speaking => VoiceTextPhase::Speaking,
    }
}

/// Applies `first` (already received) and then drains + applies a bounded
/// batch of queued `ServerEvent`s without waiting for more --
/// coalescing consecutive `ScreenUpdate`s for the *same* `pane_id` into one
/// concatenated feed instead of applying each queued chunk individually.
/// Bytes are concatenated, never dropped: `ScreenUpdate` carries raw PTY
/// output that a `vt100::Parser` must see byte-for-byte and in order (see
/// `ilium_ipc::ServerEvent::ScreenUpdate`'s doc comment) -- unlike a
/// cell-diff or full-screen snapshot, an intermediate chunk can't simply be
/// discarded once a newer one for the same pane arrives, since it may hold
/// the other half of a split escape sequence or output a later chunk
/// doesn't repeat. Concatenating still gets the win this coalescing is for:
/// N queued chunks for one busy pane become one `vt100::Parser::process`
/// call and one dirty frame instead of N of each. Events of any other kind,
/// or a `ScreenUpdate` for a *different* pane, are applied immediately in
/// arrival order -- only a same-pane_id run of consecutive `ScreenUpdate`s
/// is ever merged, so no ordering between different panes or event kinds
/// changes. The batch limit is deliberately applied only between complete
/// events, so it preserves the stream order while allowing input to run.
fn apply_server_events(
    app: &mut App,
    events_rx: &mut mpsc::Receiver<crate::connection::Received<ilium_ipc::ServerEvent>>,
    first: crate::connection::Received<ilium_ipc::ServerEvent>,
    naming_workers: &mut NamingWorkers,
    icon_search_workers: &mut IconSearchWorkers,
    trigger_execution_lease: &mut TriggerExecutionLease,
    home_dir: Option<&std::path::Path>,
) -> ServerEventDamage {
    let mut next = Some(first);
    let mut damage = ServerEventDamage::default();
    for _ in 0..MAX_SERVER_EVENTS_PER_BATCH {
        let received = match next
            .take()
            .or_else(|| app.pending_terminal_events.pop_front())
        {
            Some(event) => event,
            None => match events_rx.try_recv() {
                Ok(event) => event,
                Err(_) => break,
            },
        };
        let event_damage = server_event_damage(app, received.view());
        damage.needs_redraw |= event_damage.needs_redraw;
        damage.needs_immediate_redraw |= event_damage.needs_immediate_redraw;
        let (event, retention) = received.into_parts();
        // Register BEFORE admission so a concurrent last-owner release cannot
        // be lost between refusal and the next event-loop select.
        let mut admission_wake =
            Box::pin(crate::execution::admission_notification().notified_owned());
        admission_wake.as_mut().enable();
        let mut projection_update =
            match crate::incoming_projection::ProjectionRetention::prepare_owned(
                &event,
                app,
                retention.as_ref(),
            ) {
                Ok(update) => update,
                Err(reason) => {
                    app.pending_terminal_events.push_front(
                        crate::connection::Received::with_retention(event, retention),
                    );
                    if matches!(
                        reason,
                        ilium_execution::RejectReason::Closed
                            | ilium_execution::RejectReason::InvalidCost
                            | ilium_execution::RejectReason::AccountingPoisoned
                    ) {
                        app.status_message = Some(format!(
                            "Incoming projection cannot be admitted: {reason:?}"
                        ));
                        app.projection_admission_failure = Some(reason);
                        app.exit_reason = Some(crate::app::ClientExitReason::Quit);
                    } else {
                        app.status_message = Some(
                            "Incoming projection is waiting for bounded UI storage credit".into(),
                        );
                        app.projection_admission_wake = Some(admission_wake);
                        app.projection_busy_retry_at = (reason
                            == ilium_execution::RejectReason::Busy)
                            .then(|| tokio::time::Instant::now() + Duration::from_millis(1));
                    }
                    damage.needs_redraw = true;
                    break;
                }
            };
        app.processing_derivation_retention = projection_update.derived_retention.take();
        app.processing_event_retention = retention;
        if let ilium_ipc::ServerEvent::DebugLoggingChanged { enabled } = event {
            synchronize_debug_logging_from_server(app, enabled);
        } else if let Some(occurrence) = crate::render_cache::apply(app, event) {
            damage.needs_redraw = true;
            damage.needs_immediate_redraw = true;
            if trigger_execution_lease.claim() {
                app.handle_trigger_occurrence(occurrence);
            }
        }
        app.incoming_projection
            .commit(projection_update, app.processing_event_retention.as_ref());
        app.processing_event_retention = None;
        app.processing_derivation_retention = None;
        if app.status_message.as_deref()
            == Some("Incoming projection is waiting for bounded UI storage credit")
        {
            app.status_message = None;
        }
        app.incoming_projection
            .prune(&app.tree, &app.agent_debug_logs);
        if !app.pending_terminal_events.is_empty() {
            break;
        }
    }
    if !app.pending_terminal_events.is_empty() {
        while app.pending_terminal_events.len() < MAX_PENDING_SERVER_EVENTS {
            let Ok(event) = events_rx.try_recv() else {
                break;
            };
            app.pending_terminal_events.push_back(event);
        }
        damage.needs_redraw |= confirm_suspended_panes_from_pending_snapshots(app);
    }
    dispatch_pending_app_work(app, naming_workers, icon_search_workers, home_dir);
    damage
}

/// Network intake appends to the original FIFO. Callers which already popped
/// its oldest member pass that member directly to `apply_server_events`.
fn queue_server_event_in_order(
    app: &mut App,
    event: crate::connection::Received<ilium_ipc::ServerEvent>,
) -> Option<crate::connection::Received<ilium_ipc::ServerEvent>> {
    if app.pending_terminal_events.is_empty() && app.projection_admission_wake.is_none() {
        return Some(event);
    }
    app.pending_terminal_events.push_back(event);
    confirm_suspended_panes_from_pending_snapshots(app);
    if app.projection_admission_wake.is_some() {
        None
    } else {
        app.pending_terminal_events.pop_front()
    }
}

/// Lookahead proves only cancellation of a suspended instance absent from an
/// authoritative later snapshot. Every received payload and guard remains in
/// FIFO custody; snapshot installation and all other effects happen normally.
fn confirm_suspended_panes_from_pending_snapshots(app: &mut App) -> bool {
    let mut changed = false;
    for received in &app.pending_terminal_events {
        let ilium_ipc::ServerEvent::TreeSnapshot(tree) = received.view() else {
            continue;
        };
        for (pane_id, runtime) in &mut app.panes {
            let crate::app::PaneRuntime::Terminal(view) = runtime else {
                continue;
            };
            if !view.has_suspended_output() || tree.get(*pane_id).is_some() {
                continue;
            }
            let identity = std::sync::Arc::clone(&view.identity);
            if view.confirm_removed(&identity) {
                tracing::warn!(
                    ?pane_id,
                    reason = "confirmed_server_snapshot_removal",
                    "suspended terminal instance canceled after authoritative removal confirmation"
                );
                changed = true;
            }
        }
    }
    changed
}

/// Applies the server-accepted state to this client's writer and UI without
/// persisting or queueing another request. This is what makes one Debug toggle
/// converge every client already attached to the same detached session.
fn synchronize_debug_logging_from_server(app: &mut App, enabled: bool) {
    if !enabled {
        tracing::info!("server logging disable synchronization requested");
    }
    app.debug_logging.synchronize(enabled);
    mark_debug_logging_pending(app);
}

/// Applies one received input event and any immediately queued successors.
/// Consecutive mouse-motion reports are coalesced to their newest position:
/// only that position can determine the current hover state, while keeping
/// every non-motion event in order preserves click, drag, and scroll input.
fn dispatch_ready_input_events(
    app: &mut App,
    input_rx: &mut terminal_input_owner::InputReceiver,
    naming_workers: &mut NamingWorkers,
    icon_search_workers: &mut IconSearchWorkers,
    home_dir: Option<&std::path::Path>,
    first: Result<terminal_input_owner::InputEvent, terminal_input_owner::InputFailure>,
    deferred: &mut input_backlog::InputBacklog<
        Result<terminal_input_owner::InputEvent, terminal_input_owner::InputFailure>,
    >,
) -> Option<terminal_input_owner::InputFailure> {
    let mut failure = None;
    let lookahead = {
        let mut ready = input_backlog::BacklogReady {
            backlog: deferred,
            upstream: input_rx,
        };
        for_each_ready_input_event_until(&mut ready, first, |event| {
            match event {
                Ok(event) => {
                    let destination = if matches!(event.view(), Event::Paste(_)) {
                        app.native_terminal_paste_destination()
                    } else {
                        None
                    };
                    if let Some(pane_id) = destination {
                        if !crate::keys::intercept_event(app, event.view()) {
                            if let Err(error) = app.capture_native_terminal_paste(pane_id, event) {
                                failure = Some(error);
                            }
                        }
                        // Preserve App::handle_event's post-key layout sampling,
                        // including an event consumed by a protected input owner.
                        app.tick_layout_animation(Instant::now());
                        dispatch_pending_app_work(
                            app,
                            naming_workers,
                            icon_search_workers,
                            home_dir,
                        );
                    } else {
                        event.dispatch(|event| {
                            dispatch_input_event(
                                app,
                                naming_workers,
                                icon_search_workers,
                                home_dir,
                                event,
                            )
                        });
                    }
                }
                Err(error) => failure = Some(error),
            }
            if let Some(error) = app.take_native_paste_failure() {
                failure = Some(terminal_input_owner::InputFailure::combine(
                    failure.take(),
                    error,
                ));
            }
            failure.is_none() && app.pending_native_paste.is_none()
        })
    };
    if let Err(refused) = deferred.restore(None, lookahead) {
        // Preserve every incoming original on a violated two-slot invariant.
        // Existing backlog ownership is unchanged and drains during shutdown.
        for event in [refused.original, refused.lookahead].into_iter().flatten() {
            failure = Some(match event {
                Ok(original) => {
                    terminal_input_owner::InputFailure::undispatched(original, failure.take())
                }
                Err(error) => terminal_input_owner::InputFailure::combine(failure.take(), error),
            });
        }
    }
    failure
}

trait InputBatchItem: Sized {
    fn view(&self) -> Option<&Event>;
    fn coalesce_motion(self) -> Result<(), Self>;
}

impl InputBatchItem
    for Result<terminal_input_owner::InputEvent, terminal_input_owner::InputFailure>
{
    fn view(&self) -> Option<&Event> {
        self.as_ref()
            .ok()
            .map(terminal_input_owner::InputEvent::view)
    }

    fn coalesce_motion(self) -> Result<(), Self> {
        match self {
            Ok(event) => event.coalesce_motion().map_err(Ok),
            Err(error) => Err(Err(error)),
        }
    }
}

trait ReadyInput {
    type Item: InputBatchItem;
    fn try_next(&mut self) -> Result<Self::Item, mpsc::error::TryRecvError>;
}

impl ReadyInput for terminal_input_owner::InputReceiver {
    type Item = Result<terminal_input_owner::InputEvent, terminal_input_owner::InputFailure>;

    fn try_next(&mut self) -> Result<Self::Item, mpsc::error::TryRecvError> {
        self.try_recv()
    }
}

#[cfg(test)]
impl InputBatchItem for Event {
    fn view(&self) -> Option<&Event> {
        Some(self)
    }

    fn coalesce_motion(self) -> Result<(), Self> {
        if is_mouse_motion(Some(&self)) {
            Ok(())
        } else {
            Err(self)
        }
    }
}

#[cfg(test)]
impl ReadyInput for mpsc::Receiver<Event> {
    type Item = Event;

    fn try_next(&mut self) -> Result<Self::Item, mpsc::error::TryRecvError> {
        self.try_recv()
    }
}

fn is_mouse_motion(event: Option<&Event>) -> bool {
    matches!(
        event,
        Some(Event::Mouse(crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Moved,
            ..
        }))
    )
}

/// Count every consumed original, including superseded motion, toward the
/// existing fairness limit. Any lookahead already received is dispatched in
/// this batch; the next original after the boundary remains in the receiver.
#[cfg(test)]
fn for_each_ready_input_event<S: ReadyInput>(
    input_rx: &mut S,
    first: S::Item,
    mut dispatch: impl FnMut(S::Item),
) {
    let remaining = for_each_ready_input_event_until(input_rx, first, |event| {
        dispatch(event);
        true
    });
    assert!(
        remaining.is_none(),
        "an uninterrupted batch dispatches its lookahead"
    );
}

/// A paused consumer returns any already-read lookahead to its bounded owner.
/// It never consumes the next original after a blocked semantic Paste.
fn for_each_ready_input_event_until<S: ReadyInput>(
    input_rx: &mut S,
    first: S::Item,
    mut dispatch: impl FnMut(S::Item) -> bool,
) -> Option<S::Item> {
    let mut next = Some(first);
    let mut received = 1;
    loop {
        let event = match next.take() {
            Some(event) => event,
            None => {
                if received == MAX_INPUT_EVENTS_PER_BATCH {
                    break;
                }
                match input_rx.try_next() {
                    Ok(event) => {
                        received += 1;
                        event
                    }
                    Err(_) => break,
                }
            }
        };
        let event = if is_mouse_motion(event.view()) {
            let mut newest_motion = event;
            while received < MAX_INPUT_EVENTS_PER_BATCH {
                let candidate = match input_rx.try_next() {
                    Ok(candidate) => {
                        received += 1;
                        candidate
                    }
                    Err(_) => break,
                };
                if is_mouse_motion(candidate.view()) {
                    match newest_motion.coalesce_motion() {
                        Ok(()) => newest_motion = candidate,
                        Err(original) => {
                            newest_motion = original;
                            next = Some(candidate);
                            break;
                        }
                    }
                } else {
                    next = Some(candidate);
                    break;
                }
            }
            newest_motion
        } else {
            event
        };
        if !dispatch(event) {
            return next;
        }
        if received == MAX_INPUT_EVENTS_PER_BATCH && next.is_none() {
            break;
        }
    }
    None
}

/// Applies and clears `pending`, if it holds a merged `ScreenUpdate` run --
/// see `apply_server_events`.
#[cfg(test)]
fn flush_pending_screen_update(
    app: &mut App,
    pending: &mut Option<(ilium_core::NodeId, u64, u64, Vec<u8>)>,
) -> bool {
    if let Some((pane_id, first_sequence, sequence, bytes)) = pending.take() {
        let is_displayed = app.is_pane_displayed(pane_id);
        crate::render_cache::apply(
            app,
            ilium_ipc::ServerEvent::ScreenUpdate {
                pane_id,
                first_sequence,
                sequence,
                bytes,
            },
        );
        return is_displayed;
    }
    false
}

/// Dispatches one crossterm input event, then drains and actually spawns
/// any `PendingRetitleRequest`s that dispatch produced -- from manual actions
/// or the automatic trigger router.
/// `App` only ever queues these (see `PendingRetitleRequest`'s doc
/// comment); this is the one place with both an `App` and a
/// `NamingWorkers` handle to actually start the background worker.
fn dispatch_input_event(
    app: &mut App,
    naming_workers: &mut NamingWorkers,
    icon_search_workers: &mut IconSearchWorkers,
    home_dir: Option<&std::path::Path>,
    event: Event,
) {
    if let Event::Resize(cols, rows) = &event {
        app.set_screen_area(Rect::new(0, 0, *cols, *rows));
        crate::onboarding::integration::resize(app);
        return;
    }
    app.handle_event(event);
    dispatch_pending_app_work(app, naming_workers, icon_search_workers, home_dir);
}

/// Drains synchronous `App` outboxes into the async/background owners. This
/// must run after every event-loop branch, not only keyboard input, because
/// voice tool execution invokes the same semantic methods and can request
/// tests, icon search, retitling, or restructuring too.
fn handle_naming_admission(
    result: Result<(), Box<ilium_execution::Rejected<crate::naming_workers::NamingRequest>>>,
    app: &mut App,
) {
    let Err(rejected) = result else {
        return;
    };
    use ilium_execution::RejectReason;
    let reason = rejected.reason;
    if matches!(
        reason,
        RejectReason::Busy
            | RejectReason::QueueFull
            | RejectReason::JobLimit
            | RejectReason::InputBytes
            | RejectReason::ResultBytes
            | RejectReason::WorkerBytes
    ) {
        match app.requeue_naming_request(rejected.value) {
            Ok(()) => {
                app.status_message = Some(format!("Inference waiting for admission: {reason:?}"))
            }
            Err(original) => reject_naming_original(*original, RejectReason::QueueFull, app),
        }
        return;
    }
    reject_naming_original(rejected.value, reason, app);
}

fn reject_naming_original(
    original: crate::naming_workers::NamingRequest,
    reason: ilium_execution::RejectReason,
    app: &mut App,
) {
    let provider = original.settings.settings.selected_provider;
    let message = format!("Inference did not start: {reason:?}");
    use crate::naming_workers::NamingKind;
    match original.kind {
        NamingKind::ProjectName(_) => app.is_project_name_loading = false,
        NamingKind::SessionTitle(request) => {
            app.titles_loading.remove(&request.input.pane_id);
        }
        NamingKind::TerminalTitle(input, _) => {
            app.titles_loading.remove(&input.pane_id);
        }
        NamingKind::InferenceTest => app.finish_inference_test(
            provider,
            Duration::ZERO,
            Err(anyhow::anyhow!(message.clone())),
        ),
        NamingKind::Models(provider) => app.finish_model_discovery(
            provider,
            String::new(),
            Duration::ZERO,
            Err(message.clone()),
        ),
        NamingKind::Restructure(request, _) => {
            app.fail_project_restructure(request.project_id, anyhow::anyhow!(message.clone()))
        }
        #[cfg(test)]
        NamingKind::LastPrompt(_) => {}
    }
    app.status_message = Some(message);
}
fn handle_exact_prompt_admission(
    result: Result<
        (),
        Box<ilium_execution::Rejected<crate::naming_workers::ExactAgentPromptTranscriptRequest>>,
    >,
    app: &mut App,
) {
    if let Err(refused) = result {
        if matches!(
            refused.reason,
            ilium_execution::RejectReason::Closed | ilium_execution::RejectReason::InvalidCost
        ) {
            app.status_message = Some(format!(
                "Exact transcript recovery did not start: {:?}",
                refused.reason
            ));
        } else {
            app.retain_exact_prompt_retry(refused.value);
        }
    }
}
fn dispatch_pending_smart_copy_work(app: &mut App, workers: &mut SmartCopyWorkers) {
    // Keep refused App requests intact until their owner permits another attempt.
    if app.take_smart_copy_cancel_requested() {
        workers.cancel();
    } // Revocation must not wait for the admission deadline.
    if !workers.begin_retry_turn(Instant::now()) {
        return;
    } // Unrelated input or release wakes never accelerate a refused original.
    let Some(request) = app.take_pending_smart_copy_request() else {
        return;
    }; // Move the exact captured request only when eligible.
    let Err(rejected) = workers.start(request) else {
        return;
    }; // Successful custody is now entirely in the worker owner.
    let reason = rejected.reason; // Classify coordinator admission, never provider response text.
    use ilium_execution::RejectReason; // Match the worker's actual refusal clock policy.
    if matches!(
        reason,
        RejectReason::Busy
            | RejectReason::QueueFull
            | RejectReason::JobLimit
            | RejectReason::InputBytes
            | RejectReason::ResultBytes
            | RejectReason::WorkerBytes
    ) {
        // Only bounded resource contention retries.
        app.retry_pending_smart_copy_request(rejected.value); // Preserve the original strings, settings, generation, and source guard.
        app.status_message = Some(format!("Smart Copy admission deferred: {reason:?}")); // Keep refusal visible without reconstructing the request.
        return; // The worker already armed the next admissible attempt time.
    } // Closed, invalid, or corrupted admission terminates this original.
    app.apply_smart_copy_worker_event(crate::smart_copy_workers::SmartCopyWorkerEvent {
        generation: rejected.value.generation,
        pane_id: rejected.value.pane_id,
        update: SmartCopyWorkerUpdate::Failed(format!("Smart Copy admission failed: {reason:?}")),
        retention: None,
    }); // Preserve the existing terminal UI disposition.
} // Streaming results never flow back through this pre-submission retry path.
fn dispatch_pending_app_work(
    app: &mut App,
    naming_workers: &mut NamingWorkers,
    icon_search_workers: &mut IconSearchWorkers,
    home_dir: Option<&std::path::Path>,
) {
    if let Some(request) = app.take_pending_icon_semantic_search() {
        if let Err(request) = icon_search_workers.try_request(request) {
            app.queue_icon_semantic_search(request);
        }
    }
    app.collect_exact_prompt_reports();
    for original in std::mem::take(&mut app.pending_exact_prompt_retries) {
        handle_exact_prompt_admission(
            naming_workers.spawn_exact_agent_prompt_transcript_worker(original),
            app,
        );
    }
    naming_workers.set_automatic_ai_decision(
        app.onboarding_revision,
        app.onboarding.is_none() && app.onboarding_progress.automatic_ai_allowed(),
    );
    naming_workers.collect(); // Publish revocation before collecting any due provider preflight retry.
    naming_workers.cancel_stale_exact_prompt_workers(|pane_id, session_id| {
        app.known_agent_history_context(pane_id)
            .is_some_and(|(_, known_id, _)| known_id == session_id)
    });
    for pane_id in app.take_pending_exact_prompt_worker_cancellations() {
        naming_workers.cancel_exact_prompt_worker(pane_id);
    }

    // The keypress captured a verified path and its byte length before
    // queuing Enter. Keep that immutable context even if the agent becomes
    // historical before this dispatch turn runs.
    if let Some(home_dir) = home_dir {
        for check in app.take_pending_last_prompt_transcript_checks() {
            handle_exact_prompt_admission(
                naming_workers.spawn_exact_agent_prompt_transcript_worker(
                    crate::naming_workers::ExactAgentPromptTranscriptRequest {
                        home: home_dir.to_path_buf(),
                        pane_id: check.pane_id,
                        project_path: check.project_path,
                        agent_class: check.agent_class,
                        session_id: check.session_id,
                        verified_path: check.verified_path,
                        baseline_length: check.baseline_length,
                        submitted_after: check.submitted_after,
                        prompt_epoch: check.prompt_epoch,
                    },
                ),
                app,
            );
        }
    } else {
        app.take_pending_last_prompt_transcript_checks();
    }

    if naming_workers.begin_retry_turn(Instant::now()) {
        // Check the absolute deadline before moving App's captured originals.
        for original in std::mem::take(&mut app.pending_naming_retries) {
            // Never recapture current settings or decision for a refused request.
            handle_naming_admission(naming_workers.retry_original(original), app);
            // Fresh snapshot allocation cannot strand an older admitted snapshot.
        } // Actual refusals arm the next turn while preserving the same request allocations.
    } // Every unrelated event leaves a not-yet-due original exactly where it was.
    if let Err(reason) = naming_workers.set_inference_settings(&app.inference_settings) {
        // Only fresh provider work needs the latest settings snapshot.
        let state = if matches!(
            reason,
            ilium_execution::RejectReason::Busy | ilium_execution::RejectReason::WorkerBytes
        ) {
            "waiting for admission"
        } else {
            "admission failed"
        }; // Only transient storage refusal schedules another attempt.
        app.status_message = Some(format!("Inference settings {state}: {reason:?}")); // Existing originals and exact transcript work already advanced independently.
        return; // Preserve fresh App outboxes for a timed retry or an independent settings change.
    } // Every admitted original retains its original immutable settings Arc.
      // Setup may have suppressed the startup inference. Admit one fresh attempt
      // for this decision after Finish; an error must not retry every frame.
    if app.onboarding.is_none()
        && app.onboarding_progress.automatic_ai_allowed()
        && app.project_name.is_none()
        && !app.is_project_name_loading
        && app.project_name_attempt_revision != Some(app.onboarding_revision)
    {
        app.project_name_attempt_revision = Some(app.onboarding_revision);
        app.is_project_name_loading = true;
        handle_naming_admission(
            naming_workers.spawn_project_name_worker(app.session_cwd.clone()),
            app,
        );
    }

    if app.take_pending_inference_test() {
        handle_naming_admission(naming_workers.spawn_inference_test_worker(), app);
    }
    if let Some(provider) = app.take_pending_model_refresh() {
        handle_naming_admission(naming_workers.spawn_model_discovery_worker(provider), app);
    }

    for request in app.take_pending_retitle_requests() {
        if app.onboarding.is_some() || !app.onboarding_progress.automatic_ai_allowed() {
            let pane_id = match &request {
                crate::app::PendingRetitleRequest::Session { input, .. } => input.pane_id,
                crate::app::PendingRetitleRequest::Terminal { input, .. } => input.pane_id,
            };
            app.titles_loading.remove(&pane_id);
            continue;
        }
        match request {
            crate::app::PendingRetitleRequest::Session {
                input,
                title_generation,
                trigger,
            } => match home_dir {
                Some(home_dir) => handle_naming_admission(
                    naming_workers.spawn_session_title_worker(
                        crate::naming_workers::SessionTitleWorkerRequest {
                            home: home_dir.to_path_buf(),
                            input,
                            title_generation,
                            trigger,
                        },
                    ),
                    app,
                ),
                None => {
                    app.record_agent_debug_event(
                        input.pane_id,
                        ilium_ipc::AgentDebugEventDraft {
                            severity: ilium_ipc::AgentDebugSeverity::Error,
                            kind: ilium_ipc::AgentDebugEventKind::TitleInferenceFailed,
                            summary: "Agent title inference could not start".to_string(),
                            fields: vec![ilium_ipc::AgentDebugField::plain(
                                "reason",
                                "home directory unavailable",
                            )],
                            correlation_id: None,
                            metadata: Default::default(),
                        },
                    );
                    app.titles_loading.remove(&input.pane_id);
                    app.status_message =
                        Some("Could not infer title: home directory unavailable".to_string());
                }
            },
            crate::app::PendingRetitleRequest::Terminal { input, trigger } => {
                handle_naming_admission(
                    naming_workers.spawn_terminal_title_worker(input, trigger),
                    app,
                );
            }
        }
    }

    for request in app.take_pending_restructure_requests() {
        if app.onboarding.is_some() || !app.onboarding_progress.automatic_ai_allowed() {
            app.cancel_project_restructure_after_ai_decision(request.project_id);
            continue;
        }
        match home_dir {
            Some(home_dir) => handle_naming_admission(
                naming_workers.spawn_restructure_worker(request, home_dir.to_path_buf()),
                app,
            ),
            None => {
                app.fail_project_restructure(
                    request.project_id,
                    anyhow::anyhow!("home directory unavailable"),
                );
            }
        }
    }
}

#[cfg(test)]
mod responsiveness_tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn merged_screen_updates_respect_the_byte_ceiling() {
        assert!(screen_updates_fit(MAX_MERGED_SCREEN_BYTES_PER_BATCH - 1, 1));
        assert!(!screen_updates_fit(MAX_MERGED_SCREEN_BYTES_PER_BATCH, 1));
        assert!(!screen_updates_fit(usize::MAX, usize::MAX));
    }

    #[test]
    fn blocked_semantic_paste_keeps_later_key_in_native_fifo() {
        let (tx, mut rx) = mpsc::channel(2);
        tx.try_send(Event::Key(KeyEvent::new(
            KeyCode::Char('x'),
            KeyModifiers::NONE,
        )))
        .unwrap();
        let mut dispatched = Vec::new();
        let deferred = for_each_ready_input_event_until(
            &mut rx,
            Event::Paste("whole\noriginal".into()),
            |event| {
                dispatched.push(event);
                false
            },
        );
        assert!(deferred.is_none());
        assert_eq!(dispatched, vec![Event::Paste("whole\noriginal".into())]);
        assert!(matches!(
            rx.try_recv(),
            Ok(Event::Key(KeyEvent {
                code: KeyCode::Char('x'),
                ..
            }))
        ));
    }

    #[test]
    fn stopped_motion_dispatch_returns_original_paste_lookahead() {
        let (tx, mut rx) = mpsc::channel(2);
        tx.try_send(Event::Paste("original lookahead".into()))
            .unwrap();
        tx.try_send(Event::Key(KeyEvent::new(
            KeyCode::Char('x'),
            KeyModifiers::NONE,
        )))
        .unwrap();
        let motion = Event::Mouse(crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Moved,
            column: 1,
            row: 1,
            modifiers: KeyModifiers::NONE,
        });
        let deferred = for_each_ready_input_event_until(&mut rx, motion, |_| false);
        assert_eq!(deferred, Some(Event::Paste("original lookahead".into())));
        assert!(matches!(
            rx.try_recv(),
            Ok(Event::Key(KeyEvent {
                code: KeyCode::Char('x'),
                ..
            }))
        ));
    }

    #[test]
    fn input_after_the_batch_boundary_remains_queued() {
        let (input_tx, mut input_rx) = mpsc::channel(MAX_INPUT_EVENTS_PER_BATCH + 1);
        let first = Event::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        for input_index in 0..MAX_INPUT_EVENTS_PER_BATCH {
            let character = if input_index + 1 == MAX_INPUT_EVENTS_PER_BATCH {
                '!'
            } else {
                'b'
            };
            input_tx
                .try_send(Event::Key(KeyEvent::new(
                    KeyCode::Char(character),
                    KeyModifiers::NONE,
                )))
                .expect("test input channel has exact capacity");
        }
        let mut dispatched = Vec::new();

        for_each_ready_input_event(&mut input_rx, first, |event| dispatched.push(event));

        assert_eq!(dispatched.len(), MAX_INPUT_EVENTS_PER_BATCH);
        assert!(matches!(
            input_rx.try_recv(),
            Ok(Event::Key(KeyEvent {
                code: KeyCode::Char('!'),
                ..
            }))
        ));
        assert!(input_rx.try_recv().is_err());
    }

    #[test]
    fn motion_coalescing_never_consumes_a_paste_past_the_sixty_fourth_original() {
        use crossterm::event::{MouseEvent, MouseEventKind};

        let motion = |column| {
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Moved,
                column,
                row: 1,
                modifiers: KeyModifiers::NONE,
            })
        };
        let (input_tx, mut input_rx) = mpsc::channel(MAX_INPUT_EVENTS_PER_BATCH + 1);
        for column in 1..MAX_INPUT_EVENTS_PER_BATCH - 1 {
            input_tx
                .try_send(motion(column as u16))
                .expect("motion queued");
        }
        input_tx
            .try_send(Event::Paste("original-64".into()))
            .expect("boundary paste");
        input_tx
            .try_send(Event::Paste("original-65".into()))
            .expect("deferred paste");
        let mut dispatched = Vec::new();

        for_each_ready_input_event(&mut input_rx, motion(0), |event| dispatched.push(event));

        assert_eq!(dispatched.len(), 2, "only motion may be coalesced");
        assert!(matches!(&dispatched[0], Event::Mouse(event)
            if event.kind == MouseEventKind::Moved && event.column == (MAX_INPUT_EVENTS_PER_BATCH - 2) as u16));
        assert!(matches!(&dispatched[1], Event::Paste(text) if text == "original-64"));
        assert!(matches!(input_rx.try_recv(), Ok(Event::Paste(text)) if text == "original-65"));
    }

    #[test]
    fn output_redraws_are_limited_but_input_redraws_are_immediate() {
        let last_draw_at = Instant::now();
        let early = last_draw_at + Duration::from_millis(5);
        assert_eq!(
            output_redraw_delay(early, last_draw_at),
            Duration::from_millis(28)
        );
        assert!(!output_redraw_is_due(false, early, last_draw_at));
        assert!(output_redraw_is_due(true, early, last_draw_at));
        assert!(output_redraw_is_due(
            false,
            last_draw_at + OUTPUT_FRAME_INTERVAL,
            last_draw_at,
        ));
    }

    #[test]
    fn hidden_screen_updates_advance_the_cache_without_damaging_the_frame() {
        let project_directory = tempfile::tempdir().unwrap();
        let mut app = App::new(
            "hidden-output-test".to_owned(),
            project_directory.path().to_path_buf(),
        );
        let group_id = app.tree.add_group(ilium_core::ROOT_ID, "work").unwrap();
        let visible_pane_id = app
            .tree
            .add_pane(group_id, "visible", ilium_core::PaneContentKind::Terminal)
            .unwrap();
        let hidden_pane_id = app
            .tree
            .add_pane(group_id, "hidden", ilium_core::PaneContentKind::Terminal)
            .unwrap();
        app.panes.insert(
            visible_pane_id,
            crate::app::PaneRuntime::Terminal(Box::new(crate::terminal_view::TerminalView::new(
                24, 80,
            ))),
        );
        app.panes.insert(
            hidden_pane_id,
            crate::app::PaneRuntime::Terminal(Box::new(crate::terminal_view::TerminalView::new(
                24, 80,
            ))),
        );
        app.set_screen_area(Rect::new(0, 0, 120, 40));
        app.focus_pane(visible_pane_id);
        let mut hidden_update = Some((hidden_pane_id, 1, 1, b"hidden output".to_vec()));

        assert!(!flush_pending_screen_update(&mut app, &mut hidden_update));
        let crate::app::PaneRuntime::Terminal(hidden_view) = &app.panes[&hidden_pane_id] else {
            panic!("hidden pane must remain a terminal");
        };
        assert!(hidden_view.with_screen(|screen| screen.contents().contains("hidden output")));

        let mut visible_update = Some((visible_pane_id, 1, 1, b"visible output".to_vec()));
        assert!(flush_pending_screen_update(&mut app, &mut visible_update));
    }

    #[test]
    fn repeated_activity_and_hidden_debug_events_do_not_damage_frames() {
        let project_directory = tempfile::tempdir().unwrap();
        let mut app = App::new(
            "event-damage-test".to_owned(),
            project_directory.path().to_path_buf(),
        );
        let group_id = app.tree.add_group(ilium_core::ROOT_ID, "work").unwrap();
        let visible_pane_id = app
            .tree
            .add_pane(group_id, "visible", ilium_core::PaneContentKind::Terminal)
            .unwrap();
        let hidden_pane_id = app
            .tree
            .add_pane(group_id, "hidden", ilium_core::PaneContentKind::Terminal)
            .unwrap();
        app.tree.mark_node_focused(hidden_pane_id).unwrap();
        app.focus_pane(visible_pane_id);

        let first_activity = ilium_ipc::ServerEvent::NodeActivityChanged {
            node_id: hidden_pane_id,
            activity_revision: 1,
        };
        assert!(server_event_damage(&app, &first_activity).needs_redraw);
        app.apply_node_activity(hidden_pane_id, 1);

        let repeated_activity = ilium_ipc::ServerEvent::NodeActivityChanged {
            node_id: hidden_pane_id,
            activity_revision: 2,
        };
        assert!(!server_event_damage(&app, &repeated_activity).needs_redraw);

        let hidden_debug_snapshot = ilium_ipc::ServerEvent::PaneDebugLogSnapshot {
            pane_id: hidden_pane_id,
            through_sequence: 0,
            retained_from_sequence: 1,
            dropped_entry_count: 0,
            entries: Vec::new(),
        };
        assert!(!server_event_damage(&app, &hidden_debug_snapshot).needs_redraw);
    }

    #[test]
    fn user_visible_failures_are_promoted_without_misclassifying_progress() {
        assert!(status_message_is_failure(
            "Could not save inference settings: permission denied"
        ));
        assert!(status_message_is_failure("Save failed: disk full"));
        assert!(status_message_is_failure(
            "Board path picker lost its parent dialog"
        ));
        assert!(!status_message_is_failure("Testing inference provider…"));
        assert!(!status_message_is_failure("Card saved"));
    }

    #[test]
    fn client_surface_key_distinguishes_settings_tabs_without_owned_text() {
        let project_directory = tempfile::tempdir().unwrap();
        let mut app = App::new(
            "surface-key-test".to_owned(),
            project_directory.path().to_path_buf(),
        );

        assert_eq!(
            client_surface_key(&app),
            ClientSurfaceKey {
                mode_label: "normal",
                settings_tab_label: None,
            }
        );

        app.action_open_settings();
        let crate::app::Mode::Settings(settings) = &mut app.mode else {
            panic!("settings action must open settings mode");
        };
        settings.tab = crate::app::SettingsTab::Debug;

        assert_eq!(
            client_surface_key(&app),
            ClientSurfaceKey {
                mode_label: "settings",
                settings_tab_label: Some("Debug"),
            }
        );
    }

    #[test]
    fn provider_failure_survives_the_following_voice_channel_close() {
        let provider_failure = ilium_voice::VoiceConnectionState::Failed(
            "OpenAI rejected one production tool schema".to_owned(),
        );

        assert!(!should_report_unexpected_voice_stop(
            true,
            &provider_failure
        ));
        assert!(should_report_unexpected_voice_stop(
            true,
            &ilium_voice::VoiceConnectionState::Listening
        ));
        assert!(!should_report_unexpected_voice_stop(
            false,
            &ilium_voice::VoiceConnectionState::Listening
        ));
    }

    #[test]
    fn terminating_tool_output_dominates_a_parallel_batch() {
        let outputs = vec![
            ilium_voice::VoiceToolOutput {
                call_id: "ordinary".to_owned(),
                result: std::sync::Arc::new(serde_json::json!({ "status": "ok" })),
                request_follow_up: true,
                terminate_session_after_delivery: false,
                allocation_hold: None,
                retained_bytes: 0,
            },
            ilium_voice::VoiceToolOutput {
                call_id: "stop".to_owned(),
                result: std::sync::Arc::new(serde_json::json!({ "status": "ok" })),
                request_follow_up: false,
                terminate_session_after_delivery: true,
                allocation_hold: None,
                retained_bytes: 0,
            },
        ];

        assert!(voice_tool_outputs_request_shutdown(&outputs));
        assert!(!voice_tool_outputs_request_shutdown(&outputs[..1]));
    }

    #[test]
    fn realtime_context_update_is_emitted_once_per_agent_boundary_change() {
        let mut app = App::new(
            "default".to_owned(),
            std::path::PathBuf::from("/tmp/project"),
        );
        let group_id = app.tree.add_group(ilium_core::ROOT_ID, "work").unwrap();
        let pane_id = app
            .tree
            .add_pane(group_id, "agent", ilium_core::PaneContentKind::Terminal)
            .unwrap();
        app.focus_pane(pane_id);
        let control_plane = crate::control::ControlPlane::default();

        assert!(pending_voice_target_context_update(
            &app,
            &control_plane,
            Some(crate::control::VoiceTargetContext::NoDetectedAgent),
        )
        .is_none());

        app.tree
            .set_pane_status(
                pane_id,
                ilium_core::PaneStatus::from_activity(
                    ilium_core::AgentClass::Codex,
                    ilium_core::AgentActivity::Idle,
                    None,
                ),
            )
            .unwrap();
        let (context, command) = pending_voice_target_context_update(
            &app,
            &control_plane,
            Some(crate::control::VoiceTargetContext::NoDetectedAgent),
        )
        .expect("agent detection should update Realtime context");

        assert_eq!(context, crate::control::VoiceTargetContext::DetectedAgent);
        match command {
            ilium_voice::VoiceCommand::UpdateContext(context) => {
                let instructions = context.instructions();
                let tools = context.tools();
                assert!(instructions.contains("agent-default dictation rule is active"));
                assert!(instructions.contains("user's complete utterance as `text`"));
                assert!(tools
                    .iter()
                    .any(|definition| definition.name == "ilium_send_to_terminal"));
            }
            _ => panic!("agent detection should produce a context update"),
        }
        assert!(pending_voice_target_context_update(&app, &control_plane, Some(context)).is_none());
    }

    #[tokio::test]
    async fn stop_request_reconciles_to_disabled_without_a_running_actor() {
        let mut app = App::new(
            "default".to_owned(),
            std::path::PathBuf::from("/tmp/project"),
        );
        app.voice_settings.enabled = true;
        app.update_voice_connection_state(ilium_voice::VoiceConnectionState::Listening);
        app.stop_voice_control();
        let control_plane = crate::control::ControlPlane::default();
        let mut voice_service = None;
        let mut normal_media = crate::media_control::MediaLease::inactive_fixture();

        reconcile_voice_runtime(
            &mut app,
            &control_plane,
            &mut voice_service,
            &mut normal_media,
        )
        .await;

        assert!(!app.voice_settings.enabled);
        assert_eq!(
            app.voice_connection_state,
            ilium_voice::VoiceConnectionState::Disabled
        );
        assert!(app.take_voice_runtime_request().is_none());
    }

    fn voice_text_app() -> App {
        App::new(
            "voice-text".to_owned(),
            std::path::PathBuf::from("/tmp/project"),
        )
    }

    fn offer(sentences: &[&str], started_voice: bool) -> crate::app::VoiceTextOffer {
        crate::app::VoiceTextOffer {
            request_id: 77,
            sentences: sentences
                .iter()
                .map(|sentence| (*sentence).to_owned())
                .collect(),
            started_voice,
            _retention: None,
        }
    }

    #[test]
    fn refused_complete_state_updates_and_registration_rearm_until_credit_releases() {
        let mut app = voice_text_app();
        let mut limits = crate::ipc_preparation::request_limits();
        limits.jobs = 1;
        app.outbound_admission = Some(
            app.outbound_admission
                .as_ref()
                .unwrap()
                .child(limits)
                .unwrap(),
        );
        assert!(app.queue_request(ilium_ipc::ClientRequest::UpdateDebugLogging { enabled: false }));
        app.ui_settings.agent_debug_menu_enabled = true;
        app.request_agent_debug_menu_reconciliation();
        app.ui_settings.progress_monitor_enabled = false;
        app.request_progress_monitor_reconciliation();
        app.pending_debug_logging_server_enabled = Some(true);
        app.pending_voice_receiver_registration = true;
        reconcile_agent_debug_menu(&mut app);
        reconcile_progress_monitor_enabled(&mut app);
        reconcile_debug_logging_server(&mut app);
        reconcile_voice_receiver_registration(&mut app);
        assert!(app.pending_voice_receiver_registration);
        assert_eq!(app.pending_debug_logging_server_enabled, Some(true));
        assert_eq!(app.take_outbound_requests().len(), 1);
        reconcile_agent_debug_menu(&mut app);
        assert_eq!(
            app.take_outbound_requests(),
            vec![ilium_ipc::ClientRequest::UpdateAgentDebugMenu { enabled: true }]
        );
        reconcile_progress_monitor_enabled(&mut app);
        assert_eq!(
            app.take_outbound_requests(),
            vec![ilium_ipc::ClientRequest::UpdateProgressMonitorEnabled { enabled: false }]
        );
        reconcile_debug_logging_server(&mut app);
        assert_eq!(
            app.take_outbound_requests(),
            vec![ilium_ipc::ClientRequest::UpdateDebugLogging { enabled: true }]
        );
        reconcile_voice_receiver_registration(&mut app);
        assert_eq!(
            app.take_outbound_requests(),
            vec![ilium_ipc::ClientRequest::RegisterVoiceTextReceiver]
        );
        assert!(!app.pending_voice_receiver_registration);
        reconcile_voice_receiver_registration(&mut app);
        assert!(app.take_outbound_requests().is_empty());
    }

    #[test]
    fn voice_actor_acceptance_waits_for_reply_credit_and_all_original_sentence_slots() {
        let mut app = voice_text_app();
        app.voice_settings.enabled = true;
        app.voice_connection_state = ilium_voice::VoiceConnectionState::Listening;
        let mut limits = crate::ipc_preparation::request_limits();
        limits.jobs = 1;
        app.outbound_admission = Some(
            app.outbound_admission
                .as_ref()
                .unwrap()
                .child(limits)
                .unwrap(),
        );
        assert!(app.queue_request(ilium_ipc::ClientRequest::UpdateDebugLogging { enabled: false }));
        let first = offer(&["original first", "original second"], false);
        let pointer = first.sentences[0].as_ptr();
        app.restore_voice_text_offers(vec![first]);
        let (commands, mut received) = mpsc::channel(2);
        deliver_voice_text_offers_to(&mut app, Some(&commands));
        assert!(received.try_recv().is_err());
        assert_eq!(app.take_outbound_requests().len(), 1);
        commands
            .try_send(ilium_voice::VoiceCommand::StartPushToTalk)
            .unwrap();
        deliver_voice_text_offers_to(&mut app, Some(&commands));
        assert!(app.take_outbound_requests().is_empty());
        assert!(matches!(
            received.try_recv(),
            Ok(ilium_voice::VoiceCommand::StartPushToTalk)
        ));
        deliver_voice_text_offers_to(&mut app, Some(&commands));
        let ilium_voice::VoiceCommand::SendText(first) = received.try_recv().unwrap() else {
            panic!("first original text");
        };
        assert_eq!(first.as_str().as_ptr(), pointer);
        assert_eq!(first.as_str(), "original first");
        let ilium_voice::VoiceCommand::SendText(second) = received.try_recv().unwrap() else {
            panic!("second original text");
        };
        assert_eq!(second.as_str(), "original second");
        assert!(
            matches!(app.take_outbound_requests().as_slice(), [ilium_ipc::ClientRequest::AnswerVoiceText { result: Ok(accepted), .. }] if accepted.sentence_count == 2)
        );
        deliver_voice_text_offers_to(&mut app, Some(&commands));
        assert!(received.try_recv().is_err());
        assert!(app.take_outbound_requests().is_empty());
    }

    #[tokio::test]
    async fn typed_sentences_reach_a_running_session_in_order_as_send_text_commands() {
        let mut app = voice_text_app();
        app.voice_settings.enabled = true;
        app.update_voice_connection_state(ilium_voice::VoiceConnectionState::Listening);
        let (commands, mut received) = mpsc::channel(8);

        let result = voice_text_offer_result(
            &mut app,
            Some(&commands),
            &mut offer(&["open the settings", "close it"], false),
        )
        .expect("no temporary pressure");

        assert_eq!(
            result,
            Ok(ilium_ipc::VoiceTextAccepted {
                sentence_count: 2,
                phase: ilium_ipc::VoiceTextPhase::Listening,
                started_voice: false,
            })
        );
        for expected in ["open the settings", "close it"] {
            match received.try_recv() {
                Ok(ilium_voice::VoiceCommand::SendText(text)) => {
                    assert_eq!(text.as_str(), expected)
                }
                other => panic!("expected SendText({expected:?}), got {other:?}"),
            }
        }
        assert!(received.try_recv().is_err());
    }

    #[tokio::test]
    async fn typed_sentences_are_refused_with_the_reason_when_no_session_can_take_them() {
        use ilium_ipc::VoiceTextRejectionCode::{VoiceOff, VoiceUnavailable};

        // Voice switched off.
        let mut app = voice_text_app();
        let refusal = voice_text_offer_result(&mut app, None, &mut offer(&["hi"], false))
            .expect("no temporary pressure")
            .unwrap_err();
        assert_eq!(refusal.code, VoiceOff);
        assert!(refusal.message.contains("--start"));

        // Enabled, but the session failed to start: the failure is the reason.
        let mut app = voice_text_app();
        app.voice_settings.enabled = true;
        app.update_voice_connection_state(ilium_voice::VoiceConnectionState::Failed(
            "OpenAI API key must not be empty".to_owned(),
        ));
        let refusal = voice_text_offer_result(&mut app, None, &mut offer(&["hi"], false))
            .expect("no temporary pressure")
            .unwrap_err();
        assert_eq!(refusal.code, VoiceUnavailable);
        assert!(refusal.message.contains("API key"));

        // A session handle exists but its actor already ended.
        let (commands, received) = mpsc::channel(1);
        drop(received);
        app.update_voice_connection_state(ilium_voice::VoiceConnectionState::Listening);
        let refusal =
            voice_text_offer_result(&mut app, Some(&commands), &mut offer(&["hi"], false))
                .expect("no temporary pressure")
                .unwrap_err();
        assert_eq!(refusal.code, VoiceUnavailable);
    }

    #[test]
    fn a_start_request_switches_voice_on_only_when_it_is_off_or_failed() {
        let mut app = voice_text_app();
        app.receive_voice_text_offer(1, vec!["hi".to_owned()], false);
        assert!(
            !app.voice_settings.enabled,
            "no --start, nothing is switched on"
        );
        assert!(app.take_voice_runtime_request().is_none());

        app.receive_voice_text_offer(2, vec!["hi".to_owned()], true);
        assert!(app.voice_settings.enabled);
        assert_eq!(
            app.take_voice_runtime_request(),
            Some(crate::app::VoiceRuntimeRequest::Start)
        );

        // Already running: --start changes nothing.
        app.update_voice_connection_state(ilium_voice::VoiceConnectionState::Listening);
        app.receive_voice_text_offer(3, vec!["hi".to_owned()], true);
        assert!(app.take_voice_runtime_request().is_none());

        // Enabled but failed: --start asks for a restart.
        app.update_voice_connection_state(ilium_voice::VoiceConnectionState::Failed(
            "boom".to_owned(),
        ));
        app.receive_voice_text_offer(4, vec!["hi".to_owned()], true);
        assert_eq!(
            app.take_voice_runtime_request(),
            Some(crate::app::VoiceRuntimeRequest::Reconfigure)
        );

        let offers = app.take_voice_text_offers();
        assert_eq!(
            offers
                .iter()
                .map(|offer| (offer.request_id, offer.started_voice))
                .collect::<Vec<_>>(),
            [(1, false), (2, true), (3, false), (4, true)]
        );
    }

    #[test]
    fn a_server_offer_becomes_a_pending_offer_for_the_voice_owner() {
        let mut app = voice_text_app();
        let event = ilium_ipc::ServerEvent::VoiceTextOffered {
            request_id: 5,
            sentences: vec!["open the settings".to_owned()],
            start_voice: false,
        };
        assert!(crate::render_cache::apply(&mut app, event).is_none());
        assert_eq!(
            app.take_voice_text_offers(),
            [crate::app::VoiceTextOffer {
                request_id: 5,
                sentences: vec!["open the settings".to_owned()],
                started_voice: false,
                _retention: None,
            }]
        );
    }

    #[test]
    fn a_typed_turn_leaves_the_same_trace_as_a_recognised_utterance() {
        let mut app = voice_text_app();
        app.record_typed_voice_text(&["first".to_owned(), "second".to_owned()]);
        assert_eq!(app.voice_last_user_transcript.as_deref(), Some("second"));
        assert_eq!(app.status_message.as_deref(), Some("Voice (typed): second"));
    }
}
