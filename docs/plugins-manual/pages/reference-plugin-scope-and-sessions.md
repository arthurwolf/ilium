---
layout: page
title: Reference, plugin scope and sessions
permalink: /pages/reference-plugin-scope-and-sessions/
---

## Reference, plugin scope and sessions

A user may run several Ilium sessions, and may open projects with different needs. This page defines where a plugin is installed, where it is enabled, how a change reaches the other sessions, and which failure state is shared.

Contract version: `ilium:plugin@0.1.0`.

### Install scope

In 0.1.0 a plugin installs globally, for the user. There is no project-scope install. A package in a project folder could run code the moment a repository is opened, so the platform leaves it out until project trust exists. The shared package store, `installed.json`, grants and the global storage bucket serve every session of the user.

- [LIF-160] The host MUST accept installs only into the user scope in 0.1.0, and MUST refuse `--scope project` for install with `install.scope-unsupported`. Test: run the command.

### Enable scope

A plugin is enabled or disabled at three levels. A lower level overrides a higher one.

| Order | Level | Where stored | Set by |
|---|---|---|---|
| 1 | global | `installed.json` | `ilium plugin enable <id>` |
| 2 | project | `<project>/.ilium/plugins.toml` | `ilium plugin enable <id> --scope project` |
| 3 | session | session state | the Plugins screen toggle with "this session only" |

The effective value is the session value if set, else the project value if set, else the global value. A plugin disabled by policy or by safe mode stays off whatever the levels say.

The WIT call is `set-enabled(id, enabled, scope)` with `scope` one of `global`, `project`, `session`.

- [LIF-161] The effective enabled value MUST follow the order session, project, global. Test: set the three levels in all eight combinations.
- [LIF-162] `plugins.toml` MUST be read as a request only. It MUST NOT install, grant or raise a tier. Test: list a package that is not installed.

### Propagation to other sessions

A global change reaches running sessions within 2 seconds. Each session server watches `installed.json`. A session that is mid-call finishes the call, then applies the change. The CLI command fans out: it prints one JSONL line per reachable session with its result. An unreachable session gets the change at its next start.

```text
{"type":"session-result","session":"default","project":"/home/u/app","ok":true}
{"type":"session-result","session":"review","project":"/home/u/lib","ok":true}
```

- [LIF-163] A global enable, disable or uninstall MUST reach every running session of the user within 2 seconds. Test: two fake sessions and a changed file.
- [LIF-164] The CLI MUST print one `session-result` line per session and exit 0 only when all succeeded. Test: make one session unreachable.

A session is **stale** for a global change from the moment the change is written to `installed.json` until the session applies it. How long that lasts depends on the session's state:

- Running and watching `installed.json`: at most 2 seconds ([LIF-163]). Without a file watcher, at most 6 seconds ([LIF-089]).
- Running and inside a call: until the call returns. Every call has a deadline, so this window is bounded by that deadline.
- Not running (crashed, stopped, or never started since the change): until its next start. The session then reads `installed.json` as it finds it ([LIF-090]), so the stale window lasts as long as the session is down.

A stale session keeps its old state. An enabled plugin keeps running there and an uninstalled plugin stays loaded there, until the session applies the change. A plugin whose signing key was revoked is refused at the next load in any session that starts after the change ([lifecycle states](/pages/reference-plugin-lifecycle-states/)).

**How the CLI reports a stale session.** For `enable`, `disable` and `uninstall`, a session that does not confirm the change within the 2 seconds of LIF-163 gets a `session-result` line with `"ok":false` and the code `session.unreachable`. A session that confirms but fails to apply the change gets `"ok":false` with the code of its error. The command then exits 1 ([LIF-164]). For `install` and `update`, the same missed notification is only a `warning` line with `install.notify-failed` and does not change the exit code ([install pipeline, step 7](/pages/reference-install-update-uninstall/#the-install-pipeline)).

```text
{"type":"session-result","session":"review","project":"/home/u/lib","ok":false,"code":"session.unreachable"}
```

**Finding a stale session later.** Version 0.1.0 keeps no persistent stale marker and no per-session status list for global changes. The only record of which sessions missed a change is the output of the command that made the change, so keep that output. To check a running session afterwards, run `ilium --cwd <project> plugin list`. It prints the plugin set that this session has loaded (DEV-165 in [CLI reference](/pages/reference-plugin-cli/)), so a session that applied the change shows the new state. A session that is not running has no loaded set to show; check it after its next start.

**Security changes.** After a `disable` or `uninstall` that is meant to stop a plugin everywhere, every `session-result` line MUST show `"ok":true` and the command MUST exit 0. For each line with `"ok":false`, the plugin still runs in that session. Stop that session with `ilium --cwd <project> kill-session <session>` and then check it again after its next start. Its restarted instance reads the new state at start, so the plugin does not come back.

### Locks

Writes to `installed.json` take the lock `<data>/plugins/installed.lock`. A second writer polls every 100 ms for up to 30 seconds, then fails with `install.locked` (exit code 4). An update that is in progress holds a per-plugin lock, so two sessions never run the same trial.

`installed.lock` is an OS advisory lock (flock on Unix, LockFileEx on Windows, [LIF-066](/pages/reference-install-update-uninstall/)). The kernel releases it when the holder process exits, including after a crash, a kill or a reboot. A crashed holder therefore never blocks later writers, and the file itself stays on disk with no state to clean up. The per-plugin update lock is a different lock; its takeover rule is in [Concurrent install, upgrade and uninstall](/pages/reference-install-update-uninstall/#concurrent-install-upgrade-and-uninstall).

To tell whether a holder is alive, read the process ID and boot ID that the lock file records as text. The holder is live only when the boot ID equals the current `/proc/sys/kernel/random/boot_id` and the process ID belongs to a running `ilium` process. Otherwise the holder is gone and the lock is free. The process ID is a hint: a reused number on another boot is not a holder.

When `install.locked` (exit code 4) is printed, the lock was held by a live process for the whole 30 seconds. Run the command again once the holder has finished. If the holder is a hung process, send it SIGTERM by its recorded process ID, and SIGKILL only if it is still running after that. Stopping a session server with `ilium --cwd <project> kill-session <session>` is the same recovery at session level.

- Do not delete `installed.lock` to clear the error. Deleting the path does not release a lock that a live holder holds on the old file. The next writer then creates a new file and locks that one, so two writers could change `installed.json` at once.
- `ilium plugin doctor` has no lock item in 0.1.0 ([DEV-810](/pages/reference-plugin-cli/#trace-profile-inspect-and-doctor)). For a bug report about a lock that stays held after its holder is gone, `ilium plugin doctor --bundle` records the lock file hash ([DEV-941](/pages/reference-plugin-cli/#trace-profile-inspect-and-doctor)).

### Failure scope

The failure window is per session, per plugin side. A plugin that fails three times in one session is disabled in that session only, and a note is written to the shared registry so that other sessions show the warning. It is not disabled elsewhere.

- [LIF-165] A failure disable MUST apply to the session where it happened and MUST NOT disable the plugin in other sessions. Test: fail a plugin in session A and call it in B.

### Grants and the "Not now" counter

Grants are global and are stored in `$XDG_STATE_HOME/ilium/grants.json`. The counter of "Not now" answers for a bundled plugin's first-start prompt is stored with the grants, so the prompt does not repeat in each session.

- [LIF-166] A grant change MUST apply to every session and the "Not now" counter MUST be shared. Test: answer in one session and open another.

### See also

- [Install, update and uninstall](/pages/reference-install-update-uninstall/)
- [Plugin files on disk](/pages/reference-plugin-files/)
- [Lifecycle](/pages/lifecycle/)
- [Requirements, lifecycle and operations](/pages/reference-requirements-lifecycle-ops/)

API version checked: `ilium:plugin@0.1.0`.
