---
layout: page
title: Reference, host aggregate limits
permalink: /pages/reference-host-aggregate-limits/
---

## Reference, host aggregate limits

Most limits in [Errors and limits reference](/pages/reference-errors-and-limits/) apply to one session server, or to one plugin side inside one process. A machine can run several session servers at once, and each one can start its own helpers and jobs. This page defines the aggregate ceiling that covers all of them together on one machine: what it counts, the order in which it admits work, and what a refused request sees.

The aggregate ceiling is an addition to the per-session keys and the per-side buckets. It never raises any of them, and a request must pass both kinds of check.

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
