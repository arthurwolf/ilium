---
layout: page
title: Debugging a running plugin
permalink: /pages/reference-plugin-debugging/
---

## Debugging a running plugin

This page defines the target debug workflow for a plugin: a debug build that keeps symbols, attaching a debugger to a running plugin, and turning a trap backtrace from a release component back into source lines. Release builds strip DWARF and source maps ([DEV-303]), so this workflow is the only way to see source lines. The tools that show what a plugin did are in [Debugging tools](/pages/reference-debugging-tools/). This page describes target behaviour; it does not claim the behaviour exists today.

Requirement ids of this page are in the range `DEV-876` to `DEV-878`. The complete list is in [Requirements of the developer tooling lane](/pages/reference-requirements-dev-tooling/).

### Rules

- **[DEV-876]** `ilium plugin build` without `--release` MUST produce a debug component that keeps DWARF information and, for TypeScript, an inline source map, and MUST write `dist/<name>.debug.json` that records the `input_hash` of the build. A debug component MUST NOT be accepted by `pack` or `publish`; they fail with `package.debug-build`.

- **[DEV-877]** `ilium plugin debug <dir>` MUST start the plugin in a session and open a debugger endpoint on a loopback address with a random port. It MUST print the endpoint in an `artifact` record of kind `debug-endpoint`. It MUST refuse to bind a non-loopback address and fail with `debug.not-loopback`. The endpoint MUST close when the session ends.

- **[DEV-878]** `ilium plugin symbolize <trap.json> --debug-info <file>` MUST map each frame of a trap backtrace to a source file, line and function name when the debug information matches the `input_hash` in the trap record. When the hash differs it MUST fail with `debug.hash-mismatch` and print no guessed lines. A frame with no information MUST be printed as its function name only.

### Failure behaviour

Each rule names its failure code. A failure prints a `diagnostic` record in JSONL or one line on standard error in text, and the process exit code follows [Plugin command-line reference](/pages/reference-plugin-cli/#exit-codes).

## See also

- [Plugin command-line reference](/pages/reference-plugin-cli/)
- [Requirements of the developer tooling lane](/pages/reference-requirements-dev-tooling/)

API checked against `ilium:plugin@0.1.0`.
