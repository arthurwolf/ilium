# Background jobs

## Background jobs

An event handler has a deadline of 20 ms by default, so it cannot parse a large file or search a whole workspace. A background job moves that work out of the handler. The plugin starts a job with a kind and an input of bytes. The host runs the plugin's `job-runner` export in a second instance of the same component, under its own CPU deadline, wall-clock deadline and memory limit. The plugin then receives progress as events and reads the output once the job completes. This page covers when to use a job, how to declare and start one, the full surface, progress, cancellation, outcomes and the limits that apply.

## You will need

- The `jobs.run` grant, which the user reviews in the install dialog (see [capability jobs.run](/pages/capability-jobs-run/)).
- A `contributes.jobs` entry for each job kind the plugin runs.
- `job-runner` in the `exports` list of the side that runs the job (see [exports](/pages/api-exports/)).
- The grants of every import the job calls, such as `fs.read` or `http`, on that same side.
- A Rust plugin with `ilium-plugin-sdk`, or a TypeScript plugin with `@ilium/plugin-sdk` (see [Rust SDK](/pages/rust-sdk/) and [TypeScript SDK](/pages/typescript-sdk/)).
- Familiarity with [requests and outcomes](/pages/requests-and-outcomes/), because every job is a request.

## When to use a job

Use a job for work that takes longer than an event deadline: parsing a saved world, preparing a large image, searching a workspace or building an index. Do not use a job for one HTTP request or a short file read. Those already return through request ids, and the host completes them without a job.

## Declaring a job kind

Each kind is listed under `contributes.jobs`. The plugin also lists `job-runner` in its `exports`.

```json
{
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0",
    "exports": ["job-runner"]
  },
  "capabilities": { "server": ["jobs.run"] },
  "contributes": {
    "jobs": [{ "kind": "parse-world", "deadline_ms": 10000, "memory_mib": 128 }]
  }
}
```

`deadline_ms` is the CPU deadline, and `memory_mib` is the memory limit. Both are optional. A kind that declares no values uses the defaults in the table below. The host clamps a value above the tier ceiling to that ceiling and logs the clamp.

| Limit | Default | Ceiling T1 | Ceiling T2 and T3 | Over the limit |
|---|---|---|---|---|
| CPU deadline (`deadline_ms`) | 2,000 ms | 60,000 ms | 10,000 ms | `failed(timed-out)` |
| Wall-clock deadline (`job_wall_ms`) | 20,000 ms | 600,000 ms | 100,000 ms | `failed(timed-out)` |
| Memory (`memory_mib`) | 64 MiB | 512 MiB | 256 MiB | `failed(quota-exceeded)` |
| Input, and output, each (`job_io_mib`) | 16 MiB | 64 MiB | 16 MiB | Input: `invalid-argument` at the call. Output: `failed(invalid-argument)` |
| Concurrent jobs | 2 | 8 | 2 | Extra jobs wait in the queue |
| Queued jobs | 16 | 64 | 16 | `busy` at the start call |
| Hold of a finished result for `take-output` | 30 s | 30 s | 30 s | `none` after the hold (ABI-012) |

The quota group is the `ilium-execution` CPU token bucket of [Resource accounting](/pages/reference-resource-accounting/): 2,000 ms of CPU time, refilled at 500 ms per second, one group for each plugin side in each process. A job start spends from it.

The CPU deadline counts CPU time only. Time spent in `wait-request` or waiting for the disk counts against the wall-clock deadline instead.

## The surface

The `jobs` import has six functions. Three of them are callable only inside the job, and the `job-runner` export is the code the host calls.

| Function | Rust | TypeScript | Callable from | Meaning |
|---|---|---|---|---|
| `start` | `jobs::start(kind, input).await` returns `Result<Vec<u8>, PluginError>`; the raw `jobs::start_request` returns `Result<RequestId, PluginError>` | `await start(kind, input)` returns `Promise<Uint8Array>`; the raw `startRequest` returns `bigint` | Plugin | Starts a job of a declared kind. The awaited form resolves with the output |
| `cancel` | `jobs::cancel(job)` | `cancel(job)` | Plugin | Asks the job to stop |
| `take-output` | `jobs::take_output(job)` returns `Option<Vec<u8>>` | `takeOutput(job)` returns `Uint8Array \| undefined` | Plugin | Returns the output once |
| `report-progress` | `jobs::report_progress(percent)` takes `f32` | `reportProgress(percent)` | Inside `run` | Reports 0 to 100. NaN is ignored, other values are clamped |
| `is-cancelled` | `jobs::is_cancelled()` returns `bool` | `isCancelled()` | Inside `run` | True after a cancel request |
| `wait-request` | `jobs::wait_request(request, timeout_millis)` | `waitRequest(request, timeoutMillis)` | Inside `run` | Blocks until a slow call completes, so the job can read a file and then take the result |
| `run` (export) | `JobRunner::run(&mut self, kind: String, input: Vec<u8>)` | `run(kind, input)` member of `definePlugin` | Host calls it | Does the work. Returns the output bytes or a `PluginError` |

The SDKs also provide JSON helpers. `jobs::start_json(kind, &value)` in Rust and `startJson(kind, value)` in TypeScript encode a value as JSON bytes. The job decodes the bytes itself, so the host never looks inside an input or an output.

The job's events are `job-progress` and `request-completed`. Both are described under progress and cancellation below.

## How a job runs

<figure>
<svg viewBox="0 0 400 150" role="img" aria-label="A handler starts a job that runs in a second instance and reports back">
  <title>Job flow between the handler and the job instance</title>
  <defs>
    <marker id="job-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
      <path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="10" y="50" width="110" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="65" y="68" font-size="11" text-anchor="middle" fill="currentColor">Plugin handler</text>
  <text x="65" y="84" font-size="10" text-anchor="middle" fill="currentColor">server instance</text>
  <rect x="270" y="50" width="120" height="44" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="330" y="68" font-size="11" text-anchor="middle" fill="currentColor">Job instance</text>
  <text x="330" y="84" font-size="10" text-anchor="middle" fill="currentColor">run(kind, input)</text>
  <line x1="120" y1="72" x2="268" y2="72" stroke="currentColor" marker-end="url(#job-arrow)"/>
  <text x="195" y="64" font-size="10" text-anchor="middle" fill="currentColor">jobs.start</text>
  <line x1="268" y1="104" x2="120" y2="104" stroke="currentColor" marker-end="url(#job-arrow)"/>
  <text x="195" y="122" font-size="10" text-anchor="middle" fill="currentColor">job-progress</text>
  <text x="195" y="138" font-size="10" text-anchor="middle" fill="currentColor">request-completed</text>
</svg>
<figcaption>The handler starts the job and receives progress and completion as events. It never runs the job code itself.</figcaption>
</figure>

A job instance has three rules that differ from the plugin's own instance:

- It is never activated. `activate` does not run, and the job has no event handler.
- It has the grants of its side. It may call `log`, `clock`, `codec`, `fs`, `http`, `process`, `inference`, `storage.get`, `storage.keys`, `report-progress`, `is-cancelled` and `wait-request`. Every other import, including `jobs.start`, returns `unsupported`. Imports without a result do nothing.
- It never shares memory with the instance that started it. The host keeps a pool of job instances per plugin, at most `concurrent_jobs`, and reuses them between jobs of that plugin.

The host runs jobs on the session job worker bank. The bank has `min(4, cores / 2)` threads, and at least one. Jobs beyond the concurrency limit wait in the queue.

## Example: a job

**Rust**

This server plugin starts an import job when a command runs. The job parses the bytes in chunks and reports progress after each chunk. The `run` export and the handler share one `#[ilium_plugin]` struct.

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::{jobs, log::{self, Level}};

const PARSE_KIND: &str = "parse-world";

#[ilium_plugin(side = "server", exports = ["job-runner"])]
#[derive(Default)]
struct WorldImporter;

impl Handler for WorldImporter {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::CommandInvoked(command) = event else { continue };
            if command.command != "com.example.worlds/import" { continue; }
            // Replace with the bytes of the saved world file, read with fs::read_all first.
            let world_bytes: Vec<u8> = Vec::new();
            task::spawn(async move {
                match jobs::start(PARSE_KIND.into(), world_bytes).await {
                    Ok(output) => {
                        log::log(Level::Info, &format!("imported {} bytes", output.len()))
                    }
                    Err(error) => {
                        log::log(Level::Error, &format!("import failed: {}", error.message))
                    }
                }
            });
        }
        Vec::new()
    }
}

impl JobRunner for WorldImporter {
    fn run(&mut self, kind: String, input: Vec<u8>) -> Result<Vec<u8>, PluginError> {
        if kind != PARSE_KIND {
            return Err(PluginError {
                code: ErrorCode::InvalidArgument,
                message: format!("unknown job kind {kind}"),
                retry_after_ms: None,
            });
        }
        let chunk_size = input.len().div_ceil(100).max(1);
        let chunks: Vec<&[u8]> = input.chunks(chunk_size).collect();
        let mut output = Vec::new();
        for (index, chunk) in chunks.iter().enumerate() {
            if jobs::is_cancelled() {
                return Err(PluginError {
                    code: ErrorCode::Cancelled,
                    message: "stopped".into(),
                    retry_after_ms: None,
                });
            }
            output.extend(parse_chunk(chunk));
            jobs::report_progress((index + 1) as f32 * 100.0 / chunks.len() as f32);
        }
        Ok(output)
    }
}

// The parser for one chunk of the world file. This example copies the bytes.
fn parse_chunk(chunk: &[u8]) -> Vec<u8> {
    chunk.to_vec()
}
```

The manifest for this example lists `"exports": ["job-runner"]` and `"jobs.run"` as shown in the declaration section.

**TypeScript**

This server plugin keeps the request id of its running job, so a second command can cancel it. It reads the output with `takeOutput` when the completion event arrives. The raw `startRequest` form is used because the completion is handled in `onEvents`, not in an awaited call.

```ts
import { definePlugin, PluginError } from '@ilium/plugin-sdk';
import type { Event, Submission } from '@ilium/plugin-sdk/types';
import {
  cancel, isCancelled, reportProgress, startRequest, takeOutput,
} from '@ilium/plugin-sdk/host/jobs';
import { log } from '@ilium/plugin-sdk/host/log';

const PARSE_KIND = 'parse-world';
let activeJob: bigint | undefined;

// Replace with the real parser for one part of the world file. This example copies the bytes.
function parseChunk(chunk: Uint8Array): Uint8Array {
  return chunk.slice();
}

function concatenate(parts: Uint8Array[]): Uint8Array {
  const output = new Uint8Array(parts.reduce((sum, part) => sum + part.length, 0));
  let offset = 0;
  for (const part of parts) {
    output.set(part, offset);
    offset += part.length;
  }
  return output;
}

export const { lifecycle, handler, jobRunner } = definePlugin({
  side: 'server',
  activate() {},
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'command-invoked' && event.val.command === 'com.example.worlds/import') {
        // Replace the empty input with the bytes of the saved world file.
        activeJob = startRequest(PARSE_KIND, new Uint8Array(0));
      }
      if (event.tag === 'command-invoked' && event.val.command === 'com.example.worlds/cancel') {
        if (activeJob !== undefined) cancel(activeJob);
      }
      if (event.tag === 'request-completed' && event.val.request.tag === 'call'
        && event.val.request.val === activeJob) {
        if (event.val.outcome.tag === 'ok' && activeJob !== undefined) {
          log('info', `parsed ${takeOutput(activeJob)?.length ?? 0} bytes`);
        } else {
          log('info', `import ended with ${event.val.outcome.tag}`);
        }
        activeJob = undefined;
      }
    }
    return [];
  },
  run(kind: string, input: Uint8Array): Uint8Array {
    if (kind !== PARSE_KIND) {
      throw new PluginError('invalid-argument', `unknown job kind ${kind}`);
    }
    const chunkSize = Math.max(1, Math.ceil(input.length / 100));
    const chunkCount = Math.ceil(input.length / chunkSize);
    const parts: Uint8Array[] = [];
    for (let index = 0; index < chunkCount; index += 1) {
      if (isCancelled()) {
        throw new PluginError('cancelled', 'job stopped');
      }
      const start = index * chunkSize;
      parts.push(parseChunk(input.subarray(start, start + chunkSize)));
      reportProgress(((index + 1) * 100) / chunkCount);
    }
    return concatenate(parts);
  },
});
```

Both examples follow the same job loop: check for cancellation before each chunk, do the work, then report the percentage. A loop that skips the cancellation check keeps running after the user has cancelled, until the deadline stops it.

## Other long-running work

A job is for CPU work in the plugin's own sandbox. Three further surfaces cover other kinds of long work.

| Work | Use | Grant | Limit |
|---|---|---|---|
| A program that keeps running and prints output | `process.spawn` with a `child` handle | `process.long-lived` and `process.run:<program>`, high risk | `children`, 2 by default; 16 per session (`session_processes`), shared by `process.run` (`busy`) and `process.spawn` (`quota-exceeded`) |
| A response that arrives in pieces (SSE, NDJSON) | `http.stream` | `http:<host-pattern>` | `http_streams`, 4 by default, 64 session wide |
| Work at a later time that must survive a restart | `timers.at(epoch, name)` durable timer | None | `durable_timers`, 64 by default |

Spawned output arrives as `child-output` events, whole lines when `line-mode` is set, and the end as `child-exited`. Dropping the handle, or stopping the plugin, ends the child with SIGTERM and then SIGKILL after 2 s. A durable timer fires as `durable-timer-fired` with `late-by-millis`. If Ilium was not running at the due time, it fires once after the next activation. The same name replaces an earlier timer.

**Rust**

```rust
use ilium_plugin_sdk::host::{process, timers};
use ilium_plugin_sdk::prelude::*;

// Grants: process.long-lived and process.run:ffmpeg on the server side.
pub fn start_encode(spec: ProcessSpec) -> Result<Child, PluginError> {
    process::spawn(SpawnSpec { process: spec, stdin_open: false, line_mode: true })
}

// Run the nightly cleanup at the given time, even if Ilium restarts before then.
pub fn schedule_cleanup(due_epoch_millis: u64) -> Result<TimerId, PluginError> {
    timers::at(due_epoch_millis, "nightly-cleanup")
}
```

**TypeScript**

```ts
import { spawn } from '@ilium/plugin-sdk/host/process';
import { at } from '@ilium/plugin-sdk/host/timers';

// Grants: process.long-lived and process.run:ffmpeg on the server side.
export function startEncode(spec: ProcessSpec) {
  return spawn({ process: spec, stdinOpen: false, lineMode: true });
}

// Run the nightly cleanup at the given time, even if Ilium restarts before then.
export function scheduleCleanup(dueEpochMillis: bigint) {
  return at(dueEpochMillis, 'nightly-cleanup');
}
```

<figure>
<pre class="term">╭  ≡ ● · Encode ───────────────────────────────────────────╮
│ ffmpeg ………………………………………………………………………………………………… <span class="t-ok">▶ RUNNING</span>   │
│ <span class="t-dim">frame= 1204 fps=48 q=28.0 size= 5120kB</span>                   │
│ <span class="t-dim">child-exited status 0</span>  <span class="t-ok">✓ DONE</span>                            │
╰──────────────────────────────────────────────────────────╯</pre>
<figcaption>Illustrative, not generated. A spawned child process shown as a leader row, with its last output line and its exit.</figcaption>
</figure>

## Progress

While a job runs, the plugin receives `job-progress` events. In Rust the event is `Event::JobProgress(JobProgressEvent { request_id, percent })`. In TypeScript it is `{ tag: 'job-progress', val: { requestId, percent } }`. Progress events are coalesced by job: when several arrive before the handler runs, only the newest percentage remains, at the position of its newest occurrence. A fast job therefore does not flood the event queue.

The host shows no job progress by itself. The plugin draws it. A panel uses a `progress-row` (state, percentage and bar in one row), a `gauge` or a `line-gauge` widget (see [data display widgets](/pages/widgets-data-display/)), and the plugin updates the tree with `ui.invalidate` when the percentage changes. Keep updates under the 10 per second widget rate.

<figure>
<pre class="term">╭  ≡ ● · Import: Survival world 2026-10-02 ────────────────╮
│ <span class="t-ok">▶ RUNNING</span>  42%  <span class="t-hi">█████████████</span><span class="t-dim">░░░░░░░░░░░░░░░░░</span>           │
│ <span class="t-dim">Parsing chunk 42 of 100 in the world file.</span>               │
│ <span class="t-hi">⠋</span> Working   <span class="t-dim">Stop at any time.</span>                            │
╰──────────────────────────────────────────────────────────╯</pre>
<figcaption>Illustrative, not generated. A progress row and a spinner while a world file is imported in the background.</figcaption>
</figure>

## Cancellation

Cancellation has two sides. The plugin calls `cancel` with the request id, and the job sees the request through `is-cancelled`. The job can stop at its next check, but the outcome does not depend on what the job returns:

- Once `cancel` is requested, the outcome is `cancelled` even if `run` returns output. The output is dropped, so `take-output` returns `none`.
- A queued job that is cancelled before it starts never runs.
- Cancelling a job that has already completed does nothing.

The plugin stops, is disabled or is uninstalled: its pending jobs complete as `cancelled`. Losing the `jobs.run` grant is different. The authorization epoch changes, and the job completes as `failed(revoked)`. A job that ends this way never counts toward the restart rule.

<figure>
<pre class="term">╭  ≡ ● · Import: Survival world 2026-10-02 ────────────────╮
│ <span class="t-warn">! cancelled</span>  58%  <span class="t-warn">███████████████████</span><span class="t-dim">░░░░░░░░░░░</span>         │
│ <span class="t-dim">Stopped at chunk 58 of 100. Nothing imported.</span>            │
╰──────────────────────────────────────────────────────────╯</pre>
<figcaption>Illustrative, not generated. The same import panel after the user cancels the job at 58 percent.</figcaption>
</figure>

## Outcomes and errors

A job completes once, and a `request-completed` event carries the outcome. The outcome is `ok`, `cancelled` or `failed(code)`. The job takes its output with `take-output`. The SDK wrapper in `jobs::start` returns the output directly, or the error as `Err`.

| Situation | Outcome or error |
|---|---|
| Job returns output within limits | `ok`; take the output |
| CPU or wall-clock deadline passed | `failed(timed-out)` |
| Memory limit passed | `failed(quota-exceeded)` |
| Output over `job_io_mib` | `failed(invalid-argument)` |
| `run` returns a `plugin-error` | `failed`, carrying that error's code and message |
| Trap in the job instance | `failed(internal)`, with the trap text |
| Cancel requested, or the plugin stops | `cancelled` |
| Grant revoked during the job | `failed(revoked)` |
| Kind not declared, or input over the job input limit | `invalid-argument` at the start call |
| Job queue full | `busy` at the start call |
| CPU budget of the quota group spent | `quota-exceeded` at the start call, with `retry-after-ms` |
| `wait-request` outside `run` | `unsupported` |
| `take-output` after the 30 s hold, a second take, or an unknown id | `none` |

Every failed job is reported to the plugin and never counts toward the restart rule. A trap in the plugin's own instance, by contrast, counts. See [errors and limits](/pages/reference-errors-and-limits/) for the restart and disable rule.

<div class="warn">
<p>A job cannot start another job. <code>jobs.start</code> returns <code>unsupported</code> inside <code>run</code>. Start the second job from the plugin's own instance, after the first one completes.</p>
</div>

## What the user sees

A job itself is invisible. The user sees what the plugin draws: a progress gauge, a status line or a notification when the job completes. A cancelled job shows the state the plugin sets, because the plugin receives the `cancelled` outcome. A failed job appears only if the plugin reports it, so a plugin that wants the user to see a failure must draw or notify it.

## See also

- [Requests and outcomes](/pages/requests-and-outcomes/)
- [API: jobs](/pages/api-jobs/)
- [Capability: jobs.run](/pages/capability-jobs-run/)
- [Errors and limits](/pages/reference-errors-and-limits/)
- [Services](/pages/services/)
- [Lifecycle](/pages/lifecycle/)

API version checked: `ilium:plugin@0.1.0`.
