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
