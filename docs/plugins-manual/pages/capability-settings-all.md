---
layout: page
title: "Capability: settings.read-all and settings.write-all"
permalink: /pages/capability-settings-all/
---

## settings.read-all and settings.write-all

The `settings.read-all` and `settings.write-all` capabilities let a plugin read and change the settings of other plugins. They exist for one purpose: the settings screen itself. Only a bundled first-party (T1) plugin can hold either grant. A T2 or T3 package that asks for one fails the manifest check, so a third-party plugin never reaches the user with these grants.

The two grants unlock three functions of the `settings` import: `list-settings` lists the tabs and sections that every installed plugin declares, `get-plugin-json` reads all values of one plugin, and `set-plugin-json` changes some of them. A plugin's own values stay under `get-json` and `set-json`, which need no grant.

### settings.contribute:<tab-id>, a different grant

`settings.contribute:<tab-id>` is a separate, normal-risk grant. A plugin needs it to add a section of rows to one built-in tab, for example `settings.contribute:terminal`. It is the only grant that lets a plugin draw anything in a built-in tab, and it allows nothing else:

| Grant | Risk | Argument | Allows | Does not allow |
|---|---|---|---|---|
| `settings.read-all` | high, T1 only | none | Reading the values of other plugins and the list of tabs and sections | Changing any value |
| `settings.write-all` | high, T1 only | none | Changing the values of other plugins (needs `settings.read-all`) | Reading kernel values |
| `settings.contribute:<tab-id>` | low, all tiers | one built-in tab id, required | Drawing sections in that one built-in tab | Reading or writing any value, including kernel values |

The two groups are independent. `settings.read-all` and `settings.write-all` never imply `settings.contribute:<tab-id>`, and `settings.contribute:<tab-id>` never implies either of them. A plugin's own tabs need no grant at all, because the always-granted `settings` import covers their values. The review dialog lists each contribute grant as `Adds settings to the Terminal tab`. The argument must be one built-in tab id. A wildcard is refused, and an unknown or closed tab id fails at install (`manifest.settings-target-unknown`, `manifest.settings-target-closed`). See [Plugin settings extension](/pages/settings-extension/) for the tab ids.

<div class="note">
<p>See also: <a href="/pages/settings-extension/">Plugin settings extension</a> for how sections are placed in a tab.</p>
</div>

<figure>
<svg viewBox="0 0 320 200" role="img" aria-label="The settings screen reads and writes other plugins' values through the kernel store">
  <defs><marker id="arr-settings-all" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto"><path d="M0,0 L10,5 L0,10 z" fill="currentColor"/></marker></defs>
  <rect x="8" y="60" width="100" height="80" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="58" y="96" text-anchor="middle" fill="currentColor" font-size="11">Settings screen</text>
  <text x="58" y="112" text-anchor="middle" fill="currentColor" font-size="10">T1 plugin</text>
  <rect x="212" y="60" width="100" height="80" rx="6" fill="none" stroke="currentColor"/>
  <text x="262" y="96" text-anchor="middle" fill="currentColor" font-size="11">Other plugins'</text>
  <text x="262" y="112" text-anchor="middle" fill="currentColor" font-size="11">settings</text>
  <line x1="212" y1="84" x2="108" y2="84" stroke="currentColor" marker-end="url(#arr-settings-all)"/>
  <text x="160" y="78" text-anchor="middle" fill="currentColor" font-size="10">get-plugin-json</text>
  <line x1="108" y1="118" x2="212" y2="118" stroke="currentColor" marker-end="url(#arr-settings-all)"/>
  <text x="160" y="112" text-anchor="middle" fill="currentColor" font-size="10">set-plugin-json</text>
  <text x="160" y="170" text-anchor="middle" fill="currentColor" font-size="11">list-settings: the settings pages of every plugin</text>
</svg>
<figcaption>The settings screen reads values with get-plugin-json and writes them with set-plugin-json. The kernel store, with its validation and recovery page, stays in the kernel.</figcaption>
</figure>

<div class="warn">
<p>Writing another plugin's settings can change how that plugin behaves, including settings that control network hosts or agent behaviour. Keep <code>settings.write-all</code> to the settings screen and the guided setup plugin.</p>
</div>

<div class="note">
<p>The settings store, its validation and the kernel recovery page stay in the kernel. A plugin that uses these grants does not replace them.</p>
</div>

**You will need:**

- `settings.read-all` for `list-settings` and `get-plugin-json`. A plugin that only reads settings needs this grant alone.
- `settings.write-all` for `set-plugin-json`. It also needs `settings.read-all`. Without it, the install fails with `manifest.grant-dependency`.
- A bundled T1 plugin. Both grants are high risk (H) and are declared under `capabilities.client`, the side of the settings screen.
- A language: Rust with `ilium-plugin-sdk`, or TypeScript with `@ilium/plugin-sdk`.
- Nothing else. Neither grant takes an argument.

### Grant syntax

| Grant | Valid | Result when used |
|---|---|---|
| `settings.read-all` | yes | T1 only |
| `settings.write-all` | yes | T1 only, and only together with `settings.read-all` |
| `settings.read` | no | `manifest.grant-unknown`: the grant name is `settings.read-all` |
| `settings.write-all:com.example.notes` | no | `manifest.grant-argument`: the grant takes no argument |
| `settings.all` | no | `manifest.grant-unknown` |

### Surface

| Function | Rust | TypeScript | Grant | Returns | Meaning |
|---|---|---|---|---|---|
| `list-settings` | `settings::list_settings()` | `listSettings()` | `settings.read-all` | `Vec<SettingsRef>` / `SettingsRef[]` | The tabs and sections that installed plugins declare. Plugins without settings are not listed. |
| `get-plugin-json` | `settings::get_plugin_json(plugin)` | `getPluginJson(plugin)` | `settings.read-all` | `Result<String, PluginError>` / `string` | All values of one plugin as one JSON object, for every scope. |
| `set-plugin-json` | `settings::set_plugin_json(plugin, patch)` | `setPluginJson(plugin, patch)` | `settings.write-all` | `Result<(), PluginError>` / `void` | Applies a merge patch to one plugin's values. |

The Rust module is `ilium_plugin_sdk::host::settings`. The TypeScript import is `@ilium/plugin-sdk/host/settings`. The plugin id is a string. The `SettingsRef` record has these fields:

| Field | Type | Meaning |
|---|---|---|
| `plugin` | plugin id | The plugin that declares the tab or section. |
| `tab` | string | The global tab id `<plugin-id>/<tab-id>`, for example `com.example.notes/notes`, or the built-in tab id for a section in a built-in tab. |
| `section` | string, optional | The global section id, when the entry is a section. |
| `title` | string | The title the settings screen shows. |
| `side` | `plugin-side` | `server` or `client`, the side that owns the values. |
| `kernel-tab` | string, optional | The built-in tab that holds the section, for example `terminal`. |

The WIT form, with the grant of each function written above it:

```wit
// Excerpt of ilium:host/settings in reference-wit
// settings.read-all
record settings-ref {
  plugin: plugin-id, tab: string, section: option<string>,
  title: string, side: plugin-side, kernel-tab: option<string>
}
list-settings: func() -> list<settings-ref>;
get-plugin-json: func(plugin: plugin-id) -> result<string, plugin-error>;

// settings.write-all
set-plugin-json: func(plugin: plugin-id, patch: string) -> result<_, plugin-error>;
```

### How a patch is applied

`set-plugin-json` reads its `patch` as an RFC 7396 JSON merge patch, the same rule as `set-json` for a plugin's own values:

- The patch must be one JSON object of at most 256 KiB. Anything else is `invalid-argument`.
- The patch is validated against the target plugin's schema before anything is stored.
- A key that is not in the schema is `invalid-argument`, and the message names that key.
- The patch is all or nothing. One invalid key stores nothing.
- A key set to `null` resets that key to its schema `default`.
- A patch that changes no stored value succeeds and sends no event.

When a patch changes values, the target plugin receives `settings-changed` with the changed keys, on the side that owns them. The writer gets no event of its own.

`get-plugin-json` returns the keys of every scope, so a settings screen needs one call per plugin. A plugin without settings returns `{}`. A `secret` field comes back as the empty string to every plugin except its owner, so the screen can show a masked field without seeing the value.

### Risk and tier rules

- Both grants are high risk (H). They always need an explicit grant from the user.
- A T2 or T3 package that declares either grant fails with `manifest.grant-tier`. The install review dialog never lists them.
- A T1 plugin is activated first without the grants. The user then answers the prompt described below.

<figure>

<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.settings 0.1.0 by Example Ltd                                  │
│                                                                            │
│ <span class="t-hi">Client side</span>                                                                │
│ <span class="t-warn">! [ ] Reads all plugin settings</span> <span class="t-dim">………………………………………………………………</span> <span class="t-warn">settings.read-all</span> │
│                                                                            │
│ <span class="t-warn">! marks a high-risk grant. It starts unchecked and is</span>                      │
│ <span class="t-warn">always asked for, even for bundled plugins.</span>                                │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>

<figcaption>The review dialog for a package that requests settings.read-all, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar. Checked rows are granted; a high-risk row starts unchecked.</figcaption>
</figure>

### Minimal manifest

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.settings",
  "capabilities": {
    "client": ["settings.read-all", "settings.write-all", "help.read"]
  }
}
```

### Rust example

This settings screen logs the tabs of every plugin when it activates, and sets one value of another plugin when it receives a command. A declined grant is logged and not raised: an error from `activate` counts as a crash.

**Rust**

```rust
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::host::settings;
use ilium_plugin_sdk::prelude::*;

const NOTES_PLUGIN: &str = "com.example.notes";

#[ilium_plugin(side = "client")]
#[derive(Default)]
struct SettingsScreen;

impl Lifecycle for SettingsScreen {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        for entry in settings::list_settings() {
            log::log(Level::Info, &format!("settings tab {} ({})", entry.tab, entry.side));
        }
        match settings::get_plugin_json(NOTES_PLUGIN) {
            Ok(values) => log::log(Level::Info, &format!("notes values: {values}")),
            Err(error) => log::log(Level::Warn, &format!("cannot read notes: {}", error.message)),
        }
        Ok(())
    }
}

impl Handler for SettingsScreen {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            if let Event::SettingsChanged(change) = event {
                log::log(Level::Debug, &format!("own settings changed: {}", change.keys.join(", ")));
            }
        }
        Vec::new()
    }
}

/// Sets the sync interval of the notes plugin and returns a message on refusal.
fn set_sync_interval(seconds: u32) -> Result<(), String> {
    let patch = format!("{{\"sync_interval_seconds\": {seconds}}}");
    match settings::set_plugin_json(NOTES_PLUGIN, &patch) {
        Ok(()) => Ok(()),
        Err(error) if matches!(error.code, ErrorCode::NotPermitted) => {
            Err("settings.write-all is not granted".into())
        }
        Err(error) => Err(error.message),
    }
}
```

### TypeScript example

The same plugin in TypeScript. The entry module exports only `lifecycle` and `handler`, as the manifest lists them.

**TypeScript**

```ts
import { definePlugin, PluginError, type Event, type Submission } from '@ilium/plugin-sdk';
import { log } from '@ilium/plugin-sdk/host/log';
import { getPluginJson, listSettings, setPluginJson } from '@ilium/plugin-sdk/host/settings';

const NOTES_PLUGIN = 'com.example.notes';
const SET_INTERVAL_COMMAND = 'ilium.settings/set-sync-interval';

/** Returns undefined on success, or a message that the settings screen can show. */
function setSyncInterval(seconds: number): string | undefined {
  try {
    setPluginJson(NOTES_PLUGIN, JSON.stringify({ sync_interval_seconds: seconds }));
    return undefined;
  } catch (error) {
    if (!(error instanceof PluginError)) throw error;
    return `${error.code}: ${error.message}`;
  }
}

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  activate() {
    for (const entry of listSettings()) {
      log('info', `settings tab ${entry.tab} (${entry.side})`);
    }
    try {
      log('info', `notes values: ${getPluginJson(NOTES_PLUGIN)}`);
    } catch (error) {
      if (!(error instanceof PluginError)) throw error;
      log('warn', `cannot read notes: ${error.message}`);
    }
  },
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'settings-changed') {
        log('debug', `own settings changed: ${event.val.keys.join(', ')}`);
      }
      if (event.tag === 'command-invoked' && event.val.command === SET_INTERVAL_COMMAND) {
        const refusal = setSyncInterval(300);
        if (refusal) log('warn', refusal);
      }
    }
    return [];
  },
});
```

### Errors

Every call can return `not-permitted`, `revoked` and `internal`. The grant-specific cases are:

| Situation | Code | Effect |
|---|---|---|
| The grant is not held, or the user declined it | `not-permitted` | Every call fails. Retrying does not help. |
| The grant is revoked during a call | `revoked` | The plugin is re-activated with the reduced grant set. This is not counted as a failure. |
| The plugin id is not installed | `not-found` | `get-plugin-json` and `set-plugin-json` fail. |
| The patch is not one JSON object, is over 256 KiB, has a key outside the schema, or breaks a rule of the schema | `invalid-argument` | Nothing is stored. The message names the key. |
| The target plugin has no settings | none | `get-plugin-json` returns `{}`. A non-empty patch is `invalid-argument`. |

The manifest checks that run at install or build time, with their exact text:

| Code | Text |
|---|---|
| `manifest.grant-tier` | `capability "settings.read-all" is available to bundled plugins only` |
| `manifest.grant-dependency` | `capability "settings.write-all" also needs "settings.read-all"` |
| `manifest.grant-argument` | `capability "settings.write-all" takes no argument (got "settings.write-all:x")` |
| `manifest.settings-target-grant` | `settings section "<id>" in tab "<tab>" needs capability "settings.contribute:<tab>"` |
| `manifest.settings-target-unknown` | `settings section "<id>" targets unknown tab "<tab>"` |
| `manifest.settings-target-closed` | `settings tab "<tab>" does not accept plugin sections` |

<figure>

<pre class="term"><span class="t-prompt">$</span> ilium plugin build
<span class="t-err">error</span> manifest.grant-dependency
     capability "settings.write-all" also needs "settings.read-all"
</pre>

<figcaption>Illustrative, not generated. A manifest that asks for write without read fails the build.</figcaption>
</figure>

### Who requests it

First-party plugins from [feature traceability](/pages/reference-feature-traceability/):

- Feature 89, guided setup: `settings.read-all`, `settings.write-all` and `plugins.manage`.
- Feature 90, settings UI: `settings.read-all`, `settings.write-all` and `help.read`. The settings store, its validation and the recovery page stay in the kernel.

### See also

- [settings](/pages/api-settings/) for the plugin's own values, `settings-changed` and the schema.
- [plugins.manage](/pages/capability-plugins-manage/) for enabling and disabling plugins.
- [help.read](/pages/capability-help-read/) for the help topics that the settings screen shows.
- [Settings screens](/pages/settings-screens/) for the row keywords and custom settings pages.
- [Plugin settings extension](/pages/settings-extension/) for tabs, sections and the built-in tab ids.
- [Errors and limits](/pages/reference-errors-and-limits/) for the full error code table.
- [Rust SDK](/pages/rust-sdk/) and [TypeScript SDK](/pages/typescript-sdk/) for the binding rules.

Checked against `ilium:plugin@0.1.0` (API draft of 2026-10-11).

### Security

Reading all settings returns an empty string for a plugin secret, never its value ([SEC-244]); see [Secrets](/pages/reference-secrets/).
