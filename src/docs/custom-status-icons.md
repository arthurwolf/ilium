# Custom status icons

Custom is the third Agent Monitoring display mode, next to Normal and Attention. You write ordered rules, and the first rule that matches a pane chooses the one status icon shown beside it in the tree. This page covers the rule model, every condition, the shipped defaults, the icon choices, the editor and troubleshooting. For the other modes see [Agent monitoring](agent-monitoring.md).

## Turn it on

1. Open **Settings -> Agent Monitoring** and choose the **Custom** card under **Display mode**, or set `[ui].agent_monitoring_mode = "custom"`, or set the settings path `ui.agent_monitoring_mode` to `custom` through voice control's Settings tool (see [Settings](settings.md#configtoml-structure)).
2. Select **Configure custom icons** to open the rules editor. The button shows how many rules exist and how many are enabled.

Rules are global: they apply to every project and every session, and they are saved as you edit. Custom shows exactly one three-cell status slot on every pane row, including editors and boards, so titles stay aligned. Agent identity (the icon or letter that names the agent) stays separate from the status slot.

## How rules are evaluated

- Rules run from top to bottom. The first **enabled** rule whose expression matches decides the icon. Later rules are not looked at, so put a specific rule above a broad one.
- A rule has one expression line. Conditions are joined by **AND** or **OR**. **AND binds tighter than OR**, so `A OR B AND C` means `A OR (B AND C)`. **NOT** applies to a single condition, never to a group.
- Every condition is true, false or **unknown**. A fact that cannot be observed (for example a goal on a plain terminal, or a screen that was not scanned yet) is unknown. `NOT unknown` is still unknown, so negating an unobserved fact does not make the rule match.
- If no rule matches, the **Otherwise (no rule matches)** row decides. It is empty by default, which leaves the status slot blank.
- Limits: 64 rules, 8 AND groups per rule, 8 conditions per group, 64 conditions per rule, rule names of 48 characters.

Hover a pane's status cell in Custom mode to read which rule chose the icon and why, for example `Why: Custom rule 3 "Goal paused": goal is paused AND NOT agent is working`, or why no rule matched, including how many rules depended on a fact that was not observed.

## Conditions

The picker groups conditions as the table below does. The table is checked by a test against the engine's condition registry, so it lists every condition the engine knows.

<!-- conditions:start -->
| Condition | Group | Meaning |
| --- | --- | --- |
| `always` | Pane | Matches every pane. Use it as the last rule to pick a fallback icon. |
| `pane-kind` | Pane | The kind of pane: a live agent, a plain terminal, a lost agent, an editor or a board. Custom icons apply to agent and terminal panes. |
| `editor-dirty` | Pane | The editor pane has unsaved changes. Unknown for any other pane kind. |
| `pane-bookmarked` | Pane | You bookmarked the pane in the sidebar. |
| `prompts-queued` | Pane | At least one prompt waits in the pane's prompt queue. |
| `prompt-queue-held` | Pane | The prompt queue stopped on a prompt whose delivery was interrupted. Ilium never resends it on its own, so the queue waits until you send or remove it. |
| `scheduled-input` | Pane | A prompt is queued to be sent to this pane later. |
| `agent-class` | Agent | Which agent CLI runs in the pane. Unknown unless a live agent is detected. |
| `other-agent` | Agent | The pane runs an agent recognised by a custom signature rather than Claude Code, Codex or Antigravity. Unknown unless a live agent is detected. |
| `agent-availability` | Agent | Why a lost agent is lost: unverified, a shell took the foreground, or the process exited (cleanly or with an error). Unknown for live agents and terminals. |
| `agent-turn` | Agent | The agent's current turn phase: working, needing approval, waiting for subagents, settling or idle. Unknown without a live agent. |
| `completion-unread` | Agent | The agent finished a turn you have not looked at yet. Unknown without a live agent. |
| `activity-stale` | Agent | The activity detector has not confirmed the agent's state recently. Unknown until the detector reports freshness. |
| `activity-unchecked` | Agent | The activity detector has never reported on this live agent, so its state is a guess. Unknown without a live agent. |
| `parked` | Agent | The agent is idle while a monitored task is still live: it is waiting for the task, not for you. Unknown without a live agent. |
| `goal-set` | Goal | A /goal is attached to the agent, whatever its state. Unknown without a live agent. |
| `goal-state` | Goal | The goal's state: active, paused, blocked, usage limited or reached. False when no goal is set; unknown without a live agent. |
| `monitor-count-at-least` | Monitors | The pane has at least this many progress monitors; zero monitors is always known. |
| `task-status` | Monitors | The selected (representative) monitored task is pending, running, failed or done. Unknown without a monitor. |
| `any-task-status` | Monitors | Any of the pane's monitored tasks has this status; false without monitors. |
| `monitor-healthy` | Monitors | The selected monitor's probe is answering normally. Unknown without a monitor. |
| `monitor-degraded` | Monitors | The selected monitor's probe is failing intermittently. Unknown without a monitor. |
| `monitor-failed` | Monitors | The selected monitor's probe has stopped answering. Unknown without a monitor. |
| `any-monitor-failed` | Monitors | Any monitor's probe has stopped answering; false without monitors. |
| `monitor-unread` | Monitors | The selected monitor's latest outcome has not been acknowledged. Unknown without a monitor. |
| `progress-measurable` | Progress | The selected task reports a percentage or a time estimate rather than an indeterminate spinner. Unknown without a monitor. |
| `progress-at-least` | Progress | The selected task's percentage is at or above the threshold. Unknown unless progress is measurable. |
| `progress-below` | Progress | The selected task's percentage is under the threshold. Unknown unless progress is measurable. |
| `progress-between` | Progress | The selected task's percentage lies in the range, both ends included. Unknown unless progress is measurable. |
| `eta-known` | Progress | The selected task reports a remaining-time estimate. False with a monitor that gives none; unknown without a monitor. |
| `eta-below-minutes` | Progress | The selected task's estimate is under this many minutes. Unknown without an estimate. |
| `shell-output-recent` | Terminal | A plain terminal printed output recently. Unknown for agent panes. |
| `shell-output-fast` | Terminal | A plain terminal is printing quickly, like a build log. Unknown for agent panes. |
| `shell-output-slow` | Terminal | A plain terminal is printing slowly, like a long test. Unknown for agent panes. |
| `screen-matches` | Screen text | A regular expression matches the pane's visible screen text, optionally only its first or last lines. Unknown until the screen has been scanned. |
| `pane-frozen` | Pane view | You froze the pane's screen. Known only for panes the client is currently displaying. |
| `pane-focused` | Pane view | The pane's terminal holds keyboard focus. False for every pane while focus is on the tree, a dialog or Settings. |
| `pane-selected` | Pane view | The tree cursor rests on the pane's row, whether the tree or the terminal holds keyboard focus. |
| `pane-visible` | Pane view | The pane's terminal is on screen in the right panel, alone or in a split view. Combine NOT with this to flag panes you are not looking at. |
| `blocking-error` | Pane view | A failed action targeting this pane left a retained blocking error that has not been dismissed. Known only for panes the client is currently displaying. |
| `session-error` | Pane view | The session holds a retained error that belongs to no particular pane, such as a server-wide failure. The same for every pane. Known only while the client is displaying the session. |
| `session-cost-above` | Pane view | The agent session's tracked spend exceeds this many dollars. Unknown until cost tracking has a figure for the pane. |
| `context-above-percent` | Pane view | The agent's context window was more than this percentage full at its latest prompt. Unknown until cost tracking has read the session's transcript. |
<!-- conditions:end -->

Several conditions take a value: a pane kind, an agent, a turn phase, a goal state, a task status, a percentage or a number of minutes or dollars. For panes with several monitors, the selected-task and progress conditions refer to Ilium's representative monitor, while the **any** conditions inspect every monitor and are false when there are none. Two **any** conditions joined by AND can be satisfied by different monitors.

### Screen text and regular expressions

`screen-matches` tests a regular expression against the text currently **visible** in the pane, never the scrollback. It can look at the whole screen, the first lines or the last lines (`L` in the editor), and can ignore case (`I`).

- The server evaluates the pattern and sends back only the match result, not the screen text.
- At most 16 distinct patterns across all enabled rules, each of at most 256 characters. A rule that would exceed the budget is disabled with a reason; an invalid pattern cannot be saved.
- A screen too large to observe, or not yet scanned, makes the condition unknown until it can be observed again.
- The Demo tree matches against synthetic screen text; the Live tree matches your real visible screens.

## Shipped default rules

New users, and Reset after confirmation, start from this ordered set. Approval, failure and unread results are bold, and the approval and unread-completion rules pulse. The last rule is a disabled example: enable it and move it above the activity and idle rules for it to affect agents.

<!-- defaults:start -->
| Order | Id | Name | Starts |
| --- | --- | --- | --- |
| 1 | `approval` | Needs your approval | on |
| 2 | `agent-lost` | Agent lost | on |
| 3 | `blocking-error` | Blocking error | on |
| 4 | `task-failed` | Task failed | on |
| 5 | `monitor-lost` | Monitor lost | on |
| 6 | `goal-blocked` | Goal blocked | on |
| 7 | `usage-limited` | Usage limit reached | on |
| 8 | `goal-paused` | Goal paused | on |
| 9 | `task-unread` | Task finished, result unread | on |
| 10 | `goal-reached` | Goal reached | on |
| 11 | `completion-unread` | Finished, not seen yet | on |
| 12 | `parked-progress-unsteady` | Parked, task monitor unsteady | on |
| 13 | `parked-progress` | Parked, task progressing | on |
| 14 | `parked` | Parked, waiting for task | on |
| 15 | `subagents` | Waiting for background agents | on |
| 16 | `settling` | Background work still running | on |
| 17 | `working-goal` | Working toward a goal | on |
| 18 | `working` | Working | on |
| 19 | `pending` | Task pending | on |
| 20 | `scheduled` | Next prompt scheduled | on |
| 21 | `goal-idle` | Goal active, agent idle | on |
| 22 | `shell-busy` | Terminal busy | on |
| 23 | `idle` | Idle | on |
| 24 | `screen-error-example` | Screen mentions an error | off |
<!-- defaults:end -->

## Icons a rule can show

| Kind | What it shows |
| --- | --- |
| Hidden | Nothing; the status slot stays blank |
| Built-in | One of the configurable status icons also used by Normal mode (for example approval, goal paused, task failed) |
| Glyph | One printable character you type |
| Braille animation | A preset (`spinner`, `loop`, `dots`, `snake`, `bounce`, `pulse`, `bar-fill`) at a chosen frame rate |
| Frames | Your own symbol frames, or a ready-made family (quarters, moon phases, clock hours, arrows, hearts, blocks), optionally ping-pong |
| Progress frames | Frames chosen by the task's percentage; a task without a measurable percent shows the first frame, tinted by monitor health |

Colour (`danger`, `warning`, `success`, `info`, `muted` follow the colour scheme, or an explicit RGB value), bold, dim and pulse are set per rule. Pulse alternates the weight twice a second, once a second at Motion `Reduced`, and stays static at `Off`. The Motion setting also slows custom animations to 2 frames per second at `Reduced` and freezes them at `Off`. An idle tree with no matched animated rule schedules no periodic redraws.

## The editor

Three columns: rules, the selected rule's conditions and icon, and a preview.

| Keys | Action |
| --- | --- |
| Up / Down, wheel, click | Select a rule |
| `A` or **+ Add** | New rule from a template (Agent needs approval, Goal paused, Task failed, Blank) |
| `Space` | Enable or disable the rule |
| `D` | Duplicate the rule |
| `N` | Rename the rule |
| `X` or Delete | Delete the rule |
| `Alt+Up` / `Alt+Down`, `Ctrl+Up` / `Ctrl+Down`, `[` / `]` | Move the rule |
| `U` or `Ctrl+Z` | Undo (twenty steps) |
| `R` | Reset to the shipped defaults, after confirmation |
| Enter, `/` or the pencil on an empty condition | Open the condition list, grouped by category with search and a one-sentence explanation |
| `N` in the conditions column | Toggle NOT on the condition |
| `O` | Switch the joining connector between AND and OR |
| `E` | Edit the condition's value |
| `V`, `G`, `C`, `B`, `D`, `P`, `+` / `-` | Icon type, icon choice, colour, bold, dim, ping-pong, animation speed |
| Tab / Shift+Tab, Esc | Next or previous column; Esc steps back one column, Back returns to Settings |

Letter keys work in either case. The **Live tree** preview shows your real agents and a "Matches N panes now" count; **Demo tree** lists synthetic situations that need no running agent. "Why this icon" under the tree names the rule that picks the focused pane's icon. A warning badge on a rule means validation found a problem, for example that an earlier rule always wins first. A refused edit shows its reason in the editor header.

## Where rules are stored

Rules live in the global `[status_rules]` table of `config.toml` (see [Settings](settings.md)). Edits apply at once and are written 300 ms after the last change, immediately when the editor closes and when you quit. A failed write keeps your edits in memory, shows the error in the editor header and retries every 5 seconds. Without a configuration folder the edits apply for the session only, and the editor says they are not saved.

Each client reads the rules once when it starts, so two clients attached to the same session do not share edits until one restarts.

## Troubleshooting

| Symptom | Likely cause and fix |
| --- | --- |
| The status slot is blank | No enabled rule matched and the Otherwise row is empty. Open the hover text for the pane; it says whether rules depended on an unobserved fact. |
| A rule never wins | A rule above it always matches first (warning badge), or its conditions are unknown for that pane. Move it up or check the condition. |
| A negated condition does not match | The fact is unknown, and NOT unknown is unknown. Add a condition that is known for that pane, such as `pane-kind`. |
| A screen-text rule is greyed out or disabled | More than 16 distinct patterns are enabled, or the pattern is invalid. Disable another pattern or fix the expression. |
| A screen-text rule never matches in a long session | Only the visible screen is searched, not scrollback. Use the first or last lines region. |
| The editor shows an error and does not save | The write to `config.toml` failed. Fix permissions or disk space; the editor retries every 5 seconds and your edits stay active. |
| A hand-edited rule vanished | A rule beyond 64 is kept unchanged under `unloaded_rules` and not loaded; an unreadable entry stays in the file untouched. A file written by a newer format loads with a warning. |
