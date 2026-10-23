---
layout: page
title: Telemetry and plugin health
permalink: /pages/reference-host-telemetry/
---

## Telemetry and plugin health

The host measures every plugin call so that it can enforce budgets, show health and help an author find a slow handler. This page defines the counters, where they live, how long they are kept, what is logged, and what is never collected. The health states and the `ilium plugin status` output are in [Health and watchdog](/pages/reference-health-and-watchdog/); this page defines the host-side measurements behind them. The user's view is in [Plugin health for users](/pages/user-plugin-health/). Requirement ids are `[HST-350]` to `[HST-363]`.

## You will need

- [Host scheduling](/pages/reference-host-scheduling/) for the call classes that the counters are keyed by.
- [Performance budgets and benchmarks](/pages/reference-host-budgets/) for the budgets the counters are compared with.

## Principles

- **Local only.** Telemetry stays in the process and in the local log. The host MUST NOT send telemetry over the network, and no plugin can read the counters of another plugin.
- **Cheap.** Recording a call costs at most 200 ns. Counters use atomic integers and fixed histograms. Recording takes no lock on the call path.
- **Bounded.** Memory for telemetry is fixed per plugin side: at most 64 KiB.
- **Content free.** Counters hold numbers and names. They never hold event payloads, file names, URLs, terminal text or prompts.

- **[HST-350]** The host MUST NOT send telemetry over the network.
- **[HST-351]** Recording one call MUST take at most 200 ns at the 99th percentile and MUST NOT take a lock.
- **[HST-352]** Telemetry memory MUST be at most 64 KiB per plugin side.
- **[HST-353]** Telemetry MUST NOT contain payloads, paths, URLs, terminal text or prompts.

## Counters

Each plugin side has these counters. A **class** is a row of the call-class table in [Host scheduling](/pages/reference-host-scheduling/).

| Counter | Keyed by | Unit | Meaning |
|---|---|---|---|
| `calls` | class | count | Calls started |
| `duration` | class | histogram | Call duration, buckets of 0.1, 0.2, 0.5, 1, 2, 5, 10, 20, 50, 100, 500, 1000 ms and above |
| `near_misses` | class | count | Calls that took at least 80 % of the deadline |
| `breaches` | class, kind (hard, soft) | count | Deadline breaches |
| `traps` | trap code | count | Traps, by code (for example `trap.deadline`, `host.stack-overflow`) |
| `queue_depth` | none | gauge, high-water | Event queue length |
| `events_dropped` | event kind | count | Events dropped by the drop-oldest policy |
| `events_coalesced` | event kind | count | Events merged |
| `quota_rejections` | none | count | `quota-exceeded` results from the `ilium-execution` bucket |
| `rate_limited` | import | count | `rate-limited` results |
| `cpu_ms` | none | counter | Sum of thread CPU time of calls (feeds the `ilium-execution` bucket) |
| `memory_peak` | none | bytes | Highest linear memory plus table size |
| `helper_round_trips` | none | histogram | Round trip of forwarded imports (T2 and T3) |
| `render_ms` | view | histogram | `views.render` duration |
| `restarts` | reason | count | Restarts, by reason |
| `failures` | reason | count | Counted failures, by reason |

The 99th percentile of `duration` is read from the histogram (the upper bound of the bucket that holds the 99th percentile). The `p50_ms` and `p99_ms` of `ilium plugin status` use the last 1,024 calls of each export; the histogram covers the whole retention.

- **[HST-354]** The host MUST keep the counters above for each plugin side, and the histogram buckets MUST be the ones listed.
- **[HST-355]** `near_misses` MUST count a call that ends in time and took at least 80 % of its deadline.
- **[HST-356]** A counter MUST NOT be readable by another plugin. Only the kernel, the CLI and the user interface read them.

## Retention

Counters are kept in memory in hourly buckets for the last 24 hours, and reset when the hosting process restarts. They are not written to disk. When an instance ends, one summary line is written to the plugin log (see below). This matches [Health and watchdog](/pages/reference-health-and-watchdog/).

- **[HST-357]** The host MUST keep hourly buckets for 24 hours and MUST discard older buckets. The summary line at the end of an instance MUST be written once.

## What is logged

The host writes its own lines to the plugin log under the plugin id, with the keys `plugin`, `side`, `kind` and the stable id of the condition. They are separate from the lines the plugin writes with `log`.

| Level | Condition | Id |
|---|---|---|
| info | Instance start and end, with a summary | `instance.started`, `instance.ended` |
| warn | A soft breach or a hard breach, with class and duration | `trap.deadline`, `host.late` |
| warn | A call abandoned in the queue | `host.queue-timeout` |
| warn | Events dropped, with counts | `events.dropped` |
| warn | A clamp of a limit to the tier ceiling | `limits.clamped` |
| warn | A deactivate breach | `deactivate.deadline` |
| error | A trap, with the trap code (and a guest backtrace when debug info is present) | `host.trap` |
| error | A helper death | `helper.died` |
| info | Slow call report: top 3 slowest calls of the last minute, once a minute, when any call took over 50 % of its deadline | `host.slow-calls` |

Lines from the host are rate limited to 20 per minute per plugin side and kind. The rest are counted and summarized in one line: "N similar lines suppressed".

- **[HST-358]** The host MUST write the lines above under the plugin id, MUST include the stable condition id, and MUST rate limit them to 20 per minute per side and kind.
- **[HST-359]** The host MUST write `host.slow-calls` at most once per minute per side and only when a call took over 50 % of its deadline.

## Spans for host developers

With the environment variable `ILIUM_PLUGIN_TRACE=1` set on the server or client, the host records one span for each call with the plugin id, side, class, queue time, run time and the number of forwarded imports. The spans go to the host's own trace log, not to the plugin log, and the setting is off by default. A span has no payload.

- **[HST-360]** Tracing MUST be off by default and MUST record no payloads.

## Reading the data

| Surface | Shows |
|---|---|
| `ilium plugin status [<id>]` | Health, 50th and 99th percentile, near-misses, traps, queue high water, peak memory, quota hits (see [Health and watchdog](/pages/reference-health-and-watchdog/)) |
| `ilium plugin stats <id> --json` | Every counter above for 24 hours, one JSON line per class (read-only; requires no grant) |
| Plugins screen, detail, Resources tab | Memory, execution bucket level, helper state |
| Diagnostics report | The `Plugins` section |
| `plugin-info` import | Health and summary numbers for the plugin that asks about itself (`plugins.manage` is needed to ask about others) |

- **[HST-361]** `ilium plugin stats <id> --json` MUST print every counter for the retained buckets as JSON lines with a `type` field, and MUST exit with code 0 when the plugin exists.
- **[HST-362]** A plugin MUST be able to read its own summary through `plugin-info` without a grant.

## Health computation inputs

Health is computed from these counters and from lifecycle state, with the thresholds in [Health and watchdog](/pages/reference-health-and-watchdog/). The host adds the following to the host-side view; they do not change the health enum.

| Signal | Source | Where shown |
|---|---|---|
| Close to deadline | Near-miss rate | Detail view, "Why" line |
| Held by quota | `quota_rejections` in the last minute | Detail view and Resources tab |
| Waiting for a helper | `helper.too-many` active | Detail view |
| Memory near the limit | `memory_peak` over 80 % of `memory_mib` | Detail view |
| Stale contributions | Stale marker active | Detail view |

- **[HST-363]** The host MUST show the signal that put a plugin in `degraded` as a "Why" line with the counter values in the detail view.

## Edge cases

| Situation | Result |
|---|---|
| The server restarts | Counters reset; summary lines from before the restart stay in the log |
| A plugin is upgraded | Counters of the old version are closed with a summary; the new version starts at zero |
| The clock jumps | Buckets use the monotonic clock; a jump does not move data between buckets |
| A plugin makes 100,000 calls a minute | Histograms stay fixed in size; only counts grow |

## Diagnosis beyond 24 hours

Hourly buckets answer questions about the last day. A plugin that degrades over several days needs a longer, coarser record.

- **[HST-524]** The host MUST fold each hourly bucket that leaves the 24-hour window into a daily summary per plugin side holding call count, failure count, near misses and the 50th, 95th and 99th percentile of call time, and MUST keep 30 daily summaries and discard older ones. A daily summary MUST add at most 4 KiB per plugin side, MUST stay on the local disk with the session data, and MUST obey the same content rules as the hourly buckets.
- **[HST-525]** `ilium plugin status <plugin-id> --days <n>` MUST print the daily summaries for up to 30 days, MUST mark a day with a restart or an upgrade, and MUST end its output with the trend of the 95th percentile call time as rising, flat or falling over the period. The summaries MUST be removed when the plugin is uninstalled.

API version checked: `ilium:plugin@0.1.0`.

## Operator observability of traps

Target behaviour. An operator must be able to see how often a plugin traps without reading every log line.

- [API-092] The host MUST keep a per-plugin, per-side counter for each trap code of [API-081] and for each restart, MUST collapse identical consecutive log lines of one trap code within 10 seconds into one line with a repeat count, and MUST retain a crash record (trap code, time, side and the last 20 log lines) for each plugin disabled by the restart rule until the plugin is enabled again or removed. `ilium plugin logs` MUST show the crash record.

## Production additions: T8-observability-perf

The registry below lists every code the host writes to a log line, with its level, its meaning and the requirement that produces it. Only codes that start with `host.` match the pattern of HST-607. The other codes are listed as they stand today; their rename needs an owner decision before the first ABI release.

| Code | Level | Meaning | Produced by |
|---|---|---|---|
| `host.trap` | error | A trap, with the trap code | HST-358 |
| `host.slow-calls` | info | Slow call report, top 3 calls of the last minute | HST-359 |
| `host.observability.dropped` | warn | Records dropped by the writer queue, one line per minute | HST-643 |
| `instance.started` | info | Instance start (not `host.` form yet) | HST-358 |
| `instance.ended` | info | Instance end with summary (not `host.` form yet) | HST-357, HST-358 |
| `trap.deadline` | warn | Soft or hard deadline breach (not `host.` form yet) | HST-358 |
| `host.late` | warn | Late call (matches the pattern) | HST-358 |
| `host.queue-timeout` | warn | Call abandoned in the queue (matches the pattern) | HST-358 |
| `events.dropped` | warn | Events dropped, with counts (not `host.` form yet) | HST-358, HST-643 |
| `limits.clamped` | warn | Limit clamped to the tier ceiling (not `host.` form yet) | HST-358 |
| `deactivate.deadline` | warn | Deactivate breach (not `host.` form yet) | HST-358 |
| `helper.died` | error | Helper death (not `host.` form yet) | HST-358 |

- **[HST-607]** Every host-written log line that reports a condition MUST carry a stable code of the form `host.<area>.<name>` matching `^host\.[a-z0-9-]+(\.[a-z0-9-]+)*$`, and the manual MUST list every code in one registry table with level, meaning and the requirement that produces it; a code MUST NOT be renamed or reused for another meaning within a major ABI version.

- **[HST-608]** The host MUST redact every log line and span attribute through the secret redactor before write, and MUST replace a matched value with `[redacted]`, including lines written by the host on behalf of a plugin.

- **[HST-609]** The trace log of `ILIUM_PLUGIN_TRACE=1` MUST be written to `<state>/plugin-trace.jsonl` with one span per line containing `trace_id`, `span_id`, `parent_id`, `plugin`, `side`, `class`, `export`, `queue_us`, `run_us`, `imports` and `outcome`, and MUST be capped at 32 MiB with the oldest half discarded on overflow.

- **[HST-610]** When tracing is on, a span MUST use the correlation id of HST-526 as `trace_id`, a span for a cross-plugin call MUST have the caller span as `parent_id`, and a span MUST be emitted exactly once per call, including calls that trap or time out.

- **[HST-612]** The host MUST expose the counters of HST-354 in Prometheus text format at `ilium plugin metrics` (stdout) with metric names prefixed `ilium_plugin_`, labels `plugin`, `side` and `class` only, and MUST NOT serve them over any network socket.

- **[HST-613]** Metric label cardinality MUST be bounded: the labels MUST NOT include export names, event kinds beyond the 32 defined kinds, error messages or user data, and the host MUST cap the metric series at 64 per plugin side.

- **[HST-614]** The histogram buckets MUST use a monotonic clock, MUST count a call in exactly one bucket, and the sum of bucket counts MUST equal the `calls` counter for the same window; a clock step backwards MUST NOT produce a negative duration (clamp to 0).

- **[HST-636]** The diagnostics bundle `ilium plugin diagnostics --out <file>` MUST contain per plugin health, metrics, last 500 log lines, effective limits, manifest and grants, MUST apply redaction (HST-608), MUST NOT contain plugin data directories, and MUST complete within 10 seconds for 64 plugins.

- **[HST-638]** A plugin MUST be able to emit a custom counter with `host::telemetry::count(name, delta)` limited to 16 distinct names (proposed, owner to confirm) per plugin side, names matching `^[a-z][a-z0-9_]{0,31}$`, and a 17th name MUST fail with `quota-exceeded`; custom counters MUST appear under the `plugin.` prefix in stats.

- **[HST-639]** Reading the counters through `ilium plugin stats` or `plugin-info` MUST take at most 5 ms for 64 plugins and MUST NOT call into any plugin or block the recording path (HST-351).

- **[HST-642]** Time used for telemetry, deadlines and spans MUST come from a monotonic clock, and the test clock `advance_clock` MUST drive histogram buckets, hourly buckets and the 24-hour window identically, so a test can age data without sleeping.

- **[HST-643]** Log, trace and telemetry writes MUST be non-blocking for the plugin call path: when the writer queue holds 8,192 records (proposed, owner to confirm) the host MUST drop the oldest records, count them in `observability_dropped` and write one `host.observability.dropped` line per minute.

- **[HST-647]** Timestamps in logs, traces and status output MUST use UTC in storage and the user's local time in terminal display, and the format `YYYY-MM-DD HH:mm:ss` MUST be used for terminal display.

## Personal data in logs and trace retention

[HST-608] redacts secrets only. The rules below cover the personal data that a support bundle can carry out of the machine, and the retention and erasure of trace spans, which hold a history of every call.

- **[HST-129]** The host MUST replace every e-mail address and every absolute path outside the user's home directory with `[redacted-email]` and `[redacted-path]` before a log line or span attribute is written, including lines written with `log`. The summary of a support bundle MUST count these replacements next to the secret redactions ([USR-335]).
- **[HST-130]** A plugin MUST NOT write personal data into a log line, a span or a custom counter: no names, message text, terminal text, prompts or contents of user files. The redactor of [HST-129] does not detect names, so this rule is the only control for them.
- **[HST-131]** The host MUST cut a log line longer than 4 KiB (proposed, owner to confirm) to its first 4 KiB, append `[truncated]`, and count the cut line under dropped log lines in `summary.txt` ([USR-335]).
- **[HST-979]** Spans in `<state>/plugin-trace.jsonl` MUST be deleted once they are more than 7 days old (proposed, owner to confirm). The host MUST run this deletion at server start. The size cap of [HST-609] still applies.
- **[HST-980]** `ilium plugin erase-data <plugin-id>` MUST remove every span of that plugin from `<state>/plugin-trace.jsonl`, by writing a new file and renaming it over the old one, and MUST report the removed count as the kind `trace` ([USR-325]). Spans of other plugins MUST stay in place.
- **[HST-981]** `ilium plugin export-data` MUST write the spans of the plugin as one JSON-lines file, using the fields of [HST-609], in the export folder ([USR-245]). Trace recordings MUST NOT be included in a support bundle ([USR-223]).


## Host-level counters (session-wide, not per plugin)

The counters above are keyed by plugin. Some incidents are not caused by a plugin: a slow machine, a hung main loop, a full memory pool or a full disk make healthy plugins look slow. This section defines the series that belong to the session server as a whole. They are printed by `ilium plugin metrics` (Prometheus text, standard output only, see [HST-612] and [HST-991]) and by `ilium plugin stats --host` (see [HST-644]), next to the per-plugin series. They carry no `plugin`, `side` or `class` label, so [HST-613] (label cardinality) is not affected, and they do not count toward the 64-series cap of a plugin side. The disk steps are defined in [Offline and low-disk behaviour](/pages/reference-offline-and-low-disk/) ([HST-661], [HST-663]); the loop watchdog is [HST-618]; the aggregate ceilings are in [Host aggregate limits](/pages/reference-host-aggregate-limits/); the audit queue is [SEC-225].

### Series

| Prometheus name | Type | Unit | Meaning |
|---|---|---|---|
| `ilium_host_loop_tick_seconds` | histogram | seconds | Duration of one kernel loop tick. Buckets 1, 2, 5, 10, 20, 50, 100, 200, 500, 1000, 2000 ms and above. `ilium plugin stats --host` prints p50 and p99 in ms, read as the upper bound of the bucket that holds the percentile |
| `ilium_host_loop_stalls_total` | counter | count | Ticks that did not complete within the watchdog threshold of 2 seconds ([HST-618]); one count per `host.loop-stalled` line |
| `ilium_host_helpers_alive` | gauge | count | Helper processes currently running |
| `ilium_host_helpers_restarted_total` | counter | count | Helper restarts, including the planned restart of an engine recycle |
| `ilium_host_memory_pool_used_bytes` | gauge | bytes | Memory reserved against the aggregate ceiling of [HST-500] |
| `ilium_host_memory_pool_ceiling_bytes` | gauge | bytes | The ceiling in force after policy keys and the tier clamp |
| `ilium_host_ceiling_rejections_total` | counter | count | Activations or reservations refused by an aggregate ceiling, label `limit` |
| `ilium_host_observability_dropped_total` | counter | count | Records lost by the host observability path, label `source` |
| `ilium_host_engine_recycles_total` | counter | count | Wasmtime Engine recycles ([HST-515]) |
| `ilium_host_disk_pressure_step` | gauge | step | Current disk-pressure step: 0 none, 1 caches evicted, 2 log writes reduced, 3 installs blocked ([HST-661]) |
| `ilium_host_clients_attached` | gauge | count | Clients currently attached to this session server |

The `limit` label of `ilium_host_ceiling_rejections_total` takes one value for each ceiling defined by [HST-500] to [HST-511] (for example `memory-pool`, `installed-plugins`, `active-sides`, `host-capacity`); the values are fixed by the host, so the label has at most 12 values. The `source` label of `ilium_host_observability_dropped_total` takes the values `audit-queue` (records dropped while the audit file could not be written, [SEC-225]), `log-queue` (the writer queue overflow of [HST-643], which is also the `host.observability.dropped` line) and `log-disk` (lines dropped under disk pressure, the number reported by the `log.dropped` line of [HST-663]). Each dropped record is counted exactly once, under the source that dropped it.

### Retention and reset

- Host series live in memory only. They are never written to disk and never sent over a network.
- They are not tied to a plugin. A plugin restart, upgrade, disable or removal does not reset or change any of them.
- They reset when the session server starts, like the per-plugin counters ([HST-357]). A client attach or detach does not reset them.
- Gauges show the current value. Counters only rise until the next server start. The tick histogram keeps one cumulative set of buckets since server start, plus the hourly buckets of the last 24 hours that `ilium plugin stats --host --json` prints as one JSON line per hour with `type` set to `host`.
- Recording a host series follows [HST-351]: at most 200 ns, no lock on the call path. Their memory counts toward the bound of [HST-644].

### Reading the host series

| Surface | Shows |
|---|---|
| `ilium plugin metrics` | Every host series beside the plugin series, with a `# HELP` and `# TYPE` line each |
| `ilium plugin stats --host` | One line per series with the current value; the tick p50 and p99 in ms |
| `ilium plugin stats --host --json` | The same as JSON lines with a `type` field |
| Diagnostics report | A `Host` block with the same values, so that a support bundle can tell a host cause from a plugin cause |

An alert rule for a plugin that is slow SHOULD first check that `ilium_host_loop_stalls_total` has not risen, that the tick p99 is below 50 ms and that `ilium_host_disk_pressure_step` is 0; if any of them is not, the cause is the host and the plugin is not at fault. A plugin whose breaches occur only while a host stall is counted MUST NOT be charged a failure for them, as for the late-call rule of [Health and watchdog](/pages/reference-health-and-watchdog/).

### Edge cases

| Situation | Result |
|---|---|
| The loop stalls for 10 seconds | One stall is counted for each tick that exceeded the threshold, and the one long tick lands in the top histogram bucket |
| The memory ceiling is lowered by policy below the used bytes | `ceiling_bytes` shows the new value at once, no plugin is evicted ([HST-511]), and each refused activation counts one rejection |
| A helper dies and is restarted | `helpers_alive` drops by one and returns, `helpers_restarted_total` rises by one, and the plugin side gets its own `helper.died` line |
| The disk recovers above 64 MiB | The step gauge returns to 0 within 30 s of [HST-663], and the dropped log lines are added once to `log-disk` |
| The metrics command runs while the server is not running | The command exits with code 1 and prints no series; it never invents zero values |

### Test

Scripted incident test, `host_telemetry_series`. On a fixture session with one healthy plugin, one memory pool of 64 MiB and a volume that the test can fill:

1. Block the kernel loop for 3 seconds with a slow synchronous host call. Assert that `ilium_host_loop_stalls_total` is 1, the tick p99 is at least 2,000 ms, and `host.loop-stalled` was written.
2. Request activations until the pool ceiling is hit. Assert that `ilium_host_memory_pool_used_bytes` is at most `ilium_host_memory_pool_ceiling_bytes`, that `ilium_host_ceiling_rejections_total{limit="memory-pool"}` equals the number of refusals, and that no active plugin was stopped.
3. Fill the volume to below 128 MiB. Assert that `ilium_host_disk_pressure_step` is 2 and that dropped log lines raise `ilium_host_observability_dropped_total{source="log-disk"}`. Free the volume and assert that the gauge is 0 and one `log.dropped` line was written.
4. Kill one helper, force one engine recycle and attach a second client. Assert the helper, recycle and client series.
5. Read every series with `ilium plugin metrics` and with `ilium plugin stats --host --json`, and assert that all 11 names are present with the correct types, and that the values of both outputs agree.
6. Restart the plugin and assert that no host series changed except by the steps above; restart the server and assert that every counter is 0.
