---
layout: page
title: Responding to a bad release across a fleet
permalink: /pages/ops-bad-release-fleet-response/
---

## Responding to a bad release across a fleet

This page gives the operator of many machines the order of work for a bad plugin release: contain it, find the machines that have it, recover them, confirm the recovery and close the incident record. It builds on the severity and acknowledgement clocks of [Operational incident response](/pages/ops-incident-response/) and on the publisher-side steps of [Yank, deprecate and takedown](/pages/publish-yank-deprecate-takedown/). Read it when a plugin version is found to be bad on more than one machine.

### You need

- The incident record file. The command `ilium plugin incident check <file>` validates it, so keep it in the form that command reads.
- Administrator access to every machine in scope, and read access to the central collector that stores the audit records of those machines.
- The fleet lockfile, with the good version of the plugin pinned in it.
- The severity table of [Operational incident response](/pages/ops-incident-response/), which gives the acknowledgement time for each level.
- The [Administrator failure runbook](/pages/guide-admin-failure-runbook/), which defines the `Actions taken` line and the other entries of the incident record.

### Contain: deny, pin and kill

Containment comes first, before any investigation. Add the plugin id to `plugins.deny_ids`, or add the bad version to the file named by `packages.blocklist_file`. Either change stops the bad version from running on the machine, and it is the first entry of the incident record.

Write an `Actions taken` line for the change, with the time of the change and the name of the administrator who made it. The line must carry a time earlier than the first investigation entry in the record. Run `ilium plugin incident check <file>` to confirm the line is there. While the blocklist change has no `Actions taken` line with a time and an administrator, the check fails with `incident.action-unrecorded`.

The good version is pinned in the fleet lockfile during recovery, not during containment. If a running plugin must be stopped at once, use the steps in [Crash isolation, health and kill switch](/pages/ops-crash-isolation-health-and-kill-switch/).

### Find affected machines using audit and health data

Do not ask users which machines installed the release. The collector already holds the audit records that show it. Read the `install`, `update` and `update.rolled-back` records for the bad version. Query the collector for the `install` and `update` records that name the bad version; the machine ids in that result are the affected machines. The machine list in the incident record must equal that result, line for line.

Health data from the machines can show problems that the audit records do not show, such as a plugin that traps after it loads. It does not replace the audit query as the source of the machine list. The health reports are described in [Telemetry and plugin health](/pages/reference-health-and-watchdog/).

### Recover with lockfile sync or rollback

There are two routes. Choose the route for each machine separately.

Use the lockfile route for a machine whose plugin data was not changed by the bad release. Pin the good version in the fleet lockfile, then run `ilium plugin sync --locked` on the machine. The command downgrades the plugin to the locked version without `--allow-downgrade`, because the lockfile is the authorised set. A successful run exits with code 0, and `ilium plugin list` then shows the good version.

Use the rollback route for a machine whose plugin data is newer than the old version expects. That happens when the bad release raised the `data_version`. In that case `sync --locked` refuses the downgrade with `migration.data-newer` and leaves the profile unchanged, so the lockfile route cannot be used. Restore the old version from the backup archive as follows.

<div class="warn">

<p>Run the dry run of the archive before the rollback. A rollback from an archive that was not checked can leave the plugin without its data.</p>

</div>

1. Run `ilium plugin restore <file> --dry-run` on the backup archive. The command exits with code 0 and lists the archive members. If it does not exit with 0, stop and fix the archive before going on.
2. Run `ilium plugin rollback <id> --restore-backup`, with the plugin id in place of `<id>`. The command exits with code 0, and the old version is the one running.

### Confirm recovery

After recovery, run `ilium plugin incident check <file>` again. Every machine in scope must report the fixed version in `ilium plugin list`, or must report the plugin as denied. A machine that still reports the bad version is outstanding. While one in-scope machine is outstanding, the check exits with a non-zero code and prints `incident.machine-outstanding` with that machine id. Repeat the recovery on that machine, then run the check again.

### Close the incident record

Do not close the incident until every machine in scope reports the fixed version or reports the plugin as denied. The check in the previous section is the test for this. A passing check with no outstanding machines is the only result that allows the record to be closed.

Level 1 incidents have one more requirement. Acknowledge the incident within the Level 1 time of the severity table, which is one hour. Attach a support bundle built with `ilium plugin support-bundle <id>` only after the attach check of [Diagnostics and logs for operators](/pages/ops-diagnostics-and-logs/) has passed, so that the file name, the size and the warning about prompts and replies were shown first. A bundle attached without that check recorded fails `ilium plugin incident check <file>` with `incident.bundle-unchecked`.

## Requirements

- **[PDU-008]** The operator MUST contain first, by adding the plugin id to `plugins.deny_ids` or the version to the file named by `packages.blocklist_file`, before any investigation.
- **[PDU-009]** Containment MUST be recorded as an `Actions taken` line with time and administrator, as the incident runbook requires.
- **[PDU-010]** The operator MUST identify affected machines from the `install`, `update` and `update.rolled-back` audit records collected centrally, not by asking users.
- **[PDU-011]** Recovery by lockfile MUST use `ilium plugin sync --locked`, which downgrades to the locked version without `--allow-downgrade` ([USR-048]).
- **[PDU-012]** A machine whose plugin data is newer than the old version MUST recover with `ilium plugin rollback <id> --restore-backup`, and the operator MUST first verify the archive with `ilium plugin restore <file> --dry-run`.
- **[PDU-013]** The incident MUST NOT be closed until every machine in scope reports the fixed version in `ilium plugin list` or reports the plugin as denied.
- **[PDU-014]** Level 1 incidents MUST be acknowledged within the severity table's time, and the operator MUST attach a support bundle built with `ilium plugin support-bundle <id>` only after the [OPS-006] attach check passes.
