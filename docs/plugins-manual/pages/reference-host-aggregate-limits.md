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

### Disk and CPU ceilings

The memory and thread ceilings above do not bound disk or CPU. The rules below add a machine-wide disk ceiling and a machine-wide CPU ceiling. Their numbers are proposals, so they are marked (proposed, owner to confirm). Once the owner confirms the CPU ceiling, it replaces the open-decision default of "none" above.

- **[HST-126]** The host MUST enforce one disk ceiling for all plugins on one machine. The ceiling counts the `data` plus `cache` bytes of every installed plugin, across every session server, and it MUST be 8,192 MiB (proposed, owner to confirm). The per-plugin `disk_mib` key and the `cache_mib` key still apply, and a write MUST pass both the per-plugin check and this ceiling.
- **[HST-127]** A plugin write that would take the machine past the disk ceiling of HST-126 MUST fail with `quota-exceeded` and the message `host-disk-ceiling`, MUST NOT evict data of another plugin to make room, and MUST NOT be counted as a failure of the plugin side. The host MUST NOT retry the write itself. A plugin MAY retry after the user or an administrator frees space.
- **[HST-128]** The host MUST enforce one CPU ceiling for all plugin sides on one machine, held as one token bucket that holds 4,000 ms of thread CPU time and refills at 4,000 ms of thread CPU time per second (proposed, owner to confirm). It charges the same work as the per-side `ilium-execution` group of [Resource accounting](/pages/reference-resource-accounting/), and it applies in addition to that group, never instead of it.
- **[HST-953]** When the CPU bucket of HST-128 is empty, a new submission and every call that returns a `request-id` MUST return `quota-exceeded` with a `retry-after-ms` value computed with the formula of [API-072] on the machine bucket. The refusal MUST NOT trap the plugin, MUST NOT count as a failure, and MUST NOT charge CPU time to any side beyond the cost of the call itself, as [API-089] requires.
- **[HST-954]** The disk ceiling and the CPU ceiling MUST NOT lower the free-space reserve of [EXT-064] or the low-disk steps of [Offline and low disk](/pages/reference-offline-and-low-disk/). When free space falls under either threshold, those steps MUST still run, and the ceiling of HST-126 MUST NOT be used to skip them.

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
| Aggregate disk ceiling (`data` plus `cache`, all plugins) | 8,192 MiB (proposed, owner to confirm) | HST-126 |
| Aggregate CPU bucket | 4,000 ms held, 4,000 ms per second refill (proposed, owner to confirm) | HST-128 |

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

## Load shedding when an aggregate ceiling stays exhausted

Target behaviour. The aggregate ceilings only refuse the next allocation ([HST-503]). If the session memory pool, the aggregate CPU bucket of [HST-128] or the handle ceiling stays at its limit, every plugin that needs more is refused in turn and the session degrades by chance. Load shedding gives the host one fixed order for pausing plugins, so that the session degrades the same way every time and the user is told what was paused. All timings here are proposed, owner to confirm.

### When shedding starts

- The host samples the three ceilings once per second. A ceiling counts as exhausted at a sample when the memory pool has less than the smallest pending reservation free, when the CPU bucket of [HST-128] is empty, or when the handle ceiling is reached.
- Shedding starts when one ceiling has been exhausted at every sample for 60 s in a row. One sample that is not exhausted restarts the 60 s count.
- Shedding runs for one ceiling at a time. The host sheds one plugin, waits 5 s for the freed resource to show in the samples, and sheds the next only if the ceiling is still exhausted. It stops as soon as one sample is not exhausted.
- A refusal by the ceiling still behaves as [HST-503] says while the 60 s run. Shedding adds to that rule and does not replace it.

### Order of candidate selection

The host picks the next plugin to shed with these steps, first match wins. The decision belongs to the host and cannot be changed by a plugin or by a manifest.

1. Essential core plugins are never candidates (LIF-042). The same holds for a plugin with a pending install, upgrade or rollback, and for a plugin that holds the only copy of unsaved user data it has declared through its save hook.
2. Dormant optional plugins first. A plugin is dormant when it has no visible view, no pending request in either direction and no running job. Among dormant plugins, the one with the oldest last activity goes first. Ties are broken by plugin id in ascending byte order.
3. Then non-dormant optional plugins, ordered by their share of the exhausted resource, largest first: `memory_bytes` for the memory pool, `cpu_pct` for the CPU bucket, `handles` for the handle ceiling, as shown in `ilium plugin top` ([HST-634]). Ties go to the oldest last activity, then to plugin id.
4. A plugin that is the cause of the exhaustion is not treated specially. The order above is the only order.

If no candidate is left, the host sheds nothing, keeps logging the aggregate refusals and shows no further notice.

### What a shed plugin sees and becomes

- The host stops both sides of the plugin with the normal stop path (a `plugin-state-changed` event with reason `shed`, then the grace period of the lifecycle) and releases the reservations of those sides.
- The package returns to `installed`, with the side reason `host-capacity`. It does not become `failed` or `failure_stopped`, no failure is counted in the 10-minute rule or the 24-hour window ([HST-950]), and the backoff of [LIF-141] is not started.
- The host writes one log record `host.shed <plugin id> <limit>`, where `<limit>` is `memory`, `cpu` or `handles`. It shows one footer notice per shedding run, naming the number of paused plugins and the first of them, for example `Paused 3 plugins to free memory (ilium plugin top)`. A second run within 10 minutes updates the same notice and does not add one.
- `ilium plugin top` marks a shed plugin with the state `shed` in its state column, with its resource columns at zero, and lists it after the active rows. `ilium plugin status` lists it as inactive with the reason `host-capacity` and the line `shed since <time>`.

### Reactivation

- A shed plugin is reactivated automatically when the limit it was shed for has had 20 percent headroom at every sample for 60 s in a row: at least 20 percent of the pool free, 20 percent of the CPU bucket held, or the handle count at or below 80 percent of the ceiling.
- Reactivation uses the admission order of [HST-502] over all waiting requests and shed plugins together, and starts one plugin per 5 s so that the headroom is not spent at once. Each start passes the checks of [HST-509] again.
- If the host sheds the plugin again within 10 minutes of its reactivation, the plugin stays shed until the user starts it with `ilium plugin enable <plugin id>` or the headroom rule holds again after 10 minutes, whichever comes first. This stops a cycle of shedding and starting.
- The user can start a shed plugin at any time. The start is then an ordinary activation and may be refused with `host-capacity`.

### Failure modes

- If stopping a candidate takes longer than its grace period, the host ends it as the lifecycle does, still without counting a failure, and goes on to the next candidate only after the 5 s wait.
- If the host restarts while plugins are shed, the shed marker is dropped and all plugins start by the admission order of [HST-502]; shedding begins again only after a new 60 s run.
- A policy key that disables a plugin wins over reactivation: a plugin disabled while shed stays disabled.

### Test

Fill the pool with three optional plugins: A with a visible view, B with a pending request, and C dormant with the oldest last activity, using a small `session_memory_mib`. Request a fourth activation and hold the pool full for 60 s. Assert that C is shed first with `host.shed <C id> memory` logged once, one footer notice shown, `ilium plugin top` showing `shed` for C, no failure counted for C, and A and B still active. Then free 20 percent of the pool for 60 s and assert that C is active again.

## Number of installed and active plugins

Memory and thread ceilings bound what plugins use, but the count of plugins is a limit of its own: startup time, the tree mirror and the settings list all grow with it. The startup budget is set for the 40 bundled plugins only, so a larger set needs an explicit rule.

- **[HST-510]** The host MUST enforce a ceiling of 256 installed plugins and 128 simultaneously active plugin sides per session server. Both values MUST be policy keys that an administrator can lower but not raise above these ceilings.
- **[HST-511]** When the active ceiling is reached, the host MUST refuse the next activation with the reason `host-capacity`, MUST keep every active plugin running, and MUST NOT evict an active plugin to make room. Startup activation MUST follow the admission order of the aggregate ceiling, and plugins left inactive MUST be listed by `ilium plugin status`.

### Limit changes between host releases

Target behaviour. Ceilings and rates can change when the host is upgraded, and a plugin has to be able to read the values it gets.

- [API-094] A host release MUST NOT lower a tier ceiling, a rate rule or a session-wide limit of [API-043], [API-070] or [API-071] below the value a plugin was admitted with within the same contract minor. A change MUST be listed in the release notes, and a plugin MUST be able to read its effective limits at runtime from the `limits` record of the host info, which names each limit, its value and its source (default, manifest or user).

### See also

- [Resource accounting](/pages/reference-resource-accounting/)
- [Errors and limits](/pages/reference-errors-and-limits/)
- [Sandbox helper](/pages/reference-host-helper/)
- [Health and watchdog](/pages/reference-health-and-watchdog/)

API version checked: `ilium:plugin@0.1.0`.
