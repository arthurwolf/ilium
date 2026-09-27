# Ilium

**A terminal multiplexer for running several AI coding agents in one workspace.**

Ilium keeps a project session running when you detach, arranges terminals, editors, and boards in a movable tree, and marks agent activity in the sidebar. You can see which pane is working, waiting for approval, or ready for you to read.

[Install](#install-from-source) · [First run](#first-run) · [Demos](#see-it-in-action) · [Daily use](#daily-use) · [Configuration](#configuration) · [Commands](#command-line-reference)

## Project status and platforms

Ilium is at version `0.1.0` and is still early. Linux is the primary platform for trying it. The repository also runs macOS and Windows CI jobs, but those ports have unresolved failures; see the [current CI runs](https://github.com/arthurwolf/ilium/actions) before relying on either platform. A configured CI job is not a claim of passing tests or a supported release. Bug reports and fixes are welcome through [issues](https://github.com/arthurwolf/ilium/issues).

Use a UTF-8 terminal with 256-colour support for the intended display. Ilium does not install Claude Code, Codex, Antigravity, or their accounts; install and sign in to the agent CLI you want to run separately.

## Install from source

Build from a clone so Cargo uses this repository's patched `vt100` dependency. You need Git, Make, Rust with Cargo via rustup, a C build toolchain, and the native libraries used by this workspace. The manifest declares Rust 1.89 as its minimum, while the checked-in `rust-toolchain.toml` selects **Rust 1.96.1** for a normal rustup build of this clone. Install that pinned toolchain when rustup requests it.

On Ubuntu, the native development packages used by CI are:

```sh
sudo apt-get update
sudo apt-get install -y libasound2-dev pkg-config libssl-dev
```

Package names vary on other Linux distributions. Build and install the two required binaries:

```sh
git clone https://github.com/arthurwolf/ilium.git
cd ilium
make install
```

`make install` builds `ilium` and `ilium-server` in release mode and installs both to `$CARGO_HOME/bin` (`~/.cargo/bin` when `CARGO_HOME` is unset). Put that directory on your `PATH`. To install into your local bin directory instead, run this from the clone:

```sh
make install BIN_DIR="$HOME/.local/bin"
```

Keep `ilium` and `ilium-server` together: the CLI starts a detached server for each session and looks beside its own executable before searching `PATH`. A plain `cargo install ilium` is not the documented installation route: the workspace's `[patch.crates-io]` applies an unreleased `vt100` fix, and downstream Cargo installs would not use this clone's patch. The reason for the patch is described in [ARCHITECTURE.md](ARCHITECTURE.md#a-note-on-the-vendored-vt100).

## First run

**Network use on a fresh install:** Automatic AI titles and tree organization are enabled by default. They use Kilo Gateway over the network unless you change the provider or disable those triggers in Settings. No account or key is required for the current free default, so lack of credentials does not mean Ilium stays offline. Read [Inference and privacy](#inference-and-privacy) before using sensitive project content.

From an existing project directory, start Ilium:

```sh
ilium
```

The TUI attaches to that directory's `default` session, creating it if needed. The left side holds the pane tree; the right side shows the selected pane. Try `Ctrl+B c` to open a terminal pane, run a CLI you already have (for example `claude` or `codex`) inside it, and watch its status appear in the tree. Ilium recognizes a supported agent by its child process and visible terminal state; it does not require an Ilium-specific launch command. Use `Ctrl+B ?` for the current keyboard reference and `Ctrl+B d` to detach. Run `ilium` again from the same directory to reattach while the server and its panes are still running.

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

## Daily use

### Arrange panes and keep the view you need

The sidebar is a tree of project groups, folders, panes, and split views. A pane can hold a terminal, the built-in editor, or a Kanban board. Select a pane to show it on the right. A split view keeps up to four panes visible together while one pane receives keyboard input. Create a split with `Ctrl+B "`, then choose its orientation and panes in the dialog.

You can move entries with the tree's hover arrows, drag a row to a new parent or position, or press `Ctrl+B m` for keyboard move mode. In move mode, up and down reorder, while left and right outdent and indent. Tree order can also be set to Manual, Type, Age, or Name in Settings; a manual structural move returns it to Manual. Right-click a tree row for actions including create, rename, and close. Clicking a pane or the tree changes focus without changing the underlying PTY process.

The terminal sends supported xterm mouse events through to an application that has enabled them. For Ilium's own history view, use the wheel or `Shift+PgUp`/`Shift+PgDn`; `Shift+End` returns to live output. `Ctrl+B [` and `Ctrl+B ]` scroll a focused terminal by a page. The built-in editor can open and save files, switch Markdown between source and rendered views, and toggle line numbers, minimap, or autosave. The board stores cards in a selected Markdown file or folder of Markdown files; choose a storage location when creating it.

### Read the sidebar signals

In the default **Normal** monitoring mode, a row can show three icon slots before its title. **Attention** mode reduces the status display to the highest-priority item that needs action. The compiled defaults are configurable in Settings → Icons, so an icon's meaning can change in your installation. Hover an icon to read its meaning and the reason Ilium chose that state.

| Slot | What it answers | Examples with default icons |
| --- | --- | --- |
| Identity | What is in this pane? | Claude Code 🦀, Codex 🐢, Antigravity ⚛️, another detected agent 🤖, or a plain terminal 🖥️. Editors and boards have their own icons. |
| Objective or scheduled work | What longer task is attached? | Agent `/goal` active 🎯, paused ⏸️, blocked 🚧, usage-limited ⌛, or reached 🏁; a progress bar/result for a monitored task; or ⏰ for scheduled input. |
| Current activity | What needs attention now? | Working ⠋, waiting for approval ✋, waiting for background agents 🕗, settling background work 🌀, parked on a monitor 💤, finished and unread 🔔, or idle ●. A plain terminal can show recent output activity. |

The finished bell remains until you open the pane. An agent that ends its turn while a registered long task is still running is parked on that monitor; Ilium waits for the task result instead of announcing the turn as finished. The monitor can later report done, task error, or loss of monitoring. A `/goal` badge reflects a provider state that Ilium observes; Ilium does not pause or resume the agent's goal.

Ilium detects the CLI process first, then inspects visible screen text for activity. Status can therefore trail a transition until the next detection pass, and a changed agent UI can affect classification. Custom signatures and detection intervals are configurable if you use a different agent CLI. Sounds and desktop notifications for useful status transitions have their own settings.

### Work with agents

Open a terminal and run an installed agent CLI, or use the tree menus to start one of the built-in providers. The sidebar shows the detected provider, the agent's changing state, and, when Ilium can verify it, a session ID in the pane title. Rename a pane yourself to keep a durable label: automatic title changes preserve manual names. Settings → Titles chooses whether future AI-authored names are compact **Labels** for finding work again or **Summaries** of the work. Changing the style does not rewrite already named panes.

The agent toolbar has several ways to deal with long output and concurrent work:

- **Smart Copy** freezes the visible terminal grid and immediately offers local semantic regions such as URLs, commands, paragraphs, code, or tables. If an inference provider is reachable, it can stream additional ranked regions. Preview a region, cycle overlapping choices, and click one to copy it. Every selection resolves against the frozen screen rather than accepting replacement text from a model. Exit the overlay or press `Esc` to return to live output.
- **Costs & stats** reads the detected Claude Code or Codex session's own transcript in the background. The header's second icon opens a popover with Overview, Tokens, Activity, and Prompts. Hover previews it; click to pin or dismiss it. Token and activity views may update as the transcript grows. Dollar cost appears only if the agent recorded it; Ilium does not price usage itself, and an agent's cost snapshot may lag current work.
- **Search** finds agent, shell, and editor content and jumps to a result. **Ask for update** sends a prompt to active agents; **Prompt queue** delivers queued prompts as their turns become ready. These actions submit input to live agents, so review what will be sent before using them.
- **Scheduled input** can send text or a keypress to a terminal pane after a chosen delay. The detached server owns the deadline, so detaching a client does not cancel it. The row shows a countdown while input is pending.
- **Text triggers** can answer matching terminal prompts. Treat a trigger as automation that submits input, and review its match and response before enabling it for confirmation or approval prompts.
- **Chatroom** is a file-backed room for agents working in the same project. `ilium chat` can read or post there even when no TUI is attached. Its project file is shared workspace data, so use it for actionable coordination rather than private messages.

### Run an agent in a Git worktree

A linked worktree gives an agent its own checkout and branch without switching the branch of a checkout another pane or user shell may be using. Use the agent button in the tree footer, a project's context menu, or `Ctrl+B W`. The creation form previews provider, branch, base, and checkout path before changing Git. You can also open an existing linked worktree; doing that does not make it Ilium-owned. If you started Ilium from a subdirectory of a repository, an Ilium-created worktree preserves that relative launch directory for its agent.

For a script or shell, create a new worktree and add an agent pane without opening the TUI. Replace the example branch name with a new branch that does not already exist:

```sh
ilium new-pane --worktree --branch agent/fix-login -- codex
```

Add `--base main` only when `main` is the starting ref you want; omitting `--base` uses the repository's configured default base. The command reports JSONL progress and a final pane/worktree result. Worktree creation runs normal Git checkout hooks and filters. Repositories using submodules or Git LFS may need a post-create setup command in Settings → Git. A nonblank setup command is available on Linux; Ilium rejects creation on other platforms before changing Git when that command is configured.

Ilium records which worktrees it created. If creation fails after Git has written a checkout, it retains the path and reports it for inspection instead of deleting files that hooks, filters, or setup may have created. Use the pane's **Worktree** menu or the project's **Manage worktrees…** menu to inspect registration, ownership, and removal blockers. A foreign worktree stays listed but cannot be removed by Ilium. Removing a verified Ilium checkout keeps its branch by default; discarding checkout files requires typing the full path, and deleting the branch is a separate choice subject to Git's safe-delete check.

Settings → Git can offer removal when an agent pane closes. Ilium offers it only after it verifies a clean worktree, a merged branch, and no other pane or unresolved custody ticket for the directory. Choosing No retains the checkout and branch. If file, process, registration, or custody checks cannot prove safety, Ilium leaves the checkout for manual inspection. Legacy ownership markers without process-custody evidence remain protected: Ilium refuses automatic removal and new terminals there. Inspect the exact Git registration and files before resolving one outside Ilium.

### Detach, restore, and resume

`Ctrl+B d` detaches the interactive client. The detached server still owns its PTYs, so existing pane processes keep running while that server remains alive. Reopening `ilium` from the same canonical project directory attaches to that session. `ilium new-session <name>` creates or attaches another named session for the same project, and `ilium ls` lists the project's known sessions.

Ilium also writes a session snapshot under `<project>/.ilium/sessions/<name>.json`. Add `.ilium/` to a project's `.gitignore` if those local snapshots should not be committed. Automatic session recovery and rolling session backups are enabled by default in the current configuration. Backups of existing snapshots are stored under `<project>/.ilium/backups/<name>/`; they do not back up the contents of files edited inside pane applications. When a server starts from a snapshot, Ilium restores its tree and launches pane programs again. For a detected Claude Code, Codex, or Antigravity session with a verified resume identity, it can start the provider's resume command so the new process continues that agent conversation. That depends on the provider's own retained session data and a verified match to this project. A server restart or machine reboot ends the original processes; restored panes are new processes. Unsaved in-process state in a terminal application is not a snapshot feature.

`--restart-server` replaces a running server while retaining its snapshot and is useful after installing a new build. `--reset-session` deletes this project's named session snapshot and starts empty; use it only when you intend to discard that saved layout. `ilium kill-session <name>` ends the named session and its panes. These commands affect the targeted project session, so check `--cwd` and the name before using them.

### Watch long-running tasks without repeated polling

An agent inside an Ilium pane can register one long-task progress monitor. It first starts and verifies the job, then gives Ilium a cheap probe that prints one JSON object: a stable `job_id`, a status (`not-started-yet`, `running`, `done`, or `error`), a percentage from 0 to 100, and bounded message or error text. The agent validates that probe with `ilium progress check` before registering it with `ilium progress set`. The latter returns JSONL acknowledgement with a monitor ID and accepted first report.

After registration, the detached server runs the recurring probe. The agent can stop checking and receive the terminal result as a message when its composer is ready. The probe runs from the Ilium server's project root, not from the pane shell's current directory, so use absolute paths or an explicit absolute `cd` in a registered command. Ilium distinguishes a job reporting `error` from a failing monitor and retains the result until it has been seen. This monitor reports task progress; it does not control an agent's `/goal` lifecycle. The [progress subcommand help](#command-line-reference) is the place to check exact flags before writing an automation script.

## Configuration

On Linux, Ilium stores global settings in `~/.config/ilium/config.toml`. Most settings are available in the live Settings screen (`Ctrl+B :` or the tree footer control), which saves changes for you. The two prefix controls are in `[keyboard]`; action-specific remaps are a separate `[keybindings]` table. Press `Ctrl+B ?` to see the effective bindings after remapping.

| Config table | What it controls |
| --- | --- |
| `[detection]` | Fast and slow agent polling intervals and `[[detection.custom_signatures]]` for other CLIs. |
| `[keyboard]` | `shortcut_base` for commands and `navigation_shortcut_base` for tree traversal. |
| `[keybindings]` | Per-action overrides using stable action names such as `new_terminal`. |
| `[ui]` | Sidebar sizing and order, icons, motion, and theme colours. |
| `[sound]` and `[notifications]` | Status-transition sounds and desktop notifications. |
| `[kanban_board]` | Board card preview height and column sizing. |
| `[inference]` | Provider, endpoint/model, and credentials for AI-assisted features. |
| `[git]` | Worktree placement, branch defaults, setup command, display, and close policy. |
| `[reset_planning]` | Claude/Codex public-reset announcements and status-bar wording. |
| `[http_api]` | Port of the local automation listener. |
| `[debug]` | Optional process file logging. |

Settings → Reset planning enables separate monitors for public Claude and Codex reset announcements by default while an interactive client is open. A scheduled event with a known time can appear as an exact countdown or rounded wording; an announcement without one appears as time TBD. These public announcements are separate from the rolling reset time of your own account, and a feed that publishes only past events cannot supply a future countdown.

### Inference and privacy

The terminal multiplexer, pane tree, agent detection, splits, and session storage work without an LLM provider. AI-authored titles, tree organization, and additional Smart Copy suggestions use the selected provider. Automatic retitling and restructuring are **enabled on a fresh install**, so these features can make network requests without a separate opt-in click.

At this revision, the source selects **Kilo Gateway** and `stepfun/step-3.7-flash:free` by default. [Kilo says](https://kilo.ai/docs/gateway/authentication) its anonymous free-model access needs no key and is rate-limited by public IP. The [model catalog](https://api.kilo.ai/api/gateway/models) identified this selected model as free and marked `mayTrainOnYourPrompts: true` when checked on 2026-09-27. Model availability, routing, limits, and free-provider data handling can change; see [Kilo's free-model guidance](https://kilo.ai/docs/getting-started/using-kilo-for-free) and the current catalog before using it for sensitive work. A local Ollama provider is available in Settings → Inference if you prefer local model execution. OpenAI-compatible, Anthropic, and OpenRouter providers are also available with their own endpoints or credentials.

Review Settings → Triggers to control automatic title and tree actions. When a provider is unavailable, terminal multiplexing continues; AI naming can retain fallback labels and automatic restructuring reports and backs off from failures. Settings → Titles chooses how future AI names are phrased. A title you renamed manually stays yours. Smart Copy starts with local candidates from the frozen screen and can add model suggestions when inference is available.

Kilo paid-proxy egress is an advanced, hand-edited option in `[inference.kilo_gateway]`. It reads enabled proxy records from a configured MongoDB collection at client start; those records are not stored in `config.toml`. It is not required for the default anonymous free route.

### Agent setup and progress instructions

Ilium offers Chatroom and progress instructions during startup and asks which **optional** sets to install. You can also choose them later in Settings → Setup. Installing them changes agent instruction files; Chatroom setup also creates or repairs `CHATROOM.md` and agent hooks. The screen shows where and what Ilium manages. The default targets are `~/.claude/CLAUDE.md` and `<project>/CLAUDE.md` for Claude Code, and `~/.codex/AGENTS.md` and `<project>/AGENTS.md` for Codex. A custom Claude global file can be selected. After you opt in, Ilium refreshes its marker-delimited blocks while preserving text outside them; a file shared by both CLIs can hold a single managed block.

The progress instructions tell an informed agent to register a server-owned monitor for work expected to last at least three minutes: start the task, validate a cheap absolute-path probe with `ilium progress check`, register with `ilium progress set`, and wait for the server's acknowledgement. After that, the agent stops polling and receives a result when the task ends. These instructions do not operate the agent's `/goal` command.

### Voice control

Voice is a separate, opt-in controller. It needs an OpenAI Realtime API key, network access, an interactive Ilium client, and working audio devices for microphone use. It is off until configured. Press `F8` or open Settings → Voice control to start it. The model can navigate Ilium, manipulate the tree, open settings, and dictate to the selected agent or terminal through Ilium's semantic tools. Review the target and action before using it for commands: an explicit terminal submission normally includes Enter, while the optional terminal-submission confirmation setting can require a yes/no step. Destructive semantic actions have separate confirmation.

`ilium voice say` sends typed sentences into the same running voice conversation. Text and speech can be mixed; each sentence is one turn and later sentences wait while the model is still responding. Examples:

```sh
ilium voice say "open the settings"
ilium voice say "create a Codex agent" "then tell it to fix the failing test"
printf '%s\n' "focus the first agent" "say hello to it" | ilium voice say -
ilium voice say --start "what agents are running?"
```

A lone `-` reads nonempty lines from standard input. Put `--` before a sentence that begins with `-`. A request accepts at most 32 sentences of at most 4,000 characters each. `--start` turns on voice control and saves that setting, or restarts a voice session that failed to start. `--timeout-s` bounds how long the CLI waits for an attached client to accept the text; the default is 30 seconds.

The voice session lives in an attached interactive client, not the detached server. From inside an Ilium pane, the command targets that pane's session. Outside a pane, use `--cwd` and `--session-name` to address the intended project session; the default name is `default`. The command never starts a server or opens a TUI. If several clients are attached, the broker offers a sentence to one client, preferring a voice session that is already running.

The command writes JSONL records with a `type` field and exits nonzero after an `error`. A successful `result` means the sentences were **queued to the live voice session**. It does not prove that the remote model understood, answered, or executed them: the provider gives no per-turn acknowledgement. Common error codes include `session-not-running`, `no-voice-client`, `voice-off`, `voice-unavailable`, `client-unresponsive`, and `timeout`. The error message and hint explain the next step. An older detached server can also lack this command's IPC support; restart Ilium after upgrading if a request times out or its connection closes unexpectedly.

For an isolated demo or adapter test, `ILIUM_VOICE_REALTIME_URL=ws://127.0.0.1:<port>/v1/realtime` points at a scripted loopback endpoint, and `ILIUM_VOICE_AUDIO=none` skips audio devices. The alternate URL accepts loopback `ws://` only. These are testing seams, not a way to send microphone audio to an arbitrary remote host.

### Local HTTP listener and logging

A project server can bind a loopback HTTP listener at `127.0.0.1` on the configured `[http_api].port` (default `8872`) and serve `POST /create_agent` to create an agent with a supplied prompt. **The endpoint has no authentication.** Loopback limits ordinary remote access, but any local process able to connect to that port can submit a request. Do not treat it as a security boundary or expose it through a port forward or reverse proxy without adding your own protection. Give concurrently running project sessions different ports if they need the listener; a server that cannot bind its configured port logs the failure and continues without that API.

Debug file logging is off by default. Turning on `[debug].file_logging_enabled` writes HTTP and LLM text requests and responses, provider errors, and application context to a private local log. Credential headers and URL parameters are redacted, but bodies can contain project content or prompts. Enable it only when that output is appropriate to retain, and turn it off after diagnosing the issue.

## Keyboard and mouse reference

Two remappable prefixes default to `Ctrl+B`: `[keyboard].shortcut_base` for commands and `[keyboard].navigation_shortcut_base` for tree movement. If running inside tmux with its default prefix, press `Ctrl+B` twice to send the prefix through, or choose a different Ilium prefix in Settings → Keyboard. `Ctrl+B ?` shows the effective live bindings.

| After `Ctrl+B` | Action |
| --- | --- |
| `↓` / `↑` | Next / previous pane in the current group. |
| `PgDn` / `PgUp` | First pane in the next / previous group. |
| `c` | New terminal pane in the selected group. |
| `W` | New agent in a Git worktree; choose provider, branch, and location. |
| `e` | New editor pane and file picker. |
| `B` | New board; choose storage format and location. |
| `g` | New group. |
| `"` | New vertical or horizontal split view. |
| `F` | Open a folder in the sidebar. |
| `!` | Prompt for a command and run it in a new terminal pane. |
| `x` | Close the selected pane or group. |
| `,` | Rename the selected tree node. |
| `m` | Enter or leave move mode; arrows reorder or indent/outdent. |
| `t` / `P` | Focus the tree / active pane. |
| `o` / `;` | Focus the next / previous visible pane. |
| `h` `j` `k` `l` | Focus a visible pane left / down / up / right. |
| `[` / `]` | Scroll a focused terminal one page up / down. |
| `f` | Search terminal history and open editor buffers. |
| `s` | Save the focused editor pane. |
| `v` | Switch a Markdown editor between Source and Rendered. |
| `n` / `b` / `a` | Toggle editor line numbers / minimap / autosave. |
| `:` | Open Settings. |
| `?` | Show or hide Help. |
| `d` | Detach this client and keep the session running. |
| `&` | Kill this project session and disconnect every client. |

Click a panel to focus it. The tree supports expand/collapse, row hover arrows, drag-and-drop reparenting, double-click rename, and context menus. Terminal history also scrolls with the wheel or `Shift+PgUp`/`Shift+PgDn`; `Shift+End` returns to live output. Full-screen applications can receive xterm mouse events and `Ctrl+End` when they handle those inputs themselves.

## Command-line reference

Commands are scoped to the canonical project directory. Run them from the intended project or pass `--cwd` to select one. The TUI Help screen covers interactive keys; `ilium --help` and subcommand `--help` show the current CLI flags.

| Command | Action |
| --- | --- |
| `ilium` | Attach to, or create, this project's `default` session. |
| `ilium new-session <name>` | Create or attach to a named session. |
| `ilium ls` | List this project's known sessions and running state. |
| `ilium kill-session <name>` | End the named session and all its panes. |
| `ilium new-pane -- <cmd>` | Add a pane running `<cmd>` to `default` without attaching a TUI. |
| `ilium new-pane --session-name <name> -- <cmd>` | Add a pane to a named session without attaching. |
| `ilium new-pane --worktree --branch <new-name> [--base <ref>] -- codex` | Create a new branch/worktree and start a supported agent there. |
| `ilium chat --help` | Show project Chatroom operations. |
| `ilium progress --help` | Show probe validation, registration, and clearing commands. Run them inside a pane. |
| `ilium voice say --help` | Show typed voice-input flags and limits. Requires an attached client. |

The angle-bracketed forms in this reference table indicate arguments to replace; the examples in [First run](#first-run), [Worktrees](#run-an-agent-in-a-git-worktree), and [Voice control](#voice-control) are executable commands. Global `--restart-server` replaces a running server while retaining its snapshot. Global `--reset-session` deletes the targeted snapshot and starts an empty session. Check the project and session before either lifecycle operation.

## How it works

Each project session has one detached `ilium-server` that owns its PTYs, tree, scheduled input, and session snapshot. A terminal-based client attaches to it and renders the tree and pane screens over a local transport. This separation lets you detach without ending pane processes. On a later server start, a saved snapshot lets Ilium reconstruct the layout and launch pane programs again, subject to each program's own resume support.

For agent identity, Ilium walks a pane's child process tree and matches known CLI signatures. It reads the rendered screen to classify current activity. Those are separate observations: a process name can identify Codex, for example, but cannot alone tell whether Codex is waiting for approval. Detection is periodically refreshed and may lag a rapid transition. [ARCHITECTURE.md](ARCHITECTURE.md) explains the crate boundaries, persistence, detection, and design decisions in detail.

## Alternatives and help

Ilium is designed for a movable project tree and agent-state cues. If you need a more established general multiplexer, see [Zellij](https://github.com/zellij-org/zellij) or [tmux](https://github.com/tmux/tmux); for tmux plus agent worktrees, see [claude-squad](https://github.com/smtg-ai/claude-squad). The [prior-art discussion](ARCHITECTURE.md#prior-art--why-not-just-use-x) explains the trade-offs that shaped Ilium. For a bug, unclear installation step, or platform-specific failure, open a [GitHub issue](https://github.com/arthurwolf/ilium/issues) with the Ilium version, OS, terminal, relevant command, and what you observed. Remove secrets and private project content from logs before attaching them.

## Contributing

Issues and pull requests are welcome. Read the repository's [agent and contributor instructions](AGENTS.md) and [architecture](ARCHITECTURE.md) before a larger change. To run the workspace checks from a clone with the documented toolchain and native dependencies:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --no-fail-fast
```

The CI workflow runs these checks on Linux and also builds and tests macOS and Windows on its configured events. A locally passing check does not establish another platform's status; see [current CI runs](https://github.com/arthurwolf/ilium/actions) for that evidence.

## License

Ilium is MIT licensed; see [LICENSE](LICENSE). The in-tree `vendor/vt100` patch (MIT, © Jesse Luehrs) and `ilium-client/vendor/tui-tree-widget` fork (MIT, © EdJoPaTo) keep their own licenses. The bundled Cascadia Code font uses SIL Open Font License 1.1 (© Microsoft Corporation); see [`ilium-client/assets/fonts/NOTICE.md`](ilium-client/assets/fonts/NOTICE.md).
