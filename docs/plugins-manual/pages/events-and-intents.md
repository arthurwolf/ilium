---
layout: page
title: Events and intents
permalink: /pages/events-and-intents/
---

## Events and intents

A plugin learns what happens through events and changes the world through intents. The kernel delivers events in batches of at most 256. The handler answers each batch with a list of submissions, each carrying one intent. The kernel checks the list against the plugin's grants, applies the accepted submissions in order, and reports each outcome as a `request-completed` event in a later batch.

## You will need

- The grants for each event and intent you use. The tables below name them. See [Capabilities](/pages/capabilities/).
- A server or client side. In Rust, `#[ilium_plugin(side = "server")]` or `side = "client"`. In TypeScript, `side: 'server'` or `side: 'client'` in `definePlugin`. A scene plugin also exports `renderer`.
- The `lifecycle` and `handler` exports (and `renderer` for a scene plugin). They are required, so the manifest never lists them in `exports`.
- Rust stable 1.90 or later with the `wasm32-wasip2` target, or Bun 1.2 or later for TypeScript.
- Background from [Worlds and sides](/pages/worlds-and-sides/).

## The handler

The handler is the `Handler` trait in Rust and the `onEvents` function in TypeScript. The kernel calls it with one batch, and it returns the submissions to apply.

| Language | Signature |
|---|---|
| WIT | `on-events: func(batch: list<event>) -> list<submission>;` |
| Rust | `fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission>` (required) |
| TypeScript | `onEvents(batch: Event[]): Submission[]` inside `definePlugin` |

An instance runs one call at a time, so a handler never re-enters itself.

The `on-events` deadline is 20 ms by default for every tier. A manifest may request up to 50 ms, and only a T1 plugin gets above 20 ms (T2 and T3 ceiling: 20 ms). A breach traps the instance, which follows the [restart and disable rule](/pages/reference-errors-and-limits/#restart-and-disable-rule): restart after 1 s, then 5 s, and disable at the third failure within 10 minutes, with a notice in the tree footer. See [Errors and limits](/pages/reference-errors-and-limits/).

## Event catalogue

An event reaches a plugin only when the plugin holds the listed grant. Lazily activated plugins receive events only after activation.

| Event | Payload | Grant | Delivery policy |
|---|---|---|---|
| `tree-changed` | `list<node-id>` | `tree.read` | Coalesced; pending lists merge into one union |
| `node-removed` | `node-id` | `tree.read` | Never dropped |
| `pane-created` | `pane-info` | `panes.read` | Never dropped |
| `pane-exited` | `node`, `status: option<s32>`, `signal: option<string>` | `panes.read` | Never dropped |
| `pane-focused` | `option<node-id>` | `panes.read` | Coalesced |
| `pane-resized` | `node`, `columns`, `rows` | `panes.read` | Coalesced by node |
| `pane-title-changed` | `node`, `title`, `source` (osc, manual, inferred) | `panes.read` | Coalesced by node |
| `agent-identified` | `node`, `signature` | `agent.read` | Never dropped |
| `agent-activity-changed` | `node`, `previous`, `current`, `revision` | `agent.read` | Never dropped |
| `agent-goal-changed` | `node`, `goal: option<string>` | `agent.read` | Coalesced by node |
| `input-submitted` | `node`, `text`, `source` (user, plugin, scheduled, voice), `epoch: u64` | `agent.read-prompts` | Never dropped |
| `output-matched` | `node`, `filter-id`, `line`, `row` | `terminal.output-filter` | Drop oldest, counted |
| `link-activated` | `node`, `detector-id`, `text` | own `link_detectors` | Never dropped |
| `pointer` | `node`, `row`, `column`, `kind` (down, up, move, drag, wheel-up, wheel-down, wheel-left, wheel-right), `button` | `terminal.pointer`, while claimed | Coalesced (move and drag) |
| `editor-opened`, `editor-closed` | `buffer-ref` | `editor.read` | Never dropped |
| `editor-changed` | `buffer`, `revision`, `first-line`, `last-line` | `editor.read` | Coalesced by buffer, debounced 100 ms |
| `editor-saved` | `buffer`, `path` | `editor.read` | Never dropped |
| `settings-changed` | `settings-change { keys, origin, side }` | always (own) | Coalesced |
| `timer-fired` | `timer-id` | always (own) | Coalesced by timer |
| `request-completed` | `request-result` | none (own timers) | Never dropped |
| `job-progress` | `request-id`, `percent` | always (own) | Coalesced by job |
| `message` | `from`, `channel`, `payload` | own sides, or the sender's `messages.peer` | Never dropped |
| `topic-published` | `topic`, `from`, `payload-json` | `topics.subscribe` | Drop oldest, counted |
| `command-invoked` | `command`, `arguments-json`, `origin`, `context` | own commands | Never dropped |
| `ui-action` | `view`, `key`, `action-id`, `kind`, `value` | own views | Never dropped (`scrolled` coalesced) |
| `key-pressed`, `key-released` | `grab`, `pane`, `chord`, `repeat` | `keys.intercept` or `wants_keys` | Never dropped |
| `view-visibility` | `view`, `visible: bool` | own views | Coalesced |
| `fs-changed` | `watch`, `paths: list<string>` | `fs.watch` | Coalesced by watch |
| `audio-frames` | `capture`, `samples: list<s16>` | audio grant | Drop oldest, counted |
| `cli-invoked` | `invocation`, `name`, `argv`, `cwd` | `cli.contribute` | Never dropped |
| `cli-cancelled` | `invocation` | `cli.contribute` | Never dropped |
| `http-called` | `call`, `method`, `path`, `headers`, `body` | `http-api.route` | Never dropped |
| `client-attached`, `client-detached` | `client-id` | server side, always | Never dropped |
| `theme-changed` | none | client side, always | Coalesced |
| `events-dropped` | `list<dropped-count>` | always (own) | Coalesced by (event, source); never dropped |

In Rust the payload records live in `ilium_plugin_sdk::types::events`. In TypeScript an event is a tagged value such as `{ tag: 'pane-exited', val: { node, status, signal } }`.

### Delivery policies

| Policy | Meaning |
|---|---|
| Coalesced | Pending events of one kind and key always merge into one, whatever the queue fill. The merged event keeps the newest payload at the position of its newest occurrence. |
| Never dropped | The event always arrives. |
| Drop oldest, counted | At a full queue the oldest pending drop-oldest event of any kind is removed and counted under its own event and source. If none is pending and the new event is drop-oldest, the new event is discarded and counted. |

When a never-dropped event cannot be queued, the plugin counts as stuck. The kernel restarts it, and the new instance receives a `tree-changed` for every node so that it can re-read its state.

Dropped events are counted. The next batch starts with one `events-dropped` event. At most one is pending: a second drop before the next batch adds to the matching (event, source) count or appends a record, and the event is never dropped. `source` is the filter id, the topic name or the decimal capture id, and none for every other kind. The full rules are in [Events](/pages/api-events/).

## Extended events

The contract adds 18 events after `events-dropped`, so the catalogue holds 56 events. A plugin that does not match them is unaffected. The `ui-action` event gains 6 kinds (17 in total), and `pointer` and `command-invoked` gain fields.

| Event | Payload | Grant | Delivery policy |
|---|---|---|---|
| `screen-changed` | `pane`, `first-row`, `last-row`, `revision` | `terminal.read-cells` | Coalesced by pane, 10 per second |
| `user-line-submitted` | `pane`, `text`, `at` | `agent.read-prompts` | Never dropped |
| `signal-changed` | `pane`, `signal`, `value` | `agent.read` | Coalesced by pane and signal |
| `agent-model-changed` | `pane`, `model` | `agent.read` | Coalesced by pane |
| `editor-pointer` | `buffer`, `pane`, `position`, `pointer` | `editor.read` | Coalesced (move) |
| `editor-viewport-changed` | `editor-viewport` | `editor.read` | Coalesced by pane, 100 ms |
| `http-stream-data`, `http-stream-closed` | `stream`, `status`, `headers`, `chunk`, or `outcome` | the stream's `http` grant | Never dropped, with back-pressure |
| `child-output`, `child-exited` | `child`, `stream`, `bytes`, or `status`, `signal` | `process.long-lived` | Never dropped |
| `pane-lifecycle-changed` | `pane`, `state`, `by` | `panes.read` | Never dropped |
| `attention-changed` | `node`, `marked`, `reason`, `by` | `tree.attention` or `tree.read` | Coalesced by node |
| `client-request` | `call`, `kind`, `payload-json` | client side, own `client_requests` | Never dropped |
| `kernel-settings-changed` | `tab`, `keys` | `settings.read-kernel:<tab>` | Coalesced by tab |
| `keymap-changed` | none | `keymap.read` | Coalesced |
| `durable-timer-fired` | `timer`, `name`, `late-by-millis` | none (own timers) | Never dropped |
| `input-uncertain` | `pane`, `dedup-key` | the grant of the original `send-input` | Never dropped |
| `display-mode-changed` | `mode` | own `display_modes` | Coalesced |

**Rust**

```rust
for event in batch {
    match event {
        Event::DurableTimerFired(fired) if fired.name == "nightly-scan" => {
            log::log(Level::Info, &format!("scan is {} ms late", fired.late_by_millis));
        }
        Event::InputUncertain(uncertain) => {
            // The server restarted before the outcome of a send-input was recorded.
            log::log(Level::Warn, &format!("input to pane {} is uncertain", uncertain.pane));
        }
        _ => {}
    }
}
```

**TypeScript**

```ts
for (const event of batch) {
  if (event.tag === 'durable-timer-fired' && event.val.name === 'nightly-scan') {
    log('info', `scan is ${event.val.lateByMillis} ms late`);
  } else if (event.tag === 'input-uncertain') {
    // The server restarted before the outcome of a send-input was recorded.
    log('warn', `input to pane ${event.val.pane} is uncertain`);
  }
}
```

## Batches, order and queues

- A batch holds at most 256 events. The host splits larger backlogs.
- Each plugin has one queue. The queue holds 1,024 events by default. The ceiling of `event_queue` is 4,096 for T1 and 1,024 for T2 and T3. The host clamps a larger request to the ceiling and logs the clamp [API-043] [EVT-046]. The manifest key `event_queue` requests a value.
- Events arrive in kernel emission order across all kinds. For one node the order is `pane-created`, other pane events, `pane-exited`, then `node-removed`. `agent-identified` always precedes `agent-activity-changed` for the same pane.
- A handler may return at most 256 submissions. More than 256 traps the instance.

The log below shows one batch under load: a counted drop, a refused submission and a trap.

<pre class="term"><span class="t-prompt">$</span> ilium plugin logs example.notes
<span class="t-dim">2026-10-11 12:04:01</span> <span class="t-ok">ok</span>   batch: 14 events, 3 submissions
<span class="t-dim">2026-10-11 12:04:01</span> <span class="t-warn">warn</span> events-dropped: output-matched 2 (drop oldest)
<span class="t-dim">2026-10-11 12:04:02</span> <span class="t-err">err</span>  #2 refused(not-permitted): notify.desktop not granted
<span class="t-dim">2026-10-11 12:04:02</span> <span class="t-err">err</span>  #3 refused(cancelled): batch refused: submission 2
<span class="t-dim">2026-10-11 12:04:03</span> <span class="t-err">err</span>  trap: on-events took 23 ms (deadline 20 ms)
<span class="t-dim">2026-10-11 12:04:03</span> <span class="t-warn">warn</span> restart in 1 s; tree-changed sent for every node
</pre>

Illustrative, not generated. The lines show the log form of a refused batch, a counted drop and a deadline trap.

## Submissions and outcomes

A submission is `record submission { intent: intent, correlation: u64, dedup-key: option<string> }`.

- `intent` is the change to make, such as `set-decoration` or `send-input`.
- `correlation` matches the outcome with the plugin's request. Numbers below 2^63 belong to the plugin. The SDK allocates from 2^63 upward.
- `dedup-key` is the optional key described below.

Some intents are applied by the server and some by the client kernel. A client plugin's submission to the server is forwarded and its outcome comes back to that client only. The table is in [Intents](/pages/api-intents/#which-side-applies-an-intent).

In Rust, `Submission::new(intent)` sets no key and `Submission::keyed(intent, key)` sets one. In TypeScript, write `{ intent, correlation, dedupKey }`, with `correlation` as a `bigint`.

Every submission completes exactly once, as a `request-completed` event whose `request-ref` is `submission(<correlation>)`.

| Outcome | Meaning |
|---|---|
| `ok` | Applied. For calls, the typed result is ready to take with a `take-*` function. It is held for 30 s or until taken. |
| `ok-node(node-id)` | Applied. Returns the node the intent created. |
| `duplicate(request-ref)` | The dedup key was already reserved. Nothing happened. The ref names the submission that reserved it. |
| `refused(plugin-error)` | An acceptance check failed. Nothing was applied, and the key was not reserved. |
| `failed(plugin-error)` | The kernel accepted the submission but could not apply it. The key stays reserved. |
| `cancelled` | Cancelled by the plugin, the user or a shutdown. A sibling of a refused submission in the same batch completes as `refused(cancelled)`, not as `cancelled` [ABI-009]. Submissions accepted but not yet applied at shutdown complete as `cancelled`. |

A take of a slow-call result before completion, a second take, or a take of an unknown id returns `none` [ABI-014]. The table of every slow call and its take function is in [Requests and outcomes](/pages/requests-and-outcomes/#slow-calls). A result is held for 30 s [ABI-012]. Pending requests are limited to 64 per plugin by default and 4,096 per session, and a call over the limit fails with `busy` [ABI-013].

### Batch checks

The kernel runs two batch-level checks over the whole list, in index order, before any effect. Check 1 is shape: a valid variant tag, required fields present, string and list lengths within the ABI caps, a `dedup-key` present where the intent requires one, and every key 1 to 128 characters from the documented set. Check 2 is the grant: requested in the manifest and currently granted. The lowest-index submission that fails (shape before grant inside one submission) completes as `refused(invalid-argument)` for shape or `refused(not-permitted)` for grant. Every other submission completes as `refused(cancelled)`. All of them carry the message `batch refused: submission <index>`, where `<index>` is the 0-based index of that offender. No effect runs and no dedup key is reserved [ABI-009]. All other checks are per submission and affect only that submission: target exists, pane ready, payload value valid (for example a text node id where a pane is needed), rate, quota, conflict and `if-current` [ABI-032]. Accepted submissions are applied in order, so a node removed by an earlier one gives `failed(not-found)` to a later one.

<figure>
<svg viewBox="0 0 420 262" role="img" aria-label="Submission checks from handler return to outcome">
  <defs>
    <marker id="ev-arrow-checks" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
      <path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="20" y="10" width="260" height="34" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="150" y="31" font-size="11" text-anchor="middle" fill="currentColor">Handler returns submissions</text>
  <text x="296" y="31" font-size="10" fill="currentColor">up to 256</text>
  <line x1="150" y1="44" x2="150" y2="62" stroke="currentColor" marker-end="url(#ev-arrow-checks)"/>
  <rect x="20" y="62" width="260" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="150" y="83" font-size="11" text-anchor="middle" fill="currentColor">Batch check: shape and grants</text>
  <text x="296" y="83" font-size="10" fill="currentColor">batch refused</text>
  <line x1="150" y1="96" x2="150" y2="114" stroke="currentColor" marker-end="url(#ev-arrow-checks)"/>
  <rect x="20" y="114" width="260" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="150" y="135" font-size="11" text-anchor="middle" fill="currentColor">Per submission: dedup-key</text>
  <text x="296" y="135" font-size="10" fill="currentColor">duplicate(request-ref)</text>
  <line x1="150" y1="148" x2="150" y2="166" stroke="currentColor" marker-end="url(#ev-arrow-checks)"/>
  <rect x="20" y="166" width="260" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="150" y="187" font-size="11" text-anchor="middle" fill="currentColor">Per submission: target checks</text>
  <text x="296" y="187" font-size="10" fill="currentColor">refused or failed</text>
  <line x1="150" y1="200" x2="150" y2="218" stroke="currentColor" marker-end="url(#ev-arrow-checks)"/>
  <rect x="20" y="218" width="260" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="150" y="239" font-size="11" text-anchor="middle" fill="currentColor">Apply in order, report outcome</text>
  <text x="296" y="239" font-size="10" fill="currentColor">request-completed</text>
</svg>
<figcaption>A batch is checked as a whole, then each submission passes its own checks. Only accepted submissions reach the application step.</figcaption>
</figure>

## Dedup keys

There is one key, `dedup-key` on the submission. The older `idempotency-key` field no longer exists in any intent.

- Format: 1 to 128 characters from `[A-Za-z0-9._:/-]`. Other characters make the submission malformed.
- Scope: the pair of plugin id and key, at the kernel that applies the intent. The server and each client keep separate tables.
- Window: 10 minutes after reservation. For `send-input` and `agent-action` the journal keeps 24 hours or 4,096 entries per session, whichever is smaller, and is written to the snapshot before the input reaches the pane. A server restart does not repeat input.
- Reservation: a key is reserved only after every acceptance check passes. A refused submission keeps its key free for a retry. An accepted one keeps it even if it later fails, so a retry of a failed action needs a new key.

The SDK helper `dedup::key_for(parts: &[&str])` joins its parts with `:` and replaces disallowed characters with `-`. It returns a key such as `finish:42:7`.

For `send-input`, two checks run at batch level before this list (see Batch checks): shape (key present, total text across all stages at most 65,536 bytes, otherwise `refused(invalid-argument)`) and the grant `terminal.write` (otherwise `refused(not-permitted)`). The kernel then checks each submission in this order. The first failing check decides the outcome.

1. Dedup key already reserved. Then `duplicate(request-ref)`.
2. Pane exists and is a terminal pane. Otherwise `refused(not-found)` or `refused(unsupported)`.
3. The pane is restored, which means its process has started. Otherwise `refused(pane-not-ready)`.
4. The rate limit has room. Otherwise `refused(rate-limited)`.
5. `prompt-epoch`, when given, equals the pane's current epoch. Otherwise `refused(conflict)`.
6. `if-current`, when given, matches the pane (activity, screen revision and foreground command). Otherwise `refused(conflict)` with the message `precondition: <field>`.
7. No other plugin holds the input lock of the pane. Otherwise `refused(conflict)` with the message `input-locked`.
8. The input is accepted: the key is reserved, the rate token is taken and the text is written. A refused attempt never takes a rate token [EVT-050].

## Intent catalogue

Every intent needs its grant. The outcome column lists the codes beyond `ok`.

| Intent | Payload | Grant | Outcomes beyond `ok` |
|---|---|---|---|
| `rename-node` | `node`, `title` | `tree.write` | `refused(not-found)`, `refused(conflict)` for a manually named node |
| `move-node` | `node`, `target`, `placement` (before, after, inside-first, inside-last) | `tree.write` | `refused(not-found)`, `refused(invalid-argument)` for a target inside the node |
| `close-node` | `node` | `tree.write` | `refused(not-found)`, `refused(conflict)` for a locked node |
| `set-node-locked` | `node`, `locked` | `tree.write` | `refused(not-found)` |
| `create-pane` | `parent`, `title`, `content-kind`, `cwd`, `command`, `initial-input` | `panes.create` (and `terminal.write` for `initial-input`) | `ok-node`; `refused(not-found)`, `failed(io)` |
| `signal-pane` | `pane`, `signal` (interrupt, terminate, kill, hangup) | `panes.signal` | `refused(not-found)`, `refused(pane-not-ready)` |
| `send-input` | `pane`, `text`, `delivery` (Typed, Submitted, Pasted), `prompt-epoch`, `stages` (at most 16, total delay at most 10 s), `raw` and `mouse` (need `terminal.raw-input`), `if-current`, `lock-user-input` (at most 2 s, needs `terminal.input-lock`), `journal` | `terminal.write` | `refused(not-found)`, `refused(unsupported)`, `refused(pane-not-ready)`, `refused(rate-limited)`, `refused(conflict)` |
| `agent-action` | `pane`, `action-id` | `agent.control` | `refused(not-found)`, `refused(pane-not-ready)`, `refused(unsupported)`, `refused(conflict)`, `refused(rate-limited)` (20 per second per pane per plugin), `failed(code)` |
| `set-decoration` | `node`, `slot`, `content`, `icon`, `sort-key`, `persist` | `tree.decorate` | `refused(not-found)`, `refused(invalid-argument)`, `refused(quota-exceeded)` |
| `clear-decoration` | `node`, `slot` | `tree.decorate` | As `set-decoration` |
| `set-virtual-rows` | `row-kind`, `rows` | `tree.decorate` | `refused(invalid-argument)` for an undeclared row kind |
| `play-sound` | sound id | `sound.play` | `refused(not-found)`, `refused(rate-limited)` (4 per second) |
| `notify` | `title`, `body`, `urgency` (low, normal, high), `sound` | `notify.desktop` | `refused(not-found)`, `refused(rate-limited)` (6 per minute) |
| `open-view`, `close-view` | `view-target`, `focus` | `ui.panels` | `refused(not-found)` |
| `invoke-command` | `command`, `arguments-json` | `commands.invoke` | `refused(not-found)`, `failed(timed-out)` after 30 s |
| `message` | `to`, `channel`, `payload` | `messages.peer:<plugin-id>` | `refused(not-found)`, `refused(invalid-argument)` over 1 MiB |
| `editor-apply-edits` | `buffer`, `base-revision`, `edits` | `editor.edit` | `refused(conflict)` for a stale revision, `refused(invalid-argument)` for overlapping edits |
| `editor-open` | `path`, `line`, `placement` (new pane, split, replace) | `editor.edit`, `fs.read` | `ok-node`; `refused(not-permitted)`, `failed(not-found)`, `failed(io)` |

## Extended intents

The contract adds 23 intents, so the catalogue holds 42. `send-input` and the `ok-epoch` outcome are described in [Requests and outcomes](/pages/requests-and-outcomes/). `restructure` is atomic: all operations apply or none does.

| Family | Intents | Grant |
|---|---|---|
| Pane lifecycle | `freeze-pane`, `unfreeze-pane`, `replace-pane-command`, `terminate-pane-process`, `resize-pane` | `panes.lifecycle` |
| Tree structure | `create-group`, `create-folder`, `create-project`, `create-split`, `restructure`, `revert-restructure` | `tree.restructure` |
| Tree state | `set-bookmarked`, `set-expanded` | `tree.write`, or own `virtual_rows` |
| Tree decoration | `set-inferred-icon` | `tree.decorate` |
| Attention | `mark-attention`, `ack-attention` | `tree.attention` |
| Chrome | `set-status-segment`, `clear-status-segment`, `set-frame-slot`, `clear-frame-slot` | `ui.chrome` |
| Notices | `toast`, `dismiss-toast` | `notify.toast` |
| Desktop | `open-external` | `open-external` |

Five intents carry optional fields: `send-input`, `set-decoration`, `rename-node`, `notify` and `set-virtual-rows`. A missing field keeps the plain behaviour.

**Rust**

```rust
let toast = ToastSpec {
    id: Some("scan-done".into()),
    tone: Tone::Success,
    title: "Scan finished".into(),
    ..Default::default()
};
Submission::keyed(Intent::Toast(toast), "toast:scan-done".to_string())
```

**TypeScript**

```ts
const submission: Submission = {
  intent: { tag: 'toast', val: { id: 'scan-done', tone: 'success', title: 'Scan finished', actions: [] } },
  correlation: 1n,
  dedupKey: 'toast:scan-done',
};
```

## Worked example: Rust

This server plugin puts a badge on each pane whose agent starts working. It needs the `agent.read` and `tree.decorate` grants and one decoration slot named `hello` in the manifest.

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::types::events::AgentActivityEvent;
use ilium_plugin_sdk::types::intents::SetDecorationSpec;
use ilium_plugin_sdk::types::tree::AgentActivity;

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct Hello;

impl Lifecycle for Hello {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        log::log(Level::Info, &format!("hello from {}", context.plugin));
        Ok(())
    }
}

impl Handler for Hello {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            let Event::AgentActivityChanged(AgentActivityEvent { node, current, .. })
                = event else { continue };
            if current != AgentActivity::Working {
                continue;
            }
            submissions.push(Submission::new(Intent::SetDecoration(SetDecorationSpec {
                node,
                slot: "hello".into(),
                content: vec![Span::plain("hi")],
                icon: None,
                sort_key: None,
                persist: false,
            })));
        }
        submissions
    }
}
```

The next snippet keys a `send-input` so that a repeated finish event types the command only once. `pane` comes from an earlier event.

```rust
let key = dedup::key_for(&["finish", "42", "7"]); // "finish:42:7"
let submission = Submission::keyed(
    Intent::SendInput(SendInputSpec {
        pane,
        text: "/compact".into(),
        delivery: Delivery::Submitted,
        prompt_epoch: None,
        stages: Vec::new(),
        raw: None,
        mouse: None,
        if_current: None,
        lock_user_input: false,
        journal: true,
    }),
    key,
);
```

## Worked example: TypeScript

This client plugin answers its own command with a desktop notice. It needs the `notify.desktop` grant. The `command-invoked` event is always delivered to the plugin's own commands.

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import type { Event, Submission } from '@ilium/plugin-sdk/types';
import { log } from '@ilium/plugin-sdk/host/log';

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  activate(context) {
    log('info', `hello from ${context.plugin}`);
  },
  onEvents(batch: Event[]): Submission[] {
    const out: Submission[] = [];
    for (const event of batch) {
      if (event.tag !== 'command-invoked' || event.val.command !== 'com.example.hello/greet') {
        continue;
      }
      out.push({
        intent: {
          tag: 'notify',
          val: { title: 'Hello', body: 'from a plugin', urgency: 'normal', sound: undefined },
        },
        correlation: 1n,
        dedupKey: undefined,
      });
    }
    return out;
  },
});
```

The `correlation: 1n` value is below 2^63, so the plugin owns it. To suppress repeats for 10 minutes, set `dedupKey` to a string such as `'greet:1'`.

## Testing a handler

Rust `FakeHost` (feature `testing`) and TypeScript `createFakeHost()` from `@ilium/plugin-sdk/testing` accept events with `deliver(events)` (a list of events in, `Vec<Submission>` out) and return the submissions. Outcomes come back as `request-completed` events when you call `settle()`. `ilium plugin test` runs every fixture against the built component, compares its expectation file, and exits 3 when one differs. Include a fixture that sends the same finish twice within the window. See [Testing](/pages/testing/) and [Test harness reference](/pages/reference-test-harness/).

## Common mistakes

- Treating an event as a command. Events report what happened. Submit an intent to change something, and expect a refusal when a check fails.
- Keeping state in local variables. An instance can restart at any time, so keep durable state in storage, node metadata or settings.
- Relying on a dropped event. Drop-oldest events can be lost under load. `events-dropped` tells you the count, not the contents.
- Reusing a key after a failure. An accepted key stays reserved, so retry a failed action with a new key.

## See also

- [Requests and outcomes](/pages/requests-and-outcomes/)
- [Retained mode](/pages/retained-mode/)
- [Lifecycle](/pages/lifecycle/)
- [Errors and limits](/pages/reference-errors-and-limits/)
- [Rust SDK](/pages/rust-sdk/) and [TypeScript SDK](/pages/typescript-sdk/)
- [Reference: events and intents](/pages/reference-events-and-intents/)

API version checked: `ilium:plugin@0.1.0`.

Requirements for this page: [Events and intents requirements](/pages/reference-requirements-events-intents/).
