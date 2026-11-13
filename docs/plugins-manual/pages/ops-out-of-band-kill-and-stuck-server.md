---
layout: page
title: Kill without a responsive server
permalink: /pages/ops-out-of-band-kill-and-stuck-server/
---

## Introduction

This page specifies the kill route that does not depend on the server loop or on any plugin cooperating. It covers the offline fallback of `ilium plugin kill`, the kill state file that the server reads at its next start, the cgroup leaf names that an operator uses for a manual kill, the supervised self-exit of a stalled server, and the incident-time use of `ilium plugin disable --all-third-party` with its timing and staged re-enable rules. Operators who must stop a misbehaving plugin during an incident, and administrators who run the server under a supervisor, need this page.

## When the normal kill is not enough

The normal kill is `ilium plugin kill <id> --reason <text>`. It asks the running server to stop the plugin. When the server does not answer, the command must not hang. After 2 s without a server reply, the command writes the kill state file and sends `cgroup.kill` to the helper cgroup leaf of the plugin. It then prints which path it used: `server` when the server handled the kill, or `offline` when the command handled it. A stopped server, for example one held with `SIGSTOP`, must still let the command finish within 3 s with exit status 0.

<pre class="term"><span class="t-prompt">$</span> ilium plugin kill example.sync --reason "stuck loop"
<span class="t-warn">warn</span> server did not reply within 2 s
<span class="t-ok">ok</span>   plugin example.sync killed
path: offline
</pre>

A kill by any path never calls an export of the plugin, including its `deactivate` export. A plugin whose `deactivate` export writes a marker file must leave no marker file after it is killed. Cleanup that depends on plugin code therefore does not run during a kill, and the operator must assume that the plugin left no cleanup behind.

## Offline kill list file read at start

A kill state file written while the server is stopped is the record of an offline kill. The server reads it at its next start, before it activates any plugin. A plugin named in the file is left in the package state `disabled-by-admin`, so it is never activated by that start. The file is `<data>/plugins/kill.json`, and its rows in the state tables are in [Containment states](/pages/reference-plugin-lifecycle-states/#containment-states). `ilium plugin unkill` has no offline path: with no session it exits with code 6 and changes nothing ([SCU-003](/pages/security-containment-states/)). For example, after `ilium plugin kill example.sync --reason test` runs with the server stopped, starting the server and running `ilium plugin list --json` must show `disabled-by-admin` for `example.sync`, and the server log must contain no `plugin.activated` line for that plugin.

An offline kill also writes one `plugin-killed` audit record. The server writes that record at the next start, because the server was not running when the kill happened. The record carries the field `path` with the value `offline`. Read it with `ilium plugin audit --event plugin-killed --plugin example.sync --json`, which prints one line for the kill.

## Killing helpers by cgroup

Each helper process of a plugin runs in its own cgroup leaf. The leaf path follows the pattern `<ilium cgroup>/helpers/<plugin-id>/`, so an operator can find the leaf of a plugin from its id alone. Writing `1` to `cgroup.kill` of that leaf stops every helper process in it. The server treats such a write as one counted helper failure. For example, writing `1` to `cgroup.kill` of `.../helpers/example.sync/` while the plugin runs makes the helper exit, and `ilium plugin health example.sync --json` then shows `failures_in_window: 1`.

The leaf is the target of the offline path of `ilium plugin kill`, so an operator who writes to it by hand uses the same leaf path that the command uses.

## Server watchdog and self-restart under a supervisor

A server started with `--supervised` reports its own liveness to the supervisor. It writes a heartbeat file every 1 s. If its main loop has not ticked for 30 s, the server exits with status 70, so that the supervisor can restart it. This rule works after the `host.loop-stalled` detection of [HST-618](/pages/reference-health-and-watchdog/), which fires after 2 s and only writes a log line and an alert. The 30 s bound is the point at which the server stops by itself.

When the heartbeat file stops updating, the server has stopped ticking, and the exit with status 70 is the signal the supervisor can act on. A kill state file written before the restart is applied at the next start, as described above.

## Disabling all third-party plugins during an incident

`ilium plugin disable --all-third-party --reason <text>` is the emergency switch of [SEC-457](/pages/guide-enterprise-deployment/). It stops every non-core plugin on the running server within 1 s. Each attached client applies the change within 5 s. The server process is not restarted, so its process identifier stays the same. The command prints the identifiers of the core plugins that it skipped, under the heading `skipped`. The same switch writes one audit record, as described in the [audit log](/pages/reference-audit-log/).

Re-enabling is a separate step and never happens on its own. `ilium plugin enable --all-third-party` re-enables the plugins one at a time, in install order. Before it enables each plugin, it runs the grant review of [Grant review and consent](/pages/reference-grant-review-and-consent/). When a plugin fails its grant review, the command stops at that plugin, prints the identifier of that plugin, and leaves that plugin and every later plugin in the state `disabled-by-admin`. Earlier plugins stay enabled. For example, when the grant review of the second plugin in install order is declined, the first plugin is `enabled` and the second and third remain `disabled-by-admin`.

Both commands must work in safe mode and with no network available. In safe mode with networking disabled, `ilium plugin disable --all-third-party --reason test` exits with status 0, and the state file lists every third-party plugin as `disabled-by-admin`. An operator can therefore use the switch on a machine that cannot reach the network or that started in safe mode, as described in [Safe mode and troubleshooting](/pages/user-safe-mode-and-troubleshooting/).

## Verification and audit

An operator or a test engineer checks this route with the steps below. Each step names the observable result that counts as a pass.

- Stop the server loop with `SIGSTOP`, run `ilium plugin kill` for a running plugin, and confirm exit status 0, the line `path: offline` and no remaining helper process within 3 s.
- Stop the server, run an offline kill, start the server, and confirm the state `disabled-by-admin` and one `plugin-killed` record with `path` set to `offline`.
- Disable all third-party plugins with three third-party and two core plugins active, and confirm the state change on the server within 1 s and the `skipped` list with the two core identifiers.
- Decline one grant review during the re-enable, and confirm that the plugins after it remain `disabled-by-admin`.

Every kill and every re-enable must also be recorded in the incident record of the site. The incident steps are described in [Crash isolation, health and kill switch](/pages/ops-crash-isolation-health-and-kill-switch/), and the plugin commands are listed in the [plugin CLI reference](/pages/reference-plugin-cli/).

## Requirements

- **[PDZ-007]** `ilium plugin kill <id> --reason <text>` MUST fall back, after 2 s without a server reply, to writing the kill state file and sending `cgroup.kill` to the plugin's helper cgroup leaf, and MUST print which path (`server` or `offline`) it used.
- **[PDZ-008]** A kill state file written while the server is stopped MUST be honoured at the next start before any activation of that plugin, leaving it in state `disabled-by-admin`.
- **[PDZ-009]** Helper cgroup leaves MUST follow the documented path pattern `<ilium cgroup>/helpers/<plugin-id>/` so an operator can find them by plugin id, and `cgroup.kill` written to that leaf MUST be treated by the server as one counted helper failure.
- **[PDZ-010]** When run under a supervisor (`--supervised`), the server MUST write a heartbeat file every 1 s and MUST exit with status 70 if its main loop has not ticked for 30 s, beyond the `host.loop-stalled` detection of [HST-618].
- **[PDZ-011]** An offline kill MUST write one `plugin-killed` audit record with the field `path` set to `offline`, written at the next start if the server was not running.
- **[PDZ-012]** A kill by any path MUST NOT call any export of the plugin, including its deactivate export.
- **[PDZ-013]** `ilium plugin disable --all-third-party --reason <text>` ([SEC-457]) MUST stop every non-core plugin within 1 s on the server and within 5 s on each attached client without restarting the server, and MUST print the ids of the core plugins it skipped.
- **[PDZ-014]** `ilium plugin enable --all-third-party` ([SEC-458]) MUST re-enable plugins one at a time in install order and MUST stop at the first plugin that fails its grant review, printing that plugin id and leaving later plugins disabled.
- **[PDZ-015]** Both commands MUST work in safe mode and with no network available.
