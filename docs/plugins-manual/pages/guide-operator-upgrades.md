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

### Data written by a newer host

A host rollback changes the binary, not the data on disk. The earlier host meets whatever the newer host wrote at its next start. These rules decide what it does; the first three are stated elsewhere and are repeated here only as they apply to a rollback.

- The earlier host reads a file only when its format number is at most its own (LIF-304). It never converts newer data downward (PKG-288).
- A file with a higher format number in `installed.json`, a bucket or a stamp makes the earlier host start with the affected plugins disabled, log `host.format-newer` naming the file, and leave the file unchanged (LIF-305).
- A plugin bucket whose stamp is newer than the earlier plugin version is refused with `migration.data-newer` on activation, unless `--restore-backup` or `--discard-data` is given (LIF-079).
- A core snapshot or config file written by a newer build is kept unmodified, and the earlier build starts with persistence read-only and a notice naming both versions (COR-093). See [Data written by a newer build, and unknown section versions](/pages/reference-config-and-data-migration/#data-written-by-a-newer-build-and-unknown-section-versions).

The following rules are new and apply to any host rollback.

- [LIF-950] Before replacing the Ilium binary, the operator MUST keep a copy of the previous Ilium binary and of the plugin state files (`installed.json`, the plugin buckets and the stamps), and MUST keep both until the checks in the next section pass. Keeping one previous binary is the minimum this page relies on (proposed, owner to confirm).
- [LIF-951] After a host rollback, an operator MUST NOT enable a plugin whose log shows `host.format-newer` until either the newer host has been started again or the file named in that log line has been restored from the copy kept under LIF-950, because the earlier host cannot read that file.

The owner has not yet decided whether this release supports a host rollback at all. LIF-950 and LIF-951 state the data rule that applies in either case, and the numbers they carry are proposals.

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

### Staged update rollout

An operator who manages many machines or sessions applies a plugin update to a small group first. This section is the target behaviour of that staged rollout.

- [LIF-306] A policy MUST be able to assign an update to a rollout stage of a named share of sessions or machines, and a host outside the current stage MUST NOT apply that update, and MUST report it as `held` in `ilium plugin list --json`.
- [LIF-307] A rollout MUST halt on its own when the plugin fails inside the failure window on a host of the current stage. A halted rollout MUST stop widening, MUST log `rollout.halt` with the plugin id and version, and MUST NOT roll back hosts that already updated.
- [LIF-308] An operator MUST be able to resume a halted rollout or cancel it with `ilium plugin rollout resume <id>` and `ilium plugin rollout cancel <id>`, and each command MUST write an audit event.

#### Stage shares and the placement of a host

A rollout names its stages as cumulative shares of the host population, written as a percentage with two decimal places (steps of 0.01 %). The shares MUST increase from stage to stage, and the last stage MUST be 100 %. Example shares (proposed, owner to confirm): stage 1 at 1 %, stage 2 at 10 %, stage 3 at 50 %, stage 4 at 100 %.

A host is placed by a number that it computes itself, the same way on every start:

1. The host reads its installation key. The server creates the key once, at its first start, and keeps it in its own state directory beside `installed.json`. Each user has their own state ([SEC-199]), so each user session is a separate host, and two users on one machine give two placements.
2. The host computes the SHA-256 of the text `<rollout id>:<installation key>`.
3. The host reads the first four bytes of that digest as a big-endian integer and takes it modulo 10,000. The result is the bucket, from 0 (0.00 %) to 9,999 (99.99 %).
4. The host is in stage *k* when its bucket is below the cumulative share of stage *k* (in units of 0.01 %). It applies the update only when the bucket is below the share of the current stage. Otherwise it reports `held`.

Consequences an operator can rely on:

- **Stable across restarts.** The bucket depends only on the rollout id and the installation key, so a restart gives the same answer. A host held before a restart is still held after it, and a host that has applied the update keeps it.
- **Widening keeps earlier hosts in.** Raising a stage's share only raises the cutoff. A host below an earlier cutoff stays below every later one, so no host moves out of a stage that it has entered.
- **Different rollouts place hosts differently.** The rollout id is part of the digest, so the same machine is not always in the first stage.
- **A host moves only if its key changes.** Deleting the state directory creates a new installation key, which can move the host into another stage.

The stage holds about its share of the hosts, not exactly that share: a hash spreads hosts at random. For a population of 300 installations, stage 1 at 1 % expects about 3 hosts and stage 2 at 10 % about 30. The count of hosts actually in a stage is known only from the hosts that report their stage (see the next section). The manual does not yet define a fleet-level count (owner to confirm).

#### What an administrator sees for a held host

- **Version and state.** A held host keeps the installed version. Its package state stays `installed-enabled`, as for any update that has not yet been applied. The newer version is shown in `update-available` from the cached registry data.
- **The `held` value.** `ilium plugin list --json` reports `held` for the plugin's pending rollout update, as [LIF-306] requires. `held` is not one of the package states in [Plugin lifecycle states](/pages/reference-plugin-lifecycle-states/), so the field that carries it is to be confirmed by the owner. The output also gives the host's stage number for that rollout (owner to confirm).
- **Not a failure.** A held host has `failed` false and is not counted by the halt rule of [LIF-307], which counts only hosts in the current stage.
- **When it updates.** A held host applies the update at its first update check after its stage's cutoff includes its bucket. It does not need a restart for that.
- **Audit.** Proposed, owner to confirm: one `rollout.held` audit event when a host first holds the update, and one `rollout.applied` event when it applies it. Both must be added to the audit catalogue of [Audit log](/pages/reference-audit-log/) before they are written.
- **Fleet view.** Held hosts are visible on each machine only. `ilium plugin list --json --inventory` ([USR-341]) has a fixed field list without the rollout stage, so a fleet tool sees a held host as running the old version and cannot tell it from a host that was never offered the update. An administrator who needs the held count across machines must run `ilium plugin list --json` on each machine, as for crash counts ([Enterprise deployment](/pages/guide-enterprise-deployment/)). The owner needs to decide whether the inventory carries the stage.

Halt, using the example shares above: in stage 1 (about 3 hosts of 300), one counted failure of the new version on a host in that stage halts the rollout under [LIF-307]. Stage 2 does not open, and the hosts that already updated keep the new version. The admin must then decide, by [LIF-308], whether to resume or cancel. How a failure on one host reaches the rollout record, and how many failures count as a halt, are not yet defined (owner to confirm).

## Operator runbook for failure modes

Each entry gives detection, mitigation and recovery. The runbook is part of the contract: the commands and codes it names MUST exist.

- [EXT-074] The runbook MUST cover a crash loop: detection by the plugin state `stopped-by-failure` with a restart count shown by `ilium plugin list --json`; mitigation by `ilium plugin disable <id>`; recovery by reading `ilium plugin logs <id>` and re-enabling. The host MUST stop restarting a plugin at the third failure within 10 minutes (HST-400).
- [EXT-075] The runbook MUST cover a quarantined or blocked plugin that users still depend on: detection by `Quarantined` or `Blocked` in the list; mitigation by naming the replacement or the fixed version; recovery by upgrade, with the data kept as stated in [SEC-327].
- [EXT-076] The runbook MUST cover an upgrade that fails part way: detection by the error code and the audit line; mitigation by the guarantee that the previous version stays installed ([EXT-062]); recovery by `ilium plugin rollback <id>` and a retry after the cause is fixed.

### See also

- [Host upgrade and restart while plugins run](/pages/host-upgrade-while-plugins-run/) for what happens to running instances at a stop
- [Compatibility and upgrades](/pages/guide-compatibility-and-upgrades/) for plugin versions, updates and plugin rollback
- [Install, update and uninstall](/pages/reference-install-update-uninstall/) for the update pipeline and the data rules
- [Plugin lifecycle states](/pages/reference-plugin-lifecycle-states/) for the states named in the checks
- [Troubleshooting plugins](/pages/user-troubleshooting/) for the restart rules and the recovery page

API version checked: `ilium:plugin@0.1.0`.

## Production additions: T9-compat-deprecation

- **[DEV-992]** `ilium plugin preflight --target <host-version>` (command name proposed, owner to confirm) MUST list, for every installed plugin, whether it would load, load with a `deprecated` record or be refused with a code, using the matrix data shipped in the target binary or a manifest file, without starting any plugin.
