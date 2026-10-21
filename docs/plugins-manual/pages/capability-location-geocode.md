---
layout: page
title: "Capability: location.geocode"
permalink: /pages/capability-location-geocode/
---

## location.geocode

The `location.geocode` capability lets a plugin turn a place name into coordinates and coordinates into a place name. The host owns the provider (Photon, with Nominatim as fallback), the user agent, the attribution and the rate limit. The first use asks the user once for consent to send place names to the provider; without it the calls return `not-permitted`. The same service backs the `location-picker` widget and `location` rows, so a plugin does not need an [http](/pages/capability-http/) grant for geocoding.

**You will need:**

- A `location.geocode` grant on the client or scene side.
- The user's one-time consent to send place names to the provider.

<figure>
<svg viewBox="0 0 420 252" role="img" aria-label="A geocode search needs consent once, is rate limited and answers with places">
  <defs><marker id="arr" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto"><path d="M0,0 L8,4 L0,8 z" fill="currentColor"/></marker></defs>
  <rect x="20" y="8" width="380" height="44" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="210" y="27" text-anchor="middle" fill="currentColor" font-size="12">Plugin calls geocode.search(query) or reverse(point)</text>
  <text x="210" y="43" text-anchor="middle" fill="currentColor" font-size="10">text 2 to 200 characters, limit 1 to 8</text>
  <line x1="210" y1="52" x2="210" y2="72" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="72" width="380" height="44" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="210" y="91" text-anchor="middle" fill="currentColor" font-size="12">Host asks consent once, then queries the provider</text>
  <text x="210" y="107" text-anchor="middle" fill="currentColor" font-size="10">1 request per second per plugin</text>
  <line x1="210" y1="116" x2="210" y2="136" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="136" width="380" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="210" y="155" text-anchor="middle" fill="currentColor" font-size="12">Plugin calls take-places</text>
  <text x="210" y="171" text-anchor="middle" fill="currentColor" font-size="10">results cached for 24 hours</text>
  <line x1="210" y1="180" x2="210" y2="200" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="200" width="180" height="40" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="110" y="224" text-anchor="middle" fill="currentColor" font-size="11">list of geo-place</text>
  <rect x="220" y="200" width="180" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="310" y="224" text-anchor="middle" fill="currentColor" font-size="11">not-permitted, rate-limited</text>
</svg>
<figcaption>The host rate-limits and caches so several plugins cannot overload the provider.</figcaption>
</figure>

### Grant syntax

| Grant | Valid | Reason |
|---|---|---|
| `location.geocode` | yes | the only form |
| `location.geocode:photon` | no | the provider is chosen by the host |
| `location.observer` | separate | reads the device location; high risk |

Normal risk. Place names leave the machine only after the user's consent. The grant does not give the user's own position; that is [location.observer](/pages/capability-location-observer/). Kernel settings tabs never need this grant.

### What it unlocks

| Name | Kind | Meaning |
|---|---|---|
| `geocode.search` | function | `search(geocode-query { text, near?, limit, language? }) -> result<request-id, plugin-error>`. |
| `geocode.reverse` | function | `reverse(point) -> result<request-id, plugin-error>`; at most one result. |
| `geocode.take-places` | function | Returns `option<list<geo-place>>`. |
| `location-picker` search | widget field | The search field of the widget and of `location` rows in own tabs. |

See [Location](/pages/api-location/) and [Media and embedded widgets](/pages/widgets-media-and-embedded/).

### Manifest

The grant goes into `manifest.json` under `capabilities`, in the list of the side that uses it (client or scene).

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.places",
  "capabilities": {
    "client": [
      "location.geocode"
    ]
  }
}
```

### Worked example

This plugin searches for Lyon, limited to five results, in French. The search returns a request id at once. The places are read with `take-places` when the `request-completed` event for that id arrives.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::Event;
use ilium_plugin_sdk::types::intents::{Intent, Submission};
use ilium_plugin_sdk::host::geocode;
use ilium_plugin_sdk::host::geocode::GeocodeQuery;

#[ilium_plugin(side = "client")]
#[derive(Default)]
struct Places {
    pending: Option<RequestId>,
}

impl Handler for Places {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            match event {
                Event::CommandInvoked(invoked) if invoked.command == "com.example.places/lookup" => {
                    let query = GeocodeQuery { text: "Lyon".into(), near: None, limit: 5, language: Some("fr".into()) };
                    match geocode::search(query) {
                        Ok(request_id) => self.pending = Some(request_id),
                        Err(error) => log::log(Level::Warn, &format!("geocode refused: {error:?}")),
                    }
                }
                Event::RequestCompleted(result) => {
                    let Some(request_id) = self.pending else { continue };
                    if result.request != request_id { continue; }
                    self.pending = None;
                    match geocode::take_places(request_id) {
                        Some(places) => log::log(Level::Info, &format!("{} places", places.len())),
                        None => log::log(Level::Warn, "geocode gave no places"),
                    }
                }
                _ => {}
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
import { search, takePlaces } from '@ilium/plugin-sdk/host/geocode';
import { log } from '@ilium/plugin-sdk/host/log';

let pending: bigint | undefined;

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  async onEvents(batch: Event[]): Promise<Submission[]> {
    for (const event of batch) {
      if (event.tag === 'command-invoked' && event.val.command === 'com.example.places/lookup') {
        try {
          pending = search({ text: 'Lyon', near: undefined, limit: 5, language: 'fr' });
        } catch (error) {
          log('warn', `geocode refused: ${String(error)}`);
        }
      } else if (event.tag === 'request-completed' && pending !== undefined
        && event.val.request.tag === 'call' && event.val.request.val === pending) {
        const requestId = pending;
        pending = undefined;
        const places = takePlaces(requestId);
        if (places) log('info', `${places.length} places`);
        else log('warn', 'geocode gave no places');
      }
    }
    return [];
  },
});
```

Show the provider attribution that the host supplies next to results, as the provider terms require.

### Errors and edge cases

| Situation | Outcome | What the plugin does |
|---|---|---|
| Grant missing or declined | `not-permitted` | Let the user type coordinates instead. |
| User refused the one-time consent | `not-permitted` | Do not ask again in a loop. |
| More than 1 request per second | `rate-limited` with `retry-after-ms` | Wait for the delay. |
| Provider unreachable | `network` | Retry later. |
| Provider slower than 10 s | `timed-out` | Retry later. |
| `text` shorter than 2 or longer than 200 characters | `invalid-argument` | Fix the input before calling. |
| `limit` below 1 or above 8 | `invalid-argument` (not clamped; [WIT](/pages/reference-wit/)) | Pass a limit from 1 to 8. |

Results are cached for 24 hours by query text, so repeating a search is free.

### What the user sees

The review dialog lists the grant when a package is installed or first activated. The line states that place names are sent to a lookup service.

<figure>

<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.places 0.1.0 by Example Ltd                                    │
│                                                                            │
│ <span class="t-hi">Client side</span>                                                                │
│ [x] Looks up places by name <span class="t-dim">……………………………………………………………………………</span> location.geocode │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>

<figcaption>The review dialog for a package that requests location.geocode, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar.</figcaption>
</figure>

Listing the grants of an installed plugin from the terminal:

<figure>

<pre class="term"><span class="t-prompt">$</span> ilium plugin grants com.example.places
<span class="t-dim">Grant                             Risk     State</span>
location.geocode                  normal   <span class="t-ok">granted</span>
</pre>

<figcaption>Output of the grant listing for this plugin. A high-risk grant that the user left unchecked shows as declined.</figcaption>
</figure>

The user can change the grant at any time in Settings > Plugins > com.example.places > Capabilities. A revocation re-activates the plugin with the reduced grant set, and the stubs of the declined imports return `not-permitted`.

### Related pages

- [location.observer](/pages/capability-location-observer/).
- [http](/pages/capability-http/).
- [Location](/pages/api-location/).
- [Widgets: media and embedded](/pages/widgets-media-and-embedded/).

### Security

The geocode request is host-initiated egress; see the inventory in [Network egress](/pages/reference-network-egress/).
