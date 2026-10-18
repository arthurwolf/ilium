---
layout: page
title: "keys.intercept"
permalink: /pages/capability-keys-intercept/
---

## Overview

The `keys.intercept` grant lets a client-side plugin take chosen key chords before the focused pane sees them. While a grab is active, a key that matches one of its chords arrives at the plugin as a `key-pressed` event, and the pane does not receive that key. A key that matches no grab takes the normal path to the pane.

The grant is high risk (H). A grab that hides keys from a shell or an editor breaks the program the user is typing into, so Ilium never grants it by default. Hold a grab only while its feature is active, and release it as soon as the feature ends.

<div class="warn">

While a grab is active, its chords never reach the pane. A plugin that grabs a common chord, or never releases its grab, can make a terminal or an editor look broken. Grab only the chords a feature needs, release them as soon as the feature ends, and list the chords in the plugin's description.

</div>

## You need

| Need | Value |
|---|---|
| Side | Client only. Declare the grant under `capabilities.client`. A server plugin cannot call `keys`. |
| Grant | `keys.intercept`. The grant takes no argument. |
| Language | Rust, through `ilium_plugin_sdk::host::keys`, or TypeScript, through `@ilium/plugin-sdk/host/keys`. |
| Companion grant | `terminal.pointer` when the same feature also selects text with the mouse. |
| Manifest entry | Optional. A `keybindings` contribution starts the feature from a chord the user can rebind. The user's remap wins over the plugin's default. |

Grant syntax is strict. `keys.intercept` is valid. `keys.intercept:ctrl-c` is refused at build time with `manifest.grant-argument`:

```text
$ ilium plugin build
error  manifest.grant-argument
       capability "keys.intercept" takes no argument
       (got "keys.intercept:ctrl-c")
```

The chords belong to the `grab` call, not to the grant. Chord strings follow the grammar of [keys](/pages/api-keys/): lower case, zero or more of `ctrl`, `alt`, `shift` and `super` joined with `+`, then one key. Examples are `alt+s`, `up` and `shift+tab`.

## Surface

The grant opens two functions and four event fields from the `keys` interface:

| Name | Kind | Rust | TypeScript | Meaning |
|---|---|---|---|---|
| `grab` | function | `keys::grab(&GrabSpec) -> Result<u64, PluginError>` | `grab(spec: GrabSpec): bigint` | Starts a grab and returns its id. TypeScript throws `PluginError`. |
| `release` | function | `keys::release(grab: u64)` | `release(grab: bigint): void` | Ends a grab. An unknown or ended id does nothing. |
| `GrabSpec.chords` | field | `Vec<String>` | `string[]` | One to 32 chords. |
| `GrabSpec.pane` | field | `Option<NodeId>` | `bigint \| undefined` | With a pane, the grab follows that pane's focus. With `none`, it is active whatever has focus. |
| `GrabSpec.until_release` | field | `bool` | `boolean` | The grab ends when the matched key is released. |
| `key-pressed` | event | `Event::KeyPressed(KeyEvent)` | `{ tag: 'key-pressed', val }` | A grabbed chord was pressed. Never dropped. |
| `key-released` | event | `Event::KeyReleased(KeyEvent)` | `{ tag: 'key-released', val }` | A grabbed key was released. Arrives only with release reports. |
| `KeyEvent.grab` | field | `Option<u64>` | `bigint \| undefined` | The grab that matched the key. |
| `KeyEvent.pane` | field | `Option<NodeId>` | `bigint \| undefined` | The focused pane when the key arrived. |
| `KeyEvent.chord` | field | `String` | `string` | The matched chord, as written in the grab. |
| `KeyEvent.repeat` | field | `bool` | `boolean` | `true` for an auto-repeat while the key is held. |

The WIT form, from `ilium:host/keys`:

```wit
// Excerpt of ilium:host/keys in reference-wit
record grab-spec { chords: list<string>, pane: option<node-id>, until-release: bool }
type grab-id = u64;
grab: func(spec: grab-spec) -> result<grab-id, plugin-error>;
release: func(grab: grab-id);
```

Key release reports need the kitty keyboard protocol. The client enables it when the outer terminal supports it. Without it, only `key-pressed` arrives, and an `until-release` grab ends at the next key that does not match.

## How a grab routes keys

<figure>
<svg viewBox="0 0 400 210" role="img" aria-label="A key press goes to the plugin when a grab matches, and to the pane otherwise">
<defs>
<marker id="arr-keys-intercept" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
<path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
</marker>
</defs>
<rect x="110" y="10" width="180" height="30" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
<text x="200" y="29" text-anchor="middle" font-size="11" fill="currentColor">Key press</text>
<line x1="200" y1="40" x2="200" y2="62" stroke="currentColor" stroke-width="1.5" marker-end="url(#arr-keys-intercept)"/>
<rect x="110" y="64" width="180" height="30" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
<text x="200" y="83" text-anchor="middle" font-size="11" fill="currentColor">Matches an active grab?</text>
<line x1="120" y1="94" x2="120" y2="138" stroke="currentColor" stroke-width="1.5" marker-end="url(#arr-keys-intercept)"/>
<text x="128" y="122" font-size="10" fill="currentColor">yes</text>
<line x1="260" y1="94" x2="260" y2="138" stroke="currentColor" stroke-width="1.5" stroke-dasharray="4 3" marker-end="url(#arr-keys-intercept)"/>
<text x="268" y="122" font-size="10" fill="currentColor">no</text>
<rect x="10" y="140" width="150" height="30" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
<text x="85" y="159" text-anchor="middle" font-size="11" fill="currentColor">Plugin: key-pressed</text>
<rect x="230" y="140" width="150" height="30" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
<text x="305" y="159" text-anchor="middle" font-size="11" fill="currentColor">Focused pane</text>
</svg>
<figcaption>A matching chord stops at the plugin and never reaches the pane. A key that no grab matches goes to the pane as usual.</figcaption>
</figure>

A grab with `pane: none` is active on that client whatever has focus, except while a kernel modal dialog is open. A grab with a pane is active only while that pane has focus. It is suspended while another pane has focus and resumes when focus returns. An `until-release` grab ends when focus changes.

A grab ends with no event to the plugin when its pane closes, when its plugin stops, or when its grant is revoked. A revocation also withdraws every grab and sends `deactivate(revoked)`. A later `release` of an ended id does nothing, so the plugin can call it without checking.

## Risk and tier rules

- `keys.intercept` is high risk (H). A T1 bundled plugin is activated first without it. At first activation the user sees one prompt, "Plugin <name> asks for: <list>", with the buttons `Allow selected` and `Not now`. `Not now` asks again at the next server start, at most three times, and then never again. Settings > Plugins can grant it at any time.
- A T2 or T3 plugin shows the review dialog at install. The grant is listed with the H mark and is unchecked by default.
- A grant is a standing permission. Nothing asks the user again on each call.
- Revoke it in Settings > Plugins > <plugin> > Capabilities, or with `ilium plugin grants <id> --revoke keys.intercept`.

## What the user sees

At install of a T2 plugin, the review dialog shows one row for the grant:

<figure>
<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.keys 0.1.0 by Example Ltd                                      │
│                                                                            │
│ <span class="t-hi">Client side</span>                                                                │
│ <span class="t-warn">! [ ] Grabs keys before Ilium sees them</span> <span class="t-dim">…………………………………………………</span> <span class="t-warn">keys.intercept</span> │
│                                                                            │
│ <span class="t-warn">! marks a high-risk grant. It starts unchecked and is</span>                      │
│ <span class="t-warn">always asked for, even for bundled plugins.</span>                                │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>The review dialog for a package that requests keys.intercept, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar. Checked rows are granted; a high-risk row starts unchecked.</figcaption>
</figure>

While a grab holds a chord, pressing that chord does nothing in the pane, and the grab itself shows no indicator. Put the chords in the plugin's description. If the feature needs on-screen help, show the chord in the plugin's own view.

## Worked example 1: hold to select (Rust)

The Smart Copy light feature starts with a command and holds `alt+s` while the user selects output. The command starts from the `alt+g` keybinding. The grab is `until-release`, so it ends when the user lets go of `alt+s`.

Manifest:

```json
{
  "capabilities": { "client": ["keys.intercept", "terminal.pointer"] },
  "contributes": {
    "keybindings": [
      { "command": "com.example.smart-copy/start", "chord": "alt+g", "mode": "global" }
    ]
  }
}
```

Plugin code:

**Rust**

```rust
use ilium_plugin_sdk::host::keys::{self, GrabSpec};
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::prelude::*;

const START_COMMAND: &str = "com.example.smart-copy/start";
const HOLD_CHORD: &str = "alt+s";

#[ilium_plugin(side = "client")]
#[derive(Default)]
struct SmartCopy {
    hold_grab: Option<u64>,
}

impl Lifecycle for SmartCopy {
    fn deactivate(&mut self, _reason: DeactivationReason) -> Option<Vec<u8>> {
        if let Some(grab) = self.hold_grab.take() {
            keys::release(grab);
        }
        None
    }
}

impl Handler for SmartCopy {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            match event {
                Event::CommandInvoked(invoked) if invoked.command == START_COMMAND => {
                    self.arm(invoked.context.pane);
                }
                Event::KeyReleased(key) if key.grab.is_some() && key.grab == self.hold_grab => {
                    // The until-release grab ended with the key. The copy itself is
                    // written from the rows that terminal.pointer highlighted.
                    self.hold_grab = None;
                }
                _ => {}
            }
        }
        Vec::new()
    }
}

impl SmartCopy {
    fn arm(&mut self, pane: Option<NodeId>) {
        let Some(pane) = pane else { return };
        if self.hold_grab.is_some() {
            return;
        }
        let spec = GrabSpec {
            chords: vec![HOLD_CHORD.to_string()],
            pane: Some(pane),
            until_release: true,
        };
        match keys::grab(&spec) {
            Ok(grab) => self.hold_grab = Some(grab),
            Err(error) => {
                log::log(Level::Warn, &format!("hold grab refused: {:?}", error.code));
            }
        }
    }
}
```

`arm` returns at once when the command has no pane or a grab is already held, so only one selection runs at a time. The `KeyReleased` arm matches only the grab that this plugin holds.

## Worked example 2: declined grant and cleanup (TypeScript)

The same feature in TypeScript shows the declined path. A `not-permitted` error means the user declined the grant. The feature logs the reason and leaves the mouse path to work. The plugin also releases its grab when it deactivates.

**TypeScript**

```ts
import { definePlugin, PluginError, type Event } from '@ilium/plugin-sdk';
import * as keys from '@ilium/plugin-sdk/host/keys';
import { log } from '@ilium/plugin-sdk/host/log';

const START_COMMAND = 'com.example.smart-copy/start';
const HOLD_CHORD = 'alt+s';

let holdGrab: bigint | undefined;

function arm(pane: bigint | undefined): void {
  if (pane === undefined || holdGrab !== undefined) {
    return;
  }
  try {
    holdGrab = keys.grab({ chords: [HOLD_CHORD], pane, untilRelease: true });
  } catch (error) {
    if (error instanceof PluginError && error.code === 'not-permitted') {
      log('info', 'keys.intercept not granted: select with the mouse instead');
      return;
    }
    const code = error instanceof PluginError ? error.code : 'unknown';
    log('warn', `hold grab refused: ${code}`);
  }
}

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  activate() {},
  deactivate() {
    if (holdGrab !== undefined) {
      keys.release(holdGrab);
    }
    holdGrab = undefined;
    return undefined;
  },
  onEvents(batch: Event[]) {
    for (const event of batch) {
      if (event.tag === 'command-invoked' && event.val.command === START_COMMAND) {
        arm(event.val.context.pane);
      } else if (event.tag === 'key-released' && event.val.grab !== undefined) {
        if (event.val.grab === holdGrab) {
          holdGrab = undefined;
        }
      }
    }
    return [];
  },
});
```

Every `u64` is a `bigint` in TypeScript, so the grab id and the pane id are `bigint` values. Compare them with `===`, never with a number.

## Edge cases and errors

| Situation | Result |
|---|---|
| Grant declined or revoked | `grab` returns `not-permitted`. The keys go to the pane as usual. |
| Chord outside the grammar, or more than 32 chords | `invalid-argument`. |
| Chord the client cannot report | `invalid-argument`. With the legacy keyboard protocol, `ctrl` with `shift` on a letter, `super`, and `ctrl` on digits and most punctuation cannot be reported. |
| `pane` names a node that is not a pane | `not-found`. |
| Another plugin holds the same chord in the same scope | `conflict`. |
| The per-plugin grab limit (`key_grabs`: 8 by default, 32 at most for tier 1, 8 for tiers 2 and 3), counted per plugin, or the session-wide cap of 32 across all plugins | `quota-exceeded`. |
| Pane closes, plugin stops, or grant revoked | The grab ends. A revocation sends `deactivate(revoked)`. No event reports the end. |
| Focus moves away from a pane grab | The grab is suspended until focus returns. An `until-release` grab ends. |
| Kernel modal dialog open | A `pane: none` grab is inactive until the dialog closes. |
| No release reports | `key-released` never arrives. An `until-release` grab ends at the next key that does not match, and that key goes to the pane. The end sends no event. |
| Key held down | `key-pressed` arrives again with `repeat: true`. Ignore repeats when counting presses. |
| `release` of an unknown or ended id | Nothing happens. |

A plugin must not retry a `grab` that returned `not-permitted`. It should fall back to its mouse path or tell the user that the feature is unavailable.

## See also

- [keys](/pages/api-keys/) for the full `keys` interface, the chord grammar and a copy-mode example.
- [terminal.pointer](/pages/capability-terminal-pointer/) for the mouse half of the Smart Copy feature.
- [terminal.write](/pages/capability-terminal-write/) for the input that a grab takes away from the pane.
- [Capabilities](/pages/capabilities/) for the grant model and the review dialog.
- [Errors and limits](/pages/reference-errors-and-limits/) for the error codes and the grab quotas.

Checked against the API version `ilium:plugin@0.1.0`.
