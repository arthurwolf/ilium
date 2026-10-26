---
layout: page
title: Scheduling, deadlines and cancellation
permalink: /pages/reference-host-scheduling/
---

## Scheduling, deadlines and cancellation

This page is the single normative source for how plugin calls are scheduled: which thread runs which call, how calls into one instance are ordered, the deadline of every call class and what happens when it is missed, what happens to work in flight when an instance traps, how host work is cancelled, and how queues push back. Other pages that mention a deadline or a limit repeat the numbers of this page and link here. A test author derives the timing and ordering tests from the requirement ids `[HST-180]` to `[HST-216]`, which are collected in [Requirements: kernel and host](/pages/reference-requirements-kernel-host/).

## You will need

- [Host architecture](/pages/reference-host-architecture/) for the processes and hosts.
- [Host runtime](/pages/reference-host-runtime/) for the epoch ticker and the hard and soft deadline mechanism.
- [Events](/pages/api-events/) for the queue policy of each event kind, and [Requests and outcomes](/pages/requests-and-outcomes/) for submissions.

## Thread model

Every plugin-hosting process has the same three kinds of thread besides its own main threads: the plugin executor, the epoch ticker and the job worker bank. The client has two more: the UI thread and the scene thread.

| Thread or pool | Process | Name | Count | Runs |
|---|---|---|---|---|
| Server main runtime | Server | `ilium-server` runtime | as today | Kernel work, the IPC loop, the dispatcher task `plugin-dispatch`, helper I/O tasks. It never runs guest code |
| Plugin executor | Server and client | `ilium-plugin-N` | server `clamp(cores / 2, 2, 4)`, client `clamp(cores / 4, 1, 2)` | Every guest call except scene frames and job runs: `activate`, `deactivate`, `on-events`, `views.render`, annotator, detector, hook and provider exports, service and inference calls |
| Epoch ticker | Every process with an Engine | `ilium-plugin-epoch` | 1 | Increases the Engine epoch every 10 ms |
| Job worker bank | Server and client | `ilium-job-N` | `min(4, cores / 2)`, at least 1 | Job instances (`job-runner.run`) |
| Client UI thread | Client | the client main thread | 1 | Input, layout, painting, hit testing. It never calls Wasmtime |
| Scene thread | Client | `ilium-plugin-scene` | 1 | Scene `renderer` frame calls |
| CLI main thread | CLI process | main | 1 | The single plugin executor of that process is a one-thread pool |

- **[HST-180]** Guest code MUST run only on the plugin executor, the scene thread, the job worker bank or (in a helper) the helper call loop. It MUST NOT run on the server main runtime, the client UI thread or the epoch ticker.
- **[HST-181]** No plugin call MUST ever run on the client UI thread. The UI thread MUST send work to the plugin executor through a bounded channel and MUST apply results on a later frame.
- **[HST-182]** The plugin executor MUST have `clamp(cores / 2, 2, 4)` threads in the server and `clamp(cores / 4, 1, 2)` threads in the client. The job worker bank MUST have `min(4, cores / 2)` threads, at least one.
- **[HST-183]** The scene thread MUST be a single dedicated thread per client, and a scene frame MUST NOT occupy a plugin executor thread.

A client UI frame therefore never waits for a plugin. The UI thread draws the last result that it holds: the last `ui-tree` of each view, the last complete scene frame, the retained decorations.

<figure>
<svg viewBox="0 0 440 230" role="img" aria-label="Threads of the client: UI thread, plugin executor, scene thread, epoch ticker and job bank with the channels between them">
<title>Client threads</title>
<desc>The UI thread sends events and render requests to the plugin executor through a bounded channel and reads results. The scene thread produces frames into a double buffer that the UI thread reads. The epoch ticker acts on the executor, scene thread and job bank. The job bank runs job instances.</desc>
<defs>
<marker id="hs-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
<path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
</marker>
</defs>
<rect x="8" y="20" width="120" height="44" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
<text x="68" y="40" font-size="11" text-anchor="middle" fill="currentColor">UI thread</text>
<text x="68" y="54" font-size="9" text-anchor="middle" fill="currentColor">input, layout, paint</text>
<rect x="170" y="20" width="130" height="44" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
<text x="235" y="40" font-size="11" text-anchor="middle" fill="currentColor">Plugin executor</text>
<text x="235" y="54" font-size="9" text-anchor="middle" fill="currentColor">on-events, render, hooks</text>
<rect x="170" y="100" width="130" height="44" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
<text x="235" y="120" font-size="11" text-anchor="middle" fill="currentColor">Scene thread</text>
<text x="235" y="134" font-size="9" text-anchor="middle" fill="currentColor">renderer frames</text>
<rect x="332" y="20" width="100" height="44" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
<text x="382" y="40" font-size="11" text-anchor="middle" fill="currentColor">Job bank</text>
<text x="382" y="54" font-size="9" text-anchor="middle" fill="currentColor">job instances</text>
<rect x="170" y="176" width="130" height="34" rx="6" fill="none" stroke="currentColor"/>
<text x="235" y="197" font-size="10" text-anchor="middle" fill="currentColor">Epoch ticker, 10 ms</text>
<line x1="128" y1="34" x2="170" y2="34" stroke="currentColor" marker-end="url(#hs-arrow)"/>
<text x="132" y="28" font-size="9" fill="currentColor">events</text>
<line x1="170" y1="52" x2="128" y2="52" stroke="currentColor" marker-end="url(#hs-arrow)"/>
<text x="136" y="64" font-size="9" fill="currentColor">trees</text>
<line x1="170" y1="122" x2="100" y2="64" stroke="currentColor" marker-end="url(#hs-arrow)"/>
<text x="106" y="104" font-size="9" fill="currentColor">frames</text>
<line x1="235" y1="176" x2="235" y2="144" stroke="currentColor" marker-end="url(#hs-arrow)"/>
<line x1="300" y1="190" x2="382" y2="190" stroke="currentColor"/>
<line x1="382" y1="190" x2="382" y2="64" stroke="currentColor" marker-end="url(#hs-arrow)"/>
</svg>
<figcaption>Dashed boxes run guest code. The UI thread only sends work and reads results. The ticker is the only thread that advances time for deadlines.</figcaption>
</figure>

## Per-instance serialization and fairness

A Store can run one call at a time. The host MUST NOT call into one instance from two threads, and MUST NOT interleave two calls.

Each plugin side has an **instance queue** of pending calls. The dispatcher picks the next call by priority, then by arrival order.

| Priority | Calls |
|---|---|
| 1 (highest) | `activate`, `deactivate` |
| 2 | Calls with a waiting caller: hook handlers, `collect`, `service-provider`, `inference-provider`, `menu-items`, `query-catalog`, `list-virtual-children`, `tokens`, `settings-validate`, `detector`, client-request answers |
| 3 | `on-events` |
| 4 | `views.render` (after `on-events`, so a render sees the state that the events produced) |
| 5 (lowest) | `annotate` |

Scene frames and job runs are not in this queue, because they run in their own Stores.

Between plugins the dispatcher is **round robin**: it keeps one ready list of plugin sides that have queued calls and gives each side one call per turn. A side gets at most 4 calls per 10 ms tick on one executor pass, so one busy plugin cannot starve the others.

- **[HST-184]** The host MUST run at most one call at a time in one Store and MUST pick the next call by the priority table, then by arrival order.
- **[HST-185]** The dispatcher MUST serve plugin sides round robin and MUST give a side at most 4 calls per 10 ms tick.
- **[HST-186]** One plugin's slow call MUST NOT delay another plugin's call by more than the other plugins' calls ahead of it in the round robin, and MUST NOT delay a client frame at all.

The cost of waiting in the instance queue is not part of the call's own deadline (the Store deadline starts when the call starts), but a call with a waiting caller has a second, **caller deadline** that starts when the call was queued. A call that cannot start within its caller deadline is abandoned and counted as a breach of the same class. The caller deadline equals the call's own deadline, except for `on-events` and `views.render`, which have none because nobody waits for them.

- **[HST-187]** A queued call with a waiting caller MUST be abandoned, with the same effect as a soft breach, when it has not started within its own deadline measured from the time it was queued.

## Call classes

This is the single normative table of calls. [Runtime model](/pages/reference-runtime-model/#2-calls-deadlines-and-interruption) and [Performance](/pages/performance/) link to it and copy its values exactly. A **hard** deadline traps the instance when it fires. A **soft** deadline marks the call late, so the host discards its result and counts a breach. A late call that also passes a grace period of 100 ms past the soft deadline is trapped. A soft class has a **streak limit**: that many breaches in a row, without a timely call in between, trap the instance. "Counts" says whether the effect is a failure under the restart rule.

All defaults are the same for every tier. The T1 column is the ceiling that a bundled plugin may ask for in its manifest `limits`, and the T2 and T3 column is the ceiling for third-party plugins. The values shown in brackets on earlier pages, for example `20 ms (50 ms)` in [Exports](/pages/api-exports/), are these T1 ceilings, not T1 defaults.

| Call class | Side | Default | T1 ceiling | T2/T3 ceiling | Kind | Effect on breach | Counts |
|---|---|---|---|---|---|---|---|
| `activate` | all | 2,000 ms | 5,000 ms | 2,000 ms | hard, yields | Trap; activation fails | yes |
| `deactivate` | all | 500 ms | 1,000 ms | 500 ms | hard | Instance dropped; no restore bytes kept; logged at warn | no |
| `on-events`, batch without a command | server, client | 20 ms | 50 ms | 20 ms | hard | Trap | yes |
| `on-events`, batch with a `command-invoked` event | server, client | 50 ms | 100 ms | 50 ms | hard | Trap | yes |
| `views.render` | client | 10 ms | 20 ms | 10 ms | hard | Trap | yes |
| `annotate` | client | 10 ms | 20 ms | 10 ms | soft, streak 3 | The annotation is skipped (late result dropped); the third breach in a row traps | trap only |
| `renderer` frame | scene | frame budget, at most 50 ms | 50 ms | 50 ms | soft, streak 30 | Frame skipped; 30 in a row trap | trap only |
| `detector` identify and classify | server | 20 ms | 50 ms | 20 ms | soft, streak 3 | Previous classification kept, no event; third in a row traps | trap only |
| `migrate` | all | 30,000 ms | 60,000 ms | 30,000 ms | hard | Migration fails, backup restored, update rolled back | no |
| `hook-handler` | owner side | 20 ms (5 to 50 ms by manifest) | 50 ms | 50 ms | soft | Counts as `pass`; third in a row disables the extension | no |
| `menu-provider.menu-items` | client | 20 ms | 20 ms | 20 ms | soft, streak 3 | Static entries only | trap only |
| `catalog-provider.query-catalog` | client | 20 ms | 20 ms | 20 ms | soft, streak 3 | Cached page used | trap only |
| `virtual-rows-provider.list-virtual-children` | server | 20 ms | 20 ms | 20 ms | soft, streak 3 | Spinner row stays | trap only |
| `token-provider.tokens` | client | 10 ms | 10 ms | 10 ms | soft, streak 3 | Lines stay plain; the host retries | trap only |
| `settings-validate` | client | 100 ms | 100 ms | 100 ms | soft, streak 3 | The save is refused, the previous value kept; the breach counts as a failure | yes |
| `service-provider` call | provider side | `command_deadline_ms` 50 ms | 100 ms | 50 ms | hard | Trap the provider; the caller's request completes `failed(internal)`; other deferred calls of the provider complete `failed(cancelled)` | yes |
| `inference-provider` `complete` and `models` | server | `command_deadline_ms` 50 ms | 100 ms | 50 ms | hard | Trap; the request is `failed(internal)` | yes |
| `job-runner.run`, CPU time | job instance | 2,000 ms | 60,000 ms | 10,000 ms | hard, own thread | `failed(timed-out)` | no |
| `job-runner.run`, wall time | job instance | 20,000 ms | 600,000 ms | 100,000 ms | hard, own thread | `failed(timed-out)` | no |
| Client-request answer (round trip) | client | request timeout, 5,000 ms | 30,000 ms | 30,000 ms | soft, request level | The request completes `failed(timed-out)` | no |
| Whole hook chain | owner side | 100 ms | 100 ms | 100 ms | soft | Remaining handlers skipped and logged | no |

Notes on the table:

- A `command-invoked` event in a batch selects `command_deadline_ms` for the whole call. A plugin that handles commands therefore gets 50 ms for the batch. This replaces the earlier wording that a command "shares" the 20 ms batch deadline.
- A hook handler's deadline is its `deadline_ms` from the manifest, clamped to 5 to 50 ms. The T2 and T3 ceiling of 50 ms is the same cap, because the manifest bound already applies to every tier.
- A late hook handler is not a plugin failure, because the contributor's plugin keeps running; the third breach in a row disables that one extension (see [Extension points](/pages/api-extension-points/)). Dynamic collect points call no export: entries arrive through the `extensions.register` import, so they have no deadline of their own.
- `detector` is the export of `agent.signatures`; its deadline is the new key `detector_deadline_ms`.
- The `deactivate` breach is not a failure because the plugin is already being stopped. The restore bytes are lost and the host logs `deactivate.deadline` at warn. A trap inside `deactivate` is treated in the same way.

- **[HST-188]** Every guest call MUST belong to exactly one row of the call-class table and MUST run under that row's default, or under the effective limit after clamping to the ceiling of the plugin's tier.
- **[HST-189]** A hard breach MUST trap the instance. A soft breach MUST discard the late result and count a breach, and a call that passes its soft deadline plus 100 ms MUST be trapped.
- **[HST-190]** The streak limits MUST be 3 for `annotate`, `detector`, `menu-items`, `query-catalog`, `list-virtual-children`, `tokens` and `settings-validate`, and 30 for scene frames. A timely call MUST reset the streak.
- **[HST-191]** A `deactivate` breach or trap MUST drop the instance, discard its restore bytes, log `deactivate.deadline`, and MUST NOT count as a failure.
- **[HST-192]** A batch that contains a `command-invoked` event MUST run under `command_deadline_ms`, and every other batch under `event_deadline_ms`.
- **[HST-193]** The effective deadline of a call MUST be clamped to the ceiling for the tier, and the clamp MUST be logged once per plugin and key.

## Deadline measurement

The clock is the host monotonic clock. The epoch ticker period is 10 ms. The host sets the instance deadline to `ceil(d / 10) + 1` ticks after the call starts. A guest that spins without returning therefore traps no earlier than `d` and no later than `d + 20` ms of wall time on a host that is not overloaded. A call that returns at `d - 1` ms never traps. A test asserts the trap time in [d, d + 20] ms (see [Host runtime](/pages/reference-host-runtime/)) [HST-081].

- **Synchronous host calls count.** The deadline clock runs while the guest waits inside a host import. A deadline that expires during an import traps the instance when the import returns. A handler that makes five 5 ms synchronous calls has used 25 ms of its 20 ms budget.
- **Work after the call does not count.** The kernel applying the returned submissions, the later delivery of a `request-completed` event, and the work of an asynchronous host service (an HTTP request, a child process, a job) are outside the call. Only the time between "call started" and "call returned" counts.
- **Suspend and clock jumps.** The host uses a monotonic clock that does not advance during system suspend on Linux and macOS. The host MUST ignore a tick that follows a gap of more than one second.

<div class="note">
<p>This is the precise form of the earlier sentence that "native host work never counts against a plugin deadline". Host work that finishes inside the import counts, because the guest is waiting for it. Host work that finishes after the call returns does not count. See <a href="/pages/performance/">Performance</a>.</p>
</div>

- **[HST-194]** Time spent inside a synchronous host import MUST count toward the deadline of the call that made it, and time after the call returned MUST NOT.
- **[HST-195]** The deadline clock MUST be monotonic, and a gap of more than one second between two ticks MUST NOT produce a deadline breach.

<figure>
<svg viewBox="0 0 440 150" role="img" aria-label="Timeline of a soft deadline: late result dropped, grace period, trap">
<title>Hard and soft deadline timelines</title>
<desc>A hard deadline traps at the deadline. A soft deadline marks the call late, drops its result, and traps only after a grace period of 100 milliseconds.</desc>
<defs>
<marker id="hd-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
<path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
</marker>
</defs>
<text x="8" y="16" font-size="10" fill="currentColor">Hard: on-events, 20 ms</text>
<line x1="20" y1="40" x2="420" y2="40" stroke="currentColor" marker-end="url(#hd-arrow)"/>
<line x1="20" y1="34" x2="20" y2="46" stroke="currentColor"/>
<text x="20" y="58" font-size="9" fill="currentColor">start</text>
<line x1="120" y1="34" x2="120" y2="46" stroke="currentColor" stroke-width="2"/>
<text x="120" y="58" font-size="9" text-anchor="middle" fill="currentColor">20 ms</text>
<text x="124" y="30" font-size="9" fill="currentColor">trap at 20 to 40 ms</text>
<text x="8" y="86" font-size="10" fill="currentColor">Soft: annotate, 10 ms</text>
<line x1="20" y1="110" x2="420" y2="110" stroke="currentColor" marker-end="url(#hd-arrow)"/>
<line x1="20" y1="104" x2="20" y2="116" stroke="currentColor"/>
<text x="20" y="128" font-size="9" fill="currentColor">start</text>
<line x1="70" y1="104" x2="70" y2="116" stroke="currentColor" stroke-width="2"/>
<text x="70" y="128" font-size="9" text-anchor="middle" fill="currentColor">10 ms</text>
<text x="74" y="100" font-size="9" fill="currentColor">late: result dropped, breach counted</text>
<line x1="270" y1="104" x2="270" y2="116" stroke="currentColor" stroke-width="2"/>
<text x="270" y="128" font-size="9" text-anchor="middle" fill="currentColor">110 ms</text>
<text x="274" y="142" font-size="9" fill="currentColor">trap if still running</text>
</svg>
<figcaption>A hard deadline ends the call. A soft deadline lets the call finish but discards its result, and only a call that runs on past the 100 ms grace period is trapped.</figcaption>
</figure>

## Activation and yielding

`activate` and long service calls may **yield**. A yield means that the call is suspended at an epoch tick and put at the back of the executor's ready list, so another plugin's call can run on that thread. The call resumes later. It is the only mechanism that lets a long call share a thread, and it applies only to the classes marked "yields" in the table.

- During the yield other calls to other plugins run. Calls to the same plugin wait in its instance queue, so the activating plugin sees no other call until `activate` returns.
- The yield time counts toward the 2,000 ms deadline, because the deadline is wall time.
- A service caller that triggered the activation waits for `activate` and then for its own call. Its total wait is bounded by `activate_deadline_ms` plus the reply deadline of the call, and it fails with `timed-out` when the activation deadline passes first.

- **[HST-196]** Only `activate`, job runs and service calls that wait for an activation MAY yield. All other classes MUST run on one thread without yielding until they return or fail.
- **[HST-197]** While a plugin side is activating, the host MUST queue its other calls and MUST NOT call into the instance until `activate` has returned successfully.

## Trap aftermath

When an instance traps (a panic, a hard deadline, a streak limit, a stuck restart or a helper crash) the host does the following, in this order.

1. The call that was running is abandoned. **No submission from that call is applied**, because a call that traps returns no value. Imports that act at once and were completed before the trap (`storage.set`, `log`, `ui.invalidate`, `timers`, `messages.send`) stay done; the host does not roll them back.
2. The in-flight event batch is **not redelivered**, with one exception. Events of the kinds in the "survives" list below, which were in the batch, are put back at the head of the new instance's queue once. If the second delivery also traps, they are dropped and the host logs `events.poison` with the event kinds.
3. Pending requests that the instance owned (jobs it started, HTTP streams, children, timers that are not durable) are cancelled. Durable timers and journal entries stay.
4. Retained contributions of the plugin (decorations, virtual rows, status segments, frame slots, row actions) stay on screen and are marked **stale** (drawn dimmed, see [Retained mode](/pages/retained-mode/)).
5. The restart rule decides when the new instance starts (see [Errors and limits](/pages/reference-errors-and-limits/)).
6. The new instance is activated with `restored` bytes from its last `deactivate` (none after a trap) and then receives one `tree-changed` batch for every node, so it can read its state again.

The events that **survive** a stuck-plugin restart or a trap, because they cannot be recovered from the tree, are: `request-completed` (for submissions of this plugin), `durable-timer-fired`, `input-uncertain` and `message`. The events `command-invoked`, `key-pressed`, `key-released`, `ui-action` and `pointer` are discarded with the queue (the user sees the plugin restart). The calls behind `cli-invoked`, `http-called` and `client-request` are completed as `cancelled` (the HTTP caller receives status 503). Every other kind is replaced by the `tree-changed` batch of step 6.

- **[HST-198]** A trap MUST apply none of the call's returned submissions, MUST NOT roll back imports that already acted, and MUST NOT redeliver the in-flight batch except for the surviving event kinds.
- **[HST-199]** The surviving event kinds (`request-completed`, `durable-timer-fired`, `input-uncertain`, `message`) MUST be redelivered at the head of the new instance's queue once, and dropped with the log id `events.poison` if the redelivery traps again.
- **[HST-200]** A stuck-queue restart MUST discard the queue except the surviving kinds, MUST complete the calls behind `cli-invoked`, `http-called` and `client-request` as `cancelled`, and the new instance MUST receive one `tree-changed` batch for every node.
- **[HST-201]** Retained contributions of a trapped instance MUST stay and be marked stale until the plugin replaces or clears them.

## Queues and backpressure

Each plugin side has one event queue. Its size is `event_queue`: 1,024 by default, with a ceiling of 4,096 for T1 and 1,024 for T2 and T3.

| Policy | Events | When the queue is full |
|---|---|---|
| Coalesce | `tree-changed` (one merged list), `screen-changed`, `signal-changed`, `attention-changed` and the other events marked coalesced in [Events](/pages/api-events/) | Always, whatever the queue fill: the newer event merges into the pending event with the same kind and key and takes its position, so a key never holds more than one slot |
| Drop oldest | `output-matched` and other events marked drop-oldest | The oldest pending drop-oldest event of any kind is removed and counted under its own event and source. With none pending, an incoming drop-oldest event is discarded and counted. The next batch carries one `events-dropped` with the counts |
| Never dropped | All others | The plugin is **stuck**: it is restarted as a failure |

A plugin gets a **batch** of up to 256 events per `on-events` call. The dispatcher builds a batch when one of these is true: 256 events are pending; or 2 ms have passed since the first pending event; or the pending events include a latency-critical kind (`key-pressed`, `key-released`, `command-invoked`, `ui-action`, `pointer`, or a `request-completed` of a user action), which flushes at once. A `request-completed` of a user action is one whose submission came from an `on-events` call whose batch held a `key-pressed`, `command-invoked` or `ui-action` event. Such a completion is flushed at once instead of after the 2 ms window [HST-204]. The host splits a larger pending list into several calls in order.

Streaming events add **back-pressure** before the queue can fill. For `http-stream-data` and `child-output` the host gives each stream a **credit window** of 256 KiB. The host counts bytes that it has enqueued for the plugin and the plugin has not yet consumed. When the count reaches the window, the host stops reading the network socket or the child pipe. It resumes reading when the count falls to half the window. A chunk is at most 64 KiB. A fast peer is therefore slowed by the operating system's own flow control and a plugin never receives more than the window.

Rate limits are applied before enqueue: a source that produces events faster than its limit (for example more than 10 `set-meter` style updates per second) is refused at the source with `rate-limited`, so the queue never sees them.

The **execution quota group** `ilium-execution` limits a plugin's total CPU use. There is one group for each plugin side in each process, and it is defined in [Errors and limits](/pages/reference-errors-and-limits/) and [Resource accounting](/pages/reference-resource-accounting/). It is a token bucket of 2,000 ms of CPU time that refills at 500 ms per second. The host charges the thread CPU time of each call, and 1 ms for every 64 KiB of payload in a submission. When the bucket is empty, submissions and calls that return a request id fail with `quota-exceeded`. The bucket balance can go below zero, because a call is charged after it ends. The bucket is empty when the balance is 0 or less. `retry-after-ms` = max(100, ceil_10(-balance_ms * 2)), where `ceil_10` rounds up to the next 10 ms: the wall time until the balance refills to 0 at 500 ms per second, never below 100. A balance of -100 ms gives 200. Calls into the plugin are not held: `on-events`, `views.render`, `activate`, `deactivate` and provider calls still run, and the plugin simply cannot submit until the bucket refills. A trap still charges the time used.

- **[HST-202]** Each plugin side MUST have one event queue of `event_queue` entries, and the policy of each event kind (coalesce, drop oldest, never dropped) MUST be the one in [Events](/pages/api-events/).
- **[HST-203]** A full queue with a never-dropped event MUST restart the plugin side as a failure.
- **[HST-204]** A batch MUST contain at most 256 events, MUST be flushed after 2 ms or at 256 events, and MUST be flushed at once when it contains a latency-critical kind.
- **[HST-205]** Streams MUST use a credit window of 256 KiB, chunks of at most 64 KiB, and the host MUST stop reading the socket or pipe at a full window and resume at half.
- **[HST-206]** Rate limits MUST be applied before an event is enqueued.
- **[HST-207]** The host MUST maintain one `ilium-execution` bucket per plugin side with capacity 2,000 ms and refill 500 ms per second, MUST charge thread CPU time and payload bytes as above, and MUST refuse submissions with `quota-exceeded` and `retry-after-ms` of at least 100 when the bucket is empty.
- **[HST-208]** An empty bucket MUST NOT hold `on-events`, `views.render`, `activate`, `deactivate` or provider calls.

## Cancellation

A request is cancelled by one of seven causes. The outcome tells the plugin which.

| Cause | Outcome | Notes |
|---|---|---|
| The plugin side stopped or trapped (instance dropped) | `cancelled` | Pending requests of that instance |
| Grant revoked | Synchronous calls that need the grant return `err(revoked)` from the moment of revocation. Pending requests that need it complete `failed(revoked)`, delivered before `deactivate(revoked)` | The plugin is re-activated with the reduced grants. Other pending `call(id)` requests are cancelled with the instance and their completions are not delivered; pending submissions follow the trap-aftermath rule |
| The plugin asked for it (`cancel(request)`, `jobs.cancel`) | `cancelled` | A cancelled job completes with the outcome `cancelled`. `jobs.cancel` returns nothing: cancelling a completed, unknown or already cancelled id is a silent no-op and produces no further completion |
| Server or client shutdown | `cancelled` | Submissions accepted but not yet applied at shutdown complete as `cancelled`. `refused(cancelled)` is used only for siblings of a refused batch |
| The request deadline passed | `failed(timed-out)` | For example the 30 s command reply deadline, or a provider that does not reply to a deferred service call within 30 s. The earlier of the two events wins |
| A service provider traps or restarts while callers wait | `failed(cancelled)` for every deferred call, at once | A breach of the 50 ms provider deadline completes the breaching call as `failed(internal)` and the other deferred calls as `failed(cancelled)` |

Cancelled host work is torn down within a bound.

| Work | Teardown |
|---|---|
| HTTP request or stream | The connection is aborted within 100 ms |
| File read, watch, scan | Stopped at the next chunk, within 100 ms |
| Child process | SIGTERM, then SIGKILL after 2 s (POSIX); see [Platform support](/pages/reference-platform-support/) for Windows |
| Running job | The cancel flag is set; `is-cancelled` returns true. If the job has not returned within 1,000 ms (`job_cancel_grace_ms`), the host traps the job instance at the next tick |
| Helper call | A `Cancel` message is sent; if the helper has not answered 500 ms after the call's deadline, the host kills it and counts one failure (HST-162) |

- **[HST-209]** Each cancellation cause MUST produce the outcome in the table, and a completed request MUST NOT be changed by a later cancel. `jobs.cancel` of a completed, unknown or already cancelled id MUST be a silent no-op.
- **[HST-210]** Cancelled HTTP and file work MUST stop within 100 ms, child processes MUST receive SIGTERM then SIGKILL after 2 s, and a job that ignores the cancel flag MUST be trapped 1,000 ms after the flag is set.

## Hook chain execution

`run-hook` is asynchronous: it returns a request id at once, the chain runs on the host, and the result arrives as `request-completed` (the SDK `await` form hides this). The owner's current call is never blocked by the chain, so a 100 ms chain does not conflict with the owner's 20 ms `on-events` deadline.

- Handlers run **strictly one after another** in priority order, never in parallel, each in the contributor's own instance on the owner's side.
- A handler that cannot start within its own deadline measured from the moment it was due (the contributor's instance is busy) is skipped as `pass` with a breach counted.
- The 100 ms chain cap is measured from the start of the first handler call. Handlers not yet started when it expires are skipped and logged `extension.chain-cap`.
- A T2 or T3 handler's helper round trip (see [Sandbox helper](/pages/reference-host-helper/)) is inside the handler deadline. Its budget is 0.5 ms at the 99th percentile, so the usable deadline of a 5 ms handler is 4.5 ms.
- A hook on a client-owned point runs only handlers in that client. A hook on a server-owned point runs only handlers on the server. No hook crosses a process.

- **[HST-211]** `run-hook` MUST NOT block the owner's call, handlers of one chain MUST run strictly sequentially, and the 100 ms chain cap MUST be measured from the first handler's start.
- **[HST-212]** A handler MUST be skipped as `pass` when it cannot start within its own deadline, and a skipped or late handler MUST count a breach.
- **[HST-213]** A hook chain MUST run only handlers hosted by the process that owns the point.

## Latency of keys and input

A key that a plugin has grabbed (see [Keys](/pages/api-keys/)) travels like this: the UI thread decodes the key, finds the grab, and puts a `key-pressed` event on the client dispatcher channel. The dispatcher flushes it at once. An idle executor thread starts the `on-events` call. The plugin returns submissions. The host applies them, and a `send-input` submission reaches the pane.

| Step | Budget (99th percentile, idle machine) |
|---|---|
| Key decoded to the dispatcher channel | 0.1 ms |
| Channel to the start of the plugin call (T1) | 1 ms |
| Channel to the start of the plugin call (T2 or T3, helper round trip included) | 1.5 ms |
| Plugin returns to the intent applied (client-side intent) | 0.5 ms |
| Plugin returns to the intent applied on the server (forwarded over IPC) | 3 ms |
| Whole path, key press to bytes at the PTY, with a plugin handler of 0.1 ms | 5 ms |

The benchmarks B16 and B17 of [Performance budgets and benchmarks](/pages/reference-host-budgets/) measure these.

- **[HST-214]** A grabbed key MUST flush its batch at once, and the 99th percentile from the dispatcher channel to the start of the plugin call MUST be at most 1 ms for T1 and 1.5 ms for T2 and T3 on an idle reference machine.
- **[HST-215]** The 99th percentile from key press to bytes at the PTY through a plugin handler of 0.1 ms MUST be at most 5 ms on an idle reference machine.

## Errors and edge cases

| Situation | Result | Id |
|---|---|---|
| Two calls to one instance arrive together | They run one after the other by priority | none |
| Caller deadline passes while the call waits in the queue | The call is abandoned, a breach is counted | `host.queue-timeout` |
| A call yields during `activate` and another plugin runs | Allowed; the activating plugin gets no other call | none |
| Deadline expires inside a synchronous import | Trap when the import returns | `trap.deadline` |
| System suspend during a call | The tick after the gap is ignored | none |
| Trap with a surviving event in the batch | Redelivered once to the new instance | `events.poison` on a second trap |
| Empty CPU bucket | Plugin submissions return `quota-exceeded` with `retry-after-ms`; `on-events` and other calls into the plugin still run | `quota-exceeded` |
| Job ignores the cancel flag | Trapped 1,000 ms later; the job completes `cancelled` | `cancelled` |
| Soft call passes the soft deadline plus 100 ms | Trap | `trap.deadline` |
| Stream credit window full | The host stops reading the socket or pipe | none |
| Hook chain exceeds 100 ms | Remaining handlers are skipped | `extension.chain-cap` |

- **[HST-216]** A call that is abandoned in the queue MUST log `host.queue-timeout` and count a breach of its class.

## See also

- [Host runtime](/pages/reference-host-runtime/) for how deadlines are enforced.
- [Errors and limits](/pages/reference-errors-and-limits/) for the limit keys and the restart rule.
- [Events](/pages/api-events/), [Intents](/pages/api-intents/) and [Requests and outcomes](/pages/requests-and-outcomes/).
- [Extension points](/pages/api-extension-points/), [Menu provider](/pages/api-menu-provider/) and [Background jobs](/pages/background-jobs/).
- [Performance](/pages/performance/) for the plugin author's view.

API version checked: `ilium:plugin@0.1.0`.
