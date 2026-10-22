---
layout: page
title: Guide, data migration
permalink: /pages/guide-data-migration/
---

## Guide, data migration

A plugin that stores data will one day change how it stores it. A key gets a new name, a blob is split in two, a file moves. This guide tells you how to ship that change safely: the data version, the `migrate` export, the backup that the host takes first, what happens when the migration fails, and how a downgrade is handled. It is normative, and every rule has a requirement id.

Contract version: `ilium:plugin@0.1.0`.

### The mechanism in one paragraph

The manifest declares a number, `data_version`. The host remembers which data version each bucket and the data directory were last written with. When a new version of the plugin starts with a higher `data_version`, the host takes a backup, calls the optional export `migrator.migrate(from, to)` once per bucket (the global bucket and the data directory at the first activation, each project or session bucket when its session first activates the plugin, LIF-104), stamps the new number when it succeeds, and only then calls `activate`. If `migrate` fails, the host restores the backup and keeps the old version. A plugin that never changes its storage layout declares nothing and needs none of this.

You will need:

- A plugin that stores data in `storage`, in its data directory or on tree nodes ([storage](/pages/api-storage/), [file access](/pages/file-access/), [node metadata](/pages/capability-tree-metadata/)).
- The manifest key `data_version` and the optional export `migrator`, listed in `exports`. See [Reference: manifest](/pages/reference-manifest/).
- The `FakeHost` of [Testing plugins](/pages/testing/) for the tests below.

### What is versioned

| Data | Versioned | How it is migrated |
|---|---|---|
| Storage buckets `global`, `project`, `session` | yes, each bucket has its own stamp | by `migrate`, with `storage` |
| The `shared` bucket | no | it is a cache; the host clears it when `data_version` changes |
| The data directory | yes, one stamp | by `migrate`, with `fs` on the data scope |
| Node metadata the plugin owns | yes, one stamp per snapshot | by `migrate`, with `tree.metadata` and `tree.read` |
| Persistent decorations | no | the plugin sets them again after `activate`; the old ones are shown stale until then |
| Settings values | no | the kernel ignores a value that no longer fits its row and lists it on the recovery page. A plugin may copy its own values in `migrate` with the `settings` import. See [settings extension](/pages/settings-extension/) |
| Restore bytes | tagged | dropped when `data_version` differs from the version that wrote them (LIF-103) |
| Durable timers and journal entries | no | they keep their names and ids |

- [LIF-100] The manifest key `data_version` MUST be an unsigned 32-bit integer. When absent it MUST be 1. A value of 0 or a non-integer MUST be refused with `manifest.data-version`. Test: install manifests with 0, 1, "2" and 4294967295.
- [LIF-101] The host MUST store the data version of each bucket and of the data directory in a place the plugin cannot read or write, and MUST stamp it only after a successful migration or on first creation. Test: read the stamp through the CLI `ilium plugin info <id> --json` after a migration.
- [LIF-102] The host MUST NOT migrate data it did not stamp itself. A bucket with no stamp (created before the plugin ever stored data) MUST be stamped with the current `data_version` at first write. Test: first `put` on a new plugin.
- [LIF-103] The host MUST drop restore bytes when the `data_version` of the running version differs from that of the version that produced them, and MUST log `restore.dropped-data-version`. Test: change `data_version` between two builds and read `restored`.

### The `migrator` export

```wit
// Excerpt of ilium:exports/migrator in reference-wit
interface migrator {
  migrate: func(from: u32, to: u32) -> result<_, plugin-error>;
}
```

`from` is the lowest data version found among the buckets and the data directory the plugin can see in this run. `to` is the new `data_version`. The call has the same imports and grants as `activate`. It runs on a migration instance, before `activate`, and `activate` runs on a fresh instance afterwards.

| Item | Value |
|---|---|
| Deadline | hard, default 30,000 ms; T1 may request up to 60,000 (`migrate_deadline_ms`) |
| CPU | charged to the plugin's execution quota like any call |
| Memory | the plugin's `memory_mib` |
| Side | each side that exports `migrator`, server first; a client side runs at its first attach after the update |
| Skipped when | `data_version` is unchanged, or the plugin has no `migrator` (with no `migrator` the host stamps the new number, drops the restore bytes, clears the `shared` cache bucket and logs `migration.skipped`) |

- [LIF-104] For each bucket, the host MUST call `migrate(from, to)` at most once per update, the first time a host instance activates with that bucket in scope and the bucket stamp is lower than `data_version`, with `from` less than `to`. The global bucket and the data directory migrate at the first activation after the swap. A project or session bucket migrates when its session first activates the plugin. Test: update from 1 to 3, open two sessions, and assert one call `(1, 3)` for the global bucket and one for each session bucket.
- [LIF-105] A call that returns an error, traps or misses the deadline MUST be a migration failure (LIF-108). It MUST NOT count toward the failure window of the plugin. Test: return an error from `migrate` and read `failures-in-window`.
- [LIF-106] `migrate` MUST be idempotent. The host MAY call it again with the same arguments after an interrupted run (LIF-110). Test: run the call twice on a fixture and compare the data.

### Backup and failure

Before `migrate`, the host takes a backup of the buckets in scope and the data directory.

- [LIF-107] Unless backups are disabled by `migration_backup_mib = 0` or `--no-backup`, the host MUST copy the global, project and session buckets in use and the data directory to `backups/<id>/<version>-<utc timestamp>/` before it calls `migrate`. The timestamp has the form `YYYYMMDDTHHmmssZ`. The host MUST refuse with `install.no-space`, before touching any data, when free space is less than twice the data size plus 100 MiB. This data check comes after the package check of the install or update, which needs three times the package size plus 64 MiB (Offline and low disk). The backup size is capped by the key `migration_backup_mib` in the `[plugins]` table of `config.toml` (integer 0 to 1048576, default 2048; 0 disables backups). Data above the cap is refused with `migration.backup-too-large` (stage I, exit 1) unless `--no-backup` is given, and then a failed migration cannot be undone. A copy-on-write copy MAY replace the full copy. Test: fill the disk and update; set the cap to 1 and update with 2 MiB of data, with and without `--no-backup`.
- [LIF-108] On a migration failure the host MUST restore the backup, keep the previous version active, log `migration.failed` with the plugin's error code and message, and mark the update rolled back. Test: fail `migrate` after it wrote half the keys, then compare the buckets with the backup.
- [LIF-109] After an update that succeeded and was activated, the host MUST keep only the newest backup of that plugin and delete older ones in `backups/<id>/`. Test: update twice and list the backups.

An interrupted migration (power loss, kill) leaves the registry entry `update.state: "migrating"`.

- [LIF-110] At the next start the host MUST restore the backup, increment `update.attempts` in `installed.json` and run the update again from the beginning. The host MUST write `update.state: "migrating"` and the attempt count with fsync before it calls `migrate`. When `attempts` reaches 2 it MUST roll back with `migration.interrupted` and mark the candidate `skip`. Test: kill the process in the middle of `migrate` twice and assert the rollback and `skip`.

### Downgrade

- [LIF-111] The host MUST refuse to activate a version whose `data_version` is lower than the stamp of any bucket in scope, with `migration.data-newer`. The user may run the command with `--restore-backup` (the host restores the backup taken before the update) or `--discard-data` (the host deletes the plugin's buckets and data directory). Test: roll back after a migration, with and without each flag. This is the same rule as [LIF-079](/pages/reference-install-update-uninstall/).

### Many sessions on one machine

Each project session has its own server, and the package store and the global bucket are shared by all of them.

- [LIF-112] A migration that touches the global bucket or the data directory MUST hold the cross-process lock `plugins/locks/<id>.migrate.lock`. The other servers MUST first deactivate their old instances with `upgrade`, then wait for the lock for at most the migrate deadline plus 5 s, and then skip the migration when they find the stamp already new. Test: update with two servers, count calls to `migrate`.
- [LIF-113] While the stamp of a global, project or session bucket is higher than the data version of a running instance, a `put`, `write-batch` or `remove` from that instance MUST return `conflict` with the message `stale-data-version`. Test: leave one server on the old version and write to the global, project and session buckets.
- [LIF-114] A project bucket and a session bucket MUST be migrated by the session that opens them, each with its own backup. Test: open two projects after an update.

### Several keys at once, and durability

The `storage` import has a batch write, `write-batch`, that applies several changes all at once or not at all ([storage](/pages/api-storage/)). Use it to write a split in one step.

- [LIF-115] `put` and `write-batch` MUST return only after the data is committed to stable storage with a flush, so that a power loss after the return does not lose it. Test: kill the process after a `put` returns and read it back.
- [LIF-116] If a plugin must change several files of its data directory, it SHOULD write a new set of files first and write a version marker file last, so that a crash leaves either the old set or the complete new set. The host does not give transactions for files. Test: kill the host between the two writes and assert the plugin reads the old set.

### Worked example: rename a key and split a blob

Version 1 of a notes plugin stores everything in the project bucket under `prefs` as a JSON object with the fields `theme` and `layout`, and it counts opened notes under `count`. Version 2 stores `theme` and `layout` as two keys, and it renames `count` to `opened`. The manifest of version 2 sets `"data_version": 2` and lists `"migrator"` in `exports`.

**Rust**

```rust
use ilium_plugin_sdk::host::storage::{self, Bucket, Entry};
use ilium_plugin_sdk::prelude::*;

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct Notes;

impl Migrator for Notes {
    fn migrate(&mut self, from: u32, to: u32) -> Result<(), PluginError> {
        if from < 2 && to >= 2 {
            migrate_v1_to_v2()?;
        }
        Ok(())
    }
}

fn migrate_v1_to_v2() -> Result<(), PluginError> {
    let mut entries = Vec::new();
    if let Some(blob) = storage::get(Bucket::Project, "prefs") {
        let prefs: serde_json::Value = serde_json::from_slice(&blob)
            .map_err(|error| PluginError::invalid_argument(format!("prefs: {error}")))?;
        for field in ["theme", "layout"] {
            if let Some(value) = prefs.get(field) {
                entries.push(Entry::put(field, value.to_string().into_bytes()));
            }
        }
        entries.push(Entry::remove("prefs"));
    }
    if let Some(count) = storage::get(Bucket::Project, "count") {
        entries.push(Entry::put("opened", count));
        entries.push(Entry::remove("count"));
    }
    // One atomic write: a crash leaves version 1 data or version 2 data, never a mix.
    storage::write_batch(Bucket::Project, &entries)
}
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import { get, writeBatch } from '@ilium/plugin-sdk/host/storage';

function migrateV1ToV2(): void {
  const entries: { key: string; value?: Uint8Array }[] = [];
  const blob = get('project', 'prefs');
  if (blob !== undefined) {
    const prefs = JSON.parse(new TextDecoder().decode(blob));
    for (const field of ['theme', 'layout']) {
      if (field in prefs) {
        entries.push({ key: field, value: new TextEncoder().encode(JSON.stringify(prefs[field])) });
      }
    }
    entries.push({ key: 'prefs' });
  }
  const count = get('project', 'count');
  if (count !== undefined) {
    entries.push({ key: 'opened', value: count });
    entries.push({ key: 'count' });
  }
  writeBatch('project', entries);
}

export const { lifecycle, handler, migrator } = definePlugin({
  side: 'server',
  migrate(from: number, to: number) {
    if (from < 2 && to >= 2) migrateV1ToV2();
  },
});
```

An entry with a value writes the key. An entry with no value removes it.

### Testing a migration

Test the migration against a stored fixture of the old layout. The `FakeHost` can load a storage snapshot and set the data version it believes is stored (see [Testing plugins](/pages/testing/)).

**Rust**

```rust
#[test]
fn migrates_v1_prefs_and_count() {
    let mut host = FakeHost::new()
        .with_storage_snapshot("tests/fixtures/notes-v1.json")
        .with_data_version(1);
    let outcome = host.migrate::<Notes>(2);
    assert!(outcome.is_ok());
    assert_eq!(host.storage_get("project", "opened"), Some(b"7".to_vec()));
    assert_eq!(host.storage_get("project", "prefs"), None);
    assert_eq!(host.storage_get("project", "theme"), Some(b"\"dark\"".to_vec()));
}
```

**TypeScript**

```ts
import { createFakeHost } from '@ilium/plugin-sdk/testing';

test('migrates v1 prefs and count', () => {
  const host = createFakeHost().withStorageSnapshot('tests/fixtures/notes-v1.json').withDataVersion(1);
  expect(host.migrate(2).ok).toBe(true);
  expect(host.storageGet('project', 'prefs')).toBeUndefined();
  expect(host.storageGet('project', 'theme')).toEqual(new TextEncoder().encode('"dark"'));
});
```

Also test the failure path: feed a corrupt `prefs` value and assert that `migrate` returns an error and that the storage is unchanged. Because the batch is atomic, nothing is half written.

### Edge cases

| Situation | What happens |
|---|---|
| `data_version` goes up with no `migrator` | The host stamps the new number, drops restore bytes, clears the `shared` bucket and logs `migration.skipped`. |
| The plugin is disabled when the update arrives | The migration runs at the first activation after the plugin is enabled. |
| Two versions in a row (1 to 3) | One call `migrate(1, 3)`. The plugin handles the steps itself. |
| `migrate` calls a host function the grants forbid | `not-permitted`; the migration fails. |
| The update was installed with `--no-backup` and `migrate` fails | The previous version stays active, the data may be half migrated, and the host logs `migration.no-backup-failed` at level error. |
| The data directory is read-only | `migration.io`; the update rolls back. See [offline and low disk](/pages/reference-offline-and-low-disk/). |

### What the user sees

An update shows a progress line `Updating data of Notes (2 of 5 steps)` in the CLI. In Settings > Plugins the row shows `migrating` while the call runs. A failure shows the toast `Update failed, rolled back` and the log line `migration.failed`.

### See also

- [Install, update and uninstall](/pages/reference-install-update-uninstall/)
- [Storage](/pages/api-storage/)
- [Lifecycle](/pages/lifecycle/)
- [Versioning](/pages/versioning/)
- [Persistence and restart](/pages/reference-persistence-and-restart/)
- [Requirements, lifecycle and operations](/pages/reference-requirements-lifecycle-ops/)

API version checked: `ilium:plugin@0.1.0`.

## Production additions: T9-compat-deprecation

- **[DEV-982]** A migration of several steps MUST be applied as one transaction over the plugin's storage: if step k fails, the host MUST restore the storage as it was before step 1 and report `migrate.failed` with the step index k.
- **[DEV-985]** `ilium plugin update <id> --dry-run-migrate` (proposed, owner to confirm) MUST run the migrator on a copy of the storage, print the step count and the resulting size, change nothing, and exit 0 on success or 1 with the `migrate.*` code on failure.
