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
| Memory near the limit | `memory_peak` over 90 % of `memory_mib` | Detail view |
| Stale contributions | Stale marker active | Detail view |

- **[HST-363]** The host MUST show the signal that put a plugin in `degraded` as a "Why" line with the counter values in the detail view.

## Edge cases

| Situation | Result |
|---|---|
| The server restarts | Counters reset; summary lines from before the restart stay in the log |
| A plugin is upgraded | Counters of the old version are closed with a summary; the new version starts at zero |
| The clock jumps | Buckets use the monotonic clock; a jump does not move data between buckets |
| A plugin makes 100,000 calls a minute | Histograms stay fixed in size; only counts grow |

API version checked: `ilium:plugin@0.1.0`.
