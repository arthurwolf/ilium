---
layout: page
title: Scheduler
permalink: /pages/core-plugin-scheduler/
---

## Scheduler

Scheduler is the plugin form of Ilium's delayed and queued input. It sends text to a pane after a countdown, even after a restart. It holds a prompt queue that delivers one prompt each time an agent finishes a turn. It sends multi-stage input that cannot interleave with your typing, and it retries safely when a delivery is uncertain. Its id is `ilium.core.scheduler` and it runs on both sides: the client side draws the schedule dialog and the queue dialog, and the server side owns the timers and the writes to the terminal.

Writing into a pane is a kernel service. The plugin only submits the `send-input` intent, and the kernel checks the grants, the dedup key and the preconditions before one byte reaches the PTY.

### What it covers

- **Scheduled input.** The menu entry is "Hit key(s) X time from now". A countdown of hours, minutes and seconds ends in a send of the text, then Enter. A pane has at most one pending schedule; a new one replaces the old one. It survives a restart (Scheduled input in the automation guide).
- **Prompt queue.** The menu entry is "Queue prompt...". One queued prompt is sent each time the agent finishes a turn, which is a Working to Idle or Done transition, not whenever it is idle (Prompt queue).

### See it

<figure><img src="/assets/demos/04-scheduled-input.gif" alt="A five-second countdown sends ping once to an echo-only local fixture" loading="lazy"><figcaption>Current Ilium sends ping once after a five-second countdown. The receiving terminal is a labelled local echo-only fixture.</figcaption></figure>

<figure><img src="/assets/demos/06-prompt-queue.gif" alt="Two prompts are queued for a local Codex-shaped fixture and sent in order after each turn; no model is used and the output is synthetic" loading="lazy"><figcaption>Current Ilium sends two queued prompts in order. A local Codex-shaped fixture supplies synthetic output; no model is used.</figcaption></figure>

### What the user sees

The first render is the schedule dialog **Schedule input for _pane name_**, with the countdown fields, the payload and the Send Enter switch. The second is the dialog **Queue prompt after agent finishes**, with the prompt text, the delivery mode and the repeat count. This is the baseline user interface; a calendar mode is not part of it.

<figure>
<pre class="term">╭─ Schedule input for claude-api ──────────────╮
│                                              │
│ Hours ……………………………………………………………………………………………… 2 │
│ Minutes ………………………………………………………………………………………… 0 │
│ Seconds ……………………………………………………………………………………… 30 │
│ Payload ……………………………………………………………………… npm test │
│ Send Enter …………………………………………………………………………… [x] │
│                                              │
│<span class="t-dim"> Ctrl+Enter schedule · Esc cancel</span>             │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The dialog opens from the menu entry "Hit key(s) X time from now" of a terminal pane. The countdown starts at 30 seconds.</figcaption>
</figure>

<figure>
<pre class="term">╭─ Queue prompt after agent finishes ──────────╮
│<span class="t-hi"> Run the full test suite and report</span>           │
│                                              │
│                                              │
│ Delivery ………………………………………………… ‹ Run X times › │
│ Times ……………………………………………………………………………………………… 2 │
│                                              │
│<span class="t-dim"> Ctrl+Enter enqueue · Esc cancel</span>              │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The dialog opens from "Queue prompt..." of a terminal pane. Enter inserts a new line in the text area; Ctrl+Enter enqueues.</figcaption>
</figure>

### Behaviour contract

Every value below is the baseline value. A calendar mode or a queue panel with edit and move keys is not parity; if a build adds one it is listed as "New in the plugin build, not parity" and has no row here.

| Id | Rule | Exact value or text | Baseline source |
|---|---|---|---|
| scheduler-B01 | Menu entry for scheduled input | "Hit key(s) X time from now", on terminal panes only (shell or agent); editors and boards do not have it | `src/docs/automation.md#scheduled-input` |
| scheduler-B02 | Dialog fields | Hours: any whole number, blank is 0. Minutes: 0 to 59, blank is 0. Seconds: 0 to 59, initial 30. Payload: single line, any Unicode. Send Enter: on by default | `src/docs/automation.md#scheduled-input` |
| scheduler-B03 | Dialog keys | Tab and Shift+Tab move between fields, Space toggles Send Enter, Ctrl+Enter schedules, Esc cancels | `src/docs/automation.md#scheduled-input` |
| scheduler-B04 | Zero delay | Refused: "Choose a delay of at least one second" | `src/docs/automation.md#scheduled-input` |
| scheduler-B05 | Empty payload with Send Enter off | Refused: "Enter text, enable Send Enter, or use both" | `src/docs/automation.md#scheduled-input` |
| scheduler-B06 | Minutes or seconds above 59 | Refused with a message naming the field. A non-digit typed in a number field is ignored | `src/docs/automation.md#scheduled-input` |
| scheduler-B07 | Pending schedules per pane | One. A new schedule replaces the old one | `src/docs/automation.md#scheduled-input` |
| scheduler-B08 | Text and Enter | The text is written, then Enter as a separate step, while the pane input gate is held so typed input cannot interleave. Text only writes no Enter. Enter only sends a single Enter | `src/docs/automation.md#scheduled-input` |
| scheduler-B09 | Deadline passes | The scheduler waits for the input gate and, when the working directory of the pane is missing, retries after workspace recovery. It does not check whether the agent is idle | `src/docs/automation.md#scheduled-input` |
| scheduler-B10 | Restart | The deadline survives a restart. The pane closing discards the schedule | `src/docs/automation.md#scheduled-input` |
| scheduler-B11 | Queue entry | Menu entry "Queue prompt..."; dialog "Queue prompt after agent finishes". Enter inserts a new line; Ctrl+Enter enqueues | `src/docs/automation.md#prompt-queue` |
| scheduler-B12 | Delivery modes | Once; Run X times (count field default 2, a positive whole number); Enqueue forever, marked DANGER | `src/docs/automation.md#prompt-queue` |
| scheduler-B13 | Warning text of Enqueue forever | "DANGER: this will re-send forever whenever the agent finishes. Do not run it unmonitored." | `src/docs/automation.md#prompt-queue` |
| scheduler-B14 | Empty prompt | Refused: "Write a prompt before enqueueing it" | `src/docs/automation.md#prompt-queue` |
| scheduler-B15 | Clear | Menu entry "Clear prompt queue", shown only while the queue is not empty | `src/docs/automation.md#prompt-queue` |
| scheduler-B16 | Delivery trigger | One prompt per finished turn (Working to Idle or Done), not a timer | `src/docs/automation.md#prompt-queue` |
| scheduler-B17 | Order | First in, first out. A repeating entry goes back to the tail with its count decreased | `src/docs/automation.md#prompt-queue` |
| scheduler-B18 | Capacity | At most 100 entries per pane | `src/docs/automation.md#prompt-queue` |
| scheduler-B19 | Fenced delivery | Before writing, the server records that it is about to deliver the head and saves the session. A prompt that may have reached the PTY is not replayed after a crash | `src/docs/automation.md#prompt-queue` |
| scheduler-B20 | Record | Each delivery is recorded in the agent debug log as a queued prompt | `src/docs/automation.md#prompt-queue` |
| scheduler-B21 | Storage | Snapshot section `ilium.core.scheduler`, holding `scheduled_input` and `prompt_queue` per pane (see [config and data migration](/pages/reference-config-and-data-migration/)) | `ilium-core/src/lib.rs` |

### Manifest

This block is a fragment. It lists the contributions. The whole manifest is in [Complete manifest](#complete-manifest).

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.scheduler",
  "capabilities": {
    "server": ["terminal.write", "terminal.raw-input", "terminal.input-lock",
               "agent.read", "journal.read",
               "extensions.provide:ilium.core.scheduler/schedule-sources",
               "extensions.provide:ilium.core.scheduler/before-send-scheduled"],
    "client": ["ui.panels"]
  },
  "contributes": {
    "extension_points": [
      { "id": "schedule-sources", "kind": "collect", "version": "0.1.0" },
      { "id": "before-send-scheduled", "kind": "hook", "mode": "veto", "version": "0.1.0" }
    ]
  }
}
```

`terminal.raw-input` is a high-risk grant, and the user confirms it at install.

### Complete manifest

This is the whole manifest of the plugin at `ilium:plugin@0.1.0`. The Manifest section above shows the contributions in detail; here the required keys, the sides, the `capabilities` per side and the `activation` triggers are all present. Activation follows [startup and lazy activation](/pages/reference-kernel-and-boot/#startup-and-lazy-activation-of-the-30-core-plugins). [COR-050]

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.scheduler",
  "name": "Scheduler",
  "version": "0.1.0",
  "publisher": "Ilium",
  "description": "Scheduler is the plugin form of Ilium's delayed and queued input.",
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
      "terminal.write",
      "terminal.raw-input",
      "terminal.input-lock",
      "agent.read",
      "journal.read",
      "extensions.provide:ilium.core.scheduler/schedule-sources",
      "extensions.provide:ilium.core.scheduler/before-send-scheduled"
    ],
    "client": [
      "ui.panels"
    ]
  },
  "contributes": {
    "extension_points": [
      {
        "id": "schedule-sources",
        "kind": "collect",
        "title": "Trigger sources",
        "entry_schema": {
          "type": "object",
          "properties": {
            "id": { "type": "string" },
            "title": { "type": "string" },
            "service": { "type": "string" }
          },
          "required": ["id", "title", "service"]
        },
        "risk": "standard",
        "max_entries": 64,
        "dynamic": false,
        "version": "0.1.0"
      },
      {
        "id": "before-send-scheduled",
        "kind": "hook",
        "mode": "veto",
        "title": "Before a scheduled or queued input is sent",
        "payload_schema": {
          "type": "object",
          "properties": {
            "pane": { "type": "string" },
            "text_length": { "type": "integer" },
            "schedule": { "type": "string" },
            "source": { "type": "string" }
          },
          "required": ["pane", "text_length", "schedule", "source"]
        },
        "risk": "high",
        "deadline_ms": 20,
        "max_handlers": 8,
        "version": "0.1.0"
      }
    ],
    "panels": [
      { "id": "scheduler.queue", "title": "Scheduler queue", "modal": false }
    ]
  },
  "activation": [
    "startup",
    "view:scheduler.queue"
  ],
  "limits_tier": "first-party"
}
```

### Samples

#### 1. Schedule input for a pane

Text is sent when a countdown ends, even after a restart. The client draws three `number` rows for hours, minutes and seconds, a `text-input` for the payload and a `toggle` for Send Enter, all from [input widgets](/pages/widgets-input/). The state is kept in the snapshot section `ilium.core.scheduler`. The server arms a durable timer with [durable timers](/pages/api-durable-timers/) and [clock and timers](/pages/api-clock-and-timers/), and sends on `durable-timer-fired` with the `send-input` [intent](/pages/api-intent-send-input/). See [events](/pages/api-events/).

**Rust**

```rust
use ilium_plugin_sdk::host::{clock, timers};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::intents::SendInputSpec;
use ilium_plugin_sdk::types::intents::Delivery;

// server manifest: "capabilities": { "server": ["terminal.write"] }
// The client view draws the dialog fields:
//   tree.number("Hours", 0.0).key("hours").minimum(0.0)
//   tree.number("Minutes", 0.0).key("minutes").minimum(0.0).maximum(59.0)
//   tree.number("Seconds", 30.0).key("seconds").minimum(0.0).maximum(59.0)
//   tree.text_input("Payload", String::new()).key("payload")
//   tree.toggle("Send Enter", true).key("send-enter")
impl Scheduler {
    /// One pending schedule per pane: the timer is named `send:{pane}`, and a
    /// durable timer with the same name replaces the earlier one.
    fn arm(&mut self, pane: NodeId, form: &ScheduleForm) -> Result<(), String> {
        let delay_seconds = (form.hours * 60 + form.minutes) * 60 + form.seconds;
        if delay_seconds == 0 {
            return Err("Choose a delay of at least one second".into());
        }
        if form.payload.is_empty() && !form.send_enter {
            return Err("Enter text, enable Send Enter, or use both".into());
        }
        let name = format!("send:{pane}");
        let due_epoch_ms = clock::now_unix_millis() + delay_seconds * 1000;
        timers::at(due_epoch_ms, &name).map_err(|e| e.to_string())?;
        self.pending.insert(pane, (form.payload.clone(), form.send_enter)); // snapshot section
        Ok(())
    }

    fn on_event(&mut self, event: Event) -> Vec<Submission> {
        let Event::DurableTimerFired(fired) = event else { return Vec::new() };
        let Some(pane) = self.pane_of_timer(&fired.name) else { return Vec::new() };
        let Some((text, send_enter)) = self.pending.remove(&pane) else { return Vec::new() };
        // A timer that came due while Ilium was stopped fires once, late.
        vec![Submission::keyed(Intent::SendInput(plain_send(pane, text, send_enter)), fired.name)]
    }
}

fn plain_send(pane: NodeId, text: String, send_enter: bool) -> SendInputSpec {
    // Submitted types the text, then sends Enter as a separate stage.
    let delivery = if send_enter { Delivery::Submitted } else { Delivery::Typed };
    SendInputSpec {
        pane, text, delivery, prompt_epoch: None, stages: vec![],
        raw: None, mouse: None, if_current: None, lock_user_input: false, journal: true,
    }
}
```

**TypeScript**

```ts
import type { Event, Submission } from '@ilium/plugin-sdk';
import { nowUnixMillis } from '@ilium/plugin-sdk/host/clock';
import * as timers from '@ilium/plugin-sdk/host/timers';
import { number, textInput, toggle } from '@ilium/plugin-sdk/ui/build';

// server manifest: "capabilities": { "server": ["terminal.write"] }
// The client view draws the dialog fields:
const hours = number('Hours', 0, { key: 'hours', minimum: 0 });
const minutes = number('Minutes', 0, { key: 'minutes', minimum: 0, maximum: 59 });
const seconds = number('Seconds', 30, { key: 'seconds', minimum: 0, maximum: 59 });
const payload = textInput('Payload', '', { key: 'payload' });
const sendEnter = toggle('Send Enter', true, { key: 'send-enter' });

const pending = new Map<bigint, { text: string; sendEnter: boolean }>();

// One pending schedule per pane: the durable timer is named `send:{pane}`
// and a timer with the same name replaces the earlier one.
export function arm(pane: bigint, form: { hours: number; minutes: number; seconds: number;
  payload: string; sendEnter: boolean }): string | undefined {
  const delaySeconds = (form.hours * 60 + form.minutes) * 60 + form.seconds;
  if (delaySeconds === 0) return 'Choose a delay of at least one second';
  if (form.payload === '' && !form.sendEnter) return 'Enter text, enable Send Enter, or use both';
  timers.at(nowUnixMillis() + BigInt(delaySeconds) * 1000n, `send:${pane}`);
  pending.set(pane, { text: form.payload, sendEnter: form.sendEnter }); // snapshot section
  return undefined;
}

export function onEvent(event: Event): Submission[] {
  if (event.tag !== 'durable-timer-fired') return [];
  const pane = BigInt(event.val.name.slice('send:'.length));
  const job = pending.get(pane);
  if (job === undefined) return [];
  pending.delete(pane);
  // A timer that came due while Ilium was stopped fires once, late.
  return [{
    intent: {
      tag: 'send-input',
      val: {
        pane, text: job.text, delivery: job.sendEnter ? 'submitted' : 'typed', promptEpoch: undefined,
        stages: [], raw: undefined, mouse: undefined, ifCurrent: undefined,
        lockUserInput: false, journal: true,
      },
    },
    correlation: 1n,
    dedupKey: event.val.name,
  }];
}
```

#### 2. Prompt queue

Prompts wait in a list and one goes out each time the agent finishes a turn. The dialog "Queue prompt after agent finishes" collects the text, the delivery mode (once, run X times, forever) and the count. The server watches `agent-activity-changed` (see [agent primitives](/pages/primitives-agent/) and [events](/pages/api-events/)) and, on a Working to Idle or Done transition, submits `send-input` with `delivery: submitted` for the head of the queue. A repeating entry goes to the tail with its count reduced. The queue holds at most 100 entries per pane.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::intents::SendInputSpec;
use ilium_plugin_sdk::types::intents::Delivery;
use ilium_plugin_sdk::types::tree::AgentActivity;

// server manifest: "capabilities": { "server": ["terminal.write", "agent.read"] }
// client dialog: tree.text_area(..).key("prompt"), a delivery select
// (Once, Run X times, Enqueue forever) and tree.number("Times", 2.0).key("times")
const QUEUE_LIMIT: usize = 100;

impl Handler for Scheduler {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            let Event::AgentActivityChanged(change) = event else { continue };
            let finished_turn = matches!(change.previous, AgentActivity::Working)
                && matches!(change.current, AgentActivity::Idle | AgentActivity::Done);
            let Some(queue) = self.queues.get_mut(&change.node) else { continue };
            if !finished_turn || queue.is_empty() {
                continue;
            }
            // Fenced delivery: record the head as "about to deliver" and save the
            // snapshot section before the send, so a crash never replays it.
            let mut entry = queue.remove(0);
            let text = entry.text.clone();
            if entry.repeats_again() && queue.len() < QUEUE_LIMIT {
                queue.push(entry); // goes to the tail with its count decreased
            }
            submissions.push(Submission::keyed(
                Intent::SendInput(SendInputSpec {
                    pane: change.node, text, delivery: Delivery::Submitted,
                    prompt_epoch: None, stages: vec![], raw: None, mouse: None,
                    if_current: None, lock_user_input: false, journal: true,
                }),
                format!("queue:{}:{}", change.node, change.revision),
            ));
        }
        submissions
    }
}
```

**TypeScript**

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';

// server manifest: "capabilities": { "server": ["terminal.write", "agent.read"] }
// client dialog: textArea for the prompt, a delivery select and a number row.
interface QueueEntry { text: string; mode: 'once' | 'times' | 'forever'; remaining: number }
const QUEUE_LIMIT = 100;
const queues = new Map<bigint, QueueEntry[]>();

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    for (const event of batch) {
      if (event.tag !== 'agent-activity-changed') continue;
      const change = event.val;
      const finishedTurn = change.previous === 'working'
        && (change.current === 'idle' || change.current === 'done');
      const queue = queues.get(change.node);
      if (!finishedTurn || queue === undefined || queue.length === 0) continue;
      // Fenced delivery: save the snapshot section before the send.
      const entry = queue.shift()!;
      if (entry.mode === 'times') entry.remaining -= 1;
      if ((entry.mode === 'forever' || (entry.mode === 'times' && entry.remaining > 0))
          && queue.length < QUEUE_LIMIT) queue.push(entry);
      submissions.push({
        intent: {
          tag: 'send-input',
          val: {
            pane: change.node, text: entry.text, delivery: 'submitted',
            promptEpoch: undefined, stages: [], raw: undefined, mouse: undefined,
            ifCurrent: undefined, lockUserInput: false, journal: true,
          },
        },
        correlation: BigInt(submissions.length + 1),
        dedupKey: `queue:${change.node}:${change.revision}`,
      });
    }
    return submissions;
  },
});
```

#### 3. Multi-stage input without interleaving

A slash command and its argument arrive together. The plugin submits `send-input` with `stages` and `lock-user-input`, so typed keys queue until the stages finish. See [send-input stages](/pages/api-intent-send-input/), [terminal.input-lock](/pages/capability-terminal-input-lock/) and [terminal.raw-input](/pages/capability-terminal-raw-input/).

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::intents::{InputStage, SendInputSpec};
use ilium_plugin_sdk::types::intents::Delivery;

// manifest: "capabilities": { "server": ["terminal.write",
//   "terminal.raw-input", "terminal.input-lock"] }
/// A slash command and its argument arrive together. Keys the user types
/// meanwhile are queued, not lost (the lock lasts at most 2 s).
fn slash_command(pane: NodeId, command: &str, argument: &str) -> Vec<Submission> {
    let stages = SendInputSpec {
        pane,
        text: String::new(),
        delivery: Delivery::Typed,
        prompt_epoch: None,
        stages: vec![
            InputStage { text: command.into(), delay_ms: 0, delivery: Delivery::Typed },
            InputStage { text: argument.into(), delay_ms: 150, delivery: Delivery::Typed },
            InputStage { text: String::new(), delay_ms: 120, delivery: Delivery::Submitted },
        ],
        raw: None,
        mouse: None,
        if_current: None,
        lock_user_input: true,
        journal: true,
    };
    // A raw Escape (0x1b) clears a half-typed line first. It is its own
    // submission because raw bytes replace the other fields.
    let clear = SendInputSpec { raw: Some(vec![0x1b]), stages: vec![], ..stages.clone() };
    vec![
        Submission::keyed(Intent::SendInput(clear), format!("clear-{pane}")),
        Submission::keyed(Intent::SendInput(stages), format!("slash-{pane}-{command}")),
    ]
}
```

**TypeScript**

```ts
import type { Submission } from '@ilium/plugin-sdk';

// manifest: "capabilities": { "server": ["terminal.write",
//   "terminal.raw-input", "terminal.input-lock"] }
// A slash command and its argument arrive together. Keys the user types
// meanwhile are queued, not lost (the lock lasts at most 2 s).
export function slashCommand(pane: bigint, command: string, argument: string): Submission[] {
  const base = {
    pane, text: '', delivery: 'typed' as const, promptEpoch: undefined,
    mouse: undefined, ifCurrent: undefined, journal: true,
  };
  return [
    {
      // A raw Escape (0x1b) clears a half-typed line first.
      intent: { tag: 'send-input', val: {
        ...base, stages: [], raw: new Uint8Array([0x1b]), lockUserInput: false } },
      correlation: 1n,
      dedupKey: `clear-${pane}`,
    },
    {
      intent: { tag: 'send-input', val: {
        ...base, raw: undefined, lockUserInput: true,
        stages: [
          { text: command, delayMs: 0, delivery: 'typed' },
          { text: argument, delayMs: 150, delivery: 'typed' },
          { text: '', delayMs: 120, delivery: 'submitted' },
        ] } },
      correlation: 2n,
      dedupKey: `slash-${pane}-${command}`,
    },
  ];
}
```

#### 4. Retry after uncertain delivery

A prompt is never sent twice. Each send has a dedup key. On `input-uncertain` the plugin checks the input journal, which needs [journal.read](/pages/capability-journal-read/), and retries once after a back-off, using [durable timers](/pages/api-durable-timers/), only if the entry is missing. The outcome model is in [requests and outcomes](/pages/requests-and-outcomes/); the events are in [events](/pages/api-events/).

**Rust**

```rust
use ilium_plugin_sdk::host::{clock, journal, timers};
use ilium_plugin_sdk::prelude::*;

// manifest: "capabilities": { "server": ["terminal.write", "journal.read"] }
// Every send carries a dedup key and journal: true. The host writes the
// journal entry before the PTY write.
impl Handler for Scheduler {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            match event {
                Event::InputUncertain(uncertain) => {
                    let since = clock::now_unix_millis().saturating_sub(60_000);
                    let seen = journal::read(Some(uncertain.pane), since, 50)
                        .iter()
                        .any(|entry| entry.payload_json.contains(&uncertain.dedup_key));
                    if !seen {
                        // Not delivered: retry once after a short back-off.
                        if let Ok(timer) = timers::set(2_000, None) {
                            self.retries.push((timer, uncertain.pane, uncertain.dedup_key));
                        }
                    }
                }
                Event::TimerFired(timer) => {
                    if let Some(at) = self.retries.iter().position(|(t, _, _)| *t == timer) {
                        let (_, pane, key) = self.retries.remove(at);
                        // Same key: if the first send did arrive, this is a duplicate.
                        submissions.push(self.resend(pane, key));
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
import { nowUnixMillis } from '@ilium/plugin-sdk/host/clock';
import * as journal from '@ilium/plugin-sdk/host/journal';
import * as timers from '@ilium/plugin-sdk/host/timers';

// manifest: "capabilities": { "server": ["terminal.write", "journal.read"] }
// Every send carries a dedup key and journal: true. The host writes the
// journal entry before the PTY write.
const retries = new Map<bigint, { pane: bigint; key: string }>();

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    for (const event of batch) {
      if (event.tag === 'input-uncertain') {
        const { pane, dedupKey } = event.val;
        const since = nowUnixMillis() - 60_000n;
        const seen = journal.read(pane, since, 50).some((e) => e.payloadJson.includes(dedupKey));
        if (!seen) {
          // Not delivered: retry once after a short back-off.
          retries.set(timers.set(2_000, undefined), { pane, key: dedupKey });
        }
      } else if (event.tag === 'timer-fired' && retries.has(event.timerId)) {
        const { pane, key } = retries.get(event.timerId)!;
        retries.delete(event.timerId);
        // Same key: if the first send did arrive, this is a duplicate.
        submissions.push(resend(pane, key));
      }
    }
    return submissions;
  },
});
```

### Extension points

Other plugins change what `ilium.core.scheduler` does through the points below. The plugin owns each point (grant `extensions.provide:ilium.core.scheduler/<point>`). Contributors need `extensions.contribute:ilium.core.scheduler/<point>`. See [Extension points](/pages/api-extension-points/) for the mechanism and [Extending core plugins](/pages/extending-core-plugins/) for the five ways to work with a core feature.

| Point | Kind | What it does | Payload | Risk |
|---|---|---|---|---|
| `schedule-sources` | collect | Add a trigger source such as a calendar or a reset forecast. | entry: id, title, service (returns the next fire times) | standard |
| `before-send-scheduled` | hook (veto) | Refuse a scheduled or queued input at the moment it would be sent. | pane, text_length, schedule, source | high |

**Services** (grant `services.use:<name>`):

- `ilium.core.scheduler/queue`: enqueue, list, cancel: manage the prompt queue

**Topics** (list under `contributes.topics.subscribes`):

- `ilium.core.scheduler/fired`: { schedule, pane }

**Replaceable:** yes. A replacement lists `"replaces": ["ilium.core.scheduler"]` and provides the whole surface (see [Extending core plugins](/pages/extending-core-plugins/#replacing-a-core-plugin)).

#### Example: Refuses scheduled prompts on days the plugin marks as holidays

A package adds a `veto` handler. Handlers run until one refuses.

```json
{
  "api": "ilium:plugin@0.1.0",
  "exports": [
    "hook-handler"
  ],
  "capabilities": { "server": ["extensions.contribute:ilium.core.scheduler/before-send-scheduled"] },
  "contributes": {
    "extensions": [
      {
        "point": "ilium.core.scheduler/before-send-scheduled",
        "id": "skip-on-holiday",
        "priority": 50,
        "handler": "skip_on_holiday"
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
            "skip_on_holiday" => {
                // call.payload_json holds: pane, text_length, schedule, source
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
    if (call.extension !== 'skip_on_holiday') return { tag: 'pass' };
    // call.payloadJson holds: pane, text_length, schedule, source
    const payload = JSON.parse(call.payloadJson);
    void payload;
    return { tag: 'refuse', val: 'reason' };
  },
});
```

A handler that is late, traps or returns a reply the mode does not allow counts as `pass`. See [Extension points](/pages/api-extension-points/#hooks).

### Extension points this plugin owns

The table gives the values that [EXT-052] requires for each point this plugin owns. The page does not state the version, deadline, max or dynamic values, so they come from the defaults in [Extension points](/pages/api-extension-points/) and are marked (default). Both points run on the server side, so the main manifest grants `extensions.provide:ilium.core.scheduler/schedule-sources` and `extensions.provide:ilium.core.scheduler/before-send-scheduled` in its `server` capabilities and declares both in `contributes.extension_points`.

| Point | Side | Version | Deadline | Max | Dynamic |
|---|---|---|---|---|---|
| `ilium.core.scheduler/schedule-sources` | server | 0.1.0 (default) | none (default) | 64 entries (default) | no (default) |
| `ilium.core.scheduler/before-send-scheduled` | server | 0.1.0 (default) | 20 ms (default) | 8 handlers (default) | no (default) |

Side basis: `schedule-sources` is server because the server side owns the timers that use the fire times; `before-send-scheduled` is server because it refuses input at the moment the server would send it to the terminal.

### ABI used

- [durable timers and input journal](/pages/api-durable-timers/) and [clock and timers](/pages/api-clock-and-timers/)
- [send-input stages](/pages/api-intent-send-input/), [terminal.input-lock](/pages/capability-terminal-input-lock/) and [terminal.raw-input](/pages/capability-terminal-raw-input/)
- [journal.read](/pages/capability-journal-read/)
- [input widgets](/pages/widgets-input/)
- [agent primitives](/pages/primitives-agent/)
- [requests and outcomes](/pages/requests-and-outcomes/) and [events](/pages/api-events/)

### See also

- [Core replacement matrix](/pages/reference-core-replacement-matrix/) for the surface, stored data, CLI names and install codes that a replacement of this plugin must keep.
- [Core plugins](/pages/core-plugins/)
- [Notifications](/pages/core-plugin-notifications/)
- [Terminal](/pages/core-plugin-terminal/)
