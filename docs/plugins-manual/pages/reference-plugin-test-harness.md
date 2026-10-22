---
layout: page
title: Test harness contract for CI
permalink: /pages/reference-plugin-test-harness/
---

## Test harness contract for CI

This page defines what an author may rely on when the plugin test harness runs in CI: determinism, the fake clock, fake capability grants and the output of failures. The harness API is in [Test harness reference](/pages/reference-test-harness/). This page describes target behaviour; it does not claim the behaviour exists today.

Requirement ids of this page are in the range `DEV-888` to `DEV-890`. The complete list is in [Requirements of the developer tooling lane](/pages/reference-requirements-dev-tooling/).

### Rules

- **[DEV-888]** A test run MUST be deterministic: with the same component, fixtures, seed and harness version, two runs on any supported host MUST give the same pass and fail set and byte-identical failure output. The harness MUST print its seed in the `summary` record and accept it again with `--seed`.

- **[DEV-889]** The harness MUST give a test only the capabilities that the test grants explicitly through the fake host. A host call that needs a capability the test did not grant MUST fail with `capability.denied` exactly as in a session, and the failing test MUST name the capability. Real network, real file system outside the test directory and the real clock MUST be unreachable and MUST fail with `harness.real-resource`.

- **[DEV-890]** A failed test MUST print the test name, the first differing line of the expected and actual output with at most 40 lines of context, and the host calls made in order with their fake times. The output MUST NOT contain absolute paths, user names or wall-clock times, so that CI logs from different machines compare equal.

### Failure behaviour

Each rule names its failure code. A failure prints a `diagnostic` record in JSONL or one line on standard error in text, and the process exit code follows [Plugin command-line reference](/pages/reference-plugin-cli/#exit-codes).

## See also

- [Plugin command-line reference](/pages/reference-plugin-cli/)
- [Requirements of the developer tooling lane](/pages/reference-requirements-dev-tooling/)

API checked against `ilium:plugin@0.1.0`.
