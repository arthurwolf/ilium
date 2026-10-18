---
layout: page
title: Ambient look
permalink: /pages/core-plugin-ambient/
---

## Ambient look

The Ambient look plugin (id `ilium.core.ambient`, side: client) is the plugin form of the shared look of animated backgrounds. It owns the colour, palette, dithering and display controls that every scene gets, the Animations settings tab with its previews, the shared location that sky and map scenes read, and the semantic animation choice that picks a scene from what the workspace is doing.

Painting the background and applying colour, filters and dithering to each cell is done by the client kernel. The plugin supplies the controls and the preview. The scenes themselves are in the [Scenes plugin](/pages/core-plugin-scenes/), and they follow the look chosen here without any colour code of their own.

### What it covers

- **Shared look of animations.** Colour mode, palette, filters, brightness, contrast and more, as described under The shared look in the Animations page of the user documentation.
- **Shared dithering and display controls.** Dithering, panel placement, frame-rate cap, speed and density (see Shared dithering and Shared display controls).
- **Semantic animation and the Animations tab.** The scene list with previews, and the Loop and Live playback modes (see Semantic animation and Playback: Loop and Live).

### See it

<figure><img src="/assets/demos/26-animations.gif" alt="The Animations settings tab changes the scene and the shared look of the background" loading="lazy"><figcaption>The Animations tab: pick a scene, then change colour, palette and dithering. The preview updates as the controls change.</figcaption></figure>

### What the user sees

<figure><pre class="term">╭─ Animations ─────────────────────────────────╮
│ <span class="t-hi">Scene</span>  ▸ Stars   Clouds   Pipes   Video      │
│                                              │
│ <span class="t-ok">⣿⣷⣶⣤⣀⠀⠀⢀⣠⣴⣶⣿⣿⣶⣤⣀⠀⠀⠀⢀⣠⣴⣶⣿⣷⣶⣤⡀</span>                 │
│ <span class="t-dim">Preview, live</span>                                │
│ Colour mode …………………………………………………………… ‹ Color ›│
│ Palette ………………………………………………………………………… ‹ Dusk ›│
│ Dithering …………………………………………………………… ‹ Bayer 4 ›│
│ Frame-rate cap ………………………………………………… ‹ 24 fps ›│
│                                              │
╰──────────────────────────────────────────────╯</pre><figcaption>Rendered at 48 columns. Illustrative. The scene list on top, a preview row, then the shared look controls with leaders. The preview glyphs depend on the icon set.</figcaption></figure>
<figure><pre class="term">╭─ Location ───────────────────────────────────╮
│ <span class="t-dim">⣀⣤⣤⣀⠀⠀⠀⢀⣠⣤⣤⣤⣄⣀⠀⠀⠀⠀⣀⣤⣤⣀⠀⠀⠀⠀⣀⣤⣀⠀⠀⠀⠀⠀</span>           │
│ <span class="t-dim">⠀⠀⣿⣿⣿⣷⣶⣶⣿⣿⣿⣿⣿⣿⣷⣶⣤⣀⠀⣿⣿⣿⣿⣷⣶⣶⣿⣿⣿⣿⣷⠀⠀⠀</span>           │
│ <span class="t-dim">⠀⠀⠀⠈⠛⠿⣿⣿⣿⣿⡿⠟⠁⠀⠀⠈⠛⠿⠀⠀⠛⠿⣿⣿⡿⠛⠁⠀⠀⠈⠀⠀⠀⠀</span>           │
│ <span class="t-dim">⠀⠀⠀⠀⠀⠀⠈⠙⠛⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠈⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀</span>           │
│ <span class="t-err">●</span> <span class="t-hi">Lisbon, Portugal</span>                           │
│ Latitude ………………………………………………………………………… 38.7223│
│ Longitude ……………………………………………………………………… -9.1393│
│                                              │
╰──────────────────────────────────────────────╯</pre><figcaption>Rendered at 48 columns. Illustrative. The location picker draws the world as a Braille map. The marker shows the chosen point and the label is the geocoded place name.</figcaption></figure>
### Manifest

This block is a fragment. It lists the contributions. The whole manifest is in [Complete manifest](#complete-manifest).

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.ambient",
  "name": "Ambient look",
  "version": "0.1.0",
  "publisher": "Ilium",
  "client": {
    "entry": "client.wasm",
    "world": "ilium:client/base@0.1.0",
    "exports": ["views"]
  },
  "capabilities": {
    "client": ["location.geocode", "media.video", "ui.panels"]
  },
  "contributes": {
    "panels": [{ "id": "gallery", "title": "Scenes", "modal": false }],
    "settings": [
      {
        "kind": "tab", "id": "animations", "title": "Animations", "sections": [
          { "id": "look", "title": "Look", "scope": "client", "rows": [
            { "type": "choice", "key": "scene", "title": "Scene", "default": "stars",
              "options": [{ "value": "stars", "title": "Stars" },
                          { "value": "clouds", "title": "Clouds" }] },
            { "type": "preview", "view": "scene-preview", "height": 4, "live": true },
            { "type": "toggle", "key": "semantic", "title": "Semantic animation",
              "default": false },
            { "type": "location", "key": "observer", "title": "Observer location",
              "search": true, "allow_current": true, "precision": 4,
              "default": { "lat": 38.7223, "lon": -9.1393, "label": "Lisbon" } },
            { "type": "map", "key": "saved_places", "title": "Saved places",
              "value_type": "string", "key_pattern": "^[a-z-]{1,24}$",
              "max_entries": 8, "key_title": "Name", "value_title": "Label", "default": {} }
          ] }
        ]
      }
    ]
  }
}
```

### Complete manifest

This is the whole manifest of the plugin at `ilium:plugin@0.1.0`. The Manifest section above shows the contributions in detail; here the required keys, the sides, the `capabilities` per side and the `activation` triggers are all present. Activation follows [startup and lazy activation](/pages/reference-kernel-and-boot/#startup-and-lazy-activation-of-the-30-core-plugins). [COR-050]

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.ambient",
  "name": "Ambient look",
  "version": "0.1.0",
  "publisher": "Ilium",
  "description": "The Ambient look plugin (id ilium.core.ambient, side: client) is the plugin form of the shared look of animated backgrounds.",
  "license": "MIT",
  "client": {
    "entry": "client.wasm",
    "world": "ilium:client/base@0.1.0",
    "exports": [
      "views"
    ]
  },
  "capabilities": {
    "client": [
      "location.geocode",
      "media.video",
      "ui.panels",
      "settings.write-kernel:animations"
    ]
  },
  "activation": [
    "startup"
  ],
  "contributes": {
    "panels": [
      {
        "id": "gallery",
        "title": "Scenes",
        "modal": false
      }
    ],
    "settings": [
      {
        "kind": "tab",
        "id": "animations",
        "title": "Animations",
        "sections": [
          {
            "id": "look",
            "title": "Look",
            "scope": "client",
            "rows": [
              {
                "type": "choice",
                "key": "scene",
                "title": "Scene",
                "default": "stars",
                "options": [
                  {
                    "value": "stars",
                    "title": "Stars"
                  },
                  {
                    "value": "clouds",
                    "title": "Clouds"
                  }
                ]
              },
              {
                "type": "preview",
                "view": "scene-preview",
                "height": 4,
                "live": true
              },
              {
                "type": "toggle",
                "key": "semantic",
                "title": "Semantic animation",
                "default": false
              },
              {
                "type": "location",
                "key": "observer",
                "title": "Observer location",
                "search": true,
                "allow_current": true,
                "precision": 4,
                "default": {
                  "lat": 38.7223,
                  "lon": -9.1393,
                  "label": "Lisbon"
                }
              },
              {
                "type": "map",
                "key": "saved_places",
                "title": "Saved places",
                "value_type": "string",
                "key_pattern": "^[a-z-]{1,24}$",
                "max_entries": 8,
                "key_title": "Name",
                "value_title": "Label",
                "default": {}
              }
            ]
          }
        ]
      }
    ]
  },
  "limits_tier": "first-party"
}
```

### Kernel tabs this plugin owns

This plugin is the single owner of the following built-in Settings tabs. It holds `settings.write-kernel:<tab>` for each one (T1 only, one holder per tab, see [kernel settings](/pages/api-kernel-settings-and-keymap/#writing-a-kernel-tab)). The values stay in the same `config.toml` tables as before, so no user setting moves. [COR-030] [COR-033]

| Tab id | Label | Config table | Readers |
|---|---|---|---|
| `animations` | Animations | project YAML `animation` | Scenes |

If the plugin is disabled, the tab stays in the rail with its stored values shown read-only and the notice "Ambient look is disabled. Values are kept." The kernel keeps reading the keys it consumes itself (listed in the Consumer column of the key table in [config and data migration](/pages/reference-config-and-data-migration/)). Behaviour that only this plugin provides is off. The exact result for this plugin is its row in [kernel and boot](/pages/reference-kernel-and-boot/). [COR-022]

### Samples

#### 1. Preview in Settings

A `preview` row is bound to the scene the user chose, and a panel draws the same frames on a `surface`. The plugin writes whole rows of pixels with `write_surface`. See [media and embedded widgets](/pages/widgets-media-and-embedded/) and [settings rows](/pages/reference-settings-rows/).

**Rust**

```rust
use ilium_plugin_sdk::host::ui;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::ui::{PixelFormat, ViewTarget};
use ilium_plugin_sdk::ui::build::Tree;

// Manifest row: { "type": "preview", "view": "scene-preview", "height": 4, "live": true }
impl Views for AmbientPanel {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        let mut tree = Tree::new();
        let preview = tree
            .surface(PixelFormat::Rgb8)
            .min_columns(40)
            .min_rows(4)
            .key("frame");
        tree.finish(preview)
    }
}

impl AmbientPanel {
    /// Called when the preview scene produced a new frame of `rows` full rows.
    fn show_frame(&self, pixels: &[u8]) -> Result<(), PluginError> {
        ui::write_surface(ViewTarget::Panel("preview".into()), "frame", 0, PixelFormat::Rgb8, pixels)
    }
}
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import { writeSurface } from '@ilium/plugin-sdk/host/ui';
import { surface, tree } from '@ilium/plugin-sdk/ui/build';

// Manifest row: { "type": "preview", "view": "scene-preview", "height": 4, "live": true }
export const { lifecycle, handler, views } = definePlugin({
  side: 'client',
  activate() {},
  onEvents() { return []; },
  render() {
    return tree(surface({ key: 'frame', format: 'rgb8', minColumns: 40, minRows: 4 }));
  },
});

/** Called when the preview scene produced a new frame of whole rows. */
export function showFrame(pixels: Uint8Array): void {
  writeSurface({ tag: 'panel', val: 'preview' }, 'frame', 0, 'rgb8', pixels);
}
```

#### 2. Shared location

One `location` row holds the observer position for every scene that needs it, and a `map` row keeps saved places. In a panel the `location-picker` binds to the row and geocodes a typed place name. The geocoder is [the geocode API](/pages/api-geocode/) with the [location.geocode grant](/pages/capability-location-geocode/); other pages used are [settings rows](/pages/reference-settings-rows/) and [pickers](/pages/widgets-pickers/). Place names are sent to the geocoding provider, so the settings row should say so.

**Rust**

```rust
use ilium_plugin_sdk::host::geocode::{self, GeocodeQuery};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::ui::build::Tree;

// Manifest rows: "location" (key "observer") and "map" (key "saved_places").
impl Views for AmbientPanel {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        let mut tree = Tree::new();
        let place = tree.location_picker().key("place").bind("observer").search(true);
        tree.finish(place)
    }
}

/// Starts a place search; the places arrive after `request-completed`.
fn find_place(text: &str) -> Result<RequestId, PluginError> {
    geocode::search(&GeocodeQuery { text: text.into(), near: None, limit: 5, language: Some("en".into()) })
}

fn place_names(request: RequestId) -> Vec<String> {
    geocode::take_places(request).unwrap_or_default().into_iter().map(|place| place.name).collect()
}
```

**TypeScript**

```ts
import { search, takePlaces } from '@ilium/plugin-sdk/host/geocode';
import { locationPicker, tree } from '@ilium/plugin-sdk/ui/build';

// Manifest rows: "location" (key "observer") and "map" (key "saved_places").
export function render() {
  return tree(locationPicker({ key: 'place', bind: 'observer', search: true }));
}

/** Starts a place search; the places arrive after request-completed. */
export const findPlace = (text: string) =>
  search({ text, near: undefined, limit: 5, language: 'en' });

export const placeNames = (request: bigint): string[] =>
  (takePlaces(request) ?? []).map((place) => place.name);
```

#### 3. Scene gallery

An `image` thumbnail for each scene and a `video` clip for the selected one. Both come from the package's assets. See [media and embedded widgets](/pages/widgets-media-and-embedded/) and the [media.video grant](/pages/capability-media-video/).

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::ui::{MediaFit, ImageSource, VideoSource};
use ilium_plugin_sdk::ui::build::Tree;

impl Views for AmbientPanel {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        let mut tree = Tree::new();
        let thumb = tree
            .image(ImageSource::Asset(format!("thumbs/{}.png", self.selected)))
            .fit(MediaFit::Contain)
            .key("thumb");
        let clip = tree
            .video(VideoSource::Asset(format!("clips/{}.webm", self.selected)))
            .autoplay(true)
            .looped(true)
            .fit(MediaFit::Cover)
            .key("clip");
        let both = tree
            .layout(Direction::Horizontal, [Constraint::Length(20), Constraint::Fill(1)])
            .child(thumb).child(clip);
        tree.finish(both)
    }
}
```

**TypeScript**

```ts
import { fill, image, layout, length, tree, video } from '@ilium/plugin-sdk/ui/build';

let selected = 'stars';

export function render() {
  const thumb = image({ tag: 'asset', val: `thumbs/${selected}.png` }, { key: 'thumb', fit: 'contain' });
  const clip = video({ tag: 'asset', val: `clips/${selected}.webm` }, {
    key: 'clip', autoplay: true, loop: true, fit: 'cover',
  });
  return tree(layout(
    { direction: 'horizontal', constraints: [length(20), fill(1)] },
    [thumb, clip],
  ));
}
```

#### 4. Semantic animation

When the user turns semantic animation on, the plugin proposes a scene. It keeps the last suggestion in global storage so the same choice returns after a restart. How the suggestion is chosen from the workspace state is the plugin's own logic. See [storage](/pages/api-storage/) and the [event catalogue](/pages/api-events/).

**Rust**

```rust
use ilium_plugin_sdk::host::{settings, storage::{self, Bucket}};
use ilium_plugin_sdk::prelude::*;

const LAST_SCENE: &str = "last_suggestion";

impl Handler for AmbientPlugin {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::SettingsChanged(change) = event else { continue };
            if !change.keys.iter().any(|key| key == "semantic") {
                continue;
            }
            let values: serde_json::Value =
                serde_json::from_str(&settings::get_json()).unwrap_or_default();
            if values["semantic"].as_bool() != Some(true) {
                continue;
            }
            let last = storage::get(Bucket::Global, LAST_SCENE)
                .and_then(|bytes| String::from_utf8(bytes).ok())
                .unwrap_or_else(|| "stars".into());
            self.suggestion = Some(last); // offered as the starting scene
        }
        Vec::new()
    }
}

fn remember(scene: &str) -> Result<(), PluginError> {
    storage::put(Bucket::Global, LAST_SCENE, scene.as_bytes())
}
```

**TypeScript**

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { getJson } from '@ilium/plugin-sdk/host/settings';
import { get, put } from '@ilium/plugin-sdk/host/storage';

const LAST_SCENE = 'last_suggestion';
let suggestion: string | undefined;

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  activate() {},
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag !== 'settings-changed' || !event.val.keys.includes('semantic')) continue;
      const values = JSON.parse(getJson()) as { semantic?: boolean };
      if (values.semantic !== true) continue;
      const bytes = get('global', LAST_SCENE);
      suggestion = bytes ? new TextDecoder().decode(bytes) : 'stars'; // the starting scene
    }
    return [];
  },
});

export function remember(scene: string): void {
  put('global', LAST_SCENE, new TextEncoder().encode(scene));
}
```

### Extension points

Other plugins change what `ilium.core.ambient` does through the points below. The plugin owns each point (grant `extensions.provide:ilium.core.ambient/<point>`). Contributors need `extensions.contribute:ilium.core.ambient/<point>`. See [Extension points](/pages/api-extension-points/) for the mechanism and [Extending core plugins](/pages/extending-core-plugins/) for the five ways to work with a core feature.

| Point | Kind | What it does | Payload | Risk |
|---|---|---|---|---|
| `palettes` | collect | Add a palette to the shared palette choice. | entry: id, title, stops (list of colours) | standard |
| `style-presets` | collect | Add a style preset: a full set of shared look values. | entry: id, title, values | standard |

**Services** (grant `services.use:<name>`):

- `ilium.core.ambient/look`: get: current shared look and palette

**Topics** (list under `contributes.topics.subscribes`):

- `ilium.core.ambient/look-changed`: { palette, mode }

Every scene receives the palette in its constructor.

**Replaceable:** yes. A replacement lists `"replaces": ["ilium.core.ambient"]` and provides the whole surface (see [Extending core plugins](/pages/extending-core-plugins/#replacing-a-core-plugin)).

#### Example: Adds a four-colour palette

A package adds this entry in its manifest. It needs no code.

```json
{
  "api": "ilium:plugin@0.1.0",
  "capabilities": { "client": ["extensions.contribute:ilium.core.ambient/palettes"] },
  "contributes": {
    "extensions": [
      {
        "point": "ilium.core.ambient/palettes",
        "id": "harbour",
        "priority": 50,
        "entry": {
          "title": "Harbour",
          "stops": [
            "#0b1d2a",
            "#1f6f8b",
            "#99d5c9",
            "#f6f1d1"
          ]
        }
      }
    ]
  }
}
```

### ABI used

- [Media and embedded widgets](/pages/widgets-media-and-embedded/) and [media.video](/pages/capability-media-video/)
- [Settings rows](/pages/reference-settings-rows/)
- [Geocode API](/pages/api-geocode/) and [location.geocode](/pages/capability-location-geocode/)
- [Pickers](/pages/widgets-pickers/)
- [Storage](/pages/api-storage/)
- [Events](/pages/api-events/)

### See also

- [Core plugins](/pages/core-plugins/)
- [Scenes plugin](/pages/core-plugin-scenes/), the scenes that use this look
- [Settings plugin](/pages/core-plugin-settings/)
