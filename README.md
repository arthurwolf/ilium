<p align="center"><img src="assets/ilium-mark.svg" alt="" width="68" height="68"></p>
<h1 align="center">Ilium</h1>
<p align="center"><strong>AI names agents and organizes your project tree.</strong><br>Keep agent work, terminals, editors, and boards together, with activity in view.</p>
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
<p><strong>AI names agents and organizes the project tree</strong></p>
<p><a href="assets/demos/01-ai-tree.gif?raw=true" target="_blank" rel="noopener noreferrer"><img src="assets/demos/01-ai-tree.gif" alt="Ilium names agent panes and organizes the project tree around their work" width="100%"></a></p>
</td>
<td width="50%" valign="top">
<p><strong>Track agent status and current goals</strong></p>
<p><a href="assets/demos/08-agent-activity.gif?raw=true" target="_blank" rel="noopener noreferrer"><img src="assets/demos/08-agent-activity.gif" alt="Agent status changes as work progresses, with its goal shown in the tree" width="100%"></a></p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p><strong>Control Ilium and prompt agents with voice</strong></p>
<p><a href="assets/demos/02-voice-control.gif?raw=true" target="_blank" rel="noopener noreferrer"><img src="assets/demos/02-voice-control.gif" alt="Voice commands trigger Ilium actions and send a prompt to Claude" width="100%"></a></p>
</td>
<td width="50%" valign="top">
<p><strong>Use agents, editors, and terminals side by side</strong></p>
<p><a href="assets/demos/10-mixed-splits.gif?raw=true" target="_blank" rel="noopener noreferrer"><img src="assets/demos/10-mixed-splits.gif" alt="View terminal, editor, and board panes together in a split view" width="100%"></a></p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<p><strong>Resume agent sessions after a reboot</strong></p>
<p><a href="assets/demos/24-agents-survive-reboots.gif?raw=true" target="_blank" rel="noopener noreferrer"><img src="assets/demos/24-agents-survive-reboots.gif" alt="After a reboot sequence, Ilium restores the pane layout and resumes Claude and Codex" width="100%"></a></p>
</td>
<td width="50%" valign="top">
<p><strong>Choose an animation, then watch it play</strong></p>
<p><a href="assets/demos/26-animations.gif?raw=true" target="_blank" rel="noopener noreferrer"><img src="assets/demos/26-animations.gif" alt="A gallery of recorded animation demos" width="100%"></a></p>
</td>
</tr>
</tbody>
</table>

All demos, with the feature each one shows: [Demo gallery](src/docs/demos.md).

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
- **Track**: identify agent CLIs from process trees and read activity from terminal screens. [Agent monitoring](src/docs/agent-monitoring.md)
- **Separate**: start agents in linked Git worktrees. Unclear file, process, or ownership state blocks cleanup. [Worktrees](src/docs/worktrees.md)
- **Resume**: detach without stopping panes. Recovery rebuilds the layout and relaunches pane programs. [Session recovery](src/docs/session-recovery.md)

Also included:

- Agent [cost tracking](src/docs/agent-cost.md) and [notifications](src/docs/notifications.md).
- Optional remote compaction of Claude and Codex sessions through your Inference model, configured in [Settings](src/docs/settings.md#remote-compaction).
- A compaction optimizer (**Settings → Optimization**) that scans your Codex and Claude Code transcripts and recommends the cheapest auto-compaction threshold, applied to the agent's own config only after you confirm, with one-click revert. See [Settings](src/docs/settings.md#optimization).
- AI [titles, tree organization and custom instructions](src/docs/titles-and-instructions.md), with [provider and privacy controls](src/docs/inference-and-privacy.md).
- [Voice control](src/docs/voice.md).
- [Scheduled input, text triggers, prompt queues, progress monitors and Chatroom](src/docs/automation.md).
- [Editors, boards](src/docs/editors-and-boards.md) and [Smart Copy](src/docs/smart-copy.md).
- Ambient [animated backgrounds](src/docs/animations.md): landscapes, space, maps, live data, video and more, all sharing one look.

Agent worktrees and recovery in short: start an agent with `Ctrl+B W` or

```sh
ilium new-pane --worktree --branch agent/fix-login -- codex
```

After a restart, Ilium restores the layout and relaunches pane programs, resuming verified Claude Code, Codex, or Antigravity sessions when provider data remains. Unsaved process state does not survive. Snapshots live in `<project>/.ilium/sessions/`; add `.ilium/` to `.gitignore` to keep them out of Git.

## Command line

| Command | What it does |
| --- | --- |
| `ilium` | Attach to or create the current project's `default` session. |
| `ilium new-session <name>` / `ilium ls` | Create or attach to a named session / list sessions. |
| `ilium new-pane [--keep-open] -- <cmd>` | Add a terminal pane without attaching the TUI. The pane closes when the command exits unless `--keep-open` is given. |
| `ilium chat --help` / `ilium progress --help` | Chatroom and progress commands. |
| `ilium voice say --help` | Typed voice input. |
| `ilium panes` / `ilium broadcast <message>` | List panes, or message agents, across every running session; filter by project, agent, state, text or regex. |

Every command and flag: [Command-line reference](src/docs/cli-reference.md).

## Settings and data

Open Settings with `Ctrl+B :` and the key map with `Ctrl+B ?`. Global settings live in `~/.config/ilium/config.toml` on Linux. Terminals, agent detection and session storage work without an LLM; AI titles, tree organization and optional Smart Copy suggestions use the provider you choose. File logging is off by default. See [Settings](src/docs/settings.md) and [How it works](src/docs/how-it-works.md).

## Project, help, and licence

Report problems in [issues](https://github.com/arthurwolf/ilium/issues) with your version, OS, terminal, command and observed behaviour; remove private content from logs. Design: [ARCHITECTURE.md](ARCHITECTURE.md). Contribution rules: [AGENTS.md](AGENTS.md) and [Contributing](src/docs/contributing.md).

Ilium is [MIT licensed](LICENSE). The vendored `vt100` patch and `tui-tree-widget` fork retain their MIT licences. Cascadia Code uses SIL Open Font License 1.1; see the [font notice](ilium-client/assets/fonts/NOTICE.md).
