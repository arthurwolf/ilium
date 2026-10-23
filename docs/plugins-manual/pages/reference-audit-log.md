---
layout: page
title: Audit log
permalink: /pages/reference-audit-log/
---

## Audit log

The audit log is one tamper-evident record of security decisions: installs, grants, consents, trust, policy and sandbox events. It is not the plugin log. The plugin log mixes lines the plugin wrote with host lines, is rate limited and is trimmed. The pane journal records pane input and output for plugins that hold `journal.read`, and it is trimmed oldest first. Neither is fit to answer "who allowed what, and when". The audit log is.

A **hash chain** links each record to the one before it by including the previous record's SHA-256, so removing or changing a record breaks every later link.

### You will need

- The CLI: `ilium plugin audit`.
- Access to the user's state directory for the file itself.
- The stores described in [Grant review and consent](/pages/reference-grant-review-and-consent/).

## Location and access

**[SEC-220]** The log is written only by the kernel to `audit.jsonl` in the audit directory: Linux `$XDG_STATE_HOME/ilium/audit/`, macOS `~/Library/Application Support/Ilium/state/audit/`, Windows `%LOCALAPPDATA%\Ilium\state\audit\`. The file has mode 0600 in a 0700 directory. It is in the deny set ([SEC-060]): no plugin scope reaches it, and `journal.read` does not expose it.

## Record schema

**[SEC-221]** Each line is one JSON object:

| Field | Type | Meaning |
|---|---|---|
| `ts` | string | UTC ISO 8601 with milliseconds, for example `2026-10-12T08:14:02.114Z` |
| `seq` | int | Starts at 1, increases by 1; a gap is a defect |
| `prev_hash` | string | SHA-256 (hex) of the previous line's bytes without the trailing newline; the first record uses 64 zeros |
| `session` | string | Session name |
| `actor` | object | `{ "kind": "user-tui" or "cli" or "policy" or "kernel" or "plugin", "pane": "<pane id>"?, "plugin": "<id>"? }` |
| `event` | string | See the catalogue |
| `plugin_id`, `version`, `tier` | string | When the event concerns a plugin |
| `publisher_fingerprint`, `package_sha256` | string | When known |
| `grant`, `target` | string | Grant string or consent target |
| `result` | string | `ok`, `refused`, `cancelled`, `failed` |
| `reason` | string | Short code |

**[SEC-222]** Event catalogue. Each of these events MUST be written exactly once per occurrence, except the rate-limited ones.

| Event | When |
|---|---|
| `install`, `update`, `downgrade`, `uninstall` | End of the install pipeline, including cancelled and refused attempts |
| `enable`, `disable`, `failure-disable` | State change, with the actor |
| `grant-given`, `grant-revoked` | Each grant change |
| `consent-given`, `consent-withdrawn` | Each consent change |
| `trust-add`, `trust-remove` | Trust store change |
| `replacement-on`, `replacement-off` | A core plugin replacement is turned on or off |
| `policy-loaded`, `policy-violation` | Policy read at start; a plugin or file violates it |
| `sandbox-unavailable`, `sandbox-degraded` | Once per start and prerequisite |
| `signature-failure`, `integrity-failure`, `blocklist-hit` | Package or load checks fail |
| `process-run` | A T2 or T3 plugin runs a program, or a pane command starts unconfined ([SEC-066]) |
| `write-outside-data` | A write outside `data` and `cache` by a plugin |
| `egress-host-first-use` | First request of a plugin to a host ([SEC-087]) |
| `extension-disabled-after-breaches` | A contribution is stopped after repeated breaches |
| `store-external-edit` | A store changed outside the server ([SEC-113]) |
| `h-grant-refused` | Summary of refused H calls, at most one record per plugin and kind per minute, with a count |
| `project-trust` | A project is trusted or untrusted |
| `safe-mode` | The server started in safe mode |
| `audit-reset` | The server recreated a deleted log file, with `prev_hash` of zeros |
| `audit-dropped` | Records were dropped while the disk was full ([SEC-225]); written when space returns, with the count |

**[SEC-223]** Redaction. A record never contains terminal text, prompts, transcripts, file contents or request bodies. Argument values equal to a current plugin secret (and its base64 and percent-encoded forms) are replaced with `<redacted>` ([SEC-243]). Process arguments are logged in full otherwise.

## Retention and failure

**[SEC-224]** Rotation. The log rotates at `audit.max_mib` (default 64 MiB) or when the oldest record is older than `audit.keep_days` (default 365). The new file starts with a record whose `prev_hash` is the hash of the last line of the rotated file, and the old file is renamed `audit-<first seq>.jsonl` and kept until its age exceeds `keep_days`. Policy may change both values.

**[SEC-225]** Disk full. If a record cannot be written, the server keeps running, keeps the record in memory (up to 1,000 records), retries every 10 s, and shows a notice. When the in-memory queue is full, the oldest `h-grant-refused` and `egress-host-first-use` records are dropped first, no other record type is dropped while one of those two remains, the `audit-dropped` record is never dropped, and a `audit-dropped` record with the count is written when space returns.

**[SEC-226]** Verification. `ilium plugin audit --verify` re-computes the chain and prints `ok` or the first broken `seq`. The chain detects edits by someone who cannot also rewrite the whole file; an attacker with write access to the file can rebuild it. For stronger assurance the policy can set `[audit] sink` to `syslog` or `journald` ([SEC-228]).

## Viewing and export

**[SEC-227]** `ilium plugin audit [--plugin <id>] [--since <time>] [--event <name>] [--limit <n>]` prints matching records as JSONL (one `audit` object per line, then one `summary`). `--since` accepts an ISO 8601 time or a duration such as `24h`. Settings > Plugins > Activity shows the same records in a list.

**[SEC-228]** With `[audit] sink = "syslog"` or `"journald"` the server also sends each record to the system log with the identifier `ilium-audit`, one JSON object per message. The local file is still written.

## Example

```text
{"ts":"2026-10-12T08:14:02.114Z","seq":412,"prev_hash":"9c1f…","session":"default","actor":{"kind":"user-tui"},"event":"grant-given","plugin_id":"example.notes","version":"1.2.0","tier":"t2","grant":"fs.read:project","result":"ok"}
{"ts":"2026-10-12T08:14:02.240Z","seq":413,"prev_hash":"4ab7…","session":"default","actor":{"kind":"cli","pane":"shell-3"},"event":"install","plugin_id":"example.notes","version":"1.2.0","tier":"t2","package_sha256":"d5e0…","result":"ok"}
```

## Crash and core dump handling

Target behaviour (specified here before the code is written).

- [SEC-445] The server and every plugin helper MUST disable core dumps for themselves (`RLIMIT_CORE` 0 and the non-dumpable flag on Linux, the equivalent on macOS and Windows) so that a crash cannot write plugin secrets, granted file contents or tokens into a core file.
- [SEC-446] The crash report the server writes for itself or a helper MUST contain only the plugin id, the error code, the host version and a stack of host frames. It MUST NOT contain memory, argument values, secret values, granted data or file contents, and a redaction test MUST run over every report before it is stored.
- [SEC-447] Crash reports MUST be stored with mode 0600 in a 0700 folder beside the audit directory, MUST be deleted after 14 days or when the plugin is uninstalled, and MUST NOT be sent anywhere unless the user runs the explicit export command. Each report written MUST add one `crash-report` audit record ([SEC-222]) naming the file.

## Edge cases

| Situation | Result |
|---|---|
| The file is deleted while the server runs | The server recreates it with a record `audit-reset` and `prev_hash` of zeros; the loss is shown in a notice |
| Two servers (two sessions of one user) write | There is one file per user. Writers take a file lock, read the last `seq` and hash under the lock, and append. The `session` field tells them apart |
| Clock goes backwards | `ts` may decrease; `seq` stays the ordering |
| A plugin tries to read the file | `not-permitted`; the deny set applies |

### See also

- [Debugging](/pages/debugging/)
- [Capability: journal.read](/pages/capability-journal-read/)
- [Admin policy](/pages/reference-admin-policy/)
- [Secrets](/pages/reference-secrets/)

Contract: `ilium:plugin@0.1.0`.

## Production additions: T2-security

- **[SEC-730]** The audit log MUST be tamper-evident: each record MUST carry the SHA-256 of the previous record, `ilium plugin audit --verify` MUST report the first broken index with exit code 3, and rotation MUST write the last hash as the first record of the next file.
- **[SEC-955]** Crash record retention. One period and one count apply to every crash record of [SEC-447]: a crash record MUST be deleted once it is more than 30 days old (proposed, owner to confirm), and when more than 200 crash records exist the oldest MUST be deleted first until 200 remain (proposed, owner to confirm). The 14-day period in [SEC-447] is superseded by this rule and the owner MUST align that text; [USR-228], [USR-236] and [USR-348] already state 30 days and 200 records. `ilium plugin uninstall <id>`, with or without `--purge`, MUST delete every crash record of that plugin at once, so the `--purge` question for crash records is closed. Crash records are not deleted by `--keep-data`, which keeps data and storage only.
- **[SEC-956]** Damaged final record. If the audit file ends in a line that is not a complete JSON object followed by a newline (a crash or power loss during a write leaves one), the server MUST, at its next start and before it appends any record: move those partial bytes to `audit-torn-<seq>.jsonl` in the audit directory, where `<seq>` is the `seq` the next record will take; truncate `audit.jsonl` to the end of its last complete record; append one `audit-torn` record (a new event, listed here and added to the catalogue of [SEC-222]) with `prev_hash` equal to the hash of that last complete record and `reason` naming the moved file. The partial bytes are not part of the chain and MUST NOT be hashed. The server MUST write this record once per moved file, and MUST NOT drop the partial bytes without moving them aside.
- **[SEC-957]** Verification of a damaged final record. `ilium plugin audit --verify` MUST report a partial final line as `torn-tail` with its byte offset, and MUST NOT report it as a broken chain or as tampering. The chain check covers complete records only. A wrong `seq` or `prev_hash` in a complete record MUST still be reported as the first broken `seq` with exit code 3 ([SEC-730]). When the only finding is a `torn-tail` line, the command exits 0 (proposed, owner to confirm).

## Crash record location, one rule

Two pages gave two answers for where a crash record lives. [USR-229] says the server state folder. [SEC-447] says a 0700 folder beside the audit directory. This section names one path and replaces both statements.

**The path.** Crash records live in `<state>/crashes/`, where `<state>` is the server state folder: Linux `$XDG_STATE_HOME/ilium/`, macOS `~/Library/Application Support/Ilium/state/`, Windows `%LOCALAPPDATA%\Ilium\state\`. The audit directory of [SEC-220] is `<state>/audit/`, so the audit directory and the crash folder share the parent `<state>`. "Beside the audit directory" in [SEC-447] and "the server state folder" in [USR-229] both describe this one folder. The path is proposed, owner to confirm.

**Rules.**

- The folder has mode 0700 and each file mode 0600 ([SEC-447]). It is in the deny set ([SEC-060]): no plugin scope reaches it, and it is never inside a plugin data folder ([USR-229]).
- Records of plugins, of their helpers ([HST-518]) and of the host itself all go in this one folder. There is no second folder per kind.
- A file is named `<kind>-<id>-<time>.json`. `<kind>` is `plugin`, `helper` or `host`. `<id>` is the plugin id, or `host` for a host crash. `<time>` is UTC `YYYYMMDDTHHMMSSZ`. If two records get the same name, the later one gets `-2`, `-3` and so on; no record overwrites another.
- The content rules of [SEC-446] and [USR-227] apply: plugin id and version, side, reason, exit status, export name, host version and host frames, and no payloads, argument values, secrets or granted data. Core dumps are off ([SEC-445], [HST-519]); a dump an administrator enabled is never copied into this folder.
- Each record written adds one `crash-report` audit record ([SEC-447], [SEC-222]) with `target` set to the file name.

**Retention.** The rule is [SEC-955]: a record is deleted when more than 30 days old, and when more than 200 exist the oldest are deleted until 200 remain ([USR-228]). Both values are proposed, owner to confirm. The 14 days in [SEC-447] are superseded. `ilium plugin uninstall <id>`, with or without `--purge`, deletes every crash record of that plugin at once ([SEC-955]); `--keep-data` does not keep them. Host records (`host-host-<time>.json`) belong to no plugin and are removed only by age or count.

**Who reads the folder.** `ilium plugin crashes` and the support bundle ([USR-229]) read only `<state>/crashes/`. `ilium plugin crashes --host` lists the host records. A record outside this folder is not listed and not bundled. A crash record is never sent anywhere unless the user runs the export command or attaches it to a bundle ([SEC-447]). If the folder is missing, the command lists nothing and exits 0; the server recreates it with mode 0700 at the next crash. If the record cannot be written (disk full), the server keeps running, writes the `crash-report` audit record with `result` `failed` and `reason` `crash-write-failed`, and shows a notice.

**Requirements table change.** The row for [SEC-447] in the security-policy requirements file should read "Crash reports MUST be mode 0600 in `<state>/crashes/`, never be sent unprompted, and add a `crash-report` audit record; the 14-day period and the 'beside the audit directory' path are superseded by [SEC-955] and this section." The wording of [USR-229] stays valid.

## Audit records for data movement and erasure

The catalogue of [SEC-222] covers installs, grants, consent, trust and block events. A privacy or security review also needs to know when plugin data left the host or was destroyed, and a restore runbook needs the trail ([guide-backup-and-restore](/pages/guide-backup-and-restore/)). The events below are added to the catalogue. They are written exactly once per command run, by the kernel, at the end of the run, including refused and failed runs. Their names and fields are proposed, owner to confirm.

| Event | Command |
|---|---|
| `data-export` | `ilium plugin export <id> --out <file>`, `ilium plugin export-data` ([USR-245]) |
| `data-import` | `ilium plugin import-data <folder>` ([USR-262]) |
| `backup` | `ilium plugin backup <file.tar.zst>` ([HST-652]) |
| `restore` | `ilium plugin restore <file>` with `--merge` or `--replace`, and `--force`; a `--dry-run` writes none |
| `data-report` | `ilium plugin data-report [<id>]` ([USR-322]) |
| `erase-data` | `ilium plugin erase-data` ([USR-246]) |
| `data-prune` | `ilium plugin data prune` |
| `uninstall` with `reason` `purge` | `ilium plugin uninstall <id> --purge` or the Delete data choice ([HST-977]) |
| `purge-user` | `ilium plugin admin purge-user <user>` ([HST-689]) |

**Fields.** Each record uses the schema of [SEC-221] and adds these in the existing fields: `actor` is `cli` or `user-tui` (`policy` is never the actor of an erase); `plugin_id` names the plugin, or a comma-separated list of at most 20 ids followed by `+<n>` when more; `target` is the output path (export, backup), the input path (import, restore) or the user name (`purge-user`); `reason` carries the result code of the command; `result` is `ok`, `refused`, `cancelled` or `failed`. The flags used (for example `--merge`, `--force`, `--include-grants`, `--include-secrets`, `--yes`, `--yes-erase-all`) and the byte count and file count are written in a `detail` string of the form `flags=--merge,--force bytes=1048576 files=312`. Secret values are never written, and `--include-secrets` is recorded only as a flag.

**What a record never holds.** The data itself: no setting values, file contents, storage keys, log lines or grant strings from the moved data. [SEC-223] applies. A path is written as given on the command line after resolving it to an absolute path, because the path is not plugin data.

**Result codes.** The `reason` field uses the code the command printed, for example `restore.server-running` ([HST-974]), `purge.user-active` ([HST-689]) or `cancelled-by-user`. A run that exits non-zero writes `result` `failed` or `refused`.

**Rules for specific commands.**

- `restore` writes one record with the archive path, the mode (`merge` or `replace`), `--force` if given, and the counts of plugins restored and skipped. A refused restore into a live data root is recorded as `refused`.
- `erase-data` writes one record per run with the plugins erased and the byte and file count per data kind. It also names the audit records it kept ([USR-248]).
- `uninstall --purge` writes the normal `uninstall` record and the `detail` shows `purge=1`.
- Copies the administrator wrote elsewhere are not deleted by these commands and are not tracked by the record ([HST-977]).

**Which records the user can read, and which erasure keeps.** A user reads these records with `ilium plugin audit --event data-export` and so on ([SEC-227]). `ilium plugin export-data` includes the audit records that name that user's plugins ([USR-245]), so the user sees every export, restore and erasure involving their data. `ilium plugin erase-data` deletes data kinds, not the audit log: all of these records are kept for `audit.keep_days` (default 365) after the erasure ([SEC-224], [USR-248]) and the command lists them as kept with that period. They are deleted when the rotated file passes the period ([USR-220]). On a host where one user's records name another user (`purge-user` by an administrator), `export-data` includes only records whose `target` or `plugin_id` belongs to the requesting user.

**Failure modes.**

- If the audit file cannot be written, the command still completes and the record is queued and retried under [SEC-225]; `data-export` and `erase-data` records are not among the types dropped first.
- If the process is killed after moving data and before the record is written, the next server start finds no record. The command prints the same facts on its standard output before it exits, so the operator has them; this gap is why the record is written before the command prints its final line where possible.
- `--dry-run` and read-only listings (`data-report`) write `data-report` once but no record for a dry-run restore.
