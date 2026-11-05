---
layout: page
title: Helpers when the server dies
permalink: /pages/reference-helper-orphan-and-server-death/
---

## Helpers when the server dies

This page specifies what happens to the sandbox helper processes when the Ilium server exits without a chance to clean up, for example after SIGKILL, an out-of-memory kill or a crash. It sets the time limits within which no helper may keep running, the checks the next server start performs, and the log and audit records that describe what was reaped. Authors who run plugins in a sandbox, operators who read the audit log after a crash, and anyone who writes tests for the helper lifecycle need this contract.

## Parent-death contract

Each helper process is a child of the server. Before a helper reads any plugin bytes, it sets `PR_SET_PDEATHSIG` to SIGKILL, so the kernel kills the helper as soon as the server dies. A SIGKILL sent to the server ends every helper within 1 s.

On hosts that do not provide parent-death signals, the helper uses its dispatcher channel as a fallback. When the dispatcher channel reaches end-of-file, the helper exits within 2 s with exit status 0. The server does not need to be killed for this fallback to run: closing the dispatcher socket from the server side is enough.

Whatever the cause of the server's exit, no helper survives more than 5 s after the server process is gone. A busy plugin loop does not change this limit. After the wait, no helper process remains and the helper cgroup leaf for that server is empty.

## Detection and reaping at next start

When the server starts, it scans for helper cgroup leaves left behind by an earlier run. A leaf belongs to an earlier run when its boot id or its server pid differs from the current ones. For each such leaf, the server kills the processes in the leaf and removes the leaf. This happens before any plugin is activated, so no `plugin.activated` log line appears before the orphans are gone.

Each stale helper found at start is recorded. The log receives one line with the code `host.helper-orphan-reaped` for each one. The audit log receives one record with the event name `helper-orphan-reaped`, carrying the plugin id and the previous server pid. Operators can list these records with `ilium plugin audit --event helper-orphan-reaped --json`, which prints one JSON line per record.

Reaped orphans do not count as plugin failures. After a reap at start, `ilium plugin health example.sync --json` reports `failures_in_window` as 0 for the plugin, so a crash of the server cannot push a plugin over the three-failure limit.

## Cgroup leaf cleanup

Helpers are placed in helper cgroup leaves under the helper cgroup path. A leaf that still holds processes after the server has gone is an orphan, and the next start removes it. A leaf whose server has exited must be empty within 5 s. The next start kills any process still found in a stale leaf, then removes the leaf directory.

## Audit and log records

The two records written for a reaped orphan describe the same event in two places. The log line uses the code `host.helper-orphan-reaped`, and the audit record uses the event name `helper-orphan-reaped`. Each carries the plugin id and the pid of the server that was running before the crash. Each stale helper found at start produces one log line and one audit record.

<div class="note">
<p>Check the audit record first after a crash. The log line shows the same event and is useful when you watch the log live.</p>
</div>

## Test recipes

To test the parent-death contract, start a server with two active plugins, send SIGKILL to the server process id, and list the processes one second later. No process whose command line names a helper for that server pid may remain.

To test the dispatcher fallback, close the dispatcher socket of a running helper from the server side while the server keeps running. The helper pid must be gone within 2 s, and its exit status must be 0.

To test reaping at start, create a stale leaf that holds a sleeping process under the helper cgroup path pattern, then start the server. The process must be gone and the leaf directory removed before the first `plugin.activated` log line appears.

To test the records, run the stale-leaf start again and check that the log contains exactly one `host.helper-orphan-reaped` line. Then run `ilium plugin audit --event helper-orphan-reaped --json` and check that it prints one line with `plugin_id` and `previous_server_pid`.

To test the time limit, kill the server with SIGKILL while a plugin runs a busy loop, wait 5 s, and check that no helper pid remains and that the helper cgroup leaf is empty.

To test the failure count, reap one orphan at start, then run `ilium plugin health example.sync --json` and check that `failures_in_window` is 0.

## Requirements

- **[PDZ-001]** Each helper MUST set `PR_SET_PDEATHSIG` to SIGKILL before it reads any plugin bytes, so that SIGKILL of the server ends every helper within 1 s.
- **[PDZ-002]** A helper MUST also exit within 2 s when its dispatcher channel reaches end-of-file, as a fallback for hosts without parent-death signals.
- **[PDZ-003]** At start the server MUST scan for helper cgroup leaves left by a previous boot id or server pid, kill their processes and remove the leaves before activating any plugin.
- **[PDZ-004]** Each stale helper found at start MUST be logged with the code `host.helper-orphan-reaped` and written to the audit log as one `helper-orphan-reaped` record with the plugin id and the previous server pid.
- **[PDZ-005]** A helper MUST NOT survive more than 5 s after the server pid is gone, whatever the cause of the server's exit.
- **[PDZ-006]** The next start MUST NOT count reaped orphans as plugin failures in the three-failure window.
