---
layout: page
title: Patterns cookbook
permalink: /pages/guide-patterns-cookbook/
---

## Patterns cookbook

This page collects twelve recipes that most plugins need sooner or later: debounce, throttle, cache with a time limit, windowed lists, retry with backoff, idempotent submissions, state that survives a restart, stale-result guards, batch handling, slow work in jobs, talking between the two sides, and migrating stored data. Each recipe states the problem, lists the limits that shape the answer, gives Rust and TypeScript code, shows a test, and lists the traps.

The recipes use one idea over and over: keep the decision logic in a plain function or struct that takes the current time and the incoming event as arguments, and keep the host calls in a thin handler around it. The core is then testable with plain numbers. The fake host also has a fake clock (`advance_clock`, see [Test harness reference](/pages/reference-test-harness/)), so the tests below call the core directly where the logic is pure and use the fake host to check the glue and the timers.

Requirement ids of this page are in the range `AUT-001` to `AUT-099`. The complete list is in [Requirements for author guides](/pages/reference-requirements-author-guides/).

### You need

- A plugin that builds and activates. Start with [Your first plugin](/pages/getting-started-first-plugin/) if you have none.
- The grants named in each recipe. The clock, timers and storage need no grant.
- The limits below. Every number comes from [Errors and limits](/pages/reference-errors-and-limits/) and [Performance](/pages/performance/). This page does not define them again.

| Limit | Value | Page |
|---|---|---|
| Timer minimum, timers per plugin | 50 ms, 64 | [Clock and timers](/pages/api-clock-and-timers/) |
| `on-events` deadline | 20 ms by default (a tier 1 plugin may ask for up to 50 ms) | [Performance](/pages/performance/) |
| Storage key, value, bucket | 256 bytes, 1 MiB, 16 MiB (`storage_bucket_mib`) | [Storage](/pages/api-storage/) |
| Restore bytes from `deactivate` | 1 MiB | [Lifecycle](/pages/lifecycle/) |
| Dedup window | 10 minutes | [Requests and outcomes](/pages/requests-and-outcomes/) |
| Toast rate | 3 per 10 s | [Toast](/pages/api-toast/) |
| Tree limits | 4,096 nodes, 1,024 KiB | [UI model](/pages/ui-model/) |
| Topic payload | 64 KiB | [Messages and topics](/pages/api-messages-and-topics/) |

### The shape of every recipe

Rust plugins keep a `Core` struct with pure methods. A method receives `now` in milliseconds and returns what the handler should do, as a small enum. The handler turns the enum into host calls.

```rust
/// What the handler must do after the core decides.
pub enum Action {
    Nothing,
    ArmTimer { after_millis: u32 },
    CancelTimer,
    Run,
}
```

The TypeScript version is the same, with a discriminated union.

```ts
export type Action =
  | { tag: 'nothing' }
  | { tag: 'arm-timer'; afterMillis: number }
  | { tag: 'cancel-timer' }
  | { tag: 'run' };
```

A core MUST NOT call the clock itself, and a handler SHOULD read the clock once per batch and pass the value in. [AUT-001]

## Recipe 1: debounce

**Problem.** The user types or resizes, and your plugin gets an event for every change. You want to act once, a short time after the last change.

**Constraints.** A timer needs at least 50 ms. A plugin holds at most 64 timers. Timer events coalesce while they wait, so a late handler sees one `timer-fired`. There is no "reset" call. You restart a debounce by cancelling the old timer and setting a new one.

**Rule.** A debounce MUST cancel the pending timer before it arms a new one. Otherwise each change leaves a timer behind, and a busy editor reaches the limit of 64 held timers. [AUT-002]

**Rule.** The handler MUST compare the id of each `timer-fired` event with the id it stored, and ignore every other id. [AUT-003]

**Rust**

```rust
use ilium_plugin_sdk::host::timers;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::ids::TimerId;

pub const QUIET_MILLIS: u32 = 300;

#[derive(Default)]
pub struct Debounce {
    pending: Option<TimerId>,
}

impl Debounce {
    /// Call on every change. Restarts the quiet period.
    pub fn touch(&mut self) -> Result<(), PluginError> {
        if let Some(old) = self.pending.take() {
            timers::cancel(old);
        }
        self.pending = Some(timers::set(QUIET_MILLIS, None)?);
        Ok(())
    }

    /// Call for every timer-fired event. True when the quiet period ended.
    pub fn fired(&mut self, timer: TimerId) -> bool {
        if self.pending == Some(timer) {
            self.pending = None;
            return true;
        }
        false
    }

    /// Call from deactivate. Timers are lost on restart anyway; this keeps the count honest.
    pub fn stop(&mut self) {
        if let Some(old) = self.pending.take() {
            timers::cancel(old);
        }
    }
}
```

**TypeScript**

```ts
import * as timers from '@ilium/plugin-sdk/host/timers';

export const QUIET_MILLIS = 300;

export class Debounce {
  private pending: bigint | undefined;

  touch(): void {
    if (this.pending !== undefined) timers.cancel(this.pending);
    this.pending = timers.set(QUIET_MILLIS, undefined);
  }

  fired(timer: bigint): boolean {
    if (this.pending !== timer) return false;
    this.pending = undefined;
    return true;
  }

  stop(): void {
    if (this.pending !== undefined) timers.cancel(this.pending);
    this.pending = undefined;
  }
}
```

**Test.** The fake host moves time only through `advance_clock`, and `deliver` never runs timers. The test below types three times, 200 ms apart, and expects one action after the quiet period.

```rust
#[test]
fn three_changes_give_one_action() {
    let mut host = FakeHost::new();
    host.activate::<Editor>();
    for _ in 0..3 {
        host.deliver(vec![text_changed()]);
        host.advance_clock(200);
    }
    assert!(host.submitted().is_empty());      // 200 ms after the last change
    host.advance_clock(100);                   // 300 ms: the quiet period ends
    assert_eq!(host.submitted().len(), 1);
}

#[test]
fn unknown_timer_is_ignored() {
    let mut host = FakeHost::new();
    host.activate::<Editor>();
    let submissions = host.deliver(vec![Event::TimerFired(TimerId(999))]);
    assert!(submissions.is_empty());
}
```

**Traps.**

- A debounce delay MUST be 50 ms or more, because a shorter delay is refused with `invalid-argument`. [AUT-004]
- Timers are lost on restart. A debounce that was pending when the plugin restarted does not fire. Treat the next change as the start of a new quiet period.
- Do not use a repeating timer and a "last change" stamp to simulate a debounce. It wakes the plugin every period for nothing.

## Recipe 2: throttle and rate limits

**Problem.** An event arrives many times per second, and your reaction is a host call with a rate limit: a toast, a status segment, a meter update.

**Constraints.** A toast is limited to 3 per 10 s. A status segment and a frame slot are limited to 10 per second per slot. A meter family is limited to 10 per second. Over the rate, the call fails with `rate-limited` and the error carries `retry-after-ms`.

**Rule.** A plugin SHOULD keep its own send rate under the host rate, and SHOULD NOT depend on `rate-limited` as flow control. [AUT-005]

**Rule.** After `rate-limited`, the plugin MUST wait at least `retry-after-ms` before it sends the same kind of call again. [AUT-006]

**Rust**

```rust
/// Allows one action per `interval_millis`. The caller passes `now`.
pub struct Throttle {
    interval_millis: u64,
    last: Option<u64>,
}

impl Throttle {
    pub fn new(interval_millis: u64) -> Self {
        Self { interval_millis, last: None }
    }

    pub fn allow(&mut self, now: u64) -> bool {
        match self.last {
            Some(last) if now.saturating_sub(last) < self.interval_millis => false,
            _ => {
                self.last = Some(now);
                true
            }
        }
    }

    /// Call when the host answered rate-limited with a retry delay.
    pub fn back_off(&mut self, now: u64, retry_after_ms: u32) {
        self.last = Some(now + u64::from(retry_after_ms).saturating_sub(self.interval_millis));
    }
}
```

**TypeScript**

```ts
export class Throttle {
  private last: bigint | undefined;
  constructor(private readonly intervalMillis: bigint) {}

  allow(now: bigint): boolean {
    if (this.last !== undefined && now - this.last < this.intervalMillis) return false;
    this.last = now;
    return true;
  }

  backOff(now: bigint, retryAfterMs: number): void {
    const wait = BigInt(retryAfterMs);
    this.last = now + (wait > this.intervalMillis ? wait - this.intervalMillis : 0n);
  }
}
```

**Test.** Both tests call the core with plain numbers, so no clock is needed.

```rust
#[test]
fn throttle_allows_one_per_interval() {
    let mut throttle = Throttle::new(100);
    assert!(throttle.allow(1_000));
    assert!(!throttle.allow(1_050));
    assert!(throttle.allow(1_100));
}

#[test]
fn back_off_delays_the_next_send() {
    let mut throttle = Throttle::new(100);
    assert!(throttle.allow(0));
    throttle.back_off(0, 2_000);
    assert!(!throttle.allow(1_500));
    assert!(throttle.allow(2_000));
}
```

**Traps.** A throttle MUST take its time from `monotonic-millis`, because wall time can jump backwards and then `now - last` underflows in unsigned arithmetic. The code above uses `saturating_sub` for this reason. [AUT-007]

## Recipe 3: cache with a time limit

**Problem.** You fetch something slow, such as an HTTP answer or a scan, and you want to reuse it for a while.

**Constraints.** Buckets are `global`, `project`, `session` and `shared`. The first three are durable and count against `storage_bucket_mib` (16 MiB for tiers 2 and 3). The `shared` bucket is a cache: the host may clear it at any time, it is shared by all sessions of the user, and it counts against `shared_cache_mib`. A value is at most 1 MiB. The `lease` call lets exactly one caller per key refresh. A folder named `/cache` in the `fs` scope is a different thing: a file area, counted with `data` in `disk_mib`.

| Data | Where | Why |
|---|---|---|
| Can be rebuilt, shared by sessions | `shared` bucket | The host may clear it. Entries evict oldest first. |
| Can be rebuilt, large, a file | `/cache` in `fs` | Files suit big data. The host may evict it. |
| Cannot be rebuilt (user choices, counters) | `global`, `project` or `session` | Durable. Not cleared by the host. |
| Rebuilt per run | In memory | Lost on restart. Cheapest. |

**Rule.** Data that the plugin can rebuild MUST go to the `shared` bucket or to `/cache`, never to a durable bucket, so that it does not fill the 16 MiB bucket. [AUT-008]

**Rule.** A cached value MUST carry its fetch time as `now-unix-millis` inside the stored bytes, and the reader MUST compare it with the current time. The host does not expire entries by age. [AUT-009]

**Rule.** When several sessions can refresh the same entry, only the caller whose `lease` returned true MAY refresh it. [AUT-010]

**Rust**

```rust
use ilium_plugin_sdk::host::storage::{self, Bucket};

const TTL_MILLIS: u64 = 10 * 60 * 1000;

fn encode(fetched_at: u64, body: &[u8]) -> Vec<u8> {
    let mut out = fetched_at.to_le_bytes().to_vec();
    out.extend_from_slice(body);
    out
}

fn decode(bytes: &[u8]) -> Option<(u64, &[u8])> {
    let (head, body) = bytes.split_at_checked(8)?;
    Some((u64::from_le_bytes(head.try_into().ok()?), body))
}

/// Returns fresh bytes, stale bytes while another session refreshes, or None.
pub fn read_cached(key: &str, now: u64) -> Option<Vec<u8>> {
    let stored = storage::get(Bucket::Shared, key);
    let fresh = stored
        .as_deref()
        .and_then(decode)
        .filter(|(at, _)| now.saturating_sub(*at) < TTL_MILLIS);
    if let Some((_, body)) = fresh {
        return Some(body.to_vec());
    }
    if storage::lease(key, 60_000) {
        return None; // the caller fetches, then calls write_cached
    }
    stored.as_deref().and_then(decode).map(|(_, body)| body.to_vec())
}

pub fn write_cached(key: &str, now: u64, body: &[u8]) {
    let _ = storage::put(Bucket::Shared, key, &encode(now, body));
}
```

**TypeScript**

```ts
import * as storage from '@ilium/plugin-sdk/host/storage';

const TTL_MILLIS = 10n * 60n * 1000n;

export function readCached(key: string, now: bigint): Uint8Array | undefined {
  const stored = storage.get('shared', key);
  const parsed = stored && stored.length >= 8 ? split(stored) : undefined;
  if (parsed && now - parsed.at < TTL_MILLIS) return parsed.body;
  if (storage.lease(key, 60_000)) return undefined;
  return parsed?.body;
}

function split(bytes: Uint8Array): { at: bigint; body: Uint8Array } {
  const view = new DataView(bytes.buffer, bytes.byteOffset, 8);
  return { at: view.getBigUint64(0, true), body: bytes.subarray(8) };
}
```

**Test.** `encode` and `decode` are pure, so test them without the host. `read_cached` and `write_cached` take `now` as an argument (the handler reads the clock once per batch, [AUT-001]), so the TTL branch can be tested with fixed numbers.

**Traps.**

- A `put` into `shared` of a value over 1 MiB fails with `invalid-argument`, not `quota-exceeded`.
- The lease is not a lock. It prevents duplicate work, not races. Two callers can still write. The last write wins. That is fine for a cache.
- Never keep the only copy of user data in `shared`. The host may clear it.

## Recipe 4: lists larger than the tree

**Problem.** You have thousands of rows, such as search hits, log lines or files. A tree is limited to 4,096 nodes and 1,024 KiB, and a render must finish in 10 ms.

**Constraints.** The `%list` and `table` widgets keep `selected` and `offset` as host-owned state, which the plugin reads through `ui.widget-state`. There is no view-level virtual list. Virtual-row providers exist only for sidebar rows of the tree, with cursors and a Load more row. The caps `check-list` (256 items) and `list-editor` (64 items) are enforced at render time: a larger one traps with `ui.invalid-tree`.

**Rule.** A view MUST render only the rows in the visible window plus a small margin, and MUST NOT put all rows into the tree. The ceiling is 4,096 nodes and 1,024 KiB for the whole tree. [AUT-011]

**Rule.** A view that hides rows behind a window MUST show how many rows exist and where the window is, for example `41 to 60 of 3,212`, so that the user knows more exist. [AUT-012]

**Rust**

```rust
pub struct Window {
    pub first: usize,
    pub len: usize,
}

/// Window for a list of `total` rows, a host offset and a visible height.
pub fn window(total: usize, offset: usize, visible: usize) -> Window {
    let margin = 4;
    let first = offset.saturating_sub(margin).min(total);
    let end = (offset + visible + margin).min(total);
    Window { first, len: end - first }
}
```

**TypeScript**

```ts
export function window(total: number, offset: number, visible: number): { first: number; len: number } {
  const margin = 4;
  const first = Math.min(Math.max(offset - margin, 0), total);
  const end = Math.min(offset + visible + margin, total);
  return { first, len: end - first };
}
```

A plugin that cannot hold all rows in memory pages them from its source. A catalogue provider returns pages with `limit` (1 to 256), a `next` cursor and `total`. A sidebar virtual-row provider returns rows with a cursor and the host adds the Load more row. See [Catalogs](/pages/api-catalogs/) and [Virtual rows provider](/pages/api-virtual-rows-provider/).

<figure>
<pre class="term">╭  ≡ ● · Search hits ────────────────────────╮
│ 41 to 60 of 3,212                          │
│ src/lib.rs:41                              │
│ src/lib.rs:57                              │
│ src/app.rs:3                               │
│ ...                                        │
│ Load more                                  │
╰────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The count line tells the user that the window is part of a larger list.</figcaption>
</figure>

**Test.**

```rust
#[test]
fn window_is_clamped_to_the_list() {
    let w = window(10, 8, 5);
    assert_eq!(w.first, 4);
    assert_eq!(w.first + w.len, 10);
    assert_eq!(window(0, 0, 5).len, 0);
}
```

**Traps.** Do not size the tree by the number of rows. A width change or a resize calls `render` again, and a plugin that rebuilds 3,000 rows per render misses the 10 ms deadline.

## Recipe 5: retry with backoff

**Problem.** A request fails, and some failures are worth another try.

**Constraints.** A request completes with an outcome. A failed one carries an error code. Of the fifteen codes, only some describe a passing problem.

| Code | Retry? | Why |
|---|---|---|
| `timed-out`, `busy`, `network`, `rate-limited` | Yes, with backoff | The cause can pass. For `rate-limited`, wait at least `retry-after-ms`. |
| `io` | Maybe, with backoff | The error can pass or repeat. Stop after a few tries. |
| `quota-exceeded` | After freeing space | A retry fails the same way until something is removed. |
| `not-permitted`, `revoked`, `unsupported` | No | The user or the host decided. A retry cannot change it. |
| `invalid-argument`, `not-found`, `conflict` | No | The request is wrong. Change it. |
| `cancelled` | Only on the next trigger | The plugin or the user stopped it. |
| `pane-not-ready` | Yes, after the next pane event | The pane process has not started. |
| `internal` | Once, then report | A host fault. |

**Rule.** A plugin MUST NOT retry `not-permitted`, `revoked`, `invalid-argument` or `unsupported`. [AUT-013]

**Rule.** A retry loop MUST have a maximum attempt count and a growing delay. A suggested schedule is 1 s, 2 s, 4 s, up to 60 s, with at most 5 attempts. [AUT-014]

**Rust**

```rust
pub fn is_transient(code: ErrorCode) -> bool {
    matches!(
        code,
        ErrorCode::TimedOut | ErrorCode::Busy | ErrorCode::Network | ErrorCode::RateLimited | ErrorCode::Io
    )
}

/// Delay before attempt `attempt` (1-based), or None when to give up.
pub fn backoff_millis(attempt: u32, retry_after_ms: Option<u32>) -> Option<u32> {
    const MAX_ATTEMPTS: u32 = 5;
    if attempt > MAX_ATTEMPTS {
        return None;
    }
    let base = 1_000u32.saturating_mul(1 << (attempt - 1)).min(60_000);
    Some(base.max(retry_after_ms.unwrap_or(0)))
}
```

**TypeScript**

```ts
export function backoffMillis(attempt: number, retryAfterMs?: number): number | undefined {
  const MAX_ATTEMPTS = 5;
  if (attempt > MAX_ATTEMPTS) return undefined;
  const base = Math.min(1000 * 2 ** (attempt - 1), 60_000);
  return Math.max(base, retryAfterMs ?? 0);
}
```

The retry itself is a timer: `timers.set(delay, None)`, and the `timer-fired` handler starts the request again. The attempt count lives in memory. A restart loses it, which is the right result, since the failure may be gone.

**Test.**

```rust
#[test]
fn backoff_doubles_and_gives_up() {
    assert_eq!(backoff_millis(1, None), Some(1_000));
    assert_eq!(backoff_millis(3, None), Some(4_000));
    assert_eq!(backoff_millis(2, Some(9_000)), Some(9_000));
    assert_eq!(backoff_millis(6, None), None);
}

#[test]
fn failed_request_is_logged_not_trapped() {
    let mut host = FakeHost::new().grant("http:api.example.com");
    host.activate::<Fetcher>();
    host.deliver(vec![command("com.example.fetch/run")]);
    let request = host.pending_requests()[0];
    host.complete(request, Outcome::Failed(ErrorCode::Network));
    assert!(host.logs().iter().any(|line| line.message.contains("retry")));
}
```

## Recipe 6: idempotent submissions

**Problem.** An event can repeat, and your reaction must not repeat with it: a notice, a decoration, a send-input.

**Constraints.** A submission may carry a `dedup-key` of 1 to 128 characters from `A-Z`, `a-z`, `0-9`, `.`, `_`, `:`, `/` and `-`. The key is scoped to the plugin id and stays reserved for 10 minutes. A second submission with the same key completes as `duplicate` with a reference to the first.

**Rule.** A submission whose repeat would harm the user (a notice, a sent line) MUST carry a `dedup-key` built from what makes the action unique, such as an id and a revision. [AUT-015]

**Rule.** A `dedup-key` MUST NOT be reused for different content within 10 minutes. The second submission is dropped as `duplicate` even though its content differs. [AUT-016]

**Rust**

```rust
fn finished_notice(node: u64, revision: u64) -> Submission {
    let mut submission = Submission::new(Intent::Notify(notice_for(node)));
    submission.dedup_key = Some(format!("finished:{node}:{revision}"));
    submission
}
```

**TypeScript**

```ts
function finishedNotice(node: bigint, revision: bigint): Submission {
  return {
    intent: { tag: 'notify', val: noticeFor(node) },
    correlation: nextCorrelation++,
    dedupKey: `finished:${node}:${revision}`,
  };
}
```

**Test.** Complete the same key twice in the fake host and check that `outcomes()` shows `Outcome::Duplicate` for the second. Check that the key string matches `^[A-Za-z0-9._:/-]{1,128}$`.

**Traps.** The window is 10 minutes. After it, the same key is accepted again. If an action must be once for all time, record it in storage too.

## Recipe 7: state that survives a restart

**Problem.** The plugin stops and starts, and some state must come back.

**Constraints.** `deactivate` returns up to 1 MiB of restore bytes, with a deadline of 500 ms. The bytes come back as `restored` in the next `activate`. After a trap, the instance is dropped and `deactivate` is never called, so nothing is saved. The restore bytes are not a migration path, and the host drops them when `data_version` differs from the build that wrote them.

**Rule.** The first byte of restore bytes MUST be a format version, and `activate` MUST check the version and the length before it reads. A mismatch MUST fall back to defaults. [AUT-017]

**Rule.** State that must outlive a trap MUST be written to storage when it changes, not only in `deactivate`. [AUT-018]

**Rust**

```rust
const FORMAT: u8 = 1;

#[derive(Default)]
pub struct Counter {
    pub value: u32,
}

impl Counter {
    pub fn save(&self) -> Vec<u8> {
        let mut out = vec![FORMAT];
        out.extend_from_slice(&self.value.to_le_bytes());
        out
    }

    pub fn load(bytes: &[u8]) -> Self {
        match bytes {
            [FORMAT, a, b, c, d] => Self { value: u32::from_le_bytes([*a, *b, *c, *d]) },
            _ => Self::default(),
        }
    }
}
```

**TypeScript**

```ts
const FORMAT = 1;

export function save(value: number): Uint8Array {
  const out = new Uint8Array(5);
  out[0] = FORMAT;
  new DataView(out.buffer).setUint32(1, value, true);
  return out;
}

export function load(bytes: Uint8Array | undefined): number {
  if (!bytes || bytes.length !== 5 || bytes[0] !== FORMAT) return 0;
  return new DataView(bytes.buffer, bytes.byteOffset).getUint32(1, true);
}
```

**Test.** The core test needs no host. The host test uses the lifecycle methods of the fake host (`deactivate`, `restart_with`).

```rust
#[test]
fn restore_round_trip() {
    let mut host = FakeHost::new();
    host.activate::<Editor>();
    let bytes = host.deactivate("upgrade");
    host.restart_with(bytes.clone());
    assert_eq!(bytes[0], 1);                   // the format byte
}

#[test]
fn bad_restore_bytes_fall_back_to_defaults() {
    assert_eq!(Counter::load(&[]).value, 0);
    assert_eq!(Counter::load(&[9, 1, 0, 0, 0]).value, 0);
    let saved = Counter { value: 7 }.save();
    assert_eq!(Counter::load(&saved).value, 7);
}
```

## Recipe 8: ignore stale results

**Problem.** The user changes the query while a request for the old one is running. The old answer must not overwrite the new one.

**Rule.** A plugin that starts work for a changing input MUST keep a generation number, store it with the request, and discard a completion whose generation is not the current one. [AUT-019]

**Rust**

```rust
use std::collections::HashMap;

#[derive(Default)]
pub struct Latest {
    generation: u64,
    by_request: HashMap<u64, u64>,
}

impl Latest {
    pub fn start(&mut self, request: u64) {
        self.generation += 1;
        self.by_request.insert(request, self.generation);
    }

    /// True only for the newest request.
    pub fn accept(&mut self, request: u64) -> bool {
        self.by_request.remove(&request) == Some(self.generation)
    }
}
```

**TypeScript**

```ts
export class Latest {
  private generation = 0;
  private byRequest = new Map<bigint, number>();
  start(request: bigint): void {
    this.generation += 1;
    this.byRequest.set(request, this.generation);
  }
  accept(request: bigint): boolean {
    const mine = this.byRequest.get(request);
    this.byRequest.delete(request);
    return mine === this.generation;
  }
}
```

**Test.** Start two requests, accept the first (false), accept the second (true). Also check that an unknown id is rejected.

A request can also be cancelled when it supports cancellation (jobs do, through `jobs.cancel`). Cancelling saves work but does not replace the guard, because a completion may already be in the queue.

## Recipe 9: handle a whole batch

**Problem.** `on-events` receives a batch, not one event.

**Rule.** The handler MUST process every event of the batch in order, and MUST return before the event deadline (20 ms, 50 ms for tier 1). [AUT-020]

**Rule.** After an `events-dropped` entry, the handler MUST rebuild its picture from a read call instead of trusting its incremental state. [AUT-021]

```rust
fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
    let now = clock::monotonic_millis();
    let mut submissions = Vec::new();
    for event in batch {
        match event {
            Event::EventsDropped(_) => self.resync(),
            Event::TimerFired(timer) => self.on_timer(timer, now, &mut submissions),
            other => self.core.apply(other, now, &mut submissions),
        }
    }
    submissions
}
```

The batch gives you a free coalescing step. If three `agent-activity-changed` events for one node arrive together, apply them all and submit one decoration for the final state.

## Recipe 10: slow work belongs in a job

**Problem.** A step takes more than a few milliseconds: parsing a large file, hashing, formatting.

**Rule.** Work that can take longer than 10 ms MUST run as a job or a request. A handler that spends its 20 ms deadline on work traps the instance. [AUT-022]

A job runs under 2 s CPU, 20 s wall and 64 MiB, and reports progress. Start it from the handler, keep its request id, and read its output after `request-completed`. The full worked example, with cancellation and progress, is in [Performance](/pages/performance/) and [Jobs](/pages/api-jobs/). Combine it with recipe 8 so a result for an old input is dropped.

## Recipe 11: choose how the two sides talk

| Need | Use | Limits |
|---|---|---|
| News nobody answers | Topic publish | Payload 64 KiB, valid JSON, dropped and counted when the queue is full |
| A direct note to a known plugin | Message | Never dropped. A full queue marks the receiver stuck. |
| One answer from one client now | Client request | At most 16 kinds, timeout 1 to 30,000 ms |
| Durable data that both sides need | Each side keeps its own copy in storage; the other side asks with a client request | Atomic `put` per key, no multi-key transaction. Do not use the `shared` bucket: the host may clear it. |

**Rule.** A server side that needs data from the client MUST use a client request with an explicit timeout, and MUST handle `timed-out` and `not-found` (the client is gone). [AUT-023]

See [Client requests](/pages/api-client-requests/) and [Messages and topics](/pages/api-messages-and-topics/).

## Recipe 12: migrate stored data

**Problem.** A new version stores data differently.

**Constraints.** The host owns the migration. The manifest key `data_version` (an unsigned 32-bit integer, default 1) names the layout. When a new version declares a higher number, the host takes a backup, calls the optional export `migrator.migrate(from, to)` once per bucket per update (global bucket at first activation, each project or session bucket when its session first activates the plugin), stamps the new number when it succeeds, and rolls back when it fails. The `shared` bucket is not migrated: the host clears it when `data_version` changes. The full rules, the backup, the downgrade case and the tests are in [Data migration](/pages/guide-data-migration/). `put` is atomic for one key only, so a migration that touches several keys must be safe to run again.

**Rule.** A plugin that changes how it stores data MUST raise `data_version` and MUST provide a `migrator` export that converts the old layout, instead of reading both layouts inside `activate`. [AUT-024]

**Rule.** A `migrate` function MUST be idempotent and MUST write the new value of a key before it removes the old one, so that an interrupted run can start again from the same arguments. [AUT-025]

```rust
fn migrate(from: u32, to: u32) -> Result<(), PluginError> {
    if from < 2 && to >= 2 {
        if let Some(old) = storage::get(Bucket::Global, "notes") {
            storage::put(Bucket::Global, "notes.v2", &upgrade_notes(&old))?;
            storage::remove(Bucket::Global, "notes");
        }
    }
    Ok(())
}
```

If `notes` is already gone, the step does nothing, so a second run is harmless. Compare with the version rules in [Compatibility and upgrades](/pages/guide-compatibility-and-upgrades/).

## Edge cases and errors

| Situation | What happens | Recipe |
|---|---|---|
| Timer id not tracked | Ignore the event | 1 |
| Wall clock jumps backwards | Use monotonic time for intervals | 2 |
| `shared` cleared by the host | The reader sees no value and refetches | 3 |
| Window offset beyond the list | Clamp, show an empty window | 4 |
| `rate-limited` with `retry-after-ms` | Wait at least that long | 2, 5 |
| Same `dedup-key`, different content | Second submission is a duplicate | 6 |
| Restore bytes over 1 MiB | Dropped with a warn line | 7 |
| Trap | No `deactivate`, no restore bytes | 7 |
| `events-dropped` | Resync from a read | 9 |
| Completion for an old input | Discard by generation | 8 |

## What the user sees

Nothing of these patterns directly. They see the effects: a panel that settles after typing, a notice that appears once, a list that says `41 to 60 of 3,212`, and a plugin that is not disabled after a restart. The failure cases show up in `ilium plugin logs <id>` (see [Debugging](/pages/debugging/)).

## See also

- [Quality checklist](/pages/guide-quality-checklist/) for a review list that covers these patterns
- [Troubleshooting](/pages/guide-troubleshooting/) when a pattern misbehaves
- [Performance](/pages/performance/) for deadlines and budgets
- [Testing](/pages/testing/) for the fake host
- [Requests and outcomes](/pages/requests-and-outcomes/) for outcomes and error codes

API version checked: `ilium:plugin@0.1.0`.
