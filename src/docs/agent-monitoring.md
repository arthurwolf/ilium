# Agent monitoring

Ilium watches every terminal pane for an AI coding agent, works out which agent it is and what that agent is doing, and shows the answer in the project tree. This page explains how identity and activity are detected, what each status indicator means, how Normal and Attention display modes differ, how the `/goal` badge, progress footers and reserved terminal space behave, how to recover work when an agent stops, and which agent CLIs are supported. Detection is read-only: Ilium observes processes and the visible screen and never controls the agent.

Contents:

- [How detection works](#how-detection-works)
- [Supported agent CLIs](#supported-agent-clis)
- [The three icon slots in the tree](#the-three-icon-slots-in-the-tree)
- [Status indicators and their defaults](#status-indicators-and-their-defaults)
- [Normal mode and Attention mode](#normal-mode-and-attention-mode)
- [The `/goal` badge](#the-goal-badge)
- [Progress monitors and progress displays](#progress-monitors-and-progress-displays)
- [Reserved terminal space](#reserved-terminal-space)
- [The Agent Monitoring settings tab](#the-agent-monitoring-settings-tab)
- [Last prompt banner and agent toolbar](#last-prompt-banner-and-agent-toolbar)
- [Recover work after an agent stops](#recover-work-after-an-agent-stops)
- [The stats dot and session statistics](#the-stats-dot-and-session-statistics)
- [Configuration reference](#configuration-reference)
- [Troubleshooting](#troubleshooting)

Related pages: [Agent cost](agent-cost.md), [Notifications](notifications.md), [Session recovery](session-recovery.md), [Automation](automation.md), [Settings](settings.md).

## How detection works

Detection combines two independent signals.

1. **Identity from the process tree.** The server walks the process tree below each pane's shell and looks for a process whose name contains a known substring (`claude`, `codex`, `agy`, and the generic entries `opencode` and `aider`). A match makes the pane an agent pane. Identity does not depend on what is printed on screen, so a changed splash banner cannot break it. Ilium also records the matched process, its start time and how deep in the tree it sits, so a reused operating-system process ID cannot inherit the previous agent's state.
2. **Activity from the rendered screen.** The visible character grid of the pane is classified as working, waiting for approval, waiting for background workers, or idle. A process name cannot tell you whether the agent is mid-turn, so this part has to read the screen.

Further rules:

- One idle sample straight after an active turn is provisional. Ilium keeps the active state, schedules a quick recheck, and marks the turn as finished (unread) only after a second consecutive idle sample from the same process.
- For Claude Code, the latest recognised completed-turn summary is a boundary: earlier working and background-wait lines belong to the previous turn.
- Goal state is read from the provider's own status row and is independent of current activity.
- Unread completion clears when you open the pane, submit new terminal input, or the agent starts working again. Opening the pane clears the bell for every attached client.
- Ilium can answer known one-time interstitial dialogs for you. Currently this is Claude Code's "resume full session" prompt, answered with one keystroke per agent process. It is on by default (`auto_answer_interstitial_prompts` under `[detection]`).

### Polling cadence

Detection is adaptive rather than fixed.

| Pane state | Default check interval | Setting |
| --- | --- | --- |
| Working or waiting for approval | 10 seconds | `working_poll_seconds` |
| Idle, finished, or plain shell | 45 seconds | `idle_poll_seconds` |

Both values are whole seconds, are applied to a running server within moments of saving, and are clamped to a minimum of 500 ms (a value of `0` means 500 ms). They are detection checks only. They do not change how fast an agent works and are unrelated to progress-monitor polling.

### Custom agent signatures

Add your own agent CLI under **Settings -> Agent Monitoring -> Custom agent signatures**, or in `config.toml`:

```toml
[detection]
working_poll_seconds = 10
idle_poll_seconds = 45

[[detection.custom_signatures]]
process_name = "myagent"
agent_class = "other"
```

- `process_name` is a lowercase substring matched against process names. It is trimmed and lowercased on save, and must not be empty. It does not match prompts, command output or arbitrary command-line arguments.
- `agent_class` is one of `claude`, `codex`, `antigravity` or `other`. `other` shows the generic agent identity and carries the matched process name.
- Matching only classifies a process that is already running. Adding a signature does not install or launch anything.
- A custom signature gives identity and a running or idle state. It has no declared transcript contract, so features that need a provider transcript (history-file path, session statistics, resume) are not offered for it.
- Keep signatures non-overlapping. This page makes no promise about precedence between overlapping substrings.

## Supported agent CLIs

| CLI | Launch name | Identity | Resume on restore | Session conversion | History-file path | Cost and stats |
| --- | --- | --- | --- | --- | --- | --- |
| Claude Code | `claude` | Yes | `claude --resume <id>` | Yes (to Codex) | Yes | Yes |
| Codex | `codex` | Yes | `codex resume <id>` | Yes (to Claude Code) | Yes | Yes |
| Antigravity | `agy` | Yes | `agy --conversation <id>` | No | No (SQLite store) | No |
| OpenCode, Aider | `opencode`, `aider` | Generic identity only | No | No | No | No |
| Custom signature | any | As configured | No | No | No | No |

`ilium new-pane --worktree` accepts exactly one built-in agent command: `claude`, `codex` or `agy`. See [Worktrees](worktrees.md).

## The three icon slots in the tree

Every pane row has three icon slots before its title.

| Slot | Shows |
| --- | --- |
| Identity | Which agent runs here (or a plain terminal, editor or board) |
| Objective | The agent's `/goal`, a monitored task, or a scheduled input |
| Now | What the agent is doing this moment |

Identity renders as an icon by default. In **Settings -> User Interface** you can choose full name, one letter, the configured icon, or no identifier. Every glyph below is configurable in **Settings -> Icons** and under `[ui]` in `config.toml`.

Hover any row to see a popover with the meaning and a grey "why" line giving the rule that selected the icon and the evidence observed.

## Status indicators and their defaults

### Identity

| Default glyph | Meaning |
| --- | --- |
| crab | Claude Code |
| turtle | Codex |
| atom | Antigravity |
| robot | Any other detected agent |
| desktop | Plain terminal with no agent |
| memo / chart | Editor / board pane |

### Objective slot

| Default glyph | Meaning |
| --- | --- |
| target | `/goal` active |
| pause | `/goal` paused |
| construction | `/goal` blocked |
| hourglass | `/goal` stopped by a usage limit |
| chequered flag | `/goal` reached |
| Braille bar (cyan, yellow when degraded) | A monitored task's progress, in twelve percentage buckets |
| circle | Task registered but not started |
| green tick | Task done |
| red cross | Task failed |
| warning sign | Task observation lost by its monitor |
| alarm clock | A scheduled input is pending |

### Now slot

| Default glyph | Meaning |
| --- | --- |
| animated Braille spinner | Agent is working (90 ms frames) |
| animated clock | Agent waits for background agents or tasks it started |
| raised hand (bold) | Agent waits for your approval |
| cyclone | Turn is over, but a background shell or task it started is still running |
| zzz | Agent is idle and parked on a live progress monitor: not finished, no bell |
| bell (pulsing) | Agent finished a turn you have not seen |
| dot | Idle at its prompt, nothing running or unread |
| lifebuoy | Agent unavailable (exited or lost terminal ownership) |
| angular Braille loop | Ordinary terminal whose visible grid changed, or received your key input, within the last 60 seconds |

Notes:

- Task icons are bold while unread and dim once seen.
- Motion level **Off** (Settings -> User Interface) freezes activity animation without stopping any agent.
- Plain-terminal activity is event-driven, not polled. The angular loop runs at normal speed for five seconds, slows to one frame per 500 ms until 60 seconds, then disappears. An idle terminal does no screen checks or animation work.
- The `Settings -> Agent Monitoring` tab has one icon row per status above, so each can be changed independently.

### Task progress frames

A monitored task's progress icon is drawn from a family of frames. **Settings -> Agent Monitoring -> Progress fill style** offers Braille, Blocks, Moons and Quarters. For a custom family set `[ui].task_progress_frames` to a list of 2 to 13 printable frames, all one cell wide or all two cells wide.

## Normal mode and Attention mode

**Settings -> Agent Monitoring -> Display mode** (`[ui].agent_monitoring_mode`, `"normal"` or `"attention"`; default `normal`).

- **Normal** shows the objective and now slots side by side.
- **Attention** shows one status icon: the highest-priority signal. Agent identity stays separate. The underlying evidence is unchanged, so a paused goal that is also waiting for approval is still paused; Attention just shows the approval.

Attention priority, highest first:

1. Waiting for approval.
2. Task monitor failed (and the task has no terminal result).
3. Task error.
4. A non-active goal: paused, blocked, usage-limited or reached.
5. Successful task whose result is unread.
6. Unread completed turn.

An active goal does not claim the Attention slot. A terminal task error remains visible after acknowledgement; successful task and turn completion do not.

When nothing in that list applies and the agent is simply working, Attention would otherwise be blank. **Attention running indicator** (`[ui].attention_running_indicator`) chooses what to show:

| Value | Label | Appearance |
| --- | --- | --- |
| `off` | Off | Nothing; a working agent looks like a quiet one |
| `icon` (default) | Working icon | The configurable Working icon, animated as in Normal mode |
| `spinner` | Spinner | A Braille spinner |
| `pulsing_dot` | Pulsing dot | A dot breathing between bright and dim |
| `steady_dot` | Steady dot | A steady dim dot, no motion |
| `title_accent` | Title accent | No glyph; the title takes an accent colour and italics |

Normal mode ignores this setting and always shows the working animation.

## The `/goal` badge

Claude Code and Codex support a long-term `/goal`. Ilium reads the provider's own status row and maps it to the objective slot: active, paused, blocked, usage-limited or reached. Key points:

- The badge reports observed state. Ilium does not set, pause, resume or complete goals and never touches an agent's `/goal`.
- Goal state is independent of current activity: an active goal can sit beside an idle agent. Goal "reached" is not proof that every background task succeeded.
- A blocked goal is not inferred from error-looking terminal text, and is separate from approval requests and task failures.
- A progress monitor's result message never pauses, resumes or otherwise changes the agent's goal.
- Each of the five goal states has its own icon row in Settings -> Agent Monitoring.

## Progress monitors and progress displays

A *task* is a long-running job that an agent registered with the `ilium progress` command from inside its pane. The detached Ilium server, not the agent, then runs a small probe command on a timer and shows the result as a footer on the pane and an icon in the tree.

```sh
ilium progress check --command '/abs/path/to/probe.sh'
ilium progress set   --command '/abs/path/to/probe.sh' --interval-seconds 5
ilium progress status
ilium progress clear --monitor-id 12
```

| Subcommand | Purpose |
| --- | --- |
| `check --command <cmd>` | Run and validate one probe through the server without installing it |
| `set --command <cmd> [--interval-seconds <n>]` | Validate, then atomically start or replace this pane's monitor; waits for a server acknowledgement with the monitor ID and first report. Default interval 1 second |
| `status` | Print the current registration, latest report and monitor health |
| `clear [--monitor-id <id>]` | Stop the monitor and clear retained progress; the ID fences the call so a stale agent cannot clear a newer monitor |

Every operation prints exactly one JSONL record. The probe must print exactly one JSON object with:

| Field | Meaning |
| --- | --- |
| `job_id` | Stable non-empty identifier |
| `status` | `not-started-yet`, `running`, `error` or `done` |
| `percent` | Finite number from 0 to 100 |
| `message` | One-line description, always shown in the footer |
| `details` | Optional multi-line text (at most 8 KiB), shown in a tooltip when the pointer rests on the footer |
| `error` | Required when `status` is `error` |

Important rules for the probe:

- It runs from the server's working directory (the project root), not the pane's current directory, and without your shell aliases. Use absolute paths.
- It is started as a new process every tick, so keep it cheap; raise `--interval-seconds` if it is not.
- Task `error` (the job failed) is kept distinct from monitor failure (Ilium could not observe it).
- Final results are persisted. The server delivers a result to the agent only when it sees a clean agent prompt.
- An idle agent waiting on a live monitor is shown as parked, and no "finished" bell is raised for it.

The agent-side instructions that teach agents to use this (and the project chatroom) can be installed from the guided setup; see [Automation](automation.md).

### Progress display settings

All under **Settings -> Agent Monitoring**.

| Setting | Config key | Default | Notes |
| --- | --- | --- | --- |
| Progress monitor | `[ui].progress_monitor_enabled` | On | Off stops observation and rejects new registrations. Shell access and already-recorded results are unchanged. Live-toggleable |
| Progress footer lines | `[ui].progress_max_lines` | 4 | 1 to 20 task-detail rows below the status and percentage line |
| Hide completed progress after | `[ui].completed_progress_hide_after_seconds` | 60 | Steps of 30 seconds; `0` = Never |
| Progress fill style | `[ui].task_progress_frames` | Braille | Braille, Blocks, Moons, Quarters or custom |

Hide-after behaviour: the delay starts from the server's final observation and does not restart when a client reconnects. Hiding changes presentation only. Text disappears but the reserved rows remain; error details, unread indicators, retained results and queued notifications are untouched, and the child terminal is not resized.

## Reserved terminal space

Enabled prompt, progress and agent-toolbar displays reserve their rows **before** terminal interaction begins, including in plain shells that might later start an agent. Updating, wrapping, clearing or hiding their text leaves the child terminal's dimensions unchanged, so a running program never sees its window jump.

To reclaim or change the space, turn a display off or lower its row limit in Settings. The child can still be resized by explicit settings changes, split changes and resizing the outer terminal. Editors and boards reserve no terminal metadata space, and tiny panes keep at least one terminal cell where physically possible.

## The Agent Monitoring settings tab

| Row | What it does |
| --- | --- |
| Display mode | Normal or Attention |
| Attention running indicator | What a quietly working agent shows in Attention mode |
| Working poll interval | Seconds between checks while working or awaiting approval |
| Idle poll interval | Seconds between checks while idle |
| Custom agent signatures | Add, edit or remove process-name rules |
| Progress monitor | Master switch for monitors |
| Progress footer lines | Row limit of the footer |
| Hide completed progress after | Footer expiry |
| Progress fill style | Frame family |
| Status icons | One row per status listed above |

Every row has an in-app help topic (open it from the row).

## Last prompt banner and agent toolbar

Two optional per-pane displays sit above a detected agent's terminal.

- **Last prompt banner** (`[ui].last_prompt_enabled`, default on). Shows the last prompt Ilium could reconstruct exactly as submitted to the agent. If later input cannot be recognised, the banner keeps the last captured prompt rather than guessing. **Last prompt banner lines** (`[ui].last_prompt_max_lines`, 1 to 20, default 4) fixes how many rows are reserved; longer prompts wrap and keep their beginning and end.
- **Agent toolbar** (`[ui].agent_toolbar_enabled`, default on). Action buttons above detected agents (for example compact, clear, model, effort, stop, copy screen, Smart Copy, copy last message; which appear depends on the agent). Hide or reopen it from the pane's icon or the right-click menu (**Hide agent toolbar** / **Show agent toolbar**). **Agent toolbar labels** adds text after the icons.

## Recover work after an agent stops

When a detected Claude or Codex process exits or loses terminal ownership, the pane keeps its agent identity and shows an **unavailable** indicator. Hover for the reason. Ilium names an exit cause only when it has a matching process receipt; a missing nested process is never turned into an invented cause.

### Copy what you need

Right-click the terminal pane. The menu offers:

| Item | What it copies |
| --- | --- |
| Copy last submitted prompt | The exact last prompt, keeping line breaks and trailing spaces |
| Copy selection | The current selection |
| Copy line to clipboard | The line under the pointer |
| Copy visible terminal to clipboard | The visible screen |
| Copy full terminal history | The whole scrollback |
| Copy path to history file | The verified absolute path to the agent's JSONL transcript |

Details:

1. If terminal-owned editing made the latest prompt uncertain, the menu says so and offers any **previous exact prompt** separately.
2. The history-file path is offered only for Claude Code and Codex, and only when Ilium has verified the transcript's embedded identity and launch directory. Antigravity stores conversations in SQLite and custom signatures have no transcript contract, so neither offers a path.
3. If transcript discovery reaches its safety limit, the menu warns that the path is unavailable. Screen, selection and prompt copying still work. Ilium never offers an unverified path.
4. Scrolling and selection stay local to Ilium even if the stopped agent left mouse tracking on.

### Keep working

- The header stats dot stays available for the retained session. If a transcript refresh fails, retained figures stay visible with an incompleteness warning and the reason.
- You can still paste into or type in a surviving shell. Agent automation (scheduled input, prompt queue delivery) stops once the agent loses ownership.
- If a write was only partly delivered before cancellation, Ilium refuses further input to that terminal. Copy the recovered text into another pane to continue.
- To continue the conversation under another provider, use **Convert to** from the tree menu; see [Session recovery](session-recovery.md#convert-a-session-between-claude-and-codex).

## The stats dot and session statistics

For a detected Claude Code or Codex pane, the second icon in the pane header opens a costs-and-stats popover. Hover previews it; click pins it. It is non-modal, so keystrokes keep reaching the agent. Click the icon or its close control to dismiss it.

The popover has four tabs: **Overview**, **Tokens**, **Activity** and **Prompts**. All data is parsed locally from the agent's own JSONL transcript on a low-priority worker thread; nothing is sent to the server or any network. Large transcripts are read incrementally. Dollar figures here appear only when the agent itself recorded them (Claude Code's own cost snapshot); the cost indicators described in [Agent cost](agent-cost.md) price tokens separately.

## Configuration reference

```toml
[detection]
working_poll_seconds = 10
idle_poll_seconds = 45
auto_answer_interstitial_prompts = true

[[detection.custom_signatures]]
process_name = "myagent"
agent_class  = "other"

[ui]
agent_monitoring_mode = "normal"          # or "attention"
attention_running_indicator = "icon"
progress_max_lines = 4                    # 1 to 20
completed_progress_hide_after_seconds = 60  # 0 = never hide
last_prompt_enabled = true
last_prompt_max_lines = 4                 # 1 to 20
agent_toolbar_enabled = true
```

Invalid values are rejected with an explicit message rather than silently replaced (for example an unknown `agent_monitoring_mode`, or `progress_max_lines` outside 1 to 20). The config file lives at `~/.config/ilium/config.toml` on Linux.

## Troubleshooting

| Symptom | Likely cause and fix |
| --- | --- |
| A pane shows a plain terminal although an agent runs | The agent process name matches no signature. Add a custom signature, or check you launched the real CLI rather than a wrapper with a different process name |
| Status lags behind the screen | Lower `working_poll_seconds` or `idle_poll_seconds` (minimum 500 ms); shorter intervals cost more CPU |
| Footer vanished but I want it back | Set **Hide completed progress after** to Never |
| Terminal looks shorter than expected | Prompt, progress and toolbar displays reserve rows. Turn one off or lower its line limit |
| No "history file" menu item | Not Claude Code or Codex, transcript not yet verified, or discovery hit its limit |
| Attention mode shows no goal | An active goal is intentionally not an Attention signal; switch to Normal mode to see it |
