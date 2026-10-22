---
layout: page
title: Reference, install, update and uninstall
permalink: /pages/reference-install-update-uninstall/
---

## Reference, install, update and uninstall

This page specifies how a plugin package gets onto a machine, how it is replaced by a newer version, how an update is undone, and what is kept or deleted when it is removed. It also lists every `ilium plugin` command that manages installed plugins, with arguments, JSON line output, exit codes and error codes. The build and signing commands (`new`, `build`, `pack`, `keygen`, `sign`, `trust`) are in [Packages and manifests](/pages/packages/).

Contract version: `ilium:plugin@0.1.0`. A registry is optional (decision D27); see [Install and update](/pages/reference-install-and-update/) for the registry path. A package is a file, and every step on this page works offline ([offline and low disk](/pages/reference-offline-and-low-disk/)).

### Terms

- Package store: the directory that holds unpacked, verified packages. It is immutable after install.
- Registry: the file `installed.json`. It says which version of each plugin is active.
- Active version: the version the registry points to. Instances run from it.
- Staging: a private directory where a package is unpacked and checked before it enters the store.
- Data version: the number `data_version` in the manifest. It describes the layout of the data a plugin stores. See [Data migration](/pages/guide-data-migration/).

### On-disk layout

All paths are relative to the plugin root, `<data dir>/plugins/`, where `<data dir>` comes from the `directories` crate (`~/.local/share/ilium` on Linux). The cache directory is `<cache dir>/ilium/plugins/`.

```text
<data dir>/plugins/
  installed.json            registry, versioned format
  installed.lock            exclusive OS advisory lock; holder pid and boot id for messages
  store/<id>/<version>/     immutable package: package.iliumpkg, manifest.json, manifest.sha256, components, files
  staging/<uuid>/           uncommitted unpack of one operation
  locks/<id>.migrate.lock   cross-process lock of one data migration
  backups/<id>/<version>-<utc timestamp>/   pre-migration copy of data and buckets
  data/<id>/                the plugin data directory (fs data scope)
  storage/<id>/global.db    global bucket
<state dir>/plugins/logs/<session-slug>/<id>.log   plugin log (not under the plugin root)
<state dir>/trust.json      the user trust store, mode 0600 (not under the plugin root)
<cache dir>/ilium/plugins/
  <id>/                     the plugin cache directory
  compile/                  compiled components for T2 and T3, see offline and low disk
```

The user trust store is `$XDG_STATE_HOME/ilium/trust.json` (Linux `~/.local/state/ilium/trust.json` when `XDG_STATE_HOME` is unset), mode 0600, written atomically. Its format is in [Signing and trust](/pages/reference-signing-and-trust/#the-trust-store). The variable `ILIUM_TRUST_STORE` replaces the path with an absolute file path. In `backups/`, `<version>` is the version that was active before the update, and the timestamp is `YYYYMMDDTHHMMSSZ` in UTC, for example `backups/com.example.notes/0.3.0-20261012T091404Z/`.

The project and session buckets live with the project, under `<project>/.ilium/`. See [scope and sessions](/pages/reference-plugin-scope-and-sessions/).

### The registry file

`installed.json` is a JSON object that carries its own format string, so that a future host can read an old file. It is serialised with 2-space indentation, LF line endings and one final LF. The array `plugins` is sorted by `id` in byte order. This page owns the schema; every other page links here.

```json
{
  "format": "ilium-installed/1",
  "plugins": [
    {
      "id": "com.example.notes",
      "active_version": "0.3.1",
      "previous_version": "0.3.0",
      "state": "installed-enabled",
      "source": "registry",
      "source_url": "https://registry.example.org/",
      "channel": "stable",
      "publisher_id": "com.example",
      "key_id": "46ef773ed515997e3f107ae73619e6a31921c98f96d468eb858a02b48d5ae011",
      "package_sha256": "807e3b71e81cd569717e02318607832667fac642717e8b0e0eeb2426e030a34c",
      "manifest_sha256": "7ff76366f824af4ec0b54fdee14ff3661b38567668e38f90c13920662f05deea",
      "pinned": false,
      "skip": [],
      "declined": [],
      "failure_stopped": [],
      "installed_at": "2026-10-12T09:14:04Z",
      "update": null
    }
  ],
  "purge_tombstones": []
}
```

| Key | Type | Meaning |
|---|---|---|
| `id` | string | The plugin id. |
| `active_version` | string | The version the host runs. |
| `previous_version` | string or null | The one version that rollback returns to. |
| `state` | string | The stored package state: `installed-enabled`, `installed-disabled-by-user`, `disabled-untrusted`, `disabled-blocked` or `uninstall-pending`. The other package states are derived at load. See [plugin lifecycle states](/pages/reference-plugin-lifecycle-states/#package-states). |
| `source` | string | `bundled`, `file`, `url`, `registry` or `dev`. An install from an offline bundle records `file`. |
| `source_url` | string or null | The URL or registry address the package came from. |
| `channel` | string | `stable`, `beta`, `dev` or `file`. |
| `publisher_id` | string or null | The publisher id bound to the signing key; null for T1 and T3. |
| `key_id` | string or null | The key id of the signer: 64 lowercase hexadecimal characters; null for T1 and T3. |
| `package_sha256` | string | SHA-256 of the package file, 64 lowercase hexadecimal characters. |
| `manifest_sha256` | string | SHA-256 of the packed `manifest.json`, 64 lowercase hexadecimal characters. The host compares it at every load. |
| `pinned` | bool | The user pinned the version; update does not offer a newer one. |
| `skip` | list of strings | Versions that update does not offer. |
| `declined` | list of strings | Versions whose review the user declined. The declined grants themselves are in its entry in `grants.json`. |
| `failure_stopped` | list of strings | The sides stopped by the third counted failure. The only value in 0.1.0 is `server`. |
| `installed_at` | string | RFC 3339 in UTC with seconds precision and the suffix `Z`. |
| `update` | object or null | An update in progress: `{ "candidate", "state", "attempts", "started_at" }`. `state` is `staged`, `migrating` or `trial`. `attempts` is an integer from 0 to 2. `started_at` has the form of `installed_at`. |
| `purge_tombstones` | list (top level) | Plugin ids whose project and session data must still be deleted. |

The data version of each bucket is stamped with the bucket, not in this file; see [Data migration](/pages/guide-data-migration/). The tier is not stored; the host computes it again at every load from `key_id` and the trust store.

The JSON Schema of the file:

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "ilium-installed/1",
  "type": "object",
  "additionalProperties": false,
  "required": ["format", "plugins", "purge_tombstones"],
  "properties": {
    "format": { "const": "ilium-installed/1" },
    "plugins": { "type": "array", "items": { "$ref": "#/$defs/plugin" } },
    "purge_tombstones": { "type": "array", "items": { "type": "string" } }
  },
  "$defs": {
    "sha256": { "type": "string", "pattern": "^[0-9a-f]{64}$" },
    "utc": { "type": "string", "pattern": "^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$" },
    "plugin": {
      "type": "object",
      "additionalProperties": false,
      "required": ["id", "active_version", "previous_version", "state", "source",
                   "source_url", "channel", "publisher_id", "key_id", "package_sha256",
                   "manifest_sha256", "pinned", "skip", "declined", "failure_stopped",
                   "installed_at", "update"],
      "properties": {
        "id": { "type": "string" },
        "active_version": { "type": "string" },
        "previous_version": { "type": ["string", "null"] },
        "state": { "enum": ["installed-enabled", "installed-disabled-by-user",
                            "disabled-untrusted", "disabled-blocked", "uninstall-pending"] },
        "source": { "enum": ["bundled", "file", "url", "registry", "dev"] },
        "source_url": { "type": ["string", "null"] },
        "channel": { "enum": ["stable", "beta", "dev", "file"] },
        "publisher_id": { "type": ["string", "null"] },
        "key_id": { "oneOf": [{ "$ref": "#/$defs/sha256" }, { "type": "null" }] },
        "package_sha256": { "$ref": "#/$defs/sha256" },
        "manifest_sha256": { "$ref": "#/$defs/sha256" },
        "pinned": { "type": "boolean" },
        "skip": { "type": "array", "items": { "type": "string" } },
        "declined": { "type": "array", "items": { "type": "string" } },
        "failure_stopped": { "type": "array", "items": { "enum": ["server"] } },
        "installed_at": { "$ref": "#/$defs/utc" },
        "update": {
          "oneOf": [
            { "type": "null" },
            { "type": "object", "additionalProperties": false,
              "required": ["candidate", "state", "attempts", "started_at"],
              "properties": {
                "candidate": { "type": "string" },
                "state": { "enum": ["staged", "migrating", "trial"] },
                "attempts": { "type": "integer", "minimum": 0, "maximum": 2 },
                "started_at": { "$ref": "#/$defs/utc" }
              } }
          ]
        }
      }
    }
  }
}
```

- [LIF-065] The host MUST write `installed.json` atomically: write a temporary file in the same directory, fsync it, rename it over the old file, then fsync the directory. Test: kill the writer at each step (fault injection) and assert that a reader sees the old or the new file, never a partial one.
- [LIF-066] `installed.lock` is an exclusive OS advisory lock (flock on Unix, LockFileEx on Windows, through the `file_lock` module of `ilium-platform`) on `<data dir>/plugins/installed.lock`. The file also holds the holder's process id and boot id, for messages only. The host MUST hold the lock while it changes the store or the registry. A crashed holder releases the lock by process exit, so no takeover rule exists. A second writer MUST poll every 100 ms for up to 30 s, then fail with `install.locked` and exit code 4. Test: hold the lock and start an install; kill the holder with SIGKILL and assert that the next install gets the lock at once.
- [LIF-067] The host MUST refuse a registry whose `format` is a higher number than it knows (for example `ilium-installed/99`), with `install.registry-format`, and MUST NOT rewrite it. Test: set `format` to `ilium-installed/99`.
- [LIF-282] The host MUST validate `installed.json` against the schema above at start and refuse a file that does not match it with `install.state-corrupt`, without rewriting it. Test: write one file per property with a wrong type, an extra key, a short `key_id` and a local-time `installed_at`, and assert `install.state-corrupt` for each.

### The install pipeline

`ilium plugin install <file.iliumpkg>` runs these steps in order. This table groups the 11 numbered steps of SEC-100 into 7 rows; other pages cite SEC-100 step numbers (policy is SEC-100 step 7, commit is step 11), so refer to steps by name. A failure at any step stops the pipeline, removes what the pipeline created and leaves the registry unchanged. Step 2 is the validation order of [Package format](/pages/reference-package-format/#the-validation-order) (PKG-149); this page does not define a second order.

| Step | Action | On failure | Code |
|---|---|---|---|
| 1 | Take `installed.lock`. | Wait, then give up. | `install.locked` |
| 2 | Run validation stages 1 to 11: archive, manifest-parse, schema, semantic, inventory, components, signature, trust, policy, dependencies, review. Stage 11 shows the grant review and records the answers. | Nothing is written yet. | the codes of the failing stage in the [catalogue](/pages/reference-manifest-schema/#error-catalogue) |
| 3 | Unpack to `staging/<uuid>/`. | Delete the staging directory. | `install.io`, `install.no-space` |
| 4 | Prepare the components: T1 ships precompiled code; T2 and T3 are compiled into the compile cache. | Delete the staging directory. | `install.compile-failed` |
| 5 | Rename the staging directory to `store/<id>/<version>/`. Write `manifest.sha256`. | Delete the staging directory. | `install.io` |
| 6 | Update `installed.json` atomically (see LIF-065). Write the plugin's entry in `grants.json`. | Delete the new store entry. | `install.io` |
| 7 | Tell the running servers (see Propagation). Release the lock. | A failed notification is a warning only. | `install.notify-failed` |

- [LIF-068] The host MUST NOT instantiate or run any code of the package before SEC-100 step 11 (commit) has finished. Test: install a package whose start function writes a file and decline the review; assert the file is absent.
- [LIF-069] A failure at any step MUST leave the registry byte for byte unchanged and MUST delete the staging directory and any new store entry. Test: inject a failure at each step and compare the registry hash.
- [LIF-070] An install MUST NOT require network access. Test: run the install in a network namespace with no interfaces.
- [LIF-071] Validation MUST run in the stage order of PKG-149, so that the cheapest checks refuse first and no prompt appears for a package that would be refused later. Test: a package with an invalid manifest (stage 3) and an invalid signature (stage 7) reports `manifest.schema` and no signature code.
- [LIF-072] Installing the same id and version with the same manifest SHA-256 MUST be a no-op that exits 0 and prints `install.already-installed` as a `warning` line; with `--reinstall` it rewrites `store/<id>/<version>/` from the package. A different manifest SHA-256 for the same id and version MUST be refused at stage 9 with `package.version-reused` (exit 1). The host compares the manifest hash and not the package hash, because the signature document changes the package bytes. Test: install a package twice, then a modified copy with the same version, then the same manifest signed again.

Grant review in a non-interactive run (no TTY): normal grants are accepted only with `--yes`; a high-risk grant is accepted only when named with `--grant <name>`. Without them the install ends with `install.review-required` and exit code 1 (PKG-430).

### Update

A higher version of an installed id is an update. It is the same pipeline, committed beside the old version, followed by a swap. The candidate lives in `store/<id>/<version>/` from step 5 on, so the waiting period never depends on `staging/`.

1. Steps 1 to 5 run as for an install. The dialog shows only the new capabilities. The package state is `update-staged` only when the new version asks for grants beyond those the user accepted, and stays so until the review is answered (LIF-078); otherwise the update goes on to step 2.
2. Step 6 writes `previous_version` and `update: { candidate, state: "staged", attempts: 0, started_at }`, and leaves `active_version` unchanged.
3. Each running server receives the change. At the next safe point ([Install and update](/pages/reference-install-and-update/), PKG-278) each live instance receives `deactivate(upgrade)` and returns its restore bytes; the swap waits for every instance to return ([LIF-044](/pages/lifecycle/)).
4. Before the host calls `migrate`, it writes `update.state: "migrating"` to `installed.json` with fsync. The data migration runs ([Data migration](/pages/guide-data-migration/)). If the host stops while the state is `migrating`, the next start restores the backup, increments `attempts` and runs the migration again; when `attempts` reaches 2 the host rolls back with `migration.interrupted` and marks the candidate `skip`.
5. The host sets `update.state` to `trial` and the server activates the candidate. The context carries `restored`, unless the data version changed, in which case the bytes are dropped.
6. The candidate becomes `active_version` only after its first `activate` returns `ok` on the server side. Then the host sets `active_version` to the candidate, sets `previous_version`, sets `update` to `null` and logs `update.ok`. The update is committed.
7. If that first `activate` fails, traps or misses its deadline, the host rolls back at once: it restores the data snapshot if a migration ran, reactivates the previous version and sets `update` to `null`. It marks the candidate `skip`, logs `update.rolled-back`, shows the toast `Update failed, rolled back`, and does not count this failure in the failure window.

Failures after a successful first activation follow the normal failure window ([Lifecycle](/pages/lifecycle/#failure-counting-and-restart)) and never roll back by themselves.

- [LIF-073] The first server to apply an update MUST decide it. After step 6, a failure on another server MUST follow the normal failure rule and MUST NOT roll back. Test: update with two servers running, fail the second server's activation and assert the version stays.
- [LIF-074] An update MUST be atomic: after any interruption, either the old or the new version is active, never neither. Test: kill the installer or the server at each step and assert that a plugin version is active after restart.
- [LIF-075] The candidate becomes `active_version` only after its first `activate` returns `ok` on the server side. If that first `activate` fails, traps or misses its deadline, the host MUST roll back at once to the previous version and its pre-update data snapshot, mark the candidate `skip`, log `update.rolled-back`, and not count the failure in the failure window. A failure after a successful first activation MUST follow the normal failure window and MUST NOT roll back. Test: install a candidate whose `activate` returns an error and assert the failure window of the plugin is empty; then fail a later call and assert no rollback.
- [LIF-076] The host MUST keep exactly one previous version in `store/` and delete older ones after a committed update. Test: update three times and list `store/<id>/`.
- [LIF-077] A blocked candidate MUST NOT be tried again unless the user installs it again explicitly. Test: roll back, restart the server and assert that no activation of the candidate occurs.
- [LIF-078] An update MUST NOT send grants to the new version beyond those the user accepted. A new version that asks for more is `update-staged` until the review is answered. Test: update with an extra grant and decline.

Disk cost: the store holds the active and the previous version, so a plugin uses about twice its package size, plus the newest pre-migration backup, which lasts until the next committed update.

If activation of the new version is deferred (a lazy plugin has no trigger yet), the update stays in `trial` until the first activation. The previous version is not removed in that time.

### Rollback and downgrade

`ilium plugin rollback <id>` makes the previous version active. `ilium plugin install --allow-downgrade <file>` installs a lower version than the active one. Both follow the same data rule.

- [LIF-079] Activating a version whose `data_version` is lower than the stamp of any bucket in scope MUST be refused with `migration.data-newer` (stage I, exit 1), unless `--restore-backup` or `--discard-data` is given. This applies to downgrade and rollback. The user can pass `--restore-backup`, which restores the data snapshot taken before the update and loses what was written since, or `--discard-data`, which deletes the plugin's buckets and data directory. Test: update to a version that migrates data, write data, roll back without a flag and assert `migration.data-newer`; repeat with each flag.
- [LIF-080] `--restore-backup` MUST fail with `rollback.no-backup` when the snapshot is gone. Test: delete the backup and roll back with the flag.
- [LIF-081] Rollback MUST send `deactivate(upgrade)` to the running instances and activate the previous version with the same rules as an update. Test: read the reasons in the log.

### Uninstall

`ilium plugin uninstall <id>` removes a plugin. The table says what is kept and what is deleted.

| Item | Default uninstall | With `--purge` | With `--keep-data` |
|---|---|---|---|
| Store entries (all versions) | deleted | deleted | deleted |
| Data directory | kept | deleted | kept |
| Cache directory and compile cache | deleted | deleted | kept |
| Global bucket | kept with the data directory | deleted | kept |
| Project bucket | kept | deleted for this project, tombstone for others | kept |
| Session bucket | kept until its session is deleted | deleted for this session, tombstone for others | kept |
| Shared bucket | kept | deleted | kept |
| Settings values | kept | deleted | kept |
| Grants and first-use answers | deleted | deleted | deleted |
| Node metadata in snapshots | kept, inert | deleted for the live session, tombstone for others | kept, inert |
| Persistent decorations | removed | removed | removed |
| Durable timers | deleted | deleted | deleted |
| Backups `backups/<id>/` | kept | deleted | kept |
| Migration locks `locks/<id>.*` | kept | deleted | kept |
| Journal entries of the plugin | kept until normal expiry | kept until normal expiry | kept until normal expiry |
| Trusted publisher keys | kept | kept | kept |
| User extension order entries | deleted | deleted | deleted |
| Failure counters | deleted | deleted | deleted |
| Log files | kept 7 days, then deleted | deleted | kept 7 days |

- [LIF-082] An uninstall without `--purge` MUST keep the data directory, the buckets, the settings values and `backups/<id>/`, with or without a TTY, and MUST NOT ask a question. It prints the kept paths as `warning` lines. `--keep-data` is accepted and differs from the default only in the cache directory, the compile cache and the log files (see the table above). Test: run it with standard input closed and again on a TTY, and assert no prompt and the warning lines.
- [LIF-083] The host MUST delete grants (its entry in `grants.json`), first-use answers, durable timers, failure counters and persistent decorations on every uninstall, with or without `--purge`. Data, storage and settings are kept unless `--purge` is given. A reinstall MUST ask for grants again. Test: install, grant, uninstall, install and assert the review dialog appears.
- [LIF-084] Node metadata and session data of an uninstalled plugin MUST stay inert: the host never passes them to another plugin, and keeps them in the snapshot. A reinstall of the same id MAY read them again. With `--purge` the host MUST delete them in the live session and add a tombstone, so that other projects delete theirs when they next open. Test: purge, reopen another project and assert the keys are gone.
- [LIF-290] The host MUST provide `ilium plugin export <id> --out <file>`, which writes one archive holding the plugin's data directory, buckets and settings values for the current project scope. The archive MUST NOT contain grants, first-use answers, trusted keys, logs or the package itself. It MUST work while the plugin is enabled or disabled and MUST NOT change any plugin data. Test: export an installed plugin, list the archive and assert the data, bucket and settings entries and none of grants, keys, logs or package files; compare the plugin data before and after and assert no change.
- [LIF-291] `ilium plugin export` MUST write a consistent snapshot: no bucket batch (a `storage.batch` call) appears half applied. It MUST create the file owner-only, MUST refuse an existing file with `export.exists` unless `--force` is given, and MUST fail with `export.unknown-plugin` for an id that is not installed, leaving no partial file. Test: export during a running batch and assert all or none of its keys; export twice to one path and assert `export.exists`, then again with `--force`; export an unknown id and assert the error and no file; assert the file mode is 0600.
- [LIF-292] `ilium plugin import <id> --from <file>` MUST restore an archive made by `ilium plugin export` into the installed plugin with the same id, replacing its data, buckets and settings values. It MUST refuse an archive for another id with `import.id-mismatch` and an archive of an unknown format version with `import.format-unsupported`, and then MUST leave the existing data untouched. Test: export on one root, install the same id on a second root, import and read the keys back; import an archive of another id and one with a bumped format and assert the error codes and unchanged data.
- [LIF-293] A `ilium plugin uninstall <id> --purge` on a TTY MUST print one line before it deletes, naming `ilium plugin export <id>` as the way to keep a copy. Without a TTY it MUST print no such prompt and MUST NOT ask a question (LIF-082). The line MUST NOT delay or block the purge. Test: run the purge on a TTY and assert the hint line and the purge; run it with standard input closed and assert no prompt and the purge.
- [LIF-085] Uninstall MUST first set `uninstall-pending`, then stop all instances, then delete files. After a crash, the next start MUST finish it. Test: kill the host after the state is written.
- [LIF-086] Uninstall MUST NOT delete data of other plugins, and MUST NOT remove a publisher key. Test: compare the other plugins' directories before and after.

### Command reference

All commands print JSON lines when output is not a terminal, or when `--json` is given. Every line has a `type`: `progress`, `result`, `warning`, `error` or `summary`. Paths are explicit fields.

| Command | Purpose |
|---|---|
| `ilium plugin install <source> [--allow-unsigned] [--allow-downgrade] [--trust <key_id>] [--channel stable\|beta\|dev] [--yes] [--grant <name>]... [--locked] [--no-backup] [--unskip]` | Install or update from a package file, URL or registry id. `--unskip` clears the `skip` mark that a rollback set on that version. Full flag list: [Distribution commands](/pages/reference-plugin-distribution-cli/#install). |
| `ilium plugin update <file>` | Same as install, but fails with `install.not-installed` when the id is not installed. |
| `ilium plugin uninstall <id> [--purge] [--keep-data]` | Remove a plugin. The two flags exclude each other. |
| `ilium plugin enable <id> [--scope <scope>] [--session <name>]` | Turn a plugin on. |
| `ilium plugin disable <id> [--scope <scope>] [--session <name>]` | Turn a plugin off. |
| `ilium plugin list [--json]` | List plugins and states. |
| `ilium plugin info <id> [--json]` | Show one plugin: manifest summary, grants, state, health. |
| `ilium plugin rollback <id> [--restore-backup] [--discard-data]` | Activate the previous version. |
| `ilium plugin reinstall <id>` | Repair the store entry from the cached package. |
| `ilium plugin logs <id> [--tail <n>]` | Print the plugin log. |
| `ilium plugin reset-failures <id> [--session <name>]` | Clear the failure window and the failure stop. |
| `ilium plugin status [<id>] [--json]` | Print health and metrics. See [health and watchdog](/pages/reference-health-and-watchdog/). |
| `ilium plugin stats <id> [--json]` | Print resource usage. See [resource accounting](/pages/reference-resource-accounting/). |
| `ilium plugin bisect start\|good\|bad\|reset` | Find a bad plugin. See [safe mode](/pages/user-safe-mode-and-troubleshooting/). |

Exit codes are those of the one table in [Distribution commands](/pages/reference-plugin-distribution-cli/#exit-codes): 0 ok, 1 failed, 2 usage, 3 findings, 4 environment or locked, 5 not found, 6 no session, 7 policy, 8 network, 130 interrupted. This page keeps no table of its own. A declined review exits 1 with `install.review-declined`.

JSON line examples:

```text
{"type":"progress","step":"validate","message":"validating package","current":3,"total":7}
{"type":"warning","code":"install.notify-failed","message":"session /home/u/proj not reached","session":"default"}
{"type":"result","id":"com.example.notes","version":"0.3.1","previous":"0.3.0","state":"installed-enabled","tier":"T2","action":"updated","active_from":"now"}
{"type":"error","code":"manifest.file-hash","message":"files/panel.wasm hash differs","path":"files/panel.wasm"}
{"type":"summary","ok":false,"errors":1,"warnings":0}
```

- [LIF-087] Every command MUST print only JSON lines on standard output when output is not a terminal, MUST use the exit codes of the table in [Distribution commands](/pages/reference-plugin-distribution-cli/#exit-codes) and MUST give each error line a `code` from the [error catalogue](/pages/reference-manifest-schema/#error-catalogue). Test: run each command in a pipe and parse every line.
- [LIF-088] `enable` and `disable` MUST be idempotent: they exit 0 and print a `result` line with `changed: false` when nothing changes. Test: disable twice.

### Propagation to running servers

Ilium runs one server per project session, so one install can reach many servers.

- [LIF-089] A running server MUST apply an `installed.json` change within 2 s with a file watcher, and within 6 s when watching is unavailable (polling every 5 s). The installing CLI MUST also send a `plugins-changed` message to every server it can find in the runtime directory; a server it cannot reach is reported as a `warning` line with `install.notify-failed` and picks the change up at its watch or at its next start. Test: install with two servers, assert both log `registry.changed` within 2 s with the watcher on and within 6 s with the watcher off.
- [LIF-090] A server that is not running MUST use the registry as it finds it at start. Test: install, then start a server and assert the new version is active.
- [LIF-091] A change MUST be applied to a server under the per-plugin serialisation of [lifecycle states](/pages/reference-plugin-lifecycle-states/). Test: install an update during a disable.

### Install or update while the plugin is active in many sessions

Every server swaps its own instances, as in the update steps. Their order is not defined, and the first to finish decides (LIF-073). A session that cannot swap yet because the plugin is busy in a call waits at most the deactivate deadline, then drops the instance. A session on another project whose plugin data is shared (the global bucket) takes the migration lock, as [Data migration](/pages/guide-data-migration/) describes.

### Interrupted install and recovery

- [LIF-092] At start, and at the start of every `ilium plugin` command that takes the lock, the host MUST delete `staging/` directories whose creating process is gone or that are older than 1 hour. `installed.json` never references `staging/`: a staged update lives in `store/<id>/<version>/` (step 5) and is never touched by this rule. Test: create an orphan staging directory and start a server; stage an update, wait past 1 hour and assert the candidate is still in `store/`.
- [LIF-093] A store entry that no field of `installed.json` references (`active_version`, `previous_version` or `update.candidate`) and that is older than 1 hour MUST be deleted at start. A registry entry whose store entry is missing or fails its hash check MUST put the plugin in `refused-at-load` with `package.corrupt`. `ilium plugin reinstall <id>` MUST repair it from the cached package or fail with `package.cache-missing`. Test: truncate a file in the store.
- [LIF-094] After a power loss the host MUST find either the old or the new registry, because of LIF-065, and MUST finish or undo an `update` in `staged`, `migrating` or `trial` by the rules above at the next activation. Test: kill the machine at step 6 (simulated) and restart.

### Error codes

The catalogue in [Manifest schema and error catalogue](/pages/reference-manifest-schema/#error-catalogue) is the only list of codes. This table names the codes that this page raises and the check that provokes each.

| Code | Meaning | Test hint |
|---|---|---|
| `package.manifest-missing` | The archive has no `manifest.json`. | Remove `manifest.json` from a package. |
| `package.entry-unexpected` | An entry is outside the allowed layout. | Add `extra.txt` at the root. |
| `manifest.file-hash` | A file hash differs from the inventory. | Flip a byte in a component. |
| `package.corrupt` | The store entry fails its hash check at load. | Truncate a stored component. |
| `package.cache-missing` | Reinstall has no cached package. | Delete the cache, run reinstall. |
| `manifest.signature-invalid` | The signature does not match. | Edit the manifest after signing. |
| `trust.key-unknown` | The signer is not trusted and the user declined or could not answer. | Install a package of a new key without `trust add`. |
| `trust.key-revoked` | The signer key is on the revoked list. | Add the key to the revoked list. |
| `policy.tier-denied` | The tier is not allowed on this machine by policy. | Deny T3 in the policy file. |
| `package.api-unsupported` | The package API window does not include this host. | Set `api` to a window above the host. |
| `manifest.service-cycle` | The non-optional `requires.services` entries form a cycle. | Install a cycle. |
| `policy.blocked` | The administrator policy forbids the plugin. | Block the id in the policy file. |
| `install.review-declined` | The review was declined. | Answer No in the review dialog. |
| `install.needs-input` | A decision is needed and no person can answer. | Run with no TTY and no `--yes`. |
| `install.locked` | The install lock is held for more than 30 s. | Hold the lock. |
| `install.io` | A file operation failed. | Make the store read-only. |
| `install.no-space` | Too little free space before writing. | Fill the volume. |
| `install.compile-failed` | The component could not be compiled. | Install a component with invalid code. |
| `install.not-installed` | `update` was used for an id that is not installed. | Update an unknown id. |
| `install.already-installed` | Warning: same id, version and manifest hash. | See LIF-072. |
| `package.version-reused` | Same id and version, different manifest hash. | See LIF-072. |
| `install.registry-format` | The registry format is newer than the host. | See LIF-067. |
| `install.state-corrupt` | `installed.json` does not match its schema. | See LIF-282. |
| `install.notify-failed` | A running server could not be told. | Remove the socket of a server. |
| `install.no-previous` | There is no earlier version for a rollback. | Roll back a first install. |
| `migration.data-newer` | The data is newer than the target supports. | See LIF-079. |
| `migration.interrupted` | The migration was interrupted twice. | See LIF-110. |
| `rollback.no-backup` | The pre-update snapshot is gone. | See LIF-080. |
| `update.rolled-back` | Warning: the candidate failed and the host returned. | See LIF-075. |

### Edge cases

| Situation | What happens |
|---|---|
| Install of a package while the user edits grants in Settings | The install waits on the lock for the registry, up to 30 s. |
| Install of an id that exists in the store but not in the registry | Treated as a new install; the old store entry is replaced. |
| Update to a version that the policy blocks | `policy.blocked`, nothing changes. |
| Rollback with no previous version | `install.no-previous`, exit code 5. |
| Uninstall of the plugin that provides a service others require | The consumers stay active and get `not-found`. The CLI prints a `warning` per dependant. |
| Disk full during step 3 or step 5 | `install.no-space` before writing, or `install.io` during it. The staging directory is removed. See [offline and low disk](/pages/reference-offline-and-low-disk/). |

### What the user sees

A successful install adds the plugin to Settings > Plugins. A failed install prints its errors and runs no plugin code. A rolled-back update shows the toast `Update failed, rolled back` and keeps the old version. The screens are described in [Managing plugins](/pages/user-managing-plugins/).

## Concurrent install, upgrade and uninstall

Two `ilium` processes can change the same plugin at once, for example two CLI calls or a CLI call and a running server.

- [EXT-060] Every install, upgrade, rollback and uninstall of a plugin id MUST take an exclusive lock on that id in the plugin store before it reads the current state. A second operation on the same id MUST wait up to 30 s and then fail with `install.busy`; operations on different ids MUST NOT block each other.
- [EXT-061] A process that holds the lock MUST release it when it exits. A lock whose holder is gone MUST be taken over by the next caller, and the takeover MUST be logged as `install.stale-lock`.
- [EXT-062] The store MUST show either the old or the new version of a plugin to a reader, never a mixture. An uninstall that races an upgrade MUST end with the plugin either fully removed or fully at the new version, and the audit log MUST record the order in which the operations ran.

### See also

- [Plugin lifecycle states](/pages/reference-plugin-lifecycle-states/)
- [Lifecycle](/pages/lifecycle/)
- [Data migration](/pages/guide-data-migration/)
- [Packages and manifests](/pages/packages/)
- [Security model](/pages/security/)
- [Plugin scope and sessions](/pages/reference-plugin-scope-and-sessions/)
- [Requirements, lifecycle and operations](/pages/reference-requirements-lifecycle-ops/)

API version checked: `ilium:plugin@0.1.0`.
