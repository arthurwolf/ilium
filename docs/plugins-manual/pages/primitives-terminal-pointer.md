---
layout: page
title: Terminal pointer
permalink: /pages/primitives-terminal-pointer/
---

## Terminal pointer

A plugin can take the mouse of one terminal pane to implement selection helpers, link clicks, drag-and-drop of files onto a prompt, or a modifier gesture such as Ctrl+click to open. The pointer has a common model for terminal panes, plugin views and editors: whole-cell coordinates plus a position inside the cell, modifier keys, the click count and a `move` kind for hover. The kernel keeps the mouse protocol encodings (X10, SGR, UTF-8) and the auto-scroll at the edges.

### You will need

| Grant | Unlocks |
|---|---|
| `terminal.pointer` | `claim-pointer`, `claim-pointer-gesture`, `release-pointer`, highlights, the `pointer` event of a claimed pane |

See [terminal.pointer](/pages/capability-terminal-pointer/). The calls are synchronous. The base claim and highlight rules are in [Terminal primitive](/pages/primitives-terminal/).

### Signatures

```wit
claim-pointer: func(pane: node-id) -> result<_, plugin-error>;
claim-pointer-gesture: func(pane: node-id, modifiers: pointer-modifiers) -> result<_, plugin-error>;
release-pointer: func(pane: node-id);

enum pointer-kind { down, up, move, drag, wheel-up, wheel-down, wheel-left, wheel-right }
enum pointer-button { none, left, middle, right }
flags pointer-modifiers { shift, control, alt }
record view-pointer {
  kind: pointer-kind, button: pointer-button, modifiers: pointer-modifiers,
  column: u16, row: u16,    // cell inside the area, 0-based
  sub-x: u8, sub-y: u8,     // 0..255 inside the cell; 128 when the terminal reports cells only
  clicks: u8,               // 1, 2 or 3 for down events; 0 otherwise
}
```

The terminal `pointer` event carries the same fields: `node`, `row`, `column`, `kind`, `button`, plus the shape fields `sub-x`, `sub-y` and `modifiers`.

| Rust (`primitives::terminal`) | TypeScript | Meaning |
|---|---|---|
| `claim_pointer(pane)` | `claimPointer` | Take every mouse event of the pane. |
| `claim_pointer_gesture(pane, modifiers)` | `claimPointerGesture` | Take the mouse only while those modifiers are held. |
| `release_pointer(pane)` | `releasePointer` | Give it back. |

### The pointer model

A pointer event has a kind and a button. `down` and `up` mark a press and a release. `drag` repeats while a button is held and the pointer moves. `move` is new and reports a pointer with no button held; it is sent only while a claim is held and the pointer is over the pane. The four wheel kinds replace the older single `wheel` kind in `view-pointer`.

The position is `column` and `row` of the cell, counted from 0 in the area of the widget or pane, and `sub-x`, `sub-y` for the place inside the cell, from 0 at the left or top edge to 255 at the right or bottom edge. Terminals that report only whole cells give 128 for both. A drawing plugin can use the sub-cell position to choose a half block or a Braille dot. `clicks` is 1, 2 or 3 for a `down` event, and is counted by the host with the user's double-click interval.

<figure>
<svg viewBox="0 0 420 150" role="img" aria-label="Pointer position: cell coordinates plus a position inside the cell">
  <rect x="20" y="20" width="240" height="110" rx="8" fill="none" stroke="currentColor"/>
  <g stroke="currentColor" opacity="0.4">
    <path d="M60 20 V130 M100 20 V130 M140 20 V130 M180 20 V130 M220 20 V130"/>
    <path d="M20 47 H260 M20 75 H260 M20 102 H260"/>
  </g>
  <rect x="140" y="47" width="40" height="28" fill="none" stroke="currentColor" stroke-width="2"/>
  <circle cx="165" cy="55" r="3" fill="currentColor"/>
  <text x="270" y="50" fill="currentColor" font-size="12">column 3, row 1</text>
  <text x="270" y="70" fill="currentColor" font-size="12">sub-x 160, sub-y 60</text>
  <text x="270" y="90" fill="currentColor" font-size="12">kind move, button none</text>
</svg>
<figcaption>One cell is highlighted. The dot is the pointer; sub-x and sub-y locate it inside the cell.</figcaption>
</figure>

### Claims, gestures and drag-over

A whole claim stops the host from forwarding the mouse to the program and switches off its own selection and wheel scrollback on that pane. A gesture claim is gentler. It applies only while the listed modifiers are held when the button goes down. A plain click still reaches the program, so an editor in the pane keeps working, and Ctrl+click goes to the plugin. When a gesture claim is held, the plugin gets events only for presses that match, and for the drags and the release that follow them.

Only one plugin holds the claim of a pane at a time. If another plugin claims a pane that already has a whole claim, the call returns `conflict`. Two plugins may hold gesture claims with different modifier sets on the same pane. The same modifier set is `conflict`. Overlap is defined per pane: a whole claim overlaps every other claim on that pane, from the same or another plugin, and two gesture claims overlap only when their modifier sets are equal. So a gesture claim from another plugin on a wholly claimed pane returns `conflict`, a whole claim on a pane that holds any gesture claim returns `conflict`, and a plugin's own whole and gesture claims on one pane also return `conflict`. When a press matches both claims, the claim with the larger modifier set wins that press and the gesture that follows it; the other plugin receives no event for it. When the held modifiers match only one claim, that claim gets the press.

A drag that carries data, such as a file dragged from a tree list, produces `drag-over` actions on plugin view nodes that declare drop targets, with a `drag-over-value` of source key, target key, index and acceptance (`none`, `before`, `after`, `inside`). A terminal pane does not take drops itself; a plugin that wants a file dropped on a prompt claims the pointer and watches for `up` after a `drag`.

### Rust and TypeScript

This plugin claims Ctrl+click on terminal panes and sends the path under the pointer to its own command.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::primitives::terminal::{self, PointerModifiers};
use ilium_plugin_sdk::types::ui::PointerKind;

#[ilium_plugin(side = "client")]
#[derive(Default)]
struct CtrlOpen;

impl Handler for CtrlOpen {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            match event {
                Event::PaneCreated(pane) => {
                    let _ = terminal::claim_pointer_gesture(pane.node, PointerModifiers::CONTROL);
                }
                Event::Pointer(pointer) if pointer.kind == PointerKind::Down => {
                    let hot = pointer.sub_x > 128; // right half of the cell
                    log::log(Level::Info, &format!("ctrl-click {}:{} right={}",
                        pointer.row, pointer.column, hot));
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
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { claimPointerGesture } from '@ilium/plugin-sdk/primitives/terminal';
import { log } from '@ilium/plugin-sdk/host/log';

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'pane-created') {
        try { claimPointerGesture(event.val.node, ['control']); } catch { /* conflict */ }
      } else if (event.tag === 'pointer' && event.val.kind === 'down') {
        const hot = event.val.subX > 128;
        log('info', `ctrl-click ${event.val.row}:${event.val.column} right=${hot}`);
      }
    }
    return [];
  },
});
```

### Outcomes, errors and limits

| Call | Error | Cause |
|---|---|---|
| `claim-pointer`, `claim-pointer-gesture` | `not-found` | no such pane |
| `claim-pointer`, `claim-pointer-gesture` | `conflict` | another plugin holds an overlapping claim |
| `claim-pointer-gesture` | `invalid-argument` | the modifier set is empty |
| any | `not-permitted` | `terminal.pointer` is missing or revoked |

The host releases every claim of a plugin when the plugin stops, is disabled or loses the grant. `move` events are coalesced per pane; the plugin sees the latest position, not every cell crossed.

### What the user sees

While a whole claim is held, Ilium shows the pane title with the claim marker of the plugin in the status bar. Under a gesture claim nothing changes until the user holds the modifier. The render below is a terminal pane whose path under the pointer is highlighted by a plugin layer.

<pre class="term">╭──≡ ● · Shell────────────────────────────────────────────────────────╮
│<span class="t-prompt">$</span> ls src                                                             │
│main.rs   <span class="t-hi">lib.rs</span>   util.rs                                           │
│<span class="t-prompt">$</span>                                                                    │
╰─────────────────────────────────────────────────────────────────────╯
</pre>

Requirement ids for this page: [PRI-007]. See the [requirements index](/pages/reference-requirements/).

### See also

- [Terminal primitive](/pages/primitives-terminal/)
- [Terminal reads](/pages/primitives-terminal-reads/)
- [Terminal input](/pages/primitives-terminal-input/)
- [Editor viewport and pointer](/pages/primitives-editor-viewport/)
