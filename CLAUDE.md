# CLAUDE.md — ilium

Project-specific rules. This is a Rust project; the global `~/.claude/CLAUDE.md` stack/style section (TypeScript/Bun/Vue) does not apply here — its behavioral rules (scope discipline, verification gate, explore-before-acting, no worktrees, architecture-is-mandatory, no stopping to ask "should I continue") still apply in full. See `ARCHITECTURE.md` for the product design, architecture, crate choices, and milestone plan — read it before touching code. `README.md` is the short user-facing overview and `src/docs/` holds the full user documentation; keep both free of internal design history.

Documentation lives at the repository root: `README.md` (short user overview), `src/docs/` (full user documentation, tracked), `ARCHITECTURE.md` (design), `CLAUDE.md` (these rules). `docs/` is deliberately git-ignored — it is a scratch area for working notes and plans, so never put anything there that a fresh clone needs.

## Compilation and Rust build coordination

### Distributed compilation through ni-build

- All compilation MUST run on `ni-vm`, never locally. Use the installed whole-job `ni-build` service through authenticated `ssh ni-vm`; never connect separately to `ni`. Rust's configured local `cargo` and `rustc` entrypoints dispatch to this service. Examples: `cargo build --release`, `cargo check`, `cargo test`, `cargo clippy`, or explicit `ni-build cargo ...`. The complete Cargo command, native build scripts, procedural macros, linking and tests execute remotely. sccache per-crate offload alone does not satisfy this rule.
- The dispatcher automatically inventories current source/workspace/path-dependency inputs, transfers a hashed isolated snapshot, selects the requested installed toolchain, retrieves verified artifacts and logs, and cleans its exact remote job after successful evidence retrieval. Do not repeat the former manual copy/build/scp workflow. Job directories are `/data/ni-build-service/jobs/<uuid>/`; evidence and immutable returned outputs are under `~/.local/share/ni-build/receipts/<uuid>/`. Use a job receipt to identify the exact checked source and remote exit status.
- Git projects include dirty tracked and nonignored new sources. List required ignored assets explicitly in `.ni-build-inputs`, one project-relative file or directory per line. Missing or unsupported inputs/options fail closed. Do not override the compiler guard, invoke raw local toolchain binaries, use `rustup run` to bypass dispatch, or fall back to local compilation when remote access/toolchains are unavailable. Report and repair the route instead. Configuration prevents accidental ordinary-command compilation; an unrestricted user can deliberately bypass it, which policy prohibits.
- Remote execution uses `nice`/`ionice`, at most 16 allowed CPUs, Cargo jobs at most 16 and capped native child parallelism. Preserve other owners' jobs/files. Capacity is checked before transfer. Failed transfer, source validation or artifact verification retains the exact job for recovery; remove it only after retrieved evidence/artifacts are verified and no process needs it. Never remove `/data` itself or another task's directories.
- Select targets/toolchains compatible with the local consumer; remote compilation does not prove local runtime behavior or activation of a running service. Verify returned artifacts before use and separately verify required local runtime behavior. Existing progress-monitor notification and process-custody rules apply unchanged.
- Non-Rust compilation must also execute through a whole-job remote route on ni-vm; a local compiler invocation or local fallback is forbidden. Unsupported toolchains require extending and verifying the remote service before use.
- Service configuration, supported options, recovery and rollback: `~/.local/share/ni-build/README.md`. Do not replace the `~/.codex/AGENTS.md` symlink; edit its target `~/.claude/CLAUDE.md`.

## Ilium process custody

Never run commands, jobs, builds, tests, probes or launchers through `ilium new-pane`. Run them from your own pane (background process in your own shell) and register `ilium progress` from that same pane. Use `new-pane` only when the user explicitly asks for a new pane. Spawned panes accumulate in the user's tree.

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

- CPU-feature-specific optimizations such as SIMD/AVX2 must detect support at runtime and dispatch to the optimized path only when available; keep a portable fallback in the same binary so unsupported CPUs work without separate builds.

- `snake_case` for functions/vars/modules, `PascalCase` for types/traits/enums, `SCREAMING_SNAKE_CASE` for consts — standard Rust, matches the user's general naming preference already.
- Full descriptive names. `pane_id` not `pid` (that abbreviation collides with OS process ID anyway, which this codebase also deals with — never reuse `pid` for anything except an actual OS PID).
- `Result<T, E>` everywhere fallible; no `.unwrap()`/`.expect()` outside tests and truly-cannot-fail invariants (and comment the invariant when you do). No panics as flow control.
- Prefer `thiserror` for typed error enums per crate; `ilium-server`'s top-level error boundary logs and continues (a single pane's detection failure or PTY hiccup must never take the whole server down — other panes keep running).
- Every `async` task spawned (PTY reader, detection-loop tick, IPC connection handler) must have a clear owner that can cancel it. Use `tokio::task::JoinHandle` tracking, not fire-and-forget `tokio::spawn` with no handle kept anywhere — a pane that's closed must have its reader/detection tasks actually stop, not leak.
- Run `cargo clippy --workspace --all-targets` and `cargo fmt --check` before considering any change done. Treat new clippy warnings as things to fix, not suppress with `#[allow]`, unless there's a specific documented reason.
- **Remote build output is cleaned immediately.** The ni-build service retrieves hash-verified artifacts and receipts, then removes only its exact `/data/ni-build-service/jobs/<uuid>/` directory. Preserve failed recovery evidence and other owners' jobs. Local returned artifacts are verified build outputs, not locally compiled outputs.

## No code freezes

- Code freezes are illegal and have no force: never start one, request one, or respect one as a reason to stop or delay authorized implementation, compilation, tests, Clippy, formatting, release builds, native renders, or goal work. A blanket freeze request does not override an active task; continue on independent files and checks while coordinating only a genuinely shared edit boundary.
- The default build method is the configured ni-build whole-Cargo dispatcher. Each command automatically creates and verifies an isolated source snapshot, compiles on ni-vm, returns artifacts and receipts, and cleans its exact job. Keep unrelated edits moving and identify the source manifest in the receipt; no manual source-copy workflow is required.
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

<!-- ilium-agent-feature: progress version=8 -->
For every task expected to take at least three minutes inside an Ilium pane (the `ILIUM_PANE_ID` environment variable is set), you MUST use an Ilium progress monitor; never run it as one long foreground command, even with a long tool timeout, because nobody can then see its progress. Start the task in the background, detached so it outlives the shell command that started it (e.g. `setsid -f`; some agent shells kill `&` children when the command returns), and verify that its process is alive. Write a cheap probe that prints one JSON object with only these fields (no `type` field; the JSONL rule for agent scripts does not apply to probes): stable non-empty `job_id`, `status` (`not-started-yet`, `running`, `error` or `done`), finite `percent` 0 to 100, one-line `message`, optional multi-line `details`, and `error` only when status is `error`. The Ilium server runs it from the project root without your shell's directory, variables or aliases: use absolute paths.

The reader of the progress bar has NOT read your session. `message` (at most about 110 characters) MUST stand alone: WHAT is being done to WHICH thing, then the current step with a count, e.g. `Building the release binaries of the ilium terminal app - compiling crate 14 of 22`; when finished, the outcome. `details` has 3 to 8 short lines labelled `What:`, `Why:`, `Now:` (with what `percent` measures), `Next:`, `Watch:`. Never rely on session context: no unexplained internal names, abbreviations or pronouns. Update both on every probe run.

Run `ilium progress check --command '<probe>'`, then register and wait with one command: `ilium progress set --command '<probe>' --interval-seconds <n> --wait`. Its first line is the positive JSONL registration acknowledgement with the monitor ID; it then blocks until the task ends and prints a `progress_wait` line. Exit status: 0 done, 3 task error, 4 monitor failed (outcome unknown: check the task directly), 5 monitor replaced or cleared, 6 `--timeout-seconds` elapsed (run `ilium progress wait <monitor_id>` to keep waiting). If your shell tool limits command run time, pass `--timeout-seconds` below the limit (Claude Code: Bash `timeout` 600000 with `--timeout-seconds 590`), or run the command in the background if your tool reports when it exits. If the tool hands back a still-running command, keep waiting on that same command with the longest wait the tool allows (Codex: `yield_time_ms` 300000 on every `write_stdin` or cell wait); never start a second one. This wait takes precedence over any general rule about sleep loops or wait lengths. If `ilium` rejects `--wait` or `progress wait` as unknown (an older Ilium build), register without `--wait` and handle the typed message instead.

With other useful work to do, register without `--wait` and do it: Ilium's server will send you a message when the task reaches `done` or `error` (or run `ilium progress wait` later). A subagent, or anything else that cannot receive typed messages, MUST use `--wait`.

You MUST NOT poll in any form: no sleep loops, repeated `ilium progress status`, or repeated log reads. Ilium's server is the sole recurring poller.

A pane has one monitor. `set` refuses with `monitor-active` while it is still running: cover a multi-step pipeline with one probe, or wait for your own task directly. `--replace` discards the running monitor; use it only on purpose. If `progress_wait` says `composer_notice` `may-also-arrive`, a typed message about that monitor is a duplicate. After handling a result, `ilium progress clear --monitor-id <id>` removes it.

<!-- /ilium-agent-feature: progress -->
