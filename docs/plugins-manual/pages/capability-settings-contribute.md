---
layout: page
title: "Capability: settings.contribute:<kernel-tab-id>"
permalink: /pages/capability-settings-contribute/
---

## settings.contribute:<kernel-tab-id>

The `settings.contribute:<kernel-tab-id>` capability lets a plugin add a section with rows to a built-in Settings tab, for example the Terminal tab. The section is always labelled with the plugin name, so the user can tell plugin rows from kernel rows. The grant gives permission to draw the section and nothing else: it does not read kernel values, and it does not write them. A plugin's own Settings tab needs no grant.

**You will need:**

- A `settings.contribute:<kernel-tab-id>` grant with one tab id, for example `terminal`.
- A `contributes.settings` entry with `kind: "section"` whose `tab` is that kernel tab id.

<figure>
<svg viewBox="0 0 420 252" role="img" aria-label="A section for a kernel tab is declared, checked against the grant and drawn with a plugin label">
  <defs><marker id="arr" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto"><path d="M0,0 L8,4 L0,8 z" fill="currentColor"/></marker></defs>
  <rect x="20" y="8" width="380" height="44" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="210" y="27" text-anchor="middle" fill="currentColor" font-size="12">Manifest declares a section for a kernel tab</text>
  <text x="210" y="43" text-anchor="middle" fill="currentColor" font-size="10">kind section, tab terminal</text>
  <line x1="210" y1="52" x2="210" y2="72" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="72" width="380" height="44" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="210" y="91" text-anchor="middle" fill="currentColor" font-size="12">Host checks the grant at install</text>
  <text x="210" y="107" text-anchor="middle" fill="currentColor" font-size="10">manifest.settings-target-grant otherwise</text>
  <line x1="210" y1="116" x2="210" y2="136" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="136" width="380" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="210" y="155" text-anchor="middle" fill="currentColor" font-size="12">Settings shows the labelled section</text>
  <text x="210" y="171" text-anchor="middle" fill="currentColor" font-size="10">values live under the plugin id</text>
  <line x1="210" y1="180" x2="210" y2="200" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="200" width="180" height="40" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="110" y="224" text-anchor="middle" fill="currentColor" font-size="11">section drawn</text>
  <rect x="220" y="200" width="180" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="310" y="224" text-anchor="middle" fill="currentColor" font-size="11">install refused</text>
</svg>
<figcaption>The kernel rows stay untouched. Plugin values are stored under the plugin id and never in a kernel table.</figcaption>
</figure>

### Grant syntax

| Grant | Valid | Reason |
|---|---|---|
| `settings.contribute:terminal` | yes | one kernel tab id |
| `settings.contribute` | no | argument required |
| `settings.contribute:*` | no | no wildcard |
| `settings.contribute:self:main` | no | an own tab needs no grant |

Normal risk. The grant is independent of `settings.read-all`, `settings.write-all` and `settings.read-kernel`, in both directions.

### What it unlocks

| Name | Kind | Meaning |
|---|---|---|
| `contributes.settings` section | contribution | `{ kind: "section", tab: "terminal", id, title, scope, placement, rows }` with at most 24 rows. Limits: 2 sections per plugin per kernel tab, 6 plugin sections per kernel tab. |
| `settings-changed` | event | Reports edits of the plugin's own values. |

Rows, row types and validation are on [Settings](/pages/api-settings/) and [Settings screens](/pages/settings-screens/). The tutorial is [Extend a built-in settings tab](/pages/tutorial-11-settings-extend-tab/).

### Manifest

The grant goes into `manifest.json` under `capabilities`, in the list of the side that uses it (the side of the section's scope).

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.clipboardhistory",
  "capabilities": {
    "client": [
      "settings.contribute:terminal"
    ]
  }
}
```

### Worked example

The section itself is declared in the manifest. The plugin code only reacts to the user's edits. This plugin logs the value of the `max_entries` row when it changes.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::Event;
use ilium_plugin_sdk::types::intents::{Intent, Submission};
#[ilium_plugin(side = "client")]
#[derive(Default)]
struct ClipboardHistory;

impl Handler for ClipboardHistory {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::SettingsChanged(changed) = event else { continue };
            if changed.keys.iter().any(|key| key == "max_entries") {
                log::log(Level::Info, &format!("settings changed: {:?}", changed.keys));
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
import { log } from '@ilium/plugin-sdk/host/log';

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag !== 'settings-changed') continue;
      if (event.val.keys.includes('max_entries')) log('info', `settings changed: ${event.val.keys.join(', ')}`);
    }
    return [];
  },
});
```

The manifest entry for the section: `{ "kind": "section", "tab": "terminal", "id": "capture", "title": "Capture", "scope": "client", "rows": [ { "type": "number", "key": "max_entries", "title": "Maximum entries", "default": 500 } ] }`.

### Errors and edge cases

| Situation | Outcome | What the plugin does |
|---|---|---|
| Section without the grant | install refused, `manifest.settings-target-grant` | Add `settings.contribute:<tab>`. |
| Unknown tab id | `manifest.settings-target-unknown` | Use a tab id from the reference. |
| Closed kernel tab | `manifest.settings-target-closed` | The tab does not accept sections. |
| Too many sections or rows | `manifest.settings-limit` | Reduce rows or sections. |
| Plugin disabled | rows dimmed and read-only, values kept | Nothing. |
| Grant declined by the user | section not drawn | Nothing. |

The values of a section are stored in `[plugins."<plugin-id>"]` and survive disabling and uninstalling until the user purges them.

### What the user sees

The review dialog lists the grant when a package is installed or first activated. The review dialog uses the wording `Adds settings to the <Tab label> tab`.

<figure>

<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.clipboardhistory 0.1.0 by Example Ltd                          │
│                                                                            │
│ <span class="t-hi">Client side</span>                                                                │
│ [x] Adds settings to the Terminal tab <span class="t-dim">…………………</span> settings.contribute:terminal │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>

<figcaption>The review dialog for a package that requests settings.contribute:terminal, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar.</figcaption>
</figure>

Listing the grants of an installed plugin from the terminal:

<figure>

<pre class="term"><span class="t-prompt">$</span> ilium plugin grants com.example.clipboardhistory
<span class="t-dim">Grant                             Risk     State</span>
settings.contribute:terminal      normal   <span class="t-ok">granted</span>
</pre>

<figcaption>Output of the grant listing for this plugin. A high-risk grant that the user left unchecked shows as declined.</figcaption>
</figure>

The user can change the grant at any time in Settings > Plugins > com.example.clipboardhistory > Capabilities. A revocation re-activates the plugin with the reduced grant set, and the stubs of the declined imports return `not-permitted`.

### Related pages

- [settings.read-kernel](/pages/capability-settings-read-kernel/).
- [settings.read-all and write-all](/pages/capability-settings-all/).
- [Settings](/pages/api-settings/).
- [Settings screens](/pages/settings-screens/).
