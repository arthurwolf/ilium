---
layout: page
title: "Capability: open-external"
permalink: /pages/capability-open-external/
---

## open-external

The `open-external` capability lets a plugin ask the host to open something outside Ilium: a web address in the default browser, a file with its desktop handler, a mail link, or an Ilium settings page with an `ilium://settings/...` link. The plugin never starts the handler itself. The host checks the target, and it asks the user to confirm the first open for each target origin (SEC-043). The origin of a web target is scheme, host and port; of a `mailto` target the scheme `mailto`; of a file target the granted scope root that contains the path, so one confirmation covers every file below that root.

**You will need:**

- An `open-external` grant. No argument.
- A target: `url(string)` for `http`, `https` or `mailto`, `file(fs-path)`, or `settings(string)`.

<figure>
<svg viewBox="0 0 420 252" role="img" aria-label="An open-external intent is checked, may ask the user once per origin, then starts the desktop handler">
  <defs><marker id="arr" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto"><path d="M0,0 L8,4 L0,8 z" fill="currentColor"/></marker></defs>
  <rect x="20" y="8" width="380" height="44" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="210" y="27" text-anchor="middle" fill="currentColor" font-size="12">Plugin submits open-external(target)</text>
  <text x="210" y="43" text-anchor="middle" fill="currentColor" font-size="10">http, https, mailto, file or settings</text>
  <line x1="210" y1="52" x2="210" y2="72" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="72" width="380" height="44" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="210" y="91" text-anchor="middle" fill="currentColor" font-size="12">Host checks the scheme and the origin</text>
  <text x="210" y="107" text-anchor="middle" fill="currentColor" font-size="10">first open per origin asks the user</text>
  <line x1="210" y1="116" x2="210" y2="136" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="136" width="380" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="210" y="155" text-anchor="middle" fill="currentColor" font-size="12">Desktop handler starts</text>
  <text x="210" y="171" text-anchor="middle" fill="currentColor" font-size="10">ok means the handler started</text>
  <line x1="210" y1="180" x2="210" y2="200" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="200" width="180" height="40" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="110" y="224" text-anchor="middle" fill="currentColor" font-size="11">ok</text>
  <rect x="220" y="200" width="180" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="310" y="224" text-anchor="middle" fill="currentColor" font-size="11">refused(not-permitted, invalid-argument)</text>
</svg>
<figcaption>The user confirms the first open for each origin. Later opens to the same origin run without a prompt.</figcaption>
</figure>

### Grant syntax

| Grant | Valid | Reason |
|---|---|---|
| `open-external` | yes | the only form |
| `open-external:https` | no | no argument; the schemes are fixed by the host |
| `open.external` | no | unknown grant name |

Normal risk, because the host confirms the first open per origin and restricts the schemes. A `file` target opens with the desktop default handler only and never executes it.

### What it unlocks

| Name | Kind | Meaning |
|---|---|---|
| `open-external` | intent | `target`: `url(string)` (`http`, `https`, `mailto` only), `file(fs-path)` or `settings(string)` for `ilium://settings/<tab-id>#<key>`. Outcome `ok` after the handler started. |

The settings link form is the supported way to point a user at a kernel setting; see [settings.read-kernel](/pages/capability-settings-read-kernel/). See [Intents](/pages/api-intents/).

### Manifest

The grant goes into `manifest.json` under `capabilities`, in the list of the side that uses it (server or client).

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.links",
  "capabilities": {
    "client": [
      "open-external"
    ]
  }
}
```

### Worked example

This plugin opens a status page when the command `com.example.links/status` runs, and a kernel setting when `com.example.links/scrollback` runs.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::Event;
use ilium_plugin_sdk::types::intents::{Intent, Submission};
use ilium_plugin_sdk::types::intents::{ExternalTarget, OpenExternalSpec};

#[ilium_plugin(side = "client")]
#[derive(Default)]
struct Links;

impl Handler for Links {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            let Event::CommandInvoked(invoked) = event else { continue };
            if invoked.command != "com.example.links/status" { continue; }
            submissions.push(Submission::new(Intent::OpenExternal(OpenExternalSpec {
                target: ExternalTarget::Url("https://status.example.com".into()),
            })));
            submissions.push(Submission::new(Intent::OpenExternal(OpenExternalSpec {
                target: ExternalTarget::Settings("ilium://settings/terminal#scrollback".into()),
            })));
        }
        submissions
    }
}
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import type { Event, Submission } from '@ilium/plugin-sdk/types';

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    for (const event of batch) {
      if (event.tag !== 'command-invoked' || event.val.command !== 'com.example.links/status') continue;
      submissions.push(
        { intent: { tag: 'open-external', val: { target: { tag: 'url', val: 'https://status.example.com' } } }, correlation: 1n, dedupKey: undefined },
        { intent: { tag: 'open-external', val: { target: { tag: 'settings', val: 'ilium://settings/terminal#scrollback' } } }, correlation: 2n, dedupKey: undefined },
      );
    }
    return submissions;
  },
});
```

Both submissions are independent. If the user declines the first open of `status.example.com`, only that one is `refused(not-permitted)`.

### Errors and edge cases

| Situation | Outcome | What the plugin does |
|---|---|---|
| Grant missing or declined | `refused(not-permitted)` | Show the address as text the user can copy. |
| User declines the origin confirmation | `refused(not-permitted)` | Do not ask again in a loop. |
| Scheme other than http, https or mailto in a `url` | `refused(not-permitted)` ([SEC-043]) | Use `file` or `settings` targets for those. |
| Settings link to an unknown tab | `refused(not-found)` | Use a tab id from the settings reference. |
| No desktop handler | `refused(unsupported)` | Show the target as text. |
| More than 10 opens in 60 seconds for one plugin (sliding window) | `refused(rate-limited)` with `retry-after-ms` | Open once per user action. |

`ok` means that the handler started, not that the page loaded.

### What the user sees

The review dialog lists the grant when a package is installed or first activated. The line says that the plugin can open links and files outside Ilium.

<figure>

<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.links 0.1.0 by Example Ltd                                     │
│                                                                            │
│ <span class="t-hi">Client side</span>                                                                │
│ [x] Opens links and files outside Ilium <span class="t-dim">……………………………………………………</span> open-external │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>

<figcaption>The review dialog for a package that requests open-external, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar.</figcaption>
</figure>

Listing the grants of an installed plugin from the terminal:

<figure>

<pre class="term"><span class="t-prompt">$</span> ilium plugin grants com.example.links
<span class="t-dim">Grant                             Risk     State</span>
open-external                     normal   <span class="t-ok">granted</span>
</pre>

<figcaption>Output of the grant listing for this plugin. A high-risk grant that the user left unchecked shows as declined.</figcaption>
</figure>

The user can change the grant at any time in Settings > Plugins > com.example.links > Capabilities. A revocation re-activates the plugin with the reduced grant set, and the stubs of the declined imports return `not-permitted`.

### Related pages

- [notify.toast](/pages/capability-notify-toast/) for toast actions.
- [http](/pages/capability-http/) for plugin-owned requests.
- [Intents](/pages/api-intents/).
- [Security](/pages/security/).

### Security

**[SEC-043]** `open-external` accepts only `https`, `http`, `mailto` and file paths inside granted scopes; each origin needs consent (SEC-043). See [Network egress](/pages/reference-network-egress/).
