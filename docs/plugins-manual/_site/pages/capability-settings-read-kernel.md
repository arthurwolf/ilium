# Capability: settings.read-kernel:<kernel-tab-id>

## settings.read-kernel:<kernel-tab-id>

The `settings.read-kernel:<kernel-tab-id>` capability lets a plugin read the values of one built-in Settings tab, for example the scrollback budget of the Terminal tab. The read is read-only (decision D13), secrets are always returned as `<redacted>`, and the plugin is told when a value changes. The argument is required and names exactly one kernel tab; there is no wildcard. The grant is normal risk for most tabs and high risk (**H**) for the `api` tab.

**You will need:**

- A `settings.read-kernel:<kernel-tab-id>` grant with one tab id from the settings reference (for example `terminal`).
- To change a value, link the user to it with [open-external](/pages/capability-open-external/): only the single owner plugin of a tab writes it, with `settings.write-kernel:<tab>` (see [kernel settings](/pages/api-kernel-settings-and-keymap/#writing-a-kernel-tab)).

<figure>
<svg viewBox="0 0 420 252" role="img" aria-label="A kernel tab is read as JSON with secrets redacted, and changes are reported as events">
  <defs><marker id="arr" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto"><path d="M0,0 L8,4 L0,8 z" fill="currentColor"/></marker></defs>
  <rect x="20" y="8" width="380" height="44" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="210" y="27" text-anchor="middle" fill="currentColor" font-size="12">Plugin calls kernel-settings.read("terminal")</text>
  <text x="210" y="43" text-anchor="middle" fill="currentColor" font-size="10">argument must match the grant</text>
  <line x1="210" y1="52" x2="210" y2="72" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="72" width="380" height="44" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="210" y="91" text-anchor="middle" fill="currentColor" font-size="12">Host returns a JSON object of the tab values</text>
  <text x="210" y="107" text-anchor="middle" fill="currentColor" font-size="10">secrets are "&lt;redacted&gt;"</text>
  <line x1="210" y1="116" x2="210" y2="136" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="136" width="380" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="210" y="155" text-anchor="middle" fill="currentColor" font-size="12">Host sends kernel-settings-changed on edits</text>
  <text x="210" y="171" text-anchor="middle" fill="currentColor" font-size="10">tab and changed keys</text>
  <line x1="210" y1="180" x2="210" y2="200" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="200" width="180" height="40" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="110" y="224" text-anchor="middle" fill="currentColor" font-size="11">JSON string of values</text>
  <rect x="220" y="200" width="180" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="310" y="224" text-anchor="middle" fill="currentColor" font-size="11">not-permitted, not-found</text>
</svg>
<figcaption>The read is one-way. A plugin that does not own the tab never writes a kernel value.</figcaption>
</figure>

### Grant syntax

| Grant | Valid | Reason |
|---|---|---|
| `settings.read-kernel:terminal` | yes | one kernel tab id |
| `settings.read-kernel` | no | argument required (`manifest.grant-argument-missing`) |
| `settings.read-kernel:*` | no | no wildcard (`manifest.grant-wildcard`) |
| `settings.read-kernel:api` | yes, high risk | the api tab holds credentials |

Normal risk for ordinary tabs and high risk for `api`. The grant is independent of `settings.contribute:<tab>` (which adds sections) and of `settings.read-all` (which reads other plugins' values).

### What it unlocks

| Name | Kind | Meaning |
|---|---|---|
| `kernel-settings.read` | function | `read(tab) -> result<string, plugin-error>`. JSON object of that tab; secrets are `"<redacted>"`. |
| `kernel-settings-changed` | event | `{ tab, keys }`. |

The tab ids are listed in [Settings](/pages/api-settings/) and [Settings screens](/pages/settings-screens/).

### Manifest

The grant goes into `manifest.json` under `capabilities`, in the list of the side that uses it (server or client).

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.scrollinfo",
  "capabilities": {
    "client": [
      "settings.read-kernel:terminal"
    ]
  }
}
```

### Worked example

This plugin reads the Terminal tab when the command `com.example.scrollinfo/show` runs and logs the scrollback value. The key name is illustrative; read the keys from the tab reference.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::Event;
use ilium_plugin_sdk::types::intents::{Intent, Submission};
use ilium_plugin_sdk::host::kernel_settings;

#[ilium_plugin(side = "client")]
#[derive(Default)]
struct ScrollInfo;

impl Handler for ScrollInfo {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::CommandInvoked(invoked) = event else { continue };
            if invoked.command != "com.example.scrollinfo/show" { continue; }
            match kernel_settings::read("terminal") {
                Ok(json) => log::log(Level::Info, &json),
                Err(error) => log::log(Level::Warn, &format!("read failed: {error:?}")),
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
import { read } from '@ilium/plugin-sdk/host/kernel-settings';
import { log } from '@ilium/plugin-sdk/host/log';

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag !== 'command-invoked' || event.val.command !== 'com.example.scrollinfo/show') continue;
      try {
        log('info', read('terminal'));
      } catch (error) {
        log('warn', `read failed: ${String(error)}`);
      }
    }
    return [];
  },
});
```

Parse the JSON defensively: a key that the kernel removes disappears from the object.

### Errors and edge cases

| Situation | Outcome | What the plugin does |
|---|---|---|
| Tab not granted | `not-permitted` | Request only the tabs you need. |
| Unknown tab id | install refused, or `not-found` at run time | Use an id from the reference. |
| Missing argument in the manifest | `manifest.grant-argument-missing` | Add the tab id. |
| Wildcard argument | `manifest.grant-wildcard` | Name one tab. |
| Secret value | returned as `"<redacted>"` | Never prompt for the real value. |
| Grant revoked later | `revoked`, then `not-permitted` | Stop reading. |

To let the user change a value, submit `open-external` with `ilium://settings/<tab-id>#<key>`.

### What the user sees

The review dialog lists the grant when a package is installed or first activated. The line names the tab. For the api tab it is high risk and unchecked.

<figure>

<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.scrollinfo 0.1.0 by Example Ltd                                │
│                                                                            │
│ <span class="t-hi">Client side</span>                                                                │
│ [x] Reads the Terminal settings <span class="t-dim">………………………………</span> settings.read-kernel:terminal │
│ <span class="t-warn">[high risk] [ ] Reads the Api settings</span> <span class="t-dim">…………………………</span> <span class="t-warn">settings.read-kernel:api</span> │
│                                                                            │
│ <span class="t-warn">[high risk] marks a high-risk grant. It starts unchecked and is</span>            │
│ <span class="t-warn">always asked for, even for bundled plugins.</span>                                │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>

<figcaption>The review dialog for a package that requests settings.read-kernel:terminal, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar.</figcaption>
</figure>

Listing the grants of an installed plugin from the terminal:

<figure>

<pre class="term"><span class="t-prompt">$</span> ilium plugin grants com.example.scrollinfo
<span class="t-dim">Grant                             Risk     State</span>
settings.read-kernel:terminal     normal   <span class="t-ok">granted</span>
settings.read-kernel:api          high     <span class="t-warn">declined</span>
</pre>

<figcaption>Output of the grant listing for this plugin. A high-risk grant that the user left unchecked shows as declined.</figcaption>
</figure>

The user can change the grant at any time in Settings > Plugins > com.example.scrollinfo > Capabilities. A revocation re-activates the plugin with the reduced grant set, and the stubs of the declined imports return `not-permitted`.

### Related pages

- [settings.contribute](/pages/capability-settings-contribute/).
- [settings.read-all and write-all](/pages/capability-settings-all/).
- [open-external](/pages/capability-open-external/).
- [Settings](/pages/api-settings/).

### Security

Kernel settings tabs are read only through this grant; a kernel secret is returned as `<redacted>` ([SEC-244]).
