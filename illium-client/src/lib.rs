//! illium-client: the `ratatui` TUI, a thin renderer/input-dispatcher over
//! `illium-ipc` -- see `app.rs`'s module docs for the render-cache
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
//! - [`naming_workers`] -- background `illium-kilo-gateway` title
//!   inference (`std::thread`, bridged into the tokio event loop).
//! - [`run`] -- the actual entry point: terminal lifecycle, connects,
//!   drives the event loop until the user quits or the connection drops.
//!
//! Everything else (`ui`, `tree_ui`, `modal`, `help`, `theme`, `layout`,
//! `text_prompt`, `explorer_overlay`, `editor_pane` and its chrome/
//! highlight/toolbar/syntax/minimap helpers, `markdown`, `keymap`,
//! `session_naming`, `project_naming`, `transcript_prompts`,
//! `project_config`, `workspace_file`, `naming`, `session_transcript`) is
//! presentation or local-file-I/O logic that doesn't care whether its
//! data came from a local `Tree` or a render-cache mirror of one.

pub mod app;
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
pub mod syntax;
pub mod terminal_guard;
pub mod terminal_view;
pub mod text_prompt;
pub mod theme;
pub mod tick;
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
/// `illium-server` session and start rendering it.
pub struct RunOptions {
    pub session_name: String,
    pub session_cwd: PathBuf,
}

/// Idle-state redraw/poll cadence -- matches the pre-client/server
/// design's own `POLL_INTERVAL`.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Runs illium-client until the user quits (`Action::Quit`) or the
/// connection to the server ends. Owns the whole terminal lifecycle
/// (raw mode, alternate screen -- see `TerminalGuard`) and the tokio
/// event loop described in the crate docs.
pub async fn run(options: RunOptions) -> Result<(), ClientError> {
    if !options.session_cwd.is_dir() {
        return Err(ClientError::InvalidSessionCwd(options.session_cwd));
    }
    let socket_path = crate::paths::socket_path(&options.session_name)?;

    // Resolved and installed once, before the terminal enters raw/
    // alternate-screen mode and before any render call -- see
    // `theme::THEME`'s doc comment on why a one-time `OnceLock` init is only
    // safe this early.
    init_config();

    // `TerminalGuard::drop` restores the terminal whether this function
    // returns `Ok`, `Err`, or panics and unwinds through this stack frame.
    let guard = TerminalGuard::enter()?;
    let result = run_inner(options, &socket_path).await;
    drop(guard);
    result
}

/// Resolves `config.toml`'s client-side tables and installs the effective
/// keybinding table / theme for the rest of the process's lifetime. A
/// config directory that can't be resolved, or a config file that fails to
/// load, is logged and falls back to defaults rather than refusing to
/// start the client over it -- matches `illium-server::main`'s own
/// "a bad optional config file is a warning, not a fatal error" policy.
fn init_config() {
    let config = match crate::paths::config_dir() {
        Ok(config_dir) => match crate::config::load(&config_dir) {
            Ok(config) => config,
            Err(error) => {
                tracing::warn!("failed to load config, using defaults: {error}");
                crate::config::ClientConfig::default()
            }
        },
        Err(error) => {
            tracing::warn!("failed to resolve config directory, using defaults: {error}");
            crate::config::ClientConfig::default()
        }
    };
    crate::keymap::init_effective_bindings(config.keybindings);
    crate::theme::init(config.theme);
}

async fn run_inner(options: RunOptions, socket_path: &std::path::Path) -> Result<(), ClientError> {
    let backend = CrosstermBackend::new(std::io::stdout());
    let mut terminal = Terminal::new(backend).map_err(ClientError::TerminalSetup)?;

    let mut app = App::new(options.session_name.clone(), options.session_cwd.clone());
    let initial_size = terminal.size().map_err(ClientError::TerminalSetup)?;
    app.set_screen_area(Rect::new(0, 0, initial_size.width, initial_size.height));

    let mut connection = Connection::connect(socket_path, options.session_name.clone()).await?;

    let (naming_events_tx, mut naming_events_rx) = mpsc::unbounded_channel();
    let mut naming_workers = NamingWorkers::new(naming_events_tx);
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

    while !app.should_quit {
        let tick_delay = if app.is_layout_animating() {
            TREE_WIDTH_ANIMATION_FRAME_INTERVAL
        } else {
            POLL_INTERVAL
        };

        tokio::select! {
            Some(event) = input_rx.recv() => {
                dispatch_input_event(&mut app, event);
            }
            server_event = connection.events.recv() => {
                match server_event {
                    Some(event) => crate::render_cache::apply(&mut app, event),
                    // The reader task ended -- the server is gone or the
                    // connection dropped; nothing left to attach to.
                    None => break,
                }
            }
            Some(naming_event) = naming_events_rx.recv() => {
                crate::tick::apply_naming_worker_event(&mut app, &mut naming_workers, naming_event);
            }
            () = tokio::time::sleep(tick_delay) => {
                crate::tick::on_tick(&mut app, Instant::now());
            }
        }

        for request in app.take_outbound_requests() {
            // A send failure means the writer task already ended (the
            // connection dropped) -- the next loop iteration's
            // `connection.events.recv()` returning `None` is what
            // actually ends the session; nothing more to do here.
            let _ = connection.requests.send(request);
        }

        terminal
            .draw(|frame| crate::ui::draw(frame, &mut app))
            .map_err(ClientError::TerminalSetup)?;
    }

    Ok(())
}

fn dispatch_input_event(app: &mut App, event: Event) {
    if let Event::Resize(cols, rows) = &event {
        app.set_screen_area(Rect::new(0, 0, *cols, *rows));
        return;
    }
    app.handle_event(event);
}

/// Reads crossterm events on a dedicated `std::thread` (crossterm's
/// `event::read()` blocks) and forwards each into a tokio channel the
/// main select loop merges -- same bridging pattern
/// `crate::naming_workers` uses for its own background workers.
fn spawn_input_forwarder() -> mpsc::UnboundedReceiver<Event> {
    let (tx, rx) = mpsc::unbounded_channel();
    std::thread::spawn(move || loop {
        match crossterm::event::read() {
            Ok(event) => {
                if tx.send(event).is_err() {
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
