<p align="center"><img src="assets/ilium-mark.svg" alt="Ilium mark" width="68" height="68"></p>
<h1 align="center">Ilium</h1>
<p align="center"><strong>Running 100+ CLI agents across dozens of projects and need some help auto-organizing that mess?</strong><br>Keep agent work, terminals, editors, and boards together, with activity in view.</p>
<p align="center"><sub>Linux · macOS · Windows · Rust · MIT</sub></p>
<p align="center"><a href="#quick-start">Quick start</a> · <a href="#daily-use">Daily use</a> · <a href="#see-it-in-action">Demos</a> · <a href="#what-ilium-does">Features</a> · <a href="src/docs/README.md">Full documentation</a></p>

> This README is the short version. The [full documentation](src/docs/README.md) has the step-by-step guides, every setting, key and command.

## Quick start

The commands and download links below require a published release. If release downloads are unavailable, [build from source](src/docs/building-from-source.md).

Linux and macOS:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://ilium-setup.pages.dev/install.sh | sh
```

Windows PowerShell:

```powershell
irm https://ilium-setup.pages.dev/install.ps1 | iex
```

For a Windows installer instead, choose the per-user [setup `.exe`](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-windows-x86_64-setup.exe) or [`.msi`](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-windows-x86_64.msi); the [ZIP](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-windows-x86_64.zip) is available for manual setup.

Direct release downloads (one package per system):

| System | Downloads |
| --- | --- |
| Linux x86_64 | [tar.gz](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-linux-x86_64.tar.gz) · [deb](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-linux-x86_64.deb) · [rpm](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-linux-x86_64.rpm) · [AppImage](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-linux-x86_64.AppImage) · [Snap](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-linux-x86_64.snap) · [Flatpak](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-linux-x86_64.flatpak) |
| Linux ARM64 | [tar.gz](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-linux-aarch64.tar.gz) · [deb](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-linux-aarch64.deb) · [rpm](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-linux-aarch64.rpm) · [AppImage](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-linux-aarch64.AppImage) · [Snap](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-linux-aarch64.snap) · [Flatpak](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-linux-aarch64.flatpak) |
| macOS Apple Silicon | [tar.gz](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-macos-aarch64.tar.gz) · [ZIP](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-macos-aarch64.zip) · [PKG](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-macos-aarch64.pkg) · [DMG](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-macos-aarch64.dmg) |
| macOS Intel | [tar.gz](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-macos-x86_64.tar.gz) · [ZIP](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-macos-x86_64.zip) · [PKG](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-macos-x86_64.pkg) · [DMG](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-macos-x86_64.dmg) |
| Windows x86_64 | [setup `.exe`](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-windows-x86_64-setup.exe) · [MSI](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-windows-x86_64.msi) · [ZIP](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-windows-x86_64.zip) |

Verify downloads against [`SHA256SUMS`](https://github.com/arthurwolf/ilium/releases/latest/download/SHA256SUMS). Details: [Installation](src/docs/installation.md).

After a qualified tag release completes, its [GitHub Packages release bundle](https://github.com/users/arthurwolf/packages/container/package/ilium-release) contains the same published assets in an OCI artifact. With [ORAS](https://oras.land/docs/installation/), run `oras pull ghcr.io/arthurwolf/ilium-release:VERSION --output ilium-release`, replacing `VERSION` with the release tag (for example, `v0.1.1`). The bundle includes the installers and five native archives; use the system installer above to install Ilium. The release's `SHA256SUMS` covers the native archives, while the published release asset digests cover every bundled file.

Open a new terminal in your project and run `ilium`. First-run setup walks through AI providers, notification sounds, keyboard practice and an optional voice test. Reopen it with `ilium --onboarding` or **Settings → Guided setup**. Re-run the installer to upgrade.

No Rust toolchain is needed for releases. Use a UTF-8 terminal with 256-colour support and install your agent CLI separately. Linux is the primary platform; macOS and Windows are implemented with different test coverage.

> **AI privacy:** First-run setup lets you choose Kilo Gateway, paid APIs, local Ollama or Skip. Automatic AI requests remain paused until setup finishes. Kilo Gateway sends prompts to its service. See [Inference and privacy](src/docs/inference-and-privacy.md).

More: [Getting started](src/docs/getting-started.md) · [Building from source](src/docs/building-from-source.md)

## See it in action

<table width="100%">
<tbody>
<tr>
<td width="50%" valign="top">
<p><strong>Find the next agent that needs you</strong></p>
<p><a href="assets/demos/27-attention-queue.gif?raw=true"><img src="assets/demos/27-attention-queue.gif" alt="Real Claude and Codex sessions show questions, blocked goals, background-agent waits and unread completed turns in one attention queue; setup and idle gaps are cut, with reading holds" width="100%"></a></p>
</td>
<td width="50%" valign="top">
<p><strong>AI names agents and organizes the project tree</strong></p>
<p><a href="assets/demos/01-ai-tree.gif?raw=true"><img src="assets/demos/01-ai-tree.gif" alt="Ilium names agent panes and organizes the project tree from local scripted fixture content; no provider request is made." width="100%"></a></p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p><strong>Control Ilium and prompt agents with voice</strong></p>
<p><a href="assets/demos/02-voice-control.gif?raw=true"><img src="assets/demos/02-voice-control.gif" alt="Typed voice input triggers Ilium actions and sends a prompt to a local fixture; no live microphone inference is shown." width="100%"></a></p>
</td>
<td width="50%" valign="top">
<p><strong>Give agent panes automatic titles</strong></p>
<p><a href="assets/demos/03-contextual-titles.gif?raw=true"><img src="assets/demos/03-contextual-titles.gif" alt="Current Ilium applies an automatic title from a supplied local response fixture; the manually named Keep pane stays unchanged." width="100%"></a></p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p><strong>Transfer a terminal screen between panes</strong></p>
<p><a href="assets/demos/05-screen-transfer.gif?raw=true"><img src="assets/demos/05-screen-transfer.gif" alt="Ilium transfers terminal output into a real Codex prompt without submitting it. Setup is cut, with reading holds and captions." width="100%"></a></p>
</td>
<td width="50%" valign="top">
<p><strong>Inspect agent usage and recorded costs</strong></p>
<p><a href="assets/demos/06-cost-and-stats.gif?raw=true"><img src="assets/demos/06-cost-and-stats.gif" alt="Costs and Stats for retained Claude and Codex sessions in current Ilium. Setup is cut, and a local file-link target and an account notice are redacted. Claude is logged out during the replay." width="100%"></a></p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p><strong>Track agent status and current goals</strong></p>
<p><a href="assets/demos/08-agent-activity.gif?raw=true"><img src="assets/demos/08-agent-activity.gif" alt="Real Claude and Codex agents show a goal, approval wait and unread completion. Pane names are staged; setup and part of a wait are cut, with a reading hold and a labelled status redaction." width="100%"></a></p>
</td>
<td width="50%" valign="top">
<p><strong>Use agents, editors, and terminals side by side</strong></p>
<p><a href="assets/demos/10-mixed-splits.gif?raw=true"><img src="assets/demos/10-mixed-splits.gif" alt="A terminal, editor, and two idle agent panes remain in a split after visiting another pane; setup is cut and an account notice is redacted." width="100%"></a></p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p><strong>Select and copy terminal output</strong></p>
<p><a href="assets/demos/11-smart-copy.gif?raw=true"><img src="assets/demos/11-smart-copy.gif" alt="Smart Copy uses local inference to select and paste a code from real Codex output. Setup and a model wait are cut, with reading holds and labelled redactions; automatic pane naming did not apply." width="100%"></a></p>
</td>
<td width="50%" valign="top">
<p><strong>Queue prompts for an agent</strong></p>
<p><a href="assets/demos/13-prompt-queue.gif?raw=true"><img src="assets/demos/13-prompt-queue.gif" alt="Queue two prompts for a local Codex-shaped fixture; Ilium sends them in order after each turn. No model; output is synthetic." width="100%"></a></p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p><strong>Get results from background tasks</strong></p>
<p><a href="assets/demos/14-progress-monitor.gif?raw=true"><img src="assets/demos/14-progress-monitor.gif" alt="A real Claude agent monitors a ten-card task and receives its completed result. Setup is cut and an account notice is redacted." width="100%"></a></p>
</td>
<td width="50%" valign="top">
<p><strong>Search agents, files, and terminals</strong></p>
<p><a href="assets/demos/17-workspace-search.gif?raw=true"><img src="assets/demos/17-workspace-search.gif" alt="Search real agent output, a shell history, and an open file, then jump to matches. Setup is cut and reading holds are added; automatic restructuring is disabled." width="100%"></a></p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p><strong>See live agent goals in the project tree</strong></p>
<p><a href="assets/demos/22-goal-indicators.gif?raw=true"><img src="assets/demos/22-goal-indicators.gif" alt="A real Codex goal changes from active to paused and back in the project tree. Setup is cut; captions, a closing hold and labelled account and path redactions are added." width="100%"></a></p>
</td>
<td width="50%" valign="top">
<p><strong>Resume agent sessions after a reboot</strong></p>
<p><a href="assets/demos/24-agents-survive-reboots.gif?raw=true"><img src="assets/demos/24-agents-survive-reboots.gif" alt="A staged reboot montage leads to retained footage of Ilium restoring the pane layout and resuming Claude and Codex." width="100%"></a></p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p><strong>Choose an animation, then watch it play</strong></p>
<p><a href="assets/demos/26-animations.gif?raw=true"><img src="assets/demos/26-animations.gif" alt="Historical animation gallery from an earlier Ilium build; current-build Voxel preview remains visually unqualified." width="100%"></a></p>
</td>
<td width="50%" valign="top">
<p><strong>Schedule input for a pane</strong></p>
<p><a href="assets/demos/04-scheduled-input.gif?raw=true"><img src="assets/demos/04-scheduled-input.gif" alt="Schedule input for a real Claude agent and a local terminal fixture in separate takes; setup is cut, reading holds are added, and local paths are redacted." width="100%"></a></p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p><strong>Set interface and board preferences</strong></p>
<p><a href="assets/demos/07a-settings-tour-a.gif?raw=true"><img src="assets/demos/07a-settings-tour-a.gif" alt="Ilium Settings: interface, terminal, editor, animation and board controls. Agent Cost, Optimization and Remote are omitted to protect local data; setup is cut and reading holds are added." width="100%"></a></p>
</td>
<td width="50%" valign="top">
<p><strong>Configure voice and AI providers</strong></p>
<p><a href="assets/demos/07b-settings-tour-b.gif?raw=true"><img src="assets/demos/07b-settings-tour-b.gif" alt="Ilium Settings: voice, reset, inference, and instruction pages. Provider credentials are unconfigured; setup is cut and reading holds are added." width="100%"></a></p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p><strong>Configure automatic titles</strong></p>
<p><a href="assets/demos/07c-settings-tour-c.gif?raw=true"><img src="assets/demos/07c-settings-tour-c.gif" alt="Ilium Settings: titles, triggers, custom agents, debugging, API, and setup. Setup paths belong to the isolated demo; other pages are cut and reading holds are added." width="100%"></a></p>
</td>
<td width="50%" valign="top">
<p><strong>Move panes without stopping their processes</strong></p>
<p><a href="assets/demos/09-pane-tree.gif?raw=true"><img src="assets/demos/09-pane-tree.gif" alt="Ilium moves a running fixture pane; setup is cut, captions and a hold are added, and automatic naming and restructuring are disabled." width="100%"></a></p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p><strong>Request updates from active agents</strong></p>
<p><a href="assets/demos/12-ask-for-update.gif?raw=true"><img src="assets/demos/12-ask-for-update.gif" alt="Ilium asks idle Claude and Codex agents for updates while skipping an agent awaiting approval; pane names were staged, setup and waits were cut, and a reading pause was added." width="100%"></a></p>
</td>
<td width="50%" valign="top">
<p><strong>Track work on a Kanban board</strong></p>
<p><a href="assets/demos/15-kanban.gif?raw=true"><img src="assets/demos/15-kanban.gif" alt="Move a staged Kanban card to Doing, mark it complete, add a note, then reopen the board and verify the saved Markdown file. Setup is cut; column width is 15 cells, with added captions, pointer, arrows and holds." width="100%"></a></p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p><strong>Start a Codex agent from a TODO</strong></p>
<p><a href="assets/demos/16-agent-from-line.gif?raw=true"><img src="assets/demos/16-agent-from-line.gif" alt="Start Codex from a TODO line and repair the source. Setup is cut and reading holds are added; automatic title inference reports no user transcript." width="100%"></a></p>
</td>
<td width="50%" valign="top">
<p><strong>Detach and reconnect while agents keep running</strong></p>
<p><a href="assets/demos/18-detach-reattach.gif?raw=true"><img src="assets/demos/18-detach-reattach.gif" alt="A real Claude agent continues writing files while Ilium is detached, then reappears on reconnect. Setup and waiting are cut, an account notice is redacted, and automatic naming and restructuring are disabled." width="100%"></a></p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p><strong>Edit Markdown in a pane</strong></p>
<p><a href="assets/demos/19-markdown-editor.gif?raw=true"><img src="assets/demos/19-markdown-editor.gif" alt="Edit and preview a staged Markdown note in Ilium, then read the saved file in a terminal. Setup is cut; captions, pointer, arrows and holds are added." width="100%"></a></p>
</td>
<td width="50%" valign="top">
<p><strong>Coordinate Claude and Codex with Chatroom</strong></p>
<p><a href="assets/demos/20-chatroom.gif?raw=true"><img src="assets/demos/20-chatroom.gif" alt="The current Ilium Chatroom displays locally scripted Claude and Codex messages; provider agents are not running." width="100%"></a></p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p><strong>Respond to confirmation prompts automatically</strong></p>
<p><a href="assets/demos/21-text-triggers.gif?raw=true"><img src="assets/demos/21-text-triggers.gif" alt="A real Claude agent receives an automatic text-trigger reply. Setup and an idle wait are cut, with reading holds added; automatic naming and restructuring are disabled in the isolated demo." width="100%"></a></p>
</td>
<td width="50%" valign="top">
<p><strong>Restore panes and resume agent sessions</strong></p>
<p><a href="assets/demos/23-snapshot-resume.gif?raw=true"><img src="assets/demos/23-snapshot-resume.gif" alt="Ilium restores panes and Claude and Codex continue from resumed context" width="100%"></a></p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p><strong>Create an isolated agent worktree</strong></p>
<p><a href="assets/demos/25-worktree-agent.gif?raw=true"><img src="assets/demos/25-worktree-agent.gif" alt="Ilium creates a Git worktree; a local Codex-shaped fixture shows the branch and diff. Typing is shortened and three editorial captions were added." width="100%"></a></p>
</td>
<td width="50%" valign="top">
<p><strong>Convert a session between Claude and Codex</strong></p>
<p><a href="assets/demos/26-convert-session.gif?raw=true"><img src="assets/demos/26-convert-session.gif" alt="Real Claude and Codex sessions recall a fictional mascot after conversion in both directions. Setup and idle wait are cut; an account notice is redacted." width="100%"></a></p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p><strong>Queue access to a shared resource</strong></p>
<p><a href="assets/demos/28-resource-queue.gif?raw=true"><img src="assets/demos/28-resource-queue.gif" alt="Two agents reserve a demonstration resource: one waits, takes ownership when it is released, then releases it. The recording is cropped and pauses are shortened." width="100%"></a></p>
</td>
<td width="50%" valign="top"></td>
</tr>
</tbody>
</table>

See the full [Demo gallery](src/docs/demos.md) for recording notes and feature guides.

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

Run `ilium` in the project directory to reattach. Press the prefix twice to send a literal prefix key to the focused pane. Remap it in Settings; `&` ends the session at once, without confirmation.

All keys, mouse use and remapping: [Getting started](src/docs/getting-started.md) and [Panes and layout](src/docs/panes-and-layout.md).

## What Ilium does

- **Arrange**: group panes and split the screen four ways. The project tree stays visible. [Panes and layout](src/docs/panes-and-layout.md)
- **Track**: identify agent CLIs from process trees and read activity from terminal screens, optionally showing each session's model icon in the tree. Stale activity checks show a caution with their age on hover. [Agent monitoring](src/docs/agent-monitoring.md)
- **Separate**: start agents in linked Git worktrees. Unclear file, process, or ownership state blocks cleanup. [Worktrees](src/docs/worktrees.md)
- **Resume**: detach without stopping panes. Recovery rebuilds the layout and relaunches pane programs. [Session recovery](src/docs/session-recovery.md)

Also included:

- An [attention queue](src/docs/attention-queue.md): one list of the agents that ask you a question, keep failing, wait on another agent, or changed, with a jump button and shortcut.
- Agent [cost tracking](src/docs/agent-cost.md) and [notifications](src/docs/notifications.md).
- Optional remote compaction of Claude and Codex sessions through your Inference model, configured in [Settings](src/docs/settings.md#remote-compaction).
- A compaction optimizer (**Settings → Optimization**) that scans your Codex and Claude Code transcripts and recommends the cheapest auto-compaction threshold, applied to the agent's own config only after you confirm, with one-click revert. See [Settings](src/docs/settings.md#optimization).
- AI [titles, tree organization and custom instructions](src/docs/titles-and-instructions.md), with [provider and privacy controls](src/docs/inference-and-privacy.md).
- [Voice control](src/docs/voice.md).
- [Scheduled input, text triggers, prompt queues, progress monitors and Chatroom](src/docs/automation.md).
- [Shared resource queues](src/docs/resource-queues.md) for exclusive GPU and USB-board coordination across projects.
- [Editors, boards](src/docs/editors-and-boards.md) and [Smart Copy](src/docs/smart-copy.md).
- Ambient [animated backgrounds](src/docs/animations.md): landscapes, space, maps, live data, video and more, all sharing one look.

Agent worktrees and recovery in short: start an agent with `Ctrl+B W` or

```sh
ilium new-pane --worktree --branch agent/fix-login -- codex
```

Several agents can share an existing worktree. With two or more agents, the
worktree becomes an expandable leaf-icon row under the project, with its
agents fixed beneath it. A single agent keeps the compact branch line.

After a restart, Ilium restores the layout and relaunches pane programs, resuming verified Claude Code, Codex, or Antigravity sessions when provider data remains. Unsaved process state does not survive. Snapshots live in `<project>/.ilium/sessions/`; add `.ilium/` to `.gitignore` to keep them out of Git.

## Command line

| Command | What it does |
| --- | --- |
| `ilium` | Attach to or create the current project's `default` session. |
| `ilium new-session <name>` / `ilium ls` | Create or attach to a named session / list sessions. |
| `ilium new-pane [--keep-open] -- <cmd>` | Add a terminal pane without attaching the TUI. The pane closes when the command exits unless `--keep-open` is given. |
| `ilium chat --help` / `ilium progress --help` | Chatroom and progress commands. |
| `ilium voice say --help` | Typed voice input. |
| `ilium ctl <command>` | Full remote control for agents and scripts: read the tree as JSON/YAML/Markdown, create, move, rename, close and drive panes. Start with `ilium ctl guide`. |
| `ilium panes` / `ilium broadcast <message>` | List panes, or message agents, across every running session; filter by project, agent, state, text or regex. |

Every command and flag: [Command-line reference](src/docs/cli-reference.md).

## Settings and data

Open Settings with `Ctrl+B :` and the key map with `Ctrl+B ?`. Global settings live in `~/.config/ilium/config.toml` on Linux. Terminals, agent detection and session storage work without an LLM; AI titles, tree organization and optional Smart Copy suggestions use the provider you choose. File logging is off by default. See [Settings](src/docs/settings.md) and [How it works](src/docs/how-it-works.md).

## Project, help, and licence

Report problems in [issues](https://github.com/arthurwolf/ilium/issues) with your version, OS, terminal, command and observed behaviour; remove private content from logs. Design: [ARCHITECTURE.md](ARCHITECTURE.md). Contribution rules: [CLAUDE.md](CLAUDE.md) and [Contributing](src/docs/contributing.md).

Ilium is [MIT licensed](LICENSE). The vendored `vt100` patch and `tui-tree-widget` fork retain their MIT licences. Cascadia Code uses SIL Open Font License 1.1; see the [font notice](ilium-client/assets/fonts/NOTICE.md).

Blocking failures appear as caution triangles beside affected agents and projects. Hover the triangle to read the cause; unavailable terminals also show their failure reason.
