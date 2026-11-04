---
layout: page
title: Backpressure and retries
permalink: /pages/perf-backpressure-and-retries/
---

## Backpressure and retries

A plugin does not control how fast events arrive or how fast the host accepts its requests. The host protects itself with queues, credit windows, rate limits and a shared CPU bucket. Each of these answers in a specific way: some drop events, some pause a socket, and some refuse a call with an error that says when to try again. This page explains what the plugin sees in each case, which errors are normal, and how to write a retry that does not make the problem worse. The queue policies themselves are in [Queues and backpressure](/pages/reference-host-scheduling/#queues-and-backpressure). This page is the author's side of them.

### You need

- A plugin with at least one submission or one stream, so there is something to refuse. The tutorials in [Getting started](/pages/getting-started-overview/) are enough.
- The error codes in [Errors and limits](/pages/errors-and-limits/). This page assumes you know the fifteen codes.
- [Sizing memory and CPU](/pages/perf-sizing-memory-and-cpu/) for the budgets that the refusals come from.

### Which answer means what

Each refusal tells the plugin something different. The table sorts them by what the plugin should do next.

| What the plugin sees | Meaning | Retry? | Plugin action |
|---|---|---|---|
| `busy` | A transient capacity is full: pending requests, the job queue, a picker already open, a device in use | Yes, later | Wait, then try again. Do not retry at once. |
| `rate-limited` | A rate rule throttles the call. `retry-after-ms` is always set. | Yes, after `retry-after-ms` | Wait at least `retry-after-ms`, then try again. |
| `quota-exceeded` with `retry-after-ms` | The `ilium-execution` bucket is empty, so the plugin spent its CPU budget | Yes, after `retry-after-ms` | Wait, then try again. Lower the cost of the work if this happens in normal use. |
| `quota-exceeded` without `retry-after-ms` | A stored or held quota is full, such as a storage bucket or a per-plugin count | Only after freeing something | Free space or drop work. A retry alone never succeeds. |
| `invalid-argument` | A value is over a fixed cap | No | Fix the input. Retrying the same input fails again. |
| `io` with `no-space` | The disk is low | After space is freed, or after a delay | Log it, keep the old value, and do not loop. |

A `rate-limited` or `quota-exceeded` error is not a plugin bug and does not count toward the restart rule. A `busy` error is not a failure either. Treat each as normal flow.

### What a refused call looks like in code

Every fallible call returns `result<T, plugin-error>`. The record carries `code`, `message` and an optional `retry-after-ms`. In Rust the fields are `code`, `message` and `retry_after_ms`. In TypeScript they are `code`, `message` and `retryAfterMs`.

The helper below turns the error into a delay. It uses only the values the host sends. It does not guess.

```rust
use ilium_plugin_sdk::{ErrorCode, PluginError};

/// Returns the delay in milliseconds before the call may be retried,
/// or None when a retry cannot help.
pub fn retry_delay_ms(error: &PluginError) -> Option<u32> {
    match error.code {
        ErrorCode::Busy | ErrorCode::RateLimited => {
            Some(error.retry_after_ms.unwrap_or(100).max(100))
        }
        ErrorCode::QuotaExceeded => error.retry_after_ms.map(|ms| ms.max(100)),
        _ => None,
    }
}
```

The same logic in TypeScript:

```ts
type PluginErrorLike = { code: string; retryAfterMs?: number };

// Returns the delay in milliseconds before the call may be retried,
// or undefined when a retry cannot help.
export function retryDelayMs(error: PluginErrorLike): number | undefined {
  switch (error.code) {
    case 'busy':
    case 'rate-limited':
      return Math.max(error.retryAfterMs ?? 100, 100);
    case 'quota-exceeded':
      return error.retryAfterMs === undefined ? undefined : Math.max(error.retryAfterMs, 100);
    default:
      return undefined;
  }
}
```

The `quota-exceeded` case with no `retry-after-ms` returns nothing, because a retry cannot succeed until the plugin frees storage. The `busy` and `rate-limited` cases always return a delay, because waiting is the only safe answer.

- **[PRF-010]** A plugin MUST NOT retry a refused call sooner than its `retry-after-ms` value, and MUST NOT retry a `quota-exceeded` error that has no `retry-after-ms` without first freeing the resource it names. Test: a fixture with a rate rule of 1 call per 2 seconds makes 10 attempts in 5 seconds; the attempts after the first are spaced by at least `retry-after-ms`; a fixture that fills a storage bucket and retries `put` without freeing space makes no progress.

### Retry with a timer, not with a loop

A retry inside a handler blocks the handler. It also uses the CPU of the plugin's bucket and the 20 ms deadline of the batch. A retry must be a timer that fires later, which the handler schedules and returns from at once. The host clock and timers are in [Clock and timers](/pages/api-clock-and-timers/), and a timer that must survive a restart is in [Durable timers](/pages/api-durable-timers/).

The pattern is:

1. The handler makes the call. If it succeeds, the handler is done.
2. If it is refused with a delay, the handler stores the pending work in memory and starts a timer for the delay.
3. The timer event fires in a later batch. The handler makes the call again.
4. After a fixed number of attempts, the handler gives up, logs one line at `warn` and drops the work.

- **[PRF-011]** A plugin that retries a refused call MUST schedule the retry with a timer and MUST return from the handler before the delay ends. A plugin MUST NOT wait for a retry with a sleep or a loop inside a handler. Test: a handler that receives `busy` returns within 20 ms; the retry happens in a later batch after the timer fires.
- **[PRF-012]** A retry loop MUST stop after a bounded number of attempts. (proposed, owner to confirm: the default bound is 5 attempts, with the delay doubling from `retry-after-ms` and capped at 60 seconds.) Test: a call that is always refused makes exactly 5 attempts and then logs one `warn` line.

### Waiting for a busy host

The `busy` error for pending requests means the plugin has 64 requests in flight by default, with a ceiling of 256 for T1. A plugin that starts requests faster than they complete fills that count and then receives `busy` for every new one.

A plugin counts its own requests in flight. It keeps the count below the host limit with room for other work. The plugin decreases the count when a `request-completed` arrives for the request.

- **[PRF-013]** A plugin SHOULD keep its own count of requests in flight below half of the `pending_requests` limit of its tier, so that a short burst does not reach `busy`. (proposed, owner to confirm: a half-of-limit rule is an author recommendation; the specification fixes the limit and the `busy` error only.) Test: a fixture that starts requests as fast as it can never has more than 32 in flight on T3.

### Events that arrive faster than the handler

The queue of a plugin side holds 1,024 events by default. Each event kind has one of three policies. A plugin author cannot change the policy, but the plugin can plan for it.

| Policy | Kinds (examples) | What the plugin sees |
|---|---|---|
| Coalesce | `tree-changed`, `screen-changed`, `signal-changed`, `attention-changed` | One event per key. The newest state replaces the older pending event. Read the current state, not a history. |
| Drop oldest | `output-matched` and the events marked drop-oldest in [Events](/pages/api-events/) | Old events are removed when the queue is full. The next batch starts with `events-dropped`, which gives the count. |
| Never dropped | Everything else except `message` | Nothing is dropped. A full queue restarts the plugin as a failure. |

A batch holds up to 256 events, and the host flushes it after 2 ms or when a latency-critical kind arrives. A handler that takes a long time for one batch delays the next batch, and at a full queue the never-dropped kinds lead to a restart.

- **[PRF-014]** A plugin MUST handle an `events-dropped` event by re-reading the state it depends on, and MUST NOT assume that a count of drop-oldest events is complete. Test: a fixture that receives `events-dropped` with a count of 40 re-reads its state and its final output matches a fixture that received every event.
- **[PRF-015]** A handler MUST NOT keep work in the event queue by returning late. A plugin that needs more time than the batch deadline MUST start a job, as described in [Moving slow work into a job](/pages/performance/#moving-slow-work-into-a-job). Test: a fixture that does 30 ms of work in `on-events` breaches the 20 ms deadline; the same work started as a job completes and the handler returns within 20 ms.

### Streams and the credit window

A stream of HTTP data or child output has a credit window of 256 KiB per stream. The host stops reading the socket or the pipe when the plugin holds that many unread bytes, and it resumes when the count falls to half. Chunks are at most 64 KiB. A plugin that reads its data promptly never sees the window fill. A plugin that holds on to the bytes, for example by queuing them in a list while it waits for a job, stops the stream. The peer then slows down through the operating system's own flow control.

Treat `http-stream-data` and `child-output` as a flow. Process each chunk in the handler, or copy it to a bounded buffer and start a job for the slow part. A buffer that grows without limit is the most common cause of a stalled stream.

- **[PRF-016]** A plugin MUST NOT hold more than 256 KiB of unprocessed stream bytes for one stream, and MUST process or release each chunk before the next batch arrives. Test: a fixture with a slow job receives a 10 MiB stream and its buffered bytes never exceed 256 KiB; the stream completes.

### Storage and disk: retry after space, not in a loop

A storage write that fails with `quota-exceeded` keeps the previous value. A write that fails with `io` and `no-space` also keeps the previous value. Neither is a plugin failure. A disk error does not come with `retry-after-ms`, so the delay before a retry is the plugin's own choice. (Author recommendation, proposed, owner to confirm: wait at least 10 seconds, retry at most a bounded number of times, and log one `error` line when giving up.) The host reacts to falling free space in steps, which are described in [Errors and limits](/pages/errors-and-limits/).

### Worked example: a status sender that respects the limits

A plugin updates a status segment in the tree every time a background scan makes progress. A scan of 5,000 files produces up to 5,000 updates. The `set-frame-slot` rate limit is 10 per second per slot, so most updates would be refused.

1. The scan writes its progress into a plain variable. It does not call the host for each file.
2. A timer fires every 250 ms. The handler reads the variable and makes one `set-frame-slot` call if the visible value changed.
3. A refused update returns `rate-limited` with a `retry-after-ms` value. The handler stores the newest value and does not retry the old one, because the next timer tick sends the newest value anyway.
4. The scan keeps running. The plugin never spends CPU on refused calls, and the status segment shows the newest value at most 250 ms late.

This is the pattern the rate limits expect. A plugin that retries each refused update in turn does the opposite. It spends its bucket on calls that cannot succeed.

### Limits of this page

- The host does not warn a plugin before it refuses a call. `ilium plugin stats` shows the use and limit of memory, the execution bucket, storage and disk. The pages read for this manual do not describe an author-visible count of pending requests, so PRF-013 relies on the plugin's own count. (Gap: confirm whether a pending-request figure is exposed to plugins.)
- The retry bounds in PRF-012 and the half-of-limit rule in PRF-013 are author recommendations. They are marked as proposals and need an owner decision before the manual treats them as requirements.

### See also

- [Durable timers](/pages/api-durable-timers/#overdue-bursts-and-sizing) for how a burst of overdue timers is paced.
- [Sizing memory and CPU](/pages/perf-sizing-memory-and-cpu/) for the budgets behind the refusals.
- [Benchmark workflow](/pages/perf-benchmark-workflow/) for measuring a handler that must return within its deadline.
- [Errors and limits](/pages/errors-and-limits/) for the error codes and their meaning.
- [Queues and backpressure](/pages/reference-host-scheduling/#queues-and-backpressure) for the host's queue policies in full.
- [Rate limits and retries](/pages/errors-and-limits/#rate-limits-and-retries) for the refused-submission outcome and the dedup key rules.
