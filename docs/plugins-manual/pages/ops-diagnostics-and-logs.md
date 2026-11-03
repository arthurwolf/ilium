---
layout: page
title: Diagnostics and logs for operators
permalink: /pages/ops-diagnostics-and-logs/
---

## Diagnostics and logs for operators

This page is the operator's procedure for reading plugin logs, building a diagnostics report and deciding what may leave a machine. The data formats are owned by other pages and are linked, not repeated. The [Diagnostics core plugin](/pages/core-plugin-diagnostics/) owns the report and the log-file service. [Debugging](/pages/debugging/) owns the log line format. [Telemetry and plugin health](/pages/reference-host-telemetry/) owns the counters. [Privacy](/pages/user-privacy/) owns the retention table.

### You need

- A shell on the machine where the plugin runs, as the user who runs Ilium.
- The plugin id. `ilium plugin list` prints every id.
- Read access to the state folder. Logs never need root.

### Where the data lives

| Data | Where | Who can read it | Leaves the machine |
|---|---|---|---|
| Plugin log lines (written by the plugin and by the host) | `ilium plugin logs <id>` reads them | The user running Ilium | Only when someone copies the file |
| Diagnostics log file (file logging on) | `/tmp/.ilium-<uid>/logs/<session-id>/` on Linux and macOS, `%LOCALAPPDATA%\ilium\logs` on Windows | The user running Ilium | Only when someone copies the file |
| Diagnostics report | Built on demand by the `build-report` command | The user who built it | Only when the user attaches it |
| Telemetry counters | In the process and the local log | The user running Ilium | Never (HST-350) |
| Crash records | `<state>/crashes/` | The user running Ilium | Only by the export command |

The path rules of the log folder, including the `ILIUM_DEBUG_LOG_DIR` override and the grant that does not follow it, are in [Diagnostics](/pages/core-plugin-diagnostics/#what-it-covers). An operator who moves the logs with that override MUST check the grant before relying on the report (see OPS-004).

### Requirements

- **[OPS-001]** The host MUST write one diagnostics log file for each server lifetime. When the server starts, it MUST keep the five newest log files of the session and MUST delete the older ones. Test: start the server seven times in one session and count the files in the session folder: five remain, and they are the five newest by start time.
- **[OPS-002]** File logging MUST be off by default. Turning it on or off MUST take effect without a restart of the server, and MUST NOT delete the files already written. Test: switch file logging on, write a line, switch it off, and assert that the file still exists and that no line is written after the switch.
- **[OPS-003]** `ilium plugin logs <id>` MUST print the log of that plugin only, oldest entry first. `--tail <n>` MUST print the last `n` entries and nothing else. Test: write 50 entries for two plugins and assert that `--tail 5` of one plugin prints exactly five lines, all of that plugin.
- **[OPS-004]** When the log folder is outside the granted path `/tmp/.ilium-<uid>/logs`, the report MUST show `log directory not readable: <path>` and MUST list no log files. The operator MUST NOT widen the grant by hand in the plugin manifest or in the user's grant store to make the report work. Test: set `ILIUM_DEBUG_LOG_DIR` to a folder outside the grant, build the report, and assert the line and the empty file list.
- **[OPS-005]** Ilium MUST NOT send a diagnostics report or a log file to any network address. The only action that moves them is the user's own copy, export or attach. Test: build a report and attach one log with the network disabled, and assert that no connection is opened.
- **[OPS-006]** Before a log or a report is attached to an issue, the attach flow MUST show the file name, its size in bytes and the warning that logs hold prompts and replies. Test: open the attach flow for a 1.2 MiB log and assert the three items on screen.
- **[OPS-007]** An operator who copies a log for a support request MUST remove private text from the copy first, or send the support bundle of [USR-335](/pages/reference-requirements-user-admin/) instead, which counts the redactions. The host redacts secrets and paths outside the home folder ([HST-129](/pages/reference-host-telemetry/#personal-data-in-logs-and-trace-retention)), but it does not detect names, so the copy MUST be reviewed by a person. Test: read a copy of a log that contains a name, and confirm that the review step removes it.
- **[OPS-008]** A diagnostics report section from one plugin MUST name the plugin that supplied it. A section that gets no reply within 2000 ms MUST appear as a warning line in the report, and the report MUST still be built from the other sections. Test: a section plugin that does not answer; assert the warning line and the other sections present.
- **[OPS-009]** The report MUST be at most 262144 bytes in total, and each section MUST be at most 65536 bytes. A section that is larger MUST be cut to 65536 bytes and end with the line `[section truncated]`, and the report MUST say which section was cut. Test: a section plugin that returns 70000 bytes; assert the cut and the marker. (Proposed, owner to confirm: the marker text.)
- **[OPS-010]** The default log threshold MUST be `info`. Entries at `debug` and `trace` MUST NOT appear in `ilium plugin logs` unless the threshold is raised for the plugin, and the threshold MUST return to `info` when the server restarts. Test: write one `debug` entry, read the log, raise the threshold, write again, and restart.

### Triage procedure

Use this order. Each step reads only what the previous step did not answer.

1. Check the state of the plugin with `ilium plugin status <id>`. Read the `health` field and the stopped side, if any. The state words are in [Plugin health](/pages/reference-health-and-watchdog/).
2. Read the last 50 log entries with `ilium plugin logs <id> --tail 50`. Look for the first `error` or `warn` line after the last restart.
3. Run `ilium plugin doctor`. It lists the install problems that a log line does not show.
4. Build the report only when a second person must read the host state. Run the `build-report` command from the Debug tab or from the command palette. Read the report on the machine before you attach it.
5. Attach the report or a log only through the attach flow, and only after OPS-006 and OPS-007 are met.

### Worked example

A plugin `example.kanban` shows `degraded` after a deploy. The operator follows the steps above.

<pre class="term"><span class="t-prompt">$</span> ilium plugin status example.kanban
<span class="t-warn">degraded</span> server  restarts 3 in 10 min
<span class="t-prompt">$</span> ilium plugin logs example.kanban --tail 4
2026-11-02 14:07:19 <span class="t-err">error</span> server  example.kanban trap in tick: deadline
2026-11-02 14:07:19 <span class="t-warn">warn</span>  server  example.kanban restarting in 1 s (failure 2 of 3 in 10 minutes)
2026-11-02 14:07:21 <span class="t-warn">warn</span>  server  example.kanban restarting in 5 s (failure 3 of 3 in 10 minutes)
2026-11-02 14:07:26 <span class="t-ok">info</span>  server  example.kanban started
</pre>

The log shows the deadline trap as the cause. The operator then reads the plugin's own version and the build of the deploy, not the Ilium logs. A report is not needed for this case, so nothing leaves the machine.

### Edge cases

- **The log folder is full.** Files are kept at five per session (OPS-001). A full disk is a host failure; the operator frees space in the state folder and does not delete the open log file.
- **The server restarted during the incident.** Logs of the earlier server lifetime are in their own file. Read the file whose start time matches the incident, not the newest file.
- **A plugin writes secrets into its log.** The host redacts credential headers and URL parameters only. Follow [OPS-011](#operator-response-to-a-leaked-secret) at once, then delete the plugin's log file by hand after you have copied what the support process needs.

### Operator response to a leaked secret

- **[OPS-011]** When an operator finds a secret in a log, the operator MUST disable the plugin with `ilium plugin disable <id>` as soon as the secret is found, MUST rotate the secret at its issuer, and MUST record the incident in the site's change record. Deleting the log line is not a rotation. Test: write a fixture secret, run the procedure, and assert that the plugin is disabled and that the secret no longer works at the fixture issuer.

### See also

- [Debugging](/pages/debugging/) for the line format and the levels.
- [Diagnostics](/pages/core-plugin-diagnostics/) for the report sections and the attach flow.
- [Telemetry and plugin health](/pages/reference-host-telemetry/) for the counters that the report shows.
- [Administrator failure runbook](/pages/guide-admin-failure-runbook/) for the policy keys used in an incident.
- [Ops: telemetry consent](/pages/ops-telemetry-consent/) and [Ops: crash isolation, health checks and the kill switch](/pages/ops-crash-isolation-health-and-kill-switch/).
