---
layout: page
title: Upgrading Ilium and rolling back the host
permalink: /pages/guide-operator-upgrades/
---

## Upgrading Ilium and rolling back the host

This page is the operator procedure for replacing the Ilium binary while plugins are installed, ending the running server, and undoing a host release. It covers the three open items of [Host upgrade and restart while plugins run](/pages/host-upgrade-while-plugins-run/): what happens when the binary is swapped, which restart applies, and how to roll back the host. It does not describe a plugin update. For that, see [Compatibility and upgrades](/pages/guide-compatibility-and-upgrades/).

Requirement ids have the form `LIF-nnn`. They are collected in [Requirements, lifecycle and operations](/pages/reference-requirements-lifecycle-ops/).

### Terms

- **Host**: the Ilium binary and the server process it starts. The host version is the version of that binary.
- **Clean stop**: ending the server so that it runs its shutdown path. Each live instance then receives `deactivate` with reason `shutdown`. A clean stop is the only server stop that does this (see [Host upgrade and restart while plugins run](/pages/host-upgrade-while-plugins-run/)).
- **Kill**: ending the server with a signal or a crash. No `deactivate` is sent.
- **Instance**: one running copy of one side of a plugin. It lives from `activate` until `deactivate`, a trap or a restart.
- **Package state**: the one state per installed plugin shown by `ilium plugin list --json`, for example `installed-enabled`. See [Plugin lifecycle states](/pages/reference-plugin-lifecycle-states/).
- **Plugin rollback**: `ilium plugin rollback <id>`. **Host rollback**: replacing the binary with an earlier release.

### Inputs, outputs and limits

| Item | Value | Source |
|---|---|---|
| Input of a binary replacement | The new Ilium binary on disk | this page |
| Output of `ilium plugin list --json` | The package state and side states of each plugin | [Plugin lifecycle states](/pages/reference-plugin-lifecycle-states/) |
| `deactivate` deadline at a server stop | 500 ms by default, 1,000 ms ceiling for a T1 plugin | [Host upgrade and restart while plugins run](/pages/host-upgrade-while-plugins-run/) |
| Whole server shutdown | 5 s, then remaining plugins are dropped and `host.shutdown-timeout` is logged | same page |
| Restore bytes | 1 MiB at most; a larger return is dropped with one warning | same page |
| Log lines read after an upgrade | `--tail 100` per plugin | LIF-298 |

### Two different rollbacks

Rolling back a plugin and rolling back the host are separate operations. Use the table to pick the right one.

| You want to undo | Command or step | What changes | What does not change |
|---|---|---|---|
| The version of one plugin | `ilium plugin rollback <id>` | The installed version of that plugin | The Ilium binary and the host version |
| The Ilium host release | Install the earlier binary, then end and start the server | The binary and the host version | The installed plugin versions |

### Replacing the Ilium binary

- [LIF-295] When the Ilium binary on disk is replaced, the running server MUST keep its loaded instances unchanged, and the new binary MUST take effect only at the next server start.

Replacing the binary does not stop your plugin. Its instances keep running on the old code until the server stops. At that stop each live instance receives `deactivate` with reason `shutdown`, as described in [Host upgrade and restart while plugins run](/pages/host-upgrade-while-plugins-run/). After the new server starts, the host activates each plugin again when its trigger fires.

- [LIF-299] Before replacing the Ilium binary, the operator MUST run `ilium plugin list --json` and MUST keep its output as the record to compare against after the restart.
- [LIF-300] A clean server stop MUST give the whole shutdown 5 s. A plugin that is still running at that point MUST be dropped without `deactivate`, with `host.shutdown-timeout` logged, so an operator MUST NOT treat a quiet plugin log after a stop as proof that `deactivate` ran.

Operator steps:

1. Run `ilium plugin list --json` and write down the package state and the version of each plugin (LIF-299). This is the record to compare against after the restart.
2. Replace the Ilium binary with the new release.
3. End the running server as described in the next section, then start Ilium again.
4. Run the checks in [Checks after an upgrade or a host rollback](#checks-after-an-upgrade-or-a-host-rollback).

Decision needed: confirm that a new binary takes effect only at the next server start, and do not apply it to a live server in place. The default is the rule in LIF-295, because the only documented server stop that runs `deactivate` is a clean stop with `shutdown`. Consequence: an operator must plan a restart after each binary replacement, and plugins run the old code until then.

### Ending and starting the server

- [LIF-296] An operator MUST end the running server with a clean stop, not with a kill signal, when the new binary is to take effect. A kill skips `deactivate`, so the server side loses its restore bytes and every plugin loses its in-memory state.

Decision needed: name the one `ilium` command that performs a clean server stop and start, and document it on this page. The manual does not name it today, so this page cannot give the exact command. The default is to add that command to the operator steps once the owner confirms its name. Consequence: until then an operator can only end the session through the normal Ilium quit, and the steps above name no restart command.

### Rolling back a plugin

- [LIF-297] `ilium plugin rollback <id>` MUST NOT change the Ilium binary or the host version, so an operator who needs to undo a host release MUST NOT use it for that.

The plugin rollback restores an earlier version of one plugin, and it follows the data rules of [Compatibility and upgrades](/pages/guide-compatibility-and-upgrades/). It is refused with `migration.data-newer` when the stored data is newer than the target version supports. A plugin rollback has no effect on the Ilium binary, so a plugin rollback never undoes a host upgrade.

### Rolling back the host

A host rollback is the operator's replacement of the Ilium binary with an earlier release, followed by a clean stop and a start of the server, as in LIF-296.

Decision needed: decide whether this release supports a host rollback, and how earlier binaries are kept. The default is to document a host rollback as the steps above, with the checks in the next section, and to make no promise about stored data until the owner confirms that an earlier host starts with the data a later host wrote. Consequence: operators who need to undo a host release have no confirmed procedure yet, and the only refusal the manual states for a rollback is `migration.data-newer`, which applies to a plugin rollback.

### Checks after an upgrade or a host rollback

- [LIF-298] After a host upgrade or a host rollback, the operator MUST run `ilium plugin list --json` and MUST read `ilium plugin logs <id> --tail 100` for each plugin that does not behave as expected. A plugin in `stopped-by-failure` MUST be enabled again only after its cause is fixed, because enabling clears its failure window.

Look for these log lines in the output of `ilium plugin logs`:

- `deactivate.deadline`: a plugin did not finish `deactivate` in time and its instance was dropped.
- `host.shutdown-timeout`: the server shutdown took longer than the host allows, and the remaining plugins were dropped without `deactivate`.
- `restart.scheduled`: a plugin stopped and the host will start it again.
- `update.rolled-back`: a plugin update was undone.

Compare the package state of each plugin with the record from the first step. A plugin that changed state without a plugin update or an operator action needs its log read before it is enabled again.

### Failure behaviour

| Situation | What happens | Code or log line |
|---|---|---|
| The server is killed instead of stopped cleanly | No `deactivate`; server restore bytes and in-memory state are lost | none |
| A plugin misses its `deactivate` deadline | The instance is dropped, its bytes are discarded, the plugin is not counted as failed | `deactivate.deadline` |
| The whole shutdown exceeds 5 s | Remaining plugins are dropped without `deactivate` | `host.shutdown-timeout` |
| A plugin rollback targets a version older than the stored data | The rollback is refused unless `--restore-backup` or `--discard-data` is given | `migration.data-newer` |
| `--restore-backup` is given but the snapshot is gone | The rollback fails | `rollback.no-backup` |
| A plugin was `stopped-by-failure` before the stop | The server side stays stopped after the restart (stored as `failure_stopped`) | none |

### Examples

Upgrade with a clean stop:

```
ilium plugin list --json > before.json
# replace the binary, then end the server cleanly and start Ilium again
ilium plugin list --json > after.json
ilium plugin logs my.plugin --tail 100
```

Read the log for `deactivate.deadline` and `host.shutdown-timeout`. A plugin whose package state differs between `before.json` and `after.json` without an update or an operator action needs its log read before it is enabled (LIF-298).

Wrong host rollback: running `ilium plugin rollback my.plugin` after a bad host release changes only that plugin and leaves the binary unchanged (LIF-297).

### What this page does not cover

- Plugin updates, plugin versions and data migration: [Compatibility and upgrades](/pages/guide-compatibility-and-upgrades/) and [Install, update and uninstall](/pages/reference-install-update-uninstall/).
- What each running instance sees at a stop, and which state survives: [Host upgrade and restart while plugins run](/pages/host-upgrade-while-plugins-run/) and [Persistence and restart](/pages/reference-persistence-and-restart/).
- Crash-loop guard and safe mode after repeated crashes: [Safe mode and recovery](/pages/user-safe-mode-and-troubleshooting/).

### See also

- [Host upgrade and restart while plugins run](/pages/host-upgrade-while-plugins-run/) for what happens to running instances at a stop
- [Compatibility and upgrades](/pages/guide-compatibility-and-upgrades/) for plugin versions, updates and plugin rollback
- [Install, update and uninstall](/pages/reference-install-update-uninstall/) for the update pipeline and the data rules
- [Plugin lifecycle states](/pages/reference-plugin-lifecycle-states/) for the states named in the checks
- [Troubleshooting plugins](/pages/user-troubleshooting/) for the restart rules and the recovery page

API version checked: `ilium:plugin@0.1.0`.
