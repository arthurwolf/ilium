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

## See also

- [Plugin command-line reference](/pages/reference-plugin-cli/)
- [Requirements of the developer tooling lane](/pages/reference-requirements-dev-tooling/)

API checked against `ilium:plugin@0.1.0`.

## Production additions: T9-compat-deprecation

- [DEV-965] A host release past its end-of-support date MUST make `ilium version --json` report `support: "end-of-life"` and MUST write one `warn` line `host.end-of-life` per server start; it MUST keep loading plugins unchanged. Test: build a test host with a past end-of-support date, run `ilium version --json` and start the server, and assert the `support` field, one `warn` line, and that installed plugins still load.
- [DEV-988] Raising the minimum Rust, Bun or wasm-tools version MUST be announced in the release notes of a minor release at least one minor before it takes effect, and a patch release MUST NOT raise any minimum. Test: read the release notes of the minor release before a raise and assert the announcement; compare the minimum versions of two consecutive patch releases and assert that they are equal.
- [DEV-989] `ilium plugin doctor` MUST report a toolchain newer than the newest tested one as `untested` (warning, exit 0) and MUST NOT fail the build on it; the newest tested version of each tool MUST be listed in this support matrix. Test: install a toolchain one minor version newer than the newest tested one, run `ilium plugin doctor` and assert an `untested` warning with exit 0, then assert that the build succeeds.
- [DEV-990] A project MAY carry `ilium-toolchain.json` (proposed file name, owner to confirm) naming exact versions of Rust, Bun and wasm-tools; when present, `ilium plugin build` MUST fail with `toolchain.pin-mismatch` if an installed version differs, and the component MUST be reproducible under DEV-882 with that pin. Test: write a pin, install a different Rust version, run `ilium plugin build` and assert the `toolchain.pin-mismatch` failure; with matching versions, build on two hosts and compare the component bytes.
