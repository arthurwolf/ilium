# Capability: terminal.raw-input

## terminal.raw-input

The `terminal.raw-input` capability lets a plugin send bytes to a terminal exactly as they are, send mouse events that the host encodes for the program, and ask a `terminal-view` widget to be interactive. Ordinary [terminal.write](/pages/capability-terminal-write/) sends text; raw input can send any byte sequence, so it can drive any program that reads the terminal, which is why the grant is high risk (**H**).

**You will need:**

- A `terminal.raw-input` grant (explicit, high risk).
- A pane id and the bytes or mouse events to send.
- Knowledge of the modes of the target pane: read them with [terminal.read-cells](/pages/capability-terminal-read-cells/) if mouse reporting matters.

<figure>
<svg viewBox="0 0 420 252" role="img" aria-label="A raw send-input is checked and written to the pty">
  <defs><marker id="arr" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto"><path d="M0,0 L8,4 L0,8 z" fill="currentColor"/></marker></defs>
  <rect x="20" y="8" width="380" height="44" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="210" y="27" text-anchor="middle" fill="currentColor" font-size="12">Plugin submits send-input with raw bytes or mouse events</text>
  <text x="210" y="43" text-anchor="middle" fill="currentColor" font-size="10">raw at most 64 KiB</text>
  <line x1="210" y1="52" x2="210" y2="72" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="72" width="380" height="44" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="210" y="91" text-anchor="middle" fill="currentColor" font-size="12">Host checks the grant, the stage and size rules and mouse reporting</text>
  <text x="210" y="107" text-anchor="middle" fill="currentColor" font-size="10">mouse needs the program to enable it</text>
  <line x1="210" y1="116" x2="210" y2="136" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="136" width="380" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="210" y="155" text-anchor="middle" fill="currentColor" font-size="12">Host writes the bytes to the PTY</text>
  <text x="210" y="171" text-anchor="middle" fill="currentColor" font-size="10">mouse is encoded in the protocol the program asked for</text>
  <line x1="210" y1="180" x2="210" y2="200" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="200" width="180" height="40" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="110" y="224" text-anchor="middle" fill="currentColor" font-size="11">ok</text>
  <rect x="220" y="200" width="180" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="310" y="224" text-anchor="middle" fill="currentColor" font-size="11">refused(not-permitted or unsupported)</text>
</svg>
<figcaption>Raw bytes go to the PTY unchanged. Mouse events are encoded by the host, so the plugin never writes escape sequences for them.</figcaption>
</figure>

### Grant syntax

| Grant | Valid | Reason |
|---|---|---|
| `terminal.raw-input` | yes | the only form |
| `terminal.raw-input:mouse` | no | no argument; raw bytes and mouse events share one grant |
| `terminal.raw` | no | unknown grant name |

High risk. A bundled T1 plugin still needs an explicit grant. The host does not inspect raw bytes, so the grant is as strong as keyboard access to the pane. Prefer plain text through `terminal.write` whenever the plugin can.

### What it unlocks

| Name | Kind | Meaning |
|---|---|---|
| `send-input.raw` | field | `option<list<u8>>`, written as they are, at most 64 KiB. |
| `send-input.mouse` | field | `option<list<terminal-mouse>>`; `terminal-mouse { kind, button, column, row, modifiers }`. `unsupported` when mouse reporting is off. |
| `terminal-view.interactive` | widget field | `interactive: true` on the `terminal-view` widget forwards the user's input in the view to the pane. |

The widget is described on [Media and embedded widgets](/pages/widgets-media-and-embedded/). The intent is on [Intents](/pages/api-intents/).

### Manifest

The grant goes into `manifest.json` under `capabilities`, in the list of the side that uses it (server or client).

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.rawkeys",
  "capabilities": {
    "server": [
      "panes.read",
      "terminal.write",
      "terminal.raw-input"
    ]
  }
}
```

### Worked example

This plugin sends a mouse click at column 10, row 5 to a pane when the command `com.example.rawkeys/click` runs. If the program has not enabled mouse reporting, the outcome is `refused(unsupported)`.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::Event;
use ilium_plugin_sdk::types::intents::{Intent, Submission};
use ilium_plugin_sdk::types::intents::{SendInputSpec, TerminalMouse};
use ilium_plugin_sdk::types::text::Modifiers;
use ilium_plugin_sdk::types::ui::PointerKind;

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct RawKeys;

impl Handler for RawKeys {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            let Event::CommandInvoked(invoked) = event else { continue };
            if invoked.command != "com.example.rawkeys/click" { continue; }
            let Some(pane) = invoked.context.pane else { continue };
            submissions.push(Submission::keyed(Intent::SendInput(SendInputSpec {
                pane,
                mouse: Some(vec![TerminalMouse {
                    kind: PointerKind::Down, button: PointerButton::Left, column: 10, row: 5, modifiers: Modifiers::empty(),
                }]),
                ..SendInputSpec::default()
            }), format!("rawkeys-click:{pane}")));
        }
        submissions
    }
}
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import type { Event, Submission } from '@ilium/plugin-sdk/types';
export const { lifecycle, handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    for (const event of batch) {
      if (event.tag !== 'command-invoked') continue;
      if (event.val.command !== 'com.example.rawkeys/click' || event.val.context.pane === undefined) continue;
      submissions.push({
        intent: {
          tag: 'send-input',
          val: {
            pane: event.val.context.pane,
            mouse: [{ kind: 'down', button: 'left', column: 10, row: 5, modifiers: [] }],
          },
        },
        correlation: 1n,
        dedupKey: `rawkeys-click:${event.val.context.pane}`,
      });
    }
    return submissions;
  },
});
```

To send bytes instead, set `raw` (Rust `raw: Some(vec![0x1b, b'[', b'A'])`, TypeScript `raw: new Uint8Array([0x1b, 0x5b, 0x41])`). A `send-input` always needs a dedup key, as every `send-input` does. A submission cannot mix `raw` and `text` or `stages` in one `send-input`: that is a shape failure.

### Errors and edge cases

| Situation | Outcome | What the plugin does |
|---|---|---|
| Grant missing or declined | `refused(not-permitted)` | Send plain text through `terminal.write` instead. |
| Mouse events while mouse reporting is off | `refused(unsupported)` | Read `terminal.modes` first. |
| `raw` mixed with `text` or `stages` in one `send-input` | `refused(invalid-argument)` for that submission (a shape failure); every other submission of the batch completes as `refused(cancelled)` | Send the bytes in a separate `send-input`. |
| `raw` above 64 KiB | `refused(invalid-argument)` | Split into several sends. |
| `if-current` precondition fails | `refused(conflict)` | Re-read the pane state and retry. |
| Pane exited | `refused(not-found)` | Drop the pane id. |
| Rate limit | `refused(rate-limited)` | Coalesce events. |

The precondition `if-current` is checked before any byte is written, so a stale click never lands on a changed screen.

### What the user sees

The review dialog lists the grant when a package is installed or first activated. The line is high risk and starts unchecked.

<figure>

<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.rawkeys 0.1.0 by Example Ltd                                   │
│                                                                            │
│ <span class="t-hi">Server side</span>                                                                │
│ <span class="t-warn">! [ ] Sends raw bytes and mouse to terminals</span> <span class="t-dim">…………………………</span> <span class="t-warn">terminal.raw-input</span> │
│                                                                            │
│ <span class="t-warn">! marks a high-risk grant. It starts unchecked and is</span>                      │
│ <span class="t-warn">always asked for, even for bundled plugins.</span>                                │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>

<figcaption>The review dialog for a package that requests terminal.raw-input, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar.</figcaption>
</figure>

Listing the grants of an installed plugin from the terminal:

<figure>

<pre class="term"><span class="t-prompt">$</span> ilium plugin grants com.example.rawkeys
<span class="t-dim">Grant                             Risk     State</span>
terminal.raw-input                high     <span class="t-warn">declined</span>
</pre>

<figcaption>Output of the grant listing for this plugin. A high-risk grant that the user left unchecked shows as declined.</figcaption>
</figure>

The user can change the grant at any time in Settings > Plugins > com.example.rawkeys > Capabilities. A revocation re-activates the plugin with the reduced grant set, and the stubs of the declined imports return `not-permitted`.

### Related pages

- [terminal.write](/pages/capability-terminal-write/).
- [terminal.input-lock](/pages/capability-terminal-input-lock/).
- [Media and embedded widgets](/pages/widgets-media-and-embedded/).
- [Intents](/pages/api-intents/).
