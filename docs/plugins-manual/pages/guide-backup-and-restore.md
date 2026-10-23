---
layout: page
title: Guide, backup and restore
permalink: /pages/guide-backup-and-restore/
---

## Guide, backup and restore

Plugin data is only safe when a copy of it exists and has been restored at least once. This guide sets the backup cadence, the number of archives to keep, a restore drill, and the recovery targets for plugin data. It also says which copies the host keeps by itself and which ones the administrator must manage. It is normative, and every rule has a requirement id.

Contract version: `ilium:plugin@0.1.0`.

### What is backed up, and by whom

| Copy | Made by | Kept by | Covered by |
|---|---|---|---|
| Whole-host archive (plugin data, buckets, settings; grants with `--include-grants`, secrets with `--include-secrets`) | `ilium plugin backup <file.tar.zst>` | The administrator, at the path given | HST-652, HST-654 |
| Single-plugin export (data, buckets, settings values) | `ilium plugin export <id> --out <file>` | The administrator | LIF-290 to LIF-292 |
| Pre-migration copy | The host, before `migrate` | The host: only the newest is kept | LIF-107, LIF-109 |
| Logs and audit trail | The host | The host, on the retention in [Plugin files](/pages/reference-plugin-files/) | Log rotation table in [Plugin files](/pages/reference-plugin-files/) |

The host never writes a whole-host archive on its own schedule. The cadence, the number of archives and the restore drill below are the administrator's job.

### Cadence and retention

- [HST-970] An administrator who keeps plugin data MUST run `ilium plugin backup` at least once every 24 hours (proposed, owner to confirm) from a scheduled job, and the job MUST fail loudly when the command exits non-zero. Test: schedule the job, stop the server, and read the job's exit status.
- [HST-971] The administrator MUST keep at least the 7 newest archives (proposed, owner to confirm) and MUST keep them on a different disk from the live `<data>/plugins` folder. `ilium plugin backup` MUST NOT delete any archive, including an older one at the same path, unless `--force` is given. Test: write 8 archives, then read the folder listing.
- [HST-972] An archive MUST be treated as data: it holds plugin data and, with `--include-secrets`, credentials. The administrator MUST store it with the same access limit as the plugin data folder. Test: list the owner and mode of an archive written by the command.

### Restore drill

A backup is not known to work until it has been restored. The drill below uses the same commands as a real restore, but changes nothing in the live data.

1. Check the newest archive with `ilium plugin restore <file> --dry-run`. The dry run validates the manifest and every entry hash and lists what would change. It changes no file.
2. Restore that archive into a scratch data root, never into the live one, with the live session server stopped or not started.
3. Start the plugins in the scratch root and read `ilium plugin list` and the data of one plugin you know.
4. Record the archive name, the date, the result of each step, and the time the restore took.

- [HST-973] The administrator MUST run the restore drill at least once a month (proposed, owner to confirm) and after every change of host version. A dry run that exits non-zero, or lists a hash mismatch, MUST count as a failed drill. Test: flip one byte of an entry in a copy of the archive, run `--dry-run`, and read the exit status and the listing.
- [HST-974] A drill MUST restore into a scratch data root. A restore into a live data root while a session server runs MUST be refused with `restore.server-running` (HST-653). Test: start a server and run `ilium plugin restore` against its data root, and read the error code.

### Recovery targets

These are targets for planning. They are not guarantees the host makes.

- [HST-975] For plugin data, the recovery point objective MUST be no more than 24 hours (proposed, owner to confirm), which is the backup cadence of HST-970. Data written after the newest archive is lost on restore. Test: write data, wait past the cadence, restore, and count the lost writes.
- [HST-976] For a single plugin's data in an archive up to 1 GiB (proposed, owner to confirm), the restore MUST complete within 15 minutes (proposed, owner to confirm) on the reference machine, timed from the first `restore` command to the final result line. Test: time one restore of a 1 GiB archive on the reference machine and record the result in the drill log.

### Deletion and export

- [HST-977] Deleting plugin data, with `ilium plugin uninstall <id> --purge` or the Delete data choice in the uninstall dialog, MUST NOT delete a whole-host archive or a single-plugin export that the administrator wrote elsewhere. The administrator MUST delete those copies separately when the data must be gone everywhere. Test: export a plugin, purge it, and check that the export file still exists.
- [HST-978] Before the administrator purges a plugin for a privacy request, the administrator MUST take an export or an archive of that plugin, so that the data can be restored if the request is withdrawn or wrongly scoped. Test: follow the runbook of [Administrator failure runbook](/pages/guide-admin-failure-runbook/) and check the export exists before the purge.

### Common failures

| Symptom | Cause | What to do |
|---|---|---|
| `restore.format-unsupported` | The archive has an unknown major format (HST-654) | Restore with a host that wrote the archive, then take a new archive |
| `restore.host-newer` | The archive was written by a newer host (HST-654) | Install that host version, or pass `--force` after reading the release notes |
| Dry run lists a hash mismatch | The archive was damaged or edited (HST-653) | Do not restore it. Use the previous archive and check the copy process |
| `restore.server-running` | A session server is running on the data root (HST-653) | Stop the server, or restore into a scratch root |
| `package.platform-unsupported` after a restore on another system | The plugin does not run on this operating system (HST-655) | The data is kept. Install a build for this platform, or leave the plugin refused at load |

### Where the other rules are

- [Administrator failure runbook](/pages/guide-admin-failure-runbook/) for the steps when a plugin has damaged its own data.
- [Data migration](/pages/guide-data-migration/) for the copies the host makes before a migration.
- [Plugin files](/pages/reference-plugin-files/) for where each copy lives.
