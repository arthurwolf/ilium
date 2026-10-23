---
layout: page
title: Agent detection
permalink: /pages/core-plugin-agent-detect/
---

## Agent detection

The Agent detection plugin has the id `ilium.core.agent-detect` and runs on both sides. The server side owns agent identity and activity: it identifies agent CLIs from the process tree, classifies what each agent is doing, tracks `/goal` state and puts icons, spinners and badges in the tree. The client side draws the Agents panel and follows the display mode.

Walking the process tree and scraping the vt100 screen are kernel-native. The kernel runs the detection loop and hands the plugin process lists and screen snapshots through the `detector` export. The plugin is the pure classification step, which is why a new agent CLI is one signature entry and not a new branch of code.

### What it covers

- Agent identity from the process tree and the list of supported agent CLIs (see Agent monitoring, "How detection works" and "Supported agent CLIs").
- Activity classification and status indicators (see Agent monitoring, "Status indicators and their defaults").
- Pane status signals and the three icon slots in the tree (see Agent monitoring, "The three icon slots in the tree").
- Goal tracking and the `/goal` badge (see Agent monitoring, "The /goal badge").
- The Normal and Attention display modes, and custom signatures (see Agent monitoring, "Normal mode and Attention mode" and "The Agent Monitoring settings tab").
- Agent icons and identifiers in the tree.

### See it

<figure><img src="/assets/demos/01-agent-activity.gif" alt="Tree rows whose agent icons change between working, idle and waiting" loading="lazy"><figcaption>Agent icons follow the activity of each pane: working, idle, awaiting approval, done.</figcaption></figure>

<figure><img src="/assets/demos/17-goal-indicators.gif" alt="A goal badge appears on a tree row while an agent pursues a goal" loading="lazy"><figcaption>The goal badge shows while an agent pursues a /goal and clears when the goal ends.</figcaption></figure>

### What the user sees

<figure>
<pre class="term"><span class="t-dim">╭ Tree ───────────────────────────────────────╮</span>
<span class="t-dim">│</span> <span class="t-ok">⠹</span> claude  refactor parser      <span class="t-hi">◎ goal 12m</span>   <span class="t-dim">│</span>
<span class="t-dim">│</span> <span class="t-dim">●</span> codex   write tests                       <span class="t-dim">│</span>
<span class="t-dim">│</span> <span class="t-warn">?</span> gemini  needs approval                   <span class="t-dim"> │</span>
<span class="t-dim">│</span> <span class="t-ok">✓</span> claude  docs                  <span class="t-dim">done</span>       <span class="t-dim"> │</span>
<span class="t-dim">│</span> <span class="t-dim">$</span> shell                                     <span class="t-dim">│</span>
<span class="t-dim">╰─────────────────────────────────────────────╯</span></pre>
<figcaption>Rendered at 48 columns. Illustrative: the icons depend on the icon set. The spinner on the first row is one animation that the host steps itself.</figcaption>
</figure>

<figure>
<pre class="term"><span class="t-dim">╭ Agents ─────────────────────────────────────╮</span>
<span class="t-dim">│</span> <span class="t-hi">Agent</span>    <span class="t-hi">State</span>      <span class="t-hi">Goal</span>                    <span class="t-dim">│</span>
<span class="t-dim">│</span> claude   <span class="t-ok">⠹ working</span>  ship the parser port    <span class="t-dim">│</span>
<span class="t-dim">│</span> codex    <span class="t-dim">idle</span>       …………………………………………  <span class="t-dim">      │</span>
<span class="t-dim">│</span> gemini   <span class="t-warn">approval</span>   …………………………………………  <span class="t-dim">      │</span>
<span class="t-dim">╰─────────────────────────────────────────────╯</span></pre>
<figcaption>Rendered at 48 columns. Illustrative. One row per agent with a badge, a spinner while working and the goal text.</figcaption>
</figure>

### Manifest

This block is a fragment. It lists the contributions. The whole manifest is in [Complete manifest](#complete-manifest).

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.agent-detect",
  "server": { "entry": "server.wasm", "world": "ilium:server/base@0.1.0",
              "exports": ["detector"] },
  "client": { "entry": "client.wasm", "world": "ilium:client/base@0.1.0" },
  "contributes": {
    "signatures": [
      { "id": "example-agent", "process_names": ["example-agent"],
        "aliases": ["example"], "icon": "robot" }
    ],
    "panels": [
      {
        "id": "panel",
        "title": "Agent detection",
        "modal": false
      }
    ],
    "decoration_slots": [
      { "id": "activity", "placement": "icon", "priority": 90 },
      { "id": "goal", "placement": "badge", "priority": 80 }
    ],
    "display_modes": [
      { "id": "attention", "title": "Attention", "icon": "bell", "slot": "activity",
        "hide_unmarked": true }
    ]
  },
  "capabilities": {
    "server": ["agent.read", "agent.signatures", "tree.decorate", "panes.read", "terminal.write"],
    "client": ["agent.read", "ui.panels"]
  }
}
```

### Complete manifest

This is the whole manifest of the plugin at `ilium:plugin@0.1.0`. The Manifest section above shows the contributions in detail; here the required keys, the sides, the `capabilities` per side and the `activation` triggers are all present. Activation follows [startup and lazy activation](/pages/reference-kernel-and-boot/#startup-and-lazy-activation-of-the-30-core-plugins). [COR-050]

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.agent-detect",
  "name": "Agent detect",
  "version": "0.1.0",
  "publisher": "Ilium",
  "description": "The Agent detection plugin has the id ilium.core.agent-detect and runs on both sides.",
  "license": "MIT",
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0",
    "exports": [
      "detector"
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
      "agent.read",
      "agent.signatures",
      "tree.decorate",
      "panes.read",
      "terminal.write",
      "settings.write-kernel:agent-monitoring",
      "extensions.provide:ilium.core.agent-detect/activity-rules",
      "extensions.provide:ilium.core.agent-detect/classify"
    ],
    "client": [
      "agent.read",
      "ui.panels"
    ]
  },
  "activation": [
    "startup",
    "view:agent-detect.panel"
  ],
  "contributes": {
    "extension_points": [
      {
        "id": "activity-rules",
        "kind": "collect",
        "title": "Activity rules",
        "entry_schema": {
          "type": "object",
          "required": [
            "id",
            "signature",
            "state",
            "regex",
            "region"
          ],
          "properties": {
            "id": {
              "type": "string"
            },
            "signature": {
              "type": "string"
            },
            "state": {
              "type": "string"
            },
            "regex": {
              "type": "string"
            },
            "region": {
              "type": "string",
              "enum": [
                "last-lines",
                "status-row",
                "whole-screen"
              ]
            }
          }
        },
        "risk": "standard",
        "max_entries": 64,
        "dynamic": false,
        "version": "0.1.0"
      },
      {
        "id": "classify",
        "kind": "hook",
        "mode": "transform",
        "title": "Classify",
        "payload_schema": {
          "type": "object",
          "required": [
            "pane",
            "signature",
            "state",
            "reason"
          ],
          "properties": {
            "pane": {
              "type": "string"
            },
            "signature": {
              "type": "string"
            },
            "state": {
              "type": "string"
            },
            "reason": {
              "type": "string"
            }
          }
        },
        "reply_schema": {
          "type": "object",
          "required": [
            "state"
          ],
          "properties": {
            "state": {
              "type": "string"
            }
          }
        },
        "risk": "high",
        "deadline_ms": 20,
        "max_handlers": 8,
        "version": "0.1.0"
      }
    ],
    "signatures": [
      {
        "id": "example-agent",
        "process_names": [
          "example-agent"
        ],
        "aliases": [
          "example"
        ],
        "icon": "robot"
      }
    ],
    "panels": [
      {
        "id": "panel",
        "title": "Agent detection",
        "modal": false
      }
    ],
    "decoration_slots": [
      {
        "id": "activity",
        "placement": "icon",
        "priority": 90
      },
      {
        "id": "goal",
        "placement": "badge",
        "priority": 80
      }
    ],
    "display_modes": [
      {
        "id": "attention",
        "title": "Attention",
        "icon": "bell",
        "slot": "activity",
        "hide_unmarked": true
      }
    ]
  },
  "limits_tier": "first-party"
}
```

### Interstitial prompts, signatures, states and status line

**Auto-answering interstitial prompts.** Ilium answers known one-time dialogs for the user. Today this is Claude Code's "resume full session" prompt, answered with one keystroke per agent process. The behaviour is on by default and is the key `auto_answer_interstitial_prompts` in the `[detection]` table. The plugin owns the rule and the toggle on the Agent Monitoring tab. The keystroke goes through `terminal.write`, once per process, and the plugin records the process so it never answers twice. [COR-011]

**Custom signatures.** A user adds an agent CLI in `config.toml`, and the plugin loads the entries into the signature registry.

```toml
[[detection.custom_signatures]]
process_name = "myagent"
agent_class  = "other"
```

A custom signature has the class `other`. It gets an identity and an icon, but no launch, resume or transcript contract, because Ilium has no verified contract for it. A plugin package can add a signature with [agent.signatures](/pages/capability-agent-signatures/) instead.

**Enums the plugin reports.** Do not invent values; use these.

| Enum | Values |
|---|---|
| Agent class | `claude`, `codex`, `antigravity`, `other(name)`. `antimatter` is accepted as a process-name alias of `antigravity` |
| Activity | `none`, `working`, `idle`, `awaiting-approval`, `done`, `menu` (the `agent-activity` enum of [WIT](/pages/reference-wit/)) |
| Turn | `working`, `waiting-approval`, `waiting-subagents`, `settling`, `idle` |
| Goal state | `active`, `paused`, `blocked`, `usage-limited`, `reached` |
| Pane kind | `plain-shell`, `agent`, `editor`, `board`, `agent-unavailable` |

**Status line capture.** Claude Code and Antigravity can report a status line (model, context use). The kernel keeps the hidden helper commands that capture it, because they run inside the agent process and need no plugin. The plugin reads the captured values through `agent.read`. See [feature traceability](/pages/reference-feature-traceability/) (F-111).

**Polling.** `working_poll_seconds` (default 10) and `idle_poll_seconds` (default 45) in `[detection]` set the adaptive loop. The loop itself stays in the kernel.

### Signature maintenance: versioning, fixtures and the policy when an agent CLI changes its UI

Agent CLIs change their screens without notice, so the signature set is maintained like code, not like data.

**Versioning.** The built-in signatures are part of this core plugin, and core plugins are versioned with the server build ([COR-230]). A server build carries exactly one signature set. At boot the server refuses an installed override of this plugin whose declared host ABI is incompatible, logs `core.abi-mismatch` and runs the bundled copy, so a stale override can never replace the signatures a build was tested with. `ilium plugin list` shows the plugin version, which equals the build's signature-set version.

**Fixtures.** Every built-in signature is backed by captured screen fixtures: real vt100 screen text of the agent mid-turn, idle, awaiting approval and showing a menu, plus a plain shell prompt. The parity suite classifies each fixture and compares the result with the expected activity. No test runs a real `claude` or `codex` binary. A fixture that starts failing after an agent CLI update is a bug report against the signature. The fixture's expected value is never loosened to make the test pass.

**Policy when an agent CLI changes its UI.**

1. Capture the new screen text as a fixture next to the old one.
2. Update the signature so that both the old and the new screens classify correctly, unless the old version is retired (below).
3. Ship the change in the next server build. The change is listed under Changed or Fixed in the release notes with the agent CLI name and the version range it affects.

**Added and retired agent CLIs.** A release announces an added agent CLI under Added in the release notes. A retired signature is announced under Removed one release before it goes, and stays in the set for at least that one release (proposed, owner to confirm). A pane that matches a retired signature keeps its identity for that release and shows the note `Agent CLI retired in the next release` in the Agents panel. After removal the pane is shown as a plain shell, unless a plugin or a custom signature matches it.

**Precedence when the same process name matches more than one source.** The kernel uses the first match, in this order:

1. A `[[detection.custom_signatures]]` entry in `config.toml`. It is the user's explicit choice and wins over everything.
2. A built-in signature of this plugin.
3. A signature from another plugin's [agent.signatures](/pages/capability-agent-signatures/) contribution. When two plugins match, detectors run T1 first, then by plugin id, and the first signature id wins ([HST-452]).

A custom entry that shadows a built-in signature has class `other`, so the user loses the launch, resume and transcript contract of the built-in entry for that process name. `ilium plugin check` and the Agent Monitoring tab show a warning `signature shadowed` naming both sources. A plugin signature never shadows a built-in one. A plugin `resume` command is accepted only under the rules of [SEC-042], and plugin regular expressions use the linear-time engine of [SEC-046], so a conflicting or hostile signature can only lose the precedence contest or be refused, never stall detection.

**Live application.** Signature changes apply live without a restart ([COR-024], see [Kernel and boot](/pages/reference-kernel-and-boot/)). After a change to `config.toml` or to a plugin's signatures, the loop reclassifies all panes on the next tick. During the reclassification window, which is at most one detection interval plus the 2 second reclassification bound of [COR-219], a pane keeps its previous icon and activity. Then the icon changes to the new result. The user sees no blank state and no flicker through `none`. A signature that fails to parse is skipped, the previous set stays active and diagnostics shows the line (see [Diagnostics](/pages/core-plugin-diagnostics/)).

### Auto-answered prompts: audit line, opt-out and limits

The auto-answer described under [Interstitial prompts](#interstitial-prompts-signatures-states-and-status-line) types into the user's agent session without a user action, so it is visible, bounded and switchable.

**Switch.** `auto_answer_interstitial_prompts` in the `[detection]` table is a boolean, default `true`. The Agent Monitoring tab shows the same switch and saves it at once. The change is live ([COR-024]). When it is `false`, no keystroke is sent and the prompt stays on the screen for the user.

**Prompts answered.** The list is closed:

| Agent | Prompt matched | Answer sent |
|---|---|---|
| Claude Code | The "resume full session" dialog | One keystroke that selects the full-session option, followed by Enter |

No other prompt is answered. Approval questions (`awaiting-approval`) are never answered by this feature. A new entry in the list needs a signature change under the maintenance policy above.

**Audit line.** Each auto-answer writes one line to the plugin log of `ilium.core.agent-detect`, on the model of [PAN-032], with these fields: the node id of the pane, the signature id, the prompt id, the answer text and the time (`YYYY-MM-DD HH:mm:ss`). Read it with `ilium plugin logs ilium.core.agent-detect`.

**Limits.** The plugin answers once per agent process. It also answers at most 3 times per pane per minute (proposed, owner to confirm). A fourth match in the minute is not answered and logs `auto-answer.rate-limited`.

**Changed prompt text.** The answer is sent only when the matched text equals the known prompt text exactly. When the text has changed, nothing is sent, the line `auto-answer.prompt-changed` is written with the node id, signature id and the first 80 characters of the unknown dialog's title row, and diagnostics shows a warning. Because detector calls run on the plugin executor ([HST-450]), a slow or failed call also sends nothing.

### Screen text handed to detectors: privacy and retention rules

A `detector` export receives the screen of every unidentified or identified pane: the `rows` list of the `detector-screen` record, plus `title` and `cursor-row` ([agent.signatures](/pages/capability-agent-signatures/#the-detector-export)). A screen can show a token or a typed password, as [User privacy](/pages/user-privacy/) says for search and recap, and detectors see it more often than those features do.

**Rules for the plugin.**

- A detector MUST NOT write screen text to storage, logs or settings, and MUST NOT send it through any network, message or topic capability. `classify` is pure and has no file or network access, and the sandbox enforces this for a detector.
- A detector MUST NOT put screen text in the `objective` or `goal` strings beyond the text of the agent's own objective line.
- The text is valid only for the one call. The host does not keep a copy for the plugin between calls.

**What the host passes.** Only the rows needed for classification: the visible rows of the pane (the `columns` by `rows` grid), never scrollback, and only for panes with a changed screen `revision`. Rows of a pane the plugin's grant does not cover are not sent.

**Tier and grant.** Receiving screen text needs the `agent.signatures` grant (class normal, tier S, server side) and a `detector` entry in `server.exports`. A package that lists `detector` without the grant fails to load with an install error.

**What the user sees.** `ilium plugin permissions <id>` lists `agent.signatures` for the plugin, and the review dialog at install and update states `Reads the screen text of your panes`. The user can refuse the grant. Without it the package installs without the detector, or the install is cancelled, and the pane falls back to the built-in signatures. Revoking the grant takes effect live and the next tick calls no detector of that plugin. For this core plugin, the grant list equals its manifest and cannot be extended ([COR-221]).

### Kernel tabs this plugin owns

This plugin is the single owner of the following built-in Settings tabs. It holds `settings.write-kernel:<tab>` for each one (T1 only, one holder per tab, see [kernel settings](/pages/api-kernel-settings-and-keymap/#writing-a-kernel-tab)). The values stay in the same `config.toml` tables as before, so no user setting moves. [COR-030] [COR-033]

| Tab id | Label | Config table | Readers |
|---|---|---|---|
| `agent-monitoring` | Agent Monitoring | `[ui]`, `[detection]` | Progress, Agent toolbar, Notifications |

If the plugin is disabled, the tab stays in the rail with its stored values shown read-only and the notice "Agent detection is disabled. Values are kept." The kernel keeps reading the keys it consumes itself (listed in the Consumer column of the key table in [config and data migration](/pages/reference-config-and-data-migration/)). Behaviour that only this plugin provides is off. The exact result for this plugin is its row in [kernel and boot](/pages/reference-kernel-and-boot/). [COR-022]

### Samples

#### Identify an agent CLI

A custom signature recognises a new agent CLI. The plugin declares it under [agent.signatures](/pages/capability-agent-signatures/) and implements the `detector` export from the [agent primitive](/pages/primitives-agent/). When the kernel accepts a match it emits `agent-identified` ([events](/pages/api-events/)), and the plugin logs the signature id.

**Rust**

```rust
use ilium_plugin_sdk::exports::{Classification, Detector, DetectorProcess, DetectorScreen};
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::AgentIdentifiedEvent;
use ilium_plugin_sdk::types::tree::AgentActivity;

#[ilium_plugin(side = "server", exports = ["detector"])]
#[derive(Default)]
struct AgentDetect;

impl Lifecycle for AgentDetect {}

impl Handler for AgentDetect {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            if let Event::AgentIdentified(AgentIdentifiedEvent { node, signature }) = event {
                log::log(Level::Info, &format!("node {node} runs {signature}"));
            }
        }
        Vec::new()
    }
}

impl Detector for AgentDetect {
    fn identify(&mut self, processes: Vec<DetectorProcess>) -> Option<String> {
        let found = processes.iter().any(|process| process.name == "example-agent");
        found.then(|| "example-agent".to_string())
    }

    fn classify(&mut self, _signature: String, screen: DetectorScreen) -> Classification {
        let shows = |text: &str| screen.rows.iter().any(|row| row.contains(text));
        let activity = if shows("Allow this command?") {
            AgentActivity::AwaitingApproval
        } else if shows("esc to interrupt") {
            AgentActivity::Working
        } else {
            AgentActivity::Idle
        };
        Classification { activity, objective: None, goal: None }
    }
}
```

**TypeScript**

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { log } from '@ilium/plugin-sdk/host/log';

export const { lifecycle, handler, detector } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'agent-identified') {
        log('info', `node ${event.val.node} runs ${event.val.signature}`);
      }
    }
    return [];
  },
  identify(processes) {
    return processes.some((process) => process.name === 'example-agent') ? 'example-agent' : undefined;
  },
  classify(_signature, screen) {
    const shows = (text: string) => screen.rows.some((row) => row.includes(text));
    const activity = shows('Allow this command?')
      ? 'awaiting-approval'
      : shows('esc to interrupt')
        ? 'working'
        : 'idle';
    return { activity, objective: undefined, goal: undefined };
  },
});
```

#### Activity icons in the tree

Working agents get an animated icon and idle ones a still icon. The plugin reads agents under [agent.read](/pages/capability-agent-read/) and decorates rows under [tree.decorate](/pages/capability-tree-decorate/). On `agent-activity-changed` it submits `set-decoration`; the host steps the `frames` itself (see [tree and window chrome](/pages/api-tree-chrome/)). The event shape is on [events](/pages/api-events/), the agent types on the [agent primitive](/pages/primitives-agent/).

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::AgentActivityEvent;
use ilium_plugin_sdk::types::intents::{Intent, SetDecorationSpec, DecorationVisibility};
use ilium_plugin_sdk::types::tree::AgentActivity;

fn activity_decoration(node: NodeId, current: AgentActivity) -> Submission {
    let (frames, glyph) = match current {
        AgentActivity::Working => (
            ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]
                .iter().map(|frame| StyledText::plain(frame)).collect(),
            "⠋",
        ),
        AgentActivity::AwaitingApproval => (Vec::new(), "?"),
        _ => (Vec::new(), "●"),
    };
    Submission::keyed(
        Intent::SetDecoration(SetDecorationSpec {
            frames,
            frame_ms: 100,
            icon: Some(glyph.into()),
            ..SetDecorationSpec::text(node, "activity", "")
        }),
        format!("activity-{node}"),
    )
}

fn on_events(batch: Vec<Event>) -> Vec<Submission> {
    batch
        .into_iter()
        .filter_map(|event| match event {
            Event::AgentActivityChanged(AgentActivityEvent { node, current, .. }) => {
                Some(activity_decoration(node, current))
            }
            _ => None,
        })
        .collect()
}
```

**TypeScript**

```ts
import type { Event, Submission } from '@ilium/plugin-sdk';

const SPINNER = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

function activityDecoration(node: bigint, current: string): Submission {
  const working = current === 'working';
  return {
    intent: {
      tag: 'set-decoration',
      val: {
        ...baseDecoration(node, 'activity', ''),
        icon: working ? SPINNER[0] : current === 'awaiting-approval' ? '?' : '●',
        frames: working ? SPINNER.map((frame) => ([{ text: frame }])) : [],
        frameMs: 100,
      },
    },
    correlation: 1n,
    dedupKey: `activity-${node}`,
  };
}

export function onEvents(batch: Event[]): Submission[] {
  const out: Submission[] = [];
  for (const event of batch) {
    if (event.tag === 'agent-activity-changed') {
      out.push(activityDecoration(event.val.node, event.val.current));
    }
  }
  return out;
}
```

#### Status signals and goals

The goal badge and the signal slots follow the agent. On `agent-goal-changed` the plugin shows the goal in the `goal` slot. When the goal ends it submits `clear-decoration`, as described on [tree and window chrome](/pages/api-tree-chrome/). The plugin mirrors `signal-changed` into a second slot. Event fields are in [events](/pages/api-events/) and the agent view is on the [agent primitive](/pages/primitives-agent/).

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::{AgentGoalEvent, SignalChangedEvent};
use ilium_plugin_sdk::types::intents::{DecorationKey, Intent, SetDecorationSpec};

fn goal_submission(event: AgentGoalEvent) -> Submission {
    let AgentGoalEvent { node, goal } = event;
    match goal {
        Some(text) => Submission::keyed(
            Intent::SetDecoration(SetDecorationSpec::text(node, "goal", format!("goal {text}"))),
            format!("goal-{node}"),
        ),
        None => Submission::keyed(
            Intent::ClearDecoration(DecorationKey { node, slot: "goal".into() }),
            format!("goal-clear-{node}"),
        ),
    }
}

fn signal_submission(event: SignalChangedEvent) -> Submission {
    let text = format!("{}: {}", event.signal, event.value);
    Submission::new(Intent::SetDecoration(SetDecorationSpec::text(event.pane, "activity", text)))
}
```

**TypeScript**

```ts
import type { Event, Submission } from '@ilium/plugin-sdk';

export function goalSubmission(event: Extract<Event, { tag: 'agent-goal-changed' }>): Submission {
  const { node, goal } = event.val;
  if (goal === undefined) {
    return {
      intent: { tag: 'clear-decoration', val: { node, slot: 'goal' } },
      correlation: 1n,
      dedupKey: `goal-clear-${node}`,
    };
  }
  return {
    intent: { tag: 'set-decoration', val: baseDecoration(node, 'goal', `goal ${goal}`) },
    correlation: 1n,
    dedupKey: `goal-${node}`,
  };
}

export function signalSubmission(event: Extract<Event, { tag: 'signal-changed' }>): Submission {
  const { pane, signal, value } = event.val;
  return {
    intent: { tag: 'set-decoration', val: baseDecoration(pane, 'activity', `${signal}: ${value}`) },
    correlation: 1n,
    dedupKey: undefined,
  };
}
```

#### Agents panel

A panel lists agents with a status badge and a spinner. The client side draws a `table` ([collections](/pages/widgets-collections/)), a `badge` per state and a `spinner` for working agents ([status and feedback](/pages/widgets-status-and-feedback/)), as a panel under [ui.panels](/pages/capability-ui-panels/). The server side feeds it over the `message` channel.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::layout::Area;
use ilium_plugin_sdk::types::ui::{TableColumn, TableRow, UiTree, ViewTarget};
use ilium_plugin_sdk::types::text::Tone;
use ilium_plugin_sdk::ui::build::Tree;

impl Views for AgentsPanel {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        let mut tree = Tree::new();
        let columns = vec![
            TableColumn::new("agent", "Agent").priority(9),
            TableColumn::new("state", "State").priority(5),
            TableColumn::new("goal", "Goal").priority(1),
        ];
        let rows = self.agents.iter().map(|agent| {
            let state = match agent.activity.as_str() {
                "working" => "⠹ working",
                "awaiting-approval" => "approval",
                other => other,
            };
            TableRow::new(vec![agent.name.clone(), state.into(), agent.goal.clone()]).id(&agent.id)
        }).collect();
        let table = tree.table(columns, rows).key("agents");
        let waiting = self.agents.iter().filter(|agent| agent.activity == "awaiting-approval").count();
        let badge = tree.badge().count(waiting as u32).tone(Tone::Warning).key("waiting");
        let busy = tree.spinner().label("Watching agents");
        let root = tree.layout(Direction::Vertical, vec![Constraint::Length(1), Constraint::Fill(1), Constraint::Length(1)])
            .child(badge).child(table).child(busy);
        tree.finish(root)
    }
}

// In the server side: Intent::Message with channel "agents" and the rows as JSON bytes.
```

**TypeScript**

```ts
import { badge, fill, layout, length, spinner, table, tree } from '@ilium/plugin-sdk/ui/build';

const columns = [
  { id: 'agent', title: 'Agent', width: { tag: 'length', val: 10 }, priority: 9 },
  { id: 'state', title: 'State', width: { tag: 'length', val: 10 }, priority: 5 },
  { id: 'goal', title: 'Goal', width: { tag: 'fill', val: 1 }, priority: 1 },
];

export function render() {
  const rows = state.agents.map((agent) => ({
    id: agent.id,
    cells: [
      agent.name,
      agent.activity === 'working' ? '⠹ working' : agent.activity,
      agent.goal,
    ],
  }));
  const waiting = state.agents.filter((agent) => agent.activity === 'awaiting-approval').length;
  return tree(layout(
    { direction: 'vertical', constraints: [length(1), fill(1), length(1)] },
    [
      badge({ count: waiting, tone: 'warning', key: 'waiting' }),
      table(columns, rows, { key: 'agents' }),
      spinner({ label: 'Watching agents' }),
    ],
  ));
}

// The server side sends the rows with an intent of tag 'message' on channel 'agents'.
```

#### Normal and Attention modes

A display mode changes what the tree emphasises. The plugin declares `display_modes` in the manifest and listens for `display-mode-changed` ([events and intents](/pages/events-and-intents/), [events](/pages/api-events/)). In Attention mode the panel lists only agents that need the user or have just finished: those awaiting approval and those whose activity is `done`.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;

#[derive(Default, PartialEq)]
enum Mode {
    #[default]
    Normal,
    Attention,
}

impl Handler for AgentsPanel {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            if let Event::DisplayModeChanged(mode) = event {
                self.mode = match mode.as_deref() {
                    Some("attention") => Mode::Attention,
                    _ => Mode::Normal,
                };
            }
        }
        Vec::new()
    }
}

impl AgentsPanel {
    fn visible_agents(&self) -> impl Iterator<Item = &AgentRow> {
        self.agents.iter().filter(|agent| {
            self.mode == Mode::Normal || agent.activity == "awaiting-approval" || agent.activity == "done"
        })
    }
}
```

**TypeScript**

```ts
import type { Event, Submission } from '@ilium/plugin-sdk';

let mode: 'normal' | 'attention' = 'normal';

export function onEvents(batch: Event[]): Submission[] {
  for (const event of batch) {
    if (event.tag === 'display-mode-changed') {
      mode = event.val.mode === 'attention' ? 'attention' : 'normal';
    }
  }
  return [];
}

export const visibleAgents = (agents: AgentRow[]) =>
  agents.filter(
    (agent) => mode === 'normal' || agent.activity === 'awaiting-approval' || agent.activity === 'done',
  );
```

#### 6. Signals from the agent

A signal is a named value the agent reports, such as the branch it works on. `signal-changed` arrives coalesced per pane and signal, so the plugin stores only the latest value and updates the row for that pane.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;

impl Handler for Signals {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            if let Event::SignalChanged(change) = event {
                self.signals.entry(change.pane).or_default().insert(change.signal, change.value);
            }
        }
        Vec::new()
    }
}
```

**TypeScript**

```ts
export function onEvents(events: Event[]): Submission[] {
  for (const event of events) {
    if (event.tag !== 'signal-changed') continue;
    const bySignal = signals.get(event.val.pane) ?? new Map<string, string>();
    bySignal.set(event.val.signal, event.val.value);
    signals.set(event.val.pane, bySignal);
  }
  return [];
}
```

### Detection loop and budget

The poll loop is native and runs in the server. This plugin's detector runs on the plugin executor, one call per pane, in parallel with other plugins' detectors. One poll cycle spends at most 50 ms on detector calls. A slow call keeps the previous activity and counts as a breach. A third breach in a row traps the call; the side stops only after a third counted failure within the failure window (LIF-030), with no restart. The full rules are in the [detection loop](/pages/capability-agent-signatures/#detection-loop) section of the signatures page and in [Host scheduling](/pages/reference-host-scheduling/).

- [COR-219] When `ilium.core.agent-detect` is off, every pane MUST report agent class `none` and activity `unknown` within one detection interval, and on re-enable all panes MUST be reclassified within 2 seconds (proposed, owner to confirm). Test: switch the plugin off with two panes open and read their classes, switch it on, and time the reclassification.

### Extension points

Other plugins change what `ilium.core.agent-detect` does through the points below. The plugin owns each point (grant `extensions.provide:ilium.core.agent-detect/<point>`). Contributors need `extensions.contribute:ilium.core.agent-detect/<point>`. See [Extension points](/pages/api-extension-points/) for the mechanism and [Extending core plugins](/pages/extending-core-plugins/) for the five ways to work with a core feature.

| Point | Kind | What it does | Payload | Risk |
|---|---|---|---|---|
| `activity-rules` | collect | Add screen patterns that mean working, idle or awaiting approval for one signature. | entry: id, signature, state, regex, region (last-lines, status-row, whole-screen) | standard |
| `classify` | hook (transform) | Adjust the detected activity before it is published. | pane, signature, state, reason; reply: state | high |

**Services** (grant `services.use:<name>`):

- `ilium.core.agent-detect/state`: get, list: current identity and activity of panes

**Topics** (list under `contributes.topics.subscribes`):

- `ilium.core.agent-detect/state-changed`: { pane, signature, previous, current }

`contributes.signatures` entries register agent CLIs. This plugin turns them into identity.

**Replaceable:** yes. A replacement lists `"replaces": ["ilium.core.agent-detect"]` and provides the whole surface (see [Extending core plugins](/pages/extending-core-plugins/#replacing-a-core-plugin)).

#### Example: Marks a pane as awaiting approval when the last lines contain the question

A package adds this entry in its manifest. It needs no code.

```json
{
  "api": "ilium:plugin@0.1.0",
  "capabilities": { "server": ["extensions.contribute:ilium.core.agent-detect/activity-rules"] },
  "contributes": {
    "extensions": [
      {
        "point": "ilium.core.agent-detect/activity-rules",
        "id": "build-waiting",
        "priority": 50,
        "entry": {
          "id": "build-waiting",
          "signature": "claude",
          "state": "awaiting-approval",
          "regex": "Do you want to proceed\\?",
          "region": "last-lines"
        }
      }
    ]
  }
}
```

### Extension points this plugin owns

| Point | Side | Version | Deadline | Max | Dynamic |
|---|---|---|---|---|---|
| `ilium.core.agent-detect/activity-rules` | server | 0.1.0 (default) | none (default) | 64 entries (default) | no (default) |
| `ilium.core.agent-detect/classify` | server | 0.1.0 (default) | 20 ms (default) | 8 handlers (default) | no (default) |

### ABI used

- [Agent primitive](/pages/primitives-agent/) and [agent.signatures](/pages/capability-agent-signatures/)
- [agent.read](/pages/capability-agent-read/) and [tree.decorate](/pages/capability-tree-decorate/)
- [Tree and window chrome](/pages/api-tree-chrome/)
- [Events](/pages/api-events/) and [events and intents](/pages/events-and-intents/)
- [ui.panels](/pages/capability-ui-panels/), [status and feedback](/pages/widgets-status-and-feedback/) and [collections](/pages/widgets-collections/)

### See also

- [Core replacement matrix](/pages/reference-core-replacement-matrix/) for the surface, stored data, CLI names and install codes that a replacement of this plugin must keep.
- [Agent toolbar](/pages/core-plugin-agent-toolbar/), which acts on the agents this plugin identifies.
- [Fleet](/pages/core-plugin-fleet/), which lists the same agents across sessions.
- [Cost](/pages/core-plugin-cost/), which adds spend to the same rows.

### Security

Risk marks of this plugin's extension points follow the corrected table in [Extension security](/pages/reference-extension-security/) ([SEC-293]).
