# Agent cost

Ilium can show how much each agent is costing you, right in the project tree. Indicators range from a quiet coloured glyph to a five-cell meter, a burn sparkline, group totals and a detail card, and the tree can be sorted by spend. This page explains what is measured, the two units (estimated API dollars or plan quota), how "expensive" is decided through five calibrations, every display option, the budget and history settings, how to add model prices, and the caveats you should know before trusting a number. Everything is computed on your machine from the agents' own transcripts.

Contents:

- [What the numbers mean](#what-the-numbers-mean)
- [Quick start](#quick-start)
- [Choose a unit: dollars or plan quota](#choose-a-unit-dollars-or-plan-quota)
- [Choose how expensive is decided](#choose-how-expensive-is-decided)
- [Choose what to display](#choose-what-to-display)
- [Sorting the tree by cost](#sorting-the-tree-by-cost)
- [Prices and `[cost.prices]`](#prices-and-costprices)
- [Configuration reference](#configuration-reference)
- [Caveats](#caveats)
- [Troubleshooting](#troubleshooting)

Related pages: [Agent monitoring](agent-monitoring.md) (the stats popover), [Settings](settings.md), [Notifications](notifications.md).

## What the numbers mean

- Costs are **estimates**. For Claude Code and Codex, Ilium reads the agent's transcript (including sub-agent transcripts), counts input, output, cache-read and cache-write tokens per model, and multiplies by API list prices per million tokens.
- Claude Code also records its own running total in the transcript. When that figure is larger than Ilium's estimate it wins.
- Subscription plans are **not** billed these amounts. Treat the figures as relative weight, not an invoice.
- Figures can lag live use because transcripts are parsed on a low-priority background worker.
- Everything stays local. Cost tracking never touches the server wire or the network.
- Models with no known price are reported as unpriced rather than guessed. The figure then shows a leading `~` meaning "at least this much" (a lower bound).
- Advisor-model calls made inside Claude Code appear in no transcript, so they are only covered by Claude Code's own recorded total.

## Quick start

1. Open **Settings** (`Ctrl+B :`) and choose the **Agent Cost** tab.
2. Leave the unit on **API dollars** and the calibration on **Relative to your history** (the defaults).
3. The **Five-cell meter** is already enabled and appears when you hover an agent row. Turn on more options, and switch an option to **Always visible** if you want it on every row.
4. Hover an agent to see its meter. Enable **Detail card** for the full breakdown.

All settings auto-save and apply to a running client immediately.

## Choose a unit: dollars or plan quota

| Metric | `metric` value | What it measures | Works for |
| --- | --- | --- | --- |
| API dollars (default) | `dollars` | Estimated cost at API list prices | Claude Code and Codex |
| Plan quota | `quota` | Percentage points of the Codex rate-limit window used up while the agent ran | Codex only |

Everything else (thresholds, budget, burn rate, sparkline, history) follows the chosen unit.

Plan quota notes:

- Only Codex transcripts record quota, and what is recorded is the **whole account's** use, so agents running at the same time are counted together.
- Claude Code agents show no indicator under this metric. Their detail card explains why and still shows the dollar estimate.
- **Quota window** (`quota_window`) picks which Codex rate-limit window to follow: `primary` (the shorter, rolling window; default) or `secondary` (the longer, weekly window). Changing it recomputes every agent and re-reads history for the other window. Sessions that never reported that window show nothing.
- Quota cut points, budget and burn-rate bands are stored separately (`quota_fixed_cuts`, `quota_burn_cuts`, `quota_budget_percent`), so switching unit never overwrites your dollar settings.

## Choose how expensive is decided

Ilium turns an amount into one of **five levels** (shown as glyph height and colour from green to red, and as how many meter cells are filled). The calibration decides where the level boundaries lie. Pick one under **Agent Cost**.

| Calibration | `calibration` value | Rates each agent against | Good for | Limitation |
| --- | --- | --- | --- | --- |
| Fixed bands | `fixed_bands` | The same fixed thresholds for everyone | Predictable, absolute reading | Arbitrary: choose the preset that matches your usual spend |
| Relative to open agents | `peer_relative` | The median of the agents open right now | Spotting the outlier | Says nothing about absolute size; agents that spent nothing are ignored |
| Relative to your history | `own_history` (default) | Percentiles of your own past sessions | A scale that fits how you work | Needs enough history (see below) |
| Budget | `budget` | A per-agent budget | Hard spending limits | Per agent session, not per day or project |
| Burn rate | `burn_rate` | Current spend per hour | Catching a runaway agent | The amount shown beside the level is still the running total |

Parameter rows appear under the selected calibration only.

### Fixed bands

Pick a preset of four ascending cut points. Dollar presets (default is the third):

| Preset | Level steps up at |
| --- | --- |
| 1 | $0.25, $1, $4, $10 |
| 2 | $0.50, $2, $8, $25 |
| 3 (default) | $1, $5, $20, $50 |
| 4 | $2, $10, $40, $100 |
| 5 | $5, $25, $100, $250 |

Quota presets (percentage points of the plan window; default is the second): 0.5/2/5/12, 1/3/8/20, 2/6/15/35, 5/12/30/60. For other values set `fixed_cuts` or `quota_fixed_cuts` by hand in `config.toml` (four ascending positive numbers). Fixed bands are also the fallback while history or peers are not available.

### Relative to your history

Ilium reads a bounded tail of each past session transcript: Claude Code's own recorded total, or Codex's cumulative token count priced at list prices. Sessions without a recorded total are skipped. History shorter than 8 sessions is too thin to calibrate against, in which case fixed bands apply.

**History window** (`history_days`) chooses how many days of past sessions are read: 7, 14, 30, 60, 90 (default), 180, 365 or 730 in the settings UI; the file accepts 1 to 3650. Longer windows read more files on the first scan. Results are cached by file size and modification time and scanned on one lowest-priority worker.

### Budget

Indicators fill toward a budget for each agent session, and a red `!` appears once an agent passes it. Default is $10 (`budget_usd`), or 10 percent of the plan window under the quota metric (`quota_budget_percent`).

- Dollar steps in the UI: $1, $2, $5, $10, $20, $50, $100, $200, $500, $1000. Any other amount can be set as `budget_usd`.
- Quota steps: 1, 2, 5, 10, 20, 30, 50, 75, 100 percent.

### Burn rate

Rates how fast an agent is spending right now. Presets are four ascending cut points in dollars per hour (default is the second): 0.25/1/3/8, 0.5/2/5/15, 1/4/10/30, 2/8/20/60. Quota presets are in percentage points per hour: 0.5/2/5/10, 1/4/10/20, 2/8/20/40. Custom values: `burn_cuts`, `quota_burn_cuts`.

## Choose what to display

Each option below can be switched on or off independently, and has a visibility of **Only when hovering the entry** (the default) or **Always visible**. If an option is enabled, it is drawn just left of the hover action buttons on the row.

| Option | Config table | Shows | Default |
| --- | --- | --- | --- |
| Level glyph | `level_glyph` | One coloured height glyph per agent, green (cheap) to red (expensive); quietest, no numbers | Off |
| Level glyph and amount | `level_dollars` | The glyph plus the exact amount: estimated dollars, or percent of plan quota | Off |
| Five-cell meter | `meter` | Five cells that fill as the agent gets more expensive; while a transcript loads it shows an empty track | On, hover |
| Burn sparkline | `sparkline` | Spend over a window as one bar per time slice; idle stretches show the lowest bar | Off |
| Group and project totals | `group_totals` | Project and group rows show the sum beneath them; a group with an agent still loading shows a dot | Off |
| Detail card | `detail_card` | A card beside the tree with the full breakdown: amount, what it was rated against, burn rate, token classes, cost per model, quota | Off |
| Total line above the tree | `header_total` | The total estimated spend of every agent in the tree title; `~` marks a lower bound | Off |
| Burn spike marker | `burn_marker` | An up arrow beside the indicator while the agent spends at least twice as fast as lately | Off |

Notes:

- The "glyph and amount" option already contains the glyph, so enabling both shows it once.
- A group is rated on the same scale as a single agent, so group totals tend to look high.
- The detail card covers part of the pane area while shown. A hover card dismisses when the pointer leaves the row. Hover shows it for the hovered agent; Always keeps it on the selected one. Groups and non-agent rows have no card.
- The spike marker needs another indicator visible on the row and ignores spending below $1 per hour.
- Total line counts agents whose transcripts could be read.

### Sparkline window and width

- **Sparkline window** (`sparkline_window_minutes`): 5, 10, 15, 30, 45, 60, 90, 120, 180, 240, 360 (default, six hours), 480, 720, 1080, 1440, 2880, 4320, 10080, 20160 or 43200 minutes in the UI. Any value from 1 to 525600 can be set in the file. Each cell covers window divided by width.
- **Sparkline width** (`sparkline_cells`): 4 to 24 slices, default 8. Wider sparklines need a wider sidebar. When space runs out, bars are dropped from the left first.

## Sorting the tree by cost

**Sort tree by cost** (`sort_by_cost`, default off) orders every group and project by descending spend. It overrides the order chosen in Settings -> User Interface while on. Your manual order is untouched and returns when you switch it off. Turning it on starts cost tracking even if no indicator is enabled.

## Prices and `[cost.prices]`

Ilium contains a built-in table of list prices per million tokens (input, output, cache read, cache write). Models are matched by the **longest model-name prefix**, so dated snapshots such as `claude-haiku-4-5-20251001` match their family. Built-in families cover the current Claude Fable, Opus, Sonnet and Haiku lines and the GPT-5.6 Sol, Terra and Luna lines. Anthropic rows follow Anthropic's published table (cache writes assume the 1.25x five-minute rate); OpenAI rows are third-party reports of published prices with a 0.1x cache read and no cache-write charge.

To add or override a model, add a table under `[cost.prices]` keyed by the model-name prefix:

```toml
[cost.prices."my-new-model"]
input = 3.0        # dollars per million input tokens
output = 15.0
cache_read = 0.30
cache_write = 3.75  # optional, defaults to 0
```

Rules:

- Keys are lowercased; matching is by prefix. On equal prefix length your override wins over a built-in.
- All four values must be finite and not negative. An invalid entry is dropped on load.
- Reasoning tokens are already counted inside `output`.
- When a transcript contains a model with no price, its figure is flagged with `~` until you add a price.

## Configuration reference

```toml
[cost]
metric = "dollars"            # "dollars" or "quota"
quota_window = "primary"      # "primary" or "secondary"
calibration = "own_history"   # fixed_bands | peer_relative | own_history | budget | burn_rate
fixed_cuts = [1.0, 5.0, 20.0, 50.0]
burn_cuts = [0.5, 2.0, 5.0, 15.0]
budget_usd = 10.0
quota_fixed_cuts = [1.0, 3.0, 8.0, 20.0]
quota_burn_cuts = [1.0, 4.0, 10.0, 20.0]
quota_budget_percent = 10.0
history_days = 90
sparkline_window_minutes = 360
sparkline_cells = 8
sort_by_cost = false

[cost.meter]
enabled = true
visibility = "hover"          # "hover" or "always"
```

The other display tables (`level_glyph`, `level_dollars`, `sparkline`, `group_totals`, `detail_card`, `header_total`, `burn_marker`) take the same two keys. Out-of-range hand edits are repaired on load: cut points must be ascending and positive, the dollar budget at least 0.01, the quota budget at least 0.1, history 1 to 3650 days, sparkline width 4 to 24.

## Caveats

- **Not a bill.** List-price estimates; subscriptions are not billed this amount.
- **Transcript lag.** Numbers update when the transcript snapshot changes, not instantly.
- **Unknown models.** Priced as unknown, shown with `~`, until you add `[cost.prices]`.
- **Sub-agents.** Claude Code writes sub-agent and workflow calls to separate files; Ilium folds them in, de-duplicated by message ID. Advisor-model calls are only covered by Claude Code's own recorded total.
- **Quota is account-wide.** Concurrent Codex agents share one reading; Claude Code has none.
- **Budget is per agent session.** Not per day, not per project.
- **First scan.** A long history window reads many files once, at lowest priority.
- **Providers without transcripts.** Antigravity and custom signatures have no cost data.

## Troubleshooting

| Symptom | Explanation |
| --- | --- |
| Meter stays an empty track | The transcript is still loading or has no data yet |
| Every agent looks cheap or every agent looks expensive | Relative calibrations hide absolute size. Switch to Fixed bands or Budget |
| History calibration seems ignored | Fewer than 8 usable past sessions: fixed bands are used until there is enough history |
| `~` before a figure | A model without a known price. Add it under `[cost.prices]` |
| Nothing under the quota metric for Claude Code | Claude Code transcripts carry no quota; use the dollar metric |
| Quota shows nothing for a session | That session never reported the chosen window; try the other window |
