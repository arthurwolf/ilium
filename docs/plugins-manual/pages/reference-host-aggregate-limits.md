---
layout: page
title: Reference, host aggregate limits
permalink: /pages/reference-host-aggregate-limits/
---

## Reference, host aggregate limits

Most limits in [Errors and limits reference](/pages/reference-errors-and-limits/) apply to one session server, or to one plugin side inside one process. A machine can run several session servers at once, and each one can start its own helpers and jobs. This page defines the aggregate ceiling that covers all of them together on one machine: what it counts, the order in which it admits work, and what a refused request sees.

The aggregate ceiling is an addition to the per-session keys and the per-side buckets. It never raises any of them, and a request must pass both kinds of check.

### Terms

- **Machine**: the one computer on which the host runs. Whether several operating-system users on it share one ceiling is an open decision below.
- **Session server**: the process that owns one project session. Each has its own `session_memory_mib` pool (default 1,024 MiB) and `session_helpers` cap (default 16), as in [Errors and limits reference](/pages/reference-errors-and-limits/).
- **Reservation**: the memory a request holds from admission until its instance, helper or job ends. An instance reserves its configured `memory_mib`, not what it uses ([Resource accounting](/pages/reference-resource-accounting/)). A helper reserves the sum of its hosted instances' `memory_mib` plus 32 MiB ([HST-164](/pages/reference-host-helper/)).
- **Aggregate ceiling**: the machine-wide cap on the total of all reservations (memory) and on the total thread count (threads).
- **Request**: an activation of a plugin side, the spawn of a helper, or the start of a job.
- **Core T1 plugin**: a built-in plugin of trust tier T1 that reserves at server start.
- **Aggregate refusal**: a refusal caused by the aggregate ceiling, as opposed to a session key.

### Inputs and outputs

| Input | Source |
|---|---|
| Reservation size of the request | The manifest `memory_mib` and the helper overhead above |
| Thread count of the request | The threads of the helper or job |
| Current machine totals | The host, across all session servers |
| Per-session state | `session_memory_mib`, `session_helpers` and the other session keys |

| Outcome | What happens |
|---|---|
| Admitted | The reservation is counted and the request proceeds |
| Refused | The side stays `inactive` with the reason `host-capacity`; an aggregate refusal is logged; the request waits for a retry |

The numeric values of both aggregate ceilings are not set by the repository; see the open decisions below. No error code is returned to the plugin for an aggregate refusal; it is visible only as the reason `host-capacity` ([Health and watchdog](/pages/reference-health-and-watchdog/) covers how inactive sides are shown).

### What the ceiling counts

| Resource | What is counted | Covers |
|---|---|---|
| Memory | The reserved `memory_mib` of each instance, plus 32 MiB per helper, as in [Sandbox helper](/pages/reference-host-helper/) | Every session server, helper and job on the machine |
| Threads | The threads of each helper and each job | Every helper and job on the machine |
| CPU time | Not covered by an aggregate ceiling yet; each plugin side keeps its own `ilium-execution` bucket | See the open decisions below |

### Admission order

When a request does not fit the aggregate ceiling, the host works through this order.

1. Core T1 plugins reserve first, at server start, as in [Resource accounting](/pages/reference-resource-accounting/).
2. Every other request is admitted in the order it was made across all sessions on the machine.
3. Before it refuses a request, the host drops unused compile cache pages and tries once more, as it does for the session memory pool.
4. A refused request waits. When capacity returns, waiting requests are retried in the order they were made.

### What a refused request sees

A request refused by the aggregate ceiling leaves its plugin side `inactive` with the reason `host-capacity`. The plugin detail shows that reason, and the log records an aggregate refusal. The refusal is not a failure, so it does not start the restart rule of [Errors and limits reference](/pages/reference-errors-and-limits/).

### Rules

- **[HST-500]** The host MUST enforce one aggregate ceiling on the memory of every session server, helper and job on one machine. Each reservation MUST be counted against that ceiling in the same way that [HST-164](/pages/reference-host-helper/) counts a helper against `session_memory_mib`.
- **[HST-501]** An activation MUST pass the aggregate ceiling and every session key that applies to it. Passing one check MUST NOT skip another.
- **[HST-502]** The host MUST admit waiting requests against the aggregate ceiling in this order: core T1 plugins at server start first, then every other request in the order they were made across all sessions.
- **[HST-503]** A request refused by the aggregate ceiling MUST leave the side `inactive` with the reason `host-capacity`, MUST be logged as an aggregate refusal, MUST NOT count as a failure, and MUST be retried when capacity returns, in the order of HST-502.
- **[HST-504]** The host MUST count the threads of every helper and job against an aggregate thread ceiling. It MUST refuse a new helper or job with `host-capacity` when it would exceed that ceiling, and the refusal MUST NOT count as a failure.

- **[HST-509]** The aggregate ceiling MUST NOT raise any session key or any per-side bucket, and a request MUST pass the session key checks and the aggregate check on every attempt, including a retry of a waiting request.

### Examples

1. Two session servers are running. A third plugin activation in session B would fit `session_memory_mib` of session B but would push the machine total past the aggregate memory ceiling. The side stays `inactive` with `host-capacity`, the log records an aggregate refusal, and no failure is counted. When a plugin in session A is deactivated and its reservation is released, the waiting activation is retried.
2. A plugin activation passes the aggregate ceiling but would exceed `session_memory_mib`. It is refused with `session.memory-exceeded` and the reason `host-capacity` ([HST-165](/pages/reference-host-helper/)). The aggregate ceiling did not skip the session check (HST-501).
3. A helper spawn would take the machine past the aggregate thread ceiling. The spawn is refused with `host-capacity` and no failure is counted (HST-504).

### Failure behaviour

- An aggregate refusal never starts the restart rule or the failure streak.
- A refusal does not change the state of other running sides.
- Behaviour when the machine totals cannot be read is not settled by the repository; see the first open decision below for the related ceiling values, and treat this case as undecided.

### Limits

| Limit | Value | Source |
|---|---|---|
| Helper memory overhead | 32 MiB per helper | [Sandbox helper](/pages/reference-host-helper/) |
| `session_memory_mib` | 1,024 default | [Errors and limits reference](/pages/reference-errors-and-limits/) |
| `session_helpers` | 16 default | [Errors and limits reference](/pages/reference-errors-and-limits/) |
| Aggregate memory ceiling | Not set (open decision, default 4,096 MiB) | Below |
| Aggregate thread ceiling | Not set (open decision, default 64) | Below |

### What this page does not cover

- Per-session keys and per-side buckets: [Errors and limits reference](/pages/reference-errors-and-limits/).
- How an instance reservation is computed and admitted inside one session: [Resource accounting](/pages/reference-resource-accounting/).
- Helper charging and `session.memory-exceeded`: [Sandbox helper](/pages/reference-host-helper/).
- Restart and failure counting: [Health and watchdog](/pages/reference-health-and-watchdog/).

### Open decisions

These values are a choice for the project owner. The repository does not set them. Each item gives the default that this page assumes until the owner decides.

- Decision needed: the value of the aggregate memory ceiling, in MiB. Default: 4,096 MiB, which is four session pools at their 1,024 MiB default, so a normal load of four sessions is not refused while the total is still bounded.
- Decision needed: the value of the aggregate thread ceiling. Default: 64 threads, which is four sessions at the 16-helper default, assuming one thread per helper. The repository does not state how many threads one helper uses.
- Decision needed: whether an aggregate CPU ceiling is added. Default: none in this version, because each plugin side already has a bucket that limits its rate and no measured figure exists for a machine total.
- Decision needed: whether the ceiling is shared across operating-system users on one machine. Default: it applies per operating-system user, because each user installs plugins into their own profile.
- Decision needed: whether a refusal by the aggregate ceiling gets its own error code instead of the `host-capacity` reason. Default: no new code, so that refusals look the same to plugin authors as session refusals.
- Decision needed: whether the memory of a job is counted apart from its plugin side's instance reservation. Default: it is counted within that reservation, so no separate job accounting is added.

### See also

- [Resource accounting](/pages/reference-resource-accounting/)
- [Errors and limits](/pages/reference-errors-and-limits/)
- [Sandbox helper](/pages/reference-host-helper/)
- [Health and watchdog](/pages/reference-health-and-watchdog/)

API version checked: `ilium:plugin@0.1.0`.
