<p align="center"><img src="assets/ilium-mark.svg" alt="Ilium mark" width="68" height="68"></p>
<h1 align="center">Ilium</h1>
<p align="center"><strong>Running 100+ CLI agents across dozens of projects and need some help auto-organizing that mess?</strong><br>Keep agent work, terminals, editors, and boards together, with activity in view.</p>
<p align="center"><sub>Linux · macOS · Windows · Rust · MIT</sub></p>
<p align="center"><a href="#quick-start">Quick start</a> · <a href="#daily-use">Daily use</a> · <a href="#see-it-in-action">Demos</a> · <a href="#what-ilium-does">Features</a> · <a href="src/docs/README.md">Full documentation</a></p>

> This README is the short version. The [full documentation](src/docs/README.md) has the step-by-step guides, every setting, key and command.

## Quick start

Linux preview: [v0.1.1-linux.3](https://github.com/arthurwolf/ilium/releases/tag/v0.1.1-linux.3) provides Linux x86_64 (six formats) and Linux ARM64 (four formats) from one source commit, with one `SHA256SUMS` covering all ten files. The public downloads have been checked against their checksums. This prerelease is not marked as `latest`; the full cross-platform v0.1.1 release remains pending.

**Not yet available for this preview:** two of six Linux ARM64 formats (rpm, Snap), macOS and Windows. Plugins are an unsupported development preview.

Plugin code is present for future integration and is not yet available for use.

Install the Linux x86_64 preview (no root required):

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://ilium-setup.pages.dev/install.sh | sh
```

The Linux installer supports x86_64 and ARM64 (aarch64); ARM64 has been verified in an emulated guest only.

Windows PowerShell **(coming soon: the Windows package is not yet available)**:

```powershell
irm https://ilium-setup.pages.dev/install.ps1 | iex
```

Windows downloads are **not yet available**: per-user setup `.exe`, `.msi`, and ZIP.

Direct downloads (Linux x86_64 preview; other systems not yet available):

| System | Downloads / availability |
| --- | --- |
| Linux x86_64 (6 of 6 available and verified) | [tar.gz](https://github.com/arthurwolf/ilium/releases/download/v0.1.1-linux.3/ilium-linux-x86_64.tar.gz) · [deb](https://github.com/arthurwolf/ilium/releases/download/v0.1.1-linux.3/ilium-linux-x86_64.deb) · [rpm](https://github.com/arthurwolf/ilium/releases/download/v0.1.1-linux.3/ilium-linux-x86_64.rpm) · [AppImage](https://github.com/arthurwolf/ilium/releases/download/v0.1.1-linux.3/ilium-linux-x86_64.AppImage) · [Snap](https://github.com/arthurwolf/ilium/releases/download/v0.1.1-linux.3/ilium-linux-x86_64.snap) · [Flatpak](https://github.com/arthurwolf/ilium/releases/download/v0.1.1-linux.3/ilium-linux-x86_64.flatpak) |
| Linux ARM64 (4 of 6 available and verified; emulated QEMU, not native hardware) | [tar.gz](https://github.com/arthurwolf/ilium/releases/download/v0.1.1-linux.3/ilium-linux-aarch64.tar.gz) · [deb](https://github.com/arthurwolf/ilium/releases/download/v0.1.1-linux.3/ilium-linux-aarch64.deb) · [AppImage](https://github.com/arthurwolf/ilium/releases/download/v0.1.1-linux.3/ilium-linux-aarch64.AppImage) · rpm **(coming soon: only a limited `--nodeps` install was tested)** · Snap **(coming soon: install not verified)** · [Flatpak](https://github.com/arthurwolf/ilium/releases/download/v0.1.1-linux.3/ilium-linux-aarch64.flatpak) |
| macOS Apple Silicon | tar.gz, ZIP, PKG, DMG **(coming soon; not yet available)** |
| macOS Intel | tar.gz, ZIP, PKG, DMG **(coming soon; not yet available)** |
| Windows x86_64 | setup `.exe`, MSI, ZIP **(coming soon; not yet available)** |

**Linux ARM64: 4 of 6 ARM64 formats available and verified.** The tar.gz, deb, AppImage and Flatpak ran in an emulated ARM64 (QEMU) guest; no native ARM64 hardware has been tested. Both architectures are in the same prerelease, [v0.1.1-linux.3](https://github.com/arthurwolf/ilium/releases/tag/v0.1.1-linux.3).

Verify every package using [`SHA256SUMS`](https://github.com/arthurwolf/ilium/releases/download/v0.1.1-linux.3/SHA256SUMS) of `v0.1.1-linux.3`, which lists both architectures. Details: [Installation](src/docs/installation.md).

The GitHub Packages release bundle is **not yet available**. After a qualified release, it is planned to contain the published assets as an OCI artifact retrievable with [ORAS](https://oras.land/docs/installation/).

Open a new terminal in your project and run `ilium`. First-run setup walks through AI providers, notification sounds, keyboard practice and an optional voice test. Reopen it with `ilium --onboarding` or **Settings → Guided setup**. For this preview, upgrade by installing a newer qualified package.

No Rust toolchain is needed for releases. Use a UTF-8 terminal with 256-colour support and install your agent CLI separately. Linux is the primary platform; macOS and Windows are implemented with different test coverage.

> **AI privacy:** First-run setup lets you choose Kilo Gateway, paid APIs, local Ollama or Skip. Automatic AI requests remain paused until setup finishes. Kilo Gateway sends prompts to its service. See [Inference and privacy](src/docs/inference-and-privacy.md).

More: [Getting started](src/docs/getting-started.md) · [Building from source](src/docs/building-from-source.md)

## See it in action

<table width="100%">
<tbody>
<tr>
<td width="50%" valign="top">
<p><strong>Find the next agent that needs you</strong></p>
<p><a href="assets/demos/27-attention-queue.gif?raw=true"><img src="assets/demos/27-attention-queue.gif" alt="Three real Claude sessions show two waiting questions and one unread completed turn in the attention queue, jumped to with Ctrl+B i and listed with Ctrl+B O; setup is cut, with reading holds, a redacted status bar and two frames with a blanked header line" width="100%"></a></p>
</td>
<td width="50%" valign="top">
<p><strong>AI names agents and organizes the project tree</strong></p>
<p><a href="assets/demos/01-ai-tree.gif?raw=true"><img src="assets/demos/01-ai-tree.gif" alt="Three real Claude Code agents (Haiku 5.5) work in an isolated project. When one finishes, Ilium's default trigger regroups the panes by topic using OpenAI gpt-4.1, twice; the three pane titles are then shown matching their content. Staged project and prompts; one wait is cut and two short holds are added; the status bar shows an empty-skipped restructuring message during the first 30 seconds, before the first agent finishes." width="100%"></a></p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p><strong>Control Ilium and prompt agents with voice</strong></p>
<p><a href="assets/demos/02-voice-control.gif?raw=true"><img src="assets/demos/02-voice-control.gif" alt="A real OpenAI Realtime voice session (gpt-realtime-2.1) in an isolated Ilium. The speech is synthesized (OpenAI text-to-speech) and played into a virtual microphone, not spoken live. The model hears six commands and calls Ilium tools: open a new terminal, open Settings, show the Triggers tab, close Settings, close that terminal, stop voice mode. Long waits between commands are cut; setup is skipped." width="100%"></a></p>
</td>
<td width="50%" valign="top">
<p><strong>Give agent panes automatic titles</strong></p>
<p><a href="assets/demos/03-contextual-titles.gif?raw=true"><img src="assets/demos/03-contextual-titles.gif" alt="Ilium titles one shell pane and two real Claude Code panes with a local Ollama model (qwen3:8b). The shell titles itself from its output, a name typed by hand stays, and a Claude pane is titled from its first prompt. Codex is not shown (usage limit); the second Claude stands in. The boot and a 45 s detection wait are cut (captioned), with reading holds." width="100%"></a></p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p><strong>Transfer a terminal screen between panes</strong></p>
<p><a href="assets/demos/05-screen-transfer.gif?raw=true"><img src="assets/demos/05-screen-transfer.gif" alt="Ilium transfers terminal output into a real Codex prompt without submitting it. Setup is cut, with reading holds and captions. Local scratch paths were masked frame by frame, so the affected lines are blank." width="100%"></a></p>
</td>
<td width="50%" valign="top">
<p><strong>Inspect agent usage and recorded costs</strong></p>
<p><a href="assets/demos/06-cost-and-stats.gif?raw=true"><img src="assets/demos/06-cost-and-stats.gif" alt="Costs and Stats for retained Claude and Codex sessions in current Ilium. Setup is cut, and a local file-link target and an account notice are redacted. Claude is logged out during the replay. An account login notice was masked frame by frame (a floating ACCOUNT NOTICE REDACTED box also appears briefly), so the affected lines are blank." width="100%"></a></p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p><strong>Track agent status and current goals</strong></p>
<p><a href="assets/demos/08-agent-activity.gif?raw=true"><img src="assets/demos/08-agent-activity.gif" alt="Real Claude and Codex agents show a goal, approval wait and unread completion. Pane names are staged; setup and part of a wait are cut, with a reading hold and a labelled status redaction. Local scratch paths and a notice were masked frame by frame, so the affected lines are blank; the status area shows a labelled redaction." width="100%"></a></p>
</td>
<td width="50%" valign="top">
<p><strong>Use agents, editors, and terminals side by side</strong></p>
<p><a href="assets/demos/10-mixed-splits.gif?raw=true">Historical recording, not embedded (one frame shows an unredacted login-expiry account notice and some frames show scratch-path fragments; keyframe review 2026-11-05). A terminal, editor, and two idle agent panes remain in a split after visiting another pane; setup is cut and an account notice is redacted.</a></p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p><strong>Select and copy terminal output</strong></p>
<p><a href="assets/demos/11-smart-copy.gif?raw=true"><img src="assets/demos/11-smart-copy.gif" alt="Smart Copy uses local inference to select and paste a code from real Codex output. Setup and a model wait are cut, with reading holds and labelled redactions; automatic pane naming did not apply." width="100%"></a></p>
</td>
<td width="50%" valign="top">
<p><strong>Queue prompts for an agent</strong></p>
<p><a href="assets/demos/13-prompt-queue.gif?raw=true"><img src="assets/demos/13-prompt-queue.gif" alt="Queue two prompts for a real Claude agent; Ilium sends each one after the previous turn ends, in order. Idle waits are cut; only a sampled review was completed." width="100%"></a></p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p><strong>Get results from background tasks</strong></p>
<p><a href="assets/demos/14-progress-monitor.gif?raw=true">Historical recording, not embedded: a scan found local scratch-path text in some frames and no full review was made (see the demo gallery). A real Claude agent monitors a ten-card task.</a></p>
</td>
<td width="50%" valign="top">
<p><strong>Search agents, files, and terminals</strong></p>
<p><a href="assets/demos/17-workspace-search.gif?raw=true">Historical recording, not embedded: a scan found local scratch-path text in some frames and no full review was made (see the demo gallery). Searches real agent output, a shell history and a file.</a></p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p><strong>See live agent goals in the project tree</strong></p>
<p><a href="assets/demos/22-goal-indicators.gif?raw=true"><img src="assets/demos/22-goal-indicators.gif" alt="A real Codex goal changes from active to paused and back in the project tree. Setup is cut; captions, a closing hold and labelled account and path redactions are added." width="100%"></a></p>
</td>
<td width="50%" valign="top">
<p><strong>Recover a session after a simulated reboot</strong></p>
<p><a href="assets/demos/24-agents-survive-reboots.gif?raw=true"><img src="assets/demos/24-agents-survive-reboots.gif" alt="A simulated reboot card precedes an isolated Ilium server shutdown and relaunch; a local fixture recovers a saved note. No machine reboot or provider is used." width="100%"></a></p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p><strong>Choose an animation, then watch it play</strong></p>
<p><a href="assets/demos/26-animations.gif?raw=true"><img src="assets/demos/26-animations.gif" alt="Thirteen of the 60 background animations, each shown first in Settings and then in its full-screen preview on the 2026-11-05 release build. Real capture in isolated state; scene-navigation steps are cut; chapter numbers skip 5 and 9 because two scenes were removed after review; the other scenes were not captured or were left out as too faint." width="100%"></a></p>
</td>
<td width="50%" valign="top">
<p><strong>Schedule input for a pane</strong></p>
<p><a href="assets/demos/04-scheduled-input.gif?raw=true"><img src="assets/demos/04-scheduled-input.gif" alt="Schedule input for a real Claude agent and a local terminal fixture in separate takes; setup is cut, reading holds are added, local paths are redacted, and captions were retimed to their actions." width="100%"></a></p>
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
<p><a href="assets/demos/07c-settings-tour-c.gif?raw=true"><img src="assets/demos/07c-settings-tour-c.gif" alt="Ilium Settings: titles, triggers, custom agents, debugging, API, and setup. Setup paths belong to the isolated demo; other pages are cut and reading holds are added. A local scratch path on the Setup page was masked in the late frames, so that line is blank." width="100%"></a></p>
</td>
<td width="50%" valign="top">
<p><strong>Move panes without stopping their processes</strong></p>
<p><a href="assets/demos/09-pane-tree.gif?raw=true"><img src="assets/demos/09-pane-tree.gif" alt="Ilium moves a running fixture pane; setup is cut, captions and a hold are added, and automatic naming and restructuring are disabled." width="100%"></a></p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p><strong>Request updates from active agents</strong></p>
<p><a href="assets/demos/12-ask-for-update.gif?raw=true"><img src="assets/demos/12-ask-for-update.gif" alt="Ilium asks idle Claude and Codex agents for updates while skipping an agent awaiting approval; pane names were staged, setup and waits were cut, and a reading pause was added. Local scratch paths and title-inference error text were masked frame by frame, so the affected lines are blank." width="100%"></a></p>
</td>
<td width="50%" valign="top">
<p><strong>Track work on a Kanban board</strong></p>
<p><a href="assets/demos/15-kanban.gif?raw=true"><img src="assets/demos/15-kanban.gif" alt="Move a staged Kanban card to Doing, mark it complete, add a note, then reopen the board and verify the saved Markdown file. Setup is cut; column width is 15 cells, with added captions, pointer, arrows and holds. A local scratch path in the file-picker title was masked frame by frame; the masked callout area in the last scene is blank." width="100%"></a></p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p><strong>Start a Codex agent from a TODO</strong></p>
<p><a href="assets/demos/16-agent-from-line.gif?raw=true">Historical recording, not embedded (a scratch path with the local username and a title-inference error are visible; keyframe review 2026-11-05). Start Codex from a TODO line and repair the source. Setup is cut and reading holds are added; automatic title inference reports no user transcript.</a></p>
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
<p><strong>Coordinate local models with Chatroom</strong></p>
<p><a href="assets/demos/20-chatroom.gif?raw=true"><img src="assets/demos/20-chatroom.gif" alt="Two local models generate messages that a script posts to Ilium Chatroom; waits are shortened. The local username and host name in the shell prompt are masked." width="100%"></a></p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p><strong>Respond to confirmation prompts automatically</strong></p>
<p><a href="assets/demos/21-text-triggers.gif?raw=true"><img src="assets/demos/21-text-triggers.gif" alt="A real Claude agent receives an automatic text-trigger reply. Setup and an idle wait are cut, with reading holds added; automatic naming and restructuring are disabled in the isolated demo." width="100%"></a></p>
</td>
<td width="50%" valign="top">
<p><strong>Restore a saved session</strong></p>
<p><a href="assets/demos/23-snapshot-resume.gif?raw=true"><img src="assets/demos/23-snapshot-resume.gif" alt="An isolated Ilium server shuts down and relaunches; a local session fixture recovers a saved note. The reboot card is simulated; no provider or machine reboot is used." width="100%"></a></p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p><strong>Create an isolated agent worktree</strong></p>
<p><a href="assets/demos/25-worktree-agent.gif?raw=true"><img src="assets/demos/25-worktree-agent.gif" alt="Ilium creates a real worktree through its native workflow and starts a local fixture in an isolated demo project; terminal states are held, captions added and a path redacted." width="100%"></a></p>
</td>
<td width="50%" valign="top">
<p><strong>Convert a session between Claude and Codex</strong></p>
<p><a href="assets/demos/26-convert-session.gif?raw=true">Historical recording, not embedded: local scratch paths and a plan label are visible in its frames (see the demo gallery). Real Claude and Codex sessions recall a fictional mascot after conversion in both directions.</a></p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p><strong>Queue access to a shared resource</strong></p>
<p><a href="assets/demos/28-resource-queue.gif?raw=true"><img src="assets/demos/28-resource-queue.gif" alt="Two isolated projects reserve, queue for, take and release a shared resource in the full interface; local fixtures, captions and path redactions are used." width="100%"></a></p>
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
- **Track**: identify agent CLIs from process trees and read activity from terminal screens, with optional model icons in the tree. Choose Normal or Attention status icons, or configure ordered Custom icon rules. Stale activity checks show a caution with their age on hover. [Agent monitoring](src/docs/agent-monitoring.md), [Custom status icons](src/docs/custom-status-icons.md)
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
