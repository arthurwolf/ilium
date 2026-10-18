---
layout: page
title: "Capability: jobs.run"
permalink: /pages/capability-jobs-run/
---

## jobs.run

The `jobs.run` grant lets a plugin start background jobs. A job runs the plugin's `job-runner.run` export on a separate worker instance, with its own CPU deadline, wall-clock deadline and memory limit. The job never shares memory with the code that started it.

Use a job for work that is too long for an event handler, such as scanning a large folder, parsing a long transcript or building an export. Work that finishes in a few milliseconds needs no job. The starting side uses three calls (`start`, `cancel` and `take-output`), and the job side uses three more (`report-progress`, `is-cancelled` and `wait-request`). This page covers the grant, the limits, the failure rules and the review dialog. The full function reference is on the [jobs](/pages/api-jobs/) page.

### What you need

- The `jobs.run` grant. It takes no argument.
- A `job-runner` export. Add `"job-runner"` to the manifest `exports` list of the side that runs the job, and implement it: the `JobRunner` trait in Rust, or the `run` member of `definePlugin` in TypeScript. The `with-jobs` world fragment adds the import and export (see [exports](/pages/api-exports/) for the world fragments). See [exports](/pages/api-exports/).
- One `contributes.jobs` entry for every job kind the plugin starts.
- A side that supports jobs: `server`, `client` or `scene`. A scene declares the grant in `capabilities.scene`.

Manifest for the worked examples below:

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.logs",
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0",
    "exports": ["job-runner"]
  },
  "capabilities": { "server": ["jobs.run", "fs.read:project", "agent.read"] },
  "contributes": {
    "jobs": [ { "kind": "count-errors", "deadline_ms": 5000, "memory_mib": 64 } ]
  }
}
```

### Grant syntax

| Grant | Valid | Reason |
|---|---|---|
| `jobs.run` | yes | the only form |
| `jobs.run:any` | no | the grant takes no argument (`manifest.grant-argument`) |
| `jobs` | no | the grant name is incomplete (`manifest.grant-unknown`) |

### Risk and install

- The grant has the `normal` risk level. A T1 bundled plugin receives it at activation, up to the manifest ceiling in the table below.
- A T2 or T3 package lists the grant in the review dialog, checked by default. The user can uncheck it. A declined grant is shown dimmed, and the plugin's `granted` list omits it.
- The host refuses a package whose component exports differ from its manifest `exports` list, with `manifest.exports-mismatch`. A `contributes.jobs` entry without a `job-runner` export fails for that reason.

<figure>

<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.jobs 0.1.0 by Example Ltd                                      │
│                                                                            │
│ <span class="t-hi">Server side</span>                                                                │
│ [x] Runs background work <span class="t-dim">…………………………………………………………………………………………………………</span> jobs.run │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>

<figcaption>The review dialog for a package that requests jobs.run, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar. Checked rows are granted; a high-risk row starts unchecked.</figcaption>
</figure>

### Surface

| Name | Rust | TypeScript | Runs in | Meaning |
|---|---|---|---|---|
| `start` | `jobs::start(kind, &input).await` returns `Result<Vec<u8>, PluginError>`; `jobs::start_request` returns the request id | `await start(kind, input)` returns `Promise<Uint8Array>`; `startRequest` returns the request id | server, client, scene | Starts a declared kind. The async form resolves to the output bytes. |
| `start_json` | `jobs::start_json(kind, &value)` | `startJson(kind, value)` | starting side | Encodes the value as JSON bytes. The job decodes it. |
| `cancel` | `jobs::cancel(job)` | `cancel(job)` | starting side | Sets the cancel flag and returns at once. |
| `take-output` | `jobs::take_output(job)` returns `Option<Vec<u8>>` | `takeOutput(job)` returns `Uint8Array \| undefined` | starting side | Returns the output once, after an `ok` completion. |
| `report-progress` | `jobs::report_progress(percent)`, `f32` | `reportProgress(percent)` | job only | Reports a value from 0 to 100. |
| `is-cancelled` | `jobs::is_cancelled()` returns `bool` | `isCancelled()` | job only | Returns true after a cancel. |
| `wait-request` | `jobs::wait_request(request, timeout_millis)` | `waitRequest(request, timeoutMillis)` | job only | Blocks until a slow call completes, such as `fs.read-all`. |
| `run` | `JobRunner::run(&mut self, kind: String, input: Vec<u8>) -> Result<Vec<u8>, PluginError>` | `run(kind, input)` member of `definePlugin` | job instance | Does the work and returns the output bytes, or fails with a `PluginError`. |

Two events report the job to the starting side. `job-progress` carries the `request-id` and the `percent`; only the newest value per job reaches the plugin in one batch. `request-completed` carries the outcome: `ok`, `cancelled` or `failed(code)`.

A job instance has no `activate` and no event handler. It may call `log`, `clock`, `codec`, `fs`, `http`, `process`, `inference`, `storage.get` and `storage.keys`, with the grants of its side. Every other import returns `unsupported`.

### Limits

Each limit has a default, a ceiling for the plugin's tier, and an effect when it is reached. The manifest may ask for more than the default, up to the ceiling. A request above the ceiling is clamped and logged, never refused. The user can change the host-side values under `[plugins.limits]` in `config.toml`.

| Limit | Default | T1 ceiling | T2 and T3 ceiling | Over the limit |
|---|---|---|---|---|
| CPU deadline (`deadline_ms`) | 2 s | 60 s | 10 s | `failed(timed-out)` |
| Wall-clock deadline | 20 s | 600 s | 100 s | `failed(timed-out)` |
| Memory (`memory_mib`) | 64 MiB | 512 MiB | 256 MiB | `failed(quota-exceeded)` |
| Input, and output, each | 16 MiB | 64 MiB | 16 MiB | input: `invalid-argument` at the call; output: `failed(invalid-argument)` |
| Concurrent jobs per plugin | 2 | 8 | 2 | extra jobs wait in the queue |
| Queued jobs | 16 | 64 | 16 | `busy` |
| Pending requests per plugin | 64 | 256 | 64 | `busy` |

The per-kind values in `contributes.jobs` override the CPU and memory defaults for that kind, within the same ceilings. Time spent in `wait-request` counts against the wall-clock deadline only. All plugins in one server or client share one pool of job worker instances per server or client. Its size is fixed by the host, is not a setting and cannot be changed by a plugin; it is `min(4, cores / 2)` with a minimum of 1. Jobs queue when those workers are busy.

### How a job runs

<figure>
<svg viewBox="0 0 320 300" role="img" aria-label="Job flow from start call to worker run, progress events and take-output">
  <defs>
    <marker id="jr-arrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
      <path d="M0,0 L8,4 L0,8 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="10" y="8" width="300" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="160" y="32" text-anchor="middle" font-size="12" fill="currentColor">Plugin calls start(kind, input)</text>
  <line x1="160" y1="48" x2="160" y2="70" stroke="currentColor" marker-end="url(#jr-arrow)"/>
  <rect x="10" y="70" width="300" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="160" y="94" text-anchor="middle" font-size="12" fill="currentColor">job-runner.run on a worker, with a deadline</text>
  <line x1="160" y1="110" x2="160" y2="132" stroke="currentColor" marker-end="url(#jr-arrow)"/>
  <rect x="10" y="132" width="300" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="160" y="156" text-anchor="middle" font-size="12" fill="currentColor">job-progress events, coalesced by job</text>
  <line x1="160" y1="172" x2="160" y2="194" stroke="currentColor" marker-end="url(#jr-arrow)"/>
  <rect x="10" y="194" width="300" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="160" y="218" text-anchor="middle" font-size="12" fill="currentColor">request-completed; take-output reads bytes</text>
  <text x="160" y="270" text-anchor="middle" font-size="11" fill="currentColor">A cancel call sets is-cancelled for the job.</text>
</svg>
<figcaption>The job runs apart from the event handler, and its result comes back as a completed request.</figcaption>
</figure>

The starting side gets a request id at once. A job waits in the queue while the concurrency limit is full. Later jobs of the same plugin reuse the job instance. A finished result stays available to `take-output` for 30 s or until it is taken. A second take, or a take after the 30 s, returns `none`, which the async wrapper reports as `not-found`.

### Edge cases and errors

| Situation | Result |
|---|---|
| Grant declined, or revoked while the plugin runs | `start` returns `not-permitted`, and `run` is never called. Pending jobs that need the grant complete as `failed(revoked)`. |
| Kind not listed in `contributes.jobs` | `start` returns `invalid-argument`. |
| Input over the input limit | `start` returns `invalid-argument` at once. Nothing is queued. |
| Job queue full | `start` returns `busy`. Jobs within the queue limit wait without an error. |
| Output over the output limit | The job completes as `failed(invalid-argument)`. |
| CPU or wall-clock deadline passed | `failed(timed-out)`. |
| Memory limit reached | `failed(quota-exceeded)`. |
| Trap inside `run` | `failed(internal)`, with the trap text. The failure does not count toward the plugin restart rule. |
| `run` returns a `plugin-error` | The job fails with that error code and message. |
| `cancel` called | The outcome is `cancelled`, whatever the job returns, and its output is dropped. A queued job never starts. |
| `cancel` on a finished job | No effect. |
| `start` called from inside a job | `unsupported`. |
| `wait-request` called outside `run` | `unsupported`. |
| Plugin disabled while jobs run | Pending jobs are cancelled. |

`report-progress` clamps a value below 0 to 0 and a value above 100 to 100. A NaN value is ignored.

### Example in Rust

The plugin counts error lines in a log file each time an agent goes idle. The handler reads the file, starts the job and logs the result. The job counts the lines and reports progress.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::{fs, jobs, log::{self, Level}};
use ilium_plugin_sdk::task;
use ilium_plugin_sdk::types::events::AgentActivityEvent;
use ilium_plugin_sdk::types::tree::AgentActivity;

const COUNT_KIND: &str = "count-errors";

#[ilium_plugin(side = "server", exports = ["job-runner"])]
#[derive(Default)]
struct ErrorCounter;

impl Lifecycle for ErrorCounter {}

impl Handler for ErrorCounter {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::AgentActivityChanged(AgentActivityEvent { current, .. }) = event
            else { continue; };
            if current == AgentActivity::Idle {
                task::spawn(count_errors_after_idle());
            }
        }
        Vec::new()
    }
}

async fn count_errors_after_idle() {
    let path = FsPath { scope: "project".into(), path: "logs/session.log".into() };
    let transcript = match fs::read_all(path).await {
        Ok(bytes) => bytes,
        Err(error) => {
            log::log(Level::Warn, &format!("read failed: {}", error.message));
            return;
        }
    };
    match jobs::start(COUNT_KIND, &transcript).await {
        Ok(output) => {
            let errors = String::from_utf8_lossy(&output);
            log::log(Level::Info, &format!("errors: {errors}"));
        }
        Err(error) => log::log(Level::Warn, &format!("count failed: {}", error.message)),
    }
}

impl JobRunner for ErrorCounter {
    // Only kinds declared in contributes.jobs reach this function.
    fn run(&mut self, _kind: String, input: Vec<u8>) -> Result<Vec<u8>, PluginError> {
        let text = String::from_utf8_lossy(&input);
        let lines: Vec<&str> = text.lines().collect();
        let mut count = 0u32;
        for (index, line) in lines.iter().enumerate() {
            if jobs::is_cancelled() {
                break; // the outcome is already cancelled, so the output is dropped
            }
            if line.contains("error") {
                count += 1;
            }
            if index % 1000 == 0 {
                jobs::report_progress(100.0 * index as f32 / lines.len() as f32);
            }
        }
        jobs::report_progress(100.0);
        Ok(count.to_string().into_bytes())
    }
}
```

### Example in TypeScript

The same plugin in TypeScript. It uses the same manifest, with the TypeScript build as the `server` entry.

**TypeScript**

```ts
import { definePlugin, type Event, type PluginError, type Submission } from '@ilium/plugin-sdk';
import { readAll } from '@ilium/plugin-sdk/host/fs';
import { log } from '@ilium/plugin-sdk/host/log';
import { start, isCancelled, reportProgress } from '@ilium/plugin-sdk/host/jobs';

const COUNT_KIND = 'count-errors';

async function countErrorsAfterIdle(): Promise<void> {
  try {
    const transcript = await readAll({ scope: 'project', path: 'logs/session.log' });
    const output = await start(COUNT_KIND, transcript);
    log('info', `errors: ${new TextDecoder().decode(output)}`);
  } catch (error) {
    log('warn', `count failed: ${(error as PluginError).message}`);
  }
}

export const { lifecycle, handler, jobRunner } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'agent-activity-changed' && event.val.current === 'idle') {
        void countErrorsAfterIdle();
      }
    }
    return [];
  },
  run(_kind: string, input: Uint8Array): Uint8Array {
    const lines = new TextDecoder().decode(input).split('\n');
    let count = 0;
    for (let index = 0; index < lines.length; index++) {
      if (isCancelled()) break;
      if (lines[index].includes('error')) count++;
      if (index % 1000 === 0) reportProgress((100 * index) / lines.length);
    }
    reportProgress(100);
    return new TextEncoder().encode(String(count));
  },
});
```

### What the user sees

The host draws no progress bar or status for a job. The plugin draws progress with its own widgets. A failed job reaches the user only when the plugin reports it, for example through a log line, a notice or a status row. The plugin log of the example above looks like this:

<figure>

<pre class="term"><span class="t-prompt">$</span> ilium plugin logs com.example.logs
<span class="t-dim">2026-10-11 09:14:02</span> <span class="t-ok">info</span>  errors: 37
<span class="t-dim">2026-10-11 09:14:09</span> <span class="t-warn">warn</span>  count failed: job deadline passed
</pre>

<figcaption>Illustrative, not generated. The plugin log after two idle transitions.</figcaption>
</figure>

### See also

- [jobs](/pages/api-jobs/) for the full function reference and the cancellation details.
- [background jobs](/pages/background-jobs/) for when to use a job and how it fits the plugin's visible behaviour.
- [exports](/pages/api-exports/) for the `job-runner` export and the other optional exports.
- [fs.read](/pages/capability-fs-read/) for the file reads that most jobs perform.
- [process.run](/pages/capability-process-run/) for external programs that a job starts.

Checked against `ilium:plugin@0.1.0` (API draft of 2026-10-11).
