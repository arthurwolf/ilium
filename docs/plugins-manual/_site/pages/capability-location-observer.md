# Capability: location.observer

## location.observer

The `location.observer` grant lets a client-side or scene plugin read the observer position. The observer position is the latitude and longitude that the user sets in Settings > Location. The host does not use GPS, an operating system location service or IP geolocation, so the value is the user's own setting and not a live reading of where the computer is.

The grant is high risk (H), because a position is personal data. It always needs an explicit user grant, including for a bundled plugin.

The grant covers one function, `current`. It returns the position, or `none` when there is no position to give.

**You will need:**

- A plugin built against `ilium:plugin@0.1.0`, with a client world (`ilium:client/base`) or a scene world (`ilium:anim/base`). The server side cannot hold this grant.
- The grant in the capability list of that side: `capabilities.client` for a client plugin, or `capabilities.scene` for a scene.
- The Rust SDK (`ilium-plugin-sdk`) or the TypeScript SDK (`@ilium/plugin-sdk`). Both expose the same function.
- Nothing else. A plugin cannot set the position. Only the user can, in Settings > Location.

<div class="warn">
<p>A position reveals where the user is. Grant it only to a plugin whose visible feature needs it. The grant does not allow network traffic: a plugin that sends the value to a host also needs an <code>http:&lt;host&gt;</code> grant, and the user should know that the value leaves the computer.</p>
</div>

<figure>
<svg viewBox="0 0 400 200" role="img" aria-label="A scene calls current, the host checks the grant and reads the observer position set in Settings > Location, then returns a tuple or none">
  <defs>
    <marker id="arr-loc" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
      <path d="M0,0 L8,4 L0,8 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="8" y="40" width="124" height="56" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="70" y="62" text-anchor="middle" fill="currentColor" font-size="12">Scene plugin</text>
  <text x="70" y="80" text-anchor="middle" fill="currentColor" font-size="10">location.observer</text>
  <rect x="268" y="40" width="124" height="56" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="330" y="62" text-anchor="middle" fill="currentColor" font-size="12">Host grant check</text>
  <text x="330" y="80" text-anchor="middle" fill="currentColor" font-size="10">returns option</text>
  <line x1="134" y1="60" x2="264" y2="60" stroke="currentColor" marker-end="url(#arr-loc)"/>
  <text x="199" y="50" text-anchor="middle" fill="currentColor" font-size="11">current()</text>
  <line x1="264" y1="84" x2="134" y2="84" stroke="currentColor" marker-end="url(#arr-loc)"/>
  <text x="199" y="104" text-anchor="middle" fill="currentColor" font-size="11">lat, lon or none</text>
  <rect x="268" y="128" width="124" height="44" rx="6" fill="none" stroke="currentColor"/>
  <line x1="330" y1="126" x2="330" y2="98" stroke="currentColor" marker-end="url(#arr-loc)"/>
  <text x="330" y="146" text-anchor="middle" fill="currentColor" font-size="11">Observer position</text>
  <text x="330" y="162" text-anchor="middle" fill="currentColor" font-size="10">Settings &gt; Location</text>
  <text x="199" y="190" text-anchor="middle" fill="currentColor" font-size="10">A declined or revoked grant also returns none.</text>
</svg>
<figcaption>The host checks the grant before it reads the observer position. A declined or revoked grant gives the same none as an unset position, so the scene shows its default view.</figcaption>
</figure>

### Grant syntax

The grant string is the name alone. The grant takes no argument.

| Grant | Valid | Reason |
|---|---|---|
| `location.observer` | yes | the only form |
| `location.observer:city` | no | install fails with `manifest.grant-argument` |
| `location.current` | no | `current` is the function name, not the grant; install fails with `manifest.grant-unknown` |

### Surface

The grant adds one host function, from the WIT interface `ilium:host/location`. The Rust import is `ilium_plugin_sdk::host::location` and the TypeScript import is `@ilium/plugin-sdk/host/location`.

| Function | Rust | TypeScript | WIT signature | Meaning |
|---|---|---|---|---|
| `current` | `ilium_plugin_sdk::host::location::current()` | `current()` from `@ilium/plugin-sdk/host/location` | `func() -> option<tuple<f64, f64>>` | The observer position, or none |

The WIT form is:

```wit
// Excerpt of ilium:host/location in reference-wit
current: func() -> option<tuple<f64, f64>>;
```

The return value maps to these types:

| Part | Rust | TypeScript | Meaning |
|---|---|---|---|
| `option` | `Option<(f64, f64)>` | `[number, number] \| undefined` | `None` or `undefined` when there is no position |
| first value | `f64` | `number` | latitude, in decimal degrees on WGS 84 |
| second value | `f64` | `number` | longitude, in decimal degrees on WGS 84 |

The call is synchronous. It has no `async` form and returns at once, so Rust code does not use `.await` and TypeScript code does not use `await`.

The value behaves as follows:

- **No accuracy.** The host returns no accuracy value, because the position is the user's own setting.
- **No staleness.** The value is as old as the setting and never goes stale. A plugin that needs the value calls `current` when it needs it. A change in Settings > Location takes effect at the next call.
- **No position.** When the user has not set a position, the call returns none.

### Worked example 1: Rust scene

The scene declares the grant in `capabilities.scene`:

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.scenes",
  "scene": { "entry": "scene.wasm", "world": "ilium:anim/base@0.1.0", "exports": [] },
  "capabilities": { "scene": ["location.observer"] }
}
```

The scene chooses the centre of its map. It uses its own default when the call returns none:

**Rust**

```rust
use ilium_plugin_sdk::host::location;

/// Centre of the map view. Falls back to `default_centre` when the grant is
/// declined or the user has not set a position.
pub fn map_centre(default_centre: (f64, f64)) -> (f64, f64) {
    match location::current() {
        Some((latitude, longitude)) => (latitude, longitude),
        None => default_centre,
    }
}
```

Call it from the scene constructor with a fixed fallback, for example `map_centre((52.52, 13.405))`.

### Worked example 2: TypeScript client plugin

The client plugin declares the grant in `capabilities.client`:

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.weather-panel",
  "capabilities": { "client": ["location.observer"] }
}
```

The panel label reads the position through a named import, and it destructures the tuple in the order latitude, then longitude:

**TypeScript**

```ts
import * as location from '@ilium/plugin-sdk/host/location';

/** Short label for the weather panel. Never throws when the grant is missing. */
export function observerLabel(): string {
  const position = location.current();
  if (position === undefined) {
    return 'Set your position in Settings > Location to see local weather.';
  }
  const [latitude, longitude] = position;
  return `Observer at ${latitude.toFixed(2)}, ${longitude.toFixed(2)}`;
}
```

A plugin that does not want the coordinates on screen can drop the second branch and show only the set or unset state. The value stays inside the plugin until the plugin sends it somewhere.

### What the user sees

When a T2 or T3 package is installed, the review dialog lists the grant in the Scene group. The row shows the label, a `HIGH` mark in the danger colour, and the grant string. High risk rows start unchecked.

<figure>

<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.location 0.1.0 by Example Ltd                                  │
│                                                                            │
│ <span class="t-hi">Client side</span>                                                                │
│ <span class="t-warn">! [ ] Reads your location</span> <span class="t-dim">………………………………………………………………………………</span> <span class="t-warn">location.observer</span> │
│                                                                            │
│ <span class="t-warn">! marks a high-risk grant. It starts unchecked and is</span>                      │
│ <span class="t-warn">always asked for, even for bundled plugins.</span>                                │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>

<figcaption>The review dialog for a package that requests location.observer, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar. Checked rows are granted; a high-risk row starts unchecked.</figcaption>
</figure>

A bundled T1 plugin is not shown the review dialog. It is activated first without the grant. On its first activation the host shows a prompt. `Not now` asks again at the next server start, at most three prompts in total, and then the user grants it in Settings > Plugins:

<figure>

<pre class="term"><span class="t-hi">Plugin com.example.scenes asks for:</span>
  <span class="t-err">HIGH</span> [ ] Know your configured location  location.observer
<span class="t-dim">[Allow selected]  [Not now]</span></pre>

<figcaption>Illustrative, not generated. The first-activation prompt for a bundled plugin. Choosing Not now asks again at the next server start, at most three prompts in total.</figcaption>
</figure>

The user can change a grant at any time in Settings > Plugins > <plugin> > Capabilities, or with `ilium plugin grants <id> --revoke location.observer`.

When the host refuses a call, it writes a refusal line to the plugin log. Run `ilium plugin logs <id>` to see it. A declined grant writes this line:

```json
{
  "type": "refusal",
  "grant": "location.observer",
  "function": "location.current",
  "reason": "declined"
}
```

### Risk and tier rules

- `location.observer` is high risk (H). It always needs an explicit grant, even for a T1 bundled plugin.
- **T1 bundled plugins** show a prompt per plugin at first activation (repeated at the next start after Not now, at most three prompts in total): `Plugin <name> asks for: <list>`, with the buttons `Allow selected` and `Not now`.
- **T2 and T3 packages** show the grant in the review dialog. The user checks each high risk row.
- **Client and scene worlds only.** The grant is available on the client side (`C`) and the animation world (`A`). It is not available on the server side.
- **Per side and per plugin.** The grant is recorded per plugin, per side and per user configuration, not per project.

### Declined, revoked and unset positions

These three situations return `none`. A call when the manifest never asked for the grant is not one of them: it traps with `host.capability-not-requested`. The plugin cannot tell them apart from the value alone, which is why the scene shows its default view.

| Situation | `current` returns | Plugin log |
|---|---|---|
| Grant declined at install or at the prompt | `none` | refusal line, reason `declined` |
| Grant revoked while the plugin runs | `none` at the next call | refusal line, reason `revoked` |
| User has not set a position | `none` | no refusal line |

A revocation first withdraws the grant for in-flight work. The plugin then receives `deactivate(revoked)` and is re-activated with the reduced grant set. That restart does not count as a failure of the plugin.

### Edge cases and errors

- **No error codes.** `current` returns an option, not a result, so no error code exists for it. The universal codes of the error table do not apply.
- **Refusal is not an error.** A plugin should show its default view and should not show a message that suggests the location failed. A message such as "No observer position. Set one in Settings > Location." is correct when the user has not set a position, but it is misleading for a declined grant.
- **Install errors.** `location.observer:city` fails with `manifest.grant-argument`, with the text `capability "location.observer" takes no argument (got "location.observer:city")`. A grant on the server side fails with `manifest.grant-side`, with the text `capability "location.observer" is not available on the server side`.
- **Tuple order.** The first value is always the latitude. Swapping the two values puts the position in the wrong place without any error.
- **Network use.** The value never leaves the plugin on its own. Sending it to a host also needs the `http:<host>` grant.

### Who requests it

The first-party live-data scenes request this grant, together with `http:<data-host>` for each data source and `jobs.run`. The live-data scenes use the observer position to choose which local data to fetch.

### See also

- [api-location](/pages/api-location/) for the import and its value behaviour in full.
- [capability-http](/pages/capability-http/) for the network request that a location might feed.
- [capability-jobs-run](/pages/capability-jobs-run/) for work that reads the location on a schedule.
- [api-scenes](/pages/api-scenes/) for the scene world and its manifest.
- [reference-capabilities](/pages/reference-capabilities/) for every grant in one table.

Checked against `ilium:plugin@0.1.0` (API draft of 2026-10-11).
