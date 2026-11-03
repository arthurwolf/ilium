---
layout: page
title: Sizing memory and CPU
permalink: /pages/perf-sizing-memory-and-cpu/
---

## Sizing memory and CPU

A plugin has four budgets that can stop it: the memory of each instance, the shared session memory pool, the CPU that all of its calls spend from one bucket, and the CPU and memory of its jobs. This page shows how the host charges each one, how to measure your plugin against them, and how to choose the `limits` values to put in the manifest. The numbers themselves are in [Performance](/pages/performance/) and [Errors and limits](/pages/reference-errors-and-limits/). This page explains what they mean for a plugin you are sizing.

### You need

- A plugin that builds and runs. The toolchain is in [Getting started: toolchain](/pages/getting-started-toolchain/).
- The `ilium plugin stats` command, specified in [Resource accounting](/pages/reference-resource-accounting/).
- The tier of your plugin. A bundled plugin is T1, a plugin signed by a trusted publisher key is T2, and an unsigned local package is T3. The ceilings differ by tier ([Performance](/pages/performance/#limits-you-can-change)).

### Which budget charges what

| Budget | What is charged | Default | Scope | What a breach does |
|---|---|---|---|---|
| `memory_mib` | Linear memory and tables of one instance | 32 MiB | One instance | Trap, counted as a failure |
| Session memory pool | `memory_mib` of every instance, reserved when it activates | 1,024 MiB | One session server | Instance stays `inactive` with `host-capacity`, not a failure |
| `ilium-execution` bucket | Thread CPU time of each call, and 1 ms per 64 KiB of submitted payload | 2,000 ms capacity, refills at 500 ms per second | One plugin side in one process | New submissions return `quota-exceeded` with `retry-after-ms` |
| Job CPU and wall time | CPU time and elapsed time of one job | 2 s CPU, 20 s wall | One job | `failed(timed-out)` |
| Job memory | Memory of one job | 64 MiB | One job | `failed(quota-exceeded)` |
| Handler deadlines | Wall time from the start of a call to its return | 20 ms events, 10 ms render, 2 s activate | One call | Trap for the hard deadlines |

The session pool is the only budget shared by all plugins. The execution bucket is shared by the calls of one plugin side, so a slow handler in one side does not spend the bucket of the other side.

### Memory is reserved, not used

The host reserves the configured `memory_mib` of an instance when the instance activates. It does not reserve what the instance uses. A plugin with `memory_mib` 128 and an actual peak of 9 MiB still takes 128 MiB of the pool. Set the value to what the plugin needs with headroom, and no more. A larger value takes capacity from every other plugin.

The reservation is released when the instance deactivates. Memory that the plugin frees during a run does not return to the pool until then.

- **[PERF-001]** An instance MUST reserve its effective `memory_mib` from the session memory pool when it activates, and MUST release that reservation when it deactivates. Test: with a pool of 1,024 MiB and eight instances of 128 MiB each, the ninth activation stays `inactive` with the reason `host-capacity` and no failure is counted; deactivating one of the eight lets the ninth start.
- **[PERF-002]** When an instance grows its memory past its effective `memory_mib`, the host MUST trap the instance and count the trap as a failure. Test: a fixture that allocates 33 MiB with `memory_mib` 32 traps; the same fixture with `memory_mib` 64 passes.

### Measuring memory

Run `ilium plugin stats <id>` while the plugin does its normal work. It prints the current use and the effective limit of each resource, with the source of the limit ([Resource accounting](/pages/reference-resource-accounting/#reading-the-numbers)). Read the memory line after the heaviest handler you have, such as a first index build or a large render, not only after activation.

<pre class="term"><span class="t-prompt">$</span> ilium plugin stats com.example.index
<span class="t-dim">RESOURCE     USED     LIMIT    SOURCE</span>
memory       <span class="t-hi">19.0 MiB</span> 32 MiB   default
exec bucket  1240 ms  2000 ms  fixed
storage      3.1 MiB  16 MiB   default
disk         12 MiB   256 MiB  default
</pre>

Illustrative, not generated. The plugin uses 19 MiB of its 32 MiB default. That is 59 percent, so it is above the half-of-default target in the next section. The `--json` form prints the same numbers as `plugin-stat` lines, and `source` is one of `default`, `manifest`, `user`, `ceiling` or `fixed`.

<div class="note">
<p>Measure on a release build. A debug build uses more memory and runs slower, so a debug measurement overstates both. Rule AUT-105 in <a href="/pages/performance/">Performance</a> says the same for timings.</p>
</div>

### Choosing a memory value

Choose the value from the measured peak, not from the ceiling. Use this procedure:

1. Measure the peak memory after the heaviest realistic input, with `ilium plugin stats` on a release build.
2. Round the peak up to the next whole MiB and add a headroom of 50 percent. This is the target `memory_mib`.
3. If the target is above the default of your tier's ceiling, reduce the working set first, for example by streaming a file in chunks instead of reading it whole. A value above the ceiling is clamped, and the clamp is logged once per start ([Resource accounting](/pages/reference-resource-accounting/#which-limit-wins)).
4. Write the value in the manifest `limits` block. Do not write a value above the target for safety. A bigger value only makes the pool smaller for everyone.

- **[PERF-003]** A plugin SHOULD set `memory_mib` to no more than 1.5 times its measured peak on a release build, after the heaviest realistic input. Test: a fixture reports a peak of 20 MiB; the manifest value is at most 30 MiB.

Rule AUT-102 in [Performance](/pages/performance/#one-table-of-budgets) asks for a plugin to stay under half of each default budget in normal use. The two rules fit together. When the default is too small for the measured peak, raise the value to the target from step 2. When the peak is under half the default, leave the default alone.

### The execution bucket

Every call spends thread CPU time from one bucket for the plugin side. The bucket holds 2,000 ms and refills at 500 ms per second. A plugin that keeps its average spend below the refill rate never empties the bucket. A plugin that spends faster than that for a few seconds only empties it after the first 2,000 ms.

A handler that spends 20 ms on every event batch uses 20 ms of the bucket each time. At 10 batches per second, it spends 200 ms per second, which is 40 percent of the refill rate. That is a comfortable steady state. At 40 batches per second it spends 800 ms per second and empties the bucket within a few seconds, after which submissions are refused.

- **[PERF-004]** A plugin SHOULD keep its average spend on the execution bucket below 250 ms per second, measured over 60 seconds in normal use, which is half of the refill rate. (proposed, owner to confirm: the 250 ms figure is a proposal; the specification fixes only the bucket size and refill rate.) Test: a fixture that runs its normal event load for 60 seconds reports a spend under 15,000 ms in total.
- **[PERF-005]** A call that is refused with `quota-exceeded` MUST NOT be retried in a loop. The plugin MUST wait at least `retry-after-ms` before it submits again. Test: a submission loop against an empty bucket makes no more submissions in the refusal window than the `retry-after-ms` allows.

The retry pattern is in [Backpressure and retries](/pages/perf-backpressure-and-retries/).

### Keep handlers short, move work to jobs

A handler has a 20 ms deadline per event batch. A plugin that does more than that work inside the handler breaches the deadline and is restarted. The fix is to split the work. A short handler reads what it needs and starts a job. The job runs with its own CPU and memory budgets. The procedure is in [Moving slow work into a job](/pages/performance/#moving-slow-work-into-a-job).

Choose the job budgets with the same procedure as the memory value. Measure the job CPU time and its peak memory on a release build, then set `job_deadline_ms`, `job_wall_ms` and `job_memory_mib` in the manifest to the measured value with headroom. Keep `job_wall_ms` above `job_deadline_ms`, because a job that waits for I/O uses wall time and not CPU time.

- **[PERF-006]** A plugin MUST NOT run a loop over its whole data set inside `on-events` or `views.render`. Work over a data set MUST run in a job. Test: a fixture with 100,000 items makes `on-events` return within 20 ms, and the same work appears as a running job in `ilium plugin stats`.

### Check the limits with lint

`ilium plugin lint` reports static weak spots, including `lint.limit-above-ceiling` (a manifest value over the ceiling of its tier) and `lint.activation-startup` (an activation that does too much at startup). Run it after every change to the `limits` block. A value over the ceiling is clamped by the host and never refused, so a lint finding is the only early sign of a value that does not do what you wrote.

### Worked example: sizing an index plugin

The plugin `com.example.index` keeps an in-memory index of about 40,000 notes. Its default `memory_mib` of 32 MiB is too small.

1. The first index build on a release build, with the largest test corpus, reports a peak of 37 MiB with `ilium plugin stats`.
2. Step 2 of the procedure gives 37 MiB plus 50 percent, which is 55.5 MiB. Round up to 56 MiB.
3. A T3 plugin has a ceiling of 128 MiB, so 56 MiB is allowed.
4. The manifest sets `"limits": { "memory_mib": 56 }`. The index build now runs in the instance, and the rest of the plugin uses the same limit.
5. The change also moves the build into a job, so `on-events` only starts it. The job has `job_memory_mib` of 64 MiB, the default, which covers the build's measured peak.

The plugin now takes 56 MiB of the pool instead of 32 MiB. With the default pool, that still leaves room for about 17 more plugins of the default size before the pool is full.

### Limits of this page

- The session pool and the tier ceilings are fixed by the host. Changing a user's pool size is a user action under `[plugins.limits]`, not a plugin decision.
- The pool figure for T2 and T3 helper processes is not stated the same way in every page. [Performance](/pages/performance/#costs-by-tier-and-language) says that a helper charges the sum of its hosted instances' `memory_mib` plus 32 MiB against the session pool. [Resource accounting](/pages/reference-resource-accounting/#session-memory-pool) says that each instance reserves its configured limit at activation. This page follows the second. (proposed, owner to confirm: which statement is the rule for T2 and T3 helpers.)

### See also

- [Backpressure and retries](/pages/perf-backpressure-and-retries/) for what a plugin does when a budget refuses a call.
- [Benchmark workflow](/pages/perf-benchmark-workflow/) for timing a handler against its deadline.
- [Performance](/pages/performance/) for the deadlines and the single table of budgets.
- [Resource accounting](/pages/reference-resource-accounting/) for the counters and the admission order of the session pool.
- [Errors and limits](/pages/reference-errors-and-limits/) for every limit key and its error.
