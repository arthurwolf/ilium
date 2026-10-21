---
layout: page
title: jobs
permalink: /pages/api-jobs/
---

## jobs

A job is work that runs in the background, away from the plugin's event handler. The plugin starts a job with a kind and an input of bytes. The host runs the plugin's `job-runner` export in a second instance of the same component, in its own memory, under a CPU deadline, a wall-clock deadline and a memory limit. The plugin then reads the output, watches the progress, or cancels the job.

Use a job for work that takes longer than the event deadline of 20 ms: parsing a large file, searching a workspace, calling a model and waiting for its answer, or building an export. Short work that finishes in a few milliseconds needs no job.

**You will need:** the `jobs.run` grant, one `contributes.jobs` entry for each job kind, and `job-runner` in the `exports` list of the side that runs the job (see [exports](/pages/api-exports/)). The `job-runner` export exists in the server world, the client world and the animation world, so a job can belong to any side. Any import the job calls, such as `fs.read` or `inference`, needs its grant on that same side. The grant page is [Capability: jobs.run](/pages/capability-jobs-run/).

### Interface

```wit
// jobs (S C A, grant jobs.run)
start: func(kind: string, input: list<u8>) -> result<request-id, plugin-error>;
cancel: func(job: request-id);
take-output: func(job: request-id) -> option<list<u8>>;
// callable only inside job-runner.run:
report-progress: func(percent: f32);
is-cancelled: func() -> bool;
wait-request: func(request: request-id, timeout-millis: u32) -> result<_, plugin-error>;

// job-runner (export of S C A, fragment with-jobs)
run: func(kind: string, input: list<u8>) -> result<list<u8>, plugin-error>;
```

The SDK imports are `ilium_plugin_sdk::host::jobs` in Rust and `@ilium/plugin-sdk/host/jobs` in TypeScript (see [Rust SDK](/pages/rust-sdk/) and [TypeScript SDK](/pages/typescript-sdk/)).

The SDK names of the calls on this page, including the `_request` / `Request` starting forms and the `_json` / `Json` names, follow [Wrapper names in the SDKs](/pages/reference-wit/#wrapper-names-in-the-sdks).

### Surface

| Function | Rust | TypeScript | Grant | Call | Meaning |
|---|---|---|---|---|---|
| `start` | `jobs::start(kind, input).await` returns `Result<Vec<u8>, PluginError>`. The raw form `jobs::start_request` returns `Result<RequestId, PluginError>`. | `await start(kind, input)` returns `Promise<Uint8Array>`. The raw form `startRequest` returns `bigint`. | `jobs.run` | async | Starts a job of a declared kind. The awaited form resolves with the output. |
| `cancel` | `jobs::cancel(job)` | `cancel(job)` | `jobs.run` | sync | Asks the job to stop. See cancellation below. |
| `take-output` | `jobs::take_output(job)` returns `Option<Vec<u8>>` | `takeOutput(job)` returns `Uint8Array \| undefined` | `jobs.run` | sync | Returns the output bytes once. |
| `report-progress` | `jobs::report_progress(percent)` takes `f32` | `reportProgress(percent)` | inside `run` only | sync | Reports a percentage from 0 to 100. |
| `is-cancelled` | `jobs::is_cancelled()` returns `bool` | `isCancelled()` | inside `run` only | sync | Returns true after a cancel. |
| `wait-request` | `jobs::wait_request(request, timeout_millis)` returns `Result<(), PluginError>` | `waitRequest(request, timeoutMillis)` | inside `run` only | blocks | Waits until a slow call completes, so the job can start a call and then take its result. |

The `job-runner` export has one function, `run(kind, input)`. In Rust it is `JobRunner::run(&mut self, kind: String, input: Vec<u8>) -> Result<Vec<u8>, PluginError>`. In TypeScript it is the `run` member of `definePlugin`. It returns a `Uint8Array` or throws a `PluginError`.

The JSON helpers `jobs::start_json(kind, &value)` in Rust and `startJson(kind, value)` in TypeScript encode a value as JSON bytes. The job decodes the bytes itself.

### What a job instance can call

A job instance is not activated. Its `activate` never runs and it has no event handler. It may call `log`, `clock`, `codec`, `fs`, `http`, `process`, `inference`, `storage.get`, `storage.keys`, `report-progress`, `is-cancelled` and `wait-request`, with the grants of its side. Every other import, including `jobs.start`, returns `unsupported`. Imports without a result do nothing.

The host reuses job instances between jobs. The live instances of one plugin side never exceed `concurrent_jobs` ([HST-018]); the worker threads that run them are one shared bank per server or client ([HST-182]). A job never shares memory with the instance that started it, so the only data that cross are the input bytes, the output bytes and the progress value.

### Manifest and limits

Declare each kind in `contributes.jobs`. `deadline_ms` is the CPU deadline and `memory_mib` the memory limit. Both are optional.

```json
{
  "contributes": {
    "jobs": [{ "kind": "export-pdf", "deadline_ms": 60000, "memory_mib": 256 }]
  }
}
```

A value above the tier ceiling is clamped and logged. The CPU deadline counts CPU time only, so time spent in `wait-request` counts only against the wall-clock limit.

| Limit | Default | Ceiling T1 | Ceiling T2 and T3 | Over the limit |
|---|---|---|---|---|
| CPU deadline (`deadline_ms`) | 2,000 ms | 60,000 ms | 10,000 ms | `failed(timed-out)` |
| Wall-clock deadline | 20,000 ms | 600,000 ms | 100,000 ms | `failed(timed-out)` |
| Memory (`memory_mib`) | 64 MiB | 512 MiB | 256 MiB | `failed(quota-exceeded)` |
| Input, and output, each | 16 MiB | 64 MiB | 16 MiB | Input: `invalid-argument` at the call. Output: `failed(invalid-argument)`. |
| Concurrent jobs | 2 | 8 | 2 | Extra jobs wait in the queue. |
| Queued jobs | 16 | 64 | 16 | `busy` |
| Finished result held | 30 s | 30 s | 30 s | `take-output` returns `none` |

### Job lifecycle

<figure>
<svg viewBox="0 0 400 190" role="img" aria-label="Job lifecycle from start to a final outcome">
<title>Job lifecycle</title>
<defs>
<marker id="job-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
<path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
</marker>
</defs>
<rect x="10" y="20" width="100" height="36" rx="6" fill="none" stroke="currentColor"/>
<text x="60" y="42" font-size="12" text-anchor="middle" fill="currentColor">Started</text>
<line x1="110" y1="38" x2="146" y2="38" stroke="currentColor" marker-end="url(#job-arrow)"/>
<rect x="150" y="20" width="110" height="36" rx="6" fill="none" stroke="currentColor"/>
<text x="205" y="42" font-size="12" text-anchor="middle" fill="currentColor">Running</text>
<text x="266" y="44" font-size="10" fill="currentColor">progress events</text>
<line x1="180" y1="56" x2="70" y2="100" stroke="currentColor" marker-end="url(#job-arrow)"/>
<line x1="205" y1="56" x2="205" y2="98" stroke="currentColor" marker-end="url(#job-arrow)"/>
<line x1="230" y1="56" x2="325" y2="98" stroke="currentColor" marker-end="url(#job-arrow)"/>
<rect x="10" y="100" width="100" height="36" rx="6" fill="none" stroke="currentColor"/>
<text x="60" y="122" font-size="12" text-anchor="middle" fill="currentColor">Done</text>
<rect x="150" y="100" width="110" height="36" rx="6" fill="none" stroke="currentColor"/>
<text x="205" y="122" font-size="12" text-anchor="middle" fill="currentColor">Cancelled</text>
<rect x="280" y="100" width="110" height="36" rx="6" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
<text x="335" y="122" font-size="12" text-anchor="middle" fill="currentColor">Failed</text>
<text x="10" y="162" font-size="11" fill="currentColor">Done: take-output returns the bytes once.</text>
<text x="10" y="178" font-size="11" fill="currentColor">Failed: the completion carries a plugin-error code.</text>
</svg>
<figcaption>A job ends in one of three outcomes: done, cancelled, or failed with an error code.</figcaption>
</figure>

The next diagram shows the calls and events between the plugin's handler, the host and the job instance.

<figure>
<svg viewBox="0 0 420 270" role="img" aria-label="Sequence of a job: start, progress events, completion and take-output">
<title>Job sequence</title>
<defs>
<marker id="jobseq-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
<path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
</marker>
</defs>
<text x="55" y="16" font-size="12" text-anchor="middle" fill="currentColor">Handler</text>
<text x="210" y="16" font-size="12" text-anchor="middle" fill="currentColor">Host</text>
<text x="365" y="16" font-size="12" text-anchor="middle" fill="currentColor">Job instance</text>
<line x1="55" y1="24" x2="55" y2="258" stroke="currentColor" stroke-dasharray="3 3"/>
<line x1="210" y1="24" x2="210" y2="258" stroke="currentColor" stroke-dasharray="3 3"/>
<line x1="365" y1="24" x2="365" y2="258" stroke="currentColor" stroke-dasharray="3 3"/>
<line x1="55" y1="44" x2="208" y2="44" stroke="currentColor" marker-end="url(#jobseq-arrow)"/>
<text x="62" y="39" font-size="10" fill="currentColor">start(kind, input)</text>
<line x1="208" y1="62" x2="57" y2="62" stroke="currentColor" stroke-dasharray="4 3" marker-end="url(#jobseq-arrow)"/>
<text x="62" y="77" font-size="10" fill="currentColor">request id at once</text>
<line x1="210" y1="92" x2="363" y2="92" stroke="currentColor" marker-end="url(#jobseq-arrow)"/>
<text x="218" y="87" font-size="10" fill="currentColor">run(kind, input)</text>
<line x1="363" y1="116" x2="212" y2="116" stroke="currentColor" stroke-dasharray="4 3" marker-end="url(#jobseq-arrow)"/>
<text x="218" y="111" font-size="10" fill="currentColor">report-progress(40)</text>
<line x1="208" y1="136" x2="57" y2="136" stroke="currentColor" stroke-dasharray="4 3" marker-end="url(#jobseq-arrow)"/>
<text x="62" y="131" font-size="10" fill="currentColor">job-progress 40</text>
<line x1="55" y1="162" x2="208" y2="162" stroke="currentColor" marker-end="url(#jobseq-arrow)"/>
<text x="62" y="157" font-size="10" fill="currentColor">cancel(job) (optional)</text>
<line x1="363" y1="190" x2="212" y2="190" stroke="currentColor" marker-end="url(#jobseq-arrow)"/>
<text x="218" y="185" font-size="10" fill="currentColor">returns bytes or error</text>
<line x1="208" y1="212" x2="57" y2="212" stroke="currentColor" stroke-dasharray="4 3" marker-end="url(#jobseq-arrow)"/>
<text x="62" y="207" font-size="10" fill="currentColor">request-completed</text>
<line x1="55" y1="238" x2="208" y2="238" stroke="currentColor" marker-end="url(#jobseq-arrow)"/>
<text x="62" y="233" font-size="10" fill="currentColor">take-output(job)</text>
<line x1="208" y1="254" x2="57" y2="254" stroke="currentColor" stroke-dasharray="4 3" marker-end="url(#jobseq-arrow)"/>
<text x="62" y="251" font-size="10" fill="currentColor">bytes, once</text>
</svg>
<figcaption>Solid arrows are calls, dashed arrows are returns and events. A cancel is optional; when it comes, the outcome is cancelled whatever the job returns.</figcaption>
</figure>

- `start` returns a request id at once. A job waits in the queue while the concurrency limit is full.
- While the job runs, `job-progress` events arrive with the request id (`job`) and the `percent` (`f32`). Delivery is coalesced by job: only the newest value per job reaches the plugin in one batch. The event needs no grant beyond `jobs.run`.
- When the job ends, `request-completed` arrives with an outcome: `ok`, `cancelled` or `failed(code)`.
- `take-output` returns the bytes once. The kernel holds a finished result for 30 s or until it is taken. A later take returns `none`, which the SDK reports as `not-found`.

`report-progress` clamps a value below 0 to 0 and a value above 100 to 100. A NaN value is ignored.

**Cancellation.** `cancel` sets a flag and returns at once. It returns nothing: cancelling a completed, unknown or already cancelled job is a silent no-op and produces no further completion. The job sees the flag through `is-cancelled` and should return early. Once a cancel is requested, the outcome is `cancelled`, whatever the job returns or whichever deadline it reaches, and the output is dropped. A queued job that is cancelled never starts. Cancelling a finished job does nothing. When the plugin is disabled, its pending jobs are cancelled. When a plugin restarts after a trap, jobs started by the earlier instance are cancelled as well.

### Example 1: a cancellable PDF export

The example exports one cancellable PDF job on the server side. The command `com.example.notes/export-pdf` starts the job, `com.example.notes/cancel-export` stops it, and a desktop notice reports the result. Both command ids are declared under `contributes.commands`. The helpers `read_notebook`, `split_pages` and `render_page` are the plugin's own code.

Manifest excerpt:

```json
{
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0",
    "exports": ["job-runner"]
  },
  "capabilities": { "server": ["jobs.run", "notify.desktop"] },
  "contributes": { "jobs": [{ "kind": "export-pdf", "deadline_ms": 60000, "memory_mib": 256 }] }
}
```

**Grants:** `jobs.run`, `notify.desktop`

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::jobs;
use ilium_plugin_sdk::types::events::CommandInvokedEvent;
use ilium_plugin_sdk::types::intents::{NotifySpec, Urgency};
use ilium_plugin_sdk::types::requests::RequestResult;

#[ilium_plugin(side = "server", exports = ["job-runner"])]
#[derive(Default)]
struct NotesExport {
    job: Option<RequestId>,
}

impl Lifecycle for NotesExport {} // the default methods are enough

impl Handler for NotesExport {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            match event {
                Event::CommandInvoked(CommandInvokedEvent { command, .. }) => {
                    if command == "com.example.notes/export-pdf" && self.job.is_none() {
                        match jobs::start_request("export-pdf", &read_notebook()) {
                            Ok(job) => self.job = Some(job),
                            Err(error) => submissions.push(notice(&error.message)),
                        }
                    } else if command == "com.example.notes/cancel-export" {
                        if let Some(job) = self.job {
                            jobs::cancel(job);
                        }
                    }
                }
                Event::RequestCompleted(RequestResult { request: RequestRef::Call(job), outcome })
                    if self.job == Some(job) =>
                {
                    self.job = None;
                    let body = match outcome {
                        Outcome::Ok => {
                            let size = jobs::take_output(job).map_or(0, |pdf| pdf.len());
                            format!("PDF ready, {size} bytes")
                        }
                        Outcome::Cancelled => "Export cancelled".to_string(),
                        Outcome::Failed(error) => format!("Export failed: {}", error.message),
                        _ => "Export ended".to_string(),
                    };
                    submissions.push(notice(&body));
                }
                _ => {}
            }
        }
        submissions
    }
}

impl JobRunner for NotesExport {
    // Runs in the job instance, so `self.job` is not visible here.
    fn run(&mut self, _kind: String, input: Vec<u8>) -> Result<Vec<u8>, PluginError> {
        let pages = split_pages(&input);
        let mut output = Vec::new();
        for (index, page) in pages.iter().enumerate() {
            if jobs::is_cancelled() {
                return Err(PluginError {
                    code: ErrorCode::Cancelled,
                    message: "export stopped".into(),
                    retry_after_ms: None,
                });
            }
            output.extend(render_page(page));
            jobs::report_progress(100.0 * (index + 1) as f32 / pages.len() as f32);
        }
        Ok(output)
    }
}

fn notice(body: &str) -> Submission {
    Submission::new(Intent::Notify(NotifySpec {
        title: "Notes export".into(),
        body: body.into(),
        urgency: Urgency::Normal,
        sound: None,
    }))
}
```

**TypeScript**

```ts
import { definePlugin, PluginError, type Event, type Submission } from '@ilium/plugin-sdk';
import { cancel, isCancelled, reportProgress, startRequest, takeOutput } from '@ilium/plugin-sdk/host/jobs';

let activeJob: bigint | undefined;

function notice(body: string): Submission {
  // Only one notice per batch, so the fixed correlation number cannot clash.
  return {
    intent: {
      tag: 'notify',
      val: { title: 'Notes export', body, urgency: 'normal', sound: undefined },
    },
    correlation: 1n,
    dedupKey: undefined,
  };
}

export const { lifecycle, handler, jobRunner } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    const out: Submission[] = [];
    for (const event of batch) {
      if (event.tag === 'command-invoked') {
        const { command } = event.val;
        if (command === 'com.example.notes/export-pdf' && activeJob === undefined) {
          activeJob = startRequest('export-pdf', readNotebook());
        } else if (command === 'com.example.notes/cancel-export' && activeJob !== undefined) {
          cancel(activeJob);
        }
      } else if (event.tag === 'request-completed' && activeJob !== undefined) {
        const { request, outcome } = event.val;
        if (request.tag !== 'call' || request.val !== activeJob) continue;
        const finishedJob = activeJob;
        activeJob = undefined;
        const body = outcome.tag === 'ok'
          ? `PDF ready, ${takeOutput(finishedJob)?.byteLength ?? 0} bytes`
          : `Export ${outcome.tag}`;
        out.push(notice(body));
      }
    }
    return out;
  },
  run(_kind: string, input: Uint8Array): Uint8Array {
    const pages = splitPages(input);
    const parts: Uint8Array[] = [];
    pages.forEach((page, index) => {
      if (isCancelled()) throw new PluginError('cancelled', 'export stopped');
      parts.push(renderPage(page));
      reportProgress((100 * (index + 1)) / pages.length);
    });
    return concatBytes(parts);
  },
});
```

### Example 2: wait for a slow call inside a job

A job runs on its own instance and has no event loop, so it cannot wait for `request-completed`. It starts the slow call in its raw request form, calls `wait-request` to block until the call completes, and then takes the result. This job asks the selected model for a summary and returns the text. It needs `jobs.run` on the side that starts it and `inference` on the side that runs the job. The manifest declares the kind `summarise` with `deadline_ms` 2,000 (the CPU deadline) and relies on the 20 s wall-clock default, so the wait is limited to 15,000 ms.

**Grants:** `jobs.run`, `inference`

**Rust**

```rust
use ilium_plugin_sdk::host::{inference, jobs};
use ilium_plugin_sdk::host::inference::{ChatMessage, CompletionRequest};
use ilium_plugin_sdk::prelude::*;

impl JobRunner for Summariser {
    fn run(&mut self, _kind: String, input: Vec<u8>) -> Result<Vec<u8>, PluginError> {
        let text = String::from_utf8(input).map_err(|_| PluginError {
            code: ErrorCode::InvalidArgument,
            message: "the input is not UTF-8 text".into(),
            retry_after_ms: None,
        })?;
        let request = inference::complete_request(CompletionRequest {
            messages: vec![ChatMessage { role: "user".into(), content: text }],
            max_output_tokens: None,
            json_schema: None,
            purpose: "note-summary".into(),
        })?;
        jobs::report_progress(10.0);
        // Waiting counts against the wall-clock limit, not against the CPU deadline.
        jobs::wait_request(request, 15_000)?;
        match inference::take_completion(request) {
            Some(completion) => Ok(completion.text.into_bytes()),
            None => Err(PluginError {
                code: ErrorCode::NotFound,
                message: "the completion was already taken".into(),
                retry_after_ms: None,
            }),
        }
    }
}
```

**TypeScript**

```ts
import { PluginError } from '@ilium/plugin-sdk';
import { completeRequest, takeCompletion } from '@ilium/plugin-sdk/host/inference';
import { reportProgress, waitRequest } from '@ilium/plugin-sdk/host/jobs';

export function run(_kind: string, input: Uint8Array): Uint8Array {
  const text = new TextDecoder('utf-8', { fatal: true }).decode(input);
  const request = completeRequest({
    messages: [{ role: 'user', content: text }],
    maxOutputTokens: undefined,
    jsonSchema: undefined,
    purpose: 'note-summary',
  });
  reportProgress(10);
  // Waiting counts against the wall-clock limit, not against the CPU deadline.
  waitRequest(request, 15_000);
  const completion = takeCompletion(request);
  if (completion === undefined) {
    throw new PluginError('not-found', 'the completion was already taken');
  }
  return new TextEncoder().encode(completion.text);
}
```

### What the user sees

A job has no view and no handler, so it draws nothing by itself. The user sees what the plugin shows: a desktop notice when the export ends (limited to 6 notices per minute), a `progress-row` or `gauge` widget in a pane or panel, updated from `job-progress` (see [ui](/pages/api-ui/)), and the lines in `ilium plugin logs`. Refused calls and traps are logged there as well.

A progress row built from the `percent` of `job-progress` is drawn by the client's progress widgets. The status prefix glyphs (`▶` running, `✓` done, `✕` error) and the bar characters (`█` filled, `░` unfilled) are those of the client progress bar.

<figure>
<pre class="term">╭  ≡ ● · Notes export──────────────────────────────────────────────────────╮
│ <span class="t-ok">▶ RUNNING  64%  ███████████████░░░░░░░░░</span>                                 │
│ <span class="t-dim">page 9 of 14 · Esc cancels, the job stops after the current page</span>         │
╰──────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>A panel built by the example plugin from <code>job-progress</code> values. The border, the title chrome, the status glyph and the bar characters are those that the client draws; the text inside is the plugin's own. This frame is composed by hand from the client's glyph table, not captured from a running build.</figcaption>
</figure>

<pre class="term"><span class="t-prompt">$</span> ilium plugin logs com.example.notes --tail 4
<span class="t-dim">2026-10-12 09:30:05</span> info  job export-pdf started (input 412 KiB)
<span class="t-dim">2026-10-12 09:30:09</span> info  job export-pdf progress 64
<span class="t-dim">2026-10-12 09:30:12</span> info  job export-pdf done (output 1.2 MiB)
<span class="t-dim">2026-10-12 09:30:12</span> info  notice sent: PDF ready, 1258291 bytes
</pre>

The log lines are written by the plugin through the `log` import; the exact text is the plugin's choice. They are an illustration of the form, not captured output.

### Errors

| Where it appears | Code | Cause |
|---|---|---|
| Call to `start` | `invalid-argument` | The kind is not declared in `contributes.jobs`, or the input is over `job_io_mib`. |
| Call to `start` | `busy` | The job queue is full. |
| Call to `start` | `not-permitted` | `jobs.run` is not granted. |
| Call to `start` | `quota-exceeded` | The CPU budget of the execution group is spent. `retry-after-ms` is set. |
| Completion | `failed(timed-out)` | The CPU or wall-clock deadline passed. |
| Completion | `failed(quota-exceeded)` | The job reached its memory limit. |
| Completion | `failed(invalid-argument)` | The output is over `job_io_mib` (the limit applies to input and output each). |
| Completion | `failed(internal)` | A trap in the job instance. The trap text is in `message`. |
| Completion | `failed(<code>)` | `run` returned an error with that code. |
| Completion | `cancelled` | A cancel was requested. |
| Call to `wait-request` | `not-found` | The request id is unknown. |
| Call to `wait-request` | `timed-out` | The call did not complete within `timeout-millis`. The request keeps running. |
| Call to `wait-request` | `unsupported` | `wait-request` was called outside `run`. |
| `take-output` | `not-found` in the SDK wrappers | The output was already taken or held longer than 30 s. |

Concurrency is not an error: a job past `concurrent_jobs` waits in the queue. A job failure never counts toward the plugin's restart rule, so a failing job cannot disable the plugin.

### Edge cases

- A kind that is declared but whose side lacks `job-runner` in `exports` fails the install review, not the call.
- A job that returns normally after `cancel` still ends as `cancelled`, and its output is dropped.
- Two handlers that start jobs of the same kind get separate request ids; the input is copied at the call.
- The job instance may be reused for the next job, so keep no state in it across jobs.
- `report-progress` outside `run` is ignored. It has no result to carry an error.

### Used by

- Workspace search (feature 9), session stats (feature 31) and the compaction optimiser (feature 82) run long reads as jobs.
- Convert a session between Claude and Codex (feature 77) starts its conversion with `jobs.start`.

### See also

- [Plugin exports](/pages/api-exports/) for the `job-runner` export.
- [Events](/pages/api-events/) for `job-progress` and `request-completed`.
- [Background jobs](/pages/background-jobs/) for the execution model and the cancellation rules.
- [Concurrency for plugin authors](/pages/concurrency-for-plugin-authors/) for the call order, deadlines and yield rules that handlers and jobs share.
- [Requests and outcomes](/pages/requests-and-outcomes/) for holds and takes.
- [inference](/pages/api-inference/) for the call used in the second example.
- [codec](/pages/api-codec/) for work that fits under 16 MiB and needs no job.
- [Capability: jobs.run](/pages/capability-jobs-run/) for the grant.
- [Errors and limits](/pages/reference-errors-and-limits/) for the full code table.

Checked against `ilium:plugin@0.1.0`.
