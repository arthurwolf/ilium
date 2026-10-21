---
layout: page
title: Agent toolbar
permalink: /pages/core-plugin-agent-toolbar/
---

## Agent toolbar

The Agent toolbar plugin has the id `ilium.core.agent-toolbar` and runs on both sides. It owns the per-agent chrome in the pane frame and the tree: the last-prompt banner, the toolbar and its actions, the model and context facts, the reserved terminal space, the recovery hint after an agent stops, the Configure agent action and the agent debug history. The server side reads agent facts and sends actions. The client side draws the slots and the popover.

The frame around a pane and the terminal underneath it are kernel-native. The plugin fills the frame slots that the kernel offers through `ui.chrome` and sends actions through `agent.control`.

### What it covers

- The last-prompt banner and the agent toolbar (see Agent monitoring, "Last prompt banner and agent toolbar").
- Configure agent (see Agent monitoring, "Last prompt banner and agent toolbar").
- Reserved terminal space (see Agent monitoring, "Reserved terminal space").
- Recover work after an agent stops (see Agent monitoring, "Recover work after an agent stops").
- Agent debug history (see Agent monitoring, "Troubleshooting").

### See it

<figure><img src="/assets/demos/01-agent-activity.gif" alt="A pane frame shows the last prompt on top while the agent works and a toolbar on the right" loading="lazy"><figcaption>The banner shows the last prompt while the agent works. The same recording shows the activity icons from the Agent detection plugin.</figcaption></figure>

### What the user sees

<figure>
<pre class="term">╭ claude · opus · 41% context ─────────────────╮
│ > refactor the parser to use a lexer         │
├──────────────────────────────────────────────┤
│                                              │
│  ... terminal content ...                    │
│                                              │
╰──────────────────────────────────────────────╯</pre>
<figcaption>Rendered at 48 columns. Illustrative. The banner is the first row, and the toolbar slot and the model text sit in the header.</figcaption>
</figure>

<figure>
<pre class="term"><span class="t-dim">╭ Debug history ──────────────────────────────╮</span>
<span class="t-dim">│</span> <span class="t-hi">Time</span>      <span class="t-hi">Kind</span>          <span class="t-hi">Detail</span>              <span class="t-dim">│</span>
<span class="t-dim">│</span> 14:02:11  input-sent    /compact            <span class="t-dim">│</span>
<span class="t-dim">│</span> 14:02:12  <span class="t-ok">activity</span>      idle to working     <span class="t-dim">│</span>
<span class="t-dim">│</span> 14:05:40  <span class="t-warn">activity</span>      working to approval <span class="t-dim">│</span>
<span class="t-dim">╰─────────────────────────────────────────────╯</span></pre>
<figcaption>Rendered at 48 columns. Illustrative. The debug-history popover anchored to a tree row.</figcaption>
</figure>

### Manifest

This block is a fragment. It lists the contributions. The whole manifest is in [Complete manifest](#complete-manifest).

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.agent-toolbar",
  "server": { "entry": "server.wasm", "world": "ilium:server/base@0.1.0" },
  "client": { "entry": "client.wasm", "world": "ilium:client/base@0.1.0" },
  "contributes": {
    "commands": [
      { "id": "compact", "title": "Compact agent context" },
      { "id": "history", "title": "Show agent debug history" }
    ],
    "tree_toolbar": [
      { "id": "compact", "icon": "arrows-minimize", "label": "Compact",
        "command": "compact" }
    ],
    "row_actions": [
      { "id": "history", "icon": "history", "label": "Debug history",
        "command": "history", "applies_to": ["agent"], "order": 200 }
    ],
    "pane_frame_slots": [
      { "id": "prompt", "placement": "banner", "applies_to": "agents" },
      { "id": "model", "placement": "header-text", "applies_to": "agents" },
      { "id": "recover", "placement": "footer-row", "applies_to": "agents" }
    ]
  },
  "capabilities": {
    "server": ["agent.read", "agent.read-prompts", "agent.transcript",
               "agent.control", "journal.read"],
    "client": ["agent.read", "agent.read-prompts", "ui.chrome"]
  }
}
```

### Complete manifest

This is the whole manifest of the plugin at `ilium:plugin@0.1.0`. The Manifest section above shows the contributions in detail; here the required keys, the sides, the `capabilities` per side and the `activation` triggers are all present. Activation follows [startup and lazy activation](/pages/reference-kernel-and-boot/#startup-and-lazy-activation-of-the-30-core-plugins). [COR-050]

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.agent-toolbar",
  "name": "Agent toolbar",
  "version": "0.1.0",
  "publisher": "Ilium",
  "description": "The Agent toolbar plugin has the id ilium.core.agent-toolbar and runs on both sides.",
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
      "agent.read",
      "agent.read-prompts",
      "agent.transcript",
      "agent.control",
      "journal.read"
    ],
    "client": [
      "agent.read",
      "agent.read-prompts",
      "ui.chrome",
      "extensions.provide:ilium.core.agent-toolbar/toolbar-actions",
      "extensions.provide:ilium.core.agent-toolbar/banner-segments"
    ]
  },
  "activation": [
    "event:agent-activity-changed",
    "startup"
  ],
  "contributes": {
    "extension_points": [
      {
        "id": "toolbar-actions",
        "kind": "collect",
        "title": "Toolbar actions",
        "entry_schema": {
          "type": "object",
          "required": [
            "id",
            "signature",
            "label",
            "icon",
            "command",
            "requires_action"
          ],
          "properties": {
            "id": {
              "type": "string"
            },
            "signature": {
              "type": "string"
            },
            "label": {
              "type": "string"
            },
            "icon": {
              "type": "string"
            },
            "command": {
              "type": "string"
            },
            "requires_action": {
              "type": "boolean"
            }
          }
        },
        "max_entries": 64,
        "dynamic": false,
        "version": "0.1.0"
      },
      {
        "id": "banner-segments",
        "kind": "collect",
        "title": "Banner segments",
        "entry_schema": {
          "type": "object",
          "required": [
            "id",
            "priority",
            "max_width"
          ],
          "properties": {
            "id": {
              "type": "string"
            },
            "priority": {
              "type": "integer"
            },
            "max_width": {
              "type": "integer"
            }
          }
        },
        "max_entries": 64,
        "dynamic": false,
        "version": "0.1.0"
      }
    ],
    "commands": [
      {
        "id": "compact",
        "title": "Compact agent context"
      },
      {
        "id": "history",
        "title": "Show agent debug history"
      }
    ],
    "tree_toolbar": [
      {
        "id": "compact",
        "icon": "arrows-minimize",
        "label": "Compact",
        "command": "compact"
      }
    ],
    "row_actions": [
      {
        "id": "history",
        "icon": "history",
        "label": "Debug history",
        "command": "history",
        "applies_to": [
          "agent"
        ],
        "order": 200
      }
    ],
    "pane_frame_slots": [
      {
        "id": "prompt",
        "placement": "banner",
        "applies_to": "agents"
      },
      {
        "id": "model",
        "placement": "header-text",
        "applies_to": "agents"
      },
      {
        "id": "recover",
        "placement": "footer-row",
        "applies_to": "agents"
      }
    ]
  },
  "limits_tier": "first-party"
}
```

### Toolbar action table

The toolbar shows the buttons that fit the detected agent. The same list of actions is registered as `toolbar-actions` entries (see Extension points), so a third-party plugin can add a button next to these. [COR-011]

| Action id | Label | What it does | Agents | Route |
|---|---|---|---|---|
| `compact` | Compact | Shrinks the agent context | Claude Code, Codex | Sends `/compact`, or starts remote compaction when the [compaction](/pages/core-plugin-compaction/) plugin is set to remote |
| `clear` | Clear | Clears the conversation | Claude Code, Codex | Sends `/clear` |
| `configure` | Configure | Opens the agent configuration dialog | Claude Code, Codex | Calls the [agent setup](/pages/core-plugin-agent-setup/) service |
| `stop` | Stop | Interrupts the current turn | all agents | Sends the interrupt key |
| `copy-screen` | Copy screen | Copies the visible screen | all agents | Reads the screen |
| `smart-copy` | Smart Copy | Starts Smart Copy on the frozen screen | all agents | Calls the [smart copy](/pages/core-plugin-smart-copy/) service |
| `copy-last-message` | Copy last message | Copies the agent's last answer | Claude Code, Codex | Reads the transcript |
| `effort` | Effort | Chooses the reasoning effort | Claude Code, Codex | Sends the agent's own command |
| `model` | Model | Chooses the model | Claude Code, Codex | Sends the agent's own command |
| `fast-mode` | Fast mode | Switches fast mode | agents that have it | Sends the agent's own command |
| `exit` | Exit | Ends the agent | all agents | Sends the exit command |
| `text-selection` | Text selection | Lets the user select text with the mouse | all agents | Toggles the terminal selection mode |

The toolbar is shown when `agent_toolbar_enabled` is on (default). The user hides or reopens it from the pane icon or the right-click menu. `show_toolbar_labels` adds text after the icons. The last-prompt banner has its own key `last_prompt_enabled` and its line count `last_prompt_max_lines` (1 to 20, default 4). The kernel reserves these rows before terminal interaction begins.

### Samples

#### Last-prompt banner

The last prompt the user typed shows in the pane frame. The plugin reads prompts under [agent.read-prompts](/pages/capability-agent-read-prompts/). On `user-line-submitted` ([events](/pages/api-events/)) it submits `set-frame-slot` with the trimmed prompt in the banner slot of that pane, as described on [tree and window chrome](/pages/api-tree-chrome/).

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::UserLineEvent;
use ilium_plugin_sdk::types::intents::{Intent, SetFrameSlotSpec};

const BANNER_CHARS: usize = 120;

impl Handler for Toolbar {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut out = Vec::new();
        for event in batch {
            let Event::UserLineSubmitted(UserLineEvent { pane, text, .. }) = event else {
                continue;
            };
            let trimmed: String = text.trim().chars().take(BANNER_CHARS).collect();
            if trimmed.is_empty() {
                continue;
            }
            out.push(Submission::new(Intent::SetFrameSlot(SetFrameSlotSpec {
                pane,
                slot: "prompt".into(),
                content: RichText::plain(format!("> {trimmed}")),
                icon: None,
                tooltip: None,
            })));
        }
        out
    }
}
```

**TypeScript**

```ts
import type { Event, Submission } from '@ilium/plugin-sdk';

const BANNER_CHARS = 120;

export function onEvents(batch: Event[]): Submission[] {
  const out: Submission[] = [];
  for (const event of batch) {
    if (event.tag !== 'user-line-submitted') continue;
    const trimmed = event.val.text.trim().slice(0, BANNER_CHARS);
    if (trimmed === '') continue;
    out.push({
      intent: {
        tag: 'set-frame-slot',
        val: {
          pane: event.val.pane,
          slot: 'prompt',
          content: [{ text: `> ${trimmed}` }],
          icon: undefined,
          tooltip: undefined,
        },
      },
      correlation: 1n,
      dedupKey: undefined,
    });
  }
  return out;
}
```

#### Agent toolbar actions

Buttons in the tree toolbar and the hover strip run agent actions. The plugin declares them under [ui.chrome](/pages/capability-ui-chrome/). A click arrives as `command-invoked` ([events](/pages/api-events/)); the client side passes it to the server side with a `message` ([messages and topics](/pages/api-messages-and-topics/)). The server side holds [agent.control](/pages/capability-agent-control/) and submits the `agent-action` intent, such as `compact` ([intents](/pages/api-intents/)).

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::CommandInvokedEvent;
use ilium_plugin_sdk::types::intents::{AgentActionSpec, Intent, MessageSpec, MessageTarget, PluginSide};

// Client side: forward the command with the pane id.
fn forward(invoked: &CommandInvokedEvent) -> Vec<Submission> {
    let Some(pane) = invoked.context.pane else { return Vec::new() };
    vec![Submission::new(Intent::Message(MessageSpec {
        to: MessageTarget { plugin: "ilium.core.agent-toolbar".into(), side: PluginSide::Server, client: None },
        channel: invoked.command.clone(),
        payload: pane.to_string().into_bytes(),
    }))]
}

// Server side: run the action once per pane in each 10-minute window. A press with the same key inside that window completes as duplicate, because a press carries no request id to key on.
fn run_action(channel: &str, payload: &[u8]) -> Option<Submission> {
    let pane: NodeId = String::from_utf8_lossy(payload).parse().ok()?;
    if channel != "compact" {
        return None;
    }
    Some(Submission::keyed(
        Intent::AgentAction(AgentActionSpec { pane, action_id: "compact".into() }),
        format!("compact:{pane}"),
    ))
}
```

**TypeScript**

```ts
import type { Event, Submission } from '@ilium/plugin-sdk';

// Client side: forward the command with the pane id.
export function forward(event: Extract<Event, { tag: 'command-invoked' }>): Submission[] {
  const pane = event.val.context.pane;
  if (pane === undefined) return [];
  return [{
    intent: {
      tag: 'message',
      val: {
        to: { plugin: 'ilium.core.agent-toolbar', side: 'server', client: undefined },
        channel: event.val.command,
        payload: new TextEncoder().encode(pane.toString()),
      },
    },
    correlation: 1n,
    dedupKey: undefined,
  }];
}

// Server side: run the action once per pane in each 10-minute window. A press with the same key inside that window completes as duplicate, because a press carries no request id to key on.
export function runAction(channel: string, payload: Uint8Array): Submission | undefined {
  if (channel !== 'compact') return undefined;
  const pane = BigInt(new TextDecoder().decode(payload));
  return {
    intent: { tag: 'agent-action', val: { pane, actionId: 'compact' } },
    correlation: 1n,
    dedupKey: `compact:${pane}`,
  };
}
```

#### Model and context in the frame

The frame shows the model and how full the context window is. The server side reads typed facts with the [agent transcript primitive](/pages/primitives-agent-transcript/) under [agent.transcript](/pages/capability-agent-transcript/). On `agent-model-changed` ([events](/pages/api-events/)) it sends the model and a percentage to the client side as a `message` ([messages and topics](/pages/api-messages-and-topics/)), which fills the header slot.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::primitives::agent;
use ilium_plugin_sdk::types::events::AgentModelEvent;
use ilium_plugin_sdk::types::intents::{Intent, MessageSpec, MessageTarget, PluginSide};

fn context_percent(used_tokens: u32, window: Option<u32>) -> Option<u32> {
    let window = window.filter(|size| *size > 0)?;
    Some((u64::from(used_tokens) * 100 / u64::from(window)) as u32)
}

fn model_message(event: AgentModelEvent, used_tokens: u32) -> Submission {
    let model = event.model;
    let percent = context_percent(used_tokens, model.context_window);
    let text = match percent {
        Some(value) => format!("{} · {value}% context", model.model),
        None => model.model.clone(),
    };
    let _ = agent::prompt_epoch(event.pane); // the epoch lets later inputs detect a newer prompt
    Submission::new(Intent::Message(MessageSpec {
        to: MessageTarget { plugin: "ilium.core.agent-toolbar".into(), side: PluginSide::Client, client: None },
        channel: "model".into(),
        payload: format!("{}|{text}", event.pane).into_bytes(),
    }))
}
```

**TypeScript**

```ts
import type { Event, Submission } from '@ilium/plugin-sdk';

export function contextPercent(usedTokens: number, window: number | undefined): number | undefined {
  if (window === undefined || window <= 0) return undefined;
  return Math.floor((usedTokens * 100) / window);
}

export function modelMessage(
  event: Extract<Event, { tag: 'agent-model-changed' }>,
  usedTokens: number,
): Submission {
  const { pane, model } = event.val;
  const percent = contextPercent(usedTokens, model.contextWindow);
  const text = percent === undefined ? model.model : `${model.model} · ${percent}% context`;
  return {
    intent: {
      tag: 'message',
      val: {
        to: { plugin: 'ilium.core.agent-toolbar', side: 'client', client: undefined },
        channel: 'model',
        payload: new TextEncoder().encode(`${pane}|${text}`),
      },
    },
    correlation: 1n,
    dedupKey: undefined,
  };
}
```

#### Clear the banner and recover work

The banner and the slots clear when an agent stops, and a recovery hint appears. On `agent-activity-changed` ([events](/pages/api-events/)) the client side submits `clear-frame-slot` and sets the `recover` slot with the last prompt, using the intents on [tree and window chrome](/pages/api-tree-chrome/).

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::AgentActivityEvent;
use ilium_plugin_sdk::types::intents::{ClearFrameSlotSpec, Intent, SetFrameSlotSpec};
use ilium_plugin_sdk::types::tree::AgentActivity;

fn on_stop(event: AgentActivityEvent, last_prompt: Option<&str>) -> Vec<Submission> {
    if event.current != AgentActivity::None && event.current != AgentActivity::Done {
        return Vec::new();
    }
    let pane = event.node;
    let mut out = vec![Submission::new(Intent::ClearFrameSlot(ClearFrameSlotSpec {
        pane,
        slot: "prompt".into(),
    }))];
    if let Some(prompt) = last_prompt {
        out.push(Submission::new(Intent::SetFrameSlot(SetFrameSlotSpec {
            pane,
            slot: "recover".into(),
            content: RichText::plain(format!("Stopped. Last prompt: {prompt}")),
            icon: None,
            tooltip: None,
        })));
    }
    out
}
```

**TypeScript**

```ts
import type { Event, Submission } from '@ilium/plugin-sdk';

export function onStop(
  event: Extract<Event, { tag: 'agent-activity-changed' }>,
  lastPrompt: string | undefined,
): Submission[] {
  const { node, current } = event.val;
  if (current !== 'none' && current !== 'done') return [];
  const out: Submission[] = [{
    intent: { tag: 'clear-frame-slot', val: { pane: node, slot: 'prompt' } },
    correlation: 1n,
    dedupKey: undefined,
  }];
  if (lastPrompt !== undefined) {
    out.push({
      intent: {
        tag: 'set-frame-slot',
        val: {
          pane: node,
          slot: 'recover',
          content: [{ text: `Stopped. Last prompt: ${lastPrompt}` }],
          icon: undefined,
          tooltip: undefined,
        },
      },
      correlation: 2n,
      dedupKey: undefined,
    });
  }
  return out;
}
```

#### Debug history popover

A popover shows the recent journal of a pane. The server side reads the pane journal with [journal.read](/pages/capability-journal-read/) and sends the entries to the client side as a `message` ([messages and topics](/pages/api-messages-and-topics/)). The client side opens a popover anchored to the tree row ([popovers](/pages/api-popovers/)) with a `table` of entries ([collections](/pages/widgets-collections/)).

**Rust**

```rust
// Server side.
use ilium_plugin_sdk::host::{clock, journal};

fn history_json(pane: NodeId) -> String {
    let since = clock::now_unix_millis() - 3_600_000;
    let entries = journal::read(pane, since, 50);
    let rows: Vec<_> = entries
        .iter()
        .map(|entry| serde_json::json!({ "at": entry.at, "kind": entry.kind, "detail": entry.detail }))
        .collect();
    serde_json::Value::Array(rows).to_string()
}

// Client side.
use ilium_plugin_sdk::host::ui::{self, AnchorSide, KeyAnchor, PopupAnchor, PopupOptions};

fn open_history(row_key: &str) -> Result<(), PluginError> {
    ui::open_popup_with(
        "history",
        PopupAnchor::AtKey(KeyAnchor { key: row_key.into(), side: AnchorSide::Right }),
        PopupOptions { modal: false, pinnable: true, dismiss_on_leave: true, close_on_escape: true },
    )
}
```

**TypeScript**

```ts
// Server side.
import { nowUnixMillis } from '@ilium/plugin-sdk/host/clock';
import { read } from '@ilium/plugin-sdk/host/journal';

export function historyJson(pane: bigint): string {
  const entries = read(pane, nowUnixMillis() - 3_600_000n, 50);
  return JSON.stringify(
    entries.map((entry) => ({ at: Number(entry.at), kind: entry.kind, detail: entry.detail })),
  );
}

// Client side.
import { openPopupWith } from '@ilium/plugin-sdk/host/ui';

export const openHistory = (rowKey: string) =>
  openPopupWith('history', { tag: 'at-key', val: { key: rowKey, side: 'right' } },
    { modal: false, pinnable: true, dismissOnLeave: true, closeOnEscape: true });
```

#### Goal line in the banner

`agent-goal-changed` carries the goal text, or nothing when the goal is cleared. The toolbar keeps the latest goal per pane and shows it under the last-prompt banner.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;

impl Handler for GoalLine {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            if let Event::AgentGoalChanged(change) = event {
                match change.goal {
                    Some(goal) => { self.goals.insert(change.node, goal); }
                    None => { self.goals.remove(&change.node); }
                }
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
    if (event.tag !== 'agent-goal-changed') continue;
    if (event.val.goal === undefined) goals.delete(event.val.node);
    else goals.set(event.val.node, event.val.goal);
  }
  return [];
}
```

### Extension points

Other plugins change what `ilium.core.agent-toolbar` does through the points below. The plugin owns each point (grant `extensions.provide:ilium.core.agent-toolbar/<point>`). Contributors need `extensions.contribute:ilium.core.agent-toolbar/<point>`. See [Extension points](/pages/api-extension-points/) for the mechanism and [Extending core plugins](/pages/extending-core-plugins/) for the five ways to work with a core feature.

| Point | Kind | What it does | Payload | Risk |
|---|---|---|---|---|
| `toolbar-actions` | collect | Add a button to the toolbar of panes that run a signature. | entry: id, signature, label, icon, command, requires_action | standard |
| `banner-segments` | collect | Add a short segment to the last-prompt banner. | entry: id, priority, max_width (the plugin fills it with `set-frame-slot`) | standard |

**Services** (grant `services.use:<name>`):

- `ilium.core.agent-toolbar/last-prompt`: get: last prompt text of an agent pane

**Replaceable:** yes. A replacement lists `"replaces": ["ilium.core.agent-toolbar"]` and provides the whole surface (see [Extending core plugins](/pages/extending-core-plugins/#replacing-a-core-plugin)).

#### Example: Adds a Review button to Claude panes

A package adds this entry in its manifest. It needs no code.

```json
{
  "api": "ilium:plugin@0.1.0",
  "capabilities": { "server": ["extensions.contribute:ilium.core.agent-toolbar/toolbar-actions"] },
  "contributes": {
    "extensions": [
      {
        "point": "ilium.core.agent-toolbar/toolbar-actions",
        "id": "run-review",
        "priority": 50,
        "entry": {
          "signature": "claude",
          "label": "Review",
          "icon": "eye",
          "command": "start-review"
        }
      }
    ]
  }
}
```

### Extension points this plugin owns

The plugin owns both collect points above. The values come from the page where it states them; a value the page does not state is the kernel default and is marked (default).

| Point | Side | Version | Deadline | Max | Dynamic |
|---|---|---|---|---|---|
| `ilium.core.agent-toolbar/toolbar-actions` | client | 0.1.0 (default) | none (default) | 64 entries (default) | no (default) |
| `ilium.core.agent-toolbar/banner-segments` | client | 0.1.0 (default) | none (default) | 64 entries (default) | no (default) |

The side is client for both points because the page declares the toolbar buttons under `ui.chrome` (granted only to the client side) and fills the banner slot with `set-frame-slot`. The main manifest above declares both points under `contributes.extension_points` and grants `extensions.provide` for each on the client side.

### ABI used

- [agent.read-prompts](/pages/capability-agent-read-prompts/), [agent.control](/pages/capability-agent-control/) and [agent.transcript](/pages/capability-agent-transcript/)
- [Agent transcript primitive](/pages/primitives-agent-transcript/)
- [Tree and window chrome](/pages/api-tree-chrome/) and [ui.chrome](/pages/capability-ui-chrome/)
- [Intents](/pages/api-intents/), [events](/pages/api-events/) and [messages and topics](/pages/api-messages-and-topics/)
- [Popovers](/pages/api-popovers/), [journal.read](/pages/capability-journal-read/) and [collections](/pages/widgets-collections/)
- [Agent events](/pages/primitives-agent/)

### See also

- [Core replacement matrix](/pages/reference-core-replacement-matrix/) for the surface, stored data, CLI names and install codes that a replacement of this plugin must keep.
- [Agent detection](/pages/core-plugin-agent-detect/), which supplies the activity changes used here.
- [Fleet](/pages/core-plugin-fleet/), which sends text to the same agents from the command line.
