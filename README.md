<p align="center"><img src="assets/ilium-mark.svg" alt="" width="68" height="68"></p>
<h1 align="center">Ilium</h1>
<p align="center"><strong>AI names agents and organizes your project tree.</strong><br>Keep agent work, terminals, editors, and boards together, with activity in view.</p>
<p align="center"><sub>Linux-first · Rust · MIT</sub></p>
<p align="center"><a href="#quick-start">Quick start</a> · <a href="#daily-use">Daily use</a> · <a href="#see-it-in-action">Demos</a> · <a href="#worktrees-and-session-recovery">Worktrees</a> · <a href="#full-reference">Reference</a></p>

## Quick start

Linux is the primary platform. [Install the prerequisites](#install-and-platform-support), then build Ilium and point it at a project:

```sh
git clone https://github.com/arthurwolf/ilium.git
cd ilium
make install
./target/release/ilium --cwd /absolute/path/to/project
```

Use a UTF-8 terminal with 256-colour support. Install your agent CLI separately.

> **Network by default:** fresh installs send automatic title and tree-organization requests to Kilo Gateway. Disable those triggers or choose a local provider before entering sensitive content. [See the inference settings](#inference-and-privacy).

## Daily use

Prefix: `Ctrl+B`.

| After prefix | Action |
| --- | --- |
| `c` / `e` / `B` | New terminal / editor / board |
| `W` | New agent in a Git worktree |
| `g` / `"` | New group / split view |
| `m` / `,` | Move / rename the selected entry |
| `f` | Search agents, terminals, and files |
| `[` / `]` | Scroll pane history |
| `:` / `?` | Settings / key reference |
| `d` / `&` | Detach / kill the session |

Reattach: run `ilium` in the project directory. In tmux, double the prefix to pass it through. Remap it in Settings.

## What Ilium does

<table width="100%">
<tbody>
<tr>
<td width="50%" valign="top"><strong>Arrange</strong><br>Group panes and split the screen four ways. The project tree stays visible.</td>
<td width="50%" valign="top"><strong>Track</strong><br>Identify agent CLIs from process trees and read activity from terminal screens.</td>
</tr>
<tr>
<td width="50%" valign="top"><strong>Separate</strong><br>Start agents in linked Git worktrees. Unclear file, process, or ownership state blocks cleanup.</td>
<td width="50%" valign="top"><strong>Resume</strong><br>Detach without stopping panes. Recovery rebuilds the layout and relaunches pane programs.</td>
</tr>
</tbody>
</table>

Also included: workspace search, Smart Copy, scheduled input, prompt queues, Chatroom, and transcript-backed Costs &amp; stats for Claude Code and Codex. Costs are reported, not estimated; the snapshot can lag live use.

### Agent monitoring

Normal mode separates agent identity, longer-running work, and current activity. Attention mode shows the highest-priority status. Hover for the reason. A live progress monitor can suppress the finished-turn alert while the agent is idle. A `/goal` badge reports observed state; Ilium does not control the agent.

## See it in action

<table width="100%">
<tbody>
<tr>
<td width="50%" valign="top">
<p>
<strong>AI names agents and organizes the project tree</strong>
</p>
<p>
<a href="assets/demos/01-ai-tree.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/01-ai-tree.gif" alt="Ilium names agent panes and organizes the project tree around their work" width="100%">
</a>
</p>
</td>
<td width="50%" valign="top">
<p>
<strong>Control Ilium and prompt agents with voice</strong>
</p>
<p>
<a href="assets/demos/02-voice-control.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/02-voice-control.gif" alt="Voice commands trigger Ilium actions and send a prompt to Claude" width="100%">
</a>
</p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p>
<strong>Give agent panes automatic titles</strong>
</p>
<p>
<a href="assets/demos/03-contextual-titles.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/03-contextual-titles.gif" alt="Ilium titles agent panes from their content while preserving manual names" width="100%">
</a>
</p>
</td>
<td width="50%" valign="top">
<p>
<strong>Transfer a terminal screen between panes</strong>
</p>
<p>
<a href="assets/demos/05-screen-transfer.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/05-screen-transfer.gif" alt="Transfer the visible screen to another pane" width="100%">
</a>
</p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p>
<strong>Inspect agent usage and recorded costs</strong>
</p>
<p>
<a href="assets/demos/06-cost-and-stats.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/06-cost-and-stats.gif" alt="Review an agent’s tokens, activity, prompts, and recorded cost" width="100%">
</a>
</p>
</td>
<td width="50%" valign="top">
<p>
<strong>Track agent status and current goals</strong>
</p>
<p>
<a href="assets/demos/08-agent-activity.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/08-agent-activity.gif" alt="Agent status changes as work progresses, with its goal shown in the tree" width="100%">
</a>
</p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p>
<strong>Use agents, editors, and terminals side by side</strong>
</p>
<p>
<a href="assets/demos/10-mixed-splits.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/10-mixed-splits.gif" alt="View terminal, editor, and board panes together in a split view" width="100%">
</a>
</p>
</td>
<td width="50%" valign="top">
<p>
<strong>Select and copy terminal output</strong>
</p>
<p>
<a href="assets/demos/11-smart-copy.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/11-smart-copy.gif" alt="Select a region of terminal output and copy it" width="100%">
</a>
</p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p>
<strong>Queue prompts for an agent</strong>
</p>
<p>
<a href="assets/demos/13-prompt-queue.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/13-prompt-queue.gif" alt="Queued prompts reach the agent in order as its turns finish" width="100%">
</a>
</p>
</td>
<td width="50%" valign="top">
<p>
<strong>Get results from background tasks</strong>
</p>
<p>
<a href="assets/demos/14-progress-monitor.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/14-progress-monitor.gif" alt="A background task reports its result to an agent" width="100%">
</a>
</p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p>
<strong>Search agents, files, and terminals</strong>
</p>
<p>
<a href="assets/demos/17-workspace-search.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/17-workspace-search.gif" alt="Find agents, files, and terminals, then jump to a match" width="100%">
</a>
</p>
</td>
<td width="50%" valign="top">
<p>
<strong>See live agent goals in the project tree</strong>
</p>
<p>
<a href="assets/demos/22-goal-indicators.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/22-goal-indicators.gif" alt="A live Codex goal changes the objective icon in the project tree" width="100%">
</a>
</p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p>
<strong>Resume agent sessions after a reboot</strong>
</p>
<p>
<a href="assets/demos/24-agents-survive-reboots.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/24-agents-survive-reboots.gif" alt="After a reboot sequence, Ilium restores the pane layout and resumes Claude and Codex" width="100%">
</a>
</p>
</td>
<td width="50%" valign="top"></td>
</tr>
</tbody>
</table>

<details>
<summary>Show the other 14 demos</summary>

<table width="100%">
<tbody>
<tr>
<td width="50%" valign="top">
<p>
<strong>Schedule input for a pane</strong>
</p>
<p>
<a href="assets/demos/04-scheduled-input.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/04-scheduled-input.gif" alt="A five-second timer sends text to an agent and then to a terminal" width="100%">
</a>
</p>
</td>
<td width="50%" valign="top">
<p>
<strong>Set interface and board preferences</strong>
</p>
<p>
<a href="assets/demos/07a-settings-tour-a.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/07a-settings-tour-a.gif" alt="The first chapter of the settings tour shows interface and board options" width="100%">
</a>
</p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p>
<strong>Configure voice and AI providers</strong>
</p>
<p>
<a href="assets/demos/07b-settings-tour-b.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/07b-settings-tour-b.gif" alt="The second chapter of the settings tour shows voice and inference providers" width="100%">
</a>
</p>
</td>
<td width="50%" valign="top">
<p>
<strong>Configure automatic titles</strong>
</p>
<p>
<a href="assets/demos/07c-settings-tour-c.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/07c-settings-tour-c.gif" alt="The third chapter of the settings tour shows automatic titles and setup options" width="100%">
</a>
</p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p>
<strong>Rearrange panes without stopping their processes</strong>
</p>
<p>
<a href="assets/demos/09-pane-tree.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/09-pane-tree.gif" alt="Move a pane into a group and reorder it while its process keeps running" width="100%">
</a>
</p>
</td>
<td width="50%" valign="top">
<p>
<strong>Request updates from active agents</strong>
</p>
<p>
<a href="assets/demos/12-ask-for-update.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/12-ask-for-update.gif" alt="Send an update request to active agents" width="100%">
</a>
</p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p>
<strong>Track work on a Kanban board</strong>
</p>
<p>
<a href="assets/demos/15-kanban.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/15-kanban.gif" alt="Move a card through the Kanban board" width="100%">
</a>
</p>
</td>
<td width="50%" valign="top">
<p>
<strong>Start a Codex agent from a TODO</strong>
</p>
<p>
<a href="assets/demos/16-agent-from-line.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/16-agent-from-line.gif" alt="Start Codex from a TODO line and have it repair the source" width="100%">
</a>
</p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p>
<strong>Detach and reconnect while agents keep running</strong>
</p>
<p>
<a href="assets/demos/18-detach-reattach.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/18-detach-reattach.gif" alt="Reattach while agents and panes keep running" width="100%">
</a>
</p>
</td>
<td width="50%" valign="top">
<p>
<strong>Edit Markdown in a pane</strong>
</p>
<p>
<a href="assets/demos/19-markdown-editor.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/19-markdown-editor.gif" alt="Edit a Markdown note inside Ilium" width="100%">
</a>
</p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p>
<strong>Coordinate Claude and Codex with Chatroom</strong>
</p>
<p>
<a href="assets/demos/20-chatroom.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/20-chatroom.gif" alt="Claude and Codex exchange a handoff through Chatroom" width="100%">
</a>
</p>
</td>
<td width="50%" valign="top">
<p>
<strong>Respond to confirmation prompts automatically</strong>
</p>
<p>
<a href="assets/demos/21-text-triggers.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/21-text-triggers.gif" alt="A text trigger responds to an agent confirmation question" width="100%">
</a>
</p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p>
<strong>Restore panes and resume agent sessions</strong>
</p>
<p>
<a href="assets/demos/23-snapshot-resume.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/23-snapshot-resume.gif" alt="Ilium restores panes and Claude and Codex continue from resumed context" width="100%">
</a>
</p>
</td>
<td width="50%" valign="top">
<p>
<strong>Run agents in their own worktrees</strong>
</p>
<p>
<a href="assets/demos/25-worktree-agent.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/25-worktree-agent.gif" alt="A locally labelled Codex-shaped fixture runs on agent/map-east-side; a terminal shows its worktree path, branch, and file diff" width="100%">
</a>
</p>
</td>
</tr>
</tbody>
</table>

</details>

## Worktrees and session recovery

Start an agent from the tree, a project menu, `Ctrl+B W`, or the CLI:

```sh
ilium new-pane --worktree --branch agent/fix-login -- codex
```

Use an unused branch; `--base <ref>` overrides the repository default. Creation runs Git hooks and filters and reports JSONL progress. Settings → Git has a Linux post-create command for submodules and LFS.

Ilium removes only worktrees it created. It keeps branches by default; discarding files requires the full path, and branch deletion is separate. Cleanup is offered only for clean, merged worktrees with no other pane or unresolved ownership claim. Unclear file or process state blocks removal. If creation fails after Git leaves a checkout behind, Ilium reports its path.

Snapshots live at `<project>/.ilium/sessions/<name>.json`; rolling snapshot backups live under `.ilium/backups/`. Add `.ilium/` to `.gitignore` if session data should stay out of the repository. Backups do not include files edited inside pane applications.

After a server restart or reboot, Ilium rebuilds the tree and relaunches pane programs. It can request a verified Claude Code, Codex, or Antigravity session when that provider's data still exists; the original processes and unsaved in-process state do not survive. `--restart-server` keeps the snapshot, `--reset-session` deletes it, and `ilium kill-session <name>` ends the session and its panes.

## Full reference

### Install and platform support

Ilium is version `0.1.0`. Linux is the primary platform; macOS and Windows have platform-specific transport, process, and PTY implementations. The current CI workflow builds and tests the full workspace, with different coverage and results by OS:

| Platform | Implementation and limits | Automated checks | Latest public CI for `ed82eb0` |
| --- | --- | --- | --- |
| Linux | Primary platform. Worktree post-create setup is available. | Workspace build and tests, plus formatting and Clippy. | [Run #102](https://github.com/arthurwolf/ilium/actions/runs/36311258075/job/108597492305): formatting and Clippy passed; workspace tests failed. |
| macOS | Platform support is implemented; worktree post-create setup is Linux-only. | Workspace build and tests on pushes to `master` and manual runs; skipped on pull requests. | [Run #102](https://github.com/arthurwolf/ilium/actions/runs/36311258075/job/108597491928): build passed; workspace tests failed. |
| Windows | Platform support is implemented; worktree post-create setup is Linux-only. Most PTY integration tests are Unix-only; one ConPTY smoke test runs on Windows. | Workspace build and tests. | [Run #102](https://github.com/arthurwolf/ilium/actions/runs/36311258075/job/108597492028): build failed, so tests did not run. |

Run #102 is the latest public CI result checked on 2026-09-27. The public run page reports job and step outcomes; it does not expose the underlying failure details while logged out. Check [CI](https://github.com/arthurwolf/ilium/actions) for newer results. Builds need Git, Make, rustup/Cargo, a C toolchain, and native development libraries. Ubuntu also needs:

```sh
sudo apt-get update
sudo apt-get install -y libasound2-dev pkg-config libssl-dev
```

`make install` builds both binaries into `$CARGO_HOME/bin` (normally `~/.cargo/bin`). Add it to `PATH`, or set `BIN_DIR`:

```sh
make install BIN_DIR="$HOME/.local/bin"
```

The manifest requires Rust 1.89; this checkout pins 1.96.1. Build from the clone to use Ilium's [patched `vt100`](ARCHITECTURE.md#a-note-on-the-vendored-vt100); `cargo install ilium` skips that patch. Keep `ilium` and `ilium-server` together: the client searches beside itself, then on `PATH`, for the server.

### Settings, inference, and privacy

Open Settings with `Ctrl+B :`. On Linux, global settings are in `~/.config/ilium/config.toml`; `[keyboard]` sets prefixes and `[keybindings]` remaps actions. `Ctrl+B ?` shows the active map.

#### Inference and privacy

The multiplexer, detection, and session storage work without an LLM. AI titles, tree organization, and optional Smart Copy suggestions use the selected provider. New installs enable title and tree triggers and use Kilo Gateway's `stepfun/step-3.7-flash:free`. Disable the triggers or choose a local provider such as Ollama before entering sensitive content.

Kilo's [authentication guide](https://kilo.ai/docs/gateway/authentication) says anonymous free models need no key and are limited by public IP. Its [2026-09-27 model catalog](https://api.kilo.ai/api/gateway/models) marked the default model free and `mayTrainOnYourPrompts: true`; check [current guidance](https://kilo.ai/docs/getting-started/using-kilo-for-free) because availability and data handling can change. OpenAI-compatible, Anthropic, and OpenRouter providers are also available. MongoDB paid-proxy egress is an advanced hand-edited setting, not the default.

File logging is off by default. When enabled, logs can retain HTTP/LLM request bodies and project prompts; credential headers and URL parameters are redacted. Treat logs as sensitive.

### Automation and agent setup

Scheduled input and text triggers can send commands to panes. Check the target and result when using them around confirmation prompts. Reset planning follows public Claude and Codex announcements and possible-reset forecasts; it cannot know private rolling limits or promise that a forecast will happen.

Optional setup writes marked Chatroom or progress instructions to Claude and Codex files, preserving text outside those blocks. Chatroom setup also creates or repairs `CHATROOM.md` and agent hooks. The files are `~/.claude/CLAUDE.md`, `<project>/CLAUDE.md`, `~/.codex/AGENTS.md`, and `<project>/AGENTS.md`.

For long jobs, an agent can validate a JSON probe with `ilium progress check`, register it with `ilium progress set`, then wait for Ilium's result. The detached server polls from the project root. Probes need a stable `job_id`, status, percentage, and bounded details; use absolute paths. See `ilium progress --help` for flags. Progress monitoring does not manage an agent's `/goal`.

The server also listens on `127.0.0.1:8872` for unauthenticated `POST /create_agent`. Any local process that can reach the port can submit a request. Change `[http_api].port` if sessions collide; do not expose the listener through a port forward or reverse proxy without protection. A bind failure disables the listener while the session continues.

### Voice

Voice control is off until configured. It needs an OpenAI Realtime key, network access, and an attached client; microphone use also needs audio devices. Use `F8` or Settings → Voice control. Check the selected target before sending text to a terminal or agent. Destructive semantic actions require confirmation; terminal submission includes Enter unless its optional confirmation is enabled.

`ilium voice say` sends typed text to the live voice conversation:

```sh
ilium voice say --start "what agents are running?"
printf '%s\n' "focus the first agent" "say hello to it" | ilium voice say -
```

`--start` saves the voice-on setting. A lone `-` reads nonempty input lines; `--` protects text starting with `-`. Each request accepts up to 32 sentences of 4,000 characters each. From outside Ilium, use `--cwd` and `--session-name`; an attached client is required. A JSONL `result` confirms the text was queued, not that the remote model acted. Errors include a code and hint.

### Command-line reference

| Command | What it does |
| --- | --- |
| `ilium` | Attach to or create the current project's `default` session. |
| `ilium new-session <name>` / `ilium ls` | Create or attach to a named session / list sessions. |
| `ilium new-pane -- <cmd>` | Add a terminal pane without attaching the TUI. |
| `ilium new-pane --session-name <name> -- <cmd>` | Add a terminal pane to a named session. |
| `ilium chat --help` / `ilium progress --help` | Show Chatroom and progress commands. |
| `ilium voice say --help` | Show typed voice input. |

Run commands from the intended project directory or pass `--cwd`. Angle brackets mark placeholders. `ilium --help` has the full CLI reference.

### Editors, boards, and Smart Copy

Mouse actions focus panes, move tree entries, open context menus, and scroll terminal history. The editor saves files, renders Markdown, and supports line numbers, a minimap, and autosave. Boards store cards in a Markdown file or folder you choose.

Smart Copy freezes the visible terminal screen and offers regions to copy. An inference model may suggest coordinates; it does not provide replacement clipboard text.

### How it works

One `ilium-server` per project session owns the PTYs, tree, scheduled input, and snapshot. The client renders state over local IPC: a Unix socket on Unix or a named pipe on Windows. Detaching the client leaves the server and pane processes running. Agent identity comes from child processes; activity comes from terminal screens, so the two observations can briefly disagree. See [ARCHITECTURE.md](ARCHITECTURE.md) for crate boundaries and design choices.

### Project, help, and licence

Ilium is early software. Check [CI](https://github.com/arthurwolf/ilium/actions) and report failures or installation problems in [issues](https://github.com/arthurwolf/ilium/issues). Include the version, OS, terminal, command, and observed behaviour; remove private content from logs. [AGENTS.md](AGENTS.md) covers contributions; [ARCHITECTURE.md](ARCHITECTURE.md) covers design.

Workspace checks: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace --no-fail-fast`.

For an established multiplexer, see [tmux](https://github.com/tmux/tmux) or [Zellij](https://github.com/zellij-org/zellij). [claude-squad](https://github.com/smtg-ai/claude-squad) combines tmux with worktrees; [prior art](ARCHITECTURE.md#prior-art--why-not-just-use-x) explains Ilium's choices.

Ilium is [MIT licensed](LICENSE). The vendored `vt100` patch and `tui-tree-widget` fork retain their MIT licences. Cascadia Code uses SIL Open Font License 1.1; see the [font notice](ilium-client/assets/fonts/NOTICE.md).
