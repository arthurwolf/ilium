---
layout: page
title: Platform support
permalink: /pages/reference-platform-support/
---

## Platform support

The plugin host runs the same Wasmtime configuration on every platform that Ilium ships on, but the sandbox for third-party plugins needs Linux kernel features. This page lists which platforms run which tiers, what differs between them, and how the host stops processes on each. It describes the contract. The platform is not implemented yet, and rows marked "not shipped" are not committed. Requirement ids are `[HST-380]` to `[HST-388]`.

## You will need

- [Sandbox reference](/pages/reference-sandbox/) for what the Linux sandbox requires.
- [Host runtime](/pages/reference-host-runtime/) for the engine configuration and `.cwasm` rules.
- [Sandbox helper protocol](/pages/reference-host-helper/) for the helper.

## Support matrix

| Platform | Architecture | Ilium client and server | T1 (bundled) | T2 and T3 (third-party) | Notes |
|---|---|---|---|---|---|
| Linux | x86_64 | Shipped | Yes | Yes, with the sandbox prerequisites | Reference platform |
| Linux | aarch64 | Shipped | Yes | Yes, with the sandbox prerequisites | `.cwasm` for this triple is separate |
| macOS | aarch64 | Shipped | Yes | Refused | `sandbox.unavailable` |
| macOS | x86_64 | Not shipped | None | None | Not committed |
| Windows | x86_64 | Shipped | Yes | Refused | `sandbox.unavailable` |
| Windows | aarch64 | Not shipped | None | None | Not committed |
| WSL 2 | as the Linux host | As Linux | Yes | Yes if the kernel prerequisites hold | A real Linux kernel |
| Linux container | as the Linux host | As Linux | Yes | Only if user namespaces and cgroup delegation are granted | Many defaults refuse |

- **[HST-380]** On Windows and macOS the host MUST refuse to load T2 and T3 plugins with `sandbox.unavailable`, naming the platform, and MUST NOT run a third-party plugin without the helper confinement on any platform.
- **[HST-381]** The plugin host MUST work for T1 plugins on every shipped platform with the same Wasmtime version and the same `runtime_config()`, except for the platform switches in this page.
- **[HST-382]** A package for a platform that is not shipped MUST NOT be reported as supported in any user interface.

## Precompiled artifacts

A `.cwasm` file is machine code for one target triple and one Wasmtime fingerprint. The release embeds, for every T1 plugin, the `.wasm` component and one `.cwasm` per shipped target triple. The host picks the `.cwasm` that matches the running triple and fingerprint. If none matches, it compiles the `.wasm` with `Component::new` and the compile cache, and logs `host.cwasm-incompatible`.

| Target triple | `.cwasm` embedded |
|---|---|
| `x86_64-unknown-linux-gnu` | Yes |
| `aarch64-unknown-linux-gnu` | Yes |
| `aarch64-apple-darwin` | Yes |
| `x86_64-pc-windows-msvc` | Yes |

A third-party package never carries a `.cwasm` (see [Host runtime](/pages/reference-host-runtime/)). Cross-compiling a `.cwasm` for another triple uses the same `runtime_config()` with the target set, in the release build only.

- **[HST-383]** The release MUST embed the `.wasm` and one `.cwasm` per shipped triple for each T1 plugin, and the host MUST fall back to `Component::new` when no `.cwasm` matches.

## Differences that a plugin can observe

| Aspect | Linux | macOS | Windows |
|---|---|---|---|
| Local IPC | Unix domain socket | Unix domain socket | Named pipe |
| Trap handling in Wasmtime | Signals-based traps | Signals-based traps (Mach exceptions are not used) | Structured exception handling |
| Guard region | 32 MiB guard, 4 GiB reservation | Same | Same |
| File path separators in `fs` | `/` | `/` | `\` accepted, normalized to `/` in plugin-visible paths |
| Case sensitivity of `fs` paths | Case sensitive | Case insensitive by default | Case insensitive |
| Time source | `CLOCK_MONOTONIC` (does not advance in suspend) | `mach_absolute_time` (does not advance in suspend) | `QueryPerformanceCounter` (advances in suspend; host ignores a tick gap over 1 s) |
| `process.run` | Allowed with grant | Allowed with grant | Allowed with grant; no helper confinement, so T1 only |
| Child termination | SIGTERM, then SIGKILL after 2 s | SIGTERM, then SIGKILL after 2 s | Job object: `TerminateJobObject` after 2 s; `CTRL_BREAK_EVENT` first for console children |

- **[HST-384]** The host MUST present paths to plugins with `/` separators on every platform, and MUST document case sensitivity per platform as above.
- **[HST-385]** The host MUST terminate child processes with SIGTERM then SIGKILL after 2 s on Linux and macOS, and with a job object after 2 s on Windows. Children MUST run inside the job object so that closing the job ends the whole tree.
- **[HST-386]** A deadline MUST NOT be reported as breached because a suspend gap passed on Windows. The host MUST ignore a tick gap of more than one second.

## What is the same everywhere

- The component model contract `ilium:plugin@0.1.0`, the WIT files, the tier and grant rules.
- The call classes, deadlines and ceilings of [Host scheduling](/pages/reference-host-scheduling/).
- The limit keys of [Errors and limits](/pages/reference-errors-and-limits/).
- The envelope and handshake of [Plugin traffic over IPC](/pages/reference-host-ipc/).

The results of the conformance suite MUST be identical on every shipped platform for T1 plugins. Timing benchmarks are measured per platform, and the gate in [Performance budgets and benchmarks](/pages/reference-host-budgets/) applies to the Linux reference machine only. Other platforms record their numbers.

- **[HST-387]** The conformance suite MUST pass on every shipped platform for T1 and, on Linux, for T2 and T3.
- **[HST-388]** The performance gate MUST apply to the Linux x86_64 reference machine, and other platforms MUST record results without a gate.

## Errors

| Id | When | Effect |
|---|---|---|
| `sandbox.unavailable` | Platform or prerequisite missing for T2 or T3 | The plugin shows `Cannot start: sandbox unavailable` with the platform |
| `host.cwasm-incompatible` | No `.cwasm` for the triple or fingerprint | Compiled at load; start is slower |

## Edge cases

| Situation | Result |
|---|---|
| A user moves a data directory between operating systems | Plugin data is not portable; paths in plugin storage are the plugin's own business |
| A T1 plugin on Windows uses `process.run` | Allowed with grant; the child sits in the job object |
| A Linux host without user namespaces | T1 works; T2 and T3 get `sandbox.unavailable` with the sysctl |
| Running under Rosetta on macOS | Not supported; the host reports the triple it was built for |

## See also

- [Host architecture](/pages/reference-host-architecture/) and [Packages](/pages/packages/).

API version checked: `ilium:plugin@0.1.0`.
