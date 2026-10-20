---
layout: page
title: Building settings screens
permalink: /pages/settings-screens/
---

## Building settings screens

A plugin offers its settings in one of two ways. **Rows** are a list of values that the kernel draws for you, declared inline in the manifest with 18 row types such as toggles, numbers, choices, locations and meters. The [row gallery](/pages/reference-settings-rows/) shows each one. A **custom page** is drawn by the plugin: a plugin tab with `"render": "view"` whose `views` export returns a widget tree, which can hold tabs, tables, previews and schema forms. Both kinds save through the same settings store and the same validation. The kernel also keeps a recovery page that no plugin can replace or hide.

A plugin can do two things with the Settings screen. It can add **a tab of its own** to the rail, and it can add **sections of rows** to a built-in tab such as Terminal or Editor. Its own tabs and its sections in built-in tabs are both declared under `contributes.settings`. The model is explained in [Plugin settings extension](/pages/settings-extension/).

Use rows when toggles, numbers and choices are enough. Use a custom page when the screen needs more than one kind of content.

### What you need

- A manifest entry in `contributes.settings`, with `kind` `"tab"` (a new tab) or `"section"` (rows in a tab). Every key is listed in the [Manifest reference](/pages/reference-manifest/) and the [settings schema reference](/pages/reference-settings-schema/).
- Grants. A plugin's own tabs and their rows need only the always-granted `settings` import for the plugin's own values. A section in a built-in tab needs one `settings.contribute:<tab-id>` grant per tab, for example `settings.contribute:terminal`. A custom page also needs `ui.panels` on the client side. Reading or writing another plugin's values needs `settings.read-all` or `settings.write-all`; those are high-risk grants available only to first-party T1 plugins, so a third-party settings screen cannot edit another plugin's configuration.
- For a custom page, a client side that exports `views`. Write it in Rust with `ilium_plugin_sdk` ([Rust SDK](/pages/rust-sdk/)) or in TypeScript with `@ilium/plugin-sdk` ([TypeScript SDK](/pages/typescript-sdk/)).
- The widgets `tabs`, `block`, `paragraph` and `schema-form`, described in [Layout and containers](/pages/widgets-layout-and-containers/) and [Input widgets](/pages/widgets-input/).

## Two kinds of settings page

| Kind | Declared with | Drawn by | Use it when |
|---|---|---|---|
| Rows | `contributes.settings` entry with `kind` `tab` (`render` `rows`) or `kind` `section` | The kernel's generic renderer | The page is a list of values such as toggles, numbers and choices |
| Custom page | `contributes.settings` entry with `kind` `tab` and `"render": "view"`, plus the `views` export | The plugin, through `render` | The page needs tabs, tables, previews, or a mix of widgets |

Rows cost no code. A custom page is a normal view: the host calls `render` when the page opens, after `ui.invalidate`, and on resize, never once per frame.

## Rows in a tab or a section

A `tab` entry adds a page to the rail. It holds one to eight inline sections. A `section` entry adds a group of rows to a built-in tab, or to one of the plugin's own tabs, with `tab` set to a built-in tab id or to `self:<tab-id>`:

| Field | Type | Meaning |
|---|---|---|
| `kind` | `tab` or `section` | What the entry adds. |
| `id` | string | Local id, `[a-z][a-z0-9-]{0,31}`. The global id is `<plugin-id>/<id>`. |
| `title` | string | Tab title (1 to 32 characters) or section title (at most 40). |
| `tab` | string | Section only. A built-in tab id (needs `settings.contribute:<tab-id>`) or `self:<tab-id>`. |
| `placement` | string | Section only. `end` (default), `before:<section>` or `after:<section>`. `before:main` is refused at install with `manifest.settings-anchor`. |
| `scope` | `server` or `client` | The side whose values the rows edit. Defaults to `client`. |
| `rows` | list | One to 24 rows, one of the 18 row types. |

Every value row becomes one key of the plugin's own values, stored under `[plugins."<plugin-id>"]`. A row's `type` decides its control, and the kernel compiles each value row into the JSON Schema subset below, so the same validation applies to rows and to `set-json`.

Any keyword outside this table fails the install with `manifest.schema-keyword`.

| Keyword | Effect on the page |
|---|---|
| `type` | `object`, `boolean`, `integer`, `number` or `string`. A `string` or `integer` with `enum` becomes a choice. |
| `properties`, `required` | Fields of the object, and which ones must hold a value. |
| `title`, `description`, `help` | Field label, a detail line, and a short hint shown under the field. |
| `default` | Value used when no value is stored, and the value a `null` patch resets to. |
| `minimum`, `maximum`, `step`, `unit` | Range of a number, the change made by Up and Down, and a suffix such as `items`. |
| `pattern`, `maxLength` | Validation of a text field. |
| `enum`, `enum_titles` | Choice values, and their display strings (same length as `enum`). |
| `secret` | Masked field. The owning plugin reads the value; other plugins read an empty string. |
| `multiline` | Text area instead of a single-line input. |
| `picker` | `"file"` or `"directory"`, with optional `extensions`. The picked path is then available through `fs.read:picked`. |
| `items`, `maxItems` | Array of one primitive type, or of objects of up to eight primitive fields, for `list` rows. |
| `x-visible-when`, `x-scope`, `x-applies` | Annotations from `visible_when`, `scope: "project"` and `applies: "restart"`. They hide the row, store its value per project, and draw `↻`. |
| `x-steps`, `x-special-values` | Annotations from `steps` and `special_values` of a number row. |

A settings tab for the example plugin looks like this:

```json
{
  "contributes": {
    "settings": [
      {
        "kind": "tab",
        "id": "general",
        "title": "Demo",
        "sections": [
          {
            "id": "badges",
            "title": "Badges",
            "scope": "client",
            "rows": [
              { "type": "toggle", "key": "show_badges", "title": "Show badges",
                "default": true },
              { "type": "number", "key": "badge_limit", "title": "Badge limit",
                "unit": "items", "minimum": 1, "maximum": 50, "step": 1,
                "default": 12,
                "description": "Items shown next to each tree row, from 1 to 50." },
              { "type": "choice", "key": "theme", "title": "Theme", "default": "auto",
                "options": [
                  { "value": "auto", "title": "Follow system" },
                  { "value": "dark", "title": "Dark" },
                  { "value": "light", "title": "Light" }
                ] }
            ]
          }
        ]
      }
    ]
  }
}
```

The kernel validates every saved patch against the compiled schema before it is stored. A plugin's tab and section ids must be unique in the plugin (`manifest.settings-id-clash`), and a tab title must not equal a built-in tab label (`manifest.settings-title-clash`). Every row key must be unique across all tabs and sections of the plugin (`manifest.settings-key-clash`). A section in a built-in tab is placed after the built-in rows, and `before:main` is refused, so a plugin can never push a built-in tab's rows down.

<figure>
<pre class="term">╭  ≡ ● · Demo · plugin──────────────────────────────────╮
│<span class="t-dim">⧉ Added by example.demo 0.1.0                          </span>│
│                                                       │
│<span class="t-hi">BADGES                                                 </span>│
│  Show badges …………………………………………………………………………………… ‹ On ›  │
│  Badge limit ………………………………………………………………………… -  12  + *  │
│<span class="t-dim">    Items shown next to each tree row, from 1 to 50.   </span>│
│<span class="t-hi">  Theme ……………………………………………………………… ← Follow system →  +  </span>│
╰───────────────────────────────────────────────────────╯</pre>
<figcaption>The rows of the example tab, as the client draws them: rounded frame, <code>…</code> leaders, the selected row in the accent colours.</figcaption>
</figure>

## Custom pages with the views export

A custom page is a plugin tab with `"render": "view"`, drawn by the `views` export of the owning side. The tab entry has an `id`, a `title`, an optional Tabler `icon` and a glyph for the rail. It has no `sections`: the whole page belongs to the plugin, and the view target carries the tab id.

```json
{
  "client": {
    "entry": "client.wasm",
    "world": "ilium:client/base@0.1.0",
    "exports": ["views"]
  },
  "contributes": {
    "settings": [
      { "kind": "tab", "id": "demo", "title": "Demo", "icon": "adjustments",
        "render": "view", "after": "text-triggers" }
    ]
  },
  "capabilities": { "client": ["ui.panels"] }
}
```

The host calls `render` with the view target `settings-page("demo")`, which carries the local tab id:

```wit
record area { columns: u16, rows: u16 }
render: func(view: view-target, area: area) -> ui-tree;
```

A custom page usually mixes a `tabs` widget with a `schema-form` widget. The `schema-form` widget takes the plugin id and the id of one of the plugin's own sections, and draws that section's rows inside the custom page. The same validation and save path applies. Only the selected tab renders, so a tab's children are drawn only when that tab is chosen.

```text
block  titles: "Demo"  borders: all
  tabs  titles: "General", "About"  key: "demo-tab"
    child 0: schema-form  plugin: "example.demo"  section: "badges"
    child 1: paragraph    lines: "Badges count open items per tree row."
```

### Widgets a settings view may embed

A settings view is a normal view, so it may use the display widgets, `schema-form`, `button` and the `location-picker` ([Settings schema reference](/pages/reference-settings-schema/)). It may not use `popup`, `dialog`, `menu`, `video`, `terminal-view` or any input widget other than `button`, `schema-form` and `location-picker` ([UI API](/pages/api-ui/)); a tree that contains one traps with `ui.widget-not-allowed` on render. Kernel tabs never host view content. A view cannot reach the rows of a built-in tab; it can only open them through the link `ilium://settings/<tab-id>#<key>`.

**Rust**

The Rust SDK builds the tree with `ilium_plugin_sdk::ui::build::Tree`. Each widget has one constructor. A node is attached to its parent with `.child(...)`, and `tree.finish(root)` returns the `UiTree`.

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::ui::build::Tree;

#[ilium_plugin(side = "client", exports = ["views"])]
#[derive(Default)]
struct DemoSettings;

impl Views for DemoSettings {
    fn render(&mut self, view: ViewTarget, _area: Area) -> UiTree {
        let mut tree = Tree::new();
        let ViewTarget::SettingsPage(tab_id) = view else {
            let root = tree.paragraph(["This plugin draws no such view."]);
            return tree.finish(root);
        };
        if tab_id != "demo" {
            let root = tree.paragraph(["Unknown settings page."]);
            return tree.finish(root);
        }
        let badges = tree.schema_form("example.demo", "badges");
        let about = tree.paragraph(["Badges count open items per tree row."]);
        let tabs = tree
            .tabs(["General", "About"])
            .action_id("demo-tab")
            .child(badges)
            .child(about);
        let root = tree.block().title("Demo").child(tabs);
        tree.finish(root)
    }
}
```

### TypeScript example

**TypeScript**

In TypeScript, `definePlugin` returns one object per export. Settings are read with `getJson` from `@ilium/plugin-sdk/host/settings`, which returns the JSON text of the plugin's own values for that side. The `settings-changed` event carries a record with the changed keys, the origin of the write and the receiving side, and it also arrives for the plugin's own writes.

```ts
import { definePlugin, type Event } from '@ilium/plugin-sdk';
import { getJson, setJson } from '@ilium/plugin-sdk/host/settings';
import { log } from '@ilium/plugin-sdk/host/log';

type GeneralValues = { show_badges: boolean; badge_limit: number; theme: string };

function readValues(): GeneralValues {
  return JSON.parse(getJson()) as GeneralValues;
}

// Resets the badge limit to its schema default (12) with a null patch.
export function resetBadgeLimit(): void {
  setJson(JSON.stringify({ badge_limit: null }));
}

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  activate() {
    log('info', `badge limit is ${readValues().badge_limit}`);
  },
  onEvents(batch: Event[]) {
    for (const event of batch) {
      if (event.tag !== 'settings-changed') continue;
      if (event.val.keys.includes('badge_limit')) {
        log('info', `badge limit is now ${readValues().badge_limit}`);
      }
    }
    return [];
  },
});
```

## Reading and writing values from code

The `settings` import works on the plugin's own values. Keys belong to the side named by their entry's `scope`, so a client plugin never sees server keys through `get-json`.

| Rust | TypeScript | WIT | Grant | Errors |
|---|---|---|---|---|
| `get_json()` | `getJson()` | `get-json() -> string` | always | none |
| `set_json(patch)` | `setJson(patch)` | `set-json(patch) -> result` | always | `invalid-argument` |
| `list_settings()` | `listSettings()` | `list-settings() -> list<settings-ref>` | `settings.read-all` | none |
| `get_plugin_json(plugin)` | `getPluginJson(plugin)` | `get-plugin-json(...)` | `settings.read-all` | `not-found` |
| `set_plugin_json(plugin, patch)` | `setPluginJson(plugin, patch)` | `set-plugin-json(...)` | `settings.write-all` | `not-found`, `invalid-argument` |

`set-json` reads the patch as an RFC 7396 JSON merge patch. A key set to `null` resets to its schema `default`. A key that is not in the schema is `invalid-argument` and the message names it. The patch is applied all or nothing: one invalid key stores nothing. A patch that changes no stored value succeeds and sends no `settings-changed`.

`settings-changed` carries a `settings-change` record: `keys` (the changed keys, sorted), `origin` (`user`, `plugin`, `reset`, `other-plugin` or `file`) and `side` (`server` or `client`, the side that owns the keys). In Rust it is `Event::SettingsChanged(change)`, and in TypeScript `{ tag: 'settings-changed', val: { keys, origin, side } }`. A plugin has one code path for every change, including its own `set-json`.

`list-settings` returns `settings-ref` records with `plugin`, `tab`, `section` (when the entry is a section), `title`, `scope` and `kernel-tab` (when the section is in a built-in tab). A plugin without settings is not listed.

## Values saved from a page

Input widgets send a `ui-action` event, and the host applies a debounce before the plugin sees the value. Schema forms send no `ui-action` at all: the host validates and saves each change, and the owning plugin receives `settings-changed`.

| Widget | Sends | When |
|---|---|---|
| `toggle` | `changed` with `flag` | At once |
| `choice` | `changed` with `index` | At once |
| `number` | `changed` with `number` | 600 ms after the last change, whether from Up, Down, plus, minus or typed digits |
| `text-input` | `changed` with `text` | 600 ms after the last key; `submitted` on Enter when `submit-on-enter` is set |
| `text-area` | `changed` with `text` | 600 ms while the text is at most 10,000 bytes, 1 s above that |

The rows of a plugin tab and of a section follow the same timing (see the [auto-save table](/pages/settings-extension/)): toggles, choices and dialog confirmations write at once, number steps and typed text write 600 ms after the last change, text areas write 600 ms after the last change while the text is at most 10,000 bytes and 1 s after it above that. Closing Settings flushes any pending write.

**[SET-032]** The host MUST show the saving state on a row from the moment the user edits it until the write completes or is refused: the row's trailing marker reads `Saving…` (U+2026) while the 600 ms or 1 s debounce runs or a `set-json` is pending, and the marker disappears when the write completes or the save is refused. It is the same for the row of a plugin tab and for the schema-form of a view.

<figure>
<pre class="term">╭  ≡ ● · General · plugin───────────────────────────────╮
│<span class="t-hi">BADGES                                                 </span>│
│  Show badges …………………………………………………………………………………… ‹ On ›  │
│<span class="t-hi">  Badge limit ………………………………………………………………………… -  60  + *  </span>│
│<span class="t-warn">    badge_limit: 60 is above the maximum of 50         </span>│
│                                                       │
│<span class="t-dim">The stored value stays 12.                             </span>│
╰───────────────────────────────────────────────────────╯</pre>
<figcaption>A rejected value. The old value is kept, and the error replaces the description in the warning style.</figcaption>
</figure>

<figure>
<svg viewBox="0 0 400 170" role="img" aria-label="Two ways a settings page reaches the settings store">
  <title>Settings page paths</title>
  <desc>The manifest feeds either the kernel row renderer or the plugin views export. Both paths validate and write through the settings store.</desc>
  <defs>
    <marker id="settings-arrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
      <path d="M0,0 L8,4 L0,8 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="8" y="62" width="96" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
  <text x="56" y="86" text-anchor="middle" font-size="12" fill="currentColor">Manifest</text>
  <rect x="142" y="14" width="120" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="202" y="38" text-anchor="middle" font-size="12" fill="currentColor">Row renderer</text>
  <rect x="142" y="116" width="120" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
  <text x="202" y="140" text-anchor="middle" font-size="12" fill="currentColor">Plugin views</text>
  <rect x="290" y="62" width="102" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="341" y="86" text-anchor="middle" font-size="12" fill="currentColor">Settings store</text>
  <path d="M104 74 L142 40" fill="none" stroke="currentColor" marker-end="url(#settings-arrow)"/>
  <path d="M104 90 L142 132" fill="none" stroke="currentColor" marker-end="url(#settings-arrow)"/>
  <path d="M262 34 L328 62" fill="none" stroke="currentColor" marker-end="url(#settings-arrow)"/>
  <path d="M262 136 L328 102" fill="none" stroke="currentColor" marker-end="url(#settings-arrow)"/>
</svg>
<figcaption>Rows and custom pages both save through the settings store, which validates each patch against the compiled schema. The kernel recovery page is outside this path.</figcaption>
</figure>

## Errors and edge cases

| Error or event | When | What happens |
|---|---|---|
| `invalid-argument` from `set-json` | The patch is not a JSON object, breaks the schema, names a key outside the schema, or is over 256 KiB | Nothing is stored. The message names the JSON path of the failing field. |
| `manifest.schema-keyword` | A schema uses a keyword outside the subset | Install is refused with `settings schema keyword "<keyword>" at <json path> is not supported`. |
| `manifest.settings-target-grant` | A section names a built-in tab without `settings.contribute:<tab-id>` | Install is refused. |
| `manifest.settings-target-closed` | A section names a tab that takes no plugin sections, such as Animations or Debug | Install is refused. |
| `manifest.settings-key-clash` | Two row keys with the same name in one plugin, in any tab or section | Install is refused. |
| `manifest.grant-dependency` | `settings.write-all` without `settings.read-all` | Install is refused. |
| `settings.section-dropped` | A built-in tab already shows six plugin sections | Load warning. The section is not drawn, and the recovery page lists it. |
| `not-found` from `get-plugin-json` | The named plugin is not installed | The call fails. |
| Failure of `views.render` | `render` traps or breaks its deadline | The instance restarts. Three failures in 10 minutes disable the plugin. |

The full list of codes and limits is in [Errors and limits](/pages/reference-errors-and-limits/).

Other rules that affect a page:

- A `secret` value is returned to its owner. `get-plugin-json` returns it as an empty string to every other plugin, and the page shows the field masked.
- A schema form for another plugin's rows is drawn only when the rendering plugin has `settings.write-all`. Otherwise the widget shows the muted text `Settings page not available`.
- A plugin whose custom page fails shows this tree footer notice after the third failure: `Plugin <name> stopped after 3 failures in 10 minutes: <export> <code or trap> (<view>). Turn it on again in Settings > Plugins.`.
- A disabled plugin keeps its values. Its rows are dimmed and read-only, with a link to the recovery page. See [Lifecycle](/pages/lifecycle/).

<div class="warn">

A custom page whose `render` fails or traps is restarted like any other plugin instance. Three failures within 10 minutes disable the plugin, and the recovery page is then the only settings screen that can re-enable it. Do not make a plugin's custom page the only place a user can turn that plugin off.

</div>

## The kernel recovery page

The kernel owns one settings page that no plugin can replace. It can enable and disable plugins and open the raw configuration file. It stays reachable when the settings plugin is disabled or has failed, so a broken settings plugin can never hide the switch that repairs it. A disabled plugin's rows and tab link here through `[ Open Plugins page ]`.

<figure>
<pre class="term">╭  ≡ ● · Recovery──────────────────────────────────────────────╮
│ Plugins that failed or were disabled are listed first.       │
│                                                              │
│   disabled  example.demo     0.1.0  3 failures in 10 min     │
│   enabled   ilium.cost-view  0.1.0                           │
│   enabled   ilium.animations 0.1.0                           │
│                                                              │
│   [e] enable   [d] disable   [r] reset failures              │
│   [u] uninstall   [f] forget settings   [s] safe mode        │
│   [o] open config file   [q] close                           │
╰──────────────────────────────────────────────────────────────╯</pre>
<figcaption>Illustrative. Recovery page with every action. The settings plugin is disabled, so the recovery page is the only settings screen.</figcaption>
</figure>

The recovery page has six plugin actions, one action for the configuration file, and a close key. Each has one key, shown in the render: `e` enable, `d` disable, `r` reset failures, `u` uninstall, `f` forget settings, `s` safe mode, `o` open the configuration file, and `q` closes the page. Uninstall removes the plugin ([Install, update and uninstall](/pages/reference-install-update-uninstall/)). Reset failures clears the failure window without starting the plugin. Forget settings deletes the plugin's saved settings values for this user and starts the plugin with its defaults, and it keeps the plugin's permissions and data. Safe mode restarts Ilium with third-party plugins off ([Troubleshooting plugins](/pages/user-troubleshooting/#safe-mode)). Enable always clears the failure window. [LIF-272]

The page is opened from the `[ Open Plugins page ]` link that a dimmed plugin row shows, from the failed-start notice, or from Settings when the Settings plugin itself works. When the Settings plugin is off, the notice and the key `Ctrl+B` then `:` open the recovery page instead of Settings.

- [USR-160] The recovery page MUST offer the actions enable, disable, reset failures, uninstall, forget settings, safe mode and open config file, each with the key in the render, and MUST open from `Ctrl+B` then `:` when `ilium.core.settings` is off. Test: turn the Settings plugin off, press the keys, and assert the page and each key.

- [LIF-272] Enable from the recovery page MUST clear the failure window, and Reset failures MUST clear it without activating. Test: fail a plugin three times, press each.

### See also

- [Plugin settings extension](/pages/settings-extension/)
- [Settings schema reference](/pages/reference-settings-schema/)
- [Manifest reference](/pages/reference-manifest/)
- [Settings API](/pages/api-settings/)
- [Settings capability for other plugins](/pages/capability-settings-all/)
- [UI panels capability](/pages/capability-ui-panels/)
- [Accessibility and theming](/pages/accessibility-and-theming/)
- [Errors and limits](/pages/reference-errors-and-limits/)
- [Security model](/pages/security/)

Checked against the API version `ilium:plugin@0.1.0`.
