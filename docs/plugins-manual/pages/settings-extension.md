---
layout: page
title: Extending Settings
permalink: /pages/settings-extension/
---

## Extending Settings

A plugin can put its own settings into Ilium's Settings screen. It can do that in two ways:

- **A. A new tab.** The plugin adds one or two tabs of its own to the rail, next to the built-in tabs. The plugin decides what the tab holds.
- **B. Sections in an existing tab.** The plugin adds a group of rows to a built-in tab, such as Terminal, Editor or Sound. The rows sit under a divider that names the plugin, so the user always sees where they came from.

Most plugins need only A. Use B when a setting belongs beside related built-in settings. Both ways use the same 18 row types, save rules and storage. Step-by-step guides are [tutorial 10, a new tab](/pages/tutorial-10-settings-new-tab/) and [tutorial 11, a section in an existing tab](/pages/tutorial-11-settings-extend-tab/). Exact field tables and error codes are in the [settings schema reference](/pages/reference-settings-schema/), the calls a plugin uses to read and write values are in [settings API](/pages/api-settings/), and the screen itself is described in [building settings screens](/pages/settings-screens/).

### The model

Every settings tab has a stable id. Built-in tabs use kebab-case ids such as `terminal`, `editor` or `user-interface`. These ids never change, even when the label on screen changes. A plugin's own tab has a global id made of the plugin id and its local id, for example `example.clipboard-history/main`.

A plugin declares its settings in `contributes.settings`. Each entry has a `kind`:

- `kind: "tab"` creates a new tab in the rail.
- `kind: "section"` adds a group of rows. Its `tab` field names either a built-in tab id (for example `terminal`) or one of the plugin's own tabs, written `self:<tab-id>`.

A section is placed at an **anchor**. An anchor is a named point in a tab:

- `end` (the default) puts the section after the last built-in row.
- `before:<section>` puts it directly before a named section.
- `after:<section>` puts it directly after a named section.

Most built-in tabs have one unnamed section called `main`, so `before:main` is refused with `manifest.settings-anchor`. Tabs with headings, such as Remote compaction, expose named anchors: `behaviour`, `technique-per-agent`, `custom-prompt`, `input-shaping` and `model`. Kernel anchors are never renamed or removed.

A new tab is placed in the rail with `after`, which names the tab it follows. The default is `text-triggers`. The tail of the rail (`debug`, `api`, `about`, `setup`) is pinned, so a plugin tab never appears after `debug`.

### Ordering

The order of plugin content inside one tab is fixed by three keys, applied in turn:

1. The anchor. Built-in rows come first. A `before:` block sits directly before its named section, an `after:` block sits directly after it, and `end` blocks follow the last built-in row.
2. The `order` number, from 0 to 1000 (default 500). Lower numbers come first.
3. The plugin id, compared as bytes, then the order of declaration inside the plugin.

All sections of one plugin at one anchor sit together under one divider. The installation order and the enable order never change the result, so two machines with the same plugins show the same page.

<figure>
<svg viewBox="0 0 400 220" role="img" aria-label="Order of plugin blocks in a tab with named anchors">
  <defs>
    <marker id="se-order-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
      <path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="8" y="8" width="200" height="20" rx="4" fill="none" stroke="currentColor"/>
  <text x="16" y="22" font-size="11" fill="currentColor">behaviour</text>
  <text x="256" y="22" font-size="10" fill="currentColor">kernel section</text>
  <rect x="8" y="34" width="200" height="20" rx="4" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
  <text x="16" y="48" font-size="11" fill="currentColor">Plugin A block</text>
  <text x="256" y="48" font-size="10" fill="currentColor">plugin A, after:behaviour</text>
  <rect x="8" y="60" width="200" height="20" rx="4" fill="none" stroke="currentColor"/>
  <text x="16" y="74" font-size="11" fill="currentColor">technique-per-agent</text>
  <text x="256" y="74" font-size="10" fill="currentColor">kernel section</text>
  <rect x="8" y="86" width="200" height="20" rx="4" fill="none" stroke="currentColor"/>
  <text x="16" y="100" font-size="11" fill="currentColor">custom-prompt</text>
  <text x="256" y="100" font-size="10" fill="currentColor">kernel section</text>
  <rect x="8" y="112" width="200" height="20" rx="4" fill="none" stroke="currentColor"/>
  <text x="16" y="126" font-size="11" fill="currentColor">input-shaping</text>
  <text x="256" y="126" font-size="10" fill="currentColor">kernel section</text>
  <rect x="8" y="138" width="200" height="20" rx="4" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
  <text x="16" y="152" font-size="11" fill="currentColor">Plugin B block</text>
  <text x="256" y="152" font-size="10" fill="currentColor">plugin B, after:input-shaping</text>
  <rect x="8" y="164" width="200" height="20" rx="4" fill="none" stroke="currentColor"/>
  <text x="16" y="178" font-size="11" fill="currentColor">model</text>
  <text x="256" y="178" font-size="10" fill="currentColor">kernel section</text>
  <rect x="8" y="190" width="200" height="20" rx="4" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
  <text x="16" y="204" font-size="11" fill="currentColor">Plugin A block, end</text>
  <text x="256" y="204" font-size="10" fill="currentColor">plugin A, end</text>
</svg>
<figcaption>Remote compaction tab: plugin blocks are placed by anchor. Kernel sections keep their order.</figcaption>
</figure>

### Grants

A plugin needs no grant for its own tab or for sections in its own tab. The always-granted `settings` access covers the plugin's own values.

To add sections to a built-in tab, the plugin needs one grant per tab: `settings.contribute:<tab-id>`, for example `settings.contribute:terminal`. The grant takes one tab id with no wildcard. It gives permission to draw sections in that tab and nothing else. The review dialog shows it as "Adds settings to the Terminal tab". The grant is normal risk and gives no read or write access to built-in values.

`settings.read-all` and `settings.write-all` are a different grant and stay high risk. Neither grant implies `settings.contribute`, and the reverse is also true. See [capability: settings.read-all and settings.write-all](/pages/capability-settings-all/).

### Row types

A row is one setting line group: a label, a control and a description. Rows are declared inline in the manifest. The table lists the 18 types. Every type, in every state, is on the [row gallery](/pages/reference-settings-rows/).

| Type | Stores | What the user gets |
|---|---|---|
| `toggle` | boolean | `‹ On ›` or `‹ Off ›`, or `[x]` and `[ ]` with `style: "checkbox"` |
| `choice` | string or integer | A short list of `options`, stepped with Left and Right, or a filterable dialog |
| `number` | integer or decimal | A stepper `- 42 + *`, or a slider with a `●` thumb |
| `text` | string | A single line, optionally masked with `secret` as `••••` |
| `text-area` | string | A multi-line editor opened with Enter |
| `path` | string | A file or folder picker; stores the path as text only |
| `color` | `#rrggbb` | A swatch picker with hex entry |
| `keybinding` | key chord | A capture of a chord for one of the plugin's own commands |
| `button` | nothing | A button that runs one of the plugin's commands |
| `info` | nothing | Read-only text; can be updated at runtime if `live` is set |
| `group` | nothing | A folding heading that holds up to 12 child rows |
| `list` | array | A list editor for strings or small objects of up to 8 fields |
| `location` | `{ lat, lon, label? }` | The world-map picker with an address search |
| `preview` | nothing | A live area drawn by the plugin; own tabs only |
| `notice` | nothing | A banner with a tone; can be dismissed |
| `map` | object | An editor for string keys and their values |
| `multi-choice` | array | A checklist, or a dialog with check marks |
| `meter` | nothing | A segmented bar, updated by the plugin |

Every value row has a `key`, a `title` and a `default`. The kernel turns each value row into one property of a JSON schema, so the same validation and merge rules apply to plugin rows and built-in rows. A row whose default does not match its own schema refuses the package at install time.

Three fields of every value row change how a row behaves:

- `depends_on` makes the row read-only when its condition is false. The row stays visible and focusable and says `Unavailable: <title> is <value>.`
- `visible_when` hides the row when its condition is false. A hidden row keeps its value and search skips it.
- `scope: "project"` stores the value in the project's `.ilium/` settings under `[plugins."<plugin-id>"]`. The project value wins while that project is open.
- `applies: "restart"` draws the marker `↻` after the value and the note `Applies after restart.`

Use `depends_on` when the user should see where a setting lives. Hiding a row moves the rows under the cursor.

### Safety rules

Plugins can add to Settings. They cannot change what is already there.

- A plugin cannot hide, disable, reorder, relabel or restyle a built-in row, and it cannot change the rows of a built-in section.
- Plugin keys are stored only in `[plugins."<plugin-id>"]`. A section in the Terminal tab never writes the `[terminal]` table.
- The kernel handles Up, Down, Tab, Esc, `q`, `?`, `/` and `r` before any plugin row sees input. Plugin rows receive values, never raw key events.
- Plugin text (titles, descriptions, info text) is plain text. The kernel strips control characters and bidirectional overrides and draws the text in its own styles. A plugin cannot draw a fake built-in divider or a fake close button.

The user always sees who added a section. A plugin tab shows its name and version in the first line of the page, and its frame title ends with `· plugin`. A plugin block inside a built-in tab starts with a divider line of the form `▏ <glyph> <PLUGIN NAME> ──── plugin ─`. Every line of plugin content carries a dim `▏` gutter in column 0, so the label stays visible after the user scrolls past the divider.

### Limits

| Item | Limit |
|---|---|
| Tabs per plugin | 2 |
| Sections per plugin tab | 8 |
| Sections per plugin, per built-in tab | 2 |
| Plugin sections in one built-in tab, all plugins | 6 |
| Rows per section (a group counts its children) | 24 |
| Value keys per plugin | 128 |
| Segments in a meter, entries in a map | 8 and 64 |
| Choice options | 64 |
| List items, and fields per item object | 64, and 8 |
| Text value | 4,096 characters (text area: 65,536) |
| All values of one plugin, serialized | 256 KiB |
| Manifest titles, section titles, descriptions, info text, button labels | 48, 40, 200, 120 and 20 characters |

When the limit for plugin sections in one built-in tab is reached, the extra sections are not drawn. They are listed on the recovery page with the warning `settings.section-dropped`. Install-time errors name the item, its path in the manifest and the limit.

### Values and saving

Values for a plugin's settings are stored in its own table in `config.toml`:

```toml
[plugins."example.clipboard-history"]
enabled = true
max_entries = 500
ignore_patterns = ["^sk-[A-Za-z0-9]{20,}$", "^ghp_"]
record_panes = true
```

A row's key follows its section's `scope`. A row with `scope: "project"` is written to the project's `.ilium/` settings instead of `config.toml`; the project value wins while that project is open. The `side` field of a `settings-changed` event names the receiving side. Client-scope keys are read on the client. Server-scope keys are checked by the client against the same schema, written to the same table and passed to the server through the normal configuration refresh. A section has a single scope, so a plugin that needs both sides declares two sections.

Changes are saved as the user makes them. There is no Save button.

| Interaction | Saved |
|---|---|
| Toggle, choice step, choice picked from a list | At once |
| Number steps | 600 ms after the last step; steps within the window become one write |
| Typed number or text | 600 ms after the last keystroke, or at once on Enter or when leaving the row |
| Text area | 1 s after the last keystroke, and at once when the editor closes |
| Path, colour, keybinding, list item | At once, when the dialog is confirmed |
| Plugin call `set-json` | At once, without a delay |

While a write is pending, the row shows a dim `◌` after the value. A row whose value differs from its default shows a dim `•` after the label, and the `r` key resets the focused row to its default.

A save runs three checks in order: the schema, then the plugin's optional `settings-validate` export, then the write. The export has a deadline of 100 ms. A failed check keeps the previous value and puts the message in place of the row's description. Closing Settings writes every pending change at once.

### Live content

Eight calls let a plugin update its own rows while it runs. They need no grant and are limited to 10 per second per plugin. A plugin tab can also draw its whole page with a view. A built-in tab never runs plugin code while it paints.

| Call | Effect |
|---|---|
| `set-info`, `set-info-rich` | the text of a live `info` row |
| `set-choices`, `set-choice-items` | the options of a dynamic `choice` or `multi-choice` row |
| `set-row-note` | a dim note under the description |
| `set-row-state` | `idle`, `loading`, `pending` or `error` on a row, with a message of at most 80 characters |
| `set-item-states` | the state of single list items |
| `set-meter` | the segments of a `meter` row |

**Rust**

```rust
use ilium_plugin_sdk::host::settings;
use ilium_plugin_sdk::types::{RowState, RowStateKind};

fn mark_loading() -> Result<(), PluginError> {
    settings::set_row_state("stars", &RowState {
        kind: RowStateKind::Loading,
        message: Some("asking the server".into()),
    })
}
```

**TypeScript**

```ts
import { setRowState } from '@ilium/plugin-sdk/host/settings';

export function markLoading(): void {
  setRowState('stars', { kind: 'loading', message: 'asking the server' });
}
```

A plugin that needs a built-in value can read it. The grant `settings.read-kernel:<tab-id>` and the import `kernel-settings` return the values of that tab as read-only JSON, with secrets replaced by `<redacted>`. No plugin writes a built-in value; a link with `ilium://settings/<tab-id>#<key>` opens Settings at that row.

### Search

Settings has a filter. Press `/` or `Ctrl+F` to open it, and type part of a name. The field replaces the `Esc or q to close` hint on the second header line. The filter matches tab titles, row titles, descriptions, section titles, keywords and keys, and for plugin rows the plugin name and id. A row hidden by `visible_when` is skipped. The rail then shows only the tabs with matches, each with a count, and the content shows only the matching rows, each with its section title and, for plugin rows, the plugin divider. Esc clears the filter, and a second Esc closes Settings.

<figure>
<pre class="term">⚙ Settings                                                               ✕ Close
<span class="t-dim">/ max                                                           [ Guided setup ]</span>
╭  ≡ ● · Navigation╮ ╭  ≡ ● · Clipboard history · plugin───────────────────────╮
│ ❯  Terminal 1    │ │                                                         │
<span class="t-hi">│ ⎘  Clipboard 1   │</span> │<span class="t-hi">  CAPTURE                                                </span>│
│                  │ │  Maximum entries ………………………………………………………… -   500    + *  │
│                  │ │<span class="t-dim">    Oldest entries are removed first.                    </span>│
│                  │ │                                                         │
│                  │ │                                                         │
│                  │ │                                                         │
│                  │ │                                                         │
╰──────────────────╯ ╰─────────────────────────────────────────────────────────╯</pre>
<figcaption>The filter with the query <code>max</code>. The rail keeps the two tabs with a match and a count. The content shows the matching row under its section title. A match in a built-in tab shows the plugin divider when the row comes from a plugin.</figcaption>
</figure>

Plugins need no search code. To make a row easier to find, give it `keywords`, at most 4 per row.

### Reset to default

The `r` key on a focused value row sends the patch `{ "<key>": null }` and needs no confirmation. The section title of a plugin tab, and the divider of a plugin block, carry a `[ Reset ]` control. It sets every key of the section that has `reset: true` to `null`, after the question `Reset <n> settings of <plugin name>?`. A section reset has the origin `reset` in `settings-changed`. Built-in rows have no reset key.

### Declaring and reading, in both languages

A tab and a section differ only in the entry. The values are read the same way in both cases.

```json
{ "contributes": { "settings": [
  { "kind": "section", "id": "terminal-capture", "tab": "terminal",
    "placement": "end", "title": "Terminal capture", "scope": "client",
    "rows": [ { "type": "toggle", "key": "record_panes", "title": "Record pane copies",
                "default": true } ] }
] } }
```

**Rust**

```rust
use ilium_plugin_sdk::host::settings;

fn record_panes() -> bool {
    let values: serde_json::Value =
        serde_json::from_str(&settings::get_json()).unwrap_or_default();
    values["record_panes"].as_bool().unwrap_or(true)
}
```

**TypeScript**

```ts
import { getJson } from '@ilium/plugin-sdk/host/settings';

export function recordPanes(): boolean {
  const values = JSON.parse(getJson()) as { record_panes?: boolean };
  return values.record_panes ?? true;
}
```

The section needs the grant `settings.contribute:terminal` in the manifest. The full build is [tutorial 11](/pages/tutorial-11-settings-extend-tab/).

### Lifecycle

Settings outlive the plugin. The table shows each state.

| State | Plugin tab | Section in a built-in tab | Values |
|---|---|---|---|
| Running | Normal | Normal | Editable |
| Restarting after a failure | Notice with the attempt number and wait time | Divider shows `plugin · restarting` | Editable, unless the plugin has `settings-validate`; then read-only |
| Turned off by the user | Rail entry dimmed, with a link to the Plugins page | Divider shows `plugin · off`; rows dimmed | Kept, read-only |
| Disabled after repeated failures | The stop notice and a link to the Plugins page | Divider shows `plugin · disabled`; rows dimmed | Kept, read-only |
| Refused at load (API or schema mismatch) | Dimmed, with the required API version | As disabled | Kept, untouched |
| Uninstalled | Gone from the rail | Gone | Kept until the user purges them |

A plugin that fails three times within 10 minutes is disabled. Disabling, crashes, upgrades and uninstalls never delete values. An upgrade can narrow a range, so a stored value may stop matching its row. The kernel then ignores it, uses the default and lists it on the recovery page as `settings.value-ignored`. The stored text stays in the file. Only `ilium plugin uninstall --purge` or "Forget settings" on the recovery page deletes values. A reinstall reads them again.

<div class="warn">

A plugin that declares `settings-validate` has its rows read-only while it restarts, because the kernel cannot check a new value without the plugin. Plugins without that export keep their rows editable during a restart.

</div>

### What the user sees

The renders are 80 columns wide, with the geometry of the Settings screen at 80 by 32: a navigation frame of 20 columns, a gap of 1 and a content frame of 59. They are built from the layout code of the client. `◂` marks the selected rail entry, which the real rail shows with the selected style (lavender, near-black text), not with a glyph. The scrollbar is a `│` track with a `┃` thumb. The microphone glyph of Voice control is one cell in the width tables of the client, so a browser that draws it two cells wide shifts that one line. The same holds for the lightning glyph of the Inference tab in the rails below.

The plugin tab `Clipboard history` uses its `short_title` `Clipboard`, because 17 cells do not fit the 14 cells the rail leaves for a label at this width. It sits after `Text Triggers` and before the pinned tail.

<figure>
<pre class="term">⚙ Settings                                                               ✕ Close
Esc or q to close                                               [ Guided setup ]
╭  ≡ ● · Navigation╮ ╭  ≡ ● · Clipboard history · plugin───────────────────────╮
│                  │ │                                                     ┃   │
│ ◐  User Interface│ │  ⎘ Added by example.clipboard-history 1.2.0         ┃   │
│ ▦  Icons         │ │                                                     ┃   │
│ ◉  Agents        │ │  CAPTURE                                            ┃   │
│ ¤  Agent Cost    │ │  Keep history …………………………………………………………… ‹ On ›        ┃   │
│ ↗  Optimization  │ │    Record text copied in panes and the editor.      ┃   │
│ ⇥  Remote        │ │  ──────────────────────                             ┃   │
│ ⌨  Keyboard      │ │  Maximum entries …………………………………………………… -   500    + *┃ ? │
│ ❯  Terminal      │ │    Oldest entries are removed first.                ┃   │
│ ✎  Editor        │ │  ──────────────────────                             ┃   │
│ ▣  Session       │ │  Store ……………………………………………………………………………… ‹ Text only › ┃   │
│ ⑂  Git           │ │    Which clipboard content is kept.                 ┃   │
│ ▥  Kanban Board  │ │  ──────────────────────                             ┃   │
│ ▷  Animations    │ │                                                     ┃   │
│ ♪  Sound         │ │  PRIVACY                                            │   │
│ 🎙  Voice control │ │  Ignore patterns …………………………………………………… ‹ 2 items ›   │ ?│
│ ↻  Reset planning│ │    Copies that match are never stored.              │   │
│ ⚡ Inference     │ │  ──────────────────────                             │   │
│ ≡  LLM text      │ │                                                     │   │
│ ≔  Titles        │ │                                                     │   │
│ ⚑  Triggers      │ │                                                     │   │
│ ¶  Text Triggers │ │                                                     │   │
│ ⎘  <span class="t-hi">Clipboard    ◂</span>│ │                                                     │   │
│ ⌕  Debug         │ │                                                     │   │
│ ⇄  API           │ │                                                     │   │
│ ⓘ  About         │ │                                                     │   │
│ ⚙  Setup         │ │                                                     │   │
╰──────────────────╯ ╰─────────────────────────────────────────────────────────╯</pre>
<figcaption>The rail with a plugin tab, and the first part of the tab. The first line names the plugin and its version. Section titles are upper case.</figcaption>
</figure>

Inside a built-in tab, a plugin block follows the built-in rows, at the anchor. The block starts with a divider that names the plugin, and a dim `▏` gutter marks every plugin line, so the label stays visible after the user scrolls past the divider.

<figure>
<pre class="term">⚙ Settings                                                               ✕ Close
Esc or q to close                                               [ Guided setup ]
╭  ≡ ● · Navigation╮ ╭  ≡ ● · Terminal─────────────────────────────────────────╮
│                  │ │                                                         │
│ ◐  User Interface│ │  Scrollback budget ……………………………………………… -  8 MiB   + *  ? │
│ ▦  Icons         │ │    Maximum retained terminal output per pane; takes     │
│ ◉  Agents        │ │  ──────────────────────                                 │
│ ¤  Agent Cost    │ │  New pane directory …………………………………………………… ‹ Project r  ? │
│ ↗  Optimization  │ │    Choose the initial directory for a terminal pane.    │
│ ⇥  Remote        │ │  ──────────────────────                                 │
│ ⌨  Keyboard      │ │  Smart Copy light ………………………………………………………… ‹ On ›         │
│ ❯  Terminal     ◂│ │    Hold the key below over a terminal and click regi    │
│ ✎  Editor        │ │  ──────────────────────                                 │
│ ▣  Session       │ │  Smart Copy light key …………………………………… ← Control   + →    │
│ ⑂  Git           │ │    The modifier key that starts Smart Copy light whi    │
│ ▥  Kanban Board  │ │  ──────────────────────                                 │
│ ▷  Animations    │ │  Parser pool ……………………………………………………………… -  0 MiB   + *  ? │
│ ♪  Sound         │ │    No aggregate parser memory cap: initialized panes    │
│ 🎙  Voice control│ │  ──────────────────────                                 │
│ ↻  Reset planning│ │<span class="t-hi">▏ ⎘ CLIPBOARD HISTORY ────────────────────── plugin ─</span>    │
│ ⚡ Inference     │ │▏ Terminal capture                                       │
│ ≡  LLM text      │ │▏ Record pane copies …………………………………………… ‹ On ›          ? │
│ ≔  Titles        │ │▏   Also store text copied from terminal panes.          │
│ ⚑  Triggers      │ │▏ ──────────────────────                                 │
│ ¶  Text Triggers │ │▏ Ignore copies under ……………………………………… -  3 chars  + *    │
│ ⎘  Clipboard     │ │▏   Short copies such as a single word are skipped.      │
│ ⌕  Debug         │ │▏ ──────────────────────                                 │
│ ⇄  API           │ │▏ Skip secret-looking text …………………………… ‹ On ›            │
│ ⓘ  About         │ │▏   Tokens and keys are never stored.                    │
│ ⚙  Setup         │ │▏ ──────────────────────                                 │
╰──────────────────╯ ╰─────────────────────────────────────────────────────────╯</pre>
<figcaption>The Terminal tab with a plugin section at the anchor <code>end</code>. The built-in rows are unchanged.</figcaption>
</figure>

When the plugin is disabled after failures, its block stays visible, dimmed and read-only, with the reason above it:

<figure>
<pre class="term">╭  ≡ ● · Terminal─────────────────────────────────────────╮
│  Parser pool ……………………………………………………………… -  0 MiB   + *│ ? │
│    No aggregate parser memory cap: initialized panes│   │
│  ──────────────────────                             │   │
│▏ ⎘ CLIPBOARD HISTORY ─────────── plugin · disabled ─│   │
│▏ <span class="t-warn">! Plugin Clipboard history stopped after 3</span>         │   │
│▏   failures in 10 minutes: handler trap.            │   │
│▏   Values are kept.  [ Open Plugins page ]          │   │
│▏ Terminal capture                                   │   │
│▏ Record pane copies …………………………………………… ‹ On ›        ┃ ? │
│▏   Read-only while the plugin is disabled.          ┃   │
│▏ ──────────────────────                             ┃   │
│▏ Ignore copies under ……………………………………… -  3 chars  + *┃   │
│▏   Read-only while the plugin is disabled.          ┃   │
│▏ ──────────────────────                             ┃   │
╰─────────────────────────────────────────────────────────╯</pre>
<figcaption>A plugin section after the plugin was disabled. Values are kept.</figcaption>
</figure>

While the plugin restarts after a failure, a plugin tab shows a notice. A plugin that exports <code>settings-validate</code> has its rows read-only until the new instance runs:

<figure>
<pre class="term">╭  ≡ ● · Clipboard history · plugin───────────────────────╮
│                                                         │
│  ⎘ Added by example.clipboard-history 1.2.0             │
│  ◌ Restarting after a failure (attempt 2, in 5 s).      │
│    Rows are read-only until it is back, because         │
│    this plugin checks values itself.                    │
│                                                         │
│  CAPTURE                                                │
│  Keep history …………………………………………………………… ‹ On ›            │
│    Record text copied in panes and the editor.          │
│  ──────────────────────                                 │
│  Maximum entries …………………………………………………… -   500    + *  ? │
│    Oldest entries are removed first.                    │
│  ──────────────────────                                 │
╰─────────────────────────────────────────────────────────╯</pre>
<figcaption>The plugin tab while the plugin restarts.</figcaption>
</figure>

Rows of a plugin tab can be of any of the 18 types. The tab, scrolled down, shows several of them:

<figure>
<pre class="term">╭  ≡ ● · Clipboard history · plugin───────────────────────╮
│  Ignore patterns …………………………………………………… ‹ 2 items ›   │ ? │
│    Copies that match are never stored.              │   │
│  ──────────────────────                             │   │
│  Clear history ………………………………………………………… [ Clear now ] │   │
│    Deletes every stored entry at once.              │   │
│  ──────────────────────                             │   │
│  Stored entries ……………………………………………………… 312 entries   │   │
│    Updated by the plugin while it runs.             │   │
│  ──────────────────────                             │   │
│                                                     │   │
│  STORAGE                                            │   │
│  History file …………………………………………………………… ‹ ~/clips.db ›│   │
│    File picker; other folders need fs access.       │   │
│  ──────────────────────                             │   │
│  Highlight colour ………………………………………………… ‹ ██ #4a9eff ›│   │
│    Colour of the newest entry in the picker.        │   │
│  ──────────────────────                             │   │
│  Picker key ………………………………………………………………… ‹ Ctrl+Alt+V ›│   │
│    Opens the history picker.                        │   │
│  ──────────────────────                             │   │
│  Label ……………………………………………………………………………… ‹ work ›      │   │
│    Shown in the picker title; 40 characters.        │   │
│  ──────────────────────                             ┃   │
│  Paste template ……………………………………………………… ‹ 3 lines ›   ┃   │
│    Enter opens the editor; saved after 1 s.         ┃   │
│  ──────────────────────                             ┃   │
│  ▾ Advanced                                         ┃   │
│    Sync between panes ……………………………………… ‹ Off ›       ┃   │
│      Needs Keep history.                            ┃   │
│    ──────────────────────                           ┃   │
│    Sync interval …………………………………………………… -   5 s    + *┃   │
│      Unavailable: Sync between panes is Off.        ┃   │
│    ──────────────────────                           ┃   │
╰─────────────────────────────────────────────────────────╯</pre>
<figcaption>More row types: a list, a button, a live info row, a path, a colour, a key, a text, a text area and a group. The group holds a toggle and a dependent number that is read-only because its <code>depends_on</code> is false.</figcaption>
</figure>

### How the pieces fit

The kernel keeps one settings registry. It is built when the client starts and rebuilt when a plugin is installed, enabled, disabled or upgraded. It holds the tabs in rail order, the anchors of each built-in tab and the plugin blocks for each anchor. Keys, mouse, help and search all read the same row list, so plugin rows behave like built-in rows.

<figure>
<svg viewBox="0 0 410 160" role="img" aria-label="Plugin manifest flows into the settings registry, then the screen and one row model">
  <defs>
    <marker id="se-flow-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
      <path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="6" y="36" width="118" height="52" rx="6" fill="none" stroke="currentColor"/>
  <text x="65" y="58" font-size="11" text-anchor="middle" fill="currentColor">Plugin manifest</text>
  <text x="65" y="74" font-size="10" text-anchor="middle" fill="currentColor">contributes.settings</text>
  <rect x="146" y="36" width="118" height="52" rx="6" fill="none" stroke="currentColor"/>
  <text x="205" y="58" font-size="11" text-anchor="middle" fill="currentColor">Settings registry</text>
  <text x="205" y="74" font-size="10" text-anchor="middle" fill="currentColor">kernel, checked</text>
  <rect x="286" y="36" width="118" height="52" rx="6" fill="none" stroke="currentColor"/>
  <text x="345" y="58" font-size="11" text-anchor="middle" fill="currentColor">Settings screen</text>
  <text x="345" y="74" font-size="10" text-anchor="middle" fill="currentColor">rail and content</text>
  <line x1="124" y1="62" x2="143" y2="62" stroke="currentColor" marker-end="url(#se-flow-arrow)"/>
  <line x1="264" y1="62" x2="283" y2="62" stroke="currentColor" marker-end="url(#se-flow-arrow)"/>
  <rect x="6" y="116" width="398" height="30" rx="6" fill="none" stroke="currentColor"/>
  <text x="205" y="135" font-size="11" text-anchor="middle" fill="currentColor">One row model: paint, keys, mouse, help, search</text>
  <line x1="345" y1="88" x2="345" y2="114" stroke="currentColor" marker-end="url(#se-flow-arrow)"/>
</svg>
<figcaption>The manifest is read once into the registry. The screen draws built-in and plugin rows through one row model.</figcaption>
</figure>

Built-in tabs keep their own row code. A plugin block can never change a built-in row index.

### Security

Project-scope values are ignored until the user trusts the project ([SEC-310], [SEC-311]). See [Extension security](/pages/reference-extension-security/).

Requirements for this page: [Settings requirements](/pages/reference-requirements-settings/).
