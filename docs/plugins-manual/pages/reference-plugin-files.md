---
layout: page
title: Plugin files on disk
permalink: /pages/reference-plugin-files/
---

## Plugin files on disk

This page lists every file and folder that the plugin platform creates, where it lives on Linux, macOS and Windows, what format it has, who writes it and what removes it. It is the one table that the user pages, the administration guide and the troubleshooting page point to. A host implementer uses it to place files. A test author uses it to check what an install, a grant change or an uninstall left behind.

Contract version: `ilium:plugin@0.1.0`. Requirement ids have the form `[USR-nnn]` and are collected in [User and administrator requirements](/pages/reference-requirements-user-admin/).

<div class="note">
<p>Decision status. The platform is not implemented. Folder and file names below are the proposed layout. They are marked normative here so that tests can be written, and the platform owner may still rename a file. Names that other pages already use, such as <code>installed.json</code> and <code>plugins/staging/</code>, are kept.</p>
</div>

### Contents

- How paths are chosen
- The table of items
- The plugin log
- File permissions
- What each action leaves behind
- Back up and move plugins to another machine
- Errors and edge cases
- Requirements

### How paths are chosen

Ilium never writes a hard-coded `~`. It asks the operating system for three roots through the `directories` crate, with the project name `ilium` and no organisation: the configuration folder, the data folder and the cache folder. A fourth root, the state folder (`<state>`), holds the grant, consent, trust and audit files.

| Root | Linux | macOS | Windows |
|---|---|---|---|
| Configuration (`<config>`) | `~/.config/ilium` | `~/Library/Application Support/ilium` | `%APPDATA%\ilium\config` |
| Data (`<data>`) | `~/.local/share/ilium` | `~/Library/Application Support/ilium` | `%APPDATA%\ilium\data` |
| Cache (`<cache>`) | `~/.cache/ilium` | `~/Library/Caches/ilium` | `%LOCALAPPDATA%\ilium\cache` |
| State (`<state>`) | `$XDG_STATE_HOME/ilium` (default `~/.local/state/ilium`) | `~/Library/Application Support/Ilium/state` | `%LOCALAPPDATA%\Ilium\state` |

On Linux the roots follow `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME` and `XDG_STATE_HOME` when they are set. The rest of this page writes `<config>`, `<data>`, `<cache>` and `<state>` and never repeats the system paths. A project folder (`<project>`) is the canonical launch folder of a session. Project-scope files live in `<project>/.ilium/`.

- [USR-200] The host MUST resolve `<config>`, `<data>`, `<cache>` and `<state>` (the per-OS paths of [SEC-112]) with the `directories` crate for the project name `ilium`, and MUST NOT write plugin files anywhere else. Test: start Ilium with `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME` and `XDG_STATE_HOME` pointing at four empty folders, install a plugin, and assert that no plugin file appears outside those folders and the project's `.ilium` folder.

### The table of items

Paths start at `<config>`, `<data>`, `<cache>`, `<state>` or `<project>`. The plugin root is `<data>/plugins/`. The layout of the plugin root is owned by [Install, update and uninstall](/pages/reference-install-update-uninstall/); this table repeats it for users and adds the user-facing files.

| Item | Location | Format | Written by | Removed by |
|---|---|---|---|---|
| Registry (which plugins exist, versions, enabled flags) | `<data>/plugins/installed.json` | JSON | Install, update, enable, disable, uninstall | Uninstall edits it. Nothing deletes the file |
| Registry lock | `<data>/plugins/installed.lock` | Exclusive OS advisory lock on this file; the file also holds the holder's process id and boot id for messages | Every command that changes the registry | The lock is released when the holder exits |
| Installed packages | `<data>/plugins/store/<id>/<version>/` | `manifest.json`, `manifest.sha256`, components, files | Install, update | Uninstall. An update removes the old version after the swap |
| Unpacking area | `<data>/plugins/staging/<uuid>/` | Same as a package | Install, update | The rename into the store, a failed operation, or the next start (older than 1 hour or its process is gone) |
| Pre-migration copies | `<data>/plugins/backups/<id>/<version>-<utc timestamp>/` | Copy of data and buckets | Update with a data migration | The next committed update keeps only the newest; `uninstall --purge` deletes them |
| Plugin data | `<data>/plugins/data/<id>/` | Whatever the plugin writes | The plugin, through `fs.write:data` | `uninstall --purge`, or Delete data in the uninstall dialog |
| Purge note | `<data>/plugins/purged.json` | JSON list of plugin id and purge time | `uninstall --purge` | Ilium, once every known project has been opened and cleaned of that plugin's data; a project never reopened keeps its data |
| Global storage bucket | `<data>/plugins/storage/<id>/global.db` | Key-value database | The plugin, through `storage` | Same as plugin data |
| Project and session buckets | Inside `<project>/.ilium/` and the session snapshot `<project>/.ilium/sessions/<session>.json` | Database and JSON | The plugin and the host | Same as plugin data, for the open project and session. Other projects get a tombstone |
| Grants and consent answers | `<state>/grants.json` and `consents.json` (one file each for all plugins) | JSON | Review dialog, Capabilities tab, `ilium plugin grants`, first-use prompts | Every uninstall |
| Trusted publisher keys | `<state>/trust.json`, or the path in `ILIUM_TRUST_STORE` | JSON object `ilium-trust/1` with a `keys` list; see [Signing and trust](/pages/reference-signing-and-trust/) | `ilium plugin trust add` and the trust prompt | `ilium plugin trust remove` |
| Plugin settings, user scope | `<config>/config.toml`, table `[plugins."<id>"]` | TOML | The Settings screen | `uninstall --purge`, Forget settings |
| Plugin settings, project scope | `<project>/.ilium/config.yaml`, key `plugins.<id>` | YAML | The Settings screen | Same two actions, for the open project |
| Plugin limits | `<config>/config.toml`, table `[plugins.limits]` | TOML | The user | The user |
| Plugin cache | `<cache dir>/ilium/plugins/<id>/` | Whatever the plugin writes | The plugin, through `fs.write:cache` | The host may evict it at any time. Removed on every uninstall (USR-208) |
| Compile cache | `<cache dir>/ilium/plugins/compile/` | Compiled components | Install of a T2 or T3 plugin | The host, when space is low |
| Plugin log | `<state>/plugins/logs/<session-slug>/<id>.log` and rotated files | Text, five columns | The plugin and the host | Rotation. See below |
| User extension order and toggles | `<config>/config.toml`, table `[plugins.extensions]` | TOML | The Extensions tab | Every uninstall of the owner |
| Replacement record | `<config>/config.toml`, table `[plugins.replaces]` | TOML | The replace confirmation | Undoing the replacement |
| Administrator policy | See [Plugin policy](/pages/reference-plugin-policy/) | TOML | The administrator | The administrator |
| Audit log | `<state>/audit/audit.jsonl` (see [Audit log](/pages/reference-audit-log/)) | JSON lines | The host | Rotation by `audit.max_mib` and `audit.keep_days` |

The two last settings-adjacent tables, `[plugins.extensions]` and `[plugins.replaces]`, are proposed names that the owner of [Extension points](/pages/api-extension-points/) confirms.

- [USR-201] The host MUST keep each item at the location in the table, and MUST create the parent folders on first use. Test: on a clean profile, run install, grant, trust add, enable, disable and uninstall, then list the folders and match each file against the table.
- [USR-202] The host MUST write `installed.json`, the grant files and the trust store atomically: it writes a temporary sibling file, flushes it and renames it over the target. Test: kill the process during a grant change and assert that the file holds either the old or the new content, never a mix.
- [USR-203] Grants MUST be stored per user and per plugin, and MUST NOT be stored in a project folder. Test: grant `fs.read:project` while a project is open, open a second project and assert that the grant applies there and that `<project>/.ilium/` holds no grant.

### Contents of the user-facing files

The grant file has one entry per side. A grant value is `granted`, `declined` or `revoked`. A grant that the manifest never requested has no entry. Consent answers for the prompts that are not grants sit in the same file.

```json
{
  "server": { "tree.read": "granted", "http:api.example.com": "declined" },
  "client": { "ui.panels": "granted" },
  "consent": {
    "open_external": ["https://status.example.com"],
    "http_hosts": ["api.example.com"],
    "location_geocode": true,
    "first_start_not_now": 1
  }
}
```

`first_start_not_now` counts how often the user chose Not now at the first-start prompt of a bundled plugin. See [Install and manage plugins](/pages/user-install-and-manage/#first-start-of-a-bundled-plugin).

The fingerprint of a key is shown as 64 lowercase hexadecimal characters in 16 groups of 4 (see the CLI reference, [DEV-112]). The short form is the first 8 characters, for example `3f2a91c0`. It is shown only beside the full form or in `trust list` ([SEC-177]).

### The plugin log

Each plugin has one log, shared by all its sides. The host writes refusals, traps, restarts and process runs into it, and the plugin writes its own lines through the `log` import. See [log](/pages/api-log/).

| Property | Value |
|---|---|
| File | `<state>/plugins/logs/<session-slug>/<id>.log`, with rotated files `<id>.log.1` to `<id>.log.4` |
| Line format | Five columns: local timestamp `YYYY-MM-DD HH:mm:ss`, level padded to five characters, side padded to six, plugin id, message |
| Rotation | When the file passes 4 MiB it becomes `.1`, `.1` becomes `.2`, `.2` becomes `.3`, `.3` becomes `.4`, and the old `.4` is deleted |
| Retention | The active file plus at most 4 rotated files (5 files per plugin). Rotated files older than 14 days are deleted at server start |
| Rate limit | 200 lines per second for a standard plugin. Extra lines are dropped and counted in one `warn` summary line |
| Who can read it | The user who owns the folder. A plugin cannot read its own log |
| After uninstall | Kept for 7 days, then deleted. `--purge` deletes it at once |

- [USR-204] The host MUST rotate the log at 4 MiB, MUST keep at most four rotated files, and MUST delete rotated files older than 14 days at server start. Test: write 5 MiB of log lines through a test plugin and list the folder; then age `<id>.log.2` by 15 days, restart, and assert it is gone.
- [USR-205] Each log line MUST have the five columns timestamp, level, side, plugin id and message, with the timestamp in the form `YYYY-MM-DD HH:mm:ss`. Test: read the log and match every line against the pattern.

### File permissions

On Linux and macOS, every folder under the plugin roots has mode `0700` and every file has mode `0600`. On Windows the host sets an access list that allows only the current user. A package that ships files with other modes does not change this: the host extracts files with the modes above.

- [USR-206] The host MUST create plugin folders with owner-only access and plugin files with owner-only read and write access. Test: after an install, run `stat` on every created path and assert mode `0700` for folders and `0600` for files on Linux.
- [USR-207] The host MUST refuse to read a grant file or the trust store when another user can write to it, and MUST treat every third-party plugin as not allowed to run until the file is fixed. Test: set the mode of a grant file to `0666`, start Ilium, and assert that T2 and T3 plugins are refused at load with the code `grants.unsafe-permissions` while core plugins still run.

### What each action leaves behind

The default uninstall, `--purge` and `--keep-data` are specified in the table of [Install, update and uninstall](/pages/reference-install-update-uninstall/#uninstall). The rows that matter to a user:

| Action | Package | Grants | Settings | Data, storage | Cache | Extensions order | Log | Trusted keys |
|---|---|---|---|---|---|---|---|---|
| Disable | kept | kept | kept | kept | kept | kept | kept | kept |
| Uninstall, default | removed | removed | kept | kept, no question asked | removed | removed | kept 7 days | kept |
| Uninstall `--purge` | removed | removed | removed | removed | removed | removed | removed | kept |
| Uninstall `--keep-data` | removed | removed | kept | kept | removed | removed | kept 7 days | kept |
| Forget settings (recovery page) | kept | kept | removed | kept | kept | kept | kept | kept |
| `trust remove` | kept | kept | kept | kept | kept | kept | kept | key removed |

Trust entries are never removed by an uninstall, because one key may sign several plugins. After `trust remove`, plugins signed with that key stay installed and show `untrusted` in `ilium plugin list`.

- [USR-208] A default uninstall MUST remove the package, the grants, the cache and the Extensions order entries, and MUST keep settings. Test: install a plugin, write one setting, uninstall with no flag, and list what remains.
- [USR-209] `--purge` MUST also remove settings, data, storage buckets and the log, and `--keep-data` MUST keep data and storage without asking. The two flags exclude each other. Test: run each flag on a copy of the same profile and compare the remains with the table.

### Back up and move plugins to another machine

Copy these items, in this order, with Ilium stopped:

1. `<data>/plugins/store/` and `<data>/plugins/installed.json`, so the same versions install again.
2. The trust store `trust.json`, so signed packages are still trusted.
3. `<state>/grants.json` and `consents.json`, if you want the same answers. Skip it to be asked again on the new machine.
4. `<config>/config.toml` and the `.ilium` folders of your projects, for settings.
5. `<data>/plugins/data/` and `<data>/plugins/storage/`, for the plugin's own data.

Skip the cache, the logs, the staging folder and the backups. A machine that runs a different operating system cannot run T2 and T3 plugins unless it is Linux (see [Plugins overview](/pages/user-plugins-overview/)). Core and bundled plugins need no copy, because they ship with Ilium.

### Secret rotation

Target behaviour (specified here before the code is written).

- [SEC-451] `ilium plugin secret rotate <id> <name>` MUST replace a stored secret value without reinstalling or disabling the plugin. The old value MUST be overwritten in the secret store, the plugin MUST receive a `secret-changed` event for that name, and the audit log MUST record the rotation without the value.
- [SEC-452] The HTTP API token and the lock tokens MUST be rotatable with `ilium plugin token rotate [--http|--lock <id>]`. The old token MUST stop working at the moment the command returns, and a plugin that held a lock token MUST receive a fresh one through its next request without a restart.
- [SEC-453] A secret MAY carry an `expires` time set by the user or the policy. The Plugins screen MUST warn 14 days before it passes, and after it passes the secret MUST NOT be delivered: the plugin gets `secret.expired` (not an empty value) until the secret is rotated.

### Errors and edge cases

| Situation | What happens |
|---|---|
| `installed.json` is missing | The host treats the profile as empty and starts with core plugins only. Packages in `store/` stay on disk and are not listed. `ilium plugin reinstall <id>` repairs a store entry from the cached package |
| `installed.json` does not parse | The host starts with core plugins only, writes one error to the Ilium log, and opens the recovery page. It does not overwrite the file |
| A grant file does not parse | The file is renamed to `grants.json.corrupt-<YYYYMMDD-HHmmss>` and the server logs `grants.unreadable`. T2 and T3 plugins start as not installed with no grants; T1 plugins start with only normal grants ([HST-044]) |
| A package folder is missing but the registry lists it | The plugin is refused at load with the code `package.corrupt` |
| The disk is full during a write | The action stops, the old file stays, and the error is `install.io` or `io` |
| Two Ilium servers share one profile | Each write takes `installed.lock`. The second writer polls every 100 ms for up to 30 s, then fails the action with `install.locked` (exit code 4, [LIF-066]) |

### See also

- [Install and manage plugins](/pages/user-install-and-manage/)
- [Plugin privacy](/pages/user-privacy/)
- [Troubleshooting plugins](/pages/user-troubleshooting/)
- [Plugin policy](/pages/reference-plugin-policy/)
- [Administration guide](/pages/guide-administration/)
- [log](/pages/api-log/)
- [Lifecycle](/pages/lifecycle/)

API version checked: `ilium:plugin@0.1.0`.

## Production additions: T10-operations

**[HST-651]** The host MUST provide `ilium plugin du [<plugin-id>] [--json]` that reports, per plugin, the bytes used by store, data, storage buckets, cache, logs and backups, each as an integer, within 5 s for 100 plugins, and the sum MUST equal the on-disk size of those folders within 1 MiB. Test: write known bytes and compare.

**[HST-652]** The host MUST provide `ilium plugin backup <file.tar.zst> [--include-grants] [--include-secrets]` (proposed, owner to confirm) that writes a consistent archive of the items listed in the backup procedure while Ilium is running, by taking each bucket lock briefly (at most 2 s per bucket), MUST exclude cache, logs, staging and compile cache, MUST write to `<file>.partial` and rename on success, and MUST exit 0 only after the archive is verified readable. Secrets MUST NOT be included unless `--include-secrets` is given.

**[HST-653]** The host MUST provide `ilium plugin restore <file> [--dry-run] [--merge|--replace]` that validates the archive manifest and every entry hash before changing anything, MUST refuse a running session server with `restore.server-running`, MUST apply the restore atomically per plugin (all of a plugin's data or none), and MUST on any failure leave the pre-restore state intact. `--dry-run` MUST change no file and list what would change.

**[HST-654]** The backup archive MUST contain a `backup.json` with `format: "ilium-backup/1"` (proposed, owner to confirm), the host version, creation time (UTC, ISO 8601), the OS, and a SHA-256 for each entry. A restore of an archive with an unknown major format MUST fail with `restore.format-unsupported`, and a restore of a newer host version's archive MUST fail with `restore.host-newer` unless `--force` is given.

**[HST-655]** A restore on a different operating system MUST rewrite no plugin data bytes, MUST normalise path separators in `installed.json` and settings, MUST mark every T2/T3 plugin whose package targets an unavailable platform `refused-at-load` with `package.platform-unsupported` while keeping its data, and MUST list those plugins in the restore result.

**[HST-665]** At start the host MUST detect when `<data>` or `<state>` is on a network filesystem (NFS, SMB, FUSE-remote), MUST log `data.network-fs` once, and MUST fall back to a lock scheme that does not rely on advisory locks alone (lock file with owner id and boot id, renewed every 10 s (proposed, owner to confirm), stale after 60 s). If neither scheme can be guaranteed it MUST start in read-only mode.

**[HST-666]** The host MUST treat plugin ids as lowercase ASCII on disk so two ids that differ only by case cannot collide, MUST reject at install any package whose file paths collide after case folding or Unicode NFC/NFD normalisation with `package.path-collision`, and MUST create store paths with no characters outside `[a-z0-9._-]`.

**[HST-667]** On Windows the host MUST use extended-length paths (`\\?\` prefix) for every file under the plugin roots and MUST accept a package whose longest relative path is at most 240 bytes (proposed, owner to confirm); a longer path MUST be rejected at install with `package.path-too-long` naming the file.

**[HST-668]** The host MUST refuse to follow a symbolic link or reparse point inside any plugin root when reading or writing plugin files, MUST log `fs.symlink-refused` with the path, and MUST fail the operation with `io` and the message `symlink`. A symlink at the root folder itself (e.g. `<data>/plugins` pointing to another disk) MUST be allowed only when owned by the current user.

**[HST-669]** The host MUST support the environment variable `ILIUM_PLUGIN_DATA_DIR` (proposed, owner to confirm) and the key `plugins.data_dir` to place `<data>/plugins/` elsewhere, MUST validate that the target exists, is owned by the current user and has mode 0700 before use, and MUST refuse to start plugins from it with `data.root-invalid` otherwise. Changing it MUST NOT move existing data automatically.

**[HST-672]** The compile cache MUST be capped at 1024 MiB (`plugins.compile_cache_mib`), MUST evict least-recently-used entries when over the cap, MUST NOT evict an entry in use, and MUST be written atomically (temp file plus rename) so a crash never leaves a loadable truncated entry; a truncated entry MUST fail its recorded SHA-256 and be rebuilt.

**[HST-673]** The plugin cache at `<cache>/plugins/<id>/` MUST be limited by the key `cache_mib` (default 64, ceiling 512) (proposed, owner to confirm). On exceeding it the host MUST fail the write with `quota-exceeded` message `cache-full` and MUST evict only whole files oldest-modified-first when the cache volume has less than 512 MiB free, logging `cache.evict` with the count.

**[HST-675]** Entries in `purged.json` MUST be kept for 365 days, MUST be applied when the matching project is next opened (deleting that plugin's bucket and settings there, logged `purge.applied`), and a project folder with plugin data whose plugin is not installed MUST be listed by `ilium plugin doctor` as orphaned with size and MUST NOT be deleted automatically.

**[HST-690]** The host MUST NOT use wall-clock time for lock staleness, retention or quota refill; it MUST use a monotonic clock for durations within a run and persisted UTC timestamps only for display and retention across runs. A wall clock moved backwards MUST NOT cause a retained backup, log or quarantine file to be deleted earlier than its stated age.

### Backup contents, consistency and restore rules

The backup procedure in [Back up and move plugins to another machine](#back-up-and-move-plugins-to-another-machine) is the list of items. The rules below define what an archive holds, what it does not guarantee, how it is protected, and how a restore treats the trust store and the data version.

**[HST-959]** A restored `trust.json` MUST NOT be trusted on its own. Step 2 of the backup procedure copies the file, but the keyed MAC of [SEC-721](/pages/reference-signing-and-trust/) is held in the user's OS secret store and does not travel. On a machine where the MAC cannot be verified, the host MUST keep the restored file as `trust.json.tampered-<YYYYMMDD-HHmmss>`, MUST refuse T2 and T3 plugins with `trust.store-tampered`, and MUST tell the user to re-establish each publisher with `ilium plugin trust add <publisher.pub>` (proposed, owner to confirm: a `trust import` command that recomputes the MAC is an alternative the owner may choose instead). Test: copy `trust.json` alone to a profile with no MAC key, start Ilium, and assert that the file is renamed, that T3 plugins are refused with `trust.store-tampered`, and that the message names the `trust add` command.

**[HST-960]** The archive written by `ilium plugin backup` MUST hold exactly the items of the backup procedure that are in scope for the flags given, plus `backup.json` ([HST-654](#production-additions-t10-operations)): the store and `installed.json`; `trust.json`; `grants.json` and `consents.json` only with `--include-grants`; the settings of `<config>/config.toml` and of each project `.ilium/` folder that the host has opened (proposed, owner to confirm); and the plugin data and storage buckets. It MUST NOT hold cache, logs, staging, compile cache, the pre-migration copies under `backups/`, or secrets unless `--include-secrets` is given. `backup.json` MUST list every entry path with its size and SHA-256. Test: back up a profile that holds each item, then compare the archive listing with this list and with the entries of `backup.json`.

**[HST-961]** A backup MUST NOT claim cross-bucket consistency. Each bucket is captured under its own lock of at most 2 s ([HST-652](#production-additions-t10-operations)), so a restored global bucket and a restored project or session bucket can come from different moments. `backup.json` MUST record, for each bucket entry, the UTC time at which its lock was taken, and `ilium plugin backup` MUST print the warning `backup.not-cross-bucket-consistent` (proposed, owner to confirm) when the archive holds more than one bucket. A plugin that keeps an invariant across buckets MUST keep that invariant inside one bucket, because a restore cannot guarantee that two buckets come from the same moment. Test: back up a profile with one global and one project bucket, then read the two capture times in `backup.json` and the warning line.

**[HST-962]** The archive is plain data and holds personal data, so `ilium plugin backup` MUST create the `.partial` file and the final archive with mode `0600`, MUST create the folder beside them with mode `0700` when it creates the folder, and MUST refuse to write the archive inside `<data>/plugins/` with `backup.inside-live-root` (proposed, owner to confirm), so that a copy never sits in the live tree. The host does not encrypt archives in this version, and the command output MUST say so and name the archive path. Erasure requests under [HST-978](/pages/guide-backup-and-restore/) reach only the live data root and the archives the administrator keeps; a copy that a user made outside Ilium is not reached by the host. Test: run the backup into a folder of mode `0755` and into the live root, and assert the mode of both files and the two refusals or results.

**[HST-963]** `ilium plugin restore` MUST compare the `data_version` stamp of each bucket in the archive with the `data_version` of the plugin package that will be active after the restore, before it changes any file. A stamp higher than that version MUST be refused with `migration.data-newer` (exit 1) and MUST NOT be overridden by `--merge` or `--replace`; the restore succeeds only after a version with that data version is installed. A stamp lower than that version MUST be migrated by the update path after the data is written, and the host MUST take the pre-update backup of [LIF-107](/pages/guide-data-migration/) first. A plugin that is not installed and has no package in the archive MUST be refused with `restore.plugin-not-installed` (proposed, owner to confirm) and the message MUST name the plugin id. Test: restore an archive with stamp 3 into a profile whose plugin has data version 2, and assert the refusal and an unchanged listing; repeat with stamp 1, and with an uninstalled plugin and no package in the archive.

### Cloud-synced and roaming folders

The lock and flush rules of this page assume one machine and a local disk. A folder that a sync client copies to other machines breaks both assumptions, and a network filesystem is only part of that case ([HST-665](#production-additions-t10-operations) covers NFS, SMB and FUSE-remote). This section covers `<data>/plugins`, `<state>`, `<config>` and a project's `.ilium/` folder when any of them sits inside Dropbox, iCloud Drive, OneDrive, Google Drive, Syncthing or a Windows roaming profile.

**What goes wrong.** The risks below are properties of those products, not of Ilium, and the host cannot repair them after the fact.

- `installed.lock` and the per-bucket locks are OS advisory locks ([LIF-066], [LIF-120]). They exist only inside one machine's kernel. A sync client does not honour them, so two machines can each hold the lock and each write `installed.json` or `global.db` at the same time. The last upload wins and the other write is lost.
- The fsync rule of [LIF-118] means the change is on the local disk when `put` returns. It says nothing about the copy a sync client uploads. A client can upload a bucket file while the host is half-way through a write, and another machine then opens a torn file. The integrity check of [HST-657] moves such a file aside and starts an empty bucket, which loses the data.
- A sync client that sees two writers keeps both files and names the second one, for example `global (conflicted copy).db`, `installed (conflicted copy 2026-10-23).json` or `installed-DESKTOP-ABC.json`. Nothing reads these files, so the registry the host uses can be the older of the two.
- The `data_version` stamp of a bucket ([LIF-107]) can go backwards. A machine with an older plugin version syncs its bucket over a newer one, and the newer plugin then finds a lower stamp than it wrote. The host would migrate that bucket again.

**What the host does (proposed, owner to confirm).** At start, and again when a root is changed with `plugins.data_dir` ([HST-669](#production-additions-t10-operations)), the host checks each root for the signals below. A match is not proof of syncing, so the host never stops a plugin because of a match alone.

| Signal | Where it is looked for |
|---|---|
| `.dropbox`, `.dropbox.cache` | the root or any parent folder |
| `.icloud` placeholder files, or a path under `~/Library/Mobile Documents/` | the root or any parent folder |
| `desktop.ini`, or the Windows file attributes of OneDrive (`RECALL_ON_DATA_ACCESS`, `PINNED`, `UNPINNED`), or a path under `%OneDrive%`, `%OneDriveCommercial%` | the root or any parent folder |
| a path under the Google Drive mount (`G:\My Drive`, `~/Google Drive`, `~/Library/CloudStorage/GoogleDrive-*`) | the root path |
| `.stfolder`, `.stignore` | the root or any parent folder |
| a path under a Windows roaming profile (`%APPDATA%` that is the roaming `AppData\Roaming`, not `%LOCALAPPDATA%`) | the root path |

On a match the host:

1. logs `host.synced-folder` once per run and per root, naming the folder, the signal and the product it believes it saw;
2. keeps running, and shows a warning row on the Plugins screen that names the folder, says that locks and writes are not safe there and points to the alternative below. The row stays until the root moves or the signal is gone;
3. lists the finding in `ilium plugin doctor` and in the result of `ilium plugin du`.

The warning is not an error. Many users keep a project's `.ilium/` folder in a synced folder on purpose, and plugin settings in it are small text files that merge well.

**Conflict copies.** The host looks in each root for files that match the conflict naming of the products above next to `installed.json`, `trust.json`, `grants.json`, `consents.json`, `config.toml` and every `*.db` bucket. While a conflict copy of `installed.json` exists the host MUST refuse every write to `installed.json`, so install, update, uninstall, enable and disable fail with `install.registry-conflict` (proposed, owner to confirm) and exit code 4, with the message naming both files. Reads keep working and enabled plugins keep running. The user resolves it by comparing the two files, keeping one, deleting the copy and running `ilium plugin doctor`. A conflict copy of a bucket file is listed by `ilium plugin doctor` but does not stop writes, because the host opens the file with the exact name only.

**Supported alternative.** Keep all roots local, and do not point `plugins.data_dir` or `XDG_*` variables into a synced folder. Move data between machines with `ilium plugin backup` and `restore` ([HST-652](#production-additions-t10-operations), [HST-653](#production-additions-t10-operations)), or for one plugin with `ilium plugin export` and `import` ([LIF-290]). A synced folder can hold the archive file: the archive is written to `<file>.partial` and renamed, so the sync client sees a finished file. The archive must not be written inside `<data>/plugins/` ([HST-962](#backup-contents-consistency-and-restore-rules)).

Failure modes: a sync client that locks a file during upload makes the host's write fail with `io`, which the host retries as it does any `io` failure; a placeholder file (`.icloud`, OneDrive online-only) that is not downloaded reads as missing and the host reports `data.root-invalid` ([HST-669](#production-additions-t10-operations)) for a root, or opens an empty bucket for a bucket file after the integrity check fails.

### Which export to use: formats side by side

Five kinds of copy exist. They answer different questions and are not interchangeable.

| | Single-plugin archive | Whole-host archive | Bucket JSON lines | Privacy folder | Pre-migration copy |
|---|---|---|---|---|---|
| Command | `ilium plugin export <id> --out <file>` | `ilium plugin backup <file.tar.zst>` | `ilium plugin data export <id> <dir>` | `ilium plugin export-data` | written by the host before `migrate` |
| Counterpart | `ilium plugin import <id> --from <file>` ([LIF-292]) | `ilium plugin restore <file>` ([HST-653](#production-additions-t10-operations)) | `ilium plugin data import <id> <dir>` ([HST-656]) | `ilium plugin import-data <folder>` ([USR-262]) | restored by the host on a failed migration ([LIF-107]) |
| Format name | not yet named in the requirements; it carries a format version and an unknown one is refused with `import.format-unsupported` | `backup.json` with `format: "ilium-backup/1"` ([HST-654](#production-additions-t10-operations)) | JSON lines, one object per key, with `data_version` | `manifest.json` with format `1` and SHA-256 per file ([USR-323]) | none; a copy of the live layout |
| Holds | the plugin's data folder, its buckets of every scope, non-secret settings values | store, `installed.json`, `trust.json`, buckets, settings; grants with `--include-grants`; secrets only with `--include-secrets` | every bucket of one plugin: base64 key, base64 value, data version | settings, project storage references, grants, plugin logs, audit records, crash records, as readable files | global, project and session buckets and the data directory of one plugin at one version |
| Never holds | secrets in any form ([LIF-290]), grants, the package | cache, logs, staging, compile cache, `backups/`, secrets by default ([HST-960]) | package, settings, grants, files of the data folder | bucket bytes, the package, secrets | the package, settings, grants |
| Human can read it | no (archive) | no (archive) | partly: keys and values are base64 | yes | no |
| Checks on restore | id match, format version | every entry hash, host version, `data_version` ([HST-963](#backup-contents-consistency-and-restore-rules)) | `migration.data-newer` for a higher data version | target plugins installed, grants re-checked against machine policy | none; used by the update path |
| Other OS | not stated | data bytes unchanged, T2/T3 plugins of another platform refused at load ([HST-655](#production-additions-t10-operations)) | bytes are OS independent | files are OS independent | same machine only |
| Unknown format number | `import.format-unsupported` | `restore.format-unsupported` for an unknown major format ([HST-654](#production-additions-t10-operations)); `restore.host-newer` for a newer host unless `--force` | no format number is defined today | no refusal is defined today | not applicable |

**Rules of choice.**

- To move or restore a machine, use `backup` and `restore`. The privacy folder carries no bucket bytes and no package, so a machine moved with `export-data` loses plugin data.
- To move one plugin, use `export` and `import` for a full copy, or `data export` and `data import` when the target already has the plugin and only the buckets move.
- To read what the host holds about a user, or to answer a data-access request, use `export-data` ([USR-245]). It is a report, not a backup.
- The pre-migration copy is for the host. Do not copy it by hand, because the next committed update keeps only the newest ([LIF-107]).

**Stable interchange format.** [LIF-304] gives every on-disk format a number, but only the whole-host archive has a defined refusal for an unknown major number ([HST-654](#production-additions-t10-operations)). Third-party tools may therefore depend on `ilium-backup/1` and on `backup.json` fields (format, host version, creation time, OS, per-entry SHA-256), and on nothing else. The bucket JSON-lines layout, the privacy-folder file names and the single-plugin archive layout may change between host versions. Tools that need them must read the format number first and stop on a number they do not know.
