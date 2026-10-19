---
layout: page
title: Test harness reference
permalink: /pages/reference-test-harness/
---

## Test harness reference

The test harness runs a plugin without Ilium. It stands in for the host: it delivers events, answers host calls, draws views into text and records everything the plugin sends. This page is the normative contract of the harness. The how-to is [Testing plugins](/pages/testing/). Unit tests for the harness itself are written from the rules on this page.

Terms:

- **FakeHost**: the harness object for one plugin on one side.
- **FakeSession**: the harness object for several plugins and both sides in one fake session.
- **Native mode**: the plugin is compiled for the host machine and called as plain Rust or TypeScript. Fast, no component.
- **Component mode**: the built `.wasm` component is loaded in a real WebAssembly runtime and called through its exports. Slower, proves the artifact.
- **Fixture**: a JSON file that sets the starting state of the fake host and lists events to deliver.
- **Expectation file**: a JSON file next to a fixture that states what the plugin must submit.
- **Golden file**: a text file that holds an expected render.
- **Replay file**: a JSONL recording of a session, used to re-run it.

## You will need

- A project from [Project templates](/pages/reference-project-templates/), or a plugin with the SDK `testing` feature (Rust) or `@ilium/plugin-sdk/testing` (TypeScript).
- Rust 1.90 or later, or Bun 1.2 or later.
- For component mode: a built `dist/<side>.wasm` from `ilium plugin build`.

## Modes

| Mode | What runs | Selected by | Speed | Proves |
|---|---|---|---|---|
| Native | The plugin crate or module as a library | `cargo test`, `bun test` | milliseconds per test | Logic, views, state |
| Component | `dist/<side>.wasm` in a WebAssembly runtime | `ilium plugin test`, or `ILIUM_PLUGIN_TEST_MODE=component` | tens of milliseconds per test | The artifact, the exports, the limits |

| Rust | TypeScript | Returns |
|---|---|---|
| `host.mode() -> Mode` | `host.mode(): 'native' \| 'component'` | The mode of this host. The default is native under `cargo test` and `bun test`, and component under `ilium plugin test` (`--component <file>` only picks the wasm file). |

- **[DEV-400]** Both modes MUST expose the same `FakeHost` API. A test written for native mode MUST run in component mode with no source change when it uses only the API on this page.
- **[DEV-401]** In component mode the harness MUST enforce the deadlines and the memory ceiling of [Errors and limits](/pages/reference-errors-and-limits/). In native mode it MUST NOT, and it MUST say so in `host.mode()`, which returns `Mode::Native` (TypeScript `'native'`) or `Mode::Component` (`'component'`).
- **[DEV-402]** The project's own tests run in native mode with `cargo test` or `bun test`. `ilium plugin test` runs the fixtures with expectation files in component mode, as [Plugin command-line reference](/pages/reference-plugin-cli/) defines. A test written with `FakeHost` MAY be run in component mode by setting `ILIUM_PLUGIN_TEST_MODE=component`, which makes `FakeHost` load `dist/<side>.wasm` instead of the native type.
- **[DEV-403]** In component mode a trap MUST be reported as a test failure with the trap code, unless the test calls `expect_trap`.
- **[DEV-404]** `host.mode()` MUST return `native` under `cargo test` and `bun test`, and `component` under `ilium plugin test` (`--component <file>` only picks the wasm file) or `ILIUM_PLUGIN_TEST_MODE=component`. `assert_within_budget` MUST be skipped with the reason `needs-component` when `host.mode()` is `native`.

## Setup: the builder

A host is built in steps, then activated. Setup is complete at `activate`. Grants and tier cannot change the builder after that, except through the runtime calls below.

| Rust | TypeScript | Effect |
|---|---|---|
| `FakeHost::new()` | `createFakeHost()` | Empty host: no grants, tier T1, side server, empty tree |
| `FakeHost::for_manifest(path)` | `createFakeHost().forManifest(path)` | Takes id, side, grants and tier from a manifest. Returns `Result` in Rust. |
| `.grant(name)` | `.grant(name)` | Adds one grant |
| `.grants(&[names])` | `.grants([names])` | Adds several grants |
| `.tier(Tier::T2)` | `.tier('T2')` | Sets the tier |
| `.side(Side::Client)` | `.side('client')` | Sets the side |
| `.platform(Platform::Linux)` | `.platform('linux')` | Sets the reported platform |
| `.with_fixture(path)` | `.withFixture(path)` | Loads a fixture |
| `.seed(u64)` | `.seed(bigint)` | Sets the random seed |
| `.build()` | `.build()` | Optional. Ends the setup without activating. |

- **[DEV-410]** Every builder method MUST return the builder, so calls chain. Calling a builder method after `activate` MUST fail with `harness.setup-closed`.
- **[DEV-411]** A host built with `for_manifest` MUST take its grants from the `capabilities` object of the manifest for the chosen side. A grant not in the manifest MUST be refused by `.grant` and by `host.grant_now` with `harness.grant-not-declared`, because a plugin can never hold a grant its manifest does not request. `FakeHost::new()` has no manifest and accepts any grant name that exists in the grant catalogue.
- **[DEV-412]** An unknown grant name MUST fail with `harness.grant-unknown`.
- **[DEV-413]** The default seed MUST be 1. `ILIUM_PLUGIN_TEST_SEED` overrides the default. An explicit `.seed` call wins.
- **[DEV-414]** The default clock MUST be the fake clock at `2030-01-01 00:00:00` UTC. Wall time never leaks into a test.

### Grants

| Rust | TypeScript | Effect |
|---|---|---|
| `host.grant_now(name)` | `host.grantNow(name)` | Grants at run time, as if the user approved. Re-activates an active plugin with the new `granted` list. |
| `host.revoke(name)` | `host.revoke(name)` | Revokes at run time. Re-activates an active plugin with the smaller `granted` list. The next call that needs the grant is refused with `not-permitted`. |
| `host.grants_changed(&[names])` | `host.grantsChanged([names])` | Replaces the whole granted list in one step, as if the user changed several grants at once. Requests that need a removed grant complete as `failed(revoked)` before `deactivate(revoked)`; the plugin is then re-activated. |
| `host.breach_deadline(export)` | `host.breachDeadline(export)` | Makes the next call of the export breach its deadline without waiting in fake time. It applies the effect of the call class in [Scheduling, deadlines and cancellation](/pages/reference-host-scheduling/#call-classes): a trap for hard classes, a discarded result for soft classes, a dropped instance without failure for `deactivate`. |

- **[DEV-415]** `host.grant_now(name)` on an active plugin MUST call `deactivate(granted)`, then `activate` with `instance` plus 1 and the new `granted` list. `host.revoke(name)` MUST do the same with `deactivate(revoked)`. No event is delivered.
- **[DEV-564]** `host.grants_changed(list)` MUST follow the revocation rule of [Runtime model](/pages/reference-runtime-model/#13-link-time-grant-enforcement): requests that need a removed grant complete as `failed(revoked)` before `deactivate(revoked)`, then `activate` runs with the new list. A list that only adds grants MUST use `deactivate(granted)`.
- **[DEV-565]** `host.breach_deadline(export)` MUST apply the effect of the call class in the scheduling table, and a `deactivate` breach MUST NOT count as a failure.
- **[DEV-416]** After `revoke`, or for a capability the test declined, intents that need the grant MUST complete as `refused(plugin-error{code: not-permitted})`, and host imports that need it MUST return `Err(not-permitted)`. A call to a function of a capability the manifest never requested MUST trap with `host.capability-not-requested`.
- **[DEV-417]** Runtime results MUST use the WIT `error-code` enum of [Errors and limits](/pages/reference-errors-and-limits/#the-plugin-error-record) and no other value. Dotted codes such as `harness.setup-closed` or `trap.deadline` are harness, fixture, trap and command-line diagnostics only, and MUST NOT appear in `plugin-error.code`.

**Error values.** Runtime results always use the WIT error-code enum. Dotted codes are used only for harness, fixture, trap and CLI diagnostics, never inside `plugin-error.code`. The table maps the situations of this page to the values a test asserts.

| Situation | Value the test asserts |
|---|---|
| Grant revoked or declined, intent | `refused(plugin-error{code: not-permitted})` |
| Grant revoked or declined, import | `Err(not-permitted)` |
| Function of a capability never requested | trap `host.capability-not-requested` |
| Path out of the granted scope | `Err(not-permitted)` |
| Storage value or bucket over its quota | `Err(quota-exceeded)` |
| Setting value that fails validation | `Err(invalid-argument)` |
| Bad dedup key | `refused(plugin-error{code: invalid-argument, message starting "dedup-key-invalid:"})` |
| Unmatched fake process or network call | `Err(internal)` with the message prefix `harness.unexpected-call:`, and the test fails at teardown with `harness.unexpected-call` |

## Activation

| Rust | TypeScript | Returns |
|---|---|---|
| `host.activate::<P>() -> PluginHandle` | `host.activate({ lifecycle, handler }): Promise<PluginHandle>` | A `PluginHandle`. Panics or throws on failure. |
| `host.try_activate::<P>() -> Result<PluginHandle, PluginError>` | `host.tryActivate({ lifecycle, handler }): Promise<PluginHandle \| PluginError>` | The handle, or the error the plugin returned |
| none | `host.activateModule(path): Promise<PluginHandle>` | A `PluginHandle`. Imports a fresh copy of the module. |
| `host.deactivate(reason: DeactivationReason) -> Option<Vec<u8>>` | `host.deactivate(reason): Promise<Uint8Array \| undefined>` | The restore bytes the plugin returned |

- **[DEV-420]** `activate` MUST receive an `Activation` with these fields: `plugin` is the manifest id, or `"test.plugin"` for `FakeHost::new()`; `api` is `"ilium:plugin@0.1.0"`; `instance` is 1, then plus 1 on each re-activation; `side` is the fixture side, default `"server"`; `settings_json` is the fixture settings object serialized, or `"{}"`; `granted` is the current grants, sorted; `restored` is `None`, unless `restart_with(bytes)` supplied bytes.
- **[DEV-421]** `activate` MUST panic (Rust) or throw (TypeScript) with the plugin error code and message when the plugin returns an error, so a failing activation fails the test with a clear message. `try_activate` MUST return the error instead.
- **[DEV-422]** `activateModule` MUST import a fresh copy of the module on each call, so module state never leaks between tests. A module cached by the runtime MUST NOT be reused.
- **[DEV-423]** A plugin that does not return from `activate` in 2,000 ms of fake time MUST fail with `trap.deadline` in component mode. In native mode the harness MUST advance the fake clock by the recorded host call time only, and no deadline applies.
- **[DEV-424]** `deactivate(reason)` MUST run `deactivate` with a reason of the WIT `deactivation-reason` enum (`shutdown`, `disabled`, `upgrade`, `revoked` or `granted`; `crash-restart` is reserved and the harness refuses it with `harness.reason-reserved`). The default reason is `shutdown`.

### Isolation between tests

- **[DEV-425]** Each test MUST get its own host. Nothing a plugin stores in memory, in the fake storage or in the fake file system may be visible to another test.
- **[DEV-426]** In Rust, one host per `#[test]` function is enough, because each test builds its own. A plugin type with static state is a bug in the plugin, and the harness cannot isolate it. Use no `static mut` and no `OnceLock` that holds state.
- **[DEV-427]** In Bun, the preload file `@ilium/plugin-sdk/testing/setup` MUST reset the harness registry before each test. Import the plugin with `activateModule`, not with a top-level `import`, when the plugin keeps state at module level.
- **[DEV-428]** Tests MAY run in parallel. The harness MUST hold no global mutable state between hosts.
- **[DEV-429]** In TypeScript, `activate({ lifecycle, handler })`, `tryActivate`, `activateModule(path)`, `deactivate` and `settle()` MUST return promises. `deliver(events)` MUST be synchronous and return `Submission[]`. Tests `await` the promise-returning calls.

## Delivering events and reading submissions

| Rust | TypeScript | Effect |
|---|---|---|
| `host.deliver(events: Vec<Event>) -> Vec<Submission>` | `host.deliver(events: Event[]): Submission[]` | Runs `on-events` once |
| `host.deliver_one(event: Event) -> Vec<Submission>` | `host.deliverOne(event): Submission[]` | Same, for one event |
| `host.settle()` | `host.settle(): Promise<void>` | Delivers queued completions as `request-completed` events and repeats until the queue is empty |
| `host.outcomes() -> Vec<Outcome>` | `host.outcomes(): Outcome[]` | Outcomes of all submissions so far, in submission order |
| `host.submitted() -> Vec<Submission>` | `host.submitted(): Submission[]` | All submissions so far, in order |
| `host.pending_requests() -> Vec<RequestRef>` | `host.pendingRequests(): RequestRef[]` | Requests not yet completed |
| `host.complete(request: RequestRef, outcome: Outcome)` | `host.complete(request, outcome)` | Completes one request by hand |

`Outcome` has the cases of the WIT `outcome` variant: `Ok`, `OkNode(u64)`, `Duplicate(RequestRef)`, `Refused(PluginError)`, `Failed(PluginError)`, `Cancelled` and `OkEpoch(u64)`. `Outcome::Ok` is a unit case and carries no payload; in TypeScript it is `{ tag: 'ok' }`.

- **[DEV-430]** `deliver` MUST pass the events to the plugin in one batch, in the given order. It MUST fail with `harness.batch-too-large` when the list holds more than 256 events, because the real host splits batches at 256.
- **[DEV-431]** `deliver` MUST return the submissions the plugin returned from `on-events`, without applying them. It MUST also record them in `submitted()`.
- **[DEV-432]** `deliver` MUST NOT deliver outcomes by itself. Outcomes are queued as `request-completed` events. `settle()` delivers them and also delivers the submissions that those events produce, until the queue is empty or 100 rounds pass. After 100 rounds `settle` MUST fail with `harness.settle-limit`.
- **[DEV-433]** More than 256 submissions from one batch MUST fail the test with `trap.submission-batch` in both modes, because the real host traps the instance.
- **[DEV-434]** The harness MUST apply the batch refusal rule of [Events and intents](/pages/reference-events-and-intents/): a refused submission completes as `refused(not-permitted)` or `refused(invalid-argument)` and all others complete as `refused(cancelled)` with the message `batch refused: submission <index>`.
- **[DEV-435]** The harness MUST validate dedup keys: charset `[A-Za-z0-9._:/-]`, 1 to 128 characters, and a second submission with the same key inside 10 fake minutes MUST complete as `duplicate`. A bad key MUST complete as `refused(plugin-error{code: invalid-argument, message starting "dedup-key-invalid:"})`.
- **[DEV-436]** `deliver` MUST NOT run timers. Time moves only through `advance_clock`.

### Worked example

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::testing::{events, FakeHost};
use ilium_plugin_sdk::types::tree::AgentActivity;

#[test]
fn badge_then_outcome() {
    let mut host = FakeHost::for_manifest("manifest.json").unwrap();
    host.activate::<Plugin>();
    let submissions = host.deliver(vec![events::activity(42, AgentActivity::Idle, AgentActivity::Working)]);
    assert_eq!(submissions.len(), 1);
    host.settle();
    assert_eq!(host.outcomes(), vec![Outcome::Ok]);
}
```

```ts
import { expect, test } from 'bun:test';
import { createFakeHost, events } from '@ilium/plugin-sdk/testing';

test('badge then outcome', async () => {
  const host = createFakeHost().forManifest('manifest.json');
  await host.activateModule('../src/server');
  const submissions = host.deliver([events.activity(42n, 'idle', 'working')]);
  expect(submissions.length).toBe(1);
  await host.settle();
  expect(host.outcomes()).toEqual([{ tag: 'ok' }]);
});
```

### Event builders

The `testing` module builds the events that most tests need, so a test does not spell a whole record.

| Rust | TypeScript | Returns |
|---|---|---|
| `events::activity(node: u64, previous: AgentActivity, current: AgentActivity) -> Event` | `events.activity(node: bigint, previous, current): Event` | `agent-activity-changed` with `revision` 1 |
| `events::command(name: &str, arguments_json: &str) -> Event` | `events.command(name, argumentsJson): Event` | `command-invoked` with origin `palette` and an empty context |

- **[DEV-447]** The builders MUST produce valid events of the WIT `event` variant. `activity` MUST raise `revision` by 1 for each event built for the same node in one host.

## Views and rendering

| Rust | TypeScript | Returns |
|---|---|---|
| `host.render(target: ViewTarget, columns: u16, rows: u16) -> Screen` | `host.render(target, columns: number, rows: number): Screen` | `Screen` |
| `host.send_action(target: ViewTarget, key: &str, kind: ActionKind, value: Option<ActionValue>)` | `host.sendAction(target, key, kind, value)` | Delivers a `ui-action` event |
| `host.click(target: ViewTarget, column: u16, row: u16) -> bool` | `host.click(target, column, row): boolean` | Delivers a click on a cell |
| `host.key(target: ViewTarget, key: &str)` | `host.key(target, key)` | Delivers a key press |
| `screen.text() -> String` | `screen.text(): string` | Text of the screen, rows joined by `\n`, trailing spaces trimmed |
| `screen.cell(row: u16, column: u16) -> Cell` | `screen.cell(row, column): Cell` | `Cell` with `symbol`, `foreground`, `background`, `bold`, `italic`, `underline` |
| `screen.row(row: u16) -> String` | `screen.row(row): string` | One row as a string |
| `screen.find(text: &str) -> Option<(u16, u16)>` | `screen.find(text): [number, number] \| undefined` | First `(row, column)` where the text starts, or none |

`columns` and `rows` are `u16` in Rust and `number` in TypeScript. A view is named by its `view-target`; no default view exists. `ViewTarget` has the five cases of the WIT `view-target` variant:

| WIT case | Rust | TypeScript |
|---|---|---|
| `panel(string)` | `ViewTarget::Panel(String)` | `{ tag: 'panel', val: string }` |
| `pane(node-id)` | `ViewTarget::Pane(u64)` | `{ tag: 'pane', val: bigint }` |
| `popup(string)` | `ViewTarget::Popup(String)` | `{ tag: 'popup', val: string }` |
| `settings-page(string)` | `ViewTarget::SettingsPage(String)` | `{ tag: 'settings-page', val: string }` |
| `onboarding-step(string)` | `ViewTarget::OnboardingStep(String)` | `{ tag: 'onboarding-step', val: string }` |

Scenes are not views. A scene is drawn with `FakeScene`, which the section [Scenes](#scenes) defines.

- **[DEV-446]** `render` MUST accept exactly the five `ViewTarget` cases above. A scene id passed as a view target MUST fail with `harness.view-target-unknown`.

- **[DEV-440]** `cell(row, column)` MUST take the row first, then the column, and both start at 0. Reading outside the screen MUST fail with `harness.cell-out-of-range`.
- **[DEV-441]** `render` MUST run the same code path as the client: view tree, layout, then cells. The width unit is terminal columns, and wide characters use two columns.
- **[DEV-442]** A view that returns a node the client would refuse (for example a node limit breach) MUST fail the test with `trap.invalid-ui-tree`, the trap code the client raises.
- **[DEV-443]** `render` MUST fail with `harness.render-size` when the width is below 10 or above 500, or the height is below 1 or above 200.
- **[DEV-444]** `click` MUST find the action under the cell and deliver it as the client would. A click on a cell with no action MUST return `false`.
- **[DEV-445]** A render MUST be pure. Rendering twice with no event between MUST give equal screens.

## The fakes

Each fake is a property of the host. All of them are deterministic.

### Fake clock

| Call | Effect |
|---|---|
| `host.clock().now() -> u64` | Current fake instant in milliseconds since the Unix epoch. TypeScript: `host.clock().now(): bigint`. |
| Rust `host.advance_clock(ms: u64)` | Moves time forward by milliseconds and fires due timers in order |
| TypeScript `host.advanceClock(ms: number \| bigint)` | The same. A number must be a non-negative safe integer. |
| `host.set_clock(instant_ms: u64)` | Sets time. TypeScript `host.setClock(ms: bigint)`. Moving back MUST fail with `harness.clock-backwards`. |

- **[DEV-450]** `advance_clock` MUST fire every `timer-fired` and `durable-timer-fired` event that falls due, in time order, in one batch per distinct due time. A timer that re-arms inside the advanced span MUST fire again inside it.
- **[DEV-452]** A negative number, a fraction or an unsafe integer passed to `advanceClock` MUST fail with `harness.clock-argument`.
- **[DEV-451]** Time zone MUST be UTC. The fake clock has no daylight saving.

### Fake file system

Backs `fs.read`, `fs.write` and `fs.watch`. The fixture key `files` fills it.

| Rust | TypeScript | Effect |
|---|---|---|
| `host.fs().write(path: impl Into<FsPath>, bytes: &[u8])` | `host.fs().write(path: FsPath \| string, bytes: Uint8Array)` | Writes a file for test setup |
| `host.fs().read(path: impl Into<FsPath>) -> Option<Vec<u8>>` | `host.fs().read(path): Uint8Array \| undefined` | Reads a file for an assertion |

A path is an `FsPath` of a scope and a relative path, for example `FsPath{ scope: "project", path: "notes.md" }`. The shorthand string form is `"project:notes.md"`.

- **[DEV-455]** The fake file system MUST enforce the scope of the grant: a read outside the granted scopes MUST return `Err(not-permitted)`.
- **[DEV-456]** The harness MUST support `host.fs().write(path, bytes)` and `host.fs().read(path)` for test setup and for assertions. A write by the test to a watched path MUST deliver `fs-changed`.
- **[DEV-458]** A path that is absolute or contains `..` MUST fail with `harness.fs-path-invalid`.
- **[DEV-457]** The fake file system MUST be in memory. It MUST NOT touch the real disk.

### Fake process

Backs `process.run`. Register results before the call.

| Rust | TypeScript | Effect |
|---|---|---|
| `host.process().expect(program: &str, arguments: &[&str]) -> ProcessExpectation` | `host.process().expect(program, args): ProcessExpectation` | Registers one expected run |
| `.returns(exit: ProcessExit)` | `.returns({ status, signal, stdout: Uint8Array, stderr: Uint8Array })` | The next matching run returns this. `ProcessExit{ status: Option<i32>, signal: Option<i32>, stdout: Vec<u8>, stderr: Vec<u8> }`. |
| `.fails(error: PluginError)` | `.fails(error)` | The next matching run completes as `failed(error)` |
| `host.process().calls() -> Vec<ProcessSpec>` | `host.process().calls(): ProcessSpec[]` | Every run requested, in order, with `cwd`, `stdin` and `environment` |
| `host.process().allow_unexpected()` | `host.process().allowUnexpected()` | Unmatched runs return `ProcessExit{ status: Some(0), signal: None, stdout: [], stderr: [] }` instead of failing the test |

Matching rule: `program` and `arguments` must be equal exactly. `cwd`, `stdin` and `environment` are recorded and are not matched. The grant checked is `process.run:<program>`.

- **[DEV-460]** A run with no matching expectation MUST return `Err(internal)` with a message that starts with `harness.unexpected-call:`, and the test MUST fail at teardown with `harness.unexpected-call`, unless `allow_unexpected` is set. No real process ever starts.
- **[DEV-464]** The fake process MUST match `program` and `arguments` exactly, MUST record `cwd`, `stdin` and `environment` for `calls()`, and MUST check the `process.run:<program>` grant before it looks for an expectation.

### Fake network

Backs `http.request`. The fixture key `http` fills it.

- **[DEV-462]** A request with no matching entry MUST return `Err(internal)` with a message that starts with `harness.unexpected-call:`, and the test MUST fail at teardown with `harness.unexpected-call`. No real network call ever happens.
- **[DEV-463]** The fake HTTP client MUST check the `http:<host-pattern>` grant against the request host.

### Fake storage and settings

| Rust | TypeScript | Effect |
|---|---|---|
| `host.storage().dump(bucket: Bucket) -> BTreeMap<String, Vec<u8>>` | `host.storage().dump(bucket): Map<string, Uint8Array>` | Every key and value of one bucket |
| `host.storage().get(bucket: Bucket, key: &str) -> Option<Vec<u8>>` | `host.storage().get(bucket, key)` | One value |
| `host.settings().set(key: &str, value: serde_json::Value) -> Result<(), PluginError>` | `host.settings().set(key, value: unknown)` | Changes a setting. TypeScript throws `PluginError`. |
| `host.settings().get(key: &str) -> Option<serde_json::Value>` | `host.settings().get(key)` | Reads a setting |

- **[DEV-465]** `storage` MUST behave as the real store: a `put` is visible to `get` at once inside the same call batch, and values survive `deactivate` then `activate` on the same host.
- **[DEV-466]** `storage.put` of a value over 1,048,576 bytes MUST return `Err(quota-exceeded)`. A put that makes the bucket sum of key and value bytes exceed `storage_bucket_mib` x 1,048,576 (default 16 MiB) MUST return `Err(quota-exceeded)`. `host.storage().dump(bucket)` returns that bucket as a map.
- **[DEV-467]** `host.settings().set(key, value)` MUST validate the value against the settings schema and deliver `settings-changed` (Rust `Event::SettingsChanged`, TypeScript tag `settings-changed`) with the WIT `settings-change` payload. An invalid value MUST return `Err(invalid-argument)`.

### Fake tree, panes, agents and jobs

`host.tree()` returns a `FakeTree`. A tree node has this shape in Rust (`TreeNode`), in TypeScript and in a fixture: `{ "id": u64, "parent": u64 or null, "name": string, "class": "container" | "pane" | "folder", "content_kind": a WIT content-kind such as "ilium.terminal", "agent": optional { "signature", "activity", "objective", "goal" } }`. The cases of `class` are the WIT `node-class` cases.

| Rust | TypeScript | Effect |
|---|---|---|
| `tree.add(node: TreeNode) -> u64` | `tree.add(node): bigint` | Adds a node and delivers `tree-changed` |
| `tree.rename(id: u64, name: &str)` | `tree.rename(id, name)` | Renames a node and delivers `tree-changed` |
| `tree.set_activity(id: u64, activity: AgentActivity)` | `tree.setActivity(id, activity)` | Delivers `agent-activity-changed` with `revision` plus 1 |
| `tree.remove(id: u64)` | `tree.remove(id)` | Removes a node and its children and delivers `node-removed` |
| `tree.get(id: u64) -> Option<TreeNode>` | `tree.get(id): TreeNode \| undefined` | Reads a node |

- **[DEV-470]** `host.tree()` MUST offer `add`, `rename`, `set_activity`, `remove` and `get` with the signatures above. Each call that changes the tree MUST queue the matching events for the next `deliver` or `settle`.
- **[DEV-471]** Node ids MUST be assigned in increasing order from 1.
- **[DEV-472]** `host.jobs().list() -> Vec<JobRecord>` MUST list the jobs the plugin started, where `JobRecord{ request: u64, kind: String, input: Vec<u8> }`. `host.jobs().finish(request, Ok(bytes))` or `host.jobs().finish(request, Err(plugin-error))` MUST deliver one `request-completed` event whose `request-result` carries the job's correlation. `host.jobs().progress(request, percent, message)` MUST deliver `job-progress`.
- **[DEV-473]** `host.cli().invoke(args: &[&str], stdin: &[u8]) -> Invocation` MUST deliver `cli-invoked`. The `Invocation` exposes `name`, `arguments`, `stdout()`, `stderr()` and `exit_code() -> Option<u8>`.

## FakeSession

`FakeSession` runs several plugins, both sides, core plugins and extension points in one fake session. `FakeHost` runs one plugin only.

| Rust | TypeScript | Effect |
|---|---|---|
| `FakeSession::new()` | `createFakeSession()` | Empty session |
| `.install(package_dir)` | `.install(dir)` | Installs a plugin from a project folder |
| `.install_core(name)` | `.installCore(name)` | Installs a stub of a bundled core plugin |
| `.start()` | `await .start()` | Activates all plugins in dependency order |
| `session.server(id) -> &mut FakeHost` | `session.server(id): FakeHost` | The server side `FakeHost` of one plugin |
| `session.client(id) -> &mut FakeHost` | `session.client(id): FakeHost` | The client side `FakeHost` of one plugin |
| `host.run_hook(call: HookCall) -> HookReply` | `host.runHook(call): HookReply` | Runs the hook handler of the plugin with a `hook-call` and returns its `hook-reply` |
| `session.call_service(caller_id, name, version_range, request_json) -> Result<ServiceReply, PluginError>` | `session.callService(callerId, name, versionRange, requestJson): ServiceReply` | Calls a service from the plugin `caller_id`, routed to the provider |
| `.pump()` | `await .pump()` | Moves every queue until all are empty |

- **[DEV-480]** A session MUST resolve `services.use` against `services.provide` and route service calls between hosts.
- **[DEV-481]** A session MUST route `messages` between the sides of one plugin.
- **[DEV-482]** A session MUST run extension points: a hook point calls its contributors by `priority` and applies the reply rules in [Core extension points](/pages/reference-core-extension-points/).
- **[DEV-483]** `install_core` MUST install the stub of the Core stubs table below. The stub exposes the extension points and services of that core plugin and nothing else. The real core plugin code is never loaded. An unknown core plugin name MUST fail with `harness.core-unknown`.
- **[DEV-484]** `pump` MUST fail with `harness.settle-limit` after 100 rounds.
- **[DEV-485]** The session MUST apply the restart rule when a plugin traps, using fake time.
- **[DEV-486]** `session.server(id)` and `session.client(id)` MUST return the host of that side. An id that is not installed, or a side the plugin did not build, MUST fail with `harness.plugin-unknown`.
- **[DEV-487]** `host.run_hook(call)` MUST call `handle-hook` of the plugin with the `HookCall` and return the `HookReply`: `pass`, `replace(string)`, `refuse(string)` or `answer(string)`. A plugin without the hook handler MUST make it fail with `harness.not-a-hook-handler`.
- **[DEV-488]** `session.call_service(...)` MUST resolve the service against `services.provide` and the version range, call `handle` of the provider, and return its `ServiceReply`. A missing provider MUST return `Err(not-found)`.
- **[DEV-489]** A core stub MUST reply as the Core stubs table says: a `collect` point yields an empty list, a hook yields `pass`, and a service yields `{"ok":true}` for every method.

#### Core stubs

Each stub exposes exactly the extension points of [Core extension points](/pages/reference-core-extension-points/) for its core plugin and the services named below.

| Core plugin | Extension points (kind) | Services | Stub reply |
|---|---|---|---|
| `ilium.core.tree` | `before-move` (veto), `before-close` (veto), `node-sorts` (collect) | none | collect: empty list; hooks: `pass` |
| `ilium.core.layout` | `layout-presets` (collect), `before-split` (veto) | none | collect: empty list; hooks: `pass` |
| `ilium.core.terminal` | `before-send-input` (veto), `open-link` (first), `link-actions` (collect) | none | collect: empty list; hooks: `pass` |
| `ilium.core.smart-copy` | `recognisers` (collect), `after-copy` (transform) | none | collect: empty list; hooks: `pass` |
| `ilium.core.editor` | `before-save` (transform), `file-openers` (collect), `line-badges` (collect) | none | collect: empty list; hooks: `pass` |
| `ilium.core.kanban` | `card-actions` (collect), `card-moved` (observe) | none | collect: empty list; hooks: `pass` |
| `ilium.core.agent-detect` | `activity-rules` (collect), `classify` (transform) | none | collect: empty list; hooks: `pass` |
| `ilium.core.agent-toolbar` | `toolbar-actions` (collect), `banner-segments` (collect) | none | collect: empty list |
| `ilium.core.fleet` | `selectors` (collect), `before-broadcast` (transform) | none | collect: empty list; hooks: `pass` |
| `ilium.core.cost` | `price-sources` (collect), `quota-sources` (collect) | `usage` | collect: empty list; service: `{"ok":true}` |
| `ilium.core.titles` | `before-apply-title` (transform), `title-styles` (collect) | none | collect: empty list; hooks: `pass` |
| `ilium.core.inference` | `before-request` (transform), `after-response` (observe), `provider-profiles` (collect) | none | collect: empty list; hooks: `pass` |
| `ilium.core.notifications` | `before-notify` (veto), `channels` (collect) | none | collect: empty list; hooks: `pass` |
| `ilium.core.voice` | `transcript` (transform), `voice-intents` (collect) | none | collect: empty list; hooks: `pass` |
| `ilium.core.scheduler` | `schedule-sources` (collect), `before-send-scheduled` (veto) | none | collect: empty list; hooks: `pass` |
| `ilium.core.text-triggers` | `rule-packs` (collect), `before-fire` (veto) | none | collect: empty list; hooks: `pass` |
| `ilium.core.progress` | `footer-segments` (collect), `probe-kinds` (collect) | none | collect: empty list |
| `ilium.core.chatroom` | `before-post` (transform), `message-renderers` (collect) | none | collect: empty list; hooks: `pass` |
| `ilium.core.worktrees` | `before-create` (transform), `post-create-steps` (collect), `before-remove` (veto) | none | collect: empty list; hooks: `pass` |
| `ilium.core.sessions` | `snapshot-sections` (collect), `before-restore` (observe) | none | collect: empty list; hooks: `pass` |
| `ilium.core.compaction` | `techniques` (collect), `before-compact` (veto) | none | collect: empty list; hooks: `pass` |
| `ilium.core.settings` | `config-importers` (collect), `onboarding-steps` (collect) | none | collect: empty list |
| `ilium.core.keys-and-help` | `key-hints` (collect), `help-sources` (collect) | none | collect: empty list |
| `ilium.core.search` | `sources` (collect), `rank` (transform) | none | collect: empty list; hooks: `pass` |
| `ilium.core.ambient` | `palettes` (collect), `style-presets` (collect) | none | collect: empty list |
| `ilium.core.scenes` | `scene-packs` (collect), `scene-data-sources` (collect) | none | collect: empty list |
| `ilium.core.recap` | `fact-sources` (collect), `before-recap` (transform) | `recap` | collect: empty list; hooks: `pass`; service: `{"ok":true}` |
| `ilium.core.session-convert` | `formats` (collect), `after-convert` (observe) | `convert` | collect: empty list; hooks: `pass`; service: `{"ok":true}` |
| `ilium.core.agent-setup` | `instruction-blocks` (collect), `targets` (collect), `before-install` (veto) | `install` | collect: empty list; hooks: `pass`; service: `{"ok":true}` |
| `ilium.core.diagnostics` | `log-sinks` (observe), `report-sections` (collect) | `report` | collect: empty list; hooks: `pass`; service: `{"ok":true}` |


## Traps, restarts and failure injection

| Rust | TypeScript | Effect |
|---|---|---|
| `host.expect_trap(code: &str)` | `host.expectTrap(code)` | The next trap must have this code. It is then consumed. |
| `host.inject_trap(code: &str)` | `host.injectTrap(code)` | The next call into the plugin traps with this code |
| `host.inject_failure(import: &str, error: PluginError)` | `host.injectFailure(import, error)` | The next call to the import returns the error |
| `host.restarts() -> Vec<Restart>` | `host.restarts(): Restart[]` | Restarts so far, where `Restart{ at_ms: u64, delay_ms: u32 }` |
| `host.is_disabled() -> bool` | `host.isDisabled(): boolean` | True after the third failure inside 10 minutes |
| `host.diagnostics() -> Vec<Diagnostic>` | `host.diagnostics(): Diagnostic[]` | Harness diagnostics such as `events-not-granted` |

- **[DEV-490]** After a failure the harness MUST apply the restart rule of [Errors and limits](/pages/reference-errors-and-limits/#restart-and-disable-rule) in fake time. Count the failures in the last 10 minutes, including this one. One means restart after 1 s, two means restart after 5 s, three means disable on that side. A failure expires 10 minutes after it happened.
- **[DEV-491]** An instance that restarts MUST receive `tree-changed` for every node, and its in-memory state MUST be new.
- **[DEV-492]** A disabled plugin MUST receive no events. `host.is_disabled()` MUST return true.
- **[DEV-493]** `inject_failure` MUST accept only error codes in the catalogue of the import. Others fail with `harness.error-unknown`.

### Lifecycle control

| Rust | TypeScript | Effect |
|---|---|---|
| `inject_trap(code: &str)` | `injectTrap(code)` | The next call traps with that trap code |
| `restart_with(bytes: Vec<u8>)` | `restartWith(bytes: Uint8Array)` | `deactivate(upgrade)`, then `activate` with `restored = Some(bytes)` |
| `with_storage_snapshot(path)` | `withStorageSnapshot(path)` | Builder. Loads storage from a JSON file of `{bucket:{key:base64}}`. |
| `with_data_version(u32)` | `withDataVersion(n)` | Builder. Sets the stored data version before activation. |
| `crash_after_journal_write()` | `crashAfterJournalWrite()` | Traps after the next journal append is committed |
| `failures() -> Vec<FailureRecord{ at_ms: u64, code: String }>` | `failures()` | The failure window of the side |
| `lifecycle_state() -> LifecycleState` | `lifecycleState()` | The instance state of the side |
| `restored() -> Option<Vec<u8>>` | `restored()` | The `restored` bytes of the current activation |
| `logs() -> Vec<LogLine{ level: Level, message: String }>` | `logs()` | Lines the plugin wrote with `log` |
| `fixture_events() -> Vec<Event>` | `fixtureEvents()` | The decoded `events` list of the loaded fixture |

`LifecycleState` is the `instance-state` enum of [Plugin lifecycle states](/pages/reference-plugin-lifecycle-states/): in Rust `LifecycleState::StoppedByFailure`, in TypeScript `'stopped-by-failure'`.

- **[DEV-494]** `restart_with(bytes)` MUST call `deactivate(upgrade)` and discard its bytes, then `activate` with `instance` plus 1 and `restored` set to the given bytes. It MUST NOT count as a failure.
- **[DEV-495]** `inject_trap(code)` MUST count as a failure and MUST apply the restart rule of DEV-490. The code MUST be in the trap code table of [Errors and limits](/pages/reference-errors-and-limits/#trap-codes), or the call MUST fail with `harness.error-unknown`.
- **[DEV-496]** `crash_after_journal_write()` MUST trap after the next journal append is durable, so the next activation sees the entry.
- **[DEV-497]** `with_storage_snapshot` and `with_data_version` MUST fail with `harness.setup-closed` after `activate`. A snapshot file with an unknown shape MUST fail with `harness.snapshot-invalid`.
- **[DEV-498]** `failures()` MUST list the failures of the last 10 minutes of fake time, oldest first. `lifecycle_state()` MUST return `stopped-by-failure` after the third failure in the window, and otherwise the current instance state (`active`, `restart-scheduled` or `inactive`).

## Snapshot and golden files

| Call | Effect |
|---|---|
| `assert_golden(path: &str, actual: &str)` | Compares text to a golden file. TypeScript `assertGolden(path, actual)`. |
| `assert_snapshot(name: &str, value: &impl Serialize)` | Compares a value to the stored snapshot named `name` under `tests/snapshots/`. TypeScript `assertSnapshot(name, value)`. |
| `host.snapshot() -> HostSnapshot` | Returns the state record for restore tests: `HostSnapshot{ instance: u64, state: LifecycleState, storage: BTreeMap<String, BTreeMap<String, Vec<u8>>>, restored: Option<Vec<u8>> }`. TypeScript `host.snapshot()`. |

- **[DEV-500]** `assert_golden` MUST compare bytes after converting `\r\n` to `\n`. It MUST fail with a unified diff that shows at most 40 changed lines.
- **[DEV-501]** When `ILIUM_PLUGIN_UPDATE_GOLDEN=1` is set, `assert_golden` MUST write the actual text to the file and pass. It MUST print `golden updated: <path>` to standard error.
- **[DEV-502]** A missing golden file MUST fail with `harness.golden-missing`, except when update mode is on. `ilium plugin test --update` sets the variable.
- **[DEV-503]** The harness MUST NOT update golden files when the environment variable `CI` is set and non-empty, and MUST fail with `harness.golden-update-in-ci`.
- **[DEV-504]** A snapshot file MUST be pretty JSON with sorted keys and a trailing newline.

## Fixtures and expectation files

A fixture is JSON with `"schema": 1`. An unknown key at any depth MUST be refused with `fixture.unknown-key` and its JSON path, for example `fixture.unknown-key: $.events[2].foo`.

| Key | Type | Meaning |
|---|---|---|
| `schema` | integer | Always 1 |
| `grants` | string list | Grants for the host |
| `tier` | string | `T1`, `T2` or `T3` |
| `side` | string | `server`, `client` or `scene` |
| `clock` | string | Start time, `YYYY-MM-DD HH:mm:ss` UTC |
| `tree` | list | Nodes of the tree node shape in [Fake tree, panes, agents and jobs](#fake-tree-panes-agents-and-jobs): `id`, `parent`, `name`, `class`, `content_kind`, optional `agent` |
| `panes` | list | Pane info for terminal and basic panes |
| `agents` | list | Agent panes with `signature` and `activity` |
| `settings` | object | Current setting values |
| `storage` | object | Starting storage keys and values |
| `files` | object | Path to content, text or `{"base64": "..."}` |
| `http` | list | Fake network entries: `method`, `url`, `status`, `body` |
| `processes` | list | Fake process entries: `program`, `arguments`, `status`, `signal`, `stdout`, `stderr` |
| `events` | list | Events to deliver, `{"tag","val"}` |
| `completions` | list | Outcomes to deliver after the events |
| `lifecycle` | list | Optional. Steps `{"step": "trap" \| "advance" \| "restart" \| "grant" \| "revoke", "value": string \| number}`, run in order after the events. `trap` takes a trap code, `advance` milliseconds, `restart` base64 restore bytes, `grant` and `revoke` a grant name. |
| `expect_state` | string | Optional. One `LifecycleState` name in kebab-case that `lifecycle_state()` MUST return when the steps end |
| `frames` | list | Only for `side` `scene`: `FrameInput` records to draw, see [Scenes](#scenes) |

- **[DEV-510]** A fixture value MUST override the builder only when the builder did not set that value. The builder wins.
- **[DEV-511]** Fixture parsing MUST report the JSON path of the first bad key, for example `fixture.bad-value: $.tree[2].parent`.
- **[DEV-512]** Node ids in a fixture MUST be unique. A repeat MUST fail with `fixture.duplicate-id`.
- **[DEV-513]** Integers above 2^53 MUST be written as strings in a fixture, and the loader MUST read them as 64-bit values.

An expectation file has the same base name plus `.expect.json`.

```json
{
  "schema": 1,
  "submissions": { "match": "subset", "items": [ { "intent": "set-decoration", "slot": "work" } ] },
  "logs": { "match": "exact", "items": [ { "level": "info", "contains": "activate" } ] },
  "outcomes": { "match": "ordered", "items": [ "ok" ] }
}
```

| Match mode | Meaning |
|---|---|
| `exact` | Same count and same order, every given field equal |
| `ordered` | The given items appear in this order, other items may sit between |
| `subset` | Each given item appears at least once, order ignored |

- **[DEV-515]** In an expectation item, only the given fields are compared. Fields absent from the item are ignored.
- **[DEV-514]** A fixture with `side` `scene` MUST accept only the keys `schema`, `side`, `settings`, `clock`, `lifecycle`, `expect_state` and `frames`. Any other key MUST fail with `fixture.unknown-key`. `lifecycle` steps MUST run after the events. `expect_state` MUST fail the fixture with `fixture.state-mismatch` when the state differs.
- **[DEV-516]** `host.run_fixture(path)` MUST load the fixture, deliver the events, settle, and compare the expectation file when it exists. A missing expectation file means the run only asserts that nothing traps.
- **[DEV-517]** `ilium plugin test` MUST run every `tests/fixtures/*.json` as a test case named `fixture:<file>` and MUST compare the expectation file when one exists.

## Property tests and fuzz hooks

| Rust | TypeScript | Effect |
|---|---|---|
| `host.fuzz_events(seed: u64, count: u32) -> Vec<Event>` | `host.fuzzEvents(seed: bigint, count: number): Event[]` | A valid random event batch |
| `host.fuzz_run(seed: u64, rounds: u32) -> Result<(), FuzzFailure{ seed: u64, round: u32, code: String }>` | `host.fuzzRun(seed, rounds): void` | Delivers random batches. TypeScript throws on failure. |

- **[DEV-520]** The harness MUST offer `host.fuzz_events(seed, count)` which builds a valid random event batch from the event catalogue and grants. The same seed MUST give the same batch.
- **[DEV-521]** `host.fuzz_run(seed, rounds)` MUST deliver `rounds` random batches and fail on any trap that is not an expected trap. The failing seed and round MUST be in the message, so the failure can be replayed.
- **[DEV-522]** A property test MAY use `proptest` in Rust and `fast-check` in TypeScript. The SDK MUST export `arbitrary_event()` for Rust and `arbitraryEvent` for TypeScript, which generate valid events.
- **[DEV-523]** `ILIUM_PLUGIN_TEST_SEED` MUST make fuzz runs reproducible across machines.

## Coverage

- **[DEV-530]** `ilium plugin test --coverage` MUST write line coverage for the plugin source. In Rust it uses `cargo llvm-cov`, in TypeScript `bun test --coverage`. If the tool is missing the command MUST exit 4 with `toolchain.coverage-tool-missing`.
- **[DEV-531]** Coverage output MUST go to `.ilium/coverage/` as `lcov.info` and `summary.json`. The summary MUST list `lines_total`, `lines_covered` and `percent`.
- **[DEV-532]** `--min-coverage <percent>` MUST make the command exit 3 when coverage is lower, with the message `coverage 71.2 percent is below 80`.

## Replay files

A replay file records the inputs of one session so a bug can be reproduced.

```json
{"type":"header","version":1,"api":"ilium:plugin@0.1.0","plugin":"example.agent-tools","side":"server","seed":1,"start":"2030-01-01 00:00:00"}
{"type":"event-batch","at":0,"events":[{"tag":"pane-created","val":{"node":7,"kind":"terminal"}}]}
{"type":"completion","at":120,"request":3,"outcome":{"tag":"ok"}}
{"type":"tick","at":1000}
{"type":"host-call","at":130,"import":"storage.get","key":"count","result":{"tag":"some","val":"3"}}
```

| Rust | TypeScript |
|---|---|
| `host.record_to(path: &Path)` | `host.recordTo(path)` |
| `host.replay(path: &Path) -> Result<(), ReplayMismatch{ line: u32, expected: String, actual: String }>` | `host.replay(path): Promise<void>`, rejects with `ReplayMismatch` |

- **[DEV-540]** `host.record_to(path)` MUST write a replay file. The header MUST be the first line.
- **[DEV-541]** `host.replay(path)` MUST re-run the recorded inputs and compare the new host calls with the recorded `host-call` lines. A difference MUST fail with `replay.diverged` and the first differing line number.
- **[DEV-542]** A replay file with a version other than 1 MUST be refused with `replay.version-unsupported`.
- **[DEV-543]** Time values `at` are fake milliseconds from `start`.
- **[DEV-544]** `ilium plugin test --replay <file>` MUST replay one file and exit 0 on a match, 3 on divergence.

## Test report records

`ilium plugin test` writes the records defined in [Plugin command-line reference](/pages/reference-plugin-cli/). The harness itself reports each native test through the test runner of the language, and the table below applies to the records of a fixture run.

`ilium plugin test` prints JSONL records when stdout is not a terminal or `--json` is given. Otherwise it prints one line per test and the final line `summary: <passed> passed, <failed> failed, <skipped> skipped (<ms> ms)`.

| Record `type` | Fields |
|---|---|
| `test-case` | `name`, `status` (`pass`, `fail`, `trap`, `skipped`), `duration_ms`, `reason` (required when `status` is `skipped`, present for `fail` and `trap`, absent for `pass`) |
| `summary` | `passed`, `failed`, `skipped`, `duration_ms` |

- **[DEV-550]** The test names in records MUST be stable between runs.
- **[DEV-551]** A fixture with no expectation file MUST count as passed and print the warning `test.no-expectation`.
- **[DEV-552]** `ilium plugin test` MUST print JSONL records when stdout is not a terminal or `--json` is given. Otherwise it MUST print one line per test and the final line `summary: <passed> passed, <failed> failed, <skipped> skipped (<ms> ms)`.

## Scenes

A scene is drawn by a renderer, not by a view. `FakeScene` drives one renderer of the `anim` world.

| Rust | TypeScript | Effect |
|---|---|---|
| `FakeScene::new::<R: SceneRenderer>(scene_id: &str, columns: u16, rows: u16) -> FakeScene` | `createFakeScene(renderer, sceneId, columns, rows): FakeScene` | A scene of the given size on the fake clock |
| `scene.frame(input: FrameInput) -> Screen` | `scene.frame(input): Screen` | Calls `render` once and reads the surface as a `Screen` |
| `scene.set_palette(palette_rgb: Vec<u32>)` | `scene.setPalette(paletteRgb)` | Calls `set-palette` |

`FrameInput` is the `frame-input` record of `ilium:anim/renderer-types`: `time_millis`, `wall_millis`, `now_unix_millis`, `width`, `height` and `palette_rgb`. `FrameInput::default()` takes `width` and `height` from the scene size and the time values from the fake clock.

- **[DEV-560]** `FakeScene::frame` MUST call `render` of the renderer once and return the surface cells as a `Screen` of the scene size.
- **[DEV-561]** The fake clock MUST supply `time_millis` as the fake milliseconds since the scene was created, so equal inputs give equal screens.
- **[DEV-562]** `set_palette` MUST pass the palette to the renderer before the next frame.

## Harness types

| Type | Definition |
|---|---|
| `Mode` | enum `Native`, `Component` (TypeScript `'native'`, `'component'`) |
| `Screen` | `columns: u16`, `rows: u16`, `cell(row, column) -> Cell`, `text() -> String` (rows joined by `\n`, trailing spaces trimmed) |
| `Cell` | `symbol: String`, `foreground: Color`, `background: Color`, `bold: bool`, `italic: bool`, `underline: bool` |
| `Submission` | `intent: Intent`, `correlation: u64`, `dedup_key: Option<String>`, `delivery: Delivery` |
| `PluginHandle` | `side: String`, `instance: u64` |
| `Invocation` | `name: String`, `arguments: Vec<String>`, plus `stdout()`, `stderr()`, `exit_code()` |
| `Diagnostic` | `code: String`, `message: String` |
| `Restart` | `at_ms: u64`, `delay_ms: u32` |

- **[DEV-563]** Every type of this table MUST exist in both SDK `testing` modules with the field names above, camelCase in TypeScript.

## Edge cases

- A test that activates twice on one host MUST fail with `harness.already-active`.
- `deliver` before `activate` MUST fail with `harness.not-active`.
- A fixture with `side: client` and a `deliver` of server-only events MUST fail with `harness.event-not-for-side`.
- A plugin that holds the wrong grant for an event still receives nothing, and `deliver` MUST NOT fail. It MUST record the diagnostic `events-not-granted` in `host.diagnostics()`.
- Two fakes asked for the same path in `files` and `fs().write` MUST give the write precedence.

## Complete error code list

Every dotted code named on this page and on the conformance page is in this table or in the trap code table of Errors and limits. `ilium plugin test` also reports the codes of the command-line reference.

| Code | Meaning |
|---|---|
| `harness.setup-closed` | Builder call after activation |
| `harness.grant-not-declared` | Grant not in the manifest |
| `harness.grant-unknown` | Grant name not in the catalogue |
| `harness.batch-too-large` | More than 256 events in one `deliver` |
| `harness.settle-limit` | Queue not empty after 100 rounds |
| `harness.cell-out-of-range` | `cell` outside the screen |
| `harness.render-size` | Render size outside 10 to 500 by 1 to 200 |
| `harness.clock-backwards` | `set_clock` moved time back |
| `harness.golden-missing` | Golden file absent |
| `harness.golden-update-in-ci` | Update requested when `CI` is set |
| `harness.already-active` | Second activation |
| `harness.not-active` | Call before activation |
| `harness.error-unknown` | Unknown code in `inject_failure` |
| `harness.event-not-for-side` | Event does not belong to the side |
| `harness.unexpected-call` | A fake process or network call had no expectation. Reported at teardown. |
| `harness.fs-path-invalid` | Fake file system path is absolute or contains `..` |
| `harness.view-target-unknown` | `render` target is not one of the five `ViewTarget` cases |
| `harness.clock-argument` | `advanceClock` argument is negative, fractional or not a safe integer |
| `harness.reason-reserved` | `deactivate(crash-restart)` was requested |
| `harness.core-unknown` | `install_core` name is not a core plugin |
| `harness.plugin-unknown` | `session.server` or `session.client` names a plugin or side that is not installed |
| `harness.not-a-hook-handler` | `run_hook` on a plugin without a hook handler |
| `harness.snapshot-invalid` | Storage snapshot file has an unknown shape |
| `harness.grant-unknown`, `harness.grant-not-declared` | See Setup |
| `fixture.unknown-key`, `fixture.bad-value`, `fixture.duplicate-id`, `fixture.state-mismatch` | Fixture errors |
| `replay.diverged`, `replay.version-unsupported` | Replay errors |
| `toolchain.coverage-tool-missing` | `--coverage` needs `cargo llvm-cov` or `bun test --coverage` |
| `test.no-expectation` | Warning: a fixture has no expectation file |
| `trap.deadline`, `trap.memory`, `trap.table`, `trap.submission-batch`, `trap.invalid-ui-tree`, `trap.abi`, `trap.panic` | Trap codes (a never-requested capability traps with `host.capability-not-requested`). The table in [Errors and limits](/pages/reference-errors-and-limits/#trap-codes) owns them. |

## See also

- [Testing plugins](/pages/testing/)
- [Conformance suite](/pages/reference-conformance-suite/)
- [Debugging tools](/pages/reference-debugging-tools/)
- [Plugin command-line reference](/pages/reference-plugin-cli/)
- [Events and intents](/pages/reference-events-and-intents/)
- [Errors and limits](/pages/reference-errors-and-limits/)

API checked against `ilium:plugin@0.1.0`.
