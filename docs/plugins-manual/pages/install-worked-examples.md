---
layout: page
title: Worked examples for install, update and rollback
permalink: /pages/install-worked-examples/
---

## Worked examples for install, update and rollback

This page walks through the install life of one plugin, `com.example.notes`, from a first install to a rollback and an uninstall. Each step gives the command, the exit code, and the state that the next step can rely on. Use it to check a script or to see how the rules of the reference pages combine.

The exit codes come from the table in [Distribution commands](/pages/reference-plugin-distribution-cli/#exit-codes). The flags are explained in [Install flag matrix](/pages/install-flag-matrix/). The state changes are the ones of [Install and update](/pages/reference-install-and-update/).

### You will need

- A registry configured with `ilium plugin registry add <url> --root-key <key_id>`, or a package file, or an offline bundle. Each example says which one it needs.
- A terminal in which `ilium plugin` runs. Examples that run under `--yes` need no attached client.

### Example 1: install from a file

The user has the package `notes-1.2.0.iliumpkg` from the publisher.

1. `ilium plugin install ./notes-1.2.0.iliumpkg`. The package is checked, the review is shown, and the user accepts it. Exit 0.
2. The state is `installed.json` with `com.example.notes` at `1.2.0`, and the plugin is `On` unless the user left it off ([USR-013](/pages/user-install-and-manage/)).
3. Run the same command again. The version and manifest hash match, so the install is a no-op: it exits 0 and prints `install.already-installed` as a warning ([PKG-274](/pages/reference-install-and-update/)).

- **[INS-101]** The second install of the same file MUST NOT change any file. Test: hash the store and `installed.json`, run the command again, and compare.

### Example 2: install from the registry

The registry offers `com.example.notes`. The user wants the newest `1.x` release.

1. `ilium plugin search notes --category editors --limit 5`. The list is sorted by relevance because a query is given ([PKG-382](/pages/reference-registry/)). Exit 0.
2. `ilium plugin info com.example.notes`. The record shows the latest version, the `yanked`, `deprecated` and `successor` fields ([PKG-437](/pages/reference-plugin-distribution-cli/#search-info-verify-lock)).
3. `ilium plugin install com.example.notes@^1.2`. The range is resolved to one version, the package is downloaded, and the install runs as in Example 1. Exit 0.

If the registry is not configured, step 1 fails with `registry.none-configured` and exit 1 ([PKG-435](/pages/reference-plugin-distribution-cli/#search-info-verify-lock)).

- **[INS-102]** A search with `--limit 0` or `--limit 101` MUST fail with `usage.invalid-value` and exit 2 before any network request ([PKG-824](/pages/reference-registry/), proposed, owner to confirm).
- **[INS-103]** `ilium plugin info` of a plugin that is yanked MUST show the field `yanked` in its `plugin` record when the source is a registry ([PKG-437](/pages/reference-plugin-distribution-cli/#search-info-verify-lock)). Test: yank version 1.2.1, run `info`, and assert the field appears.

### Example 3: a locked project

A project keeps `ilium-plugins.lock` so that every machine installs the same versions.

1. On the machine that has the right versions, `ilium plugin lock` writes the lock file for the project from the installed set ([Distribution commands](/pages/reference-plugin-distribution-cli/#search-info-verify-lock)).
2. On a second machine, `ilium plugin install com.example.notes --locked` installs the locked version and hash. It never resolves a new version ([PKG-428](/pages/reference-plugin-distribution-cli/#install)).
3. If the package in the registry has changed hash, the command fails with `dependency.lock-mismatch` and exit 1. Nothing is installed.

- **[INS-104]** `install --locked` MUST NOT write `installed.json` when `dependency.lock-mismatch` is raised. Test: change the lock file's hash for one plugin, run the command, and compare `installed.json` before and after.

### Example 4: an update that goes wrong, then a rollback

The user updates `com.example.notes` from `1.2.0` to `1.3.0`. The new version fails its first activation.

1. `ilium plugin update com.example.notes`. The newest version in the channel is installed, and the candidate becomes active only after its first activation succeeds ([LIF-075](/pages/reference-install-update-uninstall/)). If that activation fails, the previous version stays active.
2. Suppose the update succeeded but the user finds a bug. `ilium plugin rollback com.example.notes` makes `1.2.0` active again and marks `1.3.0` as `skip` ([PKG-290](/pages/reference-install-and-update/#rollback), [PKG-292](/pages/reference-install-and-update/#rollback)). Exit 0.
3. `ilium plugin update com.example.notes` now offers nothing, because `1.3.0` is marked `skip`. It is offered again when a newer version exists.
4. After the publisher ships a fix, the administrator can install `1.3.0` with `ilium plugin install ./notes-1.3.0.iliumpkg --unskip` to clear the mark for that version only ([INS-014](/pages/install-flag-matrix/#rules-for-each-flag)).

If the data of `1.3.0` migrated the stored data to a newer `data_version`, a rollback to `1.2.0` is refused with `migration.data-newer` and exit 1 unless the backup is restored ([LIF-079](/pages/reference-install-update-uninstall/)). Restoring the backup is `ilium plugin rollback com.example.notes --restore-backup`.

- **[INS-105]** `rollback` of a plugin with no previous version MUST fail with `install.no-previous` and exit 5 ([PKG-433](/pages/reference-plugin-distribution-cli/#update-rollback-pin-channel)). Test: first-install a plugin, run `rollback`, and assert exit 5 and no change in `installed.json`.
- **[INS-106]** `rollback` MUST NOT delete the newer version from the store. The store keeps the files until the next committed update removes them ([PKG-266](/pages/reference-install-and-update/)). Test: roll back, then list the store folder and assert that `1.3.0` is still present.

### Example 5: a fleet update that partly fails

An administrator runs `ilium plugin update --all --dry-run` first, then the real command.

1. The dry run prints one result per unpinned plugin and changes nothing.
2. The real `ilium plugin update --all` continues after a failure of one plugin, prints every failure, and exits 1 when at least one plugin failed ([PKG-432](/pages/reference-plugin-distribution-cli/#update-rollback-pin-channel)).
3. The administrator reads the failure lines, fixes the cause (for example a missing registry key), and runs `update` for the failed plugin only.

- **[INS-107]** `update --all` MUST leave every plugin that succeeded at its new version after a failure of another plugin. Test: make one of two plugins fail its signature check, run `update --all`, and assert exit 1 and the new version of the other plugin.

### Example 6: an offline install from a bundle

A machine with no network needs `com.example.notes`. A machine with network prepares the bundle.

1. On the connected machine: `ilium plugin export-bundle ./bundle com.example.notes --with-deps`. The folder holds the signed metadata and the package. If a dependency is missing and `--with-deps` is absent, the command fails with `dependency.unsatisfied` ([PKG-440](/pages/reference-plugin-distribution-cli/#registry-and-mirror)).
2. Copy the folder to the offline machine on approved media.
3. On the offline machine: `ilium plugin install --from-bundle ./bundle com.example.notes`. The install reads the bundle and never opens a network connection ([PKG-426](/pages/reference-plugin-distribution-cli/#install), [INS-011](/pages/install-flag-matrix/#rules-for-each-flag)).

- **[INS-108]** `install --from-bundle` MUST fail with `install.download-failed` only when the bundle is incomplete, and MUST NOT try to fetch the missing file from the network (proposed, owner to confirm). Test: delete the package from a bundle, run the install with the network blocked, and assert exit 1 and no connection attempt.

### Example 7: install while another install holds the lock

Two administrators run an install at the same moment.

1. The first install takes the lock `installed.lock`.
2. The second install polls every 100 ms for up to 30 s, then fails with `install.locked` and exit 4 ([PKG-270](/pages/reference-install-and-update/)).
3. The second administrator waits and runs the command again. Nothing was changed by the failed attempt.

The lock is an OS advisory lock. If the first process crashes, the lock is released when the process exits, so the second attempt succeeds without any manual step.

- **[INS-109]** A second install that fails with `install.locked` MUST leave `installed.json` and the store unchanged. Test: hold the lock in one process, run an install in another, and compare the files.

### Example 8: uninstall, keep or purge the data

1. `ilium plugin uninstall com.example.notes` removes the program files and keeps the plugin data under `<data>/plugins/data/<id>/`.
2. `ilium plugin uninstall com.example.notes --purge` also deletes the data, the settings, the log and the pre-migration backups ([reference-plugin-files](/pages/reference-plugin-files/)).

The purge deletes local data only. It does not delete a whole-host archive or a single-plugin export that the administrator wrote elsewhere ([HST-977](/pages/guide-backup-and-restore/)).

- **[INS-110]** `uninstall` without `--purge` MUST keep the plugin data folder unchanged. Test: write a file into the data folder, uninstall, reinstall the same version, and assert that the file is still there.

### Edge cases

- A source path that does not exist: `usage.file-not-found`, exit 2, before any work ([PKG-426](/pages/reference-plugin-distribution-cli/#install)).
- An `http://` URL: `install.insecure-url`, exit 1.
- A network failure during a registry install: `registry.unreachable`, exit 8 ([PKG-424](/pages/reference-plugin-distribution-cli/#exit-codes)).

### See also

- [Install flag matrix](/pages/install-flag-matrix/)
- [Install and update](/pages/reference-install-and-update/)
- [Install, update and uninstall reference](/pages/reference-install-update-uninstall/)
- [Distribution commands](/pages/reference-plugin-distribution-cli/)
- [Install and manage plugins](/pages/user-install-and-manage/)
- [Offline and low disk](/pages/reference-offline-and-low-disk/)
