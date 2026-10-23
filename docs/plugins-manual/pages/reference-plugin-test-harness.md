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

### Exit codes for a test run

This table covers `ilium plugin test` only. It applies the general codes of [Plugin command-line reference](/pages/reference-plugin-cli/#exit-codes) to a run. The `exit_code` field of the `result` record and the process exit status are the same value (proposed, owner to confirm).

| Code | When the run returns it |
|---|---|
| 0 | Every selected case is `pass` or `skipped`. A flaky case ([DEV-963]) still counts as `pass`, and the report lists it as flaky. |
| 1 | The run did not reach its end: a fixture cannot be read (`test.fixture-invalid`), the component is missing (`test.component-missing`), the run budget ran out (`test.run-timeout`, see below), or the report file cannot be written (`test.report-write-failed`). Any failure without a more specific code also returns 1. |
| 2 | A usage error before any work starts: an unknown flag, `--timeout-ms` or `--run-timeout-ms` out of range (`usage.out-of-range`), or a `--filter` that matches no fixture (`test.no-match`). |
| 3 | The run reached its end and at least one case is `fail` or `trap`. A timed-out case, a case that failed with a harness code such as `harness.real-resource`, and a quarantined failure all count here. |
| 130 | The user pressed Ctrl+C. The report is still written, with `complete` set to `false`. |

A failed case does not stop the run. Every selected case runs in sorted fixture-name order, so one report lists every failure of the run. The run ends early only for the run budget or an interrupt. A quarantined failure still returns 3; the pipeline excludes it by reading the `quarantined` flag of the report ([DEV-960]). A CI pipeline MUST NOT retry a release run ([DEV-956]).

### Report file for CI

`ilium plugin test --report <file>` writes one JSON document for the run (proposed, owner to confirm). The harness does not write JUnit XML. One schema serves every CI system, and JUnit has no fields for the seed, the harness version or the component hash that a rerun needs. A CI system that shows JUnit results converts this file in a step of its own.

- The top-level `schema` field is exactly `ilium-test-report/1`. A reader MUST refuse any other value. The number rises only when a field is removed or changes meaning, as for the conformance report of [DEV-923]. Adding a field keeps the number.
- Top-level fields: `schema`, `complete` (`false` after an interrupt or a run-budget stop), `exit_code`, `plugin` (`id` and `version`), `side`, `harness_version`, `seed`, `component_sha256` (of `dist/<side>.wasm`), `cases` and `summary`.
- Each entry of `cases` has `name` (stable between runs, [DEV-550]), `fixture`, `status` (`pass`, `fail`, `trap` or `skipped`), `duration_ms`, `attempts` (1 unless the case was retried), `flaky` ([DEV-963]), `quarantined` ([DEV-960]) and `reason` whenever the status is not `pass`.
- `summary` has `passed`, `failed`, `trapped`, `skipped`, `flaky`, `quarantined` and `duration_ms`.
- Cases appear in sorted fixture-name order ([DEV-919]). The file holds no absolute path, user name or wall-clock timestamp ([DEV-890]), so two runs with the same inputs differ only in `duration_ms`.
- The file is written atomically, through a temporary file and a rename as in [DEV-015], after the run ends. It is written after a run-budget stop and after an interrupt, and then `complete` says whether every selected case ran. Without `--report` no file is written.

### Run limits and parallel runs

Limits on each case:

- A case runs for at most `--timeout-ms` of real time. The default is 10,000 ms and the maximum is 600,000 ms. The `[test] timeout_ms` key of `ilium-plugin.toml` sets the project default, and the flag wins ([DEV-920], [DEV-903]). A case over its timeout gets status `fail` with reason `timeout`.
- The deadlines of each plugin call, such as the 2,000 ms of `activate` in [DEV-423], still apply inside a case. See [Errors and limits](/pages/reference-errors-and-limits/).

Limits on the run:

- `--run-timeout-ms <n>` sets the wall-clock budget of the whole `test` command (proposed, owner to confirm). The default is 600,000 ms and the maximum is 3,600,000 ms. A larger value exits 2 with `usage.out-of-range`.
- When the budget runs out, the running case is stopped and fails with reason `timeout`. Each case not yet started gets status `skipped` with reason `run-timeout`. The report is written with `complete` set to `false`, and the command exits 1 with `test.run-timeout`.
- The CI job timeout MUST be longer than the run budget. The harness then ends the run and writes the report, and the runner does not kill it first. See [CI recipes](/pages/guide-ci-recipes/).

Parallel runs:

- `ilium plugin test` runs its cases one at a time, in sorted order (proposed, owner to confirm).
- A CI pipeline may run several `ilium plugin test` processes at once, for example one per `--filter` set or one per project. Each process holds its own fake state ([DEV-428]), so the processes do not affect each other. Each process needs its own `--report` file.
- Two processes MUST NOT run with `--update` on the same project, because both would rewrite the same golden files. CI never uses `--update` ([DEV-503]).
- A CI host SHOULD run at most one harness process per CPU thread (proposed, owner to confirm). Extra processes slow every case, so a healthy plugin can fail with reason `timeout`.

## See also

- [Plugin command-line reference](/pages/reference-plugin-cli/)
- [Requirements of the developer tooling lane](/pages/reference-requirements-dev-tooling/)

API checked against `ilium:plugin@0.1.0`.
