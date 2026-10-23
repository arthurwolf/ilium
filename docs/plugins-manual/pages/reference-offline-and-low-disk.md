---
layout: page
title: Reference, offline and low disk
permalink: /pages/reference-offline-and-low-disk/
---

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

- [HST-693] With no network, an update check MUST fail within the connect timeout, default 10 s (proposed, owner to confirm), with `network` and MUST NOT block startup, retry no more often than every 15 minutes with exponential backoff to 6 hours, MUST show cached registry metadata for at most 14 days marked `stale`, and MUST NOT show a stale entry as installable if its signature freshness window has passed.
- [HST-694] `ilium plugin verify` and local install MUST succeed offline using the stored trust store and a cached revocation list at most 7 days (proposed, owner to confirm) old (`plugins.revocation_max_age_days`); with an older list the host MUST warn with `revocation.stale` and, for plugins from the registry only, MUST follow the policy key `offline_revocation = allow|deny` (default `allow`).
- [HST-695] A download interrupted by network loss MUST be resumable with a range request up to 3 times and then fail with `network`, MUST leave only a partial file under staging marked with its URL and byte count, MUST remove that partial after 24 hours, and the old version MUST remain active and unchanged throughout.

### Low disk and disk full

The host checks free space on the data volume before an install or an update. It needs the package size times 3, plus 64 MiB. A separate data check runs before `migrate`: twice the data size plus 100 MiB (Data migration). If less is free, the action stops before any write with `install.no-space`.

| Event | Result |
|---|---|
| Disk full during install | The staging folder is removed. Nothing is registered. `install.no-space`, with the free space. |
| Disk full during update | The trial is abandoned. The old version stays. |
| Disk full when a plugin writes | The write returns `io` with the message `no-space`, never `quota-exceeded` ([LIF-119]). Storage writes in a batch roll back. |
| Disk full when the host writes a log | Lines are dropped. A count is kept. |
| Disk full when the snapshot is written | Previous snapshot kept. See [Persistence](/pages/reference-persistence-and-restart/). |
| Free space below 64 MiB while running | The host shows one warning and blocks installs and updates. Plugins keep running. |
| `ilium plugin backup <file>` runs out of space before the archive is complete | Precheck refuses with `backup.no-space` before any write. A disk-full error during the write returns `io` with `no-space`, removes `<file>.partial` and leaves any archive already at `<file>` unchanged. |
| `ilium plugin restore <file>` runs out of space | Precheck refuses with `restore.no-space` before any write. A disk-full error during the restore leaves each plugin at its pre-restore data or fully restored, never part-written. |

- [LIF-172] The host MUST check free space before install and update, and MUST leave no partial files when it refuses. Test: a small fake volume.
- [LIF-173] A disk-full error MUST NOT count as a plugin failure. Test: fill the disk and write.

- [HST-661] The host MUST apply these steps as free space on the data volume falls: below 512 MiB (proposed, owner to confirm) evict the plugin cache and compile cache oldest-first; below 128 MiB also stop log writes except `error` lines and shrink rotation to 1 MiB x 2; below 64 MiB block installs, updates and migrations (existing rule). Each step MUST log `disk.pressure.<step>` once, MUST re-evaluate every 30 s, and MUST undo itself above its threshold plus 10 percent.
- [HST-662] The host MUST publish a `host.disk-low` event to plugins that hold `events:host` with fields `free_mib`, `volume` and `step`, at most once per 60 s, and the footer warning MUST show the free space in MiB. A plugin MUST NOT receive the volume's absolute path.
- [HST-663] When free space returns above 64 MiB, the host MUST resume blocked installs, resume log writes, clear the footer warning within 30 s and report the number of dropped log lines in one `log.dropped` line, and plugin writes that returned `io` `no-space` MUST succeed on retry with no host restart. Test: fill then free the volume.

- [HST-132] `ilium plugin backup <file>` MUST refuse before any write with `backup.no-space` (exit 1) when the free space on the volume that holds `<file>` is less than the backed-up bytes reported by `ilium plugin du` for the items in the archive, plus 512 MiB (proposed, owner to confirm). The reserve keeps the backup from taking the volume below the first disk-pressure step of HST-661, so the running server's snapshot writes keep their room. Test: fake volume with free space one byte under the threshold; run backup; assert exit 1 and `backup.no-space`, and assert that neither `<file>` nor `<file>.partial` exists.
- [HST-133] A disk-full error during `ilium plugin backup` MUST return `io` with the message `no-space` (LIF-119), MUST delete `<file>.partial`, MUST NOT change or delete an archive that already exists at `<file>` (HST-971 applies), and MUST exit 1. Test: fill the fake volume during the write; list the folder; assert no `.partial` file and that the old archive keeps its SHA-256.
- [HST-134] `ilium plugin restore <file>` MUST refuse before any write with `restore.no-space` (exit 1) when the free space on the data volume is less than twice the uncompressed size of the entries it would write, plus 100 MiB (proposed, owner to confirm). The rule has the same shape as the migration check of LIF-107, because the pre-restore copy stays on disk until each plugin's restore commits. `--dry-run` MUST NOT run this check, since it writes nothing. Test: restore an archive with 2 MiB of entries onto a fake volume with less than 104 MiB free; assert `restore.no-space`, and assert that every file under the data root keeps its hash.
- [HST-135] A disk-full error during `ilium plugin restore` MUST leave each plugin either at its pre-restore data or fully restored (HST-653), MUST return `io` with the message `no-space`, MUST remove the temporary files the restore created under `staging/`, MUST exit 1, and MUST list each plugin id that was not restored. Test: shrink the fake volume after the precheck passes; compare each bucket's hash with its pre-restore hash.

### Read-only data directory

If the data folder is not writable, the host starts in a read-only mode. Installed plugins run. Install, update, uninstall, enable and disable fail with `io`. Storage writes fail with `io`. The footer shows "Plugin data is read-only."

- [LIF-174] A read-only data folder MUST NOT stop the server or the running plugins. Test: mark the folder read-only.

- [HST-664] The host MUST detect a non-writable data folder at start by creating and removing a probe file, MUST re-probe every 30 s, MUST leave read-only mode without restart when the probe succeeds and log `data.writable-again`, and MUST NOT lose storage writes already acknowledged. A plugin write attempted while read-only MUST return `io` with the message `read-only` and MUST NOT count as a failure.

### Read-only data root

The data folder check above covers a folder that cannot be written. This section covers the wider case: `<data>/plugins` (the plugin data root) or a project's `.ilium/` is on a read-only mount. Typical causes are live media, an immutable home folder, a container image, and a checked-out repository whose `.ilium` is mounted read-only. Each root is judged on its own, so a read-only `<data>/plugins` can sit beside a writable project `.ilium/`, and the other way round.

**Detection.** The host probes each root at start by creating and removing a probe file (see HST-664). A root that fails the probe with a read-only error (`EROFS` on Unix, the read-only attribute or a read-only volume on Windows) is read-only. A root that fails for any other reason (permission denied, no space) is handled by the rules for that cause. A read-only network filesystem start (HST-665) ends in the same state.

| Item | Behaviour in a read-only root |
|---|---|
| Start | The server starts. It never aborts because a root is read-only. |
| Storage buckets | Opened read-only. Reads work; no journal, lock or temporary file is created in the root. |
| Log | One line `host.data-readonly` at level `warn`, naming the root path and the cause, once per root per server start. |
| Plugins screen | A banner "Plugin data is read-only: `<path>`" at the top, and the lifecycle actions are greyed with the reason. The footer shows the same notice as in the read-only data folder case. |
| Plugin writes | Return `io` with the message `read-only`. Never `quota-exceeded` and never `no-space`. A read-only write does not count as a plugin failure. |
| `install`, `update`, `uninstall`, `migrate`, `backup restore` | Refused with `install.readonly` (exit 1) before any write. Staging, locks and `installed.lock` are not touched. |
| Data migration | Never started. The host does not call `migrate`, does not take the pre-migration copy and does not write the migration stamp. |
| Enable, disable | Refused with `install.readonly`; the enabled set is the one on disk. |
| `ilium plugin backup <file>` | Allowed when `<file>` is on a writable volume. It only reads the root. |
| Leaving the state | When the next probe (30 s) succeeds, the host logs `data.writable-again`, reopens buckets read-write and lifts the refusals without a restart. |

- A plugin never sees the mount, the path or the cause. It sees only `io` with the message `read-only`.
- A plugin whose installed version needs a data migration (the stored data version is older than the manifest `data_version`) is kept at its installed version and loaded with its stored data, read-only. The plugin screen shows "Migration pending: data is read-only". The host never runs `migrate` against a stamp it cannot write, because a migration without its stamp would run again at every start.
- The host MUST start, load and run installed plugins when `<data>/plugins` or a project's `.ilium/` is read-only, and MUST open storage buckets read-only; this extends [LIF-174] and [HST-664]. Test: mount the root read-only and start all fixtures.
- A write to a read-only root MUST return `io` with the message `read-only`, never `quota-exceeded` ([LIF-119], [EXT-063]) and never `no-space` ([EXT-064]); the message `no-space` stays reserved for a full volume. Test: write to a read-only bucket and to a full one; compare the messages.
- Install, update, uninstall, migrate and backup restore MUST fail with `install.readonly` before any write, and MUST log `host.data-readonly` once per root with the path. The reference page for these commands lists `install.read-only` for the registry root ([LIF-725]); the two codes name the same refusal and the spelling is to be reconciled in the install page and the requirements file. Test: chmod and remount each root read-only and run each command.
- The host MUST NOT run a data migration against a read-only stamp, and MUST treat the free-space check of [LIF-107] as not applicable while the root is read-only, so the refusal is `install.readonly` and never `install.no-space`. Test: set the stored data version below the manifest version on a read-only mount; assert that `migrate` is not called and no file changes.

#### Failure modes

| Failure | Result |
|---|---|
| The root turns read-only while the server runs (remount, failing media) | The first failed write puts the root in read-only mode. Acknowledged writes stay; in-flight batch writes roll back with `io` `read-only`. |
| Only the project `.ilium/` is read-only | Project-scoped writes fail with `io` `read-only`; global storage and installs of global plugins keep working. |
| The probe file cannot be created for an unrelated reason | The reason is reported (for example `io` `permission-denied`), not `read-only`. |
| Both roots are read-only | All lifecycle commands are refused; running plugins and the recovery page (`ilium plugin recovery`) still work, its repair actions are greyed. |

### Corruption

| Damaged item | Handling |
|---|---|
| `installed.json` | Core plugins only, recovery page opens, file not overwritten |
| A package folder | The plugin is `refused-at-load` with `package.corrupt` |
| `grants.json` | Renamed to `grants.json.corrupt-<YYYYMMDD-HHmmss>`, logged `grants.unreadable`; T2 and T3 plugins start as not installed with no grants |
| A storage database | The file is renamed `<id>.db.corrupt-<time>`, an empty bucket is created, the plugin gets a notice and the log has one error |
| A compile cache entry | Deleted and rebuilt |

- [LIF-175] A corrupt storage database MUST be moved aside and replaced with an empty one, never deleted. Test: write garbage into the file.

- [HST-657] The host MUST run a database integrity check when a storage database is first opened in a server's lifetime, MUST bound it to 2 s per MiB-class database of at most 16 MiB, and a check that is not finished in 10 s MUST be treated as failed and handled as corruption (move aside, empty bucket). The check MUST NOT block other plugins from starting.
- [HST-659] When `installed.json` is unreadable, the host MUST NOT overwrite it, MUST rebuild a read-only candidate registry in memory by scanning `store/<id>/<version>/manifest.json` with valid `manifest.sha256`, MUST offer it on the recovery page with a Rebuild action, and MUST copy the damaged file to `installed.json.corrupt-<time>` before writing a rebuilt file. Rebuilt plugins MUST start disabled.
- [HST-660] The recovery page MUST be reachable with `ilium plugin recovery` (text mode) and from the TUI footer notice, MUST list each damaged item with its path, the detected problem code and the available actions (Rebuild, Restore from backup, Start without), MUST work when the plugin platform is entirely unavailable, and MUST exit non-zero code 78 from the CLI while any item is unresolved.

### The compile cache

T2 and T3 components are compiled ahead of use and cached in `<cache dir>/ilium/plugins/compile/`. The key is the component hash, the Wasmtime version and the CPU features. A change of any part makes a new entry. The cache may be removed at any time and is rebuilt on demand. It is not needed to run, only to start fast.

- [LIF-176] The compile cache key MUST include the component hash, the Wasmtime version and the CPU features, and a missing cache MUST NOT stop a plugin. Test: delete the cache and start.

### See also

- [Plugin files on disk](/pages/reference-plugin-files/)
- [Install, update and uninstall](/pages/reference-install-update-uninstall/)
- [Resource accounting](/pages/reference-resource-accounting/)
- [Requirements, lifecycle and operations](/pages/reference-requirements-lifecycle-ops/)

API version checked: `ilium:plugin@0.1.0`.
