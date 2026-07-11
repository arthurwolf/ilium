//! illium entrypoint: terminal setup/teardown and the event loop. All
//! domain/UI logic lives in the other modules -- this file only owns
//! process lifecycle concerns (raw mode, alternate screen, panics) and
//! the top-level poll/tick/draw loop.

mod agent_detect;
mod app;
mod editor_chrome;
mod editor_highlight;
mod editor_pane;
mod editor_toolbar;
mod explorer_overlay;
mod help;
mod keymap;
mod layout;
mod markdown;
mod minimap;
mod modal;
mod naming;
mod project_config;
mod project_naming;
mod session_naming;
mod syntax;
mod term_pane;
mod text_prompt;
mod theme;
mod transcript_prompts;
mod tree_ui;
mod ui;
mod workspace_file;

use std::collections::HashMap;
use std::io::{self, Stdout};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use clap::Parser;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, supports_keyboard_enhancement, EnterAlternateScreen,
    LeaveAlternateScreen,
};
use illium_core::NodeId;
use illium_kilo_gateway::KiloGatewayClient;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::Terminal;
use sysinfo::System;

use crate::app::App;
use crate::layout::UiLayout;

/// illium: a tmux-like terminal multiplexer TUI.
#[derive(Parser, Debug)]
#[command(
    name = "illium",
    about = "A tmux-like terminal multiplexer TUI",
    version
)]
struct Cli {
    /// Project directory for this session. Every terminal pane spawns
    /// here, and the file-picker (new editor pane) opens rooted here.
    /// Defaults to the directory illium was launched from.
    #[arg(long, default_value = ".")]
    cwd: PathBuf,
}

/// How often `App::tick_detection` runs, shared across all panes (a
/// single fixed interval is the correct v1 scope -- see the project
/// README for the later, per-pane-adaptive milestone).
///
/// `agent_detect::refresh` does a full-system process enumeration every
/// tick to keep `identify_agent`'s pid/parent tree walk correct; measured
/// at 120-300ms on a dev box with several thousand processes (it no
/// longer reads `cwd`/`environ` for every process, only for the one
/// matched agent pid per pane -- see `agent_detect::refresh`'s doc
/// comment). 2s leaves that comfortable headroom against `POLL_INTERVAL`
/// below without risking the tick eating a visible fraction of every
/// cycle; there's little detection-latency benefit to going much lower
/// until that scan itself moves off the main thread.
const DETECTION_INTERVAL: Duration = Duration::from_secs(2);

/// How long each event poll waits before returning control to the main
/// loop, so the detection tick still runs promptly between keystrokes.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// RAII guard that restores the terminal to its normal state on drop,
/// including on panic unwind, so a crash never leaves the user's shell
/// stuck in raw mode / the alternate screen.
struct TerminalGuard {
    keyboard_enhancement_pushed: bool,
}

impl TerminalGuard {
    fn enter() -> anyhow::Result<Self> {
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;

        // Not every terminal supports the Kitty keyboard protocol; only
        // push the enhancement flags when the terminal says it can
        // disambiguate keys, and remember to pop them again in `Drop`.
        let keyboard_enhancement_pushed = supports_keyboard_enhancement().unwrap_or(false);
        if keyboard_enhancement_pushed {
            execute!(
                io::stdout(),
                PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
            )?;
        }

        Ok(Self {
            keyboard_enhancement_pushed,
        })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        // Best-effort at every step: this runs during panic unwinding too,
        // where an earlier failure shouldn't stop us from attempting the
        // rest -- it's the last chance to leave the terminal usable.
        if self.keyboard_enhancement_pushed {
            let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
        }
        let _ = execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen);
        let _ = disable_raw_mode();
    }
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Resolved before entering raw mode / the alternate screen, so a bad
    // `--cwd` prints a plain error to a normal terminal instead of inside
    // a TUI that then has to tear itself back down.
    let session_cwd = cli
        .cwd
        .canonicalize()
        .map_err(|err| anyhow::anyhow!("--cwd {:?} is not a valid directory: {err}", cli.cwd))?;
    if !session_cwd.is_dir() {
        anyhow::bail!("--cwd {:?} is not a directory", session_cwd);
    }

    // Reading the tiny local config is synchronous, but inference runs after
    // the first frame so the title can visibly show its braille progress
    // indicator instead of leaving the user at a blank terminal.
    let stored_project_name = project_naming::load_stored_project_name(&session_cwd);
    let project_name = stored_project_name.as_ref().ok().and_then(Clone::clone);
    let project_name_error = stored_project_name.err().map(|error| error.to_string());
    let should_infer_project_name = project_name.is_none() && project_name_error.is_none();

    // `TerminalGuard::drop` is what actually restores the terminal; it
    // runs whether `run` returns `Ok`, `Err`, or panics and unwinds
    // through this stack frame.
    let guard = TerminalGuard::enter()?;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let result = run(
        &mut terminal,
        session_cwd,
        project_name,
        project_name_error,
        should_infer_project_name,
    );

    // Restore the terminal before propagating any error, so the error
    // message prints to a normal terminal instead of a mangled
    // alternate-screen one.
    drop(guard);

    result
}

fn run(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    session_cwd: PathBuf,
    project_name: Option<String>,
    project_name_error: Option<String>,
    should_infer_project_name: bool,
) -> anyhow::Result<()> {
    // The terminal exists before any child PTY is spawned, so derive the
    // pane's real content size first. Passing it into App lets recovered
    // full-screen programs initialize at the correct geometry instead of
    // observing the temporary 24x80 fallback and retaining that layout.
    let initial_size = terminal.size()?;
    let initial_layout =
        UiLayout::from_screen_area(Rect::new(0, 0, initial_size.width, initial_size.height));
    let (initial_pane_rows, initial_pane_cols) = initial_layout.pane_content_size();
    let mut app = App::new_with_project_name_and_pane_size(
        session_cwd,
        project_name,
        initial_pane_rows,
        initial_pane_cols,
    )?;
    if let Some(error) = project_name_error {
        app.status_message = Some(format!("Could not infer project name: {error}"));
    }
    let (mut project_name_receiver, mut project_name_worker) = if should_infer_project_name {
        let (sender, receiver) = mpsc::channel();
        let cwd = app.session_cwd.clone();
        let worker = std::thread::spawn(move || {
            let result =
                project_naming::bootstrap_project_name(&cwd, &KiloGatewayClient::default());
            let _ = sender.send(result);
        });
        app.is_project_name_loading = true;
        (Some(receiver), Some(worker))
    } else {
        (None, None)
    };

    // Size the initial pane to its real on-screen content box now that we
    // know the real terminal size, rather than the arbitrary default
    // `App::new` spawned it with -- and use the *pane content* area (past
    // the tree column and the pane's own border), not the full terminal,
    // so the shell inside never believes it has more columns than its box
    // actually shows.
    app.set_layout(initial_layout);

    let mut system = System::new_all();
    // Resolved once rather than per worker spawn -- home doesn't change
    // mid-session, and a title-inference worker can't do anything useful
    // without it (see `spawn_title_inference_workers`).
    let home_dir = directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf());
    let mut title_workers: HashMap<NodeId, TitleWorker> = HashMap::new();

    while !app.should_quit {
        poll_project_name_worker(
            &mut app,
            &mut project_name_receiver,
            &mut project_name_worker,
        );
        poll_title_workers(&mut app, &mut title_workers);
        terminal.draw(|frame| ui::draw(frame, &mut app))?;

        if event::poll(POLL_INTERVAL)? {
            let event = event::read()?;
            match &event {
                Event::Resize(cols, rows) => {
                    app.set_layout(UiLayout::from_screen_area(Rect::new(0, 0, *cols, *rows)));
                }
                _ => {
                    if let Err(err) = app.handle_event(event) {
                        app.status_message = Some(format!("Error: {err}"));
                    }
                }
            }
        }

        if app.last_detect_tick.elapsed() > DETECTION_INTERVAL {
            app.tick_detection(&mut system);
            app.last_detect_tick = Instant::now();
            if let Some(home) = &home_dir {
                spawn_title_inference_workers(&mut app, home, &mut title_workers);
            }
        }

        app.tick_autosave();
    }

    Ok(())
}

/// Moves a finished inference result into application state without ever
/// blocking the event loop; the title keeps spinning while the channel is
/// empty. The completed worker is joined immediately to retain ownership and
/// surface no leaked task handles.
fn poll_project_name_worker(
    app: &mut App,
    receiver: &mut Option<Receiver<anyhow::Result<project_naming::ProjectNameBootstrap>>>,
    worker: &mut Option<JoinHandle<()>>,
) {
    let Some(active_receiver) = receiver.as_ref() else {
        return;
    };
    let result = match active_receiver.try_recv() {
        Ok(result) => result,
        Err(TryRecvError::Empty) => return,
        Err(TryRecvError::Disconnected) => Err(anyhow::anyhow!(
            "project-name worker stopped before returning a result"
        )),
    };

    app.is_project_name_loading = false;
    *receiver = None;
    if let Some(completed_worker) = worker.take() {
        let _ = completed_worker.join();
    }
    match result {
        Ok(bootstrap) => app.project_name = Some(bootstrap.project_name),
        Err(error) => app.status_message = Some(format!("Could not infer project name: {error}")),
    }
}

/// One in-flight `session_naming::infer_pane_title` worker, keyed by the
/// pane it's inferring a title for. Unlike the single project-name worker,
/// several of these can run concurrently -- one per agent pane whose
/// session ID just became known -- so they're tracked in a map rather than
/// a pair of `Option`s.
struct TitleWorker {
    receiver: Receiver<anyhow::Result<String>>,
    handle: JoinHandle<()>,
}

/// Spawns one worker per pane `App::panes_needing_title_inference` reports,
/// and immediately marks each as loading so `tick_detection`'s next pass
/// (and the tree's spinner) both see it as already in flight.
fn spawn_title_inference_workers(
    app: &mut App,
    home: &Path,
    workers: &mut HashMap<NodeId, TitleWorker>,
) {
    for pending in app.panes_needing_title_inference() {
        let (sender, receiver) = mpsc::channel();
        let home = home.to_path_buf();
        let cwd = app.session_cwd.clone();
        let handle = std::thread::spawn(move || {
            let result = session_naming::infer_pane_title(
                &KiloGatewayClient::default(),
                &home,
                &cwd,
                &pending.agent_class,
                &pending.session_id,
            );
            let _ = sender.send(result);
        });
        app.mark_title_inference_started(pending.pane_id);
        workers.insert(pending.pane_id, TitleWorker { receiver, handle });
    }
}

/// Moves every finished title-inference result into application state,
/// same non-blocking pattern as `poll_project_name_worker` generalized to
/// a map of concurrent workers.
fn poll_title_workers(app: &mut App, workers: &mut HashMap<NodeId, TitleWorker>) {
    let mut completed = Vec::new();
    for (&pane_id, worker) in workers.iter() {
        match worker.receiver.try_recv() {
            Ok(result) => completed.push((pane_id, result)),
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => completed.push((
                pane_id,
                Err(anyhow::anyhow!(
                    "session-title worker stopped before returning a result"
                )),
            )),
        }
    }

    for (pane_id, result) in completed {
        if let Some(worker) = workers.remove(&pane_id) {
            let _ = worker.handle.join();
        }
        match result {
            Ok(title) => app.apply_inferred_title(pane_id, title),
            Err(error) => app.fail_title_inference(pane_id, &error.to_string()),
        }
    }
}
