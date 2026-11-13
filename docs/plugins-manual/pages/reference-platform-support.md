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

## Supported terminals

The client draws in the terminal it runs in, so that terminal decides which colours and keys reach a plugin. The table names the terminal hosts that each shipped platform must be tested in. A terminal that the release notes do not name is not part of the support statement.

| Platform | Terminal hosts to test | Notes |
|---|---|---|
| Linux (x86_64 and aarch64) | GNOME Terminal, Konsole, kitty | Any terminal that reports the colour depth of HST-390 below |
| macOS (aarch64) | Terminal.app, iTerm2 | Legacy keyboard unless the terminal reports the kitty protocol |
| Windows (x86_64) | Windows Terminal | The client runs under ConPTY, so the terminal host sets colour depth and key reports |

- **[HST-389]** The release notes MUST name, for each shipped platform, the terminal emulators from the table above that the release was tested in, and MUST NOT describe the client as supported in any other terminal (proposed, owner to confirm: the table is the minimum set). Test: read the release notes and compare the named terminals with the release test log.
- **[HST-390]** The client MUST expect at least 256 colours from the outer terminal (proposed, owner to confirm). When the outer terminal offers fewer colours, the client MUST still start, MUST map each `indexed` and `rgb` colour of a plugin to the nearest colour the terminal offers, and MUST NOT fail the plugin's call. Test: start the client in a 16-colour terminal and read the first paint of a plugin that uses an `rgb` colour.

The colour rules of [Accessibility and theming](/pages/accessibility-and-theming/) state the contrast of the resolved colour, so the mapping above can change a plugin's contrast. Key release reports and the legacy keyboard fallback are described in [API keys](/pages/api-keys/).

## Platform support lifecycle

A shipped platform leaves the matrix only through the notice below. A platform that is not shipped has no lifecycle and needs no notice.

- **[HST-391]** A shipped platform MUST NOT be removed from the support matrix in a release unless the minor release before it named the removal in its release notes (proposed, owner to confirm: at least one minor release before). A patch release MUST NOT remove a platform. Test: read the release notes of the minor release before a removal and assert the notice; compare the matrix of two consecutive patch releases.
- **[HST-392]** When a shipped platform is removed, the release MUST stop shipping Ilium and its T1 bundled plugins for that platform, including the `.cwasm` for its triple (HST-383). An installed T2 or T3 plugin whose package targets the removed platform MUST be marked `refused-at-load` with `package.platform-unsupported` and MUST keep its data, as HST-655 requires for a restore on another operating system. Test: remove a platform in a test build, start the host on it, and read the state of each installed plugin.

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

## Windows: files held open by another process

On Windows a file that another process holds open cannot be renamed, replaced or deleted. Real-time virus scanners, the search indexer and backup tools open new files for a short time, so a rename that follows a write fails now and then with a sharing violation (`ERROR_SHARING_VIOLATION`) or with access denied (`ERROR_ACCESS_DENIED`). Linux and macOS do not have this failure, so the rules below apply to Windows only.

| Step | Rule |
|---|---|
| First failure with a sharing violation or access denied | The host retries the same operation. It waits 25 ms before the first retry and doubles the wait each time, up to 400 ms |
| Total retry time | 2 seconds from the first failure |
| Result when a retry succeeds | The call succeeds. The plugin or the user sees no error |
| Result when the 2 seconds end | The operation fails with `io`. The error message starts with `file-in-use:` and names the path |
| State of the target after a final failure | The previous file is intact. No partial file exists under the target name, and the host has removed its temporary file |

The rule covers the plugin calls `fs.rename`, `fs.remove`, `fs.write` in `replace` mode and `write-atomic` ([fs](/pages/api-fs/)), and every atomic replace the host does itself: package install, update and uninstall, the session snapshot, `installed.json`, `grants.json`, `consents.json`, the trust store and `config.toml`. A plugin does not retry inside its handler; the host has already waited. An `io` error whose message starts with `file-in-use:` is retryable later, like any transient `io` error. The message is for logs and for the user; a plugin decides on the code `io` alone. The retry time counts as host time and not against the handler deadline of the plugin, because the async `fs` call completes later with its outcome.

Access denied that comes from a missing permission, not from an open handle, cannot succeed on retry. The host cannot tell the two apart at the first failure, so it retries both for the same 2 seconds. After the 2 seconds it reports `not-permitted` when the current user has no write access to the target, and `io` with the `file-in-use:` message in every other case.

- **[PRM-304]** On Windows the system must retry a rename, replace or delete that fails with a sharing violation or access denied for up to 2 seconds, with a wait that starts at 25 ms and doubles up to 400 ms, before it returns an error. The rule must apply to `fs.rename`, `fs.remove`, replacing writes and `write-atomic`, and to the host's own writes for install, update, uninstall, the session snapshot, `installed.json`, `grants.json`, `consents.json`, the trust store and `config.toml`. Test: on Windows hold an exclusive handle on the target for 1 second, call `fs.rename` and run an install, and assert that both succeed with no error reported.
- **[PRM-305]** When the retry time of [PRM-304] ends without success, the system must keep the previous target file intact, must leave no partial file under the target name and no temporary file beside it, and must report `io` with a message that starts with `file-in-use:` and names the path. Test: on Windows hold an exclusive handle on the target for 5 seconds, call `fs.rename`, write `grants.json` and take a snapshot, and assert the `io` error whose message starts with `file-in-use:` and holds the path, the unchanged bytes of each target and the absence of temporary files.

## What is the same everywhere

- The component model contract `ilium:plugin@0.1.0`, the WIT files, the tier and grant rules.
- The call classes, deadlines and ceilings of [Host scheduling](/pages/reference-host-scheduling/).
- The limit keys of [Errors and limits](/pages/reference-errors-and-limits/).
- The envelope and handshake of [Plugin traffic over IPC](/pages/reference-host-ipc/).

The results of the conformance suite MUST be identical on every shipped platform for T1 plugins. Timing benchmarks are measured per platform, and the gate in [Performance budgets and benchmarks](/pages/reference-host-budgets/) applies to the Linux reference machine only. Other platforms record their numbers.

- **[HST-387]** The conformance suite MUST pass on every shipped platform for T1 and, on Linux, for T2 and T3.
- **[HST-388]** The performance gate MUST apply to the Linux x86_64 reference machine, and other platforms MUST record results without a gate.

## Moving a data directory between operating systems

A user moves to another operating system by taking a whole-host archive with `ilium plugin backup` and restoring it on the new machine ([Backup and restore](/pages/guide-backup-and-restore/), [Plugin files](/pages/reference-plugin-files/)). Copying the data directory by hand is not a supported route, because only the restore procedure applies the rules below. The restore decides everything; no plugin runs during it.


What the restore does, in order:

1. It reads the OS recorded in `backup.json` ([HST-654]) and compares it with the running OS. When they differ, the cross-OS rules apply. When the archive format is unknown or the host is newer, the restore fails first with `restore.format-unsupported` or `restore.host-newer`.
2. It copies plugin data and storage buckets byte for byte. Keys and values are not parsed, re-encoded or re-lined.
3. It rewrites path separators to the new platform's form in `installed.json` and in the settings it restores. Plugin-visible paths stay `/` on every platform (HST-384).
4. It checks each installed T2 and T3 plugin against the platform rules of the [Support matrix](#support-matrix). A plugin whose package targets a platform not available here (for example a Linux-only T2 plugin restored on Windows, which gets `sandbox.unavailable` under HST-380) is marked `refused-at-load` with `package.platform-unsupported`. Its data stays on disk and is included in later backups.
5. The restore result lists every refused plugin with its id and error code. The user sees the same state in the plugin list.

A refused plugin comes back when the user installs a build for the new platform: the data is still there and loads under the plugin's normal `data_version` rules.

### What a plugin author must do for data that crosses operating systems

The host does not translate plugin data, so the plugin must write data that means the same on every platform:

- Store paths as relative paths with `/` separators, relative to a directory the host gives the plugin. Never store an absolute path, a drive letter or a home directory.
- Never put OS-specific line endings (`\r\n`) or case-folded assumptions into keys. Keys that differ only by case collide on case-insensitive file systems (see the differences table).
- Treat stored bytes as the plugin's own format. Start restore bytes with a format byte and fall back to defaults when it is unknown (AUT-206, [Compatibility and upgrades](/pages/guide-compatibility-and-upgrades/)).

### The trust store does not move


A cross-OS restore therefore always needs the user to re-establish trust, even for plugins whose platform is supported; their data is intact but T2 and T3 plugins stay refused until then.

| Situation after a cross-OS restore | State | Data |
|---|---|---|
| Plugin package targets the new platform, publisher re-trusted | Loads normally | Kept, unchanged |
| Plugin package does not target the new platform | `refused-at-load`, `package.platform-unsupported` | Kept, unchanged |
| Publisher not yet re-trusted | Refused, `trust.store-tampered` | Kept, unchanged |
| Archive from an unknown format or a newer host | Restore fails, nothing written | Not applicable |

## Errors

| Id | When | Effect |
|---|---|---|
| `sandbox.unavailable` | Platform or prerequisite missing for T2 or T3 | The plugin shows `Cannot start: sandbox unavailable` with the platform |
| `package.platform-unsupported` | Installed package does not target this platform (restore on another OS, removed platform) | `refused-at-load`; data kept |
| `host.cwasm-incompatible` | No `.cwasm` for the triple or fingerprint | Compiled at load; start is slower |
| `io` with a `file-in-use:` message | Windows: another process held the target open for the whole 2 second retry time ([PRM-305]) | The operation fails; the previous file is intact |
| `data.root-unresolved` | No home directory and no variable that names the roots ([PRM-314]) | The server and the client refuse to start and name the variables to set |

## Running in a container

The support matrix lists a Linux container as Linux. A container changes four facts that a desktop login hides: the server can be process 1, standard input can be something other than a terminal, the root file system can be read-only, and the user id can have no entry in `/etc/passwd` and no home directory. The rules below make each of them a defined case.

| Fact | Behaviour |
|---|---|
| The server is PID 1 | It reaps every orphaned child that the kernel hands to it, so no zombie process stays. `SIGTERM` starts the graceful stop of [PRS-002](/pages/lifecycle/) with the same budgets, and the server exits within 10 seconds |
| No TTY on standard input | The server starts and serves clients as usual; it never reads standard input and never needs a terminal. Commands that do not draw, such as `ilium plugin ...`, work without a terminal ([Plugin CLI](/pages/reference-plugin-cli/)) |
| Read-only root file system with a writable data volume | The roots are taken from `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_STATE_HOME`, `XDG_CACHE_HOME` and `XDG_RUNTIME_DIR`, or from `ILIUM_CONFIG_DIR` and `plugins.data_dir`. Roots on the volume are used normally. A root that is not writable follows the read-only mode of [Offline and low disk](/pages/reference-offline-and-low-disk/) (HST-664): installed plugins run and writes fail with `io` |
| Arbitrary uid with no passwd entry, or `HOME` unset or not writable | The host does not look up the passwd entry when the variables above name the roots. It starts with those roots. The runtime directory follows the fallback of [Runtime directory and socket path](/pages/reference-kernel-and-boot/#runtime-directory-and-socket-path) |
| No home directory and no variable that names a root | The server and the client refuse to start with `data.root-unresolved`, and the message lists the variables that would fix it. The host never writes into the current directory or `/` as a guess |

`data.root-unresolved` means that the host could not decide where a root is. It differs from `data.root-invalid` (HST-669), which means that a configured plugin data root exists but fails the owner and mode check, and from the read-only mode, which means that a root is known and cannot be written.

- **[PRM-313]** When the server runs as PID 1 it must reap orphaned child processes so that none stays a zombie for more than 1 second, and it must treat `SIGTERM` as the graceful stop of [PRS-002] and exit within 10 seconds. Test: in a container run the server as PID 1, make a pane's child exit after its parent, assert that no zombie remains after 1 second, send `SIGTERM` and assert a clean exit within 10 seconds with acknowledged storage writes on disk.
- **[PRM-314]** When `HOME` is unset, is not writable or the uid has no passwd entry, the system must start with the roots named by the `XDG_*` variables, `ILIUM_CONFIG_DIR` and `plugins.data_dir`, must use the read-only mode of HST-664 for a root that cannot be written, and must refuse to start with `data.root-unresolved`, naming the variables to set, only when a root can be resolved from neither a home directory nor a variable. Test: start the server in a container with `--read-only`, no TTY, an arbitrary uid and the `XDG_*` variables pointing at a volume, and assert that it starts and a client attaches; unset the variables and assert `data.root-unresolved`.
- **[PRM-315]** `ilium plugin doctor` must print whether a container was detected, whether the server is PID 1, the uid and whether it has a passwd entry, whether standard input is a TTY, and each resolved root with its writable state. Test: run `ilium plugin doctor` in the container of [PRM-314] and compare the five facts with the container's configuration.

## Edge cases

| Situation | Result |
|---|---|
| A user moves a data directory between operating systems | A whole-host restore rewrites no plugin data bytes and refuses, at load, plugins whose package does not target the new platform, keeping their data (HST-655). See [Moving a data directory between operating systems](#moving-a-data-directory-between-operating-systems) |
| A T1 plugin on Windows uses `process.run` | Allowed with grant; the child sits in the job object |
| A Linux host without user namespaces | T1 works; T2 and T3 get `sandbox.unavailable` with the sysctl |
| Running under Rosetta on macOS | Not supported; the host reports the triple it was built for |

## See also

- [Host architecture](/pages/reference-host-architecture/) and [Packages](/pages/packages/).

API version checked: `ilium:plugin@0.1.0`.

## Production additions (round 11)

- **[PRQ-003]** The system must detect missing mouse, enhanced-keyboard and Unicode-width support at client start, keep every core plugin action reachable by keyboard without a mouse, switch to the stable-glyph set automatically only when the terminal cannot draw the icon set with one notice naming the reason, and start without error under TERM=dumb or tmux.
