---
layout: page
title: Crash isolation, health checks and the kill switch
permalink: /pages/ops-crash-isolation-health-and-kill-switch/
---

## Crash isolation, health checks and the kill switch

This page is the operator's view of three controls that work together when a plugin misbehaves: crash isolation keeps one failure from stopping Ilium, health checks show the failure to a script or a person, and the kill switch stops a plugin on purpose. The rules themselves are owned by [Health and watchdog](/pages/reference-health-and-watchdog/), [Safe mode and troubleshooting](/pages/user-safe-mode-and-troubleshooting/) and the [Administrator failure runbook](/pages/guide-admin-failure-runbook/). This page adds the operator procedure, the exit-code contract for scripts and the rules that tie the controls together.

### You need

- The plugin id and a shell as the user who runs Ilium.
- The policy file of the site, when the machine is managed.
- The [plugin lifecycle states](/pages/reference-plugin-lifecycle-states/) open, to read the state words.

### Crash isolation

A plugin runs in its own side (server or client), and a side failure is contained to that side. The rules, in short:

- A failed side of a plugin never stops the server or the other plugins ([HST-010]).
- A helper that dies counts one failure and restarts on the schedule of [LIF-133](/pages/reference-health-and-watchdog/). Five restarts in ten minutes stop the sandboxed side with `helper.unstable` ([LIF-134]).
- Three crashes of the server during start move the next start into safe mode ([LIF-186]). Three more during safe mode move it further ([LIF-978], [LIF-979]).

The operator adds one rule on top of these. An operator MUST NOT treat a restart loop as a plugin problem until the logs show that the plugin started the loop. A host crash and a plugin crash look the same from the tree, and they need different actions.

### Health states and the exit code of `ilium plugin health`

`ilium plugin health` exits with the code of the worst state it finds ([HST-617]). Scripts MUST use this table and nothing else.

| Exit code | Meaning | Operator action |
|---|---|---|
| 0 | No plugin is `failed` or `degraded` | None |
| 1 | At least one plugin is `degraded` | Read the plugin log within the hour |
| 2 | At least one plugin is `failed` | Read the stopped side, then the crash record |
| Other | The command itself did not complete | Treat as a failure of monitoring ([LIF-955]) |

### Requirements

- **[OPS-201]** A monitoring script MUST treat every exit code other than 0, 1 and 2 as a failure of the monitoring, and MUST NOT treat it as a healthy result. Test: run the script with a missing `ilium` binary and assert that the alert shows a monitoring failure, not `healthy`.
- **[OPS-202]** An operator MUST read the state of a plugin with `ilium plugin status <id>` before any action that changes it. A kill, disable or uninstall MUST NOT be the first step of a triage. Test: a fixture plugin in `degraded` state; the runbook steps must reach the status read before the kill command.
- **[OPS-203]** A kill MUST carry a reason. `ilium plugin kill <plugin-id> --reason <text>` MUST refuse an empty reason and MUST print the refusal code without stopping the plugin. Test: run the command with an empty reason and assert the refusal and that the plugin is still running. (Proposed, owner to confirm: the refusal code name.)
- **[OPS-204]** A killed plugin MUST stay killed across a server restart and across a reload of the policy file, until an explicit re-enable runs the grant review ([SEC-458](/pages/reference-requirements-security-policy/)). A restart MUST NOT re-enable it, and a policy reload that does not list it MUST NOT re-enable it. (Proposed, owner to confirm: the policy-reload part, which extends HST-516.) Test: kill, restart the server, reload the policy, and assert the state `disabled-by-admin` after each step.
- **[OPS-205]** A kill MUST leave the crash records of the plugin in place. (Proposed, owner to confirm.) The kill switch MUST NOT delete the crash marks of the crash-loop guard ([LIF-294] clears them only after a stable start). Test: kill a plugin that has crash marks and assert that the marks and the records still exist after the kill.
- **[OPS-206]** The kill switch MUST NOT be applied to an essential core plugin that the host protects ([LIF-030], [LIF-042]). The command MUST refuse it and name the reason. (Proposed, owner to confirm: the refusal wording and the refusal code.) Test: run the kill command against an essential core plugin and assert the refusal and that the plugin still runs.
- **[OPS-207]** When a plugin is killed, every pending request of its instances MUST complete as `cancelled` within the times of [HST-517](/pages/reference-requirements-kernel-host/), and the other plugins MUST keep their pending requests. Test: give a fixture plugin three pending requests, kill it, and count the outcomes.
- **[OPS-208]** The operator MUST record each kill and each re-enable in the incident record of the site, with the actor, the reason and the time, in addition to the audit record of [SEC-458]. Test: read the incident record after one kill and one re-enable.

### Worked example: a plugin in a restart loop

A plugin `example.sync` restarts every few seconds. The operator follows the runbook order and stops only when the cause is clear.

1. Check the health and the exit code.

<pre class="term"><span class="t-prompt">$</span> ilium plugin health example.sync
<span class="t-err">failed</span>  server  stopped-by-failure  reason helper.unstable
<span class="t-prompt">$</span> <span class="t-dim">echo $?</span>
2
</pre>

2. Read the log for the first failure after the last good start.

<pre class="term"><span class="t-prompt">$</span> ilium plugin logs example.sync --tail 6
2026-11-03 08:40:02 <span class="t-err">error</span> server  example.sync trap in export tick: memory
2026-11-03 08:40:03 <span class="t-warn">warn</span>  server  example.sync restarting in 1 s (failure 1 of 3 in 10 minutes)
</pre>

3. The trap is in the plugin. Stop the loop with a reasoned kill. The reason is written to the audit record.

<pre class="term"><span class="t-prompt">$</span> ilium plugin kill example.sync --reason "memory trap loop, ticket 4411"
<span class="t-ok">ok</span>   example.sync  disabled-by-admin
</pre>

4. Confirm that the other plugins are healthy, and open the incident record (OPS-208). The plugin stays disabled after a restart (OPS-204) until someone re-enables it after a grant review.

The example's ticket number and the output line are illustrative.

### Edge cases

- **The loop is in the host, not the plugin.** The logs show `host.` codes, or `host.loop-stalled` ([HST-618]). A kill does not help; follow [the crash-loop section](/pages/user-safe-mode-and-troubleshooting/#the-crash-loop-guard) and report the host crash.
- **Kill during a pending request.** The request is cancelled, not retried (OPS-207). The caller sees `cancelled` and the caller's own rule applies.
- **The machine reboots during a kill.** The kill state is in the user's store and is written with the atomic rule of [HST-680]. Read the state after the start; do not assume it.

### Decisions for the owner to confirm

- The refusal code and wording for the kill of an essential core plugin (OPS-206).
- The refusal code for an empty kill reason (OPS-203).
- Whether the exit codes of OPS-201 need a fourth code for a missing binary, instead of the "other" row.
- The one-hour window in the exit-code table for reading a `degraded` log. It is an operating habit, not a host rule.

### See also

- [Health and watchdog](/pages/reference-health-and-watchdog/) for the rows and thresholds of the health states.
- [Administrator failure runbook](/pages/guide-admin-failure-runbook/) for the policy keys and the emergency kill.
- [Diagnostics and logs for operators](/pages/ops-diagnostics-and-logs/) for reading the log before a kill.
- [Telemetry consent for operators](/pages/ops-telemetry-consent/).
