---
layout: page
title: Reference, health and watchdog
permalink: /pages/reference-health-and-watchdog/
---

## Reference, health and watchdog

A plugin can fail loudly, with a trap, or quietly, by being slow. This page defines the health states that Ilium shows for a plugin, the watchdog that enforces deadlines, how accurate that enforcement is, what happens to a stuck call, a stuck sandbox helper or a task that never ends, and which metrics the host keeps. A test author can use the tolerances here to assert bounds instead of exact times.

Contract version: `ilium:plugin@0.1.0`. Failure counting and restart delays are in [Lifecycle](/pages/lifecycle/#failure-counting-and-restart). Resource budgets are in [Resource accounting](/pages/reference-resource-accounting/).

### Health states

The health of a plugin is one value of the enum `health-state`. It is computed from the package state, the instance states of all sides and a few counters. The host evaluates the list below from the top and takes the first state that matches.

| Order | Health | Matches when | Driven by |
|---|---|---|---|
| 1 | `refused` | the package state is `refused-at-load` | load checks, policy |
| 2 | `disabled` | the package state is `installed-disabled-by-user` or `disabled-by-policy`, or safe mode is on | user, administrator |
| 3 | `failed` | any side is `stopped-by-failure` | the third counted failure in a window |
| 4 | `restarting` | any side is `restart-scheduled`, or `activating` after a failure | a counted failure |
| 5 | `degraded` | any degraded condition below holds | the counters below |
| 6 | `dormant` | the plugin is enabled and every side is `inactive`, and no side waits for host capacity | no trigger has fired |
| 7 | `healthy` | none of the above | |

A plugin that waits for host capacity (`host-capacity`) is `degraded`, not `dormant`.

Degraded conditions. A condition holds while the counter inside its time span meets the threshold.

| Condition | Threshold | Time span |
|---|---|---|
| Near-misses | 5 or more calls that ended within their deadline but took at least 80% of it | the last 5 minutes |
| Event queue | the queue held at least 75% of `event_queue` (768 of 1,024 by default) at any sample | the last 1 minute |
| Rate limits | 1 or more results of `rate-limited` returned to the plugin | the last 5 minutes |
| Quota hits | 1 or more results of `quota-exceeded` returned to the plugin | the last 5 minutes |
| Failures in the window | 1 or more counted failures, and the instance is active again | the failure window |
| Waiting for memory | the side is `inactive` with the reason `host-capacity` | until it activates |

- [LIF-122] The host MUST compute `health` from the first matching row of the first table, and MUST report it in `plugin-info`, in `ilium plugin status` and on the Plugins screen. Test: drive a plugin into each state and read the value.
- [LIF-123] The host MUST use the thresholds and time spans in the second table. Test: with `advance_clock`, produce 4 and then 5 near-misses and assert the change, and wait 5 minutes and assert the return to `healthy`.
- [LIF-124] A plugin that is stopped, disabled or dormant MUST NOT be reported `degraded`. Test: disable a degraded plugin.

### The watchdog

Every export call and every instance start runs under a deadline. The host enforces deadlines with Wasmtime epoch interruption.

- A background thread increments the engine epoch every tick: 10 ms in every process. Ticks come from a monotonic clock.
- Before an export call the host sets the epoch deadline to `ceil(D / 10) + 1` ticks after the call starts, where `D` is the deadline in milliseconds. When the epoch passes the deadline, the running Wasm code traps at its next safe point.
- Fuel metering is not used. Epoch interruption costs less, and the CPU budget is measured by thread time instead (see [Resource accounting](/pages/reference-resource-accounting/)).

Accuracy. An epoch tick is not a precise timer. A hard-deadline call that spins is interrupted no earlier than `D` and no later than `D + 20` ms of wall time on a host that is not overloaded. Soft deadlines do not trap at `D`; they follow the call classes in [Host scheduling](/pages/reference-host-scheduling/).

| Side | Tick | A hard deadline of `D` traps between |
|---|---|---|
| Server | 10 ms | `D` and `D` plus 20 ms |
| Client and scene | 10 ms | `D` and `D` plus 20 ms |

For example, `render_deadline_ms` of 10 ms on the client traps a slow render between 10 ms and 30 ms after it began. A plugin must not depend on a deadline being exact. A call that ends before `D` never traps.

- [LIF-125] The host MUST interrupt a hard-deadline call that runs longer than its deadline `D` at a time `t` with `D <= t <= D + 20 ms` of wall time measured from the start of the call, on a host that is not overloaded. Test: a plugin that loops forever; assert the trap time is in [D, D + 20] ms, and that a call that returns at D - 1 ms never traps.
- [LIF-126] The host MUST NOT interrupt a call before its deadline. Test: a call that takes 0.9 times `D` completes.
- [LIF-127] The host MUST NOT use fuel to enforce deadlines. Test: build the engine configuration and assert that fuel is off.
- [LIF-128] The tick MUST be 10 ms in every process and the instance deadline MUST be `ceil(D / 10) + 1` ticks after the call starts. Test: read the engine configuration.

Time in host calls. A host call that a plugin makes in the middle of an export is synchronous, and its wall time counts toward the export's deadline. The epoch cannot interrupt the host while it runs inside the call, so a slow host call makes the export trap at the first safe point after the call returns. Calls that wait for something external, such as `http`, `process` or a service reply, are not synchronous. They return a request id at once, and the wait is not part of the deadline.

- [LIF-129] Wall time spent inside a synchronous host call MUST count toward the deadline of the export that made it. Time waiting for an asynchronous request MUST NOT. Test: a handler that calls a slow synchronous host function (fixture clock) and a handler that waits for a request.

### Async tasks, pending requests and orphans

- Each asynchronous request has its own timeout (for example 30 s for a command or service reply, 120 s for inference). See [Errors and limits](/pages/reference-errors-and-limits/).
- When an instance is dropped or deactivated, every request it has pending completes as `cancelled`, every timer and watch ends, and no code runs for them. Exceptions: on a trap, completions of submissions are delivered to the new instance, and on revocation requests that need the revoked grant complete as `failed(revoked)` (see [Scheduling, deadlines and cancellation](/pages/reference-host-scheduling/)).
- A task that never completes is handled by its timeout. The plugin gets `timed-out`.

- [LIF-130] The host MUST complete all pending requests of a dropped or deactivated instance as `cancelled`, and MUST NOT call into the instance for them. Test: drop an instance with three pending requests and count outcomes.
- [LIF-131] A request that has no reply within its timeout MUST complete as `timed-out`. Test: a service that never answers.

### The sandbox helper (T2 and T3)

Third-party plugins run in a helper process. Each T2 or T3 plugin side runs in its own helper, and a helper never hosts two plugins (see [HST-150]). T1 plugins run in the Ilium process and have no helper.

| Item | Rule |
|---|---|
| Heartbeat | The server sends a ping every 1 s. The helper answers on its control thread. |
| Hung helper | 3 missed answers (3 s) or a call that exceeds its deadline by 500 ms without a trap. The server terminates the helper with SIGTERM, then SIGKILL after 2 s ([HST-171]). |
| Crash | A crash, a kill or an out-of-memory kill of the helper. |
| Effect on the plugin whose export was running | one counted failure |
| Idle helper crash (no export running) | no counted failure for anyone |

- [LIF-132] A hung helper MUST be killed after 3 missed heartbeats or 500 ms past a deadline, whichever is first. Test: a helper that stops answering.
- [LIF-133] After a helper dies, the host MUST count one failure for the plugin side that was running and MUST restart that side in a new helper on the schedule of LIF-028 and LIF-029 (1 s after the first counted failure in a window, 5 s after the second). Test: crash a helper and read the failure window of its plugin side.
- [LIF-134] The host MUST restart a helper after a crash at most 5 times in 10 minutes. After that, the T2 and T3 plugins of the session MUST move to the instance state `stopped-by-failure` on the server side with the reason `helper.unstable`. Test: kill the helper six times.

### Metrics

The host keeps these values per plugin and per side.

| Metric | Meaning |
|---|---|
| `calls` | export calls, per export |
| `p50_ms`, `p99_ms` | duration of calls, over the last 1,024 calls of each export |
| `near_misses` | calls that took at least 80% of their deadline |
| `traps` | counted traps |
| `queue_high_water` | the highest event queue length since the instance began |
| `memory_peak_bytes` | the highest linear memory plus table size |
| `quota_hits` | results of `quota-exceeded` and `rate-limited` returned |

Retention: counters are kept in memory for the life of the session server, in hourly buckets for the last 24 hours. They are not written to disk, and they reset at a server restart. The latest summary is written to the plugin log when an instance ends.

- [LIF-135] The host MUST keep the metrics above for each plugin and side, and MUST discard buckets older than 24 hours. Test: read the metrics after 25 simulated hours.
- [LIF-136] A server restart MUST reset the metrics. Test: restart and read.

### Surfaces

`ilium plugin status [<id>] --json` prints one line per plugin:

```text
{"type":"plugin-status","id":"com.example.notes","health":"degraded","reason":"near-misses","sides":[{"side":"server","state":"active","failures_in_window":1,"calls":4120,"p50_ms":0.4,"p99_ms":9.1,"near_misses":6,"traps":1,"queue_high_water":812,"memory_peak_bytes":9437184,"quota_hits":0}]}
```

The Plugins screen shows a health column ([Managing plugins](/pages/user-managing-plugins/)). The recovery page lists plugins that are not `healthy` first. The diagnostics report of [Diagnostics](/pages/core-plugin-diagnostics/) has a section `Plugins` with the same fields.

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin status
<span class="t-dim">ID                    HEALTH     P99    NEAR</span>
ilium.core.tree       healthy    1.2ms  0
com.example.notes     <span class="t-warn">degraded</span>   9.1ms  6
com.example.cursor    <span class="t-err">failed</span>     -      -
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The status list shows health, the 99th percentile call time and near-misses.</figcaption>
</figure>

- [LIF-137] `ilium plugin status` MUST print the health and the metrics of every installed plugin, as JSON lines when output is not a terminal. Test: run it in a pipe and parse.
- [LIF-138] The diagnostics report MUST contain a `Plugins` section with the health of every plugin that is not `healthy`. Test: degrade a plugin and generate the report.

### Edge cases

| Situation | What happens |
|---|---|
| A plugin is stuck in a loop inside `on-events` | The epoch traps it within the bound of LIF-125. One failure counts. |
| A plugin is stuck inside a slow host call | The call returns, then the epoch traps it at the next safe point. |
| `activate` never returns | The activate deadline traps it. One failure counts. |
| A T1 plugin loops while the UI waits for its render | The render deadline traps it within 30 ms on the client. The view shows its last good frame, marked stale. |
| The machine sleeps during a call | The monotonic clock may not advance; the host treats a gap of more than 1 s as a clock skip and does not count the call as a breach. |


### Correlation ids across a call chain

- [HST-526] The host MUST assign a correlation id to each originating input, event or hook chain, MUST pass it unchanged through every dispatcher call, handler step and cross-plugin message on both server and client sides, and MUST include it in every host and plugin log entry written during that chain and in the diagnostics bundle.

### See also

- [Plugin lifecycle states](/pages/reference-plugin-lifecycle-states/)
- [Lifecycle](/pages/lifecycle/)
- [Errors and limits](/pages/reference-errors-and-limits/)
- [Performance](/pages/performance/)
- [Debugging](/pages/debugging/)
- [Requirements, lifecycle and operations](/pages/reference-requirements-lifecycle-ops/)

API version checked: `ilium:plugin@0.1.0`.

## Trap and refusal observability

Target behaviour of the production plugin system.

- [ABI-079] Every ABI trap and every ABI refusal code MUST produce a log record with the stable fields `plugin`, `code` and `phase`.
- [ABI-080] The host MUST expose a counter per ABI code, labelled by plugin id, so an operator can alert on ABI-class failures without parsing messages.

## Production additions: T8-observability-perf

- [HST-615] The host MUST record each health transition as a log line with code `host.health.changed` containing the old state, new state and the triggering row, and MUST emit at most one transition per plugin side per 10 seconds, applying the latest state after the hold. Test: flip a plugin's health five times within 10 seconds and assert one `host.health.changed` line that carries the final state.
- [HST-617] The host MUST provide `ilium plugin health` that exits with code 0 when no plugin is `failed` or `degraded`, 1 when any is `degraded`, 2 when any is `failed`, and prints one JSON line per plugin when output is not a terminal. Test: run the command with plugins in each health state and assert the exit code, and one JSON line per plugin when the output is piped.
- [HST-618] The host watchdog MUST detect a stalled host event loop: when the kernel loop has not completed a tick for 2 seconds, the host MUST write `host.loop-stalled` with the name of the running plugin call if any, and MUST interrupt that call if it is a plugin call. Test: block the host loop with a slow synchronous plugin host call in a fixture and assert that `host.loop-stalled` is written after 2 seconds and names that call.
- [HST-641] The host MUST raise a user-visible notification once per plugin per 24 hours when a plugin becomes `degraded` or `failed`, naming the plugin and the reason, and MUST NOT raise one for a plugin the user has muted with `ilium plugin mute <id>`. Test: make a plugin degraded twice within one hour and assert one notification; mute the plugin and assert that no notification is raised.
- [HST-648] On server shutdown the host MUST flush logs, telemetry summary lines and traces within 2 seconds, and a plugin side that does not stop within 1 second of the stop request MUST be interrupted and logged with `host.shutdown.forced`. Test: give a fixture plugin side that ignores the stop request, assert that it is interrupted at 1 second with `host.shutdown.forced` logged, and that the flush finishes within 2 seconds.

## Production additions: T10-operations

- [HST-679] At server start the host MUST finish or discard every interrupted multi-file operation recorded in `<data>/plugins/journal/` (proposed path, owner to confirm) (install rename, update swap, purge): committed steps MUST be completed and uncommitted ones MUST be rolled back, the result MUST be logged as `recovery.completed` with a count, and no plugin MUST run before this finishes. Test: kill the host between the steps of an update swap and of a purge, restart, and assert the final state, one `recovery.completed` line with the count, and that no plugin ran before recovery ended.
- [HST-680] For SIGKILL, power loss and OOM kill at each of these points the host MUST leave a consistent state on next start: during install staging, during the rename into store, during update swap, during migrate, during a storage batch, during snapshot write, during grants.json write. Every write of grants.json, installed.json, consents.json and trust.json MUST be done by writing a temp file in the same folder, fsync, rename, then fsync the folder. Test: run a kill-point matrix with one case per listed point, restart after each kill and assert a consistent state, and check with the fault-injection writer that each state file is written in the temp, fsync, rename, fsync order.
