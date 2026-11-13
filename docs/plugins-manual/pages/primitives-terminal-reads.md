---
layout: page
title: Terminal reads
permalink: /pages/primitives-terminal-reads/
---

## Terminal reads

The base terminal primitive returns plain text rows. This page covers the reads a plugin needs to reimplement Smart Copy, link detection, screen scraping and history search: styled cells, a frozen snapshot, the terminal modes, the foreground process, scrollback text and a regex search over it, plus two events, `screen-changed` and `user-line-submitted`. The kernel keeps the vt100 emulation and the scrollback store. The plugin never sees a byte stream; it asks for rows when it needs them.

This page extends [Terminal primitive](/pages/primitives-terminal/). Writing to the terminal is in [Terminal input](/pages/primitives-terminal-input/), and the mouse is in [Terminal pointer](/pages/primitives-terminal-pointer/).

### You will need

| Grant | Risk | Unlocks |
|---|---|---|
| `terminal.read-cells` | normal | `read-cells`, `modes`, `foreground-process`, `freeze-snapshot`, `read-snapshot-cells`, `drop-snapshot`, the `screen-changed` event |
| `terminal.history` | high | `history-text`, `take-history`, `search-history`, `take-matches` |
| `agent.read-prompts` | high | the `user-line-submitted` event, for every terminal pane |

The grants are listed in [Capabilities](/pages/reference-capabilities/). `terminal.read-screen` stays the grant for the older text-only `read-screen`; see [terminal.read-screen](/pages/capability-terminal-read-screen/). A high-risk grant needs the user's explicit approval at install, and a bundled first-party plugin gets it through its tier. Both sides can import the interface: the server side reads the authoritative screen, and the client side reads the client's mirror.

### Signatures

```wit
// Excerpt of ilium:primitives/terminal in reference-wit
interface terminal {
  // terminal.read-cells
  record cell { text: string, fg: option<color>, bg: option<color>, add: modifiers, wide: bool }
  record cell-rows { first-row: u32, columns: u16, rows: list<list<cell>>, revision: u64 }
  read-cells: func(pane: node-id, part: screen-part, first-row: u32, count: u32) -> option<cell-rows>;

  flags terminal-mode { alternate-screen, mouse-reporting, bracketed-paste,
                        application-cursor, focus-reporting, synchronized-output }
  modes: func(pane: node-id) -> option<terminal-mode>;

  record foreground-process { name: string, command-line: list<string>, cwd: option<string> }
  foreground-process: func(pane: node-id) -> option<foreground-process>;

  freeze-snapshot: func(pane: node-id) -> result<u64, plugin-error>;
  read-snapshot-cells: func(snapshot: u64, first-row: u32, count: u32) -> option<cell-rows>;
  drop-snapshot: func(snapshot: u64);

  // terminal.history
  history-text: func(pane: node-id, max-bytes: u32) -> result<request-id, plugin-error>;
  take-history: func(request: request-id) -> option<string>;
  record history-match { row: u32, column: u16, length: u16 }
  search-history: func(pane: node-id, regex: string, max-matches: u16) -> result<request-id, plugin-error>;
  take-matches: func(request: request-id) -> option<list<history-match>>;
}
```

| Rust (`primitives::terminal`) | TypeScript (`primitives/terminal`) | Returns |
|---|---|---|
| `read_cells(pane, part, first_row, count)` | `readCells` | `Option<CellRows>` |
| `modes(pane)` | `modes` | `Option<TerminalMode>`; a flag set in Rust, a string array in TypeScript |
| `foreground_process(pane)` | `foregroundProcess` | `Option<ForegroundProcess>` |
| `freeze_snapshot(pane)` | `freezeSnapshot` | `Result<u64, PluginError>` (throws) |
| `read_snapshot_cells(snapshot, first_row, count)` | `readSnapshotCells` | `Option<CellRows>` |
| `drop_snapshot(snapshot)` | `dropSnapshot` | nothing |
| `history_text(pane, max_bytes)` | `historyText` | `Result<RequestId, PluginError>`; the text arrives through `take_history` |
| `search_history(pane, regex, max_matches)` | `searchHistory` | `Result<RequestId, PluginError>`; the matches arrive through `take_matches` |

The two history functions are requests: they return a request id at once, the host works off the main path, and `request-completed` tells the plugin to call `take-history` or `take-matches`. See [Requests and outcomes](/pages/requests-and-outcomes/).

### Cells

A `cell` holds the grapheme cluster at that position in `text`, the resolved foreground and background colours (`none` means the terminal default), the modifier flags `add` (bold, dim, italic, underline, reverse, strike) and `wide`. A wide character occupies two columns: the first cell has `wide: true` and the text, and the second cell has an empty `text`. Row `first-row` counts from the top of the part you asked for. For `visible` it is the top visible row; for `scrollback` it is the oldest retained row.

`columns` is the width of every row at the time of the read. A row that is shorter than the pane is padded with default cells, so `rows[i].len() == columns` always holds. `revision` is the screen revision of the pane, the same counter that `read-screen` returns. A read never blocks the PTY reader: the host copies the requested rows under a short lock.

<figure>
<svg viewBox="0 0 420 170" role="img" aria-label="read-cells returns styled cells from the vt100 grid">
  <rect x="10" y="20" width="120" height="120" rx="8" fill="none" stroke="currentColor"/>
  <text x="70" y="42" text-anchor="middle" fill="currentColor">kernel</text>
  <text x="70" y="64" text-anchor="middle" fill="currentColor" font-size="11">PTY reader</text>
  <text x="70" y="84" text-anchor="middle" fill="currentColor" font-size="11">vt100 grid</text>
  <text x="70" y="104" text-anchor="middle" fill="currentColor" font-size="11">scrollback store</text>
  <rect x="290" y="20" width="120" height="120" rx="8" fill="none" stroke="currentColor"/>
  <text x="350" y="42" text-anchor="middle" fill="currentColor">plugin</text>
  <text x="350" y="64" text-anchor="middle" fill="currentColor" font-size="11">read-cells</text>
  <text x="350" y="84" text-anchor="middle" fill="currentColor" font-size="11">freeze-snapshot</text>
  <text x="350" y="104" text-anchor="middle" fill="currentColor" font-size="11">search-history</text>
  <path d="M290 60 H135" stroke="currentColor" fill="none" marker-end="url(#a)"/>
  <path d="M135 100 H285" stroke="currentColor" fill="none"/>
  <text x="212" y="54" text-anchor="middle" fill="currentColor" font-size="11">ask for rows</text>
  <text x="212" y="116" text-anchor="middle" fill="currentColor" font-size="11">copy of cells</text>
  <text x="210" y="160" text-anchor="middle" fill="currentColor" font-size="11">screen-changed: at most 10 per second per pane</text>
</svg>
<figcaption>The plugin pulls rows; the kernel pushes only the coalesced screen-changed event.</figcaption>
</figure>

### Modes and the foreground process

`modes` returns the flags the running program set. `alternate-screen` is on for full-screen programs such as an editor or a pager, `mouse-reporting` for programs that asked for mouse input, `bracketed-paste` for DEC mode 2004, `application-cursor` for DECCKM, `focus-reporting` for focus events and `synchronized-output` for DEC mode 2026. A plugin that scrapes text should check `alternate-screen` first, because the scrollback holds nothing of a full-screen program.

`foreground-process` reports the process group that currently owns the terminal: its name, its command line and its working directory. The `cwd` is a display string and grants no file access. It is `none` for a pane that was restored and has not started yet.

### Frozen snapshots

`freeze-snapshot` copies the visible screen and the scrollback into a stable object and returns its id. Reads through `read-snapshot-cells` see exactly that copy while the program keeps printing. This is what Smart Copy needs: the user selects, the plugin freezes, analyses at leisure, and writes the result to the clipboard. A snapshot is held by the host until `drop-snapshot`, until the plugin stops, or until the pane is removed. A plugin holds at most 4 snapshots per pane; the fifth call returns `quota-exceeded`. The copy shares storage with the live grid until a row changes, so a snapshot of an idle pane costs almost nothing.

### History text and search

`history-text` returns the scrollback and the visible screen as one string, newest text kept when the pane holds more than `max-bytes`. The limit is 4 MiB per call: a larger `max-bytes` is clamped to 4 MiB and the call does not fail. For `search-history`, a `max-matches` of 0 is `invalid-argument`, and a value over 10,000 is clamped to 10,000 without a failure. Lines are joined with a newline and trailing blanks are removed. `search-history` runs a regular expression of the Rust `regex` dialect over the same text and returns at most `max-matches` positions, each as row, column and length in cells, counted from the oldest retained row. Both are high risk because scrollback can hold secrets the user typed or pasted long ago. The host removes the regions that a program marked as a hidden password prompt.

### Events

| Event | Payload | Grant | Delivery |
|---|---|---|---|
| `screen-changed` | `pane`, `first-row`, `last-row`, `revision` | `terminal.read-cells` | coalesced per pane, at most 10 per second |
| `user-line-submitted` | `pane`, `text`, `at` (`epoch-ms`) | `agent.read-prompts` | never dropped |

`screen-changed` tells the plugin which rows changed between its last event and this one. Rows scrolled out of the visible part do not widen the range; the range is in visible coordinates and the revision tells the plugin it missed a scroll. `user-line-submitted` fires when the user presses Enter on a line in any terminal pane, not only agent panes. The text is the line as the program received it, with the prompt removed when the kernel can tell where the prompt ends.

### Rust and TypeScript

This example scans the visible screen after each change and underlines every URL, then keeps a frozen copy for later.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::primitives::terminal::{self, ScreenPart};

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct UrlScan { last_revision: u64 }

impl Handler for UrlScan {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::ScreenChanged(changed) = event else { continue };
            if changed.revision == self.last_revision { continue; }
            self.last_revision = changed.revision;
            let flags = terminal::modes(changed.pane).unwrap_or_default();
            if flags.contains(TerminalMode::ALTERNATE_SCREEN) { continue; }
            let count = changed.last_row - changed.first_row + 1;
            let Some(rows) = terminal::read_cells(
                changed.pane, ScreenPart::Visible, changed.first_row, count) else { continue };
            for (offset, row) in rows.rows.iter().enumerate() {
                let line: String = row.iter().map(|cell| cell.text.as_str()).collect();
                if line.contains("https://") {
                    log::log(Level::Info, &format!("url on row {}", rows.first_row + offset as u32));
                }
            }
        }
        Vec::new()
    }
}
```

**TypeScript**

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { readCells, modes } from '@ilium/plugin-sdk/primitives/terminal';
import { log } from '@ilium/plugin-sdk/host/log';

let lastRevision = 0n;

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag !== 'screen-changed') continue;
      const { pane, firstRow, lastRow, revision } = event.val;
      if (revision === lastRevision) continue;
      lastRevision = revision;
      if (modes(pane)?.includes('alternate-screen')) continue;
      const rows = readCells(pane, 'visible', firstRow, lastRow - firstRow + 1);
      if (!rows) continue;
      rows.rows.forEach((row, offset) => {
        const line = row.map((cell) => cell.text).join('');
        if (line.includes('https://')) log('info', `url on row ${rows.firstRow + offset}`);
      });
    }
    return [];
  },
});
```

The history search follows the request pattern. Submit it, return, and read the result when the completion arrives.

**Rust**

```rust
let request = terminal::search_history(pane, r"error\[E\d{4}\]", 200)?;
self.pending.insert(request, pane);
// later, in on_events, for Event::RequestCompleted with a matching id:
if let Some(matches) = terminal::take_matches(request) {
    for found in matches { /* found.row, found.column, found.length */ }
}
```

**TypeScript**

```ts
const request = searchHistory(pane, 'error\\[E\\d{4}\\]', 200);
pending.set(request, pane);
// later, in onEvents, for a request-completed event with a matching id:
const found = takeMatches(request);
for (const match of found ?? []) { /* match.row, match.column, match.length */ }
```

### Outcomes, errors and limits

`read-cells` and `read-snapshot-cells` return `none` and never a `plugin-error`, because their WIT signature is `option<cell-rows>`. The host writes a refusal line with the code to the plugin log: `pane-not-ready` when the pane has no process yet, `not-found` for no such pane or snapshot, `not-permitted` without the grant, and `quota-exceeded` when `count` is above 1,000 rows. Only the calls in the table below return a `plugin-error`.

| Call | Error | Cause |
|---|---|---|
| `freeze-snapshot` | `pane-not-ready` | the pane has no process yet |
| `freeze-snapshot` | `not-found` | no such pane |
| `freeze-snapshot` | `quota-exceeded` | more than 4 live snapshots on this pane |
| `search-history` | `invalid-argument` | the regex does not compile in the `regex` dialect, or is over 1 KiB, or `max-matches` is 0 |
| `history-text`, `search-history` | completion `timed-out` | the host gave up after its deadline |
| any call | `not-permitted` | the grant is missing or revoked |

| Limit | Value |
|---|---|
| Rows per `read-cells` | 1,000 |
| `history-text` | 4 MiB, newest kept |
| Matches per search | the `max-matches` you pass, at most 10,000 |
| Live snapshots per pane and plugin | 4 |
| `screen-changed` | 10 per second per pane |

### What the user sees

Nothing changes in the pane while a plugin reads it. A plugin that uses the reads typically draws with highlights or the status bar. This render shows a terminal pane after a build, with a plugin highlight on the error row (the row is drawn in the warning colour of the theme).

<pre class="term">╭──≡ ● · Shell────────────────────────────────────────────────────────╮
│<span class="t-prompt">$</span> cargo build                                                        │
│   Compiling demo v0.1.0                                             │
│<span class="t-err">error[E0308]: mismatched types</span>                                       │
│ <span class="t-dim">--&gt;</span> src/main.rs:12:5                                                │
│<span class="t-prompt">$</span>                                                                    │
╰─────────────────────────────────────────────────────────────────────╯
</pre>

### See also

- [Terminal primitive](/pages/primitives-terminal/)
- [Terminal input](/pages/primitives-terminal-input/)
- [Terminal pointer](/pages/primitives-terminal-pointer/)
- [Errors and limits](/pages/reference-errors-and-limits/)
