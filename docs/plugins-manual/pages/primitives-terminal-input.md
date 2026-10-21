---
layout: page
title: Terminal input
permalink: /pages/primitives-terminal-input/
---

## Terminal input

Input to a terminal pane is an intent, `send-input`, not a function. The intent lets a plugin reproduce what the built-in features do: send a command in stages with delays, write raw bytes, send mouse events in the protocol the program enabled, refuse to write when the pane changed, hold the user's keys while it types, and journal the write so a restart cannot lose track of it. A `terminal-view` widget can also forward the user's keys to a pane directly.

### You will need

| Grant | Risk | Unlocks |
|---|---|---|
| `terminal.write` | high | `send-input` with `text` and `stages`, `prompt_epoch` |
| `terminal.raw-input` | high | `raw` bytes, `mouse` events, `interactive: true` on `terminal-view` |
| `terminal.input-lock` | high | `lock-input`, `unlock-input`, `lock-user-input` |

See [terminal.write](/pages/capability-terminal-write/) and [Capabilities](/pages/reference-capabilities/). The journal needs no grant for the plugin's own entries.

### Signatures

```wit
lock-input: func(pane: node-id, millis: u16) -> result<_, plugin-error>;   // at most 2,000 ms
unlock-input: func(pane: node-id);

record input-stage { text: string, delay-ms: u16, delivery: delivery }
record terminal-mouse { kind: pointer-kind, button: pointer-button, column: u16, row: u16, modifiers: pointer-modifiers }
record input-precondition { activity: option<agent-activity>, screen-revision: option<u64>, foreground: option<string> }
```

The `send-input` intent takes these optional fields. Without them it sends plain text as described in [Terminal](/pages/primitives-terminal/).

| Field | Type | Rule |
|---|---|---|
| `stages` | `list<input-stage>` | At most 16 stages, total delay at most 10 s. Replaces `text`. Sent in order, each after its `delay-ms`. |
| `raw` | `option<list<u8>>` | Written as given. At most 64 KiB. Needs `terminal.raw-input`. |
| `mouse` | `option<list<terminal-mouse>>` | Encoded by the host in the protocol the program enabled. `unsupported` when `mouse-reporting` is off. |
| `if-current` | `option<input-precondition>` | A mismatch is `refused(conflict)` before any byte is written. |
| `lock-user-input` | `bool` | Hold the user's keys during the stages. The lock lasts as long as the stages, so a batch with `lock-user-input` whose total delay is over 2 s is refused with `invalid-argument`. |
| `journal` | `bool` | Write a journal entry before the PTY write. |

The outcome of a successful write to an agent pane is `ok-epoch(n)`, the new prompt epoch. A plugin that matches only `ok` receives it through the SDK as `ok`.

### Stages and the two-stage submit

Some agent programs lose text that arrives in the same read as the submit key. A signature can declare a two-stage submit, and `stages` lets any plugin do the same by hand: first stage `typed` with the text, a delay of 80 ms, second stage with the submit key. Each stage has its own `delivery`: `typed`, `submitted` or `pasted`. The host sends stages from one queue per pane, so two intents to the same pane never interleave their stages.

### Preconditions

`if-current` makes a write conditional. `activity` must equal the agent activity the host detects, `screen-revision` must equal the revision from `read-cells` or `read-screen`, and `foreground` must equal the name of the foreground process. The check order is the base checks (shape, grant, duplicate key, pane, pane restored, rate, prompt epoch), then `if-current`, then the lock. The refusal is `refused(conflict)` and names the failed condition, and no key is reserved, so the plugin can retry after reading again.

### Input lock

`lock-input` holds the user's keys in a queue for up to 2,000 ms. The keys are not lost: they are delivered after `unlock-input` or when the time runs out, in order. Only one lock per pane exists; a second `lock-input` from another plugin returns `conflict`. The `lock-user-input` field of `send-input` takes the lock for exactly the duration of its stages, so a `send-input` with `lock-user-input` and a total stage delay over 2 s is refused with `invalid-argument`. If another plugin holds the lock when a batch with `lock-user-input` starts, the whole `send-input` is `refused(conflict)` and no stage is sent. When the pane closes or the plugin is disabled, the host drops the stages still queued for that pane, releases the lock the plugin holds and delivers the held user keys; no event is sent. `unlock-input` has no result and no failure: it does nothing when this plugin holds no lock on the pane or when the pane does not exist.

### Journaled writes

With `journal: true`, the host writes a journal entry (kind `input-sent`, with the dedup key) before the PTY write, and completes it with the delivery result. If the server stops between the write and the outcome, the restarted plugin receives `input-uncertain { pane, dedup-key }` instead of a silent loss, and decides whether to send again. Durable timers and the journal are described in [Lifecycle](/pages/lifecycle/).

<figure>
<svg viewBox="0 0 440 150" role="img" aria-label="send-input checks, lock, stages and journal order">
  <rect x="6" y="50" width="70" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="41" y="74" text-anchor="middle" fill="currentColor" font-size="11">intent</text>
  <rect x="96" y="50" width="80" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="136" y="74" text-anchor="middle" fill="currentColor" font-size="11">K checks</text>
  <rect x="196" y="50" width="80" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="236" y="74" text-anchor="middle" fill="currentColor" font-size="11">if-current</text>
  <rect x="296" y="50" width="60" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="326" y="74" text-anchor="middle" fill="currentColor" font-size="11">lock</text>
  <rect x="376" y="50" width="60" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="406" y="74" text-anchor="middle" fill="currentColor" font-size="11">stages</text>
  <path d="M76 70 H96 M176 70 H196 M276 70 H296 M356 70 H376" stroke="currentColor"/>
  <text x="236" y="120" text-anchor="middle" fill="currentColor" font-size="11">journal entry is written before the first PTY write</text>
</svg>
<figcaption>A refusal at any step writes nothing and reserves no dedup key.</figcaption>
</figure>

### Interactive terminal view

The `terminal-view` widget embeds a live pane in a plugin view. With `interactive: true` and `terminal.raw-input`, the view forwards keys and mouse to the pane's program while it has focus; pressing Escape twice leaves it. `scrollback: true` lets the user scroll, and `highlights` names a layer of the plugin that is drawn over the view. See [Media and embedded widgets](/pages/widgets-media-and-embedded/).

### Rust and TypeScript

This example waits for the agent to be idle, types a command in two stages, locks the keyboard for the duration and journals it.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::intents::{Delivery, InputStage, Intent, SendInputSpec, InputPrecondition};

let spec = SendInputSpec {
    pane,
    stages: Some(vec![
        InputStage { text: "/compact".into(), delay_ms: 0, delivery: Delivery::Typed },
        InputStage { text: "\r".into(), delay_ms: 80, delivery: Delivery::Typed },
    ]),
    if_current: Some(InputPrecondition {
        activity: Some(AgentActivity::Idle), screen_revision: None, foreground: None }),
    lock_user_input: true,
    journal: true,
    ..SendInputSpec::default()
};
Submission::keyed(Intent::SendInput(spec), format!("compact-{pane}"))
```

**TypeScript**

```ts
const submission: Submission = {
  intent: { tag: 'send-input', val: {
    pane,
    stages: [
      { text: '/compact', delayMs: 0, delivery: 'typed' },
      { text: '\r', delayMs: 80, delivery: 'typed' },
    ],
    ifCurrent: { activity: 'idle', screenRevision: undefined, foreground: undefined },
    lockUserInput: true,
    journal: true,
  } },
  correlation: 1n,
  dedupKey: `compact-${pane}`,
};
```

A mouse click sent as input, for a program that enabled mouse reporting:

**Rust**

```rust
let click = [PointerKind::Down, PointerKind::Up].map(|kind| TerminalMouse {
    kind, button: PointerButton::Left, column: 12, row: 4, modifiers: PointerModifiers::empty() });
```

**TypeScript**

```ts
const click = (['down', 'up'] as const).map((kind) => ({
  kind, button: 'left', column: 12, row: 4, modifiers: [],
}));
```

### Outcomes, errors and limits

| Case | Result |
|---|---|
| Precondition mismatch | `refused(conflict)`, nothing written |
| Mouse reporting off | `refused(unsupported)` |
| More than 16 stages, delay over 10 s, `raw` over 64 KiB | `refused(invalid-argument)` |
| `lock-input` over 2,000 ms | `invalid-argument` |
| `lock-input` on a missing pane | `not-found` |
| Lock held by another plugin | `conflict` |
| Server stopped mid-write with `journal` | `input-uncertain` after restart |
| Grant missing | `not-permitted` |

The rate of 20 submissions per second per pane per plugin still applies, and so do the dedup key rules.

### What the user sees

While a lock is held, the user's keystrokes in that pane are held back and appear after the write. A staged command appears as it is typed:

<pre class="term">╭──≡ ● · Claude─────────────────────────────────────────────────────────╮
│<span class="t-dim">&gt;</span> /compact                                                             │
╰───────────────────────────────────────────────────────────────────────╯
</pre>

Requirement ids for this page: [PRI-003]. See the [requirements index](/pages/reference-requirements/).

### See also

- [Terminal primitive](/pages/primitives-terminal/)
- [Agent transcript and signals](/pages/primitives-agent-transcript/)
- [Requests and outcomes](/pages/requests-and-outcomes/)
