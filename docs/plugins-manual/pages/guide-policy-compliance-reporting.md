---
layout: page
title: Policy compliance reporting
permalink: /pages/guide-policy-compliance-reporting/
---

## Why report

This page specifies how an operator proves which admin policy each machine actually runs, and how to spot drift between the policy loaded in the server and the policy file on disk. Administrators who must show an auditor or a change board what a machine enforces need these checks. Fleet operators who collect status from many machines need them too.

The reporting relies on three things: a local status command, an effective policy hash that appears in the audit log, and a collection recipe that gathers the status from each machine. The status command never contacts the network, so it works on isolated machines. The policy itself is described in [Admin policy](/pages/reference-admin-policy/) and [Plugin policy](/pages/reference-plugin-policy/).

## The policy status command

Run `ilium plugin policy status --json` on the machine to check. The command reads only local files. It prints one JSON object that contains `policy_sha256` of the system policy file, `loaded_at`, `schema_version`, an `origins` map that names `system` or `user` for each effective key, and `ignored_files` as an integer count.

Test the command on a machine that has a system policy and a stricter user file. The JSON must contain `policy_sha256`, `loaded_at`, `schema_version`, an `origins` map with `system` or `user` for each key, and `ignored_files` as an integer. Each key that the user file tightens appears with the origin `user`. Each key that comes only from the system file appears with the origin `system`.

The command must not open any network connection. To check this, run it in a network namespace that has no interfaces. It must exit with the same output and the same exit code as it does with networking available.

The output does not include values from the user file that are not stricter than the system file. If a user file sets a looser value for a key, the JSON shows only the system value for that key. The JSON then has no `notice` field.

## Effective policy hash

The `policy_sha256` value is the SHA-256 hash of the system policy file bytes, as the server read them at start. Compute it with `sha256sum` on the system policy file, for example `/etc/ilium/policy.toml` on Linux, right after the server starts. The value must match the `policy_sha256` field from the status command.

The same hash appears in the audit log. The `after` field of the latest `policy-reloaded` audit record holds this value. Auditors can therefore tie a running server to the exact bytes of the file that it loaded. The audit record format is described in [Audit log](/pages/reference-audit-log/).

## Drift detection

Drift means that the file on disk no longer matches the policy that the server loaded. This happens when an administrator edits the file but has not yet restarted the server or reloaded the policy.

To detect drift, append a comment line to the system policy file while the server runs, then run the status command. The JSON must then show `pending_restart: true`, and the command must exit with code 4. Drift is detected by comparing the hash of the file on disk with the loaded hash. Any difference, even one comment line, counts as drift.

## Collecting from a fleet

To collect status from many machines, run `ilium plugin policy status --json` on each machine through your existing management tool. Keep each JSON object together with the host name that produced it. Record the exit code next to each object, because the exit code carries the drift and ignored-policy state. A collection run can then compare `policy_sha256` across machines and list every machine with `pending_restart: true` or with `policy: "ignored"`.

Because the command reads only local files, the collection job does not need outbound network access beyond its own management channel. Store the collected JSON objects with the other compliance evidence for the review period.

## Failure cases

When the system policy file is ignored, the status command reports the problem instead of hiding it. The file is ignored when it fails the owner and mode check of [SEC-182]. A file writable by group or others is an example. In that case the command prints `policy: "ignored"` with a `reason` that names the permission problem, and it exits with code 3.

To test this case, make the system policy file world-writable and start the server. The status command must print `policy: "ignored"` with a `reason` that names the permission problem, and it must exit with code 3. Fix the file mode, restart the server and run the command again to confirm that the policy is loaded.

Drift and ignored policy need different responses. Drift with exit code 4 means that the administrator should restart the server or reload the policy so that the loaded policy matches the file. An ignored policy with exit code 3 means that the server does not enforce the system file at all, so the file ownership and mode must be corrected first.

## Requirements

- **[PDV-015]** `ilium plugin policy status --json` MUST print `policy_sha256` of the system file, `loaded_at`, `schema_version`, the origin (`system` or `user`) of each effective key, and the count of ignored files.
- **[PDV-016]** `policy_sha256` MUST be the SHA-256 of the system policy file bytes as read at start, and MUST equal the `after` hash of the latest `policy-reloaded` audit record.
- **[PDV-017]** When the file on disk differs from the loaded hash, the command MUST report `pending_restart: true` and exit with code 4.
- **[PDV-018]** If the system file was ignored under [SEC-182], the command MUST report `policy: "ignored"` with the reason and exit with code 3.
- **[PDV-019]** The status command MUST read only local files and MUST NOT open any network connection.
- **[PDV-020]** The output MUST NOT include notice text or user-file values that are not stricter than the system file.
