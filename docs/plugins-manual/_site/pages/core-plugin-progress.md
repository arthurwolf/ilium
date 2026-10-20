# Progress

## Progress

Progress monitors let a long task show how far it is. An agent or a script registers a probe command with `ilium progress set`, the server runs the probe on an interval, and the tree and status bar show the result. `ilium progress set --wait` and `ilium wait` block until the task ends. The plugin id is `ilium.core.progress` and it runs on both sides: the server side owns the CLI, the probes and the watchers, and the client side draws the footer and the status bar segment.

### What it covers

- Progress monitor: a probe that prints one JSON object with a status, a percent and a message, described in the Progress monitor section of the automation guide.
- The `ilium progress` command and `ilium wait`, described in the CLI reference.
- Long-lived watchers that stream progress lines instead of being polled (new in the plugin build, not parity).
- The progress footer in the tree and the count in the status bar, described in the agent monitoring guide.

### See it

<figure><img src="/assets/demos/07-progress-monitor.gif" alt="A progress monitor row fills while a long task runs and ends as done" loading="lazy"><figcaption>A registered monitor appears in the tree footer, updates on each probe run and settles when the task ends.</figcaption></figure>

### What the user sees

<figure>
<pre class="term">╭─ Progress ─────────────────────────────────────╮
│ <span class="t-hi">▶ RUNNING  42%</span> <span class="t-hi">████████</span><span class="t-dim">░░░░░░░░░░░░</span>            │
│ <span class="t-dim">  Running the unit tests, crate 9 of 22</span>        │
│ <span class="t-ok">✓ DONE  100%</span> <span class="t-ok">████████████████████</span>              │
│ <span class="t-dim">  Export of 412 sessions finished</span>              │
╰────────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Two <code>progress-row</code> widgets with percent and message. Illustrative: the bar glyphs depend on the icon set.</figcaption>
</figure>

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium progress set --command '/opt/probe.sh' --wait
{"type":"progress_set","request_id":31,"pane_id":12,"monitor_id":4,
 "progress":{"monitor_id":4,"report":{"job_id":"export-412",
 "status":"running","percent":0,"message":"Exporting 412 sessions",
 "details":"","error":null},"monitor_health":{"state":"healthy"},
 "last_observed_unix_millis":1791800000000}}
{"type":"progress_wait","pane_id":12,"monitor_id":4,"outcome":"done",
 "exit_code":0,"waited_seconds":412,"composer_notice":"suppressed",
 "next":"The task finished. ...","progress":{"monitor_id":4,...}}
<span class="t-prompt">$</span> echo $?
<span class="t-ok">0</span>
</pre>
<figcaption>Rendered at 80 columns or fewer. Illustrative. Each record is one line on a real terminal; it is wrapped here, and the text of <code>next</code> and the nested <code>progress</code> object are cut. The registration record comes first. The final <code>progress_wait</code> record ends the wait and the exit status is 0 for done.</figcaption>
</figure>

### Behaviour contract

Field order and names equal the baseline; extra fields are not allowed. The records are:

- `progress_set`: `{"type":"progress_set","request_id":<n>,"pane_id":<n>,"monitor_id":<n>,"progress":{"monitor_id":<n>,"report":{...},"monitor_health":{"state":"..."},"last_observed_unix_millis":<n>}}`
- `progress_wait`: `{"type":"progress_wait","pane_id":<n>,"monitor_id":<n>,"outcome":"done|error|monitor-failed|cleared|no-monitor|still-running","exit_code":<n>,"waited_seconds":<n>,"composer_notice":"suppressed|may-also-arrive|none","next":"...","progress":{...}}`

| Id | Rule | Exact value or text | Baseline source |
|---|---|---|---|
| progress-B01 | Exit status of `progress set --wait` and `progress wait` | 0 done, 2 usage or rejected (for example `monitor-ambiguous`), 3 task error, 4 monitor failed (outcome unknown), 5 monitor replaced or cleared, 6 `--timeout-seconds` elapsed | `src/docs/cli-reference.md#progress-wait` |
| progress-B02 | `outcome` for each exit | 0 `done`, 3 `error`, 4 `monitor-failed`, 5 `cleared` or `no-monitor`, 6 `still-running` | `src/docs/cli-reference.md#progress-wait` |
| progress-B03 | Text of `next` | `done`: "The task finished. Verify its output, then continue." `error`: "The task reported failure. Inspect its log or error, fix, and retry." `monitor-failed`: "The probe stopped working, so the task outcome is unknown. Check the task directly; do not assume success." `cleared`: "The monitor was cleared, or is not registered on this pane. Check `ilium progress status`." `no-monitor`: "This pane has no progress monitor. Register one with `ilium progress set`." `still-running`: "The task is still running. Run the same wait command again to keep waiting." A settled outcome adds " When you have handled it, remove this monitor with `ilium progress clear --monitor-id <id>`." | `ilium/src/progress_wait.rs` |
| progress-B04 | `composer_notice` | `suppressed` while a wait takes the outcome, `may-also-arrive` for a settled outcome without a wait, `none` otherwise | `src/docs/cli-reference.md#progress-wait` |
| progress-B05 | Monitors per pane | At most 8. The 9th live monitor is rejected with code `too-many-monitors`. A settled monitor whose outcome was delivered makes room | `src/docs/cli-reference.md#progress-set` |
| progress-B06 | `--interval-seconds` | Default 1, range 1 to 86400 | `src/docs/cli-reference.md#progress-set` |
| progress-B07 | Probe size limits | Command line 16 KiB, stdout 64 KiB, stderr 16 KiB, `job_id` 256 bytes, `message` 2048 bytes, `details` 8192 bytes, `error` 4096 bytes | `src/docs/cli-reference.md#the-probe-contract` |
| progress-B08 | Probe run | One shell command line, run by the server with the session project root as working directory, at most 30 s (`timeout_millis: 30_000`) | `src/docs/cli-reference.md#the-probe-contract` |
| progress-B09 | Probe failure counting | After 3 consecutive probe failures the monitor becomes `failed` (monitor health), not the task | `src/docs/cli-reference.md#the-probe-contract` |
| progress-B10 | CLI reply wait | The CLI waits up to 45 s for the server reply | `src/docs/cli-reference.md#progress-status` |
| progress-B11 | Same probe twice | Registering the same probe command again in the same pane while its monitor runs returns the existing monitor id | `src/docs/automation.md#progress-monitor` |
| progress-B12 | Restore after restart | A monitor is re-established only after one preflight probe returns the same `job_id`, and it gets a fresh monitor id | `ilium-server/src/persistence.rs` |
| progress-B13 | Delivery states | `not-queued`, `queued`, `attempted`, `delivered-to-pty`, `uncertain`, `not-deliverable`, `collected-by-waiter`. Only `queued` is retried after a restart. Undelivered outcomes are re-sent every 20 s | `ilium-server/src/agent_delivery.rs` |
| progress-B14 | Terminal result | Typed into the agent composer once (`composer_notice`); not typed when a running wait collected it | `src/docs/automation.md#progress-monitor` |
| progress-B15 | Rejection codes | `disabled`, `invalid-request`, `invalid-probe-report`, `probe-spawn-failed`, `probe-timed-out`, `probe-exited-non-zero`, `probe-output-too-large`, `probe-io-failed`, `pane-not-found`, `stale-monitor`, `too-many-monitors`, `monitor-ambiguous` | `src/docs/cli-reference.md#progress-status` |

### Settings it reads

The plugin owns no tab. It reads four keys of the User Interface table that the Agent Monitoring tab writes, through `settings.read-kernel:agent-monitoring`. Values and defaults are those of the baseline.

| Key | Default | Range or values | Baseline source |
|---|---|---|---|
| `[ui] progress_monitor_enabled` | true | true or false. Off stops observation and rejects new registrations with `disabled` | `src/docs/automation.md#progress-monitor` |
| `[ui] progress_max_lines` | 4 | Footer lines, 1 to 20 | `src/docs/automation.md#progress-monitor` |
| `[ui] completed_progress_hide_after_seconds` | 60 | Steps of 30; 0 means never hide | `src/docs/automation.md#progress-monitor` |
| `[ui] task_progress_frames` | a preset | 2 to 13 printable frames, all one cell wide or all two cells wide | `src/docs/automation.md#progress-monitor` |

All four are written by the Agent Monitoring tab ([config and data migration](/pages/reference-config-and-data-migration/)); this plugin never writes them.

### Manifest

This block is a fragment. It lists the contributions. The whole manifest is in [Complete manifest](#complete-manifest).

The contract version is 0.1. The plugin declares one subcommand, `progress`, with `check`, `set`, `wait`, `status` and `clear` as its arguments. The alias `wait` makes `ilium wait` work as `ilium progress wait`.

```json
{
  "id": "ilium.core.progress",
  "capabilities": {
    "server": ["cli.contribute", "process.run:sh", "process.long-lived", "settings.read-kernel:agent-monitoring"],
    "client": ["ui.chrome", "settings.read-kernel:agent-monitoring"]
  },
  "contributes": { "cli": { "name": "progress", "summary": "Progress monitors", "output": "jsonl", "requires_server": true, "aliases": ["wait"] } }
}
```

### Complete manifest

This is the whole manifest of the plugin at `ilium:plugin@0.1.0`. The Manifest section above shows the contributions in detail; here the required keys, the sides, the `capabilities` per side and the `activation` triggers are all present. Activation follows [startup and lazy activation](/pages/reference-kernel-and-boot/#startup-and-lazy-activation-of-the-30-core-plugins). [COR-050] Every `cli` key in the fragment above appears with the same value in this manifest.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.progress",
  "name": "Progress",
  "version": "0.1.0",
  "publisher": "Ilium",
  "description": "Progress monitors let a long task show how far it is.",
  "license": "MIT",
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0"
  },
  "client": {
    "entry": "client.wasm",
    "world": "ilium:client/base@0.1.0"
  },
  "capabilities": {
    "server": [
      "cli.contribute",
      "process.run:sh",
      "process.long-lived",
      "settings.read-kernel:agent-monitoring"
    ],
    "client": [
      "ui.chrome",
      "settings.read-kernel:agent-monitoring"
    ]
  },
  "activation": [
    "startup"
  ],
  "contributes": {
    "cli": {
      "name": "progress",
      "summary": "Progress monitors",
      "output": "jsonl",
      "requires_server": true,
      "aliases": ["wait"]
    }
  },
  "limits_tier": "first-party"
}
```

### Samples

#### ilium progress set and wait

`ilium progress set --wait` keeps its invocation open. The plugin writes the registration line, remembers the open invocation and finishes it when the monitor settles. When the user presses Ctrl+C the host sends `cli-cancelled`, and the plugin stops the probe. See [the CLI API](/pages/api-cli/), [cli.contribute](/pages/capability-cli-contribute/) and [events](/pages/api-events/).

**Rust**

```rust
use ilium_plugin_sdk::host::cli;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::CliInvokedEvent;

impl Handler for Progress {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            match event {
                Event::CliInvoked(CliInvokedEvent { invocation, name, argv, .. })
                    if name == "progress" =>
                {
                    let (monitor, progress) = self.register(&argv);
                    // Field order and names equal the baseline record.
                    let line = format!(
                        r#"{{"type":"progress_set","request_id":{},"pane_id":{},"monitor_id":{monitor},"progress":{progress}}}"#,
                        self.request_id, self.pane_id,
                    );
                    let _ = cli::write_line(invocation, &line);
                    if argv.iter().any(|a| a == "--wait") {
                        self.waiting.insert(monitor, invocation);
                    } else {
                        cli::finish(invocation, 0);
                    }
                }
                Event::CliCancelled(invocation) => self.stop_waiting(invocation),
                _ => {}
            }
        }
        Vec::new()
    }
}
```

**TypeScript**

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import * as cli from '@ilium/plugin-sdk/host/cli';

export const { handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'cli-invoked' && event.val.name === 'progress') {
        const { monitor, progress } = register(event.val.argv);
        // Field order and names equal the baseline record.
        cli.writeLine(event.val.invocation, JSON.stringify({
          type: 'progress_set', request_id: requestId, pane_id: paneId,
          monitor_id: monitor, progress,
        }));
        if (event.val.argv.includes('--wait')) waiting.set(monitor, event.val.invocation);
        else cli.finish(event.val.invocation, 0);
      } else if (event.tag === 'cli-cancelled') {
        stopWaiting(event.val);
      }
    }
    return [];
  },
});
```

#### Run the probe

A repeating timer fires on the probe interval. The plugin runs the probe command, parses the JSON object it prints and stores the status and percent. A non-zero exit is not a plugin error, it is a probe result. See [process control](/pages/api-process-control/), [process.run](/pages/capability-process-run/) and [events](/pages/api-events/).

**Rust**

```rust
use ilium_plugin_sdk::host::process::{self, ProcessSpec};

async fn run_probe(command: String, project_root: FsPath) -> Result<ProbeReport, PluginError> {
    let spec = ProcessSpec {
        // The probe is one shell command line, at most 16 KiB.
        program: "sh".to_string(),
        arguments: vec!["-c".to_string(), command],
        cwd: Some(project_root), // the session project root, as in the baseline
        stdin: None,
        environment: Vec::new(),
        timeout_millis: 30_000,
    };
    let exit = process::run(spec).await?;
    let report: ProbeReport = serde_json::from_slice(&exit.stdout)
        .map_err(|_| PluginError::invalid_argument("probe printed no JSON object"))?;
    Ok(report)
}

// In on_events: Event::TimerFired(timer) => spawn run_probe for the monitor of that timer.
```

**TypeScript**

```ts
import { run } from '@ilium/plugin-sdk/host/process';
import { PluginError } from '@ilium/plugin-sdk';

export async function runProbe(command: string, projectRoot: string): Promise<ProbeReport> {
  // One shell command line, run in the session project root, at most 30 s.
  const exit = await run({
    program: 'sh', arguments: ['-c', command], cwd: projectRoot,
    stdin: undefined, environment: [], timeoutMillis: 30_000,
  });
  try {
    return JSON.parse(new TextDecoder().decode(exit.stdout)) as ProbeReport;
  } catch {
    throw new PluginError('invalid-argument', 'probe printed no JSON object');
  }
}

// In onEvents: case 'timer-fired': run the probe of the monitor that owns event.val.
```

#### Long-lived watcher

New in the plugin build, not covered by COR-041. A watcher is a child process that stays alive and prints one JSON line per progress step. Each line is a probe-format JSON object (`job_id`, `status`, `percent`, `message`, `details`, `error`), at most 64 KiB long, and at most 10 lines per second are applied; extra lines are coalesced to the latest. An invalid line counts as one probe failure. The plugin parses each `child-output` line and settles the monitor on `child-exited`. See [process.long-lived](/pages/capability-process-long-lived/), [process control](/pages/api-process-control/) and [events](/pages/api-events/).

**Rust**

```rust
use ilium_plugin_sdk::host::process::{self, ProcessSpec, SpawnSpec};
use ilium_plugin_sdk::types::events::OutputStream;

fn start_watcher(&mut self, monitor: u64, program: &str) -> Result<(), PluginError> {
    let child = process::spawn(&SpawnSpec {
        process: ProcessSpec {
            program: program.to_string(), arguments: Vec::new(), cwd: None,
            stdin: None, environment: Vec::new(), timeout_millis: 0,
        },
        stdin_open: false,
        line_mode: true,
    })?;
    self.children.insert(monitor, child);
    Ok(())
}

// In on_events:
//   Event::ChildOutput(out) if out.stream == OutputStream::Stdout => self.apply_line(&out.bytes),
//   Event::ChildExited(exit) => self.settle(exit.child, exit.status),
```

**TypeScript**

```ts
import * as process from '@ilium/plugin-sdk/host/process';
import type { Child } from '@ilium/plugin-sdk/host/process';

const children = new Map<bigint, Child>();

export function startWatcher(monitor: bigint, program: string): void {
  children.set(monitor, process.spawn({
    process: { program, arguments: [], cwd: undefined, stdin: undefined,
      environment: [], timeoutMillis: 0 },
    stdinOpen: false,
    lineMode: true,
  }));
}

// In onEvents:
//   case 'child-output': if (event.val.stream === 'stdout') applyLine(event.val.bytes); break;
//   case 'child-exited': settle(event.val.child, event.val.status); break;
```

#### Progress footer and status bar

The client side renders one `progress-row` per monitor and sets a status segment with the number of running monitors. When none remain it clears the segment. See [chrome intents](/pages/api-intent-chrome/), [status and feedback widgets](/pages/widgets-status-and-feedback/), [ui.chrome](/pages/capability-ui-chrome/) and [data display widgets](/pages/widgets-data-display/).

**Rust**

```rust
use ilium_plugin_sdk::types::intents::SetStatusSegmentSpec;
use ilium_plugin_sdk::types::ui::ProgressState;

fn footer_row(tree: &mut Tree, m: &Monitor) -> NodeHandle {
    tree.progress_row(&m.title)
        .state(ProgressState::Running)
        .percent(m.percent)
        .message(&m.message)
        .key(&format!("monitor-{}", m.id))
}

fn status_segment(running: usize) -> Submission {
    if running == 0 {
        return Submission::new(Intent::ClearStatusSegment("progress".into()));
    }
    Submission::new(Intent::SetStatusSegment(SetStatusSegmentSpec {
        segment: "progress".into(),
        content: RichText::plain(format!("{running} running")),
        tooltip: None,
        tone: None,
    }))
}
```

**TypeScript**

```ts
import { progressRow } from '@ilium/plugin-sdk/ui/build';
import type { Submission } from '@ilium/plugin-sdk/types';

export const footerRow = (m: Monitor) =>
  progressRow(m.title, { key: `monitor-${m.id}`, state: 'running', percent: m.percent, message: m.message });

export function statusSegment(running: number): Submission {
  const intent = running === 0
    ? ({ tag: 'clear-status-segment', val: 'progress' } as const)
    : ({ tag: 'set-status-segment', val: {
        segment: 'progress', content: richText(`${running} running`),
        tooltip: undefined, tone: undefined } } as const);
  return { intent, correlation: 1n, dedupKey: undefined };
}
```

#### Status route on the loopback listener

New in the plugin build, not covered by COR-041. The plugin answers `GET /progress.json` on the loopback HTTP listener. Each `http-called` event is one request, and `respond` answers exactly that request, so a script can read progress without the terminal. The body is `{"monitors":[<progress object>]}`, where each element has the same shape as the `progress` field of a `progress status` record.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::http_api;

impl Handler for StatusRoute {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::HttpCalled(call) = event else { continue };
            if call.method != "GET" || call.path != "/progress.json" { continue; }
            let body = serde_json::to_vec(&self.snapshot()).unwrap_or_default();
            let _ = http_api::respond(call.call, 200, &[("content-type".into(), "application/json".into())], &body);
        }
        Vec::new()
    }
}
```

**TypeScript**

```ts
import { respond } from '@ilium/plugin-sdk/host/http-api';

export function onEvents(events: Event[]): Submission[] {
  for (const event of events) {
    if (event.tag !== 'http-called') continue;
    const { call, method, path } = event.val;
    if (method !== 'GET' || path !== '/progress.json') continue;
    respond(call, 200, [['content-type', 'application/json']], encode(snapshot()));
  }
  return [];
}
```

### Extension points

Other plugins change what `ilium.core.progress` does through the points below. The plugin owns each point (grant `extensions.provide:ilium.core.progress/<point>`). Contributors need `extensions.contribute:ilium.core.progress/<point>`. See [Extension points](/pages/api-extension-points/) for the mechanism and [Extending core plugins](/pages/extending-core-plugins/) for the five ways to work with a core feature.

| Point | Kind | What it does | Payload | Risk |
|---|---|---|---|---|
| `footer-segments` | collect | Add a segment to the progress footer. | entry: id, priority, max_width | standard |
| `probe-kinds` | collect | Add a ready-made probe, such as a CI run or a build. | entry: id, title, command_template, interval_seconds | standard |

**Services** (grant `services.use:<name>`):

- `ilium.core.progress/report`: start, update, finish: report progress of a task the plugin runs

**Topics** (list under `contributes.topics.subscribes`):

- `ilium.core.progress/changed`: { monitor, status, percent }

**Replaceable:** yes. A replacement lists `"replaces": ["ilium.core.progress"]` and provides the whole surface (see [Extending core plugins](/pages/extending-core-plugins/#replace)).

#### Example: Adds a probe template for a CI run

A package adds this entry in its manifest. It needs no code.

```json
{
  "api": "ilium:plugin@0.1.0",
  "capabilities": { "server": ["extensions.contribute:ilium.core.progress/probe-kinds"] },
  "contributes": {
    "extensions": [
      {
        "point": "ilium.core.progress/probe-kinds",
        "id": "ci-run",
        "priority": 50,
        "entry": {
          "title": "CI run",
          "command_template": "/usr/local/bin/ci-probe {run}",
          "interval_seconds": 30
        }
      }
    ]
  }
}
```

### ABI used

- [CLI API](/pages/api-cli/) and [cli.contribute](/pages/capability-cli-contribute/)
- [Events](/pages/api-events/)
- [Process control](/pages/api-process-control/), [process.run](/pages/capability-process-run/) and [process.long-lived](/pages/capability-process-long-lived/)
- [Chrome intents](/pages/api-intent-chrome/) and [ui.chrome](/pages/capability-ui-chrome/)
- [Status and feedback widgets](/pages/widgets-status-and-feedback/) and [data display widgets](/pages/widgets-data-display/)
- [HTTP API route](/pages/capability-http-api-route/), [HTTP API](/pages/api-http-api/)

### See also

- [Core plugins](/pages/core-plugins/)
- [Chatroom](/pages/core-plugin-chatroom/), which agents use beside progress monitors
