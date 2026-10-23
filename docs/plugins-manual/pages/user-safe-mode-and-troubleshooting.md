---
layout: page
title: Safe mode and recovery
permalink: /pages/user-safe-mode-and-troubleshooting/
---

## Safe mode and recovery

When a plugin makes Ilium hard to use, start in safe mode. Safe mode turns plugins off without removing anything, so you can find the cause and fix it. This page also covers the bisect tool and the crash-loop guard.

Contract version: `ilium:plugin@0.1.0`.

### Start in safe mode

| Command | Effect |
|---|---|
| `ilium --safe-mode` | The same as `--safe-mode=third-party`. |
| `ilium --safe-mode=third-party` | Runs the bundled plugins only. T2 and T3 plugins are not loaded. `replaces` is ignored. |
| `ilium --safe-mode=all-optional` | Runs only the essential set below. Every other plugin is off. |
| `ILIUM_SAFE_MODE=third-party` | Same as the flag. The flag wins when both are set. The variable also accepts `all-optional`, `1` (the same as `third-party`) and `0` (off). Any other value exits 2 with `usage.invalid-value`. |

The older switches stay as aliases: `--safe-plugins` is `--safe-mode=third-party`, and `--no-plugins` starts no plugin host at all (the kernel only). When more than one is given, the strictest wins. From strictest to least strict the order is: `--no-plugins` (no plugin host), `--safe-mode=all-optional` (essential set only), `--safe-mode=third-party` or `--safe-plugins` (bundled plugins only), then no safe mode. For example, `--no-plugins` with `--safe-mode=all-optional` starts the kernel only.

Safe mode changes nothing on disk. Installed packages, grants, settings and data stay as they were. The footer shows "Safe mode: third-party plugins are off."

- [LIF-180] `--safe-mode=third-party` MUST load no T2 or T3 plugin and ignore `replaces`, and `--safe-mode=all-optional` MUST load only the essential set. Neither MUST change files on disk. Test: start both and compare the profile before and after.
- [LIF-181] The environment variable MUST behave like the flag, and the flag MUST win. Test: set both to different values.
- [LIF-182] Safe mode MUST show an indicator in the footer. Test: capture the footer.

### The essential set

The essential set is the core plugins that the screen needs to stay usable. A plugin outside the set can be turned off in safe mode.

| Plugin | Why essential |
|---|---|
| `ilium.core.tree` | draws the tree |
| `ilium.core.layout` | keeps the split layout |
| `ilium.core.settings` | opens Settings and the recovery page |
| `ilium.core.sessions` | writes the session snapshot |

Core plugins that are not in this table can be disabled. See the column "Can you turn it off" in [Core plugins](/pages/core-plugins/).

- [LIF-183] The essential set MUST be exactly the four plugins above in 0.1.0, and MUST load in `all-optional` mode. Test: list running plugins in that mode.

### Find the plugin that causes a problem

`ilium plugin bisect` halves the set of enabled third-party plugins until it finds the one at fault.

| Command | Meaning |
|---|---|
| `ilium plugin bisect start` | Saves the enabled set, then enables half of it for this session. |
| `ilium plugin bisect good` | The problem is gone with this half. Test the other half next. |
| `ilium plugin bisect bad` | The problem remains. Narrow this half. |
| `ilium plugin bisect reset` | Restores the saved set. |

State lives in `<data>/plugins/bisect.json`. Each command prints JSONL records of type `bisect` with the enabled list and the remaining candidates. When one plugin remains, the record has `found` set to its id. When the problem persists with no third-party plugin enabled, the record has `found` null and `reason` `not-a-plugin` (LIF-187). The bisect changes the session level only, so it never alters your saved choices.

- [LIF-184] Bisect MUST change only the session-level enabled state and MUST restore the saved set on `reset`. Test: run to the end and compare `installed.json`.
- [LIF-185] Bisect MUST finish in at most `ceil(log2(n))` steps for `n` plugins. Test: 16 plugins, 4 steps.
- [LIF-187] A bisect that ends without a `found` plugin MUST say so and MUST NOT name a plugin. When `bad` is answered with only the empty set enabled (the problem persists with no third-party plugin), or when `good` is answered for every half so that no plugin reproduces the problem, the final record has `found` set to `null` and `reason` set to `not-a-plugin`, and the text says the fault is not caused by a third-party plugin. The saved set is restored by `reset` as usual. Test: run `start`, answer `bad` at every step on a profile where the fault is in the kernel, then read the final record.

### The crash-loop guard

If the server stops with a crash 3 times within 5 minutes during start, the next start runs in `third-party` safe mode and opens the recovery page. The guard clears after one start that stays up for 5 minutes.

A **crash mark** is one timestamped record that the host writes to the server's own state folder (not into any plugin's data) as soon as a start begins. The host removes the record when startup completes, so a mark left behind by the next start means that start ended without finishing: a panic, a kill or a power loss before startup completed. A normal exit is not a crash. The 5 minutes are measured on the monotonic clock from the moment startup completes, and a stop before then (crash or normal exit) does not count as staying up.

- [LIF-186] Three crashes during start within 5 minutes MUST cause the next start to use `third-party` safe mode. Test: write three crash marks.
- [LIF-294] The crash-loop guard MUST clear all crash marks once a start has stayed up for 5 minutes after startup completed. A stop before that point MUST leave the marks in place. Test: write three crash marks, start, advance 4 minutes 59 seconds and stop (marks remain), start again, advance 5 minutes and stop, then assert the next start is not in safe mode.

### When safe mode also crashes

The guard has two more levels. They use the same crash marks as LIF-186, and a crash in any level counts toward the next level. The levels run in this order: `third-party`, then `all-optional`, then `--no-plugins` (no plugin host, kernel only).

- [LIF-978] Three crashes during start within 5 minutes while the start runs in `third-party` safe mode MUST cause the next start to use `all-optional` safe mode (proposed, owner to confirm: the count of 3 and the 5-minute window). Test: write three crash marks, start in `third-party`, write three more crash marks within 5 minutes, and assert the next start runs in `all-optional`.
- [LIF-979] Three crashes during start within 5 minutes while the start runs in `all-optional` safe mode MUST cause the next start to use `--no-plugins` (proposed, owner to confirm: the count of 3). The start MUST open the recovery page as its first screen. Test: write three crash marks in `all-optional` and assert the next start runs with no plugin host.
- [LIF-980] Three crashes during start within 5 minutes while the start runs with `--no-plugins` MUST stop automatic restarts. Ilium MUST exit with a non-zero status and print one line with the code `crash-loop.exhausted` that names the state folder holding the crash marks and says that the fault is not caused by a plugin. Until the marks clear under LIF-294, every later start, with or without a flag, MUST use `--no-plugins`. Test: write three crash marks in `--no-plugins`, assert the exit status and the code, then start without flags and assert the start still runs with no plugin host.
- [LIF-981] Every start MUST show the level it runs in on the footer banner (`third-party`, `all-optional` or `--no-plugins`), and a level change MUST NOT clear the crash marks; only LIF-294 clears them. Test: switch levels by writing crash marks, and assert the banner text and that the marks remain after each change.

If you reach `crash-loop.exhausted`, keep the state folder unchanged and report the crash to the Ilium project through the contact in [Security response](/pages/reference-security-response/). Do not delete the crash marks by hand.

### When the Ilium server itself crashes after startup

The crash-loop guard above covers only crashes during start. This section covers the server dying while a session is running, after startup completed. A helper crash is a different case: a helper exit never stops the server or any other plugin side (HST-010) and is recorded under USR-227.

**What the host does.** The server installs a panic hook before it loads any plugin. On a panic the hook writes one **host crash record** to the crash folder, then exits with a non-zero status. The crash folder is the 0700 folder beside the audit directory, and the record has mode 0600 (SEC-447). A record holds only:

- the Ilium version and build digest;
- the operating system and architecture;
- the name of the panicking thread;
- the backtrace, as host frames only;
- the last 50 host log lines.

It holds the same kinds of fields as a plugin crash record (SEC-446): no plugin data, memory, argument values, secret values, granted data or file contents. The redaction test runs over the record before it is stored. A fatal signal or a kill that the hook cannot catch (out-of-memory kill, power loss) writes no record. The next start then finds a leftover crash mark and the missing record tells you the server did not get to report. Core dumps stay off (SEC-445), so no other forensic file exists. The write adds one `crash-report` audit record that names the file (SEC-447).

**What an attached client shows.** When the connection to the server drops without a shutdown message, each attached client shows a notice: `Server stopped unexpectedly`. The notice gives the path of the host crash record, or says `no crash record was written` when none exists, and offers two actions: **Restart** and **Close**. Restart starts a new server for the same project session. Close leaves the client. The client never restarts the server on its own.

**What is lost and what is kept.** Memory state and the server restore bytes are lost. Storage, settings, data and metadata are as of the last durable write or snapshot (LIF-157). Plugin helpers end with the server. After restart, plugins start again from stored state.

**Restart and the guard.** A server that died after startup completed but before the 5 minutes of LIF-294 had passed leaves its crash mark in place. Restarting inside that window therefore counts as one crash mark toward LIF-186. Three such crashes within 5 minutes make the next start run in `third-party` safe mode, and the levels in [When safe mode also crashes](#when-safe-mode-also-crashes) follow. A crash after the mark was cleared starts a fresh count at one.

**Finding the record.**

- `ilium plugin doctor` prints the last host crash: its time in UTC (RFC 3339), the Ilium version, the panicking thread and the record path. It prints `none` when no record exists.
- `ilium plugin crashes --host` lists every host crash record, newest first, one JSONL record per line with the time, version and path. Without `--host` the command lists plugin and helper records only.
- Host crash records are deleted after 14 days (SEC-447). SEC-955 proposes 30 days and a limit of 200 records, and the owner is to align the two. Uninstalling a plugin never deletes host records, because they belong to no plugin.
- Nothing is sent anywhere unless you run the explicit export command (SEC-447).

**In a bug report.** Attach the host crash record file named in the notice or in `ilium plugin crashes --host`, together with the output of `ilium plugin doctor`. Read the record first: it holds log lines, so remove anything you do not want to share. Report through the contact in [Security response](/pages/reference-security-response/) if you think the crash is a security fault. See also [Troubleshooting](/pages/user-troubleshooting/).

**Failure modes.**

- The crash folder is not writable: the hook skips the record, the notice says `no crash record was written`, and the exit status is still non-zero.
- A second panic inside the hook: the process aborts and no record is written.
- The disk is full: the record may be cut short. A cut record is kept and is marked in `ilium plugin doctor` as `truncated`.

### Recovery steps

1. Start with `ilium --safe-mode=third-party`.
2. Open Settings, then Plugins ([Managing plugins](/pages/user-managing-plugins/)).
3. Turn plugins on one at a time, or run `ilium plugin bisect start`.
4. When you find the plugin, update it, disable it, or uninstall it.
5. Start normally.

<figure>
<pre class="term"><span class="t-warn">Safe mode: third-party plugins are off.</span>
Plugins
 ilium.core.tree       on
 com.example.notes     off  (safe mode)
 com.example.cursor    off  (safe mode)
[b] bisect  [r] leave safe mode
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The Plugins list in safe mode with third-party plugins held off.</figcaption>
</figure>

## Production additions: T7-users-privacy-a11y

- **[USR-345]** Safe mode MUST be enterable by `ilium --safe-mode`, by `ILIUM_SAFE_MODE=1`, and automatically after 3 (proposed, owner to confirm) server starts in 5 minutes that ended in a host crash while loading plugins, and the banner MUST say which of the three caused it.


### See also

- [Managing plugins](/pages/user-managing-plugins/)
- [Plugin lifecycle states](/pages/reference-plugin-lifecycle-states/)
- [Host architecture](/pages/reference-host-architecture/)
- [Requirements, lifecycle and operations](/pages/reference-requirements-lifecycle-ops/)

API version checked: `ilium:plugin@0.1.0`.
