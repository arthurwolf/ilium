---
layout: page
title: Terminal primitive
permalink: /pages/primitives-terminal/
---

## Terminal primitive

A terminal pane shows the output of a program in a pseudo-terminal, usually a shell or an agent CLI. The terminal primitive lets a server-side plugin read the screen and selection, watch output for text, and type input. Drawing highlights and claiming the mouse for one pane are client-side only (see [terminal pointer](/pages/primitives-terminal-pointer/)). The kernel parses every output byte. The plugin receives only matches of its output filters, and it reads the screen only when it asks.

### What you need

Declare the grants you use in `capabilities.server` of the manifest. Each grant unlocks a group of calls:

| Grant | Unlocks |
|---|---|
| `terminal.read-screen` | `read_screen`, `selection` |
| `terminal.output-filter` | `add_output_filter`, `remove_output_filter`, `output-matched` events, and manifest `output_filters` |
| `terminal.pointer` | `set_highlights`, `clear_highlights`, and `claim_pointer`, `release_pointer` and `pointer` events while a claim is held (client only) |
| `terminal.write` | the `send-input` intent and `prompt_epoch` |

The primitive is available to both the server and the client sides, through the `ilium:server/base` and `ilium:client/base` worlds. Import it from `ilium_plugin_sdk::primitives::terminal` in Rust, or from `@ilium/plugin-sdk/primitives/terminal` in TypeScript. Agent identification is a separate contract, described in [primitives-agent](/pages/primitives-agent/).

```json
{
  "capabilities": {
    "server": ["terminal.read-screen", "terminal.output-filter", "notify.desktop"],
    "client": ["terminal.pointer"]
  },
  "output_filters": [{
    "id": "rate-limit", "regex": "rate limit exceeded", "panes": "agents", "rate_per_second": 1
  }],
  "link_detectors": [{ "id": "issue-ref", "regex": "#[0-9]+", "command": "example.issues/open" }],
  "context_menu": [{
    "id": "copy-block", "title": "Copy block", "command": "example.copy/block", "target": "terminal"
  }]
}
```

### Functions

Every function is synchronous. The Rust name is shown first, then the TypeScript name.

| Function | Grant | Result |
|---|---|---|
| `read_screen(pane, part, max_rows)` / `readScreen` | `terminal.read-screen` | `Option<Screen>`, `none` for an unknown pane |
| `selection(pane)` / `selection` | `terminal.read-screen` | `Option<Selection>` |
| `add_output_filter(pane, filter)` / `addOutputFilter` | `terminal.output-filter` | `Result<(), PluginError>` |
| `remove_output_filter(pane, filter_id)` / `removeOutputFilter` | `terminal.output-filter` | nothing |
| `claim_pointer(pane)` / `claimPointer` | `terminal.pointer` | `Result<(), PluginError>` |
| `release_pointer(pane)` / `releasePointer` | `terminal.pointer` | nothing |
| `set_highlights(pane, layer, regions)` / `setHighlights` | `terminal.pointer` | `Result<(), PluginError>` |
| `clear_highlights(pane, layer)` / `clearHighlights` | `terminal.pointer` | nothing |
| `prompt_epoch(pane)` / `promptEpoch` | `terminal.write` | `Option<u64>` |

In TypeScript, a `result` that fails throws a `PluginError`. The functions that return nothing never fail.

### Types

| Type | Fields | Meaning |
|---|---|---|
| `Screen` | `columns`, `rows`, `cursor_row`, `cursor_column`, `title`, `revision` | Text of one part of the pane, with the cursor. |
| `ScreenPart` | `Visible`, `Scrollback` | Which part `read_screen` returns. |
| `Selection` | `first_row`, `first_column`, `last_row`, `last_column`, `text` | The current mouse selection. |
| `OutputFilter` | `id`, `regex`, `rate_per_second` (`u16`) | A runtime rule for one pane. |
| `CellRegion` | `first_row`, `first_column`, `last_row`, `last_column`, `style`, `label` | One highlighted rectangle of cells. |
| `OutputMatchedEvent` | `node`, `filter_id`, `line`, `row` (`u32`) | A completed line matched a filter. |
| `LinkActivatedEvent` | `node`, `detector_id`, `text` | The user activated a link match. |
| `PointerEvent` | `node`, `row`, `column`, `kind`, `button` | Mouse input while a claim is held. |

`kind` is `down`, `up`, `move`, `drag`, `wheel-up`, `wheel-down`, `wheel-left` or `wheel-right` (the `pointer-kind` enum of the WIT). `button` is `left`, `middle`, `right`, or `none` for a wheel or move event. `row` and `column` count from the top visible row and the left column of the pane. The scrollback offset is not applied.

### Reading the screen

`read_screen` with `Visible` returns the bottom `max_rows` rows of the visible area, top to bottom. `cursor_row` counts from the first returned row. A cursor above the returned rows gives the number of returned rows and column 0. With `Scrollback`, it returns the rows directly above the visible area, oldest first. Each row holds the characters of its cells, with escape sequences and trailing blanks removed. A wide character appears once.

`max_rows` of 0 returns an empty list. A value above 10,000 is clamped to 10,000 without an error. `revision` is per pane. It starts at 0 when the host starts the pane, rises when either part changes, and is not saved in the snapshot. Compare it with the last revision you saw to skip unchanged work.

`selection` counts coordinates from the top of the visible area. Its text covers only the visible part, with lines joined by a newline. A selection that lies wholly in the scrollback returns `none`.

### Output filters

A filter is a regular expression in the dialect of the Rust `regex` crate. It has no look-around and no back-references. Each completed line is matched after escape sequences are removed. A pattern is at most 512 bytes, and the compiled program at most 1 MiB. `rate_per_second` is 1 to 100.

Manifest entries need `panes`: `all` (every terminal pane), `agents` (panes whose agent is identified) or `terminals` (terminal panes with no identified agent). A missing or unknown value makes the package fail to install. A runtime filter applies to the one pane named in the call, and an existing id on that pane is replaced. The host removes runtime filters when the plugin deactivates or the pane closes, without an event.

The `row` of a match is the zero-based number of the line since the pane started printing. It does not change when the pane scrolls. A match above the rate is dropped and counted in `events-dropped`, with the filter id as `source`.

### Highlights and the pointer claim

A highlight layer has a name of 1 to 32 characters from `[a-z0-9-]`. Layer names are scoped to the calling plugin, so two plugins can both use `search`. Regions of one layer are painted in list order. Where two regions overlap, the later region sets the foreground and background, and the modifiers of all overlapping regions are combined. A region outside the screen is clipped without an error. A region whose first position is after its last is `invalid-argument`.

A `label` is not drawn in the pane. It appears as a tooltip after the pointer rests on the region for 500 ms. A label is at most 128 characters; a longer label is refused with `invalid-argument`. Highlights survive `release_pointer`. They are withdrawn when the plugin is disabled, its grant is revoked, or the pane is removed.

A claim gives the plugin the pointer of one pane. While it holds the claim, the host stops forwarding mouse events to the program in the pane, and its own selection and wheel scrollback are off there. Wheel events reach the plugin as `pointer` events with kind `wheel-up`, `wheel-down`, `wheel-left` or `wheel-right`. Typing is not affected, because typing needs `terminal.write`, not a claim. The plugin that already holds a claim can call `claim_pointer` again and gets `ok`. `release_pointer` without a claim does nothing.

### Input: the send-input intent

Input is an intent, not a function. The plugin submits `send-input` with these fields:

| Field | Rule |
|---|---|
| `pane` | A terminal pane. |
| `text` | At most 64 KiB. |
| `delivery` | `typed` writes keystrokes. `submitted` writes the text, then the submit keys of the pane's signature (`"\r"` by default). `pasted` uses bracketed paste when the program enabled it (DEC mode 2004), and otherwise works like `typed`. |
| `prompt_epoch` | Optional. Refuses with `conflict` when the pane's epoch has moved on. |
| `dedup_key` | Mandatory, from 1 to 128 characters of `[A-Za-z0-9._:/-]`. The dedup journal keeps the key for 24 hours or 4,096 entries per session, whichever is smaller, once the submission is accepted. |

The rate is 20 submissions per second per pane per plugin. The checks run in this order: shape and key, grant, duplicate key, pane exists and is a terminal, pane restored, rate, prompt epoch, `if-current`, the input-lock check, then the write. A refusal does not reserve the key, so a retry with the same key is valid after the cause is fixed.

The pane's `prompt_epoch` is a counter that the kernel raises each time it records a submitted prompt. Read it when the agent is idle, and submit with it. If the agent has moved to a newer prompt in the meantime, the input is refused, and nothing is typed into the wrong prompt.

### Reads, pointer and input

The terminal primitive has three further areas. Each has a page of its own.

| Need | Calls and events | Page |
|---|---|---|
| Styled cells, snapshot, history, modes, foreground process | `read-cells`, `freeze-snapshot`, `history-text`, `search-history`, `modes`, `foreground-process`, `screen-changed`, `user-line-submitted` | [Terminal reads](/pages/primitives-terminal-reads/) |
| Sub-cell pointer, move, modifier gestures | `claim-pointer-gesture`, extended `pointer` event | [Terminal pointer](/pages/primitives-terminal-pointer/) |
| Staged, raw, mouse and conditional input, lock, journal | extended `send-input`, `lock-input`, `unlock-input` | [Terminal input](/pages/primitives-terminal-input/) |

The grants `terminal.read-cells`, `terminal.history`, `terminal.raw-input` and `terminal.input-lock` are listed in [Capabilities](/pages/reference-capabilities/). The `pointer` event gains `sub-x`, `sub-y`, `modifiers` and the kind `move`; the single `wheel` kind of the earlier surface is replaced by `wheel-up`, `wheel-down`, `wheel-left` and `wheel-right` in `view-pointer`. The new fields are optional.

### Manifest contributions

| Key | Shape | Limit |
|---|---|---|
| `output_filters` | `{ id, regex, panes, rate_per_second }` | 32 per plugin, 128 for bundled plugins |
| `link_detectors` | `{ id, regex, command }` | with the same regex limits |
| `context_menu` | `{ id, title, command, target }` | `target` is `terminal` for this primitive |

A link detector sends `link-activated` to its plugin when the user activates a match. The command named in `command` runs with the detector id and the matched text as its arguments. Output filters and link detectors are installed by the kernel and run natively on the output.

### Examples

#### Example 1: a rate-limit filter with a notice

The manifest above installs the `rate-limit` filter on agent panes, at most once per second. When it matches, the plugin sends a desktop notice. Highlights are client-only (`terminal.pointer`), so this server example does not highlight the row.

**Grants:** `terminal.output-filter`, `terminal.read-screen`, `notify.desktop`

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::OutputMatchedEvent;
use ilium_plugin_sdk::types::intents::{NotifySpec, Urgency};

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct RateLimitWatch;

impl Handler for RateLimitWatch {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            let Event::OutputMatched(OutputMatchedEvent { node, filter_id, line, row }) = event
            else { continue };
            if filter_id != "rate-limit" { continue; }
            submissions.push(Submission::new(Intent::Notify(NotifySpec {
                title: "Rate limit".into(),
                body: line.chars().take(2048).collect(),
                urgency: Urgency::High,
                sound: None,
            })));
        }
        submissions
    }
}
```

**TypeScript**

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';

let nextCorrelation = 1n;

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    for (const event of batch) {
      if (event.tag !== 'output-matched' || event.val.filterId !== 'rate-limit') {
        continue;
      }
      const { line } = event.val;
      submissions.push({
        intent: { tag: 'notify', val: {
          title: 'Rate limit', body: Array.from(line).slice(0, 2048).join(''),
          urgency: 'high', sound: undefined,
        } },
        correlation: nextCorrelation++,
        dedupKey: undefined,
      });
    }
    return submissions;
  },
});
```

Outcome: a desktop notice appears. The filter fires at most once per second, because of `rate_per_second: 1`.

#### Example 2: continue an agent only if its prompt has not changed

An agent stops at a prompt and waits. The plugin reads the prompt epoch when it sees the agent go idle, and sends `continue` only if the epoch is still the same when it acts. The Rust form follows.

**Grants:** `terminal.write`

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::primitives::terminal;
use ilium_plugin_sdk::types::intents::{Delivery, SendInputSpec};

/// Returns the submission only while the prompt is still the one seen at idle time.
fn continue_if_unchanged(pane: NodeId, epoch_at_idle: u64) -> Option<Submission> {
    if terminal::prompt_epoch(pane)? != epoch_at_idle {
        return None;
    }
    let spec = SendInputSpec {
        pane,
        text: "continue".into(),
        delivery: Delivery::Submitted,
        prompt_epoch: Some(epoch_at_idle),
    };
    let dedup_key = format!("continue:{pane}:{epoch_at_idle}");
    Some(Submission::keyed(Intent::SendInput(spec), dedup_key))
}
```

Store `epoch_at_idle` when the agent becomes idle, and return the submission from `on_events`. A `refused(conflict)` outcome means the agent moved on. In TypeScript the fields are `promptEpoch` and `dedupKey`.

### Simulated render: a client-side highlight and its tooltip

<figure>
<pre class="term">╭──≡ ● · Codex──────────────────────────────────────────╮
│<span class="t-prompt">$</span> codex run --project ilium                            │
│<span class="t-dim">Reading 14 files</span>                                       │
│<span class="t-warn">  rate limit exceeded, retry in 20 s</span>                   │
│<span class="t-dim">Retrying the request</span>                                   │
╰───────────────────────────────────────────────────────╯
</pre>
<figcaption>A terminal pane in its rounded panel with the chrome title. A client plugin's warning highlight (`terminal.pointer`, client side only) paints the rate-limit row; its label shows as a tooltip after the pointer rests there for 500 ms.</figcaption>
</figure>

### Diagram: the output path

<figure>
<svg viewBox="0 0 400 200" role="img" aria-label="Kernel parses output, the server plugin receives matches and sends submissions back">
<defs>
<marker id="term-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
<path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
</marker>
</defs>
<rect x="10" y="20" width="110" height="40" rx="6" fill="none" stroke="currentColor"/>
<text x="65" y="44" font-size="12" text-anchor="middle" fill="currentColor">Program output</text>
<line x1="120" y1="40" x2="148" y2="40" stroke="currentColor" marker-end="url(#term-arrow)"/>
<rect x="148" y="20" width="242" height="40" rx="6" fill="none" stroke="currentColor"/>
<text x="269" y="44" font-size="12" text-anchor="middle" fill="currentColor">Kernel: parser, screen, filters</text>
<line x1="269" y1="60" x2="269" y2="126" stroke="currentColor" marker-end="url(#term-arrow)"/>
<text x="278" y="98" font-size="10" fill="currentColor">output-matched</text>
<line x1="208" y1="126" x2="208" y2="60" stroke="currentColor" stroke-dasharray="4 3" marker-end="url(#term-arrow)"/>
<text x="198" y="98" font-size="10" text-anchor="end" fill="currentColor">submissions</text>
<rect x="148" y="128" width="242" height="40" rx="6" fill="none" stroke="currentColor"/>
<text x="269" y="152" font-size="12" text-anchor="middle" fill="currentColor">Plugin handler (server side)</text>
<text x="10" y="192" font-size="11" fill="currentColor">Bytes stay in the kernel. Only matches reach the plugin.</text>
</svg>
<figcaption>A server plugin receives matches and answers with submissions. Highlights come from a client plugin, not from this path.</figcaption>
</figure>

### Edge cases and errors

`claim_pointer`, `set_highlights` and `clear_highlights` need `terminal.pointer` and are available to client plugins only.

| Call | Code | When |
|---|---|---|
| any function with a pane, except `read_screen`, `selection` and `prompt_epoch` | `not-permitted` | The grant was declined or revoked. |
| any function with a pane | `not-found` | The pane does not exist or has closed. |
| `read_screen`, `selection`, `prompt_epoch` | none | Unknown pane, declined or revoked grant. A refusal line goes to the plugin log. |
| `add_output_filter` | `invalid-argument` | Empty id, invalid regex, pattern over 512 bytes, compiled program over 1 MiB, rate 0 or above 100. |
| `add_output_filter` | `quota-exceeded` | The plugin has its limit of filters, or the session has 512 runtime filters. |
| `claim_pointer` | `conflict` | Another plugin holds the claim. |
| `claim_pointer`, `set_highlights` | `unsupported` | The node exists but is not a terminal pane. |
| `set_highlights` | `invalid-argument` | Layer name not `[a-z0-9-]{1,32}`, over 4,096 regions, a region with first after last. |
| `set_highlights` | `quota-exceeded` | The plugin already has 16 layers on this pane. |
| `send-input` | `refused(pane-not-ready)` | The pane is restored but its process has not started. Retry after `pane-created` or `tree-changed` for the pane. Check the pane's `is-restored` flag before each write. |
| `send-input` | `refused(conflict)` | The prompt epoch is stale. Do not retry with the same epoch. |
| `send-input` | `refused(rate-limited)` | More than 20 per second. Retry after `retry-after-ms`: the `u32` field of the error record, in milliseconds, set for `rate-limited` to the time until the oldest counted send leaves the 1 s window (see [errors and limits](/pages/reference-errors-and-limits/)). |
| `send-input` | `duplicate(request-ref)` | The dedup key was reserved in the last 10 minutes (the dedup window, separate from the 24-hour journal). The input was not sent twice. |

### What the user sees

- With a client plugin's highlight, the row takes the theme's warning colour, and the label appears as a tooltip after the pointer rests on the row for 500 ms.
- A desktop notice appears. When an attached client shows the pane and reports focus, the notice appears in the client footer instead (see [notify](/pages/capability-notify-desktop/)).
- While a claim is held, clicks no longer select text in that pane, and the wheel goes to the plugin. Typing still reaches the program.
- A highlight stays after `release_pointer`, and disappears when the plugin is disabled, loses the grant, or the pane closes.

### Used by

- Screen transfer between panes: `read_screen`, then `send-input` with delivery `pasted`.
- Pane titles: `pane-title-changed`, raised from terminal title sequences.
- Clickable links: `link_detectors` and `link-activated`.
- Text selection and the right-click menu: `selection` and a `context_menu` item.
- Smart Copy: `read_screen` and `set_highlights`.
- Text triggers: `add_output_filter` and `output-matched`.

Requirement ids for this page: [PRI-001]. See the [requirements index](/pages/reference-requirements/).

### WIT

The contract below is an excerpt. The owner is the [WIT reference](/pages/reference-wit/), and that page wins if the two differ.

```wit
// Excerpt of ilium:primitives/terminal in reference-wit
history-text: func(pane: node-id, max-bytes: u32) -> result<request-id, plugin-error>;
take-history: func(request: request-id) -> option<string>;
```

The other functions of the `terminal` primitive are on the pages for its capabilities and in the [WIT reference](/pages/reference-wit/).

### See also

- [Terminal reads](/pages/primitives-terminal-reads/), [Terminal pointer](/pages/primitives-terminal-pointer/) and [Terminal input](/pages/primitives-terminal-input/) for these areas.
- [Keys](/pages/api-keys/) for key grabs, which go with the pointer and selection work of Smart Copy.
- [Events](/pages/api-events/) for `output-matched`, `link-activated` and `pointer`.
- [Commands](/pages/api-commands/) for the commands that a link detector runs.
- [Agent primitive](/pages/primitives-agent/) for the pane's agent identity and prompt epoch.
- [Read-screen](/pages/capability-terminal-read-screen/), [write](/pages/capability-terminal-write/), [output-filter](/pages/capability-terminal-output-filter/) and [pointer](/pages/capability-terminal-pointer/) capabilities.
- [Terminal view widget](/pages/widgets-media-and-embedded/) for showing a pane's screen inside a plugin's own UI.
- [Errors and limits](/pages/reference-errors-and-limits/) for the codes used above.

