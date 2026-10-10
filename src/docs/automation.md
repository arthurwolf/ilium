# Automation

Ilium can send input to panes on your behalf and let agents talk to Ilium. This page covers scheduled input, text triggers, the prompt queue, "Ask for update", creating an agent from an editor line, the progress monitor (`ilium progress`), the project Chatroom, the managed agent instructions that teach agents to use those features, the local HTTP API, and public reset forecasts.

Most of these features type text into a terminal pane and press Enter for you. Ilium cannot know what is on the other side of the pane at that moment, so check the target (a shell, an agent at its prompt, an agent waiting on an approval question) before you rely on an unattended send.

## Contents

- [Scheduled input](#scheduled-input)
- [Text triggers](#text-triggers)
- [Prompt queue](#prompt-queue)
- [Ask for update](#ask-for-update)
- [Create an agent from an editor line](#create-an-agent-from-an-editor-line)
- [Progress monitor](#progress-monitor)
- [Chatroom](#chatroom)
- [Agent setup](#agent-setup)
- [Local HTTP API](#local-http-api)
- [Reset forecasts](#reset-forecasts)
- [Troubleshooting](#troubleshooting)

Related pages: [Agent monitoring](agent-monitoring.md) (how Ilium decides an agent has finished), [Notifications](notifications.md), [Worktrees](worktrees.md) (the HTTP API can create worktrees), [Settings](settings.md), [Editors and boards](editors-and-boards.md), [CLI reference](cli-reference.md).

Demos: [scheduled input](../../assets/demos/04-scheduled-input.gif), [prompt queue](../../assets/demos/13-prompt-queue.gif), [text triggers](../../assets/demos/21-text-triggers.gif), [ask for update](../../assets/demos/12-ask-for-update.gif), [agent from line](../../assets/demos/16-agent-from-line.gif), [progress monitor](../../assets/demos/14-progress-monitor.gif), [chatroom](../../assets/demos/20-chatroom.gif). See [Demos](demos.md) for the full list.

---

## Scheduled input

Scheduled input sends keystrokes to one terminal pane after a countdown. It is owned by the detached server, so it still fires if you detach, close the terminal window or quit the TUI. It is stored with the session, so a pending schedule survives a server restart as long as the session snapshot is restored (see [Session recovery](session-recovery.md)).

Use it for things such as "press Enter in this agent in two hours" (to resume after a usage limit lifts) or "type `npm test` in ten minutes".

### Schedule an input

1. In the project tree, right-click a **terminal** pane (shell or agent). Editors and boards do not have this entry.
2. Choose **Hit key(s) X time from now**. The dialog **Schedule input for _pane name_** opens.
3. Fill in the countdown:
   - **Hours**: any whole number; blank counts as 0.
   - **Minutes**: 0 to 59; blank counts as 0.
   - **Seconds**: 0 to 59; starts at `30`.
4. Under **Payload**, type the text to send (any Unicode, single line). Leave it empty to send only Enter.
5. Tick **Send Enter** (on by default) to press Enter after the text.
6. Press **Ctrl+Enter** or click **[ Schedule input ]**.

The tree shows a scheduled-input marker on the pane while the countdown runs. The marker is an icon you can change under Settings, Icons.

| Key | Action |
| --- | --- |
| Tab / Shift+Tab | Next / previous field (Hours, Minutes, Seconds, Text, Send Enter, button) |
| Space (on Send Enter) | Toggle Send Enter |
| Ctrl+Enter | Schedule |
| Esc | Cancel |

### Rules and edge cases

| Situation | Behaviour |
| --- | --- |
| Total delay is 0 | Refused: "Choose a delay of at least one second". |
| Minutes or seconds above 59 | Refused with a message naming the field. |
| Non-digit typed in a number field | Ignored. |
| Empty text and Send Enter off | Refused: "Enter text, enable Send Enter, or use both". |
| Text only | The text is written to the pane without Enter. |
| Enter only | A single Enter is sent. |
| Text and Enter | Ilium submits the text, then sends Enter as a separate step so an agent composer receives them as a typed message followed by a submission. |
| A second schedule on the same pane | Replaces the first. There is one pending schedule per pane. Use it to move the deadline or change the payload. |
| The pane is closed | The schedule is discarded with it. |
| The pane's working directory is missing (a removed worktree being recovered) | The countdown is kept and retried after the workspace is recovered. |
| Another input arrives at the same moment | Ilium holds the pane's input gate for the text-to-Enter sequence, so keyboard or mouse input cannot interleave inside it. |

Scheduled input does not check whether the pane is idle. If an agent is mid-turn, or showing an approval prompt, the keystrokes land there. For "send this when the agent finishes" use the [prompt queue](#prompt-queue) instead.

---

## Text triggers

A text trigger watches terminal output for a regular expression and, when a line matches, sends a fixed one-line message plus Enter to the same pane. Typical uses are auto-answering a recurring question ("Do you want to continue? (y/n)" with `y`) or nudging an agent that prints a known "paused" notice.

Triggers are matched on the server, on the output path, so they work for hidden and detached panes exactly as for the pane you are looking at.

### Create a trigger

1. Open **Settings** (`Ctrl+B :`) and select the **Text Triggers** tab.
2. Choose **[ Add text trigger ]** and press Enter.
3. Fill in the dialog (**Add text trigger** / **Edit text trigger**):

| Field | Meaning |
| --- | --- |
| **Regexp** | A Rust `regex` pattern (not PCRE: no look-around or back-references). Must not be empty and must compile. |
| **Message** | The literal text to send. Must be a single line; Enter is added by Ilium. Not interpreted as a template. |
| **Target** (arrows select) | **Agents** (panes classified as a known agent), **Terminals** (panes classified as a plain shell) or **Both** (default). Detected agents are deliberately separate from shells so a broad shell rule does not type into an agent session. |
| **Delay (seconds)** | Whole seconds between detecting the match and sending the message. Default 60. `0` sends immediately. Maximum 86400 (24 hours); larger numbers are clamped. A blank field means the default of 60. |
| **Sample text** | A multi-line scratch area kept with the rule. The **Live preview** below it marks which sample lines match the regexp. |
| **Enabled** | A disabled rule is kept and previewable but never fires. |

4. Press Enter on **[ Save trigger ]** (or Enter in the form) to save.

Keys in the dialog: Tab / Shift+Tab move between fields, arrows select the scope, Space toggles Enabled, Enter saves, Esc cancels. In the settings list, Enter edits the selected rule, Delete removes it, and the list shows `ON` / `OFF`, the pattern, the message and the scope.

The live preview warns when the message itself matches the regexp ("Reply also matches this regexp; echoed input can loop"). Heed it: the terminal echoes what Ilium types, and a rule that matches its own reply can answer itself repeatedly. If the regexp is invalid the preview shows the compiler error and saving is refused.

### How matching works

- Ilium keeps its own copy of each pane's screen and counts visible matches per distinct matched text. A rule fires **once per new instance** of matching text, however the text later scrolls, repaints or moves. When the same text stays visible it is not answered again.
- An instance that disappears from the screen for a settle window (1.5 seconds after it was first seen missing) becomes eligible again, so a program that clears and re-prints the same line is answered the next time it appears.
- After a pane resize, newly visible text is adopted silently for 2 seconds so a full repaint does not trigger a burst of replies.
- Fast output is sampled in slices of at most 8 KiB. Matching is best effort: a transient screen state that is overwritten between scans may not be seen.
- Every delivery waits its own delay; a short delay never queues behind a longer one. Up to 64 delayed deliveries are held per pane.
- At send time Ilium re-checks that the pane is still the same terminal and that the rule still exists, is enabled, is unchanged in its message and still targets the pane's current classification. If you disable or edit the rule during the delay, the pending send is cancelled.
- The message is sent through the same staged text-then-Enter submission as other automation, and recorded as a text-trigger submission in the agent debug log.

### Storage

Rules are stored in the global `config.toml` as an explicit list:

```toml
[[text_triggers.triggers]]
id = "6f0c8d0e-3b0e-4e83-a5c1-0d3c2f4d2a11"
enabled = true
regexp = "Do you want to continue\\? \\(y/n\\)"
message = "y"
target = "both"        # "agents", "terminals" or "both"
sample_text = "Do you want to continue? (y/n)"
delay_seconds = 60
```

Notes:

- `id` is required, must be unique and not blank. Ilium assigns a UUID; hand-written IDs may be any unique string.
- A rule saved before the **Delay** setting existed has no `delay_seconds` key and loads with 60.
- Ilium reloads the list when the file changes. An invalid document (bad regexp, multi-line message, duplicate id, delay above 86400, a `[text_triggers]` table without a `triggers` list) is rejected as a whole and the server keeps the last valid list. The file is left on disk for you to repair. An explicit empty list clears all triggers; an absent table changes nothing.
- The file is read up to 512 KiB.

---

## Prompt queue

The prompt queue holds prompts for an agent pane and delivers them one at a time **each time the agent finishes a turn**. Use it to line up follow-up instructions while the agent is still working.

"Finished" is the server's detection of a completed turn (the same signal as the Done state and completion notification, see [Agent monitoring](agent-monitoring.md)), not a timer.

### Queue a prompt

1. Right-click a terminal pane in the tree and choose **Queue prompt...**. The dialog **Queue prompt after agent finishes** opens.
2. Type the prompt in the multi-line text area. **Enter inserts a new line** here; it does not submit.
3. Tab to **Delivery** and choose with the arrow keys (or Space) between:
   - **Once**: sent after the next finish, then removed.
   - **Run X times**: the same prompt is sent once per future finish until the count is used up. The count field (default `2`) must be a positive whole number.
   - **Enqueue forever (DANGER)**: re-sent after every finish for as long as it stays in the queue. The dialog warns: "DANGER: this will re-send forever whenever the agent finishes. Do not run it unmonitored."
4. Press **Ctrl+Enter** or click the enqueue button.

| Key | Action |
| --- | --- |
| Tab / Shift+Tab | Next / previous field (Text, Delivery, Times, button) |
| Left / Right / Space (on Delivery) | Change delivery mode |
| Ctrl+Enter | Enqueue |
| Esc | Cancel |

An empty or whitespace-only prompt is refused ("Write a prompt before enqueueing it").

To remove everything queued for a pane, right-click it and choose **Clear prompt queue**. That entry appears only while the queue is not empty.

### Delivery rules

- **First in, first out.** Each finish delivers exactly the head of the queue: the text, then Enter, as separate input stages.
- **Repeating entries rotate.** After a **Run X times** or **forever** prompt is delivered it moves to the back of the queue (with its remaining count reduced), so other queued prompts get their turn between repeats.
- The queue holds at most **100** entries per pane.
- Delivery is crash-safe. Before writing, the server records that it is about to deliver the head and saves the session. If the server is interrupted at that instant, the head stays visible and is **not** replayed automatically, because Ilium cannot know whether none, part or all of it reached the pane.
- The queue is stored with the session and delivered even when no client is attached.
- A queued prompt is recorded as such in the agent debug log.

---

## Ask for update

**Ask for update** types a short status request into idle agents so you can see where they stand without reading their history. The text sent is:

> please remind me, in a very compact way, what you were doing, what I asked you to do, how it went, etc, remind me what's going on

followed by Enter. Add your own wording under Settings, **LLM Instructions**, **Ask for update** (see [Titles and instructions](titles-and-instructions.md)); it is appended inside a `custom-instructions` section.

Where it appears in the tree context menu:

| Right-click target | Effect |
| --- | --- |
| An agent pane that is idle | Asks that one agent. |
| A project | Asks every idle agent inside it. |
| Empty space in the tree (the root) | Asks every idle agent in the whole session. |

The entry only appears when at least one agent in scope is **idle** (its turn state is Idle: not working and not waiting on an approval). If none is idle the status bar says "No idle agent to ask for an update". It reports "Asked for an update" or "Asked N agents for an update". If the terminal input queue is full the request is not sent and the status bar says so.

---

## Create an agent from an editor line

Right-click a line in an editor pane to start an agent that works on that line.

1. Right-click the line in the editor's Source view. The menu offers **Copy line to clipboard**, **Copy chapter to clipboard** (Markdown files, when the line is inside a heading section), **Copy entire file to clipboard**, **Create agent from line...** and, when the click landed on a URL or an existing path, **Open in editor** / **Open externally**.
2. Choose **Create agent from line...**. The dialog **Create agent from line** opens with **Agent** (Claude, Codex or Antigravity; arrows change it) and a prefilled, editable prompt.
3. Edit the prompt if you wish, then press **Ctrl+Enter** or click create. Tab / Shift+Tab move between the agent selector, the prompt and the button; Enter inserts a newline in the prompt; Esc cancels.

The default prompt is:

> /goal please do the following task: "_the line text_", note this text comes from the file _path_ at line _N_ in case this can help you gather more context

The line number is one-based and the text is the exact physical line (not interpreted as a template). The new agent pane is created next to the originating editor, in the same container. An empty prompt is refused ("Agent task cannot be empty"). The copy actions use the live editor buffer, including unsaved edits.

See [Editors and boards](editors-and-boards.md) for the editor itself.

---

## Progress monitor

The progress monitor lets a long-running job report percentage and status to the Ilium footer of the pane that started it, and tells the agent when the job ends, so the agent does not have to poll.

How it works in one paragraph: the job (or an agent on its behalf) registers a **probe**, a shell command that prints one JSON object describing the job. The **server** runs the probe on an interval, validates the output and updates a footer in that pane. When the job reaches `done` or `error`, the result goes to whoever is waiting for it: an `ilium progress wait` command that is still running gets it as its output and exit status; otherwise Ilium types a short result message into the pane when its composer is ready. After registration nobody needs to poll.

### Requirements

- The `ilium progress` commands must be run **from inside an Ilium terminal pane**. Ilium sets `ILIUM_PANE_ID`, `ILIUM_SESSION_NAME` and `ILIUM_SESSION_SOCKET` in each pane's environment and the command uses them to find the right pane and server. Outside a pane the command fails with a message naming the missing variable.
- The progress monitor setting must be on (Settings, **Agent Monitoring**, **Progress monitoring**, which also appears in the **User Interface** tab as **Progress monitor**; default on). When off, new registrations are rejected with code `disabled` and observation stops; recorded results are kept.
- The pane must be a terminal pane. It does not have to be an agent.

### Commands

Every command prints **one JSONL record** to stdout on success (one JSON object on a single line) so a script or agent can read the result without scraping prose. Failures print a JSONL `progress_request_failed` record carrying the operation, `request_id`, `pane_id`, a machine-readable `code` (lower-case with hyphens, for example `probe-timed-out`) and a `message`.

| Command | Purpose | Output `type` |
| --- | --- | --- |
| `ilium progress check --command '<probe>'` | Run the probe once through the server and validate it. Installs nothing. | `progress_check` |
| `ilium progress set --command '<probe>' [--interval-seconds N] [--wait [--timeout-seconds S]] [--replace]` | Validate, then atomically add a monitor to this pane. Waits for a server acknowledgement that contains the monitor id and the accepted first report. With `--wait`, then blocks exactly like `ilium progress wait` and prints a second line. Other monitors of the pane keep running; `--replace` clears all of them first. | `progress_set` (then `progress_wait`) |
| `ilium progress wait [MONITOR_ID] [--timeout-seconds S]` | Block until the monitor reports `done` or `error`, fails or is cleared. Without an id it picks the pane's only unsettled monitor (else its only monitor) and refuses with `monitor-ambiguous` (exit 2) when there are several. `ilium wait` is the same command. | `progress_wait` |
| `ilium progress status` | Every monitor of this pane: registration, latest report and monitor health (`monitor_count`, `running`, `monitors`). | `progress_status` |
| `ilium progress clear [--monitor-id ID \| --all]` | Stop a monitor and clear its retained progress. With `--monitor-id`, only that exact monitor. Without it, the pane's only monitor; with several, it refuses with `monitor-ambiguous` unless `--all` is given. | (clear result) |


### Waiting for a monitor

`ilium progress wait` lets an agent wait for a job the same way it waits for any other long command: it runs until the job settles, prints one line, and exits with a status that names the outcome.

| Exit status | `outcome` | Meaning |
| --- | --- | --- |
| 0 | `done` | The task reported `done`. |
| 3 | `error` | The task reported `error`; the record carries the error text. |
| 4 | `monitor-failed` | The probe stopped working, so the task outcome is unknown. |
| 5 | `cleared`, `no-monitor` | The monitor was cleared (also by `set --replace`), monitoring was switched off, or the pane has no monitor. |
| 6 | `still-running` | `--timeout-seconds` elapsed. Run the same command again to keep waiting. |
| 2 | (`progress_rejected`, code `monitor-ambiguous`) | No id was given and the pane has several monitors; the record lists their ids in `monitor_ids`. |
| 1 | (failure record) | The command could not reach the server or the connection broke. |

The record also carries `composer_notice`: `suppressed` means the server did not, and will not, type a result message for this monitor, because this command delivered it. `may-also-arrive` means a message was already typed (or may be) and is a duplicate of this record. If the wait command is killed or the agent stops it, the server falls back to typing the message, so nothing is lost.

Without `--timeout-seconds` the command waits as long as the job takes. Agent tools that limit a command's run time either move it to the background (Claude Code) or need a timeout that fits the tool: pick a `--timeout-seconds` below the tool's limit and run the command again on exit status 6.

The usual way to run a long job is one command that registers the monitor and waits for it:

```sh
# Start the job first and confirm it is alive, then:
ilium progress set --command '/home/me/bin/build-progress.sh' --interval-seconds 5 --wait
```

If no completion message arrives after the task should have finished, run `ilium progress status`: Ilium re-sends undelivered outcomes every 20 s and marks a monitor that lost its observation task as failed (outcome unknown), but `status` is the authoritative manual check.

`--interval-seconds` defaults to `1`, minimum 1, maximum 86400 (24 hours). Raise it when the probe is heavy.

Typical flow:

```sh
# 1. Start the long job first and confirm it is alive (not shown).
# 2. Validate the probe.
ilium progress check --command '/home/me/bin/build-progress.sh'
# 3. Register it and wait for the result in one command.
ilium progress set --command '/home/me/bin/build-progress.sh' --interval-seconds 5 --wait
# Or register now and wait later:
ilium progress set --command '/home/me/bin/build-progress.sh' --interval-seconds 5
ilium progress wait
# If needed:
ilium progress status
ilium progress clear --monitor-id 7
```

`set` never silently succeeds: if the server does not acknowledge within the request timeout, it reports a failure. A pane can run up to 8 monitors at once, for example a build and a test suite started by two subagents of the same agent. `set` adds a monitor and never disturbs the others. Registering the same probe command while its monitor is still running returns that monitor instead of a duplicate. Settled monitors whose outcome was already delivered make room for new ones; a ninth live monitor is refused with code `too-many-monitors`. The footer shows one gauge row per monitor in registration order, with a `+N more` row when they do not fit, and the sidebar glyph represents the most urgent one (an unread failure, then a running task). Still prefer one probe that covers a whole multi-step pipeline, and pass the monitor id to `wait` and `clear` when a pane may have several. `--replace` clears every monitor of the pane before adding the new one.

### The probe contract

The probe is any command line. On Linux and macOS Ilium runs it as `$SHELL -c "<command>"` (falling back to `/bin/sh`); on Windows it runs through `cmd.exe /D /S /C`. It must print **exactly one JSON object** on stdout and exit 0.

| Field | Type | Rules |
| --- | --- | --- |
| `job_id` | string | Required, non-empty, at most 256 bytes. **Must be identical on every run**; a changed id is rejected as a job identity change. |
| `status` | string | Required. One of `not-started-yet`, `running`, `error`, `done`. |
| `percent` | number | Required, finite, 0 to 100. For `done` Ilium forces exactly 100. |
| `message` | string | Optional, may be empty, at most 2048 bytes, no control characters. The compact line always shown in the pane footer. |
| `details` | string | Optional, at most 8192 bytes. Multi-line; newline, carriage return and tab are allowed, other control characters are not. Shown when you hover the footer. |
| `error` | string | **Required when `status` is `error`** (at most 4096 bytes); **forbidden otherwise**. An empty string counts as absent. |

Unknown fields are rejected, so a typo cannot be silently ignored.

Status transitions are checked on every poll:

- `not-started-yet` may move to any status.
- `running` may move to `running`, `error` or `done`, but never back to `not-started-yet`.
- `error` and `done` are final. Once the probe reports either, the monitor stops polling and the job is finished.

Execution limits:

| Limit | Value |
| --- | --- |
| Probe timeout | 30 seconds (the whole process tree is terminated on timeout) |
| Probe stdout | 64 KiB |
| Probe stderr | 16 KiB |
| Command length | 16 KiB |
| Interval | 1 second to 24 hours |

Probes run with no stdin, and descendants left behind by the probe are killed when it ends. A non-zero exit, a timeout, oversized output, invalid JSON, a failed validation, a changed `job_id` or an illegal status transition is an observation failure.

### Write the probe correctly

**Use absolute paths.** The probe is started by the Ilium **server**, not by your shell. It does not inherit your pane's current directory, your shell's `cd`, exported variables or aliases. It starts in the server's working directory, which is the session's project root, for the lifetime of the server. A relative path that works in your shell silently reads the wrong place (the classic symptom is a monitor stuck at 0 percent while the job is fine). Use absolute paths or begin the command with an explicit `cd /absolute/path &&`.

**Keep it cheap.** A new shell is started on every tick. Prefer reading a small state or log file the job writes, over recursive directory scans, network calls or heavy subprocesses. If the check is inherently expensive, raise `--interval-seconds`.

**Write for a stranger.** The person glancing at the footer has not read your session. Make `message` stand alone: what is being done, to which thing, and the current step with a count, for example `Building the release binaries of the ilium terminal app - compiling crate 14 of 22`. On `done` or `error` it states the outcome. Use `details` (3 to 8 short lines, with labels such as `What:`, `Why:`, `Now:`, `Next:`, `Watch:`) for the longer explanation, and update both on every poll so they track the real step.

A minimal probe script that reads a state file the job maintains:

```sh
#!/bin/sh
# /home/me/bin/build-progress.sh - cheap, absolute paths only.
state=/home/me/build/.progress
done_count=$(cat "$state/done" 2>/dev/null || echo 0)
total=$(cat "$state/total" 2>/dev/null || echo 1)
percent=$(( done_count * 100 / total ))
if [ -f "$state/finished" ]; then status=done; else status=running; fi
printf '{"job_id":"release-build-2026-10-04","status":"%s","percent":%s,"message":"Building the release binaries - crate %s of %s","details":"What: release build of the project\\nNow: compiling crate %s of %s\\nNext: install and verify"}\n' \
  "$status" "$percent" "$done_count" "$total" "$done_count" "$total"
```

Check it, then register it:

```sh
ilium progress check --command '/home/me/bin/build-progress.sh'
ilium progress set   --command '/home/me/bin/build-progress.sh' --interval-seconds 5
```

### What you see

The footer sits under the pane's content and shows the task status, percentage and the `message`, followed by up to a configured number of detail rows (the leading `message` is always visible; hover the footer to see `details`). It also shows the monitor id and warns about monitor trouble without recolouring the task as failed.

Two things are kept separate on purpose:

- **Task outcome**: what the probe reported (`running`, `done`, `error`).
- **Monitor health**: whether Ilium can observe the task. After one or two consecutive probe failures the monitor is **degraded** and keeps the last good report while it retries. After **three** consecutive failures the monitor is **failed**: it stops polling and the task outcome is explicitly **unknown**. A failed monitor is not a failed task.

When the job ends, Ilium types a message into the pane once its composer is ready (it waits for a safe boundary and never touches the agent's `/goal`):

| Event | Message typed into the pane |
| --- | --- |
| `done` | "Ilium progress monitor N reports that JOB completed successfully. Final progress: 100%. Status: MESSAGE." |
| `error` | "Ilium progress monitor N reports that JOB failed. Final progress: P%. Status: MESSAGE. Error: ERROR." |
| Monitor failed | "Ilium progress monitor N could no longer observe JOB. The task outcome is unknown. Last progress: P%. Monitor error: ..." |
| Stopped before a final status | "Ilium progress monitor N stopped before JOB reached a terminal task status. The task outcome is unknown. ..." |

This is how an agent that registered a probe learns the job is finished without polling. No message is typed for a result that a running `ilium progress wait` already returned.

### Settings

Under Settings, **Agent Monitoring** (the same rows are also reachable from **User Interface**; see [Settings](settings.md)):

| Setting | Default | Notes |
| --- | --- | --- |
| Progress monitoring (`[ui] progress_monitor_enabled`) | On | Off stops observation and rejects new registrations. Existing final results are retained and shells are untouched. |
| Progress footer lines (`[ui] progress_max_lines`) | 4 | Task-detail rows below the status line, 1 to 20. Overflow is clipped, not deleted. |
| Hide completed progress after (`[ui] completed_progress_hide_after_seconds`) | 60 | How long a done or failed footer stays visible; adjusts in 30 second steps; `0` means never hide (the setting row reads "s; 0 = never"). Presentation only: retained results, error details and unread markers are kept. The timer starts at the server's final observation and does not restart when a client reconnects. |
| Progress fill style (`[ui] task_progress_frames`) | a preset | The character family used for the progress frames, 2 to 13 printable frames, all one cell wide or all two cells wide. |

### Troubleshooting the progress monitor

| Symptom | Cause and fix |
| --- | --- |
| `ilium progress` says it is not inside an Ilium pane | Run it from a terminal pane started by Ilium, not from an unrelated shell or over SSH. |
| Rejection code `disabled` | Turn Progress monitoring on in Settings. |
| `probe-timed-out` / `probe-exited-non-zero` / `probe-output-too-large` | The probe is slow, failing or chatty. Run it by hand, keep stdout to one short object, keep stderr small. |
| `invalid-probe-report` | Not exactly one JSON object, an unknown field, percent out of range, an `error` without `status:"error"` (or the reverse), or `job_id` changed. |
| Stuck at 0 percent while the job runs | A relative path in the probe. Use absolute paths. |
| `stale-monitor` on `clear --monitor-id` | That monitor no longer exists (it was already cleared or replaced); the clear was refused on purpose. |
| `monitor-ambiguous` on `wait` or `clear` | The pane has several monitors. Pass the monitor id the `set` line printed (`ilium progress status` lists them), or `clear --all`. |
| `too-many-monitors` on `set` | The pane already has 8 live monitors. Wait for one or clear it. |
| `ilium progress wait` fails with "closed the connection" | The server restarted, or the running server is older than the `wait` command. The normal typed result message still arrives. |
| Footer shows "monitor failed" | Three probe failures in a row. Fix the probe and register again with `ilium progress set`. |

---

## Chatroom

The Chatroom is a shared, file-backed message log for a project. People and agents running in Ilium coordinate through it: claiming a shared area, reporting a blocker, handing off. It is a plain Markdown file, `CHATROOM.md`, in the project root, so it works even when no TUI is attached.

### Set it up

- **From the tree**: right-click a project and choose **Add chatroom to project**. Ilium creates the room and integrations described below.
- **From the command line**: run `ilium chat init` in the project directory. It prints `chatroom ready at <path>`.

Setup does four things, all idempotent:

1. Creates `CHATROOM.md` (title `# ILIUM CHATROOM`, a marker line, a short guidance paragraph and a `## Messages` heading). It never overwrites an existing file; a symlinked or non-regular `CHATROOM.md` is refused.
2. Adds `/CHATROOM.md` to the project's `.gitignore` (creating the file if needed).
3. Merges Ilium's hooks into `.claude/settings.local.json` and `.codex/hooks.json` for the `SessionStart` and `UserPromptSubmit` events. Each hook runs `ilium chat context --limit 40 --since-last-read --max-bytes 2048` (timeout 10 seconds, status message "Checking ilium chatroom"): a session start receives the recent room history, and each later prompt receives only records that agent has not seen, at most 2 KB, or nothing. Older Ilium hook commands are upgraded in place. Existing hooks and other settings are preserved; invalid JSON in those files stops the merge with an error instead of overwriting them.
4. Installs the managed Chatroom instruction block in the agent instruction files (see [Agent setup](#agent-setup)).

A project that already has a room gets its integrations repaired when Ilium starts; Ilium never creates a room merely by looking at a project.

### Use it in the TUI

A project with a room shows a **Chatroom** entry in the tree. Select it to open the room in the right panel:

- The message log shows `timestamp <author> content`, scrollable by wheel and scrollbar.
- A composer at the bottom ("Write message - Enter sends") posts as `user`.
- An **Agents in this project** list shows the live agent panes of that project.

### Use it from the command line

| Command | Purpose |
| --- | --- |
| `ilium chat init` | Create the room and integrations (above). |
| `ilium chat send --message "..." [--author NAME]` | Append one message. The author defaults to `ILIUM_CHATROOM_AUTHOR`, then `AGENT_NAME`, then `agent`. Prints `chatroom message sent`. |
| `ilium chat context [--limit N] [--since-last-read] [--max-bytes N]` | Print the last N messages (default 40) as plain text suitable for injection into an agent turn; with `--since-last-read`, only messages this reader has not seen; with `--max-bytes`, newest messages first within the cap. This is what the hooks run. |
| `ilium chat tail [--limit N]` | Print the last N records (default 100) as `timestamp \| author \| content` for inspection. |

`--cwd` selects the project directory. If the current directory is inside a project that has a room, Ilium walks up to the nearest ancestor holding `CHATROOM.md`, so agents started in a subdirectory still reach the right room.

### File format and limits

Each message is one line:

```text
- 2026-10-04 18:20:11 +02:00 | agent:codex | Claiming src/auth; changing the token refresh.
```

- Timestamps use `YYYY-MM-DD HH:mm:ss +HH:MM` in local time.
- Newlines and tabs inside a message are escaped so a record stays on one line; control characters are removed.
- A message is limited to **4000 characters**; author and message must not be empty.
- Writers take an advisory lock around the whole append, so simultaneous senders do not interleave. If a hand edit left the file without a trailing newline, Ilium adds one first so it does not splice onto your last line.
- You may edit the file by hand. Lines that are not in the record shape stay in the file but are ignored by the structured views, which also strip raw control bytes from fields.
- The TUI reads at most the last 8 MiB of the file and at most 200 messages at a time.

### What agents are told

The guidance deliberately discourages chatter. Agents are told to read recent coordination with `ilium chat context --limit 40` when they begin work and before changing shared areas, and to post only for a task claim or release, a blocker, dependency or question needing action, a material discovery or decision, or a handoff with the outcome and location. They must not post routine progress narration, acknowledgements or "I am working" messages, and must never rewrite `CHATROOM.md` directly.

---

## Agent setup

Ilium ships two short instruction blocks for agents: **Chatroom** (when and how to use the room) and **Progress** (hand any task expected to take three minutes or more to the progress monitor, and never poll). To be effective they have to be in the files Claude Code and Codex read at start-up, so Ilium maintains them for you.

### What gets edited

| Scope | Files |
| --- | --- |
| Global (Claude) | `~/.claude/CLAUDE.md`, or the custom path chosen for that feature in Settings |
| Global (Codex) | `~/.codex/AGENTS.md` |
| Each open project | `CLAUDE.md` and `AGENTS.md` in the project root, only when you install there from the Setup tab |

Edits are **marker-delimited blocks**, so only text between Ilium's own markers is ever replaced or removed:

```text
<!-- ilium-agent-feature: chatroom -->
...instruction text...
<!-- /ilium-agent-feature: chatroom -->
```

The Progress block also carries a schema version (`<!-- ilium-agent-feature: progress version=9 -->`), which lets Ilium upgrade an older block and leave a newer one alone. Text around the block is preserved. If a file already contains wording that looks like the Ilium instruction but has no markers, Ilium reports it as "Instruction detected" and does **not** treat it as its own or delete it. Files larger than 512 KiB are not modified. A symlinked instruction file stays a symlink after an update (for example `AGENTS.md` linking to `CLAUDE.md`).

Ilium installs and refreshes the global blocks automatically when it attaches to a session and when projects open; it does not wait for you to visit Settings. The global files already apply to every project, so Ilium never adds project copies on its own: they would repeat the same text, and a new project `AGENTS.md` would stop Codex from reading that project's `CLAUDE.md`. Treat the instruction files as managed by Ilium for these two blocks. Write your own instructions anywhere outside the markers.

### The Setup tab

Settings, **Setup** lists every target per feature with a status badge:

| Status | Meaning |
| --- | --- |
| Set up by Ilium | The current managed block is present. |
| Ilium update required | An older managed block is present; it is refreshed automatically. |
| Newer Ilium instructions detected | The file was written by a newer Ilium; left untouched. |
| Instruction detected | Matching text that Ilium did not write. |
| Not set up | No block yet. |

Per feature there is a **Global file** row (the file Ilium treats as the global Claude target; **Enter** changes it, **R** restores `~/.claude/CLAUDE.md`), a **Global** row for each global file, and a **Project** row for every open project. Press Enter on a target to refresh it now. Setup is not an on/off switch: removing a mandatory block would leave newly started agents uninformed. To remove a block, delete the text between its markers yourself.

The path you type may start with `~/`, be absolute, or be relative to the session's project directory. The choice is stored under `[agent_setup]` as `chatroom_global_file` and `progress_global_file`.

---

## Local HTTP API

The detached server can create agents on request over a small HTTP listener. It is meant for scripts and local tools ("start an agent in project X with this prompt").

> **Security warning.** The listener has **no authentication**. Any process on this machine, running as any user, that can reach `127.0.0.1` can submit a request, and a request starts an agent with a prompt the caller controls. It binds to the loopback interface only and never accepts remote connections, but you must treat the port as a local control surface: do not forward it, do not expose it through containers or tunnels, and do not run untrusted local code that you would not trust to start agents. Change the port if another session or program collides.

### Port

The default is `127.0.0.1:8872`. Change it under Settings, **API** (**HTTP API port**), or in `config.toml`:

```toml
[api]
port = 8872
```

A port of 0 is rejected. A change takes effect the next time the detached server starts; the TUI does not restart the server for you. If the port cannot be bound the server logs an error and carries on with everything else, only the API is unavailable. Each project session has its own server, so two sessions cannot both listen on the same port.

### `POST /create_agent`

Request body (JSON):

| Field | Type | Meaning |
| --- | --- | --- |
| `agent_type` | `"claude"` or `"codex"` | Which agent CLI to start. |
| `project` | string | An absolute directory path, or a bare project name. A path containing a separator is resolved directly. A bare name matches the session's own project directory name, or a directory of that name up to two levels below `~/dev` (`~/dev/name` or `~/dev/area/name`, ignoring hidden directories). A name that matches nothing, or more than one directory, is an error: give the absolute path. |
| `prompt` | string | The first prompt. Must not be empty or whitespace. |
| `workspace` | object, optional | Start the agent in a new linked Git worktree: `{"branch": "agent/inspect", "base": "main"}`. `base` is optional. Any other key (for example `path`) is rejected: Ilium chooses the worktree location itself. |

Example:

```sh
curl -sS -X POST http://127.0.0.1:8872/create_agent \
  -H 'Content-Type: application/json' \
  -d '{"agent_type":"claude","project":"/home/me/dev/app","prompt":"Fix the failing login test"}'
```

With a worktree:

```sh
curl -sS -X POST http://127.0.0.1:8872/create_agent \
  -H 'Content-Type: application/json' \
  -d '{"agent_type":"codex","project":"app","prompt":"Investigate the slow query","workspace":{"branch":"agent/slow-query","base":"main"}}'
```

Success response:

```json
{"pane_id": 42, "project_path": "/home/me/dev/app", "prompt_delivered": true}
```

The request waits until the agent's prompt is ready, then submits the prompt, so a slow-starting agent delays the response. Errors return a JSON body `{"error": "..."}`: status 400 for an empty prompt, an unresolved or ambiguous project, or an invalid worktree request; 500 for a failure creating the pane or worktree; 503 if the session is shutting down. If the HTTP client disconnects while a worktree is being created, creation continues under server control and is rolled back at safe points. See [Worktrees](worktrees.md) for what the worktree option does.

---

## Reset forecasts

The status bar can show when the providers' public, discretionary usage resets are announced. These come from public announcement feeds, not from your account.

| Setting (Settings, **Reset Planning**) | Default | Meaning |
| --- | --- | --- |
| Monitor Claude resets | On | Checks the public Claude reset feed. The feed currently supplies announced reset **history** and no future reset time, so there is no Claude countdown. |
| Monitor Codex resets | On | Checks public Codex reset announcements. A countdown appears only when an announcement carries an exact scheduled time. |
| Time display | Exact | **Exact** shows a live countdown (`2d 3h 4m 5s`). **Human** shows rounded wording such as `3 hours`, `2 days` or `tomorrow`. |

Details:

- Each feed is polled **hourly**, with a 12 second request timeout and a 1 MiB response limit. Responses older than six hours, or from the future, are rejected.
- Labels you may see: "Codex reset in 3h 12m 4s"; "Codex banked reset ..." for a banked reset; "... scheduled - time TBD" when announced without a time; "... due - awaiting confirmation" once the time has passed; and "Possible Codex reset watch window ends in ..." for a forecast window whose end is not a promised reset.
- The settings page shows per provider: `Off`, `On - checking...`, `On - check failed: ...`, `On - reset announced`, `On - history only` (Claude) or `On - no scheduled reset` (Codex).
- Public announcements are separate from your account's rolling usage windows. Ilium cannot predict a private rolling limit or guarantee a reset. Do not plan around a forecast.
- Turning a monitor off stops its network requests and clears its state.

Stored under `[reset_planning]` as `monitor_claude`, `monitor_codex` and `time_style` (`"exact"` or `"human"`). Network use is described in [Inference and privacy](inference-and-privacy.md).

---

## Troubleshooting

| Symptom | What to check |
| --- | --- |
| Scheduled input did not fire | Another schedule replaced it; the pane was closed; the server was not running (it must be, but the TUI need not be). |
| Scheduled text landed in the wrong place | The pane was not at the prompt you expected. Schedule input does not wait for idle; use the prompt queue for that. |
| Text trigger never fires | Rule disabled; Target excludes the pane's classification (an agent pane is not a shell); the regexp does not match what is on the **screen** (colours and wrapping matter); the match is a repeat of text already answered; the delay has not elapsed; the file has an invalid rule so the old list is still active. |
| Text trigger loops | The message matches its own regexp. Narrow the pattern or change the message. |
| Queued prompts do not go out | The agent has not finished a turn since they were queued; the head is fenced after an interrupted delivery (clear the queue and re-queue); the queue is at 100. |
| "Ask for update" is missing | No agent in scope is idle. |
| `ilium chat send` says the project does not have a chatroom | Run `ilium chat init` there, or add the room from the project menu. |
| Agents ignore Chatroom or Progress rules | Check Settings, Setup for the target status; start a new agent session so it re-reads its instruction files. |
| `curl` to port 8872 is refused | Server started with another `[api] port`; the port was in use when the server started (see the server log); the server has not been restarted since you changed the port. |
