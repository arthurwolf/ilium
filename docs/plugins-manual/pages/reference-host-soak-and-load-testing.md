---
layout: page
title: Soak and endurance testing
permalink: /pages/reference-host-soak-and-load-testing/
---

## Soak and endurance testing

A **soak test** runs the plugin host for a long time under a steady mix of work, then checks that its resources return to a stable level. An **endurance run** is one soak test that lasts hours or days. A **churn cycle** is one activation followed by one deactivation of the same plugin. A **live instance** is a component instance that the host holds in memory. A **helper process** is a sandbox helper that runs T2 or T3 components (see [Sandbox helper](/pages/reference-host-helper/)).

A short benchmark cannot show a slow leak. This page defines the endurance test that the release gate runs, the resources it watches, and the pass criteria. Requirement ids are `[HST-505]` to `[HST-508]`.

### You will need

- [Performance budgets and benchmarks](/pages/reference-host-budgets/) for the timing budgets and the idle wake-up measurement B24.
- [Telemetry and plugin health](/pages/reference-host-telemetry/) for the counters that the host keeps.
- [Host runtime](/pages/reference-host-runtime/) for how instances and their memory are released.
- [Host scheduling](/pages/reference-host-scheduling/) for the call classes that a soak run exercises.

## What the run does

A soak run starts one server with every plugin that the test set installs. It then repeats the following steps in a fixed order, and records the cycle number after each one:

1. Activate a plugin, make one call of each of its call classes, and deactivate it.
2. Restart a sandbox helper and repeat the calls of the T2 or T3 plugins that it hosted.
3. Detach a client and attach a new one (a session reconnect), then read the tree and the plugin list.
4. Disable, enable, install and uninstall a plugin that is not in use by another plugin.

The run also sends a trapping call at a fixed point in each cycle, so that the trap path is part of the churn.

- **[HST-505]** A soak run MUST repeat the four steps above for every plugin in the test set and MUST record, at the end of each cycle, the number of live instances, the number of helper processes, the number of open file descriptors and handles of the server, and the resident memory of the server and of each helper. A run MUST NOT skip a step because an earlier step failed.
- **[HST-506]** The steady state MUST be measured after the first full cycle. After that, the counts of live instances, helper processes and open handles MUST equal the steady-state counts at the end of every cycle, and the resident memory MUST NOT grow over the run beyond the tolerance set for the release.

Decision needed: the duration of a soak run and the length of the tolerance window. The default I would use is a 24-hour run for the nightly lane and a 72-hour run before each release, because a slow leak in a plugin that is activated once per hour needs several hours to show. The resident memory tolerance is a growth of at most 5 MiB over the last 24 hours of a run, because the steady-state variation of the allocator is of that order. Both values are a choice for the project owner and are not established by the current budgets.

## Pass criteria

A run passes only when every check of [HST-506] holds and no check of [HST-507] fails.

- **[HST-507]** The soak run MUST fail when a live instance, a helper process or an open handle remains after its owning plugin is deactivated, disabled or uninstalled, when a helper that was restarted is not replaced by a new process, or when the server records a restart of itself during the run. The failure MUST name the resource and the cycle in which it first appeared.

Decision needed: the interval between samples. The default I would use is one sample per cycle, with a cycle length of at most one minute, so that a 24-hour run yields at least 1,440 samples. The sample interval does not change the pass criteria.

## Idle cost over a long run

The idle wake-up budget B24 measures 40 loaded plugins with no events for 10 minutes. A soak run that is idle between cycles MUST keep that budget, so that a long idle period does not hide a timer that accumulates work.

- **[HST-508]** A soak run MUST run on the reference machine with a release build, and the run record MUST name the commit, the Wasmtime version and the number of cycles completed. A run that stops before its planned number of cycles MUST be reported as incomplete and MUST NOT count as a pass.

## Reading the results

The run record holds one row per cycle with the counts listed in [HST-505]. The record is kept with the release evidence. A failing run keeps its record so that the first cycle where a count rose can be found.

For a plugin in a running server, `ilium plugin status` shows the health and the metrics of each installed plugin, as described in [Health and watchdog](/pages/reference-health-and-watchdog/). It does not replace the run record, because it shows a snapshot and not the trend.

## See also

- [Performance budgets and benchmarks](/pages/reference-host-budgets/) for the timing budgets and the release gate.
- [Telemetry and plugin health](/pages/reference-host-telemetry/) and [Health and watchdog](/pages/reference-health-and-watchdog/).
- [Sandbox helper](/pages/reference-host-helper/) for helper restarts.

API version checked: `ilium:plugin@0.1.0`.
