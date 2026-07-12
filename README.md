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

- The left panel renders this tree (via `tui-tree-widget`): expand/collapse groups, select a pane to focus it on the right, drag entries to move them, and right-click an entry for create/rename/move/close actions. It eases from its normal width to twice that width whenever the pointer is over it or it has keyboard focus, then eases back when focus returns to the right panel. Hover its footer for compact new-shell/Claude/Codex/editor/group controls; hover an entry for up/down arrows. A pane arrow at its group's boundary transfers it into the adjacent group, so panes never become root-level nodes.
- The right panel renders the focused pane's live terminal. For a detected agent it titles the panel with the real agent PID and its session ID when the CLI exposes one; otherwise it explicitly says that the session is unavailable rather than guessing.
- Click either panel to focus it. When a terminal application enables an xterm mouse protocol (for example `vim`, `htop`, or `lazygit`), illium forwards clicks, drags, scrolls, and modifiers to that PTY using its requested encoding.
- Panes carry a `PaneKind` (`PlainShell` | `Agent(AgentClass, AgentActivity)`), set by the detection engine, which drives the icon/color shown next to them in the tree.

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
┌────────────────────────────────────────────────────────────┐
│ illium-server (background process, one per machine/user)    │
│                                                               │
│  ┌────────────┐   ┌───────────────┐   ┌────────────────┐   │
│  │ tree store  │   │ pane registry │   │ detection loop  │   │
│  │ (sessions/  │   │ (PTY handle + │   │ (tokio interval,│   │
│  │  groups/    │   │  vt100 screen │   │  adaptive poll  │   │
│  │  panes)     │   │  per pane)    │   │  per pane)      │   │
│  └────────────┘   └───────────────┘   └────────────────┘   │
│           UDS socket: ~/.local/share/illium/<session>.sock  │
└──────────────────────────┬────────────────────────────────┘
                            │ length-prefixed bincode frames
                ┌───────────┴───────────┐
                │      illium-client (ratatui TUI, one per attached terminal)     │
                └───────────────────────┘
```

- **illium-core** — pure domain types: `Session`, `Group`, `Pane`, `PaneId`, `PaneKind`, `AgentActivity`, tree operations (move/reorder/insert/remove). No I/O, fully unit-testable.
- **illium-pty** — adapter around `portable-pty` (spawn, resize, write) + `vt100` (parse the byte stream into a screen grid you can read text/cells from). One PTY reader task per pane.
- **illium-detect** — the agent-detection engine. Two independent signals, combined:
  - **Identity** (which CLI, if any): walk the PTY's child process tree via `sysinfo` and match process names/cmdlines against a registry of known agent signatures (`claude`, `codex`, `opencode`, …). This is the primary signal — robust against UI redesigns, unlike text scraping.
  - **Activity** (thinking vs. idle vs. blocked): scan the vt100 screen's visible text for literal markers. Both Claude Code and Codex CLI render a static `"esc to interrupt"` string (plus a live token/time counter) for the entire duration of a working turn — that substring is the reliable "working" signal, far more stable than trying to catch a spinner frame mid-cycle. An empty bordered input box with no such marker means idle; a `y/n`-style approval box means blocked.
  - Detectors are registered in a table (`Vec<AgentSignature>`), not a hardcoded if/else chain, so adding support for a new CLI is a data entry, not a code change.
- **illium-server** — owns all PTYs and the tree, runs the detection loop, persists tree state to disk (JSON snapshot, crash-recovery only — not a database), speaks the IPC protocol over a Unix domain socket per session (mirrors tmux's per-session socket model).
- **illium-client** — the `ratatui` TUI: left tree panel + right pane view, keybinding dispatch, mouse drag-and-drop for reordering, sends commands to the server and renders the screen diffs it streams back.
- **illium** (bin) — `clap`-based CLI: `illium` (attach or create default session), `illium new-session <name>`, `illium ls`, `illium kill-session <name>`, `illium new-pane -- <cmd>` — tmux-shaped surface on purpose.

### Why a process-tree check before text scraping

Text-scraping a banner is what most "detect the AI tool" hacks do, and it breaks the moment the tool changes its splash screen. Walking `/proc` (via `sysinfo`, cross-platform enough for Linux/macOS) to find that the pane's foreground process is literally named `claude` or `codex` is a much harder signal to break by accident, and it's cheap to compute alongside the poll. Text scraping is kept only as the activity signal, where there's no substitute (a process name can't tell you if the agent is mid-turn).

### Poll cadence

"A few times a minute" per pane, but adaptive rather than fixed:

- Panes currently `Working` poll fast (~5s) — you want the state flip to `Done` to show up promptly.
- Panes `Idle`/`PlainShell` poll slow (~30–60s) — nothing is changing, no reason to burn CPU reading their screen buffer.
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
| `aho-corasick` | fast multi-pattern literal search for the identity/activity marker sets |
| `notify-rust` *(optional, later)* | desktop notification on a pane's `Working → Done` transition |

## Implementation plan

Each milestone is meant to be independently runnable/demoable, not a big-bang integration.

1. **M0 — PTY passthrough skeleton.** One `portable-pty` + `vt100` + `tui-term` pipeline rendering a single full-screen pane. Proves the core rendering pipeline before anything else is built on top of it.
2. **M1 — In-process multi-pane tree.** `illium-core` tree model, left `tui-tree-widget` panel, switch focus between panes, create/close pane, create/close group. No client/server split yet — single binary.
3. **M2 — Client/server split.** Move PTY ownership + tree into `illium-server`, UDS IPC, `illium-client` becomes a thin renderer. This is what buys detach/reattach and session persistence across terminal closes.
4. **M3 — Tree manipulation.** Move-mode keybindings (pick up / drop a node, reorder siblings, indent into/out of a group) plus mouse drag-and-drop in the left panel.
5. **M4 — Agent detection engine.** `illium-detect`: signature registry, process-tree identity check, text-marker activity check, adaptive poll loop, icon/color wiring into the tree render.
6. **M5 — Polish.** Config file (keybindings, detection signatures, poll intervals, theme), tree state persisted across server restarts, optional desktop notifications on state transitions.

## Non-goals (for now)

- No WASM plugin system (Zellij already owns that niche well).
- No built-in SSH/remote-session sharing (RMUX already owns that niche).
- No attempt to *drive* the agent programmatically (send it prompts, read structured output) — illium only observes; it's a multiplexer, not an orchestration SDK. If that's wanted later, it's a different tool (closer to claude-squad or RMUX's SDK).
