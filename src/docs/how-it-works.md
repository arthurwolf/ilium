# How Ilium works

This page gives a user-level overview of Ilium's moving parts: the per-project server, the attached clients, how they talk, how agents are detected, where configuration and data live, how snapshots work, and which crate does what. It is written for users who want to understand behaviour and for contributors who need a map. The full design reference is [ARCHITECTURE.md](../../ARCHITECTURE.md).

## Contents

- [Client and server](#client-and-server)
- [Sessions and projects](#sessions-and-projects)
- [The IPC channel](#the-ipc-channel)
- [Agent detection](#agent-detection)
- [Configuration and data locations](#configuration-and-data-locations)
- [Snapshots and recovery](#snapshots-and-recovery)
- [Background animations](#background-animations)
- [Diagnostics and logs](#diagnostics-and-logs)
- [Crate map](#crate-map)
- [Platform notes](#platform-notes)
- [Troubleshooting](#troubleshooting)

## Client and server

Ilium is a client/server program, like tmux and Zellij. That split is what makes detaching possible.

- One `ilium-server` process per project session owns the pane processes (shells, agent CLIs, editors, boards), the project tree, the detection loop and session state. Detaching leaves all of it running.
- One `ilium` client per attached terminal draws the interface and sends your input. It never touches PTYs or process lists directly; it renders what the server sends.
- The first `ilium` run in a project starts the server as a separate detached process, then attaches. Closing the terminal, or detaching, does not stop it. `ilium kill-session <name>` ends the session and all its panes (see [CLI reference](cli-reference.md)).

Because the server outlives clients, an old server keeps running after you install a new build. `ilium --restart-server` replaces the project's server while retaining its snapshot (see [Session recovery](session-recovery.md)).

## Sessions and projects

The project is the canonical (symlink-resolved) directory you launch from, or the one given with `--cwd`. A bare `ilium` owns that directory's `default` session. Named sessions are created with `ilium new-session <name>`, and `ilium ls` lists the project's known sessions and whether each is running. Every command that addresses a session is scoped to the canonical project directory, so two projects never share panes or snapshots.

## The IPC channel

Client and server talk over a local channel: a Unix domain socket on Linux and macOS, a named pipe on Windows. The socket sits under `$XDG_RUNTIME_DIR/ilium/` (or the OS temporary directory when there is no runtime directory) with a readable project slug plus a digest of the canonical project path, so identities cannot collide or exceed socket-path limits. The CLI passes that exact path to both server and client.

Messages are length-prefixed binary frames carrying request types from the client and event types from the server (screen updates, pane state, status changes). A client and server of mismatched versions fail loudly rather than misparsing.

## Agent detection

The server recognises agents with two independent signals.

1. Identity: which agent CLI is running. The server walks the pane's process tree and matches process names against a registry of built-in providers (Claude Code, Codex, Antigravity) plus generic and custom signatures such as `opencode` or `aider`. This is the primary signal, because it survives UI redesigns.
2. Activity: working, idle, waiting for approval, done. The visible terminal screen is scanned for markers: an interrupt hint, a live status line with an elapsed-time token, a yes/no confirmation line, or a numbered selection menu. A working pane that stops showing activity becomes done, which is an unread alert until you open the pane.

Polling is adaptive. Panes that are working or waiting for approval are checked about every 10 seconds; idle, done and plain-shell panes are checked about every 30 to 60 seconds. Intervals are configurable in `config.toml`. The detection result feeds the tree icons, the status bar, sounds and desktop notifications. See [Agent monitoring](agent-monitoring.md) and [Notifications](notifications.md).

## Configuration and data locations

| What | Location |
| --- | --- |
| Global settings | `~/.config/ilium/config.toml` on Linux (the platform configuration directory elsewhere, resolved by the `directories` crate); `[keyboard]` for prefixes, `[keybindings]` for action remapping, `[notifications]`, `[inference]`, `[cost.prices]`, `[debug]` and others |
| Animation settings (global) | `~/.config/ilium/animation/.ilium/config.yaml` |
| Session snapshots | `<project>/.ilium/sessions/<session_name>.json` |
| Rolling snapshot backups | `<project>/.ilium/backups/` |
| IPC socket | `$XDG_RUNTIME_DIR/ilium/<project-slug>-<digest>-<session>.sock` |
| Optional process log | `/tmp/.ilium/<project-session-id>/log-<start-time>.txt` |
| Installed versions (hosted installer) | `${XDG_DATA_HOME:-~/.local/share}/ilium` |

Add `.ilium/` to your `.gitignore` to keep session data out of Git. See [Settings](settings.md) for the settings UI that edits these files.

## Snapshots and recovery

The server writes a JSON snapshot of the session after structural changes (and after monitored-task lifecycle changes) to `.ilium/sessions/<name>.json`. After a server restart or machine reboot, Ilium restores the layout and relaunches pane programs. Where the provider's own data remains, it resumes verified Claude Code, Codex or Antigravity conversations. Unsaved process state does not survive. Backups exclude files edited inside pane applications.

| Flag | Effect |
| --- | --- |
| `--restart-server` | Replace the running server and keep the snapshot. |
| `--reset-session` | Delete this project's named session snapshot and start empty. Never affects another project. |
| `ilium kill-session <name>` | End the session and its panes. |

Full details: [Session recovery](session-recovery.md).

## Background animations

Ambient animations are drawn by scene engines hosted in the client. Built-in scenes run as Rust code; packaged TypeScript animations are bundled as JavaScript and executed by V8 in a confined helper process (Bubblewrap and cgroup-v2 limits on Linux). The helper is the `ilium-animation-helper` executable and the packages are `.iliumanim` files. See [Animations](animations.md).

## Diagnostics and logs

Debug file logging is off by default because enabling it records complete HTTP and LLM text requests, responses and errors (credential headers and URL parameters stay redacted; binary audio is summarised). The `[debug].file_logging_enabled` setting turns it on or off live for both client and server. Remove private content from logs before sharing them in a bug report.

## Crate map

Ilium is a Cargo workspace with one crate per architectural layer. The layering rules are strict: the pure domain and classification crates have no I/O so they are testable without a PTY or server.

| Crate | Role |
| --- | --- |
| `ilium` | The `ilium` binary: CLI parsing, spawns the detached server, hands off to the client. |
| `ilium-client` | The ratatui terminal UI: tree panel, panes, settings, editors, boards, naming workers. |
| `ilium-server` | Owns PTYs and the tree, the IPC server, the detection loop, snapshots, notifications. One process per session. |
| `ilium-core` | Pure domain model: the tree of groups, panes and folders. No I/O. |
| `ilium-detect` | Pure agent identity and activity classification. |
| `ilium-pty` | Adapter over a PTY library and a terminal parser, plus mouse-protocol encoding. |
| `ilium-ipc` | Shared request/event types and wire framing. |
| `ilium-transport` | Where IPC bytes travel: Unix sockets or Windows named pipes. |
| `ilium-platform` | Every operating-system-specific decision in one place. |
| `ilium-agent-session` | Verifies agent session transcripts and identities. |
| `ilium-session-convert` | Converts a session between Claude Code and Codex. |
| `ilium-agent-debug` | Schema and retention policy for per-pane agent debug history. |
| `ilium-ambient` | Ambient animation scene engines (no terminal types). |
| `ilium-animation-js` | V8 runtime and sandboxed helper for JavaScript `.iliumanim` packages. |
| `ilium-gpu` | Optional GPU backend for ambient scenes (compiled only with the `gpu` feature). |
| `ilium-wikipedia` | Wikipedia data adapter for the Wikipedia animation. |
| `ilium-git` | Git adapter for worktrees and repository probes. |
| `ilium-inference` | Provider-neutral AI inference for titles, organisation and Smart Copy. |
| `ilium-kilo-gateway` | Kilo Gateway HTTP client. |
| `ilium-prompts` | Application prompts embedded at compile time. |
| `ilium-sound` | System sounds and agent-status sound mapping. |
| `ilium-voice` | Full-duplex voice and text turn engine. |
| `ilium-execution` | Bounded worker banks for finite jobs and services. |
| `ilium-logging` | Shared diagnostics for server and clients. |
| `ilium-test-fixtures` | Fake agent CLIs used by integration tests. |

Third-party building blocks include ratatui and crossterm (UI), portable-pty (PTYs), vt100 (terminal parsing, with a vendored patch), sysinfo (process walk), tokio (async runtime) and clap (CLI). The reasons for the vendored patches and the full crate choices are in [ARCHITECTURE.md](../../ARCHITECTURE.md).

## Platform notes

All operating-system decisions live in `ilium-platform`, and the transport differs per platform (Unix sockets versus Windows named pipes). Linux is the primary platform; macOS and Windows are implemented with different test coverage. See [Installation](installation.md#platform-support).

## Troubleshooting

- The tree shows a stale state: detection is adaptive; idle panes are polled every 30 to 60 seconds.
- A new build seems ignored: the old server is still loaded; run `ilium --restart-server`.
- Two terminals show different projects: sessions are scoped to the canonical project directory; check `--cwd`.
- Socket path errors: the path is derived automatically; keep `XDG_RUNTIME_DIR` short and writable, as the OS limits Unix socket path length.
