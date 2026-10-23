---
layout: page
title: Concurrency for plugin authors
permalink: /pages/concurrency-for-plugin-authors/
---

## Concurrency for plugin authors

Your plugin runs on several threads, but one instance of your code never runs two calls at once. This guide tells you where each kind of code runs, what a call waits for, how to keep each handler under its deadline, how to move slow work out of a handler, and how job instances and `activate` behave. It states what your code must do. The host rules that enforce these facts are normative in [Scheduling, deadlines and cancellation](/pages/reference-host-scheduling/) and [Runtime model](/pages/reference-runtime-model/). This page links to them and does not restate them as host rules.

Requirement ids of this page are in the range `DEV-834` to `DEV-839`. The complete list is in [Requirements for developer tooling](/pages/reference-requirements-dev-tooling/).

### You need

- [Events](/pages/api-events/) for the handler model and the batches of `on-events`.
- [jobs](/pages/api-jobs/) and [Background jobs](/pages/background-jobs/) for the job model.

## Where your code runs

| Your code | Runs on | What it means for you |
|---|---|---|
| `activate`, `deactivate`, `on-events`, `views.render`, `annotate`, hooks and providers | The plugin executor of the side (server or client) | One call at a time per instance |
| Scene `renderer` frames (scene side) | The scene thread | A frame runs in its own instance and does not take an executor thread |
| Job `run` | The job worker bank | A job runs in its own instance, which is not activated |
| Client UI thread | Never calls your code | It draws the last result it holds |

The thread table is in [Scheduling, the thread model](/pages/reference-host-scheduling/#thread-model).

- [DEV-834] A plugin MUST NOT assume that its `on-events`, `views.render` or `annotate` calls run on the client UI thread. A frame never waits for a plugin, so the plugin MUST accept that a frame shows the last result it holds until a newer one arrives.

## One call at a time per instance

Each instance of your plugin, one per side, runs one export at a time. The host never calls a second export of that instance while one is running. When more calls are waiting, the host picks the next one by priority: `activate` and `deactivate` first, then calls with a waiting caller, then `on-events`, then `views.render`, then `annotate`. Between plugins the host serves each plugin in turn. The rules are [HST-184] to [HST-186] in the scheduling reference.

What this means for your code:

- A handler can rely on the state that the previous call left in memory, because no other export of your instance runs in between.
- A render runs after the `on-events` batch, so it sees the state that the events produced.
- A call that waits in the queue does not get extra time for its own deadline. A call with a waiting caller has a second deadline that starts when it is queued, and it is abandoned if it does not start in time.

- [DEV-835] A plugin MUST NOT let one export of its instance wait for another export of the same instance, because the host never runs two exports of one instance at the same time.

## Handler deadlines

Every export runs under a deadline. The defaults for the handler calls are:

| Call | Default | Breach |
|---|---|---|
| `on-events`, batch without a `command-invoked` event | 20 ms | Hard: the instance is trapped |
| `on-events`, batch with a `command-invoked` event | 50 ms | Hard: the instance is trapped |
| `views.render` | 10 ms | Hard: the instance is trapped |
| `annotate` | 10 ms | Soft: the late result is dropped; the third breach in a row traps |

The full table, with the tier ceilings and the streak rules, is in [Call classes](/pages/reference-host-scheduling/#call-classes). A manifest `limits` block can ask for another value, but only up to the ceiling of your tier. A third-party plugin has a ceiling of 20 ms for an ordinary batch and 50 ms for a command batch.

The deadline is wall time, from the start of the call to its return. Time spent inside a synchronous host import counts. Time after the call has returned does not. A handler that makes five synchronous calls of 5 ms each has used 25 ms of a 20 ms budget. A hard breach traps the instance, and none of the submissions of that call is applied. The batch is not redelivered, except for four kinds of event that the host puts back once: `request-completed`, `durable-timer-fired`, `input-uncertain` and `message`. The worked example and the trap steps are in [Runtime model, section 2](/pages/reference-runtime-model/#2-calls-deadlines-and-interruption).

- [DEV-836] A plugin MUST keep each `on-events` batch under its deadline of 20 ms, or 50 ms for a batch with a `command-invoked` event, and each `views.render` and `annotate` call under 10 ms. A plugin that needs more MUST raise the deadline only in the manifest `limits` block and only up to the ceiling of its tier.

## Moving slow work out of a handler

A handler is for work of a few milliseconds: reading the event, updating state, and submitting the next intent. Anything longer goes to one of these two places.

| Work | Start it with | The result comes back as |
|---|---|---|
| A slow host call, such as an HTTP request | Its request function, which returns a request id at once | A `request-completed` event in a later batch. Take the result once with the matching `take-*` call. |
| Parsing, searching, building or waiting for a model | `jobs.start` | The job output, read after the job completes. For data under 16 MiB that needs no job, [codec](/pages/api-codec/) is the page that covers it. |

The rules that apply to slow calls are these. A result is held for 30 seconds, or until its first take. A plugin has at most 64 pending requests by default; a call that exceeds the limit returns `busy` at once. A job past the `concurrent_jobs` limit, which is 2 by default, waits in a queue and is not an error. The table of limits is in [Manifest](/pages/reference-manifest/).

- [DEV-837] A plugin MUST NOT wait inside a handler, render or annotate call for work that can take longer than that call's deadline. Such work MUST run as a slow call or a job, and its result MUST be read from a later `request-completed` event or from the job output.

## Job state isolation

A job runs in its own instance, created for the job. A job instance is not activated, has no event handler, and shares no memory with the handler that started it. Only three things cross between them: the input bytes, which are copied at the start; the output bytes; and the progress value.

The host reuses job instances between jobs, so a job that keeps a value in a module variable can give the next job a stale value. A job can call `log`, `clock`, `codec`, `fs`, `http`, `process`, `inference`, `storage.get`, `storage.keys`, `report-progress`, `is-cancelled` and `wait-request`, with the grants of its side. Any other import, including `jobs.start`, returns `unsupported`. The list is in [jobs, what a job instance can call](/pages/api-jobs/#what-a-job-instance-can-call).

A job that is cancelled sees `is-cancelled` return true. A job that does not return within 1,000 ms after the cancel flag is set is trapped, which is the `job_cancel_grace_ms` rule of [Cancellation](/pages/reference-host-scheduling/#cancellation). A job that fails does not count toward the restart rule of your plugin.

- [DEV-838] A job MUST NOT keep state from one run to the next, and MUST NOT rely on memory of the handler that started it. Its results MUST leave only as output bytes or progress values.

## Yield behaviour

`activate` may yield. A yield suspends the call at an epoch tick and puts it at the back of the executor's ready list, so calls to other plugins can run on that thread. Job runs and service calls that wait for an activation may yield as well. The rules are [HST-196] and [HST-197].

What this means for your code:

- Calls to your own plugin wait while your `activate` runs. Events that arrive during activation are queued and delivered in the first batch after `activate` returns `ok`.
- The yield time counts toward the activation deadline. The default for `activate` is 2,000 ms, and the deadline is wall time, not CPU time.
- Keep `activate` short. Start slow loading as a job or a slow call, and read its result later.

- [DEV-839] A plugin MUST NOT expect a call to its own instance to run while its `activate` is still running, and MUST count every yield of `activate` against the 2,000 ms default deadline, because that deadline is wall time.

## Open decisions

1. **Whether a job may write `storage`.** [Runtime model](/pages/reference-runtime-model/#1-instances-and-threads) says a job shares data with the main instance "through `storage`, `fs` and messages". [jobs](/pages/api-jobs/#what-a-job-instance-can-call) lists only `storage.get` and `storage.keys` for a job, and returns `unsupported` for any other import. This page follows the jobs list, so [DEV-838] forbids a job from keeping state in storage. If the runtime model is the correct one, a job can write storage, and [DEV-838] is stricter than the host needs. The owners of the two pages must decide which one is normative.
2. **Yield of job runs.** [Scheduling](/pages/reference-host-scheduling/#activation-and-yielding) allows job runs to yield, but the jobs page does not describe it. This page does not promise any yield behaviour for jobs. The consequence for an author is that the wall time of a job can grow while other plugins run, and the wall time limit still applies. The decision is whether the jobs page should say so.

Both items stay open until their owner records the decision on this page. Until then the stated default below applies, and tests pin the default, not the open alternative.

| Item | Owner | Decision date | Stated default until closed |
|---|---|---|---|
| Whether a job may write `storage` | The owners of [Runtime model](/pages/reference-runtime-model/) and [jobs](/pages/api-jobs/), who must agree on one page | Proposed 2026-11-06, owner to confirm | A job may call `storage.get` and `storage.keys` only. `storage.set` from a job returns `unsupported`, as the jobs list says. A job that needs to hand data back uses its output bytes or progress values. |
| Whether job runs may yield | The owner of [jobs](/pages/api-jobs/) and [Scheduling](/pages/reference-host-scheduling/#activation-and-yielding) | Proposed 2026-11-06, owner to confirm | A job may be suspended while other plugins run, so its wall time can grow. Your code MUST NOT assume that a job runs without pauses. The job's own wall-time limit applies to the full elapsed time, pauses included. |

How the defaults are tested until the items close:

- A test of a job that calls `storage.set` asserts the `unsupported` result, not a successful write.
- A test of a long job asserts that its result is correct when the job is paused and resumed, and that a job past its wall-time limit is cancelled at that limit, not at its CPU time.
- When an owner closes an item, the owner replaces its row in the table with the decision, the date, and the rule ids that now apply, then updates the tests that pinned the default.

## See also

- [Scheduling, deadlines and cancellation](/pages/reference-host-scheduling/) for the normative call classes, the queue and the trap steps
- [Runtime model](/pages/reference-runtime-model/) for instances, deadlines and re-entrancy
- [jobs](/pages/api-jobs/) for the job surface
- [Background jobs](/pages/background-jobs/) for the job lifecycle and cancellation
- [Events](/pages/api-events/) for batches and the queue policy of each event
- [Quality checklist](/pages/guide-quality-checklist/) for a review list that covers deadlines and jobs
- [Patterns cookbook](/pages/guide-patterns-cookbook/) for worked patterns

Checked against `ilium:plugin@0.1.0`.
