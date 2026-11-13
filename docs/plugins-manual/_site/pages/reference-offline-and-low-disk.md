# Reference, offline and low disk

## Reference, offline and low disk

Plugins must keep working without a network, and must fail clearly when the disk is full or read-only. This page lists what needs a network, what each disk problem does, and how the host deals with a damaged file.

Contract version: `ilium:plugin@0.1.0`.

### Offline

Installed plugins run with no network. Ilium itself needs no network for plugins.

| Action | Needs network |
|---|---|
| Run an installed plugin | no |
| Install from a local `.iliumpkg` file | no |
| Verify a signature (`ilium plugin verify`) | no |
| Update check, install from a source | yes |
| A plugin that calls `http` | yes, and it gets `network` or `timed-out` |

- [LIF-170] The host MUST load, activate and run installed plugins with no network. Test: block the network in the fixture and start all fixtures.
- [LIF-171] Install of a local file MUST NOT contact the network. Test: install with the network blocked.

### Low disk and disk full

The host checks free space on the data volume before an install or an update. It needs the package size times 3, plus 64 MiB. A separate data check runs before `migrate`: twice the data size plus 100 MiB (Data migration). If less is free, the action stops before any write with `install.no-space`.

| Event | Result |
|---|---|
| Disk full during install | The staging folder is removed. Nothing is registered. `install.io`. |
| Disk full during update | The trial is abandoned. The old version stays. |
| Disk full when a plugin writes | The write returns `io` with the message `no-space`, never `quota-exceeded` ([LIF-119]). Storage writes in a batch roll back. |
| Disk full when the host writes a log | Lines are dropped. A count is kept. |
| Disk full when the snapshot is written | Previous snapshot kept. See [Persistence](/pages/reference-persistence-and-restart/). |
| Free space below 64 MiB while running | The host shows one warning and blocks installs and updates. Plugins keep running. |

- [LIF-172] The host MUST check free space before install and update, and MUST leave no partial files when it refuses. Test: a small fake volume.
- [LIF-173] A disk-full error MUST NOT count as a plugin failure. Test: fill the disk and write.

### Read-only data directory

If the data folder is not writable, the host starts in a read-only mode. Installed plugins run. Install, update, uninstall, enable and disable fail with `io`. Storage writes fail with `io`. The footer shows "Plugin data is read-only."

- [LIF-174] A read-only data folder MUST NOT stop the server or the running plugins. Test: mark the folder read-only.

### Corruption

| Damaged item | Handling |
|---|---|
| `installed.json` | Core plugins only, recovery page opens, file not overwritten |
| A package folder | The plugin is `refused-at-load` with `package.corrupt` |
| `grants.json` | Renamed to `grants.json.corrupt-<YYYYMMDD-HHmmss>`, logged `grants.unreadable`; T2 and T3 plugins start as not installed with no grants |
| A storage database | The file is renamed `<id>.db.corrupt-<time>`, an empty bucket is created, the plugin gets a notice and the log has one error |
| A compile cache entry | Deleted and rebuilt |

- [LIF-175] A corrupt storage database MUST be moved aside and replaced with an empty one, never deleted. Test: write garbage into the file.

### The compile cache

T2 and T3 components are compiled ahead of use and cached in `<cache dir>/ilium/plugins/compile/`. The key is the component hash, the Wasmtime version and the CPU features. A change of any part makes a new entry. The cache may be removed at any time and is rebuilt on demand. It is not needed to run, only to start fast.

- [LIF-176] The compile cache key MUST include the component hash, the Wasmtime version and the CPU features, and a missing cache MUST NOT stop a plugin. Test: delete the cache and start.

### See also

- [Plugin files on disk](/pages/reference-plugin-files/)
- [Install, update and uninstall](/pages/reference-install-update-uninstall/)
- [Resource accounting](/pages/reference-resource-accounting/)
- [Requirements, lifecycle and operations](/pages/reference-requirements-lifecycle-ops/)

API version checked: `ilium:plugin@0.1.0`.
