---
layout: page
title: Upgrading Ilium and rolling back the host
permalink: /pages/guide-operator-upgrades/
---

## Upgrading Ilium and rolling back the host

This page is the operator procedure for replacing the Ilium binary while plugins are installed, ending the running server, and undoing a host release. It covers the three open items of [Host upgrade and restart while plugins run](/pages/host-upgrade-while-plugins-run/): what happens when the binary is swapped, which restart applies, and how to roll back the host. It does not describe a plugin update. For that, see [Compatibility and upgrades](/pages/guide-compatibility-and-upgrades/).

Requirement ids have the form `LIF-nnn`. They are collected in [Requirements, lifecycle and operations](/pages/reference-requirements-lifecycle-ops/).

### Two different rollbacks

Rolling back a plugin and rolling back the host are separate operations. Use the table to pick the right one.

| You want to undo | Command or step | What changes | What does not change |
|---|---|---|---|
| The version of one plugin | `ilium plugin rollback <id>` | The installed version of that plugin | The Ilium binary and the host version |
| The Ilium host release | Install the earlier binary, then end and start the server | The binary and the host version | The installed plugin versions |

### Replacing the Ilium binary

- [LIF-295] When the Ilium binary on disk is replaced, the running server MUST keep its loaded instances unchanged, and the new binary MUST take effect only at the next server start.

Replacing the binary does not stop your plugin. Its instances keep running on the old code until the server stops. At that stop each live instance receives `deactivate` with reason `shutdown`, as described in [Host upgrade and restart while plugins run](/pages/host-upgrade-while-plugins-run/). After the new server starts, the host activates each plugin again when its trigger fires.

Operator steps:

1. Run `ilium plugin list --json` and write down the package state and the version of each plugin. This is the record to compare against after the restart.
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

### See also

- [Host upgrade and restart while plugins run](/pages/host-upgrade-while-plugins-run/) for what happens to running instances at a stop
- [Compatibility and upgrades](/pages/guide-compatibility-and-upgrades/) for plugin versions, updates and plugin rollback
- [Install, update and uninstall](/pages/reference-install-update-uninstall/) for the update pipeline and the data rules
- [Plugin lifecycle states](/pages/reference-plugin-lifecycle-states/) for the states named in the checks
- [Troubleshooting plugins](/pages/user-troubleshooting/) for the restart rules and the recovery page

API version checked: `ilium:plugin@0.1.0`.
