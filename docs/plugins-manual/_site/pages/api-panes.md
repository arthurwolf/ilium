# panes

## panes

The `panes` import reads what a pane is, whether its process is running, and which pane has focus. A plugin uses it to find a pane before it acts on that pane. Pane state changes arrive as events, described in [events](/pages/api-events/). The import itself has two functions, both read-only. Creating and signalling panes is done with intents, covered on [intents](/pages/api-intents/).

A pane restored from a snapshot exists in the tree before its process starts again. A plugin must check `is-restored` before it sends input to a pane, because input to a restored pane is refused with `pane-not-ready`.

**You will need:**

- The grant `panes.read` (risk: normal, both sides). It is needed for both functions and for the pane events.
- `panes.create` to open panes and `panes.signal` to interrupt or stop their programs. Both are described on [intents](/pages/api-intents/) and in their capability pages.
- `terminal.write` when a plugin sends input to a pane. The send is an intent, not a call on this import.
- Rust 1.90 or later with the `wasm32-wasip2` target, or Bun 1.2 or later for TypeScript. The SDK crate is `ilium-plugin-sdk`, and the npm package is `@ilium/plugin-sdk`.

Import the module in Rust with `use ilium_plugin_sdk::host::panes;`. In TypeScript, use `import * as panes from '@ilium/plugin-sdk/host/panes';`. The examples below use these paths.

### Functions

| Rust | TypeScript | Parameters | Returns | Meaning |
|---|---|---|---|---|
| `panes::info(pane)` | `panes.info(pane)` | `pane: NodeId` (`bigint` in TypeScript) | `Option<PaneInfo>`, or `undefined` | Info about one pane. Returns nothing if the node is missing or is not a pane. |
| `panes::focused()` | `panes.focused()` | none | `Option<NodeId>`, or `undefined` | The pane that has focus, if one has focus. |

Neither function returns a `PluginError`. A missing pane is an empty result, not an error. Both functions are synchronous, so the plugin gets the answer in the same call.

### The pane-info record

`info` returns this record. Rust names are snake_case and TypeScript names are camelCase.

| Field (Rust / TypeScript) | Type | Meaning |
|---|---|---|
| `node` / `node` | `NodeId` / `bigint` | The tree node of the pane. Use it with [tree](/pages/api-tree/). |
| `content_kind` / `contentKind` | `String` / `string` | The kind of content, for example a plugin's own pane kind id. |
| `primitive` / `primitive` | `PrimitiveKind` / `'terminal' \| 'editor' \| 'basic'` | The primitive that hosts the pane. |
| `columns` / `columns` | `u16` / `number` | Width in cells. |
| `rows` / `rows` | `u16` / `number` | Height in cells. |
| `focused` / `focused` | `bool` / `boolean` | True when this pane has focus. |
| `title` / `title` | `String` / `string` | The current pane title. |
| `is_restored` / `isRestored` | `bool` / `boolean` | True when the pane came from a snapshot and its process has not started. |

The `primitive` field tells a plugin which primitive hosts the pane. Read the agent state of an agent pane through the [agent primitive](/pages/primitives-agent/). Read screen text through the [terminal primitive](/pages/primitives-terminal/). Basic panes are described on [basic](/pages/primitives-basic/).

### Pane events

Events report changes to panes. They use the `panes.read` grant. Each event is delivered in the batch passed to `on-events`.

| Event | Payload | Delivery |
|---|---|---|
| `pane-created` | `pane-info` | Never dropped |
| `pane-exited` | `node`, `status: option<s32>`, `signal: option<string>` | Never dropped |
| `pane-focused` | `option<node-id>` | Coalesced: only the newest value stays |
| `pane-resized` | `node`, `columns`, `rows` | Coalesced by node |
| `pane-title-changed` | `node`, `title`, `source` (`osc`, `manual` or `inferred`) | Coalesced by node |

For one node the order is fixed: `pane-created`, then other events of that pane, then `pane-exited`, then `node-removed`. A `pane-exited` and a `tree-changed` for the same node in one batch arrive in the order the kernel emitted them. Rust receives these as `Event::PaneCreated(PaneInfo)` and `Event::PaneExited(PaneExitedEvent { node, status, signal })`. TypeScript receives `{ tag: 'pane-created', val: ... }` and `{ tag: 'pane-exited', val: { node, status, signal } }`.

### Pane states after a restore

A pane written in the snapshot comes back as a restored pane. It has a node, a title and a size. Its size is the saved size from the snapshot until the client lays it out. After that, `pane-resized` gives the real size. A restored pane never reports a size of zero.

The process starts later, and then `is-restored` is false. Until then, input is refused with `pane-not-ready`.

<figure>
<svg viewBox="0 0 400 200" role="img" aria-label="Pane states from restore to exit">
  <defs>
    <marker id="panes-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
      <path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="10" y="20" width="130" height="50" rx="6" fill="none" stroke="currentColor"/>
  <text x="75" y="40" font-size="12" text-anchor="middle" fill="currentColor">Restored</text>
  <text x="75" y="56" font-size="11" text-anchor="middle" fill="currentColor">is-restored: true</text>
  <rect x="260" y="20" width="130" height="50" rx="6" fill="none" stroke="currentColor"/>
  <text x="325" y="40" font-size="12" text-anchor="middle" fill="currentColor">Live</text>
  <text x="325" y="56" font-size="11" text-anchor="middle" fill="currentColor">is-restored: false</text>
  <line x1="140" y1="45" x2="258" y2="45" stroke="currentColor" marker-end="url(#panes-arrow)"/>
  <text x="200" y="37" font-size="10" text-anchor="middle" fill="currentColor">process starts</text>
  <rect x="260" y="120" width="130" height="50" rx="6" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
  <text x="325" y="140" font-size="12" text-anchor="middle" fill="currentColor">Exited</text>
  <text x="325" y="156" font-size="11" text-anchor="middle" fill="currentColor">pane-exited event</text>
  <line x1="325" y1="72" x2="325" y2="118" stroke="currentColor" marker-end="url(#panes-arrow)"/>
  <text x="333" y="98" font-size="10" fill="currentColor">process ends</text>
  <rect x="10" y="120" width="150" height="50" rx="6" fill="none" stroke="currentColor"/>
  <text x="85" y="140" font-size="12" text-anchor="middle" fill="currentColor">send-input</text>
  <text x="85" y="156" font-size="11" text-anchor="middle" fill="currentColor">refused: pane-not-ready</text>
  <line x1="75" y1="72" x2="75" y2="118" stroke="currentColor" marker-end="url(#panes-arrow)"/>
  <text x="83" y="98" font-size="10" fill="currentColor">input attempt</text>
  <text x="10" y="194" font-size="11" fill="currentColor">Writes to a restored pane wait until it is live.</text>
</svg>
<figcaption>A restored pane has no process yet. Input is refused until the pane becomes live.</figcaption>
</figure>

### Example 1: read the focused pane and check it before a send

The first example reads the focused pane and describes it. The second function checks a target pane and returns the errors a send would need to handle. It needs `panes.read`.

**Rust**

```rust
use ilium_plugin_sdk::host::panes;
use ilium_plugin_sdk::prelude::*;

fn describe_focus() -> String {
    match panes::focused().and_then(panes::info) {
        Some(info) if info.is_restored => format!("{} is waiting for its process", info.title),
        Some(info) => format!("{} is {}x{}", info.title, info.columns, info.rows),
        None => "no pane has focus".to_string(),
    }
}

fn check_send(pane: NodeId) -> Result<(), PluginError> {
    let info = panes::info(pane).ok_or_else(|| PluginError {
        code: ErrorCode::NotFound,
        message: format!("no pane {pane}"),
        retry_after_ms: None,
    })?;
    if info.is_restored {
        return Err(PluginError {
            code: ErrorCode::PaneNotReady,
            message: "the pane has not started yet".to_string(),
            retry_after_ms: None,
        });
    }
    Ok(())
}
```

**TypeScript**

```ts
import * as panes from '@ilium/plugin-sdk/host/panes';
import { PluginError } from '@ilium/plugin-sdk';

export function describeFocus(): string {
  const focused = panes.focused();
  const info = focused === undefined ? undefined : panes.info(focused);
  if (info === undefined) {
    return 'no pane has focus';
  }
  if (info.isRestored) {
    return `${info.title} is waiting for its process`;
  }
  return `${info.title} is ${info.columns}x${info.rows}`;
}

export function checkSend(pane: bigint): void {
  const info = panes.info(pane);
  if (info === undefined) {
    throw new PluginError('not-found', `no pane ${pane}`);
  }
  if (info.isRestored) {
    throw new PluginError('pane-not-ready', 'the pane has not started yet');
  }
}
```

With a restored pane titled `shell` focused, `describe_focus` returns `"shell is waiting for its process"`.

### Example 2: send a greeting when a restored pane goes live

The second example is a server plugin. It remembers restored panes and sends a greeting after a pane goes live. It retries on `tree-changed`, because the plugin cannot know from `pane-created` alone when the process has started. Every retry checks `is-restored` again. It needs `panes.read`, `tree.read` for the `tree-changed` event, and `terminal.write` for the send. The submission carries a dedup key, which is required for `send-input`.

**Rust**

```rust
use ilium_plugin_sdk::host::panes;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::PaneExitedEvent;
use ilium_plugin_sdk::types::intents::{Delivery, SendInputSpec};

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct GreetOnLive {
    waiting: Vec<NodeId>,
}

impl GreetOnLive {
    fn try_greet(&self, pane: NodeId) -> Option<Submission> {
        let info = panes::info(pane)?;
        if info.is_restored {
            return None;
        }
        let pane_text = pane.to_string();
        Some(Submission::keyed(
            Intent::SendInput(SendInputSpec {
                pane,
                text: "hello from a plugin".to_string(),
                delivery: Delivery::Submitted,
                prompt_epoch: None,
            }),
            format!("greet:{pane_text}"),
        ))
    }
}

impl Handler for GreetOnLive {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            match event {
                Event::PaneCreated(info) if info.is_restored => self.waiting.push(info.node),
                Event::PaneExited(PaneExitedEvent { node, .. }) => {
                    self.waiting.retain(|waiting| *waiting != node);
                }
                Event::TreeChanged(changed) => {
                    for node in changed {
                        if !self.waiting.contains(&node) {
                            continue;
                        }
                        if let Some(submission) = self.try_greet(node) {
                            self.waiting.retain(|waiting| *waiting != node);
                            submissions.push(submission);
                        }
                    }
                }
                _ => {}
            }
        }
        submissions
    }
}
```

**TypeScript**

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import * as panes from '@ilium/plugin-sdk/host/panes';

const waiting = new Set<bigint>();

function tryGreet(pane: bigint): Submission | undefined {
  const info = panes.info(pane);
  if (info === undefined || info.isRestored) {
    return undefined;
  }
  return {
    intent: {
      tag: 'send-input',
      val: {
        pane,
        text: 'hello from a plugin',
        delivery: 'submitted',
        promptEpoch: undefined,
      },
    },
    correlation: pane,
    dedupKey: `greet:${pane}`,
  };
}

export const { handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    for (const event of batch) {
      if (event.tag === 'pane-created' && event.val.isRestored) {
        waiting.add(event.val.node);
      } else if (event.tag === 'pane-exited') {
        waiting.delete(event.val.node);
      } else if (event.tag === 'tree-changed') {
        for (const node of event.val) {
          if (!waiting.has(node)) {
            continue;
          }
          const submission = tryGreet(node);
          if (submission !== undefined) {
            waiting.delete(node);
            submissions.push(submission);
          }
        }
      }
    }
    return submissions;
  },
});
```

The plugin log for one restored pane looks like this:

<pre class="term"><span class="t-hi">example-greeter</span> 0.1.0 (server)
2026-10-11 12:04:11 <span class="t-warn">restore</span> pane 14 restored
2026-10-11 12:04:12 <span class="t-ok">focus</span> pane 14 has focus
2026-10-11 12:04:12 <span class="t-warn">wait</span> pane 14 queued
2026-10-11 12:04:13 <span class="t-err">refused</span> pane-not-ready
2026-10-11 12:04:14 <span class="t-ok">sent</span> greeting after tree-changed
</pre>

Illustrative, not generated. The names and timestamps are placeholders. The refused line appears only if a retry runs before the process starts.

### Lifecycle state and the pane journal

A pane's process can be running, frozen, restarting or exited. The host reports every change with `pane-lifecycle-changed { pane, state, by }` (grant `panes.read`, never dropped). `by` is the plugin that asked for the change, or none when the user or the process did it. The intents that cause changes (`freeze-pane`, `unfreeze-pane`, `replace-pane-command`, `terminate-pane-process`, `resize-pane`) are on [pane lifecycle](/pages/api-pane-lifecycle/). A frozen pane keeps its last screen; a plugin that reads it with `terminal.read-cells` gets that last screen.

The `panes` interface itself has no function for this. To find out whether a pane is frozen, a plugin keeps the last state it saw from the event. To read what happened to a pane earlier (created, exited, resumed), use the pane journal from [clock and timers](/pages/api-clock-and-timers/): the host writes `pane-created` and `pane-exited` entries into it, and `journal.read` returns them.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::Event;
use ilium_plugin_sdk::types::tree::PaneLifecycleState;
use std::collections::HashMap;

struct FrozenTracker { frozen: HashMap<NodeId, bool> }

impl FrozenTracker {
    fn on_event(&mut self, event: &Event) {
        if let Event::PaneLifecycleChanged(change) = event {
            self.frozen.insert(change.pane, change.state == PaneLifecycleState::Frozen);
        }
    }
    fn is_frozen(&self, pane: NodeId) -> bool {
        self.frozen.get(&pane).copied().unwrap_or(false)
    }
}
```

**TypeScript**

```ts
import type { PluginEvent } from '@ilium/plugin-sdk';

const frozen = new Map<bigint, boolean>();

export function onEvent(event: PluginEvent): void {
  if (event.tag === 'pane-lifecycle-changed') {
    frozen.set(event.val.pane, event.val.state === 'frozen');
  }
}

export function isFrozen(pane: bigint): boolean {
  return frozen.get(pane) ?? false;
}
```

<figure>
<pre class="term">▼ 📂 Ilium
›   🦀 ❄ Fix auth     <span class="t-dim">frozen by quota-guard</span>
›   🐢 ● Docs pass
›   🖥️    Shell
</pre>
<figcaption>A frozen agent pane. The snowflake <code>❄</code> is the default glyph of the frozen-agent icon slot; the text on the right is what the plugin that froze it may show in a decoration.</figcaption>
</figure>

### Edge cases and errors

- `info` and `focused` return nothing rather than an error. A missing pane gives `None` or `undefined`, and the plugin decides what that means.
- A `send-input` intent to a restored pane is refused with `pane-not-ready`. The plugin should keep the node and retry after `tree-changed` or `pane-created` for it, and check `is-restored` again before each write.
- A `signal-pane` intent to a restored pane whose process has not started is refused with `pane-not-ready`. A `signal-pane` to a pane whose process has exited is refused with `not-found` and the message `no running process`.
- A `create-pane` whose `cwd` does not exist is refused with `not-found`.
- A `pane-exited` event carries `status` and `signal` as options. Either may be empty, for example when the process ends by a signal or when no status is known.
- Coalesced events, such as `pane-resized`, keep only the newest value for a node. A plugin must read the current state from `info`, not assume that every intermediate size was delivered.
- A node that was removed no longer answers `info`. Treat `node-removed` as the end of that node's state.

### What the user sees

A restored pane appears in the tree with its saved title and size, and it shows no process output until the process starts. The `panes` import draws nothing itself. A user sees the results of what a plugin does with this information, such as a greeting sent to a live pane, a badge on an agent pane or a sound when a process ends.

### Used by

- Sound alerts (feature 47) use `pane-exited` and `panes.read` to play a sound when a process ends.
- Clickable links, screen transfer and the agent toolbar (features 15, 17 and 35) read pane info before they act.
- Session stats (feature 31) reads pane state for each agent pane.

Requirement ids for this page: [EVT-020], [PAN-008], [PAN-010], [PAN-011], [PAN-012]. See the [requirements index](/pages/reference-requirements/).

### A pane whose plugin is gone

When the plugin that provided a pane kind is uninstalled or off, the pane stays in the tree. The host draws a placeholder in its place and no plugin code runs. Input to the pane is refused. When the plugin returns, the pane comes back to life with its saved state (USR-142).

<figure>
<pre class="term"><span class="t-dim">Plugin Notes is not installed</span>

This pane was made by the plugin
com.example.notes (kind: notes).

Install the plugin again to see it,
or close this pane.
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The placeholder of a pane whose plugin was removed.</figcaption>
</figure>

### WIT

The contract below is an excerpt. The owner is the [WIT reference](/pages/reference-wit/), and that page wins if the two differ.

```wit
// Excerpt of ilium:host/panes in reference-wit
info: func(pane: node-id) -> option<pane-info>;
focused: func() -> option<node-id>;
```

## See also

- [Pane lifecycle](/pages/api-pane-lifecycle/) for freeze, replace, terminate and resize, and [tree chrome](/pages/api-tree-chrome/) for frame slots on a pane.
- [Terminal primitive](/pages/primitives-terminal/) for cell reads, history and modes.
- [tree](/pages/api-tree/) for the node that a pane id refers to.
- [events](/pages/api-events/) for the full event catalogue, including `tree-changed` and `node-removed`.
- [intents](/pages/api-intents/) for `create-pane`, `signal-pane` and `send-input`.
- [terminal primitive](/pages/primitives-terminal/) for reading screen text from a pane.
- [panes.read](/pages/capability-panes-read/), [panes.create](/pages/capability-panes-create/) and [panes.signal](/pages/capability-panes-signal/) for the grants.

Requirements for this page: [Panes requirements](/pages/reference-requirements-panes/).
