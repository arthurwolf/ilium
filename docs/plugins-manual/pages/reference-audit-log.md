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
