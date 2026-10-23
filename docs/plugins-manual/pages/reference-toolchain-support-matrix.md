---
layout: page
title: Toolchain support matrix
permalink: /pages/reference-toolchain-support-matrix/
---

## Toolchain support matrix

This page defines which host operating systems and CPU architectures the plugin command line supports, and the minimum version of each tool that a build needs. It also defines the error for an unsupported host. This page describes target behaviour; it does not claim the behaviour exists today.

Requirement ids of this page are in the range `DEV-882` to `DEV-884`. The complete list is in [Requirements of the developer tooling lane](/pages/reference-requirements-dev-tooling/).

### Rules

- **[DEV-882]** The `ilium plugin` commands MUST run on Linux, macOS and Windows, each on x86-64 and aarch64. A plugin built on one host MUST contain the same component bytes as the same plugin built on any other supported host with the same toolchain versions.

- **[DEV-883]** The docs MUST list a minimum version for the Rust toolchain, Node.js (or Bun for TypeScript projects) and `wasm-tools`. `ilium plugin doctor` MUST report each installed version against its minimum and MUST fail with `toolchain.too-old`, naming the tool, the found version and the minimum, before any build starts.

- **[DEV-884]** On a host outside the matrix, a command that builds MUST fail with exit code 1 and `toolchain.host-unsupported`, naming the operating system, the architecture and the list of supported pairs. Commands that do not build (`check`, `inspect`) MAY run and MUST then print a `warning` with the same code.

### Failure behaviour

Each rule names its failure code. A failure prints a `diagnostic` record in JSONL or one line on standard error in text, and the process exit code follows [Plugin command-line reference](/pages/reference-plugin-cli/#exit-codes).

### Tested versions

- Each host minor release has one row in the support table of this page ([DEV-962]). [DEV-989] requires the newest tested version of each tool (Rust, Bun and `wasm-tools`) to be listed in this page, so the table names it for each minor.
- A toolchain newer than the newest tested version is not refused. `ilium plugin doctor` reports it as `untested` (warning, exit 0), and `ilium plugin build` still succeeds.
- A toolchain older than the minimum is refused with `toolchain.too-old` before any build starts, as [DEV-883] states.
- The minimum Rust version is 1.90 and the minimum Bun version is 1.2, as [Set up a toolchain](/pages/getting-started-toolchain/) states. This page does not yet give a minimum `wasm-tools` version.

### Raising a minimum and retiring an old toolchain

- A patch release never raises a minimum. A raise appears in the release notes of a minor release at least one minor before it takes effect ([DEV-988]).
- A raised minimum keeps the previous minimum working for at least two minor releases after the raise, as [DEV-897] requires for the SDK minimums. This page does not state a separate overlap for the host minimums.
- A toolchain below the raised minimum is refused with `toolchain.too-old` by `doctor` and by `build`, as [DEV-883] states.
- A host minor past its end-of-support date keeps loading plugins. It reports `support: "end-of-life"` and writes one `host.end-of-life` warning per server start ([DEV-965]).

### Changes in component bytes from a toolchain change

- [DEV-882] promises identical component bytes across hosts when the toolchain versions are the same. It does not promise identical bytes across toolchain versions. A new Rust or `wasm-tools` release that changes the bytes is therefore not a breach of this page, but the package hash changes with it.
- A build is reproducible only with pinned inputs ([DEV-893]). The recorded toolchain in `dist/build.json` must match the installed one. When it does not, `ilium plugin verify-reproducible` fails with `build.unpinned-input` before it builds. See [Reproducible builds](/pages/guide-reproducible-builds/).
- Projects that need a fixed toolchain set exact versions in `ilium-toolchain.json` ([DEV-990], owner to confirm the file name). A mismatch fails `ilium plugin build` with `toolchain.pin-mismatch`.
- Steps when a new toolchain is installed for a project:
  1. Run `ilium plugin doctor`. Note any `untested` warning.
  2. Run `ilium plugin verify-reproducible` on the project with the new toolchain.
  3. Update the recorded or pinned versions in the same change that rebuilds the component.

## See also

- [Plugin command-line reference](/pages/reference-plugin-cli/)
- [Requirements of the developer tooling lane](/pages/reference-requirements-dev-tooling/)

API checked against `ilium:plugin@0.1.0`.

## Production additions: T9-compat-deprecation

- [DEV-965] A host release past its end-of-support date MUST make `ilium version --json` report `support: "end-of-life"` and MUST write one `warn` line `host.end-of-life` per server start; it MUST keep loading plugins unchanged. Test: build a test host with a past end-of-support date, run `ilium version --json` and start the server, and assert the `support` field, one `warn` line, and that installed plugins still load.
- [DEV-988] Raising the minimum Rust, Bun or wasm-tools version MUST be announced in the release notes of a minor release at least one minor before it takes effect, and a patch release MUST NOT raise any minimum. Test: read the release notes of the minor release before a raise and assert the announcement; compare the minimum versions of two consecutive patch releases and assert that they are equal.
- [DEV-989] `ilium plugin doctor` MUST report a toolchain newer than the newest tested one as `untested` (warning, exit 0) and MUST NOT fail the build on it; the newest tested version of each tool MUST be listed in this support matrix. Test: install a toolchain one minor version newer than the newest tested one, run `ilium plugin doctor` and assert an `untested` warning with exit 0, then assert that the build succeeds.
- [DEV-990] A project MAY carry `ilium-toolchain.json` (proposed file name, owner to confirm) naming exact versions of Rust, Bun and wasm-tools; when present, `ilium plugin build` MUST fail with `toolchain.pin-mismatch` if an installed version differs, and the component MUST be reproducible under DEV-882 with that pin. Test: write a pin, install a different Rust version, run `ilium plugin build` and assert the `toolchain.pin-mismatch` failure; with matching versions, build on two hosts and compare the component bytes.
- [DEV-994] The support matrix of [DEV-962] is the table in this page, one row per host minor release. The host maintainer role (proposed, owner to confirm) sets the release date and the security-fix end date of each minor in the release record of its first release ([DEV-958]), before the release tag is created. The `end-of-life` state of [DEV-965], the release notes of that minor and the table row MUST show the same two dates. Test: for each minor, read the release record, the release notes, the table row and the `ilium version --json` output of a host built from that tag, and assert that the release date and the end date are equal in all four places.
- [DEV-999] A security-fix end date that is already published in the table MUST NOT be moved earlier. A later end date MUST be recorded in the same table row and named in the release notes of the next patch release of that minor (proposed, owner to confirm: the notice period is not set). Test: change a published end date to an earlier date in a copy of the table and assert that the release check fails; move one to a later date and assert that the row and the notes both show the new date.
