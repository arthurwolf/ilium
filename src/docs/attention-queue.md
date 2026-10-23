# Attention queue

The attention queue answers four questions across every agent in every project: which agent is asking me something, which one keeps failing, which one waits for another agent's result, and what changed since I last looked. It is a different feature from the "Attention" monitoring mode in [Agent monitoring](agent-monitoring.md).

## Where it appears

- **Right panel.** `Ctrl+B I` toggles a list of queued agents: kind, reason, the exact question, project and age. Click a row to jump to that agent.
- **Next: needs you button.** At the bottom of the left panel, with a count badge. Click it, or press `Ctrl+B i`, to jump to the agent that needs you most. The `[+]` at its right end opens the overview below.
- **Needs you overview.** Click the `[+]`, or press `Ctrl+B O`, to open a large dialog that lists everything that needs you.
- **Arrival block.** Beside the last-prompt banner, a short line shows the agent the next press would visit (an unseen or newly arrived one first). It disappears when you type or when the item is stale.
- **Briefing box.** When you arrive at an agent through the button, the shortcut, a panel row or the overview, a box under the last-prompt banner says why you were brought there. See [Briefing](#briefing).

All three keys follow the leader-key table and can be remapped (`attention_next`, `attention_panel`, `attention_overview`).

## Needs you overview

The overview is a dialog that covers about 90% of the screen. It shows every queued agent as a card, grouped under one header per kind (Question, Stuck, Waiting, Changed) with a count. A card shows:

- the kind badge, the agent's title and how long ago it was queued (a dot marks items you have not looked at yet);
- the project, the agent class, and whether a built-in rule or the model found it;
- the reason, and the exact question when there is one, quoted and highlighted;
- who it waits on, when it waits on another agent;
- a priority gauge and a **Jump** hint.

The grid has one column below 100 terminal columns, two from 100 and three from 160. The list scrolls with the wheel, PageUp and PageDown, and a scrollbar you can drag. On a terminal smaller than 60 by 16 cells it becomes a compact two-line list. When nothing needs you it shows an "All clear" message, and it fills in by itself if something arrives while it is open. Ages update every second, and the dialog never takes keyboard focus from you beyond its own keys.

| Action | Keys or pointer |
| --- | --- |
| Jump to an agent and close | Click its card, or `Enter` on the highlighted card |
| Move the highlight | Arrow keys or `h` `j` `k` `l`, `Tab` and `Shift+Tab`, `1` to `9` |
| Scroll | Wheel, `PageUp`, `PageDown`, `Home`, `End` |
| Close | `Esc` or `q`, the `[x]` at the top right, or a click outside the dialog |

Jumping from the overview behaves exactly like a click on a row of the right panel: the agent is focused and marked as looked at.

## Order

Kinds are ranked Question, then Stuck, then Waiting on, then Changed. Inside a kind, higher priority first, then the oldest, then the lowest pane id.

## Modes

Choose one in **Settings → Attention**.

| Mode | What it does | LLM calls |
| --- | --- | --- |
| Off | No queue, button, panel or banner | None |
| Instant only | Items come only from detected agent state: waiting for approval (Question), a blocked or usage-limited goal (Stuck), waiting for subagents (Waiting on), unread completion (Changed) | None |
| Shared (default) | Instant items plus an optional `attention` list that the existing Sort call returns together with its tree plan | None extra |
| Turbo | Instant items plus one separate call per changed agent, run in parallel | One per changed agent |

The instant layer is always the floor: the model can add a reason, the exact question, a waiting-on target and a priority, but it never removes an instant item. In Shared mode a missing, malformed or partial `attention` list is ignored and never fails or retries the sort.

Turbo mode has its own concurrency limit (default 16, range 1 to 64). On HTTP 429 or 503 it halves the effective limit and grows it back after successes, and it retries with backoff. Agents whose screen evidence has not changed since their last analysis are always skipped, in every mode.

## Pressing the button

**Rotation.** The queue remembers which agents you visited. A visited agent moves to the back of the panel and the overview, and returns early only when it becomes more urgent (for example, from changed to asking a question) or after a 5 minute cooldown when its screen changed. When every agent has been visited, a press goes to the one you saw longest ago, so a long queue is walked end to end before any agent repeats. The memory is saved with the queue.

A press jumps immediately from the in-memory queue, then starts one background refresh (a press during a refresh is coalesced, and refreshes have a minimum interval). New results update the list without moving you. When **Press also sorts** is on (default), a press also runs the normal Sort.

## Briefing

Arriving at an agent from the queue puts a box at the top of that agent's pane, just below the last-prompt banner. It answers: why am I here, what is this agent doing, and what should I do.

- **Header.** The kind of item (Question, Stuck, Waiting, Changed), the words "why you're here", where the text came from (`AI` when the model wrote it, `automatic` when it comes from detection alone) and how long the item has been queued.
- **Rows.** *Why* (why it needs you), *Status* (what the agent is doing or has done, one or two sentences), *Asked* (the exact question, quoted), *Waiting* (the agent it waits on) and *Next* (a suggested next step). A row with nothing to say is left out; nothing is invented. Long text wraps and is cut with an ellipsis.
- **Buttons.** `[▾]` folds the box to a single line and unfolds it again; clicking the header text does the same. `[x]` hides the box for that item until a newer item arrives for the agent. Both are per pane and last until the client is closed.
- **Lifetime.** The box stays while the item is in the queue and updates when the model's reply arrives. A finished agent stops being queued once you have read it, so its box stays until the agent starts something new (or the item expiry passes). It also ends when the agent's pane closes or the setting is switched off.
- **Space.** The terminal below shrinks by the rows the box takes, exactly as for the banner, and always keeps at least three rows. A small pane shows one compact line; a pane narrower than 24 columns shows none.
- **Instant only.** Without a model call the box carries the automatic wording only, labelled `automatic`.

In Shared and Turbo mode the model is asked for the three texts (`status_summary`, `why_here`, `suggested_action`, each at most 300 characters) in the same call that finds the item. A missing or malformed field is ignored and the automatic wording stays.

## Settings
## Settings

All settings save as you change them.

- **Mode**: Off, Instant only, Shared, Turbo.
- **Press also sorts**: default on.
- **Turbo concurrency**: default 16.
- **Item expiry**: seconds before an unrefreshed item is dropped.
- **Large attention buttons**: default off (`attention.large_buttons` in the control API). Turn it on for a three-line, centered **Next: needs you** button and a three-line by five-column overview **+** button.
- **Show briefing on arrival**: default on. Turn it off to arrive at an agent with no box. It has no effect while the mode is Off.
- **Model for attention**: empty uses the Inference model; set a model id to override it for this feature only. The override applies to Turbo mode's own calls; Shared mode rides on the Sort call and uses the Sort model.

## Persistence and privacy

The queue is one global list, tagged by project, saved when it changes and re-validated on start: an item whose agent no longer exists or whose screen evidence differs is dropped. In Shared and Turbo modes the same bounded screen excerpts that Sort already sends are sent to your Inference provider; see [Inference and privacy](inference-and-privacy.md). Off and Instant only send nothing.

### Snoozing attention cards

Click the 💤 icon on an attention card to snooze that agent. Hover over the icon for an explanation. Snoozed agents are excluded from the overview, the attention panel and the next-agent button. The snooze survives updated text or screen evidence within the same attention category. It ends when the category changes or the agent stops needing attention.

Press **S**, or click **S show snoozed** in the overview footer, to reveal snoozed cards; use the same control again to hide them. Revealed cards are marked **snoozed**. Change the glyph under **Settings → Icons → Attention: snooze**. Snoozes are saved with the attention queue.
