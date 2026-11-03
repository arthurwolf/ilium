---
layout: page
title: Containing a malicious plugin
permalink: /pages/security-containment-states/
---

## Containing a malicious plugin

Four separate mechanisms can stop a plugin: a user disable, an administrator kill, a policy kill and a block from a revoked key or a registry takedown. Each one leaves its own record, and they can all apply to the same plugin at once. This page defines how they combine, which command moves between them, and what each transition writes.

It does not repeat the step-by-step response to a suspected plugin. That procedure is in [If you suspect a plugin is malicious](/pages/user-plugin-security-and-privacy/#if-you-suspect-a-plugin-is-malicious). The package states are defined in [Plugin lifecycle states](/pages/reference-plugin-lifecycle-states/), the grant store in [Grant review and consent](/pages/reference-grant-review-and-consent/), and the audit records in [Audit log](/pages/reference-audit-log/).

### The four mechanisms

| Mechanism | Set by | Resulting state | How it is lifted |
|---|---|---|---|
| User disable | `ilium plugin disable <id>`, or Space on the row | `installed-disabled-by-user` | `ilium plugin enable <id>`, or Space on the row |
| Admin kill | `ilium plugin kill <id> --reason <text>` | Instance state `disabled-by-admin` (HST-516) | `ilium plugin unkill <id>` only |
| Policy kill | A `kill` entry with a plugin id and version range in the policy file | Reason `policy`, audit line `policy.kill` (LIF-301) | Remove the entry and reload the policy (LIF-302) |
| Block | Key revoked as `compromised`, or a registry takedown of this version | Package state `disabled-blocked` (P15) | `enable <id> --accept-risk`, or an update signed by another trusted key of the same publisher (P16) |

The spec names the state of an admin kill but does not name the file that stores the kill flag. It is known to survive a restart, because HST-516 requires that test. See the open decisions below.

<div class="note">
<p>A kill and a user disable are not the same. A disabled plugin can be turned on again by Space or <code>ilium plugin enable</code>. A killed plugin is refused at activation, also after a restart, until <code>ilium plugin unkill</code> runs.</p>
</div>

### Precedence when several mechanisms apply

**[SCU-001]** When two or more mechanisms apply to one plugin, the host MUST show the reason of the strongest one in this order: block, policy kill, admin kill, user disable. The host MUST keep the record of every other mechanism, and it MUST NOT clear any of them when the strongest one is lifted. *Proposed, owner to confirm.* Test: disable a plugin, kill it, run `unkill`, and assert that the plugin stays disabled and the row names the user disable.

### Kill and unkill transitions

The rows below give the result of each command in each starting state. Rows marked with a source are already in the spec or in the administrator runbook. The other rows are proposed.

| Starting state | `ilium plugin kill` | `ilium plugin unkill` |
|---|---|---|
| Installed and enabled | Server stops within 1 s and each attached client within 5 s. Pending requests end as `cancelled`. State `disabled-by-admin`. Audit line `plugin.killed` (HST-517). | Clears the kill. Activation is allowed again. Whether the plugin starts at once is proposed, owner to confirm. |
| Installed and disabled by user | Kill is recorded. The user disable stays recorded too (SCU-001). | Clears the kill only. The plugin stays disabled. |
| Block (`disabled-blocked`) | Kill is recorded. The plugin stays blocked. | No change to the block. The row keeps showing the block reason (user troubleshooting page). |
| Policy kill is listed for this plugin | Kill is recorded. Activation stays refused by the policy. | Refused with `kill.policy-held`, no state change (SCU-007). Error code proposed, owner to confirm. |
| Already killed | A second `plugin.killed` record is written (SCU-010). | Clears the kill. |
| Not killed | Kill as above. | No change. The command says so (user page). |
| Not installed | Exit code 5 with `not found` (administrator runbook). | Exit code 5 with `not found`. |
| No session running | Exit code 6. Start Ilium, or start it with `ilium --safe-mode`, then run the command again (administrator runbook). | Exit code 6. |

**[SCU-002]** `ilium plugin kill <id>` and `ilium plugin unkill <id>` for an id that is not installed MUST exit with code 5 and the text `not found`, and MUST NOT write any state or audit record. Test: run both against an unknown id and assert the exit code, the text and that `ilium plugin audit` shows no new line.

**[SCU-003]** When no session is running, `ilium plugin kill` and `ilium plugin unkill` MUST exit with code 6 and MUST NOT change any stored state. *Proposed, owner to confirm* for the second half of the rule. Test: stop the session, run each command, and compare `installed.json` and the kill flag before and after.

**[SCU-004]** `ilium plugin kill` MUST refuse to run without `--reason <text>`, and it MUST store the reason in the `plugin.killed` audit record. *Proposed, owner to confirm.* The refusal code is not yet named. Test: run `kill` without `--reason` and assert that no kill is recorded and the refusal is shown.

**[SCU-005]** The kill MUST survive a restart of the server and a restart of the operating system. Each start MUST refuse activation of the killed plugin until `unkill` runs. This extends HST-516, which covers only a server restart. Test: kill, restart the operating system of an isolated test VM, start Ilium, and assert the instance state is still `disabled-by-admin`.

**[SCU-006]** `ilium plugin unkill <id>` on a plugin that is blocked MUST leave it blocked, and MUST show the block reason that is in force (`trust.key-revoked` or `registry.takedown`). Test: block a plugin by key revocation, kill it, unkill it, and assert that the state is `disabled-blocked` and the reason is shown.

### Updates, uninstall and takedowns while killed

**[SCU-007]** While a policy kill lists a plugin id, `ilium plugin unkill <id>` MUST be refused with `kill.policy-held` and MUST leave the state unchanged. *Proposed, owner to confirm.* The operator lifts a policy kill by removing the entry and reloading the policy (LIF-302). Test: add a `kill` entry, kill and unkill through the CLI, and assert the refusal and an unchanged state. Then remove the entry and assert that the plugin is still refused until `unkill`.

**[SCU-008]** An update that installs a new version of a killed plugin MUST NOT clear the kill. The kill is keyed by plugin id, so the user checks the new version and then runs `unkill`. *Proposed, owner to confirm.* Test: kill version A, install version B with `ilium plugin update`, and assert that activation is refused and the status still shows `disabled-by-admin`.

**[SCU-009]** `ilium plugin uninstall <id>` of a killed plugin MUST remove the kill flag with the plugin record, and MUST keep its `plugin.killed` audit records. *Proposed, owner to confirm.* A reinstall starts with no kill, as a fresh install does (SEC-119). Test: kill, uninstall, read the audit log, reinstall, and assert that the plugin is not killed and that the earlier `plugin.killed` record is still there.

**[SCU-010]** A second `ilium plugin kill` on a killed plugin MUST write one more `plugin.killed` record with its own reason and time, and MUST keep the state `disabled-by-admin`. *Proposed, owner to confirm.* Test: kill twice with different reasons and assert two audit records and one state.

**[SCU-011]** A registry takedown or key revocation that reaches a killed plugin MUST set the package state to `disabled-blocked` and MUST keep the kill flag set. The row MUST show the block reason. The kill flag stays so that a later `unkill` cannot restore activation of a blocked plugin, because the block is still in force. *Proposed, owner to confirm.* Test: kill a plugin, revoke its key as `compromised`, and assert that the row shows `Blocked` and that the state is `disabled-blocked` after `unkill`.

### Worked example: a plugin that contacts an unknown host

A user sees the audit line `egress-host-first-use` for `notes-sync` 0.4.2, with a host the user does not recognise. The plugin has network permission. The user works through the containment steps as follows.

1. Stop it with `ilium plugin kill notes-sync --reason "unknown egress host"`. The server stops within 1 s. The state becomes `disabled-by-admin` (HST-516, HST-517).
2. Check the state with `ilium plugin status notes-sync --json`. The output shows `disabled-by-admin`.
3. Export the evidence before any destructive step: `ilium plugin audit --plugin notes-sync --since 7d > notes-sync-audit.jsonl`. Then run `ilium plugin audit --verify`.
4. The publisher releases 0.4.3 and the user installs it with `ilium plugin update notes-sync`. The kill stays (SCU-008). Activation is still refused.
5. The user is not sure the update is safe, so the plugin stays killed. The user reads the new version's grants and audit lines and decides to uninstall with `ilium plugin uninstall notes-sync --purge`. The kill flag goes with the record (SCU-009). The exported file from step 3 keeps the evidence.

A risk note for the reader: if the registry takedown arrives during step 4, the state becomes `disabled-blocked` and `unkill` cannot clear it (SCU-011).

### Open decisions

These are not settled by the spec. Each one is proposed above and needs an owner to confirm it.

- Where the admin kill flag is stored, and whether a host upgrade keeps it (SCU-005 assumes it does).
- The refusal code for a kill without `--reason` (SCU-004).
- The error code of a refused `unkill` under a policy kill (`kill.policy-held` in SCU-007).
- Whether an `unkill` starts the plugin at once or leaves it for `ilium plugin enable` (first row of the transition table).
- The precedence order in SCU-001, which puts a block above a policy kill.

### See also

- [Administrator failure runbook](/pages/guide-admin-failure-runbook/) for the policy form of the kill and the emergency kill switch.
- [Security model](/pages/security/) for the revocation epoch that ends in-flight calls.
- [Install and manage plugins](/pages/user-install-and-manage/) for the everyday disable, enable and uninstall commands.
