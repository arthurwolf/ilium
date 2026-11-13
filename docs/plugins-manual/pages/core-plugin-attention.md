---
layout: page
title: Attention queue
permalink: /pages/core-plugin-attention/
---

## Attention queue

The attention queue answers four questions across every agent in every project: which agent is asking something, which one keeps failing, which one waits for another agent, and what changed since the user last looked. It is the plugin form of the "Next: needs you" button, the right panel, the overview dialog, the arrival block and the briefing box. The plugin id is `ilium.core.attention` and it runs on both sides: the server side builds and stores the queue, and the client side draws the button, the panel, the overview and the briefing box.

It is a different feature from the attention marks of [Notifications](/pages/core-plugin-notifications/) and from the "Attention" monitoring mode of [Agent detection](/pages/core-plugin-agent-detect/). The plugin reads agent states; it does not classify them.

### What it covers

- Attention queue: the ranked list of agents that need the user, described in the Attention queue guide (`src/docs/attention-queue.md`).
- The "Next: needs you" button with its count badge and the `[+]` overview button, at the bottom of the left panel.
- The right panel (`Ctrl+B I`), the "Needs you" overview dialog (`Ctrl+B O`) and the jump to the next agent (`Ctrl+B i`).
- The arrival block beside the last-prompt banner and the briefing box under it.
- The four modes (Off, Instant only, Shared, Turbo), rotation, snoozing and the Attention tab of Settings.

### What the user sees

<figure>
<pre class="term">╭─ Needs you ────────────────────────────────────╮
│ <span class="t-err">Question (1)</span>                                   │
│ <span class="t-hi">● api-refactor</span>  <span class="t-dim">ilium · Claude · 2m</span>            │
│   <span class="t-warn">"Run the migration now?"</span>                     │
│ <span class="t-warn">Stuck (1)</span>                                      │
│ <span class="t-hi">  docs-sweep</span>   <span class="t-dim">manual · Codex · 11m</span>           │
│   <span class="t-dim">Goal blocked: usage limit reached</span>            │
╰────────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative: the overview in its one-column form, with one header per kind and a dot on an item the user has not looked at.</figcaption>
</figure>

<figure>
<pre class="term"> <span class="t-hi">Next: needs you</span> <span class="t-err">(2)</span>                        <span class="t-dim">[+]</span>
</pre>
<figcaption>Rendered at 48 columns. Illustrative: the button at the bottom of the left panel, with the count badge and the overview button.</figcaption>
</figure>

### Behaviour contract

The exact values a parity test uses are in this table. When it differs from `src/docs/attention-queue.md` at the baseline commit, the baseline wins and this page is a defect.

| Id | Rule | Exact value or text | Baseline source |
|---|---|---|---|
| attention-B01 | Kinds and their rank | Question, then Stuck, then Waiting on, then Changed | `src/docs/attention-queue.md#order` |
| attention-B02 | Order inside a kind | Higher priority first, then the oldest, then the lowest pane id | `src/docs/attention-queue.md#order` |
| attention-B03 | Instant items | Waiting for approval is a Question; a blocked or usage-limited goal is Stuck; waiting for subagents is Waiting on; an unread completion is Changed | `src/docs/attention-queue.md#modes` |
| attention-B04 | Modes | `off`, `instant_only`, `shared` (default), `turbo` | `ilium-client/src/attention/settings.rs` |
| attention-B05 | Turbo concurrency | Default 16, range 1 to 64. HTTP 429 or 503 halves the effective limit; successes grow it back | `src/docs/attention-queue.md#modes` |
| attention-B06 | Item expiry | Default 1,800 seconds, range 60 to 86,400 | `ilium-client/src/attention/settings.rs` |
| attention-B07 | Rotation cooldown | A visited agent returns early after 5 minutes when its screen changed, or at once when its kind becomes more urgent | `src/docs/attention-queue.md#pressing-the-button` |
| attention-B08 | Overview grid | One column below 100 terminal columns, two from 100, three from 160; a compact two-line list below 60 by 16 cells | `src/docs/attention-queue.md#needs-you-overview` |
| attention-B09 | Overview size | About 90 percent of the screen | `src/docs/attention-queue.md#needs-you-overview` |
| attention-B10 | Briefing texts | `status_summary`, `why_here` and `suggested_action`, each at most 300 characters | `src/docs/attention-queue.md#briefing` |
| attention-B11 | Briefing space | The terminal keeps at least three rows; a pane narrower than 24 columns shows no box | `src/docs/attention-queue.md#briefing` |
| attention-B12 | Empty overview | The text "All clear" | `src/docs/attention-queue.md#needs-you-overview` |

### Settings

The plugin contributes the Attention tab. All rows save as the user changes them. The keys live in the `[attention]` table of the config file, and the plugin is the only writer.

| Row | Key | Default | Range or values |
|---|---|---|---|
| Mode | `[attention] mode` | `shared` | `off`, `instant_only`, `shared`, `turbo` |
| Press also sorts | `[attention] press_sorts` | true | true or false |
| Turbo concurrency | `[attention] turbo_concurrency` | 16 | 1 to 64 |
| Item expiry | `[attention] expiry_seconds` | 1800 | 60 to 86400 |
| Large attention buttons | `[attention] large_buttons` | true | true or false. On draws a three-line, centred button and a three-line by five-column overview button |
| Show briefing on arrival | `[attention] show_briefing` | true | true or false. No effect while the mode is `off` |
| Model for attention | `[attention] model_override` | empty | A model id, or empty for the Inference model. It applies to the calls of Turbo mode only |

The snooze glyph is the row "Attention: snooze" of the Icons tab, which [Settings](/pages/core-plugin-settings/) owns; this plugin reads it through `settings.read-kernel:icons`.

### Keys

All three keys are prefix chords and can be remapped in the Keyboard tab of [Keys and help](/pages/core-plugin-keys-and-help/).

| Action | Default chord | Remap name | Command |
|---|---|---|---|
| Jump to the agent that needs the user most | `Ctrl+B i` | `attention_next` | `ilium.core.attention/next` |
| Toggle the right panel | `Ctrl+B I` | `attention_panel` | `ilium.core.attention/panel` |
| Open the overview | `Ctrl+B O` | `attention_overview` | `ilium.core.attention/overview` |

Inside the overview the keys of the dialog apply: arrow keys or `h` `j` `k` `l`, `Tab` and `Shift+Tab` and `1` to `9` move the highlight; `Enter` jumps and closes; `PageUp`, `PageDown`, `Home` and `End` scroll; `S` shows or hides snoozed cards; `Esc` or `q` closes.

### CLI

The plugin declares no CLI subcommand. The control API key `attention.large_buttons` of the baseline maps to the row "Large attention buttons".

### Failure behaviour

- **The model call fails or returns a malformed list.** The instant items stay. In Shared mode a missing, malformed or partial `attention` list is ignored and never fails or retries the sort. In Turbo mode the call is retried with backoff.
- **The plugin is disabled or down.** The button, the panel, the overview, the arrival block and the briefing box are not drawn, and the three chords do nothing. The stored queue is kept and is re-validated when the plugin returns.
- **The stored queue cannot be read.** The plugin starts with an empty queue, logs one warning and rebuilds the queue from the detected agent states.
- **An agent's pane closes.** Its item and its briefing box are removed in the same frame.

### Persistence and privacy

The queue is one global list, tagged by project, with the visited memory and the snoozes. The plugin stores it in its own data folder as `queue.json` and writes it when it changes. In Shared and Turbo modes the same bounded screen excerpts that the Sort call of [Titles and organisation](/pages/core-plugin-titles/) sends go to the Inference provider. Off and Instant only send nothing.

### Manifest

This block is a fragment. It lists the contributions. The whole manifest is in [Complete manifest](#complete-manifest).

```json
{
  "id": "ilium.core.attention",
  "capabilities": {
    "server": ["agent.read", "tree.read", "terminal.read-screen", "inference"],
    "client": ["ui.panels", "ui.chrome", "settings.read-kernel:icons"]
  },
  "contributes": {
    "commands": [
      { "id": "next", "title": "Next: needs you", "category": "Attention" },
      { "id": "panel", "title": "Toggle the attention panel", "category": "Attention" },
      { "id": "overview", "title": "Open the Needs you overview", "category": "Attention" }
    ],
    "keybindings": [
      { "command": "ilium.core.attention/next", "chord": "i", "mode": "prefix" },
      { "command": "ilium.core.attention/panel", "chord": "I", "mode": "prefix" },
      { "command": "ilium.core.attention/overview", "chord": "O", "mode": "prefix" }
    ]
  }
}
```

### Complete manifest

This is the whole manifest of the plugin at `ilium:plugin@0.1.0`. The Manifest section above shows the contributions in detail; here the required keys, the sides, the `capabilities` per side and the `activation` triggers are all present. Activation follows [startup and lazy activation](/pages/reference-kernel-and-boot/#startup-and-lazy-activation-of-the-32-core-plugins). [COR-050] Every `commands` and `keybindings` key in the fragment above appears with the same value in this manifest.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.attention",
  "name": "Attention queue",
  "version": "0.1.0",
  "publisher": "Ilium",
  "description": "The attention queue ranks the agents that need the user and takes the user to the next one.",
  "license": "MIT",
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0"
  },
  "client": {
    "entry": "client.wasm",
    "world": "ilium:client/base@0.1.0"
  },
  "capabilities": {
    "server": [
      "agent.read",
      "tree.read",
      "terminal.read-screen",
      "inference",
      "extensions.provide:ilium.core.attention/item-sources"
    ],
    "client": [
      "ui.panels",
      "ui.chrome",
      "settings.read-kernel:icons",
      "extensions.provide:ilium.core.attention/before-jump"
    ]
  },
  "activation": [
    "startup"
  ],
  "contributes": {
    "commands": [
      { "id": "next", "title": "Next: needs you", "category": "Attention" },
      { "id": "panel", "title": "Toggle the attention panel", "category": "Attention" },
      { "id": "overview", "title": "Open the Needs you overview", "category": "Attention" }
    ],
    "keybindings": [
      { "command": "ilium.core.attention/next", "chord": "i", "mode": "prefix" },
      { "command": "ilium.core.attention/panel", "chord": "I", "mode": "prefix" },
      { "command": "ilium.core.attention/overview", "chord": "O", "mode": "prefix" }
    ],
    "panels": [
      { "id": "queue", "title": "Needs you", "modal": false },
      { "id": "overview", "title": "Needs you overview", "modal": true }
    ],
    "settings": [
      {
        "kind": "tab",
        "id": "attention",
        "title": "Attention",
        "sections": [
          {
            "id": "queue",
            "title": "Attention queue",
            "scope": "client",
            "rows": [
              {
                "type": "choice",
                "key": "mode",
                "title": "Mode",
                "default": "shared",
                "options": [
                  { "value": "off", "title": "Off" },
                  { "value": "instant_only", "title": "Instant only" },
                  { "value": "shared", "title": "Shared" },
                  { "value": "turbo", "title": "Turbo" }
                ]
              },
              { "type": "toggle", "key": "press_sorts", "title": "Press also sorts", "default": true },
              { "type": "number", "key": "turbo_concurrency", "title": "Turbo concurrency", "minimum": 1, "maximum": 64, "step": 1, "default": 16 },
              { "type": "number", "key": "expiry_seconds", "title": "Item expiry", "minimum": 60, "maximum": 86400, "step": 60, "default": 1800 },
              { "type": "toggle", "key": "large_buttons", "title": "Large attention buttons", "default": true },
              { "type": "toggle", "key": "show_briefing", "title": "Show briefing on arrival", "default": true },
              { "type": "text", "key": "model_override", "title": "Model for attention", "default": "" }
            ]
          }
        ]
      }
    ],
    "extension_points": [
      {
        "id": "item-sources",
        "kind": "collect",
        "title": "Add a source of attention items",
        "entry_schema": {
          "type": "object",
          "properties": {
            "id": { "type": "string" },
            "title": { "type": "string" },
            "service": { "type": "string" }
          },
          "required": ["id", "title", "service"]
        },
        "risk": "standard",
        "max_entries": 64,
        "dynamic": false,
        "version": "0.1.0"
      },
      {
        "id": "before-jump",
        "kind": "hook",
        "mode": "veto",
        "title": "Before the queue takes the user to an agent",
        "payload_schema": {
          "type": "object",
          "properties": {
            "pane": { "type": "string" },
            "kind": { "type": "string" },
            "origin": { "type": "string" }
          },
          "required": ["pane", "kind", "origin"]
        },
        "risk": "standard",
        "deadline_ms": 20,
        "max_handlers": 8,
        "version": "0.1.0"
      }
    ]
  },
  "limits_tier": "first-party"
}
```

### Samples

The samples of this page are not written yet. The plugin crate `plugins/core/attention` is a skeleton: it declares both sides and produces no contribution until the feature logic moves there.

### Extension points

Other plugins change what `ilium.core.attention` does through the points below. The plugin owns each point (grant `extensions.provide:ilium.core.attention/<point>`). Contributors need `extensions.contribute:ilium.core.attention/<point>`. See [Extension points](/pages/api-extension-points/) for the mechanism and [Extending core plugins](/pages/extending-core-plugins/) for the five ways to work with a core feature.

| Point | Kind | What it does | Payload | Risk |
|---|---|---|---|---|
| `item-sources` | collect | Add a source of attention items. | entry: id, title, service | standard |
| `before-jump` | hook: veto | Refuse a jump the queue is about to make. | payload: pane, kind, origin | standard |

**Services** (grant `services.use:<name>`):

- `ilium.core.attention/queue`: list, next: read the queue and the agent the next press would visit

**Topics** (list under `contributes.topics.subscribes`):

- `ilium.core.attention/changed`: { count, top }

**Replaceable:** yes. A replacement lists `"replaces": ["ilium.core.attention"]` and provides the whole surface (see [Extending core plugins](/pages/extending-core-plugins/#replacing-a-core-plugin)).

### Extension points this plugin owns

The values below are the ones the core extension points table uses for these two points. The item sources are collected on the server side, where the queue is built; the jump hook runs on the client side, where the jump happens.

| Point | Side | Version | Deadline | Max | Dynamic |
|---|---|---|---|---|---|
| `ilium.core.attention/item-sources` | server | 0.1.0 | none (default) | 64 entries | no |
| `ilium.core.attention/before-jump` | client | 0.1.0 | 20 ms | 8 handlers | no |

### ABI used

- [Events](/pages/api-events/)
- [agent.read](/pages/capability-agent-read/), [tree.read](/pages/capability-tree-read/) and [terminal.read-screen](/pages/capability-terminal-read-screen/)
- [Inference](/pages/api-inference/) and the [inference capability](/pages/capability-inference/)
- [ui.panels](/pages/capability-ui-panels/) and [ui.chrome](/pages/capability-ui-chrome/)
- [Settings](/pages/api-settings/) and [settings.read-kernel](/pages/capability-settings-read-kernel/)
- [Extension points](/pages/api-extension-points/)

### See also

- [Core replacement matrix](/pages/reference-core-replacement-matrix/) for the surface, stored data, CLI names and install codes that a replacement of this plugin must keep.
- [Core plugins](/pages/core-plugins/)
- [Notifications](/pages/core-plugin-notifications/), which owns the attention marks in the tree
- [Titles and organisation](/pages/core-plugin-titles/), whose Sort call carries the `attention` list in Shared mode

## Requirement statements

- **[PRM-401]** The system must rank the attention queue by kind in the order Question, Stuck, Waiting on, Changed, and inside a kind by higher priority first, then the oldest item, then the lowest pane id.
- **[PRM-402]** The system must create an instant item from detected agent state alone: waiting for approval as Question, a blocked or usage-limited goal as Stuck, waiting for subagents as Waiting on and an unread completion as Changed. A model reply must only add a reason, the exact question, a waiting-on target and a priority, and must never remove an instant item.
- **[PRM-403]** The system must make no model call in the modes `off` and `instant_only`, no call beyond the existing Sort call in the mode `shared`, and one call per changed agent in the mode `turbo` with at most `turbo_concurrency` calls in flight. In the mode `off` it must draw no button, panel, overview, arrival block or briefing box.
- **[PRM-404]** In the mode `shared` the system must ignore a missing, malformed or partial `attention` list in the Sort reply, and must never fail or retry the sort because of it.
- **[PRM-405]** In the mode `turbo` the system must halve the effective concurrency limit on HTTP 429 or 503, grow it back after successes and retry the call with backoff, and in every mode it must skip an agent whose screen evidence has not changed since its last analysis.
- **[PRM-406]** The system must bind `Ctrl+B i` to the jump to the agent that needs the user most, `Ctrl+B I` to the toggle of the right panel and `Ctrl+B O` to the overview, and must let the user remap them under the names `attention_next`, `attention_panel` and `attention_overview`.
- **[PRM-407]** A press of the Next: needs you button or its chord must jump at once from the queue held in memory, then start at most one background refresh, with a press during a refresh coalesced into it, and must also run the normal Sort when `press_sorts` is on. New results must update the list without moving the user.
- **[PRM-408]** The system must remember visited agents, move a visited agent to the back of the panel and the overview, and return it early only when its kind becomes more urgent or after 5 minutes when its screen changed. When every agent has been visited, a press must go to the agent seen longest ago. The memory must be saved with the queue.
- **[PRM-409]** The overview must show one card per queued agent under one header per kind with a count, in one column below 100 terminal columns, two from 100 and three from 160, as a compact two-line list on a terminal smaller than 60 by 16 cells, and with the text All clear when the queue is empty. `Enter` or a click on a card must focus that agent, mark it as looked at and close the dialog, and `Esc` or `q` must close it without a jump.
- **[PRM-410]** On arrival at an agent through the button, a chord, a panel row or the overview, the system must show the briefing box under the last-prompt banner while `show_briefing` is on and the mode is not `off`. The box must leave out a row with nothing to say, cut each model text at 300 characters, keep at least three terminal rows, show one compact line in a small pane and nothing in a pane narrower than 24 columns, and end when the item leaves the queue or the pane closes.
- **[PRM-411]** A snoozed agent must be left out of the overview, the panel and the Next: needs you button, must stay snoozed when its text or screen evidence changes inside the same kind, and must return when its kind changes or it stops needing attention. `S` in the overview must show and hide snoozed cards, and snoozes must be saved with the queue.
- **[PRM-412]** The system must store the queue as one global list tagged by project, write it when it changes, and on start drop every item whose agent no longer exists or whose screen evidence differs. It must clamp `turbo_concurrency` to 1 to 64 and `expiry_seconds` to 60 to 86400, and drop an item that was not refreshed within `expiry_seconds`.
- **[PRM-413]** While `ilium.core.attention` is disabled or down, the system must draw none of its surfaces, the three chords must do nothing, the stored queue must be kept unmodified, and the attention marks of the notifications plugin must keep working.
