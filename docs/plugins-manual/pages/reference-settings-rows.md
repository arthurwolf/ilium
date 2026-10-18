---
layout: page
title: Settings row gallery
permalink: /pages/reference-settings-rows/
---

## Settings row gallery

This page shows every row type of the Settings screen, as the kernel draws it. There are 18 row types: the first 12 are the base rows and the last 6 are `location`, `preview`, `notice`, `map`, `multi-choice` and `meter`. Each entry has the value type, the fields, the validation, how the value is written, a render in each state, a manifest example, and the declaration in Rust and in TypeScript. A row is declared in the manifest, so both SDKs share one JSON declaration. The two languages differ only in how the plugin reads the value and calls the row-update functions.

The field tables of the manifest are in the [settings schema reference](/pages/reference-settings-schema/). The calls that update rows at run time are in [settings](/pages/api-settings/). Tabs, sections and anchors are in the [settings extension](/pages/settings-extension/), and custom pages are in [Building settings screens](/pages/settings-screens/).

### How to read the renders

Every render uses the glyphs that the real client draws. The panel is the content frame of a plugin tab, 59 columns wide. The rail and the header are shown on the [settings extension](/pages/settings-extension/) page.

- The label sits at the left and the control at the right. Between them are grey dotted leaders made of `…`, with one blank cell on each side. A gap shorter than five cells stays blank.
- The description is the dim line under the label, with a rule of `─` under the row.
- The selected row is drawn in the accent colours, lavender with a near-black foreground. There is no marker glyph.
- A dim `•` after the label means the value differs from the default. The `r` key resets the focused row.
- A dim `◌` after the value means a write is pending or the validate callback is running.
- `↻` after the value marks `applies: "restart"`, and the description reads `Applies after restart.`
- A failed change keeps the old value and replaces the description with the message of the failure, in the warning style.
- A row with a false `depends_on` stays visible and focusable, but is read-only: the control is dim and the description reads `Unavailable: <title> is <value>.`
- A row with a false `visible_when` is not drawn at all. It keeps its value, and search skips it. Nothing of a hidden row can be rendered, so the gallery shows the rows before and after instead.

The controls use these shapes:

| Control | Shape |
|---|---|
| Toggle, `switch` style | `‹ On ›` and `‹ Off ›` |
| Toggle, `checkbox` style | `[x]` and `[ ]` |
| Choice, `stepper` style | `← value → +` |
| Choice, `list` style and `dialog` style | `‹ value ›` |
| Number, `stepper` style | `- 42 + *` |
| Number, `slider` style | a `─` track with a `●` thumb, then the number control |
| Text, path, colour, key chord, list, map | `‹ value ›` |
| Secret text | `••••` plus the last characters |
| Button | `[ Label ]` |
| Scrollbar | `│` track with a `┃` thumb |

### Declaring and reading values

Every value row has a `key` of the form `[a-z][a-z0-9_]{0,63}`, a `title` of at most 48 characters and a `default`. These fields are common to all value rows: `description` (at most 200 characters, wrapped to 2 lines), `help` (a `contributes.help` id), `depends_on`, `visible_when`, `scope`, `applies`, `keywords` (at most 4) and `reset`.

The plugin reads all its values with one call and deserialises them into one type. The examples below show only the fields of the row that is being described. All of them belong to the same struct or interface.

**Rust**

```rust
use ilium_plugin_sdk::host::settings;
use serde::Deserialize;

#[derive(Deserialize, Default)]
struct Config {
    // one field per value row, named by the row key
}

fn load_config() -> Config {
    serde_json::from_str(&settings::get_json()).unwrap_or_default()
}
```

**TypeScript**

```ts
import { getJson } from '@ilium/plugin-sdk/host/settings';

interface Config {
  // one property per value row, named by the row key
}

export function loadConfig(): Config {
  return JSON.parse(getJson()) as Config;
}
```

Every write is a merge patch (RFC 7396). A key set to `null` resets to the default. `set-json` writes at once and does not call `settings-validate`.

### 1. toggle

A boolean. The `switch` style is the default and the `checkbox` style suits a list of related flags.

| Item | Value |
|---|---|
| Value | boolean |
| Fields | `on_label`, `off_label` (the text of the two states), `style`: `"switch"` or `"checkbox"` |
| Validation | `type: boolean`; a non-boolean in `set-json` is `invalid-argument` |
| Written | at once |

<figure>
<pre class="term">╭  ≡ ● · Toggle states──────────────────────────────────╮
│  Keep history ………………………………………………………………………………… ‹ On ›  │
│<span class="t-dim">    Record text copied in panes and the editor.        </span>│
│<span class="t-dim">  ──────────────────────                               </span>│
│<span class="t-hi">  Show preview • ………………………………………………………………………… ‹ Off ›  </span>│
│<span class="t-dim">    Selected: the accent colours mark the focused row. </span>│
│<span class="t-dim">  ──────────────────────                               </span>│
│  Sync ………………………………………………………………………………………………… ‹ On › ◌  │
│<span class="t-dim">    Pending: the write has not finished yet.           </span>│
│<span class="t-dim">  ──────────────────────                               </span>│
│  Sync interval …………………………………………………………………………… ‹ Off ›  │
│<span class="t-dim">    Unavailable: Keep history is Off.                  </span>│
│<span class="t-dim">  ──────────────────────                               </span>│
│  Cloud backup …………………………………………………………………………… ‹ On › ↻  │
│<span class="t-dim">    Applies after restart.                             </span>│
│<span class="t-dim">  ──────────────────────                               </span>│
│  Notify ………………………………………………………………………………………………………… [x]  │
│<span class="t-dim">    Checkbox style, on.                                </span>│
│<span class="t-dim">  ──────────────────────                               </span>│
│  Notify sound ………………………………………………………………………………………… [ ]  │
│<span class="t-dim">    Checkbox style, off.                               </span>│
╰───────────────────────────────────────────────────────╯</pre>
<figcaption>Toggle row states: idle, selected and differing from the default, pending, read-only through <code>depends_on</code>, restart marker, and the checkbox style. The <code>•</code> after <em>Show preview</em> says that its value is not the default.</figcaption>
</figure>
**Manifest**

```json
{ "type": "toggle", "key": "keep_history", "title": "Keep history",
  "description": "Record text copied in panes and the editor.",
  "default": true }
{ "type": "toggle", "key": "notify", "title": "Notify", "style": "checkbox",
  "default": false, "on_label": "Yes", "off_label": "No" }
```

**Rust**

```rust
#[derive(Deserialize)]
struct Config {
    #[serde(default = "default_true")]
    keep_history: bool,
    #[serde(default)]
    notify: bool,
}

fn default_true() -> bool { true }

fn disable_history() -> Result<(), PluginError> {
    settings::set_json(r#"{"keep_history": false}"#)
}
```

**TypeScript**

```ts
import { setJson } from '@ilium/plugin-sdk/host/settings';

interface Config {
  keep_history: boolean;
  notify: boolean;
}

export function disableHistory(): void {
  setJson(JSON.stringify({ keep_history: false }));
}
```

### 2. choice

One value out of a list of options. The `stepper` style shows `← value → +`, the `list` style opens a list, and the `dialog` style opens the filterable choice dialog on Enter.

| Item | Value |
|---|---|
| Value | string or integer |
| Fields | `options`: `[{ value, title }]` (2 to 64), `style`: `"stepper"`, `"list"` or `"dialog"`, `dynamic` |
| Option fields | `description` (120), `disabled_reason` (40), `preview`, `help` |
| Catalogue and filter | `source: { "catalog": "<id>" }`, `filter`, `allow_custom`, `custom_pattern`, `view_modes` |
| Validation | `enum` with `enum_titles`; a value that is not an option is `invalid-argument`; a `choice` row with both or neither of `options` and `source` is `manifest.settings-source`; an unknown catalogue is `manifest.catalog-unknown` |
| Written | at once |

<figure>
<pre class="term">╭  ≡ ● · Choice states──────────────────────────────────╮
│  Store …………………………………………………………………………… ← Text only → +  │
│<span class="t-dim">    Stepper style: arrows step, + opens the list.      </span>│
│<span class="t-dim">  ──────────────────────                               </span>│
│<span class="t-hi">  Density ……………………………………………………………………… ‹ Comfortable ›  </span>│
│<span class="t-dim">    List style and dialog style show the value.        </span>│
│<span class="t-dim">  ──────────────────────                               </span>│
│  Region • …………………………………… ‹ Frankfurt (unavailable) ›  │
│<span class="t-dim">    The stored value is no longer offered.             </span>│
╰───────────────────────────────────────────────────────╯</pre>
<figcaption>Choice rows in the three styles. The last row shows a stored value that a <code>set-choices</code> call no longer offers.</figcaption>
</figure>
The dialog style opens the choice dialog of the kernel. The current option is marked with `●`. A disabled option carries `[unavailable]` and its reason on the line under it, and it cannot be chosen. The thumb of the scrollbar is `┃`.

<figure>
<pre class="term">╭  ≡ ● · Region──────────────────────────────╮
│ Filter: fra                                │
│                                            │
│ ● Frankfurt                                │
│   Europe, Germany                          │
│   Frankfurt [unavailable]                  │
│   Unavailable: no capacity left            │
│   Paris                                    │
│ Esc cancel · Enter apply                   │
╰────────────────────────────────────────────╯</pre>
<figcaption>The choice dialog of a <code>dialog</code> style row, with a filter, the current option, and a disabled option with its reason.</figcaption>
</figure>
Previews decorate options in the dialog. A `preview` is one of `{ "glyph": "x" }`, `{ "swatch": ["#rrggbb", ...] }` (at most 16 colours) or `{ "pattern": ["line", ...] }` (at most 4 lines of 8 cells). A catalogue source supplies them: `ilium.palettes` has swatch previews and `ilium.dithers` has pattern previews. The dither picker and the palette picker of the animation settings are these two catalogues.

<figure>
<pre class="term">╭  ≡ ● · Dither──────────────────────────────╮
│ Filter:                                    │
│ ● Bayer 4x4           █░█░                 │
│   Blue noise          ░█░░                 │
│   Scan lines          ████                 │
│ Esc cancel · Enter apply                   │
╰────────────────────────────────────────────╯</pre>
<figcaption>A catalogue choice with pattern previews, as in the dither row of <code>ilium.dithers</code>. The pattern cells come from the catalogue.</figcaption>
</figure>
**Manifest**

```json
{ "type": "choice", "key": "store", "title": "Store", "default": "text",
  "options": [
    { "value": "text", "title": "Text only" },
    { "value": "all", "title": "Everything",
      "description": "Images are kept as thumbnails." },
    { "value": "images", "title": "Images", "disabled_reason": "Needs 2 GiB" }
  ] }
{ "type": "choice", "key": "palette", "title": "Palette", "style": "dialog",
  "source": { "catalog": "ilium.palettes" }, "default": "sunset" }
{ "type": "choice", "key": "region", "title": "Region", "dynamic": true,
  "default": "fra", "options": [ { "value": "fra", "title": "Frankfurt" },
                                  { "value": "par", "title": "Paris" } ] }
```

**Rust**

```rust
use ilium_plugin_sdk::host::settings;
use ilium_plugin_sdk::types::CatalogItem;

#[derive(Deserialize)]
struct Config {
    store: String,
    palette: String,
    region: String,
}

fn offer_regions(names: &[(&str, &str)]) -> Result<(), PluginError> {
    let items: Vec<CatalogItem> = names
        .iter()
        .map(|(id, title)| CatalogItem::new(id, title))
        .collect();
    settings::set_choice_items("region", &items)
}
```

**TypeScript**

```ts
import { setChoiceItems } from '@ilium/plugin-sdk/host/settings';

interface Config {
  store: 'text' | 'all' | 'images';
  palette: string;
  region: string;
}

export function offerRegions(names: Array<[string, string]>): void {
  setChoiceItems('region', names.map(([id, title]) => ({
    id, title, keywords: [],
  })));
}
```

`set-choices` replaces the options of a `dynamic` row with `value` and `title` pairs. `set-choice-items` replaces them with full catalogue records, so a dynamic row can carry descriptions, previews and disabled reasons.

### 3. number

An integer or a number with a range. The `stepper` style is `- 42 + *` and the `slider` style adds a `─` track with a `●` thumb. In an area narrower than 12 cells the slider falls back to the stepper.

| Item | Value |
|---|---|
| Value | integer or number |
| Fields | `minimum`, `maximum` (required), `step` (default 1), `unit` (at most 8), `integer` (default true) |
| Fields | `style`: `"stepper"` or `"slider"`, `steps` (2 to 64 ascending numbers inside the range, replaces `step`), `special_values` (at most 8 `{ value, title }` pairs), `debounce_ms` (300 to 2,000) |
| Validation | `minimum`, `maximum`, `step`; `integer: true` rejects `4.5`; `steps` compile to `x-steps` and `special_values` to `x-special-values`, which are UI annotations only |
| Written | 600 ms after the last step, or `debounce_ms`; Enter writes at once |

The `*` button opens the entry dialog, in which the user types the number. The dialog also opens on Enter.

<figure>
<pre class="term">╭  ≡ ● · Number stepper states──────────────────────────╮
│  Maximum entries …………………………………………………………… -  500  + *  │
│<span class="t-dim">    Oldest entries are removed first.                  </span>│
│<span class="t-dim">  ──────────────────────                               </span>│
│<span class="t-hi">  Zoom • …………………………………………………………………………………… -  1.0  + *  </span>│
│<span class="t-dim">    Selected, differs from the default.                </span>│
│<span class="t-dim">  ──────────────────────                               </span>│
│  Sync interval ………………………………………………………………… -  5 s  + *  │
│<span class="t-dim">    Unavailable: Sync is Off.                          </span>│
│<span class="t-dim">  ──────────────────────                               </span>│
│  Retention ……………………………………………………………………………… - Off  + *  │
│<span class="t-dim">    A special value is drawn instead of the number.    </span>│
│<span class="t-dim">  ──────────────────────                               </span>│
│  Speed ………………………………………………………………………………… -  1.5  + * ◌  │
│<span class="t-dim">    Pending: a step was made 200 ms ago.               </span>│
╰───────────────────────────────────────────────────────╯</pre>
<figcaption>Stepper rows. The last row waits for the 600 ms debounce before it is written. A special value (<code>0</code> as <em>Off</em>) replaces the number in the control.</figcaption>
</figure>
<figure>
<pre class="term">╭  ≡ ● · Number slider──────────────────────────────────╮
│  Density                                              │
│  ───●──────────────────────  -  60  + *               │
│<span class="t-dim">    Share of cells that carry a star.                  </span>│
│<span class="t-dim">  ──────────────────────                               </span>│
│  Speed                                                │
│<span class="t-hi">  ──────────●───────────────  -  1.0  + *              </span>│
│<span class="t-dim">    Ladder steps: 0, 0.25, 0.5, 1, 2, 4.               </span>│
╰───────────────────────────────────────────────────────╯</pre>
<figcaption>The slider style: label, then the track with its thumb, then the stepper. The second row is selected.</figcaption>
</figure>
**Manifest**

```json
{ "type": "number", "key": "max_entries", "title": "Maximum entries",
  "minimum": 10, "maximum": 5000, "step": 10, "default": 500 }
{ "type": "number", "key": "speed", "title": "Speed", "style": "slider",
  "integer": false, "minimum": 0, "maximum": 4, "default": 1,
  "steps": [0, 0.25, 0.5, 1, 2, 4],
  "special_values": [ { "value": 0, "title": "Paused" } ] }
```

**Rust**

```rust
#[derive(Deserialize)]
struct Config {
    max_entries: u32,
    speed: f64,
}

fn slow_down(config: &Config) -> Result<(), PluginError> {
    let patch = serde_json::json!({ "speed": config.speed / 2.0 });
    settings::set_json(&patch.to_string())
}
```

**TypeScript**

```ts
import { setJson } from '@ilium/plugin-sdk/host/settings';

interface Config {
  max_entries: number;
  speed: number;
}

export function slowDown(config: Config): void {
  setJson(JSON.stringify({ speed: config.speed / 2 }));
}
```

### 4. text

A single-line string. With `secret: true` the row shows `••••`, and with `reveal_last` it also shows the last characters.

| Item | Value |
|---|---|
| Value | string |
| Fields | `max_length` (at most 4,096, default 256), `pattern`, `placeholder`, `secret` |
| Fields | `reveal_last` (0 to 8, only with `secret`), `debounce_ms` (300 to 2,000) |
| Validation | `maxLength`, `pattern`, and the optional `settings-validate` export |
| Written | 600 ms after the last keystroke, or at once on Enter or when the row loses focus |

<figure>
<pre class="term">╭  ≡ ● · Text states────────────────────────────────────╮
│  Label ……………………………………………………………………………………………… ‹ work ›  │
│<span class="t-dim">    Shown in the picker title; 40 characters.          </span>│
│<span class="t-dim">  ──────────────────────                               </span>│
│<span class="t-hi">  Ignore regex …………………………………………………………………… ‹ ^\d{4}$ ›  </span>│
│<span class="t-dim">    Selected.                                          </span>│
│<span class="t-dim">  ──────────────────────                               </span>│
│  Token • …………………………………………………………………………… ‹ •••• a1b2 ›  │
│<span class="t-dim">    Secret, reveal_last 4: last characters show.       </span>│
│<span class="t-dim">  ──────────────────────                               </span>│
│  Webhook ………………………………………………………………… ‹ ••••••••••••• ›  │
│<span class="t-dim">    Secret without reveal_last: dots only.             </span>│
│<span class="t-dim">  ──────────────────────                               </span>│
│  Pattern ………………………………………………………………………………………………… ‹ ( ›  │
│<span class="t-warn">    That is not a valid regular expression.            </span>│
╰───────────────────────────────────────────────────────╯</pre>
<figcaption>Text rows: plain, selected, two secrets, and a value that <code>settings-validate</code> refused. The refused value is reverted and the message replaces the description.</figcaption>
</figure>
A secret value is redacted in logs and in the snapshot. Other plugins that read the values with `get-plugin-json` see an empty string.

**Manifest**

```json
{ "type": "text", "key": "label", "title": "Label", "default": "",
  "max_length": 40, "placeholder": "work" }
{ "type": "text", "key": "token", "title": "Token", "secret": true,
  "reveal_last": 4, "default": "" }
```

**Rust**

```rust
#[derive(Deserialize)]
struct Config {
    label: String,
    token: String,
}

fn authorised(config: &Config) -> bool {
    !config.token.is_empty()
}
```

**TypeScript**

```ts
interface Config {
  label: string;
  token: string;
}

export function authorised(config: Config): boolean {
  return config.token.length > 0;
}
```

### 5. text-area

A multi-line string. Enter opens an editor dialog, and the row shows the number of lines.

| Item | Value |
|---|---|
| Value | string |
| Fields | `max_length` (at most 65,536, default 16,384), `language` |
| Fields | `debounce_ms` (300 to 2,000) |
| Validation | `maxLength`; compiles to `type: string` with `multiline: true` |
| Written | 600 ms after the last keystroke, or 1 s when the text is above 10,000 bytes (`debounce_ms` overrides both), and at once when the editor closes |

<figure>
<pre class="term">╭  ≡ ● · Text area states───────────────────────────────╮
│  Paste template ……………………………………………………………… ‹ 3 lines ›  │
│<span class="t-dim">    Enter opens the editor; saved after 1 s.           </span>│
│<span class="t-dim">  ──────────────────────                               </span>│
│<span class="t-hi">  Header ………………………………………………………………………………………… ‹ empty ›  </span>│
│<span class="t-dim">    Selected.                                          </span>│
╰───────────────────────────────────────────────────────╯</pre>
<figcaption>Text-area rows. The value is the line count; the text is edited in the dialog.</figcaption>
</figure>
<figure>
<pre class="term">╭  ≡ ● · Paste template──────────────────────╮
│ {{ text }}                                 │
│ Copied at {{ time }}                       │
│                                            │
│ Esc cancel · Enter newline · Ctrl+S save   │
╰────────────────────────────────────────────╯</pre>
<figcaption>The editor dialog of a text area. Closing it writes the value at once.</figcaption>
</figure>
**Manifest**

```json
{ "type": "text-area", "key": "template", "title": "Paste template",
  "max_length": 2000, "language": "markdown", "default": "{{ text }}" }
```

**Rust**

```rust
#[derive(Deserialize)]
struct Config {
    template: String,
}

fn render_entry(config: &Config, text: &str) -> String {
    config.template.replace("{{ text }}", text)
}
```

**TypeScript**

```ts
interface Config {
  template: string;
}

export function renderEntry(config: Config, text: string): string {
  return config.template.replace('{{ text }}', text);
}
```

### 6. path

A file or a directory chosen with the kernel file picker.

| Item | Value |
|---|---|
| Value | string |
| Fields | `picker`: `"file"` or `"directory"`, `extensions` |
| Validation | `type: string`; the extension list is checked on confirm |
| Written | on confirm |

The row stores the path as text. It grants no file access: the plugin needs its own `fs` grant to open the path, and the path must lie inside a scope it holds.

<figure>
<pre class="term">╭  ≡ ● · Path states────────────────────────────────────╮
│  History file …………………………………………………………… ‹ ~/clips.db ›  │
│<span class="t-dim">    File picker; other folders need fs access.         </span>│
│<span class="t-dim">  ──────────────────────                               </span>│
│<span class="t-hi">  Export folder ………………………………………………………………………… ‹ none ›  </span>│
│<span class="t-dim">    No value yet.                                      </span>│
╰───────────────────────────────────────────────────────╯</pre>
<figcaption>Path rows. Enter opens the file picker of the kernel.</figcaption>
</figure>
**Manifest**

```json
{ "type": "path", "key": "history_file", "title": "History file",
  "picker": "file", "extensions": ["db"], "default": "" }
```

**Rust**

```rust
#[derive(Deserialize)]
struct Config {
    history_file: String,
}

fn history_path(config: &Config) -> Option<&str> {
    (!config.history_file.is_empty()).then_some(config.history_file.as_str())
}
```

**TypeScript**

```ts
interface Config {
  history_file: string;
}

export function historyPath(config: Config): string | undefined {
  return config.history_file === '' ? undefined : config.history_file;
}
```

### 7. color

A colour as `#rrggbb`. Enter opens the colour dialog with a hue strip, swatches and a hex field.

| Item | Value |
|---|---|
| Value | string `#rrggbb` |
| Fields | `swatches`: `"theme"` or up to 16 colours |
| Validation | `pattern: ^#[0-9a-fA-F]{6}$` |
| Written | on confirm |

<figure>
<pre class="term">╭  ≡ ● · Colour states──────────────────────────────────╮
│  Highlight colour ………………………………………………… ‹ ██ #4a9eff ›  │
│<span class="t-dim">    Colour of the newest entry in the picker.          </span>│
│<span class="t-dim">  ──────────────────────                               </span>│
│<span class="t-hi">  Border colour • …………………………………………………… ‹ ██ #9d7cd8 ›  </span>│
│<span class="t-dim">    Selected.                                          </span>│
╰───────────────────────────────────────────────────────╯</pre>
<figcaption>Colour rows. The block before the hex value is drawn in the chosen colour.</figcaption>
</figure>
**Manifest**

```json
{ "type": "color", "key": "highlight", "title": "Highlight colour",
  "swatches": "theme", "default": "#4a9eff" }
```

**Rust**

```rust
#[derive(Deserialize)]
struct Config {
    highlight: String,
}

fn rgb(config: &Config) -> Option<(u8, u8, u8)> {
    let hex = config.highlight.strip_prefix('#')?;
    let value = u32::from_str_radix(hex, 16).ok()?;
    Some(((value >> 16) as u8, (value >> 8) as u8, value as u8))
}
```

**TypeScript**

```ts
interface Config {
  highlight: string;
}

export function rgb(config: Config): [number, number, number] {
  const value = parseInt(config.highlight.slice(1), 16);
  return [(value >> 16) & 255, (value >> 8) & 255, value & 255];
}
```

### 8. keybinding

A key chord for one of the plugin's own commands. The row remaps the `keybindings` entry of that command.

| Item | Value |
|---|---|
| Value | string chord |
| Fields | `command` (own command id, required) |
| Validation | the kernel chord grammar |
| Written | on confirm; the user's remap of that command's `keybindings` entry |

The capture dialog reads the next chord. With the reserved-key check, a chord that the kernel already binds shows `! used by <command>`.

<figure>
<pre class="term">╭  ≡ ● · Key chord states───────────────────────────────╮
│  Picker key ………………………………………………………………… ‹ Ctrl+Alt+V ›  │
│<span class="t-dim">    Opens the history picker.                          </span>│
│<span class="t-dim">  ──────────────────────                               </span>│
│<span class="t-hi">  Clear key ………………………………………………………………………………… ‹ unset ›  </span>│
│<span class="t-dim">    Selected.                                          </span>│
╰───────────────────────────────────────────────────────╯</pre>
<figcaption>Key chord rows. Enter opens the capture dialog.</figcaption>
</figure>
<figure>
<pre class="term">╭  ≡ ● · Picker key──────────────────────────╮
│ Press the new key chord.                   │
│                                            │
│ Ctrl+Alt+V                                 │
│ ! used by Paste in pane                    │
│ Esc cancel · Enter apply                   │
╰────────────────────────────────────────────╯</pre>
<figcaption>The capture dialog. The warning line appears when the chord is bound by the kernel.</figcaption>
</figure>
**Manifest**

```json
{ "type": "keybinding", "key": "picker_key", "title": "Picker key",
  "command": "open-picker", "default": "Ctrl+Alt+V" }
```

**Rust**

```rust
#[derive(Deserialize)]
struct Config {
    picker_key: String,
}

fn picker_chord(config: &Config) -> &str {
    &config.picker_key
}
```

**TypeScript**

```ts
interface Config {
  picker_key: string;
}

export function pickerChord(config: Config): string {
  return config.picker_key;
}
```

### 9. button

Runs one of the plugin's own commands on its declared side. The row holds no value.

| Item | Value |
|---|---|
| Value | none |
| Fields | `label` (at most 20), `command` (own command id), `confirm` |
| Validation | the command must exist in `contributes.commands` (`manifest.settings-command`) |
| Written | nothing is written; the command runs |

With `confirm` set, the kernel asks first. The question uses the confirm dialog of the kernel, with the buttons ` N ` and ` Y ` and the hint `Y/N answer · Enter confirm · Esc cancel`.

<figure>
<pre class="term">╭  ≡ ● · Button states──────────────────────────────────╮
│  Clear history …………………………………………………………… [ Clear now ]  │
│<span class="t-dim">    Deletes every stored entry at once.                </span>│
│<span class="t-dim">  ──────────────────────                               </span>│
│<span class="t-hi">  Export ……………………………………………………………………………………… [ Export ]  </span>│
│<span class="t-dim">    Selected.                                          </span>│
╰───────────────────────────────────────────────────────╯</pre>
<figcaption>Button rows. A read-only button (false <code>depends_on</code>) is dim and does not run.</figcaption>
</figure>
<figure>
<pre class="term">╭  ≡ ● · Clear history───────────────────────╮
│ Delete every stored entry?                 │
│                                            │
│      N No          Y Yes                   │
│ Y/N answer · Enter confirm · Esc cancel    │
╰────────────────────────────────────────────╯</pre>
<figcaption>The confirmation of a button with <code>confirm</code>. The buttons are drawn as filled colour blocks in the real client.</figcaption>
</figure>
**Manifest**

```json
{ "type": "button", "id": "clear_now", "title": "Clear history",
  "label": "Clear now", "command": "clear-history",
  "confirm": "Delete every stored entry?",
  "description": "Deletes every stored entry at once." }
```

A button has an `id` instead of a `key`, because it stores no value.

**Rust**

```rust
fn on_command(&mut self, command: &str) -> Vec<Submission> {
    if command == "clear-history" {
        self.entries.clear();
    }
    Vec::new()
}
```

**TypeScript**

```ts
onCommand(command: string): Submission[] {
  if (command === 'clear-history') entries.length = 0;
  return [];
}
```

### 10. info

A read-only text line. With `live: true` the plugin updates it while Settings is open. With `rich` spans it can carry tones, bold text and clickable commands.

| Item | Value |
|---|---|
| Value | none |
| Fields | `text` (at most 120), `live` |
| Fields | `rich`: spans `{ text, tone?, bold?, command? }`, at most 120 characters in total, replacing `text`; a span with `command` (an own command id) is clickable |
| Validation | the 120 character limit; an unknown `command` is `manifest.settings-command` |
| Written | nothing |

<figure>
<pre class="term">╭  ≡ ● · Info states────────────────────────────────────╮
│  Stored entries ……………………………………………………………… 312 entries  │
│<span class="t-dim">    Updated by the plugin while it runs.               </span>│
│<span class="t-dim">  ──────────────────────                               </span>│
│  Account …………………………………………………………………… Signed in as ada  │
│<span class="t-dim">    Rich spans: a tone, bold and a command.            </span>│
│<span class="t-dim">  ──────────────────────                               </span>│
│  Quota ……………………………………………………………………………………… ! 90 % used  │
│<span class="t-warn">    Warning tone from set-info-rich.                   </span>│
╰───────────────────────────────────────────────────────╯</pre>
<figcaption>Info rows. The first is a live row set by <code>set-info</code>. In the second, <em>Signed in as ada</em> is a clickable span. The third uses the warning tone.</figcaption>
</figure>
**Manifest**

```json
{ "type": "info", "id": "stored", "title": "Stored entries",
  "text": "0 entries", "live": true }
{ "type": "info", "id": "account", "title": "Account",
  "rich": [ { "text": "Signed in as " },
            { "text": "ada", "bold": true, "command": "show-account" } ] }
```

**Rust**

```rust
use ilium_plugin_sdk::host::settings;

fn show_count(count: usize) -> Result<(), PluginError> {
    settings::set_info("stored", &format!("{count} entries"))
}
```

**TypeScript**

```ts
import { setInfo } from '@ilium/plugin-sdk/host/settings';

export function showCount(count: number): void {
  setInfo('stored', `${count} entries`);
}
```

`set-info-rich(key, text)` takes a rich-text value instead of a string, for an `info` row with `live: true`. Calls to the `set-info` family are limited to 10 per second per plugin.

### 11. group

A titled group of rows, with an optional fold. The group holds no value, and it cannot contain another group.

| Item | Value |
|---|---|
| Value | none |
| Fields | `title`, `rows` (1 to 12 children, no nested `group`), `collapsed` |
| Validation | `manifest.settings-limit` above 12 children in a group (a section holds up to 24 rows, a group counting as one row plus its children); a nested group is `manifest.settings-row-field` |
| Written | nothing |

<figure>
<pre class="term">╭  ≡ ● · Group──────────────────────────────────────────╮
│  ▾ Advanced                                           │
│    Sync between panes ………………………………………………………… ‹ Off ›  │
│<span class="t-dim">      Needs Keep history.                              </span>│
│<span class="t-dim">    ──────────────────────                             </span>│
│    Sync interval …………………………………………………………… -  5 s  + *  │
│<span class="t-dim">      Unavailable: Sync between panes is Off.          </span>│
╰───────────────────────────────────────────────────────╯</pre>
<figcaption>A group with a toggle and a dependent number. The number is read-only because its <code>depends_on</code> is false.</figcaption>
</figure>
**Manifest**

```json
{ "type": "group", "title": "Advanced", "collapsed": false, "rows": [
  { "type": "toggle", "key": "sync", "title": "Sync between panes",
    "default": false },
  { "type": "number", "key": "sync_interval", "title": "Sync interval",
    "minimum": 1, "maximum": 60, "unit": "s", "default": 5,
    "depends_on": { "key": "sync", "equals": true } }
] }
```

**Rust**

```rust
#[derive(Deserialize)]
struct Config {
    sync: bool,
    sync_interval: u32,
}

fn interval(config: &Config) -> Option<u32> {
    config.sync.then_some(config.sync_interval)
}
```

**TypeScript**

```ts
interface Config {
  sync: boolean;
  sync_interval: number;
}

export function interval(config: Config): number | undefined {
  return config.sync ? config.sync_interval : undefined;
}
```

### 12. list

An array of primitive items or of small objects. The `rows` style opens the list editor, and the `inline` style is one text field joined by a separator.

| Item | Value |
|---|---|
| Value | array |
| Fields | `item` (one primitive type, or an object of primitive fields), `max_items` (at most 64) |
| Fields | `item_format`: `glob`, `url`, `host`, `path`, `regex` or `chord`; `style`: `"rows"` or `"inline"`; `separator` (default `;`); `reorderable` (default true); `remote`; `item_status`; object items with up to 8 fields (was 4) |
| Validation | `type: array`, `items`, `maxItems`; an item is checked on commit against its type and `item_format`; an invalid item is shown with `✗` and the reason and is not written |
| Written | on each committed item |

<figure>
<pre class="term">╭  ≡ ● · List states────────────────────────────────────╮
│  Ignore patterns …………………………………………………………… ‹ 3 items ›  │
│<span class="t-dim">    Copies that match are never stored.                </span>│
│<span class="t-dim">  ──────────────────────                               </span>│
│<span class="t-hi">  Allowed hosts ……………………………………………… ‹ a.test; b.test ›  </span>│
│<span class="t-dim">    Inline style, joined with ;.                       </span>│
╰───────────────────────────────────────────────────────╯</pre>
<figcaption>List rows in the rows style and in the inline style.</figcaption>
</figure>
<figure>
<pre class="term">╭  ≡ ● · Ignore patterns───────────────────────╮
│   *.log                                      │
│   secret-*                                   │
│ ✗ [a-z  Not a valid glob: unclosed class     │
│   *.tmp ◌                                    │
│                                              │
│ Enter edit · a add · d delete · Esc close    │
╰──────────────────────────────────────────────╯</pre>
<figcaption>The list editor with <code>item_format: glob</code>. The third item failed its check and is not written. The last item is pending (<code>remote</code> rows wait for <code>set-item-states</code>).</figcaption>
</figure>
Object items are edited as one row of fields. The fields are primitive types, and an object has at most 8 of them.

**Manifest**

```json
{ "type": "list", "key": "ignore", "title": "Ignore patterns",
  "item": { "type": "string" }, "item_format": "glob",
  "max_items": 32, "default": ["*.log"] }
{ "type": "list", "key": "hosts", "title": "Allowed hosts", "style": "inline",
  "separator": ";", "item": { "type": "string" }, "item_format": "host",
  "default": [] }
```

**Rust**

```rust
use ilium_plugin_sdk::host::settings;
use ilium_plugin_sdk::types::{RowState, RowStateKind, ItemState};

#[derive(Deserialize)]
struct Config {
    ignore: Vec<String>,
    hosts: Vec<String>,
}

fn confirm_item(index: u32) -> Result<(), PluginError> {
    let state = RowState { kind: RowStateKind::Idle, message: None };
    settings::set_item_states("ignore", &[ItemState { index, state }])
}
```

**TypeScript**

```ts
import { setItemStates } from '@ilium/plugin-sdk/host/settings';

interface Config {
  ignore: string[];
  hosts: string[];
}

export function confirmItem(index: number): void {
  setItemStates('ignore', [{ index, state: { kind: 'idle' } }]);
}
```

### 13. location

A place on Earth. Enter opens the world-map picker of the kernel.

| Item | Value |
|---|---|
| Value | object `{ lat, lon, label? }` |
| Fields | `search` (default true), `allow_current` (default false, needs `location.observer` on the client side), `precision` (decimals 0 to 6, default 4) |
| Validation | `lat` from -90 to 90, `lon` from -180 to 180, `label` at most 120 characters; both `lat` and `lon` are required |
| Written | when the user confirms `Use location` |

The address search needs `location.geocode`. The host owns the geocoding provider and asks the user once for consent to send place names. Without consent the search returns `not-permitted`.

<figure>
<pre class="term">╭  ≡ ● · Location states────────────────────────────────╮
│  Observer ………………………………………… ‹ Paris 48.8566, 2.3522 ›  │
│<span class="t-dim">    Where the stars are computed for.                  </span>│
│<span class="t-dim">  ──────────────────────                               </span>│
│<span class="t-hi">  Home ………………………………………………………………………………………… ‹ not set ›  </span>│
│<span class="t-dim">    Selected.                                          </span>│
╰───────────────────────────────────────────────────────╯</pre>
<figcaption>Location rows. The label is shown first and the coordinates follow with the row's precision.</figcaption>
</figure>
<figure>
<pre class="term">╭  ≡ ● · Observer location─────────────────────────╮
│ Address  Coordinates                             │
│ Paris, France                                    │
│ ⣿⣿⣷⣶⣤⣀⠀⠀⠀⣀⣤⣶⣿⣷⣶⣤⣀⠀⠀⠀⠀⣀⣤⣶⣿⣿⣿⣷⣤⣀⠀⠀⣀⣤⣶⣿             │
│ ⠀⠀⠙⠻⣿⣿⣿⣷⣤⣄⣀⣤⣶⣿⡿⠋⠀⠀⠀⠀⠈⠙⠻⣿⣿⣿⣿⣿⣷⣶⣤⣴⣿⣿⡿⠋             │
│ ⠀⠀⠀⠀⠀⠈⠙⠻⠿⠿⠿⠟⠋⠁⠀⠀⠀⠀⠀⠀⠀⠀⠙⠻⠿⠿⠿⠟⠋⠁⠀⠀⠀⠀⠀⠀             │
│ 48.8566, 2.3522                 Use location     │
│ Esc cancel · Enter apply                         │
╰──────────────────────────────────────────────────╯</pre>
<figcaption>The location picker. The Braille land mask, the Address and Coordinates modes and the <code>Use location</code> button are the kernel's; the map shown here is schematic.</figcaption>
</figure>
**Manifest**

```json
{ "type": "location", "key": "observer", "title": "Observer location",
  "search": true, "allow_current": true, "precision": 4,
  "default": { "lat": 48.8566, "lon": 2.3522, "label": "Paris" } }
```

**Rust**

```rust
#[derive(Deserialize)]
struct Place {
    lat: f64,
    lon: f64,
    label: Option<String>,
}

#[derive(Deserialize)]
struct Config {
    observer: Place,
}
```

**TypeScript**

```ts
interface Place {
  lat: number;
  lon: number;
  label?: string;
}

interface Config {
  observer: Place;
}
```

### 14. preview

A live area that the plugin draws with one of its own `panels` views. It is for the effect of a setting, such as a colour scheme or a sound. It exists only in the plugin's own tabs (SE10).

| Item | Value |
|---|---|
| Value | none |
| Fields | `view` (an own `panels` id drawn with `views.render(panel(<id>))`), `height` (4 to 24 rows, default 8), `live` (default false; true lets the plugin call `ui.invalidate` while Settings is open) |
| Validation | the view must exist; a `preview` row in a built-in tab is `manifest.settings-row-type` |
| Written | nothing |

The view may use display widgets, `schema-form`, `button` and `location-picker`. It may not embed other input widgets.

<figure>
<pre class="term">╭  ≡ ● · Preview row────────────────────────────────────╮
│  Preview                                              │
│  ╭──────────────────────────────────────────────╮     │
│  │  Newest entry      ██ #4a9eff                │     │
│  │  https://example.test/a/long/path            │     │
│  │  Older entry       ██ #9d7cd8                │     │
│  ╰──────────────────────────────────────────────╯     │
│<span class="t-dim">    Redrawn while the colour dialog is open.           </span>│
╰───────────────────────────────────────────────────────╯</pre>
<figcaption>A preview row of height 4. The plugin owns everything inside the rounded frame. The frame belongs to the view the plugin returns.</figcaption>
</figure>
**Manifest**

```json
{ "type": "preview", "view": "colour-preview", "height": 4, "live": true }
```

**Rust**

```rust
use ilium_plugin_sdk::host::ui;

fn colours_changed(&mut self) {
    ui::invalidate("colour-preview");
}
```

**TypeScript**

```ts
import { invalidate } from '@ilium/plugin-sdk/host/ui';

export function coloursChanged(): void {
  invalidate('colour-preview');
}
```

### 15. notice

A banner inside a section. It tells the user something that is not a value, such as a privacy note.

| Item | Value |
|---|---|
| Value | none |
| Fields | `id` (required), `text` (at most 200 characters, rich spans allowed), `tone`: `info`, `success`, `warning` or `danger` (default `info`), `dismissible` (default false) |
| Validation | the text limit; `id` is unique in the plugin (`manifest.settings-id`) |
| Written | a dismissal is stored per `id` in the plugin's settings table and survives restarts |

<figure>
<pre class="term">Warning, dismissible:
╭─ ! ───────────────────────────────────── [x] ─╮
│ Place names are sent to the geocoding         │
│ provider.                                     │
╰───────────────────────────────────────────────╯

Info, fixed:
╭─ ℹ ───────────────────────────────────────────╮
│ Values are stored in the project folder.      │
╰───────────────────────────────────────────────╯</pre>
<figcaption>Notice rows. The frame is the one of the privacy notice of the real client (rounded corners, <code>[x]</code> to dismiss). The tone sets the colour of the text and of the glyph.</figcaption>
</figure>
**Manifest**

```json
{ "type": "notice", "id": "privacy", "tone": "warning", "dismissible": true,
  "text": "Place names are sent to the geocoding provider." }
```

A notice has no value, so the plugin does not read it. A plugin cannot show or hide a notice at run time; use `visible_when` on a value row for that.

**Rust**

```rust
// Nothing to read: the notice has no key. The plugin only declares it.
```

**TypeScript**

```ts
// Nothing to read: the notice has no key. The plugin only declares it.
```

### 16. map

A dictionary of string keys, for environment variables or header names.

| Item | Value |
|---|---|
| Value | object of string keys |
| Fields | `value_type`: `string`, `integer`, `number` or `boolean`; `key_pattern` (regex, default `^[^\\s]{1,64}$`); `max_entries` (1 to 64, default 16); `key_title`, `value_title` (at most 24 each) |
| Validation | compiles to `additionalProperties`, `propertyNames` and `maxProperties` |
| Written | on each committed entry |

<figure>
<pre class="term">╭  ≡ ● · Map states─────────────────────────────────────╮
│  Environment ………………………………………………………………… ‹ 2 entries ›  │
│<span class="t-dim">    Passed to the formatter.                           </span>│
│<span class="t-dim">  ──────────────────────                               </span>│
│<span class="t-hi">  Headers …………………………………………………………………………… ‹ 0 entries ›  </span>│
│<span class="t-dim">    Selected.                                          </span>│
╰───────────────────────────────────────────────────────╯</pre>
<figcaption>Map rows show the number of entries. Enter opens the entry editor.</figcaption>
</figure>
<figure>
<pre class="term">╭  ≡ ● · Environment───────────────────────────╮
│ Name                 Value                   │
│ LANG                 C.UTF-8                 │
│ FORMAT               compact                 │
│                                              │
│ Enter edit · a add · d delete · Esc close    │
╰──────────────────────────────────────────────╯</pre>
<figcaption>The entry editor of a map. The two column titles come from <code>key_title</code> and <code>value_title</code>.</figcaption>
</figure>
**Manifest**

```json
{ "type": "map", "key": "environment", "title": "Environment",
  "value_type": "string", "key_pattern": "^[A-Z_]{1,32}$",
  "max_entries": 16, "key_title": "Name", "value_title": "Value",
  "default": {} }
```

**Rust**

```rust
use std::collections::BTreeMap;

#[derive(Deserialize)]
struct Config {
    environment: BTreeMap<String, String>,
}
```

**TypeScript**

```ts
interface Config {
  environment: Record<string, string>;
}
```

### 17. multi-choice

A set of options out of a list. The `checklist` style draws the options inline, and the `dialog` style opens the choice dialog with check marks.

| Item | Value |
|---|---|
| Value | array of option values |
| Fields | `options` (2 to 64, the `choice` option shape) or `source`, `min_items` (default 0), `max_items` (default the option count), `style`: `"checklist"` (default) or `"dialog"` |
| Validation | `items: { enum }`, `uniqueItems: true`, `minItems`, `maxItems` |
| Written | at once on each toggle |

<figure>
<pre class="term">╭  ≡ ● · Multi-choice───────────────────────────────────╮
│  Capture kinds                                        │
│    [x] Text                                           │
│    [x] Images                                         │
│    [ ] Files                                          │
│<span class="t-dim">    Pick at least one.                                 </span>│
│<span class="t-dim">  ──────────────────────                               </span>│
│  Targets …………………………………………………………………… ‹ Text, Images ›  │
│<span class="t-dim">    Dialog style: Enter opens the list.                </span>│
╰───────────────────────────────────────────────────────╯</pre>
<figcaption>A checklist and a dialog style row. The rule of <code>min_items</code> shows as the note under the list.</figcaption>
</figure>
**Manifest**

```json
{ "type": "multi-choice", "key": "kinds", "title": "Capture kinds",
  "min_items": 1, "default": ["text", "images"],
  "options": [ { "value": "text", "title": "Text" },
               { "value": "images", "title": "Images" },
               { "value": "files", "title": "Files" } ] }
```

**Rust**

```rust
#[derive(Deserialize)]
struct Config {
    kinds: Vec<String>,
}

fn captures_images(config: &Config) -> bool {
    config.kinds.iter().any(|kind| kind == "images")
}
```

**TypeScript**

```ts
interface Config {
  kinds: Array<'text' | 'images' | 'files'>;
}

export function capturesImages(config: Config): boolean {
  return config.kinds.includes('images');
}
```

### 18. meter

A segmented bar for a cost or a quota. The plugin updates it while Settings is open.

| Item | Value |
|---|---|
| Value | none |
| Fields | `id` (required), `title`, `segments` (initial, at most 8), `max` (default 1), `show_values` (default false) |
| Validation | at most 8 segments; a segment is `{ label, value, tone }` |
| Written | nothing; updated with `set-meter` |

<figure>
<pre class="term">╭  ≡ ● · Meter──────────────────────────────────────────╮
│  Budget used …………………………………………………… ██████░░░░ 3.1 / 5  │
│<span class="t-dim">    Updated by the plugin.                             </span>│
│<span class="t-dim">  ──────────────────────                               </span>│
│  Quota ………………………………………………………………………………………… ████░░░░░░  │
│<span class="t-dim">    Segments differ by colour, not by glyph.           </span>│
╰───────────────────────────────────────────────────────╯</pre>
<figcaption>Meter rows. The filled part is drawn with <code>█</code> and the empty part with <code>░</code>, both from the progress bars of the real client.</figcaption>
</figure>
**Manifest**

```json
{ "type": "meter", "id": "budget", "title": "Budget used", "max": 5,
  "show_values": true,
  "segments": [ { "label": "used", "value": 0, "tone": "info" } ] }
```

**Rust**

```rust
use ilium_plugin_sdk::host::settings;
use ilium_plugin_sdk::types::{MeterSegment, Tone};

fn show_budget(used: f64) -> Result<(), PluginError> {
    settings::set_meter("budget", &[MeterSegment {
        label: "used".into(), value: used, tone: Tone::Info,
    }])
}
```

**TypeScript**

```ts
import { setMeter } from '@ilium/plugin-sdk/host/settings';

export function showBudget(used: number): void {
  setMeter('budget', [{ label: 'used', value: used, tone: 'info' }]);
}
```

### Fields every value row shares

`visible_when`, `scope` and `applies` work on all value rows. They compile to the UI annotations `x-visible-when`, `x-scope` and `x-applies`, which never take part in validation.

#### visible_when

`visible_when` uses the `depends_on` shape, `{ key, equals | in | not }`, on one own key. The difference is what the user sees. A false `depends_on` leaves the row on the screen and makes it read-only. A false `visible_when` removes the row, keeps its value, and skips it in search.

<figure>
<pre class="term">╭  ≡ ● · Stars on───────────────────────────────────────╮
│<span class="t-hi">  Stars …………………………………………………………………………………………………… ‹ On ›  </span>│
│<span class="t-dim">    Show stars in the background.                      </span>│
│<span class="t-dim">  ──────────────────────                               </span>│
│  Star count …………………………………………………………………… -  300  + * ↻  │
│<span class="t-dim">    Applies after restart.                             </span>│
╰───────────────────────────────────────────────────────╯</pre>
<figcaption><code>stars</code> is On, so <code>star_count</code> is drawn. It also has <code>applies: "restart"</code>.</figcaption>
</figure>
<figure>
<pre class="term">╭  ≡ ● · Stars off──────────────────────────────────────╮
│<span class="t-hi">  Stars • …………………………………………………………………………………………… ‹ Off ›  </span>│
│<span class="t-dim">    Show stars in the background.                      </span>│
╰───────────────────────────────────────────────────────╯</pre>
<figcaption><code>stars</code> is Off. The row <code>star_count</code> is gone, and the rows below it move up. Its value of 300 is kept.</figcaption>
</figure>
**Manifest**

```json
{ "type": "number", "key": "star_count", "title": "Stars", "minimum": 10,
  "maximum": 2000, "default": 300, "applies": "restart",
  "visible_when": { "key": "stars", "equals": true } }
```

Hidden rows make the row positions jump under the cursor. Use `depends_on` when the user should see where a setting lives, and `visible_when` when the row would only confuse.

#### scope and applies

`scope: "project"` stores the value in the project's `.ilium/` settings under `[plugins."<plugin-id>"]` instead of the user configuration. The project value wins while that project is open. A `settings-changed` event carries the side that owns the changed keys in its `side` field. `applies: "restart"` draws `↻` after the value and the note `Applies after restart.` The plugin reads the new value only after it restarts.

**Rust**

```rust
fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
    for event in batch {
        let Event::SettingsChanged(change) = event else { continue };
        // change.side tells which side owns the keys; change.origin says who wrote them
        self.reload(&change.keys);
    }
    Vec::new()
}
```

**TypeScript**

```ts
onEvents(batch: Event[]): Submission[] {
  for (const event of batch) {
    if (event.tag !== 'settings-changed') continue;
    reload(event.val.keys);
  }
  return [];
}
```

### The controls of the Settings screen, by row type

The Settings screen draws these controls. Each is a style or a variant of one of the 18 row types, and no control has a row type of its own.

| Control | Row type and field | Notes |
|---|---|---|
| Toggle | `toggle` | `‹ On ›`; `style: "checkbox"` draws `[x]` |
| Stepper | `number`, `style: "stepper"` | `- 42 + *` |
| Slider | `number`, `style: "slider"` | `─` track with a `●` thumb plus the stepper |
| Number | `number` | the entry dialog opens on `*` or Enter |
| Choice | `choice` | `← value → +`, with the dialog on `+` |
| Dither | `choice` with `source: { "catalog": "ilium.dithers" }` | 15 algorithms with pattern previews |
| Palette | `choice` with `source: { "catalog": "ilium.palettes" }` | 38 presets with swatch previews |
| Colour | `color` | hex field, swatches and a hue strip |
| Key chord | `keybinding`, `list` with `item_format: "chord"` | the capture dialog |
| Glob list and URL list | `list` with `item_format: "glob"` or `"url"` | invalid items show `✗` and the reason |
| Masked secret | `text` with `secret: true` | `••••` plus `reveal_last` characters |
| Path | `path` | the kernel file picker |
| Text | `text`, `text-area` | one line or an editor dialog |
| Button | `button` | runs an own command |
| Info | `info` | live text, rich spans |
| Location | `location` | the world-map picker |

### Row calls at run time

These calls need no grant. They cover only the plugin's own rows. They are limited to 10 calls per second per plugin, all kinds together. A call over the rate is refused with `rate-limited`; nothing is coalesced.

| Call | Rust | TypeScript | Applies to |
|---|---|---|---|
| `set-info` | `settings::set_info(key, text)` | `setInfo(key, text)` | `info` with `live: true` |
| `set-info-rich` | `settings::set_info_rich(key, text)` | `setInfoRich(key, text)` | `info` with `live: true` |
| `set-choices` | `settings::set_choices(key, options)` | `setChoices(key, options)` | `choice` with `dynamic: true` |
| `set-choice-items` | `settings::set_choice_items(key, items)` | `setChoiceItems(key, items)` | `choice` and `multi-choice` with a dynamic source |
| `set-row-note` | `settings::set_row_note(key, note)` | `setRowNote(key, note)` | any row, at most 120 characters |
| `set-row-state` | `settings::set_row_state(key, state)` | `setRowState(key, state)` | any value row; `idle`, `loading`, `pending`, `error`, message at most 80 characters |
| `set-item-states` | `settings::set_item_states(key, items)` | `setItemStates(key, items)` | `list` items |
| `set-meter` | `settings::set_meter(id, segments)` | `setMeter(id, segments)` | `meter`, at most 8 segments |

A call with an unknown key or id is `not-found`. A call for the wrong row type is `invalid-argument`.

### Limits

| Limit | Value |
|---|---|
| Tabs per plugin | 2 |
| Sections per plugin tab | 8 |
| Rows per section | 24 |
| Rows per group | 12 |
| Value keys per plugin | 128 |
| Choice options, list items | 64 |
| Text value, text-area value | 4,096 and 65,536 characters |
| Object item fields | 8 |
| All values of one plugin | 256 KiB |

The full table, with the error codes, is in [Errors and limits](/pages/reference-errors-and-limits/).

### See also

- [Settings schema reference](/pages/reference-settings-schema/) for the manifest tables.
- [settings](/pages/api-settings/) for the runtime calls.
- [Plugin settings extension](/pages/settings-extension/) for tabs, sections and the rail.
- [Building settings screens](/pages/settings-screens/) for rows and custom pages.
- [Tutorial 10: a new settings tab](/pages/tutorial-10-settings-new-tab/) and [Tutorial 11: extend a built-in tab](/pages/tutorial-11-settings-extend-tab/) for complete plugins.
