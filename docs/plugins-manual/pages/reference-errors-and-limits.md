---
layout: page
title: Error codes and limits
permalink: /pages/reference-errors-and-limits/
---

## Error codes and limits

A plugin calls host functions, and some of those calls fail. A failed call returns a `plugin-error` record to the caller, and the plugin keeps running. A contract violation is different: it traps the plugin instance, which the host then ends and restarts. Resource limits bound how much CPU time, memory, storage, I/O and queue space a plugin can use. Each limit has a default. A manifest may ask for a lower value or a higher one, up to the ceiling for the plugin's tier.

Tiers decide the ceilings. T1 is first-party bundled plugins. T2 is a third-party package signed by a publisher key the user trusts. T3 is an unsigned local package, installed with `--allow-unsigned` or `ilium plugin dev` and shown with a `dev` badge. T2 and T3 share the same ceilings.

### What you need

- **Grants:** none for this page. Each call needs the grant of its own capability. A declined grant, and any call made after a revocation, returns `not-permitted`; only a call already in flight when the grant is revoked returns `revoked`. Storage is always granted.
- **Language:** Rust with `ilium_plugin_sdk`, or TypeScript with `@ilium/plugin-sdk`. Both SDKs expose the same record and codes.
- **Prerequisites:** the [Rust SDK reference](/pages/rust-sdk/) or the [TypeScript SDK reference](/pages/typescript-sdk/) for handler rules, and the [Performance guide](/pages/performance/) for how deadlines are charged.

### The plugin error record

Every fallible host function returns `result<T, plugin-error>`. The record has three fields.

| Field | Rust | TypeScript | Meaning |
|---|---|---|---|
| `code` | `error.code` (`ErrorCode`) | `error.code` (string) | One of the codes in the table below |
| `message` | `error.message` (`String`) | `error.message` | Text for logs. Do not match on it |
| `retry-after-ms` | `error.retry_after_ms` (`Option<u32>`) | `error.retryAfterMs` | Set for `rate-limited`; the CPU budget case also sets it |

The SDKs map the result to `Result<T, PluginError>` in Rust and to a return value or a thrown `PluginError` in TypeScript. The WIT surface itself has only results and traps.

| Code | Meaning | Retry |
|---|---|---|
| `not-permitted` | The capability is missing or declined, or was revoked before the call started, the argument is outside the grant, a path escapes its scope, or a policy forbids the action | No |
| `not-found` | The node, pane, buffer, file, command, plugin, provider, call or device does not exist any more | No |
| `invalid-argument` | The argument breaks a rule the plugin can check before the call: syntax, range, a per-call cap | No, fix the call |
| `conflict` | State changed or is held: a stale revision, a manual title, a held lock, an existing file for `create-new` | Yes, after reading the state again |
| `quota-exceeded` | A stored or held quota is full, or the CPU budget of the execution quota group is spent | After freeing resources, or after `retry-after-ms` |
| `rate-limited` | A rate rule throttles the call | Yes, after `retry-after-ms` |
| `busy` | Transient capacity is full: pending requests, the job queue, an open picker, a device in use | Yes, later |
| `timed-out` | A deadline passed: lock wait, HTTP, process, job time or a reply from a service or command | Yes |
| `cancelled` | Cancelled by the plugin, the user, a shutdown, or a refused batch | Maybe |
| `unsupported` | The host or platform cannot do it, such as a codec, a channel count or an unknown action, or the call is made in the wrong instance kind | No |
| `pane-not-ready` | A restored terminal pane has not started its process yet | Yes, after `pane-created` or `tree-changed` |
| `revoked` | A call already in flight when the grant was revoked (the authorization epoch changed during the call). The plugin is re-activated | No |
| `io` | An operating system I/O error other than a missing file | Maybe |
| `network` | DNS, connect, TLS or transport failure | Yes |
| `internal` | A host bug, or a trap inside a job instance | No |

Every function that returns a result can return `not-permitted`, `revoked` and `internal`. A function that returns a `request-id` can also return `busy`, and `quota-exceeded` when the CPU budget is spent. Other codes are listed per function. For example, `storage.put` returns `invalid-argument` for an empty key, a key over 256 bytes or a value over 1 MiB, and `quota-exceeded` when the bucket or the plugin's own `disk_mib` limit is full. A physically full disk returns `io` with the message `no-space`, never `quota-exceeded`.

Calls that return a `request-id` split their errors in two. Grant, scope, syntax and per-call cap checks fail at once, in the call. Disk, network, process and user failures arrive later in the completion as `failed(code)`. Check the completion, not only the call.

### Handling errors in Rust

The example stores a cursor in the session bucket. A full bucket skips the save. A busy host asks the caller to retry. Any other error is passed up, because it means a bug in the plugin or a missing grant.

**Rust**

```rust
use ilium_plugin_sdk::host::storage::{self, Bucket};
use ilium_plugin_sdk::prelude::*;

const CURSOR_KEY: &str = "cursor";

#[derive(Debug, PartialEq, Eq)]
pub enum SaveOutcome {
    Saved,
    SkippedFull,
    RetryLater,
}

pub fn save_cursor(cursor: &[u8]) -> Result<SaveOutcome, PluginError> {
    match storage::put(Bucket::Session, CURSOR_KEY, cursor) {
        Ok(()) => Ok(SaveOutcome::Saved),
        Err(error) => match error.code {
            ErrorCode::QuotaExceeded => Ok(SaveOutcome::SkippedFull),
            ErrorCode::Busy => Ok(SaveOutcome::RetryLater),
            _ => Err(error),
        },
    }
}
```

### Handling errors in TypeScript

The TypeScript version applies the same rules. A thrown `PluginError` is checked with `instanceof`, and `error.code` is a string union of the kebab-case codes. Errors that the plugin cannot fix are rethrown, so the restart rule sees them.

**TypeScript**

```ts
import { PluginError } from '@ilium/plugin-sdk';
import * as storage from '@ilium/plugin-sdk/host/storage';

const CURSOR_KEY = 'cursor';

export type SaveOutcome = 'saved' | 'skipped-full' | 'retry-later';

export function saveCursor(cursor: Uint8Array): SaveOutcome {
  try {
    storage.put('session', CURSOR_KEY, cursor);
    return 'saved';
  } catch (error) {
    if (!(error instanceof PluginError)) {
      throw error;
    }
    switch (error.code) {
      case 'quota-exceeded':
        return 'skipped-full';
      case 'busy':
        return 'retry-later';
      default:
        // invalid-argument, not-permitted and the rest are plugin bugs or missing grants.
        throw error;
    }
  }
}
```

### Traps

An author MUST treat a limit as a trap and not as a warning. [AUT-111]

A trap ends the instance. The plugin is restarted under the restart rule below. The plugin cannot catch a trap. A trap is raised for these cases, because each one is a plugin bug:

- Calling a function of a capability the manifest never requested, in an interface that the plugin linked because it requested another capability of it. An interface that no requested capability unlocks is refused at install with `manifest.import-not-requested` and never links. A function of a requested capability that the user declined is not a trap: it returns `not-permitted`. See [Host runtime](/pages/reference-host-runtime/).
- An invalid `ui-tree`: an index out of range, a graph that is not a tree, a duplicate `key` in one tree, more nodes than the effective `ui_tree_nodes` limit (4,096 by default), or a `layout` whose child count differs from its constraint count.
- Returning more than 256 submissions from one `on-events` call.
- A hard deadline breach in `activate`, `on-events` or `views.render`. A breach in `deactivate` is not a trap: the instance is dropped and no restore bytes are kept. A soft deadline breach traps and counts as a failure when it reaches the streak limit of its call class (3 consecutive, or 30 consecutive for scene frames) or when the call runs 100 ms past its deadline (grace). A hook-handler breach never counts as a failure; it counts toward the extension stop, which follows the third consecutive breach (`extension.disabled`). The call classes are in [Host scheduling](/pages/reference-host-scheduling/).
- Exceeding the linear memory or table limits.
- A Rust panic or an uncaught TypeScript exception in such an export.

### Trap codes

A trap carries one code: the seven in the table below, plus `host.capability-not-requested`, `host.import-panic` and `host.stack-overflow`, which are defined in the fault table and in [Host architecture](/pages/reference-host-architecture/). The codes are diagnostics of the host, the harness and `ilium plugin logs`. They are never a value of `plugin-error.code`, which is always one of the 15 values of the error-code enum above.

| Code | Raised when |
|---|---|
| `trap.deadline` | A hard deadline breach in `activate`, `on-events` or `views.render`, or a soft breach on the streak limit |
| `trap.memory` | Linear memory grows past the effective `memory_mib` limit |
| `trap.table` | A table grows past the effective table limit |
| `trap.submission-batch` | One `on-events` call returns more than 256 submissions |
| `trap.invalid-ui-tree` | A `ui-tree` is invalid: an index out of range, a graph that is not a tree, a duplicate `key`, more nodes than `ui_tree_nodes`, or a `layout` whose child count differs from its constraint count |
| `trap.abi` | An ABI fault: the bytes cannot be a value of the type |
| `trap.panic` | A Rust panic or an uncaught TypeScript exception in an export |

- **[API-081]** A trap MUST carry exactly one of the seven trap codes `trap.deadline`, `trap.memory`, `trap.table`, `trap.submission-batch`, `trap.invalid-ui-tree`, `trap.abi` and `trap.panic`, or, for the three host-side faults, `host.capability-not-requested` (a call of a never-requested capability), `host.import-panic` (a panic in a host import) or `host.stack-overflow` (a stack overflow in guest code), and the log line of the trap MUST name it.

Every `not-permitted` result and every trap is logged with the plugin id, the interface and the function, and shown in `ilium plugin logs`. Release builds show function names in trap backtraces only. Build with `ilium plugin build --debug` to see `file:line` for each frame.

<pre class="term">
<span class="t-prompt">$</span> ilium plugin logs example.notes --tail 4
16:02:10 <span class="t-err">err</span>  on-events 34 ms (limit 20 ms)
16:02:12 <span class="t-err">err</span>  on-events 41 ms (limit 20 ms)
16:02:20 <span class="t-err">err</span>  on-events 38 ms (limit 20 ms)
16:02:20 <span class="t-hi">note</span> 3 failures in 10 minutes: disabled
</pre>

Illustrative, not generated. A plugin that breaches its event deadline three times is restarted after the first two traps, then disabled.

<figure>
<svg viewBox="0 0 400 286" role="img" aria-label="A failed call returns plugin-error, or traps and is restarted">
  <defs>
    <marker id="err-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
      <path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="110" y="10" width="180" height="36" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="200" y="32" font-size="11" text-anchor="middle" fill="currentColor">Plugin calls a host import</text>
  <path d="M200 46 L200 68" stroke="currentColor" stroke-width="1.5" fill="none" marker-end="url(#err-arrow)"/>
  <rect x="110" y="70" width="180" height="36" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="200" y="92" font-size="11" text-anchor="middle" fill="currentColor">Host checks grant and limits</text>
  <path d="M150 106 L95 128" stroke="currentColor" stroke-width="1.5" fill="none" marker-end="url(#err-arrow)"/>
  <text x="40" y="120" font-size="10" fill="currentColor">returned</text>
  <path d="M250 106 L305 128" stroke="currentColor" stroke-width="1.5" fill="none" marker-end="url(#err-arrow)"/>
  <text x="330" y="120" font-size="10" fill="currentColor">trap</text>
  <rect x="10" y="130" width="170" height="36" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="95" y="152" font-size="11" text-anchor="middle" fill="currentColor">Return plugin-error</text>
  <rect x="210" y="130" width="180" height="36" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="300" y="152" font-size="11" text-anchor="middle" fill="currentColor">Trap: instance ends</text>
  <path d="M300 166 L300 186" stroke="currentColor" stroke-width="1.5" fill="none" marker-end="url(#err-arrow)"/>
  <rect x="210" y="188" width="180" height="34" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="300" y="209" font-size="10" text-anchor="middle" fill="currentColor">Restart after 1 s, then 5 s</text>
  <path d="M300 222 L300 242" stroke="currentColor" stroke-width="1.5" fill="none" marker-end="url(#err-arrow)"/>
  <rect x="210" y="244" width="180" height="34" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="300" y="265" font-size="10" text-anchor="middle" fill="currentColor">Third failure: disabled</text>
</svg>
<figcaption>A failed check returns a plugin-error record to the caller. A trap ends the instance, which is restarted until the third failure disables it.</figcaption>
</figure>

### ABI faults and semantic faults

Two kinds of fault exist. The kind decides whether the call returns an error or the instance traps.

| Kind | Examples | Result |
|---|---|---|
| ABI fault: the bytes cannot be a value of the type | A string that is not valid UTF-8 [ABI-017]; an enum discriminant or variant tag outside the case list; a list whose length or pointer leaves the plugin memory; more than 256 submissions [ABI-008]; memory growth past the instance limit [ABI-027] | Trap. The call does not complete, and the restart rule applies |
| Semantic fault: a valid value that breaks a documented rule | A value above a stated maximum; a colour with a non-zero top byte; a path with `..`; an unknown id | Error return. The code is `invalid-argument`, `not-found`, `quota-exceeded` or the code the function lists |
| Grant fault | A function of an interface that was linked, but whose capability was not requested | Trap `host.capability-not-requested` [ABI-023] |
| Grant fault in an intent or a call that returns `result` | The capability was requested but the user declined it | `not-permitted` (a whole batch is refused when one submission lacks its grant [ABI-009]) |

The per-function codes are the codes in the doc comment of each function in the [WIT reference](/pages/reference-wit/). The no-grant behaviour of every function is the second test of each row in the [host conformance matrix](/pages/reference-host-conformance/#1-how-to-use-the-matrices). The security policy is in [Security](/pages/security/).

### Restart and disable rule

The rule applies to each plugin on each side. A client counts its own failures separately from the server. A failing view counts against the plugin, not against the view alone.

| Failure | Action |
|---|---|
| First failure in a 10-minute window | Restart after 1 s |
| Second failure in the window | Restart after 5 s |
| Third failure in the window | Disable the plugin on that side and show a notice (an essential core plugin restarts after 5 s instead, LIF-042; in a development build the third and later failures restart after 5 s instead, LIF-226) |
| A failure when all earlier failures are older than 10 minutes | Counts as a first failure: restart after 1 s. No 30 s step exists |

Test vectors. Times are measured from the first failure. Failures at 0, 2 and 4 minutes: restart after 1 s, restart after 5 s, disabled. Failures at 0 and 11 minutes: both restart after 1 s, because the first failure has expired. Failures at 0, 6 and 11 minutes: restart 1 s, restart 5 s, restart 5 s (the failure at 0 has expired, the one at 6 minutes counts), never disabled. A scene, view or inference-provider failure counts as a failure of the whole plugin on that side. The client and the server keep independent counts. A user turn-off persists across restarts of Ilium; re-enabling in Settings resets the count to zero.
A failure expires 10 minutes after it happened. A successful run does not clear it sooner. Turn the plugin on again in Settings > Plugins, which resets the count to zero.

These count as failures: a trap, a hard deadline breach, an `activate` call that returns an error, a restart of a stuck event queue, the death of the sandbox helper during a call, and a soft deadline breach that reaches the streak limit of its call class (3 consecutive, or 30 consecutive for scene frames) or runs 100 ms past its deadline (grace). These never count: a `revoked` or `granted` re-activation, a disable, an upgrade, a shutdown, a breach or trap in `deactivate`, an activation refused for host capacity or helper count, and a failed job. A job failure never counts toward the restart rule. The full list with the rule for each case is in [Host rules for limits and restarts](#host-rules-for-limits-and-restarts).

### Resource limits in the manifest

The manifest `limits` object is a JSON object. Each key is optional. If the plugin asks for a value above the ceiling for its tier, the host clamps the value and logs the clamp. A request never fails the install. The user can also set a value for every plugin with the same key under `[plugins.limits]` in `config.toml`, or for one plugin under `[plugins."<id>".limits]`. A user value overrides the manifest value and is clamped to the tier ceiling. Session keys are the exception: they have no manifest value and the user may only lower them.

```json
"limits": {
  "memory_mib": 64,
  "job_deadline_ms": 5000,
  "job_wall_ms": 30000
}
```

The table lists the per-plugin limits. The values belong to API version `ilium:plugin@0.1.0`.

| Key | Default | Ceiling T1 | Ceiling T2/T3 | Effect when reached |
|---|---|---|---|---|
| `memory_mib` | 32 | 256 | 128 | Trap |
| `table_elements` | 10,000 | 50,000 | 20,000 | Trap |
| `event_deadline_ms` | 20 | 50 | 20 | Trap |
| `command_deadline_ms` | 50 | 100 | 50 | Trap |
| `render_deadline_ms` | 10 | 20 | 10 | Trap |
| `annotate_deadline_ms` | 10 | 20 | 10 | Late result dropped; third breach in a row traps |
| `frame_deadline_ms` | frame budget (1,000 ms divided by the effective frame rate, rounded down; 33 ms at 30 fps) | 50 | 50 | Frame skipped; 30 breaches in a row traps |
| `activate_deadline_ms` | 2,000 | 5,000 | 2,000 | Trap |
| `deactivate_deadline_ms` | 500 | 1,000 | 500 | Instance dropped, no restore bytes kept |
| `job_deadline_ms` (CPU) | 2,000 | 60,000 | 10,000 | `failed(timed-out)` |
| `job_wall_ms` | 20,000 | 600,000 | 100,000 | `failed(timed-out)` |
| `job_memory_mib` | 64 | 512 | 256 | `failed(quota-exceeded)` |
| `job_io_mib` (input and output, each) | 16 | 64 | 16 | Input: `invalid-argument` at the call. Output: `failed(invalid-argument)` |
| `concurrent_jobs` | 2 | 8 | 2 | Queued |
| `job_queue` | 16 | 64 | 16 | `busy` |
| `event_queue` | 1,024 | 4,096 | 1,024 | Stuck plugin is restarted |
| `pending_requests` (ABI-013) | 64 | 256 | 64 | `busy` |
| `timers` | 64 | 256 | 64 | `quota-exceeded` |
| `output_filters` | 32 | 128 | 32 | `quota-exceeded` |
| `watches` | 64 | 256 | 64 | `quota-exceeded` |
| `key_grabs` | 8 | 32 | 8 | `quota-exceeded` |
| `ui_tree_nodes` | 4,096 | 16,384 | 4,096 | Trap |
| `ui_tree_kib` | 1,024 | 4,096 | 1,024 | Trap |
| `decorations` | 4,096 | 16,384 | 4,096 | `refused(quota-exceeded)` |
| `node_metadata_kib` | 64 | 256 | 64 | `quota-exceeded` |
| `storage_bucket_mib` | 16 | 256 | 16 | `quota-exceeded` |
| `disk_mib` (`data` plus `cache`) | 256 | 4,096 | 256 | `failed(quota-exceeded)` |
| `fs_write_mib` (one call) | 16 | 64 | 16 | `invalid-argument` |
| `fs_read_all_mib` | 64 | 512 | 64 | `failed(quota-exceeded)` |
| `http_response_mib` | 8 | 64 | 8 | `failed(quota-exceeded)` |
| `process_output_mib` | 4 | 32 | 4 | `failed(quota-exceeded)` |
| `log_lines_per_second` | 200 | 1,000 | 200 | Dropped and counted |
| `open_videos` | 2 | 8 | 2 | `quota-exceeded` |
| `audio_captures` | 2 | 4 | 2 | `quota-exceeded` |

`pending_requests` counts the requests that have a request id and no delivered completion: accepted submissions, calls that return a `request-id` (including deferred service calls) and jobs. A request enters the count when its id is returned and leaves it when its `request-completed` is enqueued for the plugin. Refused submissions, synchronous calls and the host-internal work of a request do not count. A call that would make the count exceed the limit returns `busy`, and in a batch it is refused as `refused(busy)` for that submission only. [API-073]

The CPU time of a plugin is also charged to the `ilium-execution` quota group [API-072]. There is one group for each plugin side in each process. The group is a token bucket that holds 2,000 ms of CPU time and refills at 500 ms of CPU time per second. These values are fixed for ilium:plugin@0.1.0 and are not manifest keys. The group charges the thread time of `on-events`, `views.render`, `command`, scene frames, annotators and jobs, and it charges 1 ms for every 64 KiB of payload in a submission or a call that returns a `request-id`. When the bucket is empty, every call that returns a `request-id` and every submission returns `quota-exceeded` with a `retry-after-ms` value. The bucket balance can go below zero, because a call is charged after it ends. The bucket is empty when the balance is 0 or less. `retry-after-ms` = max(100, ceil_10(-balance_ms * 2)), where `ceil_10` rounds up to the next 10 ms: the wall time until the balance refills to 0 at 500 ms per second, never below 100. A balance of -100 ms gives 200. A trap inside a call still charges the time used. Test: run jobs that spin 2,150 ms of CPU in total from a full bucket (balance -150 ms) and assert that the next submission returns `quota-exceeded` with `retry-after-ms` 300. Second test: drive the balance to -100 ms, submit, and expect `retry-after-ms` 200. A per-kind job limit in `contributes.jobs` (`deadline_ms`, `memory_mib`) overrides the job values for that kind, within the same ceilings.

Some caps are fixed and no manifest or user setting changes them: 256 submissions per `on-events` return (a trap when exceeded), 1 MiB for a synchronous `fs.read`, 4,096 entries for `fs.list-dir`, 8 MiB, 64 headers and 8 KiB of URL per HTTP request, a timer interval of at least 50 ms, and 64 picked paths per plugin (the oldest is forgotten). A call over a fixed cap that the plugin can check in advance returns `invalid-argument`, and nothing is written or sent. The host never truncates a value silently, with the exceptions in the next table. This table is the complete list of cuts and clamps [API-060]. Every other over-limit input is refused with a code.

| Exception | What the host does |
|---|---|
| `log.log` | Cuts the line and marks it |
| `terminal.read-screen` | Clamps `max-rows` |
| Menu entries over 64 | Shows 64 and a final `… N more` entry |
| Tooltip over 200 characters | Cuts it and ends it with `…` |
| `history-text` over 4 MiB | Keeps the newest 4 MiB |
| `clipboard.read-text` over 1 MiB | Cuts the text at a character boundary within 1 MiB |
| Transcript over 512 turns or 4 MiB | Drops the oldest turns |
| Catalogue results over 256 items, geocode results over 8 | Clamps the count |
| Virtual-rows page over 256 children | Cuts the page to 256 |
| A `debounce-ms` field of a widget outside 0 to 2,000 ms, or a settings row `debounce_ms` outside 300 to 2,000 ms | Clamps the value to the nearest bound of that range |
| Layout percentage above 100 | Clamps it to 100 |
| `limits` request above the tier ceiling | Clamps it and logs it; the install never fails |
| Event queues, journal, shared cache | Drop or evict the oldest entry and count it |
| `fs.scan` over 65,536 entries | Completes as `failed(quota-exceeded)`; `max-entries` above 65,536 is `invalid-argument` |

Per-call caps that a plugin can check before the call return `invalid-argument`. The caps that return `quota-exceeded` instead (`read-cells` rows, which return `none` and log the code, CLI stdin, `write-glyphs` cells, the popup stack) do so because they depend on held or cumulative state that the plugin cannot see in advance: other panes' frozen snapshots, bytes already written to the invocation, cells already queued in the frame, and popups already open [API-061].

### Host rules for limits and restarts

This section is the host-side statement of the tables above. It adds the keys that earlier tables did not list and fixes the exact counting. The deadlines of every call class are in [Host scheduling](/pages/reference-host-scheduling/).

Additional per-plugin key:

| Key | Default | Ceiling T1 | Ceiling T2/T3 | Effect when reached |
|---|---|---|---|---|
| `detector_deadline_ms` | 20 | 50 | 20 | Previous classification kept; third breach in a row traps |

Limits that apply to a whole session (all plugins of one hosting process together). They are not manifest keys, and a user can only lower them under `[plugins.limits]`; a higher user value is clamped to the current value with a logged warning [HST-406], so these keys have no ceiling column. There are five session keys. Two further fixed session caps, 4,096 pending requests and 32 key grabs, are not keys; see [API-070] and "Fixed session caps" below.

| Key | Default | Effect when reached |
|---|---|---|
| `session_memory_mib` | 1,024 | Activation refused with `session.memory-exceeded` ([Host helper](/pages/reference-host-helper/)); the side stays `inactive` with the reason `host-capacity`; no failure |
| `session_helpers` | 16 | Activation refused with `helper.too-many` ([Host helper](/pages/reference-host-helper/)); no failure |
| `session_processes` | 16 | `process.run` returns `busy` and `process.spawn` returns `quota-exceeded`; one count covers both kinds of child |
| `session_http_streams` | 64 | `http.open-stream` returns `quota-exceeded` |
| `session_decorations` | 65,536 | `refused(quota-exceeded)` |

Counting a failure, in one list. A **failure** is one of: a trap; a hard deadline breach; an `activate` that returns an error or breaches; the restart of a stuck event queue; the death of the helper during a call, a hung helper, or a helper protocol error; and a soft deadline breach that reaches the streak limit of its call class (3 consecutive, or 30 consecutive for scene frames) or runs 100 ms past its deadline (grace). A hook-handler breach never counts as a failure; it counts toward the extension stop, which follows the third consecutive breach (`extension.disabled`). Anything else is not a failure. In particular these never count: re-activation after `granted` or `revoked`, disable, upgrade, shutdown, a breach or trap in `deactivate`, a refusal for session memory, helper count or host capacity, a failed or timed-out job, and a late hook handler.

Restart delay, per side, within a window of 10 minutes: the first failure restarts after 1 s, the second after 5 s, and the third disables the side. There is no 30 s step. A failure expires 10 minutes after it happened.

- **[HST-400]** The host MUST apply the restart rule per plugin per side: 1 s after the first failure, 5 s after the second, disabled at the third failure inside 10 minutes, and MUST NOT insert any other step. An essential core plugin is never disabled: its third and later failures restart after 5 s [LIF-042]. In a development build the third and later failures restart after 5 s instead [LIF-226].
- **[HST-401]** The host MUST count as failures exactly the list above and MUST NOT count the listed non-failures.
- **[HST-402]** A failure MUST expire 10 minutes after it happened, and a successful run MUST NOT clear it sooner.
- **[HST-403]** A breach or trap in `deactivate` MUST drop the instance, discard its restore bytes, and MUST NOT count as a failure.
- **[HST-404]** The host MUST clamp each manifest limit to the ceiling of the plugin's tier, log the clamp once, and never fail the install because of a limit request.
- **[HST-405]** The defaults MUST be the same for all tiers. The T1 values in brackets on other pages MUST be read as T1 ceilings.
- **[HST-496]** `retry-after-ms` on a `quota-exceeded` from the `ilium-execution` group MUST equal max(100, ceil_10(-balance_ms * 2)) (`ceil_10` rounds up to the next 10 ms); a balance of -100 ms gives 200.
- **[HST-497]** `session_processes` MUST allow 16 children per session, counting `process.run` and `process.spawn` children together; when it is full `process.run` MUST return `busy` and `process.spawn` MUST return `quota-exceeded`.
- **[HST-498]** A soft deadline breach MUST trap and count as a failure when it reaches the streak limit of its call class (3, or 30 for scene frames) or runs 100 ms past its deadline; a hook-handler breach MUST NOT count as a failure and MUST count toward the extension stop, which follows the third consecutive breach per contributor, point and extension id; an accepted reply within its deadline resets the count, and the stop ends at a contributor restart or Reset [EXT-021].
- **[HST-499]** The reply deadlines MUST be: command 30,000 ms, service 30,000 ms, sessions send reply 30,000 ms, inference completion 120,000 ms, HTTP API respond 30,000 ms, client request default 5,000 ms (settable 1 to 30,000) and job cancel grace 1,000 ms; each returns `timed-out` to the caller.
- **[HST-406]** The host MUST enforce the five session keys `session_memory_mib`, `session_helpers`, `session_processes`, `session_http_streams` and `session_decorations`, and MUST apply a user value under `[plugins.limits]` for them only when it is lower than the current session value (the default, or the value already set); a higher value is clamped to the current session value and the host logs a warning.
- **[HST-407]** A refusal because of `session_memory_mib` or `session_helpers` MUST leave the side `inactive` with the reason `host-capacity` and MUST retry the activation when capacity returns.
- **[HST-408]** `event_queue` MUST default to 1,024 with a ceiling of 4,096 for T1 and 1,024 for T2 and T3.
- **[HST-409]** `detector_deadline_ms` MUST default to 20 ms with ceilings of 50 ms for T1 and 20 ms for T2 and T3.
- **[HST-410]** A function of a never-requested capability in a linked interface MUST trap with `host.capability-not-requested`, an interface that no requested capability unlocks MUST be refused at install with `manifest.import-not-requested`, and a declined grant MUST return `not-permitted`.
- **[HST-411]** The client and the server MUST keep independent failure counts for the same plugin.
- **[HST-412]** The user turning a plugin on again MUST reset the failure count of that side to zero.

### Settings codes, limits and warnings

Install errors for `contributes.settings`. Each names the item, its JSON path and the rule.

| Code | Text |
|---|---|
| `manifest.settings-kind` | `settings entry at <json path> has kind "<kind>"; expected "tab" or "section"` |
| `manifest.settings-id` | `settings id "<id>" at <json path> must match [a-z][a-z0-9-]{0,31}` |
| `manifest.settings-key` | `settings key "<key>" at <json path> must match [a-z][a-z0-9_]{0,63}` |
| `manifest.settings-key-clash` | `settings key "<key>" is declared twice (<json path> and <json path>)` |
| `manifest.settings-id-clash` | `settings id "<id>" is declared twice in this plugin` |
| `manifest.settings-title-clash` | `settings tab title "<title>" is the name of a built-in tab` |
| `manifest.settings-target-unknown` | `settings section "<id>" targets unknown tab "<tab>"` |
| `manifest.settings-target-closed` | `settings tab "<tab>" does not accept plugin sections` |
| `manifest.settings-target-grant` | `settings section "<id>" in tab "<tab>" needs capability "settings.contribute:<tab>"` |
| `manifest.settings-anchor` | `settings placement "<placement>" names no section of tab "<tab>"` |
| `manifest.settings-tab-anchor` | `settings tab "<id>" cannot be placed after "<tab>"` |
| `manifest.settings-row-type` | `settings row type "<type>" at <json path> is not known` |
| `manifest.settings-row-field` | `settings row field "<field>" at <json path> is not supported for type "<type>"` |
| `manifest.settings-default` | `settings default for "<key>" does not match its row: <reason>` |
| `manifest.settings-depends` | `depends_on at <json path> names unknown key "<key>" or forms a cycle` |
| `manifest.settings-help` | `help topic "<id>" at <json path> is not in contributes.help` |
| `manifest.settings-command` | `settings row at <json path> names command "<id>" that the plugin does not contribute` |
| `manifest.settings-glyph` | `settings tab glyph "<glyph>" must be one character 1 or 2 cells wide` |
| `manifest.settings-limit` | `settings <item> at <json path> exceeds the limit of <n>` |
| `manifest.settings-owner` | `another plugin already owns the settings tab "<tab>"` (raised when a second plugin claims a tab that already has an owner) |

Load warnings are not install errors. They appear in `ilium plugin list` and on the recovery page:

| Warning | Meaning |
|---|---|
| `settings.section-dropped` | A built-in tab already shows six plugin sections; the section is not drawn |
| `settings.tab-dropped` | 16 plugin tabs are already shown; the tab is not drawn |
| `settings.anchor-missing` | An own-section anchor no longer exists; the section is placed at the end |
| `settings.value-ignored` | A stored value no longer matches its row; the default applies and the stored text is kept |

Limits are in [Errors and limits](/pages/reference-errors-and-limits/#settings-codes-limits-and-warnings). The validation answer has a deadline of 100 ms, and a timeout refuses the save with `Could not check this value: <plugin name> did not answer.`

### Config, snapshot and settings-write codes

Run-time codes of the [config and data migration](/pages/reference-config-and-data-migration/) rules. The file API reports a stale revision as `conflict` and an expired lock wait as `timed-out` (see the [file API](/pages/api-fs/)).

| Code | Raised when |
|---|---|
| `config.parse-error` | `config.toml` or `.ilium/config.yaml` cannot be parsed. The file stays unchanged, defaults apply for the run and the host boots (COR-090) |
| `config.unknown-key` | A `[plugins]` key is not owned by the plugin host |
| `settings.unknown-key` | A write names a key the kernel does not know in that tab. Nothing is written |
| `snapshot.section-too-large` | A snapshot section is larger than its `max_bytes`. It is dropped and the previous value is kept |
| `section-unsupported` | The plugin answers `restore-section` with a `schema_version` it does not accept. The section stays unchanged |
| `settings.invalid-value` | A write is outside the range of a row. Nothing is written |
| `settings.key-not-owned` | A write through `settings.write-kernel:<tab>` names a key outside the "Keys written" column of that tab (COR-034). Nothing is written |
| `settings.locked` | A write to a key in the administrator policy `settings.locked`, by the user, by `ilium config set` or by a plugin with `settings.write-all` (SEC-190) |
| `snapshot.write-failed` | A snapshot write failed. The server keeps running, the previous snapshot file is kept and the write is retried at the next snapshot (LIF-154) |

### Component, host and store codes

Codes of the component checks, of the host runtime and of the grant and trust stores. A rule that the host logs carries the code in the log line. A rule that traps carries it as the trap code. The checks of `ilium plugin check` are in [Check and lint rules](/pages/reference-check-and-lint-rules/), the runtime rules are in [Host runtime](/pages/reference-host-runtime/) and [Host architecture](/pages/reference-host-architecture/).

| Code | Raised when |
|---|---|
| `component.custom-section-too-large` | A custom section is over 4 MiB, or the custom sections together are over 8 MiB |
| `component.import-ungranted` | The component imports a host function whose grant the side does not request |
| `component.import-unknown` | The component imports an interface that is not in the contract |
| `component.invalid` | The file is not a valid WebAssembly component |
| `component.missing` | A declared side has no component |
| `component.size-warning` | A component is larger than 8 MiB. Above 32 MiB it is an error |
| `component.too-large` | The component is larger than 32 MiB |
| `component.wasi-forbidden` | A component imports `wasi:sockets` or `wasi:http`. It is refused at load and not instantiated (HST-077) |
| `component.world-mismatch` | The world of the component differs from `world` in the manifest |
| `host.capability-not-requested` | A plugin calls a function that its manifest never requested. The call traps |
| `host.cache-unavailable` | The compile cache is unreadable, unwritable or full. The host compiles in memory and logs the code once (HST-062) |
| `host.cwasm-incompatible` | No `.cwasm` fits the target triple or the engine fingerprint. The component is compiled at load and the start is slower |
| `host.engine-error` | A Wasmtime engine-level error, or a Store over its limit of 16 instances, 8 tables or 4 memories. The load or instantiation fails and the plugin is not activated |
| `host.engine-failed` | The Wasmtime engine cannot be created. The process continues without a plugin host, as with `--no-plugins` (HST-037) |
| `host.grants-unreadable` | The grant store is unreadable at start. The host starts with default grants for T1 only and refuses T2 and T3 packages until the file is repaired |
| `host.import-panic` | A Rust panic inside a host import. Only the calling instance traps (HST-036) |
| `host.late` | A call finished after a soft or a hard deadline. Written to the telemetry log with the class and the duration |
| `host.queue-timeout` | The deadline of a call passes while it waits in the queue. The call is abandoned and a breach is counted (HST-216) |
| `host.shutdown-timeout` | Shutdown of the server host takes longer than 5 s. The remaining plugins are dropped without `deactivate` |
| `host.slow-calls` | Report, at most once a minute per side, of the 3 slowest calls of the last minute when a call took over 50 % of its deadline (HST-359) |
| `host.stack-overflow` | A stack overflow in guest code. The instance traps and the process keeps running (HST-089) |
| `host.start-slow` | A startup plugin takes longer than `HOST-START`. Boot continues and the plugin is reported late |
| `host.trap` | A trap, written to the telemetry log with the trap code |
| `grants.unreadable` | The grant store file cannot be parsed. It is renamed to `<name>.corrupt-<YYYYMMDD-HHmmss>`, starts with no entries and the grants are asked again (SEC-114) |
| `grants.unsafe-permissions` | Another user can write the grant file or the trust store. T2 and T3 plugins are refused at load until the file is fixed (USR-207) |
| `trust.unreadable` | The trust store file cannot be parsed. It is handled like the grant store (SEC-114) |
| `trust.removed` | A trusted key was removed with `trust remove <key_id> --force`. A plugin that it signed is shown as `Publisher no longer trusted` |

### Install, migration and registry log codes

Codes that are logged or refused outside the install pipeline of [Manifest schema and error catalogue](/pages/reference-manifest-schema/#error-catalogue).

| Code | Raised when |
|---|---|
| `install.ok` | Logged when an install completes |
| `migration.no-backup-failed` | An update installed with `--no-backup` fails in `migrate`. The previous version stays active and the data may be half migrated. Logged at level error |
| `plugin.blocked` | `ilium plugin enable <id>` on a blocked plugin without `--accept-risk` (USR-175) |
| `registry.namespace-not-dormant` | A registry refuses to give a namespace to another publisher because it is not dormant, or the notice period is not over (PKG-656) |

### Error codes of the extended functions

No new `error-code` case exists. The extended functions reuse the 15 codes. Codes marked sync are returned by the call. Codes marked completion arrive as `failed` in `request-completed`.

| Function or intent | Sync codes | Completion codes |
|---|---|---|
| `catalogs.query`, `query-async` | `not-found` (catalogue), `invalid-argument` (limit) | `timed-out`, `internal` |
| `geocode.search`, `reverse` | `not-permitted` (no consent), `rate-limited`, `invalid-argument` | `network`, `timed-out`, `not-found` |
| `cli.write-text`, `read-stdin` | `invalid-argument`, `unsupported` (stdin `none`), `quota-exceeded` (4 MiB) | `cancelled` |
| `sessions.send` | `not-found`, `not-permitted`, `invalid-argument` | `timed-out`, `network` |
| `surface.write-glyphs`, `set-owner-ids` | `invalid-argument`, `quota-exceeded`, `unsupported` | none |
| `media.open-video-bytes` | `invalid-argument` (including more than 16 MiB), `io`, `unsupported` (mime type or codec) | none |
| `media.open-video-range` | `invalid-argument` (including more than 16 MiB), `io` | none |
| `fs.write-atomic` | `invalid-argument` (bad path, over 16 MiB), `not-permitted` | `conflict`, `io`, `not-found`, `quota-exceeded` |
| `fs.propose-agent-config` | `not-permitted` (`scope`: a path outside the folder, a credential file or the deny set), `invalid-argument` (an empty list, two changes to one path, more than 32 changes, more than 16 MiB of new content in total, a title over 80 or a summary over 200 characters), `busy`, `rate-limited` | `not-permitted` (`declined`), `timed-out`, `conflict`, `quota-exceeded` (history entry), `io`, `cancelled` |
| `fs.tail` | `not-permitted`, `not-found`, `invalid-argument` (`max-bytes` over 1 MiB), `io` | none |
| `http.open-stream` | `not-permitted`, `quota-exceeded` | `network`, `timed-out`, `rate-limited` (in `http-stream-closed`) |
| `vector.embed` | `unsupported`, `invalid-argument` | `network`, `rate-limited`, `timed-out` |
| `vector.upsert`, `create-index` | `quota-exceeded`, `invalid-argument` | none |
| `vector.top-k`, `remove-entries` | `not-found` (unknown index id), `invalid-argument` | none |
| `vector.drop-index` | none; the call returns nothing, so an unknown index id is ignored | none |
| `process.spawn` | `not-permitted`, `quota-exceeded` (`children`), `not-found` | exit status in `child-exited` |
| `timers.at` | `invalid-argument` (past by more than 1 s), `quota-exceeded` | none |
| `journal.log-event` | `rate-limited` | none |
| `settings.set-row-state`, `set-item-states`, `set-choice-items`, `set-meter` | `not-found`, `invalid-argument`, `rate-limited` | none |
| `kernel-settings.read` | `not-permitted`, `not-found` | none |
| `terminal.read-cells`, `freeze-snapshot` (`read-cells` returns `none` and logs the code; only `freeze-snapshot` returns the error) | `pane-not-ready`, `not-found`, `quota-exceeded` | none |
| `terminal.history-text`, `search-history` | `invalid-argument`, `quota-exceeded` | `timed-out` |
| `terminal.lock-input` | `conflict`, `invalid-argument` (over 2 s) | none |
| `agent.transcript` | `unsupported`, `not-found` | `io`, `quota-exceeded` |
| `client-requests.request` | `not-found` | `timed-out`, `not-found` (client detached) |
| `gpu.run` | `unsupported`, `invalid-argument` | none |

The outcomes of the extended intents are in the [events and intents reference](/pages/reference-events-and-intents/).

**Rust**

```rust
match host::geocode::search(&query) {
    Ok(request) => self.pending_geocode = Some(request),
    Err(e) if e.code == ErrorCode::NotPermitted => self.ask_consent(),
    Err(e) if e.code == ErrorCode::RateLimited => self.retry_in(Duration::from_secs(1)),
    Err(e) => return Err(e),
}
```

**TypeScript**

```ts
try {
  pendingGeocode = geocode.search(query);
} catch (e) {
  if (!isPluginError(e)) throw e;
  if (e.code === 'not-permitted') askConsent();
  else if (e.code === 'rate-limited') retryIn(1000);
  else throw e;
}
```

### Extension point codes

Install codes: `manifest.extension-point-duplicate`, `manifest.extension-point-invalid`, `manifest.extension-entry-invalid`, `manifest.extension-side`, `manifest.extension-limit`, `manifest.grant-missing-extension`, `manifest.replaces-unknown`, `manifest.replaces-not-replaceable`, `manifest.replaces-surface-incomplete` and `manifest.replaces-cycle`. See [Manifest reference](/pages/reference-manifest/#extension-points-and-replacement).

Run codes, returned by the `extensions` host functions or written to the plugin log. They do not add an error-code case: each one travels as a base code plus a message prefix, mapped in [Extension points](/pages/api-extension-points/#edge-cases-and-error-codes):

| Code | Cause |
|---|---|
| `extension.unknown-point` | The point does not exist or its owner is not enabled |
| `extension.not-granted` | The matching `extensions.provide` or `extensions.contribute` grant is missing |
| `extension.schema-mismatch` | A payload, entry (including a priority outside 0 to 100, EXT-047) or reply failed its schema. A bad reply counts as "no change" |
| `extension.cap-exceeded` | A point is full |
| `extension.reentrant` | A handler called `run-hook` on a point already on the running chain, directly or through other points |
| `extension.not-dynamic` | `register` or `unregister` on a point without `dynamic: true` |
| `extension.handler-timeout` | A handler missed its deadline. It counts as "no change" |
| `extension.reply-not-allowed` | The reply does not fit the mode. It counts as "no change" |
| `extension.vetoed` | A `veto` handler refused, or, with `on_failure: refuse`, a guard handler failed on a plugin-started action (reason `guard-failed`). The run completes as `refused(conflict)` |
| `extension.disabled` | The extension stopped after three breaches in a row, until the plugin restarts or the user presses Reset |
| `extension.chain-cap` | Handlers after the 100 ms chain cap were skipped. Log-only |
| `extension.contributor-not-ready` | A hook call met a contributor that is not yet `active`. The host skips it, treats it as `pass` and logs the code (LIF-056) |
| `extension.depth-exceeded` | A `run-hook` from a handler at depth 3. It counts as `pass`, not as a breach. Travels as `reentrant` |
| `extension.entry-rejected` | The owner refused an entry outside its allowlist (SEC-283). Log-only, to the log of the contributor and of the owner |
| `extension.veto-overridden` | The user overrode a `veto` chain or a guard on an action that the user started (SEC-291). The chain completes `ok` and the owner's log gets the code |

### Requestable limits

Requestable limits. The manifest `limits` object may ask for less, or for more up to the ceiling of the tier.

| Key | Default | Ceiling T1 | Ceiling T2 and T3 | Effect of a breach |
|---|---|---|---|---|
| `http_streams` | 4 | 16 | 4 | `quota-exceeded` |
| `children` | 2 | 8 | 2 | `quota-exceeded` |
| `durable_timers` | 64 | 256 | 64 | `quota-exceeded` |
| `vector_indexes` | 4 | 16 | 4 | `quota-exceeded` |
| `vector_entries` | 10,000 | 200,000 | 50,000 | `quota-exceeded` |
| `shared_cache_mib` | 64 | 256 | 128 | oldest entries evicted |
| `journal_entries` | 5,000 | 20,000 | 5,000 | oldest trimmed |
| `toasts_pending` | 3 | 3 | 3 | oldest replaced |
| `package_assets_mib` | 64 | 256 | 64 | `manifest.asset-size` |
| `video_buffer_mib` | 64 | 256 | 64 | decode-ahead shortens; nothing fails |
| `migrate_deadline_ms` | 30,000 | 60,000 | 30,000 | hard deadline: the migration fails, the backup is restored, not counted as a failure [LIF-105] |

Fixed caps. They are the same for every tier unless `limits_tier` is `first-party`.

| Cap | Value | Effect |
|---|---|---|
| Catalogue `limit` (caller request) | 256 items | `invalid-argument` above 256 |
| Catalogues, client request kinds, languages | 8, 16, 8 | `manifest.contribution-limit` |
| Points per plugin, extensions per plugin | 16, 64 | `manifest.extension-limit` |
| Entries per collect point, handlers per hook point | `max_entries` (at most 256), `max_handlers` (at most 8) | `extension.cap-exceeded` |
| Extension entry, hook payload, hook reply | 16 KiB, 64 KiB, 64 KiB | `manifest.extension-entry-invalid`, `extension.cap-exceeded` |
| Hook handler, whole hook chain | `deadline_ms` (5 to 50, default 20), 100 ms | counts as a pass, `extension.handler-timeout`; the third consecutive breach stops the extension (`extension.disabled`) |
| Menu entries, submenu depth | 64, 2 | truncated with `… N more`, `manifest.menu-depth` |
| Tooltip | 200 characters | truncated with `…` |
| `send-input` stages, total stage delay | 16, 10 s | `invalid-argument` |
| `send-input` text or raw bytes per intent | 64 KiB | `refused(invalid-argument)` |
| Codec input | 16 MiB | `invalid-argument` |
| Events per `on-events` batch | 256 | the excess is split into later batches |
| `fs.scan` entries | 65,536 | more matches complete as `failed(quota-exceeded)` |
| Journal payload | 4 KiB | `invalid-argument` |
| Message payload | 1 MiB | `refused(invalid-argument)` |
| Topic payload | 64 KiB | `invalid-argument` |
| Command `arguments-json` | 64 KiB | `invalid-argument` |
| Backup copy before a migration (`migration_backup_mib`) | 2,048 MiB | the migration is refused with `migration.backup-too-large` |
| Input lock | 2 s | `invalid-argument` |
| `history-text` | 4 MiB, newest kept | truncated |
| `read-cells` rows per call | 1,000 | `none`, and the host logs `quota-exceeded` |
| Transcript per call | 512 turns or 4 MiB | oldest dropped |
| HTTP stream chunk | 64 KiB | split |
| CLI stdin per invocation | 4 MiB | `quota-exceeded` |
| `write-glyphs` cells per frame | 131,072 | `quota-exceeded` |
| Decoration frames, frame time | 16, at least 90 ms | `invalid-argument` |
| `map` entries, `multi-choice` options | 64, 64 | `manifest.settings-limit` |
| `steps` ladder, `special_values` | 64, 8 | `manifest.row-format` |
| `check-list` items | 256 | `trap.invalid-ui-tree` (detail `ui.invalid-tree`), logged |
| `list-editor` items | 64 (1,024 with `first-party`) | `trap.invalid-ui-tree` (detail `ui.invalid-tree`), logged |
| `vector.embed` | 256 texts of at most 8 KiB | `invalid-argument` |
| Journal bytes per plugin, own entries | 4 MiB | oldest trimmed |
| Pre-rendered scene loop | 30 fps, 3,600 frames, 128 MiB | `manifest.scene-prerender` |
| Video from bytes | 16 MiB | `invalid-argument` |
| Font asset | 4 MiB | `quota-exceeded`, `manifest.asset-size` |
| `status_segments`, `pane_frame_slots`, `tree_toolbar`, `row_actions`, `tree_orderings`, `display_modes` | 2, 4, 4, 4, 4, 2 | `manifest.contribution-limit` |
| Geocode results | 8 | clamped |
| Popup stack per view | 4 | `quota-exceeded` |
| Widget `debounce-ms`; settings row `debounce_ms` | 0 to 2,000 ms; 300 to 2,000 ms | clamped to the nearest bound |
| `restructure` operations | 256 | `invalid-argument` |
| Minimap marks per layer, meter segments | 4,096, 8 | `trap.invalid-ui-tree` (detail `ui.invalid-tree`), logged |
| Toast actions, notice actions | 2, 3 | Toast: `invalid-argument`. Notice: the render traps with `ui.invalid-tree` (render has no result) |
| Spinner custom frames | 16 | `trap.invalid-ui-tree` (detail `ui.invalid-tree`), logged |
| Rich text of an info row | 120 characters | `invalid-argument` |

Deadlines: `query-catalog`, `menu-items` and `list-virtual-children` take 20 ms, and `tokens` takes 10 ms per call. The timeout of `client-requests.request` is 1 to 30,000 ms. A breach counts like any deadline breach. `menu-items` and `tokens` fall back to static content and do not fail the user action.

Fixed session caps. They count across every plugin of the session [API-070]. They are not manifest keys; a user may only lower the five session keys under `[plugins.limits]` (HST-406). The five session keys (`session_memory_mib`, `session_helpers`, `session_processes`, `session_http_streams`, `session_decorations`) are in the table under "Host rules for limits and restarts".

| Cap | Value | Over the limit |
|---|---|---|
| Pending requests | 4,096 | `busy` at the call |
| Key grabs | 32 | `quota-exceeded` |
| Input locks | one per pane | `conflict` |

`session_processes` is the single budget of 16 per session for `process.run` children (`busy` when full) and `process.spawn` children (`quota-exceeded` when full) together. It is not a manifest key.

Rates:

| Operation | Rate |
|---|---|
| `toast` | 3 per 10 s per plugin |
| `geocode` | 1 per second per plugin |
| `set-status-segment`, `set-frame-slot` | 10 per second per slot |
| `set-info`, `set-choices`, `set-row-note`, `set-row-state`, `set-item-states`, `set-choice-items`, `set-info-rich`, `set-meter` | 10 per second per plugin, together |
| `journal.log-event` | 20 per second per plugin |
| `journal.read` | `limit` is clamped to 1,000 entries per call (a size cap, not a rate) |
| `send-input`, `agent-action` | 20 per second per pane per plugin |
| `topics.publish` | 100 per second per plugin |
| `notify` | 6 per minute per plugin |
| `play-sound` | 4 per second per plugin |
| `open-external` | 10 per 60 s per plugin (sliding window) |
| `run-hook` | 200 per second per extension point (sliding window) |
| `mark-attention`, `ack-attention` | 10 per second per node per plugin, together |
| `screen-changed` delivery | 10 per second per pane |

Every rate in this table is a **sliding window** over the host monotonic clock: an operation is allowed when fewer than N operations of the same key were allowed in the previous window length (1 s, 10 s or 60 s). Refused operations do not enter the window. The key is the plugin, plus the slot or the pane where the table says so. The window is not reset by a restart of the plugin side, and not shared between the server and the client half. A refusal is `rate-limited` with `retry-after-ms` set to the time until the oldest counted operation leaves the window, rounded up to 10 ms. [API-071]

The first-party tier (`limits_tier: "first-party"`, bundled T1 `ilium.` plugins only) raises the settings caps: 8 tabs per plugin, 2,048 value keys, 64 rows per section, 1,024 list items and 512 choice options.

A breach shows as a refusal to the plugin and, for user-visible limits, as a warning. The render shows what the user sees when a plugin hits the toast rate.

<pre class="term">╭  ≡ ● · Plugin log───────────────────────────────────╮
│ ! ilium.guard: toast rate-limited (3 per 10 s)      │
│ ✕ ilium.guard: http_streams 4 of 4 in use           │
│ ✓ ilium.guard: retry accepted, stream 3 closed      │
╰──────────────────────────────────────────────────── ╯
</pre>

The three lines are refusals logged at level warn. Illustrative, not generated.

**Rust**

```rust
match host::http::open_stream(&request) {
    Err(e) if e.code == ErrorCode::QuotaExceeded => {
        // 4 of 4 streams are open: close an idle one, then retry on the next event.
        self.close_oldest_stream();
        self.retry_open = true;
    }
    other => self.track(other)?,
}
```

**TypeScript**

```ts
try {
  track(http.openStream(request));
} catch (e) {
  if (isPluginError(e) && e.code === 'quota-exceeded') {
    closeOldestStream(); // 4 of 4 streams are open
    retryOpen = true;
  } else throw e;
}
```

### Reply deadlines

A caller that waits for an answer gets `timed-out` when the answer does not arrive in time. These deadlines are fixed; none of them is a `limits` key. The call classes of the plugin's own handlers are in [Host scheduling](/pages/reference-host-scheduling/).

| Reply | Deadline | Result at the caller |
|---|---|---|
| Command reply | 30,000 ms | `timed-out` |
| Service reply | 30,000 ms | `timed-out` |
| Sessions send reply | 30,000 ms | `timed-out` |
| Inference completion | 120,000 ms | `timed-out` |
| HTTP API respond | 30,000 ms | `timed-out` |
| Client request, default | 5,000 ms (settable from 1 to 30,000 ms) | `timed-out` |
| Job cancel grace | 1,000 ms | the job instance is trapped at the next tick |

### Install codes: manifest and package

The complete list of install, signature, trust, registry and dependency codes, with the stage, exit code and message template of each, is in [Manifest schema and error catalogue](/pages/reference-manifest-schema/#error-catalogue). This page keeps a short list for orientation. If the two differ, the catalogue is right.

An install, an update and `ilium plugin verify` report a code with the path of the fault. Codes that start with `manifest.` are about what the manifest says. Codes that start with `package.` are about the archive. The settings and extension point tables on this page give those codes with their messages. This index lists the other codes in one place, with the page that defines each. Codes of the `ilium plugin` command itself (`toolchain.`, `new.`, `dev.`, `inspect.`, `trace.` and `usage.`) are in [Plugin command-line reference](/pages/reference-plugin-cli/), the `component.` and `lint.` rules are in [Check and lint rules](/pages/reference-check-and-lint-rules/), and the `harness.` and `fixture.` codes are in [Test harness reference](/pages/reference-test-harness/). When two pages differ, the defining page decides, and a page that cites an install code MUST use the cause that the defining page gives. [AUT-110]

Older drafts used other spellings for these codes. The current names are `manifest.unknown-key`, `manifest.api-version` and the five `manifest.file-*` codes; the [install error catalogue](/pages/reference-manifest-schema/#error-catalogue) is the only list.

| Code | Cause | Defined in |
|---|---|---|
| `manifest.api-version` | The `ilium:api-version` section of a component differs from the manifest `api` | [Package format](/pages/reference-package-format/) |
| `manifest.asset-size` | A declared asset is over its size limit | [Manifest](/pages/reference-manifest/) |
| `manifest.catalog-unknown` | A `choice` row `source` names a catalogue that no plugin provides | [Settings API](/pages/api-settings/) |
| `manifest.client-request-unknown` | A client request kind or export disagrees between the two sides | [Client requests](/pages/api-client-requests/) |
| `manifest.cli-field` | A CLI `output`, `stdin` or `argument_groups` value is invalid | [CLI](/pages/api-cli/) |
| `manifest.cli-name-reserved` | A CLI name is a kernel-owned subcommand (plugin, session, kill-session, help, version); the names of bundled core plugins give `manifest.cli-name-taken` to other plugins | [CLI](/pages/api-cli/) |
| `manifest.cli-name-taken` | A CLI name is owned by another installed plugin | [CLI](/pages/api-cli/) |
| `manifest.point-risk` | An extension point declares a risk level lower than the grant its handlers receive, or an unknown risk | [Extension security](/pages/reference-extension-security/) |
| `manifest.contribution-limit` | A contribution list is longer than its limit | [Manifest](/pages/reference-manifest/) |
| `manifest.contribution-target` | `row_actions.applies_to` names an unknown target | [Commands](/pages/api-commands/) |
| `manifest.export-grant` | An export is listed without the grant that unlocks it | [Exports](/pages/api-exports/) |
| `manifest.exports-mismatch` | `exports` and the component do not agree | [Manifest](/pages/reference-manifest/) |
| `manifest.file-unlisted` | A file is in the package but not in `files` | [Security](/pages/security/) |
| `manifest.file-missing` | A `files` entry has no file in the package | [Security](/pages/security/) |
| `manifest.file-length` | A file length differs from its `files` entry | [Security](/pages/security/) |
| `manifest.file-hash` | A file SHA-256 differs from its `files` entry | [Security](/pages/security/) |
| `manifest.file-self` | `files` lists `manifest.json` or `signature` | [Package format](/pages/reference-package-format/) |
| `manifest.files-order` | `files` is not in the order of the archive | [Package format](/pages/reference-package-format/) |
| `manifest.key-missing` | A packed manifest has no `files` key | [Package format](/pages/reference-package-format/) |
| `manifest.unknown-key` | A manifest key is outside the schema, or a key under `limits` is unknown | [Manifest](/pages/reference-manifest/) |
| `manifest.grant-argument` | A grant argument is malformed, or `http:setting:<key>` names a setting the plugin lacks | [Capabilities](/pages/capabilities/) |
| `manifest.grant-argument-missing` | A grant that needs an argument has none | [Capabilities](/pages/capabilities/) |
| `manifest.grant-dependency` | A grant needs another grant that is not requested, for example `fs.pick` without `fs.read:picked` | [fs](/pages/api-fs/) |
| `manifest.grant-program` | A `process.run` grant has arguments, a wildcard or a relative path with a separator in the program name | [process.run](/pages/capability-process-run/) |
| `manifest.grant-scope` | The file scope is unknown or does not accept that access | [Capabilities](/pages/capabilities/) |
| `manifest.grant-side` | The capability is not available on the side that requests it | [Capabilities](/pages/capabilities/) |
| `manifest.grant-tier` | The tier of the package may not hold the grant | [Capabilities](/pages/capabilities/) |
| `manifest.grant-unknown` | The name is not a capability of this API version | [Capabilities](/pages/capabilities/) |
| `manifest.grant-wildcard` | A grant argument is `*` where no wildcard is allowed | [Capabilities](/pages/capabilities/) |
| `manifest.grant-missing-extension` | A grant needed by an extension entry is missing | [Extension points](/pages/api-extension-points/) |
| `manifest.id-invalid` | The plugin id does not match the id pattern | [Packages](/pages/packages/) |
| `manifest.id-publisher` | A signed package id does not start with the publisher id | [Signing and trust](/pages/reference-signing-and-trust/) |
| `manifest.id-reserved` | A third-party package uses an `ilium.` id | [Packages](/pages/packages/) |
| `manifest.import-not-requested` | The component imports an interface that no requested capability unlocks | [Host runtime](/pages/reference-host-runtime/) |
| `manifest.limits-tier` | A `limits` key is not allowed for the tier | [Manifest](/pages/reference-manifest/) |
| `manifest.menu-depth` | A menu is nested deeper than the limit | [Manifest](/pages/reference-manifest/) |
| `manifest.publisher-mismatch` | The signature publisher does not match the trusted key | [Signing and trust](/pages/reference-signing-and-trust/) |
| `dependency.cycle` | `requires.plugins` forms a cycle (install stage 10) | [Dependency resolution](/pages/reference-dependency-resolution/) |
| `manifest.requires-invalid` | A `requires` version range is malformed | [Dependency resolution](/pages/reference-dependency-resolution/) |
| `manifest.requires-self` | A plugin requires itself | [Dependency resolution](/pages/reference-dependency-resolution/) |
| `manifest.service-cycle` | `requires.services` forms a cycle against the installed set | [Install, update, uninstall](/pages/reference-install-update-uninstall/) |
| `manifest.row-format` | A row format string is invalid | [Manifest](/pages/reference-manifest/) |
| `manifest.scene-prerender` | A scene declares prerender settings that are not allowed | [Manifest](/pages/reference-manifest/) |
| `manifest.schema-keyword` | A service schema uses a keyword outside the allowed subset | [Services](/pages/capability-services/) |
| `manifest.settings-source` | A `choice` row has both or neither of `options` and `source` (it needs exactly one) | [Packages](/pages/packages/) |
| `manifest.signature-invalid` | The signature does not verify | [Signing and trust](/pages/reference-signing-and-trust/) |
| `manifest.sound-id-taken` | A sound id repeats or is `chirping` | [Sound](/pages/capability-sound-play/) |
| `manifest.sound-format` | A sound file is missing or is not WAV PCM or OGG Vorbis | [Sound](/pages/capability-sound-play/) |
| `manifest.unstable-missing` | The component uses an unstable item the manifest does not list | [Versioning](/pages/versioning/) |
| `manifest.unstable-tier` | A T2 or T3 package lists an unstable feature outside development mode | [Versioning](/pages/versioning/) |
| `manifest.unstable-unknown` | A name in `unstable` is not an unstable feature | [Versioning](/pages/versioning/) |
| `manifest.version-invalid` | The `version` is not a plain semantic version, for example `v1.2.3` | [Dependency resolution](/pages/reference-dependency-resolution/) |
| `manifest.visible-when` | A `visible_when` expression is invalid | [Manifest](/pages/reference-manifest/) |
| `manifest.when-key` | A `when` expression uses an unknown key | [Commands](/pages/api-commands/) |
| `manifest.when-syntax` | A `when` expression has bad syntax | [Commands](/pages/api-commands/) |
| `package.api-unsupported` | The `api` is outside the window the host loads | [Versioning](/pages/versioning/) |
| `package.corrupt` | A store entry fails its hash check at load | [Install, update, uninstall](/pages/reference-install-update-uninstall/) |
| `package.downgrade` | The version is lower than the installed one and `--allow-downgrade` is absent | [Install, update, uninstall](/pages/reference-install-update-uninstall/) |
| `package.version-reused` | The version is already installed with different content | [Package format](/pages/reference-package-format/) |
| `package.publisher-key-changed` | A known publisher id arrives with a different key | [Signing and trust](/pages/reference-signing-and-trust/) |
| `package.precompiled-forbidden` | The package contains a `.cwasm` file | [Host runtime](/pages/reference-host-runtime/) |
| `package.old-format` | The file is an `.iliumanim` package | [Porting from .iliumanim](/pages/porting-iliumanim/) |
| `host.helper-missing` | A T2 or T3 package is installed but the sandbox helper binary is missing. This is not an install refusal: the package stays installed and disabled, and T1 plugins run | [Host architecture](/pages/reference-host-architecture/) |
| `package.*` | All other archive, path, signature and component faults of a `.iliumpkg` | [Package format](/pages/reference-package-format/) lists each one |

An install code never reaches a running plugin. The installer shows the code and the path in the install error, and nothing from the package runs. [Troubleshooting](/pages/guide-troubleshooting/) gives a route from the symptom to these codes.

### Refusal reasons

A refusal carries one reason word from this closed list. For the codes `not-permitted`, `quota-exceeded` and `network`, the error `message` starts with `<reason>: `, so a plugin can branch on the prefix. The same word ends the refusal line in the plugin log. A plugin MUST NOT depend on any other reason word. A call that the manifest never requested is not a refusal: it traps with `host.capability-not-requested`.

| Reason | Code | Meaning |
|---|---|---|
| `declined` | `not-permitted` | The user left the requested grant unchecked, or rejected every change of an agent configuration proposal |
| `revoked` | `not-permitted` | The grant was removed after the plugin started |
| `not-granted` | `not-permitted` | The argument is outside the grant: another host, program or command |
| `scope` | `not-permitted` | A path lies outside the granted scope |
| `policy` | `not-permitted` | The admin policy forbids the call |
| `offline` | `not-permitted` | The offline switch is on |
| `private-address` | `not-permitted` | The host name resolves to a refused address class |
| `reserved` | `not-permitted` | The destination is an Ilium port or socket |
| `downgrade` | `not-permitted` | A redirect goes from https to http |
| `not-user-initiated` | `not-permitted` | The action needs a user gesture |
| `file-type` | `not-permitted` | The file type is refused |
| `unavailable` | `not-permitted` | The target is absent, or the caller lacks the grant to learn about it |
| `response-size` | `quota-exceeded` | A buffered response is over its cap |
| `stream-size` | `quota-exceeded` | A stream is over its lifetime cap |
| `tls` | `network` | Certificate or TLS failure |

### Edge cases

| Situation | What happens |
|---|---|
| A job exceeds its CPU deadline | The job fails with `timed-out`; the plugin is not restarted |
| A job traps | The job fails with `internal` and the trap text |
| A process writes more output than its limit | The process is killed and the call fails with `quota-exceeded` |
| The plugin stops while an HTTP API call is open | The listener answers `503`; a later `respond` returns `not-found` |
| An HTTP API body is over 1 MiB, or a route gives no reply within 30 s | `413`, or `504` after 30 s |
| A known HTTP API path is called with another method | `405`, with an `Allow` header |
| A CLI invocation is open when the plugin restarts | Exit code 1, and `plugin stopped` on standard error |
| A drop-oldest event (output matches, topic messages, audio frames) arrives while the plugin queue is full | The oldest queued event of that kind is dropped and counted; the next batch starts with an `events-dropped` event. These events have no per-source cap of their own, except that an output filter delivers at most its own `rate` matches per second (1 to 100) |
| Server memory across all instances reaches 1,024 MiB | The new instance is refused and logged. It does not count as a failure; the side stays `inactive` with the reason `host-capacity` and activation is retried (see [Failure counting](/pages/lifecycle/#failure-counting-and-restart)) |

### What the user sees

- A failed plugin shows a notice in the tree footer: `Plugin <name> stopped after 3 failures in 10 minutes: <export> <code or trap> (<view>). Turn it on again in Settings > Plugins.`. The view part appears only when a `views.render` call failed.
- Settings > Plugins lists the disabled plugin. Turn it on there to clear the failure count.
- `ilium plugin logs <plugin-id>` shows refusals and traps, with the interface and function.

<pre class="term">
<span class="t-prompt">$</span> ilium plugin logs example.files --tail 2
16:10:04 <span class="t-warn">warn</span> fs.write: not-permitted (scope)
16:10:09 <span class="t-warn">warn</span> http.fetch: not-permitted (host)
</pre>

Illustrative, not generated. Two refused calls are logged with the plugin id, the interface and the function. Only refusals and traps reach the log, so a `quota-exceeded` result appears only in the caller's own handling.

Requirement ids for this page: [API-001], [API-002], [API-003], [API-004], [API-005], [API-010], [API-011], [API-012], [API-013], [API-014], [API-015], [API-016], [API-040], [API-041], [API-042], [API-043], [API-044], [API-046], [API-060], [API-061], [API-062], [API-070], [API-071], [API-072], [CAP-018], [EVT-003], [EXT-001], [EXT-023], [PRI-002], [PRI-005], [PRI-008], [PRI-014], [RND-015], [RND-016], [RND-017], [RND-018], [WID-020], [WID-021], [API-081]. See the [requirements index](/pages/reference-requirements/).

### See also

- [Rust SDK reference](/pages/rust-sdk/)
- [TypeScript SDK reference](/pages/typescript-sdk/)
- [Performance](/pages/performance/)
- [Manifest reference](/pages/reference-manifest/)
- [Lifecycle](/pages/lifecycle/)
- [Plugin settings extension](/pages/settings-extension/)
- [Debugging](/pages/debugging/)
- [Security model](/pages/security/)

Checked against the API version `ilium:plugin@0.1.0`.

### Security

Security codes: `policy.tier-denied`, `policy.id-denied`, `policy.publisher-denied`, `policy.third-party-denied` (detail codes of the one install code `policy.blocked`, written as the message prefix), `policy.grant-forbidden` (shown next to a locked row, never an install failure), `policy.source-denied`, `policy.replacement`, `policy.invalid`, `sandbox.unavailable`, `package.publisher-key-changed`, `package.blocked`, `package.path-invalid`, `package.path-duplicate`, `package.entry-symlink` or `package.entry-directory`, `package.zip-invalid`, `manifest.replaces-tier`, `manifest.secret-scope`, `trust.declined`, `manifest.display-name`, `manifest.point-risk`, `extension.depth-exceeded`, `extension.entry-rejected`, `install.review-required`, `store.version`, `trust.collision`, `settings.locked`. New `not-permitted` reasons: `policy`, `offline`, `private-address`, `downgrade`, `reserved`, `not-user-initiated`, `file-type`. Limits: [SEC-082] and [SEC-083] (egress), [SEC-290] (hooks), [SEC-029] (helper). See [Security and policy requirements](/pages/reference-requirements-security-policy/).
