---
layout: page
title: Performance budgets and benchmarks
permalink: /pages/reference-host-budgets/
---

## Performance budgets and benchmarks

This page is the normative list of performance budgets of the plugin host and the benchmarks that check them. A budget is a number that the implementation MUST meet. A benchmark is a program in `ilium-plugin-bench` that measures one budget. The plugin author's view is in [Performance](/pages/performance/). Requirement ids are `[HST-300]` to `[HST-313]`.

## You will need

- [Host runtime](/pages/reference-host-runtime/) for the Wasmtime configuration that the benchmarks record.
- [Host scheduling](/pages/reference-host-scheduling/), [Plugin traffic over IPC](/pages/reference-host-ipc/), [Sandbox helper protocol](/pages/reference-host-helper/) and [Rendering pipeline](/pages/reference-host-rendering/) for what B10 to B24 measure.

## How a benchmark runs

- **[HST-300]** Each benchmark MUST run at least 1,000 measured iterations after at least 100 warm-up iterations, on one pinned core, with the CPU governor set to a fixed frequency where the platform allows it.
- **[HST-301]** Each result MUST record the median and the 99th percentile, the Wasmtime version, the Wasmtime `Config` (as the output of `runtime_config()`), the CPU model, the core count, the operating system and the commit.
- **[HST-302]** A benchmark MUST fail the run when the budget is missed, except where the table says "record" (a result without a target).
- **[HST-303]** Benchmarks MUST run on a release build with the target features of the shipped binary, and MUST NOT use a debug build.

The reference machine is the one named in the results file. A budget is checked on that machine. The numbers on a slower machine scale; the release gate (see [Roadmap](/pages/roadmap/)) uses the reference machine only.

## Stop rule

A miss of more than 2x on B1, B3 or B4 stops the platform plan. The design assumes that a host call costs about a microsecond and that WebAssembly code runs within a small factor of native code. A miss of that size means the assumption is wrong, and the project returns to design.

- **[HST-304]** A result more than twice the budget on B1, B3 or B4 MUST be reported as a plan stop, not as an ordinary failure.

## Benchmark table

| Id | What it measures | Budget |
|---|---|---|
| B1 | Empty host call; host call with a 64-byte string; a 400 KB `list<u8>` lowered into a host resource (`write-rows`) | Median under 1 µs; 400 KB under 0.2 ms |
| B2 | Instantiation from `Component::deserialize` and `InstancePre`: Rust component, JavaScript components. Wall time and RSS per instance | Rust under 5 ms and 4 MiB; JavaScript recorded, no target |
| B3 | Beach and Carpet at 200 by 60 cells, 300 frames: native Rust (SIMD and portable), Rust wasm, TypeScript on V8 and on JavaScript components | Rust wasm 99th percentile within 1.3x native; JavaScript routes within 1.1x of V8 where V8 is the baseline, otherwise recorded |
| B4 | 200 decoration updates through the contribution store plus one tree re-render at 300 nodes, against the in-process path | Under 1 ms extra |
| B5 | One 50-event batch to 30 instantiated plugins through `on-events`, returning 0 and 10 intents each | Under 2 ms at the 99th percentile |
| B6 | Cold server start with 40 bundled components: embedded `.cwasm` via `deserialize`, embedded `.wasm` via `Component::new` with and without the compile cache, eager and lazy activation, on-demand against pooling allocation. Binary size delta | Selects the activation mode and the allocator; HOST-START under 150 ms; added RSS under 120 MiB for the chosen option |
| B7 | B3 Rust wasm with epoch interruption and a 10 ms ticker, against without | Under 5 % |
| B8 | Detection `classify` with a 200 by 60 screen through the `detector` export, 30 panes | Under 0.5 ms per pane |
| B9 | A 2 s CPU job in a second instance and on the plugin's own instance with epoch yields every 10 ms: start overhead, RSS per job, 99th percentile of concurrent `on-events` | Start under 5 ms; concurrent 99th percentile within 1.2x of idle; selects the jobs model |
| B10 | A hook chain of 5 handlers (1 per plugin, T1) with a 1 KiB payload | Under 1 ms for the chain, excluding handler bodies |
| B11 | Opening a dynamic menu with 3 menu providers returning 20 entries each | Under 5 ms from click to first frame |
| B12 | Fairness: one plugin spins at its deadline continuously; 99th percentile `on-events` latency of 29 other plugins | Within 1.5x of idle |
| B13 | A `ui-tree` of 4,096 nodes: `views.render` return, caps check, client diff against the previous tree with 5 % changed nodes | Under 8 ms from return to painted frame |
| B14 | Contribution delta replication: 1,000 increments per second to 4 clients | Server CPU under 5 % of one core; client mirror lag under 5 ms at the 99th percentile |
| B15 | Client submission round trip: client plugin returns an intent, the server applies it, the outcome arrives | Under 3 ms at the 99th percentile on a local connection |
| B16 | Key press to start of the plugin call, T1 and T2 | 1 ms (T1) and 1.5 ms (T2) at the 99th percentile |
| B17 | Key press to bytes at the PTY through a 0.1 ms plugin handler | Under 5 ms at the 99th percentile |
| B18 | Helper round trip of a forwarded import | 0.2 ms median, 0.5 ms at the 99th percentile |
| B19 | Delay of a PTY frame while the plugin event queue is full (`event_queue` entries, 1,024 by default) | Under 1 ms at the 99th percentile |
| B20 | Server decoration change to the client mirror | Under 5 ms at the 99th percentile |
| B21 | Helper start: spawn to `Ready` for a 2 MiB component | Record only (no pass or fail threshold) |
| B22 | Session memory: 40 bundled plugins plus 8 T2 helpers, steady state | Added RSS under 120 MiB for the bundled set; each helper under the sum of its hosted instances' `memory_mib` plus 32 MiB |
| B23 | `ilium` CLI cold run (CLI-COLD) | Under 60 ms with a warm file cache |
| B24 | Idle cost: 40 loaded plugins, no events, 10 minutes | Under 0.5 % of one core; at most 100 wake-ups per second |

The B1 to B9 rows are the phase-0 benchmarks of the platform plan, kept with their targets. B10 to B24 are added by this reference.

- **[HST-305]** `ilium-plugin-bench` MUST contain one program for each of B1 to B24, with the id in its name.
- **[HST-306]** The release gate MUST include B1 to B9, B16 to B20 and B23 on the reference machine, and MUST fail when any budget is missed.

## Named budgets

| Name | Definition | Budget |
|---|---|---|
| HOST-START | Time from the server process start to the point where the host accepts its first IPC attach, with the 40 bundled plugins registered (lazy activation) and the compile step skipped through `.cwasm` | 150 ms |
| CLI-COLD | Time from the start of an `ilium` CLI subcommand that a plugin contributes (`requires_server: false`) to its exit, when the file cache is warm and the server is not running. It loads only the plugins that contribute that subcommand | 60 ms |
| KEY-PATH | Key press at the client to bytes at the PTY, when a plugin handles the key | 5 ms at the 99th percentile |
| ADD-RSS | Resident memory added by the plugin host with the 40 bundled plugins registered, none activated | 120 MiB (including all of the above) |

- **[HST-307]** HOST-START MUST be at most 150 ms on the reference machine for the 40 bundled plugins, measured by B6.
- **[HST-308]** CLI-COLD MUST be at most 60 ms, measured by B23, and MUST NOT start the server or any plugin that does not contribute the subcommand.
- **[HST-309]** The resident memory that the host adds with the bundled plugins registered MUST be under 120 MiB.

## Ratios against native code

| Workload | Ratio | Baseline |
|---|---|---|
| Beach and Carpet in Rust wasm | 99th percentile at most 1.3x | Native Rust |
| Beach and Carpet in TypeScript on a JavaScript component | At most 1.1x | The V8 path used today |
| Epoch interruption overhead | At most 5 % | The same workload without epochs |

- **[HST-310]** The Rust wasm scenes MUST be within 1.3x of native Rust at the 99th percentile on B3.
- **[HST-311]** The epoch overhead on B3 MUST be at most 5 % (B7).

## Choosing between options

Some benchmarks select an option rather than pass or fail:

| Benchmark | Decision |
|---|---|
| B6 | Pooling allocator is chosen only if the on-demand allocator misses HOST-START or B2, and pooling meets both with at most 16 MiB more RSS. Otherwise the on-demand allocator stays |
| B6 | Lazy activation is the default. Eager activation is chosen only if the lazy path misses KEY-PATH on the first key |
| B9 | If a job in a second instance meets the budgets, jobs use a second instance. Otherwise jobs share the plugin instance with epoch yields |

- **[HST-312]** The pooling allocator MUST be enabled only when B6 shows that on-demand misses HOST-START or B2, that pooling meets both, and that pooling adds at most 16 MiB of RSS.

## Runtime telemetry against the budgets

The host keeps the counters that these budgets need: call durations per class, queue depths and drops, quota rejections, helper round trips, render durations, and memory per plugin. See [Telemetry and plugin health](/pages/reference-host-telemetry/) for the names. The same data feeds the `degraded` health condition "near-misses" of [Health and watchdog](/pages/reference-health-and-watchdog/): 5 or more calls in 5 minutes that took at least 80 % of their deadline.

- **[HST-313]** The host MUST count a call as a near-miss when it ends in time but took at least 80 % of its deadline, and MUST use that count for the `degraded` health condition.

## See also

- [Performance](/pages/performance/) for the guidance to plugin authors.
- [Resource accounting](/pages/reference-resource-accounting/) and [Platform support](/pages/reference-platform-support/).

API version checked: `ilium:plugin@0.1.0`.
