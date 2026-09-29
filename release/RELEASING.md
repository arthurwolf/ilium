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
