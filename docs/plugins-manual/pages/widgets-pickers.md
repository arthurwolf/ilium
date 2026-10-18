---
layout: page
title: Pickers
permalink: /pages/widgets-pickers/
---

## Pickers

A picker is an input that opens a dialog drawn by the host. The plugin shows a closed field and hears only the result. This page documents three widgets, `color-picker`, `path-field` and `location-picker`, and three uses of `choice` that the Settings screen shows as pickers: the palette picker, the dither picker and the icon picker. Their dialogs are kernel code, so the look, the keys and the network rules are the same as in the Settings screen, and a plugin cannot change them.

| Picker | Widget | Dialog | Value |
|---|---|---|---|
| Colour | `color-picker` | hue strip, shade strip, swatches, hex field | `text(#rrggbb)` |
| Path | `path-field` | the kernel file picker | `text(display path)` |
| World map | `location-picker` | Braille map, address search, coordinates | `geo(geo-point)` |
| Palette | `choice` over `ilium.palettes` | choice dialog with swatches | `text(palette id)` |
| Dither | `choice` over `ilium.dithers` | choice dialog with patterns | `text(dither id)` |
| Icon | `choice` over `ilium.icons` | choice dialog with a List and Grid switch | `text(icon id)` |

### You will need

- The `ui.panels` grant for a view that holds the picker. See [ui.panels](/pages/capability-ui-panels/).
- `fs.pick` for the file picker of a `path-field`. See [fs.pick](/pages/capability-fs-pick/).
- `location.geocode` for the address search of a `location-picker`, and `location.observer` for its `Use current location` button. See [location.geocode](/pages/capability-location-geocode/) and [location.observer](/pages/capability-location-observer/).
- Catalogue ids for the choice pickers. The host publishes `ilium.palettes`, `ilium.dithers` and `ilium.icons`. A plugin can add its own catalogue, see [Catalogues](/pages/api-catalogs/).

### Settings pages

A settings page accepts `location-picker`, and no other picker. Its value is written only through the `bind` prop to an own `location` key, and the host validates and saves it as it does for a row. For the other pickers use the matching settings row: `color`, `path`, `choice` with a catalogue source. See [Settings screens](/pages/settings-screens/).

#### color-picker

**Purpose.** Edits a colour as `#rrggbb`. The closed field shows a swatch `██` and the hex text. Enter opens the host colour dialog with a hue strip, a shade strip, a row of swatches and a hex field. The dialog is true colour, so the capture shows only its glyphs. The Ilium client has no colour dialog of its own yet, so the render follows the specification on this page.

| Prop | Type | Default | Meaning |
|---|---|---|---|
| `value` | `string` | required | The colour as `#rrggbb`. |
| `palette` | `option<string>` | none | A catalogue id of preset swatches. |
| `allow-theme-roles` | `bool` | `false` | Adds the theme roles (`accent`, `border`) to the swatches. |

| Event | Value | Sent when |
|---|---|---|
| `changed` | `text(#rrggbb)` | The user applies a colour in the dialog. |

| Input | Behaviour |
|---|---|
| Enter | Opens the dialog. |
| Tab | Cycles the dialog fields: hue, shade, swatches, hex. |
| Left, Right, Up, Down | Move the hue, shade or swatch. |
| Typing in the hex field | Sets the colour. A value that is not `#rrggbb` is refused. |
| Enter in the dialog | Applies. |
| Escape | Cancels. |
| Click on a swatch, drag in a strip | Pick. |

Theme roles: the picked colour for the swatch, `border` for the dialog frame, `danger` for an invalid value.

<figure>
<pre class="term">╭  ≡ ● · Colour rows───────────────────────────────────────────────────────╮
│ <span class="t-hi">Accent</span> <span class="t-dim">……………………………………………………………………………………………………………………………………</span> <span class="t-hi">[ ██ #7aa2f7 ]</span> │
│ Border <span class="t-dim">……………………………………………………………………………………………………………………………………</span> [ ██ #4a4a4a ] │
│ Marker <span class="t-dim">……………………………………………………………………………………………………………………………………</span> <span class="t-err">[ ██ #zzzzzz ]</span> │
│ <span class="t-dim">Background</span> <span class="t-dim">…………………………………………………………………………………………………………………………</span> <span class="t-dim">[ ██ #1a1b26 ]</span> │
╰──────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>The closed colour-picker field. The swatch ██ shows the colour and the hex value follows. Enter opens the colour dialog. An invalid value uses the error colour.</figcaption>
</figure>

<figure>
<pre class="term">╭  ≡ ● · Colour────────────────────────────────────────────────────╮
│ Hue    <span class="t-err">▀▀▀▀▀▀</span><span class="t-warn">▀▀▀▀▀▀</span><span class="t-ok">▀▀▀▀▀▀</span><span class="t-hi">▀▀▀▀▀▀▀▀▀▀▀▀</span><span class="t-err">▀▀▀▀▀▀</span>                      │
│ Shade  ▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀                  │
│ Swatches  ██ ██ ██ ██ ██ ██ ██ ██ ██ ██                          │
│ Hex <span class="t-dim">……………………………………</span> [ #7aa2f7 ]   ██                              │
│ <span class="t-dim">Tab cycles fields · arrows move · Enter applies · Esc cancels</span>    │
╰──────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>The host colour dialog. The hue strip and the shade strip are drawn with half blocks in true colour; the capture shows only the glyphs. Swatches come from the optional palette catalogue. The layout is the contract of this page; the Ilium client has no colour dialog of its own yet, so this render follows the specification, not a client capture.</figcaption>
</figure>

**Rust**

```rust
let accent = tree.color_picker("#7aa2f7").key("accent").palette("ilium.palettes").allow_theme_roles(true);
// ("accent", ActionKind::Changed, ActionValue::Text(hex)) => self.accent = hex,
```

**TypeScript**

```ts
const accent = colorPicker('#7aa2f7', { key: 'accent', palette: 'ilium.palettes', allowThemeRoles: true });
// if (key === 'accent' && value.tag === 'text') state.accent = value.val;
```

#### path-field

**Purpose.** Chooses a file or a folder. The field shows a display text and a `…` button. The text is only a label: the plugin never sees the raw path, only a picked scope that `fs.pick` created. This keeps a plugin from reading outside the paths the user chose.

| Prop | Type | Default | Meaning |
|---|---|---|---|
| `kind` | `path-kind` | required | `open-file`, `open-directory` or `save-file`. |
| `value` | `option<fs-path>` | none | The picked scope. |
| `display` | `string` | empty | The text shown. |
| `extensions` | `list<string>` | empty | File extensions the picker offers. |
| `editable` | `bool` | `false` | Lets the user type a path. The host resolves it. |

| Event | Value | Sent when |
|---|---|---|
| `changed` | `text(display path)` | The user picks a path. |
| `submitted` | `text(display path)` | Enter in an editable field. |

| Input | Behaviour |
|---|---|
| Enter on the button | Opens the host file picker. |
| Typing | Edits the text when `editable`. |
| Click on `…` | Opens the file picker. |

Theme roles: `foreground` for the text, `danger` for a path the plugin reports as missing.

<figure>
<pre class="term">╭  ≡ ● · Path rows─────────────────────────────────────────────────────────╮
│ <span class="t-hi">Project folder</span> <span class="t-dim">……………………………………………………………………………………………</span> <span class="t-hi">[ ~/dev/ilium ] [ … ]</span> │
│ Export file <span class="t-dim">……………………………………………………………………………………………………</span> [ report.html ] [ … ] │
│ Backup folder <span class="t-dim">………………………………………………………………………………</span> <span class="t-err">[ /data/old-backups ] [ … ]</span> │
│ <span class="t-dim">Cache folder</span> <span class="t-dim">……………………………………………………………………………………………</span> <span class="t-dim">[ ~/.cache/demo ] [ … ]</span> │
╰──────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>The path-field widget. The text is the display string and the … button opens the host file picker. The error row names a path the plugin has reported missing. The plugin never sees the raw path: the picked scope is what it receives.</figcaption>
</figure>

**Rust**

```rust
let export = tree.path_field(PathKind::SaveFile).key("export").display(&self.export_name).extensions(vec!["html"]);
// ("export", ActionKind::Changed, ActionValue::Text(name)) => self.export_name = name,
```

**TypeScript**

```ts
const exportField = pathField('save-file', { key: 'export', display: state.exportName, extensions: ['html'] });
// if (key === 'export' && value.tag === 'text') state.exportName = value.val;
```

#### location-picker

**Purpose.** Picks a point on the world. The host draws the real Ilium world map in Braille dots, a crosshair `┼`, an address search and a coordinates mode. The plugin receives only the point. Geocoding runs in the host with Photon and falls back to Nominatim. The user agrees once to send place names to that provider.

**The map.** The land is a Braille mask of 360 by 180 samples, one per degree, taken from the same data as the ambient world map scene. Each Braille cell holds 2 by 4 dots, and a dot is set when land covers at least half of its area. The equator and the prime meridian are drawn as `─` and `│` lines. The crosshair `┼` is drawn in the accent colour at the selected point. Land is `muted` and the sea is the background. The Selected row names the place and shows the point.

| Prop | Type | Default | Meaning |
|---|---|---|---|
| `value` | `option<geo-point>` | none | The point: `latitude` and `longitude` in degrees. |
| `label` | `option<string>` | none | The place name shown in the Selected row. |
| `search` | `bool` | `false` | Shows the Address search. Needs `location.geocode`. |
| `allow-current` | `bool` | `false` | Adds `Use current location`. Needs `location.observer`. |
| `zoom` | `bool` | `false` | Lets the wheel zoom the map. |
| `bind` | `option<string>` | none | An own `location` key. Used in settings pages only. |

| Event | Value | Sent when |
|---|---|---|
| `changed` | `geo(geo-point)` | The crosshair moves to a new point. |
| `submitted` | `geo(geo-point)` | The user presses `Use location`. |

| Input | Behaviour |
|---|---|
| Arrows | Move the crosshair by one Braille dot. Shift moves by 10 dots. |
| Tab | Switches between Address and Coordinates. |
| Typing in Address | Searches after 300 ms and lists up to 5 results. |
| Up, Down in the results | Choose a result. |
| Enter | Uses the highlighted result, or confirms the point (`Use location`). |
| Escape | Cancels. |
| Left click on the map | Sets the point. |
| Mouse wheel | Zooms, when `zoom` is true. |

Theme roles: `muted` for land, `background` for the sea, `accent` for the crosshair, `danger` for an error message.

<figure>
<pre class="term">╭  ≡ ● · Location──────────────────────────────────────────────────────╮
│ Entry <span class="t-dim">…………………………………………………………………………………………………………</span> ← Address → +         │
│ ╭  ≡ ● ·───────────────────────────────────────────────────────╮     │
│ │ Paris_                                                       │     │
│ ╰──────────────────────────────────────────────────────────────╯     │
│ <span class="t-hi">Paris, France (48.8534, 2.3488)</span>                                      │
│ Paris, Texas, United States (33.6609, -95.5555)                      │
│ Paris, Ile-de-France, France (48.8566, 2.3522)                       │
│           ⢀⢀⡀ ⢀⢠⢤⣤⠤⠄⠠⠤⢤⣤⣤⣴⣶⣶⣤⠄   │ ⠠⠄⠄          ⢀⣀⣀⣀⣀⣀    ⢀⡀         │
│    ⠽⠿⠿⠿⠷⢾⣿⣿⣾⣿⣿⣶⣿⣛⠛⢩⣽⣓⣀ ⠙⠻⠋⠉⠉⠁   ⡀│⢤⣶⠚⣿⣿⣶⣶⣶⣶⣶⣷⣾⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⡿⠿⠿⢟⡿⠷⠞⠓   │
│ ─────────⠈⢹⣿⣿⣿⣿⣿⣿⣿⣾⠿⠛⠉⠁────────⢠⣼<span class="t-err">┼</span>⠛⠟⠿⡿⣛⣛⣻⣿⠻⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⠿⡿⠛⢋───────   │
│            ⠉⠛⢿⣿⡋⢉⠙⠁           ⣤⣾⣿⣿⣿⣶⣶⣶⣶⣞⢿⣿⣟⡿⠛⠿⢿⣿⣿⠿⣿⣿⡿⠿⠿⠂ ⠁           │
│                 ⠉⠃⢀⣤⣤⣤⣄⡀      ⠛⠿⠿⠿⢿⣿⣿⣿⣿⣿⣷⡭⠁   ⠈⠋   ⣉⠛⣀⡀⠁             │
│                   ⠙⢿⣿⣿⣿⣿⣿⣶⠂      │ ⣹⣿⣿⣿⣿⡅⢀         ⠈⠁⠉⠁ ⣀⣀⠑⡒         │
│                     ⣿⣿⣿⠿⠋⠁       │ ⠘⢿⣿⠿⠃ ⠋            ⢶⡿⠿⠿⣿⣿⡶        │
│                    ⢼⡟⠉⠁          │                         ⠉     ⠁   │
│                     ⢀⡀           │    ⢀⣀⣀⣀⣀⣀⣀⡀ ⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀       │
│ ⣀⣀⣀⣀⣀⣴⣶⣶⣿⣿⣿⣿⣶⣶⣿⣿⣿⣿⣿⣭⣭⣥⣤⣤⣦⣤⣤⣴⣶⣶⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣥⣀⣀   │
│ <span class="t-hi">Selected: Paris, France (48.8534, 2.3488)</span>                            │
│ <span class="t-dim">Map: Natural Earth (public domain)</span>                                   │
│ <span class="t-dim">Search: Photon (komoot)</span>                                              │
│ <span class="t-dim">© OpenStreetMap contributors (ODbL)</span>                                  │
│          <span class="t-dim"> Esc Cancel </span>                 <span class="t-hi"> Enter Use location </span>           │
│ <span class="t-dim">Enter uses this place; arrows fine-tune it</span>                           │
╰──────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>The location-picker dialog in Address mode with a result list. The map is the real Braille land mask (360 by 180 samples resampled to 66 by 10 cells); the red ┼ is the crosshair at Paris, and the dim ─ and │ are the guide lines through it. The first result row is highlighted.</figcaption>
</figure>

The Coordinates mode replaces the address box with two number rows. Each row opens a number entry with the message `Enter latitude` or `Enter longitude`. Editing a coordinate replaces the place name with the coordinates.

<figure>
<pre class="term">╭  ≡ ● · Location──────────────────────────────────────────────────────╮
│ Entry <span class="t-dim">…………………………………………………………………………………………………………</span> ← Coordinates → +     │
│ Latitude <span class="t-dim">………………………………………………………………………………………………………………………</span> 48.8534       │
│ Longitude <span class="t-dim">……………………………………………………………………………………………………………………</span> 2.3488        │
│           ⢀⢀⡀ ⢀⢠⢤⣤⠤⠄⠠⠤⢤⣤⣤⣴⣶⣶⣤⠄   │ ⠠⠄⠄          ⢀⣀⣀⣀⣀⣀    ⢀⡀         │
│    ⠽⠿⠿⠿⠷⢾⣿⣿⣾⣿⣿⣶⣿⣛⠛⢩⣽⣓⣀ ⠙⠻⠋⠉⠉⠁   ⡀│⢤⣶⠚⣿⣿⣶⣶⣶⣶⣶⣷⣾⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⡿⠿⠿⢟⡿⠷⠞⠓   │
│ ─────────⠈⢹⣿⣿⣿⣿⣿⣿⣿⣾⠿⠛⠉⠁────────⢠⣼<span class="t-err">┼</span>⠛⠟⠿⡿⣛⣛⣻⣿⠻⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⠿⡿⠛⢋───────   │
│            ⠉⠛⢿⣿⡋⢉⠙⠁           ⣤⣾⣿⣿⣿⣶⣶⣶⣶⣞⢿⣿⣟⡿⠛⠿⢿⣿⣿⠿⣿⣿⡿⠿⠿⠂ ⠁           │
│                 ⠉⠃⢀⣤⣤⣤⣄⡀      ⠛⠿⠿⠿⢿⣿⣿⣿⣿⣿⣷⡭⠁   ⠈⠋   ⣉⠛⣀⡀⠁             │
│                   ⠙⢿⣿⣿⣿⣿⣿⣶⠂      │ ⣹⣿⣿⣿⣿⡅⢀         ⠈⠁⠉⠁ ⣀⣀⠑⡒         │
│                     ⣿⣿⣿⠿⠋⠁       │ ⠘⢿⣿⠿⠃ ⠋            ⢶⡿⠿⠿⣿⣿⡶        │
│                    ⢼⡟⠉⠁          │                         ⠉     ⠁   │
│                     ⢀⡀           │    ⢀⣀⣀⣀⣀⣀⣀⡀ ⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀       │
│ ⣀⣀⣀⣀⣀⣴⣶⣶⣿⣿⣿⣿⣶⣶⣿⣿⣿⣿⣿⣭⣭⣥⣤⣤⣦⣤⣤⣴⣶⣶⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣥⣀⣀   │
│ Selected: Paris, France (48.8534, 2.3488)                            │
│          <span class="t-dim"> Esc Cancel </span>                 <span class="t-hi"> Enter Use location </span>           │
│ <span class="t-dim">Coordinates updated</span>                                                  │
╰──────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>Coordinates mode. The two coordinate rows replace the address box, each row opens a number entry with the message Enter latitude or Enter longitude. Editing a coordinate replaces the label with the coordinates.</figcaption>
</figure>

Inside a settings page the picker is smaller and writes through `bind`, so the plugin receives `settings-changed` and reads the point from its own `location` key.

<figure>
<pre class="term">╭  ≡ ● · Observer location─────────────────────╮
│ Nairobi (-1.2900, 36.8200)                   │
│   ⣀⣀⣀⣀⣠⣤⣀⣄⣶⢒⡒⠲⣶⣶⡶⠖   ⢀⣀⣀│⣀⣀⣀⣀⣤⣤⣤⣤⣀⣀⣠⣀⣀⣀⣀     │
│   ⠉⠉⠉⠙⣿⣿⣿⣷⣦⣼⠷⠂⠈    ⣰⡬⠿⣾⢿⣿⡿⣿⣿⣿⣿⣿⣿⣿⣿⡿⠏⠉⠙⠉      │
│       ⠈⠙⠿⢋⡙⠁      ⣴⣿⣿⣶⣶⣶⣻⣿⠟⠻⣿⠿⢿⣿⠿⠃           │
│ ───────────⢼⣿⣶⣦⣄──⠈⠉⠉⢿⣿⣿<span class="t-err">┼</span>⠁──⠈──⠂⠰──⠠────     │
│             ⣿⣿⠟⠃     ⠘⠿⠟⠁⠃      ⠠⠶⠿⢿⡦        │
│            ⠐⠋           │                    │
│ ⣀⣀⣀⣤⣤⣴⣦⣤⣴⣶⣶⣶⣚⣀⣠⣀⣤⣤⣶⣶⣶⣶⣶⣶⣶⣾⣷⣶⣶⣿⣿⣿⣿⣿⣿⣿⣶⣶⣂⣀     │
│ <span class="t-dim">Click the map · arrows fine-tune</span>             │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>The location-picker embedded in a settings view next to other content. Here it is drawn at 40 by 7 cells with the point at Nairobi; the value is saved through the bind prop without any plugin code running.</figcaption>
</figure>

Geocoding limits: 1 request per second per plugin, a 24 hour cache by query text, a 10 s timeout. Without consent the search returns `not-permitted`. Errors appear in the status line under the buttons: `network`, `rate-limited` with the wait time, `timed-out`.

**Rust**

```rust
let place = tree.location_picker().key("place").value(self.point).label("Paris, France")
    .search(true).allow_current(true).zoom(true);
// ("place", ActionKind::Changed, ActionValue::Geo(point)) => self.point = Some(point),
// ("place", ActionKind::Submitted, ActionValue::Geo(point)) => self.apply(point),
```

**TypeScript**

```ts
const place = locationPicker({
  key: 'place', value: state.point, label: 'Paris, France', search: true, allowCurrent: true, zoom: true,
});
// if (value.tag === 'geo') { if (kind === 'changed') state.point = value.val; else apply(value.val); }
```

For a settings page the same widget binds to a key:

**Rust**

```rust
let place = tree.location_picker().key("place").bind("home_location");
```

**TypeScript**

```ts
const place = locationPicker({ key: 'place', bind: 'home_location' });
```

### Choice pickers

The palette, dither and icon pickers are not separate widgets. They are a `choice` with `style: "dialog"`, a catalogue `source` and a `preview` on each item, see [Input](/pages/widgets-input/) for the full prop list. The host reads the items in pages as the user scrolls. The dialog filters with the `Search:` line, marks the current item with `●`, and shows the status line `N of M options · Current: X`.

#### Palette picker

The palette catalogue `ilium.palettes` holds at least 38 presets. Each item has a `swatch` preview of 1 to 16 colours, drawn as a strip of half blocks coloured with the stops of the palette. A scene or widget that uses colour reads the chosen palette and recolours itself. The first item keeps the scene colours.

<figure>
<pre class="term">╭  ≡ ● · Palette───────────────────────────────────────────╮
│ Search: _                                                │
│ <span class="t-hi">● Scene colors</span>                                           │
│   Rainbow          ▀▀▀▀▀▀▀▀▀▀▀▀                          │
│   Pastel rainbow   ▀▀▀▀▀▀▀▀▀▀▀▀                          │
│   Cotton candy     ▀▀▀▀▀▀▀▀▀▀▀▀                          │
│   Sea glass        ▀▀▀▀▀▀▀▀▀▀▀▀                          │
│   Sunset           ▀▀▀▀▀▀▀▀▀▀▀▀                          │
│ 38 of 38 options · Current: Scene colors                 │
│ Enter applies · Esc cancels                              │
╰──────────────────────────────────────────────────────────╯
</pre>
<figcaption>A choice over the ilium.palettes catalogue with swatch previews. The names are the first entries of the real list; each swatch is a strip of half blocks coloured with the stops of the palette (1 to 16 colours). The first entry keeps the scene colours and has no swatch.</figcaption>
</figure>

**Rust**

```rust
let palette = tree.choice("Palette", Vec::<String>::new(), 0).key("palette")
    .source("ilium.palettes").style(ChoiceStyle::Dialog).filter(true).selected_id(&self.palette);
```

**TypeScript**

```ts
const palette = choice('Palette', [], 0, {
  key: 'palette', source: 'ilium.palettes', style: 'dialog', filter: true, selectedId: state.palette,
});
```

#### Dither picker

The dither catalogue `ilium.dithers` lists every matrix and error-diffusion mode. Each item has a `pattern` preview of 1 to 4 lines of at most 8 cells, drawn by the host from the algorithm. Dithering is global in Ilium, so a plugin never implements it. It only offers the choice.

<figure>
<pre class="term">╭  ≡ ● · Dither────────────────────────────────────────────────────╮
│ Search: _                                                        │
│ <span class="t-hi">● Ordered (Bayer 8x8)</span>     ⡪⢕⡪⢕⡪⢕                                 │
│   Stippled                  ⠐⠀⠂⠀⠐⠀                               │
│   Coarse (Bayer 2x2)        ⡇⢸⡇⢸⡇⢸                               │
│   Halftone dots             ⠶⠶⠶⠶⠶⠶                               │
│   Scan lines                ⠉⠉⠉⠉⠉⠉                               │
│   Floyd-Steinberg                                                │
│   <span class="t-dim">Error diffusion can shimmer on moving scenes</span>                   │
│ 15 of 15 options · Current: Ordered (Bayer 8x8)                  │
│ Enter applies · Esc cancels                                      │
╰──────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>A choice over the ilium.dithers catalogue with pattern previews. Each preview is one to four lines of at most 8 cells that the host draws in the muted colour; the Braille patterns here are examples of what the host supplies. The labels are the real DitherMode names.</figcaption>
</figure>

**Rust**

```rust
let dither = tree.choice("Dither", Vec::<String>::new(), 0).key("dither")
    .source("ilium.dithers").style(ChoiceStyle::Dialog).filter(true).selected_id(&self.dither);
```

**TypeScript**

```ts
const dither = choice('Dither', [], 0, {
  key: 'dither', source: 'ilium.dithers', style: 'dialog', filter: true, selectedId: state.dither,
});
```

#### Icon picker

The icon catalogue `ilium.icons` holds about 12,000 glyphs. Each item has a `glyph` preview. With `view-modes` the dialog gets the `View` row `← List → +` that switches between a list and a grid. The glyphs are normal UTF-8 icons. The stable alternative appears only when the user has chosen it.

<figure>
<pre class="term">╭  ≡ ● · Icon──────────────────────────────────────────────╮
│ View <span class="t-dim">…………………………………………</span> ← List → +                         │
│ Search: rocket_                                          │
│ <span class="t-hi">● 🚀  Rocket</span>                                             │
│   🛰️  Satellite                                          │
│   📡  Antenna                                            │
│ 3 of 12041 options · Current: Rocket                     │
│ Enter applies · Esc cancels                              │
╰──────────────────────────────────────────────────────────╯
</pre>
<figcaption>The icon picker as a choice over the ilium.icons catalogue with glyph previews and a View switch between List and Grid. The glyphs are normal UTF-8 icons; the stable alternative is used only when the user has opted in.</figcaption>
</figure>

**Rust**

```rust
let icon = tree.choice("Icon", Vec::<String>::new(), 0).key("icon")
    .source("ilium.icons").style(ChoiceStyle::Dialog).filter(true)
    .view_modes(vec![ViewMode::List, ViewMode::Grid]).selected_id(&self.icon);
// ("icon", ActionKind::Changed, ActionValue::Text(id)) => self.icon = id,
```

**TypeScript**

```ts
const iconChoice = choice('Icon', [], 0, {
  key: 'icon', source: 'ilium.icons', style: 'dialog', filter: true,
  viewModes: ['list', 'grid'], selectedId: state.icon,
});
```

### Worked example: a home location row

The plugin keeps one point and shows its place name. The user picks a place on the map, and the plugin converts the point to text.

**Rust**

```rust
impl Views for Home {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        let mut tree = Tree::new();
        let place = tree.location_picker().key("place").value(self.point).search(true).zoom(true);
        let accent = tree.color_picker(&self.accent).key("accent");
        let rows = tree.layout(Direction::Vertical)
            .constraints(vec![Constraint::Min(12), Constraint::Length(1)])
            .child(place).child(accent);
        tree.finish(rows)
    }
}
impl Handler for Home {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::UiAction(action) = event else { continue };
            match (action.key.as_str(), action.kind, action.value) {
                ("place", ActionKind::Changed, ActionValue::Geo(point)) => self.point = Some(point),
                ("accent", ActionKind::Changed, ActionValue::Text(hex)) => self.accent = hex,
                _ => {}
            }
        }
        Vec::new()
    }
}
```

**TypeScript**

```ts
render() {
  return tree(layout(
    { direction: 'vertical', constraints: [{ tag: 'min', val: 12 }, { tag: 'length', val: 1 }] },
    [
      locationPicker({ key: 'place', value: state.point, search: true, zoom: true }),
      colorPicker(state.accent, { key: 'accent' }),
    ],
  ));
},
onEvents(batch) {
  for (const event of batch) {
    if (event.tag !== 'ui-action') continue;
    const { key, kind, value } = event.val;
    if (key === 'place' && kind === 'changed' && value.tag === 'geo') state.point = value.val;
    if (key === 'accent' && kind === 'changed' && value.tag === 'text') state.accent = value.val;
  }
  return [];
},
```

### Edge cases and errors

| Case | Result |
|---|---|
| A `color-picker` value that is not `#rrggbb` | The field is drawn in the error colour. The dialog starts from the last valid colour. |
| A `path-field` without the `fs.pick` grant | The button is disabled and the reason is its tooltip. |
| A `location-picker` with `search` and no `location.geocode` | The Address box is hidden. Coordinates mode still works. |
| The user declines the geocoding consent | The search returns `not-permitted` and the status line says so. |
| A latitude outside -90 to 90, or a longitude outside -180 to 180 | The number entry shows its error and nothing is sent. |
| A `bind` key that is not an own `location` key | The tree is invalid: the render traps with `ui.invalid-tree`. `render` has no result type. |
| A catalogue source that does not exist | The dialog shows an empty list and the status line `0 of 0 options`. |
| A stale item id on commit | The host refuses it and keeps the old value. |

### See also

- [Input](/pages/widgets-input/)
- [Collections](/pages/widgets-collections/)
- [Widget catalogue](/pages/widgets-index/)
- [Catalogues](/pages/api-catalogs/)
- [Geocoding](/pages/api-geocode/)
- [Location](/pages/api-location/)
- [Settings screens](/pages/settings-screens/)
- [fs.pick](/pages/capability-fs-pick/)
