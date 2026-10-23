---
layout: page
title: Reference, persistence and restart
permalink: /pages/reference-persistence-and-restart/
---

## Reference, persistence and restart

This page answers one question for every kind of plugin state: what is still there after a given event? It lists the events, the kinds of state, the restore bytes of the client, the node metadata, and the order of plugin start and stop against the session snapshot.

Contract version: `ilium:plugin@0.1.0`. The state names are in [Plugin lifecycle states](/pages/reference-plugin-lifecycle-states/).

### The survival matrix

Y means the state is kept. N means it is lost. R means the plugin gets it back only if it asks (restore bytes). S means kept only for the open session.

| State | Plugin restart (after a failure) | Disable then enable | Update | Server restart | Client reconnect | Session delete | Uninstall (no flags) | Uninstall with `--purge` |
|---|---|---|---|---|---|---|---|---|
| Linear memory and variables | N | N | N | N | N | N | N | N |
| Restore bytes (server) | N | Y | Y | N | n/a | N | N | N |
| Restore bytes (client, disconnect) | n/a | n/a | Y | n/a | Y | N | N | N |
| Global storage bucket | Y | Y | Y (migrated) | Y | Y | Y | Y | N |
| Project storage bucket | Y | Y | Y (migrated) | Y | Y | Y | Y | N |
| Session storage bucket | Y | Y | Y | S | Y | N | N | N |
| `data` and `cache` folders | Y | Y | Y | Y | Y | Y | data Y, cache N | N |
| Settings values | Y | Y | Y | Y | Y | Y | Y | N |
| Grants | Y | Y | Y (new grants asked) | Y | Y | Y | N | N |
| Node metadata | Y | Y | Y | Y | Y | N | Y | N |
| Durable timers and journal | Y | Y | Y | Y | Y | N | N | N |
| Pending requests | N | N | N | N | N | N | N | N |
| Contributions (views, commands) | stale then replaced | withdrawn | replaced | rebuilt at start | replica resent | gone | withdrawn | withdrawn |
| Failure window | Y | N (reset by enable) | N (reset) | S | Y | N | N | N |

- [LIF-150] The host MUST keep and lose state as the matrix says. Test: one test per column that sets a marker in every row and reads it after the event.
- [LIF-151] Pending requests MUST complete as `cancelled` at every event in the first four columns and MUST NOT be replayed. Test: a pending request across each event.

### Restore bytes

`deactivate` may return bytes. The host hands them to the next `activate` of the same plugin side as `activation.restored`.

| Property | Rule |
|---|---|
| Size | at most 1 MiB; larger bytes are dropped and logged |
| Server side | kept in memory only, for the next activation in the same server process. A server restart drops them |
| Client side | when a client disconnects, the host deactivates the client instance with the reason `shutdown` and the client sends its bytes to the server, which stores them in the session snapshot keyed by plugin id and client side. Last writer wins when two clients detach |
| After an uninstall | dropped |
| After a trap | none exist, because `deactivate` did not run |
| Reasons that keep bytes | All five delivered reasons: `shutdown`, `disabled`, `upgrade`, `revoked`, `granted`. `crash-restart` is never delivered, so it never has bytes |

- [LIF-152] Restore bytes over 1 MiB MUST be dropped, and `activation.restored` MUST be absent. Test: return 1 MiB plus 1 byte.
- [LIF-153] Client restore bytes MUST be stored per plugin id, last writer wins, and MUST be passed to the next client activation of that plugin. Test: detach two clients with different bytes and attach a third.
- [LIF-154] A failed snapshot write MUST NOT stop the server. The host MUST log `snapshot.write-failed`, keep the previous snapshot file, and retry at the next snapshot. Test: make the snapshot folder read-only.

### Node metadata

A plugin stores metadata on tree nodes. After an activation, the plugin reads it through the tree API. The activation call does not carry it. Metadata is saved in the session snapshot with the node and lives as long as the node. A plugin that is uninstalled without `--purge` leaves its metadata in place, and reinstalling the plugin finds it again. With `--purge` the host removes it from the live session and adds a tombstone [LIF-084].

- [LIF-155] Node metadata MUST survive a plugin restart, an update, a server restart and an uninstall without `--purge`, and MUST be removed with its node. Test: set metadata, run each event, and read it.

### Order against the snapshot

Start. The kernel restores the snapshot (`restore_snapshot`) first. The plugin host starts after it and delivers one `tree-changed` batch. Stop. The host deactivates plugins first. Then `drain_session_work` runs and the final snapshot is written. A plugin therefore cannot lose a change by acting during shutdown, and cannot see a half-restored tree.

<figure>
<pre class="term">start: restore_snapshot
       host up -> activate -> tree-changed x1
stop:  deactivate(shutdown) -> drain_session_work
       -> final snapshot
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The order of snapshot and plugin host at start and stop.</figcaption>
</figure>

- [LIF-156] The plugin host MUST start after `restore_snapshot` and MUST stop before `drain_session_work`. Test: log the order in a fake kernel.
- [LIF-157] A crash of the server (no shutdown) MUST lose only the memory state and the server restore bytes. Storage, settings, data and metadata MUST be as of the last durable write or snapshot. Test: kill the fake server between two writes.

### Edge cases

| Situation | What happens |
|---|---|
| The snapshot file is corrupt at start | The file is moved aside as `<name>.broken-<timestamp>`, the session starts with one empty shell pane, and the kernel logs it. Plugin state in storage is not affected. |
| Two servers share a global bucket | See the lock rule in [Data migration](/pages/guide-data-migration/). |
| A plugin writes during `deactivate` | The write is kept if it finishes within the deactivate deadline. |

### Clock changes, suspend and resume

Wall-clock jumps and suspend must not make a timer fire early, lose a deadline or prune a backup that is still young.

- [LIF-317] Durable timers, update deadlines and the failure window MUST run on the monotonic clock. A wall-clock jump or a suspend and resume MUST NOT fire a timer early, and a timer that came due during a suspend MUST fire once after resume, not once per missed period.
- [LIF-318] Backup age pruning MUST use the age recorded as monotonic uptime plus the boot id, and MUST fall back to wall time only when the boot id changed. A backward wall-clock jump MUST NOT make the host prune a backup younger than its retention.

### Two servers on one project

Two servers, or two sessions, can point at the same project directory. They must not corrupt the snapshot, the config files or the plugin data directories.

- [COR-100] The server MUST take an exclusive lock on the snapshot, and each config file and plugin data directory it writes, before writing, and a second server that cannot get the session lock MUST refuse to start a second writer for the same session and report the owner.
- [COR-101] When two servers use different sessions of the same project, writes to shared config files and plugin data directories MUST be serialized by the lock and MUST use last-writer-wins per file with an atomic replace, never a partial write.
- [COR-102] A server that loses the lock, or finds a lock held by a dead process, MUST stop writing, MUST take over a stale lock only after verifying that the owner process no longer exists, and MUST log the decision.

### Client process crash and terminal restore

The TUI client is a separate process from the session server. A crash of the client, whether a panic, a fatal signal that can be caught, or a kill, ends only that client. This section states what the server keeps, what the user's terminal looks like afterwards, and what the next `ilium attach` finds.

| Question | Rule |
|---|---|
| Does the server stop? | No. The server, every server side of every plugin, every helper and every other attached client keep running. The failure count, restart timer and enabled state of each plugin are unchanged [HST-010] [HST-013]. |
| What happens to the crashed client's plugin sides? | The server closes the dead connection and drops that client's client host. Client-side memory is gone. Client restore bytes are not written, because `deactivate` did not run on that client (see [Restore bytes](#restore-bytes)). The bytes stored from the last clean detach of any client stay in the snapshot. |
| Who restores the terminal? | The client, before it exits and before it writes its own crash record. See the order below. |
| How does the user get back in? | `ilium attach` (or a bare `ilium` in the project) connects to the running session. The server sends a full snapshot of the tree and replays pane output as for any attach [Reference, host IPC](/pages/reference-host-ipc/). |
| What does a plugin client side see? | A new client instance is created and activated. `activation.reason` is `reconnected`, and `activation.restored` holds the client restore bytes stored per plugin id (last writer wins [LIF-153]). Contributions (views, commands) are resent as a replica, as the Client reconnect column of the matrix says. |

Terminal restore order. A panic hook and an at-exit guard both run the same idempotent restore routine, so a second call does nothing:

1. Disable mouse reporting (all xterm tracking and encoding modes the client enabled).
2. Leave the alternate screen.
3. Leave raw mode and restore the saved terminal attributes.
4. Show the cursor and reset text attributes.
5. Only then write the crash record and print one line on standard error naming the record file.

If a step fails (for example the terminal is already closed), the routine logs the error code `client.terminal-restore-failed` into the crash record and continues with the next step. It never loops and never blocks the exit for more than 1 second.

The crash record uses the same folder, file mode, content rule and retention as the host crash records: only the plugin id (absent for a crash outside a plugin), the error code, the host version and host frames, redacted before storage [SEC-446]. Retention and deletion follow [Reference, audit log](/pages/reference-audit-log/); the client adds no second period or count. A client crash adds one `crash-report` audit record naming the file.

Failure modes. A kill by `SIGKILL` cannot run any handler, so the terminal is not restored by the client; the user runs `reset`, and the server and plugins are unaffected in the same way. Attaching again while the old connection is still being torn down is allowed: the server accepts the new client at once and the old one is closed in the background.

- [HST-010] A crash of the client MUST NOT stop the server or any plugin side, and MUST NOT change any plugin's failure count, restart timer or enabled state [HST-013]. Test: kill the client under a PTY, then list plugins through a second client and compare with the list taken before.
- [LIF-150] The Client reconnect column MUST hold after a crash exactly as after a clean detach, except that the crashed client's own restore bytes are not stored. Test: set a marker in every row, kill the client, attach again, and read the markers.
- [SEC-446] The client crash record MUST contain only the plugin id, error code, host version and host frames. Test: the redaction test runs over a record written by a forced client panic.

Test for terminal restore: spawn the real client under a PTY (`ilium-pty`), wait for the alternate screen, then force a panic and, in a second run, `SIGTERM` the client. Read the PTY output and assert that it contains the reset sequences for mouse reporting, the alternate screen and the cursor, that the terminal attributes read back from the PTY are the saved cooked-mode attributes, that one crash record exists and passes redaction, and that the server's plugin list is identical before and after.

### See also

- [Plugin lifecycle states](/pages/reference-plugin-lifecycle-states/)
- [Lifecycle](/pages/lifecycle/)
- [Storage](/pages/api-storage/)
- [Plugin files on disk](/pages/reference-plugin-files/)
- [Requirements, lifecycle and operations](/pages/reference-requirements-lifecycle-ops/)

API version checked: `ilium:plugin@0.1.0`.

## Production additions: T10-operations

- [HST-674] The host MUST apply the bucket quota separately to every project and every session bucket, MUST bound the number of project buckets per plugin to 1,000 (proposed, owner to confirm) and session buckets to 1,000, and an attempt to create the 1,001st MUST return `quota-exceeded` with the message `bucket-count`. Closing a session MUST NOT delete its bucket; `ilium plugin data prune --older-than <days>` MUST remove session buckets unused for that long. Test: create 1,000 session buckets and assert the 1,001st returns `quota-exceeded`; close a session and assert its bucket remains.
- [HST-691] The session snapshot MUST cap plugin-written state at 4 MiB (proposed, owner to confirm) in total per plugin and per snapshot (`snapshot_state_kib`, default 256 (proposed, owner to confirm) per plugin), and a plugin state blob above its cap MUST be dropped with `snapshot.state-too-large` and a notice while the snapshot itself is written. The previous snapshot MUST be kept until the new one is verified readable. Test: write a 5 MiB state blob and assert `snapshot.state-too-large` while the snapshot is written; make the new snapshot unreadable and assert the previous snapshot is kept.
- [HST-692] On snapshot restore, state of a plugin that is not installed, disabled, or has a lower data version than the snapshot MUST be kept unmodified in the snapshot file, the pane or view MUST show a placeholder naming the plugin id, and installing or enabling the plugin MUST bring the state back without a new snapshot. State MUST be discarded only by `ilium session forget-plugin-state <id>`. Test: restore a snapshot with a plugin not installed, assert the placeholder and that the state is still in the file; install the plugin and assert the state returns without a new snapshot.
