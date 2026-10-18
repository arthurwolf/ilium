---
layout: page
title: Event and intent tables
permalink: /pages/reference-events-and-intents/
---

## Event and intent tables

Plugins hear about the world through events, and they change it through intents. An event is a fact the kernel reports: a pane exited, a node was renamed, an agent started working. An intent is a request the plugin sends back to the kernel: rename this node, play this sound, type this text into that terminal. The kernel checks every intent against the plugin's grants and applies it, or refuses it with an error code.

Events arrive in batches at the `handler` export. Intents leave in the list of submissions that `on-events` returns, or through the `intents.submit` import, which is used from outside a batch. This page is the single list of event names, intent names, payloads, grants and outcomes. Each namespace page repeats its own rows with more detail.

### What you need

- **Grants.** An event needs the grant in its row, unless the row says "always". An intent needs the grant in its row too. Declare grants for the side that uses them, for example:

```json
"capabilities": { "server": ["panes.read", "tree.write"] }
```

- **Language.** Rust plugins need Rust 1.90 or later with the `wasm32-wasip2` target and the `ilium-plugin-sdk` crate. TypeScript plugins need Bun 1.2 or later and `@ilium/plugin-sdk`. Nothing is installed globally. See the [Rust SDK](/pages/rust-sdk/) and [TypeScript SDK](/pages/typescript-sdk/) pages.
- **Export.** A plugin that receives events exports `handler`. The export is required on the server and client sides, and it is never listed in the manifest `exports` key.
- **Activation.** A lazily activated plugin receives nothing until one of its triggers fires. Events for an inactive plugin are not queued. The event that triggers a lazy activation, and every event produced while the plugin activates, are queued by the same queue rules and arrive in the first batch after `activate` returns `ok`, in arrival order. If `activate` fails or breaches its deadline, those queued events are discarded without an `events-dropped` report [EVT-048]. After every activation of a plugin that holds `tree.read` (first, restart or re-activation), the first batch starts with exactly one `tree-changed` event whose list holds every node id in tree pre-order, in a single event whatever the count. Without `tree.read` nothing is sent [EVT-047].

## How events are delivered

- A batch holds at most 256 events. When the backlog is longer, the host splits it into several batches.
- Each plugin has one queue, filled in kernel emission order across all event kinds. The queue holds 1,024 events by default. The ceiling of `event_queue` is 4,096 for T1 and 1,024 for T2 and T3. The host clamps a larger request to the ceiling and logs the clamp [API-043] [EVT-046]. The manifest key `event_queue` requests a value.
- Order is kept per node. A `pane-created` comes first, then other pane events, then `pane-exited`, then `node-removed`. `agent-identified` always precedes `agent-activity-changed` for the same pane.
- **Coalesced** events are merged with the pending, undelivered event of the same kind and key (node, buffer, timer, job, watch, pane or signal), whatever the queue fill. The merged event keeps the newest payload at the position of the newest occurrence, so a coalesced kind occupies at most one slot per key [EVT-043]. Pending `tree-changed` events merge into one event whose list is the union of the node ids in first-seen order. When `node-removed` for a node is queued, the host removes that node id from a pending `tree-changed` list and discards every pending coalesced event keyed by that node [EVT-041].
- **Drop oldest, counted** events can be evicted. When an event arrives at a full queue, the host first removes the oldest pending drop-oldest event of any kind and counts it under its own event and source. If no drop-oldest event is pending and the incoming event is drop-oldest, the incoming event is discarded and counted. A coalesced event whose key is already pending never causes an eviction [EVT-004]. The next batch after a drop starts with one `events-dropped` event. `events-dropped` is itself never dropped. At most one is pending: a new drop adds to the count of the matching (event, source) record or appends a new record, and the event always sits at the head of the next batch [EVT-045]. In a record, `event` is the kebab-case case name as in the WIT (for example `"output-matched"`). `source` is `some(filter id)` for `output-matched`, `some(topic name)` for topic events, `some(decimal capture id)` for capture events, and `none` for every other kind. There is one record per (event, source) pair, in order of first drop.
- **Never dropped** events are never lost. If the queue is full, no drop-oldest event is pending and a never-dropped event other than `message` (or a coalesced event with a new key) arrives, the plugin is treated as stuck. A `message` is checked when it is submitted: the sender gets `refused(busy)` instead [SVC-030]. Its instance is restarted, and the new instance receives `tree-changed` for every node so it can read its state again [EVT-004].

<figure>
<svg viewBox="0 0 400 180" role="img" aria-label="Events flow from the kernel to a plugin and submissions flow back">
  <defs>
    <marker id="ev-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto">
      <path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="6" y="50" width="110" height="60" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="61" y="74" font-size="12" text-anchor="middle" fill="currentColor">Kernel</text>
  <text x="61" y="92" font-size="11" text-anchor="middle" fill="currentColor">event queue</text>
  <rect x="284" y="50" width="110" height="60" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="339" y="74" font-size="12" text-anchor="middle" fill="currentColor">Plugin</text>
  <text x="339" y="92" font-size="11" text-anchor="middle" fill="currentColor">on-events</text>
  <line x1="118" y1="70" x2="280" y2="70" stroke="currentColor" marker-end="url(#ev-arrow)"/>
  <text x="199" y="62" font-size="11" text-anchor="middle" fill="currentColor">events</text>
  <line x1="280" y1="100" x2="118" y2="100" stroke="currentColor" marker-end="url(#ev-arrow)"/>
  <text x="199" y="118" font-size="11" text-anchor="middle" fill="currentColor">submissions</text>
  <text x="200" y="150" font-size="10" text-anchor="middle" fill="currentColor">Each submission completes once, as request-completed.</text>
  <text x="200" y="166" font-size="10" text-anchor="middle" fill="currentColor">Refusals and failures come back the same way.</text>
</svg>
<figcaption>Events reach a plugin only when it holds the grant and is active. Its submissions are applied, and each one completes once.</figcaption>
</figure>

## Event catalogue

Payloads use the WIT types of the API. Names in the Rust and TypeScript SDKs follow the naming rules of the [Rust SDK](/pages/rust-sdk/) page: `pane-exited` is `Event::PaneExited` in Rust and `{ tag: 'pane-exited', val: ... }` in TypeScript.

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
| `input-submitted` | `node`, `text`, `source` (user, plugin, scheduled, voice), `epoch: u64` | `agent.read-prompts` | never dropped |
| `output-matched` | `node`, `filter-id`, `line`, `row` | `terminal.output-filter` | drop oldest, counted |
| `link-activated` | `node`, `detector-id`, `text` | own `link_detectors` | never dropped |
| `pointer` | `node`, `row`, `column`, `kind` (down, up, move, drag, wheel-up, wheel-down, wheel-left, wheel-right), `button: option` | `terminal.pointer`, while claimed | coalesced (move and drag) |
| `editor-opened`, `editor-closed` | `buffer-ref` | `editor.read` | never dropped |
| `editor-changed` | `buffer`, `revision`, `first-line`, `last-line` | `editor.read` | coalesced by buffer, debounced 100 ms |
| `editor-saved` | `buffer`, `path` | `editor.read` | never dropped |
| `settings-changed` | `settings-change { keys, origin, side }` | always (own) | coalesced; keys unioned, the last origin kept |
| `timer-fired` | `timer-id` | always (own) | coalesced by timer |
| `request-completed` | `request-result` | always (own) | never dropped |
| `job-progress` | `job: request-id`, `percent: f32` | always (own) | coalesced by job |
| `message` | `from`, `from-side`, `from-client`, `channel`, `payload: list<u8>` | always (own sides), or the sender's `messages.peer` | never dropped |
| `topic-published` | `topic`, `from`, `payload-json` | `topics.subscribe` | drop oldest, counted |
| `command-invoked` | `command`, `arguments-json`, `origin`, `context` | own commands | never dropped |
| `ui-action` | `view`, `key`, `action-id`, `kind`, `value` | own views | never dropped (`scrolled` coalesced) |
| `key-pressed`, `key-released` | `grab`, `pane`, `chord`, `repeat` | `keys.intercept` or `wants_keys` | never dropped |
| `view-visibility` | `view`, `visible: bool` | own views | coalesced |
| `fs-changed` | `watch`, `paths: list<string>` | `fs.watch` | coalesced by watch |
| `audio-frames` | `capture`, `samples: list<s16>` | `audio.capture` or `audio.loopback` | drop oldest, counted |
| `cli-invoked` | `invocation`, `name`, `argv`, `cwd` | `cli.contribute` | never dropped |
| `cli-cancelled` | `invocation: u64` | `cli.contribute` | never dropped |
| `http-called` | `call`, `method`, `path`, `headers`, `body` | `http-api.route` | never dropped |
| `client-attached`, `client-detached` | `client-id` | server side, always | never dropped |
| `theme-changed` | none | client side, always | coalesced |
| `events-dropped` | `list<dropped-count>` (`event`, `source`, `count`) | always (own) | coalesced by (event, source); never dropped |

`events-dropped` carries the strings defined under [How events are delivered](#how-events-are-delivered) in `event` and `source`. It is the only way a plugin learns that events were lost.

### UI actions

A `ui-action` event carries the view, the widget key, the action id, a kind and a value.

- Kinds: `activated`, `changed`, `submitted`, `selected`, `toggled`, `scrolled`, `dropped`, `context-requested`, `closed`, `focus-gained`, `focus-lost`.
- Values: `none`, `text(string)`, `index(u32)`, `number(f64)`, `flag(bool)`, `drop(drop-value)` (a record with `source-key`, `target-key` and `index`).
- Debounce runs before a `changed` action is sent. A text input sends after 600 ms of rest. A text-area above 10,000 bytes waits 1 s, and an explicit `debounce-ms` wins. Toggles and choices send at once. A multi-line text area never sends `submitted`, because Enter inserts a line.

## Extended events

Eighteen further event cases are appended after `events-dropped`. All are minor. A plugin that does not match them keeps working: an unmatched case falls through the `_` arm in Rust and the `default` branch in TypeScript.

| Event | Payload | Capability | Delivery policy |
|---|---|---|---|
| `screen-changed` | `pane`, `first-row`, `last-row`, `revision` | `terminal.read-cells` | coalesced by pane, at most 10 per second |
| `user-line-submitted` | `pane`, `text`, `at` | `agent.read-prompts` | never dropped |
| `signal-changed` | `pane`, `signal`, `value` | `agent.read` | coalesced by pane and signal |
| `agent-model-changed` | `pane`, `model` | `agent.read` | coalesced by pane |
| `editor-pointer` | `buffer`, `pane`, `position`, `pointer` | `editor.read` | coalesced on move |
| `editor-viewport-changed` | `editor-viewport` | `editor.read` | coalesced by pane, 100 ms |
| `http-stream-data` | `stream`, `status: option<u16>`, `headers: option<list<header>>` (`some` on the first chunk), `chunk: list<u8>` | the `http` grant of the stream | never dropped; the host stops reading the socket while the queue is full |
| `http-stream-closed` | `stream`, `outcome: stream-outcome` (`ended`, `cancelled`, `failed(plugin-error)`) | the `http` grant of the stream | never dropped |
| `child-output` | `child`, `stream` (stdout, stderr), `bytes` | `process.long-lived` | never dropped, with back-pressure |
| `child-exited` | `child`, `status`, `signal` | `process.long-lived` | never dropped |
| `pane-lifecycle-changed` | `pane`, `state` (running, frozen, restarting, exited), `by` | `panes.read` | never dropped |
| `attention-changed` | `node`, `marked`, `reason`, `by` | `tree.attention` or `tree.read` | coalesced by node |
| `client-request` | `call`, `kind`, `payload-json` | client side, own `client_requests` | never dropped |
| `kernel-settings-changed` | `tab`, `keys` | `settings.read-kernel:<tab>` | coalesced by tab, keys unioned |
| `keymap-changed` | none | `keymap.read` | coalesced |
| `durable-timer-fired` | `timer`, `name`, `late-by-millis` | none (own timers) | never dropped |
| `input-uncertain` | `pane`, `dedup-key` | the grant of the original `send-input` | never dropped |
| `display-mode-changed` | `mode` (none for Normal) | own `display_modes` | coalesced |

Two existing payloads gain optional fields:

- `pointer` gains `sub-x`, `sub-y` and `modifiers`. The kind `move` is in the base kind list and is reported only while the pointer is claimed and over the pane.
- `command-invoked` gains the origins `toast`, `row-action`, `status-segment`, `tree-toolbar`, `frame-slot`, `virtual-row-drop` and `text`. Its context gains `virtual-row` and `menu-item`.

Two prompt events exist. `input-submitted` fires only for a pane with an identified agent, once per recorded prompt, and carries the source and the epoch. `user-line-submitted` fires for any terminal pane, agent or plain, when the user presses Enter on a line, and carries no source. For one prompt typed by the user into an agent pane, both fire, `input-submitted` first, then `user-line-submitted` in the same batch if the queue allows. A prompt sent by a plugin, a schedule or voice fires `input-submitted` only [EVT-040].

A coalesced event takes the position of its newest occurrence, which can place a coalesced event after `node-removed` for the same node. When `node-removed` is queued, the host removes that node id from a pending `tree-changed` list and discards every pending coalesced event keyed by that node (for example `pane-resized` or `pane-title-changed`), so a plugin never sees an event for a node after its `node-removed` [EVT-041].

`durable-timer-fired` arrives once, late, after a downtime. `input-uncertain` means the host cannot tell whether a journaled `send-input` reached the program before a restart. The host never replays it. The plugin decides.

### ui-action kinds and action values

Six kinds extend the base set, which makes 17: `hover-enter` and `hover-leave` (nodes with `hover-events`), `pointer` (nodes with `pointer-events`), `drag-over` (drop targets), `resized` (`split`) and `filter-changed` (`%list`, `choice`, debounced 150 ms). `scrolled`, `pointer` and `drag-over` are coalesced. The others are never dropped.

Six `action-value` cases extend the base set of six (`none`, `text`, `index`, `number`, `flag`, `drop`), which makes 12.

| Case | Payload | Sent by |
|---|---|---|
| `cell` | `cell-position` (`column`, `row`) | `surface`, `terminal-view` and `code-view` activations |
| `pointer` | `view-pointer` | `pointer` actions |
| `drag-over` | `source-key`, `target-key`, `index`, `accept` (`none`, `before`, `after`, `inside`) | `drag-over` actions |
| `item-state` | item id (empty for a tri-state toggle), `tri-state` | `check-list`, tri-state toggles |
| `geo` | `lat`, `lon` | `location-picker` |
| `ids` | `list<string>` | multi-selection in `%list`, `table`, `tree-list` |

**Rust**

```rust
match event {
    Event::UiAction(a) => match (a.kind, a.value) {
        (ActionKind::DragOver, Some(ActionValue::DragOver(d))) => {
            self.drop_hint = Some((d.target_key, d.index, d.accept));
        }
        (ActionKind::FilterChanged, Some(ActionValue::Text(t))) => self.filter = t,
        (_, Some(ActionValue::Ids(ids))) => self.selected = ids,
        (_, Some(ActionValue::Geo(p))) => self.center = (p.lat, p.lon),
        _ => {}
    },
    Event::DurableTimerFired(t) if t.late_by_millis > 60_000 => self.catch_up(&t.name),
    Event::InputUncertain(u) => out.push(self.ask_user_to_confirm(u.pane, &u.dedup_key)),
    _ => {}
}
```

**TypeScript**

```ts
switch (event.tag) {
  case 'ui-action': {
    const a = event.val;
    if (a.kind === 'drag-over' && a.value?.tag === 'drag-over') {
      dropHint = [a.value.val.targetKey, a.value.val.index, a.value.val.accept];
    } else if (a.kind === 'filter-changed' && a.value?.tag === 'text') {
      filter = a.value.val;
    } else if (a.value?.tag === 'ids') {
      selected = a.value.val;
    } else if (a.value?.tag === 'geo') {
      center = [a.value.val.lat, a.value.val.lon];
    }
    break;
  }
  case 'durable-timer-fired':
    if (event.val.lateByMillis > 60_000n) catchUp(event.val.name);
    break;
  case 'input-uncertain':
    out.push(askUserToConfirm(event.val.pane, event.val.dedupKey));
    break;
}
```

## Submissions and outcomes

```wit
record submission { intent: intent, correlation: u64, dedup-key: option<string> }
variant request-ref { submission(u64), call(request-id) }
variant outcome {
  ok, ok-node(node-id), duplicate(request-ref), refused(plugin-error), failed(plugin-error), cancelled,
  ok-epoch(u64),   // send-input on an agent pane, the new prompt epoch
}
record request-result { request: request-ref, outcome: outcome }
```

- The plugin picks the `correlation` number. A submission returned from `on-events` completes as `request-completed`, with `request-ref` `submission(<correlation>)`.
- Submissions of one batch are applied in order. A node removed by an earlier submission makes a later one fail with `failed(not-found)`.
- Every submission completes exactly once.

| Outcome | Meaning |
|---|---|
| `ok` | The intent was applied. For a call, the typed result is ready to take. |
| `ok-node(node)` | The intent was applied and created this node. |
| `duplicate(original)` | The dedup key was already reserved. Nothing happened. |
| `refused(error)` | A check before the kernel applied the intent failed. Nothing happened. |
| `failed(error)` | The kernel accepted the intent but could not apply it. |
| `ok-epoch(epoch)` | `send-input` on an agent pane was applied. The value is the new prompt epoch. |
| `cancelled` | The submission was cancelled because the plugin stopped, the user cancelled or the host shut down. A sibling of a refused submission is `refused(cancelled)`, not `cancelled`. |

### Batch refusal

A batch is refused as a whole in two cases: a malformed submission (wrong shape, a missing mandatory dedup key, or a dedup key with a disallowed character) and a missing grant. The offending submission completes as `refused(not-permitted)` or `refused(invalid-argument)`. Every other submission of that batch completes as `refused(cancelled)` with the message `batch refused: submission <index>`. Nothing in the batch is applied and no dedup key of the batch is reserved. When both a malformed submission and a missing grant exist, the malformed submission is reported first [RQO-009]. A batch of more than 256 submissions is not a refusal. It is a trap that counts as one failure; no submission is applied and none completes [RQO-010].

## Dedup keys

- **Format.** One to 128 characters from `[A-Za-z0-9._:/-]`. Another character makes the submission malformed.
- **Scope.** The pair of plugin id and key, at the kernel that applies the intent. The server and each client keep separate tables.
- **Window.** 10 minutes from the moment the key is reserved.
- **Reservation.** A key is reserved only after every acceptance check passes. A refused submission does not reserve its key, so a retry with the same key is valid once the cause is fixed. An accepted submission keeps its key even if it later fails, so a retry of a failed action needs a new key.
- **Mandatory.** `send-input` and `agent-action` must carry a key. The kernel journals these keys in the snapshot before the PTY write, so a server restart does not repeat input.
- **Helpers.** Rust `Submission::keyed(intent, key)` sets the key, and `dedup::key_for(&["finish", &node.to_string()])` builds one from parts, joining them with `:`. TypeScript writes the key string into `dedupKey`.

## Intent table

| Intent | Payload | Grant | Outcomes beyond `ok` |
|---|---|---|---|
| `rename-node` | `node`, `title` | `tree.write` | `refused(conflict)` when the user named the node by hand and the plugin is a titler |
| `move-node` | `node`, `target`, `placement` (before, after, inside-first, inside-last) | `tree.write` | `refused(invalid-argument)` when the target is inside the moved node |
| `close-node` | `node` | `tree.write` | `refused(conflict)` on a locked node |
| `set-node-locked` | `node`, `locked: bool` | `tree.write` | `refused(not-found)` |
| `create-pane` | `parent: option`, `title`, `content-kind`, `cwd`, `command`, `initial-input` | `panes.create`, plus `terminal.write` with `initial-input` | `ok-node`; `failed(io)` when spawning fails |
| `signal-pane` | `pane`, `signal` (interrupt, terminate, kill, hangup) | `panes.signal` | `refused(pane-not-ready)` for a restored pane |
| `send-input` | `pane`, `text`, `delivery` (typed, submitted, pasted), `prompt-epoch: option<u64>` | `terminal.write` | `refused(not-found)`; `refused(unsupported)`; `refused(pane-not-ready)`; `refused(rate-limited)` (20 per second per pane); `refused(conflict)` on a stale epoch |
| `agent-action` | `pane`, `action-id` | `agent.control` | `refused(not-found)`; `refused(pane-not-ready)`; `refused(unsupported)`; `refused(conflict)` when the activity is not allowed; `refused(rate-limited)` (20 per second per pane) |
| `set-decoration` | `node`, `slot`, `content`, `icon`, `sort-key`, `persist` | `tree.decorate` | `refused(invalid-argument)` for an undeclared slot |
| `clear-decoration` | `node`, `slot` | `tree.decorate` | none beyond the common codes |
| `set-virtual-rows` | `row-kind`, `rows` | `tree.decorate` | `refused(invalid-argument)` for an undeclared kind |
| `play-sound` | sound id | `sound.play` | `refused(not-found)`; `refused(rate-limited)` above 4 per second |
| `notify` | `title`, `body`, `urgency` (low, normal, high), `sound` | `notify.desktop` | `refused(rate-limited)` above 6 per minute |
| `open-view`, `close-view` | `view-target`, `focus` | `ui.panels` | `refused(not-found)` for an undeclared view |
| `invoke-command` | `command`, `arguments-json` | `commands.invoke` | `refused(conflict)` for a disabled command; `failed(timed-out)` after 30 s |
| `message` | `to`, `channel`, `payload` | `messages.peer` | `refused(not-found)`; `refused(invalid-argument)` above 1 MiB |
| `editor-apply-edits` | `buffer`, `base-revision`, `edits` | `editor.edit` | `refused(conflict)` on a stale revision |
| `editor-open` | `path`, `line`, `placement` (new pane, split, replace) | `editor.edit` | `ok-node`; `failed(io)` |

The intent `message` replaces the removed `messages` host import. Sending is the intent, and receiving is the `message` event.

## Extended intents

Twenty-three further intent cases exist, and `outcome` includes `ok-epoch(u64)`. A batch is limited as before (256 submissions), and `restructure` is atomic inside it.

| Intent | Payload | Grant | Outcome |
|---|---|---|---|
| `freeze-pane`, `unfreeze-pane` | `pane` | `panes.lifecycle` | `ok`; `refused(unsupported)` on Windows; unfreeze of a running pane is `refused(conflict)` |
| `replace-pane-command` | `pane`, `command`, `cwd`, `keep-scrollback` | `panes.lifecycle` | `ok` once the new process runs |
| `terminate-pane-process` | `pane`, `grace-ms` (at most 10,000) | `panes.lifecycle` | `ok` when the process has exited; the pane stays open |
| `resize-pane` | `pane`, `columns`, `rows` | `panes.lifecycle` | `ok`; `refused(conflict)` names the limiting neighbour |
| `create-group`, `create-folder`, `create-project`, `create-split` | parent, title and placement, path, root or 2 to 4 panes | `tree.restructure` | `ok-node` |
| `set-bookmarked` | `node`, `bookmarked` | `tree.write` | `ok` |
| `set-expanded` | `node` or `virtual-row`, `expanded` | `tree.write`, or own `virtual_rows` | `ok` |
| `restructure` | `operations` (at most 256), `label` | `tree.restructure` | `ok-node` with an undo token; all or nothing, `refused(conflict)` names the failing index |
| `revert-restructure` | `token` | `tree.restructure` | `ok`; `refused(conflict)` after a later change |
| `set-inferred-icon` | `node`, `icon`, `source` | `tree.decorate` | `ok`; a manual icon wins with `refused(conflict)` |
| `mark-attention`, `ack-attention` | `node`, `reason` (at most 80) | `tree.attention` | `ok`; `refused(rate-limited)` above 10 per second per node per plugin, the two intents together |
| `open-external` | `target`: url, file or settings | `open-external` | `ok`; `refused(not-permitted)` when the user declines; `http`, `https`, `mailto` only |
| `set-status-segment`, `clear-status-segment` | `segment`, `content`, `tooltip`, `tone` | `ui.chrome` | `ok`; 10 per second per slot (the segment id is the slot) |
| `set-frame-slot`, `clear-frame-slot` | `pane`, `slot`, `content`, `icon`, `tooltip` | `ui.chrome` | `ok`; `refused(invalid-argument)` when the pane does not match `applies_to` |
| `toast` | `id`, `tone`, `title` (60), `body` (200), `actions` (2), `duration-ms` | `notify.toast` | `ok`; 3 per 10 s (`rate-limited`); a shown `id` is replaced |
| `dismiss-toast` | `id` | `notify.toast` | `ok` |

Five base intents take optional fields. A missing field keeps the default behaviour.

| Intent | Optional fields |
|---|---|
| `send-input` | `stages`, `raw`, `mouse`, `if-current`, `lock-user-input`, `journal`. The check order is the dedup key, then `if-current`, then the lock. A success on an agent pane returns `ok-epoch` |
| `set-decoration` | `spans`, `tooltip`, `frames` (2 to 16), `frame-ms` (at least 90), `trailing`, `visibility`, `tone`. Reduced motion shows the first frame |
| `rename-node` | `source` (`plugin`, `inferred`), `expected-revision`. An inferred title never replaces a manual one |
| `notify` | `coalesce-key`, `coalesce-window-ms` (at most 60,000) |
| `set-virtual-rows` | rows gain `parent`, `expandable`, `accepts-drop`, `tooltip`, `label`, `trailing` |

The outcome `ok-epoch(u64)` is covered by `is_ok()` in Rust and `isOk()` in TypeScript, so code that only asks "did it succeed" needs no change.

This listing sends a toast, does an atomic restructure, and reads the epoch.

**Rust**

```rust
out.push(Submission::keyed(Intent::Toast(ToastSpec {
    id: Some("build".into()), tone: Tone::Success, title: "Build finished".into(),
    body: None, actions: vec![ToastAction { label: "Open".into(), command: "open-log".into() }],
    duration_ms: Some(5_000),
}), dedup::key_for(&["toast", "build"])));
out.push(Submission::new(Intent::Restructure(RestructureSpec {
    operations: vec![RestructureOp::CreateGroup(CreateGroupSpec { parent: Some(root), title: "Tests".into(), placement: Placement::InsideLast, children: ids })],
    label: "Group test panes".into(),
})));
// later, in request-completed:
if let Outcome::OkEpoch(epoch) = outcome { self.prompt_epoch = epoch; }
```

**TypeScript**

```ts
out.push({
  intent: { tag: 'toast', val: { id: 'build', tone: 'success', title: 'Build finished',
    body: undefined, actions: [{ label: 'Open', command: 'open-log' }], durationMs: 5000 } },
  correlation: nextCorrelation++, dedupKey: 'toast:build',
});
out.push({
  intent: { tag: 'restructure', val: { label: 'Group test panes', operations: [
    { tag: 'create-group', val: { parent: root, title: 'Tests', placement: 'inside-last', children: ids } }] } },
  correlation: nextCorrelation++, dedupKey: undefined,
});
// later, in request-completed:
if (outcome.tag === 'ok-epoch') promptEpoch = outcome.val;
```

## Worked example in Rust

This titler renames a pane when its shell sets the window title. The dedup key makes a repeated title a no-op for ten minutes.

**Rust**

```rust
use ilium_plugin_sdk::dedup;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::PaneTitleChangedEvent;
use ilium_plugin_sdk::types::intents::RenameNodeSpec;
use ilium_plugin_sdk::types::tree::TitleSource;

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct Titler;

impl Handler for Titler {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            let Event::PaneTitleChanged(PaneTitleChangedEvent { node, title, .. }) = event
            else {
                continue;
            };
            let spec = RenameNodeSpec {
                node,
                title: title.trim().to_string(),
                source: TitleSource::Inferred,
                expected_revision: None,
            };
            let key = dedup::key_for(&["titler", &node.to_string(), spec.title.as_str()]);
            submissions.push(Submission::keyed(Intent::RenameNode(spec), key));
        }
        submissions
    }
}
```

The manifest grants `panes.read` and `tree.write` on the `server` side. A manual title is never overwritten. The kernel refuses such a rename with `refused(conflict)`, and the plugin does not retry it.

## Worked example in TypeScript

This client plugin shows a desktop notice when a pane exits. It logs refused submissions from the completion events.

**TypeScript**

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { log } from '@ilium/plugin-sdk/host/log';

let nextCorrelation = 1n;

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  activate(context) {
    log('info', `started ${context.plugin}`);
  },
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    for (const event of batch) {
      if (event.tag === 'request-completed' && event.val.outcome.tag === 'refused') {
        log('warn', `submission refused: ${event.val.outcome.val.code}`);
        continue;
      }
      if (event.tag !== 'pane-exited') continue;
      const { node, status } = event.val;
      submissions.push({
        intent: {
          tag: 'notify',
          val: {
            title: 'Command finished',
            body: `Pane exited with status ${status ?? 'unknown'}`,
            urgency: 'normal',
            sound: undefined,
          },
        },
        correlation: nextCorrelation++,
        dedupKey: `exit:${node}`,
      });
    }
    return submissions;
  },
});
```

The manifest grants `panes.read` and `notify.desktop` on the `client` side. Every `u64` is a `bigint` in TypeScript, so the correlation counter starts at `1n`.

## Edge cases and errors

- **Declined or revoked grant.** No events of that kind are delivered. A revoked grant stops them from the next batch. The first refused call of each plugin instance writes one warn line naming the grant and the reason, `declined` or `revoked`.
- **Stale state.** `conflict` means the state changed or is held. Re-read the node, then decide. For `editor-apply-edits`, `conflict` always means a stale base revision.
- **Restored panes.** `send-input` and `signal-pane` return `pane-not-ready` until the pane has started its process. Retry after `pane-created` or `tree-changed` for that node.
- **Retries.** A refused submission does not reserve its key, so retry it unchanged. A failed submission needs a new key.
- **Prompt text.** Without `agent.read-prompts`, the plugin gets no `input-submitted` event and nothing derived from prompt text, neither length nor hash.
- **Lost events.** Drop-oldest events that overflow are counted in `events-dropped`. A never-dropped event that overflows the queue restarts the plugin, and the restart counts as a failure.
- **Restart rule.** The first failure in a 10-minute window restarts after 1 s and the second after 5 s. The third failure within the window disables the plugin on that side (see [API-040](/pages/reference-requirements-api-host/)). A manual re-enable resets the count.
- **Batch size.** A `handler` call that returns more than 256 submissions traps. Split the work across batches.

## What the user sees

When a plugin is disabled, the tree footer and Settings > Plugins show:

`Plugin <name> stopped after 3 failures in 10 minutes: <export> <code or trap> (<view>). Turn it on again in Settings > Plugins.`

The view part appears only when the failing export was `views.render`. Refused submissions are written to the plugin log at level warn. The `ilium plugin trace` command shows the last events and the outcome of each submission.

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin trace demo-titler --last 4
seq  time(ms)  kind        detail
 12  000120    event       batch 1: pane-title-changed
 13  000121    submission  rename-node corr=3
 14  000122    outcome     corr=3 ok
 15  006900    outcome     corr=4 duplicate
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The rename completes once. A repeated title with the same key completes as a duplicate and has no effect. The trace format is in <a href="/pages/reference-debugging-tools/">Debugging tools</a>.</figcaption>
</figure>

The trace also shows extended events and a toast submission.

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin trace demo-guard --last 4
seq  time(ms)  kind        detail
 31  000000    event       batch 2: pane-lifecycle-changed,
                           attention-changed
 32  000900    submission  toast corr=8
 33  000901    outcome     corr=8 ok
</pre>
<figcaption>Rendered at 48 columns. Illustrative. A guard plugin answers two events with one toast.</figcaption>
</figure>

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin logs demo-titler
2026-10-11 09:20:44 <span class="t-err">error</span> host   demo-titler
  event queue full: dropped 12 events
2026-10-11 09:20:44 <span class="t-warn">warn</span>  host   demo-titler
  restarting in 1 s (failure 1 of 3 in 10 minutes)
2026-10-11 09:21:52 <span class="t-err">error</span> host   demo-titler
  disabled after 3 failures in 10 minutes
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The plugin restarts after its first two failures and is disabled by the third, which falls within 10 minutes.</figcaption>
</figure>

Requirement ids for this page: [EVT-001], [EVT-002], [EVT-004], [EVT-006], [EVT-010], [EVT-011], [EVT-012], [EVT-013], [EVT-014], [EVT-031], [EVT-040], [EVT-041], [EVT-042], [EVT-043], [EVT-044], [EVT-045], [EVT-046], [EVT-047], [EVT-048], [EVT-049]. See the [requirements index](/pages/reference-requirements/).

### See also

- [Capability table](/pages/reference-capabilities/)
- [Errors and limits](/pages/reference-errors-and-limits/)
- [Events](/pages/api-events/) and [Intents](/pages/api-intents/)
- [Messages and topics](/pages/api-messages-and-topics/)
- [Lifecycle](/pages/lifecycle/) and [Debugging](/pages/debugging/)
- [Performance](/pages/performance/)
