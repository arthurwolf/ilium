# CLAUDE.md — ilium

Project-specific rules. This is a Rust project; the global `~/.claude/CLAUDE.md` stack/style section (TypeScript/Bun/Vue) does not apply here — its behavioral rules (scope discipline, verification gate, explore-before-acting, no worktrees, architecture-is-mandatory, no stopping to ask "should I continue") still apply in full. See `ARCHITECTURE.md` for the product design, architecture, crate choices, and milestone plan — read it before touching code. `README.md` is the short user-facing overview and `src/docs/` holds the full user documentation; keep both free of internal design history.

Documentation lives at the repository root: `README.md` (short user overview), `src/docs/` (full user documentation, tracked), `ARCHITECTURE.md` (design), `CLAUDE.md` (these rules). `docs/` is deliberately git-ignored — it is a scratch area for working notes and plans, so never put anything there that a fresh clone needs.

## Ilium process custody

Never restart Ilium or its running server, session, or panes. Do not use `--restart-server`, stop or replace the live server, or restart it indirectly through a service. The user alone restarts Ilium. After a code change, build and deploy the binaries, verify the installed artifact, and report clearly when an already-running server still has the old executable loaded.

## Workspace layout

Cargo workspace, one crate per architectural layer (see ARCHITECTURE.md "Crate roles"). Do not collapse layers back into a single crate for convenience — the boundaries exist so `ilium-core` and `ilium-detect` stay unit-testable without a PTY, a terminal, or a running server.

```
ilium/                     # workspace root
├── Cargo.toml               # workspace root manifest
├── ilium-core/              # domain: Tree of Node/NodeKind (Group|Pane), no I/O
│   └── src/lib.rs             # single-file crate: Tree, Node, NodeId, NodeKind, PaneStatus, PaneContentKind, AgentClass, AgentActivity
├── ilium-pty/               # adapter: portable-pty + vt100 + xterm mouse-protocol encoding
│   └── src/{session,mouse,query,error}.rs
├── ilium-detect/            # agent identity + activity classification
│   └── src/lib.rs             # single-file crate: AGENT_SIGNATURES registry, identify_agent, classify_activity
├── ilium-ipc/                # shared request/event types, wire (de)serialization
│   └── src/{protocol,framing,error}.rs
├── ilium-platform/          # adapter: every OS-specific decision, one place
│   └── src/{secure_fs,file_lock,runtime_dir,process_info,process_control,detached,thread_priority}.rs
├── ilium-transport/         # adapter: the local client/server channel --
│   └── src/{endpoint,stream,unix,windows}.rs   # UDS on Unix, named pipe on Windows
├── ilium-ambient/           # pure ambient-background scene engines (pipes, stars, night lights, clouds, video, spectrum, images); no ratatui, hosted by ilium-client
│   └── src/{scene,control,registry,raster,source,location,geocode,worldmap,debug}.rs, scenes/*
├── ilium-kilo-gateway/      # adapter: Kilo Gateway (OpenAI-compatible) HTTP client, used only by ilium-client's background naming workers
│   └── src/lib.rs
├── ilium-server/            # owns PTYs + tree, IPC server, adaptive detection loop -- one process per session
│   ├── src/main.rs            # daemon entrypoint: resolves real paths (paths.rs), calls lib.rs's run()
│   ├── src/lib.rs              # run(): binds the UDS listener, owns the top-level select! loop
│   └── src/{config,detection,error,mouse,notifications,pane,paths,persistence,state}.rs, src/ipc/{mod,connection,handlers}.rs
├── ilium-client/             # ratatui TUI, thin renderer over ilium-ipc (see its module map in src/lib.rs)
│   ├── src/lib.rs              # module map + run(): terminal lifecycle, connects, drives the event loop
│   └── src/{app,config,connection,render_cache,keys,mouse,tick,naming_workers}.rs, plus presentation/local-file-I/O
│       modules (ui, tree_ui, modal, help, theme, layout, editor_*, markdown/, naming*, session_*, project_*, ...)
│       that don't care whether their data came from a local Tree or a render-cache mirror of one
└── ilium/                    # the `ilium` bin: clap entrypoint, spawns ilium-server as a detached
    └── src/{main,session,error}.rs   # process and hands off to ilium_client::run for the TUI
```

`ilium-platform` owns every operating-system decision (owner-only files, locks, runtime
directories, process inspection and control, detached spawn, worker thread priority). No other
crate may reach for `std::os::unix`, `libc`, `windows-sys`, or procfs directly -- if something is
missing, add it there rather than growing a `#[cfg]` branch at the call site. `ilium-transport`
owns *where* IPC bytes travel, which is the other genuinely per-platform concern; `ilium-ipc` stays
generic over any async byte stream and must not learn about sockets or pipes.

`ilium-ipc` holds the message types both `ilium-server` and `ilium-client` depend on — never let the client reach into server-internal types directly, and never let `ilium-core` depend on `ilium-ipc` (core is pure domain, ipc is transport). `ilium-kilo-gateway` is a narrow adapter crate of its own (an LLM HTTP client, not part of the mux/detection architecture); only `ilium-client` depends on it.

## Layering rules (non-negotiable)

- `ilium-core` has zero I/O and zero async. Tree mutations are plain functions/methods returning `Result`. If you find yourself wanting `tokio` or a file handle in this crate, the logic belongs in `ilium-server` instead.
- `ilium-detect` takes a `&str`/screen snapshot and a process list in, returns a classification out. No PTY access, no direct `sysinfo` polling loop inside it — the poll *loop* (timing, scheduling, adaptive backoff) lives in `ilium-server`; `ilium-detect` is the pure classification function the loop calls.
- `ilium-pty` never knows about the tree or about agent detection. It exposes "spawn a command, get a handle to write/resize/read screen state." That's the whole contract.
- `ilium-client` never touches `portable-pty`, `vt100`, or `sysinfo` directly. It renders what the server sends over IPC and sends back user input/commands. If the client needs something the IPC protocol doesn't carry yet, extend the protocol — don't reach around it.
- New agent CLI support (a new entry for Claude Code/Codex-style detection) is a new `AgentSignature` entry in the registry table in `ilium-detect`, not a new branch in an if/else chain. If adding one requires touching more than the registry + its test, the registry abstraction has drifted — fix the abstraction, not the call site.

## Rust conventions

- `snake_case` for functions/vars/modules, `PascalCase` for types/traits/enums, `SCREAMING_SNAKE_CASE` for consts — standard Rust, matches the user's general naming preference already.
- Full descriptive names. `pane_id` not `pid` (that abbreviation collides with OS process ID anyway, which this codebase also deals with — never reuse `pid` for anything except an actual OS PID).
- `Result<T, E>` everywhere fallible; no `.unwrap()`/`.expect()` outside tests and truly-cannot-fail invariants (and comment the invariant when you do). No panics as flow control.
- Prefer `thiserror` for typed error enums per crate; `ilium-server`'s top-level error boundary logs and continues (a single pane's detection failure or PTY hiccup must never take the whole server down — other panes keep running).
- Every `async` task spawned (PTY reader, detection-loop tick, IPC connection handler) must have a clear owner that can cancel it. Use `tokio::task::JoinHandle` tracking, not fire-and-forget `tokio::spawn` with no handle kept anywhere — a pane that's closed must have its reader/detection tasks actually stop, not leak.
- Run `cargo clippy --workspace --all-targets` and `cargo fmt --check` before considering any change done. Treat new clippy warnings as things to fix, not suppress with `#[allow]`, unless there's a specific documented reason.
- **Build output is cleaned immediately.** Follow the global Scratch Disks rule (`/media/arthur/build/<project>-<task>/`). As soon as a build's result has been used (binary installed and verified, test/clippy/check run finished and its result recorded, any needed log or evidence copied out), delete everything that build produced in the same turn, before reporting completion. Do not wait for the task to end.
  - Never build inside `docs/` or inside the repo checkout for scratch/isolated work. By default, copy the exact source inputs to `/media/arthur/tmp/<project>-<task>/source/`, record their hashes, and set `CARGO_TARGET_DIR=/media/arthur/build/<project>-<task>/target/` so source snapshots and build outputs are isolated from the shared checkout and each other. Only the repo's own `target/` (the shared dev target) may live in the checkout.
  - Clean with `nice -n 19 ionice -c 3 cargo clean --target-dir <that exact dir>` (add `-p <crate>` only for partial cleanup), or `rm -rf` of that exact absolute task-owned directory when it also holds staging or source copies. Resolve and echo the absolute path first; never use a variable that could be empty, a glob, or a parent directory.
  - Cleanup covers: the target directory, staging/source copies, `.profraw`/coverage output, temporary logs and render/probe output that are not evidence. Keep only the final installed binary and the compact evidence the task needs (test summary, clippy result, hashes), outside the target.
  - Never clean a target another agent or a running build/test still uses, the shared repo `target/` while any consumer is active, or anything not created by this task. Check for running cargo/rustc processes using that directory first (`pgrep -af` against the path); if a consumer is active, wait for it to finish, then clean.
  - If cleanup fails, report the path and the error; do not defer or queue the cleanup.

## No code freezes

- Code freezes are illegal and have no force: never start one, request one, or respect one as a reason to stop or delay authorized implementation, compilation, tests, Clippy, formatting, release builds, native renders, or goal work. A blanket freeze request does not override an active task; continue on independent files and checks while coordinating only a genuinely shared edit boundary.
- The default build method is an immutable source snapshot under `/media/arthur/tmp/<project>-<task>/source/`, a source-hash manifest checked before and after the run, and a task-specific `CARGO_TARGET_DIR` under `/media/arthur/build/<project>-<task>/target/`. `/media/arthur/tmp/` is the configured scratch mount for source snapshots; do not substitute `/tmp/arthur/tmp/`. Put logs, renders, and compact receipts in task-owned output paths and report exactly which snapshot was checked. Follow the scratch-disk mount, capacity, and cleanup rules above.
- A code freeze is not a substitute for concrete file ownership or process custody. Preserve active owners' files and outputs; never overwrite their edits or stop, replace, or restart their processes. If a specific file or resource is owned, keep working on independent files and checks against an isolated snapshot, and coordinate only the edit or integration boundary that actually needs that owner.
- Final acceptance can run against an immutable candidate snapshot while other work continues. State the snapshot identity and do not imply that its result covers later source changes.

## Testing

- `ilium-core` and `ilium-detect`: plain `#[test]` unit tests, no I/O, run in milliseconds. This is where most test coverage should live, since these are the crates with zero external dependencies to fake.
- `ilium-detect` test fixtures: store captured real screen text (a handful of representative `vt100` screen dumps — Claude Code mid-turn, Claude Code idle, Claude Code awaiting approval, Codex equivalents, a plain shell prompt) as fixture files under `ilium-detect/tests/fixtures/`, and assert classification against each. When Claude Code/Codex change their UI and a fixture's expected classification starts failing, that's a real signal to update the signature registry — treat it as a bug report, not a flaky test to loosen.
- `ilium-pty`: integration-level tests that actually spawn a PTY and a trivial command (`echo`, `cat`) are fine here — this crate's whole job is talking to the OS, so faking that away would test nothing real.
- `ilium-server` IPC protocol: round-trip (de)serialize every message type; a client/server version mismatch should fail loudly, not silently misparse.
- No test should depend on a real `claude` or `codex` binary being installed — detection tests run against captured fixture text, never by shelling out to the real CLI.

### What's automated vs. what still needs a human

Every crate has unit or integration tests exercising it directly, per the per-crate policy above. End-to-end tests cover both a real PTY-rendered TUI and a live agent-CLI process through detection:

- `ilium/tests/pty_tui_smoke.rs` drives the real `ilium` binary under a genuine PTY (`ilium-pty`, not `std::process::Command` with inherited stdio). In addition to attach/help/settings coverage, it creates two live terminal panes, moves them into a vertical split through the real dialogs, asserts both viewport streams render together, focuses each child, and verifies distinct input reaches each PTY while both remain visible.
- `ilium-server/tests/live_agent_detection.rs` spawns a fake `codex`-named script by absolute path (never `PATH`, to avoid ever racing a real `codex` install), drives a real `ilium-server` end to end — real `sysinfo` process-tree walk finds it, `ilium-pty`'s live `vt100` feed is scraped, `ilium_detect::classify_activity` runs unmodified — and asserts Codex's visible `Pursuing goal (5m)` status reaches the client as `AgentWithGoal`, followed by the real `Working -> Idle`/`Done` transition and exactly one queued sound through an injected silent recorder.

Genuinely still manual: whether the rendered output actually looks right on a real terminal emulator — font rendering, color contrast, the drag-and-drop/animation "feel." These tests assert structural/textual correctness (the right text lands on the right row at the right time), not visual quality, which has no meaningful automated oracle.

## Config & data locations

Use the `directories` crate, never hardcode `~`:

- Config: `directories::ProjectDirs::from("", "", "ilium").config_dir()` → `~/.config/ilium/config.toml` on Linux.
- Session snapshots: `<project>/.ilium/sessions/<session_name>.json`. The canonical launch directory is the project boundary; a bare `ilium` owns that directory's `default` session.
- One UDS socket per project session under `$XDG_RUNTIME_DIR/ilium/` (or the OS temp dir when no runtime directory exists). `ilium/src/session.rs` creates a Claude-style readable path slug plus a digest of the canonical project path, so socket identity cannot collide or exceed `sockaddr_un` limits. The CLI passes that exact path to both detached server and client.

## Controlled tmux session

- GUI automation must run only through tmux sessions we create and control. Do not use `xdotool` or screenshot-driven desktop automation.

- `PROJECT=/absolute/project; STATE=$(mktemp -d /media/arthur/tmp/is.XXXXXX); RUNTIME=$(mktemp -d /media/arthur/tmp/ir.XXXXXX); TMUX_SERVER=ilium-ctl; TMUX_SESSION=ilium-ctl`
- Launch an isolated server/session: `tmux -L "$TMUX_SERVER" new-session -d -s "$TMUX_SESSION" "env XDG_DATA_HOME=$STATE/data XDG_CONFIG_HOME=$STATE/config XDG_RUNTIME_DIR=$RUNTIME $HOME/.local/bin/ilium --cwd $PROJECT"`.
- Remotely control the live TUI: `tmux -L "$TMUX_SERVER" attach-session -t "$TMUX_SESSION"`; use normal ilium keys, then detach with tmux `Ctrl+B d`.
- Inspect its rendered terminal without attaching: `tmux -L "$TMUX_SERVER" capture-pane -e -p -t "$TMUX_SESSION:0.0"`.
- Stop cleanly: `env XDG_DATA_HOME="$STATE/data" XDG_CONFIG_HOME="$STATE/config" XDG_RUNTIME_DIR="$RUNTIME" "$HOME/.local/bin/ilium" --cwd "$PROJECT" kill-session default`.
- Finish isolation: `tmux -L "$TMUX_SERVER" kill-server; rm -rf "$STATE" "$RUNTIME"`; keep `/media/arthur/tmp` paths short so the derived Unix socket fits.

## Scope reminders specific to this project

- This is a new project with no users yet — no backwards-compatibility shims, no feature flags, no "v2" anything. Change things directly.
- Don't build the WASM plugin system, remote/SSH sharing, or agent-driving/SDK surface — see ARCHITECTURE.md "Non-goals." If a task seems to need one of those, stop and flag it rather than building toward it.
- Milestones in ARCHITECTURE.md (M0–M5) are meant to be built in order. M0–M4 are done; M5 (config surface, snapshot-restore-on-boot, notifications) is partially done — read ARCHITECTURE.md "Implementation plan" for exactly what M5 covers (custom detection signatures, keybinding remap, a four-color theme override, snapshot respawn-on-boot, desktop notifications) and what it still deliberately leaves out (resuming an agent CLI's own session on restore; theming beyond the four colors listed) before assuming a piece of it already exists.

## Background animations: the shared look is mandatory

Every background animation (built-in, hosted `ilium-ambient` scene, Wikipedia, and every future one) must get the shared look and pattern controls for free, never by re-implementing them:

- **Colour is global, not per scene.** `ilium_ambient::style::Appearance` (one value on `AnimationSettings`) owns colour mode (Color / Greyscale / Monotone), palette (38+ presets incl. pastel), colour source, reverse/shift/spread, brightness, contrast, gamma, colour intensity, hue shift, invert, edge fade, grey tint, style presets, and the pattern contrast/invert. The client applies it per cell after dithering (`background_composition::LookPaint`). A scene draws tone into the raster and, only if it has natural colours, per-cell colours; it must not add its own brightness/contrast/hue/palette/saturation controls. Scene-internal colour choices that are the scene's content (a map's land/sea colours, a game's team colours) are allowed; they are then recoloured through the shared look.
- **Style presets and colour filters are global too.** `StylePreset` (50+ ready-made looks) and `style_filters::Filter` (46 true colour transforms: gels, sepia, duotones, thermal, night vision, channel swaps...) live in the shared `Appearance`; add new ones in `ilium-ambient/src/style.rs` / `style_filters.rs`, never in a scene.
- **The palette is passed to every scene.** `SceneEnv::palette` (a `ScenePalette`, empty unless Color mode uses a real palette) is available in every scene constructor and `Scene::set_palette` delivers later changes. Scenes with natural colours should shift them with `ScenePalette::recolor`/`at`; monochrome scenes may ignore it. Every scene constructor carries a `PALETTE (future plugin contract)` note: plugins will receive the current palette in their constructor and must follow it. Keep that note on new scenes. `AmbientSettings::create_scene` wraps every scene in `PaletteScene`, which shifts colour scenes' cell colours onto the palette by brightness; a scene may do better natively.
- **Dithering is global.** Every `DitherMode` (matrix and error-diffusion) is available to every scene through the shared Dither row; add new algorithms in `ilium-ambient/src/dither.rs`, never inside a scene.
- **Display is global.** Panel placement (both / left / right), frame-rate cap, speed, density and background on/off are shared rows, not scene settings.
- **Scene settings are only what is specific to the scene.** If something can reasonably be configured and is not, add it as a scene control (`Control` rows, with range clamping in `normalized`, persistence, a help topic and a test).
- **A new animation is not done until** it appears in the Settings list, the shared rows apply to it (the row-model tests cover every `AnimationKind`), its scene controls round-trip through `set_control`, its help topic exists, and `README.md`/`ARCHITECTURE.md` mention it. New shared controls get a help id in `animation_rows::STYLE_HELP_IDS` and a topic in `settings_help/catalog/topics.json`.

## Icon rendering

- Never "fix" an icon rendering or width issue by replacing the normal UTF-8 icons with plain stable glyphs. Diagnose and correct the rendering, cell-width, or diff behavior while keeping normal icons as the default. A stable-glyph mode may exist only as an explicit, opt-in user preference.

<!-- ilium-agent-feature: chatroom -->
If `CHATROOM.md` exists in the project root, read recent coordination with `ilium chat context --limit 40` when beginning work and before changing shared areas. Use `ilium chat send --message "..."` only for a task claim or release, blocker, dependency, material discovery or decision, or a handoff; do not post routine progress narration. Never rewrite `CHATROOM.md` directly.
<!-- /ilium-agent-feature: chatroom -->

<!-- ilium-agent-feature: progress version=6 -->
For every task expected to take at least three minutes inside an Ilium pane, you MUST use the Ilium progress-monitor lifecycle. Start the task first and verify that its process or job is alive. Then construct a cheap absolute-path probe which prints exactly one JSON object containing a stable non-empty `job_id`, a `status` of `not-started-yet`, `running`, `error`, or `done`, a finite `percent` from 0 through 100, a one-line `message`, an optional multi-line `details`, and an `error` string when status is `error`. The probe runs from the Ilium server's project root and receives no pane-shell aliases or transient environment, so use absolute paths or an explicit absolute `cd`.

The human reading the progress bar has NOT read your session and usually never will; they glance at many panes. Write `message` and `details` for a stranger who knows nothing except what these two texts say:
- `message` (one line, at most about 110 characters) is the compact description. It MUST stand alone: say in plain words WHAT is being done and to WHICH thing (project, feature, file or user-visible outcome), then the current step with a measurable count when one exists, e.g. `Building the release binaries of the ilium terminal app - compiling crate 14 of 22`. On `done` or `error` it states the outcome (what finished, or what failed and what was affected).
- `details` (3 to 8 short lines, `\n`-separated) is the long description, shown when the human hovers. Use these labelled lines, skipping one only when truly empty: `What:` the task in plain words. `Why:` the reason or request behind it, in terms the human recognises. `Now:` the current step, and what `percent` measures. `Next:` what happens when it finishes, including what you will do and whether you need anything from the human. `Watch:` anything that can go wrong or that you are waiting on.
- Never rely on session context: no unexplained internal names, ticket or phase numbers, abbreviations, pronouns such as `it` or `this`, or phrases such as `as discussed`, `the fix`, `the usual`, `native env`, `modal paste`. If a term from the work must appear, define it in the same text. Name the concrete thing instead of a label you invented.
- Test before registering: if a person who read only `message` could not say what is running and why it matters, rewrite it. Update both texts on every probe call so they track the real current step; stale text is worse than none.

You MUST run `ilium progress check --command '<probe>'` and confirm its JSONL validation result before registration. Then run `ilium progress set --command '<probe>' --interval-seconds <n>` and wait for Ilium's positive JSONL registration acknowledgement containing the monitor ID and accepted first report. After registration, the agent MUST NOT poll in any form: do not make repeated tool calls, run checking loops, sleep then recheck, repeatedly inspect logs or files, issue recurring status commands, or spend conversational turns checking progress. Ilium's detached server is the sole recurring poller, and it will send you a message when the task reaches `done` or `error`. You MAY perform other useful work that does not poll the task.

Do not manually clear a terminal result before handling its notification. Retain the task identity, final process exit evidence, progress evidence, and failure details for verification; use `ilium progress clear --monitor-id <id>` only after the lifecycle is complete or when explicitly cancelling it.

<!-- /ilium-agent-feature: progress -->
