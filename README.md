# illium

A Rust terminal multiplexer built around two ideas tmux doesn't have:

1. **Tree-structured pane list.** Sessions are a tree of groups and panes (not a flat window/pane grid), shown in a left-side panel, with the active terminal rendered full-size on the right. Panes can be freely reordered and moved between groups.
2. **Agent awareness.** illium periodically inspects each pane's content, detects whether it's running an AI coding agent (Claude Code, Codex CLI, etc.), and shows whether that agent is *thinking* or *done* via icon + color in the tree — so a glance at the sidebar tells you which of your N agent sessions need attention.

Everything else (detach/reattach, persistent sessions, PTY handling, keybindings) follows tmux's model closely enough that tmux muscle memory should mostly transfer.

## Why not just use X

This space already has prior art, researched before writing this doc:

- **[Zellij](https://github.com/zellij-org/zellij)** — the reference architecture for a modern Rust multiplexer: client/server over a Unix socket, WASM plugin system, floating panes. No tree-of-groups pane list, no agent-state detection. illium borrows its client/server split.
- **[tmux-based agent managers — claude-squad](https://github.com/smtg-ai/claude-squad)** — wraps tmux + git worktrees to run multiple Claude Code / Codex / Aider sessions with a dashboard. Not a multiplexer itself; it drives tmux.
- **[herdr](https://www.linuxlinks.com/herdr-terminal-based-agent-multiplexer/)** — a Rust, single-binary terminal multiplexer with a sidebar that classifies each pane as blocked / working / done / idle via process-name + output heuristics, zero-config, ~15 agents supported out of the box. This is the closest existing project to illium's second feature — it validates the approach (process-name + text heuristics is viable and is what real tools ship) but doesn't have the tree/group pane model, only a flat sidebar list.
- **[RMUX (Helvesec)](https://github.com/Helvesec/rmux)** — tmux-compatible (90 commands) Rust multiplexer with typed SDKs (Rust/Python/TS) for programmatic control and a `ratatui-rmux` widget for embedding live panes. No agent-state detection, no tree/groups; it's an automation-first tmux clone.

None of these combine a manipulable *tree* of panes with agent-state detection, which is the actual gap illium fills. Worth knowing herdr exists if the second feature alone is what's wanted — it's shipping today.

## Core concepts

```
Session
 └── Group            (a folder; can contain Groups and/or Panes, arbitrary depth)
      └── Pane         (one PTY: shell or command, own scrollback, own title)
```

- The left panel renders this tree (via `tui-tree-widget`): expand/collapse groups, select a pane to focus it on the right, reorder entries one step at a time (hover an entry's up/down arrows, or leader `m` for keyboard move-mode), drag-and-drop a row onto any other row or the empty space below the tree to reparent it there, and right-click an entry for create/rename/move/close actions. It eases from its normal width to twice that width whenever the pointer is over it or it has keyboard focus, then eases back when focus returns to the right panel. Hover its footer for compact new-shell/Claude/Codex/editor/group controls. A pane arrow at its group's boundary transfers it into the adjacent group, so panes never become root-level nodes. In keyboard move-mode (leader `m`), left/right (or `h`/`l`) indent the selected node into the nearest preceding sibling group / outdent it into its group's own parent — see M3 below for exactly what's implemented and what's deliberately left simpler.
- The right panel renders the focused pane's live terminal. For a detected agent it titles the panel with the real agent PID and its session ID when the CLI exposes one; otherwise it explicitly says that the session is unavailable rather than guessing.
- Click either panel to focus it. When a terminal application enables an xterm mouse protocol (for example `vim`, `htop`, or `lazygit`), illium forwards clicks, drags, scrolls, and modifiers to that PTY using its requested encoding.
- Each tree node is either a `Group` or a `Pane`; a pane carries a `PaneContentKind` (`Terminal` | `Editor`) and a `PaneStatus` (`PlainShell` | `Agent(AgentClass, AgentActivity)` | `Editor { dirty }`). The detection engine drives `PlainShell`/`Agent(...)` for terminal panes, which in turn drives the icon/color shown next to them in the tree.

### Pane states shown in the tree

| Icon/color | Meaning |
|---|---|
| plain terminal glyph, default color | ordinary shell, no agent detected |
| ✦ yellow (pulsing) | agent detected, actively **working/thinking** |
| ✦ blue | agent detected, **waiting on your approval** (y/n prompt) |
| ✦ green | agent detected, **idle/done** — sitting at its input prompt with nothing running |

The blocked/waiting-for-approval state wasn't explicitly requested but falls out of the same detection pass at near-zero extra cost, and it's the state you most want a distinct color for in practice (herdr treats it as a 4th state for the same reason).

## Architecture

Client/server, like Zellij and tmux itself — this is what makes detach/reattach and session persistence possible instead of "just a TUI app that dies with the terminal."

```
┌───────────────────────────────────────────────────────────────┐
│  illium-server (one process per session, spawned on demand)    │
│                                                                  │
│  ┌─────────────┐   ┌───────────────┐   ┌─────────────────┐    │
│  │ ServerState │   │ pane registry │   │ detection loop   │    │
│  │ (illium_core│   │ (PaneResource:│   │ (tokio task,     │    │
│  │  ::Tree --  │   │  PtySession + │   │  adaptive poll   │    │
│  │  Node/      │   │  vt100 screen │   │  per pane, see   │    │
│  │  NodeKind)  │   │  per pane)    │   │  config.rs)      │    │
│  └─────────────┘   └───────────────┘   └─────────────────┘    │
│        UDS socket: ~/.local/share/illium/<session>.sock        │
└────────────────────────────┬────────────────────────────────┘
                              │ length-prefixed bincode frames
                              │ (illium-ipc: ClientRequest / ServerEvent)
                  ┌───────────┴────────────┐
                  │ illium-client (ratatui  │
                  │ TUI, one per attached   │
                  │ terminal)               │
                  └─────────────────────────┘
```

- **illium-core** — pure domain types: a single `Tree` of `Node`s (`NodeId`, `NodeKind::Group { children, expanded }` | `NodeKind::Pane { content: PaneContentKind, status: PaneStatus }`), `AgentClass`, `AgentActivity`, tree operations (`add_group`/`add_pane`/`remove_node`/`rename_node`/`move_node`/`move_node_one_step`/`reorder_sibling`). No I/O, fully unit-testable. Groups and panes are one node type rather than two separate structs — a pane is just a node whose `NodeKind` happens to be `Pane`, which is what lets `move_node_one_step` treat "reorder within a group" and "cross a group boundary" as one operation.
- **illium-pty** — adapter around `portable-pty` (spawn, resize, write) + `vt100` (parse the byte stream into a screen grid you can read text/cells from), plus xterm mouse-protocol encoding (`mouse.rs`) so a pane's foreground app (`vim`, `htop`, `lazygit`, …) receives clicks/drags/scrolls in whatever encoding it negotiated. One PTY reader task per pane.
- **illium-detect** — the agent-detection engine. Two independent signals, combined:
  - **Identity** (which CLI, if any): walk the PTY's child process tree via `sysinfo` and match process names against a registry of known agent signatures (`claude`, `codex`, `opencode`, `aider`). This is the primary signal — robust against UI redesigns, unlike text scraping.
  - **Activity** (thinking vs. idle vs. blocked): scan the vt100 screen's visible text for markers. A literal `"esc to interrupt"` substring is one recognized "working" trigger, but real Claude Code builds also render a present-tense status line ending in an ellipsis alongside a live elapsed-time token (e.g. `"✢ Moonwalking… (running stop hooks… 1/2 · 6s · ↓ 4 tokens)"`) — `looks_like_live_status_line` catches that shape instead of matching exact wording, so it survives whichever whimsical verb is showing. A `y/n`-style confirmation line or a numbered selection menu with a `❯` cursor means blocked (`WaitingApproval`); anything else with no agent CLI detected, or an agent CLI with no such marker, is idle.
  - Detectors are registered in a table (`AGENT_SIGNATURES: &[AgentSignature]`), not a hardcoded if/else chain, so adding support for a new CLI is a data entry, not a code change.
- **illium-server** — owns all PTYs and the tree (`ServerState`), runs the detection loop, writes a JSON crash-recovery snapshot to disk after structural tree changes and, on startup, loads that snapshot back and respawns/registers each of its panes (a fresh, non-resumed invocation of the same command per pane — see M5), speaks the `illium-ipc` protocol over a Unix domain socket per session (mirrors tmux's per-session socket model). One process serves exactly one session; there is no multi-session registry.
- **illium-client** — the `ratatui` TUI: left tree panel + right pane view, keybinding dispatch (`keys.rs`/`keymap.rs`), one-step tree reordering via hover arrows or keyboard move-mode (`mouse.rs`/`keys.rs`, backed by `MoveNode`), sends `ClientRequest`s to the server and renders the `ScreenUpdate`/`TreeSnapshot`/`PaneStatusChanged` events it streams back. Also owns a built-in editor pane (syntax highlighting, markdown rendering, minimap) and background LLM-assisted session/project naming via `illium-kilo-gateway` — both grew out of the original scope during the refactor and aren't covered further in this document.
- **illium-ipc** — `ClientRequest`/`ServerEvent` wire enums plus `write_frame`/`read_frame`: a 4-byte little-endian length prefix followed by that many bytes of bincode payload, generic over any `AsyncRead`/`AsyncWrite` so both the request stream and the event stream reuse the same framing code.
- **illium** (bin) — `clap`-based CLI: `illium` (attach or create default session), `illium new-session <name>`, `illium ls`, `illium kill-session <name>`, `illium new-pane -- <cmd>` — tmux-shaped surface on purpose. Spawns `illium-server` as a separate detached process (not linked in as a library) and hands off to `illium_client::run` for the TUI.

### Why a process-tree check before text scraping

Text-scraping a banner is what most "detect the AI tool" hacks do, and it breaks the moment the tool changes its splash screen. Walking `/proc` (via `sysinfo`, cross-platform enough for Linux/macOS) to find that the pane's foreground process is literally named `claude` or `codex` is a much harder signal to break by accident, and it's cheap to compute alongside the poll. Text scraping is kept only as the activity signal, where there's no substitute (a process name can't tell you if the agent is mid-turn).

### Poll cadence

"A few times a minute" per pane, but adaptive rather than fixed:

- Panes currently `Working` or `WaitingApproval` poll fast (~5s) — for `Working` you want the state flip to `Done` to show up promptly; for `WaitingApproval` you want a quick answer (or a classification that only matched one transient screen) to resolve promptly too, rather than leaving a stale "needs input" badge up for a full slow-tier interval.
- Panes `Idle`/`Done`/`PlainShell` poll slow (~30–60s) — none of those change on their own between polls, no reason to burn CPU reading their screen buffer.
- All intervals configurable in `~/.config/illium/config.toml`.

## Key crates

| Crate | Role |
|---|---|
| [`ratatui`](https://ratatui.rs/) | TUI widget rendering |
| [`crossterm`](https://docs.rs/crossterm) | terminal backend: raw mode, input events (incl. mouse), alternate screen |
| [`portable-pty`](https://docs.rs/portable-pty) (wezterm) | cross-platform PTY spawn/resize/IO |
| [`vt100`](https://docs.rs/vt100) | terminal-escape-sequence parser → screen grid; source of both the rendered pane content and the text the detection engine scans |
| [`tui-term`](https://docs.rs/tui-term) | ratatui widget that renders a `vt100::Screen` directly — used for the right-hand live pane view |
| [`tui-tree-widget`](https://docs.rs/tui-tree-widget) | ratatui tree widget — left-hand session/group/pane panel |
| [`sysinfo`](https://docs.rs/sysinfo) | process-tree walk for agent identity detection |
| [`tokio`](https://docs.rs/tokio) | async runtime: PTY IO tasks, detection-loop timers, UDS server/client |
| `serde` + `toml` | config file, IPC message (de)serialization |
| `bincode` | wire format for the client↔server IPC frames |
| [`directories`](https://docs.rs/directories) | XDG-correct config/data/socket paths |
| [`clap`](https://docs.rs/clap) | CLI argument parsing |
| `ureq` | HTTP client for `illium-kilo-gateway`'s LLM calls (background session/project title inference) |
| [`notify-rust`](https://docs.rs/notify-rust) | desktop notification on a pane's `Working → Done`/`Idle` transition (`illium-server/src/notifications.rs`), toggled off via `config.toml`'s `[notifications]` table — see M5 |

## Implementation plan

Each milestone is meant to be independently runnable/demoable, not a big-bang integration.

1. **M0 — PTY passthrough skeleton. Done.** One `portable-pty` + `vt100` + `tui-term` pipeline rendering a single full-screen pane, now `illium-pty`'s `PtySession`.
2. **M1 — In-process multi-pane tree. Done** (superseded by M2). `illium-core`'s tree model, a left `tui-tree-widget` panel, switching focus between panes, create/close pane, create/close group all shipped first as a single binary; that single-binary form was later split apart in M2 and no longer exists as such.
3. **M2 — Client/server split. Done.** PTY ownership and the tree now live in `illium-server` (`ServerState`, one process per session, spawned on demand rather than run once per machine); `illium-client` is a thin `ratatui` renderer over `illium-ipc`. Concretely, as built:
   - One Unix domain socket per session at `~/.local/share/illium/<session_name>.sock` (via `directories::ProjectDirs::from("", "", "illium").data_dir()`), matching tmux's per-session-socket model — never one socket multiplexing every session.
   - Wire format: `illium-ipc::framing` — a 4-byte little-endian length prefix followed by that many bytes of `bincode`-encoded payload, generic over the payload type and over the async stream (so both the request and event streams reuse it, and tests can frame into an in-memory buffer).
   - Message shapes: `illium_ipc::ClientRequest` (`Attach`, `NewPane`, `NewGroup`, `ClosePane`, `MoveNode`, `RenameNode`, `ResizePane`, `KeyInput`, `MouseInput`, `Detach`, `KillSession`) sent client→server, and `illium_ipc::ServerEvent` (`TreeSnapshot(Tree)`, `ScreenUpdate { pane_id, bytes }`, `PaneStatusChanged { pane_id, status }`, `Error { message }`) pushed server→client. `TreeSnapshot` is always a full snapshot rather than a diff — the tree is small and this keeps the client from ever drifting by missing an incremental update. `ScreenUpdate` carries raw PTY bytes rather than a pre-computed cell diff, since the client already runs its own `vt100::Parser` per pane to drive `tui-term`.
   - The `illium` CLI spawns `illium-server` as a separate detached OS process (not linked in as a library), then either attaches `illium_client::run` or sends one short-lived request and exits — this is what buys detach/reattach and session persistence across terminal closes.
4. **M3 — Tree manipulation. Done.** One-step move keybindings (leader `m` toggles keyboard move-mode, up/down or `j`/`k` calls `Tree::move_node_one_step`), the same one-step move via each tree row's hover ↑/↓ arrows, and reordering siblings including the pane-crosses-a-group-boundary case (`move_node_one_step` transfers a pane into the adjacent group so panes never become root-level nodes). Arbitrary reparenting rides `illium_ipc::ClientRequest::ReparentNode` (`node_id`, `new_parent`, `index`), mirroring `Tree::move_node` directly, handled server-side in `illium-server/src/ipc/handlers.rs`. Two client-side features are built on top of it: mouse drag-and-drop (`illium-client/src/mouse.rs::compute_drop_target`) — mouse-down on a tree row starts tracking it as the drag source, mouse-up over another row drops onto a `Group` (appends as its last child) or a `Pane` (inserts as that pane's immediate predecessor in its parent group), and mouse-up over the empty space below the last row appends at the top level; and keyboard indent/outdent in move-mode (leader `m`, then left/`h` to outdent or right/`l` to indent — `illium-client/src/keys.rs::compute_indent_target`/`compute_outdent_target`) — indent moves the selected node into the nearest preceding sibling group (appended at its end), outdent moves it out into its group's own parent (positioned right after that group among its new siblings). Both client-side computations reject the unambiguously-invalid cases before ever forming a request (dropping/indenting onto the node itself or one of its own descendants, and leaving a pane parentless at the top level); every other rejection (e.g. a stale id from a concurrent structural change) comes back as `ServerEvent::Error` and is shown in the status bar rather than crashing the client. Deliberately left simpler: no visual drop-target highlight during a drag beyond the tree's existing row-hover affordance (a nice-to-have the task explicitly allowed skipping in favor of correct drop behavior), and no "indent into previous group" / "outdent" entry in the right-click context menu (a menu click has no natural "which preceding group" to indent into the way a specific drop position or an ordered sibling walk does).
5. **M4 — Agent detection engine. Done.** `illium-detect`: the `AGENT_SIGNATURES` registry table, process-tree identity check via `sysinfo` (`identify_agent`), text-marker activity check (`classify_activity`, covering the working/waiting-approval/idle states plus the numbered-selection-menu and live-status-line cases added after the original design), `illium-server`'s adaptive poll loop (`detection.rs`, fast interval for `Working` panes, slow for everything else, both configurable), and icon/color wiring into the client's tree render.
6. **M5 — Polish. Partially done.** What exists, all in `~/.config/illium/config.toml`: server-side, `illium-server/src/config.rs`'s `[detection]` table covers the two poll intervals plus `[[detection.custom_signatures]]` (an array of `{ process_name, agent_class }` tables validated into real `illium_detect::AgentSignature` values and threaded through `identify_agent_with_extra` alongside the built-in registry — `agent_class` is one of `"claude"`/`"codex"`/`"other"`, with `"other"` always resolving to `AgentClass::Other(<the process name that actually matched>)`, matching how the built-in `opencode`/`aider` signatures behave; there is no separate custom display-label field), and a `[notifications]` table (`enabled`, default `true`). Client-side, `illium-client/src/config.rs`'s `[keybindings]` table remaps which single letter dispatches an existing `keymap::Action` (`action_name = "letter"`, e.g. `quit = "x"`) — scoped to remapping only, config cannot define a brand-new action, and a config that binds two actions to the same letter is rejected as a load error rather than silently picking one; its `[theme]` table overrides four `#rrggbb` colors (`accent_bg`, `accent_fg`, `border_focused`, `border_unfocused`) via `theme::Theme`, installed once at startup into a `OnceLock` the existing `theme.rs` free functions read from internally — every other `ratatui::style::Style` in the app (border type/merge strategy, chrome icon glyphs, status bar caps, and any other inline style) stays hardcoded, not every visual is themeable. A JSON crash-recovery snapshot is written to `<data_dir>/<session>.snapshot.json` after every structural tree change; on startup, `illium-server::run` loads it, replaces the fresh tree with the snapshot's tree wholesale, and respawns/registers each snapshotted pane (`ipc::handlers::spawn_and_register_pane`, the same function `NewPane` uses) — a terminal pane gets a real, freshly-spawned PTY running the same command, an editor pane gets its path re-registered. A pane whose command fails to respawn (e.g. its binary no longer exists) is logged and dropped from the restored tree rather than left as a dead node. Deliberately out of scope: resuming an agent CLI's own session (`claude --resume <id>` / `codex resume <id>`) — a restored pane is a fresh, non-resumed invocation of the same command (see `illium-server/src/persistence.rs`'s module doc). Desktop notifications: `illium-server/src/notifications.rs`'s pure `is_finished_transition` decides which `Working -> Done`/`Idle` transitions are notification-worthy (unit tested exhaustively over every relevant status pair); `detection::run_due_panes` queues one per tick and sends it via `notify_rust::Notification::show`, run through `tokio::task::spawn_blocking` after the tree/pane locks are dropped so a slow or absent notification daemon never blocks other IPC connections. A failed send (no D-Bus session, common in containers/CI) is logged and does not affect detection.

### What's automated vs. what still needs a human

Every crate has unit or integration tests exercising it directly (see `CLAUDE.md` "Testing" for the per-crate policy). As of the M5 stage, two end-to-end tests close what used to be the two biggest manual-only gaps — actually driving a real PTY-rendered TUI, and actually driving a live agent-CLI process through detection:

- `illium/tests/pty_tui_smoke.rs` drives the real `illium` binary under a genuine PTY (`illium-pty`, not `std::process::Command` with inherited stdio): creates a pane via the non-attaching `new-pane` subcommand, attaches with the real TUI, asserts the first rendered frame contains real chrome and pane content, sends a scripted leader-key + help keystroke and asserts the help overlay actually appears on screen, then tears the session down via the CLI's own `kill-session` path. This is genuine PTY-driven TUI rendering under test, not a unit test of a rendering function in isolation.
- `illium-server/tests/live_agent_detection.rs` spawns a fake `claude`-named script by absolute path (never `PATH`, to avoid ever racing a real `claude` install), drives a real `illium-server` end to end — real `sysinfo` process-tree walk finds it, `illium-pty`'s live `vt100` feed is scraped, `illium_detect::classify_activity` runs unmodified — and asserts the real `Working -> Idle`/`Done` `PaneStatusChanged` transition arrives over a real IPC connection.

Genuinely still manual: whether the rendered output actually looks right on a real terminal emulator — font rendering, color contrast, the drag-and-drop/animation "feel." These tests assert structural/textual correctness (the right text lands on the right row at the right time), not visual quality, which has no meaningful automated oracle.

## Non-goals (for now)

- No WASM plugin system (Zellij already owns that niche well).
- No built-in SSH/remote-session sharing (RMUX already owns that niche).
- No attempt to *drive* the agent programmatically (send it prompts, read structured output) — illium only observes; it's a multiplexer, not an orchestration SDK. If that's wanted later, it's a different tool (closer to claude-squad or RMUX's SDK).
