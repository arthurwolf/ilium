# Native releases

`targets.toml` is the authoritative five-target inventory. Each archive contains
the matching `ilium` and `ilium-server`, `VERSION`, generated third-party notices
and any audited runtime libraries. The client resolves its sibling server from
the same version directory. Package checks never execute downloaded bytes before
their checksum and member inventory have passed.

## Source checks

Run from the repository root with Python 3.11 or newer:

```sh
python release/scripts/release_tool.py targets --manifest release/targets.toml
python release/scripts/release_tool.py generate-posix-table --manifest release/targets.toml --installer release/install.sh --check
python release/scripts/release_tool.py generate-windows-table --manifest release/targets.toml --installer release/install.ps1 --check
python -m unittest discover -s release/tests -p 'test_*.py'
```

The complete POSIX fixture suite runs on Linux. Windows runs its PowerShell
contracts and Pester suite under both Windows PowerShell 5.1 and current
PowerShell. Portable source tests do not establish native platform support.
Run workspace tests, all-target Clippy and formatting separately. Native
qualification must use the target's own runner, loader inspection and PTY.

## Windows installers

The `windows-installers` job repackages the audited `ilium-windows-x86_64.zip`
into a per-user WiX MSI (`ilium-windows-x86_64.msi`) and an Inno Setup EXE
(`ilium-windows-x86_64-setup.exe`). Both install flat into
`%LOCALAPPDATA%\Programs\ilium`, append that directory to the user `PATH` and need
no elevation. `build_windows_installers.py build` refuses any ZIP member outside
the install.ps1 allowlist; `smoke` silently installs, runs `ilium --version` and
uninstalls each package on the disposable runner. `release_pipeline.py aggregate`
binds the `windows-installers.json` receipt to the qualified ZIP hash and the exact
native-audit file hashes. The MSI, EXE and receipt are then attested and published
beside the archives, under version-free names so `releases/latest/download/<name>`
stays stable. They are not listed in `SHA256SUMS` or on the Pages site; GitHub asset
digests and `candidate.json` carry their hashes. The packages are unsigned, so
SmartScreen may warn until code signing is added. Render the sources offline with
`build_windows_installers.py render`. Tool versions are pinned in the workflow
(`wix` 5.0.2, Inno Setup 6.5.4).

## Linux packages

The `linux-packages` job repackages each audited `ilium-linux-<arch>.tar.gz` (x86_64
on `ubuntu-22.04`, aarch64 on `ubuntu-22.04-arm`, matching `targets.toml`) into five
formats with `build_linux_packages.py`: `.deb`, `.rpm`, `.AppImage`, `.flatpak` and
`.snap`, all named `ilium-linux-<arch>.<extension>` so `releases/latest/download/<name>`
stays stable. Nothing is rebuilt: the packages carry the exact audited bytes, and
`release_pipeline.py aggregate` binds each `linux-packages-<arch>.json` receipt to the
qualified tarball hash and the native-audit file hashes. Like the Windows installers
they are attested and published beside the archives, and are not listed in
`SHA256SUMS`, on the Pages site, or in the five-target install gates.

deb, rpm and the AppImage directory install `ilium`, `ilium-server` and the bundled
`libonnxruntime.so.1` into `/usr/lib/ilium` (Snap: `lib/ilium`, Flatpak: `/app/lib/ilium`)
with `/usr/bin` symlinks. The client resolves its sibling server through
`current_exe()`, which follows the symlink, and the `$ORIGIN` runpath finds the
runtime library. Dependencies are derived, not hand-written: the builder parses the
`DT_NEEDED` entries of every payload ELF, refuses any soname that is neither bundled
nor in its reviewed `SYSTEM_LIBRARIES` table, and refuses a payload that references
glibc newer than the 2.35 build baseline. The same table yields the deb `Depends` and
the rpm `Requires`. Packages are reproducible: every timestamp is `2000-01-01`, tar and
ar metadata are normalised, and rpm, squashfs and OSTree use their reproducible-build
switches.

Format decisions:

- **deb** is written directly (ar + tar.xz) so it needs no `dpkg-deb` or `fakeroot`.
- **rpm** uses `rpmbuild` with `AutoReqProv: no`, no debuginfo and no brp post-processing,
  because those steps would rewrite the audited bytes.
- **AppImage** is the SHA-pinned upstream type2 runtime (`release/packaging/linux/tools.json`)
  plus a zstd squashfs. `AppRun` copies the payload once into
  `${XDG_DATA_HOME:-~/.local/share}/ilium/appimage/<version>-<id>` and runs from there,
  because the session server daemonises and must outlive the FUSE mount.
- **Snap** is `snap pack` of a hand-written `snap.yaml` with `confinement: classic` (Ilium spawns
  host shells and agents and reads the host process tree). Classic confinement needs
  Snap Store review, so the asset installs only with `snap install --dangerous --classic`.
- **Flatpak** is an OSTree bundle of the payload on `org.freedesktop.Platform//24.08`.
  The sandbox is a real limit: panes run inside it, so host tools such as `git`, `claude`
  and `codex` are absent, and only sandbox processes are visible to agent detection.
  Treat it as experimental until Ilium can spawn panes on the host (`flatpak-spawn --host`).

`smoke_linux_packages.py` never trusts the builder: `inspect` unpacks every package and
compares its payload with the audited hashes (it is the only check that can cover an
architecture the machine cannot run), `containers` installs the deb on Ubuntu 22.04,
Ubuntu 24.04 and Debian 12, the rpm on Fedora 41 and openSUSE Leap 15.6 and runs the
AppImage with extract-and-run on Ubuntu 24.04, and `host` installs Snap, Flatpak, deb and
the AppImage FUSE mount on the disposable runner. Every install checks the audited
hashes, `--version`, removal, and `release/packaging/linux/lifecycle.sh`: an unprivileged
`new-pane`, `ls` and `kill-session` that proves the installed pair spawns its detached
server. `release/packaging/linux/vm_smoke.py` runs the host smoke in a throw-away
qemu/KVM Ubuntu 24.04 VM, so Snap and Flatpak can be checked without touching a developer
machine. Not covered: Debian/Ubuntu releases older than the glibc 2.35 baseline, RHEL 9
and derivatives (glibc 2.34), distribution repositories, package signing, and
Snap Store or Flathub publication.

## Licence and runtime inputs

`licence_inventory.py` binds every Cargo.lock identity to real notice bytes.
It verifies original registry `.crate` archives against Cargo.lock, binds
metadata and notice bytes to those archives, and follows local path
dependencies, including both vendored crates. `licence-sources.json` supplies
hash-pinned publisher notices absent from crate archives. Publisher explanations
are preserved; separately labelled SPDX reference text does not invent a
copyright attribution. Old hash-addressed notice files are retained as evidence.

```sh
python release/scripts/licence_inventory.py --workspace /absolute/checkout --registry-source /absolute/cargo/registry/source --source-register /absolute/checkout/release/licence-sources.json --notice-directory /absolute/new-notices --output /absolute/new-dependencies.json
```

Outputs must be new task-owned paths. Cargo notice coverage alone does not cover
native dependencies. `native_candidate.py` recursively inspects both executable
roots and includes only reachable runtime libraries. Unknown dependencies fail
qualification. Windows uses a pinned native ONNX Runtime source build with
upstream `--enable_msvc_static_runtime` and compiles the Rust pair with
`+crt-static`. Recursive native `dumpbin` inspection must reject remaining
MSVCP, VCRUNTIME or UCRT dependencies; a source-build receipt alone does not
prove that the complete executable and DLL closure uses static CRT.

`ort-source.json` identifies the official ONNX Runtime source archive used for
the runtime notices. Intel macOS requires the native Xcode source-build receipt.
Windows requires the pinned `build_windows_ort.py` source-build receipt and ships
the resulting shared `onnxruntime.dll`; the target manifest requires this source
strategy. Retain source, toolchain, build flags, library hashes and original
licence notices with native dependency and inference evidence.

The Windows helper runs only on the native `windows-2022` runner. Its explicit
inputs are the source register/archive, new source-build and Cargo output paths,
Cargo workspace/home, receipt and Cargo-environment output paths, runner identity
and bounded parallelism. The workflow supplies these arguments. The helper
builds both the ONNX shared library and the Rust executable pair and records
`publication_allowed=false`; independent candidate and installed-runtime checks
must qualify the resulting bytes.

Apple Silicon uses the official shared-library archive pinned in
`ort-runtime.json`. Linux x86_64 and aarch64 use the corresponding official
shared-library archives from the same register; this avoids linking the
cached static archive against newer glibc-only `__isoc23_*` symbols. That
register also retains historical Windows prebuilt metadata; it does not
replace the current Windows source-build requirement.
`ort_runtime.py` verifies archive and member hashes,
embedded source identity, safe symlink targets and original notices before
extracting to a new owned directory. Its receipt supplies `ORT_LIB_LOCATION`
and the explicit runtime root. `ORT_PREFER_DYNAMIC_LINK` alone does not select
shared linking in the locked `ort-sys` build. Extraction remains unqualified
until native linkage, loader relocation, dependency closure and inference pass.

`embedding-model.json` pins the test-only model and tokenizer
assets; these assets are excluded from product archives. The actual candidate
client computes a finite nonzero 384-dimensional vector. On macOS, the auditor
independently checks the held process's loaded runtime before qualification.

## Publication and installation gates

The release workflow consumes the manifest rather than maintaining a separate
target matrix. A workflow dispatch validates candidates without publishing.
Tag publication uses the exact workspace version and retains all native evidence.

The required order is native build/audit/package, five isolated candidate
installs, aggregate qualification, draft upload/readback, immutable prerelease
publication, five explicit-tag GitHub installs, Pages preview byte/header
verification and five explicit-tag preview installs, compatibility checks using
the previous production scripts, recovery preparation, stable/latest activation,
five default preview installs,
production deployment and five literal production installs. Completion requires
receipt reconciliation for every target. Publication ordering and recovery are
implemented in `release_pipeline.py`; failed stages are not completion evidence.

`native_install.py` is under **release/tests/**. It runs the prebuilt native
`pty_tui_smoke` harness against the installed pair and rejects a zero-test result.
Local fixtures, explicit-tag acquisition, preview default acquisition and the
literal production command are separate receipts. Final Windows default-command
tests use only the disposable GitHub-hosted account, with real user PATH writes,
launcher execution and ownership-aware uninstall. They do not alter a live
developer account or shim the installer.

Pages contains exactly six files: index.html, install.sh, install.ps1,
manifest.json, _headers and 404.html. The build copies canonical installer bytes;
the deployment manifest records the five archive checksums. Provider readback
must verify public bytes, response headers, unknown-path 404 and deployment
identity. Local HTTP fixtures do not prove Cloudflare behavior.

Capture the previous stable/latest release and production deployment before
activation. Recovery must restore both latest selection and Pages: an old
installer still selects GitHub latest. Preserve failed immutable releases, tags,
assets and receipts. A first release has no predecessor; recovery must explicitly
represent that state and must not claim a working stable install remains. Before
activation, prepare and verify a withdrawal site on a preview deployment. If a
first release fails after production exposure, quarantine its latest selection
and publish that withdrawal site with installer endpoints returning 404.

Retain write intentions, provider responses and authoritative readbacks in the
channel journal. An ambiguous response requires inspection of the owned release
or deployment before another write. Recovery must stop on a channel owned by
another run. Runner cancellation can interrupt recovery; retain enough evidence
to resume it rather than claiming that an automatic recovery job guarantees
restoration.

## Evidence and documentation

Keep source commit, runner, target, archive and binary hashes, native loader and
embedding output, install/PTY receipts, release readbacks and Pages readbacks.
Do not print secrets. Proposed, built, audited, uploaded, published and publicly
installed are distinct states. Advertise the commands in README only after all
five native literal-command receipts pass. Preserve the existing demo gallery
and inference/privacy text when updating README.
