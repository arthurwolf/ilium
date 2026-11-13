---
layout: page
title: clock and timers
permalink: /pages/api-clock-and-timers/
---

## clock and timers

The `clock` import gives wall-clock time and monotonic time. The `timers` import fires a `timer-fired` event after a delay, and again at a fixed interval when you ask for one. A plugin uses `clock` to read the time and to measure how long work takes. It uses `timers` to run work later or on a schedule, without a thread of its own.

Timers set with `timers.set` are not saved. When Ilium restarts the plugin, every one of them is gone, so the plugin sets its timers again in `activate`. Cancel each timer in `deactivate` so that no event arrives after shutdown. Durable timers are saved in the snapshot and fire at a wall-clock time even after a restart. They have their own page: [durable timers and input journal](/pages/api-durable-timers/).

### What you need

- **Grants:** none. `clock` is always available on the server, client and scene sides. `timers` is always available on the server and client sides.
- **Scene side:** the scene world has no `timers` import. A scene reads time from the frame input instead: `time-millis` counts from the start of the scene, and `wall-millis` counts from the last time the scene became visible.
- **Rust:** the `ilium-plugin-sdk` crate at the release pin, with Rust stable 1.90 or later and the `wasm32-wasip2` target.
- **TypeScript:** the `@ilium/plugin-sdk` package, installed with Bun 1.2 or later.
- **Handler:** timers reach the plugin only through the `handler` export. The plugin must handle `timer-fired` in its event batches.

### The surface

Every timer and clock function is synchronous. The timer id is a `u64` in WIT, `TimerId` in Rust and a `bigint` in TypeScript.

| WIT | Rust | TypeScript | Sides | Meaning |
|---|---|---|---|---|
| `now-unix-millis: func() -> u64` | `clock::now_unix_millis() -> u64` | `nowUnixMillis(): bigint` | S C A | Wall-clock time in milliseconds since the Unix epoch. It can jump when the system clock changes. |
| `monotonic-millis: func() -> u64` | `clock::monotonic_millis() -> u64` | `monotonicMillis(): bigint` | S C A | Milliseconds from a fixed origin. It never goes backwards. Use it for durations. |
| `set: func(after-millis: u32, repeat-millis: option<u32>) -> result<timer-id, plugin-error>` | `timers::set(after_millis: u32, repeat_millis: Option<u32>) -> Result<TimerId, PluginError>` | `timers.set(afterMillis: number, repeatMillis: number or undefined): bigint`, throws `PluginError` | S C | Starts a timer. The first event arrives after `after-millis`. With `repeat-millis`, events keep arriving at that interval until the timer is cancelled. |
| `cancel: func(timer: timer-id)` | `timers::cancel(timer: TimerId)` | `timers.cancel(timer: bigint): void` | S C | Stops a timer. A cancelled timer sends no further events. |
| `timer-fired(timer-id)` event | `Event::TimerFired(TimerId)` | `{ tag: 'timer-fired', timerId: bigint }` | S C | Delivered in the `handler` batch. One event per timer per batch. |

Rust and TypeScript names follow the binding rules in the Rust SDK and TypeScript SDK pages. The Rust path for `TimerId` is `ilium_plugin_sdk::types::ids`.

### WIT

```wit
// ilium:host/clock  (S C A, always)
now-unix-millis: func() -> u64;
monotonic-millis: func() -> u64;

// ilium:host/timers  (S C, always)
set:    func(after-millis: u32, repeat-millis: option<u32>) -> result<timer-id, plugin-error>;
cancel: func(timer: timer-id);
// at and cancel-durable: see durable timers
```

The `timer-fired` event carries the `timer-id`, and events for one timer are coalesced while they wait. Timers fire on the side that set them. A timer set by the server side does not reach the client side, and the reverse.

### Choosing a clock and a timer

| Need | Use | Why |
|---|---|---|
| Show a time of day or stamp a record | `now-unix-millis` | It follows the system clock and matches `epoch-ms` values in the journal and in durable timers. |
| Measure how long work took | `monotonic-millis` | It never goes backwards, even when the system clock is corrected. |
| Run something every few seconds while Ilium runs | `timers.set` with `repeat-millis` | In memory, cheap, lost on restart. |
| Wait once, then act | `timers.set` with no repeat | In memory. Use a debounce pattern to restart it. |
| Act at a wall-clock time, even across a restart | `timers.at` | Persisted. Fires once, late if Ilium was down. |

<figure>
<svg viewBox="0 0 440 150" role="img" aria-label="Ordinary timers compared with durable timers across a restart">
  <title>Ordinary and durable timers across a restart</title>
  <defs>
    <marker id="clock-compare-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
      <path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
    </marker>
  </defs>
  <text x="10" y="22" font-size="11" fill="currentColor">timers.set</text>
  <line x1="90" y1="18" x2="200" y2="18" stroke="currentColor" marker-end="url(#clock-compare-arrow)"/>
  <text x="210" y="22" font-size="11" fill="currentColor">restart: timer is gone, activate arms it again</text>
  <text x="10" y="62" font-size="11" fill="currentColor">timers.at</text>
  <line x1="90" y1="58" x2="200" y2="58" stroke="currentColor" marker-end="url(#clock-compare-arrow)"/>
  <text x="210" y="62" font-size="11" fill="currentColor">restart: timer survives, fires once, late</text>
  <text x="10" y="102" font-size="11" fill="currentColor">clock</text>
  <line x1="90" y1="98" x2="200" y2="98" stroke="currentColor" marker-end="url(#clock-compare-arrow)"/>
  <text x="210" y="102" font-size="11" fill="currentColor">always available, no state</text>
  <text x="10" y="136" font-size="10" fill="currentColor">Only timers.at is stored in the snapshot.</text>
</svg>
<figcaption>What survives a restart. Only durable timers are stored; ordinary timers must be set again in <code>activate</code>.</figcaption>
</figure>

### Rules

- **Minimum interval:** 50 ms. Both `after-millis` and `repeat-millis` must be at least 50. A value under 50, including 0, is refused.
- **Held timers:** 64 per plugin by default (the same default applies separately to durable timers). A manifest `limits` entry can raise the limit for tier 1 (bundled) plugins to 256. Tier 2 and tier 3 plugins stay at 64.
- **Coalescing:** events for one timer merge while they wait in the queue. A handler that runs late sees one `timer-fired` for that timer with the current state, not one event per missed tick.
- **Identity:** the handler matches `timer-fired` against the ids the plugin stored. Ignore an event whose id the plugin no longer tracks.
- **Delivery:** a timer never fires inside the call that set it. The first `timer-fired` is queued after the delay, counted from the return of `timers.set` on the monotonic clock, and arrives in the next batch [API-074].
- **One-shot timers:** the id is freed when its `timer-fired` is queued. `timers.cancel` of a freed, unknown or already cancelled id does nothing and returns no error.
- **Cancel:** `timers.cancel` also removes a `timer-fired` of that timer that is queued and not yet delivered, so a cancelled timer never reaches the handler [API-074].
- **Repeat:** a repeating timer keeps its drift-free schedule (`first + n * repeat-millis`). Ticks missed while the queue holds a pending `timer-fired` of that timer merge into it.
- **Restart:** every ordinary timer is dropped when the instance ends, and no `timer-fired` of the old instance reaches the new one.
- **Deadline:** the `handler` call must return within the event deadline, 20 ms by default and 50 ms at tier 1. Start slow work as a request and read the result later as `request-completed`. See [Requests and outcomes](/pages/requests-and-outcomes/).

<figure>
<svg viewBox="0 0 340 210" role="img" aria-label="Timer lifecycle from activate to deactivate">
  <title>Timer lifecycle</title>
  <defs>
    <marker id="timer-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
      <path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="40" y="12" width="260" height="30" rx="6" fill="none" stroke="currentColor"/>
  <text x="50" y="31" font-size="11" fill="currentColor">activate: timers.set(5000, 5000)</text>
  <line x1="170" y1="42" x2="170" y2="62" stroke="currentColor" marker-end="url(#timer-arrow)"/>
  <rect x="40" y="64" width="260" height="30" rx="6" fill="none" stroke="currentColor"/>
  <text x="50" y="83" font-size="11" fill="currentColor">Every 5 s: timer-fired queued (timer 7)</text>
  <line x1="170" y1="94" x2="170" y2="114" stroke="currentColor" marker-end="url(#timer-arrow)"/>
  <rect x="40" y="116" width="260" height="30" rx="6" fill="none" stroke="currentColor"/>
  <text x="50" y="135" font-size="11" fill="currentColor">on-events: start status read</text>
  <line x1="170" y1="146" x2="170" y2="166" stroke="currentColor" marker-end="url(#timer-arrow)"/>
  <rect x="40" y="168" width="260" height="30" rx="6" fill="none" stroke="currentColor"/>
  <text x="50" y="187" font-size="11" fill="currentColor">deactivate: timers.cancel(timer 7)</text>
</svg>
<figcaption>A repeating timer runs from activate until deactivate cancels it.</figcaption>
</figure>

<figure>
<svg viewBox="0 0 340 170" role="img" aria-label="Timer events coalesce when the handler is slow">
<title>Timer coalescing</title>
<desc>Three timer ticks arrive while the handler is busy. The queue keeps one timer-fired event for the timer.</desc>
<text x="8" y="16" font-size="12" fill="currentColor">Ticks</text>
<line x1="60" y1="20" x2="330" y2="20" stroke="currentColor"/>
<circle cx="80" cy="20" r="5" fill="currentColor"/>
<circle cx="150" cy="20" r="5" fill="currentColor"/>
<circle cx="220" cy="20" r="5" fill="currentColor"/>
<text x="62" y="40" font-size="11" fill="currentColor">t 0 ms</text>
<text x="132" y="40" font-size="11" fill="currentColor">t 5 s</text>
<text x="202" y="40" font-size="11" fill="currentColor">t 10 s</text>
<rect x="8" y="60" width="324" height="36" rx="6" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
<text x="16" y="82" font-size="12" fill="currentColor">Queue: one timer-fired for timer 7</text>
<rect x="8" y="110" width="324" height="36" rx="6" fill="none" stroke="currentColor"/>
<text x="16" y="132" font-size="12" fill="currentColor">Handler: runs once with the latest state</text>
<text x="8" y="164" font-size="11" fill="currentColor">Ticks that wait together merge into one event.</text>
</svg>
<figcaption>A slow handler sees one event per timer, not a backlog of ticks.</figcaption>
</figure>

### Example 1: a repeating status poll (server)

The plugin reads a status file every five seconds in the server world. It cancels the timer when it is deactivated.

**Rust**

```rust
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::host::{clock, timers};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::ids::TimerId;

const POLL_MILLIS: u32 = 5_000;

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct StatusPoll {
    poll_timer: Option<TimerId>,
}

impl Lifecycle for StatusPoll {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        self.poll_timer = Some(timers::set(POLL_MILLIS, Some(POLL_MILLIS))?);
        Ok(())
    }

    fn deactivate(&mut self, _reason: DeactivationReason) -> Option<Vec<u8>> {
        if let Some(timer) = self.poll_timer.take() {
            timers::cancel(timer);
        }
        None
    }
}

impl Handler for StatusPoll {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            if let Event::TimerFired(timer) = event {
                if Some(timer) == self.poll_timer {
                    let started = clock::monotonic_millis();
                    start_status_read();
                    let elapsed = clock::monotonic_millis() - started;
                    log::log(Level::Debug, &format!("status read started in {elapsed} ms"));
                }
            }
        }
        Vec::new()
    }
}
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import type { Event, Submission } from '@ilium/plugin-sdk/types';
import { monotonicMillis } from '@ilium/plugin-sdk/host/clock';
import { log } from '@ilium/plugin-sdk/host/log';
import * as timers from '@ilium/plugin-sdk/host/timers';

const POLL_MILLIS = 5_000;
let pollTimer: bigint | undefined;

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  activate() {
    pollTimer = timers.set(POLL_MILLIS, POLL_MILLIS);
  },
  deactivate() {
    if (pollTimer !== undefined) {
      timers.cancel(pollTimer);
      pollTimer = undefined;
    }
    return undefined;
  },
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'timer-fired' && event.timerId === pollTimer) {
        const started = monotonicMillis();
        startStatusRead();
        const elapsed = monotonicMillis() - started;
        log('debug', `status read started in ${elapsed} ms`);
      }
    }
    return [];
  },
});
```

`start_status_read` and `startStatusRead` are the plugin's own helpers. They start a read through the request model and return at once. The read result arrives later as a `request-completed` event.

<pre class="term"><span class="t-prompt">$</span> ilium plugin logs example.status-poll --tail 3
<span class="t-dim">2026-10-11 09:00:05</span> <span class="t-dim">debug</span> status read started in 0 ms
<span class="t-dim">2026-10-11 09:00:10</span> <span class="t-dim">debug</span> status read started in 0 ms
<span class="t-dim">2026-10-11 09:00:15</span> <span class="t-ok">info </span> deactivating: disabled, timer 7 cancelled
<span class="t-prompt">$</span> <span class="t-dim">3 lines shown</span>
</pre>

Illustrative, not generated. The last ticks of the status poll, then the timer is cancelled when the plugin is disabled.

### Example 2: debouncing a search field (client)

The client waits for 300 ms without edits before it searches. Each edit cancels the pending timer and arms a new one. Call `query_edited` from the handler when the search field changes.

**Rust**

```rust
use ilium_plugin_sdk::host::timers;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::ids::TimerId;

const DEBOUNCE_MILLIS: u32 = 300;

#[ilium_plugin(side = "client")]
#[derive(Default)]
struct SearchBox {
    pending_timer: Option<TimerId>,
}

impl Lifecycle for SearchBox {}

impl SearchBox {
    fn query_edited(&mut self) -> Result<(), PluginError> {
        if let Some(previous) = self.pending_timer.take() {
            timers::cancel(previous);
        }
        self.pending_timer = Some(timers::set(DEBOUNCE_MILLIS, None)?);
        Ok(())
    }
}

impl Handler for SearchBox {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            if let Event::TimerFired(timer) = event {
                if Some(timer) == self.pending_timer {
                    self.pending_timer = None;
                    refresh_results();
                }
            }
        }
        Vec::new()
    }
}
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import type { Event, Submission } from '@ilium/plugin-sdk/types';
import * as timers from '@ilium/plugin-sdk/host/timers';

const DEBOUNCE_MILLIS = 300;
let pendingTimer: bigint | undefined;

function queryEdited(): void {
  if (pendingTimer !== undefined) {
    timers.cancel(pendingTimer);
  }
  pendingTimer = timers.set(DEBOUNCE_MILLIS, undefined);
}

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  activate() {},
  deactivate() {
    if (pendingTimer !== undefined) {
      timers.cancel(pendingTimer);
      pendingTimer = undefined;
    }
    return undefined;
  },
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'timer-fired' && event.timerId === pendingTimer) {
        pendingTimer = undefined;
        refreshResults();
      }
    }
    return [];
  },
});
```

### What the user sees

Timers draw nothing. The status poll writes only to the plugin log, which `ilium plugin logs` shows. The search box updates its results about 300 ms after the last keystroke. A user who types quickly sees one refresh at the end of the burst.

### Edge cases and errors

| Situation | Result |
|---|---|
| `after-millis` or `repeat-millis` under 50, including 0 | `invalid-argument`. No timer is armed. |
| The plugin already holds its limit of timers | `quota-exceeded`. Cancel an unused timer and retry. |
| `set` returns `internal` | The plugin has no timer. Treat the failure like any other host error. |
| `cancel` with an unknown or finished id | Nothing happens. No error is returned. |
| Clock functions | Never fail. |
| `handler` runs past the event deadline | The plugin traps. Move slow work to a request. |
| The event queue fills because the handler does not return | The plugin restarts. Its timers are lost, so `activate` must arm them again. |

<pre class="term"><span class="t-prompt">$</span> ilium plugin logs example.status-poll --tail 2
<span class="t-dim">2026-10-11 09:14:02</span> <span class="t-err">error</span> timers.set: invalid-argument
<span class="t-dim">2026-10-11 09:14:02</span> <span class="t-warn">warn </span> interval 20 ms is under 50 ms
<span class="t-prompt">$</span> <span class="t-dim">2 lines shown</span>
</pre>

Illustrative, not generated. A plugin that asks for a 20 ms interval logs the refusal from its own error handler.

### Time zones and locale

`now-unix-millis` is UTC. `ilium:plugin@0.1.0` has no function for the user's UTC offset, time zone or locale, and a clock import does not add one. A plugin MUST store time as UTC milliseconds and MUST state the zone or show relative time when it displays a time. Test: render the view with the test clock fixed and assert the text carries the zone name or a relative phrase such as `5 min ago`. [AUT-616] Patterns for formatting are in [Internationalisation](/pages/guide-internationalisation/). The test clock starts at 2030-01-01 00:00:00 UTC and moves only with `advance_clock`. See [Patterns cookbook](/pages/guide-patterns-cookbook/) for debounce, throttle and retry built on timers.

### Used by

- Sound alerts (feature 47) and desktop notifications (feature 48) use timers to coalesce bursts of activity.
- Reset planning (feature 88) runs an hourly timer that fetches a feed.
- Scheduled input (feature 55) and the progress monitor (feature 59) use timers for their intervals.

### Units and conventions

Times are `epoch-ms` (`u64`, milliseconds since the Unix epoch, UTC) and durations are milliseconds. Correlation numbers below 2^63 belong to the plugin and the rest to the SDK. A limit in characters counts Unicode scalar values. The rules are stated once in the [conventions of the WIT reference](/pages/reference-wit/#2-conventions).

### See also

- [log](/pages/api-log/) for writing timer results to the plugin log.
- [Events](/pages/api-events/) for the `timer-fired` delivery policy.
- [Lifecycle](/pages/lifecycle/) for `activate` and `deactivate`, where timers are armed and cancelled.
- [Durable timers and input journal](/pages/api-durable-timers/) for timers that survive a restart.
- [Errors and limits](/pages/reference-errors-and-limits/) for the event deadline and the full error code list.
- [Rust SDK reference](/pages/rust-sdk/) and [TypeScript SDK reference](/pages/typescript-sdk/) for the binding rules.

Contract: `ilium:plugin@0.1.0`.
