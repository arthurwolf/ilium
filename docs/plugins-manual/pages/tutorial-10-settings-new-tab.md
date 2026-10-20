---
layout: page
title: "10. A settings tab of your own"
permalink: /pages/tutorial-10-settings-new-tab/
---

## 10. A settings tab of your own

This step adds a tab to the Settings screen. The tab is called Clipboard history, and it uses seven of the 18 row types: a toggle, a choice, a number with a unit, a text field (also used as a secret), a keybinding, a file path and a button. One toggle is stored per project. The host draws the page from the manifest, saves each value as the user changes it, and sends `settings-changed` to the plugin. The plugin reads the saved values, reacts when they change, and checks one of them before the host saves it. The step also shows a row that hides itself, a row that applies after restart, a value stored per project, and the search filter. Every row type is on the [row gallery](/pages/reference-settings-rows/).

The tab belongs to the plugin. Its values are stored in `[plugins."example.clipboard-history"]` in `config.toml`, and the tab needs no grant. The kernel keeps every built-in tab as it was: a plugin tab can be added, but never changed.

Tutorial plugin: `example.clipboard-history`, version 0.1.0, API `ilium:plugin@0.1.0`. The client side is shown in Rust and in TypeScript in every step.

### You will need

- A clipboard-history project with a client side that records copied text and exports `handler`. This step adds the settings tab to that code.
- Grants: none new. The plugin's own values are always granted, so `capabilities.client` stays empty for this step. The clipboard grants of the project stay as they are.
- Rust: the crates `serde` and `serde_json` (already in the project) and `regex-lite`, added for the check in step 5. TypeScript: nothing more, because `RegExp` is built in.
- The SDKs: the Rust crate `ilium_plugin_sdk` and the TypeScript package `@ilium/plugin-sdk`.

### The surface this step uses

| Area | Name | Meaning |
|---|---|---|
| Manifest | `contributes.settings[]`, `kind: "tab"` | A tab in the rail. Its global id is `example.clipboard-history/history`. |
| Manifest | `sections[]`, row `type` | Each section has one `scope`. Value rows own a `key`. A `button` has an `id` and stores no value. |
| Manifest row field | `visible_when`, `scope: "project"`, `applies: "restart"` | Hide a row, store a value per project, mark a value that applies after restart. |
| Host | `settings::get_json()` / `getJson()` | Every own key, with the defaults filled in. |
| Export | `settings-validate` | Optional. Declared in `client.exports`. 100 ms, user and reset origins only. |
| Event | `settings-changed` | `SettingsChange { keys, origin, side }`. |

`origin` is `user`, `plugin`, `reset`, `other-plugin` or `file`. `keys` is sorted and has no duplicates.

### The flow

A row on the page is saved by the host, not by the plugin. The host checks the value against the schema, calls the plugin's `settings-validate` for user and reset changes, and writes the value. Only then does it send `settings-changed`. A refused save keeps the old value, and the error replaces the row's description.

<figure>
<svg viewBox="0 0 400 300" role="img" aria-label="Value flow from a user edit to the plugin">
  <defs>
    <marker id="arrow-value-flow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
      <path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="10" y="10" width="380" height="36" rx="4" fill="none" stroke="currentColor"/>
  <text x="200" y="33" font-size="11" text-anchor="middle" fill="currentColor">1. User edits a row on the Settings page</text>
  <line x1="200" y1="46" x2="200" y2="68" stroke="currentColor" marker-end="url(#arrow-value-flow)"/>
  <rect x="10" y="70" width="380" height="36" rx="4" fill="none" stroke="currentColor"/>
  <text x="200" y="93" font-size="11" text-anchor="middle" fill="currentColor">2. Host checks the schema, then settings-validate</text>
  <line x1="200" y1="106" x2="200" y2="128" stroke="currentColor" marker-end="url(#arrow-value-flow)"/>
  <rect x="10" y="130" width="380" height="36" rx="4" fill="none" stroke="currentColor"/>
  <text x="200" y="153" font-size="11" text-anchor="middle" fill="currentColor">3. Host saves the value in config.toml</text>
  <line x1="200" y1="166" x2="200" y2="188" stroke="currentColor" marker-end="url(#arrow-value-flow)"/>
  <rect x="10" y="190" width="380" height="36" rx="4" fill="none" stroke="currentColor" stroke-dasharray="5 3"/>
  <text x="200" y="213" font-size="11" text-anchor="middle" fill="currentColor">4. Event: settings-changed (keys, origin, side)</text>
  <line x1="200" y1="226" x2="200" y2="248" stroke="currentColor" marker-end="url(#arrow-value-flow)"/>
  <rect x="10" y="250" width="380" height="36" rx="4" fill="none" stroke="currentColor" stroke-dasharray="5 3"/>
  <text x="200" y="273" font-size="11" text-anchor="middle" fill="currentColor">5. Plugin reads the values and refreshes its state</text>
</svg>
<figcaption>Value flow of one user edit. Solid boxes are host steps, dashed boxes are what the plugin receives. A refused save stops after step 2, and the plugin hears nothing.</figcaption>
</figure>

### Step 1: the manifest

The manifest gains two commands, one keybinding and one settings entry of kind `tab`. The button and the keybinding both name a command of the plugin, so both commands are listed under `contributes.commands`. The tab has three sections: Capture holds the three values of the history itself, Picker holds the secret filter and the key, and Sync and files holds the project toggle, the secret token, the path and the button.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "example.clipboard-history",
  "name": "Clipboard history",
  "version": "0.1.0",
  "publisher": "Example Developer",
  "description": "Keeps a searchable history of copied text.",
  "icon": "clipboard-list",
  "client": {
    "entry": "client.wasm",
    "world": "ilium:client/base@0.1.0",
    "exports": ["settings-validate"]
  },
  "capabilities": { "client": [] },
  "activation": ["startup"],
  "contributes": {
    "commands": [
      { "id": "open-picker", "title": "Clipboard history: open picker" },
      { "id": "clear", "title": "Clipboard history: clear" }
    ],
    "keybindings": [
      { "command": "open-picker", "chord": "ctrl+alt+v", "mode": "global" }
    ],
    "settings": [
      {
        "kind": "tab",
        "id": "history",
        "title": "Clipboard history",
        "short_title": "Clipboard",
        "icon": "clipboard-list",
        "glyph": "⎘",
        "after": "text-triggers",
        "order": 500,
        "render": "rows",
        "keywords": ["copy", "paste"],
        "sections": [
          {
            "id": "capture",
            "title": "Capture",
            "scope": "client",
            "rows": [
              { "type": "toggle", "key": "enabled", "title": "Keep history",
                "default": true,
                "description": "Record text copied in panes and the editor." },
              { "type": "choice", "key": "store", "title": "Store", "default": "text",
                "options": [
                  { "value": "text", "title": "Text only" },
                  { "value": "rich", "title": "Text and HTML" }
                ],
                "description": "Which clipboard content is kept." },
              { "type": "number", "key": "max_entries", "title": "Maximum entries",
                "minimum": 10, "maximum": 5000, "step": 10, "unit": "entries",
                "default": 500,
                "description": "Oldest entries are removed first." }
            ]
          },
          {
            "id": "picker",
            "title": "Picker",
            "scope": "client",
            "rows": [
              { "type": "toggle", "key": "hide_secrets", "title": "Hide secrets",
                "default": true,
                "description": "Skip copies that look like a key or a token." },
              { "type": "text", "key": "ignore_pattern", "title": "Ignore pattern",
                "max_length": 200, "placeholder": "regular expression",
                "default": "^sk-[A-Za-z0-9]{20,}$",
                "visible_when": { "key": "hide_secrets", "equals": true },
                "description": "Copies that match are never stored." },
              { "type": "keybinding", "key": "picker_key", "title": "Picker key",
                "command": "open-picker", "default": "ctrl+alt+v",
                "description": "Opens the history picker." }
            ]
          },
          {
            "id": "sync",
            "title": "Sync and files",
            "scope": "client",
            "rows": [
              { "type": "toggle", "key": "per_project", "title": "Separate per project",
                "default": false, "scope": "project",
                "description": "Keep one history for each project." },
              { "type": "text", "key": "sync_token", "title": "Sync token",
                "secret": true, "reveal_last": 4, "max_length": 128, "default": "",
                "applies": "restart",
                "description": "Stored as a secret. Applies after restart." },
              { "type": "path", "key": "history_file", "title": "History file",
                "picker": "file", "extensions": ["db"], "default": "",
                "description": "File picker; other folders need fs access." },
              { "type": "button", "id": "clear_now", "title": "Clear history",
                "label": "Clear now", "command": "clear",
                "confirm": "Delete every stored entry?" }
            ]
          }
        ]
      }
    ]
  }
}
```

Points to notice:

- The keys `enabled`, `store`, `max_entries`, `hide_secrets`, `ignore_pattern`, `picker_key`, `per_project`, `sync_token` and `history_file` must be unique across the whole plugin. A clash fails the install with `manifest.settings-key-clash`. The button's `id` shares the same namespace.
- Every `default` must pass its own row. `max_entries` defaults to 500, which sits inside 10 to 5000. A default outside the range fails with `manifest.settings-default`.
- `visible_when` on `ignore_pattern` uses the shape `{ "key", "equals" }` on one own key. A hidden row keeps its value, and search skips it.
- `scope: "project"` on `per_project` stores that value for the open project. The project value wins while that project is open.
- `applies: "restart"` on `sync_token` draws the marker `↻` after the value and the note `Applies after restart.`
- `secret: true` with `reveal_last: 4` masks the value as `••••` and shows its last four characters. The kernel keeps the value out of logs.
- A `keybinding` row stores a remap of one of the plugin's own keybindings. Its command must be listed in `contributes.commands`, or the install fails with `manifest.settings-command`.
- `after` places the tab straight after Text Triggers. The tail of the rail (Debug, API, About and Setup) is pinned, so no plugin tab can appear after it.

<div class="note">
<p>The tab's first line reads "Added by example.clipboard-history 0.1.0", and each section title is drawn in capitals. The plugin cannot change either one, because the kernel draws them.</p>
</div>

#### What you see

Before the plugin is installed, the rail has no Clipboard entry. After it is enabled, the tab appears after Text Triggers. Its short title is used, because the full title does not fit the rail at 80 columns.

<figure>
<pre class="term">⚙ Settings                                                               ✕ Close
<span class="t-dim">Esc or q to close                                               [ Guided setup ]</span>
╭  ≡ ● · Navigation╮ ╭  ≡ ● · Text Triggers────────────────────────────────────╮
│ ⚑  Triggers      │ │<span class="t-dim">                                                         </span>│
<span class="t-hi">│ ¶  Text Triggers │</span> │<span class="t-hi">  Text Triggers                                          </span>│
│ ⌕  Debug         │ │                                                         │
│ ⇄  API           │ │                                                         │
│ ⓘ  About         │ │                                                         │
│ ⚙  Setup         │ │                                                         │
│                  │ │                                                         │
│                  │ │                                                         │
╰──────────────────╯ ╰─────────────────────────────────────────────────────────╯</pre>
<figcaption>Before: the rail without the plugin. The selected entry is Text Triggers, drawn in the selected style.</figcaption>
</figure>

<figure>
<pre class="term">⚙ Settings                                                               ✕ Close
<span class="t-dim">Esc or q to close                                               [ Guided setup ]</span>
╭  ≡ ● · Navigation╮ ╭  ≡ ● · Clipboard history · plugin───────────────────────╮
│ ⚑  Triggers      │ │                                                         │
│ ¶  Text Triggers │ │<span class="t-dim">⧉ Added by example.clipboard-history 0.1.0               </span>│
<span class="t-hi">│ ⎘  Clipboard     │</span> │                                                         │
│ ⌕  Debug         │ │<span class="t-hi">CAPTURE                                                  </span>│
│ ⇄  API           │ │<span class="t-hi">  Keep history ………………………………………………………………………………… ‹ On ›    </span>│
│ ⓘ  About         │ │                                                         │
│ ⚙  Setup         │ │                                                         │
│                  │ │                                                         │
╰──────────────────╯ ╰─────────────────────────────────────────────────────────╯</pre>
<figcaption>After: the plugin tab sits after Text Triggers, and the pinned tail follows. The frame title ends with <code>· plugin</code>.</figcaption>
</figure>

### Step 2: read the values

The plugin reads its values with one call. The call always returns every key of the tab with its default filled in, so the types need no optional fields. The plugin reads the values once when it activates, and again each time `settings-changed` arrives (step 3).

The `Config` type mirrors the keys. A `choice` row stores its option value, so `store` is a string. A `number` row stores an integer, and a `keybinding` or `path` row stores a string, so all of them map onto plain types. The full files are below. The Rust file is complete; the TypeScript file is complete too.

**Rust**

```rust
// src/lib.rs. Client side of clipboard-history, 0.1.0.
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::host::settings;
use ilium_plugin_sdk::{
    ilium_plugin, Activation, Event, FieldError, Handler, Lifecycle, PluginError,
    SettingsOrigin, Submission,
};
use serde::Deserialize;

/// The values of the `history` tab, keyed as in the manifest.
#[derive(Deserialize, Default)]
struct Config {
    enabled: bool,
    store: String,
    max_entries: u32,
    hide_secrets: bool,
    ignore_pattern: String,
    picker_key: String,
    per_project: bool,
    sync_token: String,
    history_file: String,
}

#[ilium_plugin(side = "client")]
#[derive(Default)]
struct ClipboardHistory {
    config: Config,
    entries: Vec<String>,
}

impl ClipboardHistory {
    /// Reads the saved values. A value that does not parse keeps the old config.
    fn reload(&mut self) {
        match serde_json::from_str::<Config>(&settings::get_json()) {
            Ok(config) => self.config = config,
            Err(error) => log::log(Level::Warn, &format!("settings not read: {error}")),
        }
    }

    fn trim_history(&mut self) {
        let limit = self.config.max_entries as usize;
        if self.entries.len() > limit {
            let excess = self.entries.len() - limit;
            self.entries.drain(..excess);
        }
    }
}

impl Lifecycle for ClipboardHistory {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        self.reload();
        Ok(())
    }
}

impl Handler for ClipboardHistory {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::SettingsChanged(change) = event else { continue };
            self.reload();
            if change.keys.iter().any(|key| key == "max_entries") {
                self.trim_history();
            }
            if change.keys.iter().any(|key| key == "per_project") {
                self.entries.clear(); // the project value picks another history
            }
            if change.origin == SettingsOrigin::Reset {
                log::log(Level::Info, "settings reset from the Settings screen");
            }
        }
        Vec::new()
    }

    /// Runs before a user or reset save is written. It has 100 ms, so it must
    /// stay pure: no file, network or clipboard access.
    fn settings_validate(&mut self, _patch: &str, merged: &str) -> Result<(), Vec<FieldError>> {
        let Ok(config) = serde_json::from_str::<Config>(merged) else {
            return Ok(()); // the schema check already passed; nothing to add
        };
        match regex_lite::Regex::new(&config.ignore_pattern) {
            Ok(_) => Ok(()),
            Err(_) => Err(vec![FieldError {
                key: "ignore_pattern".into(),
                message: format!("Not a valid pattern: {}", config.ignore_pattern),
            }]),
        }
    }
}
```

**TypeScript**

```ts
// src/client.ts. Client side of clipboard-history, 0.1.0.
import { definePlugin, type Event } from '@ilium/plugin-sdk';
import { log } from '@ilium/plugin-sdk/host/log';
import { getJson } from '@ilium/plugin-sdk/host/settings';

type Config = {
  enabled: boolean;
  store: 'text' | 'rich';
  max_entries: number;
  hide_secrets: boolean;
  ignore_pattern: string;
  picker_key: string;
  per_project: boolean;
  sync_token: string;
  history_file: string;
};

let config: Config | undefined;
const entries: string[] = [];

/** Reads every key of the tab. get-json fills in the defaults. */
function reload(): void {
  config = JSON.parse(getJson()) as Config;
}

function trimHistory(): void {
  if (!config) return;
  const excess = entries.length - config.max_entries;
  if (excess > 0) entries.splice(0, excess);
}

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  activate() {
    reload();
  },
  onEvents(batch: Event[]) {
    for (const event of batch) {
      if (event.tag !== 'settings-changed') continue;
      reload();
      if (event.val.keys.includes('max_entries')) trimHistory();
      if (event.val.keys.includes('per_project')) entries.length = 0;
      if (event.val.origin === 'reset') log('info', 'settings reset from the Settings screen');
    }
    return [];
  },
  /** Runs before a user or reset save is written. 100 ms, no I/O. */
  settingsValidate(_patch: string, merged: string) {
    const next = JSON.parse(merged) as Config;
    try {
      new RegExp(next.ignore_pattern);
      return { ok: true as const };
    } catch {
      return {
        ok: false as const,
        errors: [{ key: 'ignore_pattern', message: `Not a valid pattern: ${next.ignore_pattern}` }],
      };
    }
  },
});
```

Points to notice:

- The `Config` type has no optional fields, because the get call fills every default in. A missing key in the JSON is a bug in the host, so the parse fails and the old values stay.
- `reload` does not write anything. Reading is free, and the host never asks the plugin to validate a read.
- `enabled` is read by the capture code of the plugin. Clipboard reads that happen while it is `false` are skipped, so the toggle takes effect at once.

#### What you see

The page reads the same values the plugin reads. The Capture section lists its three rows, with the saved values.

<figure>
<pre class="term">╭  ≡ ● · Clipboard history · plugin─────────────────────╮
│<span class="t-dim">⧉ Added by example.clipboard-history 0.1.0             </span>│
│                                                       │
│<span class="t-hi">CAPTURE                                                </span>│
│  Keep history ………………………………………………………………………………… ‹ On ›  │
│<span class="t-dim">    Record text copied in panes and the editor.        </span>│
│<span class="t-dim">  ──────────────────────                               </span>│
│  Store ………………………………………………………………………………… ‹ Text only ›  │
│<span class="t-dim">    Which clipboard content is kept.                   </span>│
│<span class="t-dim">  ──────────────────────                               </span>│
│  Maximum entries …………………………………………………………… -  500  + *  │
│<span class="t-dim">    Oldest entries are removed first.                  </span>│
╰───────────────────────────────────────────────────────╯</pre>
<figcaption>The Capture section with the default values, as the client draws it. The leaders are <code>…</code>, and the dim title line names the plugin and its version.</figcaption>
</figure>

### Step 3: react to settings-changed

`settings-changed` arrives in the next event batch after a save. It is sent for every change to the plugin's own values, including the changes made by `set-json`, so the handler has one path for all of them. The handler in step 2 reloads the config, trims the history when the limit drops, clears it when the project switch moves, and logs a reset.

Points to notice:

- The handler checks the keys it needs, not the whole list. `keys` is a union of every change in the batch, so a single event can carry two changed keys.
- The `origin` tells the plugin why the value changed. A `plugin` change came from its own code, so the plugin should not write it back.
- Number steps are debounced. The host waits 600 ms after the last step, then writes once and sends one event. A dragged number therefore produces one event, not one per click.
- A value with `scope: "project"` changes when the user opens another project. The same event arrives with the new project value.

#### What you see

The number row is stepped with the arrow keys. The value on the screen changes at once. A dim mark shows that the write is still pending.

<figure>
<pre class="term">╭  ≡ ● · Clipboard history · plugin─────────────────────╮
│                                                       │
│<span class="t-hi">CAPTURE                                                </span>│
│  Maximum entries ……………………………………………………… -  510  + * ◌  │
│<span class="t-dim">    Oldest entries are removed first.                  </span>│
╰───────────────────────────────────────────────────────╯</pre>
<figcaption>Number row after one step. The dim <code>◌</code> shows the write is pending. After 600 ms without another step it is written and the mark goes away.</figcaption>
</figure>

The Reset control sits on the section. It asks for confirmation, and the answer sends one merge patch that sets each key of the section to `null`, so the origin of the event is `reset`.

<figure>
<pre class="term">╭  ≡ ● · Reset─────────────────────────────────╮
│ Reset 3 settings of Clipboard history?       │
│                                              │
│                        [ N ]  [ Y ]          │
│ Y/N answer · Enter confirm · Esc cancel      │
╰──────────────────────────────────────────────╯</pre>
<figcaption>The Reset confirmation of the Picker section. Y/N answer, Enter confirms, Esc cancels.</figcaption>
</figure>

### Step 4: rows that hide, apply later or live in a project

`Ignore pattern` has `visible_when` on `hide_secrets`. With the switch on, the row is shown. With the switch off, the row disappears and the stored pattern stays.

<figure>
<pre class="term">╭  ≡ ● · Clipboard history · plugin─────────────────────╮
│<span class="t-dim">⧉ Added by example.clipboard-history 0.1.0             </span>│
│                                                       │
│<span class="t-hi">PICKER                                                 </span>│
│  Hide secrets ………………………………………………………………………………… ‹ On ›  │
│<span class="t-dim">    Skip copies that look like a key or a token.       </span>│
│<span class="t-dim">  ──────────────────────                               </span>│
│  Ignore pattern ………………………… ‹ ^sk-[A-Za-z0-9]{20,}$ ›  │
╰───────────────────────────────────────────────────────╯</pre>
<figcaption>The Picker section. <code>Ignore pattern</code> is shown because <code>Hide secrets</code> is on.</figcaption>
</figure>

<figure>
<pre class="term">╭  ≡ ● · Clipboard history · plugin─────────────────────╮
│<span class="t-dim">⧉ Added by example.clipboard-history 0.1.0             </span>│
│                                                       │
│<span class="t-hi">PICKER                                                 </span>│
│<span class="t-hi">  Hide secrets• …………………………………………………………………………… ‹ Off ›  </span>│
│<span class="t-dim">    Skip copies that look like a key or a token.       </span>│
╰───────────────────────────────────────────────────────╯</pre>
<figcaption><code>Hide secrets</code> turned off. The dim <code>•</code> says the value differs from the default. The row <code>Ignore pattern</code> is hidden: it keeps its value and search skips it.</figcaption>
</figure>

`Sync token` and `Separate per project` come from the third section. The secret shows `••••` and the last four characters. The restart note is part of the description.

<figure>
<pre class="term">╭  ≡ ● · Clipboard history · plugin─────────────────────╮
│                                                       │
│<span class="t-hi">SYNC AND FILES                                         </span>│
│  Separate per project ………………………………………………………… ‹ Off ›  │
│<span class="t-dim">    Keep one history for each project.                 </span>│
│<span class="t-dim">  ──────────────────────                               </span>│
│  Sync token ………………………………………………………………………………… ••••a1b2  │
│<span class="t-dim">    Applies after restart.                             </span>│
╰───────────────────────────────────────────────────────╯</pre>
<figcaption>A <code>scope: "project"</code> row and a secret row with <code>reveal_last</code>. The secret shows <code>••••</code> and its last four characters. The restart note appears under the description.</figcaption>
</figure>

The plugin must not assume that a new `sync_token` is live. It reads the value at activation, so the new token takes effect when the plugin restarts. The marker tells the user the same thing.


### Step 5: validate the ignore pattern

The `text` row `ignore_pattern` holds a regular expression. The schema can check its length, but not whether it compiles. The plugin exports `settings-validate`, which the host calls before it saves a user or reset change. The export is listed in `client.exports`. In Rust it is the method `settings_validate` on the `Handler` impl, and in TypeScript it is the `settingsValidate` member of the `definePlugin` call. Both are in the files of step 2.

Points to notice:

- `merged` is the full value object after the patch, so the check sees the whole config. `patch` holds only what changed.
- The check runs for `user` and `reset` origins only. A `set-json` from the plugin's own code is not checked, because the plugin already knows its rules.
- A check that does not answer within 100 ms refuses the save. The rule is strict on purpose, so keep the function short and free of I/O.
- The field error replaces the row's description with the message, in the warning style, until the next change.

#### What you see

The user types `[a-z` in the Ignore pattern field and leaves the field. The save is refused, and the old value stays in place.

<figure>
<pre class="term">╭  ≡ ● · Clipboard history · plugin─────────────────────╮
│                                                       │
│<span class="t-hi">PICKER                                                 </span>│
│<span class="t-hi">  Ignore pattern ……………………………………………………………………… ‹ [a-z ›  </span>│
│<span class="t-warn">    Not a valid pattern: [a-z                          </span>│
╰───────────────────────────────────────────────────────╯</pre>
<figcaption>The save was refused by <code>settings-validate</code>. The error replaces the description in the warning style, and the old value is kept.</figcaption>
</figure>

### Step 6: find a setting with the search

Plugins need no search code. The user presses `/` or `Ctrl+F` and types part of a name. The kernel matches the tab title, the section titles, the row titles, descriptions and keys, and the `keywords` of the tab. A hidden row is skipped.

<figure>
<pre class="term">⚙ Settings                                                               ✕ Close
<span class="t-dim">/ max                                                           [ Guided setup ]</span>
╭  ≡ ● · Navigation╮ ╭  ≡ ● · Clipboard history · plugin───────────────────────╮
<span class="t-hi">│ ⎘  Clipboard 1   │</span> │                                                         │
│                  │ │<span class="t-hi">  CAPTURE                                                </span>│
│                  │ │  Maximum entries …………………………………………………………… -  500  + *    │
│                  │ │<span class="t-dim">    Oldest entries are removed first.                    </span>│
│                  │ │                                                         │
│                  │ │                                                         │
╰──────────────────╯ ╰─────────────────────────────────────────────────────────╯</pre>
<figcaption>The filter with <code>max</code>. The rail keeps the tab with the match and a count, and the content shows only the matching row under its section title.</figcaption>
</figure>

### Step 7: pack, install and see the tab

Build, pack and install the plugin as in [Tutorial 9](/pages/tutorial-09-package-and-share/). The package is unsigned, so the install needs `--allow-unsigned`.

```bash
ilium plugin build
ilium plugin pack
ilium plugin install dist/example.clipboard-history-0.1.0.iliumpkg --allow-unsigned
ilium plugin logs example.clipboard-history --tail 3
```

The review dialog lists no settings line, because the tab needs no grant. It shows only the clipboard grants of the project. The tab appears in the rail as soon as the plugin is enabled. The plugin starts at `startup`, as the manifest says, so the validation of step 5 is ready before the user opens the page.

A plugin that fails three times in ten minutes is stopped. Its tab then stays in the rail, dimmed, and the page shows the disabled state:

<figure>
<pre class="term">╭  ≡ ● · Clipboard history · plugin · disabled──────────╮
│<span class="t-warn">! Plugin Clipboard history stopped after 3             </span>│
│<span class="t-warn">  failures in 10 minutes: handler trap.                </span>│
│<span class="t-dim">  Values are kept.  [ Open Plugins page ]              </span>│
│  Keep history ………………………………………………………………………………… ‹ On ›  │
│<span class="t-dim">    Read-only while the plugin is disabled.            </span>│
╰───────────────────────────────────────────────────────╯</pre>
<figcaption>The tab after the plugin was disabled. The values are kept, the rows are read-only.</figcaption>
</figure>

<div class="warn">
<p>A plugin that is disabled, or uninstalled, never loses its values. Only <code>ilium plugin uninstall --purge</code> or "Forget settings" on the recovery page deletes them. A reinstall reads the same values again.</p>
</div>

### Test it

Test the manifest first. Change one default on purpose, for example `"default": 0` on `max_entries`, and run `ilium plugin build`. The build reports `manifest.settings-default` with the JSON path of the row. Restore the value afterwards.

Then test the page in the app:

1. Step `Maximum entries` up and down, and check `ilium plugin logs example.clipboard-history --tail 5`. A burst of steps produces one write.
2. Type `[a-z` in Ignore pattern and leave the row. The error must show on the row, and the old value must stay.
3. Turn `Hide secrets` off. The Ignore pattern row must disappear. Turn it on again: the old pattern must come back.
4. Press Reset on the Picker section and answer Y. The log must show the reset line from step 3.
5. Type a token in `Sync token`. The value must show as `••••` and the last four characters, with the marker `↻`.
6. Press `/`, type `max`, and check that the rail and the content narrow to the matching row. Press Esc to clear it.

For unit tests, use the fixture format of the [Testing](/pages/testing/) page. Give each fixture a `settings` object and the events to deliver, and assert that `settings_validate` (Rust) or `settingsValidate` (TypeScript) returns the error on `ignore_pattern` for a bad pattern.

### Troubleshooting

| Code or symptom | Cause | Fix |
|---|---|---|
| `manifest.settings-kind` | `kind` is missing or not `tab` or `section`. | Set `"kind": "tab"`. |
| `manifest.settings-id` | The id has a capital letter or starts with a digit. | Use `[a-z][a-z0-9-]{0,31}`. |
| `manifest.settings-title-clash` | The tab title equals a built-in tab name. | Choose another title. |
| `manifest.settings-key-clash` | Two rows use one key anywhere in the plugin. | Rename one key. |
| `manifest.settings-row-type`, `manifest.settings-row-field` | Misspelled row type, or a field the row does not take. | Fix the name, or remove the field. |
| `manifest.settings-default` | A default fails its own row. | Make the default pass the row. |
| `manifest.settings-command` | A `keybinding` or `button` names a command the plugin does not contribute. | Add the command to `contributes.commands`. |
| `manifest.settings-limit` | More than 24 rows in a section, or more than 8 sections. | Split the section. |
| `manifest.schema-keyword` | A keyword outside the settings subset. | Use only the keywords of the reference. |
| `invalid-argument` on save | A patch breaks a schema rule. The message names the JSON path. | Fix the value. |
| Could not check this value: did not answer | `settings-validate` took over 100 ms, or the instance is stopped. | Keep the check free of I/O. |
| A row is missing | Its `visible_when` is false. | Change the controlling row; the value is kept. |
| Rows dimmed and read-only | The plugin is disabled, or restarting with `settings-validate`. | Turn it on in Settings > Plugins. |
| `settings.value-ignored` warning | A stored value no longer fits its row. | The default applies. Set the value again. |
| Picker key inactive | Another binding of the same chord won. | Settings > Keys lists the conflict. |

### See also

- [Settings extension](/pages/settings-extension/) explains tabs, sections, anchors and ordering.
- [Row gallery](/pages/reference-settings-rows/) shows every row type in every state.
- [Tutorial 11: a section in a built-in tab](/pages/tutorial-11-settings-extend-tab/) adds rows to a built-in tab.
- [Settings schema reference](/pages/reference-settings-schema/) lists every row type, field, limit and error code.
- [Settings API](/pages/api-settings/) and [Manifest reference](/pages/reference-manifest/).
