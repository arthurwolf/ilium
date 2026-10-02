<p align="center"><img src="assets/ilium-mark.svg" alt="" width="68" height="68"></p>
<h1 align="center">Ilium</h1>
<p align="center"><strong>AI names agents and organizes your project tree.</strong><br>Keep agent work, terminals, editors, and boards together, with activity in view.</p>
<p align="center"><sub>Linux · macOS · Windows · Rust · MIT</sub></p>
<p align="center"><a href="#quick-start">Quick start</a> · <a href="#daily-use">Daily use</a> · <a href="#see-it-in-action">Demos</a> · <a href="#worktrees-and-session-recovery">Worktrees</a> · <a href="#full-reference">Reference</a></p>

## Quick start

Linux and macOS:

```sh
curl -fsSL https://ilium-setup.pages.dev/install.sh | sh
```

If you prefer package installers, use the [.deb](#deb-package), [.rpm](#rpm-package), [AppImage](#appimage), [Snap](#snap-package) or [Flatpak](#flatpak-bundle) (Linux).

Windows PowerShell:

```powershell
irm https://ilium-setup.pages.dev/install.ps1 | iex
```

Prefer a download-and-run installer on Windows? Get the latest release as a
[setup `.exe`](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-windows-x86_64-setup.exe)
or an [`.msi`](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-windows-x86_64.msi).
Both install for your user without administrator rights, add Ilium to your `PATH`,
and uninstall from Windows Settings. Use one installer, not several. To upgrade, run the newer installer.

Open a new terminal in your project and run `ilium`. First-run setup walks through AI providers, notification sounds, keyboard practice and an optional voice test. Reopen it with `ilium --onboarding` or **Settings → Guided setup**. Re-run the installer to upgrade.

No Rust toolchain is needed. Use a UTF-8 terminal with 256-colour support and install your agent CLI separately.

[Release packages](#install-from-release-packages) · [Build from source](#building-from-source)

> **AI privacy:** First-run setup lets you choose Kilo Gateway, paid APIs, local Ollama or Skip. Automatic AI requests remain paused until setup finishes. Existing configurations keep their provider; Kilo Gateway sends prompts to its service. [Inference settings](#inference-and-privacy).

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
<tr>
<td colspan="2" width="100%" valign="top">
<p>
<strong>Choose an animation, then watch it play</strong>
</p>
<p>
<a href="assets/demos/26-animations.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/26-animations.gif" alt="All 26 animations: open Settings, select a scene, close Settings, and watch it play" width="100%">
</a>
</p>
</td>
</tr>
</tbody>
</table>

<details>
<summary>Show the other 15 demos</summary>

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
<tr>
<td width="50%" valign="top">
<p>
<strong>Convert a session between Claude and Codex</strong>
</p>
<p>
<a href="assets/demos/26-convert-session.gif?raw=true" target="_blank" rel="noopener noreferrer">
<img src="assets/demos/26-convert-session.gif" alt="A Claude session converts to Codex, which still remembers the secret word, then converts back to Claude" width="100%">
</a>
</p>
</td>
<td width="50%" valign="top"></td>
</tr>
</tbody>
</table>

</details>

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

Run `ilium` in the project directory to reattach.

In tmux, double the prefix to pass it through. Remap it in Settings.

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

Also included: workspace search, Smart Copy, scheduled input, prompt queues, and Chatroom.

### Agent monitoring

See agent identity, activity, and longer-running work in the tree. Attention mode highlights the highest-priority status. Hover for the reason.

Choose running indicators in **Settings → Agent Monitoring**. A live progress monitor can suppress finished-turn alerts while work continues.

The `/goal` badge reports observed activity; Ilium does not control the agent.

#### Notifications

**Settings → Sound** also holds the desktop-notification switches, one per event: agent finished, agent needs approval, task succeeded, and task failed or lost. A master switch turns them all off. They live under `[notifications]` in `config.toml` and apply to running sessions within a couple of seconds.

A *task* is a background job an agent registered with `ilium progress`. Its outcome is a different event from the agent finishing its turn, so notifications say "background task finished (agent still working)" and lead with the pane title. By default you hear about the agent finishing, an approval prompt, and failed or lost tasks. Successful tasks stay silent and show only as ✅ in the sidebar. Task alerts are also skipped while the agent is idle or parked, because its own finished alert follows, and same-kind alerts on one pane within 30 seconds merge into the first. Both rules can be changed. Task sounds follow the same rules.

### Agent cost

**Settings → Agent Cost** adds spend indicators, totals, and cost sorting to the tree. The default meter appears when you hover an agent. Measure estimated API dollars (default) or plan quota: percentage points of the Codex rate-limit window used while the agent ran (Codex only, account-wide). Thresholds, budget and history follow the chosen unit.

Costs estimate API list prices from agent and sub-agent transcripts. They can lag live use and do not represent subscription charges. A leading `~` flags an unknown model price.

Choose thresholds based on past sessions, current agents, a budget, or burn rate. Add unknown model prices under `[cost.prices]` in `config.toml`.

## Worktrees and session recovery

Start an agent with `Ctrl+B W`, the tree menus, or the CLI:

```sh
ilium new-pane --worktree --branch agent/fix-login -- codex
```

Use an unused branch. Set `--base <ref>` to choose a different starting point.

**Settings → Git** offers a Linux post-create command for submodules and LFS. Creation runs Git hooks and filters.

Ilium offers cleanup for worktrees it created when they are clean, merged, and unused. Unclear ownership or process state blocks removal.

Branch deletion is separate; discarding files requires the full path.

Snapshots live in `<project>/.ilium/sessions/`; rolling backups live in `.ilium/backups/`. Add `.ilium/` to `.gitignore` to keep session data out of Git. Backups exclude files edited inside pane applications.

After a restart, Ilium restores the layout and relaunches pane programs. It can resume verified Claude Code, Codex, or Antigravity sessions when provider data remains.

Unsaved process state does not survive.

- `--restart-server` keeps the snapshot.
- `--reset-session` deletes the snapshot.
- `ilium kill-session <name>` ends the session and its panes.

## Full reference

### Install and platform support

Linux is the primary platform. macOS and Windows support is implemented, with different test coverage. Worktree post-create commands are Linux-only.

Ilium is early software. Check [CI results](https://github.com/arthurwolf/ilium/actions) for your platform.

### Install from release packages

Download packages from [GitHub Releases](https://github.com/arthurwolf/ilium/releases). No Rust toolchain is needed.

No releases were published when checked on 2026-09-30. Until then, [build from source](#building-from-source). [Actions artifacts](https://github.com/arthurwolf/ilium/actions/workflows/release.yml) are candidate builds, not qualified releases.

Download your archive and [`SHA256SUMS`](https://github.com/arthurwolf/ilium/releases/latest/download/SHA256SUMS) from the same release. Verify the checksum before extracting.

Keep the client, server, and runtime libraries together. Add the extracted package directory to `PATH`, then run `ilium` in your project.

#### Linux packages

Choose your archive:

- Intel/AMD: [`ilium-linux-x86_64.tar.gz`](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-linux-x86_64.tar.gz)
- ARM64: [`ilium-linux-aarch64.tar.gz`](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-linux-aarch64.tar.gz)

Verify it in the download directory:

```sh
sha256sum --ignore-missing --check SHA256SUMS
```

Confirm the selected archive reports `OK`. Extract with `tar -xzf <archive>` into a new directory, then add the package directory to `PATH`.

The [installer](#quick-start) selects your architecture and verifies downloads. It stores packages under `~/.local/share/ilium` and launchers in `~/.local/bin`, respecting XDG overrides.

Each release also carries native Linux packages for both architectures (downloads below use the newest release). They hold the same audited files as the archive, under stable names: `ilium-linux-<arch>.<extension>` with `<arch>` of `x86_64` or `aarch64`. Every package needs glibc 2.35 or newer (Ubuntu 22.04, Debian 12, Fedora 41 or later) and is unsigned; check the asset digest on the release page.

##### deb package

Download: [x86_64](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-linux-x86_64.deb) · [aarch64](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-linux-aarch64.deb).

Debian, Ubuntu and Mint: `sudo apt install ./ilium-linux-x86_64.deb`. It installs to `/usr/lib/ilium` and links `ilium` into `/usr/bin`.

##### rpm package

Download: [x86_64](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-linux-x86_64.rpm) · [aarch64](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-linux-aarch64.rpm).

Fedora: `sudo dnf install ./ilium-linux-x86_64.rpm`. openSUSE: `sudo zypper install ./ilium-linux-x86_64.rpm`. Same layout as the deb.

##### AppImage

Download: [x86_64](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-linux-x86_64.AppImage) · [aarch64](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-linux-aarch64.AppImage).

`chmod +x ilium-linux-x86_64.AppImage && ./ilium-linux-x86_64.AppImage`. It needs FUSE. The first run copies Ilium to `~/.local/share/ilium/appimage` so the session server outlives the mount.

##### Snap package

Download: [x86_64](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-linux-x86_64.snap) · [aarch64](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-linux-aarch64.snap).

`sudo snap install --dangerous --classic ilium-linux-x86_64.snap`. It uses classic confinement, so it is not on the Snap Store. Commands: `ilium` and `ilium.server`.

##### Flatpak bundle

Download: [x86_64](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-linux-x86_64.flatpak) · [aarch64](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-linux-aarch64.flatpak).

Experimental: `flatpak install --user ilium-linux-x86_64.flatpak`. Panes run inside the sandbox, so your host `git`, `claude` and `codex` are not visible.

Packages are installed, run and removed in containers or disposable virtual machines before a release is published. To remove one, use your package manager (`apt remove ilium`, `dnf remove ilium`, `snap remove ilium`, `flatpak uninstall io.github.arthurwolf.Ilium`) or delete the AppImage and `~/.local/share/ilium/appimage`.

#### macOS packages

Choose your archive:

- Apple Silicon: [`ilium-macos-aarch64.tar.gz`](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-macos-aarch64.tar.gz)
- Intel: [`ilium-macos-x86_64.tar.gz`](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-macos-x86_64.tar.gz)

Compare this output with the archive's entry in `SHA256SUMS`:

```sh
shasum -a 256 <archive>
```

After the hashes match, extract with `tar -xzf <archive>` into a new directory. Add the package directory to `PATH`.

The [installer](#quick-start) selects your architecture and uses the same layout as Linux.

#### Windows packages

For a guided install use [`ilium-windows-x86_64-setup.exe`](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-windows-x86_64-setup.exe) or [`ilium-windows-x86_64.msi`](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-windows-x86_64.msi) from the same release. To unpack by hand, choose [`ilium-windows-x86_64.zip`](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-windows-x86_64.zip). Compare this output with its entry in `SHA256SUMS`:

```powershell
Get-FileHash .\ilium-windows-x86_64.zip -Algorithm SHA256
```

After the hashes match, extract into a new directory:

```powershell
Expand-Archive .\ilium-windows-x86_64.zip -DestinationPath .\ilium-package
```

Add the directory containing both executables and runtime DLLs to your user `PATH`.

The [installer](#quick-start) supports PowerShell 5.1+, installs under `%LOCALAPPDATA%\ilium`, and updates your user `PATH`. Open a new terminal after installing.

### Building from source

Install Git, [rustup](https://rustup.rs/), and the native tools below. Clone the repository: it includes patched dependencies needed for the build.

`rust-toolchain.toml` selects the Rust version. Cargo downloads dependencies on the first build.

These commands build a local client/server pair. For portable release packages, follow [the release guide](release/RELEASING.md).

#### Linux source build

On Debian/Ubuntu:

```sh
sudo apt-get update
sudo apt-get install -y git build-essential pkg-config libasound2-dev libssl-dev
git clone https://github.com/arthurwolf/ilium.git
cd ilium
cargo build --locked --release -p ilium -p ilium-server
./target/release/ilium
```

On other distributions, install equivalent C/C++ tools, Make, pkg-config, ALSA, and OpenSSL development packages.

Run `make install` to install both binaries into `~/.cargo/bin` (or `$CARGO_HOME/bin`). For another destination:

```sh
make install BIN_DIR="$HOME/.local/bin"
```

Add your chosen directory to `PATH`.

#### macOS source build

Install Xcode Command Line Tools (`xcode-select --install`), Git, and rustup. Use a native terminal for your architecture:

```sh
git clone https://github.com/arthurwolf/ilium.git
cd ilium
cargo build --locked --release -p ilium -p ilium-server
./target/release/ilium
```

Run `make install` to install both binaries into `~/.cargo/bin` (or `$CARGO_HOME/bin`). Choose another directory with `make install BIN_DIR="$HOME/.local/bin"`. Add it to `PATH`.

#### Windows source build

Install Git, rustup with the MSVC toolchain, and Visual Studio Build Tools with **Desktop development with C++** and a Windows SDK.

In Developer PowerShell for Visual Studio:

```powershell
git clone https://github.com/arthurwolf/ilium.git
Set-Location ilium
cargo build --locked --release -p ilium -p ilium-server
.\target\release\ilium.exe
```

Run from `target\release`, or copy both executables and required runtime DLLs into one directory on your user `PATH`.

Open a new terminal before running `ilium` in your project.

### Settings, inference, and privacy

Open Settings with `Ctrl+B :` and the active key map with `Ctrl+B ?`.

On Linux, global settings live in `~/.config/ilium/config.toml`. Use `[keyboard]` for prefixes and `[keybindings]` for action remapping.

#### Inference and privacy

Terminals, agent detection, and session storage work without an LLM. AI titles, tree organization, and optional Smart Copy suggestions use your selected provider.

New installs enable AI titles and tree organization through Kilo Gateway. Choose a local provider such as Ollama or disable the triggers before entering sensitive content.

Kilo's default free model was marked as permitting prompt training when checked on 2026-09-27. Review its [data and usage guidance](https://kilo.ai/docs/getting-started/using-kilo-for-free). You can also select OpenAI-compatible, Anthropic, or OpenRouter providers.

File logging is off by default. Enabled logs can retain project prompts and request bodies, with credentials redacted. Treat logs as sensitive.

#### Custom instructions

**Settings → LLM Instructions** collects seven optional instruction inputs. The same values are available in their feature tabs:

| Instructions | Feature tab |
|---|---|
| Voice assistant | Voice control |
| Entry naming | Titles |
| Organization; shared naming and organization context | Inference |
| Project naming; Smart Copy | Inference |
| Ask for update | Agent Monitoring |

Select an input and press Enter, or click it. In feature tabs, `i` focuses the instruction inputs. Apply with `Ctrl+S`; Esc cancels. Delete clears a selected input. Empty inputs use the built-in defaults. Shared context applies to entry names, project names, and tree organization.

Instructions are added to new requests and saved globally. Both locations edit the same value. Voice instructions refine the assistant's behavior; the other inputs refine their specific task while preserving its required output format.

### Automation and agent setup

Scheduled input and text triggers send commands to panes. Check the target before using them around confirmation prompts.

Reset forecasts use public Claude and Codex announcements. They cannot predict private rolling limits or guarantee a reset.

Optional agent setup adds marked Chatroom and progress instructions to Claude and Codex configuration files, preserving surrounding text. Chatroom setup also creates `CHATROOM.md` and hooks.

For long jobs, validate a JSON probe with `ilium progress check`, then register it with `ilium progress set`. Ilium polls from the project root and reports the result. Use absolute paths; see `ilium progress --help`.

The server accepts unauthenticated `POST /create_agent` requests on `127.0.0.1:8872`. Any local process can submit one. Keep this listener private; change `[http_api].port` if sessions collide.

### Voice

Enable voice control with `F8` or **Settings → Voice control**. It needs an OpenAI Realtime key, network access, and an attached client. Microphone input also needs audio devices.

Check the target before sending text. Destructive semantic actions require confirmation. Terminal submission includes Enter unless you enable its confirmation option.

Send typed text to the live voice conversation:

```sh
ilium voice say --start "what agents are running?"
printf '%s\n' "focus the first agent" "say hello to it" | ilium voice say -
```

`--start` saves the voice-on setting; `-` reads input lines.

Outside Ilium, pass `--cwd` and `--session-name`. An attached client is required. A JSONL result confirms queuing; check the target for the outcome. See `ilium voice say --help`.

### Ambient backgrounds

**Settings → Animations** adds a Braille background, with a full-screen preview (`f`). Choose landscapes, space scenes, video, images, or an audio spectrum.

Backgrounds are off by default. Ilium saves your choice per project in `.ilium/config.yaml`. The settings put scenes beside grouped controls; Loop playback builds 30 fps frames in the background and shows packed-frame RAM usage.

Choose **Semantic** and enable **Background** to use the animation recommended by AI tree reorganization. **Project** scope is the default; **Entry** follows the selected pane, group or split. Every reorganization records recommendations, including scene parameters: Paris work can use the offline Paris map, and pathfinding work can use Carpet's Snake. Changing selection makes no extra AI request. Missing recommendations show a status asking you to reorganize the project.

- **Stars, Earth, and satellite clouds** share a location set by address, coordinates, or map. Earth and cloud imagery need network access.
- **Voxel landscape** slowly pans over an isometric block world: 52 surface biomes, forests, deserts, villages, cave mouths and ravines, with 208 feature recipes. Choose zoom, detail and vegetation or structure density; use monochrome or pastel dithering with palette, hue, saturation and lightness controls. Its 64 pixel textures are original Ilium artwork.
- **Solar system** offers all eight planets, orbit paths, distance and size realism, and speeds up to ten simulated years per second.
- **Topographic maps** draws contour lines as Braille dots on a slowly panning map or a turning globe, from public elevation surveys embedded offline: Earth (NOAA ETOPO 2022), the Moon (NASA LOLA), Mars (MOLA), Venus (Magellan), Mercury (MESSENGER) and Ceres (Dawn), plus four generated fictional worlds. Choose the world or a cycle, flat or globe, zoom, number of contour lines or a fixed spacing in metres, index lines, thickness, dotted or hidden lines below zero, relief shading, colours, pan direction and speed, start position, and a zero-level shift or tide that floods and drains the shores.

All animations share one look and one set of display controls (Settings -> Animations, lower left): colour mode (Color, Greyscale or Monotone), 38 palettes (pastel, neon, sunset, ocean, viridis, solarized, nord and more), brightness (turn it down to keep the background discreet), contrast, gamma, colour intensity, hue shift, invert, edge fade, grey tint, 20 style presets (Whisper, Subtle, Neon night, Matrix, Amber terminal, Blueprint, Paper and ink and more), 15 dithering methods (Bayer, blue noise, halftone, scan lines, crosshatch, Floyd-Steinberg, Atkinson and more), pattern contrast and invert, a frame-rate cap, and whether the animation shows behind both panels, only the left tree panel or only the right terminal panes. Changing any of them changes every animation.
- **OpenStreetMap** draws real streets, buildings, water, green spaces and railways around ten world places. The bundled catalogue works offline; choose a place or a tour, hold or pan the map, toggle each layer, and adjust dot brightness, line width and the shared palette. Local Overpass JSON or an explicit HTTPS endpoint and coordinates are optional. Map data: OpenStreetMap contributors, ODbL.
- **Galactic empires** follows a procedural 320-system star map with rounded colored territories as empires expand, build fleets, make peace and fight to unite the galaxy. A quarter-galaxy view drifts clockwise; simulation and camera speeds are separate controls.
- **Hex expedition** pans over an endless sea of explorer's islands generated as you watch: jungle, savanna, desert, arctic and volcanic lands, each island with its shore, biomes, one moored ship, one distant goal temple or pyramid, and villages, camps, ruins, caves, mines and shrines placed where they belong. Maps are revealed in the next hex by hex, with animated water, swaying trees, smoke, campfires, geysers, lava and weather. The tile art is drawn by Ilium; no game artwork is used.
- **Vector TD** is a full-screen tower defense that plays itself, inspired by the [Vector TD](https://www.crazygames.com/game/vector-td) browser game. An AI builds, upgrades and unlocks glowing vector towers against waves of monsters (a boss every tenth wave), sends waves early when its defence is strong, and moves from wave to wave, map to map and level to level while towers and monsters grow stronger. Pick one of six maps or play them all in turn, the start level, waves per level, difficulty and game speed. Colours are black and white or a colour scheme (Neon, Cool, Warm, Phosphor) with brightness, contrast, hue and saturation sliders. The towers, monsters and maps are Ilium's own.
- **Stars** offers hour/day-per-second speeds, optional constellation lines and simulated satellites. Turning the horizon off includes the whole sky.
- **Lily pads** supports up to 64 opaque leaves, with optional rooted placement.
- **Video** needs `ffmpeg`; random scenes also need `ffprobe`. Folder scans, decoding and cleanup run away from input handling, with limits and timeouts.
- **Audio spectrum** uses `pw-record` or `parec` on Linux and WASAPI on Windows. macOS needs a loopback device such as BlackHole.
- **Images** accepts files, folders, or URLs, with slideshow and pan/zoom options.
- **Wikipedia** scrolls random articles linked from today’s English Main Page, keeping headings, references, infoboxes, and images. Choose readable Text or the default Braille with zoom; use greyscale or Wikipedia, Pastel, Sepia, and Night palettes, then adjust hue, saturation, lightness, and scroll speed. Downloads are cached for offline reuse.
- **Live graphs** offers 32 public keyless series: eight crypto markets with genuine OHLC candles, eight daily ECB currency reference rates, solar wind and magnetic measurements, ISS orbital estimates, earthquake activity, Wikipedia edit aggregates and Quicknet randomness. Choose line, bars or candles, time window and refresh rate. Provider minimum intervals apply; received and observed times are shown separately.
- **Digits of Pi** scrolls up to 20,000 exact digits as native text or real-font Braille. Choose font size, scroll speed, brightness and a hue for each digit.
- **Live earthquakes** plots the USGS all-day feed, including tiny, zero, negative and unknown magnitudes, with animated markers and magnitude labels. It refreshes at most once per minute.
- **Live aircraft** plots airborne positions reported by OpenSky over a Braille world coastline. Anonymous global access is limited, so refresh is at least 15 minutes. Coverage is incomplete.
- **Live boats** uses public Finnish AIS positions from Digitraffic, with a 30-second minimum refresh. It covers Finnish waters, not the global fleet. Aircraft and boats have separate map and marker colors and brightness.
- **Live chess** follows Lichess TV with dithered pieces, board orientation and independent colors. Clocks show the latest feed values; connection failures retain the last board with its receipt age.
- **Carpet** bends parallel isometric hatch lines over hidden moving spheres and tubes. Choose mouse hunters, food-seeking Snake, slow Conway Life, automated legal chess, Lichess TV chess, a bouncing DVD ball, planetary orbits, or digital and analog clocks. Snake plans safe food routes with a tapered body, feeding pulses and gently breathing food. Carpet defaults to a 12×12 Snake board; saved project settings take precedence. Camera, hatch spacing, lift, object size, easing, timing and each mode's behavior are configurable; clocks use an explicit UTC offset independently of animation speed.

Live sources retain the last good data on request failures; they do not substitute simulated events. Quicknet values are displayed without BLS signature verification.

Imagery comes from NASA GIBS and EUMETSAT (Copyright EUMETSAT). Address search uses Open-Meteo and GeoNames data (CC BY 4.0).

### Command-line reference

| Command | What it does |
| --- | --- |
| `ilium` | Attach to or create the current project's `default` session. |
| `ilium new-session <name>` / `ilium ls` | Create or attach to a named session / list sessions. |
| `ilium new-pane -- <cmd>` | Add a terminal pane without attaching the TUI. |
| `ilium new-pane --session-name <name> -- <cmd>` | Add a terminal pane to a named session. |
| `ilium chat --help` / `ilium progress --help` | Show Chatroom and progress commands. |
| `ilium voice say --help` | Show typed voice input. |

Run from your project directory or pass `--cwd`. Angle brackets mark placeholders. See `ilium --help` for all commands.

### Editors, boards, and Smart Copy

Use the mouse to focus panes, move entries, and scroll history.

The editor supports Markdown previews and autosave. Boards store cards in a Markdown file or folder you choose.

Smart Copy freezes the visible screen and offers regions to copy. AI suggestions select coordinates; clipboard text comes from the captured screen.

### How it works

One `ilium-server` per project session owns pane processes and session state. Detaching leaves them running.

The client connects over local IPC. Ilium identifies agents from processes and reads activity from terminal screens.

See [ARCHITECTURE.md](ARCHITECTURE.md) for the design.

### Project, help, and licence

Report problems in [issues](https://github.com/arthurwolf/ilium/issues). Include your version, OS, terminal, command, and observed behaviour. Remove private content from logs.

See [AGENTS.md](AGENTS.md) for contribution rules and [ARCHITECTURE.md](ARCHITECTURE.md) for design and prior art.

Workspace checks:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --no-fail-fast
```

Ilium is [MIT licensed](LICENSE). The vendored `vt100` patch and `tui-tree-widget` fork retain their MIT licences. Cascadia Code uses SIL Open Font License 1.1; see the [font notice](ilium-client/assets/fonts/NOTICE.md).
