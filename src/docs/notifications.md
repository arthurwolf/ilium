# Notifications and sounds

Ilium can tell you when an agent needs you, even if you have detached from the session or are looking at another window. It does this in two independent ways: a **sound** and a **desktop notification**. Each has its own per-event switches. This page explains the events, how to choose and test a sound, how the desktop-notification switches work, the difference between agent events and task events, the rules that merge or skip task alerts, and the `[sound]` and `[notifications]` configuration. Both are produced by the detached server, so they keep working when no terminal is attached.

Contents:

- [Two channels, many events](#two-channels-many-events)
- [Agent events and task events](#agent-events-and-task-events)
- [Sounds](#sounds)
- [Desktop notifications](#desktop-notifications)
- [How task alerts are merged or skipped](#how-task-alerts-are-merged-or-skipped)
- [Configuration reference](#configuration-reference)
- [Step by step: quiet by default, loud for approvals](#step-by-step-quiet-by-default-loud-for-approvals)
- [Troubleshooting](#troubleshooting)

Related pages: [Agent monitoring](agent-monitoring.md) (where the underlying states and progress monitors are explained), [Settings](settings.md), [Session recovery](session-recovery.md).

## Two channels, many events

| Event | Sound switch | Desktop switch | Sound default | Desktop default |
| --- | --- | --- | --- | --- |
| Agent finished (a busy agent completes its turn and waits for you) | Agent finished | Agent finished | On | On |
| Agent needs approval | Approval required | Approval required | Off | On |
| Agent started working (idle or finished agent begins a new turn) | Agent started | none | Off | not available |
| Agent waiting for background work it started | Waiting background | none | Off | not available |
| Task succeeded | Task succeeded | Task succeeded | Off | Off |
| Task failed or monitor lost | Task failed or monitor lost | Task failed or lost | On | On |

So there are six sound events and four desktop-notification events. Agent-started and waiting-background exist for sound only.

All switches live in **Settings -> Sound** (`Ctrl+B :`, then the Sound tab). Changes are written to `config.toml` and applied to running servers within a couple of seconds. There is no restart.

## Agent events and task events

- An **agent event** comes from the detection loop: the agent finished a turn, or is blocked on an approval prompt. They use the same projected status the tree shows. A transition from working (or waiting on background agents) to finished-unread triggers "agent finished"; the first-ever classification of a pane never does.
- A **task** is a background job an agent registered with `ilium progress` (see [Agent monitoring](agent-monitoring.md#progress-monitors-and-progress-displays)). Its outcome is a different event from the agent finishing its turn: **succeeded**, **failed**, or **lost** (Ilium lost the ability to observe it, which does not prove the task failed).

Task notifications are worded to make the difference clear, for example "background task finished (agent still working)", and they lead with the pane title.

By default you hear about the agent finishing, an approval prompt, and failed or lost tasks. Successful tasks stay silent and show only as a green tick in the sidebar, because a task finishing while its agent keeps working is routine progress.

## Sounds

Open **Settings -> Sound**.

### Choose a sound source

| Source | `source` value | What plays |
| --- | --- | --- |
| System beep (default) | `system_beep` | The operating system's alert: `canberra-gtk-play` or `beep` on Linux, the system beep on macOS, `MessageBeep` on Windows |
| Sound file | `sound_file` | A file you pick from sounds Ilium discovered on this system |
| Bundled chirping | `bundled_chirping` | A short sample embedded in Ilium, so it works with no sound package installed |
| Custom sound | `generated` | A tone designed in the settings: waveform, pitch, slide, envelope, pulse, harmony, brightness, noise, duration and volume |
| Muted | `muted` | Nothing plays |

### Pick, preview and design

1. Choose a source with the left and right keys (or click).
2. For **Sound file**, open the file list. Ilium searches the common sound folders that exist on your platform (to a bounded depth and count, without following directory symlinks) and lists what it finds. It never invents paths.
3. Activate **Preview** to play the selected sound once. Help screens never play audio.
4. For **Custom sound**, adjust the design controls and preview again. The design is kept when you switch to another source so reopening the studio restores it. Ranges: pitch 60 to 1800 Hz, pitch slide plus or minus 1800 cents, duration 80 to 3000 ms, volume 0 to 100 percent; waveforms are sine, triangle (default), saw and square.
5. Turn individual sound events on or off (the six rows above).

Playback is performed by the server, using whichever player is available (on Linux, for example `pw-play`, `paplay`, `mpv`, `aplay` or `ffplay`). One playback is cut off after 15 seconds. If the selected file is missing or no player exists, playback reports an error rather than failing silently.

## Desktop notifications

Desktop notifications go through the operating system's notification service. They are sent without blocking the rest of Ilium, so a slow or missing notification daemon cannot freeze your session.

In **Settings -> Sound**:

| Switch | `config.toml` key | Default |
| --- | --- | --- |
| Master switch (turns every desktop notification off at once) | `enabled` | On |
| Agent finished | `agent_finished` | On |
| Agent needs approval | `approval_required` | On |
| Task succeeded | `task_succeeded` | Off |
| Task failed or lost | `task_failed` | On |
| Skip redundant task outcomes | `suppress_redundant_task_outcomes` | On |
| Merge repeated task alerts within (seconds) | `task_coalesce_seconds` | 30 |

A notification is sent only when both the master switch and the event's own switch are on. The master switch does not change the individual switches, so you can silence everything temporarily and get your previous choices back.

The desktop switches and the sound switches are separate: you can have a sound for approvals but no pop-up, or the reverse.

## How task alerts are merged or skipped

Two rules reduce noise. Both apply to task sounds and task notifications alike. Both can be changed in Settings.

1. **Skip redundant outcomes.** Task alerts are skipped while the agent in that pane is idle, finished-unread or parked. In those states Ilium is about to deliver the result to the agent, which resumes, and its own "agent finished" alert follows, so the task alert would be a duplicate. Panes with no agent are never skipped. Turn this off with `suppress_redundant_task_outcomes = false`.
2. **Merge repeats.** Alerts of the same kind on the same pane that arrive closer together than the merge window collapse into the first. The window is 30 seconds by default, adjustable in 10-second steps up to 600 seconds. `0` disables merging. Kinds are counted separately, so a success and a failure on one pane do not merge with each other.

## Configuration reference

```toml
[notifications]
enabled = true
agent_finished = true
approval_required = true
task_succeeded = false
task_failed = true
suppress_redundant_task_outcomes = true
task_coalesce_seconds = 30        # 0 to 600; values above 600 are clamped

[sound]
source = "system_beep"            # system_beep | sound_file | bundled_chirping | generated | muted
file = "/usr/share/sounds/example.oga"   # used by source = "sound_file"

[sound.events]
agent_finished = true
approval_required = false
agent_started = false
waiting_background = false
task_succeeded = false
task_failed = true

[sound.design]                    # used by source = "generated"
waveform = "triangle"             # sine | triangle | saw | square
```

Further `[sound.design]` keys are the studio controls: `pitch_hz`, `pitch_slide_cents`, `attack_ms`, `decay_ms`, `sustain_percent`, `release_ms`, `pulse_rate_tenths_hz`, `pulse_depth_percent`, `harmony_semitones`, `harmony_mix_percent`, `brightness_percent`, `noise_percent`, `duration_ms` and `volume_percent`. Out-of-range values are clamped. A `[sound] file` path beginning with `-` is handled as data, never as an option for the player.

## Step by step: quiet by default, loud for approvals

1. Open **Settings -> Sound**.
2. Choose **Bundled chirping** (or a file you like) as the source and press **Preview**.
3. Under sound events, turn on **Approval required** so a blocked agent makes a noise.
4. Leave **Agent finished** on so you hear completed turns.
5. In the desktop switches, keep **Agent finished** and **Approval required** on, and leave **Task succeeded** off.
6. If long background tasks cause a stream of alerts, raise the merge window.

## Troubleshooting

| Symptom | Explanation |
| --- | --- |
| No sound at all | Source is Muted, the event is off, the file was removed, or no audio player is installed. Use Preview to see the error |
| No desktop pop-ups | Master switch off, the event switch off, or no notification service is running on your desktop |
| Task finished but no alert | Successful tasks are silent by default, or the agent was idle and the redundant-outcome rule applied; its own finished alert follows |
| Two task alerts became one | Merge window: same kind on the same pane within the window |
| Monitor lost alert but the task finished | "Lost" means observation failed, which does not prove the task failed. Check the task itself |
| Alerts arrive when no terminal is attached | Intended: the server owns playback and notifications |
