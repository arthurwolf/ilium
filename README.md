# ilium

A Rust terminal multiplexer built around two ideas tmux doesn't have:

1. **Tree-structured pane list.** Sessions are a tree of groups, split views, and panes (not a flat window/pane grid), shown in a left-side panel. The right panel renders either one pane or a persistent vertical/horizontal split containing up to four terminals, editors, or boards. Panes can be freely reordered and moved between containers.
2. **Agent awareness.** ilium periodically inspects each pane's content, detects whether it's running an AI coding agent (Claude Code, Codex CLI, Antigravity CLI, etc.), and shows whether that agent is *thinking* or *done* via icon + color in the tree — so a glance at the sidebar tells you which of your N agent sessions need attention.

Everything else (detach/reattach, persistent sessions, PTY handling, keybindings) follows tmux's model closely enough that tmux muscle memory should mostly transfer.

## Why not just use X

This space already has prior art, researched before writing this doc:

- **[Zellij](https://github.com/zellij-org/zellij)** — the reference architecture for a modern Rust multiplexer: client/server over a Unix socket, WASM plugin system, floating panes. No tree-of-groups pane list, no agent-state detection. ilium borrows its client/server split.
- **[tmux-based agent managers — claude-squad](https://github.com/smtg-ai/claude-squad)** — wraps tmux + git worktrees to run multiple Claude Code / Codex / Aider sessions with a dashboard. Not a multiplexer itself; it drives tmux.
- **[herdr](https://www.linuxlinks.com/herdr-terminal-based-agent-multiplexer/)** — a Rust, single-binary terminal multiplexer with a sidebar that classifies each pane as blocked / working / done / idle via process-name + output heuristics, zero-config, ~15 agents supported out of the box. This is the closest existing project to ilium's second feature — it validates the approach (process-name + text heuristics is viable and is what real tools ship) but doesn't have the tree/group pane model, only a flat sidebar list.
- **[RMUX (Helvesec)](https://github.com/Helvesec/rmux)** — tmux-compatible (90 commands) Rust multiplexer with typed SDKs (Rust/Python/TS) for programmatic control and a `ratatui-rmux` widget for embedding live panes. No agent-state detection, no tree/groups; it's an automation-first tmux clone.

None of these combine a manipulable *tree* of panes with agent-state detection, which is the actual gap ilium fills. Worth knowing herdr exists if the second feature alone is what's wanted — it's shipping today.

## Core concepts

```
Session
 └── Group            (can contain Groups, Split Views, Panes, and folder roots)
      ├── Pane         (terminal, editor, or board)
      └── Split View   (vertical/horizontal container; zero to four Panes)
           ├── Pane
           └── Pane
```

- The left panel renders this tree (via `tui-tree-widget`): expand/collapse groups, select a pane to focus it on the right, reorder entries one step at a time (hover an entry's up/down arrows, or leader `m` for keyboard move-mode), drag-and-drop a row onto any other row or the empty space below the tree to reparent it there, and right-click an entry for create/rename/move/close actions. Every tree menu also exposes **Restart**, which reloads the client executable and reattaches it to the existing detached server without restarting that server or its PTYs. The same menu has a checked **Order by** submenu for Manual, Type, Age up/down, and Name A-Z/Z-A; automatic modes sort independently inside every normal group while leaving split-view placement untouched. The choice applies live, persists as `[ui].tree_order`, and is mirrored by the User Appearance settings tab. Any arrow, drag/drop, or keyboard structural move returns the setting to Manual before sending the move. Terminal rows, including detected agents and plain shells, also expose **Hit key(s) X time from now**: the dialog accepts an hours/minutes/seconds delay, optional text, and optional Enter; the detached server persists and delivers that input at the absolute deadline even when no client remains attached. While pending, the row shows a human-readable countdown before the pane title and animates the existing clock sequence backwards. The panel eases from its normal width to twice that width whenever the pointer is over it or it has keyboard focus, then eases back when focus returns to the right panel. Structural changes use their own 220 ms eased feedback: removed rows accelerate left and dim before disappearing, while added rows enter in the opposite direction, settle softly, and only then run the existing creation blink. Its footer actions are visible whenever the panel has keyboard focus or the footer is hovered; creation controls flow from the left and a right-aligned 🎚️ opens Settings. At a nested group or split-view boundary, a pane arrow moves the pane into the enclosing group immediately before or after its former container. At a top-level group boundary, it transfers the pane into the adjacent group so panes never become root-level nodes. In keyboard move-mode (leader `m`), left/right (or `h`/`l`) indent the selected node into the nearest preceding sibling group / outdent it into its group's own parent — see M3 below for exactly what's implemented and what's deliberately left simpler.
- The right panel renders a normal pane alone, or every child of a selected split view. Two and three children follow the split's orientation; four use a 2 by 2 grid. Selecting a child keeps the whole split visible while making only that child active for keyboard and pointer input. For a detected agent, its viewport title includes the real agent PID and its session ID when the CLI exposes one; otherwise it explicitly says that the session is unavailable rather than guessing.
- Click either panel to focus it. When a terminal application enables an xterm mouse protocol (for example `vim`, `htop`, or `lazygit`), ilium forwards clicks, drags, scrolls, and modifiers to that PTY using its requested encoding.
- Each tree node is a `Container`, `Pane`, or persisted folder root. A container is either a normal `Group` or a `SplitView { orientation }`; a pane carries a `PaneContentKind` (`Terminal` | `Editor` | `Board`) and a matching `PaneStatus`. The detection engine drives `PlainShell`/`Agent(...)` for terminal panes, which in turn drives the icon/color shown next to them in the tree.

### Pane states shown in the tree

| Icon/color | Meaning |
|---|---|
| 📟, default color | ordinary shell, no agent detected |
| ✦ yellow (pulsing) | agent detected, actively **working/thinking** |
| 🕛 animated clock | agent is **waiting for background agents/tasks** it started |
| ✦ blue | agent detected, **waiting on your approval** (y/n prompt) |
| ✦ green | agent detected, **idle/done** — sitting at its input prompt with nothing running |

The blocked/waiting-for-approval state wasn't explicitly requested but falls out of the same detection pass at near-zero extra cost, and it's the state you most want a distinct color for in practice (herdr treats it as a 4th state for the same reason).

## Architecture

Client/server, like Zellij and tmux itself — this is what makes detach/reattach and session persistence possible instead of "just a TUI app that dies with the terminal."

```
┌───────────────────────────────────────────────────────────────┐
│  ilium-server (one process per session, spawned on demand)    │
│                                                                  │
│  ┌─────────────┐   ┌───────────────┐   ┌─────────────────┐    │
│  │ ServerState │   │ pane registry │   │ detection loop   │    │
│  │ (ilium_core│   │ (PaneResource:│   │ (tokio task,     │    │
│  │  ::Tree --  │   │  PtySession + │   │  adaptive poll   │    │
│  │  Node/      │   │  vt100 screen │   │  per pane, see   │    │
│  │  NodeKind)  │   │  per pane)    │   │  config.rs)      │    │
│  └─────────────┘   └───────────────┘   └─────────────────┘    │
│ UDS socket: $XDG_RUNTIME_DIR/ilium/<project-slug>-<hash>-<session>.sock │
└────────────────────────────┬────────────────────────────────┘
                              │ length-prefixed bincode frames
                              │ (ilium-ipc: ClientRequest / ServerEvent)
                  ┌───────────┴────────────┐
                  │ ilium-client (ratatui  │
                  │ TUI, one per attached   │
                  │ terminal)               │
                  └─────────────────────────┘
```

- **ilium-core** — pure domain types: one `Tree` of `Node`s, with `NodeKind::Container(ContainerNode)` for normal groups and split views, `NodeKind::Pane` for terminals/editors/boards, and `NodeKind::Folder` for persisted filesystem roots. `ContainerNode` owns child-kind and split-capacity policy; `Tree::create_split_view` validates and moves selected panes atomically. No I/O, fully unit-testable.
- **ilium-pty** — adapter around `portable-pty` (spawn, resize, write) + `vt100` (parse the byte stream into a screen grid you can read text/cells from), plus xterm mouse-protocol encoding (`mouse.rs`) so a pane's foreground app (`vim`, `htop`, `lazygit`, …) receives clicks/drags/scrolls in whatever encoding it negotiated. One PTY reader task per pane.
- **ilium-detect** — the agent-detection engine. Two independent signals, combined:
  - **Identity** (which CLI, if any): walk the PTY's child process tree via `sysinfo` and match process names against the shared built-in provider registry (`claude`, `codex`, `agy`/`antigravity`), plus generic/custom signatures (`opencode`, `aider`, …). This is the primary signal — robust against UI redesigns, unlike text scraping.
  - **Activity** (thinking vs. idle vs. blocked): scan the vt100 screen's visible text for markers. A literal `"esc to interrupt"` substring is one recognized "working" trigger, but real Claude Code builds also render a present-tense status line ending in an ellipsis alongside a live elapsed-time token (e.g. `"✢ Moonwalking… (running stop hooks… 1/2 · 6s · ↓ 4 tokens)"`) — `looks_like_live_status_line` catches that shape instead of matching exact wording, so it survives whichever whimsical verb is showing. A `y/n`-style confirmation line or a numbered selection menu with a `❯` cursor means blocked (`WaitingApproval`); anything else with no agent CLI detected, or an agent CLI with no such marker, is idle.
  - First-party providers implement one pure shared contract for command launch, process-name aliases, resume syntax, CLI argument parsing, labels, and deterministic ordering. Adding a supported provider extends that contract rather than duplicating special cases through the client and server.
- **ilium-agent-session** — the shared transcript-provenance boundary used by both server-side session discovery and client-side LLM titling. It verifies Claude/Codex JSONL stores and Antigravity's UUID database plus `history.jsonl` project binding before accepting a session, preventing cross-project identities from leaking through lossy/global stores.
- **ilium-server** — owns all PTYs and the tree (`ServerState`), runs the detection loop and the single scheduled-input executor, writes a JSON crash-recovery snapshot to `<project>/.ilium/sessions/<name>.json` after structural tree changes and restores its panes and pending absolute deadlines on startup. The CLI gives it one exact project-session socket, so one process serves exactly one session with no multi-session registry.
- **ilium-client** — the `ratatui` TUI: left tree panel + right presentation target, keybinding dispatch (`keys.rs`/`keymap.rs`), one-step tree reordering, and shared `split_layout` viewport geometry used by rendering, PTY sizing, focus, and mouse routing. It sends `ClientRequest`s to the server and renders the `ScreenUpdate`/`TreeSnapshot`/`PaneStatusChanged` events it streams back. It also owns built-in editor and board panes plus background LLM-assisted session/project naming via `ilium-kilo-gateway`.
- **ilium-ipc** — `ClientRequest`/`ServerEvent` wire enums plus `write_frame`/`read_frame`: a 4-byte little-endian length prefix followed by that many bytes of bincode payload, generic over any `AsyncRead`/`AsyncWrite` so both the request stream and the event stream reuse the same framing code.
- **ilium-sound** — cross-platform adapter for XDG/Linux, macOS, and Windows system-sound discovery plus bounded native-command playback. It also owns the pure agent-status transition mapping used by the server, while `ilium-client` only presents the discovered catalog and edits the shared settings.
- **ilium** (bin) — `clap`-based CLI: `ilium` attaches or creates the `default` session for the current canonical directory; `ilium new-session <name>`, `ilium ls`, `ilium kill-session <name>`, and `ilium new-pane --session <name> -- <cmd>` remain project-scoped. It spawns `ilium-server` as a separate detached process and hands off to `ilium_client::run` for the TUI.

### Why a process-tree check before text scraping

Text-scraping a banner is what most "detect the AI tool" hacks do, and it breaks the moment the tool changes its splash screen. Walking `/proc` (via `sysinfo`, cross-platform enough for Linux/macOS) to find that the pane's foreground process is literally named `claude`, `codex`, or `agy` is a much harder signal to break by accident, and it's cheap to compute alongside the poll. Text scraping is kept only as the activity signal, where there's no substitute (a process name can't tell you if the agent is mid-turn).

### Poll cadence

"A few times a minute" per pane, but adaptive rather than fixed:

- Panes currently `Working` or `WaitingApproval` poll fast (~5s) — for `Working` you want the state flip to `Done` to show up promptly; for `WaitingApproval` you want a quick answer (or a classification that only matched one transient screen) to resolve promptly too, rather than leaving a stale "needs input" badge up for a full slow-tier interval.
- Panes `Idle`/`Done`/`PlainShell` poll slow (~30–60s) — none of those change on their own between polls, no reason to burn CPU reading their screen buffer.
- All intervals configurable in `~/.config/ilium/config.toml`.

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
| `ureq` | HTTP client for `ilium-kilo-gateway`'s LLM calls (background session/project title inference) |
| [`notify-rust`](https://docs.rs/notify-rust) | desktop notification on a pane's `Working → Done`/`Idle` transition (`ilium-server/src/notifications.rs`), toggled off via `config.toml`'s `[notifications]` table — see M5 |

## Implementation plan

Each milestone is meant to be independently runnable/demoable, not a big-bang integration.

1. **M0 — PTY passthrough skeleton. Done.** One `portable-pty` + `vt100` + `tui-term` pipeline rendering a single full-screen pane, now `ilium-pty`'s `PtySession`.
2. **M1 — In-process multi-pane tree. Done** (superseded by M2). `ilium-core`'s tree model, a left `tui-tree-widget` panel, switching focus between panes, create/close pane, create/close group all shipped first as a single binary; that single-binary form was later split apart in M2 and no longer exists as such.
3. **M2 — Client/server split. Done.** PTY ownership and the tree now live in `ilium-server` (`ServerState`, one process per session, spawned on demand rather than run once per machine); `ilium-client` is a thin `ratatui` renderer over `ilium-ipc`. Concretely, as built:
   - One Unix domain socket per project session at `$XDG_RUNTIME_DIR/ilium/<project-slug>-<digest>-<session>.sock`, matching tmux's per-session-socket model. The snapshot remains with its project at `.ilium/sessions/<session>.json`; a digest disambiguates slug collisions.
   - Wire format: `ilium-ipc::framing` — a 4-byte little-endian length prefix followed by that many bytes of `bincode`-encoded payload, generic over the payload type and over the async stream (so both the request and event streams reuse it, and tests can frame into an in-memory buffer).
   - Message shapes: `ilium_ipc::ClientRequest` includes attach/detach, pane and container creation (`CreateSplitView` is atomic), structural moves, focus/input/resize, settings, and session-lifecycle requests. `ilium_ipc::ServerEvent` carries full `TreeSnapshot`s, terminal replay/live bytes, pane status/session metadata, and explicit errors. A full tree snapshot rather than a diff keeps attached clients from drifting after structural changes.
   - The `ilium` CLI spawns `ilium-server` as a separate detached OS process (not linked in as a library), then either attaches `ilium_client::run` or sends one short-lived request and exits — this is what buys detach/reattach and session persistence across terminal closes.
4. **M3 — Tree manipulation. Done.** One-step move keybindings (leader `m` toggles keyboard move-mode, up/down or `j`/`k` calls `Tree::move_node_one_step`), the same one-step move via each tree row's hover ↑/↓ arrows, and reordering siblings including pane-crosses-a-container-boundary cases. At a nested normal-group or split-view boundary, `move_node_one_step` exits the pane into the enclosing group immediately before/after its former container; at a top-level group boundary it transfers the pane into the adjacent group so panes never become root-level nodes. Arbitrary reparenting rides `ilium_ipc::ClientRequest::ReparentNode` (`node_id`, `new_parent`, `index`), mirroring `Tree::move_node` directly, handled server-side in `ilium-server/src/ipc/handlers.rs`. Two client-side features are built on top of it: mouse drag-and-drop (`ilium-client/src/mouse.rs::compute_drop_target`) — mouse-down on a tree row starts tracking it as the drag source, mouse-up over another row drops onto a `Group` (appends as its last child) or a `Pane` (inserts as that pane's immediate predecessor in its parent group), and mouse-up over the empty space below the last row appends at the top level; and keyboard indent/outdent in move-mode (leader `m`, then left/`h` to outdent or right/`l` to indent — `ilium-client/src/keys.rs::compute_indent_target`/`compute_outdent_target`) — indent moves the selected node into the nearest preceding sibling group (appended at its end), outdent moves it out into its group's own parent (positioned right after that group among its new siblings). Both client-side computations reject the unambiguously-invalid cases before ever forming a request (dropping/indenting onto the node itself or one of its own descendants, and leaving a pane parentless at the top level); every other rejection (e.g. a stale id from a concurrent structural change) comes back as `ServerEvent::Error` and is shown in the status bar rather than crashing the client. Deliberately left simpler: no visual drop-target highlight during a drag beyond the tree's existing row-hover affordance (a nice-to-have the task explicitly allowed skipping in favor of correct drop behavior), and no "indent into previous group" / "outdent" entry in the right-click context menu (a menu click has no natural "which preceding group" to indent into the way a specific drop position or an ordered sibling walk does).
5. **M4 — Agent detection engine. Done.** `ilium-detect`: the shared built-in provider registry plus extensible generic/custom signatures, process-tree identity check via `sysinfo` (`identify_agent`), text-marker activity check (`classify_activity`, covering the working/waiting-approval/idle states plus the numbered-selection-menu and live-status-line cases added after the original design), `ilium-server`'s adaptive poll loop (`detection.rs`, fast interval for `Working` panes, slow for everything else, both configurable), and icon/color wiring into the client's tree render.
6. **M5 — Polish. Partially done.** What exists, all in `~/.config/ilium/config.toml`: server-side, `ilium-server/src/config.rs`'s `[detection]` table covers the two poll intervals plus `[[detection.custom_signatures]]`; client-side, keybindings, the configurable `[keyboard].shortcut_base`, `[ui].tree_order`, per-provider agent icons, and the four-color theme override are configured there too. The full-screen Settings view exposes Appearance, Keyboard, Kanban Board, Sound, and About tabs. Kanban Board persists a global 1–10-line card-preview height (three lines by default) and minimum column width (20 cells by default) under `[kanban_board]`; narrower viewports page complete columns behind a horizontal scrollbar. Cards render contiguously without redundant top labels, show their source and insertion target throughout mouse drags, and expose clickable Markdown task checkboxes. Clicking the remaining card surface opens an aerated title/notes editor in the rightmost third; each keystroke is committed immediately through either the single-Markdown-file or folder-of-Markdown-files storage adapter. Sound discovers only folders/files that exist on the current system (XDG/Linux distributions, macOS system/user libraries, or Windows Media/theme folders), offers system beep or a selected file with preview, and independently enables Agent finished, approval-needed, started-working, and waiting-background events. Changes persist under `[sound]`, reach the current detached server immediately over IPC, and are picked up by other running project servers through a low-frequency global-config watcher. Playback is serialized through a bounded server-owned actor, so it works with no client attached, never duplicates per attached client, and cannot block detection or IPC. Each project session persists independently in `.ilium/sessions/<name>.json`; detected Claude, Codex, and Antigravity IDs are converted into their provider-specific resume commands when saved, so restored panes resume their own agent conversations. Desktop notifications are sent for useful finished transitions without blocking IPC.
7. **Split views. Done.** `ContainerNode` generalizes tree ownership without duplicating membership in client state. Leader `W`, the tree footer split button, or a context action opens an orientation dialog and an optional eligible-pane picker; the server applies one atomic `CreateSplitView` mutation. `RightPanelTarget` and the pure `split_layout` allocator render zero to four panes, resize each visible PTY to its own viewport, and route keyboard/mouse/editor/board interactions only to the active slot.

### What's automated vs. what still needs a human

Every crate has unit or integration tests exercising it directly (see `CLAUDE.md` "Testing" for the per-crate policy). End-to-end tests cover both a real PTY-rendered TUI and a live agent-CLI process through detection:

- `ilium/tests/pty_tui_smoke.rs` drives the real `ilium` binary under a genuine PTY (`ilium-pty`, not `std::process::Command` with inherited stdio). In addition to attach/help/settings coverage, it creates two live terminal panes, moves them into a vertical split through the real dialogs, asserts both viewport streams render together, focuses each child, and verifies distinct input reaches each PTY while both remain visible.
- `ilium-server/tests/live_agent_detection.rs` spawns a fake `claude`-named script by absolute path (never `PATH`, to avoid ever racing a real `claude` install), drives a real `ilium-server` end to end — real `sysinfo` process-tree walk finds it, `ilium-pty`'s live `vt100` feed is scraped, `ilium_detect::classify_activity` runs unmodified — and asserts the real `Working -> Idle`/`Done` `PaneStatusChanged` transition arrives over a real IPC connection and queues exactly one sound through an injected silent recorder.

Genuinely still manual: whether the rendered output actually looks right on a real terminal emulator — font rendering, color contrast, the drag-and-drop/animation "feel." These tests assert structural/textual correctness (the right text lands on the right row at the right time), not visual quality, which has no meaningful automated oracle.

## Non-goals (for now)

- No WASM plugin system (Zellij already owns that niche well).
- No built-in SSH/remote-session sharing (RMUX already owns that niche).
- No attempt to *drive* the agent programmatically (send it prompts, read structured output) — ilium only observes; it's a multiplexer, not an orchestration SDK. If that's wanted later, it's a different tool (closer to claude-squad or RMUX's SDK).
