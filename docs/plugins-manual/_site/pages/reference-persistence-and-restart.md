# Reference, persistence and restart

## Reference, persistence and restart

This page answers one question for every kind of plugin state: what is still there after a given event? It lists the events, the kinds of state, the restore bytes of the client, the node metadata, and the order of plugin start and stop against the session snapshot.

Contract version: `ilium:plugin@0.1.0`. The state names are in [Plugin lifecycle states](/pages/reference-plugin-lifecycle-states/).

### The survival matrix

Y means the state is kept. N means it is lost. R means the plugin gets it back only if it asks (restore bytes). S means kept only for the open session.

| State | Plugin restart (after a failure) | Disable then enable | Update | Server restart | Client reconnect | Session delete | Uninstall (no flags) | Uninstall with `--delete-data` |
|---|---|---|---|---|---|---|---|---|
| Linear memory and variables | N | N | N | N | N | N | N | N |
| Restore bytes (server) | N | N | Y | N | n/a | N | N | N |
| Restore bytes (client, disconnect) | n/a | n/a | Y | n/a | Y | N | N | N |
| Global storage bucket | Y | Y | Y (migrated) | Y | Y | Y | Y | N |
| Project storage bucket | Y | Y | Y (migrated) | Y | Y | Y | Y | N |
| Session storage bucket | Y | Y | Y | S | Y | N | N | N |
| `data` and `cache` folders | Y | Y | Y | Y | Y | Y | data Y, cache N | N |
| Settings values | Y | Y | Y | Y | Y | Y | Y | Y |
| Grants | Y | Y | Y (new grants asked) | Y | Y | Y | N | N |
| Node metadata | Y | Y | Y | Y | Y | N | Y | Y |
| Durable timers and journal | Y | N | Y | Y | Y | N | N | N |
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

A plugin stores metadata on tree nodes. After an activation, the plugin reads it through the tree API. The activation call does not carry it. Metadata is saved in the session snapshot with the node and lives as long as the node. A plugin that is uninstalled leaves its metadata in place; `--delete-data` does not remove it either, because it belongs to the node. Reinstalling the plugin finds it again.

- [LIF-155] Node metadata MUST survive a plugin restart, an update, a server restart and an uninstall, and MUST be removed with its node. Test: set metadata, run each event, and read it.

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
| The snapshot file is corrupt at start | The kernel loads the last good backup and logs it. Plugin state in storage is not affected. |
| Two servers share a global bucket | See the lock rule in [Data migration](/pages/guide-data-migration/). |
| A plugin writes during `deactivate` | The write is kept if it finishes within the deactivate deadline. |

### See also

- [Plugin lifecycle states](/pages/reference-plugin-lifecycle-states/)
- [Lifecycle](/pages/lifecycle/)
- [Storage](/pages/api-storage/)
- [Plugin files on disk](/pages/reference-plugin-files/)
- [Requirements, lifecycle and operations](/pages/reference-requirements-lifecycle-ops/)

API version checked: `ilium:plugin@0.1.0`.
