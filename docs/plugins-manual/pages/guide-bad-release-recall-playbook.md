---
layout: page
title: Recalling a bad plugin release
permalink: /pages/guide-bad-release-recall-playbook/
---

## Recalling a bad plugin release

This page is the playbook a publisher follows when a released plugin version turns out to be defective. It puts detection, yank, hotfix, verification, notice and the post-incident record into one ordered procedure with fixed deadlines, so that the publisher does not improvise during an incident. It is written for the author of a plugin who must take a bad version out of circulation. The actions a publisher can take on a version are described on the [Yank, deprecate and takedown](/pages/publish-yank-deprecate-takedown/) page, and the steps for refreshing a mirror are on the [Mirror runbook](/pages/publish-mirror-runbook/).

### You need

You need the plugin identifier, the version that is defective, and the version that will replace it. You need the `ilium` command with the plugin publishing commands available, and an account that can publish and yank versions of the plugin. You also need a test machine that has the bad version installed, so that you can check the rollback before you tell users to use it. Keep a copy of the incident record template at hand. Its sections are `Mirrors and bundles`, the timestamps and the installs count.

### Decide: yank, deprecate or hotfix

A confirmed defect in a released version leads to a yank of that version and a hotfix. A yank stops the registry from offering the bad version to new installs. A hotfix is a new version, with a higher version number, that replaces the bad one for everyone who updates. Deprecation does not replace a defective version, so it is not part of this playbook. Use the decision table on the [Yank, deprecate and takedown](/pages/publish-yank-deprecate-takedown/) page when the defect is not clearly in one release.

### Ordered steps with deadlines

Follow the steps in this order.

1. Confirm the defect and record the confirmation timestamp in the incident record. A Level 1 or Level 2 defect starts the deadline below.
2. Yank the bad version within 1 hour of that confirmation. Run `ilium plugin yank <id>@<version> --reason <text>`, for example `ilium plugin yank example.sync@1.2.0 --reason "defect: bad build"`. The reason must begin with `security:` for a security defect and with `defect:` for any other defect. The registry refuses any other prefix with `yank.reason-prefix`, and the command then exits with a non-zero code. A reason such as `bad build` without a prefix is therefore rejected.
3. Publish the hotfix with a version number higher than the yanked release. A lower number is refused with `package.downgrade`. For example, publish 1.2.1 after yanking 1.2.0. Publishing 1.1.9 after that yank fails with `package.downgrade`.
4. Keep the data version unchanged unless the hotfix ships a migration from the yanked version's data. Run `ilium plugin check` on the hotfix before you publish it. A hotfix that raises `data_version` without a migration produces `lint.hotfix-data-version` as an error.
5. Keep the previous good version downloadable. The rollback command needs it as a target.
6. List every offline mirror and bundle in the incident record, and mark each one as refreshed or outstanding.
7. Write the post-incident record within 5 working days of the incident. Its fields and checks are described in the section below.

### Verifying that installs moved off the bad version

After the yank and the hotfix, check the result on the test machine that has the bad version. Run `ilium plugin rollback example.sync` on that machine. The command must exit with code 0, and `ilium plugin list` must then show the previous good version. Then update the test machine to the hotfix and confirm that `ilium plugin list` shows the hotfix version. A rollback that refuses with `migration.data-newer` means the hotfix raised the data version without a migration, so go back to step 4 of the ordered steps.

Count the installs that were affected, because the post-incident record requires that number. Record the count you have, and say in the record how you obtained it.

### Notice to users and administrators

Tell the users of the plugin and their administrators that the bad version has been yanked. The notice should name the yanked version, the hotfix version that replaces it and the rollback command for a machine that still has the bad version. Send it after the yank is in effect, so that the notice never points users to a version that the registry still offers. Keep the notice short and factual.

### Post-incident record

Write the post-incident record within 5 working days. It must name four values: the detection time in `detected_at`, the yank time in `yanked_at`, the hotfix time in `hotfix_at` and the number of installs affected in `installs_affected`. Check the file with `ilium plugin incident check <file>`. The check exits with a non-zero code and names any of the four fields that is missing. The record also contains the `Mirrors and bundles` section described in the next section.

### Edge cases: already-migrated data, offline mirrors

Some users may already have run a migration that the bad version performed. A rollback to an earlier version is refused with `migration.data-newer` when the stored data is newer than the version being restored. The safe way to avoid this is to leave the data version unchanged in the hotfix, as step 4 requires. If a hotfix must change the data version, it must ship a migration from the yanked version's data. The rules for migrations are in the [Data migration guide](/pages/guide-data-migration/).

A yank does not reach a bundle or a mirror that already holds a copy of the bad version. The incident record therefore lists every offline mirror and bundle that the publisher knows of. Each entry must carry a `refreshed` or `outstanding` mark and a date. An entry marked `outstanding` stays open until someone refreshes that mirror or bundle. The steps for refreshing a mirror are in the [Mirror runbook](/pages/publish-mirror-runbook/).

## Requirements

- **[PDU-001]** The publisher MUST run `ilium plugin yank <id>@<version> --reason <text>` within 1 hour of confirming a Level 1 or Level 2 defect.
- **[PDU-002]** A yank reason MUST begin with `security:` for a security defect and with `defect:` otherwise, and the registry MUST refuse any other prefix with `yank.reason-prefix`.
- **[PDU-003]** The hotfix MUST carry a higher version number than the yanked release, because a lower one is refused with `package.downgrade`.
- **[PDU-004]** A hotfix MUST NOT raise `data_version` unless it ships a migration from the yanked version's data, because rollback is otherwise refused with `migration.data-newer`.
- **[PDU-005]** The publisher MUST keep the previous good version downloadable so that `ilium plugin rollback <id>` has a target.
- **[PDU-006]** The incident record MUST list every offline mirror and bundle the publisher knows of, because a yank does not reach a bundle already copied, and each MUST be marked refreshed or outstanding.
- **[PDU-007]** The post-incident record MUST be written within 5 working days and MUST name the detection time, yank time, hotfix time and the number of installs affected.

## Production additions (round 3)

- **[PRY-030]** A host that is online MUST fetch a recall notice and disable the recalled version within 15 minutes of its publication, record audit event `recall.applied` with the plugin id and version, and `ilium plugin list --json` MUST show `recalled: true` for that version.
