---
layout: page
title: Runtime model
permalink: /pages/reference-runtime-model/
---

## Runtime model

This page describes what the host guarantees while it runs a plugin component: which threads exist, how a call is interrupted, in what order events arrive, how memory and time are bounded, which WASI interfaces a component may link, and what happens when a plugin fails. The [WIT reference](/pages/reference-wit/) lists the functions. This page lists the rules that the functions rely on. Every rule has an id of the form [ABI-NNN], and the [requirements page](/pages/reference-requirements-wit-abi/) repeats each id with its test.

### What you need

- **The WIT reference.** Names such as `on-events`, `request-completed` and `take-response` are defined in the [WIT reference](/pages/reference-wit/).
- **A running plugin or the test harness.** The [testing page](/pages/testing/) describes the fake host. The fake host follows the same rules as this page.
- **The limits page.** Numeric limits that this page quotes are owned by [Errors and limits](/pages/reference-errors-and-limits/) and [Performance](/pages/performance/). This page repeats a value only where the rule needs it.

## 1. Instances and threads

- A plugin has one component per side (server, client, scene). The host creates **one instance of the component per plugin per side**. A plugin that has a server part and a client part therefore has two instances, in two processes, that cannot share memory.
- An instance is **single-threaded**. A component has no threads and no `wasi:threads`. The host never calls two exports of one instance at the same time [ABI-025] [ABI-049].
- A job (`job-runner.run`) runs in its own instance, created for the job and dropped after it. A job instance is not activated and has no event handler. It shares nothing with the main instance of the plugin except through `storage`, `fs` and messages.
- Every export call runs to completion on the host thread that owns the instance. The host does not preempt it between instructions. It interrupts it by the epoch mechanism in section 2.
- An instance lives from `activate` until `deactivate`, a trap, or a restart. All its memory is lost at that point. The only state that survives is what the plugin wrote to `storage`, `fs`, settings or the bytes that `deactivate` returned.

<figure>
<pre class="term">
╭  ≡ ● · One instance per side─────────────────────╮
│ server  acme.git-status  instance 3  active      │
│ client  acme.git-status  instance 1  active      │
│ job     acme.git-status  parse-report  running   │
╰──────────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. Two instances of one plugin, plus a job instance. Instance numbers count activations of one plugin on one side.</figcaption>
</figure>

## 2. Calls, deadlines and interruption

The host runs every export under a deadline. It enforces the deadline by **epoch interruption**: the engine checks a counter at function entries and loop back-edges, and a host ticker thread advances that counter every **10 ms**. The host sets the instance deadline to `ceil(d / 10) + 1` ticks after the call starts. A guest that spins therefore traps no earlier than `d` and no later than `d + 20` ms of wall time on a host that is not overloaded, and a call that returns at `d - 1` ms never traps [ABI-026] [ABI-042].

- A deadline is wall time, from the start of the call to its return. Host work that runs inside a synchronous import counts, because the plugin waits for it.
- A hard deadline breach traps the instance. The trap counts as a failure (section 12), except a `deactivate` breach, which never counts [ABI-044]. A soft deadline breach discards the late result and traps only after the streak or grace in the scheduling table. A job deadline breach does not trap the main instance: the job completes as `failed(timed-out)`.
- The deadline of each export is a **per-export deadline**. A call of one export does not borrow time from another. The single normative table of call classes, kinds and effects is [Scheduling, deadlines and cancellation](/pages/reference-host-scheduling/#call-classes). The table below repeats only the deadlines for orientation and always yields to it. A plugin may ask for another value within its tier ceiling with the manifest `limits` block.

| Export | Default deadline | Ceiling, T1 | Ceiling, T2 and T3 | When breached |
|---|---|---|---|---|
| `lifecycle.activate` | 2,000 ms | 5,000 ms | 2,000 ms | Trap, activation fails |
| `lifecycle.deactivate` | 500 ms | 1,000 ms | 500 ms | Instance dropped, no restore bytes |
| `handler.on-events` | 20 ms | 50 ms | 20 ms | Trap |
| `views.render` | 10 ms | 20 ms | 10 ms | Trap |
| `annotator.annotate` | 10 ms | 20 ms | 10 ms | Soft: annotation skipped; the third breach in a row traps |
| `catalog-provider`, `menu-provider`, `virtual-rows-provider` | 20 ms | 20 ms | 20 ms | Soft: static fallback |
| `token-provider.tokens` | 10 ms | 10 ms | 10 ms | Soft: lines stay plain |
| `settings-validate.settings-validate` | 100 ms | 100 ms | 100 ms | Soft: the save is refused and the previous value kept |
| `job-runner.run` (CPU time) | 2,000 ms | 60,000 ms | 10,000 ms | `failed(timed-out)` |
| `renderer.render` | frame budget, at most 50 ms | 50 ms | 50 ms | Soft: frame skipped; 30 in a row trap |

The CPU deadline of a job counts CPU time only. A job that waits in `jobs.wait-request` spends no CPU budget, but its wall time counts against `job_wall_ms` (20,000 ms by default).

`deactivate` may return at most 1 MiB. A larger return is dropped, one warn line is logged, and the next activation gets no `restored` value. It does not count as a failure [ABI-044].

`views.render`, `annotate` and `settings-validate` are **pure** calls. An effectful import (any import that submits an intent, starts a request, writes storage or starts a timer) called inside them returns `err(not-permitted)` with the message `effectful import in a pure call`, and the call continues. Read-only imports such as `ui.display-width`, `clock` and `tree` reads stay allowed [ABI-048].

**Worked example.** A handler that decodes a 2 MiB image inside `on-events` spends 31 ms. The deadline is 3 ticks, so the call is interrupted between 20 and 40 ms. The host logs `deadline breach in on-events: limit 20 ms`, traps the instance, applies none of the submissions of that batch, and schedules a restart (section 12).

## 3. Re-entrancy

The host never calls an export of an instance while another export of the same instance is on the stack [ABI-049]. A host import therefore never runs plugin code of the caller in the middle of the call, with two documented exceptions that stay on the same stack and do not re-enter the instance:

- `services.call` to a service that the calling plugin provides itself is **queued**. The provider export `handle` runs after the current call returns, as its own call [ABI-050].
- `extensions.run-hook` on a point that the calling handler is running returns `reentrant`. A chain on another point is allowed to depth 3, and a deeper chain returns `reentrant`.

An import that completes work later (every function with a `take-*` partner) never calls back. It delivers `request-completed` in a later batch.

**Edge case.** A plugin submits `invoke-command` for its own command from inside `on-events`. The host does not run the command at once. It queues `command-invoked` and delivers it in a later batch, in order [ABI-050].

## 4. Event delivery

An event is a fact that the host reports. The host delivers events to `handler.on-events` in batches.

- A batch holds **at most 256 events**. A longer backlog is split into several batches, in order [ABI-045].
- Each plugin side has **one queue**, filled in the order of host emission across all event kinds. The queue holds **1,024 events by default**. The manifest key `event_queue` asks for another size. The ceiling is **4,096** for T1 and **1,024** for T2 and T3 [ABI-045].
- Order is kept per node: `pane-created` first, then other pane events, then `pane-exited`, then `node-removed`. `agent-identified` precedes `agent-activity-changed` for the same pane.
- A plugin that is not active receives nothing. Events for an inactive plugin are not queued. Events produced while the plugin is activating are queued and arrive in the first batch after `activate` returns `ok`. If `activate` fails, they are discarded without an `events-dropped` report [ABI-046].
- After every activation of a plugin that holds `tree.read`, the first batch starts with exactly one `tree-changed` event that lists every node in tree pre-order. Without `tree.read` nothing is sent [ABI-047].

Each event case has one of three **delivery policies**. The policy of every case is listed in the [event matrix](/pages/reference-host-conformance/#4-event-matrix).

| Policy | What the host does when the queue is full | Counted in `events-dropped` |
|---|---|---|
| Coalesced | Always merges an event with the pending event of the same kind and key (node, buffer, timer, job, watch, pane or signal), whatever the queue fill. Only the newest state stays, at the position of its newest occurrence. Pending `tree-changed` lists merge into one union. | No |
| Drop oldest, counted | Removes the oldest event of that kind from the head of the queue. | Yes |
| Never dropped | Keeps the event. If the queue is full, no drop-oldest event is pending and such an event arrives, the plugin is treated as stuck: the host restarts the instance as a failure, and the new instance receives `tree-changed` for every node. | Not applicable |

A full queue admits a drop-oldest event by evicting the oldest queued drop-oldest event of any kind, and counts it [ABI-033]. The next batch after a drop starts with one `events-dropped` event. It carries a list of `dropped-count { event, source, count }` records, one per event name and source. `events-dropped` is coalesced by (event, source) and never dropped, and a new count merges into the pending record [ABI-036].

**Worked example.** A plugin is busy for 400 ms in a long job callback, and 2,000 `screen-changed` events for one pane arrive. They share one coalescing key (the pane), so the queue holds one event with the newest revision. `agent-activity-changed` is never dropped. A plugin that also received 1,500 `output-matched` events (drop-oldest) with the default queue of 1,024 loses the oldest 477 of them, because one slot holds the coalesced `screen-changed` event. Its next batch starts with `events-dropped([{ event: "output-matched", source: none, count: 477 }])`.

## 5. Submissions

A handler returns submissions. The host applies them in order after `on-events` returns.

- **More than 256 submissions trap the call.** This is not a refusal: the call counts as one failure, **none** of the submissions is applied, and none of them completes [ABI-008].
- Two **batch-level checks** run over the whole list, in index order, before any effect: shape (wrong shape, or a missing dedup key on `send-input` or `agent-action`), then grant. Inside one submission the shape check wins over the grant check [ABI-031].
- Every other check (target exists, pane ready, value valid, rate, quota, conflict, `if-current`) is **per submission** and affects only that submission [ABI-032].
- The lowest-index offender completes as `refused(invalid-argument)` (shape) or `refused(not-permitted)` (grant). Every other submission completes as `refused(cancelled)` with the message `batch refused: submission <index>`, where `<index>` is the 0-based position of the offending submission [ABI-009].
- A refused submission does not reserve its dedup key.
- A dedup key has 1 to 128 characters from `[A-Za-z0-9._:/-]`. It is scoped to the plugin and reserved while `now_wall_ms - accepted_wall_ms < 600,000`. The accepted time is persisted in the journal on the wall clock [ABI-010] [ABI-034].
- A repeat completes as `duplicate(request-ref)`, where `request-ref` is the ref of the first submission. The outcome makes no claim about the result of the original, and the host does not check that correlation numbers are unique [ABI-035].
- `correlation` is chosen by the plugin. Values below 2^63 belong to the plugin. Values from 2^63 up belong to the SDK [ABI-011].

The `intents.submit` import is for use outside a batch. It carries one submission, queues it, and answers `request-completed` with the request ref `submission(correlation)`; the value returned by `submit` is the `request-id` of the call.

## 6. Slow calls and results

A function whose result type is `result<request-id, plugin-error>` is a slow call. It returns at once with a request id (or a synchronous error) and completes later.

1. The call returns `ok(request-id)`. A synchronous refusal returns an error instead: `not-permitted`, `invalid-argument`, `busy` and similar.
2. Later, an event `request-completed` arrives with `request-ref call(request-id)` and an `outcome`.
3. On `ok`, the plugin calls the matching `take-*` function once.

A result is **held for 30 seconds**, or until the first take [ABI-012]. The hold starts when `on-events` returns for the batch that delivered the `request-completed` event, not when the host emitted it [ABI-037]. A take before completion, a second take, a take after the hold and a take of an unknown id all return `none`. The SDK wrappers hide this: they return the result or fail with `not-found` [ABI-014]. A `request-id` is unique per plugin side for the whole server run and is never reused, also across restarts [ABI-040].

A plugin has at most 64 pending requests by default (ceiling 256 for T1), and a session at most 4,096. The call that exceeds a limit returns `busy` at once [ABI-013]. A request counts from the return of its id to the enqueue of its completion. Refused submissions and synchronous calls do not count [ABI-041].

When a plugin is disabled or shut down, its pending requests are cancelled. After a trap, the surviving events are redelivered once at the head of the new instance's queue, completions of submissions are delivered to the new instance, and completions of `call(id)` requests are discarded [ABI-038]. When a grant is revoked, requests that need it complete as `failed(revoked)` before `deactivate(revoked)`; the other pending `call(id)` requests are cancelled with the instance [ABI-043].

**Rust**

```rust
use ilium_plugin_sdk::host::http::{self, HttpRequest};

/// Fetches a page and returns the body. The wrapper hides request-completed and take-response.
pub async fn fetch_notes(url: &str) -> Option<Vec<u8>> {
    let request = HttpRequest {
        method: "GET".to_string(),
        url: url.to_string(),
        headers: vec![],
        body: None,
        timeout_millis: 5_000,
    };
    match http::fetch(request).await {
        Ok(response) if response.status == 200 => Some(response.body),
        _ => None,
    }
}
```

**TypeScript**

```ts
import { fetch } from '@ilium/plugin-sdk/host/http';

/** Fetches a page and returns the body. The wrapper hides request-completed and take-response. */
export async function fetchNotes(url: string): Promise<Uint8Array> {
  const response = await fetch({ method: 'GET', url, headers: [], body: undefined, timeoutMillis: 5000 });
  if (response.status !== 200) throw new Error('unexpected status');
  return response.body;
}
```

## 7. Memory

Each instance has a memory cap, and the session has a pool for all instances.

| Budget | Default | Ceiling, T1 | Ceiling, T2 and T3 | When exceeded |
|---|---|---|---|---|
| `memory_mib` (one instance) | 32 | 256 | 128 | Trap, counts as a failure |
| Session memory pool (all instances) | 1,024 MiB | none; a user may only lower it (HST-406) | same, no per-tier ceiling | The new instance is refused with `session.memory-exceeded`. Not counted as a failure. |
| `job_memory_mib` (one job instance) | 64 | 512 | 256 | `failed(quota-exceeded)` |

The server total of 1,024 MiB is the default of the pool. At activation an instance charges its effective `memory_mib` to the pool, and a sandbox helper charges the sum of its hosted instances' `memory_mib` plus 32 MiB [HST-164]. Growth below the limit never fails on the pool. An instance that grows past its cap traps at the growth call; the guest never sees `-1` from `memory.grow` [ABI-027]. A refusal because the pool is full is logged as `host-capacity` and the plugin can be started again when memory is free.

## 8. JSON in strings

A field or parameter whose name ends in `-json` carries UTF-8 JSON text. The host and the SDKs apply one rule to all of them [ABI-016].

- **Grammar.** RFC 8259. A document that is not valid JSON is `invalid-argument`. A byte order mark, comments, trailing commas and `NaN` or `Infinity` are not allowed.
- **Depth.** At most 64 levels of nesting. A deeper document is `invalid-argument`.
- **Size.** The limit of the field applies. The default is 65,536 bytes for a command argument or a topic payload, 1 MiB for a service call or reply, and 256 KiB for settings (all values of one plugin together). A larger document is `invalid-argument`, or `quota-exceeded` where the field's page says so.
- **Numbers.** A JSON number is read as a 64-bit float. An integer beyond 2^53 loses exactness, so a value that may exceed 2^53 MUST be sent as a string. Ids (`node-id`, `request-id`, `buffer-id`) are `u64` and are written as JSON strings in every `*-json` field. A `bigint` in TypeScript is written as a string by the SDK.
- **Keys.** Duplicate keys in one object are `invalid-argument`. Key order is not significant.
- **Strings.** The text is UTF-8. An unpaired surrogate escape such as `\ud800` is `invalid-argument`.

**Edge case.** A service payload `{"id": 9007199254740993}` is read by the host as 9007199254740992. The plugin that needs the exact value sends `{"id": "9007199254740993"}`.

## 9. Strings and text at the boundary

All `string` values are UTF-8. A string that is not valid UTF-8 never reaches the host or the plugin: the canonical ABI traps at the boundary, and the call counts as a failure [ABI-017]. A limit in characters counts Unicode scalar values and a limit in bytes counts UTF-8 bytes. A terminal width counts cells, as given by `ui.display-width`.

## 10. WASI subset

A component links only the WASI interfaces below. Any other import is refused at load.

| WASI interface | Allowed | Notes |
|---|---|---|
| `wasi:cli` stdout and stderr | Yes | Written to the plugin log at `info` and `warn`. |
| `wasi:cli` environment, arguments, exit | Yes | The environment and arguments are empty. `exit` traps the instance. |
| `wasi:clocks` | Yes | Wall and monotonic clocks, see section 11. |
| `wasi:random` | Yes | Secure random bytes from the host, see section 11. |
| `wasi:io` | Yes | Streams and polling, for the interfaces above only. |
| `wasi:filesystem` | No | Use `ilium:host/fs`. |
| `wasi:sockets` | No | Refused at load. Use `ilium:host/http`. |
| `wasi:http` | No | Refused at load. Use `ilium:host/http`. |

A component that imports `wasi:sockets` or `wasi:http` is refused at load [ABI-024]. The check runs when the package is installed and again when the instance is created.

## 11. Time and randomness

- **Wall clock.** `ilium:host/clock.now-unix-millis` and `wasi:clocks/wall-clock` read the same host clock. It can jump when the user changes the time.
- **Monotonic clock.** `clock.monotonic-millis` and `wasi:clocks/monotonic-clock` read one host source. It never goes backwards inside the host. Values from two instances are not comparable.
- **Randomness.** `wasi:random` returns bytes from the operating system generator. A plugin MUST NOT depend on a fixed sequence.
- **Timers.** A volatile timer fires `timer-fired` in a later batch, never inside the call that set it. A durable timer fires `durable-timer-fired` and reports how late it is.

## 12. Failures and restart

A **failure** is a trap (deadline, memory, illegal instruction, invalid UTF-8, more than 256 submissions), an error from `activate`, or a stuck queue. A failure ends the instance and schedules a restart. The back-off is:

| Failure inside 10 minutes | Wait before the restart |
|---|---|
| First | 1 s |
| Second | 5 s |
| Third | Not restarted. The plugin is disabled on that side until the user turns it on again, except an essential core plugin, which restarts after 5 s (LIF-042). In a development build the third and later failures restart after 5 s instead (LIF-226). |

A restart that follows a 10-minute quiet period starts the count again at the first failure. A job failure never counts. `host-capacity` refusals never count. The `deactivation-reason` `crash-restart` is reserved and never delivered, because a crashed instance cannot be called [ABI-028].

The new instance gets `restored: none` in `activation` after a trap, because `deactivate` did not run. A restart after a clean `deactivate` (for example an upgrade) passes the bytes that `deactivate` returned.

## 13. Link-time grant enforcement

The host links imports **per instance**, from the grants of the manifest. An interface whose capability is not granted is not linked, or is linked to a stub:

- A component that imports a function of a capability its manifest never requested traps when it first calls the function, with `host.capability-not-requested`. It counts as a failure.
- A function whose grant the user declined returns `not-permitted`. The plugin keeps running.
- A grant withdrawn during a call returns `err(revoked)` to that call; later calls that need it return `err(not-permitted)` with reason `revoked`. Pending requests follow the rule in section 6 [ABI-043]. The plugin is re-activated with the reduced grants.
- A component whose `exports` differ from the required exports plus the manifest list is refused at install with `manifest.exports-mismatch` [ABI-023].

## 14. Determinism in tests

The fake host in the test harness follows this page. It fakes the clock, the file system, processes, the network, storage and settings. A test sets the clock, delivers batches in a chosen order, and checks the submissions that come back. A fake is deterministic: the same input gives the same result. A call to a fake process or network that a test did not declare fails the test.

- The fake clock starts at a fixed instant and moves only when the test advances it. The epoch ticker is not simulated: a test sets the deadline outcome with `host.breach_deadline(export)`, which applies the effect of the call class in the scheduling table, and changes grants with `host.grants_changed(list)`, which follows section 13.
- The fake queue enforces the same batch size, policies and queue sizes as the host.
- `fuzz_run(seed, rounds)` delivers random valid batches. A failure prints the seed.

## See also

- [WIT reference](/pages/reference-wit/)
- [Host conformance matrix](/pages/reference-host-conformance/)
- [ABI evolution](/pages/reference-abi-evolution/)
- [Requests and outcomes](/pages/requests-and-outcomes/)
- [Performance](/pages/performance/)
- [Errors and limits](/pages/reference-errors-and-limits/)
- [Requirements: WIT and ABI](/pages/reference-requirements-wit-abi/)


### Unknown members in host JSON payloads

- [ABI-081] A host and an SDK MUST ignore unknown object members when they read a host-defined JSON payload, and the host MUST evolve such a payload within one ABI major only by adding optional members, never by removing, renaming or changing the type of a member.

## Production additions: T3-abi-api

- [ABI-100] A canonical-ABI value that is invalid by construction (a `bool` byte other than 0 or 1, a `char` that is a surrogate or above U+10FFFF, an enum or variant discriminant with no case, an option or result tag above 1, a flags value with an undefined bit) MUST trap the call at the boundary in both directions, MUST count as one failure, and MUST log a record with code `abi.invalid-value` (proposed code name, owner to confirm) and the type name. Test: pass each kind of invalid value in each direction and assert a trap, one counted failure and one log record that names the type.
- [ABI-101] The host MUST accept an `f32` or `f64` NaN from a plugin only where the field is documented as a float that may be NaN; elsewhere it MUST refuse the value with `invalid-argument`, and it MUST NEVER send a NaN or infinity to a plugin in a host-produced field. Test: send NaN to a percent field and to a field documented to accept NaN, and assert `invalid-argument` in the first case and acceptance in the second; read every host-produced float field in a fixture and assert that none is NaN or infinite.
- [ABI-102] A plugin component MUST export `cabi_realloc` as required by the canonical ABI; the host MUST call it only during a host-to-plugin call, MUST bound the total bytes it requests in one call by the call's memory limit, and MUST trap the call with code `abi.realloc-failed` (proposed code name, owner to confirm) when it returns 0 or an out-of-range pointer or an unaligned pointer. Test: use fixtures whose `cabi_realloc` returns 0, an out-of-range pointer or an unaligned pointer, and assert that each traps with `abi.realloc-failed`; assert that realloc is called only inside a host-to-plugin call.
- [ABI-105] The host MUST limit the number of host-import calls in one export call to 100,000 by default (proposed limit, owner to confirm); the call that exceeds it MUST return `err(busy)` with reason `import-calls-exceeded` to the import and every later import in that export call, and MUST NOT trap. Test: run a loop that calls a cheap import 100,001 times and assert that the 100,001st call and each later call return `busy` with `import-calls-exceeded`, and that the export call does not trap.
- [ABI-114] A host import MUST NOT block for longer than 50 ms (proposed limit, owner to confirm) of wall time unless the page names it as a slow call that returns a `request-id`; an import that exceeds 50 ms MUST be logged as `abi.slow-import` with its name and duration. Test: a fixture host import that sleeps 60 ms and is not a slow call must log `abi.slow-import` with its name and duration, and a slow call must return a `request-id`.
