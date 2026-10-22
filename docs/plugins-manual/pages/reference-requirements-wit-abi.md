---
layout: page
title: Requirements for WIT and ABI
permalink: /pages/reference-requirements-wit-abi/
---

## Requirements for WIT and ABI

This page lists every normative rule of the WIT contract and the binary interface of `ilium:plugin@0.1.0`. Each row has an id, the rule, the page that explains it, and the test that proves it. The ids appear inline as [ABI-NNN] on the pages that state the rule. The contract is in the [WIT reference](/pages/reference-wit/).

### What you need

- **The WIT reference.** [WIT reference](/pages/reference-wit/) is the owner of all type definitions.
- **The conformance matrices.** [Host conformance matrix](/pages/reference-host-conformance/) lists the members that rule ABI-030 covers.

## Rules

| Id | Rule | Owner page | Test |
|---|---|---|---|
| ABI-001 | The WIT reference is the only canonical definition of the contract. Other pages MUST hold excerpts only. | [reference-wit](/pages/reference-wit/) | check.py excerpt check |
| ABI-002 | Case order and case counts of event (56), intent (42), outcome (7), widget (51), action-kind (17), action-value (12), error-code (15) and deactivation-reason (6, of which five are delivered) MUST match the WIT reference. | [reference-wit](/pages/reference-wit/) | case-count diff against wit/since_v0_1 |
| ABI-003 | All seven WIT packages MUST carry the one version 0.1.0 and change together. | [reference-abi-evolution](/pages/reference-abi-evolution/) | package version scan |
| ABI-004 | Each component MUST carry the custom section ilium:api-version with bytes 00 00 00 01 00 00. | [reference-abi-evolution](/pages/reference-abi-evolution/) | inspect a built component |
| ABI-005 | A WIT excerpt on another page MUST match the declared names and cases in the WIT reference. | [reference-wit](/pages/reference-wit/) | check.py excerpt check |
| ABI-006 | The optional exports a component has MUST equal the optional exports its manifest lists. | [reference-manifest](/pages/reference-manifest/) | install a component with an extra export |
| ABI-007 | The manifest exports list MUST NOT differ from the component. A difference MUST refuse install with manifest.exports-mismatch. | [reference-host-conformance](/pages/reference-host-conformance/) | install with a missing optional export |
| ABI-008 | A plugin that returns more than 256 submissions MUST trap the call; the host MUST apply none and none completes. | [reference-runtime-model](/pages/reference-runtime-model/) | return 257 submissions |
| ABI-009 | The lowest-index offender of a batch-level check MUST complete as refused(invalid-argument) (shape) or refused(not-permitted) (grant), and every other submission MUST complete as refused(cancelled) with the message 'batch refused: submission <index>', where index is 0-based. | [requests-and-outcomes](/pages/requests-and-outcomes/) | batch of 3 with 1 bad |
| ABI-010 | A dedup key MUST be 1 to 128 characters from [A-Za-z0-9._:/-] and is mandatory for send-input and agent-action. | [events-and-intents](/pages/events-and-intents/) | repeat a key twice |
| ABI-011 | Correlation numbers below 2^63 MUST belong to plugins. Numbers from 2^63 up belong to the SDK. | [reference-runtime-model](/pages/reference-runtime-model/) | submit with a high number |
| ABI-012 | The host MUST hold a finished slow-call result for 30 seconds. | [requests-and-outcomes](/pages/requests-and-outcomes/) | take after 31 s |
| ABI-013 | The host MUST allow 64 pending requests by default per plugin (ceiling 256 for T1) and 4,096 per session. Over the limit MUST return busy. | [reference-errors-and-limits](/pages/reference-errors-and-limits/) | start 65 requests |
| ABI-014 | A take before completion, a second take, or a take of an unknown id MUST return none. The SDK wrapper returns not-found. | [requests-and-outcomes](/pages/requests-and-outcomes/) | take twice |
| ABI-015 | Each event case MUST follow its delivery policy: coalesced, drop-oldest-counted or never-dropped. Loss of a drop-oldest event MUST be reported in events-dropped. | [reference-host-conformance](/pages/reference-host-conformance/) | fill a queue of 1,024 |
| ABI-016 | Names MUST use kebab-case in WIT. JSON inside strings MUST be UTF-8. | [reference-wit](/pages/reference-wit/) | scan |
| ABI-017 | A string that is not valid UTF-8 MUST trap the call. | [reference-runtime-model](/pages/reference-runtime-model/) | pass bad bytes |
| ABI-018 | Required exports (lifecycle, handler, renderer) MUST NOT be listed in the manifest exports. | [reference-manifest](/pages/reference-manifest/) | list handler in a manifest |
| ABI-019 | Cases of a variant, enum or flags MUST only be appended after the freeze. Records MUST NOT gain fields. | [reference-abi-evolution](/pages/reference-abi-evolution/) | case-order diff |
| ABI-020 | The Rust and TypeScript SDKs MUST map WIT names by the rules in the WIT reference section 10. | [reference-wit](/pages/reference-wit/) | SDK binding test |
| ABI-021 | The host MUST compare api versions by minor window and MUST report one refusal code per package, the first failing check. | [reference-abi-evolution](/pages/reference-abi-evolution/) | install a 0.3.0 package |
| ABI-022 | Version 0.1.0 MUST have exactly four unstable features: ui-calendar, plugin-services, gpu-kernels and websocket. | [reference-abi-evolution](/pages/reference-abi-evolution/) | list features |
| ABI-023 | A requested grant not in the API MUST refuse with manifest.grant-unknown. An unrequested import MUST refuse with manifest.import-not-requested. A call of an unrequested capability MUST trap host.capability-not-requested. | [reference-host-conformance](/pages/reference-host-conformance/) | three installs |
| ABI-024 | The host MUST NOT link wasi:sockets or wasi:http. | [security](/pages/security/) | import wasi:sockets |
| ABI-025 | Each plugin side MUST run one single-threaded instance, and a host call MUST NOT re-enter a plugin export. | [reference-runtime-model](/pages/reference-runtime-model/) | call an export from an import |
| ABI-026 | The host MUST interrupt a running export by an epoch ticker of 10 ms against the export deadline. | [reference-runtime-model](/pages/reference-runtime-model/) | spin in on-events |
| ABI-027 | A memory growth over the instance limit MUST trap. | [reference-runtime-model](/pages/reference-runtime-model/) | grow memory |
| ABI-028 | The deactivation reason crash-restart is reserved. The host MUST NOT deliver it. | [reference-abi-evolution](/pages/reference-abi-evolution/) | crash a plugin |
| ABI-029 | The host MUST NOT deliver an event, outcome or reason case to a plugin whose API predates the case. | [reference-abi-evolution](/pages/reference-abi-evolution/) | run a plugin built against 0.1.0 on a host that adds a case after it |
| ABI-030 | A host test suite MUST have a test for every row of the import, export, event, intent and widget matrices. | [reference-host-conformance](/pages/reference-host-conformance/) | row count check |
| ABI-031 | The kernel MUST run two batch-level checks over the whole submission list in index order before any effect: shape first, then grant. Inside one submission shape wins over grant. | [events-and-intents](/pages/events-and-intents/) | batch with a missing grant at index 1 and a bad shape at index 2 -> index 2 is invalid-argument only if checked first; the shape check runs for the whole list before the grant check, so index 2 is the offender and the others are cancelled |
| ABI-032 | Every check other than shape and grant (target, readiness, value validity, rate, quota, conflict, if-current) MUST be per submission and MUST affect only that submission. | [events-and-intents](/pages/events-and-intents/) | batch of 3 with a stale revision at index 1 -> only index 1 is refused |
| ABI-033 | A full queue MUST admit a drop-oldest event by evicting the oldest queued drop-oldest event of any kind and counting it. | [reference-runtime-model](/pages/reference-runtime-model/) | fill a queue of 1,024 with output-matched, send 1,500 -> 477 dropped counted |
| ABI-034 | A dedup key MUST be reserved while now_wall_ms - accepted_wall_ms < 600,000, measured on the wall clock. The table is in memory and a server restart clears it, except that from Phase 5 the keys of `send-input` and `agent-action` are persisted in the journal and survive. | [requests-and-outcomes](/pages/requests-and-outcomes/) | repeat a key at 599,999 ms (duplicate) and at 600,000 ms (accepted); restart the server between (only `send-input` and `agent-action` keys, from Phase 5, stay reserved) |
| ABI-035 | A repeated dedup key MUST complete as duplicate naming the ref of the first submission. The host MUST NOT check correlation uniqueness. | [requests-and-outcomes](/pages/requests-and-outcomes/) | submit a key twice with the same correlation -> two completions |
| ABI-036 | events-dropped MUST be coalesced by (event, source) and MUST never be dropped. | [reference-runtime-model](/pages/reference-runtime-model/) | drop two kinds twice -> one events-dropped event with two records |
| ABI-037 | The hold of a slow-call result MUST start when on-events returns for the batch that delivered the request-completed event. | [requests-and-outcomes](/pages/requests-and-outcomes/) | delay the batch by 40 s -> take still succeeds |
| ABI-038 | After a trap, surviving events MUST be redelivered once at the head of the new queue, completions of submissions MUST be delivered to the new instance, and completions of call(id) requests MUST be discarded. | [reference-runtime-model](/pages/reference-runtime-model/) | trap with a pending submission and a pending http call |
| ABI-039 | A coalesced event MUST NOT be reported in events-dropped when its queued event is merged. | [reference-events-and-intents](/pages/reference-events-and-intents/) | send 2,000 screen-changed -> no events-dropped |
| ABI-040 | A request-id MUST be unique per plugin side for the whole server run, start at 1 at each server start, and never be reused across plugin-instance restarts within a server run. A server restart begins a new run. | [requests-and-outcomes](/pages/requests-and-outcomes/) | restart a plugin instance -> new ids exceed the old ones |
| ABI-041 | A request MUST count toward pending_requests from the return of its id to the enqueue of its completion; refused submissions and synchronous calls MUST NOT count. | [reference-errors-and-limits](/pages/reference-errors-and-limits/) | 64 slow calls then a refused submission -> count stays 64 |
| ABI-042 | The instance deadline of a call with deadline d ms MUST be ceil(d / 10) + 1 ticks, so a spinning guest traps in [d, d + 20] ms and a call that returns at d - 1 ms never traps. | [reference-runtime-model](/pages/reference-runtime-model/) | spin with d = 20 -> trap in [20, 40] ms |
| ABI-043 | On revocation, synchronous calls needing the grant MUST return err(revoked) (reads with no error channel, such as the `tree.read` functions, instead return `none` or an empty list), pending requests needing it MUST complete failed(revoked) before deactivate(revoked), and other pending call(id) requests MUST be cancelled with the instance. | [reference-runtime-model](/pages/reference-runtime-model/) | revoke http with one http and one fs request pending |
| ABI-044 | A deactivate breach or trap MUST drop the instance and its bytes and MUST NOT count as a failure. Returned bytes over 1 MiB MUST be dropped with one warn line and MUST NOT count as a failure. | [reference-runtime-model](/pages/reference-runtime-model/) | breach deactivate three times -> plugin still restarts |
| ABI-045 | A batch MUST hold at most 256 events, and event_queue MUST default to 1,024 with a ceiling of 4,096 for T1 and 1,024 for T2 and T3. | [reference-runtime-model](/pages/reference-runtime-model/) | deliver 600 queued events -> 3 batches |
| ABI-046 | Events produced while a plugin activates MUST be queued and delivered in the first batch after activate returns ok, and MUST be discarded without events-dropped if activate fails. | [reference-events-and-intents](/pages/reference-events-and-intents/) | emit pane-created during activate |
| ABI-047 | After every activation of a plugin that holds tree.read, the first batch MUST start with one tree-changed event listing every node in pre-order. | [reference-events-and-intents](/pages/reference-events-and-intents/) | activate with 3 nodes -> one event, 3 ids |
| ABI-048 | An effectful import called inside views.render, annotate or settings-validate MUST return err(not-permitted) with the message 'effectful import in a pure call', and the call MUST continue. | [reference-runtime-model](/pages/reference-runtime-model/) | call intents.submit inside views.render |
| ABI-049 | The host MUST NOT call an export of an instance while another export of that instance is on the stack. | [reference-runtime-model](/pages/reference-runtime-model/) | call an export from an import |
| ABI-050 | A services.call to the caller's own service and an invoke-command for the caller's own command MUST be queued and run as later calls. | [reference-runtime-model](/pages/reference-runtime-model/) | invoke own command inside on-events |
| ABI-051 | Every WIT function that returns `request-id` MUST have an awaiting wrapper with the WIT name and a starting wrapper with the suffix `_request` in Rust and `Request` in TypeScript. A WIT name that ends in `-json` MUST keep the suffix as `_json` in Rust and `Json` in TypeScript. | [reference-wit](/pages/reference-wit/#wrapper-names-in-the-sdks) | SDK binding test: list the wrappers of `http`, `jobs`, `services`, `inference` and `settings` |
| ABI-052 | A feature marked for removal MUST be listed in the release notes with the minor in which it stops working, at least two minors after the marking release. | [reference-abi-evolution](/pages/reference-abi-evolution/) | Read the release notes for a marked feature and check the version gap. |
| ABI-053 | A marked feature MUST keep working until its removal minor, and the host MUST log one `deprecated` record per plugin and feature at load time, naming the feature and that minor. | [reference-abi-evolution](/pages/reference-abi-evolution/) | Load a plugin that uses a marked feature and count the records. |
| ABI-054 | Removing a feature MUST move the affected refusals to the codes of section 4 and MUST NOT change any other code. | [reference-abi-evolution](/pages/reference-abi-evolution/) | Remove a fixture feature and assert the refusal code and the other codes. |
| ABI-055 | A host that ships `ilium:plugin@1.0.0` MUST load a package whose `api` is the final `0.x` minor through that minor's frozen copy and adapter, and MUST NOT load a package whose `api` is any other `0.x` minor. | [reference-abi-major-versions](/pages/reference-abi-major-versions/) | Doc check in ilium/tests/plugin_user_flows: a fixture for the final 0.x minor loads on a 1.0.0 host and a fixture for an earlier 0.x minor is refused. |
| ABI-056 | A package whose `api` major differs from the host's major and is not the retained 0.x minor MUST be refused with `package.api-unsupported`, and the message MUST name the package's `api` and the supported versions. | [reference-abi-major-versions](/pages/reference-abi-major-versions/) | Doc check in ilium/tests/plugin_user_flows: install a 1.0.0 package on a 0.x host and expect package.api-unsupported. |
| ABI-057 | The host MUST log one `deprecated` record per plugin when it loads the final `0.x` minor, naming the major version in which that minor stops loading. | [reference-abi-major-versions](/pages/reference-abi-major-versions/) | Doc check in ilium/tests/plugin_user_flows: load a final 0.x fixture on a 1.0.0 host and count the deprecated records. |
| ABI-058 | A major release MUST NOT lower a limit or remove a feature unless the change was marked in an earlier release under ABI-052 and ABI-053, and the change MUST take effect in that major release only. | [reference-abi-major-versions](/pages/reference-abi-major-versions/) | Doc check in ilium/tests/plugin_user_flows: a major release notes entry names each lowered limit and removed feature with its marking release. |
| ABI-059 | A component built for `ilium:plugin@1.0.0` MUST carry the custom section `ilium:api-version` with bytes `00 01 00 00 00 00`, and a section that differs from the manifest `api` MUST be refused with `manifest.api-version`. | [reference-abi-major-versions](/pages/reference-abi-major-versions/) | Doc check in ilium/tests/plugin_user_flows: inspect a 1.0.0 fixture and install one with a mismatched section. |
| ABI-060 | Inside the 1.x line a host MUST apply the window rule of ABI evolution (current minor and previous one), and a package whose `api` major differs from the host's major MUST NOT be loaded through that window. | [reference-abi-major-versions](/pages/reference-abi-major-versions/) | Doc check in ilium/tests/plugin_user_flows: a 1.1.0 host loads 1.0.0 and 1.1.0 fixtures and refuses a 0.x fixture other than the retained minor. |
| ABI-061 | The host MUST bound the nesting depth of every decoded WIT value, including nested JSON strings and recursive widget trees, at 32 levels, and MUST NOT let decoding recurse on the native stack beyond that bound. | [reference-errors-and-limits](/pages/reference-errors-and-limits/) | Doc check plus a conformance case with a 33-level value |
| ABI-062 | A value nested deeper than the bound MUST be refused with the stable code `abi.depth-exceeded` and MUST NOT trap the plugin or the host. | [reference-errors-and-limits](/pages/reference-errors-and-limits/) | Conformance case asserts the code and a live host |
| ABI-063 | The host MUST bound decoding work per call by the same limits whether the value comes from a plugin return or from a host-built event, and MUST count a refusal in the ABI refusal metric of ABI-079. | [reference-errors-and-limits](/pages/reference-errors-and-limits/) | Conformance case on both directions |
| ABI-064 | The project MUST publish and run a matrix of host minor against SDK minor, covering at least the old plugin on the new host and the new SDK on the old host, for every minor inside the window. | [reference-abi-evolution](/pages/reference-abi-evolution/) | Doc check of the published matrix against the CI job list |
| ABI-065 | Each matrix cell MUST state its expected result: loads, loads with a `deprecated` record, or refused with the code of section 4. | [reference-abi-evolution](/pages/reference-abi-evolution/) | Matrix test compares each cell to the observed result |
| ABI-066 | A release MUST NOT ship while any matrix cell inside the window differs from its stated result. | [reference-abi-evolution](/pages/reference-abi-evolution/) | Release gate check in the CI description |
| ABI-067 | This page MUST hold one table that gives, for every WIT field of type string, list or blob, the maximum size and the refusal code used when it is exceeded. | [reference-wit](/pages/reference-wit/) | Doc check that every string, list and blob field has a table row |
| ABI-068 | The host MUST enforce each limit of that table before the value reaches host logic, and MUST refuse an oversize value with that row's code without truncating it. | [reference-wit](/pages/reference-wit/) | Conformance case per table row |
| ABI-069 | A limit in the table MUST NOT be raised or lowered within the same ABI major without a minor release that lists the change in its notes. | [reference-wit](/pages/reference-wit/) | Doc check of release notes against the table history |
| ABI-070 | The host MUST pin the exact Wasmtime version and the Component Model canonical ABI revision it ships with, and MUST report both in `ilium plugin doctor` output. | [reference-host-runtime](/pages/reference-host-runtime/) | Doctor output test |
| ABI-071 | A runtime upgrade that changes canonical-ABI behaviour, trap behaviour or fuel accounting MUST NOT ship until the full ABI conformance corpus of ABI-073 passes on the new runtime. | [reference-host-runtime](/pages/reference-host-runtime/) | Release gate check plus corpus run |
| ABI-072 | A runtime upgrade MUST list in the release notes every observable change in traps or refusal codes, and a change that alters a code MUST follow the removal rules of ABI-052. | [reference-host-runtime](/pages/reference-host-runtime/) | Doc check of release notes |
| ABI-073 | The project MUST publish a versioned ABI conformance corpus, with a stable identifier per case, that an alternative host or embedder can run without Ilium source. | [reference-host-conformance](/pages/reference-host-conformance/) | Corpus manifest check |
| ABI-074 | Each corpus case MUST state its input, the expected result or refusal code, and the matrix row of ABI-030 it covers. | [reference-host-conformance](/pages/reference-host-conformance/) | Corpus lint that every case has all three fields |
| ABI-075 | A host MAY claim ABI conformance for a version only when every case of that version's corpus passes, and the corpus version MUST equal the API version it checks. | [reference-host-conformance](/pages/reference-host-conformance/) | Doc check of the pass criteria |
| ABI-076 | The page MUST give a runbook for an operator whose host upgrade makes installed plugins refuse with `package.api-unsupported` or a section mismatch, covering triage, pin, roll back and bulk report in that order. | [reference-abi-major-versions](/pages/reference-abi-major-versions/) | Doc check of the four runbook steps |
| ABI-077 | The host MUST provide a command that lists every installed plugin refused for an ABI reason, with the refusal code and the plugin's declared `api`, in one output. | [reference-abi-major-versions](/pages/reference-abi-major-versions/) | CLI output test on a mixed install set |
| ABI-078 | Rolling the host back MUST NOT change installed plugin files, grants or stored settings, and a plugin refused after the upgrade MUST load again unchanged after the rollback. | [reference-abi-major-versions](/pages/reference-abi-major-versions/) | Upgrade and rollback test comparing plugin state |
| ABI-079 | Every ABI trap and every ABI refusal code MUST produce a log record with the stable fields `plugin`, `code` and `phase`. | [reference-health-and-watchdog](/pages/reference-health-and-watchdog/) | Log record test per ABI code |
| ABI-080 | The host MUST expose a counter per ABI code, labelled by plugin id, so an operator can alert on ABI-class failures without parsing messages. | [reference-health-and-watchdog](/pages/reference-health-and-watchdog/) | Metrics export test |
| ABI-081 | A host and an SDK MUST ignore unknown object members when they read a host-defined JSON payload, and the host MUST evolve such a payload within one ABI major only by adding optional members, never by removing, renaming or changing the type of a member. | [Unknown members in host JSON payloads](/pages/reference-runtime-model/) | Feed a payload with an extra member to each reader; check it is ignored and parsing still succeeds. |
| ABI-082 | Within one ABI major, only the error code and the documented reason-word prefix of a message MUST stay stable; a host MAY reword the rest of a message in a minor release, the conformance corpus MUST NOT assert on that rest, and the SDKs MUST NOT branch on message text. | [Error message text stability](/pages/reference-errors-and-limits/) | Change the non-prefix text of a fixture message; check the conformance corpus still passes and no SDK branches on message text. |
| ABI-083 | A release that removes or graduates an unstable feature MUST name it in its release notes one minor version in advance; the host MUST write one `deprecated` record per plugin that still lists the feature, and after graduation a manifest that still lists it under `unstable` MUST load with that `deprecated` record instead of being refused with `manifest.unstable-unknown`. | [Graduating unstable features](/pages/reference-abi-evolution/) | Load a manifest that lists a graduated feature under unstable; check it loads and writes one deprecated record. |

## How the rows are tested

Three kinds of test cover the rows.

1. **Static checks.** `_tools/check.py` compares each labelled WIT excerpt with the WIT reference (ABI-001, ABI-005). A diff of case order against the frozen snapshot covers ABI-002, ABI-003 and ABI-019.
2. **Install tests.** A test installs a crafted package and expects the refusal code (ABI-004, ABI-006, ABI-007, ABI-018, ABI-021, ABI-023, ABI-024).
3. **Runtime tests.** A fake host runs a plugin that breaks the rule and the test expects the trap, the refusal or the outcome (ABI-008 to ABI-017, ABI-025 to ABI-050).

**Rust**

```rust
use ilium_plugin_sdk::testing::FakeHost;

#[test]
fn more_than_256_submissions_trap() {
    let mut host = FakeHost::new();
    host.grant("tree.write");
    let result = host.run_on_events_returning(257);
    assert!(result.is_trap());
    assert_eq!(host.applied_count(), 0);
}
```

**TypeScript**

```ts
import { createFakeHost } from '@ilium/plugin-sdk/testing';

test('more than 256 submissions trap', () => {
  const host = createFakeHost();
  host.grant('tree.write');
  const result = host.runOnEventsReturning(257);
  expect(result.trapped).toBe(true);
  expect(host.appliedCount()).toBe(0);
});
```

## Edge cases

- A batch of exactly 256 submissions is valid.
- A dedup key of 129 characters is a malformed submission, so the whole batch is refused (ABI-009, ABI-010).
- A take at 30 s plus one tick returns none (ABI-012, ABI-014).

## See also

- [WIT reference](/pages/reference-wit/)
- [ABI evolution](/pages/reference-abi-evolution/)
- [Runtime model](/pages/reference-runtime-model/)
- [Host conformance matrix](/pages/reference-host-conformance/)
