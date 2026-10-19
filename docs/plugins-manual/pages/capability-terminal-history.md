---
layout: page
title: "Capability: terminal.history"
permalink: /pages/capability-terminal-history/
---

## terminal.history

The `terminal.history` capability lets a plugin read the scrollback above the visible screen, as text, and search it with a regular expression. Scrollback holds everything the program printed, including commands, tokens and passwords that scrolled off screen, so the grant is high risk (**H**).

Both calls are asynchronous: they return a request id and the result is picked up with a `take-` call once it is ready.

**You will need:**

- A `terminal.history` grant. It takes no argument and always needs an explicit grant, even for a bundled T1 plugin.
- A pane id and a plan for how much text to ask for: the answer is capped at 4 MiB, newest text kept.
- The request id pattern: see [Requests and outcomes](/pages/requests-and-outcomes/).

<figure>
<svg viewBox="0 0 420 252" role="img" aria-label="A history request is checked and later answered with text or matches">
  <defs><marker id="arr" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto"><path d="M0,0 L8,4 L0,8 z" fill="currentColor"/></marker></defs>
  <rect x="20" y="8" width="380" height="44" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="210" y="27" text-anchor="middle" fill="currentColor" font-size="12">Plugin calls history-text or search-history</text>
  <text x="210" y="43" text-anchor="middle" fill="currentColor" font-size="10">returns a request id</text>
  <line x1="210" y1="52" x2="210" y2="72" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="72" width="380" height="44" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="210" y="91" text-anchor="middle" fill="currentColor" font-size="12">Host checks the explicit grant and the size caps</text>
  <text x="210" y="107" text-anchor="middle" fill="currentColor" font-size="10">max-bytes at most 4 MiB</text>
  <line x1="210" y1="116" x2="210" y2="136" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="136" width="380" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="210" y="155" text-anchor="middle" fill="currentColor" font-size="12">Host reads the scrollback off the UI thread</text>
  <text x="210" y="171" text-anchor="middle" fill="currentColor" font-size="10">newest text is kept when truncated</text>
  <line x1="210" y1="180" x2="210" y2="200" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="200" width="180" height="40" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="110" y="224" text-anchor="middle" fill="currentColor" font-size="11">take-history or take-matches</text>
  <rect x="220" y="200" width="180" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="310" y="224" text-anchor="middle" fill="currentColor" font-size="11">not-permitted, invalid-argument</text>
</svg>
<figcaption>History reads are two steps: submit the request, then take the result when it is ready.</figcaption>
</figure>

### Grant syntax

| Grant | Valid | Reason |
|---|---|---|
| `terminal.history` | yes | the only form |
| `terminal.history:visible` | no | no argument is accepted (`manifest.grant-argument`) |
| `terminal.read-history` | no | unknown grant name |

High risk. A bundled T1 plugin still needs the explicit grant, and the review dialog shows the line in the warning style with `!`, unchecked by default. The grant does not include the live screen; that is [terminal.read-screen](/pages/capability-terminal-read-screen/).

### What it unlocks

| Name | Kind | Meaning |
|---|---|---|
| `terminal.history-text` | function | `history-text(pane, max-bytes) -> result<request-id, plugin-error>`. At most 4 MiB, newest text kept. Pick up with `take-history`. |
| `terminal.take-history` | function | Returns `option<string>`: none until ready. |
| `terminal.search-history` | function | `search-history(pane, regex, max-matches)`. `max-matches` is a `u16`. Pick up with `take-matches`. |
| `history-match` | record | `{ row: u32, column: u16, length: u16 }` in scrollback coordinates. |

See [Terminal primitives](/pages/primitives-terminal/) and [Requests and outcomes](/pages/requests-and-outcomes/).

### Manifest

The grant goes into `manifest.json` under `capabilities`, in the list of the side that uses it (server or client).

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.scrollsearch",
  "capabilities": {
    "server": [
      "panes.read",
      "terminal.history"
    ]
  }
}
```

### Worked example

This plugin searches the scrollback of a pane for the text `error:` when the command `com.example.scrollsearch/errors` runs, and logs how many lines matched.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::Event;
use ilium_plugin_sdk::types::intents::{Intent, Submission};
use ilium_plugin_sdk::primitives::terminal;

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct ScrollSearch;

impl Handler for ScrollSearch {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::CommandInvoked(invoked) = event else { continue };
            if invoked.command != "com.example.scrollsearch/errors" { continue; }
            let Some(pane) = invoked.context.pane else { continue };
            // Async wrapper: submits search-history, then take-matches when ready.
            match terminal::search_history(pane, "error:", 200).block_on() {
                Ok(matches) => log::log(Level::Info, &format!("{} matches", matches.len())),
                Err(error) => log::log(Level::Warn, &format!("search failed: {error:?}")),
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
import { searchHistory } from '@ilium/plugin-sdk/primitives/terminal';
import { log } from '@ilium/plugin-sdk/host/log';

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  async onEvents(batch: Event[]): Promise<Submission[]> {
    for (const event of batch) {
      if (event.tag !== 'command-invoked') continue;
      if (event.val.command !== 'com.example.scrollsearch/errors' || event.val.context.pane === undefined) continue;
      try {
        const matches = await searchHistory(event.val.context.pane, 'error:', 200);
        log('info', `${matches.length} matches`);
      } catch (error) {
        log('warn', `search failed: ${String(error)}`);
      }
    }
    return [];
  },
});
```

The regular expression runs in the host, so the plugin never receives the whole scrollback when it only needs positions. Ask for text with `history-text` only when the plugin must show or store it.

### Errors and edge cases

| Situation | Outcome | What the plugin does |
|---|---|---|
| Grant missing, declined or not explicit | `not-permitted` | Explain in the UI that history needs the grant. |
| `max-bytes` above 4 MiB | `invalid-argument` | Ask for less. |
| Invalid regular expression | `invalid-argument` with the position in the message | Fix the pattern. |
| Pane closed while the request runs | `take-` returns none, then `not-found` | Drop the request. |
| Too many open requests | `busy` | Retry after the previous request finished. |
| Grant revoked later | `revoked` | Discard results already taken if they were secret-bearing. |

The text is cut at a line boundary when it is truncated to `max-bytes`, newest lines kept. Plugins should not write history text to their own storage unless the user asked for it.

### What the user sees

The review dialog lists the grant when a package is installed or first activated. The line is high risk and starts unchecked.

<figure>

<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.scrollsearch 0.1.0 by Example Ltd                              │
│                                                                            │
│ <span class="t-hi">Server side</span>                                                                │
│ <span class="t-warn">! [ ] Reads the scrollback history of terminals</span> <span class="t-dim">………………………</span> <span class="t-warn">terminal.history</span> │
│                                                                            │
│ <span class="t-warn">! marks a high-risk grant. It starts unchecked and is</span>                      │
│ <span class="t-warn">always asked for, even for bundled plugins.</span>                                │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>

<figcaption>The review dialog for a package that requests terminal.history, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar.</figcaption>
</figure>

Listing the grants of an installed plugin from the terminal:

<figure>

<pre class="term"><span class="t-prompt">$</span> ilium plugin grants com.example.scrollsearch
<span class="t-dim">Grant                             Risk     State</span>
terminal.history                  high     <span class="t-warn">declined</span>
</pre>

<figcaption>Output of the grant listing for this plugin. A high-risk grant that the user left unchecked shows as declined.</figcaption>
</figure>

The user can change the grant at any time in Settings > Plugins > com.example.scrollsearch > Capabilities. A revocation re-activates the plugin with the reduced grant set, and the stubs of the declined imports return `not-permitted`.

### Related pages

- [terminal.read-screen](/pages/capability-terminal-read-screen/).
- [terminal.read-cells](/pages/capability-terminal-read-cells/).
- [journal.read](/pages/capability-journal-read/) for host-recorded pane events.
- [Security](/pages/security/).
