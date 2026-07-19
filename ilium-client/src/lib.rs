//! ilium-client: the `ratatui` TUI, a thin renderer/input-dispatcher over
//! `ilium-ipc` -- see `app.rs`'s module docs for the render-cache
//! architecture this crate is built around, and the workspace root
//! `CLAUDE.md` / README "Architecture" for how this fits the
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

pub mod agent_from_line;
pub mod app;
pub mod background_priority;
pub mod board;
pub mod board_ui;
pub mod config;
pub mod connection;
pub mod control;
pub mod editor_chrome;
pub mod editor_highlight;
pub mod editor_pane;
pub mod editor_toolbar;
pub mod error;
pub mod explorer_overlay;
pub mod help;
pub mod icon_search_workers;
pub mod icon_settings;
pub mod inference_test;
pub mod keymap;
pub mod keys;
pub mod layout;
pub mod markdown;
pub mod media_control;
pub mod minimap;
pub mod modal;
pub mod mouse;
pub mod naming;
pub mod naming_workers;
pub mod outbound_requests;
pub mod paths;
pub mod project_config;
pub mod project_naming;
pub mod prompt_queue;
pub mod render_cache;
pub mod restructure;
pub mod scheduled_input;
pub mod search_ui;
pub mod search_workers;
pub mod session_naming;
pub mod settings_ui;
pub mod split_layout;
pub mod syntax;
pub mod terminal_guard;
pub mod terminal_links;
pub mod terminal_naming;
pub mod terminal_title_inference;
pub mod terminal_view;
pub mod text_prompt;
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
pub mod voice_settings;
pub mod workspace_file;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::event::Event;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::Terminal;
use tokio::sync::mpsc;

pub use crate::app::ClientExitReason;

use crate::app::App;
use crate::connection::Connection;
use crate::error::ClientError;
use crate::icon_search_workers::IconSearchWorkers;
use crate::naming_workers::NamingWorkers;
use crate::search_workers::SearchWorkers;
use crate::terminal_guard::TerminalGuard;
use crate::trigger_execution_lease::TriggerExecutionLease;

/// Everything [`run`] needs to attach to one already-running
/// `ilium-server` session and start rendering it.
pub struct RunOptions {
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

/// Bounded capacity for the crossterm-input and naming-worker-result
/// channels the event loop selects on. A generous "few hundred" headroom
/// for interactive latency (a burst of pasted text, a flurry of mouse-move
/// events) while still giving the producer real backpressure instead of an
/// unbounded backlog the loop would have to fully drain -- stale input and
/// all -- before ever catching up, if it's ever starved by OS scheduling
/// under load. See `crate::connection` for the matching bound on the
/// server event/request channels.
const INPUT_CHANNEL_CAPACITY: usize = 256;
/// Naming-worker results are one-shot per worker (see `naming_workers.rs`),
/// so this only needs enough headroom to never be the bottleneck; it's
/// bounded at all purely for consistency with every other channel in this
/// crate, not because it could plausibly fill up.
const NAMING_EVENTS_CHANNEL_CAPACITY: usize = 16;
/// Semantic icon queries are revisioned, so a tiny bounded channel is enough:
/// the worker drains superseded typing before it runs the next CPU inference.
const ICON_SEARCH_EVENTS_CHANNEL_CAPACITY: usize = 4;
/// Upper bound for server events applied in one select turn. Terminal output
/// can arrive continuously; yielding after a bounded batch gives pointer and
/// keyboard events a reliable chance to run instead of making hover depend
/// on how chatty the displayed PTY happens to be.
const MAX_SERVER_EVENTS_PER_BATCH: usize = 16;
/// Byte ceiling for a same-pane raw-output run merged in one client turn.
/// Event count alone is insufficient because a server frame can itself carry
/// a large burst.
const MAX_MERGED_SCREEN_BYTES_PER_BATCH: usize = 64 * 1024;
/// Input is intentionally favoured over render-cache updates, but it remains
/// bounded so a key-repeat or pointer flood cannot starve incoming terminal
/// frames forever.
const MAX_INPUT_EVENTS_PER_BATCH: usize = 64;
/// Terminal output is visual state, not input acknowledgement. Capping its
/// redraw cadence at 60 Hz leaves CPU for parsing and input while every
/// keyboard/pointer-driven change still bypasses this limit immediately.
const OUTPUT_FRAME_INTERVAL: Duration = Duration::from_millis(16);

/// Remaining delay before output-only damage may trigger another draw.
fn output_redraw_delay(now: Instant, last_draw_at: Instant) -> Duration {
    OUTPUT_FRAME_INTERVAL.saturating_sub(now.saturating_duration_since(last_draw_at))
}

/// Input and semantic state changes bypass output-only frame limiting.
fn output_redraw_is_due(needs_immediate_redraw: bool, now: Instant, last_draw_at: Instant) -> bool {
    needs_immediate_redraw || output_redraw_delay(now, last_draw_at).is_zero()
}

/// Event count and byte count are independent fairness limits.
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
fn record_client_surface_change(app: &App, last_recorded_surface: &mut Option<String>) {
    let surface = match &app.mode {
        crate::app::Mode::Settings(settings) => format!(
            "{}:{}",
            crate::control::mode_label(&app.mode),
            settings.tab.label()
        ),
        _ => crate::control::mode_label(&app.mode).to_owned(),
    };
    if last_recorded_surface.as_deref() == Some(surface.as_str()) {
        return;
    }
    tracing::info!(client_surface = %surface, "client surface changed");
    *last_recorded_surface = Some(surface);
}

/// Runs ilium-client until the user quits, requests a client-only restart, or
/// loses the server connection. The typed result lets the CLI wrapper re-exec
/// only for the explicit restart path after this function restores the terminal.
pub async fn run(options: RunOptions) -> Result<ClientExitReason, ClientError> {
    if !options.session_cwd.is_dir() {
        return Err(ClientError::InvalidSessionCwd(options.session_cwd));
    }

    // Resolved and installed once, before the terminal enters raw/
    // alternate-screen mode and before any render call -- see
    // `theme::THEME`'s doc comment on why a one-time `OnceLock` init is only
    // safe this early. `config_dir` is threaded through to `run_inner` too:
    // the settings screen (`crate::app::Mode::Settings`) needs it later to
    // persist a change (`crate::config::save_ui_settings`).
    let config_dir_result = crate::paths::config_dir();
    let file_logging_enabled_hint = config_dir_result
        .as_ref()
        .ok()
        .and_then(|config_dir| {
            ilium_logging::file_logging_enabled_hint(&config_dir.join("config.toml"))
                .ok()
                .flatten()
        })
        .unwrap_or(false);
    ilium_logging::initialize(&options.log_path, file_logging_enabled_hint, "client")?;
    ilium_logging::install_panic_logging();
    let (config, config_dir) = init_config(config_dir_result, file_logging_enabled_hint);
    ilium_logging::set_enabled(config.debug.file_logging_enabled)?;
    tracing::info!(
        session_name = options.session_name,
        session_cwd = %options.session_cwd.display(),
        log_path = %options.log_path.display(),
        "ilium-client starting"
    );
    let sound_discovery = ilium_sound::discover_system_sounds();
    let voice_input_devices = ilium_voice::available_input_devices().unwrap_or_else(|error| {
        tracing::warn!(%error, "failed to enumerate voice input devices");
        Vec::new()
    });
    let voice_output_devices = ilium_voice::available_output_devices().unwrap_or_else(|error| {
        tracing::warn!(%error, "failed to enumerate voice output devices");
        Vec::new()
    });

    // `TerminalGuard::drop` restores the terminal whether this function
    // returns `Ok`, `Err`, or panics and unwinds through this stack frame.
    let guard = TerminalGuard::enter().map_err(|error| {
        tracing::error!(%error, error_debug = ?error, "failed to enter terminal UI mode");
        error
    })?;
    let result = run_inner(
        &options,
        config,
        config_dir,
        sound_discovery,
        voice_input_devices,
        voice_output_devices,
    )
    .await;
    drop(guard);
    if let Err(error) = &result {
        tracing::error!(%error, error_debug = ?error, "ilium-client exited with an error");
    } else {
        tracing::info!("ilium-client stopped");
    }
    result
}

/// Resolves `config.toml`'s client-side tables and installs the effective
/// keybinding table / theme for the rest of the process's lifetime, also
/// returning the full resolved config (for its `[ui]` table -- see
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
) -> (crate::config::ClientConfig, Option<PathBuf>) {
    let config_dir = match config_dir_result {
        Ok(dir) => Some(dir),
        Err(error) => {
            tracing::warn!("failed to resolve config directory, using defaults: {error}");
            None
        }
    };
    let config = match &config_dir {
        Some(dir) => match crate::config::load(dir) {
            Ok(config) => config,
            Err(error) => {
                tracing::warn!("failed to load config, using defaults: {error}");
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
    crate::keymap::init_effective_bindings(config.keybindings.clone());
    crate::theme::init(config.theme);
    (config, config_dir)
}

async fn run_inner(
    options: &RunOptions,
    config: crate::config::ClientConfig,
    config_dir: Option<PathBuf>,
    sound_discovery: ilium_sound::SoundDiscovery,
    voice_input_devices: Vec<String>,
    voice_output_devices: Vec<String>,
) -> Result<ClientExitReason, ClientError> {
    let backend = CrosstermBackend::new(std::io::stdout());
    let mut terminal = Terminal::new(backend).map_err(ClientError::TerminalSetup)?;

    let mut app = App::new(options.session_name.clone(), options.session_cwd.clone());
    app.apply_ui_settings(config.ui);
    app.keyboard_settings = config.keyboard;
    app.keybindings = config.keybindings;
    app.kanban_board_settings = config.kanban_board;
    app.apply_sound_settings(config.sound);
    app.apply_inference_settings(config.inference);
    app.apply_trigger_settings(config.triggers);
    app.apply_terminal_settings(config.terminal);
    app.apply_editor_settings(config.editor);
    app.apply_session_settings(config.session);
    app.apply_voice_settings(config.voice);
    app.apply_debug_settings(config.debug);
    app.request_debug_logging_reconciliation();
    app.sound_discovery = sound_discovery;
    app.voice_input_devices = voice_input_devices;
    app.voice_output_devices = voice_output_devices;
    app.config_dir = config_dir;
    let initial_size = terminal.size().map_err(ClientError::TerminalSetup)?;
    app.set_screen_area(Rect::new(0, 0, initial_size.width, initial_size.height));

    let mut connection =
        Connection::connect(&options.socket_path, options.session_name.clone()).await?;
    let mut trigger_execution_lease = TriggerExecutionLease::open(&options.socket_path);

    let (naming_events_tx, mut naming_events_rx) = mpsc::channel(NAMING_EVENTS_CHANNEL_CAPACITY);
    let mut naming_workers = NamingWorkers::new(naming_events_tx, app.inference_settings.clone());
    let (search_events_tx, mut search_events_rx) = mpsc::channel(1);
    let mut search_workers = SearchWorkers::new(search_events_tx);
    let (icon_search_events_tx, mut icon_search_events_rx) =
        mpsc::channel(ICON_SEARCH_EVENTS_CHANNEL_CAPACITY);
    let mut icon_search_workers = IconSearchWorkers::new(icon_search_events_tx);
    let mut control_plane = crate::control::ControlPlane::default();
    let mut voice_service = if app.voice_settings.enabled {
        start_voice_service(&mut app, &control_plane)
    } else {
        None
    };
    // Bus names of the MPRIS players `pause_playing_players` paused, so a
    // later stop resumes only those -- see `reconcile_voice_runtime` and
    // `VoiceSettings::pause_media_while_active`. Voice mode can already be
    // enabled at startup (a persisted setting, not just an F8 press), so
    // this covers that path the same way as an in-session toggle.
    let mut paused_media_players =
        if voice_service.is_some() && app.voice_settings.pause_media_while_active {
            crate::media_control::pause_playing_players().await
        } else {
            Vec::new()
        };
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
            app.project_icon =
                crate::project_naming::load_stored_project_icon(&app.session_cwd).unwrap_or(None);
        }
        Ok(None) => {
            app.is_project_name_loading = true;
            naming_workers.spawn_project_name_worker(app.session_cwd.clone());
        }
        Err(error) => {
            tracing::error!(
                error = %error,
                error_debug = ?error,
                "failed to load stored project name"
            );
            app.status_message = Some(format!("Could not infer project name: {error}"));
        }
    }

    let mut input_rx = spawn_input_forwarder();

    // Set so the very first pass through the loop always draws (there's
    // nothing on screen yet); every branch below that actually changes
    // visible state re-sets it, and it's cleared right after a draw
    // happens. See the module docs on why an unconditional `terminal.draw`
    // every iteration was wasted work under load.
    let mut needs_redraw = true;
    let mut needs_immediate_redraw = true;
    let mut last_draw_at = Instant::now();
    let mut last_recorded_status_message = None;
    let mut last_recorded_surface = None;

    'event_loop: while app.exit_reason.is_none() {
        let now = Instant::now();
        let mut voice_tool_outputs = Vec::new();
        let mut tick_delay = app.next_maintenance_delay(now);
        if needs_redraw && !needs_immediate_redraw {
            tick_delay = tick_delay.min(output_redraw_delay(now, last_draw_at));
        }

        tokio::select! {
            input_event = input_rx.recv() => {
                match input_event {
                    Some(event) => {
                        dispatch_ready_input_events(
                            &mut app,
                            &mut input_rx,
                            &mut naming_workers,
                            &mut icon_search_workers,
                            home_dir.as_deref(),
                            event,
                        );
                        needs_redraw = true;
                        needs_immediate_redraw = true;
                    }
                    // The input-reading thread ended (a crossterm read
                    // error -- see `spawn_input_forwarder`); keyboard and
                    // mouse are the only way `exit_reason` ever gets set
                    // (see `keys.rs`), so without this the client would
                    // otherwise keep running forever, fully unresponsive to
                    // the user, until something external kills the process.
                    None => {
                        tracing::error!("input event channel closed; client can no longer accept input");
                        break;
                    }
                }
            }
            server_event = connection.events.recv() => {
                match server_event {
                    Some(event) => {
                        if apply_server_events(
                            &mut app,
                            &mut connection.events,
                            event,
                            &mut naming_workers,
                            &mut icon_search_workers,
                            &mut trigger_execution_lease,
                            home_dir.as_deref(),
                        ) {
                            needs_immediate_redraw = true;
                        }
                        needs_redraw = true;
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
                crate::tick::apply_naming_worker_event(&mut app, &mut naming_workers, naming_event);
                needs_redraw = true;
                needs_immediate_redraw = true;
            }
            Some(search_event) = search_events_rx.recv() => {
                search_workers.finish();
                app.apply_workspace_search_result(search_event);
                needs_redraw = true;
                needs_immediate_redraw = true;
            }
            Some(icon_search_event) = icon_search_events_rx.recv() => {
                app.apply_icon_semantic_search_event(icon_search_event);
                needs_redraw = true;
                needs_immediate_redraw = true;
            }
            voice_event = next_voice_event(&mut voice_service) => {
                match voice_event {
                    Some(event) => {
                        voice_tool_outputs =
                            handle_voice_event(&mut app, &mut control_plane, event);
                    }
                    None if voice_service.is_some() => {
                        tracing::error!("voice service event stream closed");
                        voice_service = None;
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
                        let players = std::mem::take(&mut paused_media_players);
                        crate::media_control::resume_players(players).await;
                    }
                    None => {}
                }
                needs_redraw = true;
                needs_immediate_redraw = true;
            }
            () = tokio::time::sleep(tick_delay) => {
                if crate::tick::on_tick(&mut app, Instant::now(), &mut search_workers) {
                    needs_redraw = true;
                }
            }
        }

        dispatch_pending_app_work(
            &mut app,
            &mut naming_workers,
            &mut icon_search_workers,
            home_dir.as_deref(),
        );
        reconcile_debug_logging(&mut app);
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
                &mut paused_media_players,
            )
            .await;
            deliver_voice_interactions(&mut app, voice_service.as_ref()).await;
        }

        let outbound_requests = crate::outbound_requests::coalesce(app.take_outbound_requests());
        for request in outbound_requests {
            // The writer queue is deliberately bounded. Awaiting its capacity
            // applies lossless backpressure to crossterm's already-bounded
            // input channel instead of silently dropping a key or command.
            if connection.requests.send(request).await.is_err() {
                tracing::error!("server request channel closed before request delivery");
                break 'event_loop;
            }
        }

        // A confirmation-required terminal submission queues its visible
        // text as IPC before returning the tool result that makes the voice
        // model ask the question. This preserves the user-facing contract:
        // type first, then ask whether to press Enter.
        deliver_voice_tool_outputs(&mut app, &mut voice_service, voice_tool_outputs).await;
        if is_voice_tool_shutdown {
            reconcile_voice_runtime(
                &mut app,
                &control_plane,
                &mut voice_service,
                &mut paused_media_players,
            )
            .await;
            deliver_voice_interactions(&mut app, voice_service.as_ref()).await;
        }

        record_client_surface_change(&app, &mut last_recorded_surface);
        record_status_message_change(
            app.status_message.as_deref(),
            &mut last_recorded_status_message,
        );

        let can_draw = output_redraw_is_due(needs_immediate_redraw, Instant::now(), last_draw_at);
        if needs_redraw && can_draw {
            terminal
                .draw(|frame| crate::ui::draw(frame, &mut app))
                .map_err(ClientError::TerminalSetup)?;
            needs_redraw = false;
            needs_immediate_redraw = false;
            last_draw_at = Instant::now();
        }
    }

    if let Some(service) = voice_service {
        service.shutdown().await;
    }
    crate::media_control::resume_players(paused_media_players).await;

    // A dropped input/server channel remains an ordinary exit. Only the
    // explicit Restart menu action can request a process re-exec.
    Ok(app.exit_reason.unwrap_or(ClientExitReason::Quit))
}

/// Applies one coalesced Debug toggle to this client process and queues the
/// matching live update for the detached server. Credentials are never logged;
/// disabling records one final boundary event before closing the local file.
fn reconcile_debug_logging(app: &mut App) {
    let Some(enabled) = app.take_pending_debug_logging_enabled() else {
        return;
    };
    if !enabled {
        tracing::info!("client file logging disabled from Debug settings");
    }
    match ilium_logging::set_enabled(enabled) {
        Ok(()) => {
            app.queue_request(ilium_ipc::ClientRequest::UpdateDebugLogging { enabled });
            if enabled {
                tracing::info!("client file logging enabled from Debug settings");
            }
        }
        Err(error) => {
            tracing::error!(%error, "failed to apply Debug file logging setting");
            app.status_message = Some(format!("Could not apply debug logging: {error}"));
        }
    }
}

fn start_voice_service(
    app: &mut App,
    control_plane: &crate::control::ControlPlane,
) -> Option<ilium_voice::VoiceService> {
    let settings = &app.voice_settings;
    let api_key = if settings.api_key.trim().is_empty() {
        std::env::var("OPENAI_API_KEY").unwrap_or_default()
    } else {
        settings.api_key.clone()
    };
    let config = ilium_voice::VoiceRuntimeConfig {
        api_key: api_key.into(),
        model: settings.model,
        voice: settings.voice,
        reasoning_effort: settings.reasoning_effort,
        input_mode: settings.input_mode,
        vad_eagerness: settings.vad_eagerness,
        input_device_name: settings.input_device_name.clone(),
        output_device_name: settings.output_device_name.clone(),
        output_volume_percent: settings.output_volume_percent,
        instructions: crate::control::system_instructions(&settings.custom_prompt),
    };
    match ilium_voice::VoiceService::start(config, control_plane.tool_definitions()) {
        Ok(service) => {
            app.update_voice_connection_state(ilium_voice::VoiceConnectionState::Connecting);
            Some(service)
        }
        Err(error) => {
            app.update_voice_connection_state(ilium_voice::VoiceConnectionState::Failed(
                error.to_string(),
            ));
            None
        }
    }
}

async fn next_voice_event(
    voice_service: &mut Option<ilium_voice::VoiceService>,
) -> Option<ilium_voice::VoiceEvent> {
    match voice_service {
        Some(service) => service.next_event().await,
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

fn handle_voice_event(
    app: &mut App,
    control_plane: &mut crate::control::ControlPlane,
    event: ilium_voice::VoiceEvent,
) -> Vec<ilium_voice::VoiceToolOutput> {
    match event {
        ilium_voice::VoiceEvent::StateChanged(state) => {
            app.update_voice_connection_state(state);
            Vec::new()
        }
        ilium_voice::VoiceEvent::ToolInvocations(invocations) => invocations
            .into_iter()
            .map(|invocation| control_plane.execute_invocation(app, invocation))
            .collect::<Vec<_>>(),
        ilium_voice::VoiceEvent::UserTranscript(transcript) => {
            app.voice_last_user_transcript = Some(transcript);
            Vec::new()
        }
        ilium_voice::VoiceEvent::AssistantTranscript(delta) => {
            app.voice_last_assistant_transcript
                .get_or_insert_with(String::new)
                .push_str(&delta);
            Vec::new()
        }
        ilium_voice::VoiceEvent::ProviderError(error) => {
            app.status_message = Some(format!("Voice provider error: {error}"));
            Vec::new()
        }
    }
}

/// Returns completed tool calls to the model only after the event loop has
/// dispatched every terminal/UI request produced by their execution.
async fn deliver_voice_tool_outputs(
    app: &mut App,
    voice_service: &mut Option<ilium_voice::VoiceService>,
    outputs: Vec<ilium_voice::VoiceToolOutput>,
) {
    if outputs.is_empty() {
        return;
    }
    let is_session_terminating = voice_tool_outputs_request_shutdown(&outputs);
    if is_session_terminating {
        let Some(service) = voice_service.take() else {
            return;
        };
        service.shutdown_after_tool_outputs(outputs).await;
        return;
    }

    let Some(service) = voice_service.as_ref() else {
        return;
    };
    if service
        .command_sender()
        .send(ilium_voice::VoiceCommand::SubmitToolOutputs(outputs))
        .await
        .is_err()
    {
        app.update_voice_connection_state(ilium_voice::VoiceConnectionState::Failed(
            "could not return the tool result to the voice model".to_owned(),
        ));
    }
}

/// One terminating result makes shutdown the dominant disposition for the
/// whole parallel function-call batch, while all results are still returned.
fn voice_tool_outputs_request_shutdown(outputs: &[ilium_voice::VoiceToolOutput]) -> bool {
    outputs
        .iter()
        .any(|output| output.terminate_session_after_delivery)
}

/// Reconciles a pending start/stop/reconfigure and, on a real start or stop
/// (not a reconfigure -- that's a live settings change, not the user
/// entering or leaving voice mode), pauses or resumes system media playback
/// through [`crate::media_control`] when
/// `VoiceSettings::pause_media_while_active` is on. Resuming always runs
/// regardless of that setting's *current* value and drains
/// `paused_media_players` unconditionally, so toggling the setting off
/// mid-session can never strand a player paused.
async fn reconcile_voice_runtime(
    app: &mut App,
    control_plane: &crate::control::ControlPlane,
    voice_service: &mut Option<ilium_voice::VoiceService>,
    paused_media_players: &mut Vec<String>,
) {
    let Some(request) = app.take_voice_runtime_request() else {
        return;
    };
    if let Some(service) = voice_service.take() {
        service.shutdown().await;
    }
    match request {
        crate::app::VoiceRuntimeRequest::Stop => {
            app.update_voice_connection_state(ilium_voice::VoiceConnectionState::Disabled);
            let players = std::mem::take(paused_media_players);
            crate::media_control::resume_players(players).await;
        }
        crate::app::VoiceRuntimeRequest::Start => {
            *voice_service = start_voice_service(app, control_plane);
            if voice_service.is_some() && app.voice_settings.pause_media_while_active {
                *paused_media_players = crate::media_control::pause_playing_players().await;
            }
        }
        crate::app::VoiceRuntimeRequest::Reconfigure => {
            *voice_service = start_voice_service(app, control_plane);
        }
    }
}

/// Delivers lossless push-to-talk edges only after start/stop/reconfigure has
/// reconciled the actor for this input batch.
async fn deliver_voice_interactions(
    app: &mut App,
    voice_service: Option<&ilium_voice::VoiceService>,
) {
    let requests = app.take_voice_interaction_requests();
    let Some(service) = voice_service else {
        return;
    };
    for request in requests {
        let command = match request {
            crate::app::VoiceInteractionRequest::StartPushToTalk => {
                ilium_voice::VoiceCommand::StartPushToTalk
            }
            crate::app::VoiceInteractionRequest::StopPushToTalk => {
                ilium_voice::VoiceCommand::StopPushToTalk
            }
        };
        if service.command_sender().send(command).await.is_err() {
            app.update_voice_connection_state(ilium_voice::VoiceConnectionState::Failed(
                "could not deliver push-to-talk input".to_owned(),
            ));
            break;
        }
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
    events_rx: &mut mpsc::Receiver<ilium_ipc::ServerEvent>,
    first: ilium_ipc::ServerEvent,
    naming_workers: &mut NamingWorkers,
    icon_search_workers: &mut IconSearchWorkers,
    trigger_execution_lease: &mut TriggerExecutionLease,
    home_dir: Option<&std::path::Path>,
) -> bool {
    use ilium_ipc::ServerEvent;

    let mut pending_screen_update: Option<(ilium_core::NodeId, u64, Vec<u8>)> = None;
    let mut next = Some(first);
    let mut observed_non_screen_event = false;
    for _ in 0..MAX_SERVER_EVENTS_PER_BATCH {
        let event = match next.take() {
            Some(event) => event,
            None => match events_rx.try_recv() {
                Ok(event) => event,
                Err(_) => break,
            },
        };
        match event {
            ServerEvent::ScreenUpdate {
                pane_id: incoming_pane_id,
                sequence: incoming_sequence,
                bytes: mut incoming_bytes,
            } => match &mut pending_screen_update {
                Some((pending_pane_id, pending_sequence, pending_bytes))
                    if *pending_pane_id == incoming_pane_id
                        && incoming_sequence == pending_sequence.saturating_add(1)
                        && screen_updates_fit(pending_bytes.len(), incoming_bytes.len()) =>
                {
                    pending_bytes.append(&mut incoming_bytes);
                    *pending_sequence = incoming_sequence;
                }
                _ => {
                    flush_pending_screen_update(app, &mut pending_screen_update);
                    pending_screen_update =
                        Some((incoming_pane_id, incoming_sequence, incoming_bytes));
                }
            },
            ServerEvent::DebugLoggingChanged { enabled } => {
                observed_non_screen_event = true;
                flush_pending_screen_update(app, &mut pending_screen_update);
                synchronize_debug_logging_from_server(app, enabled);
            }
            other => {
                observed_non_screen_event = true;
                flush_pending_screen_update(app, &mut pending_screen_update);
                if let Some(occurrence) = crate::render_cache::apply(app, other) {
                    if trigger_execution_lease.claim() {
                        app.handle_trigger_occurrence(occurrence);
                    }
                }
            }
        }
    }
    flush_pending_screen_update(app, &mut pending_screen_update);
    dispatch_pending_app_work(app, naming_workers, icon_search_workers, home_dir);
    observed_non_screen_event
}

/// Applies the server-accepted state to this client's writer and UI without
/// persisting or queueing another request. This is what makes one Debug toggle
/// converge every client already attached to the same detached session.
fn synchronize_debug_logging_from_server(app: &mut App, enabled: bool) {
    if !enabled {
        tracing::info!("client file logging disabled after server synchronization");
    }
    match ilium_logging::set_enabled(enabled) {
        Ok(()) => {
            app.debug_settings.file_logging_enabled = enabled;
            if enabled {
                tracing::info!("client file logging synchronized from server");
            }
        }
        Err(error) => {
            tracing::error!(%error, "failed to synchronize client file logging from server");
            app.status_message = Some(format!("Could not synchronize debug logging: {error}"));
        }
    }
}

/// Applies one received input event and any immediately queued successors.
/// Consecutive mouse-motion reports are coalesced to their newest position:
/// only that position can determine the current hover state, while keeping
/// every non-motion event in order preserves click, drag, and scroll input.
fn dispatch_ready_input_events(
    app: &mut App,
    input_rx: &mut mpsc::Receiver<Event>,
    naming_workers: &mut NamingWorkers,
    icon_search_workers: &mut IconSearchWorkers,
    home_dir: Option<&std::path::Path>,
    first: Event,
) {
    let mut next = Some(first);

    for _ in 0..MAX_INPUT_EVENTS_PER_BATCH {
        let Some(event) = next.take() else {
            break;
        };
        let event = if matches!(
            event,
            Event::Mouse(crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::Moved,
                ..
            })
        ) {
            let mut newest_motion = event;
            loop {
                match input_rx.try_recv() {
                    Ok(candidate)
                        if matches!(
                            candidate,
                            Event::Mouse(crossterm::event::MouseEvent {
                                kind: crossterm::event::MouseEventKind::Moved,
                                ..
                            })
                        ) =>
                    {
                        newest_motion = candidate;
                    }
                    Ok(candidate) => {
                        next = Some(candidate);
                        break;
                    }
                    Err(_) => break,
                }
            }
            newest_motion
        } else {
            event
        };

        dispatch_input_event(app, naming_workers, icon_search_workers, home_dir, event);

        if next.is_none() {
            next = input_rx.try_recv().ok();
        }
    }
}

/// Applies and clears `pending`, if it holds a merged `ScreenUpdate` run --
/// see `apply_server_events`.
fn flush_pending_screen_update(
    app: &mut App,
    pending: &mut Option<(ilium_core::NodeId, u64, Vec<u8>)>,
) {
    if let Some((pane_id, sequence, bytes)) = pending.take() {
        crate::render_cache::apply(
            app,
            ilium_ipc::ServerEvent::ScreenUpdate {
                pane_id,
                sequence,
                bytes,
            },
        );
    }
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
        return;
    }
    app.handle_event(event);
    dispatch_pending_app_work(app, naming_workers, icon_search_workers, home_dir);
}

/// Drains synchronous `App` outboxes into the async/background owners. This
/// must run after every event-loop branch, not only keyboard input, because
/// voice tool execution invokes the same semantic methods and can request
/// tests, icon search, retitling, or restructuring too.
fn dispatch_pending_app_work(
    app: &mut App,
    naming_workers: &mut NamingWorkers,
    icon_search_workers: &mut IconSearchWorkers,
    home_dir: Option<&std::path::Path>,
) {
    if let Some(request) = app.take_pending_icon_semantic_search() {
        icon_search_workers.request(request);
    }
    naming_workers.set_inference_settings(app.inference_settings.clone());
    if app.take_pending_inference_test() {
        naming_workers.spawn_inference_test_worker();
    }
    if app.take_pending_ollama_model_refresh() {
        naming_workers.spawn_ollama_models_worker();
    }

    for request in app.take_pending_retitle_requests() {
        match request {
            crate::app::PendingRetitleRequest::Session {
                input,
                title_generation,
                trigger,
            } => match home_dir {
                Some(home_dir) => naming_workers.spawn_session_title_worker(
                    crate::naming_workers::SessionTitleWorkerRequest {
                        home: home_dir.to_path_buf(),
                        input,
                        title_generation,
                        trigger,
                    },
                ),
                None => {
                    app.titles_loading.remove(&input.pane_id);
                    app.status_message =
                        Some("Could not infer title: home directory unavailable".to_string());
                }
            },
            crate::app::PendingRetitleRequest::Terminal { input, trigger } => {
                naming_workers.spawn_terminal_title_worker(input, trigger);
            }
        }
    }

    for request in app.take_pending_restructure_requests() {
        match home_dir {
            Some(home_dir) => naming_workers.spawn_restructure_worker(
                request.project_id,
                request.contexts,
                request.current_structure,
                home_dir.to_path_buf(),
                request.project_cwd,
            ),
            None => {
                app.finish_project_restructure(
                    request.project_id,
                    Err(anyhow::anyhow!("home directory unavailable")),
                );
            }
        }
    }
}

/// Reads crossterm events on a dedicated `std::thread` (crossterm's
/// `event::read()` blocks) and forwards each into a tokio channel the
/// main select loop merges -- same bridging pattern
/// `crate::naming_workers` uses for its own background workers.
///
/// The channel is bounded, but every input event (a keystroke, a mouse
/// move) still must reach the loop in order -- unlike a `ScreenUpdate`, an
/// intermediate one can't be dropped without losing a real keypress -- so
/// this blocks (`blocking_send`) rather than drops on a full buffer. That's
/// safe here specifically because the producer is this dedicated OS
/// thread, not a tokio task: it has nothing else to do while the queue
/// drains, and `crossterm::event::read()` already paces it to real input
/// anyway (a bounded queue with a generous capacity only ever fills if the
/// main loop stalls under real load, which is exactly when backpressure
/// here -- rather than an unbounded backlog -- is wanted).
fn spawn_input_forwarder() -> mpsc::Receiver<Event> {
    let (tx, rx) = mpsc::channel(INPUT_CHANNEL_CAPACITY);
    std::thread::spawn(move || loop {
        match crossterm::event::read() {
            Ok(event) => {
                if tx.blocking_send(event).is_err() {
                    break;
                }
            }
            Err(error) => {
                tracing::error!("crossterm event read failed, stopping input thread: {error}");
                break;
            }
        }
    });
    rx
}

#[cfg(test)]
mod responsiveness_tests {
    use super::*;

    #[test]
    fn merged_screen_updates_respect_the_byte_ceiling() {
        assert!(screen_updates_fit(MAX_MERGED_SCREEN_BYTES_PER_BATCH - 1, 1));
        assert!(!screen_updates_fit(MAX_MERGED_SCREEN_BYTES_PER_BATCH, 1));
        assert!(!screen_updates_fit(usize::MAX, usize::MAX));
    }

    #[test]
    fn output_redraws_are_limited_but_input_redraws_are_immediate() {
        let last_draw_at = Instant::now();
        let early = last_draw_at + Duration::from_millis(5);
        assert_eq!(
            output_redraw_delay(early, last_draw_at),
            Duration::from_millis(11)
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
                result: serde_json::json!({ "status": "ok" }),
                request_follow_up: true,
                terminate_session_after_delivery: false,
            },
            ilium_voice::VoiceToolOutput {
                call_id: "stop".to_owned(),
                result: serde_json::json!({ "status": "ok" }),
                request_follow_up: false,
                terminate_session_after_delivery: true,
            },
        ];

        assert!(voice_tool_outputs_request_shutdown(&outputs));
        assert!(!voice_tool_outputs_request_shutdown(&outputs[..1]));
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
        let mut paused_media_players = Vec::new();

        reconcile_voice_runtime(
            &mut app,
            &control_plane,
            &mut voice_service,
            &mut paused_media_players,
        )
        .await;

        assert!(!app.voice_settings.enabled);
        assert_eq!(
            app.voice_connection_state,
            ilium_voice::VoiceConnectionState::Disabled
        );
        assert!(app.take_voice_runtime_request().is_none());
    }
}
