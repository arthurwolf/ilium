# Attention queue

The attention queue answers four questions across every agent in every project: which agent is asking me something, which one keeps failing, which one waits for another agent's result, and what changed since I last looked. It is a different feature from the "Attention" monitoring mode in [Agent monitoring](agent-monitoring.md).

## Where it appears

- **Right panel.** `Ctrl+B I` toggles a list of queued agents: kind, reason, the exact question, project and age. Click a row to jump to that agent.
- **Next: needs you button.** At the bottom of the left panel, with a count badge. Click it, or press `Ctrl+B i`, to jump to the agent that needs you most.
- **Arrival block.** Beside the last-prompt banner, a short line shows the newest queued agent. It disappears when you type or when the item is stale.

Both keys follow the leader-key table and can be remapped.

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

A press jumps immediately from the in-memory queue, then starts one background refresh (a press during a refresh is coalesced, and refreshes have a minimum interval). New results update the list without moving you. When **Press also sorts** is on (default), a press also runs the normal Sort.

## Settings

All settings save as you change them.

- **Mode**: Off, Instant only, Shared, Turbo.
- **Press also sorts**: default on.
- **Turbo concurrency**: default 16.
- **Item expiry**: seconds before an unrefreshed item is dropped.
- **Model for attention**: empty uses the Inference model; set a model id to override it for this feature only.

## Persistence and privacy

The queue is one global list, tagged by project, saved when it changes and re-validated on start: an item whose agent no longer exists or whose screen evidence differs is dropped. In Shared and Turbo modes the same bounded screen excerpts that Sort already sends are sent to your Inference provider; see [Inference and privacy](inference-and-privacy.md). Off and Instant only send nothing.
