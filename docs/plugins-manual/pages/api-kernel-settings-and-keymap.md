---
layout: page
title: kernel settings and keymap reads
permalink: /pages/api-kernel-settings-and-keymap/
---

## kernel settings and keymap reads

Two read-only imports let a plugin look at the configuration the user made in Ilium itself. `kernel-settings` returns the values of one built-in Settings tab, such as Terminal or Keyboard, as a JSON object. `keymap` returns the key bindings that are active now, and the display form of a chord such as `ctrl+shift+k`.

A plugin uses them to adapt. A plugin that draws a hint bar can show the user's real chord for a command instead of a default. A plugin that sizes its panel can read the terminal tab. Reading changes nothing. Only the one plugin that owns a tab may write that tab, with the grant `settings.write-kernel:<tab-id>` (D13, see "Writing a kernel tab"). No plugin writes a binding. Every other plugin links to the setting row.

**You will need:**

- For `kernel-settings`: the grant `settings.read-kernel:<tab-id>`, on the server or the client side. The argument is one kernel tab id such as `terminal`. It is required, and `*` is refused with `manifest.grant-wildcard`. A missing argument is refused with `manifest.grant-argument-missing`. The grant for the `api` tab is high risk (H). See [Capability: settings.read-kernel](/pages/capability-settings-read-kernel/).
- For `keymap`: the grant `keymap.read`, client side only. It takes no argument and has ordinary risk.
- The `open-external` grant for the link to a settings row.

### Interface

```wit
// kernel-settings (S C, grant settings.read-kernel:<tab-id>)
read: func(tab: string) -> result<string, plugin-error>;          // JSON object of that tab's values; secrets as "<redacted>"

// keymap (C, grant keymap.read)
record binding { command: string, chords: list<string>, context: string, is-default: bool }
bindings: func() -> list<binding>;
chord-label: func(chord: string) -> string;                         // the user-visible form, "Ctrl+Shift+K"

// events
//   kernel-settings-changed { tab: string, keys: list<string> }   (grant settings.read-kernel:<tab>; coalesced by tab, keys unioned)
//   keymap-changed                                                   (grant keymap.read; coalesced)
```

The SDK imports are `ilium_plugin_sdk::host::kernel_settings` and `ilium_plugin_sdk::host::keymap` in Rust, and `@ilium/plugin-sdk/host/kernel-settings` and `@ilium/plugin-sdk/host/keymap` in TypeScript.

| Function | Rust | TypeScript | Result |
|---|---|---|---|
| `kernel-settings.read` | `kernel_settings::read(tab)` | `read(tab)` | `Result<String, PluginError>`; `string`, throws `PluginError` |
| `keymap.bindings` | `keymap::bindings()` | `bindings()` | `Vec<Binding>`; `Binding[]` |
| `keymap.chord-label` | `keymap::chord_label(chord)` | `chordLabel(chord)` | `String`; `string` |

All calls are synchronous. They read a snapshot and return at once, so no `request-id` is involved.

| `binding` field | Rust | TypeScript | Meaning |
|---|---|---|---|
| `command` | `String` | `string` | Global command id, such as `example.notes/new-note`, or a kernel command name |
| `chords` | `Vec<String>` | `string[]` | Every chord bound to the command, in the chord syntax of [keys](/pages/api-keys/) |
| `context` | `String` | `string` | The context in which the binding applies |
| `is_default` | `bool` | `isDefault` | `true` when the user has not remapped the binding |

### The kernel tab ids

`read` takes the frozen kebab-case id of a built-in Settings tab. The ids follow the label as it stood on 2026-10-11 and never change when a label changes.

| Id | Label in Settings | Id | Label in Settings |
|---|---|---|---|
| `user-interface` | User Interface | `sound` | Sound |
| `icons` | Icons | `voice-control` | Voice control |
| `agent-monitoring` | Agent Monitoring | `reset-planning` | Reset planning |
| `agent-cost` | Agent Cost | `inference` | Inference |
| `optimization` | Optimization | `llm-instructions` | LLM Instructions |
| `remote-compaction` | Remote compaction | `titles` | Titles |
| `keyboard` | Keyboard | `triggers` | Triggers |
| `terminal` | Terminal | `text-triggers` | Text Triggers |
| `editor` | Editor | `debug` | Debug |
| `session` | Session | `api` | API |
| `git` | Git | `about` | About |
| `kanban-board` | Kanban Board | `setup` | Setup |
| `animations` | Animations | | |

An unknown id fails with `not-found`. The labels are for the reader of this table; the plugin passes the id.

### Key `automatic_ai` of the `inference` tab

The `inference` tab holds the boolean key `automatic_ai`. It is `true` when the user has switched on automatic AI features under Inference and privacy, and `false` otherwise. A plugin that sends screen text to a model, such as the recap plugin ([recap](/pages/core-plugin-recap/)), reads this key first and treats a missing key as `false`.

### What `read` returns

The result is one JSON object of the tab's values. The keys are the kernel's stable configuration keys of that tab. A key that the kernel removes in a later release disappears from the object, so read keys defensively and give each a default. Every secret value, such as an API key, is the string `"<redacted>"` and never the secret. The object has the shape of the tab at the moment of the call, and a later change does not alter an object already returned.

<figure>
<svg viewBox="0 0 420 250" role="img" aria-label="A plugin reads kernel values and bindings and is told about changes">
<title>Reading kernel settings and the keymap</title>
<desc>The user changes a setting or a binding in Settings. The kernel emits kernel-settings-changed or keymap-changed to plugins that hold the grants. The plugin then calls read or bindings and gets a snapshot.</desc>
<defs>
<marker id="kern-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
<path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
</marker>
</defs>
<text x="50" y="16" font-size="12" text-anchor="middle" fill="currentColor">User</text>
<text x="210" y="16" font-size="12" text-anchor="middle" fill="currentColor">Kernel</text>
<text x="370" y="16" font-size="12" text-anchor="middle" fill="currentColor">Plugin</text>
<line x1="50" y1="22" x2="50" y2="242" stroke="currentColor" stroke-dasharray="3 3"/>
<line x1="210" y1="22" x2="210" y2="242" stroke="currentColor" stroke-dasharray="3 3"/>
<line x1="370" y1="22" x2="370" y2="242" stroke="currentColor" stroke-dasharray="3 3"/>
<line x1="50" y1="46" x2="208" y2="46" stroke="currentColor" marker-end="url(#kern-arrow)"/>
<text x="56" y="40" font-size="11" fill="currentColor">edits a Terminal row</text>
<line x1="210" y1="76" x2="368" y2="76" stroke="currentColor" marker-end="url(#kern-arrow)"/>
<text x="216" y="70" font-size="11" fill="currentColor">kernel-settings-changed</text>
<line x1="370" y1="106" x2="212" y2="106" stroke="currentColor" marker-end="url(#kern-arrow)"/>
<text x="240" y="100" font-size="11" fill="currentColor">read("terminal")</text>
<line x1="210" y1="132" x2="368" y2="132" stroke="currentColor" marker-end="url(#kern-arrow)"/>
<text x="216" y="126" font-size="11" fill="currentColor">JSON, secrets redacted</text>
<line x1="50" y1="164" x2="208" y2="164" stroke="currentColor" marker-end="url(#kern-arrow)"/>
<text x="56" y="158" font-size="11" fill="currentColor">remaps a key</text>
<line x1="210" y1="190" x2="368" y2="190" stroke="currentColor" marker-end="url(#kern-arrow)"/>
<text x="216" y="184" font-size="11" fill="currentColor">keymap-changed</text>
<line x1="370" y1="218" x2="212" y2="218" stroke="currentColor" marker-end="url(#kern-arrow)"/>
<text x="250" y="212" font-size="11" fill="currentColor">bindings()</text>
</svg>
<figcaption>Both imports are pull reads. The events only say that something changed, so the plugin reads again to get the new state.</figcaption>
</figure>

### Example 1: read a kernel tab

The plugin reads the Terminal tab at activation and again when the kernel reports a change in it. It needs `settings.read-kernel:terminal`.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "example.hints",
  "capabilities": { "client": ["settings.read-kernel:terminal", "keymap.read"] }
}
```

**Rust**

```rust
use ilium_plugin_sdk::host::kernel_settings;
use ilium_plugin_sdk::host::log::{self, Level};
use serde_json::Value;

fn read_terminal_tab() -> Option<Value> {
    match kernel_settings::read("terminal") {
        Ok(text) => serde_json::from_str(&text).ok(),
        Err(error) => {
            log::log(Level::Warn, &format!("cannot read terminal tab: {}", error.message));
            None
        }
    }
}

fn on_kernel_settings_changed(tab: &str, keys: &[String]) {
    if tab != "terminal" {
        return;
    }
    if let Some(values) = read_terminal_tab() {
        let count = values.as_object().map(|object| object.len()).unwrap_or(0);
        log::log(Level::Info, &format!("terminal tab: {count} keys, changed {keys:?}"));
    }
}
```

**TypeScript**

```ts
import { read } from '@ilium/plugin-sdk/host/kernel-settings';
import { log } from '@ilium/plugin-sdk/host/log';

function readTerminalTab(): Record<string, unknown> | undefined {
  try {
    return JSON.parse(read('terminal')) as Record<string, unknown>;
  } catch (error) {
    log('warn', `cannot read terminal tab: ${String(error)}`);
    return undefined;
  }
}

export function onKernelSettingsChanged(tab: string, keys: string[]): void {
  if (tab !== 'terminal') return;
  const values = readTerminalTab();
  if (values) log('info', `terminal tab: ${Object.keys(values).length} keys, changed ${keys.join(',')}`);
}
```

The `kernel-settings-changed` event carries `tab` and `keys`, coalesced by tab. If several changes arrive before the plugin runs, the keys are joined into one set. The event is delivered only for tabs the plugin holds a grant for.

### Example 2: show the user's chord for a command

**Rust**

```rust
use ilium_plugin_sdk::host::keymap;

/// Returns the first chord of a command in display form, or None when unbound.
fn hint_for(command: &str) -> Option<String> {
    keymap::bindings()
        .into_iter()
        .find(|binding| binding.command == command)
        .and_then(|binding| binding.chords.first().map(|chord| keymap::chord_label(chord)))
}
```

**TypeScript**

```ts
import { bindings, chordLabel } from '@ilium/plugin-sdk/host/keymap';

export function hintFor(command: string): string | undefined {
  const found = bindings().find((binding) => binding.command === command);
  const chord = found?.chords[0];
  return chord === undefined ? undefined : chordLabel(chord);
}
```

Call `hint_for` again when `keymap-changed` arrives, and redraw the hint. `chord-label` gives the form the client shows to the user, such as `Ctrl+Shift+K`, so a hint matches what the user sees in Settings and in the help screens.

### Writing a kernel tab

The core plugin that owns a tab writes it with `kernel-settings.write`. The write goes through the same validation and auto-save path as a row edit in Settings, and the file on disk is the same `config.toml` key, so the kernel and the plugin never disagree. The owner of each of the 25 tabs is listed in [who owns each tab](/pages/reference-config-and-data-migration/#who-owns-each-tab).

```wit
// Excerpt of ilium:host/fs in reference-wit
// kernel-settings (S C, grant settings.write-kernel:<tab-id>, T1 only)
write: func(tab: string, key: string, value: string) -> result<_, plugin-error>;   // value is JSON; one key per call
```

| Function | Rust | TypeScript |
|---|---|---|
| `kernel-settings.write` | `kernel_settings::write(tab, key, json)` | `write(tab, key, json)` |

A write sends `kernel-settings-changed` to every reader of the tab, including the writer. A secret key cannot be written through this call; the owner stores it through the secrets store ([secrets](/pages/reference-secrets/)). Tabs with no owner plugin have no writer: the kernel row is the only editor.

### Link to a setting

A plugin that does not own the tab cannot write it. It links to the row with `ilium://settings/<tab-id>#<key>`, through the `open-external` intent. Ilium opens Settings at that row.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::intents::{ExternalTarget, OpenExternalSpec};

fn open_terminal_setting(key: &str) -> Submission {
    Submission {
        intent: Intent::OpenExternal(OpenExternalSpec {
            target: ExternalTarget::Settings(format!("ilium://settings/terminal#{key}")),
        }),
        correlation: 1,
        dedup_key: None,
    }
}
```

**TypeScript**

```ts
import type { Submission } from '@ilium/plugin-sdk/types';

export function openTerminalSetting(key: string): Submission {
  return {
    intent: { tag: 'open-external', val: { target: { tag: 'settings', val: `ilium://settings/terminal#${key}` } } },
    correlation: 1n,
    dedupKey: undefined,
  };
}
```

Return the submission from `on_events` (or `onEvents`). The intent needs the `open-external` grant. See [intents](/pages/api-intents/) and [Capability: open-external](/pages/capability-open-external/).

### What the user sees

Neither import draws anything. The user sees the plugin adapt, and a log line when the plugin writes one. This capture is hand-composed to show the log format:

<pre class="term">2026-10-12 10:02:11 example.hints info  terminal tab: 31 keys, changed []
2026-10-12 10:03:40 example.hints info  keymap: new-note is Ctrl+Alt+N
2026-10-12 10:04:02 example.hints info  keymap-changed: new-note is Ctrl+Shift+N
2026-10-12 10:04:20 example.hints warn  cannot read git tab: not-permitted
</pre>

### Errors

| Call | Code | Cause |
|---|---|---|
| `kernel-settings.read` | `not-permitted` | No `settings.read-kernel:<tab>` grant for that tab, or the user revoked it |
| `kernel-settings.read` | `not-found` | The tab id is unknown |
| manifest | `manifest.grant-argument-missing` | `settings.read-kernel` without a tab id |
| manifest | `manifest.grant-wildcard` | `settings.read-kernel:*` |
| manifest | `manifest.grant-tier` | `settings.read-kernel:api` in a package that may not hold high-risk grants, or `settings.write-kernel:<tab>` in any package that is not first-party (T1) |
| manifest | `manifest.settings-owner` | A second plugin declares `settings.write-kernel:<tab>` for a tab that already has an owner |
| `kernel-settings.write` | `not-permitted` | No `settings.write-kernel:<tab>` grant for that tab, or the user revoked it |
| `kernel-settings.write` | `not-found` | The tab id or the key is unknown |
| `kernel-settings.write` | `invalid-argument` | The value does not match the key's type or range |

### Limits

| Limit | Value |
|---|---|
| Writes to a kernel setting | only with `settings.write-kernel:<tab-id>`, T1, one holder per tab; none for a binding |
| Secrets in a returned object | always `"<redacted>"` |
| Wildcard tab argument | refused |
| `keymap` side | client only |
| Events | coalesced: one `kernel-settings-changed` per tab, one `keymap-changed` |

### Edge cases

- The read value is a snapshot. Read again after the event; do not cache for long.
- A key can disappear in a release. Treat a missing key as the default, not as an error.
- A chord from `bindings` may be bound to a kernel command and not to a plugin command. Check `command` before showing it.
- The server side has no `keymap`, because keys belong to the client. A server that needs a chord asks its client with [client requests](/pages/api-client-requests/).

### See also

- [Capability: settings.read-kernel](/pages/capability-settings-read-kernel/) for the grant review.
- [keys](/pages/api-keys/) for chord syntax, grabs and the `key-capture` widget.
- [commands](/pages/api-commands/) for command ids and keybinding contributions.
- [settings](/pages/api-settings/) for the plugin's own settings.
- [settings extension](/pages/settings-extension/) for the kernel tab ids and anchors in full.
- [events](/pages/api-events/) for `kernel-settings-changed` and `keymap-changed`.

Checked against `ilium:plugin@0.1.0`.
