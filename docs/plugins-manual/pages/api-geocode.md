---
layout: page
title: geocode and location picker
permalink: /pages/api-geocode/
---

## geocode and location picker

Geocoding turns a place name into coordinates and back. The host provides it as a service, so a plugin never needs the `http` grant for it: the host owns the provider (Photon, with Nominatim as fallback), the user agent, the attribution string and the rate limit. The same service feeds the `location-picker` widget, a world map the user clicks, and `location` settings rows.

### What you need

- **Grant.** `location.geocode` on the client side. The first call asks the user once for consent to send place names to the provider; without it calls return `not-permitted`.
- **Language.** Rust `ilium_plugin_sdk::host::geocode`, TypeScript `@ilium/plugin-sdk/host/geocode`.

```json
"capabilities": { "client": ["location.geocode"] }
```

### Interface

```wit
// Excerpt of ilium:host/geocode in reference-wit
record geocode-query { text: string, near: option<geo-point>, limit: u8, language: option<string> }   // text 2..200; limit 1..8
search: func(q: geocode-query) -> result<request-id, plugin-error>;
reverse: func(point: geo-point) -> result<request-id, plugin-error>;        // at most 1 result
take-places: func(request: request-id) -> option<list<geo-place>>;
```

| Function | Rust | TypeScript | Returns |
|---|---|---|---|
| `search` | `geocode::search(&GeocodeQuery)` | `search(q)` | request id; places arrive with `take_places` |
| `reverse` | `geocode::reverse(GeoPoint)` | `reverse(point)` | request id |
| `take-places` | `geocode::take_places(request)` | `takePlaces(request)` | `Option<Vec<GeoPlace>>` |

Results are cached for 24 hours by query text. Up to 8 places are returned. An empty list is a valid answer, not an error. `take-places` has no error channel: every failure in the Errors table below (except the synchronous ones `not-permitted`, `invalid-argument` and `rate-limited` returned by `search` or `reverse` themselves) arrives in the `request-completed` event of that request as an error outcome, and `take-places` then returns `none` (`None`, `undefined`). `some` with an empty list means the provider answered with no place. A second call after the first take also returns `none`. Rate: 1 request per second per plugin.

### The picker and the row

The `location-picker` widget draws the world as a Braille map (each cell holds a 2 by 4 dot pattern) and returns a `geo(geo-point)` value when the user clicks or searches. A `location` settings row embeds the same picker in a dialog and stores the point; the plugin may show it without ever seeing the coordinates as text.

<figure>
<pre class="term">╭  ≡ ● · Observer location ──────────────────╮
│ Search: Lisb▏                              │
│ ⠀⠀⣀⣤⣤⣀⠀⠀⠀⠀⠀⠀⣠⣴⣶⣦⣄⠀⠀⠀⠀⠀⣀⣤⣤⣀⠀⠀⠀⠀⠀            │
│ ⠀⣾⣿⣿⣿⣿⣷⠀⠀⠀⢀⣿⣿⣿⣿⣿⣿⡀⠀⠀⣼⣿⣿⣿⣿⣧⠀⠀⠀⠀             │
│ ⠀⠙⠿⣿⣿⠿⠋⠀⠀⠀⠀⠛⠿⣿⣿⠿⠛⠀⠀⠀⠈⠻⣿⣿⠟⠁⠀⠀⠀⠀             │
│ ▸ Lisbon, Portugal                         │
│   Lisburn, Northern Ireland                │
│ Esc cancel · Enter apply                   │
╰────────────────────────────────────────────╯
</pre>
<figcaption>The picker with a search field and results. The map uses the same Braille cells as the client's world map; the continent shapes here are schematic.</figcaption>
</figure>

<figure>
<svg viewBox="0 0 440 110" role="img" aria-label="Geocode path">
<title>Geocode path</title>
<desc>A plugin, the picker widget or a location row calls the host geocode service, which uses its cache and the provider and returns places.</desc>
<defs><marker id="gc" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0,0 L10,5 L0,10 z" fill="currentColor"/></marker></defs>
<rect x="8" y="10" width="140" height="40" rx="6" fill="none" stroke="currentColor"/><text x="78" y="34" font-size="12" text-anchor="middle" fill="currentColor">plugin, picker, row</text>
<line x1="148" y1="30" x2="190" y2="30" stroke="currentColor" marker-end="url(#gc)"/>
<rect x="190" y="10" width="110" height="40" rx="6" fill="none" stroke="currentColor"/><text x="245" y="34" font-size="12" text-anchor="middle" fill="currentColor">host service</text>
<line x1="300" y1="30" x2="340" y2="30" stroke="currentColor" marker-end="url(#gc)"/>
<rect x="340" y="10" width="92" height="40" rx="6" fill="none" stroke="currentColor"/><text x="386" y="34" font-size="12" text-anchor="middle" fill="currentColor">provider</text>
<text x="245" y="82" font-size="11" text-anchor="middle" fill="currentColor">consent once, cache 24 h, 1 request per second</text>
</svg>
<figcaption>All three callers share consent, cache and rate limit.</figcaption>
</figure>

### Examples

**Rust**

```rust
use ilium_plugin_sdk::host::geocode::{self, GeocodeQuery};

fn find(text: &str) -> Result<RequestId, PluginError> {
    geocode::search(&GeocodeQuery { text: text.into(), near: None, limit: 5, language: Some("en".into()) })
}
fn on_completed(request: RequestId) -> Vec<String> {
    geocode::take_places(request).unwrap_or_default().into_iter().map(|place| place.name).collect()
}
```

**TypeScript**

```ts
import { search, takePlaces } from '@ilium/plugin-sdk/host/geocode';

export const find = (text: string) => search({ text, near: undefined, limit: 5, language: 'en' });
export const onCompleted = (request: bigint): string[] => (takePlaces(request) ?? []).map((place) => place.name);
```

### Errors

| Case | Error |
|---|---|
| No consent | `not-permitted` |
| Over 1 request per second | `rate-limited` with `retry-after-ms` |
| Text outside 2 to 200 characters, limit outside 1 to 8 | `invalid-argument` |
| Provider failure, or 10 s without answer | `network`, `timed-out`, delivered in `request-completed` (not by `take-places`) |

The privacy notice about sending place names to the provider is shown by the host, not the plugin. See [location](/pages/api-location/).

### See also

- [location](/pages/api-location/), [catalogs](/pages/api-catalogs/), [Settings](/pages/api-settings/), [Capabilities reference](/pages/reference-capabilities/).
