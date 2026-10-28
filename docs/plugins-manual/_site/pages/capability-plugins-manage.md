# Capability: plugins.manage

## plugins.manage

The `plugins.manage` grant lets a client-side plugin list every installed plugin and turn each one on or off. Only a bundled first-party plugin at tier T1 can hold it, because turning a plugin off stops that plugin's work at once. The guided setup plugin is the intended holder. A plugin that is turned off withdraws all its contributions, cancels its pending requests and stays off until someone turns it on again.

<figure>
<svg viewBox="0 0 400 240" role="img" aria-label="The setup plugin calls set-enabled on the plugin registry, which deactivates the disabled plugin">
  <defs>
    <marker id="arr-plugins-disable" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
      <path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="20" y="10" width="360" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="200" y="35" text-anchor="middle" fill="currentColor" font-size="12">Setup plugin (client, T1, plugins.manage)</text>
  <line x1="200" y1="50" x2="200" y2="82" stroke="currentColor" marker-end="url(#arr-plugins-disable)"/>
  <text x="212" y="70" fill="currentColor" font-size="11">set-enabled(id, false, scope)</text>
  <rect x="20" y="82" width="360" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="200" y="107" text-anchor="middle" fill="currentColor" font-size="12">Plugin registry</text>
  <line x1="200" y1="122" x2="200" y2="154" stroke="currentColor" marker-end="url(#arr-plugins-disable)"/>
  <text x="212" y="142" fill="currentColor" font-size="11">ok, then deactivate(disabled)</text>
  <rect x="20" y="154" width="360" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="200" y="179" text-anchor="middle" fill="currentColor" font-size="12">Disabled plugin (contributions withdrawn)</text>
  <text x="200" y="220" text-anchor="middle" fill="currentColor" font-size="10">Requests cancelled, quotas released, data kept.</text>
</svg>
<figcaption>The setup plugin turns one plugin off through the registry. The call returns once the change is recorded. The disabled plugin then receives its deactivate call and loses its contributions.</figcaption>
</figure>

<div class="warn">
<p>A disabled plugin stops at once: its contributions are withdrawn and its pending requests are cancelled. Grant <code>plugins.manage</code> only to the guided setup plugin, and make the setup plugin ask the user before it calls <code>set-enabled</code>.</p>
</div>

### What you need

| Requirement | Detail |
|---|---|
| Grant | `plugins.manage`, risk H. It always needs an explicit allow from the user. |
| Tier | T1 bundled only. A T2 or T3 package that asks for it is refused at install. |
| Side | `client`. The `plugins` import is part of the `ilium:client/base` world. A server-side plugin cannot call it. |
| Language | Rust with `ilium-plugin-sdk`, or TypeScript with `@ilium/plugin-sdk`. |
| Manifest | The grant under `capabilities.client`, and a command if the plugin offers one. |

The grant takes no argument. The valid and invalid forms are:

| Grant | Valid | Reason |
|---|---|---|
| `plugins.manage` | yes | The only form. |
| `plugins.list` | no | `plugins.list` is the name of the function, not a grant. |
| `plugins.manage:set-enabled` | no | The grant takes no argument. |

A T2 or T3 manifest that requests the grant fails at install with the code `manifest.grant-tier` and the message `capability "plugins.manage" is available to bundled plugins only`.

### The surface

Rust paths start at `ilium_plugin_sdk::host`. TypeScript paths start at `@ilium/plugin-sdk/host`. Both functions are synchronous and return no request id.

| Function | Rust | TypeScript | Returns | Meaning |
|---|---|---|---|---|
| `%list` (`plugins.list`) | `plugins::list() -> Vec<PluginInfo>` | `list()` returns `PluginInfo[]` | `list<plugin-info>` | Every installed plugin, including disabled and failed ones. |
| `set-enabled` | `plugins::set_enabled(plugin: &str, enabled: bool, scope: &str) -> Result<(), PluginError>` | `setEnabled(plugin, enabled, scope)` returns `void` and throws `PluginError` | `result<_, plugin-error>` | Turns the plugin on or off. Returns once the change is recorded. |

The WIT form, as the SDK receives it:

```wit
// Excerpt of ilium:host/plugins in reference-wit
record plugin-info {
  id: plugin-id, name: string, version: string, tier: string, enabled: bool, failed: bool,
  state: package-state, side-states: list<side-state>, scope: string,
  disabled-reason: option<string>, health: health-state,
  publisher: string, publisher-key: option<string>,
  source: string, channel: string, pinned: bool,
  update-available: option<string>, deprecated: bool, yanked: bool
}
%list: func() -> list<plugin-info>;
set-enabled: func(plugin: plugin-id, enabled: bool, scope: string) -> result<_, plugin-error>;
```

The record `plugin-info` has nineteen fields. The first six are `id`, `name`, `version`, `tier`, `enabled` and `failed`, as before. Five fields are new: `state` (package state), `side-states` (a list of `side-state` records), `scope` (`global` in 0.1.0), `disabled-reason` and `health`. Eight more describe distribution: `publisher`, `publisher-key` (the key id, none for T3), `source` (`file`, `url` or `registry`), `channel`, `pinned`, `update-available` (the newest allowed version known from cached registry data, never fetched by this call), `deprecated` and `yanked`. Their types and rules are in [Plugin lifecycle states](/pages/reference-plugin-lifecycle-states/).

The first six fields:

| Field | Type (Rust, TypeScript) | Meaning |
|---|---|---|
| `id` | `String`, `string` | Plugin id, such as `com.example.notes`. |
| `name` | `String`, `string` | Display name from the manifest. |
| `version` | `String`, `string` | Version of the installed package. |
| `tier` | `String`, `string` | `T1` (bundled), `T2` (signed) or `T3` (unsigned local package). |
| `enabled` | `bool`, `boolean` | `true` when the plugin is turned on. |
| `failed` | `bool`, `boolean` | `true` when the host turned the plugin off after repeated failures. |

The `scope` parameter is `global`, `project` or `session`; see [Plugin scope and sessions](/pages/reference-plugin-scope-and-sessions/). A global change propagates to every running session within 2 seconds. Calling `set-enabled` with the state a plugin already has returns `ok` and changes nothing. Calling `set-enabled(id, true)` on a failed plugin also clears `failed` and starts the plugin again.

### What a disable does

1. The call returns `ok` once the host has recorded the change. It does not wait for the plugin to stop.
2. The plugin receives `deactivate(disabled)`. It has 500 ms to finish, or 1 s at the T1 ceiling. If it misses the deadline, the host drops the instance and keeps no restore bytes.
3. Its contributions are withdrawn: decorations, toolbar state, grabs, claims, highlights, filters, watches and routes.
4. Its pending requests are cancelled and its quotas are released.
5. Its `data` and storage stay in place, unless the user chooses to delete them.

A disable never counts as a failure, so it does not count toward the restart and disable rule. Enabling records the change and activates the plugin by its activation rule: a `startup` plugin starts at once, and a lazy plugin starts at its next trigger. The full lifecycle is on the [lifecycle page](/pages/lifecycle/).

### Risk and tier rules

- `plugins.manage` is a high-risk grant (H). On first activation of a T1 plugin, the host shows one prompt, and the grant stays off until the user allows it.
- Only T1 bundled plugins can hold it. A T2 or T3 package cannot receive it, even when the user trusts the publisher.
- The grant is per plugin and per side. It is recorded in the user configuration, not in the project.

A bundled plugin that requests the grant gets the first-start prompt, not the install dialog ([Install and manage plugins](/pages/user-install-and-manage/#first-start-of-a-bundled-plugin)). The high-risk row starts unchecked, as in the canonical review dialog of [Security](/pages/security/#verifying-a-package).

<figure>
<pre class="term"><span class="t-hi">╭─ Settings wants more access ─────────────────╮</span>
│ ilium.core.settings 0.1.0 · bundled (T1)     │
│                                              │
│ <span class="t-dim">Client</span>                                       │
│ [ ] <span class="t-warn">HIGH</span> Turn plugins on and off             │
│     plugins.manage                           │
│                                              │
│ <span class="t-hi"> Esc </span> Not now   <span class="t-hi"> Enter </span> Allow selected       │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The first-start prompt of a bundled plugin that requests plugins.manage. The row starts unchecked.</figcaption>
</figure>

A T2 or T3 package that requests the grant never reaches a dialog. The install stops at the manifest check:

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin install notes-switcher-0.1.0.iliumpkg
<span class="t-err">error</span> manifest.grant-tier
       Cannot install com.example.switcher 0.1.0.
       Capability "plugins.manage" is available to
       bundled plugins only.
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The error for a T2 or T3 package that requests plugins.manage. Nothing is installed.</figcaption>
</figure>

### Errors and edge cases

| Situation | Result |
|---|---|
| The user declined the grant, or it was revoked | `list()` returns an empty list. `set-enabled` returns `not-permitted`. |
| The grant is revoked while a call is in flight | The call returns `revoked`. The plugin receives `deactivate(revoked)` and restarts with the reduced grant set. |
| The plugin id is not installed | `set-enabled` returns `not-found`. |
| The plugin is already in the requested state | `set-enabled` returns `ok` and changes nothing. |
| The plugin provides a service that a consumer calls | The disable does not cascade. Later `services.call` requests from consumers return `not-found`. |
| The user declined the prompt with "Not now" | The prompt returns at the next server start, at most three times. After that, the user grants the grant from Settings > Plugins. |
| The user turns the plugin off in Settings | No notice appears. The footer notice N14 (see [Lifecycle](/pages/lifecycle/#failure-counting-and-restart)) appears only for a plugin the host turned off after failures. |

Universal codes apply as well: any call may return `internal`. A plugin that does not handle `not-permitted` should show the user where to turn the grant on, instead of retrying.

The footer notice for a plugin that the host turned off after repeated failures is notice N14. Its text and render are defined once in [Lifecycle](/pages/lifecycle/#failure-counting-and-restart). The `failed` flag of that plugin is `true`.

### Manifest

The examples below use this manifest for the Settings plugin, which is bundled (T1):

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.settings",
  "name": "Settings",
  "version": "0.1.0",
  "publisher": "Ilium",
  "client": { "entry": "client.wasm", "world": "ilium:client/base@0.1.0", "exports": [] },
  "capabilities": { "client": ["plugins.manage"] },
  "contributes": { "commands": [{ "id": "disable-notes", "title": "Disable notes" }] }
}
```

The command id is global: `ilium.core.settings/disable-notes`.

### Worked example in Rust

This client plugin turns off `com.example.notes` when the user runs the command from the palette. It checks that the plugin is installed first, so the log shows a clear reason when it is not.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::exports::Activation;
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::host::plugins;
use ilium_plugin_sdk::types::events::CommandInvokedEvent;

const TARGET_PLUGIN: &str = "com.example.notes";

#[ilium_plugin(side = "client")]
#[derive(Default)]
struct SetupPlugin;

impl Lifecycle for SetupPlugin {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        log::log(Level::Info, &format!("setup active for {}", context.plugin));
        Ok(())
    }
}

impl Handler for SetupPlugin {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::CommandInvoked(CommandInvokedEvent { command, .. }) = event else {
                continue;
            };
            if command == "ilium.core.settings/disable-notes" {
                disable_target();
            }
        }
        Vec::new()
    }
}

fn disable_target() {
    let installed = plugins::list();
    if !installed.iter().any(|plugin| plugin.id == TARGET_PLUGIN) {
        log::log(Level::Warn, "the notes plugin is not installed");
        return;
    }
    match plugins::set_enabled(TARGET_PLUGIN, false, "global") {
        Ok(()) => log::log(Level::Info, "the notes plugin is turned off"),
        Err(error) => log::log(
            Level::Error,
            &format!("set-enabled failed: {:?}: {}", error.code, error.message),
        ),
    }
}
```

The handler returns no submissions, because the disable is a direct call and not an intent. The SDK runs `set_enabled` synchronously, so the `match` sees the result at once.

### Worked example in TypeScript

The same plugin in TypeScript. `PluginError` carries the `code` and `message` fields of the `plugin-error` record.

**TypeScript**

```ts
import { definePlugin, PluginError } from '@ilium/plugin-sdk';
import type { Event, Submission } from '@ilium/plugin-sdk/types';
import { log } from '@ilium/plugin-sdk/host/log';
import { list, setEnabled } from '@ilium/plugin-sdk/host/plugins';

const TARGET_PLUGIN = 'com.example.notes';

function disableTarget(): void {
  const installed = list();
  if (!installed.some((plugin) => plugin.id === TARGET_PLUGIN)) {
    log('warn', 'the notes plugin is not installed');
    return;
  }
  try {
    setEnabled(TARGET_PLUGIN, false, 'global');
    log('info', 'the notes plugin is turned off');
  } catch (error) {
    if (!(error instanceof PluginError)) {
      throw error;
    }
    log('error', `set-enabled failed: ${error.code}: ${error.message}`);
  }
}

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  activate(context) {
    log('info', `setup active for ${context.plugin}`);
  },
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'command-invoked' && event.val.command === 'ilium.core.settings/disable-notes') {
        disableTarget();
      }
    }
    return [];
  },
});
```

### Who requests it

- The Settings plugin (`ilium.core.settings`) holds `plugins.manage` and calls `set-enabled` for the plugins the user picks.

### See also

- [settings.read-all and settings.write-all](/pages/capability-settings-all/) for the settings of the plugins listed here.
- [Lifecycle](/pages/lifecycle/) for what a disable, a restart and a revoke do to a running plugin.
- [help and plugins](/pages/api-help-and-plugins/) for the full `help` and `plugins` imports.
- [Settings screens](/pages/settings-screens/) for how a plugin adds its own page to Settings.

### Security

Enabling a plugin that policy forces off returns `not-permitted` with reason `policy` ([SEC-189]). See [Admin policy](/pages/reference-admin-policy/).
