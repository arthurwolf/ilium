---
layout: page
title: Reference, resource accounting
permalink: /pages/reference-resource-accounting/
---

## Reference, resource accounting

This page defines how the host measures what a plugin uses, which budget wins when several apply, what a plugin sees when a budget runs out, and how the user reads the numbers. The default and ceiling values are in [Errors and limits reference](/pages/reference-errors-and-limits/) and are not repeated here.

Contract version: `ilium:plugin@0.1.0`.

<div class="note">
<p>The numbers in Errors and limits reference are normative for 0.1.0.</p>
</div>

### What is measured

| Resource | Unit | Measured how | Scope |
|---|---|---|---|
| Linear memory | bytes | Wasmtime store limiter, on each `memory.grow` | one instance |
| Tables | elements | Wasmtime store limiter | one instance |
| CPU | ms of thread time | thread CPU clock, read before and after each call | one plugin side in one process |
| Wall time | ms | monotonic clock | one call or one request |
| Storage | bytes | sum of key and value sizes in a bucket | one bucket |
| Disk | bytes | sum of file sizes in `data` and `cache` | one plugin |
| Queues and request counts | items | counters | one plugin side |
| Session memory | bytes | sum of instance memory limits actually in use | one session server |

### Execution quota

The `ilium-execution` quota group is a token bucket. It holds 2,000 ms of CPU time and refills at 500 ms per second. There is one group for each plugin side in each process. A call spends its thread time. A submission also spends 1 ms for every 64 KiB of payload.

- When the bucket is empty, a new submission returns `quota-exceeded` with a `retry-after-ms` value. The bucket balance can go below zero, because a call is charged after it ends. The bucket is empty when the balance is 0 or less. `retry-after-ms` = max(100, ceil_10(-balance_ms * 2)), where `ceil_10` rounds up to the next 10 ms: the wall time until the balance refills to 0 at 500 ms per second, never below 100. A balance of -100 ms gives 200.
- A call already running is not stopped by an empty bucket. Its deadline stops it.
- A refused call is not a failure and is not counted toward the restart rule. It counts as a quota hit in the metrics of [Health and watchdog](/pages/reference-health-and-watchdog/).

- [LIF-139] The host MUST charge thread CPU time of each call to the `ilium-execution` group of the plugin side, and MUST refuse a submission with `quota-exceeded` and a `retry-after-ms` when the bucket is empty. Test: a plugin that submits in a loop; assert the refusal, the retry value, and that the plugin is not restarted.
- [LIF-140] A refusal for quota MUST NOT count as a failure. Test: read the failure window after 100 refusals.

### Session memory pool

One session server has a pool of 1,024 MiB (`session_memory_mib`). Each instance reserves its configured linear memory limit at activation, not what it uses. Admission works in this order.

1. Core T1 plugins reserve first, at server start.
2. Other plugins reserve when they activate, in activation order.
3. When a reservation does not fit, the host first drops unused compile cache pages, then tries once more.
4. If it still does not fit, the instance does not start. The side stays `inactive` with the reason `host-capacity`. The refusal is logged and shown in the plugin detail. It is not a counted failure.
5. When another instance deactivates, the host retries waiting sides in activation-request order.

- [LIF-141] An instance whose reservation does not fit MUST stay `inactive` with the reason `host-capacity`, MUST NOT be counted as a failure, and MUST start when memory is freed. Test: fill the pool with a small `session_memory_mib`, activate one more plugin, then deactivate another.
- [LIF-142] A core T1 plugin MUST be admitted before any third-party plugin at server start. Test: set a pool that fits exactly the core plugins plus one more, and start with a third-party plugin that activates on `startup`.

### Which limit wins

Three sources can set a limit: the tier ceiling, the manifest request and the user configuration. The host takes the manifest request, or the default when none. The user value in `[plugins.limits]` replaces it. The tier ceiling then clamps the result. The clamp is logged once per start.

```toml
# config.toml
[plugins.limits]
event_queue = 2048

[plugins."com.example.notes".limits]
memory_mib = 64
```

Example. A T3 plugin asks `memory_mib: 200` in its manifest. The user sets nothing. The ceiling for T2 and T3 is 128, so the plugin gets 128 and the log says `limit memory_mib clamped 200 to 128`. If the user sets 64 for this plugin, it gets 64. If the user sets 256, it gets 128.

- [LIF-143] The effective limit MUST be `min(ceiling, v)`, where `v` is the first set of the `[plugins."<id>".limits]` value, the `[plugins.limits]` value, the manifest value and the default, in that order; 0 or a missing value means not set, so the next source applies. The user can raise a value up to the ceiling and can lower it. Session keys have no manifest value and the user may only lower them; a value above the current session value is clamped to it and the host logs a warning. Test: the three cases of the example.
- [LIF-144] A per-plugin user value MUST win over a user value for all plugins. Test: set both and read the effective value.

### Helpers and cgroups

T2 and T3 instances run in the helper process of [Sandbox helper](/pages/reference-host-helper/). The helper is placed in a cgroup on Linux with a memory limit equal to the sum of the reservations of its instances plus 32 MiB for the runtime, and a CPU weight below the main process. A cgroup out-of-memory kill is a helper crash and follows the helper rules of [Health and watchdog](/pages/reference-health-and-watchdog/). The per-instance limits stay in force inside the helper, so the kill happens only when the runtime itself grows.

- [LIF-145] The helper cgroup `memory.max` MUST equal the sum of the hosted instances' `memory_mib` plus 32 MiB, the same amount charged to `session_memory_mib` by HST-164. Test: read the cgroup file for a helper with two instances.

### Reading the numbers

`ilium plugin stats [<id>] --json` prints the current use and the effective limit, one line per resource:

```text
{"type":"plugin-stat","id":"com.example.notes","side":"server","resource":"memory","used":9437184,"limit":33554432,"source":"default"}
{"type":"plugin-stat","id":"com.example.notes","side":"server","resource":"execution-ms","used":1240,"limit":2000,"source":"fixed"}
```

`source` is `default`, `manifest`, `user`, `ceiling` or `fixed`. `fixed` marks a limit that no manifest or user setting changes, such as the `ilium-execution` bucket. The Plugins screen detail tab Resources shows the same rows ([Managing plugins](/pages/user-managing-plugins/)).

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin stats com.example.notes
<span class="t-dim">RESOURCE     USED     LIMIT    SOURCE</span>
memory       9.0 MiB  32 MiB   default
exec bucket  1240 ms  2000 ms  fixed
storage      3.1 MiB  16 MiB   default
disk         12 MiB   256 MiB  default
</pre>
<figcaption>Rendered at 48 columns. Illustrative. Each row shows use, the effective limit and where the limit came from.</figcaption>
</figure>

- [LIF-146] `ilium plugin stats` MUST print use, effective limit and source for memory, the execution bucket, storage and disk. Test: parse the lines for a fixture plugin.

### Edge cases

| Situation | What happens |
|---|---|
| The user lowers a limit below current use | The plugin keeps running. The next growth is refused. Storage over the new limit rejects writes, not reads. |
| The user raises a limit above the ceiling | Clamped and logged. |
| The pool shrinks at a config reload | Running instances keep their reservation. New activations see the new pool. |
| Disk quota reached | Writes return `quota-exceeded`. See [Offline and low disk](/pages/reference-offline-and-low-disk/). |

### See also

- [Errors and limits](/pages/reference-errors-and-limits/)
- [Reference, errors and limits](/pages/reference-errors-and-limits/)
- [Performance](/pages/performance/)
- [Health and watchdog](/pages/reference-health-and-watchdog/)
- [Requirements, lifecycle and operations](/pages/reference-requirements-lifecycle-ops/)

API version checked: `ilium:plugin@0.1.0`.
