# Capability: keymap.read

## keymap.read

The `keymap.read` capability lets a client plugin read the user's key bindings and turn a chord into the label the user sees, such as `Ctrl+Shift+K`. It is read-only: a plugin never changes a binding (decision D13). It also delivers `keymap-changed` events, so hints stay correct after a remap.

**You will need:**

- A `keymap.read` grant on the client side. No argument.
- Command ids to look up, as in [Commands](/pages/api-commands/).

<figure>
<svg viewBox="0 0 420 252" role="img" aria-label="A plugin reads bindings, formats chords and refreshes on keymap-changed">
  <defs><marker id="arr" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto"><path d="M0,0 L8,4 L0,8 z" fill="currentColor"/></marker></defs>
  <rect x="20" y="8" width="380" height="44" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="210" y="27" text-anchor="middle" fill="currentColor" font-size="12">Plugin calls keymap.bindings()</text>
  <text x="210" y="43" text-anchor="middle" fill="currentColor" font-size="10">returns every binding</text>
  <line x1="210" y1="52" x2="210" y2="72" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="72" width="380" height="44" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="210" y="91" text-anchor="middle" fill="currentColor" font-size="12">Plugin calls chord-label(chord)</text>
  <text x="210" y="107" text-anchor="middle" fill="currentColor" font-size="10">user-visible form</text>
  <line x1="210" y1="116" x2="210" y2="136" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="136" width="380" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="210" y="155" text-anchor="middle" fill="currentColor" font-size="12">Host sends keymap-changed after a remap</text>
  <text x="210" y="171" text-anchor="middle" fill="currentColor" font-size="10">re-read the bindings</text>
  <line x1="210" y1="180" x2="210" y2="200" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="200" width="180" height="40" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="110" y="224" text-anchor="middle" fill="currentColor" font-size="11">list of bindings</text>
  <rect x="220" y="200" width="180" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="310" y="224" text-anchor="middle" fill="currentColor" font-size="11">not-permitted</text>
</svg>
<figcaption>Hints are built from live bindings, so they follow the user's remaps.</figcaption>
</figure>

### Grant syntax

| Grant | Valid | Reason |
|---|---|---|
| `keymap.read` | yes | the only form |
| `keymap.write` | no | does not exist; bindings are never writable by plugins |
| `keymap.read:global` | no | no argument |

Normal risk. Bindings reveal which keys the user uses, not what the user types.

### What it unlocks

| Name | Kind | Meaning |
|---|---|---|
| `keymap.bindings` | function | Returns `list<binding>`: `{ command, chords, context, is-default }`. |
| `keymap.chord-label` | function | `chord-label(chord) -> string`, for example `Ctrl+Shift+K`. |
| `keymap-changed` | event | Sent when the user changes the keymap. |

Intercepting keys is a different grant: see [keys.intercept](/pages/capability-keys-intercept/) and [Keys](/pages/api-keys/).

### Manifest

The grant goes into `manifest.json` under `capabilities`, in the list of the side that uses it (client only).

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.keyhints",
  "capabilities": {
    "client": [
      "keymap.read"
    ]
  }
}
```

### Worked example

This plugin looks up the first chord for its own command and logs the user-visible label.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::Event;
use ilium_plugin_sdk::types::intents::{Intent, Submission};
use ilium_plugin_sdk::host::keymap;

#[ilium_plugin(side = "client")]
#[derive(Default)]
struct KeyHints;

impl Handler for KeyHints {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::CommandInvoked(invoked) = event else { continue };
            if invoked.command != "com.example.keyhints/show" { continue; }
            let label = keymap::bindings()
                .into_iter()
                .find(|binding| binding.command == "com.example.keyhints/show")
                .and_then(|binding| binding.chords.first().map(|chord| keymap::chord_label(chord)));
            log::log(Level::Info, &format!("shortcut: {}", label.unwrap_or_else(|| "none".into())));
        }
        Vec::new()
    }
}
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import type { Event, Submission } from '@ilium/plugin-sdk/types';
import { bindings, chordLabel } from '@ilium/plugin-sdk/host/keymap';
import { log } from '@ilium/plugin-sdk/host/log';

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag !== 'command-invoked' || event.val.command !== 'com.example.keyhints/show') continue;
      const binding = bindings().find((b) => b.command === 'com.example.keyhints/show');
      const chord = binding?.chords[0];
      log('info', `shortcut: ${chord ? chordLabel(chord) : 'none'}`);
    }
    return [];
  },
});
```

Call `bindings()` again on every `keymap-changed` event instead of caching labels for a long time.

### Errors and edge cases

| Situation | Outcome | What the plugin does |
|---|---|---|
| Grant missing or declined | `not-permitted` | Show no shortcut hint. |
| Command has no binding | empty chord list | Show no hint. |
| Server side requests the grant | install refused, the grant is client-only | Move the code to the client side. |
| Unknown chord string | `invalid-argument` | Use chords taken from `bindings()`. |
| Grant revoked later | `revoked` | Drop cached labels. |
| Keymap changes while reading | a `keymap-changed` event follows | Re-read. |

Bindings that conflict with another plugin's chord follow the keymap conflict rule; the user's remap always wins.

### What the user sees

The review dialog lists the grant when a package is installed or first activated. The line states that the plugin reads key bindings.

<figure>

<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.keyhints 0.1.0 by Example Ltd                                  │
│                                                                            │
│ <span class="t-hi">Client side</span>                                                                │
│ [x] Reads your key bindings <span class="t-dim">…………………………………………………………………………………………</span> keymap.read │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>

<figcaption>The review dialog for a package that requests keymap.read, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar.</figcaption>
</figure>

Listing the grants of an installed plugin from the terminal:

<figure>

<pre class="term"><span class="t-prompt">$</span> ilium plugin grants com.example.keyhints
<span class="t-dim">Grant                             Risk     State</span>
keymap.read                       normal   <span class="t-ok">granted</span>
</pre>

<figcaption>Output of the grant listing for this plugin. A high-risk grant that the user left unchecked shows as declined.</figcaption>
</figure>

The user can change the grant at any time in Settings > Plugins > com.example.keyhints > Capabilities. A revocation re-activates the plugin with the reduced grant set, and the stubs of the declined imports return `not-permitted`.

### Related pages

- [keys.intercept](/pages/capability-keys-intercept/).
- [commands.invoke](/pages/capability-commands-invoke/).
- [Keys](/pages/api-keys/).
- [Commands](/pages/api-commands/).
