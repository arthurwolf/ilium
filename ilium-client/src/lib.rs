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
//! - [`naming_workers`] -- background `ilium-kilo-gateway` title
//!   inference (`std::thread`, bridged into the tokio event loop).
//! - [`run`] -- the actual entry point: terminal lifecycle, connects,
//!   drives the event loop until the user quits or the connection drops.
//!
//! Everything else (`ui`, `tree_ui`, `modal`, `help`, `theme`, `layout`,
//! `settings_ui`, `text_prompt`, `explorer_overlay`, `editor_pane` and its
//! chrome/highlight/toolbar/syntax/minimap helpers, `markdown`, `keymap`,
//! `session_naming`, `project_naming`, `transcript_prompts`,
//! `project_config`, `workspace_file`, `naming`, `session_transcript`) is
//! presentation or local-file-I/O logic that doesn't care whether its
//! data came from a local `Tree` or a render-cache mirror of one.

pub mod app;
pub mod board;
pub mod config;
pub mod connection;
pub mod editor_chrome;
pub mod editor_highlight;
pub mod editor_pane;
pub mod editor_toolbar;
pub mod error;
pub mod explorer_overlay;
pub mod help;
pub mod keymap;
pub mod keys;
pub mod layout;
pub mod markdown;
pub mod minimap;
pub mod modal;
pub mod mouse;
pub mod naming;
pub mod naming_workers;
pub mod paths;
pub mod project_config;
pub mod project_naming;
pub mod render_cache;
pub mod session_naming;
pub mod session_transcript;
pub mod settings_ui;
pub mod split_layout;
pub mod syntax;
pub mod terminal_guard;
pub mod terminal_naming;
pub mod terminal_title_inference;
pub mod terminal_view;
pub mod text_prompt;
pub mod theme;
pub mod tick;
pub mod title_inference;
pub mod transcript_prompts;
pub mod tree_ui;
pub mod ui;
pub mod workspace_file;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::event::Event;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::Terminal;
use tokio::sync::mpsc;

use crate::app::App;
use crate::connection::Connection;
use crate::error::ClientError;
use crate::layout::TREE_WIDTH_ANIMATION_FRAME_INTERVAL;
use crate::naming_workers::NamingWorkers;
use crate::terminal_guard::TerminalGuard;

/// Everything [`run`] needs to attach to one already-running
/// `ilium-server` session and start rendering it.
pub struct RunOptions {
    pub session_name: String,
    pub session_cwd: PathBuf,
    /// The CLI resolves the project-scoped session socket before handing
    /// control to the TUI, so a client can never accidentally derive the
    /// machine-wide socket belonging to a different project.
    pub socket_path: PathBuf,
}

/// Idle-state redraw/poll cadence -- matches the pre-client/server
/// design's own `POLL_INTERVAL`.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

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

/// Runs ilium-client until the user quits (`Action::Quit`) or the
/// connection to the server ends. Owns the whole terminal lifecycle
/// (raw mode, alternate screen -- see `TerminalGuard`) and the tokio
/// event loop described in the crate docs.
pub async fn run(options: RunOptions) -> Result<(), ClientError> {
    if !options.session_cwd.is_dir() {
        return Err(ClientError::InvalidSessionCwd(options.session_cwd));
    }
    let socket_path = options.socket_path.clone();

    // Resolved and installed once, before the terminal enters raw/
    // alternate-screen mode and before any render call -- see
    // `theme::THEME`'s doc comment on why a one-time `OnceLock` init is only
    // safe this early. `config_dir` is threaded through to `run_inner` too:
    // the settings screen (`crate::app::Mode::Settings`) needs it later to
    // persist a change (`crate::config::save_ui_settings`).
    let (config, config_dir) = init_config();

    // `TerminalGuard::drop` restores the terminal whether this function
    // returns `Ok`, `Err`, or panics and unwinds through this stack frame.
    let guard = TerminalGuard::enter()?;
    let result = run_inner(options, &socket_path, config, config_dir).await;
    drop(guard);
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
fn init_config() -> (crate::config::ClientConfig, Option<PathBuf>) {
    let config_dir = match crate::paths::config_dir() {
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
                crate::config::ClientConfig::default()
            }
        },
        None => crate::config::ClientConfig::default(),
    };
    crate::keymap::init_effective_bindings(config.keybindings.clone());
    crate::theme::init(config.theme);
    (config, config_dir)
}

async fn run_inner(
    options: RunOptions,
    socket_path: &std::path::Path,
    config: crate::config::ClientConfig,
    config_dir: Option<PathBuf>,
) -> Result<(), ClientError> {
    let backend = CrosstermBackend::new(std::io::stdout());
    let mut terminal = Terminal::new(backend).map_err(ClientError::TerminalSetup)?;

    let mut app = App::new(options.session_name.clone(), options.session_cwd.clone());
    app.apply_ui_settings(config.ui);
    app.keyboard_settings = config.keyboard;
    app.config_dir = config_dir;
    let initial_size = terminal.size().map_err(ClientError::TerminalSetup)?;
    app.set_screen_area(Rect::new(0, 0, initial_size.width, initial_size.height));

    let mut connection = Connection::connect(socket_path, options.session_name.clone()).await?;

    let (naming_events_tx, mut naming_events_rx) = mpsc::channel(NAMING_EVENTS_CHANNEL_CAPACITY);
    let mut naming_workers = NamingWorkers::new(naming_events_tx);
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
        Ok(Some(name)) => app.project_name = Some(name),
        Ok(None) => {
            app.is_project_name_loading = true;
            naming_workers.spawn_project_name_worker(app.session_cwd.clone());
        }
        Err(error) => {
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

    while !app.should_quit {
        let tick_delay = if app.is_layout_animating() {
            TREE_WIDTH_ANIMATION_FRAME_INTERVAL
        } else {
            POLL_INTERVAL
        };

        tokio::select! {
            Some(event) = input_rx.recv() => {
                dispatch_input_event(&mut app, &mut naming_workers, home_dir.as_deref(), event);
                needs_redraw = true;
            }
            server_event = connection.events.recv() => {
                match server_event {
                    Some(event) => {
                        apply_server_events(
                            &mut app,
                            &mut connection.events,
                            event,
                            &mut naming_workers,
                            home_dir.as_deref(),
                        );
                        needs_redraw = true;
                    }
                    // The reader task ended -- the server is gone or the
                    // connection dropped; nothing left to attach to.
                    None => break,
                }
            }
            Some(naming_event) = naming_events_rx.recv() => {
                crate::tick::apply_naming_worker_event(&mut app, &mut naming_workers, naming_event);
                needs_redraw = true;
            }
            () = tokio::time::sleep(tick_delay) => {
                if crate::tick::on_tick(&mut app, Instant::now()) {
                    needs_redraw = true;
                }
            }
        }

        for request in app.take_outbound_requests() {
            // `try_send` rather than an awaited `send`: this loop is the
            // sole producer for `connection.requests`, so blocking here
            // would stall every other branch above (input, server events,
            // ticks) on the writer task keeping up -- exactly the
            // responsiveness regression bounding this channel must not
            // introduce. A full buffer (the writer task gone, or truly
            // saturated) drops the request; the next loop iteration's
            // `connection.events.recv()` returning `None` is what actually
            // ends a dead session, same as before this channel was bounded.
            if let Err(error) = connection.requests.try_send(request) {
                tracing::warn!(
                    "dropping outbound request, connection channel full or closed: {error}"
                );
            }
        }

        if needs_redraw {
            terminal
                .draw(|frame| crate::ui::draw(frame, &mut app))
                .map_err(ClientError::TerminalSetup)?;
            needs_redraw = false;
        }
    }

    Ok(())
}

/// Applies `first` (already received) and then drains + applies every
/// `ServerEvent` already queued on `events_rx` without waiting for more --
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
/// changes.
fn apply_server_events(
    app: &mut App,
    events_rx: &mut mpsc::Receiver<ilium_ipc::ServerEvent>,
    first: ilium_ipc::ServerEvent,
    naming_workers: &mut NamingWorkers,
    home_dir: Option<&std::path::Path>,
) {
    use ilium_ipc::ServerEvent;

    let mut pending_screen_update: Option<(ilium_core::NodeId, u64, Vec<u8>)> = None;
    let mut next = Some(first);
    loop {
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
                        && incoming_sequence == pending_sequence.saturating_add(1) =>
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
            other => {
                flush_pending_screen_update(app, &mut pending_screen_update);
                let applied = crate::render_cache::apply(app, other);
                maybe_start_title_inference(app, naming_workers, home_dir, &applied);
            }
        }
    }
    flush_pending_screen_update(app, &mut pending_screen_update);
}

/// Spawns a background session-title-inference worker for whichever pane
/// `crate::title_inference::pane_ready_for_inference` says is ready right
/// now, given what `applied` just observed -- a no-op for every other
/// event/transition, or when `home_dir` couldn't be resolved (session-title
/// inference's transcript lookups are all rooted under the home
/// directory, so there is nothing useful to attempt without it).
fn maybe_start_title_inference(
    app: &mut App,
    naming_workers: &mut NamingWorkers,
    home_dir: Option<&std::path::Path>,
    applied: &crate::title_inference::AppliedEvent,
) {
    let Some(home_dir) = home_dir else {
        return;
    };
    let Some((pane_id, agent_class, session_id)) =
        crate::title_inference::pane_ready_for_inference(app, applied)
    else {
        return;
    };
    app.titles_loading.insert(pane_id);
    *app.title_inference_attempts
        .entry((pane_id, session_id.clone()))
        .or_insert(0) += 1;
    naming_workers.spawn_session_title_worker(
        pane_id,
        home_dir.to_path_buf(),
        app.session_cwd.clone(),
        agent_class,
        session_id,
        crate::naming_workers::TitleTrigger::Automatic,
    );
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
/// any `PendingRetitleRequest`s that dispatch produced -- a manual retitle
/// click (`App::action_request_retitle`) or the terminal Enter-press
/// counter reaching its trigger interval (`App::maybe_trigger_terminal_retitle`).
/// `App` only ever queues these (see `PendingRetitleRequest`'s doc
/// comment); this is the one place with both an `App` and a
/// `NamingWorkers` handle to actually start the background worker,
/// mirroring how `maybe_start_title_inference` does the same for the
/// server-event-driven agent-titling trigger.
fn dispatch_input_event(
    app: &mut App,
    naming_workers: &mut NamingWorkers,
    home_dir: Option<&std::path::Path>,
    event: Event,
) {
    if let Event::Resize(cols, rows) = &event {
        app.set_screen_area(Rect::new(0, 0, *cols, *rows));
        return;
    }
    app.handle_event(event);

    for request in app.take_pending_retitle_requests() {
        match request {
            crate::app::PendingRetitleRequest::Session {
                pane_id,
                agent_class,
                session_id,
                trigger,
            } => match home_dir {
                Some(home_dir) => naming_workers.spawn_session_title_worker(
                    pane_id,
                    home_dir.to_path_buf(),
                    app.session_cwd.clone(),
                    agent_class,
                    session_id,
                    trigger,
                ),
                None => {
                    app.titles_loading.remove(&pane_id);
                    app.status_message =
                        Some("Could not infer title: home directory unavailable".to_string());
                }
            },
            crate::app::PendingRetitleRequest::Terminal {
                pane_id,
                screen_text,
                trigger,
            } => {
                naming_workers.spawn_terminal_title_worker(pane_id, screen_text, trigger);
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
