---
layout: page
title: Host upgrade and restart while plugins run
permalink: /pages/host-upgrade-while-plugins-run/
---

## Host upgrade and restart while plugins run

Ilium can stop and start while your plugin has running instances. That happens when the user restarts Ilium, when the session server stops, after a crash, or after a reboot. This page says which of your instances keep running, which are stopped and why, what happens to calls that are still in flight, what survives, and what the operator does. It does not describe a plugin update. For that, see [Compatibility and upgrades](/pages/guide-compatibility-and-upgrades/) and [Install, update and uninstall](/pages/reference-install-update-uninstall/).

### Short answer

| Event | Running instances | Your code sees | What survives |
|---|---|---|---|
| Clean server stop | Stopped | `deactivate(shutdown)` once per live instance, in reverse start order | Storage, settings, grants, node metadata, durable timers. Restore bytes are kept only in the session snapshot for client sides |
| Server crash or kill | Stopped without a call | No `deactivate` | Storage, settings and metadata as of the last durable write or snapshot. Server restore bytes are lost |
| Client process killed | The client instance is gone | No `deactivate` | The last restore bytes the server stored for that client side |
| Host shutdown takes longer than 5 s | Remaining plugins dropped | No `deactivate` for the dropped plugins | As for a crash, for those plugins |
| Plugin restart after a counted failure | Restarted after 1 s, then 5 s | `deactivate` is not sent after a trap | Storage and settings. Linear memory is lost |

Nothing runs after a restart until the host activates the plugin again. The host starts after the kernel has restored the session snapshot, and it activates plugins on their triggers. The exact activation order is in [Lifecycle](/pages/lifecycle/).

### Keep running or stopped

- All instances stop when the server stops. Linear memory and variables are lost in every case. This follows from the rule that an instance lives from `activate` until `deactivate`, a trap or a restart. See [Runtime model](/pages/reference-runtime-model/).
- A plugin that was enabled stays enabled. Its package state is stored on disk, and it survives a server restart and a reboot. See [Plugin lifecycle states](/pages/reference-plugin-lifecycle-states/).
- A plugin side that was stopped by failure stays stopped. The server side is stored in `installed.json` as `failure_stopped`, so it survives a server restart. A client or scene side is held only in memory and is dropped when its client detaches. See [Plugin lifecycle states](/pages/reference-plugin-lifecycle-states/).
- A restart timer that was scheduled is cancelled when the server stops. It is not carried over to the next start. See [Plugin lifecycle states](/pages/reference-plugin-lifecycle-states/).
- The failure metrics reset when the server restarts. See [Health and watchdog](/pages/reference-health-and-watchdog/).

### Deactivate reason and timeout

| Case | Reason sent | Deadline | Bytes kept |
|---|---|---|---|
| Server stop begins | `shutdown` | 500 ms by default, 1,000 ms ceiling for a T1 plugin | Yes, for the next activation in the same server process only. A server restart drops them |
| Client detaches cleanly | `shutdown` | 500 ms by default | Yes, sent to the server and stored in the session snapshot |
| Client process killed or crashed | None sent | None | The last bytes the server stored |
| Plugin update or dev reload | `upgrade` | 500 ms by default | Yes |

A `deactivate` that misses its deadline drops the instance and discards its bytes. Its log line is `deactivate.deadline`. The plugin is not counted as failed. Restore bytes are limited to 1 MiB. A larger return is dropped, one warning line is logged, and the next activation gets no restored value.

The host gives the whole server shutdown 5 s. If it takes longer, the remaining plugins are dropped without `deactivate`, and the error `host.shutdown-timeout` is logged. See [Errors and limits](/pages/reference-errors-and-limits/).

### In-flight calls

- A call that is pending when its instance is deactivated or dropped completes as `cancelled`. The host does not call into the instance for it. See [Errors and limits](/pages/reference-errors-and-limits/).
- A pending request is never saved. Pending requests are lost at every event in the persistence table, including a server restart.
- `cancelled` is listed as a case where the call may or may not have taken effect. Retry only where the operation is safe to repeat. See [Errors and limits](/pages/reference-errors-and-limits/).
- A write that finishes during `deactivate` is kept only if it finishes within the deactivate deadline. See [Persistence and restart](/pages/reference-persistence-and-restart/).

### State that survives

| State | Server restart | Crash | Notes |
|---|---|---|---|
| Linear memory and variables | Lost | Lost | Never saved |
| Restore bytes (server side) | Lost | Lost | Kept in memory only |
| Restore bytes (client side) | Not applicable, the client reconnects and keeps them | Last stored bytes kept | Stored in the session snapshot |
| Global storage bucket | Kept | As of last durable write | |
| Project storage bucket | Kept | As of last durable write | |
| Session storage bucket | Kept only for the open session | As of last durable write | |
| Settings, grants, node metadata | Kept | As of last durable write | |
| Durable timers and journal | Kept | As of last durable write | |

The table is the same as the one in [Persistence and restart](/pages/reference-persistence-and-restart/). Use it as the reference. Write your restore bytes only for state that cannot be stored in storage, because they do not survive a crash.

### Your code after a restart

- Do not assume that a restored value is there. Restore bytes reach `activate` only when the stored `data_version` matches the build that wrote them. Check the first byte and fall back to defaults when it is not a format you know. See [Compatibility and upgrades](/pages/guide-compatibility-and-upgrades/).
- Treat storage as the source of truth. Write each change to storage as it happens, not only in `deactivate`.
- Make `activate` idempotent. It can run again after any restart, after a restart timer, or after a crash.
- Do not rely on a deactivate reason to detect that Ilium restarted. `shutdown` is also used for a client detach.

### Operator steps

Use these steps after a host upgrade or a restart when a plugin does not behave as expected.

1. Run `ilium plugin list --json` to read the package state and the side states of each plugin. The state is `installed-enabled`, `stopped-by-failure` or another value from [Plugin lifecycle states](/pages/reference-plugin-lifecycle-states/).
2. Read the log with `ilium plugin logs <id> --tail 100`. Look for `deactivate.deadline`, `host.shutdown-timeout`, `restart.scheduled` and `update.rolled-back`.
3. If a plugin is `stopped-by-failure`, fix the cause and enable it again. Enabling clears its failure window. See [Lifecycle](/pages/lifecycle/).
4. For a plugin update that went wrong, use `ilium plugin rollback <id>`. Rollback follows the data rules of [Install, update and uninstall](/pages/reference-install-update-uninstall/), and `--restore-backup` or `--discard-data` may be needed.

### Open decisions

These points are not established by the manual's current sources. Each one needs a decision before a requirement can be written for it.

- **Requirement ids.** The ids `HST-300` to `HST-305` are already used by [Performance budgets and benchmarks](/pages/reference-host-budgets/), and the register [Requirements: kernel and host](/pages/reference-requirements-kernel-host/) lists them. The file `reference-requirements-host-platform.md` named for this page does not exist. Choice: assign a new unused range for host restart requirements in the kernel and host register. Consequence: until then this page has no requirement ids, and its rules are not tested.
- **Ilium binary upgrade.** The sources describe a plugin update and a dev reload. They do not describe what happens to running plugins when the Ilium binary itself is replaced. Choice: document the swap as a host restart, which stops every instance with `shutdown`, or document a separate path. Consequence: operators need to know whether plugins keep running on the old binary until the next restart.
- **Who restarts Ilium.** The manual does not say which command restarts the server, or whether a restart is done by the user alone. Choice: name the supported restart command in the operator steps. Consequence: the steps above cannot name one command yet.
- **Failure window across a server restart.** The window is per plugin, side, scope and version ([Lifecycle](/pages/lifecycle/)). The sources do not say whether a window is kept across a server restart or reset. Choice: keep the window in memory and reset it, as the metrics do, or store it like `failure_stopped`. Consequence: a plugin that failed twice before a restart may get the full restart rule again, or may stay one failure from being disabled.
- **Ilium rollback.** `ilium plugin rollback` covers plugin versions only. The sources do not describe rolling back Ilium itself. Choice: state that the plugin rollback does not change the host, or describe a host rollback once it exists. Consequence: operators could assume that rolling back a plugin also undoes a host change.
- **Pending requests on a crash.** The persistence table says pending requests are lost, and the errors table says `cancelled` on shutdown. The sources do not say what a caller sees after a crash, where no `cancelled` is produced. Choice: document the caller's view after a crash as a lost call with an unknown outcome. Consequence: plugins that submit writes must check their result after activation.

### See also

- [Compatibility and upgrades](/pages/guide-compatibility-and-upgrades/) for the author rules of plugin versions and updates
- [Install, update and uninstall](/pages/reference-install-update-uninstall/) for the update pipeline and rollback
- [Lifecycle](/pages/lifecycle/) for activation order and failure counting
- [Plugin lifecycle states](/pages/reference-plugin-lifecycle-states/) for the states and transitions
- [Persistence and restart](/pages/reference-persistence-and-restart/) for the table of what survives
- [Errors and limits](/pages/reference-errors-and-limits/) for `cancelled`, `host.shutdown-timeout` and the restart rule

API version checked: `ilium:plugin@0.1.0`.
