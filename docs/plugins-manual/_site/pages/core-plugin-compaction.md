# Compaction

## Compaction

The Compaction plugin (id `ilium.core.compaction`, sides: server and client) is the plugin form of Ilium's context compaction. When an agent's context fills up, the plugin builds a summary of the transcript in a background job, routes the agent toolbar's compact button to that job, and lets the agent resume from the summary. On the client it draws the compaction optimiser, which suggests the cheapest auto-compact threshold and lets the user apply or revert it. It also converts a session between Claude and Codex.

An agent's own auto-compact trigger lives inside the agent command-line tool, so the plugin only reads it and never changes how the agent decides. The summary job and the optimiser are ordinary plugin code that use the contract version 0.1.

### What it covers

- **Remote compaction.** The summary is built through the inference model in a job, so the agent keeps working while it runs. The Remote compaction page of the user documentation describes what the user sees.
- **Compact button routing.** The compact button of the agent toolbar starts the same job as the palette command. See Starting it in Remote compaction.
- **Compaction optimiser.** The Optimization tab of Settings recommends a threshold, with Apply and Revert. See the Optimization part of the Settings tab reference.
- **Auto-compact trigger reader.** The plugin reads the trigger each agent uses (see Techniques in Remote compaction), so the recommendation matches the agent.
- **Session conversion.** Convert a session between Claude and Codex, described under Session recovery in the user documentation.

### See it

<figure><img src="/assets/demos/26-convert-session.gif" alt="A Claude session is converted to a Codex session and the converted file is opened" loading="lazy"><figcaption>Converting a session between Claude and Codex. The plugin writes the converted session and opens it for inspection.</figcaption></figure>

### What the user sees

<figure><pre class="term">╭─ Optimization ───────────────────────────────╮
│                                              │
│ Auto-compact threshold …………………………………………… 72 %│
│ Recommended ………………………………………………………………………… <span class="t-ok">65 %</span>│
│ <span class="t-ok">█████████████████████████████</span><span class="t-dim">░░░░░░░░░░░░░░░░</span>│
│                                              │
│   <span class="t-hi">[ Apply ]</span>   [ Revert ]                     │
│                                              │
╰──────────────────────────────────────────────╯</pre><figcaption>Rendered at 48 columns. Illustrative. A <code>meter</code> row shows the recommended threshold (65 per cent) beside the current one (72 per cent). Apply and Revert are <code>button</code> widgets.</figcaption></figure>
<figure><pre class="term">╭─ Compacting claude: refactor ────────────────╮
│                                              │
│ Context before …………………………………………………………………… <span class="t-err">91%</span>│
│ <span class="t-err">━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━</span><span class="t-dim">────</span>│
│ Context after ……………………………………………………………………… <span class="t-ok">24%</span>│
│ <span class="t-ok">━━━━━━━━━━━</span><span class="t-dim">──────────────────────────────────</span>│
│                                              │
│ <span class="t-dim">Summary written, agent resumed.</span>              │
│                                              │
╰──────────────────────────────────────────────╯</pre><figcaption>Rendered at 48 columns. Illustrative. A <code>line-gauge</code> of the pane's context use drops when the compacted session resumes.</figcaption></figure>
### Manifest

This block is a fragment. It lists the contributions. The whole manifest is in [Complete manifest](#complete-manifest).

The plugin ships with Ilium as a first-party (T1) package. The server side runs the job. The client side draws the panel and the optimiser.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.compaction",
  "name": "Compaction",
  "version": "0.1.0",
  "publisher": "Ilium",
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0",
    "exports": ["job-runner"]
  },
  "client": {
    "entry": "client.wasm",
    "world": "ilium:client/base@0.1.0",
    "exports": ["views"]
  },
  "capabilities": {
    "server": ["agent.transcript", "inference", "jobs.run", "panes.lifecycle",
               "fs.read:agent-homes", "fs.write:agent-homes"],
    "client": ["open-external", "ui.panels"]
  },
  "contributes": {
    "jobs": [{ "kind": "compact", "deadline_ms": 300000, "memory_mib": 256 }],
    "commands": [{ "id": "compact", "title": "Compact this agent" }],
    "panels": [{ "id": "optimizer", "title": "Compaction", "modal": false }]
  }
}
```

### Complete manifest

This is the whole manifest of the plugin at `ilium:plugin@0.1.0`. The Manifest section above shows the contributions in detail; here the required keys, the sides, the `capabilities` per side and the `activation` triggers are all present. Activation follows [startup and lazy activation](/pages/reference-kernel-and-boot/#startup-and-lazy-activation-of-the-30-core-plugins). [COR-050]

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.compaction",
  "name": "Compaction",
  "version": "0.1.0",
  "publisher": "Ilium",
  "description": "The Compaction plugin (id ilium.core.compaction, sides: server and client) is the plugin form of Ilium's context compaction.",
  "license": "MIT",
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0",
    "exports": [
      "job-runner"
    ]
  },
  "client": {
    "entry": "client.wasm",
    "world": "ilium:client/base@0.1.0",
    "exports": [
      "views"
    ]
  },
  "capabilities": {
    "server": [
      "agent.transcript",
      "inference",
      "jobs.run",
      "panes.lifecycle",
      "fs.read:agent-homes",
      "fs.write:agent-homes"
    ],
    "client": [
      "open-external",
      "ui.panels",
      "settings.write-kernel:optimization",
      "settings.write-kernel:remote-compaction"
    ]
  },
  "activation": [
    "event:agent-context-low",
    "service:compact",
    "view:compaction.optimizer"
  ],
  "contributes": {
    "jobs": [
      {
        "kind": "compact",
        "deadline_ms": 300000,
        "memory_mib": 256
      }
    ],
    "commands": [
      {
        "id": "compact",
        "title": "Compact this agent"
      }
    ],
    "panels": [
      {
        "id": "optimizer",
        "title": "Compaction",
        "modal": false
      }
    ]
  },
  "limits_tier": "first-party"
}
```

### Kernel tabs this plugin owns

This plugin is the single owner of the following built-in Settings tabs. It holds `settings.write-kernel:<tab>` for each one (T1 only, one holder per tab, see [kernel settings](/pages/api-kernel-settings-and-keymap/#writing-a-kernel-tab)). The values stay in the same `config.toml` tables as before, so no user setting moves. [COR-030] [COR-033]

| Tab id | Label | Config table | Readers |
|---|---|---|---|
| `optimization` | Optimization | none (writes the agent's own config) | none |
| `remote-compaction` | Remote compaction | `[remote_compaction]` | none |

If the plugin is disabled, the tab stays in the rail with its stored values shown read-only and the notice "Compaction is disabled. Values are kept." The kernel keeps reading the keys it consumes itself (listed in the Consumer column of the key table in [config and data migration](/pages/reference-config-and-data-migration/)). Behaviour that only this plugin provides is off. The exact result for this plugin is its row in [kernel and boot](/pages/reference-kernel-and-boot/). [COR-022]

### Samples

#### 1. Remote compaction as a job

The server side starts a `compact` job with the transcript as input and follows the job with `job-progress` events. The runner asks the inference model for a summary one chunk at a time, checks for cancellation between chunks, and returns the summary bytes. This uses [jobs](/pages/api-jobs/), the [jobs.run grant](/pages/capability-jobs-run/), the [background jobs guide](/pages/background-jobs/) and the [event catalogue](/pages/api-events/). The helpers `focused_transcript`, `summarise` and `concatBytes` are the plugin's own.

**Rust**

```rust
use ilium_plugin_sdk::host::jobs;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::{CommandInvokedEvent, JobProgressEvent};
use ilium_plugin_sdk::types::requests::RequestResult;

#[ilium_plugin(side = "server", exports = ["job-runner"])]
#[derive(Default)]
struct Compaction {
    job: Option<RequestId>,
    percent: f32,
}

impl Lifecycle for Compaction {}

impl Handler for Compaction {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            match event {
                Event::CommandInvoked(CommandInvokedEvent { command, .. })
                    if command == "ilium.core.compaction/compact" && self.job.is_none() =>
                {
                    self.job = jobs::start_request("compact", &focused_transcript()).ok();
                }
                Event::JobProgress(JobProgressEvent { request_id, percent })
                    if self.job == Some(request_id) =>
                {
                    self.percent = percent; // the client gauge reads this value
                }
                Event::RequestCompleted(RequestResult { request: RequestRef::Call(job), .. })
                    if self.job == Some(job) =>
                {
                    self.job = None;
                    let _summary = jobs::take_output(job); // resume the agent from it
                }
                _ => {}
            }
        }
        Vec::new()
    }
}

impl JobRunner for Compaction {
    fn run(&mut self, _kind: String, input: Vec<u8>) -> Result<Vec<u8>, PluginError> {
        let chunks: Vec<&[u8]> = input.chunks(32 * 1024).collect();
        let mut summary = Vec::new();
        for (index, chunk) in chunks.iter().enumerate() {
            if jobs::is_cancelled() {
                return Err(PluginError {
                    code: ErrorCode::Cancelled,
                    message: "compaction stopped".into(),
                    retry_after_ms: None,
                });
            }
            summary.extend(summarise(chunk)); // asks the inference model
            jobs::report_progress(100.0 * (index + 1) as f32 / chunks.len() as f32);
        }
        Ok(summary)
    }
}
```

**TypeScript**

```ts
import { definePlugin, PluginError, type Event, type Submission } from '@ilium/plugin-sdk';
import { isCancelled, reportProgress, startRequest, takeOutput } from '@ilium/plugin-sdk/host/jobs';

let job: bigint | undefined;
let percent = 0;

export const { lifecycle, handler, jobRunner } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'command-invoked'
          && event.val.command === 'ilium.core.compaction/compact' && job === undefined) {
        job = startRequest('compact', focusedTranscript());
      } else if (event.tag === 'job-progress' && event.val.requestId === job) {
        percent = event.val.percent; // the client gauge reads this value
      } else if (event.tag === 'request-completed' && job !== undefined
          && event.val.request.tag === 'call' && event.val.request.val === job) {
        const summary = takeOutput(job); // resume the agent from it
        job = undefined;
        void summary;
      }
    }
    return [];
  },
  run(_kind: string, input: Uint8Array): Uint8Array {
    const parts: Uint8Array[] = [];
    const size = 32 * 1024;
    const total = Math.ceil(input.byteLength / size);
    for (let index = 0; index < total; index++) {
      if (isCancelled()) throw new PluginError('cancelled', 'compaction stopped');
      parts.push(summarise(input.subarray(index * size, (index + 1) * size)));
      reportProgress((100 * (index + 1)) / total);
    }
    return concatBytes(parts);
  },
});
```

#### 2. Optimiser recommendation

The client draws the cheapest auto-compact threshold in a `meter` widget and in a `meter` settings row, with `button` widgets for Apply and Revert. The widgets are listed in the [widget catalogue](/pages/widgets-index/) and drawn as described in [data display](/pages/widgets-data-display/). The row type is in [settings rows](/pages/reference-settings-rows/) and the buttons are in [input widgets](/pages/widgets-input/). `applyThreshold` and `revertThreshold` are the plugin's own functions.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::ui::{ActionKind, MeterSegment, UiTree};
use ilium_plugin_sdk::types::text::Tone;
use ilium_plugin_sdk::ui::build::Tree;
use ilium_plugin_sdk::host::settings;

// Manifest row (client scope), placed in the Optimization tab:
// { "type": "meter", "id": "threshold", "title": "Recommended threshold",
//   "max": 100, "show_values": true,
//   "segments": [{ "label": "recommended", "value": 0, "tone": "info" }] }

fn show_recommendation(percent: f64) -> Result<(), PluginError> {
    settings::set_meter("threshold", &[
        MeterSegment { label: "recommended".into(), value: percent, tone: Tone::Info },
    ])
}

impl Views for Optimizer {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        let mut tree = Tree::new();
        let meter = tree
            .meter(vec![MeterSegment {
                label: "recommended".into(), value: self.recommended, tone: Tone::Info,
            }])
            .max(100.0)
            .show_values(true)
            .key("threshold");
        let apply = tree.button("Apply").key("apply");
        let revert = tree.button("Revert").key("revert");
        let row = tree
            .layout(Direction::Vertical, [Constraint::Length(2), Constraint::Length(3), Constraint::Length(3)])
            .child(meter).child(apply).child(revert);
        tree.finish(row)
    }
}

// In on_events: ("apply", ActionKind::Activated, _) => self.apply(),
//               ("revert", ActionKind::Activated, _) => self.revert(),
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import { setMeter } from '@ilium/plugin-sdk/host/settings';
import { button, fill, layout, length, meter, tree } from '@ilium/plugin-sdk/ui/build';

// Manifest row (client scope), placed in the Optimization tab:
// { "type": "meter", "id": "threshold", "title": "Recommended threshold",
//   "max": 100, "show_values": true,
//   "segments": [{ "label": "recommended", "value": 0, "tone": "info" }] }

export function showRecommendation(percent: number): void {
  setMeter('threshold', [{ label: 'recommended', value: percent, tone: 'info' }]);
}

let recommended = 65;

export const { lifecycle, handler, views } = definePlugin({
  side: 'client',
  activate() {},
  onEvents(batch) {
    for (const event of batch) {
      if (event.tag !== 'ui-action' || event.val.kind !== 'activated') continue;
      if (event.val.key === 'apply') applyThreshold(recommended);
      if (event.val.key === 'revert') revertThreshold();
    }
    return [];
  },
  render() {
    const gauge = meter(
      [{ label: 'recommended', value: recommended, tone: 'info' }],
      { key: 'threshold', max: 100, showValues: true },
    );
    return tree(layout(
      { direction: 'vertical', constraints: [length(2), length(3), length(3), fill(1)] },
      [gauge, button('Apply', { key: 'apply' }), button('Revert', { key: 'revert' })],
    ));
  },
});
```

#### 3. Context gauge

The client shows a pane's context use in a `line-gauge` and redraws it when the compacted session resumes. The widget is described in [data display](/pages/widgets-data-display/).

**Rust**

```rust
use ilium_plugin_sdk::host::ui;
use ilium_plugin_sdk::ui::build::Tree;

impl Views for ContextGauge {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        let mut tree = Tree::new();
        let label = format!("{}%", (self.context_ratio * 100.0).round());
        let gauge = tree
            .line_gauge(self.context_ratio)
            .label(vec![Span::plain(&label)])
            .key("context");
        tree.finish(gauge)
    }
}

// When the compacted session resumes, store the new ratio and redraw.
fn on_resumed(&mut self, ratio: f64) {
    self.context_ratio = ratio; // for example 0.91 before, 0.24 after
    ui::invalidate(ViewTarget::Panel("optimizer".into()));
}
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import { invalidate } from '@ilium/plugin-sdk/host/ui';
import { lineGauge, span, tree } from '@ilium/plugin-sdk/ui/build';

let contextRatio = 0.91;

export function onResumed(ratio: number): void {
  contextRatio = ratio; // for example 0.91 before, 0.24 after
  invalidate({ tag: 'panel', val: 'optimizer' });
}

export const { lifecycle, handler, views } = definePlugin({
  side: 'client',
  activate() {},
  onEvents() { return []; },
  render() {
    const label = `${Math.round(contextRatio * 100)}%`;
    return tree(lineGauge(contextRatio, { key: 'context', label: [span(label)] }));
  },
});
```

#### 4. Convert a session

After a Claude session is converted to a Codex one, the client submits `open-external` with the converted session file so the user can inspect it. The grant is [open-external](/pages/capability-open-external/) and the intent is in [chrome intents](/pages/api-intent-chrome/). The first time a plugin opens a target the user is asked once.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::intents::{ExternalTarget, OpenExternalSpec};

// Needs "open-external" in the client capabilities.
fn open_converted(session_file: &str) -> Submission {
    Submission {
        intent: Intent::OpenExternal(OpenExternalSpec {
            target: ExternalTarget::File(FsPath {
                scope: "agent-homes".into(),
                path: session_file.into(),
            }),
        }),
        correlation: 1,
        dedup_key: None,
    }
}
```

**TypeScript**

```ts
import type { Submission } from '@ilium/plugin-sdk/types';

// Needs "open-external" in the client capabilities.
export function openConverted(sessionFile: string): Submission {
  return {
    intent: {
      tag: 'open-external',
      val: { target: { tag: 'file', val: { scope: 'agent-homes', path: sessionFile } } },
    },
    correlation: 1n,
    dedupKey: undefined,
  };
}
```

### Extension points

Other plugins change what `ilium.core.compaction` does through the points below. The plugin owns each point (grant `extensions.provide:ilium.core.compaction/<point>`). Contributors need `extensions.contribute:ilium.core.compaction/<point>`. See [Extension points](/pages/api-extension-points/) for the mechanism and [Extending core plugins](/pages/extending-core-plugins/) for the five ways to work with a core feature.

| Point | Kind | What it does | Payload | Risk |
|---|---|---|---|---|
| `techniques` | collect | Add a compaction technique. | entry: id, title, service (summarises a transcript) | standard |
| `before-compact` | hook (veto) | Refuse a compaction, for example while a task is mid-way. | pane, technique, tokens | standard |

**Services** (grant `services.use:<name>`):

- `ilium.core.compaction/compact`: start: start a compaction of a pane

**Topics** (list under `contributes.topics.subscribes`):

- `ilium.core.compaction/finished`: { pane, tokens_before, tokens_after }

**Replaceable:** yes. A replacement lists `"replaces": ["ilium.core.compaction"]` and provides the whole surface (see [Extending core plugins](/pages/extending-core-plugins/#replace)).

#### Example: Refuses a compaction while a test run is active in the pane

A package adds a `veto` handler. Handlers run until one refuses.

```json
{
  "api": "ilium:plugin@0.1.0",
  "exports": [
    "hook-handler"
  ],
  "capabilities": { "server": ["extensions.contribute:ilium.core.compaction/before-compact"] },
  "contributes": {
    "extensions": [
      {
        "point": "ilium.core.compaction/before-compact",
        "id": "hold-while-testing",
        "priority": 50,
        "handler": "hold_while_testing"
      }
    ]
  }
}
```

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;

impl HookHandler for Plugin {
    fn handle_hook(&mut self, call: HookCall) -> HookReply {
        match call.extension.as_str() {
            "hold_while_testing" => {
                // call.payload_json holds: pane, technique, tokens
                let _payload: serde_json::Value = match serde_json::from_str(&call.payload_json) {
                    Ok(value) => value,
                    Err(_) => return HookReply::Pass,
                };
                HookReply::Refuse("reason".into())
            }
            _ => HookReply::Pass,
        }
    }
}
```

**TypeScript**

```ts
definePlugin({
  handleHook(call) {
    if (call.extension !== 'hold_while_testing') return { tag: 'pass' };
    // call.payloadJson holds: pane, technique, tokens
    const payload = JSON.parse(call.payloadJson);
    void payload;
    return { tag: 'refuse', val: 'reason' };
  },
});
```

A handler that is late, traps or returns a reply the mode does not allow counts as `pass`. See [Extension points](/pages/api-extension-points/#hooks).

### ABI used

- [Jobs](/pages/api-jobs/) and the [jobs.run grant](/pages/capability-jobs-run/)
- [Background jobs](/pages/background-jobs/)
- [Events](/pages/api-events/)
- [Widget catalogue](/pages/widgets-index/) and [data display widgets](/pages/widgets-data-display/)
- [Input widgets](/pages/widgets-input/)
- [Settings rows](/pages/reference-settings-rows/)
- [open-external grant](/pages/capability-open-external/) and [chrome intents](/pages/api-intent-chrome/)

### See also

- [Core plugins](/pages/core-plugins/)
- [Settings plugin](/pages/core-plugin-settings/), which hosts the Optimization tab
- [Agent toolbar plugin](/pages/core-plugin-agent-toolbar/), which owns the compact button
