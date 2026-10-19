---
layout: page
title: Sandbox helper protocol
permalink: /pages/reference-host-helper/
---

## Sandbox helper protocol

Plugins of tiers T2 and T3 run inside a helper process. This page defines how the host starts a helper, the messages between the host and the helper, how deadlines include the round trip, how helper memory is charged, what limits the helper count, and how a helper dies. The confinement of the helper (namespaces, seccomp, cgroup) is defined in the [Sandbox reference](/pages/reference-sandbox/) and is not repeated. Requirement ids are `[HST-150]` to `[HST-171]`.

## You will need

- [Sandbox reference](/pages/reference-sandbox/) for confinement and the trust rules for messages.
- [Host runtime](/pages/reference-host-runtime/) for the Wasmtime configuration that the helper shares.
- [Health and watchdog](/pages/reference-health-and-watchdog/) for heartbeats and restart counting.

## What a helper is

The helper is the binary `ilium-plugin-helper`, installed beside `ilium`. It contains the same Engine configuration as the host (one `runtime_config()` function, see [Host runtime](/pages/reference-host-runtime/)) and a small call loop.

| Item | Rule |
|---|---|
| Granularity | One helper per plugin per side per hosting process, as in the [Sandbox reference](/pages/reference-sandbox/). The server hosts one helper per T2 or T3 plugin side. Each client that runs a client half starts its own |
| Platform | Linux only. On Windows and macOS the host refuses T2 and T3 with `sandbox.unavailable` (see [Platform support](/pages/reference-platform-support/)) |
| Start | Lazy, at the first activation of the plugin side |
| Stop | At deactivation, at a trap, at shutdown, or when the host kills it |
| Instances | One Store at a time, one call at a time |

- **[HST-150]** The host MUST start one helper per T2 or T3 plugin side and per hosting process, and MUST NOT host two plugins in one helper.
- **[HST-151]** The helper binary MUST be `ilium-plugin-helper` in the same directory as the `ilium` executable, MUST use the same `runtime_config()` as the host, and MUST refuse to start if the host passes a different Wasmtime fingerprint.
- **[HST-152]** A helper MUST start only at the first activation of its plugin side.

## Starting a helper

The host starts a helper in this order. Each step is checked, and a failure stops the start with `helper.spawn-failed` and the failing step.

1. The host creates a `socketpair` (the dispatcher channel). It does not use a named socket or a path.
2. The host forks. In the child, before `exec`, it sets up bubblewrap-style namespaces, the mount layout, `no_new_privs` and the cgroup (or `RLIMIT_AS` and `RLIMIT_NPROC` when no controller is delegated), as in the Sandbox reference.
3. The child closes every file descriptor except the dispatcher channel and stdio on `/dev/null`, installs the seccomp filter, and executes `ilium-plugin-helper`.
4. The helper sends `Hello` with its protocol version and Wasmtime fingerprint.
5. The host checks the fingerprint and sends `Instantiate` with the component bytes (or their cache key), the grants, the limits and the `restored` bytes.
6. The helper compiles or loads the component, instantiates it and answers `Ready`.
7. The host then sends `CallExport` for `activate`.

The component bytes are sent over the channel in chunks of at most 4 MiB. The helper holds the file in memory only and never gets a path into the package store.

- **[HST-153]** The dispatcher channel MUST be an inherited `socketpair`. The helper MUST NOT create sockets or open any path outside its mounts to receive work.
- **[HST-154]** The host MUST treat a missing `Hello`, a wrong fingerprint or a `Ready` later than 5,000 ms after `Instantiate` as `helper.spawn-failed`, kill the helper, and count one failure only when the plugin had already started once.

<figure>
<svg viewBox="0 0 440 250" role="img" aria-label="Sequence of helper start and a guest call with a forwarded host call">
<title>Helper start and call</title>
<desc>The host spawns the helper, receives Hello, sends Instantiate, receives Ready. For a call the host sends CallExport. The guest makes a host call. The helper forwards HostCall. The host sends HostReply. The helper sends the call result.</desc>
<defs>
<marker id="hh-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
<path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
</marker>
</defs>
<text x="80" y="16" font-size="11" text-anchor="middle" fill="currentColor">Host</text>
<text x="360" y="16" font-size="11" text-anchor="middle" fill="currentColor">Helper</text>
<line x1="80" y1="24" x2="80" y2="240" stroke="currentColor" stroke-dasharray="4 4"/>
<line x1="360" y1="24" x2="360" y2="240" stroke="currentColor" stroke-dasharray="4 4"/>
<line x1="360" y1="42" x2="80" y2="42" stroke="currentColor" marker-end="url(#hh-arrow)"/>
<text x="220" y="36" font-size="10" text-anchor="middle" fill="currentColor">Hello (protocol, fingerprint)</text>
<line x1="80" y1="68" x2="360" y2="68" stroke="currentColor" marker-end="url(#hh-arrow)"/>
<text x="220" y="62" font-size="10" text-anchor="middle" fill="currentColor">Instantiate (component, grants, limits)</text>
<line x1="360" y1="94" x2="80" y2="94" stroke="currentColor" marker-end="url(#hh-arrow)"/>
<text x="220" y="88" font-size="10" text-anchor="middle" fill="currentColor">Ready</text>
<line x1="80" y1="130" x2="360" y2="130" stroke="currentColor" marker-end="url(#hh-arrow)"/>
<text x="220" y="124" font-size="10" text-anchor="middle" fill="currentColor">CallExport (call id, deadline)</text>
<line x1="360" y1="158" x2="80" y2="158" stroke="currentColor" marker-end="url(#hh-arrow)"/>
<text x="220" y="152" font-size="10" text-anchor="middle" fill="currentColor">HostCall (import, args)</text>
<line x1="80" y1="184" x2="360" y2="184" stroke="currentColor" marker-end="url(#hh-arrow)"/>
<text x="220" y="178" font-size="10" text-anchor="middle" fill="currentColor">HostReply</text>
<line x1="360" y1="212" x2="80" y2="212" stroke="currentColor" marker-end="url(#hh-arrow)"/>
<text x="220" y="206" font-size="10" text-anchor="middle" fill="currentColor">CallResult</text>
</svg>
<figcaption>Every message of one call carries the call id. The time from CallExport to CallResult is one deadline.</figcaption>
</figure>

## Messages

The channel carries length-prefixed `bincode` messages. A message is at most 4 MiB (8 MiB for a `terminal-view` frame), at most 5,000 messages per second per helper, as in the Sandbox reference.

| Message | From | Body | Reply |
|---|---|---|---|
| `Hello` | Helper | Protocol version, Wasmtime fingerprint, helper build | `Instantiate` |
| `Instantiate` | Host | Component bytes in chunks, grants, limits, restored bytes | `Ready` or `InstantiateFailed(code)` |
| `Ready` | Helper | Exports found, memory at start | none |
| `CallExport` | Host | Call id, export name, argument bytes, deadline in ms | `CallResult` |
| `CallResult` | Helper | Call id, result bytes, or a trap with its code | none |
| `HostCall` | Helper | Call id, import name, argument bytes | `HostReply` |
| `HostReply` | Host | Call id, result bytes or an error | none |
| `Cancel` | Host | Call id | none (the call ends in `CallResult` with a trap or its normal result) |
| `Shutdown` | Host | Reason | helper exits with code 0 |
| `Heartbeat` | Both | Counter | `Heartbeat` echo, answered on the helper control thread |
| `Log` | Helper | Level, line (max 16 KiB), call id | none |

- **[HST-155]** Messages MUST be length-prefixed `bincode`, MUST NOT exceed the sizes above, and a message of an unknown kind, a bad length or a schema violation MUST end the helper and count as a failure.
- **[HST-156]** The host MUST derive the plugin identity from the channel and MUST drop any message that names another plugin (see [Sandbox reference](/pages/reference-sandbox/)).
- **[HST-157]** A `CallResult`, `HostCall` or `HostReply` MUST carry a call id that the host issued. A message with an unknown or finished call id MUST be dropped and logged `helper.stale-call`.

## Data that is large

A bulk argument (a `ui-tree`, a surface write, a file chunk, an HTTP body) is sent as a sequence of messages, each at most 4 MiB, tagged with the call id and a chunk index. The sender marks the last chunk. The receiver assembles the chunks and enforces the same caps as [Plugin traffic over IPC](/pages/reference-host-ipc/). There is no shared memory between the host and the helper: the seccomp filter and the descriptor rule of the Sandbox reference leave only the dispatcher channel open.

- **[HST-158]** Bulk data MUST travel in chunks of at most 4 MiB over the dispatcher channel. The host MUST NOT pass file descriptors or shared memory to a helper.

## Which imports run where

| Class | Imports | Where |
|---|---|---|
| Pure compute | `codec`, `vector`, `clock.now` (coarsened), `random`, `log` (batched) | In the helper. No round trip |
| Host state or effect | Everything else: `storage`, `fs`, `http`, `process`, `ui`, submissions, `messages`, `timers`, `jobs`, `inference`, `clipboard` | Forwarded to the host with `HostCall` |

A forwarded import takes one round trip. The budget of a round trip is 0.2 ms median and 0.5 ms at the 99th percentile on an idle machine. A call with 20 forwarded imports therefore spends at most 10 ms in round trips at the 99th percentile. Authors keep this in mind for the 20 ms batch deadline (see [Performance](/pages/performance/)).

The host evaluates the grant check, the quota and the limits for a forwarded import exactly as for a T1 import. The helper never decides a grant.

- **[HST-159]** Only the pure-compute imports MAY run in the helper without a round trip. Every other import MUST be forwarded and checked by the host.
- **[HST-160]** A forwarded import round trip MUST take at most 0.2 ms median and 0.5 ms at the 99th percentile on an idle reference machine (benchmark B18).

## Deadlines include the round trip

The deadline of a call is carried in `CallExport`. The helper sets its own epoch deadline from it, so a runaway guest traps in the helper at the first tick after `d` ms, exactly as in the host. The host also runs a timer.

| Event | Result |
|---|---|
| The call returns inside the deadline | `CallResult` is delivered normally |
| The helper epoch deadline fires | The helper traps the instance and sends `CallResult` with `trap.deadline` |
| No `CallResult` arrives within 500 ms after the deadline | The host terminates the helper (SIGTERM, then SIGKILL after 2 s, as in HST-171) and treats it as `helper.hung`. One failure is counted |
| Three heartbeats in a row are missed | Same as above |

The time spent in the host serving `HostCall` messages counts inside the call's deadline, because the guest is waiting. The time that the host spends before it sends `CallExport` (queue time) does not.

- **[HST-161]** The deadline of a helper call MUST include the time of every `HostCall` round trip made during that call.
- **[HST-162]** The helper MUST set its own epoch deadline from the `CallExport` deadline. The host MUST kill a helper that has not answered 500 ms after that deadline and MUST count one failure.
- **[HST-163]** The host MUST kill a helper after three missed heartbeats (a ping every 1 s).

## Memory

The memory of the helper is charged to the session. The charge for one helper is the sum of the hosted instances' `memory_mib` plus a fixed overhead of 32 MiB, which is the same overhead that the cgroup limit uses.

| Item | Charge |
|---|---|
| Helper | sum of the hosted instances' `memory_mib` + 32 MiB |
| T1 instance in the host | `memory_mib` |
| Session total limit | `session_memory_mib` 1,024 (see [Errors and limits](/pages/reference-errors-and-limits/)) |

When the charge for an activation would pass `session_memory_mib`, the activation is refused with `session.memory-exceeded`. This refusal does not count as a failure, because the plugin did nothing wrong. The side stays `inactive` with the reason `host-capacity` and activation is retried, as in [Errors and limits](/pages/reference-errors-and-limits/).

With cgroup v2 delegated, the kernel enforces `memory.max`. Without it, the helper uses `RLIMIT_AS` and the host samples RSS once a second. An out-of-memory kill is reported as a `trap.memory` and counts as a failure.

- **[HST-164]** The host MUST charge the sum of the hosted instances' `memory_mib` plus 32 MiB per helper to `session_memory_mib` at spawn and release it at exit.
- **[HST-165]** An activation that would exceed `session_memory_mib` MUST be refused with `session.memory-exceeded`, MUST NOT count as a failure, and MUST leave the side `inactive` with the reason `host-capacity`.

## Helper count

The number of helpers per session is capped by `session_helpers`: default 16; a user may only lower it under `[plugins.limits]` (HST-406). A helper that is idle (the plugin has no calls and no work for 60 s) is shut down with `Shutdown` and restarted on demand; its Store is dropped after a `deactivate` with state saved.

When the cap is reached, the host refuses a further activation with `helper.too-many`. This refusal does not count as a failure, because the plugin did nothing wrong. The plugin shows as `installed-not-running` with the reason, and it activates when a helper is free.

- **[HST-166]** The host MUST cap live helpers at `session_helpers` and MUST refuse a further activation with `helper.too-many`, without counting a failure.
- **[HST-167]** The host SHOULD shut down a helper that has been idle for 60 s, after `deactivate`, and start it again at the next need.

## Death of a helper

| Cause | What the host sees | Effect on this plugin | Other plugins |
|---|---|---|---|
| Crash during a call | Channel closed | One failure; pending requests complete as `cancelled` | None, because helpers are per plugin |
| Crash while idle | Channel closed | No failure; the host restarts it at the next need | None |
| Hung (no answer) | Missed answer or heartbeats | One failure; the host kills it | None |
| Killed for memory | Exit signal | One failure | None |
| Protocol error | Bad message | One failure; the host kills it | None |
| Host asked it to stop | `Shutdown` | No failure | None |

The host restarts a helper at most 5 times in 10 minutes. The sixth restart attempt moves the server side of the T2 and T3 plugins of the session to `stopped-by-failure` with the reason `helper.unstable` (see [Health and watchdog](/pages/reference-health-and-watchdog/)). The restart rule for the plugin itself (1 s, 5 s, then disable) applies in addition.

On termination the host sends SIGTERM and then SIGKILL after 2 s. On shutdown of the server, all helpers receive `Shutdown`, then SIGKILL after 2 s.

- **[HST-168]** A helper that dies during a call MUST count one failure for its plugin, and a helper that dies while idle MUST NOT count one.
- **[HST-169]** After a helper dies, every request pending on it MUST complete as `cancelled`, and no retained state in the helper MUST be assumed.
- **[HST-170]** The host MUST allow at most 5 helper restarts in 10 minutes for a session, and MUST then disable the T2 and T3 plugins of that session with `helper.unstable`.
- **[HST-171]** Termination MUST be SIGTERM followed by SIGKILL after 2 s.

## Errors

| Id | When | Counts |
|---|---|---|
| `helper.spawn-failed` | A start step failed, wrong fingerprint, `Ready` too late | only after a first successful start |
| `helper.protocol-error` | Malformed or unknown message | yes |
| `helper.hung` | No answer 500 ms after the deadline, or three missed heartbeats | yes |
| `helper.stale-call` | Message for an unknown call id | no |
| `helper.too-many` | `session_helpers` reached | no |
| `helper.unstable` | More than 5 starts in 10 minutes | disables T2 and T3 of the session |
| `session.memory-exceeded` | Charge would pass `session_memory_mib` | no |
| `sandbox.unavailable` | Prerequisites missing, root user, or non-Linux platform | no |

## See also

- [Host scheduling](/pages/reference-host-scheduling/) for deadlines and cancellation.
- [Platform support](/pages/reference-platform-support/) for the platforms that can run a helper.
- [Resource accounting](/pages/reference-resource-accounting/) and [Threat model](/pages/reference-threat-model/).

API version checked: `ilium:plugin@0.1.0`.
