# intent: send-input stages

## The send-input intent in depth

`send-input` writes text, bytes or mouse events into a terminal pane as if the user had typed them. Besides a single piece of text with a delivery mode, it supports staged sends with delays, raw bytes, mouse events, a precondition, an input lock and a crash-safe journal. The kernel still owns the parts that are easy to get wrong: the two-stage submit of each agent signature, bracketed paste, and the prompt journal.

**You need:** `terminal.write` for any `send-input`. Raw bytes and mouse events also need `terminal.raw-input` (high risk). `lock-user-input` needs `terminal.input-lock` (high risk). `agent.read` is needed to read the activity that a precondition compares with. The grants belong in the manifest of the side that submits. The base grant is described in [terminal.write](/pages/capability-terminal-write/); the general rules for intents are in [intents](/pages/api-intents/).

### Records

```wit
// Excerpt of ilium:types/intents in reference-wit
enum delivery { typed, submitted, pasted }
record input-stage { text: string, delay-ms: u16, delivery: delivery }
record terminal-mouse { kind: pointer-kind, button: pointer-button, column: u16, row: u16, modifiers: pointer-modifiers }
record input-precondition { activity: option<agent-activity>, screen-revision: option<u64>, foreground: option<string> }

record send-input-spec {
  pane: node-id,
  text: string,                               // ignored when stages is not empty
  delivery: delivery,
  prompt-epoch: option<u64>,                  // refused(conflict) when stale
  // all optional
  stages: list<input-stage>,                  // at most 16, total delay at most 10 s
  raw: option<list<u8>>,                      // at most 64 KiB, needs terminal.raw-input
  mouse: option<list<terminal-mouse>>,        // needs terminal.raw-input
  if-current: option<input-precondition>,
  lock-user-input: bool,                      // at most 2 s, needs terminal.input-lock
  journal: bool,                              // write a journal entry before the PTY write
}
```

The mandatory key is the `dedup-key` of the submission. It follows the format and window rules in [intents](/pages/api-intents/). `pointer-kind`, `pointer-button` and `pointer-modifiers` are the shared types also used by the `pointer` event: kinds `down`, `up`, `move`, `drag`, `wheel-up`, `wheel-down`, `wheel-left` and `wheel-right`; buttons `none`, `left`, `middle` and `right`; modifiers `shift`, `control` and `alt`.

### Fields and rules

| Field | Rule |
|---|---|
| `stages` | Replaces `text` when present. The host sends the stages in order and waits `delay-ms` before each. A stage uses its own `delivery`. The two-stage submit rule of the pane's signature still applies to a `submitted` stage. At most 16 stages and a total delay of at most 10 s, otherwise `invalid-argument` for the batch. |
| `raw` | Bytes written as they are, with no encoding, no bracketed paste and no submit keys. At most 64 KiB. Needs `terminal.raw-input`. |
| `mouse` | The host encodes each event in the protocol the program enabled (for example SGR 1006). `refused(unsupported)` when mouse reporting is off. Needs `terminal.raw-input`. |
| `if-current` | A precondition. Every field that is set must match, otherwise `refused(conflict)` before any byte is written. The message names the failed field: `activity`, `screen-revision` or `foreground`. |
| `lock-user-input` | Hold the user's keys for the duration of the stages. The lock lasts as long as the stages, so a batch with `lock-user-input` whose total delay is over 2 s is refused with `invalid-argument`. Keys the user types meanwhile are queued, not lost. |
| `journal` | Write the journal entry before the PTY write. See "Crash safety" below. |

The text of a stage, and `text` itself, is at most 64 KiB.

### Check order

The kernel runs these checks in order. Steps 1 and 2 run at batch level over the whole list. The others are per submission. The same order, without the `mouse` step, is in [intents](/pages/api-intents/#send-input-check-order).

1. Shape, key present, sizes, stage count, total delay, `raw` size and the total text across all stages (at most 65,536 bytes). Otherwise the batch is refused with `invalid-argument`.
2. Grants: `terminal.write`, then `terminal.raw-input` if `raw` or `mouse` is set, then `terminal.input-lock` if `lock-user-input` is set. Otherwise the batch is refused with `not-permitted`.
3. Dedup key. A reserved key completes as `duplicate`.
4. The pane exists and is a terminal pane (`not-found`, `unsupported`).
5. The pane is restored (`pane-not-ready`).
6. The rate limit has room: 20 per second per pane and plugin (`rate-limited`).
7. `prompt-epoch`, if given, equals the pane's epoch (`conflict`).
8. `mouse`: reporting is enabled (`unsupported`).
9. `if-current` matches (`conflict`, message `precondition: <field>`).
10. The lock is free: another plugin holding the input lock of the pane gives `conflict` with the message `input-locked`.
11. The input is accepted: the key is reserved, the rate token is taken, the journal entry is written when requested, and the stages are sent. A refused attempt never takes a rate token.

<figure>
<svg viewBox="0 0 420 170" role="img" aria-label="Timeline of a two-stage send with an input lock">
  <defs>
    <marker id="sendinput-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
      <path d="M 0 0 L 10 5 L 0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <text x="8" y="16" font-size="11" fill="currentColor">Time</text>
  <line x1="40" y1="16" x2="410" y2="16" stroke="currentColor" marker-end="url(#sendinput-arrow)"/>
  <rect x="40" y="34" width="330" height="22" rx="4" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
  <text x="46" y="49" font-size="10" fill="currentColor">input lock: user keys are queued (at most 2 s)</text>
  <rect x="40" y="70" width="90" height="26" rx="4" fill="none" stroke="currentColor"/>
  <text x="85" y="87" font-size="10" text-anchor="middle" fill="currentColor">stage 1 "/compact"</text>
  <text x="180" y="87" font-size="10" text-anchor="middle" fill="currentColor">delay 120 ms</text>
  <rect x="230" y="70" width="140" height="26" rx="4" fill="none" stroke="currentColor"/>
  <text x="300" y="87" font-size="10" text-anchor="middle" fill="currentColor">stage 2 submit (CR)</text>
  <text x="40" y="118" font-size="10" fill="currentColor">checked before the first byte: grants, key, epoch, if-current, lock</text>
  <text x="40" y="136" font-size="10" fill="currentColor">journal entry written before stage 1 when journal is true</text>
  <text x="40" y="154" font-size="10" fill="currentColor">outcome: ok, or ok-epoch(n) on an agent pane</text>
</svg>
<figcaption>Stages, delays and the lock run inside the kernel. The plugin is not called between stages.</figcaption>
</figure>

### Example: compact an agent, only when it is idle

The plugin sends a slash command in two stages, waits for the agent to be idle, and holds the user's keys while the stages run. The `if-current` precondition prevents the text from landing in a running turn.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::intents::{InputPrecondition, InputStage, SendInputSpec};
use ilium_plugin_sdk::types::intents::Delivery;
use ilium_plugin_sdk::types::tree::AgentActivity;

fn compact(pane: NodeId, attempt: u32) -> Submission {
    let spec = SendInputSpec {
        pane,
        text: String::new(),
        delivery: Delivery::Typed,
        prompt_epoch: None,
        stages: vec![
            InputStage { text: "/compact".into(), delay_ms: 0, delivery: Delivery::Typed },
            InputStage { text: String::new(), delay_ms: 120, delivery: Delivery::Submitted },
        ],
        raw: None,
        mouse: None,
        if_current: Some(InputPrecondition {
            activity: Some(AgentActivity::Idle),
            screen_revision: None,
            foreground: None,
        }),
        lock_user_input: true,
        journal: true,
    };
    Submission::keyed(Intent::SendInput(spec), format!("compact-{pane}-{attempt}"))
}
```

**TypeScript**

```ts
import type { Submission } from '@ilium/plugin-sdk/types';

function compact(pane: bigint, attempt: number): Submission {
  return {
    intent: {
      tag: 'send-input',
      val: {
        pane, text: '', delivery: 'typed', promptEpoch: undefined,
        stages: [
          { text: '/compact', delayMs: 0, delivery: 'typed' },
          { text: '', delayMs: 120, delivery: 'submitted' },
        ],
        raw: undefined, mouse: undefined,
        ifCurrent: { activity: 'idle', screenRevision: undefined, foreground: undefined },
        lockUserInput: true,
        journal: true,
      },
    },
    correlation: 1n,
    dedupKey: `compact-${pane}-${attempt}`,
  };
}
```

### Example: raw bytes and a mouse click

Send the escape sequence for "cursor up", then click at column 12, row 3. The program must have mouse reporting on, or the click is refused with `unsupported`. The check of the modes uses the `terminal.modes` read of the [terminal primitives](/pages/primitives-terminal/).

**Rust**

```rust
let spec = SendInputSpec {
    pane,
    text: String::new(),
    delivery: Delivery::Typed,
    prompt_epoch: None,
    stages: vec![],
    if_current: None,
    lock_user_input: false,
    journal: false,
    raw: Some(vec![0x1b, b'[', b'A']),
    mouse: Some(vec![
        TerminalMouse { kind: PointerKind::Down, button: PointerButton::Left, column: 12, row: 3, modifiers: PointerModifiers::empty() },
        TerminalMouse { kind: PointerKind::Up,   button: PointerButton::Left, column: 12, row: 3, modifiers: PointerModifiers::empty() },
    ]),
};
```

**TypeScript**

```ts
const val = {
  pane, text: '', delivery: 'typed' as const, promptEpoch: undefined, stages: [],
  raw: new Uint8Array([0x1b, 0x5b, 0x41]),
  mouse: [
    { kind: 'down', button: 'left', column: 12, row: 3, modifiers: [] },
    { kind: 'up', button: 'left', column: 12, row: 3, modifiers: [] },
  ],
  ifCurrent: undefined, lockUserInput: false, journal: false,
};
```

### Input lock

`terminal.lock-input(pane, millis)` and `unlock-input(pane)` (see [terminal primitives](/pages/primitives-terminal/)) hold the user's keys on their own. `lock-user-input` on `send-input` does the same for exactly the duration of the stages and releases the lock when the last stage is sent or the intent fails. Only one lock exists per pane. A second plugin asking for it gets `conflict`. A requested duration over 2,000 ms is `invalid-argument`. Queued keys are delivered to the program in order after the unlock, so nothing the user typed is lost.

### Crash safety: journal and input-uncertain

With `journal: true` the kernel writes a pane journal entry (kind `input-sent`, with the dedup key and the delivery result) before the PTY write. A `send-input` on an agent pane and `agent-action` always journal their keys, from Phase 5. If the server stops between the write and the recorded outcome, the restarted plugin receives `input-uncertain { pane, dedup-key }` once, and the kernel never resends silently. The plugin reads the screen, decides whether the text arrived, and resends under a new key if it did not. In the sketches below, `screen_shows` and `screenShows` stand for the plugin's own helper that reads cells and looks for the text.

**Rust**

```rust
Event::InputUncertain(InputUncertainEvent { pane, dedup_key }) => {
    // read the visible rows with terminal::read_cells (terminal.read-cells), decide, then:
    if !self.screen_shows(pane, "/compact") {
        out.push(compact(pane, 2));   // a new attempt number gives a new key: the old key is reserved and would complete as duplicate
    }
    log::log(Level::Warn, &format!("input {dedup_key} on {pane} was uncertain"));
}
```

**TypeScript**

```ts
if (event.tag === 'input-uncertain') {
  // read the visible rows with readCells (terminal.read-cells), decide, then:
  if (!screenShows(event.val.pane, '/compact')) out.push(compact(event.val.pane, 2));   // new attempt number, new key
  log('warn', `input ${event.val.dedupKey} on ${event.val.pane} was uncertain`);
}
```

### The ok-epoch outcome

On an agent pane the outcome is `ok-epoch(n)`, where `n` is the new prompt epoch. Store it, and pass it later as `prompt-epoch` to refuse a send that raced with another prompt. A plugin that tests for `ok` only keeps working through `is_ok()` or `isOk()`.

### Staged signature actions

An agent signature in the manifest may declare an action as stages with the same rules, so `agent-action` uses them too:

```json
{ "actions": { "compact": { "stages": [
  { "keys": "/compact", "delay_ms": 0 },
  { "keys": "\r", "delay_ms": 120 } ] } } }
```

### Errors and limits

| Condition | Result |
|---|---|
| More than 16 stages, total delay over 10 s, `raw` over 64 KiB, `lock-user-input` with a total stage delay over 2 s | batch refused, `invalid-argument` |
| `raw`, `mouse` or lock without its grant | batch refused, `not-permitted` |
| Restored pane that has not started | `refused(pane-not-ready)` |
| Mouse reporting off | `refused(unsupported)` |
| `if-current` mismatch, stale epoch or held lock | `refused(conflict)`; the message names the failed precondition |
| More than 20 sends per second per pane and plugin | `refused(rate-limited)` with `retry-after-ms` |
| Server stops during a journaled send | `input-uncertain` after restart, no replay |

### What the user sees

While the lock is held the user's keys wait, and a typed line appears after the stages finish. The stages appear in the terminal as typing, with the pauses of `delay-ms`. The tree shows the lock glyph from the client icon set on the pane for the duration.

<pre class="term"><span class="t-dim">▼ 📂 Ilium</span>
<span class="t-hi">›   🦀 🔒 ⠋ Fix auth</span>
<span class="t-dim">›   🖥️    Shell</span>

<span class="t-dim">pane 7 (claude)  send-input  compact-7-1</span>
<span class="t-ok">0 ms     stage 1 typed   "/compact"</span>
<span class="t-ok">120 ms   stage 2 submit  CR</span>
<span class="t-ok">121 ms   outcome         ok-epoch(5)</span>
</pre>

Illustrative, not generated: the tree row uses glyphs from the client icon inventory (agent, lock, working spinner); the log lines are an example of the layout, not a capture.

### See also

- [Intents](/pages/api-intents/) for batches, dedup keys and the other intents.
- [Events](/pages/api-events/) for `input-uncertain` and `request-completed`.
- [Terminal primitives](/pages/primitives-terminal/) and [agent primitives](/pages/primitives-agent/) for `modes`, `read-cells`, `lock-input` and the activity states.
- [terminal.write](/pages/capability-terminal-write/) and [agent.control](/pages/capability-agent-control/).
