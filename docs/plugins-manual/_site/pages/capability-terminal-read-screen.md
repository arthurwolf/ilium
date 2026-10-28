# terminal.read-screen

The `terminal.read-screen` grant lets a plugin read the text that a terminal pane shows: the visible rows, the scrollback above them and the user's current selection. The server reads the authoritative screen. The client reads its own mirror of the same screen. Many first-party features need this grant, so it is common, but it exposes everything printed in the pane. Tell the user what the plugin reads before they grant it.

<figure>
<svg viewBox="0 0 360 170" role="img" aria-label="The plugin calls read-screen for a pane and receives the screen record or none">
  <defs><marker id="arr-read-screen" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto"><path d="M0,0 L10,5 L0,10 z" fill="currentColor"/></marker></defs>
  <rect x="20" y="10" width="320" height="44" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="180" y="36" text-anchor="middle" fill="currentColor" font-size="12">Plugin (needs terminal.read-screen)</text>
  <rect x="20" y="110" width="320" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="180" y="136" text-anchor="middle" fill="currentColor" font-size="12">Terminal pane screen</text>
  <line x1="60" y1="54" x2="60" y2="110" stroke="currentColor" marker-end="url(#arr-read-screen)"/>
  <text x="74" y="76" fill="currentColor" font-size="10">read-screen(pane, part, max-rows)</text>
  <line x1="320" y1="110" x2="320" y2="54" stroke="currentColor" marker-end="url(#arr-read-screen)"/>
  <text x="312" y="102" text-anchor="end" fill="currentColor" font-size="10">screen record or none</text>
</svg>
<figcaption>The plugin asks for the visible or scrollback rows of one pane. The host returns a screen record, or none when the grant is missing or the pane is not a terminal. Typing into a pane needs terminal.write instead.</figcaption>
</figure>

<div class="warn">

A terminal screen can show secrets: tokens printed by a command, passwords typed at a prompt, private file contents and the text of a private conversation. A plugin that holds this grant can read all of it. Explain this to the user in the plugin's description, and read only the rows the feature needs.

</div>

## You will need

- The grant `terminal.read-screen` on every side that reads screens. Grants are per side: a server-side read needs the grant on the server side, and a client-side read needs it on the client side.
- [panes.read](/pages/capability-panes-read/) if the plugin must discover which panes exist and what kind they are. An event such as `agent-activity-changed` already carries the pane's node id, but receiving that event needs [agent.read](/pages/capability-agent-read/), so the examples below list `agent.read` beside `terminal.read-screen`.
- A plugin written in Rust with `ilium-plugin-sdk`, or in TypeScript with `@ilium/plugin-sdk`. The two read functions are synchronous, so they need no `await`.

## Grant syntax

- Valid: `terminal.read-screen`
- Invalid: `terminal.read-screen:visible`. The part of the screen is an argument of the call, not of the grant. A grant with an argument fails the install with `manifest.grant-argument`.

## The surface

The grant opens two functions of the `terminal` primitive (`ilium:primitives/terminal`). Both are synchronous and return an option.

| Rust (`primitives::terminal`) | TypeScript (`@ilium/plugin-sdk/primitives/terminal`) | Arguments | Returns |
|---|---|---|---|
| `read_screen(pane, part, max_rows)` | `readScreen(pane, part, maxRows)` | `pane`: node id (`NodeId`, a `bigint` in TypeScript). `part`: `ScreenPart::Visible` or `ScreenPart::Scrollback` (`'visible'` or `'scrollback'`). `max_rows`: `u32` | `Option<Screen>`, or `undefined` |
| `selection(pane)` | `selection(pane)` | `pane`: node id | `Option<Selection>`, or `undefined` |

The `Screen` record has these fields:

| Field (Rust / TypeScript) | Type | Meaning |
|---|---|---|
| `columns` / `columns` | `u16` / `number` | Width of the screen in terminal cells. |
| `rows` / `rows` | `Vec<String>` / `string[]` | The returned rows, top to bottom. |
| `cursor_row` / `cursorRow` | `u16` / `number` | Cursor row, counted in the returned rows. |
| `cursor_column` / `cursorColumn` | `u16` / `number` | Cursor column, counted in terminal cells. |
| `title` / `title` | `Option<String>` / `string` or `undefined` | The title the program set, or none. |
| `revision` / `revision` | `u64` / `bigint` | Counter that rises when the screen changes. |

The `Selection` record has `first_row`, `first_column`, `last_row` and `last_column` (`u32` and `u16`; `number` in TypeScript), and `text` (`String`; `string`).

`max_rows` is clamped to 10,000. A larger value does not fail: the host returns at most 10,000 rows.

## How the reads behave

- **Visible part.** `ScreenPart::Visible` returns the bottom `max_rows` rows of the visible area, top to bottom. The newest output and the prompt sit at the bottom, so this is the part most features need. The cursor row and column refer to the returned rows, and row 0 is the first returned row. When the cursor sits above the returned rows, `cursor_row` is the number of returned rows and `cursor_column` is 0.
- **Scrollback part.** `ScreenPart::Scrollback` returns the rows directly above the visible area. The host counts upward from the newest scrollback row and returns the rows top to bottom, oldest first. When the history holds fewer rows than `max_rows`, the record has fewer rows. When there is no history, `rows` is empty.
- **Zero rows.** `max_rows` of 0 returns a record with an empty `rows` list. The revision and the cursor are still filled in.
- **Row text.** Each row is the printed characters of its cells, with escape sequences and trailing blanks removed. A wide character, such as a CJK glyph, appears once in the string, and its second cell is not repeated.
- **Revision.** The revision is a counter per pane. It starts at 0 when the host starts the pane and rises whenever either part changes. It is not saved in the snapshot, so it starts again after a restart. Compare revisions only within one run of the plugin. When `revision` equals the value you stored, the screen has not changed and you can skip the work.
- **Selection.** The coordinates count from the top of the visible area. `text` holds only the part of the selection inside the visible area, with lines joined by a newline. A selection that lies entirely in the scrollback returns none.
- **Snapshot semantics.** Each call is a snapshot. A later call can return different rows, and the call itself never blocks or pushes. To learn that the screen changed, use the `screen-changed` event of [terminal.read-cells](/pages/capability-terminal-read-cells/), or use [terminal.output-filter](/pages/capability-terminal-output-filter/) for text patterns, or [agent.read](/pages/capability-agent-read/) for activity changes, and then read the screen.

## Risk and tier rules

- The grant is not marked H (high risk). The review dialog still shows the privacy warning above, and the user should read it before granting.
- A T1 bundled plugin receives it automatically, up to its manifest ceiling.
- A T2 or T3 plugin lists the grant in the review dialog, checked by default. The user can uncheck it before installing and can revoke it later. After a revoke, reads return none.
- The grant reads screens only. Typing into them needs `terminal.write`, and mouse control needs `terminal.pointer`.

## What the review dialog says

<figure>
<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.terminal 0.1.0 by Example Ltd                                  │
│                                                                            │
│ <span class="t-hi">Server side</span>                                                                │
│ [x] Reads the text on terminal screens <span class="t-dim">……………………………………</span> terminal.read-screen │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>The review dialog for a package that requests terminal.read-screen, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar. Checked rows are granted; a high-risk row starts unchecked.</figcaption>
</figure>

## When the grant is declined

A declined or revoked grant does not raise an error. The screen reads return none, because the functions have no error result. The host writes one refusal line to the plugin log, as the location and editor reads do. A plugin that shows screen text, such as a workspace search, must show its empty state. It must not show an error that suggests the pane is broken.

<figure>
<pre class="term"><span class="t-dim">2026-10-11 09:14:02</span> <span class="t-ok">info</span>  node 42 idle: 2 rows hold "error"
<span class="t-dim">2026-10-11 09:14:03</span> <span class="t-warn">warn</span>  terminal.read-screen not granted, node 57 skipped
</pre>
<figcaption>Illustrative, not generated. Two plugin log lines: one successful read and one read refused by a missing grant. The wording of the lines is a placeholder.</figcaption>
</figure>

## Worked example in Rust

This server-side plugin reacts when an agent in a pane goes idle. It reads the last 40 visible rows, counts the rows that contain the word `error`, and sets a badge on the pane. The badge needs `tree.decorate` and a declared `errors` decoration slot.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::primitives::terminal::{self, ScreenPart};
use ilium_plugin_sdk::types::events::AgentActivityEvent;
use ilium_plugin_sdk::types::intents::SetDecorationSpec;
use ilium_plugin_sdk::types::tree::AgentActivity;

const ROWS_TO_READ: u32 = 40;

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct ErrorBadge;

impl Lifecycle for ErrorBadge {}

impl Handler for ErrorBadge {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            let Event::AgentActivityChanged(AgentActivityEvent { node, current, .. }) =
                event
            else {
                continue;
            };
            if current != AgentActivity::Idle {
                continue;
            }
            // None means the grant is missing or the pane is not a terminal.
            let Some(screen) = terminal::read_screen(node, ScreenPart::Visible, ROWS_TO_READ)
            else {
                continue;
            };
            let error_rows = screen.rows.iter().filter(|row| row.contains("error")).count();
            submissions.push(Submission::new(Intent::SetDecoration(SetDecorationSpec {
                node,
                slot: "errors".into(),
                content: vec![Span::styled(
                    format!("{error_rows} errors"),
                    Style::role(ColorRole::Danger),
                )],
                icon: None,
                sort_key: None,
                persist: false,
            })));
        }
        submissions
    }
}
```

The manifest requests the grant on the server side and declares the slot:

```json
{
  "capabilities": {
    "server": ["terminal.read-screen", "agent.read", "tree.decorate"]
  },
  "contributes": {
    "decoration_slots": [{ "id": "errors", "placement": "badge", "priority": 40 }]
  }
}
```

## Worked example in TypeScript

The same feature in TypeScript. The `readScreen` call takes the `bigint` node id from the event. The `span` and `role` helpers come from `@ilium/plugin-sdk/ui/build`.

**TypeScript**

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { readScreen } from '@ilium/plugin-sdk/primitives/terminal';
import { role, span } from '@ilium/plugin-sdk/ui/build';

const ROWS_TO_READ = 40;
let nextCorrelation = 1n;

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    for (const event of batch) {
      if (event.tag !== 'agent-activity-changed') continue;
      const { node, current } = event.val;
      if (current !== 'idle') continue;
      // undefined means the grant is missing or the pane is not a terminal.
      const screen = readScreen(node, 'visible', ROWS_TO_READ);
      if (screen === undefined) continue;
      const errorRows = screen.rows.filter((row) => row.includes('error')).length;
      submissions.push({
        intent: {
          tag: 'set-decoration',
          val: {
            node,
            slot: 'errors',
            content: [span(`${errorRows} errors`, { fg: role('danger') })],
            icon: undefined,
            sortKey: undefined,
            persist: false,
          },
        },
        correlation: nextCorrelation++,
        dedupKey: undefined,
      });
    }
    return submissions;
  },
});
```

## Edge cases and errors

The two functions have no error codes. A failed read is a none, and the plugin log shows the reason only when the grant is missing.

| Situation | Result |
|---|---|
| Grant missing, declined or revoked | none, and one refusal line in the plugin log |
| Pane is not a terminal, or the node id is unknown | none, with no log line |
| Pane has no selection (`selection`) | none |
| `max_rows` above 10,000 | clamped to 10,000 rows |
| `max_rows` of 0 | empty `rows`, with the revision and the cursor |
| Scrollback shorter than `max_rows` | fewer rows, no error |

A plugin should treat none as "nothing to show now". It should not retry in a loop. When the grant is the reason, the user must grant it again in Settings.

## What the user sees

- In the review dialog, the row "Read text shown in terminals" with the grant name at the right edge.
- In a first-party feature that needs the grant, such as workspace search, an empty state when the grant is declined.
- In a `terminal-view` widget, which embeds a pane's screen. Without the grant the widget draws an empty frame with the text `not permitted`.

## Minimal example

The manifest excerpt requests the grant on the client side, where a copy feature runs. The client reads its own mirror of the screen, so the grant belongs on the client side.

```json
{
  "capabilities": {
    "client": ["terminal.read-screen", "agent.read", "clipboard.write"]
  }
}
```

The raw function, as the host imports it:

```wit
// Excerpt of ilium:primitives/terminal in reference-wit
// ilium:primitives/terminal
// terminal.read-screen
read-screen: func(pane: node-id, part: screen-part, max-rows: u32)
    -> option<screen>;
```

A call that reads the last ten visible rows of pane 42 returns a record like this:

```text
read-screen(42, visible, 10) -> some({ columns: 120,
                                       rows: [ "$ cargo test", "..." ],
                                       cursor-row: 9, cursor-column: 0,
                                       title: some("review-agent"),
                                       revision: 318 })
```

## First-party features that request it

- Feature 9, workspace search.
- Feature 15, clickable links in the terminal.
- Feature 16, text selection and right-click menu.
- Feature 17, screen transfer between panes.
- Feature 32, agent debug history.
- Feature 35, agent toolbar.
- Feature 41, automatic pane titles.
- Feature 65, Smart Copy light.
- Feature 66, Smart Copy AI.

## See also

- [primitives-terminal](/pages/primitives-terminal/) for the whole `terminal` primitive.
- [terminal.pointer](/pages/capability-terminal-pointer/) for the highlights that copy features draw over the screen.
- [terminal.output-filter](/pages/capability-terminal-output-filter/) for a rule that watches new output without reading the whole screen.
- [terminal.write](/pages/capability-terminal-write/) for sending input to a pane.
- [panes.read](/pages/capability-panes-read/) for finding the panes before reading them.

Checked against the API version `ilium:plugin@0.1.0`.
