---
layout: page
title: "5. TODO scanner with files and jobs"
permalink: /pages/tutorial-05-todo-scanner/
---

## 5. TODO scanner with files and jobs

This step scans the project's Rust source files for `TODO` lines and lists them in a panel. The file list comes from a scoped scan of the project. The text search runs in a background job, so a large project does not block the server. The panel shows the job's progress in a gauge, and Enter on a result opens the editor at that line.

The step adds three parts: a pure search in the core crate, a server workflow that chains a scan into a job and publishes the result, and a client panel that subscribes to the result.

### You need

- [Tutorial step 4](/pages/tutorial-04-finished-notice/) finished.
- Server grants, new: `fs.read:project` (the scan and the reads; the scope is the session's project directory, and its `.ilium` folder is hidden, see [capability fs.read](/pages/capability-fs-read/)) and `jobs.run` (see [capability jobs.run](/pages/capability-jobs-run/)). Existing grants are unchanged.
- Client grants, new: `fs.read:project` (the editor needs it for the path it opens), `editor.edit` (see [capability editor.edit](/pages/capability-editor-edit/)) and `topics.subscribe:example.agent-tools/*` (see [capability topics.subscribe](/pages/capability-topics-subscribe/)).
- Rust with `ilium-plugin-sdk` and `serde_json`, or TypeScript with `@ilium/plugin-sdk` and Bun. Every listing has a **Rust** block and a **TypeScript** block. The TypeScript scan workflow follows the Rust one in the same section.

### What changed from step 4

- `core/src/todo.rs` is new. It finds `TODO` lines in a text and makes no host calls.
- `server/src/scan.rs` is new. It starts the scan, starts the job and publishes the results.
- `server/src/lib.rs` routes the scan command, request completions, job progress and topics to `scan.rs`.
- `client/src/todos.rs` is new. It shows the results and opens the editor.
- `manifest.json` adds the grants, the job, the topics, the command, the panel and the keybinding.

### The flow

`fs.scan` returns a request id at once. When its `request-completed` event arrives, the plugin takes the file list with `take-entries` and starts a job with it. The job reads each file and returns the hits as JSON. When the job completes, the plugin publishes the hits to `example.agent-tools/todos`, and the panel redraws. Each `job-progress` event becomes a publish to `example.agent-tools/scan-progress`.

<figure>
<svg viewBox="0 0 400 320" role="img" aria-label="Command, scan, job, publish and panel for the TODO scan">
  <title>TODO scan request chain</title>
  <desc>A command starts fs.scan. Its completion starts a job. The job reads files through the project scope, the server publishes the hits, and the client panel shows them.</desc>
  <defs>
    <marker id="todo-arrow" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
      <path d="M0,0 L8,4 L0,8 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="20" y="8" width="240" height="34" rx="4" fill="none" stroke="currentColor"/>
  <text x="140" y="29" font-size="11" text-anchor="middle" fill="currentColor">Command: scan-todos</text>
  <line x1="140" y1="42" x2="140" y2="60" stroke="currentColor" marker-end="url(#todo-arrow)"/>
  <rect x="20" y="62" width="240" height="34" rx="4" fill="none" stroke="currentColor"/>
  <text x="140" y="83" font-size="11" text-anchor="middle" fill="currentColor">Server: fs.scan (async)</text>
  <line x1="140" y1="96" x2="140" y2="114" stroke="currentColor" marker-end="url(#todo-arrow)"/>
  <rect x="20" y="116" width="240" height="34" rx="4" fill="none" stroke="currentColor"/>
  <text x="140" y="137" font-size="11" text-anchor="middle" fill="currentColor">Server: jobs.start (todo-scan)</text>
  <line x1="140" y1="150" x2="140" y2="168" stroke="currentColor" marker-end="url(#todo-arrow)"/>
  <rect x="20" y="170" width="240" height="34" rx="4" fill="none" stroke="currentColor"/>
  <text x="140" y="191" font-size="11" text-anchor="middle" fill="currentColor">Job: job-runner.run on worker bank</text>
  <line x1="140" y1="204" x2="140" y2="222" stroke="currentColor" marker-end="url(#todo-arrow)"/>
  <rect x="20" y="224" width="240" height="34" rx="4" fill="none" stroke="currentColor"/>
  <text x="140" y="245" font-size="11" text-anchor="middle" fill="currentColor">Server: topics.publish</text>
  <line x1="140" y1="258" x2="140" y2="276" stroke="currentColor" marker-end="url(#todo-arrow)"/>
  <rect x="20" y="278" width="240" height="34" rx="4" fill="none" stroke="currentColor"/>
  <text x="140" y="299" font-size="11" text-anchor="middle" fill="currentColor">Client: TODO panel and editor</text>
  <rect x="296" y="62" width="100" height="34" rx="4" fill="none" stroke="currentColor" stroke-dasharray="5 3"/>
  <text x="346" y="83" font-size="11" text-anchor="middle" fill="currentColor">Scope: project</text>
  <line x1="262" y1="79" x2="294" y2="79" stroke="currentColor" stroke-dasharray="5 3" marker-end="url(#todo-arrow)"/>
  <rect x="296" y="170" width="100" height="34" rx="4" fill="none" stroke="currentColor" stroke-dasharray="5 3"/>
  <text x="346" y="191" font-size="11" text-anchor="middle" fill="currentColor">Scope: project</text>
  <line x1="262" y1="187" x2="294" y2="187" stroke="currentColor" stroke-dasharray="5 3" marker-end="url(#todo-arrow)"/>
</svg>
<figcaption>Command, scan, job, publish and panel. Dashed boxes are the project scope that the scan and the job read through.</figcaption>
</figure>

### Surface used by this step

| Call or event | Side | Kind | Meaning |
|---|---|---|---|
| `fs::scan(&path, glob, max_entries)` | server | async, `request-id` | Lists files that match a glob such as `**/*.rs`. `max_entries` is at most 65,536 (see [Fixed caps](/pages/reference-errors-and-limits/#requestable-limits)). |
| `fs::take_entries(request)` | server | sync, `Option` | The matched paths and entries, once. |
| `fs::read(&path, offset, max_bytes)` | server, job | sync, `Result` | Reads up to 1 MiB. A job uses the plugin's own grants (U29). |
| `jobs::start(kind, input)` | server | async, `request-id` | Starts a declared job kind with bytes as input. |
| `jobs::take_output(job)` | server | sync, `Option` | The job's bytes, once. |
| `jobs::report_progress(percent)` | job | sync | Reports 0 to 100. Only the newest value per job arrives. |
| `jobs::is_cancelled()` | job | sync, `bool` | True after a cancel. |
| `topics::publish(topic, payload_json)` | server | sync, `Result` | Publishes JSON to a declared topic under the plugin's id. |
| Event `command-invoked` | server | event | The scan command ran. |
| Event `request-completed` | server | event | A scan or job finished, with its outcome. |
| Event `job-progress` | server | event | The percent of a running job. |
| Event `topic-published` | client | event | The payload of a subscribed topic. |
| Intent `editor-open` | client | intent | Opens a file at a 0-based line, in a split pane. |
| Widgets `gauge` and `list` | client | UI tree | The progress bar and the hit list. |

Widget defaults come from the SDK builder (W1). A list sends `activated` with the row index on Enter (W11).

### Manifest

This step adds the entries below to the step 4 manifest. Keep every other entry from step 4. `exports` lists `job-runner`, so the host creates a job instance from the server component. The job's `deadline_ms` (a limit on CPU time, not wall time) and `memory_mib` are requested limits. A value above the tier ceiling is clamped and logged, never refused. For a T1 plugin the ceilings are 60,000 ms of CPU and 512 MiB (L2).

```json
{
  "server": { "entry": "server.wasm", "world": "ilium:server/base@0.1.0", "exports": ["job-runner"] },
  "capabilities": {
    "server": ["fs.read:project", "jobs.run"],
    "client": ["fs.read:project", "editor.edit", "topics.subscribe:example.agent-tools/*"]
  },
  "contributes": {
    "commands": [{ "id": "scan-todos", "title": "Scan the project for TODO lines" },
                 { "id": "show-todos", "title": "Show TODO results" }],
    "keybindings": [
      { "command": "example.agent-tools/scan-todos", "chord": "s", "mode": "prefix" },
      { "command": "example.agent-tools/show-todos", "chord": "t", "mode": "prefix" }
    ],
    "panels": [{ "id": "todos", "title": "TODO scan", "modal": false }],
    "jobs": [{ "kind": "todo-scan", "deadline_ms": 10000, "memory_mib": 64 }],
    "topics": { "publishes": ["example.agent-tools/todos", "example.agent-tools/scan-progress"] }
  }
}
```

### The pure search

The search makes no host calls, so `cargo test` runs it on the development machine.

**Rust**

```rust
// core/src/todo.rs. Finds TODO lines in one file's text.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoHit {
    pub path: String,
    pub line: u32,
    pub text: String,
}

/// Returns every line that contains `TODO`, numbered from 1.
pub fn find_todos(path: &str, text: &str) -> Vec<TodoHit> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| line.contains("TODO"))
        .map(|(index, line)| TodoHit {
            path: path.to_string(),
            line: index as u32 + 1,
            text: line.trim().to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_lines_from_one() {
        let hits = find_todos("src/a.rs", "fn a() {}\n// TODO: fix\n");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].line, 2);
    }
}
```

### Server: scan, job and publish

`scan.rs` keeps the request id of the scan and of the job in two thread-local slots. A completion for any other request matches neither slot and is ignored. `run_job` runs on the worker bank, in a second instance of the plugin, under the manifest's CPU deadline and memory limit.

**Rust**

```rust
// server/src/scan.rs. TODO scan workflow on the server side.
use std::cell::Cell;

use agent_tools_core::todo::{find_todos, TodoHit};
use ilium_plugin_sdk::host::{fs, jobs, log::{self, Level}, topics};
use ilium_plugin_sdk::prelude::*;

pub const SCAN_COMMAND: &str = "example.agent-tools/scan-todos";
pub const JOB_KIND: &str = "todo-scan";
pub const RESULTS_TOPIC: &str = "example.agent-tools/todos";
pub const PROGRESS_TOPIC: &str = "example.agent-tools/scan-progress";
const SCAN_GLOB: &str = "**/*.rs";
const MAX_FILES: u32 = 4096;
const MAX_READ_BYTES: u32 = 1024 * 1024;

thread_local! {
    static SCAN_REQUEST: Cell<Option<u64>> = Cell::new(None);
    static JOB_REQUEST: Cell<Option<u64>> = Cell::new(None);
}

fn project_path(path: &str) -> FsPath {
    FsPath { scope: "project".to_string(), path: path.to_string() }
}

/// Starts the scan. Its completion arrives later as `request-completed`.
pub fn start_scan() {
    match fs::scan(&project_path(""), SCAN_GLOB, MAX_FILES) {
        Ok(request) => SCAN_REQUEST.with(|slot| slot.set(Some(request))),
        Err(error) => log::log(Level::Warn, &format!("scan not started: {}", error.message)),
    }
}

/// Returns true when the completed request belongs to this workflow.
pub fn on_request_completed(request: u64) -> bool {
    if SCAN_REQUEST.with(|slot| slot.get()) == Some(request) {
        SCAN_REQUEST.with(|slot| slot.set(None));
        start_job(request);
        return true;
    }
    if JOB_REQUEST.with(|slot| slot.get()) == Some(request) {
        JOB_REQUEST.with(|slot| slot.set(None));
        publish_results(request);
        return true;
    }
    false
}

fn start_job(scan_request: u64) {
    let Some(entries) = fs::take_entries(scan_request) else {
        log::log(Level::Warn, "scan finished without a result");
        return;
    };
    let paths: Vec<String> = entries.into_iter().map(|(path, _)| path).collect();
    let Ok(input) = serde_json::to_vec(&paths) else {
        log::log(Level::Warn, "scan result could not be encoded");
        return;
    };
    match jobs::start(JOB_KIND, &input) {
        Ok(job) => JOB_REQUEST.with(|slot| slot.set(Some(job))),
        Err(error) => log::log(Level::Warn, &format!("job not started: {}", error.message)),
    }
}

fn publish_results(job: u64) {
    let Some(output) = jobs::take_output(job) else {
        log::log(Level::Warn, "job finished without output");
        return;
    };
    if let Err(error) = topics::publish(RESULTS_TOPIC, &String::from_utf8_lossy(&output)) {
        log::log(Level::Warn, &format!("results not published: {}", error.message));
    }
}

/// Forwards job progress as a topic, so the panel can draw the gauge.
pub fn on_job_progress(job: u64, percent: f32) {
    if JOB_REQUEST.with(|slot| slot.get()) != Some(job) {
        return;
    }
    let payload = serde_json::json!({ "percent": percent }).to_string();
    if let Err(error) = topics::publish(PROGRESS_TOPIC, &payload) {
        log::log(Level::Warn, &format!("progress not published: {}", error.message));
    }
}

/// The job body. Runs on the worker bank, not in the event handler.
pub fn run_job(kind: &str, input: &[u8]) -> Result<Vec<u8>, PluginError> {
    if kind != JOB_KIND {
        return Err(plugin_error(ErrorCode::InvalidArgument, "unknown job kind"));
    }
    let paths: Vec<String> = serde_json::from_slice(input)
        .map_err(|error| plugin_error(ErrorCode::InvalidArgument, &error.to_string()))?;
    let total = paths.len().max(1);
    let mut hits: Vec<TodoHit> = Vec::new();
    for (index, path) in paths.iter().enumerate() {
        if jobs::is_cancelled() {
            return Err(plugin_error(ErrorCode::Cancelled, "scan cancelled"));
        }
        let bytes = fs::read(&project_path(path), 0, MAX_READ_BYTES)?;
        hits.extend(find_todos(path, &String::from_utf8_lossy(&bytes)));
        jobs::report_progress(((index + 1) * 100 / total) as f32);
    }
    serde_json::to_vec(&hits)
        .map_err(|error| plugin_error(ErrorCode::Internal, &error.to_string()))
}

fn plugin_error(code: ErrorCode, message: &str) -> PluginError {
    PluginError { code, message: message.to_string(), retry_after_ms: None }
}

#[ilium_plugin(side = "server", exports = ["job-runner"])]
#[derive(Default)]
pub struct AgentTools;

impl JobRunner for AgentTools {
    fn run(&mut self, kind: String, input: Vec<u8>) -> Result<Vec<u8>, PluginError> {
        run_job(&kind, &input)
    }
}
```

The handler in `server/src/lib.rs` gets these new arms. The step 4 arms are unchanged.

```rust
// server/src/lib.rs, new arms of the handler body.
Event::CommandInvoked(invocation) if invocation.command == scan::SCAN_COMMAND => {
    scan::start_scan();
}
Event::RequestCompleted(result) => {
    if let RequestRef::Call(request) = result.request {
        scan::on_request_completed(request);
    }
}
Event::JobProgress(progress) => scan::on_job_progress(progress.request_id, progress.percent),
```

Points to notice:

- A failed scan takes the same branch as a successful one. `take_entries` returns `None`, and the log says `scan finished without a result`.
- A second scan replaces the first id in its slot, so the first completion is ignored. The host holds that result for 30 seconds and then drops it.
- A job failure never counts toward the plugin's restart rule.

### Client: the results panel

The client keeps the last results and the last progress value. Enter on a row sends an `editor-open` intent.

**Rust**

```rust
// client/src/todos.rs. Results panel and the editor jump.
use std::cell::{Cell, RefCell};

use agent_tools_core::todo::TodoHit;
use ilium_plugin_sdk::host::ui;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::intents::{EditorOpenSpec, EditorPlacement};
use ilium_plugin_sdk::types::layout::{Constraint, Direction};
use ilium_plugin_sdk::types::ui::ViewTarget;
use ilium_plugin_sdk::ui::build::Tree;

pub const PANEL: &str = "todos";
const RESULTS_TOPIC: &str = "example.agent-tools/todos";
const PROGRESS_TOPIC: &str = "example.agent-tools/scan-progress";

thread_local! {
    static HITS: RefCell<Vec<TodoHit>> = RefCell::new(Vec::new());
    static PERCENT: Cell<f64> = Cell::new(0.0);
}

/// Stores a topic payload. Returns true when the panel should redraw.
pub fn on_topic(topic: &str, payload_json: &str) -> bool {
    if topic == RESULTS_TOPIC {
        let hits: Vec<TodoHit> = serde_json::from_str(payload_json).unwrap_or_default();
        HITS.with(|cell| *cell.borrow_mut() = hits);
        PERCENT.with(|cell| cell.set(100.0));
        return true;
    }
    if topic == PROGRESS_TOPIC {
        let percent = serde_json::from_str::<serde_json::Value>(payload_json)
            .ok()
            .and_then(|value| value["percent"].as_f64())
            .unwrap_or(0.0);
        PERCENT.with(|cell| cell.set(percent));
        return true;
    }
    false
}

/// Opens the editor at the selected hit. Hit lines are 1-based, but the editor
/// takes a 0-based line (U32), so the line is converted here.
pub fn open_hit(index: u32) -> Option<Submission> {
    let hit = HITS.with(|cell| cell.borrow().get(index as usize).cloned())?;
    Some(Submission {
        intent: Intent::EditorOpen(EditorOpenSpec {
            path: FsPath { scope: "project".to_string(), path: hit.path },
            line: Some(hit.line - 1),
            placement: EditorPlacement::Split,
        }),
        correlation: u64::from(hit.line),
        dedup_key: None,
    })
}

/// A gauge for the scan progress above the list of hits.
pub fn render_panel() -> UiTree {
    let percent = PERCENT.with(|cell| cell.get());
    let rows: Vec<String> = HITS.with(|cell| {
        cell.borrow()
            .iter()
            .map(|hit| format!("{}:{}  {}", hit.path, hit.line, hit.text))
            .collect()
    });
    let mut tree = Tree::new();
    let gauge = tree.gauge(percent / 100.0).key("progress");
    let list = tree.list(rows).key("hits");
    let body = tree.layout(
        Direction::Vertical,
        vec![Constraint::Length(1), Constraint::Fill(1)],
        vec![gauge, list],
    );
    let root = tree.block().title("TODO scan").child(body);
    tree.finish(root)
}

pub fn is_todos_panel(view: &ViewTarget) -> bool {
    matches!(view, ViewTarget::Panel(name) if name == PANEL)
}
```

```rust
// client/src/lib.rs, new arms of the handler body.
Event::TopicPublished(topic) => {
    if todos::on_topic(&topic.topic, &topic.payload_json) {
        ui::invalidate(ViewTarget::Panel(todos::PANEL.to_string()));
    }
}
Event::UiAction(action) if todos::is_todos_panel(&action.view) => {
    if let (UiActionKind::Activated, UiValue::Index(index)) = (&action.kind, &action.value) {
        submissions.extend(todos::open_hit(*index));
    }
}
```

Points to notice:

- The panel reads no files. It shows what the server published.
- Results arrive once, when the job completes. The list does not grow during a scan, and the gauge moves on each progress value.
- Without `fs.read` for the path, `editor-open` is refused with `not-permitted` (E4).

### The same workflow in TypeScript

The TypeScript server uses the same commands, topics and manifest. `start` resolves with the job output and gives no job id, so the scan sets a `scanning` flag and forwards each `job-progress` event while it is set. Errors are thrown as `PluginError`, and a `u64` such as the read offset is a `bigint` (S5).

**TypeScript**

```ts
// core/src/todo.ts. Finds TODO lines in one file's text. No host imports.
export interface TodoHit {
  path: string;
  line: number;
  text: string;
}

export function findTodos(path: string, text: string): TodoHit[] {
  const hits: TodoHit[] = [];
  text.split('\n').forEach((line, index) => {
    if (line.includes('TODO')) {
      hits.push({ path, line: index + 1, text: line.trim() });
    }
  });
  return hits;
}
```

**TypeScript**

```ts
// server/src/scan.ts. TODO scan workflow in TypeScript.
import { definePlugin, PluginError } from '@ilium/plugin-sdk';
import { read, scan } from '@ilium/plugin-sdk/host/fs';
import { isCancelled, reportProgress, start } from '@ilium/plugin-sdk/host/jobs';
import { log } from '@ilium/plugin-sdk/host/log';
import { publish } from '@ilium/plugin-sdk/host/topics';
import type { Event, Submission } from '@ilium/plugin-sdk/types';
import { findTodos, type TodoHit } from '../../core/src/todo';

const SCAN_COMMAND = 'example.agent-tools/scan-todos';
const JOB_KIND = 'todo-scan';
const RESULTS_TOPIC = 'example.agent-tools/todos';
const PROGRESS_TOPIC = 'example.agent-tools/scan-progress';
const MAX_READ_BYTES = 1024 * 1024;
let scanning = false;

const projectPath = (path: string) => ({ scope: 'project', path });

async function runScan(): Promise<void> {
  scanning = true;
  try {
    const entries = await scan(projectPath(''), '**/*.rs', 4096);
    const paths = entries.map(([path]) => path);
    const output = await start(JOB_KIND, new TextEncoder().encode(JSON.stringify(paths)));
    publish(RESULTS_TOPIC, new TextDecoder().decode(output));
  } catch (error) {
    const reason = error instanceof PluginError ? error.message : String(error);
    log('warn', `scan failed: ${reason}`);
  } finally {
    scanning = false;
  }
}

function onEvents(batch: Event[]): Submission[] {
  for (const event of batch) {
    if (event.tag === 'command-invoked' && event.val.command === SCAN_COMMAND) {
      void runScan(); // runScan handles its own errors
    } else if (event.tag === 'job-progress' && scanning) {
      publish(PROGRESS_TOPIC, JSON.stringify({ percent: event.val.percent }));
    }
  }
  return [];
}

function runJob(kind: string, input: Uint8Array): Uint8Array {
  if (kind !== JOB_KIND) {
    throw new PluginError('invalid-argument', `unknown job kind ${kind}`);
  }
  const paths: string[] = JSON.parse(new TextDecoder().decode(input));
  const hits: TodoHit[] = [];
  for (const [index, path] of paths.entries()) {
    if (isCancelled()) {
      throw new PluginError('cancelled', 'scan cancelled');
    }
    const bytes = read(projectPath(path), 0n, MAX_READ_BYTES);
    hits.push(...findTodos(path, new TextDecoder().decode(bytes)));
    reportProgress(((index + 1) * 100) / paths.length);
  }
  return new TextEncoder().encode(JSON.stringify(hits));
}

export const { lifecycle, handler, jobRunner } = definePlugin({
  lifecycle: {
    activate(context) {
      log('info', `todo scanner active for ${context.plugin}`);
    },
  },
  handler: { onEvents },
  jobRunner: { run: runJob },
});
```

### Test it

Run the search tests without the host, then start the plugin and try the panel.

**Rust**

```bash
cargo test -p agent-tools-core todo
ilium plugin dev
```

**TypeScript**

```bash
bun test core/src/todo.test.ts
ilium plugin dev
```

Press the prefix key and `s` to scan, then the prefix key and `t` to show the results. Press Enter on a row to open the editor at that line.

### Progress row in the panel

The gauge shows progress as a bar. The `progress-row` widget shows one job with a state glyph, a title, a percent and a message, so the panel can name the running scan. A builder call replaces the gauge call: `progress_row(state, title, percent, message)` in Rust, `progressRow({ state, title, percent, message })` in TypeScript. The states are `waiting`, `running`, `done` and `error`, drawn `○`, `▶`, `✓` and `!`. The panel code of this step keeps the gauge.

### Edge cases and errors

| Situation | What happens |
|---|---|
| No `TODO` lines | The topic carries `[]`. The list is empty and the gauge reaches 100. |
| A file cannot be read | The job fails with the read's code, such as `not-found` or `io`. The panel keeps the last results. |
| `fs.scan` finds more than 4,096 `.rs` files | The scan completes as `failed(quota-exceeded)` and no job starts. The result is never truncated (E5). |
| A file is over 1 MiB | Only the first MiB is read. For the rest, call `fs::read` again with a larger offset. |
| 16 jobs already queued | `jobs::start` returns `busy` (see [jobs](/pages/api-jobs/)). |
| Job runs past 10 seconds of CPU | The job fails as `timed-out`, and its output is dropped. |
| Results over 64 KiB | `topics::publish` refuses the payload with `invalid-argument` (a topic payload is at most 64 KiB, see [topics](/pages/capability-topics-subscribe/)). A few hundred hits can reach that size. |

### What the user sees

The panel is one box with a gauge above a list. While the job runs, the list shows the hits of the last completed scan, and the gauge moves. When the job completes, the new hits replace the list.

<figure>
<pre class="term">╭  ≡ ● · TODO scan ──────────────────────────────────────────────╮
│ <span class="t-ok">▶</span> Scanning  ▰▰▰▰▰▰▱▱▱▱ 60%                                     │
│                                                                │
│ <span class="t-dim">File</span>               <span class="t-dim">Line  Text</span>                                  │
│ ──────────────────────────────────────────────────────────     │
│<span class="t-hi"> src/store.rs         42  // TODO: expire entries after toke</span>    │
│ src/auth.rs          17  // TODO: rotate the signing key       │
│ src/http.rs          88  // TODO: retry on 503                 │
╰────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>Illustrative. A rounded panel with a running-state glyph and a meter-style bar for the 60 percent of the scan. The table keeps the hits of the last completed scan. A 0.1 host draws the same row with the <code>gauge</code> widget.</figcaption>
</figure>

Enter on a row opens the editor in a split pane, with the line marked. The gutter numbers are 1-based.

<figure>
<pre class="term">╭  ≡ ● · src/store.rs ───────────────────────────────────────────╮
│ 40 │     let entry = self.entries.get(&key)?;                  │
│ 41 │     Some(entry.value.clone())                             │
│ <span class="t-hi">42 │     // TODO: expire entries after the token lifetime</span>      │
│ 43 │ }                                                         │
╰────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>Illustrative. The editor opens at line 42, which is highlighted. The gutter numbers are 1-based. The code around it is a stand-in.</figcaption>
</figure>

### See also

- [Tutorial step 4](/pages/tutorial-04-finished-notice/) explains the request outcomes used here.
- [Background jobs](/pages/background-jobs/) explains job deadlines, progress and cancel.
- [fs](/pages/api-fs/) and [jobs](/pages/api-jobs/) list every call used in this step.
- [Tutorial step 6: a notes pane](/pages/tutorial-06-notes-pane/) adds a pane kind.

API checked against `ilium:plugin@0.1.0`.
