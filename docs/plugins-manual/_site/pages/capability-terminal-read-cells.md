# Capability: terminal.read-cells

## terminal.read-cells

The `terminal.read-cells` capability lets a plugin read the cells of a terminal pane, not only its text. A cell carries its text, foreground and background colour, text modifiers and a wide-character flag. The plugin also reads the terminal modes that the running program enabled, the foreground process, and takes a frozen snapshot of screen plus scrollback that stays stable while the program keeps writing.

The grant is the colour-aware sibling of [terminal.read-screen](/pages/capability-terminal-read-screen/), which returns plain text only. It does not read history above the scrollback window of the visible screen request; for the full scrollback as text use [terminal.history](/pages/capability-terminal-history/).

**You will need:**

- A `terminal.read-cells` grant on the side that reads. The grant takes no argument.
- A pane id from the tree or from a [pane event](/pages/api-events/).
- Optional: the `screen-changed` event, which this grant also enables, to know when to read again.

<figure>
<svg viewBox="0 0 420 252" role="img" aria-label="A read-cells call is checked and answered with cell rows and a revision">
  <defs><marker id="arr" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto"><path d="M0,0 L8,4 L0,8 z" fill="currentColor"/></marker></defs>
  <rect x="20" y="8" width="380" height="44" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="210" y="27" text-anchor="middle" fill="currentColor" font-size="12">Plugin calls read-cells(pane, part, first_row, count)</text>
  <text x="210" y="43" text-anchor="middle" fill="currentColor" font-size="10">count is at most 1,000 rows</text>
  <line x1="210" y1="52" x2="210" y2="72" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="72" width="380" height="44" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="210" y="91" text-anchor="middle" fill="currentColor" font-size="12">Host checks the grant, the pane and the range</text>
  <text x="210" y="107" text-anchor="middle" fill="currentColor" font-size="10">unknown pane returns none</text>
  <line x1="210" y1="116" x2="210" y2="136" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="136" width="380" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="210" y="155" text-anchor="middle" fill="currentColor" font-size="12">Host copies the cells from the live screen</text>
  <text x="210" y="171" text-anchor="middle" fill="currentColor" font-size="10">no mutation, a revision number is attached</text>
  <line x1="210" y1="180" x2="210" y2="200" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="200" width="180" height="40" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="110" y="224" text-anchor="middle" fill="currentColor" font-size="11">cell-rows with revision</text>
  <rect x="220" y="200" width="180" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="310" y="224" text-anchor="middle" fill="currentColor" font-size="11">none, or not-permitted</text>
</svg>
<figcaption>A read of cells is a copy. The revision lets the plugin tell whether the screen changed between two reads.</figcaption>
</figure>

### Grant syntax

| Grant | Valid | Reason |
|---|---|---|
| `terminal.read-cells` | yes | the only form |
| `terminal.read-cells:*` | no | the grant takes no argument (`manifest.grant-argument`) |
| `terminal.cells` | no | unknown grant name (`manifest.grant-unknown`) |

The grant is a normal-risk capability. Screen contents can still hold secrets, so the review dialog states that the plugin reads what is on the screen. It is granted automatically to bundled T1 plugins up to their manifest ceiling.

### What it unlocks

| Name | Kind | Meaning |
|---|---|---|
| `terminal.read-cells` | function | `read-cells(pane, part, first-row, count) -> option<cell-rows>`. `part` is a `screen-part`. `count` is at most 1,000. |
| `terminal.modes` | function | Returns the `terminal-mode` flags: alternate screen, mouse reporting, bracketed paste, application cursor, focus reporting, synchronized output. |
| `terminal.foreground-process` | function | Returns `foreground-process { name, command-line, cwd }`. The `cwd` is for display only. |
| `terminal.freeze-snapshot` | function | Returns a snapshot id: a stable copy of screen and scrollback. Read it with `read-snapshot-cells`, release it with `drop-snapshot`. |
| `screen-changed` | event | `{ pane, first-row, last-row, revision }`, coalesced to at most 10 per second per pane. |

The interface is described on [Terminal primitives](/pages/primitives-terminal/) and the events on [Events](/pages/api-events/). Pane lookups go through [Panes](/pages/api-panes/).

### Manifest

The grant goes into `manifest.json` under `capabilities`, in the list of the side that uses it (server or client).

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.cellview",
  "capabilities": {
    "server": [
      "panes.read",
      "terminal.read-cells"
    ]
  }
}
```

### Worked example

This plugin copies the first 40 visible rows when the command `com.example.cellview/snapshot` runs. The snapshot keeps the copy stable while the program continues to write.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::Event;
use ilium_plugin_sdk::types::intents::{Intent, Submission};
use ilium_plugin_sdk::primitives::terminal::{self, ScreenPart};

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct CellView;

impl Handler for CellView {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::CommandInvoked(invoked) = event else { continue };
            if invoked.command != "com.example.cellview/snapshot" { continue; }
            let Some(pane) = invoked.context.pane else { continue };
            match terminal::freeze_snapshot(pane) {
                Ok(snapshot) => {
                    if let Some(rows) = terminal::read_snapshot_cells(snapshot, 0, 40) {
                        log::log(Level::Info, &format!("{} rows, revision {}", rows.rows.len(), rows.revision));
                    }
                    terminal::drop_snapshot(snapshot);
                }
                Err(error) => log::log(Level::Warn, &format!("freeze failed: {error:?}")),
            }
        }
        Vec::new()
    }
}
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import type { Event, Submission } from '@ilium/plugin-sdk/types';
import { freezeSnapshot, readSnapshotCells, dropSnapshot } from '@ilium/plugin-sdk/primitives/terminal';
import { log } from '@ilium/plugin-sdk/host/log';

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag !== 'command-invoked') continue;
      if (event.val.command !== 'com.example.cellview/snapshot' || event.val.context.pane === undefined) continue;
      try {
        const snapshot = freezeSnapshot(event.val.context.pane);
        const rows = readSnapshotCells(snapshot, 0, 40);
        if (rows) log('info', `${rows.rows.length} rows, revision ${rows.revision}`);
        dropSnapshot(snapshot);
      } catch (error) {
        log('warn', `freeze failed: ${String(error)}`);
      }
    }
    return [];
  },
});
```

The snapshot is released in every path. A plugin that forgets `drop-snapshot` keeps the copy until deactivation, and the host counts snapshots against the plugin memory quota. A `freeze-snapshot` that would take the plugin over that quota returns `quota-exceeded` and creates no snapshot.

### Errors and edge cases

| Situation | Outcome | What the plugin does |
|---|---|---|
| Grant missing or declined | `not-permitted` | Fall back to [terminal.read-screen](/pages/capability-terminal-read-screen/) if that is granted. |
| Unknown pane or pane closed | `none`, or `not-found` for `freeze-snapshot` | Drop the cached pane id. |
| `count` above 1,000 | `invalid-argument` | Read in chunks and compare the `revision` of each chunk. |
| Pane has no screen yet | `pane-not-ready` | Wait for the next `screen-changed` event. |
| `freeze-snapshot` over the memory quota | `quota-exceeded` | Drop older snapshots, then retry. |
| Snapshot id already dropped | `none` | Do not reuse a dropped snapshot. |
| Grant revoked later | `revoked`, then `not-permitted` | Stop reading; the plugin is re-activated with the reduced set. |

Reading cells does not wake an idle terminal. A read of a pane that the user closed during the call returns none, not an error.

### What the user sees

The review dialog lists the grant when a package is installed or first activated. The line states the colour-aware reading of the screen.

<figure>

<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.cellview 0.1.0 by Example Ltd                                  │
│                                                                            │
│ <span class="t-hi">Server side</span>                                                                │
│ [x] Reads colours and cells of terminal screens <span class="t-dim">………………</span> terminal.read-cells │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>

<figcaption>The review dialog for a package that requests terminal.read-cells, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar.</figcaption>
</figure>

Listing the grants of an installed plugin from the terminal:

<figure>

<pre class="term"><span class="t-prompt">$</span> ilium plugin grants com.example.cellview
<span class="t-dim">Grant                             Risk     State</span>
terminal.read-cells               normal   <span class="t-ok">granted</span>
</pre>

<figcaption>Output of the grant listing for this plugin. A high-risk grant that the user left unchecked shows as declined.</figcaption>
</figure>

The user can change the grant at any time in Settings > Plugins > com.example.cellview > Capabilities. A revocation re-activates the plugin with the reduced grant set, and the stubs of the declined imports return `not-permitted`.

### Related pages

- [terminal.read-screen](/pages/capability-terminal-read-screen/) for plain text.
- [terminal.history](/pages/capability-terminal-history/) for scrollback text and search.
- [Terminal primitives](/pages/primitives-terminal/).
- [Capabilities and grants](/pages/capabilities/).
