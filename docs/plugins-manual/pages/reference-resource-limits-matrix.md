---
layout: page
title: Resource limits matrix
permalink: /pages/reference-resource-limits-matrix/
---

## Resource limits matrix

This page specifies one table that lists every per-plugin resource limit in one place: memory, CPU, time, stack, processes, file descriptors and disk. For each limit it gives the default, the manifest key, the hard host cap, the enforcement mechanism, the exact error code that the plugin sees, what the operator sees, and whether a breach counts toward the three-failure window. Plugin authors need it to predict what a breach does to their code, and operators need it to read health reports and logs; the table is generated from the host constants, so it cannot drift from the code.

## Matrix: limit, default, hard maximum, enforcer

The matrix has one row for each of the seven limits. Every row has a default, a manifest key where one exists, a hard host cap, the mechanism that enforces the limit and the error code. Memory is capped by the `memory_mib` key, which defaults to 32 MiB. The ceiling is 256 MiB for a tier 1 plugin and 128 MiB for a tier 2 or tier 3 plugin. The Wasm engine traps the instance on a breach with `trap.memory`. When the helper runs under cgroup v2, the kernel enforces `memory.max`, which is the sum of the hosted instances' `memory_mib` plus 32 MiB of overhead.

| Limit | Default | Manifest key | Hard host cap | Enforcer | Error code |
|---|---|---|---|---|---|
| Memory | 32 MiB | `memory_mib` | 256 MiB at tier 1, 128 MiB at tiers 2 and 3 | Cgroup `memory.max` of the helper, or `RLIMIT_AS` when no controller is delegated | `trap.memory` |
| CPU | `cpu.weight` of 10 for each helper | `cpu_ms_per_s` | The `cpu.max` quota that the manifest key sets | Cgroup `cpu.max` of the helper, and the `ilium-execution` budget of 2,000 ms, refilled at 500 ms per second | `quota-exceeded`, with `retry-after-ms` set |
| Time | `event_deadline_ms` of 20 ms | `event_deadline_ms`, `command_deadline_ms` and the other deadline keys | 50 ms for `event_deadline_ms` at tier 1, and 20 ms at tiers 2 and 3 | Host deadline on each call class | `trap.deadline` |
| Stack | 512 KiB of Wasm stack (`max_wasm_stack`) | None | 512 KiB, fixed by the host and not raised by the manifest | Wasm engine stack limit | `host.stack-overflow` |
| Processes | `pids.max` of 64 plus 8 for each allowed child | None | The `pids.max` value of the helper's cgroup | Cgroup `pids.max` of the helper | `busy` with the reason `process-limit` |
| File descriptors | 64 descriptors for each helper | None | 64 descriptors, set by the helper | `RLIMIT_NOFILE` set by the helper | `quota-exceeded` |
| Disk | 256 MiB (`disk_mib`, covering `data` and `cache` together) | `disk_mib` | 4,096 MiB at tier 1, and 256 MiB at tiers 2 and 3 | Host measurement of the allocated blocks of `data` and `cache` | `failed(quota-exceeded)` |

The time limit is the deadline of each call class, not a single number. A breach of a hard deadline in `activate`, `on-events` or `views.render` traps the instance with `trap.deadline`. The deadlines of every call class are listed in [Errors and limits](/pages/reference-errors-and-limits/).

The processes limit is enforced by the cgroup of the helper. The helper cannot start more processes than `pids.max` allows. A spawn that reaches the limit fails with an error, and the helper keeps running.

The file descriptor limit applies to the helper process. The helper closes every descriptor that it does not need before it runs the plugin code, and the limit covers the descriptors that remain open.

## Result seen by the plugin vs by the operator

A plugin sees a limit breach as a value that it can handle, or as a trap that ends the current call. The error code in the table is the one the plugin receives. The operator sees the breach in the plugin log and in the health report of the plugin. The log is read with `ilium plugin logs`, and the health report is read with `ilium plugin health <id> --json`.

When a breach kills the helper, the host writes a log line that names the limit and the observed value at the time of the breach. For a memory kill, the line contains `limit: memory` and an `observed` byte count, which is at least the configured limit. The host also writes a line for every trap, so a stack trap appears in the log under `host.stack-overflow`.

A processes breach is different. When a spawn returns `busy` with the reason `process-limit`, the helper is not killed, the spawning call returns the error, and the plugin keeps answering later calls. The health report does not change for this breach.

A file descriptor breach returns `quota-exceeded` to the call that opens the descriptor, and the health report keeps its previous failure count.

Disk breaches return `failed(quota-exceeded)` in the completion of the file call. The plugin can free space and retry, and the operator sees the breach in the log.

## Counting as failure or not

Only some breaches count toward the three-failure window. The window is the count that the host keeps for each plugin, and three counted failures lead to a restart or a disable, as described in [Errors and limits](/pages/reference-errors-and-limits/). The health report shows the count as `failures_in_window`.

Memory and stack breaches count. A memory breach traps the instance, and the helper kill counts as one failure. A stack breach traps the instance and counts as one failure. A hard time breach counts as one failure. A soft time breach counts only when it reaches the streak limit of its call class or runs 100 ms past its deadline.

Processes, file descriptors, disk and CPU breaches do not count. Each of these returns an error to the call that breached the limit, so the plugin is not trapped, and the host does not count a failure. A refused spawn, a refused open, a refused write and a CPU budget refusal all leave `failures_in_window` at 0.

## Test for each row

Each row has a test that the manual tests can run. The matrix test parses the table on this page and checks that memory, CPU, time, stack, processes, file descriptors and disk each appear once. It also checks that every row has a value in all six columns.

The stack test calls an export that recurses without a bound. The result must be a trap with the code `host.stack-overflow`, and `ilium plugin health <id> --json` must show `failures_in_window: 1`.

The processes test spawns child processes until the limit is reached. The failing spawn must return `busy` with the reason `process-limit`, the helper process must keep the same process ID, and the plugin must keep answering calls.

The memory test allocates past the memory limit. The log line for the kill must contain `limit: memory` and an `observed` byte count at or above the configured limit.

The file descriptor test opens descriptors past the limit and checks that `failures_in_window` stays at 0.

The drift test changes the default memory limit constant in the host source without regenerating this page, and runs the manual tests. The drift test must fail, and the failure must name the memory row.

## Generation from host constants

This page is generated from the same host constants that the host uses to enforce the limits. A value in the table is never typed by hand. When a constant changes, the page is regenerated from the new value, and the drift test checks that the published table matches the constants. A table that does not match the constants makes the manual tests fail.

Operators who need the full list of keys, ceilings and call classes should read [Errors and limits](/pages/reference-errors-and-limits/). The sandbox that enforces the cgroup limits is described in [Sandbox](/pages/reference-sandbox/), and the operator steps for reading health reports are in [Debugging](/pages/debugging/).

## Requirements

- **[PDZ-016]** Every limit MUST have exactly one row giving its default, manifest key, hard host cap, enforcement mechanism and the exact error code (`trap.memory`, `trap.deadline`, `host.stack-overflow`, `quota-exceeded`, `helper.unstable` or another named code).
- **[PDZ-017]** Exhausting the Wasm stack MUST trap with the code `host.stack-overflow` and MUST count as one failure in the three-failure window.
- **[PDZ-018]** Reaching `pids.max` MUST return `busy` with the reason `process-limit` to the plugin on the spawning call, as defined by [SEC-039], and MUST NOT kill the helper.
- **[PDZ-019]** A limit breach that kills the helper MUST be logged with the limit name and the observed value at the time of the breach.
- **[PDZ-020]** Each row MUST state whether a breach counts toward the three-failure window, and the host behaviour MUST match that column.
- **[PDZ-021]** The matrix MUST be generated from the same source as the host constants, and a test MUST fail when the published table and the constants differ.
