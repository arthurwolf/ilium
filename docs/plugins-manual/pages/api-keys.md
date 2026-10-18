---
layout: page
title: keys
permalink: /pages/api-keys/
---

## keys

The `keys` import lets a client plugin take a key chord for itself. A call that takes a chord is a **grab**. While a grab is active, the keys it names go to the plugin as `key-pressed` events and do not reach the pane underneath. Every other key reaches the pane as usual. A grab ends when the plugin releases it, when its pane closes, when its plugin stops, or, for an `until-release` grab, when the key is released or the next key that does not match arrives.

Use a grab for modal input: a copy mode, a jump mode, or a key that must work while the pane keeps focus. Do not use a grab for ordinary shortcuts. A `keybindings` entry in the manifest is enough for a command that the user triggers from a key.

Three other ways to deal with keys exist next to a grab, and the page covers each in its own section:

- A **panel or basic pane that wants keys** (`wants_keys: true`) receives the keys that no focused widget consumed, without any grant.
- A **key-capture widget** lets the user record a chord into one of the plugin's own settings.
- A **keymap read** shows the user's current bindings. It is read-only and is described in [kernel settings and keymap reads](/pages/api-kernel-settings-and-keymap/).

### You will need

| Need | Value |
|---|---|
| Side | Client only. The `keys` import is in the client world, `ilium:client/base`. A server plugin cannot call it. |
| Grant | `keys.intercept`, high risk. A grab hides keys from the pane the user types into, so the user must allow it in the review dialog. See [Capability: keys.intercept](/pages/capability-keys-intercept/). |
| Rust | Crate `ilium-plugin-sdk`, Rust stable 1.90 or later, target `wasm32-wasip2`. |
| TypeScript | Package `@ilium/plugin-sdk`, Bun 1.2 or later. |
| Chords | Written in the grammar in the next section. The client must be able to report each chord. |

Import the module in each language:

**Rust**

```rust
use ilium_plugin_sdk::host::keys;
```

**TypeScript**

```ts
import * as keys from '@ilium/plugin-sdk/host/keys';
```

### Interface

```wit
// Excerpt of ilium:host/keys in reference-wit
// keys (C, grant keys.intercept)
record grab-spec { chords: list<string>, pane: option<node-id>, until-release: bool }
type grab-id = u64;
grab: func(spec: grab-spec) -> result<grab-id, plugin-error>;
release: func(grab: grab-id);
```

### Chord syntax

A chord is written in lower case: zero or more modifiers, each followed by `+`, then one key. The modifiers are `ctrl`, `alt`, `shift` and `super`, in any order. The grammar is `chord = *(modifier "+") key`.

- A key is one printable character, such as `c` or `1`. Shift plus a letter is written `shift+a`. The plus sign is written `plus`, and the space bar is `space`.
- A named key is one of `enter`, `escape`, `tab`, `backspace`, `delete`, `insert`, `home`, `end`, `page-up`, `page-down`, `up`, `down`, `left`, `right`, or `f1` to `f24`.

Examples: `alt+c`, `up`, `shift+tab`, `ctrl+plus`.

The user-visible form of a chord is different: the client writes `Ctrl+Shift+K`, with capitalised modifiers, and arrow keys as `↑`, `↓`, `←`, `→` and page keys as `Pg↑`, `Pg↓` in its help. A plugin that needs that form calls `keymap.chord-label` (see [kernel settings and keymap reads](/pages/api-kernel-settings-and-keymap/)) instead of formatting the chord itself.

### Surface

| Name | Kind | Rust | TypeScript | Meaning |
|---|---|---|---|---|
| `keys.grab` | function | `grab(spec: &GrabSpec) -> Result<u64, PluginError>` | `grab(spec: GrabSpec): bigint` | Starts a grab and returns its id. TypeScript throws `PluginError`. |
| `keys.release` | function | `release(grab: u64)` | `release(grab: bigint): void` | Ends a grab. It has no result, so an unknown or ended id does nothing. |
| `GrabSpec.chords` | field | `Vec<String>` | `string[]` | The chords to take. One to 32 chords, each in the grammar above. An empty list is `invalid-argument`. |
| `GrabSpec.pane` | field | `Option<NodeId>` | `bigint \| undefined` | With a pane, the grab is active only while that pane has focus. Without one, it is active whatever has focus. |
| `GrabSpec.until_release` | field | `bool` | `boolean` | When `true`, the grab ends when the matched key is released. Without release reports, it ends at the next key that does not match. |
| `key-pressed` | event | `Event::KeyPressed(KeyEvent)` | `{ tag: 'key-pressed', val }` | A key that matches a grab was pressed, or an unconsumed key reached a `wants_keys` pane. Never dropped. |
| `key-released` | event | `Event::KeyReleased(KeyEvent)` | `{ tag: 'key-released', val }` | A grabbed key was released. Arrives only with release reports. |
| `KeyEvent.grab` | field | `Option<u64>` | `bigint \| undefined` | The grab that matched the key. `None` for a key delivered to a `wants_keys` pane. |
| `KeyEvent.pane` | field | `Option<NodeId>` | `bigint \| undefined` | The focused pane when the key arrived. |
| `KeyEvent.chord` | field | `String` | `string` | The matched chord, as written in the grab. |
| `KeyEvent.repeat` | field | `bool` | `boolean` | `true` for an auto-repeat while the key is held. |

Key release reports use the kitty keyboard protocol. The client enables that protocol when the outer terminal supports it. Otherwise only `key-pressed` arrives.

### Example 1: copy mode

The user presses `alt+c`. A keybinding starts the command, and the plugin takes the keys `up`, `down`, `enter` and `escape` while the copy mode runs. `up` and `down` move the selection. `enter` copies the selected rows and ends the mode. `escape` ends it without copying. The grab names a pane, so it is suspended while another pane has focus and resumes when focus returns, as described in the routing rules below.

The manifest declares the keybinding and the grants:

```json
{
  "capabilities": { "client": ["keys.intercept", "clipboard.write"] },
  "contributes": {
    "keybindings": [
      { "command": "com.example.copy-mode/start", "chord": "alt+c", "mode": "global" }
    ]
  }
}
```

**Rust**

```rust
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::host::{clipboard, keys};
use ilium_plugin_sdk::prelude::*;

const START_COMMAND: &str = "com.example.copy-mode/start";
const COPY_CHORDS: [&str; 4] = ["up", "down", "enter", "escape"];

#[ilium_plugin(side = "client")]
#[derive(Default)]
struct CopyMode {
    grab: Option<u64>,
    selected_rows: usize,
    // Filled by the plugin's own code from the pane text. Not an SDK call.
    screen_rows: Vec<String>,
}

impl Lifecycle for CopyMode {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        Ok(())
    }

    fn deactivate(&mut self, _reason: DeactivationReason) -> Option<Vec<u8>> {
        None
    }
}

impl Handler for CopyMode {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            match event {
                Event::CommandInvoked(invoked) if invoked.command == START_COMMAND => {
                    if let Some(pane) = invoked.context.pane {
                        self.start(pane);
                    }
                }
                Event::KeyPressed(key) => self.on_key(key),
                _ => {}
            }
        }
        Vec::new()
    }
}

impl CopyMode {
    fn start(&mut self, pane: NodeId) {
        if self.grab.is_some() {
            return;
        }
        let spec = GrabSpec {
            chords: COPY_CHORDS.iter().map(|chord| chord.to_string()).collect(),
            pane: Some(pane),
            until_release: false,
        };
        match keys::grab(&spec) {
            Ok(grab) => self.grab = Some(grab),
            Err(_) => log::log(Level::Warn, "copy mode not started: the host refused the grab"),
        }
    }

    fn on_key(&mut self, key: KeyEvent) {
        let Some(grab) = self.grab else { return };
        if key.grab != Some(grab) || key.repeat {
            return;
        }
        match key.chord.as_str() {
            "down" => self.selected_rows = (self.selected_rows + 1).min(self.screen_rows.len()),
            "up" => self.selected_rows = self.selected_rows.saturating_sub(1),
            "enter" => {
                self.copy_selection();
                self.end();
            }
            "escape" => self.end(),
            _ => {}
        }
    }

    fn copy_selection(&self) {
        let text = self.screen_rows[..self.selected_rows].join("\n");
        if clipboard::write_text(&text).is_err() {
            log::log(Level::Warn, "copy mode: the clipboard write was refused");
        }
    }

    fn end(&mut self) {
        if let Some(grab) = self.grab.take() {
            keys::release(grab);
        }
        self.selected_rows = 0;
    }
}
```

**TypeScript**

```ts
import { definePlugin, PluginError, type Event, type Submission } from '@ilium/plugin-sdk';
import { writeText } from '@ilium/plugin-sdk/host/clipboard';
import * as keys from '@ilium/plugin-sdk/host/keys';
import { log } from '@ilium/plugin-sdk/host/log';

const START_COMMAND = 'com.example.copy-mode/start';
const COPY_CHORDS = ['up', 'down', 'enter', 'escape'];

let grab: bigint | undefined;
let selectedRows = 0;
// Filled by the plugin's own code from the pane text. Not an SDK call.
let screenRows: string[] = [];

function end(): void {
  if (grab !== undefined) {
    keys.release(grab);
    grab = undefined;
  }
  selectedRows = 0;
}

function onKey(chord: string): void {
  if (chord === 'down') selectedRows = Math.min(selectedRows + 1, screenRows.length);
  else if (chord === 'up') selectedRows = Math.max(0, selectedRows - 1);
  else if (chord === 'enter') {
    try {
      writeText(screenRows.slice(0, selectedRows).join('\n'));
    } catch {
      log('warn', 'copy mode: the clipboard write was refused');
    }
    end();
  } else if (chord === 'escape') end();
}

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  activate() {},
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'command-invoked' && event.val.command === START_COMMAND) {
        const pane = event.val.context.pane;
        if (pane === undefined || grab !== undefined) continue;
        try {
          grab = keys.grab({ chords: COPY_CHORDS, pane, untilRelease: false });
        } catch (error) {
          const code = error instanceof PluginError ? error.code : 'unknown';
          log('warn', `copy mode not started: ${code}`);
        }
      } else if (event.tag === 'key-pressed' && grab !== undefined) {
        if (event.val.grab === grab && !event.val.repeat) onKey(event.val.chord);
      }
    }
    return [];
  },
});
```

The `start` step ignores a second command while a grab is active, so one copy mode runs at a time. A refused grab leaves the plugin idle and writes a warning to its log.

### Example 2: jump mode

The jump mode takes `alt+j`, `alt+k` and `escape` for one pane. The keys move a line counter, and `escape` ends the mode. The grab is not `until-release`: it stays active until the plugin releases it. The command is started by a keybinding that the manifest declares for `alt+g`.

**Rust**

```rust
use ilium_plugin_sdk::host::keys;
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::prelude::*;

#[ilium_plugin(side = "client")]
#[derive(Default)]
struct JumpMode {
    grab: Option<u64>,
    line_index: u32,
}

impl Lifecycle for JumpMode {}

impl Handler for JumpMode {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            match event {
                Event::CommandInvoked(invoked) if invoked.command == "com.example.jump/start" => {
                    if let (None, Some(pane)) = (self.grab, invoked.context.pane) {
                        let spec = GrabSpec {
                            chords: vec!["alt+j".into(), "alt+k".into(), "escape".into()],
                            pane: Some(pane),
                            until_release: false,
                        };
                        match keys::grab(&spec) {
                            Ok(grab) => self.grab = Some(grab),
                            Err(error) => {
                                log::log(Level::Warn, &format!("jump mode: {:?}", error.code))
                            }
                        }
                    }
                }
                Event::KeyPressed(key) if key.grab == self.grab && !key.repeat => {
                    match key.chord.as_str() {
                        "alt+j" => self.line_index += 1,
                        "alt+k" => self.line_index = self.line_index.saturating_sub(1),
                        "escape" => {
                            if let Some(grab) = self.grab.take() {
                                keys::release(grab);
                            }
                            log::log(Level::Info, &format!("jump mode ended at {}", self.line_index));
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        Vec::new()
    }
}
```

**TypeScript**

```ts
import { definePlugin, PluginError, type Event, type Submission } from '@ilium/plugin-sdk';
import * as keys from '@ilium/plugin-sdk/host/keys';
import { log } from '@ilium/plugin-sdk/host/log';

const START_COMMAND = 'com.example.jump/start';
const JUMP_CHORDS = ['alt+j', 'alt+k', 'escape'];

let grab: bigint | undefined;
let lineIndex = 0;

function startJumpMode(pane: bigint): void {
  if (grab !== undefined) {
    return;
  }
  try {
    grab = keys.grab({ chords: JUMP_CHORDS, pane, untilRelease: false });
    log('info', 'jump mode started');
  } catch (error) {
    const code = error instanceof PluginError ? error.code : 'unknown';
    log('warn', `jump mode not started: ${code}`);
  }
}

function endJumpMode(): void {
  if (grab === undefined) {
    return;
  }
  keys.release(grab);
  grab = undefined;
  log('info', `jump mode ended at line ${lineIndex}`);
}

function onJumpKey(chord: string): void {
  if (chord === 'alt+j') {
    lineIndex += 1;
  } else if (chord === 'alt+k') {
    lineIndex = Math.max(0, lineIndex - 1);
  } else if (chord === 'escape') {
    endJumpMode();
  }
}

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  activate() {
    log('info', 'jump plugin ready');
  },
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'command-invoked' && event.val.command === START_COMMAND) {
        const pane = event.val.context.pane;
        if (pane !== undefined) {
          startJumpMode(pane);
        }
      } else if (event.tag === 'key-pressed' && grab !== undefined) {
        if (event.val.grab === grab && !event.val.repeat) {
          onJumpKey(event.val.chord);
        }
      }
    }
    return [];
  },
});
```

The manifest needs `"capabilities": { "client": ["keys.intercept"] }` and a keybinding that maps `alt+g` to `com.example.jump/start` with mode `global`.

<div class="warn">
<p>A grab takes its chords from the pane even when the plugin does nothing with them. Plain letters such as <code>j</code> or <code>k</code> would stop the user typing those letters into the pane. Use modifier chords or named keys in a grab.</p>
</div>

### Routing

<figure>
<svg viewBox="0 0 400 200" role="img" aria-label="Key routing: a matching key goes to the plugin, every other key to the pane">
  <title>Key routing with a grab</title>
  <defs>
    <marker id="keys-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
      <path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="10" y="12" width="120" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="70" y="34" font-size="12" text-anchor="middle" fill="currentColor">Key press</text>
  <line x1="70" y1="48" x2="70" y2="74" stroke="currentColor" marker-end="url(#keys-arrow)"/>
  <rect x="10" y="74" width="120" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="70" y="96" font-size="12" text-anchor="middle" fill="currentColor">Grab matches?</text>
  <line x1="130" y1="92" x2="190" y2="92" stroke="currentColor" marker-end="url(#keys-arrow)"/>
  <text x="160" y="86" font-size="10" text-anchor="middle" fill="currentColor">yes</text>
  <rect x="190" y="74" width="150" height="36" rx="6" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
  <text x="265" y="96" font-size="12" text-anchor="middle" fill="currentColor">Plugin key-pressed</text>
  <line x1="70" y1="110" x2="70" y2="138" stroke="currentColor" marker-end="url(#keys-arrow)"/>
  <text x="76" y="128" font-size="10" fill="currentColor">no</text>
  <rect x="10" y="138" width="120" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="70" y="160" font-size="12" text-anchor="middle" fill="currentColor">Focused pane</text>
  <text x="10" y="194" font-size="11" fill="currentColor">A grab takes only the keys it names.</text>
</svg>
<figcaption>A matching key goes to the plugin and never reaches the pane. Every other key goes to the pane as usual.</figcaption>
</figure>

Rules:

- A grab with no pane is active whatever has focus, except in kernel modal dialogs such as the file picker.
- A grab with a pane is active only while that pane has focus. It is suspended while another pane has focus and resumes when focus returns.
- An `until-release` grab is never suspended. It ends when focus changes, even if its pane is set and regains focus later.
- Without release reports, an `until-release` grab ends at the next key that does not match. That key goes to the pane as usual. The plugin receives no event for the end, so copy or save inside the `key-pressed` handler when the result must not be lost.
- A grab ends when its pane closes or its plugin stops. The plugin gets no event for it. A later `release` call for that id does nothing.
- Revoking `keys.intercept` withdraws every grab of the plugin at once.
- A grab on a chord that another plugin already holds in the same scope returns `conflict`. The kernel's own global chord registry, with its leader key and presets, is not part of the plugin API.

### Keys without a grab: `wants_keys`

A panel or a basic pane that declares `wants_keys: true` in its `panels` entry receives the keys that no focused widget consumed, as `key-pressed` events with `grab` empty. The mode needs no grant, because the keys were not going to another pane: the plugin's own surface has focus. Use it for list navigation in a panel. The `mode_label` of the panel (at most 16 cells) is shown in the status bar while the panel has focus, which is the place to tell the user that a mode is active.

```json
{
  "contributes": {
    "panels": [
      { "id": "outline", "title": "Outline", "placement": "right",
        "wants_keys": true, "mode_label": "OUTLINE" }
    ]
  }
}
```

### Capturing a chord in settings: the `key-capture` widget

A plugin whose setting is a chord does not grab keys. It shows a `key-capture` widget, in a panel or inside a settings view, and the host records the next chord the user presses. Enter starts the capture, the next chord is taken, Escape cancels and Backspace clears. A click on the widget starts the capture too. While capturing, the widget shows `⌨ press keys…`; otherwise it shows the chord in its display form, for example `Ctrl+Shift+K`.

| Prop | Rust | TypeScript | Meaning |
|---|---|---|---|
| `chord` | `.chord(Option<String>)` | `chord` | The current value, in the grab grammar. |
| `allow-modifier-only` | `.allow_modifier_only(bool)` | `allowModifierOnly` | Accepts a chord of modifiers alone. |
| `reserved-check` | `.reserved_check(bool)`, default `true` | `reservedCheck` | Looks the chord up in the keymap. A chord bound by the kernel shows `! used by <command>` and the widget takes the `warning` colour. |

The widget sends `changed` with the chord as text, and `closed` when the user cancels. The check against the keymap needs no grant, because the widget compares inside the host; a plugin that wants to list the bindings itself needs `keymap.read`. Only one chord is captured per widget. The global chord registry stays in the kernel: capturing a chord stores it in the plugin's own setting, and the plugin binds it through `keybindings` or its own grab. It never writes the kernel keymap.

**Rust**

```rust
// types::ui::ActionKind, ActionValue
let chord = tree.key_capture().chord(self.chord.clone()).reserved_check(true).key("chord");

// in on_events
if let Event::UiAction(UiAction { key, kind: ActionKind::Changed, value: ActionValue::Text(text), .. }) = event {
    if key == "chord" {
        self.chord = Some(text);
    }
}
```

**TypeScript**

```ts
import { keyCapture } from '@ilium/plugin-sdk/ui/build';

const chord = keyCapture({ key: 'chord', chord: currentChord, reservedCheck: true });

// in onEvents
if (event.tag === 'ui-action' && event.val.key === 'chord' && event.val.kind === 'changed'
    && event.val.value.tag === 'text') {
  currentChord = event.val.value.val;
}
```

<figure>
<pre class="term">╭  ≡ ● · Quick jump─────────────────────────────────────────────╮
│ Jump to line ……………………………………………………………………… <span class="t-hi">⌨ press keys…</span>        │
│ Copy mode ………………………………………………………………………… Alt+C                  │
│ Close panel ……………………………………………………………………… Ctrl+Shift+K          │
│   <span class="t-warn">! used by Close pane</span>                                        │
│                                                               │
│ <span class="t-dim">Enter capture · Esc cancel · Backspace clear</span>                  │
╰───────────────────────────────────────────────────────────────╯
</pre>
<figcaption>A settings panel with three <code>key-capture</code> rows. The border, title chrome and the grey <code>…</code> leaders are what the client draws; the strings <code>⌨ press keys…</code> and <code>! used by &lt;command&gt;</code> are those the widget specification names. The figure is a composed illustration, not a screen capture.</figcaption>
</figure>

### Errors and limits

| Code | When it happens on `keys.grab` |
|---|---|
| `not-permitted` | The plugin lacks `keys.intercept`, or the user declined it. |
| `invalid-argument` | A chord breaks the grammar, the list is empty or has more than 32 chords, or the client cannot report the chord. |
| `not-found` | The `pane` does not exist. |
| `conflict` | Another plugin already holds that chord in the same scope. Choose another chord or wait. |
| `quota-exceeded` | The plugin holds its maximum of grabs. The limit `key_grabs` is 8 per plugin by default and 32 at most, and `session_key_grabs` caps all plugins together at 32. Release a grab, then retry. |
| `revoked` | The grant was revoked while the call was in flight. |

| Limit | Value |
|---|---|
| Chords per grab | 32 |
| Grabs per plugin (`key_grabs`) | 8 by default, 32 at most |
| Grabs per session, all plugins (`session_key_grabs`) | 32 |
| Chords per `key-capture` widget | 1 |
| `mode_label` | 16 cells |

A chord the client cannot report depends on the keyboard protocol in use. With the legacy protocol, the chords `ctrl+shift` on a letter, `super` and `ctrl` on digits or most punctuation cannot be reported, and `keys.grab` returns `invalid-argument`. With the kitty protocol, every chord of the grammar can be reported. A plugin that must run on both should use chords such as `alt+c` or named keys, which both protocols report.

`keys.release` has no result, so it never fails. Releasing an id that has already ended does nothing.

### What the user sees

When the plugin asks for `keys.intercept`, the review dialog lists the grant with its high risk. If the user declines it, every call returns `not-permitted`. A grab has no visual indicator of its own, so a plugin should show its mode in the status line. The `key-hints` widget in [ui](/pages/api-ui/) draws keys with labels in the status line, and a hint with an empty command is a label only. Hints in the client are separated by `·`, as in `Esc cancel · Enter apply`.

<pre class="term"><span class="t-prompt">$</span> ilium plugin logs com.example.copy-mode
<span class="t-dim">2026-10-12 09:14:02</span> info  copy mode started
<span class="t-dim">2026-10-12 09:14:03</span> key   down: 2 rows selected
<span class="t-dim">2026-10-12 09:14:04</span> key   down: 3 rows selected
<span class="t-dim">2026-10-12 09:14:05</span> info  3 rows copied, mode ended
</pre>

The log lines are written by the plugin; the text is an illustration of the form. After the last line the grab is released and the arrow keys reach the shell in the pane again.

### See also

- [kernel settings and keymap reads](/pages/api-kernel-settings-and-keymap/) for `keymap.bindings` and `chord-label`.
- [primitives terminal](/pages/primitives-terminal/) for pointer claims and highlights, which a plugin often shows during a grab.
- [ui](/pages/api-ui/) for the `key-hints` and `key-capture` widgets and for widgets that take focus.
- [events](/pages/api-events/) for the event envelope that carries `key-pressed` and `key-released`.
- [commands](/pages/api-commands/) for the commands that keybindings start.
- [clipboard](/pages/api-clipboard/) for `write-text`, which copy modes use.
- [capability keys.intercept](/pages/capability-keys-intercept/) for the review dialog and the grant rules.
- [errors and limits](/pages/reference-errors-and-limits/) for the full error codes and quota table.
- [manifest reference](/pages/reference-manifest/) for the `keybindings` and `panels` contributions and the chord grammar.

Checked against `ilium:plugin@0.1.0`.
