---
layout: page
title: Conformance suite
permalink: /pages/reference-conformance-suite/
---

## Conformance suite

The conformance suite is a fixed list of cases that check one claim each about the contract `ilium:plugin@0.1.0`. It has two uses. A plugin author runs it to learn that a built plugin follows the contract. A maintainer of Ilium or of an SDK runs it to learn that a host or an SDK still follows the contract after a change.

Terms:

- **Case**: one check with an id, a setup, a stimulus and an expected result.
- **Subject**: the thing under test. It is a plugin component, an SDK, or the host.
- **Verdict**: `pass`, `fail` or `skip` for one case.

## You will need

- A built component in `dist/` for plugin subjects. See [Plugin command-line reference](/pages/reference-plugin-cli/).
- The harness from [Test harness reference](/pages/reference-test-harness/). The suite runs on `FakeHost` in component mode.

## Running the suite

`ilium plugin test --conformance [--subject <plugin|sdk|host>] [--case <id>]`

| Subject | Needs | What it checks |
|---|---|---|
| `plugin` (default) | `dist/*.wasm` | That the component behaves within the contract limits |
| `sdk` | The SDK source tree | That the SDK maps every contract rule to its API |
| `host` | A running session | That the host enforces every contract rule |

- **[DEV-600]** `--subject plugin` MUST run only the cases marked `plugin` in the catalogue below. It MUST skip cases that need a grant the plugin does not request, with the reason `grant-not-requested`.
- **[DEV-601]** `--subject sdk` and `--subject host` MUST be refused with exit 2 and `usage.subject-unavailable` in a normal install. They are available only in the Ilium source tree, where the maintainer enables them with `ILIUM_CONFORMANCE_MAINTAINER=1`.
- **[DEV-602]** The suite MUST print one `conformance` record per case, then a `summary` record. A `fail` verdict makes the exit code 3. A suite with only `pass` and `skip` exits 0.
- **[DEV-603]** `--case <id>` MUST run one case. An unknown id MUST fail with exit 2 and `usage.invalid-value`.
- **[DEV-604]** The case ids MUST be stable. A case MUST NOT change meaning. A changed rule gets a new case id, and the old id is retired in the retired list at the end of this page.
- **[DEV-605]** Every case MUST be deterministic. It MUST use the fake clock and the default seed.
- **[DEV-606]** One case MUST finish within 10,000 ms of wall time. A longer case is a `fail` with the message `case exceeded 10000 ms`.
- **[DEV-607]** Durations in case expectations are fake-clock milliseconds, except the call time measured in epoch-based wall-clock milliseconds (fuel is off and never enforces or measures deadlines, per HST-058 and LIF-127). A case that needs a deadline breach MUST use the built-in probe plugin `conformance-probe.wasm` with its busy-wait import, never the plugin under test.

A record:

```json
{"type":"conformance","case":"CONF-LIFE-02","title":"activate stays under its deadline","subject":"plugin","verdict":"pass","duration_ms":41}
{"type":"summary","passed":31,"failed":0,"skipped":5,"duration_ms":18420}
```

## Case catalogue

Case ids have the form `CONF-<AREA>-<nn>`. The tables list the id, the claim, the stimulus and the expected result. The subject column says who can run the case: `P` plugin, `S` SDK, `H` host.

### Lifecycle

| Id | Claim | Stimulus | Expected | Subject |
|---|---|---|---|---|
| CONF-LIFE-01 | `activate` runs once before any event | Activate, then deliver one event | `activate` call precedes `on-events` | P S H |
| CONF-LIFE-02 | `activate` stays under its deadline | Subject P: activate the subject plugin and measure the call time. Subject H: the probe plugin busy-waits for 2,100 ms in `activate`. | P: call time under the effective deadline. H: `trap.deadline`. | P H |
| CONF-LIFE-03 | `deactivate` over 500 ms is cut | The probe plugin busy-waits for 600 ms in `deactivate` | Instance dropped, restore bytes discarded, not counted as a failure (`failures()` unchanged) | H |
| CONF-LIFE-04 | An error from `activate` fails the start | `activate` returns an error | Plugin not active, error code logged | P S H |
| CONF-LIFE-05 | Deactivate reason is one of the five delivered reasons | Deactivate with each of `shutdown`, `disabled`, `upgrade`, `revoked` and `granted` | The plugin sees that reason. `crash-restart` is reserved and never delivered. | S H |

### Events and submissions

| Id | Claim | Stimulus | Expected | Subject |
|---|---|---|---|---|
| CONF-EVT-01 | A batch holds at most 256 events | Queue 600 events | Batches of at most 256 | H |
| CONF-EVT-02 | A batch of events stays under its deadline | Subject P: deliver `sample_batch(256)` and measure the call time. Subject H: the probe plugin busy-waits for 25 ms in `on-events`. | P: call time under 20 ms. H: `trap.deadline`. | P H |
| CONF-EVT-03 | Coalesced events keep the newest state | Send three renames for one node | One event with the last name | H |
| CONF-EVT-04 | `events-dropped` follows a full queue | Fill the queue past 1,024 | Next batch starts with `events-dropped` | H |
| CONF-EVT-05 | Ungranted events are not delivered | Remove the grant, send the event | No delivery | P H |
| CONF-EVT-06 | More than 256 submissions trap | Return 256 submissions (passes), then 257 submissions | 256: no trap; 257: `trap.submission-batch` | P S H |
| CONF-EVT-07 | Batch refusal cancels the rest | Third submission is invalid | Others `refused(cancelled)` with `batch refused: submission 2` | S H |
| CONF-EVT-08 | Dedup key repeats are duplicates | Same key twice in 10 minutes | Second is `duplicate` | S H |
| CONF-EVT-09 | A dedup key outside the charset is refused | Key with a space | `refused(plugin-error{code: invalid-argument, message starting "dedup-key-invalid:"})` | S H |
| CONF-EVT-10 | Outcomes arrive as `request-completed` | Submit one intent | One event with the same correlation | P S H |

### Grants

| Id | Claim | Stimulus | Expected | Subject |
|---|---|---|---|---|
| CONF-GRANT-01 | An import whose grant the user declined is refused | The manifest requests `fs.read:project`, the test declines it, the plugin calls `fs.read` | `Err(not-permitted)` | P S H |
| CONF-GRANT-02 | Revocation takes effect at once | Revoke, then call | `Err(not-permitted)` with reason `revoked` for an import made after the revocation, `Err(revoked)` only for a call already in flight (HST-075), `refused(plugin-error{code: not-permitted})` for an intent | H |
| CONF-GRANT-03 | Grant arguments are matched exactly | Grant `fs.read:project`, read `FsPath{scope: "data"}` | `Err(not-permitted)` | H |
| CONF-GRANT-04 | A grant absent from the manifest cannot be gained | `host.grant_now` of a grant not in the manifest | `harness.grant-not-declared` | H |
| CONF-GRANT-05 | A function of a capability never requested traps | Call a function of a capability the manifest never requested | `host.capability-not-requested` | H |

### Views

| Id | Claim | Stimulus | Expected | Subject |
|---|---|---|---|---|
| CONF-VIEW-01 | A render stays under its deadline | Subject P: render each declared view at 80x24 and measure the call time. Subject H: the probe plugin busy-waits for 15 ms in `render`. | P: call time under 10 ms. H: last good render stays, `trap.deadline` counted. | P H |
| CONF-VIEW-02 | A view is pure | Render twice | Equal screens | P S |
| CONF-VIEW-03 | Node limits are enforced | Return a tree of 4,097 nodes, then one of 4,096 | `trap.invalid-ui-tree` at 4,097 nodes; 4,096 renders | H |
| CONF-VIEW-04 | An action with an unknown key is dropped | Click an action key the view never set | Not delivered | H |

### Storage, memory and state

| Id | Claim | Stimulus | Expected | Subject |
|---|---|---|---|---|
| CONF-STORE-01 | Storage survives a restart | Put, trap, restart, get | Value present | H |
| CONF-STORE-02 | A value over 1 MiB is refused | Put a value of 1,048,576 bytes (passes), then 1,048,577 bytes | 1,048,576: `ok`; 1,048,577: `Err(invalid-argument)` | S H |
| CONF-STORE-03 | A bucket over 16 MiB is refused | Fill a bucket to exactly 16 MiB (passes), then add 1 byte | 16 MiB: `ok`; one more byte: `Err(quota-exceeded)` | S H |
| CONF-MEM-01 | Memory over `memory_mib` traps | The probe plugin stays just under the default 32 MiB (passes), then allocates 40 MiB | under: no trap; 40 MiB: `trap.memory` (see the trap code table of [Errors and limits](/pages/reference-errors-and-limits/#trap-codes)) | H |
| CONF-MEM-02 | The ceiling is 128 MiB for T2 and T3 | Request 256 in the manifest | Install succeeds; the value is clamped to 128 MiB and the clamp is logged | H |

### Restart rule

| Id | Claim | Stimulus | Expected | Subject |
|---|---|---|---|---|
| CONF-RESTART-01 | First failure restarts after 1 s | Trap once | Restart at 1 s | H |
| CONF-RESTART-02 | Second failure restarts after 5 s | Trap twice in 10 minutes | Restart at 5 s | H |
| CONF-RESTART-03 | Third failure in 10 minutes disables | Trap three times | Plugin disabled | H |
| CONF-RESTART-04 | Old failures expire | Traps at 0, 6 and 11 minutes of fake time | Restarts after 1 s, 5 s and 5 s; never disabled | H |
| CONF-RESTART-05 | A lone old failure does not count | Traps at 0 and 11 minutes of fake time | Both restart after 1 s | H |

### Package and manifest

| Id | Claim | Stimulus | Expected | Subject |
|---|---|---|---|---|
| CONF-PKG-01 | A package is deterministic | Pack twice from one build | Equal bytes | P |
| CONF-PKG-02 | The api-version section matches the manifest | Read the custom section | `0.1.0` as three u16 big-endian | P |
| CONF-PKG-03 | A hash mismatch is refused at install | Change one byte of a file | `manifest.file-hash` | H |
| CONF-PKG-04 | An unsigned package installs only as T3 | Install without a signature | Tier T3 with the dev badge | H |
| CONF-PKG-05 | A manifest with an unknown key is refused | Add `colour` at top level | `manifest.unknown-key` | H |

- **[DEV-610]** The catalogue above MUST be the minimum. A release of the suite MAY add cases and MUST NOT remove a case without a retirement note.
- **[DEV-611]** Cases for the restart rule MUST use fake time. No case waits for real seconds.
- **[DEV-612]** A case for a limit MUST test the value just under the limit (pass) and the value just over (fail), unless the table says otherwise.
- **[DEV-613]** A `skip` MUST carry a reason: `grant-not-requested`, `side-not-built`, `subject-mismatch`, `needs-session` or `needs-component`.

## Writing a plugin-specific conformance case

A plugin author can add cases to the project. A case is a test function that uses `conformance::case`.

```rust
use ilium_plugin_sdk::testing::{conformance, FakeHost};

#[test]
fn event_budget_holds() {
    conformance::case("PROJ-EVT-01", "handler stays under 20 ms", |host: &mut FakeHost| {
        host.activate::<Plugin>();
        host.deliver(conformance::sample_batch(256));
        conformance::assert_within_budget(host, "event", 20);
    });
}
```

```ts
import { test } from 'bun:test';
import { conformance, type FakeHost } from '@ilium/plugin-sdk/testing';

test('event budget holds', async () => {
  await conformance.case('PROJ-EVT-01', 'handler stays under 20 ms', async (host: FakeHost) => {
    await host.activateModule('../src/server');
    host.deliver(conformance.sampleBatch(256));
    conformance.assertWithinBudget(host, 'event', 20);
  });
});
```

`conformance::case(id: &str, title: &str, body: impl FnOnce(&mut FakeHost))` in Rust and `conformance.case(id, title, async (host: FakeHost) => ...)` in TypeScript build the host the same way: `FakeHost::for_manifest("manifest.json")` in component mode with the default seed. The case receives the host as its argument and never builds one.

- **[DEV-624]** `conformance::case` and `conformance.case` MUST pass a `FakeHost` for `manifest.json` in component mode with the default seed to the body. The body MUST NOT build its own host.
- **[DEV-620]** A project case id MUST start with `PROJ-` so it can never clash with a suite case id.
- **[DEV-621]** `sample_batch(n)` MUST return `n` valid events for the grants of the host, using the fuzz generator with the host seed.
- **[DEV-622]** `assert_within_budget(host, kind, millis)` MUST compare the recorded time of the last call of that kind with `millis`. `kind` is one of `'event'`, `'render'`, `'activate'` and `'deactivate'`, and `millis` is an integer in milliseconds (Rust `u32`, TypeScript `number`). It MUST be skipped in native mode with the reason `needs-component`.

## Record fields

| Field | Type | Meaning |
|---|---|---|
| `type` | string | Always `conformance` for a case record |
| `case` | string | Case id |
| `title` | string | The claim, as in the catalogue |
| `subject` | string | `plugin`, `sdk` or `host` |
| `verdict` | string | `pass`, `fail` or `skip` |
| `reason` | string | For `skip`, one of the reasons in DEV-613. For `fail`, the message. |
| `duration_ms` | integer | Time the case took |

- **[DEV-640]** A `fail` record MUST carry `expected` and `actual` fields that describe the difference, as strings of at most 200 characters.
- **[DEV-641]** The `summary` record MUST list `passed`, `failed`, `skipped` and `duration_ms`. `passed` plus `failed` plus `skipped` MUST equal the number of case records.
- **[DEV-642]** Case order in the output MUST be the order of the catalogue on this page, area by area.

## Worked example: one case end to end

Case `CONF-EVT-06` checks that 257 submissions trap. The suite builds a probe batch of 257 submissions, delivers it to the component and expects `trap.submission-batch`.

```json
{"type":"conformance","case":"CONF-EVT-06","title":"more than 256 submissions trap","subject":"plugin","verdict":"pass","duration_ms":41}
```

For a plugin that returns at most 10 submissions per batch, the case drives the probe plugin, because the plugin itself never returns 257. The case therefore checks the host guard, and a plugin result `skip` with the reason `subject-mismatch` applies when the plugin world has no way to run the probe.

- **[DEV-643]** A case that cannot drive the plugin to the claimed state MUST be a `skip` with the reason `subject-mismatch`, never a `pass`.

## The edge of the suite

The suite checks the contract. It does not check:

- the look of a view on a real terminal;
- the speed on a given machine, except through the budgets above;
- the behaviour of a core plugin, which has its own tests.

## Using the suite in CI

Add `ilium plugin test --conformance --json` to the pipeline. A fail verdict exits 3. See [CI recipes](/pages/guide-ci-recipes/).

## Retired case ids

None. The first release of the suite is ids as listed.

## Edge cases

- A plugin with only a scene side skips all event and grant cases that need a server or client side, with `side-not-built`.
- A debug build runs slower than a release build and can fail a budget case. Use the release build for conformance. `--conformance` MUST refuse a debug build with exit 2 and `usage.release-build-required`, unless `--allow-debug` is given.
- **[DEV-630]** `--conformance` MUST run in component mode only. It MUST NOT run in native mode, because the limits are not enforced there.

## See also

- [Test harness reference](/pages/reference-test-harness/)
- [Errors and limits](/pages/reference-errors-and-limits/)
- [Lifecycle](/pages/lifecycle/)
- [Debugging tools](/pages/reference-debugging-tools/)

API checked against `ilium:plugin@0.1.0`.

## Production additions: T5-devtools

- **[DEV-923]** `conformance` run with `--report <file>` MUST write a JSON document `ilium-conformance/1` containing the suite version, the CLI version, the plugin id and version, the manifest SHA-256, and every case record. The file MUST be byte-identical across two runs on the same inputs, and a registry MAY require it at publish.
- **[DEV-924]** The suite MUST carry a version `MAJOR.MINOR`. `--suite-version <v>` MUST run exactly the catalogue of that version if the CLI still embeds it, else fail with `usage.suite-unavailable`. A MAJOR increase MAY remove or change a case and a MINOR increase MUST only add cases (DEV-610).
- **[DEV-925]** The suite MUST ignore the user's home directory, the process environment other than the CLI variables, and the system time zone and locale: all cases MUST run with TZ `UTC`, locale `C` and an empty environment for the plugin, so a case result MUST be the same on any machine.
