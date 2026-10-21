---
layout: page
title: Input
permalink: /pages/widgets-input/
---

## Input

Input widgets let the user change a value or start an action. The family has eleven widgets, and this page documents eight of them in full: `button`, `toggle`, `choice`, `number`, `text-input`, `text-area`, `schema-form` and `key-capture`. The three pickers, `color-picker`, `path-field` and `location-picker`, have their own page, [Pickers](/pages/widgets-pickers/), because each of them opens a host dialog. The inputs that edit a list of values (`check-list` and `list-editor`) belong to [Collections](/pages/widgets-collections/).

Every input widget is a copy of a control that the Ilium client already draws in its Settings screen and its dialogs. The plugin never draws a field itself. It describes the widget in its `ui-tree`, and the host draws it with the same glyphs, the same dotted leaders and the same keys as the native controls. A user who knows the Settings screen already knows how these widgets behave.

| Widget | Edits | Event value | Documented |
|---|---|---|---|
| `button` | nothing, it starts an action | none | this page |
| `toggle` | a flag, or a tri-state flag | `flag` or `item-state` | this page |
| `choice` | one option of a list or catalogue | `index` or `text` | this page |
| `number` | a bounded number | `number` | this page |
| `text-input` | one line of text, optionally secret | `text` | this page |
| `text-area` | several lines of text | `text` | this page |
| `schema-form` | the rows of one settings section | none | this page |
| `key-capture` | one key chord | `text` | this page |
| `color-picker` | a colour `#rrggbb` | `text` | [Pickers](/pages/widgets-pickers/) |
| `path-field` | a file or folder | `text` | [Pickers](/pages/widgets-pickers/) |
| `location-picker` | a point on the world map | `geo` | [Pickers](/pages/widgets-pickers/) |

### You will need

- The `ui.panels` grant on the client side for a panel, popup or settings page. A pane kind needs `panes.provide-kind` instead. See [ui.panels](/pages/capability-ui-panels/).
- A view declared in the manifest: a `panels` or `onboarding_steps` entry, or a `settings` tab with `"render": "view"`, plus `"views"` in `exports`. A `schema-form` also needs its section under `contributes.settings`.
- A client entry module. Rust uses the `ilium-plugin-sdk` crate with `side = "client"`. TypeScript uses `@ilium/plugin-sdk` and its `ui/build` subpath.
- The [widget catalogue](/pages/widgets-index/) for the rules that every widget shares, and [UI model](/pages/ui-model/) for keys and host-owned state.

### How to read the renders

Every render on this page is built from the glyphs that the real client draws. The host frame is always rounded (`╭ ╮ ╰ ╯ ─ │`) and carries the chrome title `≡ ● ·` on its top edge, as defined under [Host frame](/pages/widgets-layout-and-containers/#host-frame). A plugin `block` defaults to rounded and may use any of the six border types. A form row is a label, one space, grey dotted leaders made of `…`, one space and a value that is right-aligned to the area edge. The host draws no dots when the gap is shorter than five cells; the gap is then blank. When the row is too narrow, the leaders go first. Then the label is cut to keep at least one cell plus `…`. Only when the value alone exceeds the area width minus 2 is the value cut from the left with a leading `…`. The leaders are not part of any hit target. In the captures the accent colour of a focused widget appears as the highlighted text, a disabled widget is dimmed, and an invalid value uses the error colour.

### Keys are action ids

Give every input widget a `key`. The host copies that key into each `ui-action` event, and the handler matches on it. A widget without a key still draws, but the plugin cannot tell its actions apart, and the host loses the widget's typed text and cursor on every render. Keys are unique across the whole view, not only among siblings. A duplicate key anywhere in the `ui-tree` traps the instance with `ui.duplicate-key`.

### Props every input widget accepts

These props live on the node, not on the widget, so they work the same for all eleven input widgets.

| Prop | Type | Default | Meaning |
|---|---|---|---|
| `key` | `option<string>` | none | The action id. Required for any widget the plugin wants to hear from. |
| `tooltip` | `option<tooltip>` | none | Help text of at most 200 Unicode scalar values, shown after 500 ms of hover or when the widget has focus and the user presses F1. |
| `disabled-reason` | `option<string>` | none | Draws the widget dimmed, makes it ignore input, and shows the reason as its tooltip. |
| `tab-index` | `option<s16>` | tree order | The focus order. Values from 1 come first, in ascending order; none and 0 follow in tree order; a negative value is skipped by Tab. See [Focus order](/pages/ui-model/#focus-order). |
| `hover-events` | flag | off | Sends `hover-enter` and `hover-leave` to the plugin. |

**Rust**

```rust
let save = tree.button("Save").key("save").tooltip("Writes the file").tab_index(1);
let locked = tree.toggle("Dry run", true).key("dry-run").disabled_reason("Set by policy");
```

**TypeScript**

```ts
const save = button('Save', { key: 'save', tooltip: 'Writes the file', tabIndex: 1 });
const locked = toggle('Dry run', true, { key: 'dry-run', disabledReason: 'Set by policy' });
```

### When the plugin hears about a change

Every action arrives as a `ui-action` event with five fields: `view` (the view target), `key` (the widget key), `action-id` (in 0.1.0 always a copy of `key`), `kind` (what happened) and `value` (the payload, an option; `none` below means the option is empty). Every focusable widget also sends `focus-gained` and `focus-lost`.

| Widget | Kind | Value | When it is sent |
|---|---|---|---|
| `button` | `activated` | `none` | At once, on press. |
| `toggle` | `changed` | `flag(bool)`, or `item-state("", tri-state)` with `tri-state` | At once. |
| `choice` | `changed` | `index(u32)`, or `text(item id)` for a catalogue | At once, on each change. |
| `choice` | `filter-changed` | `text(string)` | 150 ms after the user stops typing in the dialog filter. |
| `number` | `changed` | `number(f64)` | At once for steps and drags. After `debounce-ms` for typed digits. |
| `text-input` | `changed` | `text(string)` | After `debounce-ms` (default 600 ms) from the last key. |
| `text-input` | `submitted` | `text(string)` | On Enter, when `submit-on-enter` is true. |
| `text-area` | `changed` | `text(string)` | After `debounce-ms`; unset, 600 ms while the text is at most 10,000 bytes (UTF-8) and 1 s above that, re-evaluated at each keystroke. |
| `key-capture` | `changed` | `text(chord)` | When a chord is taken or cleared. |
| `key-capture` | `closed` | `none` | When the user cancels a capture with Escape. |
| `schema-form` | none | none | Never. The owning plugin receives `settings-changed` instead. |

`text-area` never sends `submitted`, because Enter inserts a line. A secret `text-input` sends its value in both events, because the plugin needs the value. The host redacts secret values from logs and from `widget-state` reads, and never stores them in the snapshot.

<figure>
<svg viewBox="0 0 400 140" role="img" aria-label="Key presses at 0, 200 and 400 ms restart a 600 ms debounce, so changed fires at 1000 ms">
  <defs>
    <marker id="input-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
      <path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
    </marker>
  </defs>
  <text x="20" y="18" font-size="11" fill="currentColor">Key presses restart a 600 ms debounce</text>
  <rect x="160" y="40" width="180" height="14" rx="3" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
  <text x="250" y="36" font-size="10" text-anchor="middle" fill="currentColor">600 ms debounce</text>
  <line x1="20" y1="110" x2="380" y2="110" stroke="currentColor" marker-end="url(#input-arrow)"/>
  <line x1="40" y1="106" x2="40" y2="114" stroke="currentColor"/>
  <line x1="160" y1="106" x2="160" y2="114" stroke="currentColor"/>
  <line x1="340" y1="106" x2="340" y2="114" stroke="currentColor"/>
  <circle cx="40" cy="110" r="4" fill="currentColor"/>
  <circle cx="100" cy="110" r="4" fill="currentColor"/>
  <circle cx="160" cy="110" r="4" fill="currentColor"/>
  <text x="100" y="96" font-size="11" text-anchor="middle" fill="currentColor">key presses</text>
  <line x1="340" y1="104" x2="340" y2="70" stroke="currentColor" marker-end="url(#input-arrow)"/>
  <text x="346" y="88" font-size="11" fill="currentColor">changed</text>
  <text x="40" y="132" font-size="10" text-anchor="middle" fill="currentColor">0 ms</text>
  <text x="160" y="132" font-size="10" text-anchor="middle" fill="currentColor">400 ms</text>
  <text x="340" y="132" font-size="10" text-anchor="middle" fill="currentColor">1000 ms</text>
</svg>
<figcaption>Keys at 0, 200 and 400 ms. Each key restarts the 600 ms debounce, so changed fires at 1000 ms. Steps, toggles and choices are not debounced.</figcaption>
</figure>

### Widget surface

Each table lists the props with their type, default and meaning. A required prop is a constructor argument of the builder. Other props are chained setters in Rust (`Tree` in `ilium_plugin_sdk::ui::build`) or fields of a props object in TypeScript (`@ilium/plugin-sdk/ui/build`). Rust setters are snake_case and TypeScript fields are camelCase.

#### button

**Purpose.** Starts an action: save, run, delete. It is a copy of the dialog buttons of the client. The `role` sets the look and the host never confirms anything for the plugin, so a destructive action needs a `dialog` of its own.

| Prop | Type | Default | Meaning |
|---|---|---|---|
| `label` | `string` | required | The text of the button. |
| `icon` | `option<string>` | none | A host icon name drawn before the label. |
| `enabled` | `bool` | `true` | When false, the button cannot be pressed. |
| `role` | `button-role` | `secondary` | `primary`, `secondary` or `danger`. |

| Event | Value | Sent when |
|---|---|---|
| `activated` | `none` | Enter or Space with focus, or a left click. |

| Input | Behaviour |
|---|---|
| Enter or Space | Presses the focused button. |
| Left click | Focuses the button and presses it. |
| Tab | Moves focus to the next focusable widget. |

**Drawing.** A button whose area height is 1 or 2 draws `[ label ]` on the first row, left-aligned. With an icon it draws `[ icon label ]`. A button with height 3 or more draws a rounded box filling its area, with the label centred on the middle row and the odd cell going right. A label wider than the space is cut with `…`. The focused button draws bold, with its label in the `selection` role.

Theme roles: `accent` for a primary button, `danger` for a danger button, `foreground` for a secondary button and `muted` for a disabled one.

<figure>
<pre class="term">╭  ≡ ● · Buttons───────────────────────────────────────────────────────────╮
│                                                                          │
│  <span class="t-hi">[ Save ]</span>   [ Cancel ]   [ Delete ]   <span class="t-dim">[ Apply ]</span>                          │
│                                                                          │
│  primary (focused)  secondary  danger        disabled                    │
│                                                                          │
╰──────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>The four button states in a one-row area. The first button holds focus and uses the accent colours, the last one is disabled and dimmed. The second line is a note and not part of the render.</figcaption>
</figure>

<figure>
<pre class="term">╭───────╮
│ Save  │
╰───────╯
</pre>
<figcaption>The same button in an area of height 3 and width 9. The label is centred, and the odd cell goes right.</figcaption>
</figure>

**Rust**

```rust
let save = tree.button("Save").key("save").role(ButtonRole::Primary);
// in on_events: ("save", ActionKind::Activated, _) => self.save(),
```

**TypeScript**

```ts
const save = button('Save', { key: 'save', role: 'primary' });
// in onEvents: if (key === 'save' && kind === 'activated') save();
```

#### toggle

**Purpose.** Switches a flag. It is the toggle row of the Settings screen. Three styles cover three uses: `switch` for a row with leaders, `checkbox` for a list of flags, and `icon` for a toolbar. With `tri-state` the toggle also has a mixed state for a flag that is true for some members of a group.

| Prop | Type | Default | Meaning |
|---|---|---|---|
| `label` | `string` | required | The text of the row. |
| `value` | `bool` | required | The state the row shows. |
| `style` | `toggle-style` | `switch` | `switch` draws `[ On ]` and `[ Off ]`, `checkbox` draws `[x]` and `[ ]`, `icon` draws one glyph. |
| `tri-state` | `bool` | `false` | Adds the mixed state `[-]`. The event value is then an `item-state`. |
| `icon-on`, `icon-off` | `option<string>` | none | The glyphs of the `icon` style, for example `★` and `☆`. |

| Event | Value | Sent when |
|---|---|---|
| `changed` | `flag(bool)` | The user flips the toggle. |
| `changed` | `item-state("", tri-state)` | The user cycles a `tri-state` toggle. The id is the empty string. |

| Input | Behaviour |
|---|---|
| Space or Enter | Flips the focused toggle. A tri-state toggle cycles off, on, mixed. |
| Left click | Focuses and flips. |

Theme roles: `success` for on, `muted` for off, `accent` for the focused row.

<figure>
<pre class="term">╭  ≡ ● · Toggles───────────────────────────────────────────────────────────╮
│ <span class="t-hi">Show background</span> <span class="t-dim">…………………………………………………………………………………………………………………………………</span> <span class="t-hi">[ On ]</span> │
│ Reverse colours <span class="t-dim">………………………………………………………………………………………………………………………………</span> [ Off ] │
│ <span class="t-dim">Natural placement</span> <span class="t-dim">……………………………………………………………………………………………………………………………</span> <span class="t-dim">[ On ]</span> │
│ [x] Enabled                                                              │
│ [ ] Dry run                                                              │
│ [-] Some agents selected                                                 │
│ Bookmark <span class="t-dim">…………………………………………………………………………………………………………………………………………………………………</span> ★ │
│ Bookmark <span class="t-dim">…………………………………………………………………………………………………………………………………………………………………</span> ☆ │
╰──────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>Rows of the toggle widget. Switch style on rows one to three (focused, idle, disabled), checkbox style and tri-state on rows four to six, icon style with the icon-on and icon-off glyphs at the end.</figcaption>
</figure>

**Rust**

```rust
let dry = tree.toggle("Dry run", self.dry_run).key("dry-run");
let agents = tree.toggle("All agents", false).key("all").style(ToggleStyle::Checkbox).tri_state(true);
let star = tree.toggle("Bookmark", self.starred).key("star")
    .style(ToggleStyle::Icon).icon_on("★").icon_off("☆");
// ("dry-run", ActionKind::Changed, ActionValue::Flag(on)) => self.dry_run = on,
// ("all", ActionKind::Changed, ActionValue::ItemState(_, state)) => self.all = state,
```

**TypeScript**

```ts
const dry = toggle('Dry run', state.dryRun, { key: 'dry-run' });
const agents = toggle('All agents', false, { key: 'all', style: 'checkbox', triState: true });
const star = toggle('Bookmark', state.starred, { key: 'star', style: 'icon', iconOn: '★', iconOff: '☆' });
// if (value.tag === 'flag') state.dryRun = value.val;
// if (value.tag === 'item-state') state.all = value.val[1];
```

#### choice

**Purpose.** Picks one option out of a fixed list or out of a catalogue. This is the widget behind most rows of the Settings screen: palette, dither, colour filter, theme, model. It has three styles. `stepper` is the compact row with the `←` `→` `+` buttons and fits a handful of options. `list` draws every option as its own row. `dialog` shows the current value and opens the filterable choice dialog, which suits long lists such as the 38 palettes or about 12,000 icons.

| Prop | Type | Default | Meaning |
|---|---|---|---|
| `label` | `string` | required | The text of the row. |
| `options` | `list<string>` | required | The options in order. Ignored when `items` or `source` is set. |
| `selected` | `u32` | required | The index of the option shown. 0 is the first option. |
| `style` | `choice-style` | `stepper` | `stepper`, `list` or `dialog`. |
| `items` | `list<catalog-item>` | empty | Option records. Replaces `options` when not empty. |
| `source` | `option<string>` | none | A catalogue id such as `ilium.palettes`. Items page in as the user scrolls. |
| `selected-id` | `option<string>` | none | The current item id for `items` and `source`. |
| `filter` | `bool` | `false` | Shows the Search line in the dialog. |
| `view-modes` | `list<view-mode>` | none | `list` and `grid`, adds the View switch of the icon picker. |
| `disabled-marker` | `disabled-marker` | `reason` | `reason` draws `(reason)`, `cross` draws `✗`. |
| `allow-custom` | `bool` | `false` | Lets the user type a value that is not in the list. |

A `catalog-item` has `id`, `title`, `description` (up to 120 characters), `preview`, `group` (a heading in the dialog), `keywords` (up to 8, matched by the filter) and `disabled-reason` (up to 40 characters). A `preview` is one of `glyph(string)`, `swatch(list<u32>)` (1 to 16 colours, drawn as half blocks) or `pattern(list<string>)` (1 to 4 lines of at most 8 cells).

| Event | Value | Sent when |
|---|---|---|
| `changed` | `index(u32)` | A fixed list changes. |
| `changed` | `text(item id)` | A catalogue or `items` choice changes. |
| `filter-changed` | `text(string)` | The dialog filter text changes, debounced 150 ms. |

| Input | Behaviour |
|---|---|
| Left or Right | Steps to the previous or next option. It never wraps and skips disabled options. |
| Enter, or a click on `+` or on the value | Opens the dialog (`dialog` style, or the full list from a stepper). |
| Mouse wheel | Steps, like the arrow keys. |
| In the dialog | Up, Down, PageUp, PageDown move the highlight. Typing filters. Enter applies. Escape cancels. A click on a row applies it, a click outside cancels. |
| Click on a disabled option | Nothing changes. The reason stays on screen. |

Theme roles: `accent` for the value, `muted` for disabled options.

**Stepper.** The stepper is the row `Label …… ← value → +`. A click on the value or on `→` goes forward, a right click on the value goes back, a click on `+` opens the list. A disabled button is dimmed and inert.

<figure>
<pre class="term">╭  ≡ ● · Choice rows───────────────────────────────────────────────────────╮
│ <span class="t-hi">Palette</span> <span class="t-dim">………………………………………………………………………………………………………………………………………</span> <span class="t-hi">← Sunset → +</span> │
│ Dither <span class="t-dim">…………………………………………………………………………………………………………………………………</span> ← Bayer 4x4 → + │
│ <span class="t-dim">Colour filter</span> <span class="t-dim">…………………………………………………………………………………………………………………………</span> <span class="t-dim">← Sepia → +</span> │
│ Colour source <span class="t-dim">………………………………………………………………………………………………………………………</span> <span class="t-err">← Banana → +</span> │
╰──────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>Stepper style. Left and right step through the options, the value and the arrow to its right go forward, and the plus opens the full list. Row one has focus, row three is disabled, row four shows an invalid stored value in the error colour.</figcaption>
</figure>

**List.** The list style draws one row per option and marks the current one with the same `●` as the dialog.

<figure>
<pre class="term">╭  ≡ ● · Choice list───────────────────────────────────────────────────────╮
│ Theme                                                                    │
│   ● Dark                                                                 │
│     Light                                                                │
│     Auto                                                                 │
╰──────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>List style: every option is a row and the current one carries the same ● marker as the choice dialog.</figcaption>
</figure>

**Dialog.** The dialog is the choice popover of the Settings screen, drawn by the host. Its parts, from the top, are the title, the `Search:` line, the option rows, the status line and the `Enter applies · Esc cancels` hint. The status line reads `N of M options · Current: X`, where N counts the options left by the filter. A disabled option keeps its place, carries the suffix `[unavailable]` and shows `Unavailable: <reason>` under it, and typing a word of the reason finds it. The host revalidates the chosen id on commit, so a stale id cannot be applied.

<figure>
<pre class="term">╭  ≡ ● · Palette───────────────────────────────────────╮
│ Search: sun_                                         │
│   Sunburst                                           │
│ <span class="t-hi">● Sunset</span>                                             │
│   Sunrise                                            │
│   Sunken garden <span class="t-dim">[unavailable]</span>                        │
│ <span class="t-dim">    Unavailable: needs Color mode</span>                    │
│                                                      │
│ 4 of 38 options · Current: Sunset                    │
│ Enter applies · Esc cancels                          │
│                                                      │
╰──────────────────────────────────────────────────────╯
</pre>
<figcaption>The choice dialog as the Settings screen draws it. The Search line filters while the user types. The highlighted row is drawn in the selection colours, ● marks the current value, a disabled option carries the [unavailable] suffix and an indented reason line. The status line counts the filtered options. With more rows than fit, the last column of the list's area becomes a scrollbar with the ┃ thumb on the │ track.</figcaption>
</figure>

With more rows than the dialog can show, the last column of the list's area becomes a scrollbar, drawn by the [scrollbar rule](/pages/widgets-layout-and-containers/#scrollbar): `│` is the track and `┃` is the thumb. A catalogue source is read in pages of up to 256 items as the user scrolls, so a list of 12,000 icons opens at once.

<figure>
<pre class="term">╭  ≡ ● · Icon─────────────────────────────────────────────╮
│ Search: _                                               │
│<span class="t-hi"> ● Rocket</span>                                              ┃ │
│   Satellite                                           │ │
│   Saturn                                              │ │
│   Scroll                                              │ │
│   Seedling                                            │ │
│ 6 of 12041 options · Current: Rocket                    │
│ Enter applies · Esc cancels                             │
╰─────────────────────────────────────────────────────────╯
</pre>
<figcaption>A long catalogue. The track is the last column of the list's area. The list holds 12,041 rows and shows 5, so the thumb is the minimum length of one row. Further pages load as the user scrolls.</figcaption>
</figure>

The same dialog serves the palette, dither and icon pickers, which differ only in their previews. They are shown in [Pickers](/pages/widgets-pickers/).

**Rust**

```rust
// types::ui::{ChoiceStyle, ViewMode, DisabledMarker, CatalogItem}; types::text::Preview
let theme = tree.choice("Theme", vec!["Light", "Dark", "Auto"], 1).key("theme");

// A catalogue with a filter: items page in from the host.
let palette = tree.choice("Palette", Vec::<String>::new(), 0)
    .source("ilium.palettes").style(ChoiceStyle::Dialog).filter(true).key("palette");

// Own options with previews and a disabled option.
let items = vec![
    CatalogItem::new("slow", "Slow").description("Every 10 s"),
    CatalogItem::new("live", "Live").preview(Preview::Glyph("●".into())).disabled_reason("needs a key"),
];
let rate = tree.choice("Refresh", Vec::<String>::new(), 0)
    .items(items).selected_id("slow").style(ChoiceStyle::Dialog).key("refresh");

// ("theme", ActionKind::Changed, ActionValue::Index(i)) => self.theme = i,
// ("palette", ActionKind::Changed, ActionValue::Text(id)) => self.palette = id,
```

**TypeScript**

```ts
const theme = choice('Theme', ['Light', 'Dark', 'Auto'], 1, { key: 'theme' });

const palette = choice('Palette', [], 0, {
  key: 'palette', source: 'ilium.palettes', style: 'dialog', filter: true,
});

const rate = choice('Refresh', [], 0, {
  key: 'refresh', style: 'dialog', selectedId: 'slow',
  items: [
    { id: 'slow', title: 'Slow', description: 'Every 10 s', keywords: [] },
    { id: 'live', title: 'Live', preview: { tag: 'glyph', val: '●' }, disabledReason: 'needs a key', keywords: [] },
  ],
});

// if (value.tag === 'index') state.theme = value.val;
// if (value.tag === 'text') state.palette = value.val;
```

#### number

**Purpose.** Edits a bounded number. It is the number row of the Settings screen, with two styles. The `stepper` style is `- 42 cm + *`: minus and plus step the value and the star opens the entry dialog. The `slider` style adds a track `─` with a thumb `●` in front of the same buttons.

| Prop | Type | Default | Meaning |
|---|---|---|---|
| `label` | `string` | required | The text of the row. |
| `value` | `f64` | required | The current value. |
| `minimum` | `f64` | `0` | The lower bound. A typed value below it is clamped to it. |
| `maximum` | `f64` | `100` | The upper bound. A typed value above it is clamped to it. |
| `step` | `f64` | `1` | The change per step. |
| `unit` | `option<string>` | none | A unit shown after the value. |
| `style` | `number-style` | `stepper` | `stepper` or `slider`. |
| `steps` | `list<f64>` | none | A ladder of values. A step moves to the next rung instead of adding `step`. |
| `special-values` | `list<tuple<f64, string>>` | none | A value drawn as a word, for example `0` as `Off`. |
| `scale` | `u32` | `1` | Typed digits are divided by it, so `1.7` entered with scale 10 means 17 in the stored unit. |
| `debounce-ms` | `u32` | `600` | The delay for typed digits. Steps and drags are not debounced. |

| Event | Value | Sent when |
|---|---|---|
| `changed` | `number(f64)` | A step, a drag, a wheel turn, or a valid typed value. |

| Input | Behaviour |
|---|---|
| Left or Right, `-` or `+` | Steps by `step`, or to the next rung of `steps`. A value never wraps. |
| Home or End | Goes to the minimum or the maximum. |
| Enter or `*` | Opens the entry dialog. |
| Left click on `-` or `+` | Steps. |
| Click or drag on the slider track | Sets the value at the pointer, see the slider rule below. |
| Mouse wheel | Steps. |

**Slider rule.** The track is the area width minus the label column, minus the stepper part (`- value unit + *`), minus 2 gap cells. When the track is narrower than 12 cells, the slider falls back to the stepper style. Let W be the track width. The thumb cell is `round((value-minimum)/(maximum-minimum)*(W-1))`. A click at track cell x sets `value = minimum + x/(W-1)*(maximum-minimum)`, snapped to the nearest step and clamped. Track cells are `─` (`muted`) and the thumb is `●` (`accent`). In stable-glyph mode they are `-` and `o`.

Theme roles: `accent` for the value and the thumb, `muted` for the track and for a disabled step button.

<figure>
<pre class="term">╭  ≡ ● · Number rows───────────────────────────────────────────────────────╮
│ <span class="t-hi">Scroll speed</span> <span class="t-dim">………………………………………………………………………………………………………………………………………</span> <span class="t-hi">- 3 + *</span> │
│ Width <span class="t-dim">………………………………………………………………………………………………………………………………………………</span> - 42 cm + * │
│ Retention <span class="t-dim">………………………………………………………………………………………………………………………………</span> - 30 days + * │
│ Timeout <span class="t-dim">………………………………………………………………………………………………………………………………………………………………</span> Off │
│ <span class="t-dim">Port</span> <span class="t-dim">……………………………………………………………………………………………………………………………………………………</span> <span class="t-dim">- 8080 + *</span> │
│ Port <span class="t-dim">…………………………………………………………………………………………………………………………………………………</span> <span class="t-err">- 99999 + *</span> │
╰──────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>Stepper style. The minus and plus buttons step, the star opens the entry dialog. A special value (row four, zero shown as Off) replaces the number. The error row shows a stored value outside the bounds.</figcaption>
</figure>

<figure>
<pre class="term">╭  ≡ ● · Slider rows───────────────────────────────────────────────────────╮
│ <span class="t-hi">Density</span>         <span class="t-dim">────────</span><span class="t-hi">●</span><span class="t-dim">────────────</span>  - 60 % + *                        │
│ Speed           <span class="t-dim">───</span><span class="t-hi">●</span><span class="t-dim">─────────────────</span>  - 25 % + *                        │
│ Frame rate cap  <span class="t-dim">────────────────────</span><span class="t-hi">●</span>  - 30 fps + *                      │
│ <span class="t-dim">Hue shift</span>       <span class="t-dim">──────────</span><span class="t-hi">●</span><span class="t-dim">──────────</span>  - 0 deg + *                       │
╰──────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>Slider style. The track is a row of ─ and the thumb is ●. The same row carries the stepper buttons, so keyboard users can step and mouse users can drag the thumb. Below 12 cells the host falls back to the stepper.</figcaption>
</figure>

The entry dialog opens on Enter or `*`. It is the number dialog of the client: a `Value:` line, an error line and the `Enter applies · Esc cancels` hint. The host parses the exact draft against the bounds and shows its own message, so the plugin only ever receives a valid number. A step button stops at the bound and is drawn dimmed there. Areas narrower than 12 cells fall back from the slider to the stepper, and the stepper keeps single-cell `-` and `+` targets so the unit stays visible at 80 columns.

<figure>
<pre class="term">╭  ≡ ● · Speed───────────────────────────────╮
│ Value: 4x_                                 │
│ <span class="t-err">Enter a number between 25 and 300</span>          │
│                                            │
│ Enter applies · Esc cancels                │
╰────────────────────────────────────────────╯
</pre>
<figcaption>The number entry dialog opened by Enter or the star. The host parses the exact draft and shows its message on the error line. Nothing reaches the plugin until the draft is valid.</figcaption>
</figure>

**Rust**

```rust
let density = tree.number("Density", self.density).key("density")
    .style(NumberStyle::Slider).minimum(25.0).maximum(100.0).step(5.0).unit("%");
let timeout = tree.number("Timeout", self.timeout).key("timeout")
    .minimum(0.0).maximum(300.0).special_values(vec![(0.0, "Off".into())]);
// ("density", ActionKind::Changed, ActionValue::Number(v)) => self.density = v,
```

**TypeScript**

```ts
const density = number('Density', state.density, {
  key: 'density', style: 'slider', minimum: 25, maximum: 100, step: 5, unit: '%',
});
const timeout = number('Timeout', state.timeout, {
  key: 'timeout', minimum: 0, maximum: 300, specialValues: [[0, 'Off']],
});
// if (value.tag === 'number') state.density = value.val;
```

#### text-input

**Purpose.** Edits one line of text. It is the text row of the Settings screen, drawn as `Label …… [ value ]`. The host owns the contents and the cursor after the first render, so `initial` sets only the starting text. The terminal cursor is the real cursor and no cursor glyph is drawn. A secret field shows one `•` per hidden character and can keep the last characters visible, like the API key rows (`••••a1b2`).

| Prop | Type | Default | Meaning |
|---|---|---|---|
| `label` | `string` | required | The text before the field. |
| `initial` | `string` | required | The starting text. An empty string gives an empty field. |
| `placeholder` | `option<string>` | none | Text shown while the field is empty. |
| `secret` | `bool` | `false` | Hides the text. |
| `reveal-last` | `u8` | `0` | With `secret`, how many trailing characters stay visible (0 to 8). |
| `reveal` | `bool` | `false` | Lets the user hold Ctrl+R to show a secret. |
| `max-length` | `u32` | `0` | The longest allowed value in characters. `0` means the 64 KiB cap. |
| `submit-on-enter` | `bool` | `true` | Whether Enter sends `submitted`. |
| `debounce-ms` | `u32` | `600` | Delay of `changed`, from 0 to 2,000. |
| `validation` | `option<string>` | none | A regular expression. A value that does not match is drawn in the error colour. |
| `suggestions` | `option<string>` | none | A catalogue id that offers completions while typing. |

| Event | Value | Sent when |
|---|---|---|
| `changed` | `text(string)` | After the debounce. |
| `submitted` | `text(string)` | On Enter, when `submit-on-enter` is true. |

| Input | Behaviour |
|---|---|
| Typing, Backspace, Delete, arrows, Home, End | Edit the text natively, with paste support. |
| Enter | Sends `submitted`. |
| Ctrl+R held | Shows a secret while `reveal` is true. |
| Left click | Places the cursor. Dragging selects. |

Theme roles: `foreground` for the text, `muted` for the placeholder and for the leaders, `danger` for an invalid value.

<figure>
<pre class="term">╭  ≡ ● · Text rows─────────────────────────────────────────────────────────╮
│ <span class="t-hi">Title</span> <span class="t-dim">………………………………………………………………………………………………………………………………………</span> <span class="t-hi">[ My session ]</span> │
│ Folders <span class="t-dim">………………………………………………………</span> [ ~/Pictures;/data/*/wallpapers/**/*.jpg ] │
│ <span class="t-dim">Name</span> <span class="t-dim">……………………………………………………………………………………………………………………………………</span> <span class="t-dim">[ Type a name… ]</span> │
│ Host <span class="t-dim">………………………………………………………………………………………………………………………………</span> <span class="t-err">[ photon..komoot ]</span> │
│ API key <span class="t-dim">………………………………………………………………………………………………………………………………………</span> [ ••••a1b2 ] │
│ API key (revealed) <span class="t-dim">………………………………………………………………………………………</span> [ sk-live-9f3a1b2 ] │
╰──────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>Single-line text. The focused field uses the real terminal cursor, no cursor glyph is drawn. An empty field shows its placeholder, an invalid value uses the error colour, a secret shows one dot per hidden character and keeps the last characters selected by reveal-last. Holding the reveal key shows the full value.</figcaption>
</figure>

A longer prompt, for example the rename dialog, is the same field inside a `dialog`. Put the field in a nested block with the title `Value`, or `Protected value` with the hint `The stored value remains hidden` for a secret.

**Rust**

```rust
let name = tree.text_input("Name", self.name.clone()).key("name")
    .placeholder("Type a name…").submit_on_enter(true);
let key = tree.text_input("API key", String::new()).key("api-key")
    .secret(true).reveal_last(4).reveal(true).validation("^sk-[a-z0-9-]+$");
// ("name", ActionKind::Submitted, ActionValue::Text(t)) => self.rename(t),
```

**TypeScript**

```ts
const name = textInput('Name', state.name, { key: 'name', placeholder: 'Type a name…', submitOnEnter: true });
const apiKey = textInput('API key', '', {
  key: 'api-key', secret: true, revealLast: 4, reveal: true, validation: '^sk-[a-z0-9-]+$',
});
// if (kind === 'submitted' && value.tag === 'text') rename(value.val);
```

#### text-area

**Purpose.** Edits several lines of text, for example a note, a prompt or a list of commands. Enter adds a line. The `max-bytes` cap counts bytes, not characters, so a multi-byte character uses more of the cap.

| Prop | Type | Default | Meaning |
|---|---|---|---|
| `initial` | `string` | required | The starting text. |
| `placeholder` | `option<string>` | none | Text shown while the area is empty. |
| `wrap` | `bool` | `true` | Whether long lines wrap. |
| `line-numbers` | `bool` | `false` | Whether line numbers are shown. |
| `max-bytes` | `u32` | `0` | The largest allowed value in bytes. `0` means 1 MiB. |
| `debounce-ms` | `u32` | unset | Delay of `changed`, from 0 to 2,000. Unset means 600 ms while the text is at most 10,000 bytes and 1,000 ms above that, re-evaluated at each keystroke. An explicit value always wins. |
| `tab-moves-focus` | `bool` | `false` | When true, Tab leaves the area. Otherwise Tab inserts a tab. |

| Event | Value | Sent when |
|---|---|---|
| `changed` | `text(string)` | After the debounce. |

| Input | Behaviour |
|---|---|
| Typing, Enter, arrows, Home, End, PageUp, PageDown | Edit natively. |
| Left click, drag | Place the cursor, select. |
| Mouse wheel | Scrolls the area. |

Theme roles: `foreground` for the text, `muted` for line numbers and for the placeholder.

<figure>
<pre class="term">╭  ≡ ● · Notes─────────────────────────────────────────╮
│ 1  Fix the tree crash                                │
│ 2  Add the chatroom pane                             │
│ 3                                                    │
│ <span class="t-dim">Enter adds a line · Tab inserts a tab</span>                │
╰──────────────────────────────────────────────────────╯
</pre>
<figcaption>A multi-line text area with line numbers. The cursor is the real terminal cursor on line three.</figcaption>
</figure>

**Rust**

```rust
let notes = tree.text_area(self.notes.clone()).key("notes").line_numbers(true).max_bytes(4096);
```

**TypeScript**

```ts
const notes = textArea(state.notes, { key: 'notes', lineNumbers: true, maxBytes: 4096 });
```

#### schema-form

**Purpose.** Draws the rows of one settings section from the manifest. The host validates each change and saves it, and the plugin only names the section. It is the generic settings renderer of the client, so the rows, the leaders, the keys and the help anchors are exactly those of the Settings screen. All 18 row types work, including the rows documented in [Settings rows](/pages/settings-screens/).

| Prop | Type | Default | Meaning |
|---|---|---|---|
| `plugin` | `plugin-id` | required | The plugin that owns the settings: its own id. |
| `section` | `string` | required | The id of one of the plugin's own sections, or of its own tab, from `contributes.settings`. |

A `schema-form` sends no `ui-action`. The host saves each change, and the owning plugin receives `settings-changed` with the changed keys. Read the values through the [settings API](/pages/api-settings/). Keyboard and mouse follow the rows: each row behaves like the control it shows (a `choice` row steps with Left and Right, a `number` row steps with `-` and `+`). A `schema-form` for a section of another plugin is drawn only when the rendering plugin holds `settings.write-all`. See [Plugin settings extension](/pages/settings-extension/).

<figure>
<pre class="term">╭  ≡ ● · Cleanup settings──────────────────────────────────────────────────╮
│ Enabled <span class="t-dim">………………………………………………………………………………………………………………………………………………………</span> [ On ] │
│ <span class="t-hi">Port</span> <span class="t-dim">……………………………………………………………………………………………………………………………………………………</span> <span class="t-hi">- 8080 + *</span> │
│ Mode <span class="t-dim">……………………………………………………………………………………………………………………………………………</span> ← Dry run → + │
│ Token <span class="t-dim">……………………………………………………………………………………………………………………………………………</span> [ ••••a1b2 ] │
╰──────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>A schema form built from four settings rows. It uses the same rows, leaders and keys as the Settings screen.</figcaption>
</figure>

**Rust**

```rust
let form = tree.schema_form("com.example.demo", "badges").key("settings");
```

**TypeScript**

```ts
const form = schemaForm('com.example.demo', 'badges', { key: 'settings' });
```

#### key-capture

**Purpose.** Records one key chord, for example a shortcut for a plugin command. The user presses Enter on the field, then presses the chord, and the field takes it. The chord is a string such as `Ctrl+Shift+K`. The field looks up the chord in the kernel keymap when `reserved-check` is on, and shows `! used by <command>` for a chord that the kernel already binds. The kernel's own prefix chooser, which offers only the 26 `Ctrl+letter` choices, is not this widget. The global chord registry stays with the kernel, so a captured chord is a value that the plugin stores and applies itself, for example through `contributes.commands`.

| Prop | Type | Default | Meaning |
|---|---|---|---|
| `chord` | `option<string>` | none | The current chord. None shows `not set`. |
| `allow-modifier-only` | `bool` | `false` | Accepts a chord that is only a modifier, such as `Shift`. |
| `reserved-check` | `bool` | `true` | Warns about chords the kernel binds (needs `keymap.read`). |

| Event | Value | Sent when |
|---|---|---|
| `changed` | `text(chord)` | A chord is taken, or cleared with an empty string. |
| `closed` | `none` | The user cancels a capture with Escape. |

| Input | Behaviour |
|---|---|
| Enter | Starts a capture. The field shows `⌨ press keys…`. |
| Any chord during a capture | Takes the chord as the value and ends the capture. |
| Escape during a capture | Cancels and sends `closed`. |
| Backspace on the field | Clears the chord. |
| Left click | Starts a capture. |

Theme roles: `accent` while capturing, `warning` for a conflict, `muted` when disabled.

<figure>
<pre class="term">╭  ≡ ● · Shortcuts─────────────────────────────────────────────────────────╮
│ <span class="t-hi">Split pane</span> <span class="t-dim">……………………………………………………………………………………………………………………</span> <span class="t-hi">[ Ctrl+Shift+K ]</span> │
│ Rename pane <span class="t-dim">………………………………………………………………………………………………………………</span> [ ⌨ press keys… ] │
│ Toggle tree <span class="t-dim">……………………………………………………………………</span> [ Ctrl+B ]  <span class="t-warn">! used by tree.toggle</span> │
│ <span class="t-dim">Zoom</span> <span class="t-dim">…………………………………………………………………………………………………………………………………………………</span> <span class="t-dim">[ not set ]</span> │
╰──────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>The key-capture widget in four states: a bound chord with focus, capturing, a chord that the kernel keymap already uses, and a disabled field. While capturing, the next chord is taken as the value.</figcaption>
</figure>

**Rust**

```rust
let chord = tree.key_capture().chord(self.chord.clone()).reserved_check(true).key("chord");
// ("chord", ActionKind::Changed, ActionValue::Text(c)) => self.chord = Some(c).filter(|c| !c.is_empty()),
```

**TypeScript**

```ts
const chordField = keyCapture({ key: 'chord', chord: state.chord, reservedCheck: true });
// if (key === 'chord' && value.tag === 'text') state.chord = value.val || undefined;
```

### Changing a widget from the plugin

The host applies a user's change at once. To set a value from the plugin, call `set-widget-state(view, key, state)` with the state case that matches the widget. In Rust that is `ilium_plugin_sdk::host::ui::set_widget_state`, in TypeScript `setWidgetState` from `@ilium/plugin-sdk/host/ui`.

| Widget | State case | Meaning |
|---|---|---|
| `toggle` | `checked(bool)` | The flag. |
| `toggle` with `tri-state` | `tri(list<tuple<string, tri-state>>)` | One entry under the id `""`. |
| `choice` | `selected(option<u32>)` | The index. On a catalogue choice use `text(item id)`; `selected(n)` returns `invalid-argument`. |
| `text-input`, `text-area` | `text(string)` | The text. |
| `number` | `number(f64)` | The value. |
| `location-picker` | `geo(option<geo-point>)` | The point. See [Pickers](/pages/widgets-pickers/). |

**Rust**

```rust
ilium_plugin_sdk::host::ui::set_widget_state(ViewTarget::Panel("cleanup".into()), "dry-run", WidgetState::Checked(true))?;
```

**TypeScript**

```ts
setWidgetState({ tag: 'panel', val: 'cleanup' }, 'dry-run', { tag: 'checked', val: true });
```

### Worked example: a cleanup panel

The manifest declares the panel. The `views` export draws it, and the handler stores each change.

```json
{
  "id": "com.example.cleanup",
  "capabilities": { "client": ["ui.panels"] },
  "exports": ["views"],
  "contributes": {
    "panels": [{ "id": "cleanup", "title": "Clean up", "modal": true }]
  }
}
```

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::types::layout::{Area, Constraint, Direction};
use ilium_plugin_sdk::types::ui::{ActionKind, ActionValue, ButtonRole, NumberStyle, UiTree, ViewTarget};
use ilium_plugin_sdk::ui::build::Tree;

#[ilium_plugin(side = "client", exports = ["views"])]
#[derive(Default)]
struct Cleanup {
    target_name: String,
    dry_run: bool,
    keep_days: f64,
    shortcut: Option<String>,
}

impl Lifecycle for Cleanup {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        self.keep_days = 30.0;
        Ok(())
    }
}

impl Handler for Cleanup {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::UiAction(action) = event else { continue };
            match (action.key.as_str(), action.kind, action.value) {
                ("target-name", ActionKind::Changed, ActionValue::Text(name)) => self.target_name = name,
                ("dry-run", ActionKind::Changed, ActionValue::Flag(on)) => self.dry_run = on,
                ("keep-days", ActionKind::Changed, ActionValue::Number(days)) => self.keep_days = days,
                ("shortcut", ActionKind::Changed, ActionValue::Text(chord)) => {
                    self.shortcut = Some(chord).filter(|chord| !chord.is_empty());
                }
                ("run", ActionKind::Activated, ActionValue::None) => {
                    let mode = if self.dry_run { "dry run" } else { "real run" };
                    log::log(Level::Info, &format!("cleanup of {} as {mode}", self.target_name));
                }
                _ => {}
            }
        }
        Vec::new()
    }
}

impl Views for Cleanup {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        let mut tree = Tree::new();
        let name = tree.text_input("Name", self.target_name.clone()).key("target-name");
        let dry_run = tree.toggle("Dry run", self.dry_run).key("dry-run");
        let keep = tree.number("Keep days", self.keep_days).key("keep-days")
            .style(NumberStyle::Slider).minimum(1.0).maximum(365.0).unit("days");
        let shortcut = tree.key_capture().chord(self.shortcut.clone()).key("shortcut");
        let run = tree.button("Run cleanup").key("run").role(ButtonRole::Primary);
        let rows = tree
            .layout(Direction::Vertical)
            .constraints(vec![Constraint::Length(3), Constraint::Length(1), Constraint::Length(1),
                              Constraint::Length(1), Constraint::Length(3)])
            .child(name).child(dry_run).child(keep).child(shortcut).child(run);
        tree.finish(rows)
    }
}
```

**TypeScript**

```ts
import { definePlugin, type Event } from '@ilium/plugin-sdk';
import type { Submission } from '@ilium/plugin-sdk/types';
import { log } from '@ilium/plugin-sdk/host/log';
import { button, keyCapture, layout, number, textInput, toggle, tree } from '@ilium/plugin-sdk/ui/build';

const state = { targetName: '', dryRun: false, keepDays: 30, shortcut: undefined as string | undefined };

export const { lifecycle, handler, views } = definePlugin({
  side: 'client',
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag !== 'ui-action') continue;
      const { key, kind, value } = event.val;
      if (key === 'target-name' && kind === 'changed' && value.tag === 'text') state.targetName = value.val;
      else if (key === 'dry-run' && kind === 'changed' && value.tag === 'flag') state.dryRun = value.val;
      else if (key === 'keep-days' && kind === 'changed' && value.tag === 'number') state.keepDays = value.val;
      else if (key === 'shortcut' && kind === 'changed' && value.tag === 'text') state.shortcut = value.val || undefined;
      else if (key === 'run' && kind === 'activated') {
        log('info', `cleanup of ${state.targetName} as ${state.dryRun ? 'dry run' : 'real run'}`);
      }
    }
    return [];
  },
  render() {
    return tree(
      layout(
        { direction: 'vertical', constraints: [
          { tag: 'length', val: 3 }, { tag: 'length', val: 1 }, { tag: 'length', val: 1 },
          { tag: 'length', val: 1 }, { tag: 'length', val: 3 } ] },
        [
          textInput('Name', state.targetName, { key: 'target-name' }),
          toggle('Dry run', state.dryRun, { key: 'dry-run' }),
          number('Keep days', state.keepDays, { key: 'keep-days', style: 'slider', minimum: 1, maximum: 365, unit: 'days' }),
          keyCapture({ key: 'shortcut', chord: state.shortcut }),
          button('Run cleanup', { key: 'run', role: 'primary' }),
        ],
      ),
    );
  },
});
```

### What the user sees

- The widgets appear in tree order, top to bottom. Focusable widgets take focus in the [focus order](/pages/ui-model/#focus-order).
- The focused widget uses the accent colour. A text field shows the terminal cursor.
- Typing changes the text on screen at once. The plugin hears nothing until the user pauses for the debounce time.
- A secret field shows dots in place of its text, and the last characters when `reveal-last` is set.
- A disabled widget is dimmed, ignores input and shows its reason as a tooltip.

### Edge cases and errors

| Case | Result |
|---|---|
| Typed text longer than `max-length` | The host refuses the extra characters. No event is sent. |
| Typed number outside `minimum` and `maximum` | The entry dialog shows its error line and nothing is sent. A step stops at the bound. |
| A `validation` pattern that is not a valid regular expression | `ui.invalid-tree`. The instance traps, the previous tree stays (stale) and one error is logged. |
| Two widgets with the same key anywhere in one view | `ui.duplicate-key`. The instance traps. |
| More nodes than the effective `ui_tree_nodes` limit (4,096 by default, 16,384 for T1), or more bytes than `ui_tree_kib` (1 MiB by default, 4,096 KiB for T1) | `trap.invalid-ui-tree` (detail `ui.too-large`). The instance traps. |
| `choice` with `selected-id` that is not in `items` | The first option is shown and nothing is sent. |
| `set-widget-state` with a key the view does not have | `not-found` |
| `set-widget-state` with a state the widget does not take | `invalid-argument` |
| An input widget other than `button`, `schema-form` and `location-picker` in a settings page (full list in [The UI model](/pages/ui-model/#settings-views)) | `ui.widget-not-allowed`. The instance traps and the previous tree stays. Use `schema-form`, and `location-picker` with `bind`. |
| The `ui.panels` grant is declined | Every `ui` import returns `not-permitted`, and the panel is not registered. |
| The grant changes during a call | `revoked`. The plugin is activated again. |

Requirement ids for this page: [WID-026], [WID-027]. See the [requirements index](/pages/reference-requirements/).

### See also

- [Pickers](/pages/widgets-pickers/)
- [Widget catalogue](/pages/widgets-index/)
- [Text](/pages/widgets-text/)
- [Collections](/pages/widgets-collections/)
- [Status and feedback](/pages/widgets-status-and-feedback/)
- [Layout and containers](/pages/widgets-layout-and-containers/)
- [ui.panels](/pages/capability-ui-panels/)
- [Settings screens](/pages/settings-screens/)
- [Events and intents](/pages/events-and-intents/)
