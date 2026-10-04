# Building from source

This page shows how to compile Ilium on Linux, macOS and Windows, install the result with `make install`, and place the bundled animations so they are discovered. Building is optional when [release packages](installation.md) are available, and is the only route until a release is published.

## Contents

- [What you build](#what-you-build)
- [Prerequisites](#prerequisites)
- [Linux](#linux)
- [macOS](#macos)
- [Windows](#windows)
- [make install](#make-install)
- [Bundled animations](#bundled-animations)
- [Release-quality packages](#release-quality-packages)
- [Running the checks](#running-the-checks)
- [Troubleshooting](#troubleshooting)

## What you build

Three executables make up a working installation:

| Executable | Package | Role |
| --- | --- | --- |
| `ilium` | `ilium` | The command-line entry point and the terminal UI. |
| `ilium-server` | `ilium-server` | The per-project session server that owns panes. |
| `ilium-animation-helper` | `ilium-animation-js` | The confined helper that runs native animation packages. |

Two animation packages, `beach-1.0.0.iliumanim` and `carpet-1.0.0.iliumanim`, live in `ilium-animation-js/assets/packages/` and must sit beside the executables for the bundled animations to be found. See [How it works](how-it-works.md) for how these pieces cooperate.

## Prerequisites

1. Git.
2. [rustup](https://rustup.rs/). The repository's `rust-toolchain.toml` pins Rust `1.96.1` with the `clippy` and `rustfmt` components, so rustup installs the right version automatically on first use.
3. Native tools for your platform (below).
4. Clone the repository itself rather than downloading a source archive of a single crate: it includes patched dependencies (`vendor/vt100`, `vendor/crossterm-0.29.0`, `vendor/ratatui-image-11.0.6` and `ilium-client/vendor/tui-tree-widget`) that the build needs. Because of those patches Ilium cannot be installed from crates.io.

Cargo downloads other dependencies on the first build, so you need network access once. The first build takes a while; later builds are incremental.

## Linux

On Debian or Ubuntu:

1. Install the native packages:

   ```sh
   sudo apt-get update
   sudo apt-get install -y git build-essential pkg-config libasound2-dev libssl-dev
   ```

2. Clone and enter the repository:

   ```sh
   git clone https://github.com/arthurwolf/ilium.git
   cd ilium
   ```

3. Build the three executables:

   ```sh
   cargo build --locked --release -p ilium -p ilium-server -p ilium-animation-js --bin ilium --bin ilium-server --bin ilium-animation-helper
   ```

4. Copy the bundled animations beside the binaries:

   ```sh
   cp ilium-animation-js/assets/packages/{beach,carpet}-1.0.0.iliumanim target/release/
   ```

5. Run Ilium:

   ```sh
   ./target/release/ilium
   ```

On other distributions, install the equivalent C/C++ tools, Make, pkg-config, and the ALSA and OpenSSL development packages (for example `alsa-lib-devel` and `openssl-devel` on Fedora).

To lower the impact on a busy machine you can prefix the build with `nice -n 19 ionice -c 3`.

## macOS

1. Install the Xcode Command Line Tools: `xcode-select --install`.
2. Install Git and rustup.
3. Use a native terminal for your architecture (not one running under Rosetta, unless you want an Intel build).
4. Clone, build and copy the animations:

   ```sh
   git clone https://github.com/arthurwolf/ilium.git
   cd ilium
   cargo build --locked --release -p ilium -p ilium-server -p ilium-animation-js --bin ilium --bin ilium-server --bin ilium-animation-helper
   cp ilium-animation-js/assets/packages/{beach,carpet}-1.0.0.iliumanim target/release/
   ./target/release/ilium
   ```

5. Optionally run `make install` (below) and add its directory to `PATH`.

## Windows

1. Install Git, rustup with the MSVC toolchain, and Visual Studio Build Tools with **Desktop development with C++** and a Windows SDK.
2. Open Developer PowerShell for Visual Studio.
3. Clone, build and copy the animations:

   ```powershell
   git clone https://github.com/arthurwolf/ilium.git
   Set-Location ilium
   cargo build --locked --release -p ilium -p ilium-server -p ilium-animation-js --bin ilium --bin ilium-server --bin ilium-animation-helper
   Copy-Item ilium-animation-js\assets\packages\beach-1.0.0.iliumanim,ilium-animation-js\assets\packages\carpet-1.0.0.iliumanim target\release\
   .\target\release\ilium.exe
   ```

4. Run from `target\release`, or copy all three executables, both animations and required runtime DLLs into one directory on your user `PATH`.

The `make` target is aimed at Linux and macOS; on Windows copy the files manually as above.

## make install

`make install` builds the release binaries and installs all three executables and both animation packages into one directory.

| Make variable | Default | Meaning |
| --- | --- | --- |
| `CARGO_HOME` | `$HOME/.cargo` | Cargo home; used to derive `BIN_DIR`. |
| `BIN_DIR` | `$(CARGO_HOME)/bin` | Destination directory. |

Targets: `build` (release build of the three binaries), `install` (build, then copy), `test` (`cargo test --workspace`).

1. Run `make install`. This installs into `~/.cargo/bin` (or `$CARGO_HOME/bin`).
2. For another destination:

   ```sh
   make install BIN_DIR="$HOME/.local/bin"
   ```

3. Add that directory to `PATH` if it is not already.

Executables are installed with mode 755 and the animation packages with mode 644, side by side, which is what animation discovery expects.

## Bundled animations

The helper and the two `.iliumanim` packages must share a directory with `ilium`. When run from `target/release` after the copy step, or after `make install`, discovery works. If you move only `ilium`, the bundled animations disappear from Settings. Native animations additionally need Bubblewrap and cgroup-v2 delegation on Linux; see [Installation](installation.md#native-animation-prerequisites) and [Animations](animations.md).

## Release-quality packages

Portable packages (archives, deb, rpm, AppImage, Snap, Flatpak, pkg, dmg, msi, setup exe) are produced by the release tooling in `release/`, not by `make install`. See the [release guide](../../release/RELEASING.md).

## Running the checks

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --no-fail-fast
```

See [Contributing](contributing.md).

## Troubleshooting

- Linker error about `alsa` or `openssl`: install the ALSA and OpenSSL development packages and `pkg-config`.
- `error: package ... not found` or patched-crate errors: you are not building from a full clone. Clone the repository; do not copy a single crate out.
- Wrong Rust version: let rustup read `rust-toolchain.toml`; do not override with another toolchain.
- `ilium` runs but the bundled animations are absent: the helper and the two `.iliumanim` files are not beside the executable.
- `ilium` shows `command not found` after `make install`: add `BIN_DIR` to `PATH`.
- An old server keeps running after a rebuild: run `ilium --restart-server` (the snapshot is kept), see [Session recovery](session-recovery.md).
- Windows build fails to link: open Developer PowerShell so the MSVC tools and Windows SDK are on the path.
