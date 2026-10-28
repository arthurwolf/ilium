# Capability: journal.read

## journal.read

The pane journal records what happened to a pane: plugin entries written with `journal.log-event` and host entries such as `pane-created`, `pane-exited` and `input-sent` (with the dedup key and the delivery result). A plugin always reads its own entries. The `journal.read` capability extends reading to the host entries and to the entries of other plugins. Entries survive restarts and are trimmed oldest first.

**You will need:**

- A `journal.read` grant. No argument.
- Optional: `journal: true` on `send-input`, which writes an entry before the PTY write.

<figure>
<svg viewBox="0 0 420 252" role="img" aria-label="Journal reads return own entries always and host and foreign entries when the grant is held">
  <defs><marker id="arr" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto"><path d="M0,0 L8,4 L0,8 z" fill="currentColor"/></marker></defs>
  <rect x="20" y="8" width="380" height="44" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="210" y="27" text-anchor="middle" fill="currentColor" font-size="12">Plugin calls journal.read(pane, since, limit)</text>
  <text x="210" y="43" text-anchor="middle" fill="currentColor" font-size="10">own entries need no grant</text>
  <line x1="210" y1="52" x2="210" y2="72" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="72" width="380" height="44" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="210" y="91" text-anchor="middle" fill="currentColor" font-size="12">Host adds host and foreign entries when granted</text>
  <text x="210" y="107" text-anchor="middle" fill="currentColor" font-size="10">ordered by time</text>
  <line x1="210" y1="116" x2="210" y2="136" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="136" width="380" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="210" y="155" text-anchor="middle" fill="currentColor" font-size="12">Plugin receives journal-entry records</text>
  <text x="210" y="171" text-anchor="middle" fill="currentColor" font-size="10">source tells who wrote it</text>
  <line x1="210" y1="180" x2="210" y2="200" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="200" width="180" height="40" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="110" y="224" text-anchor="middle" fill="currentColor" font-size="11">list of journal-entry</text>
  <rect x="220" y="200" width="180" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="310" y="224" text-anchor="middle" fill="currentColor" font-size="11">not-permitted for foreign sources</text>
</svg>
<figcaption>Without the grant the result holds only the plugin's own entries.</figcaption>
</figure>

### Grant syntax

| Grant | Valid | Reason |
|---|---|---|
| `journal.read` | yes | the only form |
| `journal.write` | no | does not exist; writing own entries is always granted |
| `journal.read:pane` | no | no argument |

Normal risk. Journal entries record events and delivery results, not screen contents. Payloads are at most 4 KiB.

### What it unlocks

| Name | Kind | Meaning |
|---|---|---|
| `journal.read` | function | `read(pane?, since, limit) -> list<journal-entry>`; with the grant the list includes host entries and other plugins' entries. |
| `journal-entry` | record | `{ at, pane?, source, kind, payload-json }`. |
| `journal.log-event` | function | Always granted: `log-event(pane?, kind, payload-json)`; payload at most 4 KiB. |

See [Clock and timers](/pages/api-clock-and-timers/) and [Storage](/pages/api-storage/).

### Manifest

The grant goes into `manifest.json` under `capabilities`, in the list of the side that uses it (server or client).

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.audit",
  "capabilities": {
    "server": [
      "journal.read"
    ]
  }
}
```

### Worked example

This plugin reads the last 50 journal entries of a pane from the last hour and counts the host entries of kind `input-sent`.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::Event;
use ilium_plugin_sdk::types::intents::{Intent, Submission};
use ilium_plugin_sdk::host::{clock, journal};

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct Audit;

impl Handler for Audit {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::CommandInvoked(invoked) = event else { continue };
            if invoked.command != "com.example.audit/count" { continue; }
            let since = clock::now_unix_millis() - 3_600_000;
            let entries = journal::read(invoked.context.pane, since, 50);
            let sent = entries.iter().filter(|entry| entry.kind == "input-sent").count();
            log::log(Level::Info, &format!("{sent} inputs sent in the last hour"));
        }
        Vec::new()
    }
}
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import type { Event, Submission } from '@ilium/plugin-sdk/types';
import { read } from '@ilium/plugin-sdk/host/journal';
import { nowUnixMillis } from '@ilium/plugin-sdk/host/clock';
import { log } from '@ilium/plugin-sdk/host/log';

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag !== 'command-invoked' || event.val.command !== 'com.example.audit/count') continue;
      const entries = read(event.val.context.pane, nowUnixMillis() - 3_600_000n, 50);
      const sent = entries.filter((entry) => entry.kind === 'input-sent').length;
      log('info', `${sent} inputs sent in the last hour`);
    }
    return [];
  },
});
```

Entries from other sources can be absent when the grant is missing; the call does not fail, it returns fewer entries.

### Errors and edge cases

| Situation | Outcome | What the plugin does |
|---|---|---|
| Grant missing | only own entries are returned | Do not treat an empty result as proof that nothing happened. |
| `limit` above 1,000 (the per-call read cap) | clamped to 1,000 | Page with `since`. |
| Payload above 4 KiB when logging | `invalid-argument` | Store a reference instead. |
| Pane closed | entries remain until trimmed | Nothing. |
| Journal full | oldest entries trimmed | Nothing. |
| Grant revoked while a call is in flight | `revoked` | Stop reading foreign entries. |
| Call made after the grant was revoked | `not-permitted` | Stop reading foreign entries. |

Journal entries are part of the session snapshot, so they survive a restart of the server.

### What the user sees

The review dialog lists the grant when a package is installed or first activated. The line states that the plugin reads pane history records.

<figure>

<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.audit 0.1.0 by Example Ltd                                     │
│                                                                            │
│ <span class="t-hi">Server side</span>                                                                │
│ [x] Reads pane activity records <span class="t-dim">……………………………………………………………………………</span> journal.read │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>

<figcaption>The review dialog for a package that requests journal.read, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar.</figcaption>
</figure>

Listing the grants of an installed plugin from the terminal:

<figure>

<pre class="term"><span class="t-prompt">$</span> ilium plugin grants com.example.audit
<span class="t-dim">Grant                             Risk     State</span>
journal.read                      normal   <span class="t-ok">granted</span>
</pre>

<figcaption>Output of the grant listing for this plugin. A high-risk grant that the user left unchecked shows as declined.</figcaption>
</figure>

The user can change the grant at any time in Settings > Plugins > com.example.audit > Capabilities. A revocation re-activates the plugin with the reduced grant set, and the stubs of the declined imports return `not-permitted`.

### Related pages

- [terminal.write](/pages/capability-terminal-write/).
- [panes.read](/pages/capability-panes-read/).
- [Clock and timers](/pages/api-clock-and-timers/).
- [Always-granted capabilities](/pages/capability-always-granted/).

### Security

The journal is not the audit log ([Audit log](/pages/reference-audit-log/)). Input sent to terminals is redacted in journal reads ([SEC-243]).
