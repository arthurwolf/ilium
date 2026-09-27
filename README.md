# <img src="assets/ilium-mark.svg" alt="" width="34" height="34"> Ilium

**Run several coding agents in one terminal.** Ilium keeps your terminals, editors, and boards in a tree, and shows which agents are working or need you.

Linux: [install the prerequisites](#install-and-first-run), then run this command. Automatic titles and tree organization use [Kilo Gateway over the network by default](#inference-and-privacy), without requiring an API key.

```sh
git clone https://github.com/arthurwolf/ilium.git && cd ilium && make install && ./target/release/ilium
```

## See it in action

<table width="100%">
<tbody>
<tr>
<td width="50%" valign="top">
<p>
<strong>AI organizes your workspace</strong>
</p>
<p>
<a href="assets/demos/01-ai-tree.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/01-ai-tree.gif" alt="AI reorganizes the tree and opens agents whose work matches their new titles" width="100%">
</a>
</p>
</td>
<td width="50%" valign="top">
<p>
<strong>Voice control (simulated speech)</strong>
</p>
<p>
<a href="assets/demos/02-voice-control.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/02-voice-control.gif" alt="Typed voice commands use a loopback model to drive real Ilium actions and a real Claude prompt; the shell-command scene is composited from a separate take" width="100%">
</a>
</p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p>
<strong>Automatic agent titles</strong>
</p>
<p>
<a href="assets/demos/03-contextual-titles.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/03-contextual-titles.gif" alt="Ilium titles agent panes from their content while preserving manual names" width="100%">
</a>
</p>
</td>
<td width="50%" valign="top">
<p>
<strong>Schedule input</strong>
</p>
<p>
<a href="assets/demos/04-scheduled-input.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/04-scheduled-input.gif" alt="A five-second timer sends text first to an agent and then to a terminal" width="100%">
</a>
</p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p>
<strong>Move screens between panes</strong>
</p>
<p>
<a href="assets/demos/05-screen-transfer.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/05-screen-transfer.gif" alt="The visible screen is transferred between panes" width="100%">
</a>
</p>
</td>
<td width="50%" valign="top">
<p>
<strong>Agent costs and usage</strong>
</p>
<p>
<a href="assets/demos/06-cost-and-stats.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/06-cost-and-stats.gif" alt="The agent stats popover shows transcript-backed usage and activity" width="100%">
</a>
</p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p>
<strong>Interface and board settings</strong>
</p>
<p>
<a href="assets/demos/07a-settings-tour-a.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/07a-settings-tour-a.gif" alt="The first chapter of the settings tour shows interface and board options" width="100%">
</a>
</p>
</td>
<td width="50%" valign="top">
<p>
<strong>Voice and AI provider settings</strong>
</p>
<p>
<a href="assets/demos/07b-settings-tour-b.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/07b-settings-tour-b.gif" alt="The second chapter of the settings tour shows voice and inference providers" width="100%">
</a>
</p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p>
<strong>Automatic title settings</strong>
</p>
<p>
<a href="assets/demos/07c-settings-tour-c.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/07c-settings-tour-c.gif" alt="The third chapter of the settings tour shows automatic titles and setup options" width="100%">
</a>
</p>
</td>
<td width="50%" valign="top">
<p>
<strong>Agent status (simulated goal icon)</strong>
</p>
<p>
<a href="assets/demos/08-agent-activity.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/08-agent-activity.gif" alt="Real agent activity with a disclosed simulated Codex objective icon" width="100%">
</a>
</p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p>
<strong>Rearrange panes without restarting</strong>
</p>
<p>
<a href="assets/demos/09-pane-tree.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/09-pane-tree.gif" alt="Move a pane into a group and reorder it without restarting its process" width="100%">
</a>
</p>
</td>
<td width="50%" valign="top">
<p>
<strong>Agents, editors, and terminals side by side</strong>
</p>
<p>
<a href="assets/demos/10-mixed-splits.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/10-mixed-splits.gif" alt="A split view displays several terminal panes together" width="100%">
</a>
</p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p>
<strong>Select and copy terminal output</strong>
</p>
<p>
<a href="assets/demos/11-smart-copy.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/11-smart-copy.gif" alt="Smart Copy selects a region of real terminal output and copies it" width="100%">
</a>
</p>
</td>
<td width="50%" valign="top">
<p>
<strong>Request agent updates</strong>
</p>
<p>
<a href="assets/demos/12-ask-for-update.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/12-ask-for-update.gif" alt="Ask for update sends a prompt to active agents" width="100%">
</a>
</p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p>
<strong>Queue prompts</strong>
</p>
<p>
<a href="assets/demos/13-prompt-queue.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/13-prompt-queue.gif" alt="Queued prompts reach an agent in order as its turns finish" width="100%">
</a>
</p>
</td>
<td width="50%" valign="top">
<p>
<strong>Background tasks report back</strong>
</p>
<p>
<a href="assets/demos/14-progress-monitor.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/14-progress-monitor.gif" alt="A detached progress monitor reports a task result to an agent" width="100%">
</a>
</p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p>
<strong>Built-in Kanban board</strong>
</p>
<p>
<a href="assets/demos/15-kanban.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/15-kanban.gif" alt="A card moves through the built-in Kanban board" width="100%">
</a>
</p>
</td>
<td width="50%" valign="top">
<p>
<strong>Fix TODOs with Codex</strong>
</p>
<p>
<a href="assets/demos/16-agent-from-line.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/16-agent-from-line.gif" alt="Create a real Codex agent from a source TODO; its goal includes the file path and line, and it repairs the source" width="100%">
</a>
</p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p>
<strong>Search agents, files, and terminals (edited replay)</strong>
</p>
<p>
<a href="assets/demos/17-workspace-search.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/17-workspace-search.gif" alt="Workspace search returns real agent, shell, and file matches and jumps to their lines; disclosed terminal-cell repaint" width="100%">
</a>
</p>
</td>
<td width="50%" valign="top">
<p>
<strong>Detach without stopping agents</strong>
</p>
<p>
<a href="assets/demos/18-detach-reattach.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/18-detach-reattach.gif" alt="Detach from a session and reattach while its agent and panes keep running" width="100%">
</a>
</p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p>
<strong>Built-in Markdown editor</strong>
</p>
<p>
<a href="assets/demos/19-markdown-editor.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/19-markdown-editor.gif" alt="Edit a Markdown note inside Ilium" width="100%">
</a>
</p>
</td>
<td width="50%" valign="top">
<p>
<strong>Codex–Claude handoff (simulated sidebar titles)</strong>
</p>
<p>
<a href="assets/demos/20-chatroom.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/20-chatroom.gif" alt="Real Codex and Claude Chatroom exchange with disclosed simulated sidebar titles" width="100%">
</a>
</p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p>
<strong>Auto-answer confirmation prompts</strong>
</p>
<p>
<a href="assets/demos/21-text-triggers.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/21-text-triggers.gif" alt="A text trigger responds to an agent&#x27;s confirmation question" width="100%">
</a>
</p>
</td>
<td width="50%" valign="top">
<p>
<strong>Live agent goals</strong>
</p>
<p>
<a href="assets/demos/22-goal-indicators.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/22-goal-indicators.gif" alt="A real Codex goal changes the objective icon in the pane tree" width="100%">
</a>
</p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p>
<strong>Restore panes and resume agents</strong>
</p>
<p>
<a href="assets/demos/23-snapshot-resume.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/23-snapshot-resume.gif" alt="Ilium restores panes and real Claude and Codex sessions answer from their resumed context; setup and wait periods are cut" width="100%">
</a>
</p>
</td>
<td width="50%" valign="top">
<p>
<strong>Agents survive reboots</strong>
</p>
<p>
<a href="assets/demos/24-agents-survive-reboots.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/24-agents-survive-reboots.gif" alt="A real Ilium terminal runs a safely simulated reboot command; CRT shutoff footage and staged vintage boot scenes lead to retained footage of Ilium restoring the pane layout and resuming Claude and Codex sessions. This is a composite, not a live machine reboot capture." width="100%">
</a>
</p>
</td>
</tr>
</tbody>
</table>

Reboot demo credits: [CRT turn-off footage by Snowman Digital](https://www.youtube.com/watch?v=jntnUWWkZlA); [IBM VGA font from Oldschool PC Font Pack](https://int10h.org/oldschool-pc-fonts/) (CC BY-SA 4.0). The reboot and boot screens are simulated; the Ilium terminal and session recovery footage are real captures.

## What Ilium does

- **Groups and splits.** Move panes into groups, folders, and splits. A split shows up to four panes while one receives keyboard input. The tree stays visible even when the right-hand pane is busy.
- **Agent activity.** Ilium finds the CLI in a pane's process tree, reads its visible terminal state, and marks approval requests, background work, completed turns, goals, and monitored jobs. Open a completed pane to clear its unread marker. Detection can lag a fast transition or an agent UI change.
- **Persistent sessions.** `Ctrl+B d` detaches the client; your terminals and agents keep running. Come back to the same project directory and run `ilium` to reattach. A server restart or machine reboot starts *new* processes from a saved snapshot.
- **Separate checkouts.** `Ctrl+B W` starts an agent in a new Git worktree. Ilium previews the branch and path, records which worktrees it created, and refuses removal when files, processes, or ownership checks make it unsafe.
- **Copy and search.** Smart Copy freezes the visible screen and offers local regions to copy immediately; an inference model can add suggestions. The model selects coordinates on the frozen screen, not replacement clipboard text. Search jumps through agent, shell, and editor content.
- **Queues and background jobs.** Schedule text or a keypress for later, queue prompts for an agent, or let an agent register a server-owned progress monitor. Chatroom gives agents in the same project a file-backed place for coordination.

The pane toolbar also opens transcript-backed **Costs & stats** for Claude Code and Codex: tokens, activity, prompts, and cost when the agent recorded one. Ilium does not estimate dollar cost; the reported snapshot can trail live use. Optional voice control uses OpenAI Realtime and an attached client.

### The sidebar

In **Normal** monitoring mode, a row can show identity, longer-running work, and current activity in separate icon slots. **Attention** mode shows the highest-priority item needing action. Hover an icon for its meaning and the reason Ilium chose it; change icons or monitoring mode in Settings. An agent with a running registered job shows as waiting for that job, without a finished-turn notification. A `/goal` badge reports a state Ilium observed; Ilium does not control the agent's goal.

## Install and first run

Linux is the primary platform. Ilium is at `0.1.0`; macOS and Windows CI jobs exist, but the ports still have [unresolved failures](https://github.com/arthurwolf/ilium/actions). Use a UTF-8 terminal with 256-colour support. Install and sign in to the agent CLI you want to run separately.

You need Git, Make, rustup/Cargo, a C toolchain, and native development libraries. On Ubuntu, CI installs `libasound2-dev`, `pkg-config`, and `libssl-dev`:

```sh
sudo apt-get update
sudo apt-get install -y libasound2-dev pkg-config libssl-dev
```

The one-line command at the top builds both `ilium` and `ilium-server`. `make install` puts them in `$CARGO_HOME/bin` (normally `~/.cargo/bin`); add that directory to `PATH`. Or, from the clone, choose a different directory:

```sh
make install BIN_DIR="$HOME/.local/bin"
```

The manifest declares Rust 1.89 as its minimum; `rust-toolchain.toml` pins 1.96.1 for this checkout. Build from the clone so Cargo uses its [patched `vt100`](ARCHITECTURE.md#a-note-on-the-vendored-vt100). A plain `cargo install ilium` misses that workspace patch. Keep the client and server binaries together: the client looks beside itself, then on `PATH`, for `ilium-server`.

From the directory of a project you want to work on:

```sh
ilium
```

Press `Ctrl+B c` for a terminal pane. Run `claude` or `codex` there if installed; Ilium detects it without a special launch wrapper. `Ctrl+B ?` opens the live key reference, and `Ctrl+B d` detaches. Run `ilium` in that directory again to reattach.

## Worktrees and session recovery

Start a worktree agent with the tree footer button, a project menu, `Ctrl+B W`, or the CLI. The form shows the provider, new branch, starting ref, and checkout path before it changes Git. Ilium can also open an existing worktree, but it does not claim ownership of one it did not create.

```sh
ilium new-pane --worktree --branch agent/fix-login -- codex
```

Change `agent/fix-login` to an unused branch. `--base <ref>` overrides the repository's configured default base. The command writes JSONL progress and a result. Git hooks and filters run during creation; a Linux-only post-create command can be set in Settings → Git for projects using submodules or LFS.

If creation leaves a checkout behind after a failure, Ilium reports its path for inspection. Its **Worktree** and **Manage worktrees…** menus show ownership and removal blockers. It will not remove a foreign checkout. For one it owns, removal keeps the branch by default; discarding files requires the full path, and branch deletion is a separate choice. The optional close-pane cleanup offer appears only for a clean, merged worktree with no other pane or unresolved ownership claim. Unclear file or process state blocks cleanup.

Each project session has a detached server and a snapshot at `<project>/.ilium/sessions/<name>.json`. Automatic recovery and rolling backups are on by default; snapshot copies live under `<project>/.ilium/backups/<name>/`. Add `.ilium/` to your project's `.gitignore` if you do not want local session data committed. These backups cover the session snapshot, not files edited inside pane applications.

When the server starts again, Ilium rebuilds the tree and relaunches pane programs. It can ask Claude Code, Codex, or Antigravity to resume a verified session when that provider still has its session data. Original processes and unsaved in-process state do not survive a restart or reboot. `--restart-server` retains the snapshot while replacing a running server; `--reset-session` deletes the targeted snapshot and starts empty. `ilium kill-session <name>` ends that session and its panes.

## Settings and privacy

Settings (`Ctrl+B :`) covers the tree, icons, monitoring, keybindings, detection, Git, notifications, titles, inference, and automation. On Linux, global settings live in `~/.config/ilium/config.toml`. The two prefixes are in `[keyboard]`; per-action remaps are in `[keybindings]`. `Ctrl+B ?` shows the effective keys after remapping.

### Inference and privacy

The multiplexer, splits, detection, and session storage work without an LLM. AI titles, tree organization, and extra Smart Copy suggestions use the selected inference provider. On a fresh install, automatic title and tree triggers are **on**, and Kilo Gateway is selected with `stepfun/step-3.7-flash:free`. Open Settings → Triggers to disable automatic requests, or Settings → Inference to select local Ollama or another provider. Do this before putting sensitive content in a new session if you want to prevent those default network requests.

[Kilo's authentication guide](https://kilo.ai/docs/gateway/authentication) says anonymous free models need no key and are limited by public IP. On 2026-09-27, its [model catalog](https://api.kilo.ai/api/gateway/models) marked the default model free and `mayTrainOnYourPrompts: true`. Free model availability and data handling can change; check [Kilo's current guidance](https://kilo.ai/docs/getting-started/using-kilo-for-free). OpenAI-compatible, Anthropic, and OpenRouter providers are also available. Kilo paid-proxy egress via MongoDB is an advanced, hand-edited setting, not part of the default route.

Debug file logging is off by default. If enabled, it can retain HTTP/LLM request bodies and project prompts even though credential headers and URL parameters are redacted. Treat those logs as sensitive.

### Automation and agent setup

Scheduled input and text triggers can submit commands to a pane. Review their target and response before enabling them for confirmation prompts. Reset planning follows public Claude/Codex reset announcements; those are not your account's private rolling limits.

Ilium offers optional Chatroom and progress instructions at startup or in Settings → Setup. If selected, it writes marker-delimited blocks to the relevant Claude/Codex instruction files, preserving text outside the blocks. Chatroom setup also creates or repairs `CHATROOM.md` and agent hooks. The default files are `~/.claude/CLAUDE.md` and `<project>/CLAUDE.md` for Claude Code, and `~/.codex/AGENTS.md` and `<project>/AGENTS.md` for Codex.

For a job expected to last at least three minutes, an agent can validate a cheap JSON probe with `ilium progress check`, register it with `ilium progress set`, then wait for Ilium to send the final result. The detached server polls the probe from the project root. The probe needs a stable `job_id`, a status, a percentage, and bounded details; use absolute paths because the pane shell's current directory is not inherited. See `ilium progress --help` for exact flags. This monitor does not manage an agent's `/goal`.

A project server also listens on `127.0.0.1:8872` by default for unauthenticated `POST /create_agent`. Any local process that can reach that port can submit a request. Change `[http_api].port` if project sessions collide, and do not expose the listener through a port forward or reverse proxy without protection. A bind failure disables that listener while the session continues.

### Voice

Voice control is off until configured. It needs an OpenAI Realtime key, network access, and an attached interactive client; microphone use needs audio devices. Press `F8` or use Settings → Voice control. The model can navigate Ilium and send text to a terminal or agent, so check the selected target. Destructive semantic actions have separate confirmation; terminal submission normally includes Enter unless you enable its optional confirmation.

`ilium voice say` feeds typed sentences to the same live voice conversation:

```sh
ilium voice say "open the settings"
ilium voice say --start "what agents are running?"
printf '%s\n' "focus the first agent" "say hello to it" | ilium voice say -
```

`--start` saves the voice-on setting. A lone `-` reads nonempty stdin lines; `--` protects a sentence beginning with `-`. Each request accepts up to 32 sentences of 4,000 characters each. From outside an Ilium pane, select a session with `--cwd` and `--session-name`. The command needs an attached client and does not start one. Its JSONL `result` confirms queueing, **not** that the remote model acted; `error` records carry a code and hint. See `ilium voice say --help` for timeouts and other flags.

## Keys and commands

The command and navigation prefixes default to `Ctrl+B`. Inside tmux with the same prefix, press it twice to pass it through, or remap Ilium's prefix. Press `Ctrl+B ?` for the full, current keymap.

| After `Ctrl+B` | Action |
| --- | --- |
| `c` / `e` / `B` | New terminal / editor / board |
| `W` | New agent in a worktree |
| `g` / `"` | New group / split view |
| `m` / `,` | Move / rename a tree entry |
| `f` / `[` / `]` | Search / scroll the pane |
| `:` / `?` | Settings / help |
| `d` / `&` | Detach / kill the session |

Mouse actions include focusing panes, moving tree entries, context menus, and scrolling terminal history. The editor can save files, render Markdown, and toggle line numbers, minimap, or autosave. A board stores cards in a Markdown file or folder you select.

| CLI command | What it does |
| --- | --- |
| `ilium` | Attach to or create the project's `default` session. |
| `ilium new-session <name>` / `ilium ls` | Create or attach to a named session / list project sessions. |
| `ilium new-pane -- <cmd>` | Add a terminal pane without attaching a TUI. |
| `ilium new-pane --session-name <name> -- <cmd>` | Add one to a named session. |
| `ilium chat --help` / `ilium progress --help` | Show Chatroom and progress commands. |
| `ilium voice say --help` | Show typed voice input. |

Run commands from the intended project directory or use `--cwd`. Angle brackets in the table are placeholders. `ilium --help` has the full CLI reference.

## How it works

One `ilium-server` per project session owns the PTYs, tree, scheduled input, and snapshot. The terminal client renders that state over local IPC: a Unix socket on Unix, a named pipe on Windows. Detaching the client leaves the server and pane processes running. Ilium identifies an agent from its child processes and classifies activity from its rendered screen; the two observations can disagree briefly. [ARCHITECTURE.md](ARCHITECTURE.md) covers the crate boundaries and design choices.

## Project, help, and licence

Ilium is early software. [CI runs](https://github.com/arthurwolf/ilium/actions) show the current platform results; [issues](https://github.com/arthurwolf/ilium/issues) are the place for failures and installation questions. Include the Ilium version, OS, terminal, command, and observed behaviour, with private content removed from logs. [AGENTS.md](AGENTS.md) and [ARCHITECTURE.md](ARCHITECTURE.md) cover contribution rules and design. The workspace check commands are `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace --no-fail-fast`.

If you need an established general multiplexer, see [tmux](https://github.com/tmux/tmux) or [Zellij](https://github.com/zellij-org/zellij). [claude-squad](https://github.com/smtg-ai/claude-squad) combines tmux with agent worktrees; [the prior-art discussion](ARCHITECTURE.md#prior-art--why-not-just-use-x) explains Ilium's different choices.

Ilium is [MIT licensed](LICENSE). The vendored `vt100` patch and `tui-tree-widget` fork retain their own MIT licences. The bundled Cascadia Code font uses SIL Open Font License 1.1; see its [notice](ilium-client/assets/fonts/NOTICE.md).
