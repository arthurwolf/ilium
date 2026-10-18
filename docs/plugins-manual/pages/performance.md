---
layout: page
title: Performance
permalink: /pages/performance/
---

## Performance

Every plugin call runs under a deadline, and every plugin has a memory, queue and I/O budget. A call that exceeds its hard deadline traps the instance, and the host restarts the plugin (a `deactivate` breach instead drops the instance and is not a failure). Keeping each call short is the only way to keep a plugin responsive. This page lists the deadlines and budgets, the performance targets the host checks, where time is charged, and how to move slow work into a job.

### You need

- A plugin that builds and runs, so you can measure where its time goes. The toolchain is in [Getting started: toolchain](/pages/getting-started-toolchain/).
- The `jobs.run` grant for any background work, and `fs.read:project` for the example in this page. Grants are listed in [Capability table](/pages/reference-capabilities/).
- Rust with `ilium-plugin-sdk`, or TypeScript with `@ilium/plugin-sdk`. Both examples target the API version `ilium:plugin@0.1.0`.
- For the extended-widget costs below: knowledge of the widgets in [Widget catalogue](/pages/widgets-index/).

## Deadlines and budgets

The values below are the defaults, copied from the single table in [Scheduling, deadlines and cancellation](/pages/reference-host-scheduling/#call-classes). The manifest `limits` block can ask for a smaller value, or a larger one up to the ceiling for the plugin's tier (see [Limits you can change](#limits-you-can-change)).

| Call | Default deadline | Notes |
|---|---|---|
| `on-events` | 20 ms | One batch of up to 256 events. The host splits a larger batch. |
| `on-events` with a command | 50 ms | A batch that contains a `command-invoked` event runs under `command_deadline_ms` instead of 20 ms. |
| `views.render` | 10 ms | Called on open, on `ui.invalidate` and on resize only. |
| `annotate` | 10 ms | At most once per 100 ms per buffer. A breach skips the annotation; the third in a row traps. |
| `activate` | 2 s | May yield. |
| `deactivate` | 500 ms | Returned bytes are kept, up to 1 MiB. A breach drops the instance and the bytes, and does not count as a failure. |
| Scene `render` (frame) | The frame budget, at most 50 ms | Breached frames are skipped. |
| Job, CPU time | 2 s | Runs on a worker, not in a handler. |
| Job, wall time | 20 s | Includes time spent waiting for I/O. |
| Job, memory | 64 MiB | Concurrent jobs per plugin: 2. |

A breach of a hard handler deadline is logged, the instance is trapped and the plugin restarts. A soft deadline (`annotate`, scene frames, providers) discards the late result and traps only after the streak or a 100 ms grace in the scheduling table. A `deactivate` breach never counts as a failure. The first restart waits 1 s and the second 5 s. A third failure inside 10 minutes disables the plugin on that side until the user turns it on again. A job failure never counts toward this rule.

<pre class="term"><span class="t-dim">2026-10-11 10:02:17</span> <span class="t-err">error</span> deadline breach in on-events: limit 20 ms
<span class="t-dim">2026-10-11 10:02:17</span> <span class="t-warn">warn</span>  restarting in 1 s (failure 1 of 3 in 10 minutes)
<span class="t-dim">2026-10-11 10:02:18</span> <span class="t-ok">ok</span>    example.notes is running again
<span class="t-dim">2026-10-11 10:09:02</span> <span class="t-err">error</span> deadline breach in on-events: limit 20 ms
<span class="t-dim">2026-10-11 10:09:02</span> <span class="t-warn">warn</span>  plugin example.notes is now disabled (failure 3 of 3)
</pre>

Illustrative, not generated. The first breach restarts the plugin after 1 s. The third breach inside 10 minutes disables it.

## Limits you can change

The manifest `limits` block sets a value per plugin and side. The host clamps a request above the ceiling to the ceiling and logs the clamp. It never refuses the manifest for that reason. The user can set the same keys under `[plugins.limits]` in `config.toml`, for every plugin or for the whole session.

```json
{
  "limits": { "event_deadline_ms": 40, "job_wall_ms": 60000 }
}
```

| Key | Default | Ceiling, T1 | Ceiling, T2 and T3 | Effect when breached |
|---|---|---|---|---|
| `event_deadline_ms` | 20 | 50 | 20 | Trap |
| `command_deadline_ms` | 50 | 100 | 50 | Trap |
| `render_deadline_ms` | 10 | 20 | 10 | Trap |
| `activate_deadline_ms` | 2000 | 5000 | 2000 | Trap |
| `job_deadline_ms` (CPU) | 2000 | 60000 | 10000 | `failed(timed-out)` |
| `job_wall_ms` | 20000 | 600000 | 100000 | `failed(timed-out)` |
| `job_memory_mib` | 64 | 512 | 256 | `failed(quota-exceeded)` |
| `job_io_mib` (input and output, each) | 16 | 64 | 16 | `invalid-argument` |
| `concurrent_jobs` | 2 | 8 | 2 | Further jobs queue |
| `job_queue` | 16 | 64 | 16 | `busy` |

The contract also has nine requestable limits in the same form. They bound the host services that cost the most.

| Key | Default | Ceiling, T1 | Ceiling, T2 and T3 | Effect when breached |
|---|---|---|---|---|
| `http_streams` (open streams) | 4 | 16 | 4 | `quota-exceeded` |
| `children` (spawned child processes) | 2 | 8 | 2 | `quota-exceeded` |
| `durable_timers` | 64 | 256 | 64 | `quota-exceeded` |
| `vector_indexes` | 4 | 16 | 4 | `quota-exceeded` |
| `vector_entries` (all indexes) | 10,000 | 200,000 | 50,000 | `quota-exceeded` |
| `shared_cache_mib` | 64 | 256 | 128 | Oldest entries evicted |
| `journal_entries` (per pane) | 5,000 | 20,000 | 5,000 | Oldest trimmed |
| `toasts_pending` | 3 | 3 | 3 | Oldest replaced |
| `package_assets_mib` | 64 | 256 | 64 | `manifest.asset-size` |
| `event_queue` | 1,024 | 4,096 | 1,024 | Stuck plugin is restarted |
| `ui_tree_kib` | 1,024 | 4,096 | 1,024 | Trap (`ui.too-large`) |
| `ui_tree_nodes` | 4,096 | 16,384 | 4,096 | Trap (`ui.too-large`) |
| `table_elements` | 10,000 | 50,000 | 20,000 | Instance trapped |

A bundled plugin with the top-level manifest key `limits_tier: "first-party"` also lifts the settings caps and the `list-editor` cap. That tier is for bundled `ilium.` plugins only.

Tier T1 is a bundled plugin. T2 is signed by a publisher key the user trusts. T3 is an unsigned local package. A per-kind `deadline_ms` or `memory_mib` in `contributes.jobs` overrides the job defaults for that kind, within the same ceilings.

## Fixed caps

These caps do not change. A value over a per-call cap that the plugin can know in advance is refused with `invalid-argument`, and nothing is written or sent.

| Item | Cap | Error when exceeded |
|---|---|---|
| Storage key / value | 256 bytes / 1 MiB | `invalid-argument` |
| Storage bucket (default quota) | 16 MiB | `quota-exceeded` |
| Node metadata value | 64 KiB per node | `quota-exceeded` |
| `fs.read` per call | `max-bytes` up to 1 MiB, `max-entries` up to 4,096 | `invalid-argument` |
| `fs.read-all` file (default limit) | 64 MiB | `failed(quota-exceeded)` |
| HTTP response (default limit) | 8 MiB | `failed(quota-exceeded)` |
| Topic payload | 64 KiB | `invalid-argument` |
| Log line | 16 KiB, cut and marked | Line cut, not refused |
| Log lines per second, third-party | 200 | Dropped and counted |
| Dedup window for `dedup-key` | 10 minutes | A retry after the window is a new submission |
| `check-list` items / `list-editor` items | 256 / 64 | Render traps with `ui.invalid-tree` |
| Popups per view | 4 | `quota-exceeded` |
| Minimap marks per layer, meter segments | 4,096 / 8 | `invalid-argument` |
| Catalogue page | 256 items | Clamped |
| `read-cells` rows per call | 1,000 | `none`, and the host logs `quota-exceeded` |
| `history-text` | 4 MiB, newest kept | Truncated |
| `write-glyphs` cells per frame | 131,072 | `quota-exceeded` |
| Pre-rendered scene loop | 30 fps, 3,600 frames, 128 MiB | `manifest.scene-prerender` |
| HTTP stream chunk | 64 KiB | Split (`max-chunk-bytes` is a request parameter bounded by this cap) |
| `vector.embed` | 256 texts of at most 8 KiB | `invalid-argument` |

## Further deadlines and rates

New provider exports run under short deadlines. A breach counts like any other deadline breach, except that `menu-items` and `tokens` fall back to static content instead of failing the user action.

| Call | Deadline |
|---|---|
| `catalog-provider.query-catalog`, `menu-provider.menu-items`, `virtual-rows-provider.list-virtual-children` | 20 ms |
| `token-provider.tokens` | 10 ms per call |
| `settings-validate` (the optional export that checks a settings save) | 100 ms; a breach counts like any other deadline breach, and the save fails and keeps the previous value |
| `client-requests.request` timeout | 1 to 30,000 ms, set per request |

The host also limits how often a plugin may call some imports. A call over the rate is refused with `rate-limited` and `retry-after-ms`, so a plugin that updates a widget in a loop spends nothing on the refused calls.

| Rate | Limit |
|---|---|
| `toast` | 3 per 10 s per plugin |
| `geocode` | 1 request per second per plugin |
| `set-status-segment`, `set-frame-slot` | 10 per second per slot |
| `set-row-state`, `set-item-states`, `set-choice-items`, `set-info-rich`, `set-meter`, `set-info` | 10 per second per plugin, together |
| `journal.log-event` | 20 per second per plugin |
| `screen-changed` delivery | At most 10 per second per pane |

Session-wide, at most 64 HTTP streams and 16 spawned children are open across all plugins, and a pane has one input lock at a time and the session has no further cap on locks.

## What the new widgets and services cost

Most of the 51 widgets cost nothing between renders, because the host draws the tree. The table lists where a plugin does pay.

| Feature | Where the cost lands | Advice |
|---|---|---|
| `spinner`, decoration frame animation | The host steps frames. The plugin is not called per frame. | Use them instead of a timer that calls `ui.invalidate` every 100 ms. |
| `chart` with many points | Render size: the tree limit is 4,096 nodes or 1,024 KiB. | Send a series with `resample: min-max` and let the host reduce it to the width of the chart. |
| `check-list`, `list-editor`, `%list` | Items are part of the tree. | `check-list` is capped at 256 items and `list-editor` at 64; a larger one traps at render time with `ui.invalid-tree`. A `%list` has no item cap, but the whole tree is limited to 4,096 nodes and 1,024 KiB by default. Render a window of rows from the host-owned `offset` (read through `ui.widget-state`) and add a Load more row. Virtual-row providers are for sidebar rows of the tree only. See recipe 4 of the [Patterns cookbook](/pages/guide-patterns-cookbook/). |
| `choice` with a catalogue `source` | The host pages the catalogue, 256 items per page, with a 20 ms provider deadline. | Return one page per call. Never build the whole catalogue in `query-catalog`. |
| `terminal-view`, `video`, `surface` | Native decode and frame pacing. The plugin pays only for the frames it writes. | Prefer `prerender` for a scene that loops, so `render` is called once per frame and then never again. |
| `screen-changed`, `http-stream-data`, `child-output` | Event volume. | Read only the rows in `first-row` to `last-row`. Handle a whole batch in one call. |
| `set-meter`, `set-row-state` and friends | Rate limit, 10 per second together. | Update when a value changes by a visible step, not on every sample. |
| `vector` index | The host does the exact cosine search. | Stay under the `vector_entries` limit. Call `embed` with up to 256 texts at once. |
| `process.spawn` | Low priority under the plugin CPU quota. | Use `line-mode` so the plugin gets whole lines. |

<div class="note">
<p>The deadline is wall time from the start of your call to its return. Host work that runs inside a synchronous import counts, because your code is waiting for it. Host work that finishes after your call returned does not count: the rendering of a <code>surface</code> frame, a <code>chart</code> resample, a <code>video</code> decode, an HTTP request and the application of your submissions. A call that returns a request id costs you only the call. See <a href="/pages/reference-host-scheduling/">Host scheduling</a>.</p>
</div>

## One table of budgets

The limits of a plugin are spread over several pages. This table puts the ones that decide whether a plugin stays alive in one place, with the action the host takes. The values are defined in [Errors and limits reference](/pages/reference-errors-and-limits/) and repeated here only so that you can compare them. When the two pages differ, the reference wins.

| Budget | Default | Ceiling, T1 | Ceiling, T2 and T3 | When exceeded |
|---|---|---|---|---|
| `memory_mib` (one instance) | 32 | 256 | 128 | Trap, counts as a failure |
| Session memory pool (all instances) | 1,024 MiB | 4,096 MiB, all tiers (user config only) | same, no per-tier ceiling | The new instance is refused with `session.memory-exceeded`; no failure. Each instance reserves its `memory_mib` at activation |
| `storage_bucket_mib` (one bucket) | 16 | 256 | 16 | `put` fails with `quota-exceeded` |
| Restore bytes from `deactivate` | 1 MiB | 1 MiB | 1 MiB | Dropped, one warn line |
| `deactivate_deadline_ms` | 500 | 1,000 | 500 | Instance dropped, no restore bytes |
| `ilium-execution` quota group | shared | shared | shared | Submissions return `quota-exceeded` with `retry-after-ms` |

A plugin may ask for more in its manifest, and the host clamps the request to the ceiling of its tier (see [Limits you can change](#limits-you-can-change)). A plugin MUST be designed to run within the default of its tier, not within the ceiling. [AUT-101]

A plugin SHOULD stay under half of each default budget in normal use, so that a bad input does not trap it. [AUT-102]

## Caching: where to keep rebuildable data

Storage has three durable buckets (`global`, `project`, `session`), one cache bucket (`shared`) and the `/cache` folder of the `fs` scope. They differ in who clears them and what they count against.

| Place | Cleared by the host | Counts against | Use for |
|---|---|---|---|
| `global`, `project`, `session` | Never | `storage_bucket_mib` | Data you cannot rebuild |
| `shared` bucket | At any time, oldest first | `shared_cache_mib` | Small rebuildable values shared by all sessions |
| `/cache` in `fs` | At any time | `disk_mib`, with `data` | Large rebuildable files |
| Memory | On restart | `memory_mib` | Data rebuilt per run |

Rebuildable data MUST NOT go into a durable bucket, as rule AUT-008 of the cookbook also requires, because it uses up the 16 MiB that user data needs. [AUT-103] The recipe with the lease and the time limit is in the [Patterns cookbook](/pages/guide-patterns-cookbook/).

## Measuring from outside the plugin

The host and the command line give four ways to see where time and memory go. They are specified in [Plugin command-line reference](/pages/reference-plugin-cli/) and [Resource accounting](/pages/reference-resource-accounting/).

| What you want | Command or call | What it shows |
|---|---|---|
| Time of one handler while you build | `clock.monotonic-millis` around the work, shown in a `meter` or written to the log | Your own number, available in every build |
| Timings while you edit | `ilium plugin dev --timings` | Time per export call during a development session |
| A profile of a running plugin | `ilium plugin profile <id> --duration <seconds>` | One timing record per export that ran |
| Use against the limits | `ilium plugin stats <id>` | Current use and the effective limit of each resource |
| Static weak spots | `ilium plugin lint` | Rules such as `lint.limit-above-ceiling` and `lint.activation-startup` |

A debug build (`ilium plugin build --debug`) is slower than a release build. Timings for comparison with the targets MUST come from a release build. [AUT-105]

A plugin that reports its own timing SHOULD read it with `monotonic-millis` and SHOULD NOT depend on a host tool being installed in the user's setup. [AUT-104]

## Measuring a handler

The `clock` import gives a monotonic millisecond counter. A plugin can time its own work and show the result in a `meter`, so a slow handler is visible long before it breaches the 20 ms deadline. The panel below shows the share of the event deadline that the last batch used.

<figure>
<pre class="term">╭  ≡ ● · Handler budget ───────────────────────╮
│ Last batch ………………………………………………………………………… <span class="t-ok">8 ms</span> │
│ <span class="t-ok">█████</span><span class="t-dim">░░░░░░░░░░░░░░░░░░░░</span>   8 of 20 ms       │
╰──────────────────────────────────────────────╯</pre>
<figcaption>Illustrative, not generated. A plugin panel that shows how much of the 20 ms event deadline the last batch used. The bar turns to the warning colour above half the deadline.</figcaption>
</figure>

**Rust**

```rust
use ilium_plugin_sdk::host::clock;
use ilium_plugin_sdk::types::ui::MeterSegment;
use ilium_plugin_sdk::types::text::Tone;

const EVENT_DEADLINE_MS: u64 = 20;

impl Handler for Notes {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let started = clock::monotonic_millis();
        let submissions = self.handle_batch(batch);
        self.last_batch_ms = clock::monotonic_millis() - started;
        submissions
    }
}

fn budget_segments(used_ms: u64) -> Vec<MeterSegment> {
    let tone = if used_ms * 2 > EVENT_DEADLINE_MS { Tone::Warning } else { Tone::Success };
    vec![MeterSegment { label: "batch".into(), value: used_ms as f64, tone }]
}
// In render: tree.meter(budget_segments(self.last_batch_ms)).max(EVENT_DEADLINE_MS as f64).show_values(true)
```

**TypeScript**

```ts
import { monotonicMillis } from '@ilium/plugin-sdk/host/clock';
import { meter } from '@ilium/plugin-sdk/ui/build';

const EVENT_DEADLINE_MS = 20;
let lastBatchMs = 0;

// inside onEvents(batch)
const started = monotonicMillis();
const submissions = handleBatch(batch);
lastBatchMs = Number(monotonicMillis() - started);

// inside render
const tone = lastBatchMs * 2 > EVENT_DEADLINE_MS ? 'warning' : 'success';
const budget = meter([{ label: 'batch', value: lastBatchMs, tone }], { max: EVENT_DEADLINE_MS, showValues: true });
```

## Performance targets

The host has a fixed set of budgets and a benchmark for each. They are not rules for your plugin, but they show what the host promises: an empty host call under 1 µs, an event batch to 30 plugins under 2 ms, a cold start of 40 bundled plugins under 150 ms, and a key press to the terminal in 5 ms. The list, the method and the thresholds are in [Performance budgets and benchmarks](/pages/reference-host-budgets/). The detector budget is in the [detection loop](/pages/capability-agent-signatures/#detection-loop).

### Costs by tier and language

| Choice | Cost | What to do |
|---|---|---|
| T1 plugin | A host call is a function call inside the process: about 1 µs. | Nothing special. |
| T2 or T3 plugin | Each host call that has an effect, such as `storage.set` or a submission, is one round trip to the host: 0.2 ms median and 0.5 ms at the 99th percentile. Pure imports such as `codec` and `vector` run inside the helper with no round trip. | Batch work. At the 99th percentile (0.5 ms), 20 forwarded calls use 10 ms, half of a 20 ms batch. Read once, write once. |
| T2 or T3 start | A helper process starts at the first activation: record 20 to 80 ms. | Use lazy activation triggers; do not activate at startup without need. |
| Rust component | Instance about 4 MiB, start under 5 ms. | The default choice for scenes and detectors. |
| TypeScript component | Includes a JavaScript engine: a larger component and instance, and slower code than Rust. Budget it at several times the memory of a Rust plugin and measure your handlers. | Keep TypeScript handlers short. Put heavy loops in a job, or write them in Rust. Measure a TypeScript scene against its frame budget. |

Memory counts against `memory_mib` for the plugin and, for T2 and T3, the helper charges the sum of its hosted instances' `memory_mib` plus 32 MiB against `session_memory_mib`. See [Sandbox helper protocol](/pages/reference-host-helper/).

These are the targets for the tools in [Debugging tools](/pages/reference-debugging-tools/) and [Test harness reference](/pages/reference-test-harness/). They apply to the test and profile tooling, not to a plugin.

- **[DEV-850]** Tracing MUST add no more than 5 percent to the time of an event batch, and profiling MUST NOT change call time by more than 5 percent.
- **[DEV-851]** A native-mode harness test of one event batch SHOULD finish in under 10 ms, and a component-mode test in under 100 ms, on the build host, excluding the one-time load of the component.
- **[DEV-852]** `ilium plugin check` SHOULD finish in under 2 s for a package of 10 MiB or less, and `ilium plugin lint` in under 5 s.
- **[DEV-853]** `ilium plugin dev` SHOULD reload a plugin within 1 s after the build finishes, and the debounce of a change is 300 ms.

## Where time is charged

<figure>
<svg viewBox="0 0 400 200" role="img" aria-label="Three independent paths, each with its own deadline">
  <defs>
    <marker id="perf-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
      <path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="8" y="20" width="100" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="58" y="42" text-anchor="middle" font-size="11" fill="currentColor">Event batch</text>
  <rect x="150" y="20" width="110" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="205" y="34" text-anchor="middle" font-size="11" fill="currentColor">on-events</text>
  <text x="205" y="48" text-anchor="middle" font-size="10" fill="currentColor">20 ms deadline</text>
  <rect x="280" y="20" width="112" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="336" y="42" text-anchor="middle" font-size="11" fill="currentColor">Kernel applies</text>
  <path d="M108 38 L148 38" stroke="currentColor" fill="none" marker-end="url(#perf-arrow)"/>
  <path d="M260 38 L278 38" stroke="currentColor" fill="none" marker-end="url(#perf-arrow)"/>
  <rect x="150" y="88" width="110" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
  <text x="205" y="104" text-anchor="middle" font-size="10" fill="currentColor">views.render</text>
  <text x="205" y="118" text-anchor="middle" font-size="10" fill="currentColor">10 ms deadline</text>
  <rect x="280" y="88" width="112" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="336" y="104" text-anchor="middle" font-size="10" fill="currentColor">Host draws tree</text>
  <text x="336" y="118" text-anchor="middle" font-size="10" fill="currentColor">no plugin call</text>
  <path d="M260 108 L278 108" stroke="currentColor" fill="none" marker-end="url(#perf-arrow)"/>
  <rect x="150" y="150" width="110" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
  <text x="205" y="166" text-anchor="middle" font-size="10" fill="currentColor">job-runner.run</text>
  <text x="205" y="180" text-anchor="middle" font-size="10" fill="currentColor">2 s CPU deadline</text>
</svg>
<figcaption>The event path, the render path and the job path each have their own deadline. A slow render or job does not hold up the event batch that started it.</figcaption>
</figure>

A handler that finishes its batch within 20 ms charges almost nothing to the frame. A render does not run per frame. The host draws the tree itself, and the plugin pays only when it calls `ui.invalidate`.

## Moving slow work into a job

A handler that reads a large file with `fs.read` and decompresses it inside `on-events` breaches the 20 ms deadline. The fix has three parts:

1. The handler starts the read with the async `fs.read-all` and returns at once.
2. When the bytes arrive, a task starts a job of a declared kind with `jobs.start`.
3. The job runs `job-runner.run` on a worker, reports progress with `report-progress` and returns its output.

The job kind is declared in the manifest:

```json
{
  "capabilities": { "server": ["fs.read:project", "jobs.run"] },
  "contributes": {
    "jobs": [
      { "kind": "parse-report", "deadline_ms": 2000, "memory_mib": 64 }
    ]
  }
}
```

The WIT calls behind the SDK wrappers are exact:

```wit
read-all: func(path: fs-path) -> result<request-id, plugin-error>;
start: func(kind: string, input: list<u8>) -> result<request-id, plugin-error>;
take-bytes: func(request: request-id) -> option<list<u8>>;
take-output: func(job: request-id) -> option<list<u8>>;
```

The SDK wrappers hide the `request-id` and the `take-*` calls. In Rust, `fs::read_all(path).await` and `jobs::start(kind, input).await` return the bytes and the output. In TypeScript, `readAll` and `start` return promises of `Uint8Array`.

### Worked example: slow work in a job

The handler starts the work in a task and returns. The job runner decodes the input in 64 KiB chunks and reports progress after each chunk.

**Rust**

```rust
use ilium_plugin_sdk::host::fs;
use ilium_plugin_sdk::host::jobs;
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::task;
use ilium_plugin_sdk::types::paths::FsPath;

const PARSE_KIND: &str = "parse-report";
const CHUNK_BYTES: usize = 64 * 1024;

#[ilium_plugin(side = "server", exports = ["job-runner"])]
#[derive(Default)]
struct ReportNotes;

impl ReportNotes {
    /// Starts the work and returns at once. The task runs after the handler returns.
    fn parse_in_background(&mut self, path: FsPath) {
        task::spawn(async move {
            let bytes = match fs::read_all(path).await {
                Ok(bytes) => bytes,
                Err(error) => {
                    log::log(Level::Warn, &format!("read failed: {}", error.message));
                    return;
                }
            };
            match jobs::start(PARSE_KIND, bytes).await {
                Ok(report) => log::log(Level::Info, &format!("parsed {} bytes", report.len())),
                Err(error) => log::log(Level::Warn, &format!("parse failed: {}", error.message)),
            }
        });
    }
}

impl Handler for ReportNotes {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        // Your trigger calls parse_in_background(path), for example from a command.
        Vec::new()
    }
}

impl JobRunner for ReportNotes {
    fn run(&mut self, kind: String, input: Vec<u8>) -> Result<Vec<u8>, PluginError> {
        if kind != PARSE_KIND {
            return Err(PluginError {
                code: ErrorCode::InvalidArgument,
                message: format!("unknown job kind {kind}"),
                retry_after_ms: None,
            });
        }
        let chunk_count = input.len().div_ceil(CHUNK_BYTES).max(1);
        let mut output = Vec::with_capacity(input.len() / 4);
        for (index, chunk) in input.chunks(CHUNK_BYTES).enumerate() {
            if jobs::is_cancelled() {
                break;
            }
            output.extend_from_slice(&decompress_chunk(chunk));
            jobs::report_progress(((index + 1) * 100 / chunk_count) as f32);
        }
        Ok(output)
    }
}
```

`decompress_chunk` stands for your own decoder. A job never shares memory with the instance that started it.

**TypeScript**

The TypeScript version uses an async function for the same flow. `definePlugin` drains the microtask queue before it returns from each export call, so the function runs to its first `await` before the handler returns.

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import { readAll } from '@ilium/plugin-sdk/host/fs';
import { isCancelled, reportProgress, start } from '@ilium/plugin-sdk/host/jobs';
import { log } from '@ilium/plugin-sdk/host/log';
import { PluginError } from '@ilium/plugin-sdk';
import type { FsPath } from '@ilium/plugin-sdk/types';

const PARSE_KIND = 'parse-report';
const CHUNK_BYTES = 64 * 1024;

async function parseInBackground(path: FsPath): Promise<void> {
  try {
    const bytes = await readAll(path);
    const report = await start(PARSE_KIND, bytes);
    log('info', `parsed ${report.byteLength} bytes`);
  } catch (error) {
    const message = error instanceof PluginError ? error.message : String(error);
    log('warn', `parse failed: ${message}`);
  }
}

export const { lifecycle, handler, jobRunner } = definePlugin({
  side: 'server',
  onEvents(batch) {
    // Your trigger calls parseInBackground(path), for example from a command.
    return [];
  },
  run(kind, input) {
    if (kind !== PARSE_KIND) {
      throw new PluginError('invalid-argument', `unknown job kind ${kind}`);
    }
    const chunkCount = Math.max(1, Math.ceil(input.byteLength / CHUNK_BYTES));
    const parts: Uint8Array[] = [];
    for (let offset = 0; offset < input.byteLength; offset += CHUNK_BYTES) {
      if (isCancelled()) {
        break;
      }
      parts.push(decompressChunk(input.subarray(offset, offset + CHUNK_BYTES)));
      reportProgress(Math.floor(((offset / CHUNK_BYTES + 1) * 100) / chunkCount));
    }
    return concatenate(parts);
  },
});
```

`decompressChunk` and `concatenate` are your own helpers. The `parseInBackground` function is what your trigger calls.

The user sees a job only through what the plugin draws. A `progress-row` fed from `job-progress` events is the usual form:

<figure>
<pre class="term">╭  ≡ ● · Replay ───────────────────────────────╮
│ <span class="t-ok">▶ RUNNING</span>  37%  <span class="t-hi">██████████</span><span class="t-dim">░░░░░░░░░░░░░░░░░░</span> │
│ <span class="t-dim">session 152 of 412</span>                           │
│ <span class="t-ok">✓ DONE</span>  100%  <span class="t-ok">████████████████████████████</span>   │
╰──────────────────────────────────────────────╯</pre>
<figcaption>Illustrative, not generated. A progress row while a job runs, and the same row after the job completes.</figcaption>
</figure>

<div class="note">

A job's CPU deadline counts CPU time, not wall time. A job that waits on I/O with `jobs.wait-request` does not use its CPU budget, but its wall time counts against `job_wall_ms`. Keep waits short and report progress, so the user can see the work move.

</div>

### Job surface

| Function or field | Side | Meaning |
|---|---|---|
| `jobs.start(kind, input)` | Handler, task | Starts a declared job. Returns a `request-id`, or the output as an awaited call. |
| `jobs.cancel(job)` | Handler, task | Requests cancellation. The outcome is `cancelled`, whatever the job returns. |
| `jobs.report-progress(percent)` | Job only | Sets progress from 0 to 100. Values are clamped and NaN is ignored. |
| `jobs.is-cancelled()` | Job only | Returns true after `jobs.cancel` was requested. The job should stop early. |
| `jobs.wait-request(request, timeout-millis)` | Job only | Blocks the job until a slow call completes, for example `fs.read-all`. |
| `job-runner.run(kind, input)` | Export | Runs the job. Returns output bytes or a `plugin-error`. |
| `contributes.jobs` | Manifest | Declares each job kind, with optional `deadline_ms` and `memory_mib`. |
| `limits.job_io_mib` | Manifest | Caps the input and the output of each job. |

Rust names are the snake_case form of these, for example `jobs::report_progress`, `jobs::is_cancelled` and `jobs::cancel`. TypeScript uses the camelCase form, for example `reportProgress`, `isCancelled` and `cancel`. Inside a job, only the job-side functions and `log`, `clock`, `codec`, `fs`, `http`, `process`, `inference` and `storage` reads are available. Any other import returns `unsupported`.

## Errors and edge cases

| Code | Where it comes from | Meaning and fix |
|---|---|---|
| `timed-out` | Job CPU or wall deadline | Split the work into smaller jobs, or raise the kind's `deadline_ms` within the ceiling. |
| `quota-exceeded` | Job memory, `fs.read-all` size, job output | Read in parts with `fs.read` or lower the data per job. |
| `busy` | `jobs.start` when the queue is full, or 64 pending requests | Retry later, or wait for an existing request to complete. |
| `invalid-argument` | Unknown job kind, or input over `job_io_mib` | Check the kind against `contributes.jobs`. |
| `cancelled` | `jobs.cancel`, or a shutdown | Stop and clean up. The output is dropped. |
| `internal` | A trap inside a job instance | Read the trap text in the log. The job does not count toward the restart rule. |

Edge cases the host handles for you:

- A queued job never starts after `jobs.cancel`.
- A job's output over `job_io_mib` completes as `failed(invalid-argument)`.
- An event batch over 256 events is split by the host into batches of at most 256.
- Output-match, topic and audio-frame events that overflow their queue are dropped oldest first. The next batch starts with one `events-dropped` event that gives the count per source.

## What the user sees

A breach and each restart appear in the plugin log with the export name and the limit. After the third failure in 10 minutes, the tree footer and Settings > Plugins show notice N14, defined in [Lifecycle](/pages/lifecycle/#failure-counting-and-restart). The user turns the plugin on again there. Job progress is visible only when the job calls `report-progress`.

## Practical rules

- Handle a whole batch in one call. One call that handles up to 256 events is cheaper than one call per event, and the deadline covers the whole batch.
- Keep each storage value below 1 MiB, and each node metadata value below 64 KiB. A value over the bucket quota fails with `quota-exceeded`.
- Pass `max-bytes` or `max-entries` to every read or listing, so a single call cannot read an unbounded amount.
- Use a `dedup-key` on any send that might be retried. The host keeps the key for 10 minutes, so a retry does not repeat the effect.

### See also

- [Errors and limits](/pages/reference-errors-and-limits/)
- [Reference: errors and limits](/pages/reference-errors-and-limits/)
- [Background jobs](/pages/background-jobs/)
- [Jobs API](/pages/api-jobs/)
- [Lifecycle](/pages/lifecycle/)
- [Debugging](/pages/debugging/)
- [Rust SDK reference](/pages/rust-sdk/)
- [TypeScript SDK reference](/pages/typescript-sdk/)
- [File access](/pages/api-fs/)

Checked against the API version `ilium:plugin@0.1.0`.
