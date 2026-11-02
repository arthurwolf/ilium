## Prior art — why not just use X

This space already has prior art, researched before writing this doc:

- **[Zellij](https://github.com/zellij-org/zellij)** — the reference architecture for a modern Rust multiplexer: client/server over a Unix socket, WASM plugin system, floating panes. No tree-of-groups pane list, no agent-state detection. ilium borrows its client/server split.
- **[tmux-based agent managers — claude-squad](https://github.com/smtg-ai/claude-squad)** — wraps tmux + git worktrees to run multiple Claude Code / Codex / Aider sessions with a dashboard. Not a multiplexer itself; it drives tmux.
- **[herdr](https://www.linuxlinks.com/herdr-terminal-based-agent-multiplexer/)** — a Rust, single-binary terminal multiplexer with a sidebar that classifies each pane as blocked / working / done / idle via process-name + output heuristics, zero-config, ~15 agents supported out of the box. This is the closest existing project to ilium's second feature — it validates the approach (process-name + text heuristics is viable and is what real tools ship) but doesn't have the tree/group pane model, only a flat sidebar list.
- **[RMUX (Helvesec)](https://github.com/Helvesec/rmux)** — tmux-compatible (90 commands) Rust multiplexer with typed SDKs (Rust/Python/TS) for programmatic control and a `ratatui-rmux` widget for embedding live panes. No agent-state detection, no tree/groups; it's an automation-first tmux clone.

None of these combine a manipulable *tree* of panes with agent-state detection, which is the actual gap ilium fills. Worth knowing herdr exists if the second feature alone is what's wanted — it's shipping today.
