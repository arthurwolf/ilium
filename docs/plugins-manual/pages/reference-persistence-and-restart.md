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
- **[LIF-188]** When the connection to the server drops without a shutdown message, the client MUST deactivate every client and scene instance with the reason `shutdown`, every pending request of those instances MUST complete as `cancelled`, and the restore bytes they return MUST be sent to the server of the next successful attach, where they are stored under LIF-153 (proposed, owner to confirm).

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

- **[LIF-196]** When the session snapshot is corrupt at start, the host MUST first try the previous snapshot kept under HST-691. If it is readable, the session MUST restore from it and the host MUST log `snapshot.restored-previous` with both file names. Only when no readable previous snapshot exists MUST the session start with one empty shell pane (proposed, owner to confirm).

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

### Client suspend and resume

A shell or a terminal multiplexer can stop the client with job control (`Ctrl+Z`, which sends `SIGTSTP`) and continue it later with `SIGCONT`. The user is then back at the shell prompt in the same terminal, so the terminal must be in its normal state while the client is stopped. This applies to Linux and macOS. Windows has no job-control stop; minimising a ConPTY window sends no signal and changes nothing.

| Moment | What the client does |
|---|---|
| `SIGTSTP` arrives | It runs steps 1 to 4 of the terminal restore routine above (mouse reporting off, alternate screen left, raw mode left and attributes restored, cursor shown), and only then stops itself. It writes no crash record, and the connection to the server stays open |
| While stopped | Nothing runs on the client. The server sees a client that does not read, so the rules of [Slow and stalled clients](/pages/reference-host-ipc/#slow-and-stalled-clients) apply: frames are coalesced, and after 30 seconds with queued data the server disconnects it with `ipc.client-too-slow` |
| `SIGCONT` arrives | It enters raw mode, the alternate screen and the mouse modes again, reads the current terminal size, and paints the whole screen within 200 ms. If the server disconnected it meanwhile, it attaches again by itself and paints from the snapshot; the 200 ms then start when the snapshot arrives |
| Size changes around the resume | Any number of `SIGWINCH` signals that arrive within one frame interval cause one size query and one layout, with the last size |

The stopped interval is not plugin time. Client-side deadlines, timers and the failure window run on the monotonic clock (LIF-317), and the client subtracts the interval between `SIGTSTP` and `SIGCONT` from every running client-side deadline, so no plugin is reported late or counted as failed because the user suspended the client. A client timer that came due while stopped fires once after the resume.

- **[PRM-311]** When the client receives `SIGTSTP` it must restore the terminal (mouse reporting off, alternate screen left, raw mode left with the saved attributes, cursor shown) before it stops, and on `SIGCONT` it must enter those modes again, read the current terminal size and repaint the whole screen within 200 ms, applying a burst of `SIGWINCH` signals as one resize. Test: run the real client under a PTY, record the screen, send `SIGTSTP`, assert the reset sequences and cooked-mode attributes, resize the PTY three times, send `SIGCONT`, and assert within 200 ms a full repaint at the last size whose content equals the recorded screen laid out for that size.
- **[PRM-312]** The client must not report a client-side plugin deadline breach, and must not add to a plugin's failure count, for the interval in which the client was stopped, and a client that the server disconnected while it was stopped must attach again by itself on `SIGCONT`. Test: stop the client for 40 seconds while a pane prints, continue it, and assert that it shows the current screen, that no deadline breach was logged and that every plugin's failure count is unchanged.

### Limits on attached clients and sessions

Several clients can attach to one session, and one user can run several sessions. Both numbers have a tested limit, so that the capacity and the failure are known.

| Item | Value | When it is reached |
|---|---|---|
| Clients attached to one session at the same time | 16 | The 17th attach is refused with `attach.client-limit`. The new client prints `This session already has 16 attached clients. Detach one and try again.` and exits with a non-zero exit code. The 16 attached clients and the server are not affected |
| Concurrent sessions of one user | 32 supported | The limit is a tested capacity, not a refusal. Each session is its own server process and always starts. Plugin memory and threads across all sessions stay under the ceilings of [Host aggregate limits](/pages/reference-host-aggregate-limits/); a plugin whose reservation those ceilings refuse is handled as that page says, and the session itself still runs |
| Server memory added by one attached client | At most 48 MiB | 32 MiB for the outbound queues ([PRM-301] in [Slow and stalled clients](/pages/reference-host-ipc/#slow-and-stalled-clients)) plus 16 MiB for the client host bookkeeping, outcome records and replicas |
| Client restore bytes | 1 MiB per plugin, last writer wins (LIF-152, LIF-153) | They are stored per plugin id, not per client, so more clients do not add stored bytes |

A client that detaches or is disconnected frees its slot and its queues at once. Its outcome records are kept for 30 seconds as HST-116 says and are counted in the 16 MiB of bookkeeping. A client disconnected with `ipc.client-too-slow` does not hold a slot while it is away.

- **[PRM-308]** The system must accept 16 simultaneously attached clients on one session and must refuse the 17th attach with `attach.client-limit`, a message that names the limit and a non-zero exit code, without disturbing the attached clients. Test: attach 16 clients, assert that all draw, attach a 17th and assert the refusal, then detach one and assert that the next attach succeeds.
- **[PRM-309]** The system must support 32 concurrent sessions of one user on the reference machine, each with its own server, socket and snapshot. Test: with the default aggregate ceilings start 32 sessions in 32 project folders, attach one client to each, type into one pane of each and assert that every session echoes the input and writes its own snapshot.
- **[PRM-310]** Each additional attached client must add at most 48 MiB of server memory, and with 16 clients attached to a 100-pane session the 99th percentile time from pane output to the frame received by the first client must stay under 50 ms and within 1.5 times the value measured with that client alone. Test: measure the frame time and the server memory with 1 client and with 16 clients on a 100-pane session and compare both with the limits.

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

## Requirement statements

- **[COR-100]** The server MUST take an exclusive lock on the snapshot, and each config file and plugin data directory it writes, before writing, and a second server that cannot get the session lock MUST refuse to start a second writer for the same session and report the owner.
- **[COR-101]** When two servers use different sessions of the same project, writes to shared config files and plugin data directories MUST be serialized by the lock and MUST use last-writer-wins per file with an atomic replace, never a partial write.
- **[COR-102]** A server that loses the lock, or finds a lock held by a dead process, MUST stop writing, MUST take over a stale lock only after verifying that the owner process no longer exists, and MUST log the decision.
- **[HST-674]** The host MUST apply the bucket quota separately to every project and every session bucket, MUST bound the number of project buckets per plugin to 1,000 (proposed, owner to confirm) and session buckets to 1,000, and an attempt to create the 1,001st MUST return `quota-exceeded` with the message `bucket-count`. Closing a session MUST NOT delete its bucket; `ilium plugin data prune --older-than <days>` MUST remove session buckets unused for that long.
- **[HST-692]** On snapshot restore, state of a plugin that is not installed, disabled, or has a lower data version than the snapshot MUST be kept unmodified in the snapshot file, the pane or view MUST show a placeholder naming the plugin id, and installing or enabling the plugin MUST bring the state back without a new snapshot. State MUST be discarded only by `ilium session forget-plugin-state <id>`.
- **[LIF-150]** The host MUST keep and lose state as the matrix says.
- **[LIF-151]** Pending requests MUST complete as `cancelled` at every event in the first four columns and MUST NOT be replayed.
- **[LIF-152]** Restore bytes over 1 MiB MUST be dropped, and `activation.restored` MUST be absent.
- **[LIF-153]** Client restore bytes MUST be stored per plugin id, last writer wins, and MUST be passed to the next client activation of that plugin.
- **[LIF-154]** A failed snapshot write MUST NOT stop the server. The host MUST log `snapshot.write-failed`, keep the previous snapshot file, and retry at the next snapshot.
- **[LIF-155]** Node metadata MUST survive a plugin restart, an update, a server restart and an uninstall, and MUST be removed with its node.
- **[LIF-156]** The plugin host MUST start after `restore_snapshot` and MUST stop before `drain_session_work`.
- **[LIF-157]** A crash of the server (no shutdown) MUST lose only the memory state and the server restore bytes. Storage, settings, data and metadata MUST be as of the last durable write or snapshot.
- **[LIF-317]** Durable timers, update deadlines and the failure window MUST run on the monotonic clock. A wall-clock jump or a suspend and resume MUST NOT fire a timer early, and a timer that came due during a suspend MUST fire once after resume, not once per missed period.
- **[LIF-318]** Backup age pruning MUST use the age recorded as monotonic uptime plus the boot id, and MUST fall back to wall time only when the boot id changed. A backward wall-clock jump MUST NOT make the host prune a backup younger than its retention.
