# location

## location

The `location` import returns the observer position that the user sets in Settings > Location. A plugin asks for it with one synchronous call and gets back a latitude and longitude pair, or nothing when the user has not set a position. The import has one function and no events.

The position is the user's own setting. It is not read from a device: the host uses no GPS, no operating system location service and no IP geolocation . The ambient scenes read the same setting, so a plugin and a built-in scene always agree on where the user is.

A position reveals where the user is. That is why the grant is high risk (H), and why the page recommends rounding the value and keeping it on the device.

### What you need

- **Grant:** `location.observer`, risk high (H). It takes no argument and grants access to `location.current` only.
- **Side:** client plugins and scene plugins (the animation world). The server world has no location import.
- **Language:** Rust with the `ilium-plugin-sdk` crate, or TypeScript with the `@ilium/plugin-sdk` package. Both SDKs need no extra setup for this import.
- **Manifest:** list `location.observer` in the plugin's capabilities. A plugin that does not list it gets a stub that returns nothing; see [Edge cases](#edge-cases-and-errors).
- **User approval:** the user must check the grant at install (T2 and T3 packages) or accept it at first activation (T1 bundled plugins). See [What the user sees](#what-the-user-sees).

### Surface

Import paths: Rust `ilium_plugin_sdk::host::location`, TypeScript `@ilium/plugin-sdk/host/location`.

| Name | Rust | TypeScript | Returns | Meaning |
|---|---|---|---|---|
| `current` | `location::current()` | `location.current()` | `Option<(f64, f64)>`, or `[number, number]` or `undefined` | The observer position as (latitude, longitude), or nothing when no position is set. |

The return value is a tuple of two numbers:

| Element | Name | Unit | Range |
|---|---|---|---|
| 0 | latitude | decimal degrees, WGS 84 | -90 to 90, north is positive |
| 1 | longitude | decimal degrees, WGS 84 | -180 to 180, east is positive |

The import does not expose an accuracy value, a timestamp, an age, the source of the value or a change event. A plugin that needs the latest value calls `current` again.

The WIT form, from `ilium:host/location@0.1.0`:

```wit
// Excerpt of ilium:host/location in reference-wit
current: func() -> option<tuple<f64, f64>>;   // latitude, longitude
```

### How the value behaves

- The call returns the value that the setting holds at the moment of the call. A change in Settings > Location takes effect at the next call.
- The value never goes stale, so a plugin needs no age check.
- When the user has not set a position, the call returns nothing: `None` in Rust and `undefined` in TypeScript.
- The coordinates are returned as the setting holds them. Round them in the plugin before you put them in a URL or a label.

### Worked example 1: choose a weather source once

The live-data scene (feature 99) reads the position once at start and builds a data URL from it. The example rounds the coordinates to two decimal places. That is about 1.1 km of latitude, which is enough for weather and too coarse to pinpoint a home.

**Grants:** `location.observer`, `http:data.example.org`

**Rust**

```rust
use ilium_plugin_sdk::host::location;

/// Chooses the weather data URL for the observer position.
/// Falls back to a regional default when the user has not set a position.
pub fn pick_weather_source() -> String {
    match location::current() {
        Some((latitude, longitude)) => format!(
            "https://data.example.org/weather?lat={latitude:.2}&lon={longitude:.2}"
        ),
        None => "https://data.example.org/weather?region=default".to_string(),
    }
}
```

**TypeScript**

```ts
import * as location from '@ilium/plugin-sdk/host/location';

/** Chooses the weather data URL for the observer position. */
export function pickWeatherSource(): string {
  const position = location.current();
  if (position === undefined) {
    return 'https://data.example.org/weather?region=default';
  }
  const [latitude, longitude] = position;
  const latitudeText = latitude.toFixed(2);
  const longitudeText = longitude.toFixed(2);
  return `https://data.example.org/weather?lat=${latitudeText}&lon=${longitudeText}`;
}
```

Outcome: with a position, the URL carries the rounded coordinates. Without one, the default regional URL is used and the scene keeps running.

### Worked example 2: a settings row that never shows coordinates

A client plugin shows one status row in its settings page. The row says whether a position is set and never prints the numbers. It needs only the `location.observer` grant.

**Rust**

```rust
use ilium_plugin_sdk::host::location;

/// Text for the settings row. The coordinates never leave this function.
pub fn observer_position_label() -> &'static str {
    match location::current() {
        Some(_) => "Observer position set (Settings > Location)",
        None => "No observer position. Set one in Settings > Location.",
    }
}
```

**TypeScript**

```ts
import * as location from '@ilium/plugin-sdk/host/location';

/** Text for the settings row. The coordinates never leave this function. */
export function observerPositionLabel(): string {
  return location.current() === undefined
    ? 'No observer position. Set one in Settings > Location.'
    : 'Observer position set (Settings > Location)';
}
```

Outcome: the row reads either the set state or the instruction to set one. A declined grant gives the same text, so see the edge case below before you word the message more strongly.

<pre class="term">$ ilium plugin logs example.live-weather
2026-10-11 08:05:44 info  observer position set; source uses 2 decimals
2026-10-11 08:40:02 warn  no observer position: using region=default
2026-10-11 08:40:02 refusal location.observer declined: current returned none
$ 3 lines shown, 0 dropped
</pre>

Illustrative, not generated. The log records that a position was known or missing, and the refusal line names the grant. The coordinates never appear in the log.

<figure>
<svg viewBox="0 0 420 212" role="img" aria-label="The user sets a position, the host location import checks the grant, and the plugin uses it or falls back to a default">
<title>From the settings position to the weather source</title>
<desc>Settings > Location stores one observer position. The host location import answers current() for a plugin that holds the location.observer grant. The plugin uses the position to build the weather URL. When the import returns none, the plugin uses the default regional source.</desc>
<defs>
<marker id="arr-api-location" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
<path d="M0,0 L8,4 L0,8 z" fill="currentColor"/>
</marker>
</defs>
<rect x="110" y="8" width="200" height="50" rx="6" fill="none" stroke="currentColor"/>
<text x="210" y="28" font-size="12" text-anchor="middle" fill="currentColor">Settings &gt; Location</text>
<text x="210" y="44" font-size="10" text-anchor="middle" fill="currentColor">one observer position</text>
<line x1="210" y1="58" x2="210" y2="92" stroke="currentColor" marker-end="url(#arr-api-location)"/>
<text x="220" y="80" font-size="10" fill="currentColor">stored</text>
<rect x="10" y="94" width="200" height="50" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
<text x="110" y="114" font-size="12" text-anchor="middle" fill="currentColor">Host location import</text>
<text x="110" y="130" font-size="10" text-anchor="middle" fill="currentColor">grant: location.observer</text>
<line x1="210" y1="119" x2="288" y2="119" stroke="currentColor" marker-end="url(#arr-api-location)"/>
<text x="249" y="110" font-size="10" text-anchor="middle" fill="currentColor">current()</text>
<rect x="290" y="94" width="120" height="50" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
<text x="350" y="114" font-size="12" text-anchor="middle" fill="currentColor">Plugin code</text>
<text x="350" y="130" font-size="10" text-anchor="middle" fill="currentColor">scene or client</text>
<line x1="110" y1="144" x2="110" y2="172" stroke="currentColor" marker-end="url(#arr-api-location)"/>
<text x="120" y="162" font-size="10" fill="currentColor">none</text>
<rect x="10" y="174" width="200" height="30" rx="6" fill="none" stroke="currentColor"/>
<text x="110" y="193" font-size="12" text-anchor="middle" fill="currentColor">Default regional source</text>
</svg>
<figcaption>The user sets one position. The plugin reads it through the grant-checked import, and falls back to the default source when the import returns none.</figcaption>
</figure>

### What the user sees

- **Settings > Location:** the user sets the observer position once. Ambient scenes that use location read the same value.
- **Install review (T2 and T3 packages):** the review dialog lists `location.observer` as high risk. The box is unchecked by default, and the user must check it to grant the capability.
- **First activation (T1 bundled plugins):** the plugin starts without the grant. One prompt appears, and location is one checkbox in it. The buttons are `Allow selected` and `Not now`. `Not now` asks again at the next start, at most three times.
- **Revoking:** Settings > Plugins > (plugin) > Capabilities, or `ilium plugin grants <id> --revoke location.observer`.

The prompt for a bundled plugin looks like this:

<pre class="term">
╭─ Plugin example.live-weather asks for ─────────────────────────────╮
│ Plugin example.live-weather asks for:                              │
│ [x] location.observer  high risk  Read the observer position       │
│                                                                    │
│                          [ Allow selected ]      [ Not now ]       │
╰────────────────────────────────────────────────────────────────────╯
</pre>

Illustrative, not generated. The prompt layout follows the first-activation rule for high-risk grants.

### Edge cases and errors

`current` returns a value, not a result, so it has no error code. The universal codes apply only to functions that return a result, so `not-permitted`, `revoked` and `internal` never come from this import. Every refusal shows up in the plugin log instead.

| Situation | What `current` returns | What the plugin should do |
|---|---|---|
| The user has not set a position | Nothing (`None` or `undefined`) | Use a default and say that no position is set. |
| The grant was declined | Nothing, and the refusal is logged | Treat it as unknown. The plugin cannot tell this case apart from an unset position, so avoid stating that the user has no position. |
| The grant is revoked while the plugin runs | Nothing on the next call. The plugin receives `deactivate(revoked)` and re-activates with the smaller grant set | Expect nothing until the plugin has re-activated, then call again. |
| The manifest never listed the grant | The import traps when called, because that is a plugin bug | List `location.observer` in the manifest. |
| The plugin is disabled | No calls | None. |
| The user changes the position | The new value at the next call | Call again instead of caching the old value. |

The decisions define no limit, quota or rate rule for this import, so a plugin does not need to budget its calls.

<div class="warn">
<p>A position reveals where the user is. Send it to a network host only when the user has agreed to that, and only to the host named in the plugin's grants. Do not write it to storage or to the plugin log unless the feature cannot work without it.</p>
</div>

### Used by

- Live-data scenes (feature 99, Phase 4d) call `location.current` to choose the data source. The same feature fetches the data with `http.fetch`, using a grant for each data host.

### Geocoding and the picker

Coordinates still come from `location.current` with `location.observer`. Turning a name into a point, or a point into a name, is now a host service with its own grant, `location.geocode`: see [geocode and location picker](/pages/api-geocode/). A plugin that only needs a place the user chooses should use a `location` settings row or the `location-picker` widget; it then never handles coordinates as text. Settings views may embed the picker.

| Need | Use | Grant |
|---|---|---|
| Where the observer is | `location.current` | `location.observer` |
| A place the user picks | `location` row or `location-picker` | none |
| A name to coordinates | `geocode.search` | `location.geocode` |
| Coordinates to a name | `geocode.reverse` | `location.geocode` |

**Rust**

```rust
let request = geocode::reverse(GeoPoint { lat: 38.72, lon: -9.14 })?;
```

**TypeScript**

```ts
const request = reverse({ lat: 38.72, lon: -9.14 });
```

### See also

- [capability: location.observer](/pages/capability-location-observer/) for the grant rules and the review dialog.
- [audio](/pages/api-audio/) for the other high-risk device import.
- [scenes](/pages/api-scenes/) for the scene contract that feature 99 uses.
- [http](/pages/api-http/) for the data requests that follow the position.
- [capabilities](/pages/capabilities/) for the full grant list and the risk tiers.
- [errors and limits](/pages/errors-and-limits/) for the error codes that other imports use.

Interface: `ilium:host/location@0.1.0`, part of `ilium:plugin@0.1.0`.
