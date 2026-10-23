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

### Shared hosts: each user has their own stores

On a host where several OS users run Ilium, each user has separate stores: grants, consents, trust, secrets, the audit log and the plugin install directory, all under that user's own directories ([SEC-194](/pages/guide-enterprise-deployment/)). The one shared item is a system-wide install, which users cannot write ([HST-687](/pages/guide-enterprise-deployment/)).

`ilium plugin backup` reads the stores of the OS user that runs it. One run therefore captures one user's plugin data and nothing else. On a shared host, a run by a single user leaves out every other user's data. The exit status is still 0, and the restore drill still passes, because the archive it produced is internally valid. The administrator must cover each user separately:

- Keep a list of every OS user that holds plugin data. The manual defines no command that lists those users, so the list comes from the host's account records.
- Run `ilium plugin backup` once for each user, as that user, with one archive per user and a file name that names the user. Each user then has their own job under [HST-970](#cadence-and-retention), their own set of archives under [HST-971](#cadence-and-retention), and their own restore drill under [HST-973](#restore-drill).
- Restore a user's archive under the same OS account, into a scratch root ([HST-974](#restore-drill)). On a different machine the restored `trust.json` is kept as `trust.json.tampered-<YYYYMMDD-HHmmss>` ([HST-959](/pages/reference-plugin-files/)), and that user must re-establish each publisher with `ilium plugin trust add`.

**Purging a user.** `ilium plugin admin purge-user <user> --yes`, the command proposed in [HST-689](/pages/guide-enterprise-deployment/) (owner to confirm), deletes that user's plugin data, storage, grants, logs and cache. It refuses a user whose session server is running (`purge.user-active`). It does not delete archives or exports that the administrator wrote elsewhere, the same as [HST-977](#deletion-and-export). Before a purge made for a privacy request, take an archive of that user first, under the same rule as [HST-978](#deletion-and-export), so the data can be restored if the request is withdrawn. A user's archive does not hold the audit log ([HST-960](/pages/reference-plugin-files/)), so the system audit entry that `purge-user` writes is the only record of the purge that the host keeps.

### Cadence and retention

- [HST-970] An administrator who keeps plugin data MUST run `ilium plugin backup` at least once every 24 hours (proposed, owner to confirm) from a scheduled job, and the job MUST fail loudly when the command exits non-zero. Test: schedule the job, stop the server, and read the job's exit status.
- [HST-971] The administrator MUST keep at least the 7 newest archives (proposed, owner to confirm) and MUST keep them on a different disk from the live `<data>/plugins` folder. `ilium plugin backup` MUST NOT delete any archive, including an older one at the same path, unless `--force` is given. Test: write 8 archives, then read the folder listing.
- [HST-972] An archive MUST be treated as data: it holds plugin data and, with `--include-secrets`, credentials. The administrator MUST store it with the same access limit as the plugin data folder. Test: list the owner and mode of an archive written by the command.

### Archive encryption and off-site copies

**No archive leaves the host unencrypted.** The host does not encrypt archives in this version. `ilium plugin backup` writes plain bytes, and the command prints that fact and the archive path on each run ([HST-962](/pages/reference-plugin-files/), [SEC-958](/pages/reference-secrets/#storage-at-rest)). An archive made with `--include-secrets` holds credentials in that same readable form, so it is a credential store, not only a data backup.

On the host, the archive is protected by its owner-only mode ([HST-962](/pages/reference-plugin-files/) sets 0600, [HST-972](#cadence-and-retention) requires the same limit as the data folder) and by whatever disk encryption the administrator applies ([SEC-958](/pages/reference-secrets/#storage-at-rest)). No Ilium option takes a passphrase for an archive. `ILIUM_KEY_PASSPHRASE` unlocks publisher signing keys only ([PKG-211](/pages/reference-signing-and-trust/)), and the manual defines no use of it for `backup` or `restore`.

Rules for copies that leave the host (not numbered, as the administrator's own procedure):

- Encrypt an archive before it goes to any storage the administrator does not fully control, such as an off-site server, a cloud bucket, removable media or a share another party can reach. The manual names no encryption tool; the administrator chooses one.
- Keep the key or passphrase apart from the encrypted copy: not in the same folder, bucket or script, and never on a command line. This follows the rule of [PKG-494](/pages/guide-publishing/). A lost key makes the encrypted copy unrecoverable, so keep the key in a second safe place as well.
- Before relying on an encrypted copy, decrypt it into a scratch root and run `ilium plugin restore <file> --dry-run` on the decrypted file, as in the drill above. Delete the decrypted file once the drill result is recorded.
- The host's erasure does not reach copies outside Ilium ([HST-962](/pages/reference-plugin-files/)). When a plugin's data must be gone everywhere, delete each off-site copy and its key separately, as [HST-977](#deletion-and-export) requires for the copies the administrator keeps.

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
