---
layout: page
title: Events
permalink: /pages/api-events/
---

## Events

Events are what the kernel tells a plugin. The kernel queues them for each plugin and hands them over in batches to the `handler` export. Each call to `on-events` returns the submissions the plugin wants the kernel to apply. Each event has a payload, needs a capability, and has a delivery policy that says what happens when the plugin is slow: the event is kept, merged with newer ones, or dropped and counted.

### What you need

- **Grants.** Each catalogue entry names the capability it needs. Entries marked "always" need no grant. Request the grants per side in the manifest:

```json
"capabilities": { "server": ["panes.read", "sound.play"] }
```

- **Language.** Rust plugins use the `ilium-plugin-sdk` crate. TypeScript plugins use `@ilium/plugin-sdk`. Both generate their types from the WIT interfaces. See the [Rust SDK](/pages/rust-sdk/) and [TypeScript SDK](/pages/typescript-sdk/) pages.
- **Export.** A `server` or `client` plugin exports `handler`. The export is required and is never listed in the manifest `exports` key.
- **Activation.** Events for an inactive plugin are not queued. Events produced while the plugin activates are queued and arrive in the first batch after `activate` returns `ok`. Add a trigger such as `"activation": ["event:pane-exited"]` to start the plugin when that event happens. Without an `activation` key the plugin starts on first use of any of its contributions.

### Handler surface

| Language | Signature | Notes |
|---|---|---|
| WIT | `on-events: func(batch: list<event>) -> list<submission>;` | The `handler` export |
| Rust | `fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission>` | Method of the `Handler` trait, required |
| TypeScript | `onEvents(batch: Event[]): Submission[]` | Member of the object passed to `definePlugin` |

Every event is one case of the WIT variant `event`. Names follow the SDK naming rules:

- Rust: `Event::PaneExited(PaneExitedEvent { node, status, signal })`. Import the types with `ilium_plugin_sdk::types::events::Event`, which the prelude also brings in.
- TypeScript: `{ tag: 'pane-exited', val: { node, status, signal } }`. Import the type with `Event` from `@ilium/plugin-sdk/types`.

Every `u64`, including a node id, is a `bigint` in TypeScript.

### Batch and delivery rules

- A batch holds at most 256 events. The host splits a longer backlog into several batches.
- Each plugin has one queue, filled in kernel emission order across all event kinds. The queue holds 1,024 events by default. The ceiling of `event_queue` is 4,096 for T1 and 1,024 for T2 and T3. The host clamps a larger request to the ceiling and logs the clamp [API-043] [EVT-046]. The manifest key `event_queue` requests a value. The user can set it in `[plugins.limits]` in `config.toml`.
- Order is kept per node: `pane-created`, then the other pane events, then `pane-exited`, then `node-removed`. `agent-identified` always precedes `agent-activity-changed`.
- **Coalesced** events are merged with the pending, undelivered event of the same kind and key (node, buffer, timer, job, watch, pane or signal), whatever the queue fill. The merged event keeps the newest payload at the position of the newest occurrence, so a coalesced kind occupies at most one slot per key [EVT-043]. Pending `tree-changed` events merge into one event whose list is the union of the node ids in first-seen order. When `node-removed` for a node is queued, the host removes that node id from a pending `tree-changed` list and discards every pending coalesced event keyed by that node [EVT-041].
- **Drop oldest, counted** events can be evicted. When an event arrives at a full queue, the host first removes the oldest pending drop-oldest event of any kind and counts it under its own event and source. If no drop-oldest event is pending and the incoming event is drop-oldest, the incoming event is discarded and counted. A coalesced event whose key is already pending never causes an eviction [EVT-004]. The next batch after a drop starts with one `events-dropped` event. `events-dropped` is itself never dropped. At most one is pending: a new drop adds to the count of the matching (event, source) record or appends a new record, and the event always sits at the head of the next batch [EVT-045]. In a record, `event` is the kebab-case case name as in the WIT (for example `"output-matched"`). `source` is `some(filter id)` for `output-matched`, `some(topic name)` for topic events, `some(decimal capture id)` for capture events, and `none` for every other kind. There is one record per (event, source) pair, in order of first drop.
- **Never dropped** events are never lost. If the queue is full, no drop-oldest event is pending and a never-dropped event other than `message` (or a coalesced event with a new key) arrives, the plugin is treated as stuck. A `message` is checked when it is submitted: the sender gets `refused(busy)` instead [SVC-030]. Its instance is restarted, and the new instance receives `tree-changed` for every node so it can read its state again [EVT-004].

### Event catalogue

Payloads use WIT types. Capabilities are the grant strings from the manifest.

| Event | Payload | Capability | Delivery policy |
|---|---|---|---|
| `tree-changed` | `list<node-id>` | `tree.read` | coalesced; pending lists merge into one union |
| `node-removed` | `node-id` | `tree.read` | never dropped |
| `pane-created` | `pane-info` | `panes.read` | never dropped |
| `pane-exited` | `node`, `status: option<s32>`, `signal: option<string>` | `panes.read` | never dropped |
| `pane-focused` | `option<node-id>` | `panes.read` | coalesced |
| `pane-resized` | `node`, `columns`, `rows` | `panes.read` | coalesced by node |
| `pane-title-changed` | `node`, `title`, `source` (osc, manual, inferred) | `panes.read` | coalesced by node |
| `agent-identified` | `node`, `signature` | `agent.read` | never dropped |
| `agent-activity-changed` | `node`, `previous`, `current`, `revision` | `agent.read` | never dropped |
| `agent-goal-changed` | `node`, `goal: option<string>` | `agent.read` | coalesced by node |
| `input-submitted` | `node`, `text`, `source` (user, plugin, scheduled, voice), `epoch` | `agent.read-prompts` | never dropped |
| `output-matched` | `node`, `filter-id`, `line`, `row` | `terminal.output-filter` | drop oldest, counted |
| `link-activated` | `node`, `detector-id`, `text` | own `link_detectors` | never dropped |
| `pointer` | `node`, `row`, `column`, `kind` (down, up, move, drag, wheel-up, wheel-down, wheel-left, wheel-right), `button`, plus `sub-x`, `sub-y`, `modifiers` | `terminal.pointer` while claimed | coalesced (move and drag) |
| `editor-opened`, `editor-closed` | `buffer-ref` | `editor.read` | never dropped |
| `editor-changed` | `buffer`, `revision`, `first-line`, `last-line` | `editor.read` | coalesced by buffer, debounced 100 ms |
| `editor-saved` | `buffer`, `path` | `editor.read` | never dropped |
| `settings-changed` | `settings-change { keys, origin, side }` | always (own) | coalesced |
| `timer-fired` | `timer-id` | always (own) | coalesced by timer |
| `request-completed` | `request-result` | always (own) | never dropped |
| `job-progress` | `job: request-id`, `percent: f32` | always (own) | coalesced by job |
| `message` | `message-event` (`from`, `from-side`, `from-client`, `channel`, `payload: list<u8>`) | always (own sides) or the sender's `messages.peer` | never dropped |
| `topic-published` | `topic`, `from`, `payload-json` | `topics.subscribe:<pattern>` | drop oldest, counted |
| `command-invoked` | `command`, `arguments-json`, `origin` (keybinding, palette, toolbar, menu, voice, cli, toast, row-action, status-segment, tree-toolbar, frame-slot, virtual-row-drop, text), `context` | own commands | never dropped |
| `ui-action` | `view`, `key`, `action-id`, `kind`, `value` | own views | never dropped (`scrolled` coalesced) |
| `key-pressed`, `key-released` | `grab: option<u64>`, `pane: option<node-id>`, `chord`, `repeat` | `keys.intercept` or `wants_keys` | never dropped |
| `view-visibility` | `view`, `visible: bool` | own views | coalesced |
| `fs-changed` | `watch`, `paths: list<string>` | `fs.watch` | coalesced by watch |
| `audio-frames` | `capture`, `samples: list<s16>` | `audio.capture` for input frames, `audio.loopback` for loopback frames | drop oldest, counted |
| `cli-invoked` | `invocation`, `name`, `argv`, `cwd` | `cli.contribute` | never dropped |
| `cli-cancelled` | `invocation: u64` | `cli.contribute` | never dropped |
| `http-called` | `call`, `method`, `path`, `headers`, `body` | `http-api.route` | never dropped |
| `client-attached`, `client-detached` | `client-id` | server side, always | never dropped |
| `theme-changed` | none | client side, always | coalesced |
| `events-dropped` | `list<dropped-count>` (`event`, `source: option<string>`, `count: u32`) | always (own) | coalesced by (event, source); never dropped |

### Request results

`request-completed` is the only way a plugin learns the outcome of a submission or a slow host call. Its payload is `request-result`, which holds the `request` (`submission(u64)` with the correlation number, or `call(request-id)`) and the `outcome`:

- `ok`: the submission was applied. For a call, the typed result is ready to take.
- `ok-node(node-id)`: the submission created that node.
- `duplicate(request-ref)`: the `dedup-key` was already used. Nothing happened.
- `refused(plugin-error)`: the kernel refused the submission before applying it.
- `failed(plugin-error)`: the kernel accepted it but could not apply it.
- `cancelled`: the request was cancelled by the plugin, the user or a shutdown. A sibling of a refused submission completes as `refused(cancelled)`, not as `cancelled`.

In Rust and TypeScript, the SDK removes the completions that a future or promise awaits from the batch. Only unawaited completions reach `on-events`.

The `ui-action` kinds and values are listed below.

#### ui-action kinds and values

A plugin view reports user actions as `ui-action { view, key, action-id, kind, value }`, where `action-id` is a copy of `key` in 0.1.0 and `value` is an option. There are 17 kinds. The value variant has 12 cases in total.

| Kind | Value | Delivery |
|---|---|---|
| `activated` | `none`, or `text(<target>)` for a clickable span with an `action` target | never dropped |
| `changed` | `text`, `index`, `number`, `flag`, `item-state`, `geo` or `ids`, by widget; text inputs arrive after 600 ms (a text-area above 10,000 bytes waits 1 s; `debounce-ms` overrides both) | never dropped |
| `submitted` | `text` | never dropped |
| `selected` | `index`, or `ids` for multi-selection | never dropped |
| `toggled` | `flag`, or `item-state` for `check-list` items and tri-state toggles | never dropped |
| `scrolled` | `number` | coalesced |
| `dropped` | `drop(drop-value)` with `source-key`, `target-key` and `index` | never dropped |
| `context-requested` | `none` | never dropped |
| `closed` | `none` | never dropped |
| `focus-gained`, `focus-lost` | `none` | never dropped |
| `hover-enter`, `hover-leave` | `none`; only for nodes with `hover-events` | never dropped |
| `pointer` | `pointer(view-pointer)`; only for nodes with `pointer-events` | coalesced |
| `drag-over` | `drag-over(drag-over-value)`; drop targets during a drag | coalesced per target |
| `resized` | `number`; the `split` widget | never dropped |
| `filter-changed` | `text`; `%list` and `choice` | never dropped, debounced 150 ms |

```wit
// Excerpt of ilium:types/ui in reference-wit
variant action-value {
  none, text(string), index(u32), number(f64), flag(bool),
  drop(drop-value),
  cell(cell-position),
  pointer(view-pointer),
  drag-over(drag-over-value),
  /// Check-list item id, or "" for a tri-state toggle.
  item-state(tuple<string, tri-state>),
  /// location-picker.
  geo(geo-point),
  /// Multi-selection in the list, table and tree-list.
  ids(list<string>),
}
```

`view-pointer` is `{ kind: pointer-kind, button: pointer-button, modifiers: pointer-modifiers, column: u16, row: u16, sub-x: u8, sub-y: u8, clicks: u8 }`. The column and row are 0-based inside the widget's area. `sub-x` and `sub-y` are 0 to 255 within the cell and read 128 when the terminal reports whole cells only. `clicks` is 1, 2 or 3 for `down` and 0 otherwise.

**Rust**

```rust
Event::UiAction(UiAction { view, key, kind, value, .. }) => match (kind, value) {
    (UiActionKind::Pointer, ActionValue::Pointer(pointer)) if pointer.clicks == 2 => {
        self.open_cell(&key, pointer.column, pointer.row);
    }
    (UiActionKind::DragOver, ActionValue::DragOver(over)) => {
        self.hint = Some((over.target_key, over.accept));   // coalesced per target
    }
    (UiActionKind::Changed, ActionValue::Geo(GeoPoint { lat, lon })) => {
        self.home = (lat, lon);
    }
    (UiActionKind::FilterChanged, ActionValue::Text(query)) => self.filter = query,
    _ => {}
},
```

**TypeScript**

```ts
if (event.tag === 'ui-action') {
  const { kind, value, key } = event.val;
  if (kind === 'pointer' && value.tag === 'pointer' && value.val.clicks === 2) {
    openCell(key, value.val.column, value.val.row);
  } else if (kind === 'drag-over' && value.tag === 'drag-over') {
    hint = { target: value.val.targetKey, accept: value.val.accept };
  } else if (kind === 'changed' && value.tag === 'geo') {
    home = [value.val.lat, value.val.lon];
  } else if (kind === 'filter-changed' && value.tag === 'text') {
    filter = value.val;
  }
}
```

### Further events

The contract appends 18 more event cases after `events-dropped`. Each payload is a record named `<case>-event`, for example `ScreenChangedEvent` in Rust. Fields use kebab-case in WIT, `snake_case` in Rust and `camelCase` in TypeScript.

| Event | Payload | Capability | Delivery policy |
|---|---|---|---|
| `screen-changed` | `pane`, `first-row`, `last-row`, `revision` | `terminal.read-cells` | coalesced by pane, at most 10 per second |
| `user-line-submitted` | `pane`, `text`, `at: epoch-ms` | `agent.read-prompts` | never dropped |
| `signal-changed` | `pane`, `signal`, `value: string` | `agent.read` | coalesced by pane and signal |
| `agent-model-changed` | `pane`, `model: agent-model` | `agent.read` | coalesced by pane |
| `editor-pointer` | `buffer`, `pane`, `position: text-position`, `pointer: view-pointer` | `editor.read` | coalesced (move) |
| `editor-viewport-changed` | `editor-viewport` | `editor.read` | coalesced by pane, 100 ms |
| `http-stream-data` | `stream`, `status: option<u16>`, `headers: option<list<header>>` (`some` on the first chunk only), `chunk: list<u8>` | the `http` grant of the stream | never dropped; the host stops reading the socket while the queue is full |
| `http-stream-closed` | `stream`, `outcome: stream-outcome` (`ended`, `cancelled` or `failed(plugin-error)`) | the `http` grant of the stream | never dropped |
| `child-output` | `child`, `stream` (stdout, stderr), `bytes` | `process.long-lived` | never dropped; same back-pressure |
| `child-exited` | `child`, `status: option<s32>`, `signal: option<string>` | `process.long-lived` | never dropped |
| `pane-lifecycle-changed` | `pane`, `state` (`running`, `frozen`, `restarting`, `exited`), `by: option<plugin-id>` | `panes.read` | never dropped |
| `attention-changed` | `node`, `marked: bool`, `reason: option<string>`, `by: option<plugin-id>` | `tree.attention` or `tree.read` | coalesced by node |
| `client-request` | `call`, `kind`, `payload-json` | client side, own `client_requests` | never dropped |
| `kernel-settings-changed` | `tab`, `keys: list<string>` | `settings.read-kernel:<tab>` | coalesced by tab; keys unioned |
| `keymap-changed` | none | `keymap.read` | coalesced |
| `durable-timer-fired` | `timer`, `name`, `late-by-millis: u64` | always (own) | never dropped |
| `input-uncertain` | `pane`, `dedup-key` | the grant of the original `send-input` | never dropped |
| `display-mode-changed` | `mode: option<string>` (global id; none for Normal) | own `display_modes` | coalesced |

The never-dropped rule is as follows: a full queue marks the plugin as stuck and restarts it. `http-stream-data` and `child-output` add back-pressure first, so a fast network peer or a chatty child slows down before that point. `events-dropped` still counts the drop-oldest events only.

### Extended payloads of existing events

Shape changes:

- `pointer` gains `sub-x`, `sub-y`, `modifiers` and the kind `move`. A `move` is reported only while the plugin claims the pointer and the pointer is over the pane. A claim may ask for a modifier gesture, so plain clicks keep going to the program.
- `command-invoked` `origin` gains `toast`, `row-action`, `status-segment`, `tree-toolbar`, `frame-slot`, `virtual-row-drop` and `text` (a clickable span). Its `context` gains `virtual-row: option<string>` and `menu-item: option<string>`.
- `input-submitted` is joined by `user-line-submitted`, which also covers plain terminal panes. It needs the same `agent.read-prompts` grant.

#### Pane and attention events

`pane-lifecycle-changed` is the confirmation of the lifecycle intents in the [pane lifecycle](/pages/api-intent-pane-lifecycle/) page, and it also reports changes made by the user or by another plugin, with `by` naming the plugin. `attention-changed` does the same for `mark-attention` and `ack-attention` (see [chrome intents](/pages/api-intent-chrome/)). `input-uncertain` follows a crash during a journaled `send-input` (see [send-input stages](/pages/api-intent-send-input/)).

**Rust**

```rust
Event::PaneLifecycleChanged(PaneLifecycleChangedEvent { pane, state, by }) => {
    self.states.insert(pane, state);
    if state == PaneLifecycleState::Frozen && by.is_none() {
        log::log(Level::Info, &format!("pane {pane} frozen by the user"));
    }
}
Event::AttentionChanged(AttentionChangedEvent { node, marked, reason, .. }) => {
    if !marked { self.pending.remove(&node); } else if let Some(r) = reason { self.pending.insert(node, r); }
}
Event::InputUncertain(InputUncertainEvent { pane, dedup_key }) => {
    // read the screen, decide, then resend with a NEW key
    self.needs_review.push((pane, dedup_key));
}
Event::DurableTimerFired(DurableTimerFiredEvent { name, late_by_millis, .. }) => {
    if late_by_millis > 60_000 { self.catch_up(&name); }
}
```

**TypeScript**

```ts
switch (event.tag) {
  case 'pane-lifecycle-changed':
    states.set(event.val.pane, event.val.state);
    if (event.val.state === 'frozen' && event.val.by === undefined) {
      log('info', `pane ${event.val.pane} frozen by the user`);
    }
    break;
  case 'attention-changed':
    if (!event.val.marked) pending.delete(event.val.node);
    else if (event.val.reason !== undefined) pending.set(event.val.node, event.val.reason);
    break;
  case 'input-uncertain':
    needsReview.push([event.val.pane, event.val.dedupKey]);   // resend with a NEW key
    break;
  case 'durable-timer-fired':
    if (event.val.lateByMillis > 60_000n) catchUp(event.val.name);
    break;
}
```

<figure>
<svg viewBox="0 0 420 150" role="img" aria-label="How a frozen pane produces events">
  <defs>
    <marker id="events02-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
      <path d="M 0 0 L 10 5 L 0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="6" y="50" width="96" height="46" rx="6" fill="none" stroke="currentColor"/>
  <text x="54" y="70" font-size="11" text-anchor="middle" fill="currentColor">freeze-pane</text>
  <text x="54" y="85" font-size="10" text-anchor="middle" fill="currentColor">intent</text>
  <rect x="162" y="50" width="96" height="46" rx="6" fill="none" stroke="currentColor"/>
  <text x="210" y="70" font-size="11" text-anchor="middle" fill="currentColor">Kernel</text>
  <text x="210" y="85" font-size="10" text-anchor="middle" fill="currentColor">stops process</text>
  <rect x="318" y="14" width="96" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="366" y="38" font-size="10" text-anchor="middle" fill="currentColor">request-completed</text>
  <rect x="318" y="92" width="96" height="48" rx="6" fill="none" stroke="currentColor"/>
  <text x="366" y="112" font-size="10" text-anchor="middle" fill="currentColor">pane-lifecycle-</text>
  <text x="366" y="126" font-size="10" text-anchor="middle" fill="currentColor">changed: frozen</text>
  <line x1="102" y1="73" x2="160" y2="73" stroke="currentColor" marker-end="url(#events02-arrow)"/>
  <line x1="258" y1="62" x2="316" y2="38" stroke="currentColor" marker-end="url(#events02-arrow)"/>
  <line x1="258" y1="84" x2="316" y2="112" stroke="currentColor" marker-end="url(#events02-arrow)"/>
</svg>
<figcaption>One intent produces two events: the outcome for the submitter and a lifecycle event for every plugin with <code>panes.read</code>.</figcaption>
</figure>

What the user sees when a plugin freezes a pane. The tree row shows the frozen mark from the client's icon set (the default glyph of the frozen slot):

<pre class="term"><span class="t-dim">▼ 📂 Services</span>
<span class="t-dim">›   🖥️    api-server</span>
<span class="t-hi">›   🖥️ ❄ build-watch</span>
<span class="t-dim">›   🖥️    logs</span>
</pre>

Layout follows the tree row rule (identity, objective and now slots, then the title). The glyphs are those of the client icon inventory; the capture is composed from that inventory, not taken from a running client.

### Diagram: one batch

<figure>
<svg viewBox="0 0 400 160" role="img" aria-label="One batch travels from the kernel to the handler and back">
  <defs>
    <marker id="api-events-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
      <path d="M 0 0 L 10 5 L 0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <text x="8" y="20" font-size="11" fill="currentColor">One batch, one handler call</text>
  <rect x="8" y="50" width="100" height="50" rx="6" fill="none" stroke="currentColor"/>
  <text x="58" y="72" font-size="12" text-anchor="middle" fill="currentColor">Kernel</text>
  <text x="58" y="88" font-size="10" text-anchor="middle" fill="currentColor">emits events</text>
  <rect x="150" y="50" width="100" height="50" rx="6" fill="none" stroke="currentColor"/>
  <text x="200" y="72" font-size="12" text-anchor="middle" fill="currentColor">Queue</text>
  <text x="200" y="88" font-size="10" text-anchor="middle" fill="currentColor">per plugin</text>
  <rect x="292" y="50" width="100" height="50" rx="6" fill="none" stroke="currentColor"/>
  <text x="342" y="72" font-size="12" text-anchor="middle" fill="currentColor">Handler</text>
  <text x="342" y="88" font-size="10" text-anchor="middle" fill="currentColor">on-events</text>
  <line x1="108" y1="75" x2="148" y2="75" stroke="currentColor" marker-end="url(#api-events-arrow)"/>
  <line x1="250" y1="75" x2="290" y2="75" stroke="currentColor" marker-end="url(#api-events-arrow)"/>
  <text x="128" y="42" font-size="10" text-anchor="middle" fill="currentColor">events</text>
  <text x="270" y="42" font-size="10" text-anchor="middle" fill="currentColor">batch</text>
  <path d="M 342 100 L 342 130 L 58 130 L 58 102" fill="none" stroke="currentColor" marker-end="url(#api-events-arrow)"/>
  <text x="200" y="124" font-size="11" text-anchor="middle" fill="currentColor">submissions back</text>
</svg>
<figcaption>The queue hands one batch to the handler. The returned submissions go back to the kernel, which reports each outcome in a later batch as <code>request-completed</code>.</figcaption>
</figure>

### Diagram: delivery policy per event

<figure>
<svg viewBox="0 0 340 170" role="img" aria-label="Delivery policy per event kind">
<title>Delivery policy per event</title>
<desc>Three lanes show the policies. Never dropped events go to the plugin in order and a full queue restarts the plugin. Drop-oldest events lose the head of the queue and count the loss. Coalesced events keep only the latest state per key.</desc>
<text x="8" y="22" font-size="12" fill="currentColor">Never dropped</text>
<text x="150" y="22" font-size="11" fill="currentColor">Full queue: plugin restarts</text>
<rect x="8" y="30" width="324" height="26" rx="5" fill="none" stroke="currentColor"/>
<text x="16" y="47" font-size="11" fill="currentColor">pane-exited, request-completed, cli-invoked</text>
<text x="8" y="80" font-size="12" fill="currentColor">Drop oldest, counted</text>
<rect x="8" y="88" width="324" height="26" rx="5" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
<text x="16" y="105" font-size="11" fill="currentColor">audio-frames, output-matched, topic-published</text>
<text x="8" y="138" font-size="12" fill="currentColor">Coalesced by key</text>
<rect x="8" y="146" width="324" height="20" rx="5" fill="none" stroke="currentColor"/>
<text x="16" y="160" font-size="11" fill="currentColor">tree-changed, editor-changed, timer-fired</text>
</svg>
<figcaption>Each event kind has one policy. Check the policy before you rely on an event for state.</figcaption>
</figure>

### Example 1: sound on pane exit, with a settings check

The handler plays a sound when a pane exits, unless the user muted it. It logs timers and request results, and it re-reads the setting when `sound-enabled` changes.

**Grants:** `panes.read`, `sound.play`

**Rust**

```rust
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::prelude::*;

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct Notices {
    sound_muted: bool,
}

impl Lifecycle for Notices {}

impl Handler for Notices {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            match event {
                Event::PaneExited(exited) => {
                    let line = format!("pane {} exited: {:?}", exited.node, exited.status);
                    log::log(Level::Info, &line);
                    if !self.sound_muted {
                        submissions.push(Submission::new(Intent::PlaySound("ding".to_string())));
                    }
                }
                Event::TimerFired(timer_id) => {
                    log::log(Level::Debug, &format!("timer {timer_id} fired"));
                }
                Event::SettingsChanged(change) => {
                    if change.keys.iter().any(|key| key == "sound-enabled") {
                        self.sound_muted = !read_sound_enabled(); // see api-settings
                    }
                }
                Event::RequestCompleted(result) => {
                    log::log(Level::Debug, &format!("request result: {:?}", result.outcome));
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
import { log } from '@ilium/plugin-sdk/host/log';
import { definePlugin } from '@ilium/plugin-sdk';
import type { Event, Submission } from '@ilium/plugin-sdk/types';

let soundMuted = false;
let nextCorrelation = 1n;

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  activate() {},
  deactivate() { return undefined; },
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    for (const event of batch) {
      switch (event.tag) {
        case 'pane-exited':
          log('info', `pane ${event.val.node} exited: ${event.val.status}`);
          if (!soundMuted) {
            submissions.push({
              intent: { tag: 'play-sound', val: 'ding' },
              correlation: nextCorrelation++,
              dedupKey: undefined,
            });
          }
          break;
        case 'timer-fired':
          log('debug', `timer ${event.val} fired`);
          break;
        case 'settings-changed':
          if (event.val.includes('sound-enabled')) {
            soundMuted = !readSoundEnabled(); // see api-settings
          }
          break;
        case 'request-completed':
          log('debug', `request result: ${event.val.outcome.tag}`);
          break;
        default:
          break;
      }
    }
    return submissions;
  },
});
```

Outcome: when a pane exits and sound is on, the batch returns one `play-sound` submission. The kernel applies it and sends `request-completed` in a later batch. Its `request` field is `submission(<correlation>)` with the same number, and its `outcome` is `ok`. The sound id `ding` must be in the catalogue from `sound.catalog()`. An unknown id refuses only that submission.

<figure>
<pre class="term">$ ilium plugin test ./notices --fixture pane-exit.json
<span class="t-dim">{"type":"log","message":"pane 14 exited"}</span>
<span class="t-hi">{"type":"submission","index":0,"intent":"play-sound"}</span>
<span class="t-ok">{"type":"outcome","index":0,"outcome":"ok"}</span>
<span class="t-dim">1 batch, 1 submission, 0 refused</span>
</pre>
<figcaption>Illustrative, not generated. Output of the test harness for one pane-exit batch, using the handler above.</figcaption>
</figure>

### Example 2: track changed nodes and count dropped events

This server handler keeps the set of nodes that changed since it last read the tree. Because `tree-changed` is coalesced, the set holds one entry per node, however often the node changed. When the kernel reports dropped events, the handler logs the counts.

**Grants:** `tree.read`

**Rust**

```rust
use std::collections::BTreeSet;

use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::prelude::*;

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct TreeWatch {
    dirty_nodes: BTreeSet<NodeId>,
}

impl Lifecycle for TreeWatch {}

impl Handler for TreeWatch {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            match event {
                Event::TreeChanged(nodes) => self.dirty_nodes.extend(nodes),
                Event::NodeRemoved(node) => {
                    self.dirty_nodes.remove(&node);
                }
                Event::EventsDropped(counts) => {
                    for count in counts {
                        let line = format!("{} {} events dropped", count.count, count.event);
                        log::log(Level::Warn, &line);
                    }
                }
                _ => {}
            }
        }
        Vec::new()
    }
}
```

**TypeScript**

```ts
import { log } from '@ilium/plugin-sdk/host/log';
import { definePlugin } from '@ilium/plugin-sdk';
import type { Event, Submission } from '@ilium/plugin-sdk/types';

const dirtyNodes = new Set<bigint>();

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  activate() {},
  deactivate() { return undefined; },
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      switch (event.tag) {
        case 'tree-changed':
          event.val.forEach((node) => dirtyNodes.add(node));
          break;
        case 'node-removed':
          dirtyNodes.delete(event.val);
          break;
        case 'events-dropped':
          for (const count of event.val) {
            log('warn', `${count.count} ${count.event} events dropped`);
          }
          break;
        default:
          break;
      }
    }
    return [];
  },
});
```

Outcome: a batch with three `tree-changed` events for node 7 leaves one entry for node 7 in `dirtyNodes`. If the queue dropped 12 `output-matched` events, the next batch starts with `events-dropped`, and the handler logs `12 output-matched events dropped` at level warn.

### Errors and limits

| Condition | Effect |
|---|---|
| Malformed submission in a batch | The batch is refused. The lowest-index bad submission completes as `refused(invalid-argument)`, the others as `refused(cancelled)` |
| Submission without the grant it needs | The batch is refused. That submission completes as `refused(not-permitted)`; the rest of the batch is `refused(cancelled)` |
| More than 256 submissions returned | Trap. The instance restarts |
| Handler exceeds its deadline (20 ms by default, 50 ms at the tier 1 ceiling) | Trap. The instance restarts after 1 s |
| Never-dropped event with a full queue | The plugin is treated as stuck and restarted. The new instance receives `tree-changed` for all nodes |
| Restart failures | Restart after 1 s on the first failure in the window and 5 s on the second. The third failure within 10 minutes disables the plugin on that side |
| Event for a capability that was not granted | Not delivered. The plugin never sees it |

Events themselves do not fail. Keep the handler short. Move slow work to a job (see [Jobs](/pages/api-jobs/)) or to a submission whose outcome arrives later.

<div class="warn">
<p>A trap or a deadline breach ends the instance and cancels its pending requests. The new instance starts with empty state, so re-read any state it needs from the tree or from storage.</p>
</div>

### What the user sees

Events are not shown directly. The user sees their effects: a sound plays, a decoration appears, and the plugin's log lines appear in `ilium plugin logs`. A refused submission and every trap are written to the log with the plugin id, the interface and the function. When a plugin is disabled after repeated failures, the tree footer shows a notice, and the user re-enables the plugin from settings or the kernel recovery page.

Requirement ids for this page: [EVT-030]. See the [requirements index](/pages/reference-requirements/).

### See also

- [Intents](/pages/api-intents/) and its deep pages [send-input stages](/pages/api-intent-send-input/), [pane lifecycle](/pages/api-intent-pane-lifecycle/), [tree structure](/pages/api-intent-tree-structure/) and [chrome intents](/pages/api-intent-chrome/) for the submissions that handlers return and their outcomes.
- [Requests and outcomes](/pages/requests-and-outcomes/) for the completion model behind `request-completed`.
- [Plugin exports](/pages/api-exports/) for `handler` and the lifecycle that delivers the batches.
- [Clock and timers](/pages/api-clock-and-timers/) for `timer-fired`.
- [Tree](/pages/api-tree/) for the `tree.read` grant behind `tree-changed`.
- [Errors and limits](/pages/reference-errors-and-limits/) for the full limit table.
- [Lifecycle](/pages/lifecycle/) for activation, deactivation and restarts.

Checked against `ilium:plugin@0.1.0`.
