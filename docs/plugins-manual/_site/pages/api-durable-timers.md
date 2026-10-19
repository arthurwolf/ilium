# durable timers and input journal

## durable timers and input journal

Ordinary timers (see [clock and timers](/pages/api-clock-and-timers/)) live in memory and vanish when Ilium restarts. Two features survive a restart. A durable timer fires at a wall-clock time, even if Ilium was not running at that time. The pane journal records events per pane, and a `send-input` marked `journal: true` is recorded before it is written, so a crash between the write and its outcome is reported instead of lost.

Both features live in the snapshot that the server keeps. They are always granted: a plugin needs no capability for its own durable timers or its own journal entries. Reading the journal entries of the host or of other plugins needs the `journal.read` grant.

This page covers the durable timer, the journal and the input journal. The settings row state and meter calls that share section 6.33 of the spec (`set-row-state`, `set-item-states`, `set-choice-items`, `set-info-rich`, `set-meter`) belong to the settings pages (see [settings](/pages/api-settings/)).

<figure>
<svg viewBox="0 0 440 250" role="img" aria-label="Durable timer across a restart">
  <defs>
    <marker id="api-durable-arrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
      <path d="M0,0 L8,4 L0,8 z" fill="currentColor"/>
    </marker>
  </defs>
  <line x1="20" y1="60" x2="420" y2="60" stroke="currentColor"/>
  <text x="20" y="22" font-size="12" fill="currentColor">Time</text>
  <circle cx="50" cy="60" r="5" fill="currentColor"/>
  <text x="50" y="44" font-size="10" text-anchor="middle" fill="currentColor">timers.at</text>
  <rect x="100" y="52" width="150" height="16" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
  <text x="175" y="90" font-size="10" text-anchor="middle" fill="currentColor">Ilium not running</text>
  <circle cx="190" cy="60" r="5" fill="none" stroke="currentColor"/>
  <text x="190" y="44" font-size="10" text-anchor="middle" fill="currentColor">due time</text>
  <circle cx="270" cy="60" r="5" fill="currentColor"/>
  <text x="270" y="44" font-size="10" text-anchor="middle" fill="currentColor">activate</text>
  <line x1="290" y1="66" x2="290" y2="112" stroke="currentColor" marker-end="url(#api-durable-arrow)"/>
  <text x="340" y="86" font-size="10" fill="currentColor">durable-timer-fired</text>
  <rect x="150" y="114" width="280" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="160" y="131" font-size="11" fill="currentColor">timer, name</text>
  <text x="160" y="146" font-size="11" fill="currentColor">late-by-millis: how long after the due time</text>
  <text x="20" y="184" font-size="11" fill="currentColor">A timer that came due while Ilium was stopped fires once, after the</text>
  <text x="20" y="200" font-size="11" fill="currentColor">next activation, and reports how late it is. It never fires twice.</text>
  <text x="20" y="224" font-size="11" fill="currentColor">The same name replaces an earlier timer of that name.</text>
</svg>
<figcaption>A durable timer is stored in the snapshot with its due time. After a restart, an overdue timer fires once and reports its lateness.</figcaption>
</figure>

### What you need

- **Grants:** none for `timers.at`, `timers.cancel-durable`, `journal.log-event` and the reading of your own entries. `journal.read` for the entries of the host and of other plugins.
- **Sides:** server and client. Each side keeps its own timers and entries.
- **Language:** Rust with `ilium-plugin-sdk` (`host::timers`, `host::journal`), or TypeScript with `@ilium/plugin-sdk` (`@ilium/plugin-sdk/host/timers`, `@ilium/plugin-sdk/host/journal`).
- **For the input journal:** the grant of the `send-input` intent (`terminal.write`), and the `journal: true` field on the intent (see [intents](/pages/api-intents/)).

### Surface

```wit
// ilium:host/timers  (S C, always)
at:             func(epoch: epoch-ms, name: string) -> result<timer-id, plugin-error>;   // durable
cancel-durable: func(name: string);

// ilium:host/journal  (S C)
record journal-entry { at: epoch-ms, pane: option<node-id>, source: string, kind: string, payload-json: string }
log-event: func(pane: option<node-id>, kind: string, payload-json: string) -> result<_, plugin-error>;   // <= 4 KiB payload
read:      func(pane: option<node-id>, since: epoch-ms, limit: u32) -> list<journal-entry>;   // own entries; journal.read for others
```

`epoch-ms` is a `u64` count of milliseconds since the Unix epoch, the same unit as `clock.now-unix-millis`.

| WIT function | Rust | TypeScript | Meaning |
|---|---|---|---|
| `at` | `timers::at(epoch: u64, name: &str) -> Result<TimerId, PluginError>` | `timers.at(epoch: bigint, name: string): bigint` | Arms a durable timer that fires at `epoch`. The same `name` replaces an earlier timer of that name. |
| `cancel-durable` | `timers::cancel_durable(name: &str)` | `timers.cancelDurable(name: string): void` | Removes the durable timer with that name. |
| `log-event` | `journal::log_event(pane: Option<NodeId>, kind: &str, payload_json: &str) -> Result<(), PluginError>` | `journal.logEvent(pane: bigint \| undefined, kind: string, payloadJson: string): void` | Adds an entry to the journal, for a pane or with `pane` set to `none`. |
| `read` | `journal::read(pane: Option<NodeId>, since: u64, limit: u32) -> Vec<JournalEntry>` | `journal.read(pane: bigint \| undefined, since: bigint, limit: number): JournalEntry[]` | Returns entries from `since` on, at most `limit`. Without `journal.read` these are the plugin's own entries. |

`JournalEntry` is `JournalEntry { at: u64, pane: Option<NodeId>, source: String, kind: String, payload_json: String }` in Rust and `{ at: bigint; pane: bigint | undefined; source: string; kind: string; payloadJson: string }` in TypeScript.

#### Events

| Event | Payload | Rust | TypeScript | Delivery |
|---|---|---|---|---|
| `durable-timer-fired` | `timer`, `name`, `late-by-millis` (u64) | `Event::DurableTimerFired(DurableTimerFiredEvent { timer, name, late_by_millis })` | `{ tag: 'durable-timer-fired', val: { timer, name, lateByMillis } }` | never dropped |
| `input-uncertain` | `pane`, `dedup-key` | `Event::InputUncertain(InputUncertainEvent { pane, dedup_key })` | `{ tag: 'input-uncertain', val: { pane, dedupKey } }` | never dropped |

A plugin that is not running is activated for a due durable timer: the activation trigger is `durable-timer` (see [lifecycle](/pages/lifecycle/)).

### The journal

The journal holds plugin entries and host entries per pane. A plugin writes its own entries with `log-event`. The host writes entries for what it does:

| Host entry kind | When |
|---|---|
| `pane-created` | A pane is created. |
| `pane-exited` | A pane's process ends. |
| `input-sent` | A `send-input` completes. The payload holds the dedup key and the delivery result. |

The journal survives restarts in the snapshot. It is trimmed oldest first when it passes `journal_entries` (5,000 per pane by default).

### The input journal

A `send-input` with `journal: true` writes its journal entry before the PTY write. If the server stops between the write and the outcome, the restarted plugin receives `input-uncertain { pane, dedup-key }`. This is deliberate: the kernel never replays the input silently (decision D21), because the prompt could then reach the agent twice. The plugin decides whether to send again.

<figure>
<svg viewBox="0 0 440 230" role="img" aria-label="Sequence of a journaled send-input across a crash">
  <defs>
    <marker id="api-durable-seq-arrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
      <path d="M0,0 L8,4 L0,8 z" fill="currentColor"/>
    </marker>
  </defs>
  <text x="70" y="16" font-size="12" text-anchor="middle" fill="currentColor">Plugin</text>
  <text x="230" y="16" font-size="12" text-anchor="middle" fill="currentColor">Kernel</text>
  <text x="390" y="16" font-size="12" text-anchor="middle" fill="currentColor">Pane (PTY)</text>
  <line x1="70" y1="24" x2="70" y2="220" stroke="currentColor" stroke-dasharray="3 3"/>
  <line x1="230" y1="24" x2="230" y2="220" stroke="currentColor" stroke-dasharray="3 3"/>
  <line x1="390" y1="24" x2="390" y2="220" stroke="currentColor" stroke-dasharray="3 3"/>
  <line x1="70" y1="44" x2="228" y2="44" stroke="currentColor" marker-end="url(#api-durable-seq-arrow)"/>
  <text x="150" y="38" font-size="10" text-anchor="middle" fill="currentColor">send-input, journal: true</text>
  <rect x="150" y="56" width="160" height="22" rx="4" fill="none" stroke="currentColor"/>
  <text x="230" y="71" font-size="10" text-anchor="middle" fill="currentColor">journal entry written first</text>
  <line x1="232" y1="96" x2="388" y2="96" stroke="currentColor" marker-end="url(#api-durable-seq-arrow)"/>
  <text x="310" y="90" font-size="10" text-anchor="middle" fill="currentColor">PTY write</text>
  <text x="230" y="130" font-size="11" text-anchor="middle" fill="currentColor">server stops before the outcome</text>
  <line x1="150" y1="138" x2="310" y2="138" stroke="currentColor" stroke-dasharray="2 3"/>
  <line x1="228" y1="168" x2="72" y2="168" stroke="currentColor" marker-end="url(#api-durable-seq-arrow)"/>
  <text x="150" y="162" font-size="10" text-anchor="middle" fill="currentColor">activate, then input-uncertain</text>
  <text x="70" y="194" font-size="10" fill="currentColor">The plugin checks the pane and decides whether to resend.</text>
  <text x="70" y="212" font-size="10" fill="currentColor">The kernel never replays the input itself.</text>
</svg>
<figcaption>With <code>journal: true</code> the entry exists before the byte is written. After a restart the plugin learns which dedup key is in doubt.</figcaption>
</figure>

### Worked example: scheduled input

The plugin sends a prompt to a pane after a delay, and survives a restart. It keeps the pane and text in `storage` (see [storage](/pages/api-storage/)), arms a durable timer named after the job, and on firing sends the text with a journaled `send-input`. If Ilium was down, the timer fires late and the plugin decides whether the delay is still acceptable.

**Rust**

```rust
use ilium_plugin_sdk::host::{clock, timers};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::intents::{Delivery, SendInputSpec};

const STALE_AFTER_MILLIS: u64 = 10 * 60 * 1000;

/// Arms the timer. `delay_millis` counts from now.
fn schedule(job: &str, delay_millis: u64) -> Result<TimerId, PluginError> {
    timers::at(clock::now_unix_millis() + delay_millis, job)
}

fn send_scheduled(pane: NodeId, text: String, job: &str) -> Submission {
    let intent = Intent::SendInput(SendInputSpec {
        pane,
        text,
        delivery: Delivery::Submitted,
        journal: true,
        ..SendInputSpec::default()
    });
    Submission::keyed(intent, format!("scheduled:{job}"))
}

fn on_event(event: Event, pane: NodeId, text: &str) -> Option<Submission> {
    match event {
        Event::DurableTimerFired(fired) if fired.late_by_millis <= STALE_AFTER_MILLIS => {
            Some(send_scheduled(pane, text.to_string(), &fired.name))
        }
        Event::DurableTimerFired(fired) => {
            log_late(&fired.name, fired.late_by_millis);
            None
        }
        Event::InputUncertain(uncertain) => {
            // The dedup key names the job. Look at the pane before sending again.
            log_uncertain(&uncertain.dedup_key);
            None
        }
        _ => None,
    }
}
```

**TypeScript**

```ts
import * as clock from '@ilium/plugin-sdk/host/clock';
import * as timers from '@ilium/plugin-sdk/host/timers';
import type { Event, Submission } from '@ilium/plugin-sdk/types';

const STALE_AFTER_MILLIS = 10n * 60n * 1000n;

/** Arms the timer. `delayMillis` counts from now. */
export function schedule(job: string, delayMillis: bigint): bigint {
  return timers.at(clock.nowUnixMillis() + delayMillis, job);
}

function sendScheduled(pane: bigint, text: string, job: string): Submission {
  return {
    intent: {
      tag: 'send-input',
      val: { pane, text, delivery: 'submitted', journal: true },
    },
    correlation: 0n,
    dedupKey: `scheduled:${job}`,
  };
}

export function onEvent(event: Event, pane: bigint, text: string): Submission | undefined {
  if (event.tag === 'durable-timer-fired') {
    if (event.val.lateByMillis > STALE_AFTER_MILLIS) {
      logLate(event.val.name, event.val.lateByMillis);
      return undefined;
    }
    return sendScheduled(pane, text, event.val.name);
  }
  if (event.tag === 'input-uncertain') {
    // The dedup key names the job. Look at the pane before sending again.
    logUncertain(event.val.dedupKey);
  }
  return undefined;
}
```

`log_late`, `log_uncertain` and their TypeScript equivalents are the plugin's own helpers. `send-input` requires a dedup key (see [intents](/pages/api-intents/)). The unset optional fields of `send-input` (`stages`, `raw`, `mouse`, `if-current`, `lock-user-input`) keep their defaults.

### Worked example: a pane journal entry

A plugin records when it pauses a pane, and reads the last entries back for a panel.

**Rust**

```rust
use ilium_plugin_sdk::host::{clock, journal};
use ilium_plugin_sdk::prelude::*;

fn record_pause(pane: NodeId, reason: &str) -> Result<(), PluginError> {
    journal::log_event(Some(pane), "paused", &format!(r#"{{"reason":"{reason}"}}"#))
}

fn recent(pane: NodeId) -> Vec<String> {
    let since = clock::now_unix_millis().saturating_sub(60 * 60 * 1000);
    journal::read(Some(pane), since, 20)
        .into_iter()
        .map(|entry| format!("{} {} {}", entry.at, entry.kind, entry.payload_json))
        .collect()
}
```

**TypeScript**

```ts
import * as clock from '@ilium/plugin-sdk/host/clock';
import * as journal from '@ilium/plugin-sdk/host/journal';

export function recordPause(pane: bigint, reason: string): void {
  journal.logEvent(pane, 'paused', JSON.stringify({ reason }));
}

export function recent(pane: bigint): string[] {
  const since = clock.nowUnixMillis() - 60n * 60n * 1000n;
  return journal
    .read(pane, since, 20)
    .map((entry) => `${entry.at} ${entry.kind} ${entry.payloadJson}`);
}
```

### What the user sees

A pane with a scheduled input shows the scheduled-input glyph in the objective column of its tree row, next to the identity glyph and the activity spinner. The client default for that glyph is `⏰` (the `ScheduledInput` icon slot).

<figure>
<pre class="term">▼ 📂 Ilium
›   🦀 ⏰ ⠋ Fix auth
›   🖥️    Shell
</pre>
<figcaption>A Claude pane with a scheduled input shows the objective glyph before the working spinner. Glyphs are the client defaults from the icon settings; column spacing is approximate. Illustrative.</figcaption>
</figure>

The plugin log records what happened around a restart:

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin logs example.scheduler --tail 4
<span class="t-dim">2026-10-12 08:59:50</span> <span class="t-ok">info </span> timers.at job-17 due 09:00:00
<span class="t-dim">2026-10-12 09:04:12</span> <span class="t-ok">info </span> durable-timer-fired job-17 late 252000 ms
<span class="t-dim">2026-10-12 09:04:12</span> <span class="t-ok">info </span> send-input pane 5 journal=true
<span class="t-dim">2026-10-12 09:04:12</span> <span class="t-ok">info </span> input-sent ok
<span class="t-prompt">$</span> <span class="t-dim">4 lines shown</span>
</pre>
<figcaption>The server was down at 09:00:00. The timer fires once after the next activation and reports 252,000 ms of lateness. Illustrative, not generated.</figcaption>
</figure>

### Error codes

| Function | Code | When |
|---|---|---|
| `at` | `invalid-argument` | `epoch` is in the past by more than 1 s. |
| `at` | `quota-exceeded` | The plugin already holds `durable_timers` timers. |
| `log-event` | `invalid-argument` | The payload is over 4 KiB. |
| `log-event` | `quota-exceeded` | The `journal_entries` quota is full. |
| `log-event` | `rate-limited` | Too many entries in a short time. |
| `cancel-durable`, `read` | none | These functions return no error. |
| journaled `send-input` | as for `send-input` | The `journal` field adds no error code of its own. |

### Limits

| Limit | Default | Ceiling tier 1 | Ceiling tier 2 and 3 |
|---|---|---|---|
| `durable_timers` | 64 | 256 | 64 |
| `journal_entries` (per pane, own entries) | 5,000 | 20,000 | 5,000 |
| Journal payload | 4 KiB | 4 KiB | 4 KiB |

`durable_timers` and `journal_entries` are requestable in the manifest `limits` up to the ceiling for the tier. Beyond `journal_entries`, the oldest entries are trimmed.

### Edge cases

| Situation | Result |
|---|---|
| The server was off at the due time | The timer fires once after the next activation. `late-by-millis` is the lateness. |
| `at` with a name that is in use | The earlier timer is replaced. |
| `at` with a time up to 1 s in the past | Accepted. A longer past time is `invalid-argument`. |
| Crash after the PTY write, before the outcome | `input-uncertain` after restart. The input is not replayed. |

<div class="warn">
<p>Do not treat <code>input-uncertain</code> as a failure to retry blindly. The text may already be in the prompt. Read the screen first (see <a href="/pages/primitives-terminal/">terminal</a>), then decide.</p>
</div>

### See also

- [clock and timers](/pages/api-clock-and-timers/) for ordinary, in-memory timers and the clock.
- [intents](/pages/api-intents/) for `send-input` and its dedup key.
- [Events](/pages/api-events/) for the delivery policy of `durable-timer-fired` and `input-uncertain`.
- [Lifecycle](/pages/lifecycle/) for the `durable-timer` activation trigger.
- [storage](/pages/api-storage/) for keeping the job's pane and text across restarts.
- [Errors and limits](/pages/errors-and-limits/) for the full code list.

Interface version: `ilium:host/timers` and `ilium:host/journal`, version 0.1.
