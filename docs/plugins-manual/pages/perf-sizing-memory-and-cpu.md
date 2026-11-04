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

- **[PRF-001]** An instance MUST reserve its effective `memory_mib` from the session memory pool when it activates, and MUST release that reservation when it deactivates. Test: with a pool of 1,024 MiB and eight instances of 128 MiB each, the ninth activation stays `inactive` with the reason `host-capacity` and no failure is counted; deactivating one of the eight lets the ninth start.
- **[PRF-002]** When an instance grows its memory past its effective `memory_mib`, the host MUST trap the instance and count the trap as a failure. Test: a fixture that allocates 33 MiB with `memory_mib` 32 traps; the same fixture with `memory_mib` 64 passes.

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

- **[PRF-003]** A plugin SHOULD set `memory_mib` to no more than 1.5 times its measured peak on a release build, after the heaviest realistic input. Test: a fixture reports a peak of 20 MiB; the manifest value is at most 30 MiB.

Rule AUT-102 in [Performance](/pages/performance/#one-table-of-budgets) asks for a plugin to stay under half of each default budget in normal use. The two rules fit together. When the default is too small for the measured peak, raise the value to the target from step 2. When the peak is under half the default, leave the default alone.

### The execution bucket

Every call spends thread CPU time from one bucket for the plugin side. The bucket holds 2,000 ms and refills at 500 ms per second. A plugin that keeps its average spend below the refill rate never empties the bucket. A plugin that spends faster than that for a few seconds only empties it after the first 2,000 ms.

A handler that spends 20 ms on every event batch uses 20 ms of the bucket each time. At 10 batches per second, it spends 200 ms per second, which is 40 percent of the refill rate. That is a comfortable steady state. At 40 batches per second it spends 800 ms per second and empties the bucket within a few seconds, after which submissions are refused.

- **[PRF-004]** A plugin SHOULD keep its average spend on the execution bucket below 250 ms per second, measured over 60 seconds in normal use, which is half of the refill rate. (proposed, owner to confirm: the 250 ms figure is a proposal; the specification fixes only the bucket size and refill rate.) Test: a fixture that runs its normal event load for 60 seconds reports a spend under 15,000 ms in total.
- **[PRF-005]** A call that is refused with `quota-exceeded` MUST NOT be retried in a loop. The plugin MUST wait at least `retry-after-ms` before it submits again. Test: a submission loop against an empty bucket makes no more submissions in the refusal window than the `retry-after-ms` allows.

The retry pattern is in [Backpressure and retries](/pages/perf-backpressure-and-retries/).

### Keep handlers short, move work to jobs

A handler has a 20 ms deadline per event batch. A plugin that does more than that work inside the handler breaches the deadline and is restarted. The fix is to split the work. A short handler reads what it needs and starts a job. The job runs with its own CPU and memory budgets. The procedure is in [Moving slow work into a job](/pages/performance/#moving-slow-work-into-a-job).

Choose the job budgets with the same procedure as the memory value. Measure the job CPU time and its peak memory on a release build, then set `job_deadline_ms`, `job_wall_ms` and `job_memory_mib` in the manifest to the measured value with headroom. Keep `job_wall_ms` above `job_deadline_ms`, because a job that waits for I/O uses wall time and not CPU time.

- **[PRF-006]** A plugin MUST NOT run a loop over its whole data set inside `on-events` or `views.render`. Work over a data set MUST run in a job. Test: a fixture with 100,000 items makes `on-events` return within 20 ms, and the same work appears as a running job in `ilium plugin stats`.

### Job pool sizing

Each plugin side runs its jobs in a second instance of its component, and at most `concurrent_jobs` of them are live at once ([Background jobs](/pages/api-jobs/)). Size the pool of one side with two numbers: the jobs it can run at once, and the memory each one may use. The worst case is `concurrent_jobs` times `memory_mib`.

| Tier | Default `concurrent_jobs` | Ceiling `concurrent_jobs` | Ceiling job `memory_mib` | Worst case at the ceilings |
|---|---|---|---|---|
| T1 | 2 | 8 | 512 MiB | 4,096 MiB |
| T2 and T3 | 2 | 2 | 256 MiB | 512 MiB |

The aggregate page leaves one question open: whether the memory of a job is counted apart from its plugin side's instance reservation ([Host aggregate limits](/pages/reference-host-aggregate-limits/)). Its default is that it is counted within that reservation. This page uses that default for the session memory pool, which sees the instance reservation only. The machine-wide ceiling counts the reservation of each running job. If the open question is decided the other way, the worst case in the table is the charge per plugin side.

A job start is refused in a fixed order. The job pages list the codes but not their order, so this order is proposed for sizing:

1. The input is over the input cap: `invalid-argument`, at the call.
2. The side already has `concurrent_jobs` live jobs: the job waits in the queue.
3. The queue already holds `queued jobs` entries: `busy`.
4. A slot is free but the machine-wide ceiling for memory or threads has no room: `host-capacity`. This is not a failure ([HST-504](/pages/reference-host-aggregate-limits/)).
5. The running job goes over its `memory_mib`: `failed(quota-exceeded)`. It goes over its CPU `deadline_ms`: `failed(timed-out)`.

- **[PRF-041]** A plugin author MUST size the job pool of each side as the worst case of `concurrent_jobs` times `memory_mib`, and MUST state that worst case in MiB in the commit or review that changes either value. Test: a review checklist item for a change of `concurrent_jobs` or `memory_mib` requires the worst-case figure in MiB to appear in the commit message.
- **[PRF-042]** A job start MUST be refused in the order of the five steps of this section: an oversized input first, then queueing, then `busy` when the queue is full, then `host-capacity` when the machine-wide ceiling has no room, and a runtime quota or timeout only once the job runs. (proposed, owner to confirm: the order of steps 2 to 4 is a default.) Test: with two live jobs and a full queue of 16 entries, a 17th start returns `busy`, and a start with an oversized input returns `invalid-argument` even while the queue is full.
- **[PRF-043]** A job start refused with `host-capacity` MUST NOT count as a failure, and the same start MUST succeed once memory or threads are released. Test: fill the machine-wide ceiling with running jobs, start one more and assert `host-capacity` with no failure counted; release one job and assert the same start succeeds.

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

### Tree scale

A plugin with `tree.read` can see every node of the project tree, so a large tree makes its reads and its change events larger. The manual states no ceiling on the number of nodes in a session. The numbers below are proposed defaults that a plugin can plan for.

| Item | Proposed default | What happens at the default |
|---|---|---|
| Nodes in one session tree | 10,000 tested | No node is refused. The benchmark B25 is recorded at this size. The budgets on this page are not promised above it. |
| Result of `find` | 4,096 ids | The first 4,096 ids in tree order are returned, with no error. |
| Ids in one `tree-changed` event | 4,096 ids | Further ids go into more events, in first-seen order. |

- **[PRF-037]** A `find` that matches more than 4,096 nodes MUST return the first 4,096 node ids in tree order and MUST NOT return an error. (proposed, owner to confirm: whether `find` truncates or refuses is an open decision; this is the default.) Test: a tree of 5,000 nodes and a `find` with every field unset returns 4,096 ids, and the first of them is the first node in tree order.
- **[PRF-038]** When the pending `tree-changed` ids of one plugin side would exceed 4,096 entries, the host MUST deliver them in more than one `tree-changed` event, each holding at most 4,096 ids in first-seen order, and MUST NOT drop an id. (proposed, owner to confirm: the 4,096 figure is a default, and the rule replaces the single merged event for a larger burst.) Test: a burst that changes 10,000 nodes delivers ids in three events that cover each of the 10,000 ids exactly once.
- **[PRF-039]** A plugin MUST NOT call `find` with every field unset, or walk `children` over the whole tree, inside `on-events` or `views.render`. It MUST build its index once, in an activation or a job, and update it from `tree-changed` and `node-removed`. Test: a fixture that walks a 5,000 node tree inside `on-events` is trapped at the 20 ms deadline, and the same fixture with an index built in a job is not.
- **[PRF-040]** The benchmark for a 10,000 node tree MUST record, without a pass or fail threshold, the median and the 99th percentile of one `children` call on the root and of the delivery of one `tree-changed` event with 1,000 ids. Test: the benchmark result holds both numbers for the run, and the run does not fail on their values.

### Limits of this page

- Whether the memory of a job is counted apart from its plugin side's reservation is open in [Host aggregate limits](/pages/reference-host-aggregate-limits/). This page uses the default. The worst case in the job pool table is the figure to use if the answer changes.
- The thresholds of the tree scale section (4,096 ids and 10,000 nodes) are proposed defaults, not measured limits.
- The battery and thermal rules are on [Power, battery and thermal behaviour](/pages/reference-power-and-battery/). They are proposed, not shipped.
- The session pool and the tier ceilings are fixed by the host. Changing a user's pool size is a user action under `[plugins.limits]`, not a plugin decision.
- The pool figure for T2 and T3 helper processes is not stated the same way in every page. [Performance](/pages/performance/#costs-by-tier-and-language) says that a helper charges the sum of its hosted instances' `memory_mib` plus 32 MiB against the session pool. [Resource accounting](/pages/reference-resource-accounting/#session-memory-pool) says that each instance reserves its configured limit at activation. This page follows the second. (proposed, owner to confirm: which statement is the rule for T2 and T3 helpers.)

### See also

- [Power, battery and thermal behaviour](/pages/reference-power-and-battery/) for the budgets that apply on battery.
- [Backpressure and retries](/pages/perf-backpressure-and-retries/) for what a plugin does when a budget refuses a call.
- [Benchmark workflow](/pages/perf-benchmark-workflow/) for timing a handler against its deadline.
- [Performance](/pages/performance/) for the deadlines and the single table of budgets.
- [Resource accounting](/pages/reference-resource-accounting/) for the counters and the admission order of the session pool.
- [Errors and limits](/pages/reference-errors-and-limits/) for every limit key and its error.
