# CLAUDE.md — illium

Project-specific rules. This is a Rust project; the global `~/.claude/CLAUDE.md` stack/style section (TypeScript/Bun/Vue) does not apply here — its behavioral rules (scope discipline, verification gate, explore-before-acting, no worktrees, architecture-is-mandatory, no stopping to ask "should I continue") still apply in full. See `README.md` for the product design, architecture, crate choices, and milestone plan — read it before touching code.

## Workspace layout

Cargo workspace, one crate per architectural layer (see README "Architecture"). Do not collapse layers back into a single crate for convenience — the boundaries exist so `illium-core` and `illium-detect` stay unit-testable without a PTY, a terminal, or a running server.

```
illium/
├── Cargo.toml              # workspace root
├── illium-core/            # domain: Session/Group/Pane tree, no I/O
├── illium-pty/             # adapter: portable-pty + vt100
├── illium-detect/          # agent identity + activity classification
├── illium-server/          # owns PTYs + tree, IPC server, detection loop
├── illium-client/          # ratatui TUI, thin renderer over IPC
├── illium-ipc/             # shared request/response types, wire (de)serialization
└── src/main.rs (or illium-cli/) # clap entrypoint, talks to illium-client + illium-server lifecycle
```

`illium-ipc` holds the message types both `illium-server` and `illium-client` depend on — never let the client reach into server-internal types directly, and never let `illium-core` depend on `illium-ipc` (core is pure domain, ipc is transport).

## Layering rules (non-negotiable)

- `illium-core` has zero I/O and zero async. Tree mutations are plain functions/methods returning `Result`. If you find yourself wanting `tokio` or a file handle in this crate, the logic belongs in `illium-server` instead.
- `illium-detect` takes a `&str`/screen snapshot and a process list in, returns a classification out. No PTY access, no direct `sysinfo` polling loop inside it — the poll *loop* (timing, scheduling, adaptive backoff) lives in `illium-server`; `illium-detect` is the pure classification function the loop calls.
- `illium-pty` never knows about the tree or about agent detection. It exposes "spawn a command, get a handle to write/resize/read screen state." That's the whole contract.
- `illium-client` never touches `portable-pty`, `vt100`, or `sysinfo` directly. It renders what the server sends over IPC and sends back user input/commands. If the client needs something the IPC protocol doesn't carry yet, extend the protocol — don't reach around it.
- New agent CLI support (a new entry for Claude Code/Codex-style detection) is a new `AgentSignature` entry in the registry table in `illium-detect`, not a new branch in an if/else chain. If adding one requires touching more than the registry + its test, the registry abstraction has drifted — fix the abstraction, not the call site.

## Rust conventions

- `snake_case` for functions/vars/modules, `PascalCase` for types/traits/enums, `SCREAMING_SNAKE_CASE` for consts — standard Rust, matches the user's general naming preference already.
- Full descriptive names. `pane_id` not `pid` (that abbreviation collides with OS process ID anyway, which this codebase also deals with — never reuse `pid` for anything except an actual OS PID).
- `Result<T, E>` everywhere fallible; no `.unwrap()`/`.expect()` outside tests and truly-cannot-fail invariants (and comment the invariant when you do). No panics as flow control.
- Prefer `thiserror` for typed error enums per crate; `illium-server`'s top-level error boundary logs and continues (a single pane's detection failure or PTY hiccup must never take the whole server down — other panes keep running).
- Every `async` task spawned (PTY reader, detection-loop tick, IPC connection handler) must have a clear owner that can cancel it. Use `tokio::task::JoinHandle` tracking, not fire-and-forget `tokio::spawn` with no handle kept anywhere — a pane that's closed must have its reader/detection tasks actually stop, not leak.
- Run `cargo clippy --workspace --all-targets` and `cargo fmt --check` before considering any change done. Treat new clippy warnings as things to fix, not suppress with `#[allow]`, unless there's a specific documented reason.

## Testing

- `illium-core` and `illium-detect`: plain `#[test]` unit tests, no I/O, run in milliseconds. This is where most test coverage should live, since these are the crates with zero external dependencies to fake.
- `illium-detect` test fixtures: store captured real screen text (a handful of representative `vt100` screen dumps — Claude Code mid-turn, Claude Code idle, Claude Code awaiting approval, Codex equivalents, a plain shell prompt) as fixture files under `illium-detect/tests/fixtures/`, and assert classification against each. When Claude Code/Codex change their UI and a fixture's expected classification starts failing, that's a real signal to update the signature registry — treat it as a bug report, not a flaky test to loosen.
- `illium-pty`: integration-level tests that actually spawn a PTY and a trivial command (`echo`, `cat`) are fine here — this crate's whole job is talking to the OS, so faking that away would test nothing real.
- `illium-server` IPC protocol: round-trip (de)serialize every message type; a client/server version mismatch should fail loudly, not silently misparse.
- No test should depend on a real `claude` or `codex` binary being installed — detection tests run against captured fixture text, never by shelling out to the real CLI.

## Config & data locations

Use the `directories` crate, never hardcode `~`:

- Config: `directories::ProjectDirs::from("", "", "illium").config_dir()` → `~/.config/illium/config.toml` on Linux.
- Data/sockets/session snapshots: same `ProjectDirs` `.data_dir()` → `~/.local/share/illium/`.
- One UDS socket per session (`<data_dir>/<session_name>.sock`), matching tmux's per-session-socket model described in the README — do not multiplex all sessions over one socket.

## Scope reminders specific to this project

- This is a new project with no users yet — no backwards-compatibility shims, no feature flags, no "v2" anything. Change things directly.
- Don't build the WASM plugin system, remote/SSH sharing, or agent-driving/SDK surface — see README "Non-goals." If a task seems to need one of those, stop and flag it rather than building toward it.
- Milestones in the README (M0–M5) are meant to be built in order — each is independently runnable. Don't jump ahead to agent detection (M4) before the tree model and pane rendering (M1) actually work end to end.
<!-- NEXUS:START -->
## Nexus Memory Substrate
- Identity: [Soul](/home/developer/.config/nexus/soul.md)
- Project Context: [Project Context](/home/developer/dev/ai/illium/.nexus/context.md)
<!-- NEXUS:END -->
