---
layout: page
title: Administrator failure runbook
permalink: /pages/guide-admin-failure-runbook/
---

## Administrator failure runbook

Use this page when a plugin on a managed machine causes trouble. Find the symptom in the first table, then follow its section. Each section gives the exact `ilium` commands, the policy keys that stop a plugin, the audit events to look for, and the time limits that decide what happens next. The stance and the policy file itself are explained in the [Plugin administration guide](/pages/guide-administration/).

Contract version: `ilium:plugin@0.1.0`. Requirement ids have the form `[USR-nnn]`, collected in [User and administrator requirements](/pages/reference-requirements-user-admin/).

<div class="note">
<p>Decision status. The platform is not implemented. The commands and keys below are the ones the other pages of this manual define. Items the manual does not settle are listed in [Open decisions](#open-decisions).</p>
</div>

### You need

- Shell access as the administrator, and the policy file for the machine (`/etc/ilium/policy.toml` on Linux, see [Administration guide](/pages/guide-administration/#write-the-policy)).
- The id of the plugin, and for a key problem the `key_id` of the signing key (`ilium plugin trust list --json`).
- The audit log of each affected user, at `$XDG_STATE_HOME/ilium/audit/audit.jsonl` on Linux (default `~/.local/state/ilium/audit/audit.jsonl`).

### Triage by symptom

| Symptom | Section | First command |
|---|---|---|
| A plugin traps, is slow, or shows `degraded` or `failed` | [Bad plugin](#bad-plugin) or [Watchdog timeouts](#watchdog-timeouts) | `ilium plugin status <id> --json` |
| An update failed, or the plugin went back to an older version | [Failed update](#failed-update) | `ilium plugin status <id> --json` |
| Ilium stops or restarts again and again at start | [Crash loop](#crash-loop) | `ilium --safe-mode=third-party` |
| Requests from a plugin time out | [Watchdog timeouts](#watchdog-timeouts) | `ilium plugin status <id> --json` |
| A publisher's signing key is known or suspected to be leaked | [Compromised signing key](#compromised-signing-key) | `ilium plugin trust list --json` |

### Bad plugin

A plugin that misbehaves but involves no key follows these steps. Each step is reversible until the last.

1. **Get a working screen.** Run `ilium --safe-mode=third-party`. This loads no T2 or T3 plugin and changes nothing on disk. The flag is not stored, so the next normal start returns to the earlier state. [LIF-180]
2. **Find the plugin, if it is not known.** Run `ilium plugin bisect start`, then answer `good` or `bad` after each test, until the record names one plugin in `found`. Finish with `ilium plugin bisect reset`. The bisect changes only the session, never the saved set. [LIF-184]
3. **Stop it for everyone.** Add the id to `plugins.deny_ids` in the policy file. Ilium reads the policy at server start, so the plugin is disabled at the next start and shows `Blocked by policy`. Its data stays on disk. A running plugin is not stopped by an edit alone.
4. **Remove it.** Run `ilium plugin uninstall <id>`. Without `--purge` the data folder, storage and stored settings stay, and grants and failure counters are deleted. Run `ilium plugin uninstall <id> --purge` only when the data is no longer needed; it also deletes the plugin log and the compiled cache for the plugin. [PKG-294, PKG-295]
5. **Check who had it.** Search the audit log for the plugin id: `jq -r 'select(.plugin_id=="<id>")' "$XDG_STATE_HOME/ilium/audit/audit.jsonl"`.

Policy keys that stop an installed plugin: `plugins.deny_ids` (glob on ids, refusal `policy.id-denied`), `packages.allowed_tiers`, `packages.allow_third_party` and the publisher rules. `plugins.allow_ids` refuses ids outside its globs, and `grants.deny` only removes the grant; the plugin keeps running without it. [SEC-186, SEC-198, admin policy]

Audit events to look for: `policy-loaded`, `policy-violation`, `safe-mode`, `grant-revoked`.

### Failed update

An update that fails is handled by the host first. The rules below say when it rolls back by itself and when you do it by hand.

- **Automatic rollback.** If the first `activate` of the new version returns an error, traps or misses its deadline, the host returns to the previous version at once, marks the new version `skip`, logs `update.rolled-back` and shows the toast `Update failed, rolled back`. This failure is not counted in the failure window. [LIF-075]
- **No automatic rollback later.** A failure after a successful first `activate` follows the normal failure window (see [Watchdog timeouts](#watchdog-timeouts)) and never rolls back by itself. Use the manual steps below.
- **Manual rollback.** Run `ilium plugin rollback <id>`. It makes the one previous version in `store/` active again. Only one previous version is kept on disk, so a second rollback is not possible. With no previous version the command fails with `install.no-previous`. [PKG-290, LIF-076]
- **Data newer than the old version.** A rollback to a version whose `data_version` is lower than the stored data is refused with `migration.data-newer`, exit 1. Choose `--restore-backup` (restore the pre-update snapshot) or `--discard-data` (drop the newer data) explicitly. The host never converts newer data downward. [PKG-288]
- **Skip mark.** A rolled-back version is not offered by `ilium plugin update` until a newer version exists or someone runs `ilium plugin install <file> --unskip`. [PKG-292]
- **Interrupted update.** A crash or power loss before the registry commit leaves `installed.json` unchanged; the old version keeps running. Run the update again. [PKG-269, PKG-271]
- **Lock held.** A second transaction that waits 30 s for the install lock fails with `install.locked`, exit code 4. Wait for the first to finish; the lock releases on process exit.
- **Staged candidate that never switches.** A staged update becomes active when the plugin is idle or at the next server start. `ilium plugin restart <id>` forces the switch. [PKG-278]

Audit and log events: `update.rolled-back`. Policy keys: none needed; to block a bad release use `publishers.deny` (see [Compromised signing key](#compromised-signing-key)) or `plugins.deny_ids`.

### Crash loop

A crash mark is written as soon as a start begins and removed when startup completes. A mark left behind means that start did not finish. [LIF-186, LIF-294]

- **The guard.** Three crashes during start within 5 minutes make the next start use `third-party` safe mode and open the recovery page. The guard clears after one start that stays up for 5 minutes after startup completed. A stop before that point keeps the marks.
- **What to do.** Run `ilium --safe-mode=third-party` and follow [Bad plugin](#bad-plugin) steps 2 to 4 to find and stop the plugin that the crashes point to.
- **Evidence.** Before you remove anything, keep a copy of the crash records: `ilium plugin support-bundle <id> --out <file>` writes a zip with `summary.txt`, `log.txt`, `stats.txt` and `crashes.txt`. The `<file>` must not exist already; the command fails with `support-bundle.exists` otherwise. [USR-221, DEV-832]
- **Helper crashes.** A helper is restarted at most 5 times in 10 minutes. After the sixth crash the T2 and T3 plugins of that session stop with the reason `helper.unstable`. Clear each affected plugin with `ilium plugin reset-failures <id>` once the cause is fixed. [LIF-134]

Audit and log events: `safe-mode` (written at each safe-mode start). Crash records live in the server state folder, not in the plugin data folder. [USR-229]

### Watchdog timeouts

Every export call runs under a deadline. A call that runs past its deadline is interrupted by the host; a slow helper is killed. [LIF-125, LIF-132]

- **Failure window.** Failures are counted per plugin, per side and per scope in a sliding window of 10 minutes. The first counted failure restarts the side after 1 s, the second after 5 s, and the third stops that side with `stopped-by-failure`. No restart is scheduled after the third. [LIF-027, LIF-028, LIF-029, LIF-030]
- **Notice text.** The user sees `Plugin <name> stopped after 3 failures in 10 minutes: <export> <code or trap>. Turn it on again in Settings > Plugins.` [USR-130]
- **Hung helper.** The server pings the helper every 1 s. Three missed answers, or no reply 500 ms after a deadline, make the server send SIGTERM and then SIGKILL after 2 s. The event is `helper.hung`, and one failure is counted. [LIF-132, HST-171]
- **Requests with no reply.** A request that gets no reply within its timeout completes as `timed-out` for the plugin. The examples given are 30 s for a command or service reply and 120 s for inference. [LIF-131]
- **Degraded, not failed.** `degraded` is shown, not stopped, when a threshold in [Health and watchdog](/pages/reference-health-and-watchdog/) is met: 5 or more near-misses in 5 minutes, 768 or more of 1,024 queued events in 1 minute, or 1 or more rate-limit or quota results in 5 minutes.
- **Recovery.** Run `ilium plugin reset-failures <id>` to clear the window and the failure stop. Enabling the plugin, or installing an update, clears it too. [LIF-036]
- **Metrics.** Counters are kept in memory for 24 hours, in hourly buckets, and reset at each server restart. Read them with `ilium plugin status <id> --json` before a restart if you need them. [LIF-135, LIF-136]

Accuracy: a hard deadline of `D` traps the call between `D` and `D` + 20 ms of wall time on a host that is not overloaded. Do not expect a call to trap at exactly `D`. [LIF-125]

### Compromised signing key

Use this section when a publisher's private key may be exposed. The key controls who may install that publisher's plugins, so each step below stops the key's plugins at the next opportunity.

1. **Revoke the key on each machine.** Run `ilium plugin trust revoke <key_id> --reason compromised`. The entry is written to the user's trust store. Plugins signed by the key move to `disabled-blocked` with the reason `trust.key-revoked`, and they stay off until a user runs `enable --accept-risk` or installs an update signed by another trusted key of the same publisher (lifecycle row P16). [PKG-243; lifecycle row P15]
2. **Block it by policy on managed machines.** Put the 64-character fingerprint in `publishers.deny`, or remove it from `publishers.allow`. A plugin signed by a denied key is refused with `policy.publisher-denied`, and an installed plugin that violates a publisher rule is disabled at the next start. [SEC-186, admin policy]
3. **Find the plugins.** Run `ilium plugin trust list --json` for the key status, then `ilium plugin status --json` for each installed plugin.
4. **Do not remove the key instead.** `ilium plugin trust remove <key_id> --force` also disables the plugins, but they return as soon as the key is trusted again. Use `revoke` for a compromised key. [PKG-226]
5. **Leave users a way forward.** A `disabled-blocked` plugin starts again only with `ilium plugin enable <id> --accept-risk`, or with an update signed by another trusted key of the same publisher. Tell users which of the two applies.

Audit events to look for: `trust-add`, `trust-remove`, `signature-failure`, `blocklist-hit`, `policy-violation`. The revocation itself is logged as `trust.key-revoked`.

If the publisher still controls the key, the publisher can issue a rotation statement to a new key. That is the publisher's step and the host accepts it only from a key that is currently trusted for the same publisher. [PKG-238, PKG-241]

### Time limits

| Limit | Value | Source |
|---|---|---|
| Deadline trap window | `D` to `D` + 20 ms | [LIF-125](/pages/reference-health-and-watchdog/) |
| Heartbeat to a helper | every 1 s; a hung helper after 3 missed answers (3 s) | [LIF-132](/pages/reference-health-and-watchdog/) |
| Helper past its deadline | 500 ms before the helper is killed | [LIF-132](/pages/reference-health-and-watchdog/) |
| Kill sequence | SIGTERM, then SIGKILL after 2 s | [HST-171](/pages/reference-host-helper/) |
| Failure window | 10 minutes, sliding | [LIF-027](/pages/lifecycle/#failure-counting-and-restart) |
| Restart after first and second failure | 1 s, then 5 s | [LIF-028, LIF-029](/pages/lifecycle/#failure-counting-and-restart) |
| Helper restarts before the side stops | at most 5 in 10 minutes | [LIF-134](/pages/reference-health-and-watchdog/) |
| Crash-loop guard trigger | 3 crashes during start within 5 minutes | [LIF-186](/pages/user-safe-mode-and-troubleshooting/) |
| Crash-loop guard reset | one start that stays up 5 minutes after startup | [LIF-294](/pages/user-safe-mode-and-troubleshooting/) |
| Install lock wait | 30 s, then `install.locked` | [PKG-270](/pages/reference-install-and-update/) |
| Audit queue when the file cannot be written | up to 1,000 records, retry every 10 s | [Administration guide](/pages/guide-administration/#keep-the-audit-trail) |
| Automatic update check (when enabled) | at server start, then every 24 hours with up to 1 hour of random delay | [PKG-301](/pages/reference-install-and-update/) |
| Metrics retention | 24 hours, hourly buckets, reset at restart | [LIF-135, LIF-136](/pages/reference-health-and-watchdog/) |
| Security report acknowledgement | 3 working days; first assessment 10 working days | [SEC-321](/pages/reference-security-response/) |
| Vulnerability embargo | 90 days from the report, or until a fix | [SEC-321](/pages/reference-security-response/) |
| Publisher silence before a blocklist entry | 30 days | [SEC-326](/pages/reference-security-response/) |

### Policy keys used in this runbook

| Key | Type | Effect here |
|---|---|---|
| `plugins.deny_ids` | list of globs | Stops a plugin by id at the next start |
| `plugins.allow_ids` | list of globs | Only these ids may run |
| `packages.allowed_tiers` | list | Limits the tiers that install |
| `packages.allow_third_party` | boolean | Refuses all third-party plugins when false |
| `publishers.allow` | list of fingerprints | Only these signing keys are accepted |
| `publishers.deny` | list of fingerprints | Refuses these signing keys |
| `packages.blocklist_file` | path | Extra local blocklist |
| `grants.deny` | list | Removes grants; does not disable the plugin |
| `audit.sink`, `audit.max_mib`, `audit.keep_days` | | Where the audit log goes, and its rotation |

A policy file that is malformed, or that a normal user can write, is ignored or refused as described in [Plugin policy](/pages/reference-admin-policy/). `ilium plugin doctor` warns when the policy disables a tier that the platform allows.

### Audit events

| Event | Meaning | Section |
|---|---|---|
| `policy-loaded` | The policy was read at start | [Bad plugin](#bad-plugin) |
| `policy-violation` | A plugin or file breaks the policy | [Bad plugin](#bad-plugin) |
| `safe-mode` | The server started in safe mode | [Crash loop](#crash-loop) |
| `grant-revoked` | A grant was removed | [Bad plugin](#bad-plugin) |
| `signature-failure`, `integrity-failure`, `blocklist-hit` | A package or load check failed | [Compromised signing key](#compromised-signing-key) |
| `trust-add`, `trust-remove` | The trust store changed | [Compromised signing key](#compromised-signing-key) |
| `update.rolled-back` | A candidate failed its first activation and the host returned | [Failed update](#failed-update) |
| `helper.hung`, `helper.unstable` | The helper was killed, or stopped after too many crashes | [Watchdog timeouts](#watchdog-timeouts) |

The names are those used by the pages that define them. The audit log lists the first group; see [Audit log](/pages/reference-audit-log/) for the full list and each record's fields.

### Escalation path

1. **The publisher.** The plugin's manifest may name a `security_contact` (SEC-325). `ilium plugin inspect <file>` shows it. Report a flaw in a plugin to that contact, not to the Ilium project, unless the flaw is in the host.
2. **The Ilium project, for a host flaw.** Report to the security contact in `SECURITY.md` or in `ilium --version --verbose`. Until that address is published the contact is the placeholder `security@ilium.example` (SEC-320). Include the version, platform, steps, expected and actual result, and whether a plugin was involved.
3. **The release blocklist.** If a publisher does not answer a serious report within 30 days, the Ilium project may add the package or key to the release blocklist (SEC-326). On your own machines, use `packages.blocklist_file` or `publishers.deny` now; do not wait for the project.

Who inside your organisation is told, and in what order, is not defined in the manual. See [Open decisions](#open-decisions).

## Registry backup and disaster recovery

This section is target behaviour for the public plugin system. This section is for the operator of a registry. It sets the backup and restore procedure for the registry index and how to rebuild it.

- [PKG-708] A registry operator MUST back up the signed index and the signed metadata at least daily, MUST keep the registry signing key in a separate backup under its own access control, and MUST test a restore at least once a quarter. A restore MUST bring back the last signed head or a later one, never an older version of `plugins/<id>.json`.
- [PKG-709] A registry MUST be able to rebuild its index from the signed releases alone. After a restore or a rebuild the operator MUST verify every signature and package digest, compare the result with the transparency log head, and publish the verification result. The registry MUST NOT serve a rebuilt index to clients until that check passes.

### Open decisions

| Subject | Choice in this manual | Consequence |
|---|---|---|
| Command for a compromised key | `ilium plugin trust revoke <key_id> --reason compromised` (PKG-243). The [Administration guide](/pages/guide-administration/#respond-to-a-bad-plugin) step 5 uses `ilium plugin trust remove <key_id> --force` instead. | With `revoke`, plugins stay off until a user accepts the risk. With `remove --force`, they return as soon as the key is trusted again. The two pages must agree before release. |
| Revoking a key in the system store | PKG-243 says the administrator revokes keys in the system store, but no command or file format for it is given. | Until the platform owner defines it, the runbook uses `publishers.deny` for machine-wide blocking. |
| Security contact address | `security@ilium.example`, a placeholder (SEC-320). | Host flaws have no real destination until the project publishes one. |
| Clearing crash marks by hand | None documented. The guard clears itself after 5 minutes of stable running (LIF-294). | An administrator who wants to clear marks at once has no supported command. |
| Text of the notice for a policy-disabled plugin | SEC-186 gives `Disabled by your administrator`. The lifecycle table and the Administration guide give `Blocked by policy`. | An administrator searching for the notice text may find the other one. |
| Internal escalation inside an organisation | Not defined. | Each administrator must set their own contact order. |

### Requirements

- [USR-230] The runbook MUST give, for each of the five symptoms in its triage table, a first command that exists in the CLI reference and a section that lists the policy keys and audit events for that symptom. Test: check each row against the CLI reference and each named key against the policy reference.
- [USR-231] An administrator MUST be able to stop a plugin for all users by its id without deleting its data, using `plugins.deny_ids` and the next server start. Test: add the id to `plugins.deny_ids`, restart the server, and assert the state `Blocked by policy` and that the data folder still exists.
- [USR-232] On a compromised signing key the administrator MUST run `ilium plugin trust revoke <key_id> --reason compromised` and MUST NOT rely on `trust remove --force` as the response. Test: revoke a key used by an installed plugin and assert the plugin state `disabled-blocked` with the reason `trust.key-revoked`.
- [USR-233] The runbook MUST state the time limits of the failure window, the helper kill sequence, the crash-loop guard and the install lock as the source pages define them. Test: compare each value in the time-limits table with its source page.
- [USR-234] The runbook MUST name the audit file path for Linux and each event it tells the administrator to search for, and every event name MUST appear in the source page that defines it. Test: run the `jq` command of the Bad plugin section on a fixture audit log and search each event name in the source pages.
- [USR-235] The runbook MUST name the escalation path in this order: the plugin's security contact, then the Ilium project's security contact, then the release blocklist, and MUST list the security contact address as an open decision while it is a placeholder. Test: check the page for the three steps and the open decision row.

### Emergency kill switch

An operator who learns that one plugin version is harmful needs to stop it on every machine at once, without waiting for each user to disable it. This section is the target behaviour. It is different from disabling one plugin on one machine and from a registry yank, which only blocks new installs.

- [LIF-301] A policy file MUST be able to list a plugin id and a version range under a `kill` key. A host that loads such a policy MUST refuse to activate any installed version inside a listed range, MUST stop a running instance of it with reason `policy`, and MUST log `policy.kill` once per plugin id.
- [LIF-302] A killed version MUST stay installed and its data MUST stay untouched, so lifting the `kill` entry and reloading policy MUST let the plugin start again without a reinstall.
- [LIF-303] A kill MUST take effect on a running host within 60 s of the policy file changing, MUST NOT need a server restart, and MUST apply before the crash-loop guard counts the stop as a failure.

## Emergency kill switch for one plugin

The registry blocklist acts on a release schedule and on publisher decisions. When a plugin is causing harm now, an administrator needs a switch that acts at once on every attached client and on the server.

- **[HST-516]** The host MUST provide `ilium plugin kill <plugin-id> --reason <text>`, which MUST stop the server side and every attached client side of the plugin, set it to `disabled-by-admin`, and prevent reactivation until `ilium plugin unkill <plugin-id>`. It MUST work in safe mode and without a network.
- **[HST-517]** The kill switch MUST take effect on the server within 1 s and on every attached client within 5 s of the command, MUST cancel the pending requests of the plugin as `cancelled`, and MUST write an audit event `plugin.killed` with the administrator, the reason and the time. A client that reconnects later MUST learn the killed state before it activates the plugin.

### Recover a disabled side

Target behaviour. After the third failure inside 10 minutes the host disables a side and waits for a manual enable ([API-041]). This section is the recovery procedure.

- [API-093] The host MUST offer `ilium plugin enable <id>` and an administrator bulk form for every disabled plugin, MUST show the crash record of [API-092] before it enables, and MUST clear the restart window on an enable so that the 10-minute count starts again at zero. An enable MUST NOT clear the crash record before the plugin has run for 10 minutes without a trap.

### See also

- [Plugin administration guide](/pages/guide-administration/)
- [Plugin policy](/pages/reference-admin-policy/)
- [Audit log](/pages/reference-audit-log/)
- [Troubleshooting plugins](/pages/user-troubleshooting/)
- [Safe mode and recovery](/pages/user-safe-mode-and-troubleshooting/)
- [Health and watchdog](/pages/reference-health-and-watchdog/)
- [Security response](/pages/reference-security-response/)

API version checked: `ilium:plugin@0.1.0`.

## Production additions: T10-operations

- [HST-696] `ilium plugin doctor [--json] [--fix]` MUST check the registry, lock files, staging folders, store hashes, bucket integrity, quarantine files, permissions (USR-206/207), disk space and orphaned data, MUST print one line per finding with a stable code, MUST change nothing without `--fix`, and with `--fix` MUST only perform the reversible actions (move aside, rebuild index, remove stale lock) and list each. Exit code 0 means no findings, 1 findings, 2 fixed.
- [HST-697] A lock holder is stale only when its recorded process is gone, or its boot id differs from the current one, or its heartbeat is older than 60 s; the host MUST NOT break a lock whose holder passes these checks, MUST log `lock.takeover` with the old holder's pid and boot id on takeover, and a command that waits for a held lock MUST fail with `registry.locked` after 30 s naming the holder.
