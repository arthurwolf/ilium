---
layout: page
title: Compatibility and upgrades
permalink: /pages/guide-compatibility-and-upgrades/
---

## Compatibility and upgrades

A plugin lives longer than the build that you first ship. Users keep it for years, Ilium changes under it, and you change it yourself. This guide tells you which changes are safe, which ones break a user's saved state, and how to ship each kind of change. It covers the five version numbers that a plugin touches, the names that you may never change, the update path that the host runs, and a worked upgrade from version 1 to version 2.

Requirement ids of this page are in the range `AUT-200` to `AUT-299`. The complete list is in [Requirements for author guides](/pages/reference-requirements-author-guides/). The rules of the contract itself are in [Versioning](/pages/versioning/), and the exact update pipeline is in [Install, update and uninstall](/pages/reference-install-update-uninstall/).

### You need

- A packaged plugin that you have shipped, or plan to ship. See [Package and share](/pages/tutorial-09-package-and-share/).
- The data rules of [Data migration](/pages/guide-data-migration/) if your plugin stores data.
- The version rules of [Versions and dependencies](/pages/reference-dependency-resolution/) if your plugin depends on other plugins.

## The five version numbers

A plugin carries five numbers. They move independently, and mixing them up is the most common source of upgrade bugs.

| Number | Where it is written | Who reads it | When you change it |
|---|---|---|---|
| Plugin `version` | `manifest.json`, semantic version with an optional `-prerelease` | The installer, the user, the resolver | Every release. A version that is already installed with different content is refused with `package.version-reused`. |
| Contract `api` | `manifest.json` and the `ilium:api-version` section of each component | The installer | When you rebuild against a newer SDK. In `ilium:plugin@0.1.0` there is only this one value. |
| `data_version` | `manifest.json`, an integer, default 1 | The migration step | When the layout of your stored data changes. |
| Extension point or service version | The declaration of the point or service | Contributors and callers | When a payload or reply schema changes. |
| Restore format byte | The first byte of your restore bytes | Your own `activate` | When the layout of the bytes from `deactivate` changes. |

A plugin MUST raise `version` for every package that it publishes, because the host refuses a second package with the same version and different content. [AUT-201]

The contract version has no effect on your own versions. A plugin at version 3.2.0 may target `ilium:plugin@0.1.0`, and a plugin at version 0.1.0 may target a later contract.

## Names that you may not change

Some names are saved by the host in the user's snapshot, settings and keymap. A rename leaves the old saved value pointing at nothing.

| Name | Saved where | What a rename does to the user |
|---|---|---|
| Plugin `id` | Registry, grants, settings, storage | The plugin becomes another plugin. Grants, storage and settings are not carried over. |
| Pane kind id | Session snapshot | Saved panes of the old kind cannot be restored. |
| Command id | Keymap, saved layouts, scripts that call `ilium` | A bound key does nothing. |
| Settings key | `config.toml`, settings storage | The saved value is ignored and listed on the recovery page. |
| Decoration slot | Persistent decorations | The old decoration stays until the user clears it. |
| Topic name | Subscriptions of other plugins | Subscribers get nothing. |
| Storage key | The plugin's own buckets | The plugin loses its data unless it migrates it. |
| Service and method names | Callers that declare `requires.services` | Callers fail with `not-found`. |
| Extension point id | Contributors | Contributions go dormant. |

A plugin MUST NOT rename or remove any name of this table in a release that keeps the same `id`. To change a name, add the new name, keep the old one working, and document both. [AUT-202]

The old name SHOULD keep working for at least two minor releases of the plugin before it is removed, and a removal MUST be announced in the changelog of the release that removes it. [AUT-203]

### Keep an old command id working

A new version may want a clearer command id. The pattern is to declare both ids, and to let the old one call the same code.

```json
"contributes": {
  "commands": [
    { "id": "export-notes", "title": "Export notes" },
    { "id": "notes-export", "title": "Export notes (old name)" }
  ]
}
```

**Rust**

```rust
fn on_command(&mut self, id: &str) -> Vec<Submission> {
    match id {
        // "notes-export" is the name used before 2.0.0. Keep it until 4.0.0.
        "export-notes" | "notes-export" => self.start_export(),
        _ => Vec::new(),
    }
}
```

**TypeScript**

```ts
function onCommand(id: string): Submission[] {
  switch (id) {
    case 'export-notes':
    case 'notes-export': // used before 2.0.0, kept until 4.0.0
      return startExport();
    default:
      return [];
  }
}
```

The old command SHOULD be hidden from the command list and from the palette once the new one exists, but it MUST still run when a key or a script calls it. [AUT-204]

## What each kind of change needs

| Change | Bump | Breaks users? | What you must do |
|---|---|---|---|
| Bug fix, no data or name change | Patch | No | Ship it. |
| New command, setting, pane kind or topic | Minor | No | Give new settings a default so that old saved values stay valid. |
| New grant | Minor | The update waits for review | The user sees the new grant. Until they accept, the old version stays active. Work without the grant. |
| Removed grant | Minor | No | Drop it from the manifest. |
| New data layout | Minor or major | Yes, if unmigrated | Raise `data_version` and ship a `migrator`. |
| Renamed name (table above) | Major, and keep the old name | Yes | Keep the old name working. |
| Changed payload of a topic, service or extension point | Major of that point | Yes for callers | Raise the point version. Contributions that name the old major go dormant. |
| New build for a newer contract | Minor | No, if the host supports it | Rebuild and set `api`. |
| Dropped support for a platform | Major | Yes | Say so in the package description and the changelog. |

A new grant MUST be treated as optional at run time. The update stays `update-staged` until the user answers the review, and a user who declines keeps the old version. Code that needs the new grant MUST check the call result and fall back when the call returns `not-permitted`. [AUT-205]

## What the host does when it updates your plugin

The host runs an update in a fixed order. Your code sees three of the steps.

1. The new package is checked like an install. A higher version of an installed id is an update.
2. The new version is staged beside the old one.
3. Each running instance of the old version gets `deactivate(upgrade)` and returns restore bytes.
4. If `data_version` rose, the host takes a backup and calls `migrator.migrate(from, to)` once per side.
5. The new version is activated. It gets the restore bytes only when its `data_version` equals the one of the build that wrote them.
6. If activation succeeds, the update is committed. If it fails, the host restores the backup, reactivates the old version and shows `Update failed, rolled back`.

| Your code | Runs when | Must |
|---|---|---|
| `deactivate(upgrade)` | Old version, before the swap | Return restore bytes that start with a format byte, within 500 ms and 1 MiB |
| `migrate(from, to)` | New version, once per side, before `activate` | Be idempotent and finish within its deadline |
| `activate` | New version | Check the restore format byte and the length, and fall back to defaults |

A plugin MUST write the restore bytes in a layout whose first byte identifies the format, and MUST fall back to defaults when the first byte is not a format it knows, because the host delivers restore bytes only when the `data_version` equals that of the build that wrote them. [AUT-206]

Data that must survive a layout change MUST live in storage and be converted by a `migrator`, because restore bytes are delivered only when the `data_version` matches, and across versions only a `migrator` carries data. [AUT-207]

## Downgrades and rollbacks

The user can roll back with `ilium plugin rollback <id>` or install a lower version with `--allow-downgrade`. The host refuses a rollback with `migration.data-newer` when the stored data version is higher than the target supports. The user can then restore the backup taken before the update, which loses what was written since, or discard the data.

A plugin SHOULD keep its data readable by the previous release for one release, by adding keys instead of changing the meaning of old ones. A user who rolls back then loses nothing. [AUT-208]

## Upgrading Ilium under your plugin

The host loads the contract version it ships and the one before it. A package whose `api` falls outside that window is refused with `package.api-unsupported`. Version 0.1.0 is the first release and has no previous version.

| Situation | What happens | What you do |
|---|---|---|
| The user upgrades Ilium and your `api` is still in the window | The plugin keeps working | Rebuild when it suits you |
| Your `api` falls out of the window | A package outside the window is refused with `package.api-unsupported`. See [Versioning](/pages/versioning/) for the window. | Rebuild with the current SDK and publish a new version |
| Your plugin lists an unstable feature that was renamed | Refused with `manifest.unstable-unknown` | Read the changelog of Ilium before upgrading, and update the name |
| An item you use is marked `@since` a newer version than your `api` | Refused with `manifest.grant-unknown` or an import error | Raise `api` and rebuild |

A plugin SHOULD rebuild and publish against each new minor contract version before the host drops the minor it was built for (the host loads the current minor and the previous one), so that users never hold a plugin outside the window. [AUT-209]

A published plugin MUST NOT list an unstable feature unless the plugin is for development use, because a T2 or T3 package that lists one is installable only in development mode. [AUT-210]

## Version syntax, prereleases and channels

A version is `MAJOR.MINOR.PATCH` with an optional `-prerelease`, for example `2.0.0-beta.1`. A leading `v` is refused with `manifest.version-invalid`. A prerelease sorts below its release, so `2.0.0-beta.2` is older than `2.0.0`. A prerelease does not satisfy a plain range such as `^2.0.0` on the stable channel. See [Versions and dependencies](/pages/reference-dependency-resolution/).

| Release kind | Version example | Channel | Who gets it |
|---|---|---|---|
| Release | `2.0.0` | stable | Everyone who updates |
| Release candidate | `2.0.0-rc.1` | beta | Testers who chose the beta channel |
| Fix of a release | `2.0.1` | stable | Everyone who updates |

A package that is meant for testers MUST carry a prerelease part in its version, so that it cannot be taken for a release. [AUT-211]

## Depending on other plugins

A plugin that needs another plugin declares it in `requires` with a range. The range is the promise that the other plugin keeps its names and schemas within that range.

- Use `^MAJOR.MINOR` for a dependency that follows the rule above. It accepts later minors of the same major.
- Declare a dependency as `optional` when the plugin can work without it, and test both cases.
- A range that cannot be met blocks the install with `dependency.unresolved`. A cycle is refused with `manifest.requires-cycle` or `manifest.service-cycle`.

A plugin MUST declare a `requires` range no wider than the versions it was tested with. [AUT-212]

A plugin that provides a service or an extension point MUST raise the major version of the point when it changes the payload incompatibly, and MUST keep the old major working for as long as it lists the old major as supported. [AUT-213]

## Worked example: from 1.4.0 to 2.0.0

The plugin `com.example.notes` stores its notes under one key in the `global` bucket. Version 2.0.0 splits them into one key per note, renames a command and adds a setting and an optional grant.

The manifest changes are:

```json
{
  "id": "com.example.notes",
  "version": "2.0.0",
  "publisher": "Example Labs",
  "api": "ilium:plugin@0.1.0",
  "data_version": 2,
  "exports": ["migrator"],
  "capabilities": { "server": ["notify.toast"] },
  "contributes": {
    "commands": [
      { "id": "export-notes", "title": "Export notes" },
      { "id": "notes-export", "title": "Export notes (old name)" }
    ]
  }
}
```

The migration code reads the old blob and writes the new keys. It writes the new keys first and removes the old key last.

**Rust**

```rust
impl Migrator for NotesPlugin {
    fn migrate(&mut self, from: u32, to: u32) -> Result<(), PluginError> {
        if from >= 2 || to < 2 {
            return Ok(());
        }
        let Some(blob) = storage::get(Bucket::Global, "notes") else {
            return Ok(()); // nothing to convert, or a second run
        };
        for (index, note) in split_notes(&blob).into_iter().enumerate() {
            storage::put(Bucket::Global, &format!("note:{index:06}"), &note)?;
        }
        storage::remove(Bucket::Global, "notes");
        Ok(())
    }
}
```

**TypeScript**

```ts
export const migrator = {
  migrate(from: number, to: number): void {
    if (from >= 2 || to < 2) return;
    const blob = storage.get('global', 'notes');
    if (blob === undefined) return; // nothing to convert, or a second run
    splitNotes(blob).forEach((note, index) => {
      storage.put('global', `note:${String(index).padStart(6, '0')}`, note);
    });
    storage.remove('global', 'notes');
  },
};
```

The test calls `migrate` twice on the same fixture and compares the data. The second call must change nothing. The fake host can start with an old data version (`with_data_version`) and a stored snapshot (`with_storage_snapshot`), so the test starts from the layout of version 1.4.0. See [Data migration](/pages/guide-data-migration/).

## Edge cases and errors

| Situation | What happens | What the author does |
|---|---|---|
| Same version, different content | Refused with `package.version-reused` | Raise the version |
| Lower version than installed | Refused with `package.downgrade` unless `--allow-downgrade` | Publish a higher version that contains the fix |
| Update asks for a new grant | Staged until the user answers | Work without it |
| Update fails to activate | Rolled back, backup restored | Read `ilium plugin logs <id>` and fix |
| `migrate` fails | Backup restored, old version stays | Make the migration idempotent and test it |
| Rollback after a migration | Refused with `migration.data-newer` | Keep data readable by the previous release |
| `api` outside the window | `package.api-unsupported` | Rebuild |
| Restore bytes from an older `data_version` | Dropped, one log line | Do not keep needed data only in restore bytes |
| Two servers update at once | The first to finish decides | Nothing, the host serialises it |

## What the user sees

An update shows the new grants in a review dialog and keeps the old version active until the user answers. A failed update shows `Update failed, rolled back`. A rollback that cannot keep data offers two choices, restore the backup or discard the data. In Settings > Plugins the version and the state of the plugin are shown. See [Managing plugins](/pages/user-managing-plugins/).

## See also

- [Versioning](/pages/versioning/) for the contract rules
- [Data migration](/pages/guide-data-migration/) for the `migrator` export and the backup
- [Install, update and uninstall](/pages/reference-install-update-uninstall/) for the pipeline
- [Publishing and support](/pages/guide-publishing-and-support/) for releases and changelogs
- [Quality checklist](/pages/guide-quality-checklist/) for a pre-release review

API version checked: `ilium:plugin@0.1.0`.
