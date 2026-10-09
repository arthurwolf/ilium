# Native releases

`targets.toml` is the authoritative five-target inventory. Each archive contains
the matching `ilium`, `ilium-server` and `ilium-animation-helper`, verified
`beach-1.0.0.iliumanim` and `carpet-1.0.0.iliumanim`, `VERSION`, generated
third-party notices and any audited runtime libraries. The client resolves its sibling server, animation helper and bundled plugin
archives from the same version directory. Package checks never execute downloaded bytes before
their checksum and member inventory have passed.

## GitHub Packages release bundle

The `ghcr-package` job distributes the accepted release assets as one OCI asset
bundle at `ghcr.io/arthurwolf/ilium-release:<release-tag>`. The bundle contains
installers and native archives; it is retrieved with ORAS rather than run as a
container. Direct Release downloads remain available for individual platforms.

This job depends on `complete`, after all five public installations and the final
Release/Pages readback. Manual dispatch never publishes. Its dedicated token has
package write permission; the other jobs retain their existing permissions. The
publisher binds every payload hash to the immutable Release receipt, including
installers outside the native archive `SHA256SUMS` inventory.

Matching existing versions are verified without another push; a conflicting
version fails without replacement. Success additionally requires an anonymous
full registry pull with exact file hashes and GitHub metadata proving public
visibility and linkage to `arthurwolf/ilium`. The metadata API uses the scoped
workflow credential; anonymous download verification uses an empty registry
credential configuration. Neither source annotations nor an authenticated push
alone prove public availability. A newly created private package must have its
visibility and repository access reconciled before the same version can pass.

### First publication and visibility recovery

GitHub creates a new Container registry package as private. Linking the package
to a public repository inherits access permissions, not public visibility.
See [GitHub package visibility](https://docs.github.com/en/packages/learn-github-packages/configuring-a-packages-access-control-and-visibility).

After the qualified tagged workflow creates `ilium-release`, the release owner
opens the exact [Ilium package page](https://github.com/users/arthurwolf/packages/container/package/ilium-release)
and selects **Package settings**,
confirms the owner is `arthurwolf`, the package is `ilium-release`, and the linked
repository is `arthurwolf/ilium`, then changes that package's visibility to
Public. Preserve repository access inheritance; do not change other packages.
This is part of first-publication work, not a substitute for the native gates.
Do not upload a placeholder package to create the settings page.

If anonymous verification fails after the upload, retain the failed job's
`ghcr-publication-receipt` artifact, especially `ghcr-intent.json` and
`publisher.jsonl`. Change visibility only for the verified qualified package,
then rerun the failed package job on the **same tagged workflow run**. Do not
move the tag, replace its digest, or dispatch a new workflow: dispatch is
nonpublishing. The publisher verifies an existing matching version without
another push and rejects conflicting bytes. Completion requires the successful
receipt's exact manifest digest, public metadata, and anonymous full-pull hashes;
a visible Package entry by itself does not establish usable distribution.

The `ghcr-publication-receipt` Actions artifact retains publication/readback
evidence. A passing job must identify the Package page and immutable manifest
digest. These contracts do not claim that a package is already publicly
available; actual tagged workflow execution and public readback remain required.

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
the install.ps1 allowlist. Its `smoke` command requires `--tag`, `--installers`,
`--archive`, `--audit-report`, `--manifest` and `--log`. The workflow supplies the
downloaded native Windows ZIP and `native-audit.json`, the source target manifest,
the built installer directory and a new log directory under the runner's temporary
directory. The log directory's parent must already exist. The gate validates the
complete audited ZIP and the unchanged schema-1 `windows-installers.json` receipt,
then binds both installer hashes and every payload hash before installation.

Native execution requires x64 Windows, 64-bit Python 3.11 or newer and an exclusive
disposable account. The GitHub-hosted job supplies that account; an independently
prepared test account requires `--disposable-account`. That flag records an
operator attestation and does not create or isolate an account. Preflight refuses
an existing installation, related MSI/Inno registration, an equivalent user PATH
entry, reparse paths, or a pre-existing empty `Programs` directory that the MSI
could remove. Use the original default per-user install location; do not prepare
a real user's account by deleting their files, registrations or PATH entries.

After the ordinary MSI and EXE transactions, the native gate runs seven additional
EXE install/reinstall/uninstall transactions using labelled PATH fixtures in that
same exclusive account: absent, empty REG_SZ, empty REG_EXPAND_SZ, trailing empty
segments with each registry type, a quoted slash alias and an environment-expanded
pre-existing entry. Each fixture has independent lifecycle logs and requires exact
typed restoration before the original account PATH is restored. A different or
unacknowledged PATH state is preserved and fails the gate.

Three further transactions append labelled synthetic foreign-tool suffix entries
after reinstall, starting from absent, empty REG_SZ and trailing REG_EXPAND_SZ
baselines. Uninstall must preserve those exact suffix entries and registry types
while removing the owned Ilium entry. The expected removal readback includes the
suffix before any compensating write; restoring a lost suffix afterward cannot
turn that failed uninstall into a pass. Only a successful exact fixture readback
authorizes returning the exclusive account to its original PATH.

The EXE retains its original PATH ownership receipt under its existing per-user
uninstall key. Reinstall keeps the first baseline. A pre-existing equivalent entry
is borrowed and left unchanged; an appended entry is removed only with a matching
receipt and unchanged original prefix. Concurrent suffix entries are retained,
while a moved, duplicated or otherwise ambiguous owned entry prevents uninstall.
Registry writes preserve an existing string type. The receipt does not provide an
atomic compare-and-swap against unrelated registry writers: the script rechecks
immediately before writing and requires an exact readback. Native compilation and
these executable fixture results are required acceptance evidence; portable tests
alone cannot qualify this installer behavior.

For each format, the gate installs, checks the complete flat file inventory and
all audited hashes, and requires exact `ilium --version` and
`ilium-server --version` output plus the helper's one-line JSONL `--version` result and both official archive hashes. Only Inno's recorded `unins000.exe` and
`unins000.dat` are accepted outside the audited payload. The installed client uses
the public `--cwd PROJECT new-pane -- SYSTEM_CMD /d /q /k COMMAND`, `ls` and
`kill-session default` commands with a fresh task-owned project and a unique
shell-written marker. The gate holds the original job-owned server and shell
process handles, observes both exits and requires the private job to become empty.
It then reinstalls the same retained package and repeats file, version and
lifecycle checks. Reinstallation must preserve the first installed PATH and
registration snapshots. Installer clients have a 300-second wait bound; installed
CLI commands have a 60-second bound. A timeout or required reboot fails the gate.

Uninstall acceptance requires the directory to disappear, the retained MSI
product identities to become unknown, and the typed user PATH, machine PATH and
scoped registration snapshots to match their original state. The gate records
primary and cleanup failures separately. Compensating PATH restoration is limited
to an acknowledged installer transition and cannot turn failed uninstall into a
pass. Unrecognized concurrent PATH edits remain untouched. An unsettled MSI
transaction or unverified process retirement prevents further uninstall/PATH
writes; a partial EXE installation without captured uninstaller custody also
requires separate reconciliation. The gate never stops the shared MSI service or
signals a process selected only by name or cached PID.

Child execution uses absolute installed image paths, fresh application state and
an installed-directory/system-directory PATH with inherited runtime overrides
removed. The evidence explicitly records `physical_source_hiding=false`: this
gate does not make checkout or build files physically inaccessible. Any additional
source-hidden native acceptance must be run and recorded separately using
task-owned copies and preserving the original source and user-owned VM state.

Retain `smoke.jsonl`, job journals, command streams and installer logs under
`--log`; the existing `diagnostics-windows-installers` upload retains that directory
on success or failure for 30 days. The terminal smoke result binds the original
input hashes and reports native EXE/MSI status separately from public release
verification. Portable import and regression checks belong to the source suite,
which is discovered on every native target as well as the source job. Passing
those checks proves neither native installer acceptance nor publication. Actual
EXE/MSI acceptance still requires a successful run with the final audited artifacts.

`release_pipeline.py aggregate` binds `windows-installers.json` to the qualified
ZIP hash and exact native-audit file hashes. Workflow success gates installer
artifact upload and aggregation; aggregate does not consume the diagnostic smoke
journal as a separate qualification receipt. The MSI, EXE and receipt are then attested and published
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

deb, rpm and the AppImage directory install `ilium`, `ilium-server`,
`ilium-animation-helper`, the two official `.iliumanim` archives and bundled
`libonnxruntime.so.1` into `/usr/lib/ilium` (Snap: `lib/ilium`, Flatpak: `/app/lib/ilium`)
with `/usr/bin` symlinks for the public client and server. The client resolves
its sibling server and private animation helper through
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
  Its initial trusted CLI forwards the whole application to the host from the
  exact running deployment. Panes, server, helper and native process detection
  then use the host interfaces; Flatpak itself is a distribution wrapper, not
  the confinement boundary for unverified animation packages. Treat the bundle
  as experimental until the installed native host-launch and animation gates pass.

Native animation qualification additionally requires `/usr/bin/bwrap`, a unified
cgroup-v2 hierarchy and a writable delegated ancestor of the actual launcher with
`memory`, `pids` and `cpu` enabled for child groups. The child joins its own group
before executing Bubblewrap. The deb and rpm declare the Bubblewrap dependency;
tarball, AppImage and classic Snap need installed-runtime proof of that executable
and the kernel permissions. Package installation, an outer Snap or Flatpak sandbox,
and a successful version command are not evidence of native animation confinement.
An unavailable prerequisite must fail the installed animation gate; no installer
changes host cgroup ownership or controller configuration.

Each Linux architecture also executes the retained native sandbox test artifact
through `native_sandbox_runner.py` before package evidence is sealed. The four
exact cases cover descendant retirement, task limits, physical memory limits,
and ordinary service helper startup with `Delegate=no`. Receipts bind the test,
helper and Rust source hashes, require one executed passing test per case, and
require fresh owned services to be retired. Seal readback compares the copied
artifact metadata and digest with the retained regular artifact file. Aggregation
retains separate `audits/<rust-target>-native-sandbox-artifact.json` files for the
two Linux architectures; candidate readback inventories these files and checks
their bytes against each sealed marker before release or registry publication. The
x86_64 minimum-system VM receives the matching candidate runtime and artifact
and repeats these cases as its disposable unprivileged user. Cloud-init installs
Bubblewrap explicitly before the guest readiness marker, so the sandbox gate
does not depend on a package installation happening later. These checks
supplement the installed package animation/V8 gates; they do not replace them.
Source integration is implemented; native execution and complete candidate
qualification of this integration remain required before publication.


Flatpak cannot create the helper's required nested namespaces inside its own
sandbox. The initial CLI instead reads `[Instance] app-path` in `/.flatpak-info`,
which Flatpak documents as the host path of the *running* `/app` deployment,
including a private user installation. It executes that deployment's `ilium`
through `flatpak-spawn --host`; the host CLI hashes itself and its sibling server
and helper against the in-sandbox payload before starting any session. The
package's `[Session Bus Policy]` grants `org.freedesktop.Flatpak=talk`, which
Flatpak documents as arbitrary host-command authority. This is deliberate for a
terminal multiplexer that spawns host agents; it must be reviewed as such, not
represented as Flatpak confinement. The existing host helper retains bounded
binary pipes, Bubblewrap, its owned cgroup and post-bootstrap syscall seal.
`flatpak-spawn --watch-bus` is only an additional lifetime hint: upstream source
retries without it on `INVALID_ARGS`, so it is not an ownership certificate.
The native Flatpak gate must prove PTY and stdio behavior, exact three-binary
identity, host cgroup delegation, real Beach/Carpet rendering, task and physical
memory bounds, owned cancellation and descendant drain. Do not replace any of
these checks with a version response or a passed package installation.

### Linux acceptance boundaries

`smoke_linux_packages.py` compares package and installed files with the supplied
`linux-packages-<arch>.json` receipt. The smoke receipt alone does not authenticate
its own source: `release_pipeline.py aggregate` separately binds its tarball hash
and complete `package_files` map to the qualified native archive and audit. Preserve
both layers when evaluating a candidate.

Each mode has explicit supported formats. Empty, duplicate or unsupported requests
fail. `inspect --formats flatpak` is unsupported; Flatpak byte checks occur after
installation into a fresh private user installation. Its required host gate remains
in the workflow.

| Mode | Required formats | Evidence and execution environment |
| --- | --- | --- |
| `inspect` | `deb,rpm,appimage,snap` | Nonexecuting extraction and receipt-bound payload/layout checks; may inspect another architecture. |
| `containers` | `deb,rpm,appimage` | Native Docker architecture; deb on Ubuntu 22.04, Ubuntu 24.04 and Debian 12; rpm on Fedora 41 and openSUSE Leap 15.6; AppImage extract-and-run on Ubuntu 24.04. |
| `host` | `snap,flatpak,deb,appimage` | Native disposable unprivileged account; real snapd, Flatpak sandbox and AppImage FUSE mount. Package operations and protected installation readbacks use noninteractive sudo; application checks stay unprivileged. |

A passed runtime result requires all receipt-listed audited members, including
client, server, VERSION, runtime libraries and third-party notices, at the actual
installed or mounted locations. It also checks the finite package-owned layout and
launcher targets. The installed client, server and helper must report the exact expected
versions, and the installed client must complete the real isolated lifecycle.
AppImage checks include the mounted/extracted AppDir and the materialized cache used
by the entrypoint; extract-and-run does not qualify FUSE support. Flatpak checks
enter through the actual sandbox and must verify that its shipped CLI reaches
the matching host deployment and retains the native animation limits above.

`release/packaging/linux/lifecycle.sh` creates one isolated pane and requires the
exact `default running` listing. It allows twelve bounded listing probes for the
server's session record to become visible; a stopped default session or another
running session does not qualify it. It then kills the owned default session and
checks that it is no longer running. Preserve this bounded readiness behavior and
its regression tests when changing package acceptance.

Removal command success and authoritative readback are both required. A failed
manager query is unverified absence, and dangling launchers, installed payloads or
retained mounts fail the gate. Host checks refuse existing Ilium installations or
data they would otherwise adopt. Private state is removed only after the relevant
package or mount teardown is verified; failed cleanup remains failure evidence.
`--flatpak-user-dir` must name a new directory whose parent exists. Both callers
supply it explicitly so lifecycle HOME/XDG isolation cannot select another Flatpak
installation. No user project data is an uninstall target.

### Run and retain the native Linux gates

Run the following from the source checkout on each architecture's disposable
native machine, using that candidate's `linux-packages/` directory. The workflow
uses `ubuntu-22.04` for x86_64 and `ubuntu-22.04-arm` for aarch64. Runtime modes
reject a different native or Docker-daemon architecture. Installing `rpm`,
`rpm2cpio`, `cpio`, `squashfs-tools`, `flatpak` and `fuse3` supplies the additional
Ubuntu inspection/sandbox tools. Docker, working snapd, a usable FUSE device/helper,
an unprivileged account with noninteractive sudo, and network access for distro
and Flatpak runtime dependencies are required. Missing prerequisites fail the
requested gate; do not turn them into qualified skips or disable the sandbox.

```bash
set -euo pipefail # Preserve a failed native gate when tee records its JSONL output.
case "$(uname -m)" in # Select the actual native Linux architecture.
    x86_64) architecture=x86_64 ;; # Use the Intel/AMD package set.
    aarch64|arm64) architecture=aarch64 ;; # Use the ARM64 package set.
    *) printf '%s\n' 'Unsupported native Linux architecture' >&2; exit 1 ;; # Refuse cross-architecture qualification.
esac # Complete the supported native architecture selection.
package_directory="$(pwd)/linux-packages" # Use the candidate produced from this architecture's audited tarball.
native_directory="$(pwd)/native-linux" # Keep the matching native audit and archive from this architecture's build.
native_audit="$native_directory/native-audit.json" # Bind both execution modes to the exact audited payload.
native_archive="$native_directory/ilium-linux-$architecture.tar.gz" # Retain the archive paired with these packages.
release_tag=$(python -c 'import json,sys; print(json.load(open(sys.argv[1], encoding="utf-8"))["tag"])' "$package_directory/linux-packages-$architecture.json") # Read the builder's pinned tag.
verification_directory=$(mktemp -d "${TMPDIR:-/tmp}/ilium-linux-acceptance.XXXXXX") # Keep logs and private installation paths outside release assets.
python release/scripts/smoke_linux_packages.py inspect --arch "$architecture" --packages "$package_directory" --formats deb,rpm,appimage,snap | tee "$verification_directory/inspect.jsonl" # Require all supported nonexecuting inspectors.
mkdir -p "$verification_directory/containers" "$verification_directory/host" # Retain the exact complete terminal result streams for sealing.
python release/scripts/smoke_linux_packages.py containers --arch "$architecture" --packages "$package_directory" --formats deb,rpm,appimage --log "$verification_directory/containers" --audit-report "$native_audit" --workspace "$(pwd)/Cargo.toml" --manifest "$(pwd)/release/targets.toml" | tee "$verification_directory/containers/containers-results.jsonl" # Require six installed helper sessions in native distribution containers.
python release/scripts/smoke_linux_packages.py host --arch "$architecture" --packages "$package_directory" --formats snap,flatpak,deb,appimage --log "$verification_directory/host" --flatpak-user-dir "$verification_directory/flatpak-install" --audit-report "$native_audit" --workspace "$(pwd)/Cargo.toml" --manifest "$(pwd)/release/targets.toml" | tee "$verification_directory/host/host-results.jsonl" # Require four real host-format transactions with fixed Flatpak identity.
python release/scripts/native_sandbox_runner.py --artifact-directory "$native_directory/evidence" --helper "$native_directory/candidate/ilium-animation-helper" --workspace "$(pwd)/Cargo.toml" --output "$verification_directory/native-sandbox" # Execute all four exact ordinary-service cases against the retained candidate.
python release/scripts/validate_animation_smoke.py --workspace "$(pwd)/Cargo.toml" --manifest "$(pwd)/release/targets.toml" --target "$architecture-unknown-linux-gnu" --tag "$release_tag" --audit "$native_audit" --archive "$native_archive" --packages "$package_directory" --container-log "$verification_directory/containers" --host-log "$verification_directory/host" --native-sandbox-receipt "$verification_directory/native-sandbox/native-sandbox-tests.json" # Seal the sandbox execution receipt and all ten native animation proofs into the existing package artifact set.
printf '%s\n' "$verification_directory" # Retain this directory with the candidate's source and receipt hashes.
```

Expected successful coverage is four inspection results, six container results
(three deb, two rpm, one AppImage), and four host results, each with its command's
passed summary and zero process exit status. Retain every JSONL result and complete
transaction log, the source commit and smoke/lifecycle source hashes, native
system/architecture, package and source-archive hashes, the bound package receipt
and native audit, actual installed-member checks, both version outputs, lifecycle
output, and removal/absence readbacks. The workflow uploads inspection, container
and host diagnostics even when a gate fails. Those logs stay outside the exact
five-packages-plus-receipt artifact inventory for each architecture.

For additional x86_64 VM acceptance, use the supplied QEMU helper from an
x86_64 Linux host with QEMU image/system tools, `cloud-localds` and SSH/SCP.
It defaults to KVM on hosts with hardware acceleration; `--accelerator tcg` runs
the same guest under QEMU software emulation when nested KVM is unavailable.
It verifies the Ubuntu 22.04 amd64 image digest, matching the declared x86_64
Linux release baseline, boots a disposable overlay and
runs the supported inspection subset plus every requested host format. Before
copying an input, it requires the guest destination to be absent, including dangling
symlinks. It then hashes every transferred source and package file inside the guest
and checks those hashes against the host manifest before invoking either smoke mode.
Retain `vm-inputs-manifest.json`, `vm-inputs-verification.jsonl` and the transfer logs
with the smoke results. A successful host-side transfer or source hash alone does
not prove which scripts and packages the guest executed. The helper retains the
foreground QEMU child through graceful shutdown and bounded reap. With the variables
above and a fresh `vm-work` path:

```bash
python release/packaging/linux/vm_smoke.py --packages "$package_directory" --arch x86_64 --work "$verification_directory/vm-work" --log "$verification_directory/vm-log" --formats deb,appimage,snap,flatpak | tee "$verification_directory/vm.jsonl" # Run only on an x86_64 KVM host and preserve the real VM result under pipefail.
```

The release workflow runs this guest gate for the x86_64 package row using QEMU
TCG, so hosted-runner nested-KVM availability is not assumed. The VM result and
guest transfer/smoke logs are retained in the Linux diagnostics artifact; the
large downloaded base image and copy-on-write overlay stay outside that artifact.
The supplied VM helper supports x86_64 only and does not run RPM host acceptance;
RPM remains covered in the native distribution containers. An ARM64 VM requires
its own native guest execution and retained evidence; x86_64 emulation or successful
cross-architecture inspection does not substitute for it.

Passing portable fixtures establishes the gate's rejection behavior. Passing these
native commands qualifies their exact supplied candidate bytes and environments;
it does not establish a current public release or public installation transport.
Current release publication and public installer acceptance remain separate gates.
Not covered: Debian/Ubuntu releases older than the glibc 2.35 payload baseline,
RHEL 9 and derivatives (glibc 2.34), distribution repositories, package signing,
Snap Store or Flathub publication. Flatpak uses its declared platform runtime;
its sandbox/runtime prerequisites remain separate from direct host-loader support.

## macOS packages

The `macos-packages` job runs on both manifest runners: Apple Silicon on
`macos-15` and Intel on `macos-15-intel`. It builds `ilium-macos-<arch>.zip`,
`.pkg` and `.dmg` from the audited native tarball. The builder preserves the
post-relocation, post-signing client, server, dylibs, version and notice bytes.
It does not rebuild them. ZIP and DMG contain one complete Ilium folder. The
PKG installs to `/usr/local/lib/ilium/<version>/<arch>`, requires administrator
approval and leaves PATH unchanged. The builder selects legacy PKG compression
so the publication host can inspect its payload without running macOS tools.

The containers are unsigned and not notarized. Valid ad-hoc executable seals
required by Apple Silicon do not establish Developer ID distribution signing.
The build and smoke receipts retain these separate trust states. Neither a
successful build nor a portable parser fixture establishes native acceptance.

### Native macOS acceptance

`build_macos_packages.py build` writes three packages and
`macos-packages-<arch>.json`. Its state stays `built-not-qualified` with
`publication_allowed=false`. `smoke_macos_packages.py` must then qualify all
three formats and write the separate `macos-smoke-<arch>.json` receipt. There
is no skip-format or success-override option.

For each format the smoke gate checks the installed payload against the exact
native audit, exercises both executable versions and creates, lists and stops a
real pane through the installed client/server. It checks the live server's
ancestry and the pane's nonce-bearing readiness marker. During execution it
hides the native artifact, package directory and retained build inputs, then
restores all three. The embedding gate holds the installed client alive, checks
its loaded dylib through `vmmap`, binds the binary, model and runtime hashes, and
requires a finite nonzero 384-dimensional result from the reviewed wrapper.

For DMG, the gate copies the complete mounted payload into an isolated directory
and detaches the source image before executing it. For PKG, it invokes Apple's
Installer on a fresh task-owned HFS+ volume. It checks package metadata and BOM,
installed root ownership and receipt files; removes the stopped payload; forgets
the receipt on that volume; and verifies the host's package receipt set stayed
unchanged. It detaches only images it can bind to its owned image files. Failed
process retirement, source restoration, receipt removal or image teardown fails
the gate and retains recovery evidence.

The workflow supplies a fresh build directory and a disjoint smoke root under
the runner's temporary directory. It uploads the five final files for each
architecture after native smoke passes, and uploads both build and smoke
diagnostics on failure as well as success. Local execution needs the same native
architecture, Apple packaging/image tools, Python 3.11+, a complete qualified
native artifact and noninteractive sudo for the isolated PKG transaction. Do
not run this gate against an existing user's installation or adopt their mounts.

### Aggregation and publication

Aggregation requires exactly ten macOS files: ZIP, PKG, DMG and both receipts
for each architecture. It reconstructs the native/source binding from the
qualified native artifacts, then checks the receipts against that binding and
the current acceptance sources. It rehashes every container, independently
parses ZIP and PKG payloads, and checks their members and installation metadata
against the native audit. DMG filesystem traversal needs Darwin; the aggregate
host binds the exact DMG hash to the native mount, source-detachment and installed
lifecycle evidence rather than substituting a portable parser result.

`candidate.json` retains the complete macOS asset map and native/source bindings.
Every later candidate read repeats these checks. Attestation covers the packages
and receipts; the shared draft, publication, public asset readback and recovery
paths use the same complete asset inventory. The stable names match the installation guide's (src/docs/installation.md)
six direct ZIP/PKG/DMG links. GitHub asset digests and `candidate.json` carry their
hashes; the five-entry `SHA256SUMS` remains the portable native-archive inventory
used by the public installers. These source contracts still require successful
native Actions execution and public download verification for the final release.

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

The Windows helper runs only on the native `windows-2025` runner. Its explicit
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
The installed animation gate runs while each audited payload is installed and
before removal. The installed `ilium release-animation-probe` subcommand uses
the normal process quota and the sibling helper to discover Beach and Carpet,
seed and render two accepted frames each, and physically retire each helper.
`release/scripts/smoke_installed_animation.py` binds its four JSONL records to
the installed client, helper, archives, native audit and exact source hashes.
Direct archive/install acceptance passes absolute `--workspace`, `--manifest`,
`--audit`, `--root` and new `--output` paths, plus `--os`, `--arch`, `--tag` and
`--format`. Snap, Flatpak and AppImage smoke invoke their real installed
launchers and record the observed executable root as well as the host payload
root. The macOS ZIP/PKG/DMG smoke retains one nested receipt per format;
Windows MSI/EXE smoke runs the probe inside the same private job; Linux host
and distribution-container smoke retain separate per-format receipts. Missing
frames, different bytes, failed sandbox launch or uncertain physical cleanup
block the downstream qualification and publication gates. An extracted host
tree cannot establish native Snap or Flatpak execution. Windows and macOS
currently fail closed until their native helper sandboxes are qualified by the
runtime owner; a helper `--version` result is insufficient.
After those native jobs pass, `validate_animation_smoke.py` seals their Windows
and Linux probe journals into internal `windows-animation-smoke.json` and
`linux-animation-smoke-<arch>.json` artifacts. Aggregate copies these under
`candidate/audits`, checks exact source, archive, installed package and audit
hashes, and reparses the probe records on every candidate read. Missing or
changed markers refuse publication. They are not added to the 36 public
release assets.
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

## First GitHub Packages publication

A newly published GHCR package defaults to private. Linking the package to the
public Ilium repository grants inherited access permissions, but does not make
the package publicly visible. See [GitHub container registry documentation](https://docs.github.com/en/packages/working-with-a-github-packages-registry/working-with-the-container-registry)
and [package visibility settings](https://docs.github.com/en/packages/learn-github-packages/configuring-a-packages-access-control-and-visibility).

After the first qualified upload creates `ilium-release`, the release operator
must use its GitHub Package settings to select Public visibility. Preserve the
uploaded version and the retained `ghcr-intent.json`; do not replace the version
or weaken anonymous verification to make the workflow green. Then rerun the
failed `ghcr-package` job on the same qualified tag. The publisher reconciles
existing layers against the release asset hashes before pulling by immutable
digest with an empty authentication configuration. Completion requires both
anonymous retrieval and public package metadata linked to `arthurwolf/ilium`.
A successful authenticated push alone is not public package delivery.

## Evidence and documentation

Keep source commit, runner, target, archive and binary hashes, native loader and
embedding output, install/PTY receipts, release readbacks and Pages readbacks.
Do not print secrets. Proposed, built, audited, uploaded, published and publicly
installed are distinct states. Advertise the commands in the installation guide (src/docs/installation.md) only after all
five native literal-command receipts pass. Preserve the existing demo gallery
and inference/privacy text when updating the installation guide (src/docs/installation.md).
