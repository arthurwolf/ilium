---
layout: page
title: Settings schema reference
permalink: /pages/reference-settings-schema/
---

## Settings schema reference

This page lists every key of `contributes.settings`: tab and section entries, the eighteen row types, save rules, events, limits and error codes. For the model, read [Plugin settings extension](/pages/settings-extension/). Every row is shown in every state on the [row gallery](/pages/reference-settings-rows/).

### You will need

- The manifest layout: [Manifest reference](/pages/reference-manifest/).
- The runtime calls: [Settings API](/pages/api-settings/).
- A worked build: [Tutorial 10](/pages/tutorial-10-settings-new-tab/) adds a tab, and [Tutorial 11](/pages/tutorial-11-settings-extend-tab/) adds a section to a built-in tab.

<figure>
<svg viewBox="0 0 420 230" role="img" aria-label="Schema structure: a tab entry holds sections, and each section holds rows">
  <defs>
    <marker id="schema-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
      <path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="6" y="92" width="108" height="60" rx="6" fill="none" stroke="currentColor" stroke-width="1.5"/>
  <text x="60" y="116" font-size="11" text-anchor="middle" fill="currentColor">Tab entry</text>
  <text x="60" y="134" font-size="10" text-anchor="middle" fill="currentColor">id, title, render</text>
  <rect x="146" y="12" width="116" height="50" rx="6" fill="none" stroke="currentColor" stroke-width="1.5"/>
  <text x="204" y="32" font-size="11" text-anchor="middle" fill="currentColor">Section</text>
  <text x="204" y="50" font-size="10" text-anchor="middle" fill="currentColor">id, title, scope</text>
  <rect x="146" y="88" width="116" height="50" rx="6" fill="none" stroke="currentColor" stroke-width="1.5"/>
  <text x="204" y="108" font-size="11" text-anchor="middle" fill="currentColor">Section</text>
  <text x="204" y="126" font-size="10" text-anchor="middle" fill="currentColor">id, title, scope</text>
  <rect x="146" y="164" width="116" height="50" rx="6" fill="none" stroke="currentColor" stroke-width="1.5"/>
  <text x="204" y="184" font-size="11" text-anchor="middle" fill="currentColor">Section</text>
  <text x="204" y="202" font-size="10" text-anchor="middle" fill="currentColor">id, title, scope</text>
  <rect x="296" y="12" width="118" height="202" rx="6" fill="none" stroke="currentColor" stroke-width="1.5"/>
  <text x="355" y="32" font-size="11" text-anchor="middle" fill="currentColor">Rows, 1 to 24</text>
  <text x="304" y="52" font-size="10" fill="currentColor">Value rows</text>
  <text x="304" y="68" font-size="10" fill="currentColor">toggle, choice, number</text>
  <text x="304" y="84" font-size="10" fill="currentColor">text, text-area, path</text>
  <text x="304" y="100" font-size="10" fill="currentColor">color, keybinding, list</text>
  <text x="304" y="116" font-size="10" fill="currentColor">location, map, multi-choice</text>
  <text x="304" y="148" font-size="10" fill="currentColor">Rows with no value</text>
  <text x="304" y="164" font-size="10" fill="currentColor">button, info, group</text>
  <text x="304" y="180" font-size="10" fill="currentColor">preview, notice, meter</text>
  <line x1="114" y1="122" x2="145" y2="37" stroke="currentColor" stroke-width="1.5" marker-end="url(#schema-arrow)"/>
  <line x1="114" y1="122" x2="145" y2="113" stroke="currentColor" stroke-width="1.5" marker-end="url(#schema-arrow)"/>
  <line x1="114" y1="122" x2="145" y2="189" stroke="currentColor" stroke-width="1.5" marker-end="url(#schema-arrow)"/>
  <line x1="262" y1="37" x2="295" y2="37" stroke="currentColor" stroke-width="1.5" marker-end="url(#schema-arrow)"/>
  <line x1="262" y1="113" x2="295" y2="113" stroke="currentColor" stroke-width="1.5" marker-end="url(#schema-arrow)"/>
  <line x1="262" y1="189" x2="295" y2="189" stroke="currentColor" stroke-width="1.5" marker-end="url(#schema-arrow)"/>
</svg>
<figcaption>A tab entry holds sections, and each section holds rows.</figcaption>
</figure>

### Tab and section entries

Each entry in `contributes.settings` has a `kind`: `"tab"` adds a rail tab, and `"section"` adds rows to a built-in tab or to one of the plugin's own tabs.

#### Tab entries (kind "tab")

| Key | Type | Required | Default | Meaning |
|---|---|---|---|---|
| `kind` | string | yes | none | `"tab"` |
| `id` | string | yes | none | `[a-z][a-z0-9-]{0,31}`. Global id `<plugin-id>/<id>` |
| `title` | string | yes | none | 1 to 32 characters. Not a built-in tab label |
| `short_title` | string | no | `title`, cut with `…` | At most 12 cells, for a narrow rail |
| `icon` | string | no | none | Tabler icon name |
| `glyph` | string | no | `⧉` | One grapheme, one or two cells wide |
| `after` | string | no | `text-triggers` | Tab id. Never `debug`, `api`, `about` or `setup` |
| `order` | integer | no | 500 | 0 to 1000. Breaks ties among tabs with the same `after` |
| `render` | string | no | `"rows"` | `"rows"` or `"view"` (drawn by the `views` export) |
| `keywords` | list | no | none | At most 8 terms, each at most 24 characters |
| `sections` | list | for `"rows"` | none | 1 to 8 sections, with keys `id`, `title`, `scope`, `rows` and `collapsed` |

Inline sections are drawn in declaration order. `collapsed: true` starts a section folded.

#### Section entries (kind "section")

| Key | Type | Required | Default | Meaning |
|---|---|---|---|---|
| `kind` | string | yes | none | `"section"` |
| `id` | string | yes | none | `[a-z][a-z0-9-]{0,31}`, unique in the plugin |
| `tab` | string | yes | none | Built-in tab id (needs `settings.contribute:<tab-id>`), or `self:<tab-id>` |
| `placement` | string | no | `"end"` | `"end"`, `before:<anchor>` or `after:<anchor>` |
| `order` | integer | no | 500 | 0 to 1000. Orders sections at one anchor |
| `title` | string | no | none | At most 40 characters |
| `scope` | string | no | `"client"` | `"client"` or `"server"`. Applies to every key in the section |
| `rows` | list | yes | none | 1 to 24 rows. A group counts as one row plus its children |
| `collapsed` | boolean | no | `false` | Plugin tabs only |

### Built-in tab ids

Ids are frozen. Anchors are the section ids a plugin can place blocks before or after.

| Rail order | Id | Rail label | Accepts sections, anchors |
|---|---|---|---|
| 1 | `user-interface` | User Interface | yes, `main` |
| 2 | `icons` | Icons | yes, `main` |
| 3 | `agent-monitoring` | Agents | yes, `main` |
| 4 | `agent-cost` | Agent Cost | yes, `main` |
| 5 | `optimization` | Optimization | yes, `main` |
| 6 | `remote-compaction` | Remote | yes, five named anchors |
| 7 | `keyboard` | Keyboard | yes, `main` |
| 8 | `terminal` | Terminal | yes, `main` |
| 9 | `editor` | Editor | yes, `main` |
| 10 | `session` | Session | yes, `main` |
| 11 | `git` | Git | yes, `main` |
| 12 | `kanban-board` | Kanban Board | yes, `main` |
| 13 | `animations` | Animations | no |
| 14 | `sound` | Sound | yes, `main` |
| 15 | `voice-control` | Voice control | yes, `main` |
| 16 | `reset-planning` | Reset planning | yes, `main` |
| 17 | `inference` | Inference | yes, `main` |
| 18 | `llm-instructions` | LLM text | yes, `main` |
| 19 | `titles` | Titles | yes, `main` |
| 20 | `triggers` | Triggers | yes, `main` |
| 21 | `text-triggers` | Text Triggers | yes, `main` |
| 22 | `debug` | Debug | no |
| 23 | `api` | API | no |
| 24 | `about` | About | no |
| 25 | `setup` | Setup | no |

The rail shows the compact label when the full one does not fit.

### Row types

Every row has a `type`. A value row also has a `key`, and the host compiles it into one schema property that is checked like the rest of the manifest. There are 18 types: they are listed below in order.

| Type | Value | Compiles to | Writes |
|---|---|---|---|
| `toggle` | boolean | `boolean` | at once |
| `choice` | string or integer | `enum`, `enum_titles` | at once |
| `number` | integer or number | `minimum`, `maximum`, `step`, `unit`, `x-steps`, `x-special-values` | 600 ms after the last step |
| `text` | string | `maxLength`, `pattern`, `secret` | 600 ms after the last keystroke |
| `text-area` | string | `maxLength`, `multiline` | 600 ms after the last keystroke, or 1 s when the text is above 10,000 bytes (SET-023) |
| `path` | string | `picker`, `extensions` | at once on confirm |
| `color` | `#rrggbb` string | `pattern` | at once on confirm |
| `keybinding` | chord string | chord grammar | at once on confirm |
| `button` | none | none | invokes a command |
| `info` | none | none | none, or live text |
| `group` | none | none | none, folds children |
| `list` | array | `items`, `maxItems` | per committed item |
| `location` | object `{ lat, lon, label? }` | `object`, `properties`, `required`, `x-widget: location` | on `Use location` |
| `preview` | none | none | none, own tabs only |
| `notice` | none | none | none, a dismissal is stored per `id` |
| `map` | object of string keys | `additionalProperties`, `propertyNames`, `maxProperties` | per committed entry |
| `multi-choice` | array | `items: { enum }`, `uniqueItems`, `minItems`, `maxItems` | at once on each toggle |
| `meter` | none | none | none, updated with `set-meter` |

Compilation of the fields common to all value rows: `visible_when`, `scope` and `applies` compile to the annotations `x-visible-when`, `x-scope` and `x-applies`. They are UI annotations and never take part in validation. `steps` compiles to `x-steps` plus the `minimum` and `maximum` of the ladder, and `special_values` compiles to `x-special-values`.

The renders below show the row at rest in a plugin tab, 57 columns wide, with the real glyphs. The [row gallery](/pages/reference-settings-rows/) shows selected, pending, read-only, restart and error states for each type, with the declaration in Rust and TypeScript.

#### toggle

Keys: `on_label`, `off_label` (default `On` and `Off`), `style`: `"switch"` (default, `‹ On ›`) or `"checkbox"` (`[x]` and `[ ]`, for lists of related flags). The default is a boolean.

```json
{ "type": "toggle", "key": "enabled", "title": "Keep history", "default": true }
```

<figure>
<pre class="term">  Keep history ……………………………………………………………………………………… ‹ On ›  </pre>
<figcaption>Toggle row, switch style.</figcaption>
</figure>

#### choice

Keys: `options` (2 to 64 objects with `value` and `title`, required unless `source` is set), `style` (`"list"`, `"stepper"` or `"dialog"`), `dynamic` (boolean). The default must be one option value.

Further keys: the option fields `description` (at most 120), `disabled_reason` (at most 40, the option is drawn muted and cannot be chosen), `preview` (`{ "glyph" }`, `{ "swatch": [...] }` with at most 16 colours, or `{ "pattern": [...] }` with at most 4 lines of 8 cells) and `help`; and the row fields `source` (`{ "catalog": "<id>" }`, replaces `options`; `source` with `options` is `manifest.settings-source`, an unknown catalogue is `manifest.catalog-unknown`), `filter` (default true for a catalogue or more than 12 options), `allow_custom` with `custom_pattern`, and `view_modes` (`["list"]` or `["list", "grid"]`).

```json
{ "type": "choice", "key": "store", "title": "Store", "default": "text",
  "options": [ { "value": "text", "title": "Text only" }, { "value": "rich", "title": "Text and HTML" } ] }
```

<figure>
<pre class="term">  Store ……………………………………………………………………………………… ‹ Text only ›  </pre>
<figcaption>Choice row, list style.</figcaption>
</figure>

#### number

Keys: `minimum` and `maximum` (required), `step` (default 1), `unit` (at most 8 characters), `integer` (default `true`). Left and Right step; `*` types a value.

Further keys: `style` (`"stepper"` or `"slider"`), `steps` (an ascending ladder of 2 to 64 numbers inside the range, replaces `step`), `special_values` (at most 8 `{ value, title }` pairs drawn instead of the number, for example `0` as `Off`) and `debounce_ms` (300 to 2,000, replaces the 600 ms write delay).

```json
{ "type": "number", "key": "max_entries", "title": "Maximum entries", "minimum": 10, "maximum": 5000, "step": 10, "default": 500 }
```

<figure>
<pre class="term">  Maximum entries ………………………………………………………………… -  500  + *  </pre>
<figcaption>Number row, stepper style.</figcaption>
</figure>

#### text

Keys: `max_length` (default 256, at most 4,096), `pattern`, `placeholder`, `secret` (masked with `••••`). Further keys: `reveal_last` (0 to 8, only with `secret`; `••••a1b2` shows the last characters) and `debounce_ms` (300 to 2,000).

```json
{ "type": "text", "key": "label", "title": "Label", "max_length": 40, "default": "work" }
```

<figure>
<pre class="term">  Label …………………………………………………………………………………………………… ‹ work ›  </pre>
<figcaption>Text row.</figcaption>
</figure>

#### text-area

Keys: `max_length` (default 16,384, at most 65,536), `language` (syntax hint), and `debounce_ms` (300 to 2,000). Enter opens the editor modal.

```json
{ "type": "text-area", "key": "paste_template", "title": "Paste template", "max_length": 4096, "default": "{text}" }
```

<figure>
<pre class="term">  Paste template ……………………………………………………………………… ‹ 1 line ›  </pre>
<figcaption>Text-area row.</figcaption>
</figure>

#### path

Keys: `picker` (`"file"` or `"directory"`, required), `extensions`. The row stores text only and grants no file access.

```json
{ "type": "path", "key": "history_file", "title": "History file", "picker": "file", "extensions": ["db"], "default": "" }
```

<figure>
<pre class="term">  History file ………………………………………………………………… ‹ ~/clips.db ›  </pre>
<figcaption>Path row.</figcaption>
</figure>

#### color

Keys: `swatches` (`"theme"` or up to 16 colours). The value must match `^#[0-9a-fA-F]{6}$`.

```json
{ "type": "color", "key": "highlight", "title": "Highlight colour", "swatches": "theme", "default": "#4a9eff" }
```

<figure>
<pre class="term">  Highlight colour ……………………………………………………… ‹ ██ #4a9eff ›  </pre>
<figcaption>Colour row.</figcaption>
</figure>

#### keybinding

Keys: `command`, one of the plugin's own `keybindings` contributions (required; otherwise `manifest.settings-command`). Enter captures a chord and Delete clears it.

```json
{ "type": "keybinding", "key": "picker_key", "title": "Picker key", "command": "open-picker", "default": "ctrl+alt+v" }
```

<figure>
<pre class="term">  Picker key ……………………………………………………………………… ‹ Ctrl+Alt+V ›  </pre>
<figcaption>Keybinding row.</figcaption>
</figure>

#### button

Keys: `id` (required, not `key`), `label` (at most 20 characters), `command` (required), `confirm` (optional question). A button stores no value.

```json
{ "type": "button", "id": "clear_now", "title": "Clear history", "label": "Clear now", "command": "clear", "confirm": "Delete every stored entry?" }
```

<figure>
<pre class="term">  Clear history ………………………………………………………………… [ Clear now ]  </pre>
<figcaption>Button row.</figcaption>
</figure>

#### info

Keys: `id` (required when `live` is true), `text` (at most 120 characters), `live` (boolean). The plugin updates a live row with `set-info`. `rich` is a list of spans `{ text, tone?, bold?, command? }` (at most 120 characters in total) that replaces `text`. A span with `command`, an own command id, is clickable. `set-info-rich` updates a live row with rich text.

```json
{ "type": "info", "id": "stored", "title": "Stored entries", "text": "none yet", "live": true }
```

<figure>
<pre class="term">  Stored entries …………………………………………………………………… 312 entries  </pre>
<figcaption>Live info row.</figcaption>
</figure>

#### group

Keys: `title` (required), `rows` (1 to 12, no nested group), `collapsed`. Enter folds the group.

```json
{ "type": "group", "title": "Advanced", "collapsed": false, "rows": [ { "type": "toggle", "key": "sync", "title": "Sync", "default": false } ] }
```

<figure>
<pre class="term">  ▾ Advanced
    Sync between panes ……………………………………………………………… ‹ Off ›  </pre>
<figcaption>Group row.</figcaption>
</figure>

#### list

Keys: `item` (one primitive type, or an object of at most eight primitive fields), `max_items` (at most 64), `default` (array). Enter opens the list editor.

Further keys: `item_format` for string items (`glob`, `url`, `host`, `path`, `regex` or `chord`; checked on commit, and an invalid item is shown with `✗` and the reason and is not written), `style` (`"rows"`, the `list-editor` widget, or `"inline"`, one text field), `separator` (inline style, default `;`), `reorderable` (default true), `remote` (each committed item shows as pending until the plugin confirms it with `set-item-states`), `item_status` (reserves a status cell per item). Object items have at most eight fields.

```json
{ "type": "list", "key": "ignore_patterns", "title": "Ignore patterns", "item": { "type": "string", "max_length": 200 }, "max_items": 32, "default": [] }
```

<figure>
<pre class="term">  Ignore patterns ………………………………………………………………… ‹ 2 items ›  </pre>
<figcaption>List row.</figcaption>
</figure>

#### location

Keys: `search` (default true), `allow_current` (default false, needs `location.observer` on the client side), `precision` (decimals 0 to 6, default 4). The value is `{ lat, lon, label? }`. Both `lat` and `lon` are required, `lat` is from -90 to 90 and `lon` from -180 to 180. Enter opens the world-map picker, and the value is written when the user confirms `Use location`.

```json
{ "type": "location", "key": "observer", "title": "Observer location", "search": true, "precision": 4, "default": { "lat": 48.8566, "lon": 2.3522, "label": "Paris" } }
```

<figure>
<pre class="term">  Observer location ……………………… ‹ Paris 48.8566, 2.3522 ›  </pre>
<figcaption>Location row.</figcaption>
</figure>

#### preview

Keys: `view` (an own `panels` id, required), `height` (4 to 24 rows, default 8), `live` (default false; true lets the plugin call `ui.invalidate` on the view while Settings is open). The row holds no value and exists in the plugin's own tabs only.

```json
{ "type": "preview", "view": "colour-preview", "height": 4, "live": true }
```

<figure>
<pre class="term">  Preview
  ╭────────────────────────────────────────╮
  │  Newest entry   ██ #4a9eff             │
  ╰────────────────────────────────────────╯</pre>
<figcaption>Preview row of height 4. The plugin draws inside the frame.</figcaption>
</figure>

#### notice

Keys: `id` (required), `text` (at most 200 characters, rich spans allowed), `tone` (`info`, `success`, `warning`, `danger`; default `info`), `dismissible` (default false). A dismissal is stored per `id` and survives restarts.

```json
{ "type": "notice", "id": "privacy", "tone": "warning", "dismissible": true, "text": "Place names are sent to the geocoding provider." }
```

<figure>
<pre class="term">╭─ ! ───────────────────────────────────────── [x] ─╮
│ Place names are sent to the geocoding provider.   │
╰───────────────────────────────────────────────────╯</pre>
<figcaption>Notice row, warning tone, dismissible.</figcaption>
</figure>

#### map

Keys: `value_type` (`string`, `integer`, `number` or `boolean`), `key_pattern` (regex, default `^[^\\s]{1,64}$`), `max_entries` (1 to 64, default 16), `key_title` and `value_title` (at most 24 characters each).

```json
{ "type": "map", "key": "environment", "title": "Environment", "value_type": "string", "max_entries": 16, "key_title": "Name", "value_title": "Value", "default": {} }
```

<figure>
<pre class="term">  Environment ……………………………………………………………………… ‹ 2 entries ›  </pre>
<figcaption>Map row.</figcaption>
</figure>

#### multi-choice

Keys: `options` (2 to 64, the `choice` option shape) or `source`, `min_items` (default 0), `max_items` (default the option count), `style` (`"checklist"` inline, default, or `"dialog"`). The value is an array of option values without duplicates.

```json
{ "type": "multi-choice", "key": "kinds", "title": "Capture kinds", "min_items": 1, "default": ["text"], "options": [ { "value": "text", "title": "Text" }, { "value": "images", "title": "Images" } ] }
```

<figure>
<pre class="term">  Capture kinds
    [x] Text
    [ ] Images</pre>
<figcaption>Multi-choice row, checklist style.</figcaption>
</figure>

#### meter

Keys: `id` (required), `segments` (initial, at most 8 of `{ label, value, tone }`), `max` (default 1), `show_values` (default false). The plugin updates it with `set-meter`.

```json
{ "type": "meter", "id": "budget", "title": "Budget used", "max": 5, "show_values": true }
```

<figure>
<pre class="term">  Budget used ………………………………………………………… ██████░░░░ 3.1 / 5  </pre>
<figcaption>Meter row.</figcaption>
</figure>

### Common row keys

| Key | Rows | Rule |
|---|---|---|
| `key` | value rows | `[a-z][a-z0-9_]{0,63}`, unique in the plugin |
| `id` | button, info, group | `[a-z][a-z0-9-]{0,31}`, unique in the plugin |
| `title` | all but unlabelled info | 1 to 48 characters. The label column |
| `description` | all | At most 200 characters |
| `default` | value rows | Required. Must pass the compiled schema |
| `help` | all | An id from `contributes.help`. Adds a `?` |
| `depends_on` | all | `{ "key", "equals" }`, `{ "key", "in" }` or `{ "key", "not" }`. Own keys only |
| `keywords` | all | At most 4 terms |
| `reset` | value rows | Default `true`. `false` keeps the row out of a section reset |
| `visible_when` | value rows | The `depends_on` shape on one own key. A false condition hides the row |
| `scope` | value rows | `"project"` stores the value in the project's `.ilium/` settings under `[plugins."<plugin-id>"]`. The project value wins while that project is open |
| `applies` | value rows | `"restart"` draws `↻` after the value and the note `Applies after restart.` |

A false `depends_on` leaves the row on the screen, read-only, with the description `Unavailable: <title> is <value>.` A false `visible_when` hides the row instead: it keeps its value and search skips it. The `label` key belongs to `button` only. The row `preview`, `notice` and `meter` take only the common keys that apply to a row with no value.

### Validation and saving

- **Install.** Every default and option is checked.
- **Load.** A stored value that no longer matches its row is ignored, and the default applies. The host never clamps a stored value. A number typed in the editor that is outside `min` and `max` is refused with the row note, and the stored value stays; the number stepper stops at the bounds [SET-020].
- **Save.** Schema check, then `settings-validate`, then the write, then `settings-changed`.

| Change | Shown | Written |
|---|---|---|
| Toggle, choice, dialog confirm | at once | at once |
| Number step | at once | 600 ms after the last step |
| Number or text typed | as typed | 600 ms after the last keystroke, or `debounce_ms` |
| Text-area | as typed | 600 ms after the last keystroke, or 1 s when the text is above 10,000 bytes (SET-023), or `debounce_ms` |
| `set-json` from the plugin | at once | at once |

While a write is pending, the control shows a dim `◌` after the value. A failed change keeps the old value, and the message replaces the description line in the warning style. Closing Settings writes every pending change first.

### Keys on a plugin row

The kernel handles these keys before any plugin row sees them.

| Key | Effect |
|---|---|
| Up, Down | Move between rows |
| Left, Right | Step a number or choice, or flip a toggle |
| Enter, Space | Open the row's editor, or invoke a button |
| `*` | Type a value into a number row |
| `r` | Reset the focused value row |
| `?` | Open the row's help topic |
| `/` or Ctrl+F | Open the search filter. Esc clears it |
| Esc, `q` | Close Settings |

### Events and callbacks

**`settings-changed`** receives one record.

```wit
// Excerpt of ilium:types/events in reference-wit
enum settings-origin { user, plugin, reset, other-plugin, file }
record settings-change { keys: list<string>, origin: settings-origin, side: string }
```

- `keys`: changed keys, sorted, without duplicates.
- `origin`: `user`, `plugin`, `reset`, `other-plugin` or `file`.
- `side`: `"client"` or `"server"`, the receiving side. The field is named `side` so it is not confused with a row's `scope`, which is a storage scope (`"project"`). A section's `scope` key (`"client"` or `"server"`) names the side that hosts the section.

Coalesced writes send one event with the last origin and the union of keys.

**`settings-validate`** is an optional export on the side that owns the scope.

```wit
record field-error { key: string, message: string }
settings-validate: func(patch: string, merged: string) -> result<_, list<field-error>>;
```

It runs only for `user` and `reset` origins, after the schema check. Its deadline is 100 ms. A timeout, trap or stopped plugin refuses the save with `Could not check this value: <plugin name> did not answer.`

### Data schema subset

Every data schema in a manifest (row schemas, extension-point payload, entry and reply schemas, method schemas) may use only the keywords in the keyword table of [Plugin settings, row schemas](/pages/api-settings/): `type`, `enum`, `enum_titles`, `items`, `maxItems`, `title`, `help`, `description`, `default`, `minimum`, `maximum`, `step`, `unit` and the other rows of that table. Any other keyword fails the install with `manifest.schema-keyword`. The per-use size caps are on the page that uses the schema.

### Limits

An excess is refused with `manifest.settings-limit` unless the row says otherwise.

| Item | Limit |
|---|---|
| Tabs per plugin | 2 |
| Sections per plugin tab | 8 |
| Sections per plugin, per built-in tab | 2 |
| Plugin sections in one built-in tab | 6, extra ones are dropped with `settings.section-dropped` |
| Plugin tabs in the rail | 16, extra ones are dropped with `settings.tab-dropped` |
| Rows per section, children per group | 24 (a `group` counts as one row plus its children), and 12 |
| Value keys per plugin | 128 |
| Object item fields in a list | 8 |
| Segments in a meter | 8 |
| Notice text | 200 characters |
| Map entries | 64 |
| Choice options, list items | 64 |
| Tab, section and row titles | 32, 40 and 48 characters |
| Description, info text, button label | 200, 120 and 20 characters |
| Text value, text-area value | 4,096 and 65,536 characters, `invalid-argument` on save |
| All values of one plugin | 256 KiB, `invalid-argument` on save |
| `set-info`, `set-row-note` text | 120 characters, `invalid-argument` |
| `set-info`, `set-choices`, `set-row-note`, `set-row-state`, `set-item-states`, `set-choice-items`, `set-info-rich` and `set-meter` calls | 10 per second per plugin, all these calls together. A call over the rate is refused with `rate-limited` and the previous state stays [SET-050] |
| `settings-validate` answer | 100 ms, save refused on timeout |

### Error codes

Install errors refuse the package and name the JSON path of the entry.

| Code | Cause |
|---|---|
| `manifest.settings-kind` | Unknown `kind` |
| `manifest.settings-id` | Bad tab, section or row id |
| `manifest.settings-key` | Bad value key |
| `manifest.settings-key-clash` | Duplicate key |
| `manifest.settings-id-clash` | Duplicate tab or section id |
| `manifest.settings-title-clash` | Tab title is a built-in label |
| `manifest.settings-target-unknown` | Missing target tab |
| `manifest.settings-target-closed` | Target tab takes no sections |
| `manifest.settings-target-grant` | Grant missing |
| `manifest.settings-anchor` | Unknown anchor, or `before:main` |
| `manifest.settings-tab-anchor` | Bad `after` |
| `manifest.settings-row-type` | Unknown row type |
| `manifest.settings-row-field` | Field not allowed for type |
| `manifest.settings-default` | Default fails its row |
| `manifest.settings-depends` | Unknown key or cycle |
| `manifest.settings-help` | Help id not in `contributes.help` |
| `manifest.settings-command` | Command not contributed |
| `manifest.settings-glyph` | Glyph not one 1 or 2 cell grapheme |
| `manifest.settings-limit` | Limit exceeded |
| `manifest.catalog-unknown` | A `source` names an unknown catalogue |
| `manifest.settings-source` | A `choice` row has both or neither of `options` and `source` (it needs exactly one) |
| `manifest.schema-keyword` | Keyword outside the subset |

Runtime and load codes:

| Code | Cause |
|---|---|
| `invalid-argument` | A value is over its length limit, or `settings.open` names another plugin's target |
| `invalid-argument` | A `set-info`, `set-choices` or `set-row-note` call breaks a rule the plugin can check, such as text over 120 characters |
| `not-found` | A row-update call names an unknown key or id |
| `rate-limited` | More than 10 row-update calls per second per plugin, all kinds together |
| `settings.section-dropped` | A built-in tab already shows six plugin sections; the section is not drawn (warning) |
| `settings.tab-dropped` | 16 plugin tabs are already shown; the tab is not drawn (warning) |
| `settings.value-ignored` | A stored value no longer matches its row (warning) |
| `settings.anchor-missing` | An own-section anchor is gone, so the section moves to `end` (warning) |

### Reading the values in both languages

The rows of every type are read with one call and written with a merge patch. The plugin needs no grant for its own values.

**Rust**

```rust
use ilium_plugin_sdk::host::settings;

let values: serde_json::Value =
    serde_json::from_str(&settings::get_json()).unwrap_or_default();
let keep = values["enabled"].as_bool().unwrap_or(true);
settings::set_json(r#"{"max_entries": 800, "label": null}"#)?;
```

**TypeScript**

```ts
import { getJson, setJson } from '@ilium/plugin-sdk/host/settings';

const values = JSON.parse(getJson()) as Record<string, unknown>;
const keep = typeof values['enabled'] === 'boolean' ? values['enabled'] : true;
setJson(JSON.stringify({ max_entries: 800, label: null }));
```

A `null` resets the key to its default. Per-type declarations are on the [row gallery](/pages/reference-settings-rows/).

### Versioning

- The `page` entries and `settings_pages` are removed, and `settings-changed` sends the record above.
- Built-in tab ids and anchors are frozen. Rows are closed, so an unknown row type or field refuses the package.

Requirement ids for this page: [SET-001], [SET-002], [SET-003], [SET-004], [SET-005], [SET-010], [SET-011], [SET-020], [SET-021], [SET-022], [SET-023], [SET-024], [SET-031], [SET-050], [SET-051], [SET-060], [SET-061], [SET-062], [SET-063], [SET-064]. See the [requirements index](/pages/reference-requirements/).

### Security

Regex `pattern` values are limited per [SEC-046]; `secret` rows follow [SEC-242]. See [Secrets](/pages/reference-secrets/).
