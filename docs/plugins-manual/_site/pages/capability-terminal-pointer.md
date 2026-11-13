# terminal.pointer

## Overview

The `terminal.pointer` grant lets a client-side plugin take the mouse in a terminal pane. The plugin claims a pane, receives every pointer event for that pane, and draws highlight regions over its text. Copy features use it so the user can select a block of output with the mouse. The grant never changes the text in the pane.

A claim belongs to one plugin per pane. While it is held, the host stops sending mouse input to the pane's program, stops its own text selection on that pane, and stops the wheel from scrolling the scrollback. All three return when the claim ends.

<figure>
<svg viewBox="0 0 400 150" role="img" aria-label="Pointer events go from the pane to the claiming plugin, and the plugin sends highlights back to the pane">
  <defs><marker id="arr-pointer-flow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto"><path d="M0,0 L10,5 L0,10 z" fill="currentColor"/></marker></defs>
  <rect x="20" y="10" width="360" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="200" y="35" text-anchor="middle" fill="currentColor" font-size="12">Client plugin (holds the claim)</text>
  <rect x="20" y="100" width="360" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="200" y="125" text-anchor="middle" fill="currentColor" font-size="12">Terminal pane (display and mouse)</text>
  <line x1="120" y1="100" x2="120" y2="54" stroke="currentColor" marker-end="url(#arr-pointer-flow)"/>
  <text x="134" y="78" fill="currentColor" font-size="10">pointer events</text>
  <line x1="280" y1="54" x2="280" y2="100" stroke="currentColor" marker-end="url(#arr-pointer-flow)"/>
  <text x="294" y="78" fill="currentColor" font-size="10">set-highlights</text>
</svg>
<figcaption>While the plugin holds the claim, the pane's pointer events go to the plugin and not to the pane. Highlights are drawn over the pane, and the text stays the same.</figcaption>
</figure>

## What you need

- The grant `terminal.pointer`, on the client side only. The grant is normal risk. A bundled T1 plugin receives it up to its manifest ceiling. A T2 or T3 plugin needs an explicit grant in the review dialog, and the grant is remembered.
- The grant [`terminal.read-screen`](/pages/capability-terminal-read-screen/) when the plugin reads the text under the highlights.
- The Rust crate `ilium-plugin-sdk` (Rust 1.90 or later, target `wasm32-wasip2`), or the npm package `@ilium/plugin-sdk`.

The grant takes no argument. `terminal.pointer:claim` is refused at build time with `manifest.grant-argument`: `capability "terminal.pointer" takes no argument (got "terminal.pointer:claim")`. The claim is a call made at run time, not a grant argument.

```json
{
  "capabilities": {
    "client": ["terminal.read-screen", "terminal.pointer", "clipboard.write"]
  }
}
```

## The surface

| Function | Rust (`primitives::terminal`) | TypeScript (`primitives/terminal`) | Result | Meaning |
|---|---|---|---|---|
| `claim-pointer` | `claim_pointer(pane)` | `claimPointer(pane)` | `Result<(), PluginError>`, throws | Sends the pane's pointer events to this plugin until it is released. |
| `release-pointer` | `release_pointer(pane)` | `releasePointer(pane)` | none | Gives the pointer back to the host. It never fails. |
| `set-highlights` | `set_highlights(pane, layer, &[CellRegion])` | `setHighlights(pane, layer, regions)` | `Result<(), PluginError>`, throws | Replaces every region of one layer. |
| `clear-highlights` | `clear_highlights(pane, layer)` | `clearHighlights(pane, layer)` | none | Removes one layer and its regions. |

In Rust, `pane` is a `NodeId`. In TypeScript it is a `bigint`, because every `u64` node id is a `bigint` in TypeScript. A layer is a string.

A highlight is a `CellRegion`:

| Field | Rust | TypeScript | Meaning |
|---|---|---|---|
| first row | `first_row: u32` | `firstRow: number` | First row, counted from the top visible row (0). |
| first column | `first_column: u16` | `firstColumn: number` | First column, counted from the left (0). |
| last row | `last_row: u32` | `lastRow: number` | Last row, inclusive. |
| last column | `last_column: u16` | `lastColumn: number` | Last column, inclusive. |
| style | `style: Style` | `style: Style` | Foreground, background and text modifiers. |
| label | `label: Option<String>` | `label: string \| undefined` | Optional text for a tooltip, at most 128 characters. |

Pointer events arrive in the `on_events` batch as `Event::Pointer` in Rust, or as `{ tag: 'pointer', val }` in TypeScript. The payload is a `PointerEvent`:

| Field | Type | Meaning |
|---|---|---|
| `node` | `node-id` | The terminal pane. |
| `row` | `u32` | Visible row of the pointer, from the top (0). The scrollback offset is not applied. |
| `column` | `u16` | Column of the pointer, from the left (0). |
| `kind` | `down`, `up`, `move`, `drag`, `wheel-up`, `wheel-down`, `wheel-left` or `wheel-right` | What happened. |
| `button` | `left`, `middle`, `right` or `none` | The button. `none` for wheel and move events. |

The WIT forms are:

```wit
// Excerpt of ilium:types/ui in reference-wit
enum pointer-kind { down, up, move, drag, wheel-up, wheel-down, wheel-left, wheel-right }
enum pointer-button { none, left, middle, right }
```

The event record `pointer-event` is defined in `ilium:types/events` and also carries `modifiers`, `sub-x` and `sub-y`. The button is `none` for a wheel event and for `move`.

```wit
// Excerpt of ilium:primitives/terminal in reference-wit
record cell-region {
  first-row: u32, first-column: u16, last-row: u32, last-column: u16,
  style: style, label: option<string>,
}
```

## How a claim behaves

- **Events.** While the claim is held, every mouse event for the pane goes to the plugin. Drag events are coalesced, so a fast drag produces fewer events than the mouse moved. Use the row of the latest event, not a sum of movements.
- **Wheel.** A wheel event (`wheel-up`, `wheel-down`, `wheel-left` or `wheel-right`) goes to the plugin and does not scroll the scrollback. The plugin decides what the wheel means.
- **Typing.** Keyboard input is not affected. Typing into the pane needs `terminal.write`, not a claim.
- **One owner.** A pane has at most one claim. A second plugin gets `conflict`. Calling `claim-pointer` again from the plugin that already holds the claim returns `ok` and changes nothing.
- **Release.** `release-pointer` without a claim does nothing and returns no error.
- **Lifetime.** A claim ends on `release-pointer`, when the plugin is deactivated for any reason, and when the pane is removed. When the `terminal.pointer` grant is revoked, the host releases the claim and re-activates the plugin with the reduced grant set.

Release the claim as soon as the feature no longer needs the mouse. A claim that stays open blocks the normal mouse behaviour of the pane.

## How highlights behave

- **Layer names** match `[a-z0-9-]{1,32}`. A layer belongs to the calling plugin, so two plugins can both use a layer called `search` without touching each other.
- **Limits.** A layer holds at most 4,096 regions. A plugin may hold at most 16 layers in one pane. Both limits are checked before anything is written.
- **Clipping.** A region outside the visible screen is clipped to it without an error. A region whose first row or column is after its last is `invalid-argument`.
- **Overlap.** Regions of one layer are painted in list order. Where two regions overlap, the later region sets the foreground and background. The `add` and `remove` modifiers of all overlapping regions are combined.
- **Labels.** The label is not drawn in the pane. The client shows it as a tooltip when the pointer rests on the region for 500 ms.
- **Lifetime.** Highlights survive `release-pointer`, so a plugin can give the mouse back and keep the result on screen. They are withdrawn when the plugin is disabled, when its grant is revoked, or when the pane is removed.

## Errors

Each call checks its grant, the pane and the arguments before it writes anything. On an error, nothing changes.

| Code | Call | When | What to do |
|---|---|---|---|
| `not-permitted` | all four | The grant is missing or was declined. | Offer a keyboard path. Do not retry. |
| `not-found` | claim, set, clear | The pane does not exist. | Read the tree again. |
| `unsupported` | claim, set | The node exists but is not a terminal pane. | Check the node kind before calling. |
| `conflict` | claim | Another plugin holds the claim. | Wait for it to release, or fall back. Retry after reading the state. |
| `invalid-argument` | set | First row or column after the last, over 4,096 regions, a bad layer name, or a label over 128 characters. | Fix the call. Do not retry it unchanged. |
| `quota-exceeded` | set | The plugin already holds 16 layers in this pane. | Clear a layer it no longer uses. |
| `revoked` | all | The grant was revoked during the call. | Do not retry. The plugin is re-activated. |

## What the user sees

While a claim is held, the pane behaves as described above. The highlighted rows use the colour of the style. The label appears only as a tooltip.

<figure>
<pre class="term"><span class="t-prompt">$</span> ./scripts/check.sh
<span class="t-dim">Running 3 checks</span>
<span class="t-hi">error[E0308]: mismatched types in src/lib.rs</span>
<span class="t-hi">  --&gt; src/lib.rs:42:9</span>
<span class="t-hi">   |     expected `u32`, found `i64`</span>
<span class="t-err">check failed: 1 error</span>
<span class="t-prompt">$</span> _
</pre>
<figcaption>Illustrative, not generated. A pane with the accent highlight over the three rows of one error block. The label Output block is a tooltip, not text in the pane.</figcaption>
</figure>

When a plugin asks for the grant, the review dialog lists it in a 70-column box. The wording comes from the grant's entry in the capability catalogue.

<figure>
<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.terminal 0.1.0 by Example Ltd                                  │
│                                                                            │
│ <span class="t-hi">Client side</span>                                                                │
│ [x] Claims the pointer in terminals <span class="t-dim">………………………………………………………</span> terminal.pointer │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>The review dialog for a package that requests terminal.pointer, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar. Checked rows are granted; a high-risk row starts unchecked.</figcaption>
</figure>

<figure>
<svg viewBox="0 0 400 150" role="img" aria-label="A claim moves between unclaimed and claimed states">
  <defs><marker id="arr-pointer-life" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto"><path d="M0,0 L10,5 L0,10 z" fill="currentColor"/></marker></defs>
  <rect x="20" y="50" width="120" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="80" y="76" text-anchor="middle" fill="currentColor" font-size="12">Unclaimed</text>
  <rect x="260" y="50" width="120" height="44" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="320" y="76" text-anchor="middle" fill="currentColor" font-size="12">Claimed</text>
  <line x1="140" y1="62" x2="258" y2="62" stroke="currentColor" marker-end="url(#arr-pointer-life)"/>
  <text x="200" y="44" text-anchor="middle" fill="currentColor" font-size="10">claim-pointer: ok</text>
  <line x1="258" y1="84" x2="142" y2="84" stroke="currentColor" marker-end="url(#arr-pointer-life)"/>
  <text x="200" y="108" text-anchor="middle" fill="currentColor" font-size="10">release, deactivate, revoke</text>
  <text x="200" y="140" text-anchor="middle" fill="currentColor" font-size="10">Only one plugin holds a claim per pane.</text>
</svg>
<figcaption>A claim is held until the plugin releases it or the plugin stops. Highlights stay on screen after a release.</figcaption>
</figure>

## Example 1: select a block of rows with the mouse (Rust)

The plugin starts a pick from a command the user triggers. A left press sets the first row. Dragging extends the highlight, and the release keeps it and gives the pointer back. The region spans the full width of the pane, and the host clips any part beyond the screen.

**Grants:** `terminal.pointer`

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::primitives::terminal::{self, CellRegion};
use ilium_plugin_sdk::types::ui::{PointerButton, PointerKind};
use ilium_plugin_sdk::types::events::PointerEvent;
use ilium_plugin_sdk::types::text::{Color, ColorRole, Modifiers};

/// Selects whole rows of one pane with the mouse.
#[derive(Default)]
struct BlockCopy {
    pane: Option<NodeId>,
    anchor_row: Option<u32>,
    current_row: u32,
}

impl BlockCopy {
    /// Called from a command the user triggers. The claim can be refused.
    fn start(&mut self, pane: NodeId) -> Result<(), PluginError> {
        terminal::claim_pointer(pane)?;
        self.pane = Some(pane);
        self.anchor_row = None;
        Ok(())
    }

    fn on_pointer(&mut self, pointer: PointerEvent) -> Result<(), PluginError> {
        if self.pane != Some(pointer.node) {
            return Ok(());
        }
        match pointer.kind {
            PointerKind::Down if matches!(pointer.button, Some(PointerButton::Left)) => {
                self.anchor_row = Some(pointer.row);
                self.current_row = pointer.row;
                self.paint()
            }
            PointerKind::Drag => {
                self.current_row = pointer.row;
                self.paint()
            }
            PointerKind::Up => {
                self.current_row = pointer.row;
                self.paint()?;
                self.finish();
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn paint(&self) -> Result<(), PluginError> {
        let (Some(pane), Some(anchor_row)) = (self.pane, self.anchor_row) else {
            return Ok(());
        };
        let region = CellRegion {
            first_row: anchor_row.min(self.current_row),
            first_column: 0,
            last_row: anchor_row.max(self.current_row),
            last_column: 79,
            style: Style {
                fg: Some(Color::Role(ColorRole::Accent)),
                bg: None,
                add: Modifiers::empty(),
                remove: Modifiers::empty(),
            },
            label: Some("Output block".to_string()),
        };
        terminal::set_highlights(pane, "copy-block", &[region])
    }

    /// Gives the pointer back. The highlight stays on screen.
    fn finish(&mut self) {
        if let Some(pane) = self.pane.take() {
            terminal::release_pointer(pane);
        }
        self.anchor_row = None;
    }
}

impl Handler for BlockCopy {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            if let Event::Pointer(pointer) = event {
                // A failed call means the pane or the grant went away. The host has already
                // ended the claim on that path, so the error needs no further action here.
                let _ = self.on_pointer(pointer);
            }
        }
        Vec::new()
    }
}
```

Outcome: after the user drags from row 10 to row 14, the pane shows an accent highlight over rows 10 to 14, and the claim is released. The mouse selects text again as soon as `finish` runs.

## Example 2: fall back to the keyboard when the grant is declined (TypeScript)

The same feature in TypeScript. The start function treats `not-permitted` and `conflict` as a reason to offer the keyboard path instead of failing. The handler marks one clicked row and releases the claim.

**Grants:** `terminal.pointer`

**TypeScript**

```ts
import { PluginError, definePlugin } from '@ilium/plugin-sdk';
import { claimPointer, releasePointer, setHighlights } from '@ilium/plugin-sdk/primitives/terminal';
import type { Event, Submission } from '@ilium/plugin-sdk/types';

let activePane: bigint | undefined;

/** Starts a mouse pick. Returns the input mode the UI should show. */
export function startBlockPick(pane: bigint): 'mouse' | 'keyboard' {
  try {
    claimPointer(pane);
  } catch (error) {
    const is_declined = error instanceof PluginError
      && (error.code === 'not-permitted' || error.code === 'conflict');
    if (is_declined) {
      return 'keyboard';
    }
    throw error;
  }
  activePane = pane;
  return 'mouse';
}

export default definePlugin({
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag !== 'pointer' || event.val.node !== activePane) {
        continue;
      }
      if (event.val.kind === 'up' && event.val.button === 'left') {
        setHighlights(event.val.node, 'copy-block', [{
          firstRow: event.val.row, firstColumn: 0, lastRow: event.val.row, lastColumn: 79,
          style: { fg: { tag: 'role', val: 'accent' }, bg: undefined, add: {}, remove: {} },
          label: 'Output row',
        }]);
        releasePointer(event.val.node);
        activePane = undefined;
      }
    }
    return [];
  },
});
```

Outcome: with the grant, the user clicks a row and that row is highlighted. Without it, `startBlockPick` returns `keyboard`, the pane keeps its normal mouse behaviour, and the feature shows its keyboard path.

## Who requests it

- Feature 65, Smart Copy light.
- Feature 66, Smart Copy AI.

## See also

- [terminal.read-screen](/pages/capability-terminal-read-screen/) for the text that a highlight covers.
- [Terminal primitive](/pages/primitives-terminal/) for the full terminal import and the output filters.
- [Events](/pages/api-events/) for the `pointer` event in the full event list.
- [clipboard](/pages/capability-clipboard/) for copying the selected text.
- [Errors and limits](/pages/reference-errors-and-limits/) for the error codes in general.

Checked against the API version `ilium:plugin@0.1.0`.
