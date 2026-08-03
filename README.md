# ilium

A terminal multiplexer for people running several AI coding agents at once.

Like tmux, ilium keeps your terminals alive in a background server you can detach from and reattach to. Unlike tmux, it organizes them as a **tree** you can rearrange, and it **watches each pane to tell you what its agent is doing** — thinking, waiting for your approval, or done — so a glance at the sidebar tells you which session needs you.

```
╭  ≡ ● · Ilium──────────────────┬  ≡ ● · cargo run─────────────────────────────────────────────────────╮
│▼  🗂️   acme-api               │   Compiling acme-api v0.1.0 (/ram/acme-api)                          │
│›▼  📁   default               │    Finished `dev` profile [unoptimized + debuginfo] target(s)        │
│››   📟   shell                │     Running `target/debug/acme-api`                                  │
│››   📟   cargo test           │acme-api up                                                           │
│››   📟   cargo build --release│█                                                                     │
│››   📟   cargo run            │                                                                      │
│                               │                                                                      │
│                               │                                                                      │
╰───────────────────────────────┴──────────────────────────────────────────────────────────────────────╯
```

## Status

**Early. Expect rough edges.** ilium is usable day to day but has not been through a public release cycle, and the version is `0.1.0` for a reason.

**Linux is the tested platform.** macOS and Windows have compile-time fallbacks for the platform-specific pieces (process-tree walks, runtime directories, system sounds) but are not tested — reports and fixes welcome.

## What it gives you over tmux

- **A tree, not a grid.** Sessions hold groups, groups hold panes and nested groups, and any node can be dragged, reordered, indented, or outdented. Panes are terminals, built-in editors, or Kanban boards.
- **Agent state at a glance.** ilium detects Claude Code, Codex CLI, and Antigravity by walking the pane's process tree, then reads the screen to classify what that agent is doing right now.
- **Split views.** A container that shows up to four panes side by side, persistently, without losing the tree.
- **Real detach/reattach.** A background server owns the PTYs. Close your terminal, come back, everything is still running.
- **Per-project sessions.** Sessions are scoped to the directory you launch from, so `ilium` in two different projects gives you two independent workspaces.
- **Mouse support that passes through.** Clicks, drags, and scrolls reach `vim`, `htop`, or `lazygit` in whatever xterm encoding they negotiated.

### Pane states in the sidebar

| Icon/color | Meaning |
|---|---|
| 📟, default color | ordinary shell, no agent detected |
| 📟 + spinner | ordinary shell with output or keyboard activity in the last 60 seconds |
| ✦ yellow (pulsing) | agent is **working/thinking** |
| 🕛 animated clock | agent is **waiting on background tasks** it started |
| ✦ blue | agent is **waiting for your approval** (y/n prompt) |
| ✦ green | agent is **idle/done** — at its prompt with nothing running |

A finished turn stays marked `« [done] »` until you actually open that pane, so you cannot miss one while looking elsewhere.

## Install

Requirements:

- Linux (see [Status](#status))
- Rust **1.89 or newer** (`rustup` recommended)
- A terminal with 256-color and UTF-8 support

```sh
git clone https://github.com/arthurwolf/ilium
cd ilium
make install
```

That builds `ilium` and `ilium-server` in release mode and installs both into `$CARGO_HOME/bin` (`~/.cargo/bin` by default). Make sure that directory is on your `PATH`. To install elsewhere:

```sh
make install BIN_DIR=~/.local/bin
```

> **Why not `cargo install ilium`?** ilium depends on a patched `vt100` (an unreleased upstream fix for a resize panic) wired in through `[patch.crates-io]`. That patch only applies to this workspace, so a crates.io install would silently build against the broken version. Until the fix is released upstream, building from a clone is the supported path. See [ARCHITECTURE.md](ARCHITECTURE.md#a-note-on-the-vendored-vt100).

Both binaries are needed: the `ilium` CLI spawns `ilium-server` as a detached process. It looks for `ilium-server` next to the `ilium` executable first, falling back to your `PATH`, so as long as `make install` puts them in the same directory you only need that directory on your `PATH`.

## Quickstart

```sh
cd ~/code/my-project
ilium
```

That attaches to (or creates) this project's `default` session. Then:

- `Ctrl+A c` — new terminal pane
- `Ctrl+A !` — prompt for a command and run it in a new pane
- `Ctrl+B ↓` / `Ctrl+B ↑` — move between panes
- `Ctrl+A ?` — the full keyboard reference
- `Ctrl+A d` — detach; everything keeps running

Start an agent by opening a terminal and running `claude` or `codex` in it — ilium notices on its own, no configuration needed. The tree's right-click menu also has one-click entries for launching each supported agent.

### CLI

| Command | What it does |
|---|---|
| `ilium` | Attach to or create this project's `default` session |
| `ilium new-session <name>` | Create/attach a named session in this project |
| `ilium ls` | List this project's sessions and whether each is running |
| `ilium kill-session <name>` | Gracefully end a session and all its panes |
| `ilium new-pane --session <name> -- <cmd>` | Add a pane running `<cmd>` without attaching a TUI |
| `ilium chat …` | File-backed room so agents in a project can coordinate |

Useful flags: `--cwd <dir>` targets another project directory, `--restart-server` replaces the running server while keeping the session snapshot (use after installing a new build), and `--reset-session` deletes this project's snapshot and starts empty.

## Keybindings

Two prefixes, both remappable:

- **Leader — `Ctrl+A`** for commands (`[keyboard].shortcut_base`)
- **Tree navigation — `Ctrl+B`** for moving through the tree (`[keyboard].navigation_shortcut_base`)

`Ctrl+A ?` always shows the live table, including any remapping you have done.

### Tree navigation (`Ctrl+B`)

| Key | Action |
|---|---|
| `↓` / `↑` | Cycle to the next/previous pane in the current group |
| `PgDn` / `PgUp` | Jump to the first pane in the next/previous group |

### Commands (`Ctrl+A`)

| Key | Action |
|---|---|
| `c` | New terminal pane in the selected group |
| `e` | New editor pane (opens a file picker) |
| `B` | New board (choose storage format and location) |
| `g` | New group |
| `W` | New vertical or horizontal split view |
| `f` | Open a folder in the sidebar |
| `!` | Prompt for a command, run it in a new terminal pane |
| `x` | Close the selected pane or group |
| `r` | Rename the selected node |
| `m` | Move mode (up/down reorders, left/right outdents/indents) |
| `t` / `p` | Focus the tree panel / the active pane |
| `o` / `;` | Focus the next/previous visible pane |
| `h` `j` `k` `l` | Focus the visible pane left/down/up/right |
| `[` / `]` | Scroll the focused terminal one page up/down |
| `/` | Search terminal history and open editor buffers |
| `s` | Save the focused editor pane |
| `v` | Toggle editor Source/Rendered view (markdown only) |
| `n` / `b` / `a` | Toggle line numbers / minimap / autosave in the editor |
| `S` | Open settings |
| `?` | Show or hide the help screen |
| `d` | Detach this client, leave the session running |
| `Q` | Kill this project session and disconnect every client |

### Mouse and history

Click either panel to focus it. Tree rows support expand/collapse, hover reorder arrows, drag-and-drop reparenting, and right-click context menus.

Terminal history scrolls with the wheel or `Shift+PgUp`/`Shift+PgDn`. `Shift+End` jumps back to live output. `Ctrl+End` is forwarded to full-screen applications that handle it themselves (such as Claude Code).

## Configuration

Config lives at `~/.config/ilium/config.toml` and most of it is editable live from the Settings screen (`Ctrl+A S`), which writes the file for you.

| Table | Covers |
|---|---|
| `[detection]` | Fast/slow poll intervals, plus `[[detection.custom_signatures]]` to teach ilium about an agent CLI it doesn't ship with |
| `[keyboard]` | `shortcut_base`, `navigation_shortcut_base`, and per-action keybinding overrides |
| `[ui]` | Left-panel sizing policy, `tree_order`, per-provider agent icons, theme colors |
| `[sound]` | Which agent transitions play a sound, and which sound |
| `[notifications]` | Desktop notifications on agent completion |
| `[kanban_board]` | Card preview height and minimum column width |
| `[inference]` | Provider and model for optional LLM-assisted naming |
| `[http_api]` | `port` for the loopback automation listener (default `8872`) |
| `[debug]` | `file_logging_enabled` — off by default |

> **Note on the loopback HTTP API.** Each server binds `127.0.0.1:<port>` and serves `POST /create_agent`, which spawns an agent with a given prompt. It is bound to loopback and never a public interface, but it is **unauthenticated**, so any process running as your user can drive it. Change `[http_api].port` per project if you run several sessions at once — a server that cannot bind its port logs the failure and carries on without the API.

Session snapshots are stored per project in `<project>/.ilium/sessions/<name>.json`. Add `.ilium/` to your project's `.gitignore`.

### Optional LLM features

ilium can use an LLM to auto-name sessions and panes and to reorganize the tree. This is **optional and off the critical path** — every core feature (multiplexing, detection, splits, persistence) works without any credentials. Providers supported: Kilo Gateway (default, has a free tier), local Ollama, OpenAI-compatible endpoints, Anthropic, and OpenRouter. Configure under Settings → Inference, or turn the behavior off under Settings → Triggers.

Voice control is a separate opt-in feature requiring an OpenAI Realtime key; it is disabled unless you configure it.

Debug file logging is off by default because it records full HTTP and LLM request/response bodies. Credential headers and URL parameters are redacted when it is on.

## How it works

A short version: one `ilium-server` process per project session owns every PTY and the tree; the TUI is a thin client talking to it over a Unix socket with length-prefixed bincode frames. Agent identity comes from walking the pane's child process tree (robust), and agent activity comes from scanning the rendered screen (the only way to know if a turn is in progress). Panes that are working get polled fast; idle ones get polled slowly.

The long version — crate boundaries, the detection design, the wire protocol, and the milestone history — is in **[ARCHITECTURE.md](ARCHITECTURE.md)**.

## Alternatives

If ilium isn't the right fit, [Zellij](https://github.com/zellij-org/zellij) is the mature Rust multiplexer, [claude-squad](https://github.com/smtg-ai/claude-squad) drives tmux plus git worktrees for parallel agents, and **herdr** already ships agent-state detection in a flat sidebar. ARCHITECTURE.md has a fuller [comparison](ARCHITECTURE.md#prior-art--why-not-just-use-x).

## Contributing

Issues and pull requests are welcome. Before submitting:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets
cargo test --workspace
```

[CLAUDE.md](CLAUDE.md) documents the layering rules and conventions this codebase is held to; it is worth skimming before a non-trivial change.

## License

MIT — see [LICENSE](LICENSE).

Two dependencies are vendored in-tree and keep their own licenses: `vendor/vt100` (MIT, © Jesse Luehrs) carries an unreleased upstream fix, and `ilium-client/vendor/tui-tree-widget` (MIT, © EdJoPaTo) is a local fork. The bundled Cascadia Code font is licensed under the SIL Open Font License 1.1 (© Microsoft Corporation); see `ilium-client/assets/fonts/NOTICE.md`.
