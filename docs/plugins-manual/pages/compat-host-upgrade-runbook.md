---
layout: page
title: Host upgrade runbook
permalink: /pages/compat-host-upgrade-runbook/
---

## Host upgrade runbook

This runbook is for an operator whose Ilium upgrade leaves some installed plugins refused, with `package.api-unsupported` or `manifest.api-version`. It sets the order of the work: triage, pin, roll back, then the bulk report. Each step names the command and the state it leaves behind. The runbook never changes plugin files, grants or stored settings, and it does not need a network connection unless a rebuilt package has to be downloaded.

## You will need

- A shell on the machine that runs Ilium, with the `ilium` command of the new version and of the previous version.
- The release notes of the new host, which state the window (see [Deprecation timeline](/pages/compat-deprecation-timeline/)).
- [Load and refusal decisions](/pages/compat-load-decision-table/) to read the refusal codes.
- The failure runbook for the other incident classes: [Administrator failure runbook](/pages/guide-admin-failure-runbook/).

## Before the upgrade: inventory

An inventory before the upgrade shows which plugins the new window will refuse. Take it while the old host still runs.

1. Save the list of installed plugins: `ilium plugin list --json > before.json`.
2. Save the health and metrics of each plugin: `ilium plugin status --json > status-before.json`.
3. Compare the `api` value of each installed package with the window of the new host from its release notes.

- **[COMP-015]** Before a host upgrade, the operator MUST save the list and the status of every installed plugin, so that a refusal after the upgrade can be compared with the state before it.
- Proposed, owner to confirm: `ilium plugin list --json` carries the `api` value of each installed package, so that step 3 needs no unpacking of the store.

## Step 1: triage

Triage answers two questions for each refused plugin: why it is refused, and whether any other plugin is refused for the same reason.

1. Read the refusal code and the plugin id: `ilium plugin list --json` and `ilium plugin status <id> --json`.
2. Read the audit line of the refusal: `ilium plugin audit --export <file>` writes the audit log, and the line carries the code, the id and the `api` value of the package.
3. Check the host version: the window of the running host is in its release notes. A refusal of `package.api-unsupported` means the package's minor left the window. A refusal of `manifest.api-version` means the package is damaged or was built with a mixed SDK, and no host upgrade fixes it.
4. Check the health counters for the ABI class. [Health and watchdog](/pages/reference-health-and-watchdog/) lists the per-code counter, labelled by plugin id.

| Code | Cause | Is a rollback of the host the fix? |
|---|---|---|
| `package.api-unsupported` | The `api` minor left the window | Yes, if the old host is still needed for this plugin |
| `manifest.api-version` | The section differs from the `api` value | No; the author must rebuild |
| `manifest.unstable-unknown` | A feature name was removed or renamed | No; the author must update the manifest |

- **[COMP-016]** Triage MUST classify every refused plugin by its code before any other step, and it MUST leave each plugin's files, grants and data unchanged.

## Step 2: pin

A pin keeps a plugin on the version that the host still accepts while the author prepares a new one. `ilium plugin pin <id>@<version>` records the exact version. Update commands skip a pinned plugin, and the resolver accepts only the pinned version ([PKG-358] of [Install, update and uninstall](/pages/reference-install-and-update/)).

A pin does not make a refused plugin load. It keeps the plugin at the version it is on, so that an update does not replace a working version with one that is also refused.

<pre class="term"><span class="t-prompt">$</span> ilium plugin pin notes-caller@0.1.0
<span class="t-ok">ok</span>   notes-caller pinned at 0.1.0
<span class="t-prompt">$</span> ilium plugin unpin notes-caller
<span class="t-ok">ok</span>   notes-caller unpinned
</pre>

Illustrative, not generated.

- **[COMP-017]** A pin set during triage MUST be listed in the bulk report of step 4, and it MUST be removed with `ilium plugin unpin <id>` once a version that the host accepts is installed.

## Step 3: roll back

There are two different rollbacks. Use the one the refusal calls for.

| Rollback | Command | When to use it | What it changes |
|---|---|---|---|
| Plugin | `ilium plugin rollback <id>` | A newer version of the plugin is the one refused, and the previous version on disk is accepted | Makes the one previous version in the store active. Grants are kept where the old manifest still asks for them ([PKG-290]) |
| Plugin data | `ilium plugin rollback <id> --restore-backup` | Data written by the new version cannot be read by the old one | Restores the copy taken before the update. Data written since is lost |
| Host | The previous Ilium build, installed by the project's documented installer | Several plugins are refused and the author cannot publish a fix in time | Changes the host only. Plugin files, grants and settings do not change |

<div class="warn">
<p>`--restore-backup` discards what was written after the update. Use it only when `migration.data-newer` or a data error blocks the plugin, and only after the audit export of step 1 has been saved.</p>
</div>

A host rollback is the way to load a plugin again after an upgrade that refused it. The ABI rules make that safe: the host does not change plugin files, grants or stored settings on a rollback, and a plugin refused after the upgrade loads again unchanged.

- **[COMP-018]** A host rollback MUST NOT change installed plugin files, grants or stored settings, and a plugin refused after the upgrade MUST load again unchanged once the earlier host that accepts its `api` runs again ([ABI-078] of [ABI major versions](/pages/reference-abi-major-versions/)).
- Proposed, owner to confirm: `--restore-backup` is used only after the audit export of step 1 has been saved, so that the data it discards can be traced.
- **[COMP-019]** A plugin rollback whose stored data version is higher than the target supports MUST be refused with `migration.data-newer`, and the operator MUST NOT force it through a restore without the audit export of step 1.

Check the state after each rollback with `ilium plugin list --json` and compare it with `before.json` from the inventory. A plugin that moved back to a state it had before the upgrade is done. A plugin in a different state needs another step.

## Step 4: bulk report

A bulk report lists every plugin refused for an ABI reason, so that the operator can send the author one list and track each row until it is fixed.

The host provides one command for this purpose. Its name and output are not fixed yet (proposed, owner to confirm: `ilium plugin list --refused-abi --json`, one line per plugin with the id, the installed version, the refusal code and the declared `api`). Until it exists, build the list from `ilium plugin list --json` by keeping the rows whose state is a refusal.

Each row of the report carries:

- the plugin id and the installed version,
- the refusal code,
- the `api` value of the package,
- the window of the host that refused it,
- whether the plugin is pinned, and
- the action taken: pinned, rolled back, or left for the author.

- **[COMP-020]** The bulk report MUST contain one row per refused plugin, in one output, with the refusal code and the declared `api` value of each.

## Worked example: a host moves from 0.2.0 to 0.3.0

The window of the old host is `0.1` and `0.2`. The new host, `0.3.0`, has the window `0.2` and `0.3`, so a plugin built for `0.1` is refused.

1. Before the upgrade, `before.json` lists `notes-caller 0.4.1` with `api` `ilium:plugin@0.1.0`, and `title-tools 1.2.0` with `api` `ilium:plugin@0.2.0`.
2. After the upgrade, `notes-caller` is refused with `package.api-unsupported`. `title-tools` is active.
3. Triage shows one plugin for the code `package.api-unsupported`. The author is asked for a build against `0.2` or `0.3`.
4. A plugin rollback does not help `notes-caller` if the previous version on disk was also built for `0.1`. The operator checks the previous version in `before.json`. If it is also `0.1`, the rollback is skipped and the plugin stays refused.
5. The operator keeps the host at `0.3.0`, because `title-tools` needs it, and lists `notes-caller` in the bulk report as left for the author.

If the operator decides that `notes-caller` matters more than the new host, the host rollback of step 3 brings it back. `title-tools` also loads on the older host, because its `api` `0.2` is inside that host's window.

## Edge cases

- Two installed versions of one plugin that are both refused: the rollback does not help. The author must publish a build for the window.
- A refusal that appears only after a second restart: the audit line of that start has the code. Compare with the inventory before concluding that the upgrade caused it.
- A plugin that was pinned before the upgrade and is refused after it: the pin does not change the refusal. Keep the pin so that updates stay on the pinned version.
- An upgrade of the host while a plugin is in the middle of a data migration: the migration rules of [Data migration](/pages/guide-data-migration/) apply, and the host rollback does not reverse a migrator that already ran.

- **[COMP-021]** A runbook run MUST leave the audit export of step 1 and the two `ilium plugin list --json` files in the operator's own folder, not in the plugin store.

## What the operator tells the author

The operator sends the authors a message only when the operator decides to, and the message names the refusal code, the plugin id, the `api` value and the window that the host supports. The text of a host message can change in a minor release ([ABI-082]), so quote the code and not the sentence.

## See also

- [Compatibility and upgrades](/pages/guide-compatibility-and-upgrades/) for the rules an author follows
- [Load and refusal decisions](/pages/compat-load-decision-table/) for the decision order and the state after an upgrade
- [Deprecation timeline](/pages/compat-deprecation-timeline/) for the removals that can refuse a package
- [Administration guide](/pages/guide-administration/) for the policy that governs updates in a fleet
- [CLI output stability](/pages/reference-cli-output-stability/) for the fields that the scripts above may parse

## Open decisions

- The name and output of the bulk-report command (proposed above, owner to confirm).
- Whether `ilium plugin list --json` carries the `api` value (proposed above, owner to confirm).

API version checked: `ilium:plugin@0.1.0`.
