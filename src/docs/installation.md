# Installation

This page explains every supported way to install Ilium: the hosted one-line installers, the release packages for Linux, macOS and Windows, checksum verification, upgrading, uninstalling, and the prerequisites for native animations. No Rust toolchain is needed for any of these routes. To compile Ilium yourself, see [Building from source](building-from-source.md).

Status note: at the last check (2026-10-03) no releases had been published and the hosted installers were not yet live. Until releases exist, [build from source](building-from-source.md). The GitHub Actions artifacts of the [release workflow](https://github.com/arthurwolf/ilium/actions/workflows/release.yml) are candidate builds, not qualified releases. All download links below select assets from the newest published release and will work once one exists.

## Contents

- [Platform support](#platform-support)
- [Before you start](#before-you-start)
- [Option 1: hosted installer](#option-1-hosted-installer)
- [Option 2: release packages](#option-2-release-packages)
- [Linux packages](#linux-packages)
- [macOS packages](#macos-packages)
- [Windows packages](#windows-packages)
- [Verifying checksums](#verifying-checksums)
- [Native-animation prerequisites](#native-animation-prerequisites)
- [First run](#first-run)
- [Upgrading](#upgrading)
- [Uninstalling](#uninstalling)
- [Troubleshooting](#troubleshooting)

## Platform support

Linux is the primary platform. macOS and Windows support is implemented, with different test coverage. Worktree post-create commands are Linux-only. Ilium is early software; check the [CI results](https://github.com/arthurwolf/ilium/actions) for your platform.

| Platform | Architectures | Package formats |
| --- | --- | --- |
| Linux | x86_64 (Intel/AMD), aarch64 (ARM64) | tar.gz, deb, rpm, AppImage, Snap, Flatpak |
| macOS | Apple Silicon (aarch64), Intel (x86_64) | tar.gz, zip, pkg, dmg |
| Windows | x86_64 (native AMD64) | setup exe, msi, zip |

Linux packages target the glibc 2.35 baseline (Ubuntu 22.04, Debian 12 and the tested Fedora and openSUSE environments). Flatpak uses its declared platform runtime. All packages are unsigned: check the asset digest on the release page.

## Before you start

1. Use a UTF-8 terminal with 256-colour support.
2. Install your agent CLI (Claude Code, Codex, Antigravity, or another) separately. Ilium launches and watches agents; it does not bundle them.
3. Decide on one installation route. Use one installer, not several: for example, do not install both the MSI and the setup exe on Windows.
4. Optional but useful: `ffmpeg` and `ffprobe` for the video animations, and `pw-record` or `parec` (Linux) for the audio-spectrum animation. See [Animations](animations.md).

## Option 1: hosted installer

The installers pick your architecture, download the matching archive, verify it against the release checksums, and install it for your user without administrator rights.

Linux and macOS:

```sh
curl -fsSL https://ilium-setup.pages.dev/install.sh | sh
```

Windows PowerShell:

```powershell
irm https://ilium-setup.pages.dev/install.ps1 | iex
```

### What the shell installer does

1. Detects your operating system and architecture.
2. Downloads the archive for the requested version (default: the latest release) from immutable tagged assets at `https://github.com/arthurwolf/ilium/releases` together with `SHA256SUMS`.
3. Verifies the archive checksum, then validates the package contents (client, server, `VERSION`, third-party notices and, when present, the animation helper and the two bundled animations).
4. Installs a complete version directory under `~/.local/share/ilium/versions/<version>` and records an ownership receipt under `installer-state`, so uninstall removes only files the installer put there.
5. Atomically switches a small `current` pointer file to the new version.
6. Places launchers in `~/.local/bin` and, unless you disable it, adds that directory to your shell profile through a recorded block that uninstall can remove again.

The data directory respects `XDG_DATA_HOME` and the launcher directory respects `XDG_BIN_HOME`.

### Shell installer options

Pass options after `sh -s --` when piping, or run a downloaded copy directly.

| Option | Meaning |
| --- | --- |
| `--version V` | Install a specific version (for example `1.2.3`) instead of the latest. |
| `--install-dir DIR` | Absolute directory for the version store (default `${XDG_DATA_HOME:-$HOME/.local/share}/ilium`). |
| `--bin-dir DIR` | Absolute directory for launchers (default `${XDG_BIN_HOME:-$HOME/.local/bin}`). |
| `--no-modify-path` | Do not edit any shell profile. |
| `--uninstall` | Remove the installation the installer owns. |

Example, installing a pinned version without touching your profile:

```sh
curl -fsSL https://ilium-setup.pages.dev/install.sh | sh -s -- --version 1.2.3 --no-modify-path
```

Paths must be absolute and free of control characters, `:` and `..` components; otherwise the installer stops with a diagnostic naming the stage, version, target and a recovery command.

### What the PowerShell installer does

It supports Windows PowerShell 5.1 and current PowerShell, works when piped to `iex`, and needs no elevation, no execution-policy change and no machine-wide `PATH` change. It installs under `%LOCALAPPDATA%\ilium`, updates your user `PATH`, and supports `-Version V`, `-InstallDir DIR`, `-BinDir DIR`, `-NoModifyPath` and `-Uninstall`. Open a new terminal after installing so the new `PATH` applies. Native AMD64 is the only supported Windows architecture; the installer checks the real operating-system architecture rather than environment variables.

To pass parameters while using a downloaded script:

```powershell
& .\install.ps1 -Version 1.2.3 -NoModifyPath
```

## Option 2: release packages

Download packages from [GitHub Releases](https://github.com/arthurwolf/ilium/releases). Asset names are stable: `ilium-<os>-<arch>.<extension>`, with `<os>` of `linux`, `macos` or `windows` and `<arch>` of `x86_64` or `aarch64`.

Every portable archive contains the client (`ilium`), the server (`ilium-server`), the animation helper, the two bundled `.iliumanim` animations and runtime libraries. Keep them together. The helper and animations live beside the client; moving only `ilium` prevents bundled animation discovery. Add the extracted package directory to `PATH`, then run `ilium` in your project.

Download the archive and [`SHA256SUMS`](https://github.com/arthurwolf/ilium/releases/latest/download/SHA256SUMS) from the same release, and [verify the checksum](#verifying-checksums) before extracting.

## Linux packages

### Archive (tar.gz)

Choose your architecture:

- Intel/AMD: [`ilium-linux-x86_64.tar.gz`](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-linux-x86_64.tar.gz)
- ARM64: [`ilium-linux-aarch64.tar.gz`](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-linux-aarch64.tar.gz)

1. Download the archive and `SHA256SUMS` into one directory.
2. Verify: `sha256sum --ignore-missing --check SHA256SUMS`. Confirm your archive reports `OK`.
3. Extract into a new directory: `mkdir ilium-package && tar -xzf ilium-linux-x86_64.tar.gz -C ilium-package`. Check the archive layout with `tar -tzf` if the extracted directory is nested.
4. Add the directory that contains `ilium` to `PATH`.
5. Run `ilium` in your project.

### deb package

Download: [x86_64](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-linux-x86_64.deb) · [aarch64](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-linux-aarch64.deb).

1. Verify the checksum.
2. Install on Debian, Ubuntu or Mint: `sudo apt install ./ilium-linux-x86_64.deb`.

It installs to `/usr/lib/ilium` and links `ilium` into `/usr/bin`. The package declares Bubblewrap as a dependency.

### rpm package

Download: [x86_64](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-linux-x86_64.rpm) · [aarch64](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-linux-aarch64.rpm).

1. Verify the checksum.
2. Fedora: `sudo dnf install ./ilium-linux-x86_64.rpm`. openSUSE: `sudo zypper install ./ilium-linux-x86_64.rpm`.

Same layout as the deb, with the same Bubblewrap dependency.

### AppImage

Download: [x86_64](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-linux-x86_64.AppImage) · [aarch64](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-linux-aarch64.AppImage).

1. Verify the checksum.
2. `chmod +x ilium-linux-x86_64.AppImage`
3. `./ilium-linux-x86_64.AppImage`

The AppImage needs FUSE. The first run copies Ilium to `~/.local/share/ilium/appimage` so the session server outlives the mount. Check that `/usr/bin/bwrap` exists on the host for native animations.

### Snap package

Download: [x86_64](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-linux-x86_64.snap) · [aarch64](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-linux-aarch64.snap).

1. Verify the checksum.
2. `sudo snap install --dangerous --classic ilium-linux-x86_64.snap`

The snap uses classic confinement, so it is not on the Snap Store. Commands: `ilium`, `ilium.server` and `ilium.helper` (the bundled animation helper).

### Flatpak bundle

Download: [x86_64](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-linux-x86_64.flatpak) · [aarch64](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-linux-aarch64.flatpak).

1. Verify the checksum.
2. `flatpak install --user ilium-linux-x86_64.flatpak`

Flatpak is experimental. The package forwards its trusted CLI to the host from the exact running Flatpak deployment, so panes can use host tools. This grants Ilium full host-command access; Flatpak is a distribution format here, not a security boundary for the CLI. Animation packages still run only in Ilium's separately confined helper. A candidate must pass the real installed host-launch, animation-resource and process-cleanup checks before it is published as a qualified release.

### How Linux packages are qualified

The Linux release gate requires receipt-bound installed-file checks, client, server and helper versions, Beach and Carpet rendering through the installed helper, an isolated pane lifecycle, and verified removal on each native architecture. Distribution containers cover deb, rpm and AppImage extract-and-run; disposable native runners cover deb, Snap, the real Flatpak sandbox and an AppImage FUSE mount. Separate VM evidence and public installation evidence remain distinct from candidate checks. Details: [Linux acceptance boundaries](../../release/RELEASING.md#linux-acceptance-boundaries).

## macOS packages

Choose your archive:

- Apple Silicon: [`ilium-macos-aarch64.tar.gz`](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-macos-aarch64.tar.gz)
- Intel: [`ilium-macos-x86_64.tar.gz`](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-macos-x86_64.tar.gz)

1. Download the archive and `SHA256SUMS`.
2. Compare `shasum -a 256 <archive>` with the archive's entry in `SHA256SUMS`.
3. Extract with `tar -xzf <archive>` into a new directory.
4. Add the package directory to `PATH`.

The hosted installer selects your architecture and uses the same layout as Linux.

The macOS package builders also produce these formats (publication pending):

| Mac | ZIP | Installer PKG | Disk image DMG |
| --- | --- | --- | --- |
| Apple Silicon | [Download ZIP](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-macos-aarch64.zip) | [Download PKG](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-macos-aarch64.pkg) | [Download DMG](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-macos-aarch64.dmg) |
| Intel | [Download ZIP](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-macos-x86_64.zip) | [Download PKG](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-macos-x86_64.pkg) | [Download DMG](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-macos-x86_64.dmg) |

The PKG needs administrator approval and installs into `/usr/local/lib/ilium/<version>/<arch>`. Add that directory to `PATH`; the package does not change your shell configuration. You can keep separate versions and choose the one your shell uses.

The ZIP, PKG and DMG packages are unsigned and not notarized. macOS may block downloaded software. Review the release and verify its digest before approving an installation through macOS security controls.

## Windows packages

For a guided install, use [`ilium-windows-x86_64-setup.exe`](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-windows-x86_64-setup.exe) or [`ilium-windows-x86_64.msi`](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-windows-x86_64.msi). Both install for your user without administrator rights, add Ilium to your `PATH`, and uninstall from Windows Settings. Use one installer, not several. To upgrade, run the newer installer.

To unpack by hand, choose [`ilium-windows-x86_64.zip`](https://github.com/arthurwolf/ilium/releases/latest/download/ilium-windows-x86_64.zip):

1. Download the zip and `SHA256SUMS`.
2. Compare the output of `Get-FileHash .\ilium-windows-x86_64.zip -Algorithm SHA256` with its entry in `SHA256SUMS`.
3. Extract: `Expand-Archive .\ilium-windows-x86_64.zip -DestinationPath .\ilium-package`
4. Add the directory containing all three executables, both bundled animations and the runtime DLLs to your user `PATH`.
5. Open a new terminal and run `ilium`.

## Verifying checksums

Every release carries a `SHA256SUMS` file listing the digest of each asset. Always download it from the same release as your package.

| Platform | Command |
| --- | --- |
| Linux | `sha256sum --ignore-missing --check SHA256SUMS` (run in the download directory) |
| macOS | `shasum -a 256 <archive>`, then compare with `SHA256SUMS` |
| Windows | `Get-FileHash <file> -Algorithm SHA256`, then compare with `SHA256SUMS` |

A mismatch means a damaged or altered download: delete it and download again. Do not extract or install it.

## Native-animation prerequisites

Native animations run in a separately confined helper. They need:

- `/usr/bin/bwrap` (Bubblewrap), and
- a Linux cgroup-v2 delegation that lets Ilium create its own child group with the `memory`, `pids` and `cpu` controls.

The deb and rpm declare Bubblewrap as a dependency. For an archive, AppImage or classic Snap installation, check that Bubblewrap is available on the host. Installing it does not by itself grant cgroup delegation or user-namespace permission; those come from your system (cgroup-v2 controller delegation to your user and permission to create user namespaces). Release qualification demonstrates those permissions and cleans up its own processes. See [Animations](animations.md) for what the scenes do.

## First run

Open a new terminal in your project and run `ilium`. First-run setup walks through AI providers, notification sounds, keyboard practice and an optional voice test. Reopen it with `ilium --onboarding` or **Settings → Guided setup**. First-run setup lets you choose Kilo Gateway, paid APIs, local Ollama or Skip; automatic AI requests stay paused until setup finishes. See [Getting started](getting-started.md) and [Inference and privacy](inference-and-privacy.md).

## Upgrading

| Installation | How to upgrade |
| --- | --- |
| Hosted installer | Run the installer again; it installs the new version beside the old and switches the `current` pointer. |
| deb / rpm | Install the newer package with the same command. |
| AppImage | Replace the downloaded image; remove old caches under `${XDG_DATA_HOME:-$HOME/.local/share}/ilium/appimage` if wanted. |
| Snap / Flatpak | Install the newer file with the same command. |
| Windows setup exe / msi | Run the newer installer. |
| macOS pkg | Install the newer package; versions live side by side under `/usr/local/lib/ilium/<version>/<arch>`. |
| Archives | Extract the new archive into a new directory and point `PATH` at it. |
| Built from source | Pull, rebuild and `make install` again. |

A running session keeps the old `ilium-server` loaded until it restarts. After installing a new server binary, `ilium --restart-server` replaces the project's running server and keeps its snapshot, so panes are recovered (see [Session recovery](session-recovery.md)).

## Uninstalling

Stop your Ilium sessions first (`ilium kill-session <name>` for each, see [CLI reference](cli-reference.md)).

| Installation | Command |
| --- | --- |
| Hosted shell installer | `curl -fsSL https://ilium-setup.pages.dev/install.sh \| sh -s -- --uninstall` |
| Hosted PowerShell installer | Run the installer script with `-Uninstall` |
| deb | `sudo apt remove ilium` |
| rpm | `sudo dnf remove ilium` (or the zypper equivalent) |
| Snap | `sudo snap remove ilium` |
| Flatpak | `flatpak uninstall --user io.github.arthurwolf.Ilium` |
| AppImage | Delete the image and the version cache under `${XDG_DATA_HOME:-$HOME/.local/share}/ilium/appimage` that you intend to remove |
| Windows setup exe / msi | Windows Settings, Installed apps |
| Archive / source build | Delete the directory or the installed binaries |

The shell uninstaller removes only versions it installed (checked against receipts), preserves modified or unknown content, and removes the profile block it added. Your project files and `.ilium/` session data are separate from the package installation and are not touched. Your global settings in `~/.config/ilium/config.toml` also remain.

## Troubleshooting

- `ilium: command not found`: the install directory is not on `PATH`. Open a new terminal, or add `~/.local/bin` (hosted installer), the package directory (archives) or `$CARGO_HOME/bin` (source install) to `PATH`.
- Bundled animations are missing: the helper and the two `.iliumanim` files must sit beside `ilium`. Do not move the client on its own.
- AppImage will not start: install FUSE support, or extract and run it with `--appimage-extract`.
- macOS blocks the download: the packages are unsigned and not notarized; verify the digest and approve through macOS security settings.
- Native animations fail: check `/usr/bin/bwrap`, unprivileged user namespaces and cgroup-v2 delegation (see above).
- Installer fails: it prints `stage`, `version`, `target` and a `recovery` command; re-run that command, adding `--no-modify-path` if profile editing is the problem.
- Old behaviour after upgrade: the running server is the old one; use `ilium --restart-server`.
