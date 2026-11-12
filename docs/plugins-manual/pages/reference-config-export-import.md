---
layout: page
title: Configuration export and import
permalink: /pages/reference-config-export-import/
---

## Configuration export and import

This page specifies the command and the file format that copy a plugin set from one machine to another. The export records the plugin ids, the exact versions and the non-secret settings of each plugin, and the import applies that file to a second machine. Administrators who keep several machines alike and developers who need to reproduce a setup on a clean machine both need this contract, because it defines what is copied, what is never copied, and how a bad file is rejected.

## What is exported

The command `ilium plugin config export --out <file>` writes one TOML file at the path given after `--out`. The file lists every installed plugin by its plugin id and exact version, followed by that plugin's non-secret settings. Each plugin appears with its own section, so a reader can see which values belong to which plugin without any other file.

The export contains only the data listed above. Secrets, keyring entries, capability grants, consent records and the per-plugin random identifier are never written to the file. The section on secrets and machine-bound data lists these exclusions in full.

## File format and version

Every export file starts with the line `version = 1`. The file is TOML, and the export command writes it with the file mode `0600`, so only the owner of the file can read it. A reader can check the mode with `stat` on Linux and confirm that the file holds both plugin ids and their exact versions.

The version number tells an importer which layout to expect. An importer that supports version 1 reads version 1 files. A file that declares a higher version than the importer supports is refused with the reason `config-version-unsupported` and exit code 2.

## Import modes and conflicts

The command `ilium plugin config import <file>` checks the whole file first. Every plugin section is validated against that plugin's settings schema, which is described in [Settings schema](/pages/reference-settings-schema/). If any value fails, whether it has the wrong type or the key does not exist, the import applies nothing. The output names the invalid key, and the command exits with code 2. A file with one valid plugin section and one invalid setting therefore leaves both plugins unchanged.

Before a real import, you can run `ilium plugin config import --dry-run <file>`. The dry run writes nothing to the settings store. It prints one JSON line for each change, and each line carries the `plugin_id`, the `key`, the `old` value and the `new` value. A file that changes three settings produces three such lines, and the settings store has the same hash afterwards as before the dry run.

<div class="note">
<p>Run the dry run first on any machine where the current settings matter. It shows every value the import would replace, so you can check the list before anything changes.</p>
</div>

## Interaction with admin policy

An administrator can lock a setting in the system policy file. When an import contains a locked key, the importer skips that key, lists it in its output with the word `locked`, and keeps the value that the policy sets. The import never edits the policy file, so the hash of that file is the same after the import as before it. The rules for writing the policy file are in the [Admin policy reference](/pages/reference-admin-policy/).

A locked key does not stop the rest of the file from applying. Every unlocked key in a valid file is still applied, as long as the whole file passes validation.

## Secrets and machine-bound data

The export is safe to copy between machines, because it carries no secret values. A secret stored for a plugin is not written to the file, and the file never contains a key named `secret`. The keyring entries are not written either. Grants and consent records stay on the machine where the user gave them, so a second machine asks again for any permission that a plugin needs. The per-plugin random identifier, which is named `instance_id`, is also left out, because it belongs to one installation. The storage of secrets is described in [Secrets](/pages/reference-secrets/).

## Audit records and errors

Each export and each import writes one audit record. The record has the event name `config-export` for an export and `config-import` for an import. Its `detail` field holds the byte count and the file count of the file, under the names `detail.bytes` and `detail.files`. The record never holds a setting value, so reading the audit log does not reveal the contents of the settings. The audit log is described in [Audit log](/pages/reference-audit-log/).

Two kinds of failure end an import with exit code 2: a file that fails validation, and a file whose version is higher than the supported version. In both cases the output gives the reason, and the settings store is left unchanged. A dry run that fails validation exits with the same code and writes nothing.

## Requirements

- **[PDV-008]** `ilium plugin config export --out <file>` MUST write a TOML file with `version = 1`, mode 0600, listing plugin ids, exact versions and each plugin's non-secret settings.
- **[PDV-009]** The export MUST NOT contain secrets, keyring entries, grants, consent records or the per-plugin random identifier.
- **[PDV-010]** `ilium plugin config import <file>` MUST validate the whole file against every plugin's settings schema before applying anything, and on any error MUST apply nothing and exit with code 2.
- **[PDV-011]** Import MUST skip any key the admin policy locks, list it as `locked` in its output, and MUST NOT modify the policy file.
- **[PDV-012]** `ilium plugin config import --dry-run <file>` MUST print each change as one JSONL line and MUST write nothing.
- **[PDV-013]** Import of a file whose `version` is higher than the supported version MUST fail with reason `config-version-unsupported` and exit code 2.
- **[PDV-014]** Each export and import MUST write one audit record of event `config-export` or `config-import` with the byte count and file count in `detail` and no setting values.

## Production additions (round 12)

- **[PRP-026]** The system must provide ilium config export and ilium config import using one TOML profile of enabled state, non-secret settings and keymap, with a dry-run diff, no import of secrets, grants or trust decisions, per-key unknown key reports with exit code 1, and idempotent re-import.
