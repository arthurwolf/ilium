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

### Harness version and host fidelity

`harness_version` names the code that stood in for the host during a run. Its value depends on the mode of the harness ([Test harness reference](/pages/reference-test-harness/), [DEV-400], [DEV-401]):

| Mode | `harness_version` is | Where the host behaviour comes from |
|---|---|---|
| Native | The version of the SDK `testing` module that the test binary or script linked, for example `sdk-testing 0.4.2`. | The fake host inside the SDK. It does not enforce deadlines or the memory ceiling ([DEV-401]). |
| Component | The version of the embedded test host of the installed `ilium` command line, for example `cli-host 0.4.2`. | The test host built into the CLI that runs `ilium plugin test`. |

The value is `<source> <version>`, where `<source>` is `sdk-testing` or `cli-host`. The harness prints it in the `summary` record and writes it to the `harness_version` field of the report file, so [DEV-888] compares like with like. Two runs with a different `harness_version` are not required to match.

Behaviour of the embedded host:

- The embedded host of CLI release X behaves as the host of release X for every limit of [Errors and limits](/pages/reference-errors-and-limits/), every error code and every renderer of the UI model. A test that passes on it proves the behaviour of release X and no other release.
- `--host-contract <version>` selects the embedded contract of an older host (proposed, owner to confirm). The CLI ships the contracts of the host contract versions it can still emulate. `harness_version` then reads `cli-host <cli version> (contract <version>)`, so the report records both the CLI that ran and the contract that it emulated.
- A version that the CLI does not embed fails before any case runs with `usage.host-contract-unavailable` and exit 2, as in [DEV-933]. The message lists the versions that the installed CLI embeds.
- `--host-contract` applies to component mode. In native mode the flag exits 2 with `usage.host-contract-unavailable`, because the SDK `testing` module carries exactly one contract and a native run cannot emulate another.

When the SDK `testing` version and the embedded host of the CLI disagree on the host contract:

| Difference | What the run does |
|---|---|
| Same contract major and minor | No message. |
| Same major, different minor | The run goes on. The harness prints the `warning` code `test.harness-mismatch` once, naming both versions, and the report lists it in `warnings`. Exit code rules are unchanged, so a warning alone does not change the exit code. |
| Different major | The run is refused before any case runs, with the error `test.harness-mismatch` and exit 1. The message names both versions and tells the author to align the SDK and the CLI. |

The harness compares the contract that the SDK `testing` module declares in its metadata ([DEV-996]) with the contract of the embedded host that runs the component. The comparison happens in component mode, because only there do both artifacts take part. A native run uses the SDK alone and has nothing to compare.

Consequences for CI:

- A green run proves the behaviour of the `harness_version` in its report and nothing more. To prove the behaviour of the host a user will meet, a CI pipeline pins the CLI version and the SDK version together and runs the oldest and newest contract the manifest `requires` range admits ([DEV-933]).
- A pipeline that wants no drift MAY treat `test.harness-mismatch` as an error by checking the `warnings` field of the report. The harness has no flag that turns the warning into a failure.
- Upgrading the SDK without the CLI, or the CLI without the SDK, is allowed inside one contract major. Across a major the run is refused, so a stale tool cannot report a pass against a host that it does not model.

The mapping from each SDK `testing` version to the host contract versions that it matches is part of the release metadata of the SDK ([DEV-996]) and is repeated in the compatibility table of [SDK deprecation and removal](/pages/reference-sdk-deprecation/). The matching table for the CLI lists the contracts each CLI release embeds. Both are published with the release, and the CLI prints its own with `ilium plugin test --host-contract list` (proposed, owner to confirm).

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
