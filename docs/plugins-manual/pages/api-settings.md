---
layout: page
title: settings
permalink: /pages/api-settings/
---

## settings

Each plugin declares its settings in its manifest, under `contributes.settings`. A plugin can add a tab of its own to the Settings screen, and it can add sections of rows to a built-in tab such as Terminal or Editor. Each row is one value, and the kernel validates every change against the schema that the rows compile to. The kernel saves the values, draws the rows, and tells the plugin which keys changed. A plugin reads and writes its own values with no grant. A settings screen can also read and write the values of other plugins, but only with the all-settings grants.

The `settings` import lives in `ilium:host/settings`. It has five functions and one event for values. Eight row-update calls, one read-only import for built-in values and one client-side `open` call sit beside them (see [Dynamic content](#dynamic-content)). The 18 row types are shown on the [row gallery](/pages/reference-settings-rows/).

**You need:**

- No grant for the plugin's own values. `get-json` and `set-json` are always granted on the `server` and `client` sides. A plugin's own tab needs no other grant.
- `settings.contribute:<tab-id>` for each built-in tab where the plugin adds a section, for example `settings.contribute:terminal`. It allows drawing sections in that tab and nothing else. It is normal risk, and it neither implies nor is implied by the two all-settings grants. See [settings.read-all and settings.write-all](/pages/capability-settings-all/).
- `settings.read-kernel:<tab-id>` for the read-only `kernel-settings.read` call. It is normal risk for most tabs and high risk for the `api` tab, takes one tab id and gives no write access.
- `settings.read-all` for `list-settings` and `get-plugin-json`. This grant is high risk and available only to bundled first-party (T1) plugins.
- `settings.write-all` for `set-plugin-json`. It also needs `settings.read-all`. It is high risk and T1 only.
- A language: Rust with `ilium_plugin_sdk`, or TypeScript with `@ilium/plugin-sdk`.
- A `contributes.settings` entry in the manifest, if the plugin has settings. Without one, `get-json` returns `{}`.

<div class="note">
<p>The scene side has no settings import. A scene receives its own settings as the JSON string in its constructor argument (api-scenes).</p>
</div>

The SDK names of the calls on this page, including the `_request` / `Request` starting forms and the `_json` / `Json` names, follow [Wrapper names in the SDKs](/pages/reference-wit/#wrapper-names-in-the-sdks).

### Surface

| Name | Rust | TypeScript | Grant | Sides | Result and effect |
|---|---|---|---|---|---|
| `get-json` | `settings::get_json() -> String` | `getJson(): string` | always | server, client | Returns the plugin's own values for its side as one JSON object, from its tabs and sections alike. |
| `set-json` | `settings::set_json(&str) -> Result<(), PluginError>` | `setJson(patch: string): void`, throws `PluginError` | always | server, client | Applies a merge patch to the own values after validation. |
| `list-settings` | `settings::list_settings() -> Vec<SettingsRef>` | `listSettings(): SettingsRef[]` | `settings.read-all` | server, client | Lists the tabs and sections that every plugin with settings declares. |
| `get-plugin-json` | `settings::get_plugin_json(&str) -> Result<String, PluginError>` | `getPluginJson(plugin: string): string` | `settings.read-all` | server, client | Returns all values of another plugin. Secret values are empty strings. |
| `set-plugin-json` | `settings::set_plugin_json(&str, &str) -> Result<(), PluginError>` | `setPluginJson(plugin: string, patch: string): void` | `settings.write-all` | server, client | Applies a merge patch to another plugin's values, validated against that plugin's schema. |
| `settings-changed` | `Event::SettingsChanged(SettingsChange)` | `{ tag: 'settings-changed', val: SettingsChange }` | always | server, client | Carries the changed keys, the origin and the receiving side. Coalesced per batch. |

The `SettingsRef` record has these fields: `plugin` (the plugin id), `tab` (the global tab id `<plugin-id>/<tab-id>`, or the built-in tab id for a section in a built-in tab), `section` (the global section id, when the entry is a section), `title`, `scope` (`server` or `client`) and `kernel-tab` (an optional built-in tab id). In Rust it is imported from `ilium_plugin_sdk::host::settings`. In TypeScript it is imported as a type from `@ilium/plugin-sdk/host/settings`.

The `SettingsChange` record has three fields: `keys` (the changed keys, sorted, no duplicates), `origin` and `side` (the receiving side, `server` or `client`). The origin is one of:

- `user`: a row on the Settings screen changed, or the `r` key reset a row.
- `plugin`: the plugin's own `set-json` call.
- `reset`: a section's `[ Reset ]` control ran.
- `other-plugin`: a plugin with `settings.write-all` changed this plugin's values.
- `file`: `config.toml` was edited on disk and reloaded.

`settings-changed` arrives for every change to the plugin's own values. That includes a change the plugin makes with `set-json` and a change the user makes on the page. A plugin therefore has one code path for both. If writes with different origins are coalesced into one event, `origin` is the last one and `keys` is the union.

### Manifest: rows and their schema

A `tab` or `section` entry in `contributes.settings` holds rows. Each value row has a `type`, a `key`, a `title` and a `default`. The kernel compiles every value row into one property of a JSON-Schema subset validated with draft 2020-12, so the same checks run for rows and for `set-json`. The full field tables are in the [settings schema reference](/pages/reference-settings-schema/).

```json
{
  "contributes": {
    "settings": [
      {
        "kind": "section",
        "id": "notes-editing",
        "tab": "editor",
        "placement": "end",
        "title": "Notes",
        "scope": "client",
        "rows": [
          { "type": "toggle", "key": "sound_enabled", "title": "Play a sound",
            "default": true },
          { "type": "number", "key": "indent_width", "title": "Indent width",
            "minimum": 1, "maximum": 8, "step": 1, "unit": "spaces",
            "default": 4 },
          { "type": "number", "key": "zoom", "title": "Zoom", "integer": false,
            "minimum": 0.5, "maximum": 2, "step": 0.1, "default": 1 },
          { "type": "text", "key": "sync_token", "title": "Sync token",
            "secret": true, "default": "" },
          { "type": "text-area", "key": "template", "title": "Template",
            "default": "" },
          { "type": "choice", "key": "density", "title": "Density",
            "default": "comfortable",
            "options": [
              { "value": "compact", "title": "Compact" },
              { "value": "comfortable", "title": "Comfortable" }
            ] }
        ]
      },
      {
        "kind": "tab",
        "id": "notes",
        "title": "Notes",
        "icon": "notes",
        "render": "view"
      }
    ]
  }
}
```

The `Notes` tab in the example is a custom page: it has `"render": "view"`, so the plugin's `views` export draws it. A plugin declares its custom pages this way, as tabs, and not in a separate list.

The keywords that a row's schema may use are fixed. Any other keyword is refused at install with `manifest.schema-keyword`.

| Keyword | Applies to | Meaning |
|---|---|---|
| `type` | all | `object`, `boolean`, `integer`, `number`, `string` or `array`. The `enum` form is a `string` or `integer` with `enum`. |
| `enum` | `string`, `integer` | The allowed values. Any other value is `invalid-argument`. |
| `enum_titles` | `enum` | Display strings for the `enum` values, same length and order. |
| `items`, `maxItems` | `array` | One primitive type, or an object of at most eight primitive fields, for a `list` row. At most 64 items. |
| `title`, `help`, `description` | all | Row label, help text shown with the field, and documentation text. |
| `default` | all | The value used before the user or the plugin sets one. |
| `minimum`, `maximum`, `step` | `integer`, `number` | Range and the step of the control. |
| `unit` | `integer`, `number` | Text shown after the value, such as `spaces`. |
| `pattern`, `maxLength`, `required` | `string`, `object` | Standard JSON-Schema checks. |
| `secret` | `string` | The field is masked on the page, and the value is redacted from logs. |
| `multiline` | `string` | The field is a text area. |
| `picker` | `string` | `"file"` or `"directory"`, with optional `extensions`. Adds a path chooser. |
| `x-visible-when` | all value rows | Compiled from `visible_when`. A false condition hides the row. It is a UI annotation and never takes part in validation. |
| `x-scope` | all value rows | Compiled from `scope: "project"`. The value is stored per project. |
| `x-applies` | all value rows | Compiled from `applies: "restart"`. Draws `↻` and the note `Applies after restart.` |
| `x-steps`, `x-special-values` | `integer`, `number` | Compiled from `steps` and `special_values`. A ladder of allowed stops, and values drawn as a title. |

The error codes `manifest.catalog-unknown` (a choice `source` names an unknown catalogue) and `manifest.settings-source` (a `choice` row with both or neither of `options` and `source`) are install-time manifest errors.

Row keys must be unique within one plugin, across all its tabs and sections. A duplicate is refused at install with `manifest.settings-key-clash`.

### Live reload of settings

The kernel store is the single copy of every value. A write by the user, by a file edit, or by a tab owner (`settings.write-kernel`) reaches a running plugin as the event `settings-changed` (own keys) or `kernel-settings-changed` (kernel tabs) without a restart. Values that need a restart are listed in [kernel and boot](/pages/reference-kernel-and-boot/), table "Live reload and restart". [COR-024]

### Writing values: merge patch

`set-json` and `set-plugin-json` take a JSON merge patch (RFC 7396). A key in the patch replaces the stored value. A key that is absent is unchanged. A key set to `null` resets that key to its schema `default`. The patch is applied as a whole: if one key fails, nothing is stored. A patch that changes no value succeeds and sends no `settings-changed` event.

A key that belongs to the other side (see the scope rule below) is `invalid-argument`, and so is a key that is not in the schema. `set-json` is not debounced, and it does not call the plugin's own `settings-validate` export, because the plugin already knows its rules.

### Optional validation: settings-validate

A plugin may export `settings-validate` on the side that owns the keys. The kernel calls it after the schema check and before a save, for changes made by the user and for resets. It is never called for `set-json` or `set-plugin-json`.

```wit
record field-error { key: string, message: string }   // message <= 120 characters
settings-validate: func(patch: string, merged: string) -> result<_, list<field-error>>;
```

The call has a deadline of 100 ms. A timeout, a trap or a stopped plugin refuses the save, and the previous value stays. Each `field-error` replaces the matching row's description line in the warning style.

### Dynamic content

These calls change a row while the plugin runs. They need no grant, because they cover only the plugin's own rows. They are limited to 10 calls per second per plugin, all kinds together, and nothing is coalesced. A call over the limit returns `rate-limited`. A call that names an unknown key or id returns `not-found`.

| Call | Rust | TypeScript | Applies to | Effect |
|---|---|---|---|---|
| `set-info(key, text)` | `settings::set_info` | `setInfo` | an `info` row with `live: true` | Replaces the text, at most 120 characters. |
| `set-info-rich(key, spans)` | `settings::set_info_rich` | `setInfoRich` | a live `info` row with `rich` | Replaces the spans `{ text, tone, bold, command }`. |
| `set-choices(key, options)` | `settings::set_choices` | `setChoices` | a `choice` row with `dynamic: true` | Replaces the options, 1 to 64. A stored value that is no longer offered shows as `‹ value (unavailable) ›`. |
| `set-choice-items(key, items)` | `settings::set_choice_items` | `setChoiceItems` | a `choice` or `multi-choice` row with a `source` catalogue | Replaces the items of the catalogue, with description, preview and disabled reason. |
| `set-row-note(key, note)` | `settings::set_row_note` | `setRowNote` | any row | Sets or clears a dim note under the description, at most 120 characters. |
| `set-row-state(key, state)` | `settings::set_row_state` | `setRowState` | any value row | Sets `idle`, `loading`, `pending` or `error`, with a message of at most 80 characters. |
| `set-item-states(key, states)` | `settings::set_item_states` | `setItemStates` | a `list` row with `remote` or `item_status` | Sets the state of single items. |
| `set-meter(id, segments)` | `settings::set_meter` | `setMeter` | a `meter` row | Replaces up to 8 segments `{ label, value, tone }`. |

`open(target)` is a client import. It opens Settings at a plugin's tab, at one of its sections, or at one row. The targets are `<plugin-id>/<tab-id>`, `<plugin-id>/<section-id>` and `<plugin-id>/<section-id>/<key>`. A target of another plugin is refused with `invalid-argument`.

`kernel-settings.read(tab)` returns the values of one built-in tab as JSON, read-only, with secrets replaced by `<redacted>`. It needs `settings.read-kernel:<tab-id>`. A change arrives as the event `kernel-settings-changed`. To point the user at a built-in row, open the link `ilium://settings/<tab-id>#<key>` with `open-external`.

**Rust**

```rust
use ilium_plugin_sdk::host::settings::{self, MeterSegment, RowState, RowStateKind, Tone};

fn show_budget(used: f64) -> Result<(), PluginError> {
    settings::set_meter("budget", &[
        MeterSegment { label: "used".into(), value: used, tone: Tone::Success },
    ])?;
    settings::set_row_state("api_key", &RowState {
        kind: RowStateKind::Loading,
        message: Some("checking the key".into()),
    })
}
```

**TypeScript**

```ts
import { setMeter, setRowState } from '@ilium/plugin-sdk/host/settings';

export function showBudget(used: number): void {
  setMeter('budget', [{ label: 'used', value: used, tone: 'success' }]);
  setRowState('api_key', { kind: 'loading', message: 'checking the key' });
}
```

### Examples

Both examples use the always-granted own values. The Rust example adds the `serde_json` crate to the plugin's `Cargo.toml` to build and read JSON.

**Example 1: write and read own values**

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::settings;

fn remember_density(choice: &str) -> Result<(), PluginError> {
    let patch = serde_json::json!({ "density": choice }).to_string();
    settings::set_json(&patch)
}

fn read_indent() -> u32 {
    let values: serde_json::Value = serde_json::from_str(&settings::get_json())
        .unwrap_or(serde_json::Value::Null);
    values["indent_width"].as_u64().unwrap_or(4) as u32
}
```

**TypeScript**

```ts
import { getJson, setJson } from '@ilium/plugin-sdk/host/settings';

export function rememberDensity(choice: 'compact' | 'comfortable'): void {
  setJson(JSON.stringify({ density: choice }));
}

export function readIndent(): number {
  const values: Record<string, unknown> = JSON.parse(getJson());
  const indent = values['indent_width'];
  return typeof indent === 'number' ? indent : 4;
}
```

Calling `set_json` with `{"indent_width": 12}` fails with `invalid-argument`, because the maximum is 8. The message names the key. Nothing is stored.

**Example 2: react to a change**

This client-side handler keeps the indent in memory and refreshes it only when `indent_width` changes. The change can come from the plugin or from the user.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::settings;

#[ilium_plugin(side = "client")]
#[derive(Default)]
struct IndentWatcher {
    indent: u32,
}

impl Handler for IndentWatcher {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::SettingsChanged(change) = event else { continue };
            if change.keys.iter().any(|key| key == "indent_width") {
                let values: serde_json::Value = serde_json::from_str(&settings::get_json())
                    .unwrap_or(serde_json::Value::Null);
                self.indent = values["indent_width"].as_u64().unwrap_or(4) as u32;
            }
        }
        Vec::new()
    }
}
```

**TypeScript**

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { getJson } from '@ilium/plugin-sdk/host/settings';

let indent = 4;

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag !== 'settings-changed') continue;
      if (!event.val.keys.includes('indent_width')) continue;
      const values: Record<string, unknown> = JSON.parse(getJson());
      const next = values['indent_width'];
      indent = typeof next === 'number' ? next : 4;
    }
    return [];
  },
});
```

**Example 3: read the settings of other plugins (T1 only)**

A settings screen lists the tabs and sections of every plugin and reads one plugin's values. The plugin needs `settings.read-all` in its manifest and the user must grant it.

```ts
import { getPluginJson, listSettings } from '@ilium/plugin-sdk/host/settings';

export function dumpNotesSettings(): string[] {
  const lines: string[] = [];
  for (const entry of listSettings()) {
    lines.push(`${entry.plugin} / ${entry.tab} (${entry.scope}): ${entry.title}`);
  }
  lines.push(getPluginJson('ilium.notes'));
  return lines;
}
```

### Scope and access

Each entry's `scope` decides which side owns its keys. A `client` key is read and written by the client instance of the plugin, and a `server` key by the server instance. `get-json` on a side returns only that side's keys, `set-json` with a key of the other side is `invalid-argument`, and `settings-changed` goes only to the owning side. A section must be single-scope. A plugin that needs both sides declares two sections.

A settings screen uses `list-settings` to find the tabs and sections. A section is drawn as rows inside its tab. A plugin tab with `"render": "view"` is drawn by the plugin's `views` export through the `settings-page("<tab-id>")` view target. See [Building settings screens](/pages/settings-screens/).

### Errors

| Code | Returned by | When |
|---|---|---|
| `invalid-argument` | `set-json`, `set-plugin-json`, `open`, row-update calls | The patch is not one JSON object, breaks the schema, names an unknown key or a key of the other side, or is over 256 KiB. An `open` target of another plugin is refused too. |
| `not-found` | `get-plugin-json`, `set-plugin-json`, row-update calls | The named plugin is not installed, or a row key or id is unknown. |
| `rate-limited` | row-update calls | More than 10 calls per second. |
| `not-permitted` | `list-settings`, `get-plugin-json`, `set-plugin-json`, `kernel-settings.read` | The grant is missing or was declined. Universal. |
| `revoked`, `internal` | all | Universal. `revoked` means the plugin is re-activated. |

`get-json` has no error code of its own. A tab or section that a plugin does not declare is simply left out of `list-settings`.

### What the user sees

- **A plugin tab** appears in the Settings rail, next to the built-in tabs. Its frame title ends with `· plugin`, and its first line names the plugin and its version.
- **A section in a built-in tab** sits under a divider line that names the plugin and the word `plugin`. Every line of the plugin's rows carries a dim gutter, so the user can always tell whose rows they are.
- Each row has a label, dotted leaders and a control. Secret fields show `••••`, and the last characters when `reveal_last` is set. A changed value has a dim `•` after its label, and a pending write has a dim `◌` after the value.
- A change saved on the page reaches the plugin as `settings-changed` in its next event batch.
- A rejected value is not saved. The reason names the key, so the user can correct the field.
- A disabled plugin's rows stay visible, but read-only and dimmed. Its values are kept.
- The kernel recovery page stays reachable when the settings plugin itself fails. It can re-enable any plugin, so a broken plugin never hides its own repair switch.

<figure>
<pre class="term">╭  ≡ ● · Notes · plugin─────────────────────────────────╮
│<span class="t-dim">⧉ Added by example.notes 0.1.0                         </span>│
│                                                       │
│<span class="t-hi">SYNC                                                   </span>│
│  Play a sound ………………………………………………………………………………… ‹ On ›  │
│<span class="t-dim">    Also play it when a note is saved.                 </span>│
│  Indent width• ……………………………………………………………… -   4    + *  │
│<span class="t-dim">    Spaces per level.                                  </span>│
│<span class="t-hi">  Density ……………………………………………………………… ← Comfortable →  +  </span>│
│<span class="t-dim">    How tight the list is drawn.                       </span>│
│  Sync token ………………………………………………………………………………… ••••a1b2  │
│<span class="t-dim">    Stored as a secret.                                </span>│
╰───────────────────────────────────────────────────────╯</pre>
<figcaption>The rows of the notes tab, as the client draws them: leaders of <code>…</code>, the selected row in the accent colours, the dim <code>•</code> mark on a changed value and a masked secret.</figcaption>
</figure>

### How the pieces fit

<figure>
<svg viewBox="0 0 420 170" role="img" aria-label="Settings flow from manifest rows to page, store and plugin">
  <defs><marker id="arr-settings-flow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto"><path d="M0,0 L10,5 L0,10 z" fill="currentColor"/></marker></defs>
  <rect x="8" y="20" width="110" height="44" rx="6" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
  <text x="63" y="46" font-size="12" text-anchor="middle" fill="currentColor">Manifest</text>
  <rect x="155" y="20" width="110" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="210" y="46" font-size="12" text-anchor="middle" fill="currentColor">Settings rows</text>
  <rect x="302" y="20" width="110" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="357" y="46" font-size="12" text-anchor="middle" fill="currentColor">Kernel store</text>
  <line x1="118" y1="42" x2="154" y2="42" stroke="currentColor" marker-end="url(#arr-settings-flow)"/>
  <text x="136" y="60" font-size="10" text-anchor="middle" fill="currentColor">schema</text>
  <line x1="265" y1="42" x2="301" y2="42" stroke="currentColor" marker-end="url(#arr-settings-flow)"/>
  <text x="283" y="60" font-size="10" text-anchor="middle" fill="currentColor">save</text>
  <rect x="302" y="110" width="110" height="44" rx="6" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
  <text x="357" y="136" font-size="12" text-anchor="middle" fill="currentColor">Plugin</text>
  <line x1="357" y1="64" x2="357" y2="109" stroke="currentColor" marker-end="url(#arr-settings-flow)"/>
  <text x="349" y="92" font-size="10" text-anchor="end" fill="currentColor">settings-changed</text>
  <text x="8" y="166" font-size="11" fill="currentColor">The plugin never reads the page. It reads only the store.</text>
</svg>
<figcaption>The manifest rows draw the page. Every save passes through the kernel store, which validates it and then tells the plugin what changed.</figcaption>
</figure>

### Edge cases

- **Concurrent writes.** Each patch is applied to the stored values at the moment the kernel handles it. Two patches in one batch apply in order.
- **Types.** An `integer` field rejects `4.5`. A `number` field accepts `4`. A string over `maxLength` is `invalid-argument`.
- **Unset keys.** A key with a `default` and no stored value reads as its default in `get-json`.
- **Secrets.** A `secret` value is kept out of logs and out of the snapshot. Other plugins see an empty string from `get-plugin-json`.
- **Restarts.** A restart does not reset stored values. A plugin that restarts after a failure reads them again with `get-json`. Rows of a restarting plugin stay editable, unless the plugin exports `settings-validate`.
- **Disable and uninstall.** Disabling or uninstalling a plugin never deletes its values. They stay in `[plugins."<plugin-id>"]` until the user purges them. See [Lifecycle](/pages/lifecycle/).

### See also

- [ui](/pages/api-ui/) for the view that renders a custom settings page.
- [storage](/pages/api-storage/) for values the user does not edit.
- [Plugin exports](/pages/api-exports/) for the `views` export.
- [Events](/pages/api-events/) for the event batch that carries `settings-changed`.
- [Building settings screens](/pages/settings-screens/) for rows and custom page design.
- [Plugin settings extension](/pages/settings-extension/) for tabs, sections and anchors.
- [settings.read-all and settings.write-all](/pages/capability-settings-all/) for the grant rules.
- [Plugin settings schema](/pages/reference-settings-schema/) for the 256 KiB cap on all values of one plugin, and [Errors and limits](/pages/reference-errors-and-limits/) for the full code table.

### Security

Secret settings are stored outside plain settings files and redacted from logs; see [Secrets](/pages/reference-secrets/).
