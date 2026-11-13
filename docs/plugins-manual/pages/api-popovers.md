---
layout: page
title: popovers and tooltips
permalink: /pages/api-popovers/
---

## popovers and tooltips

A tooltip is a short note that appears after the pointer rests on something. A popover is a small non-modal panel anchored to a widget, a tree row or the pointer. Both are available to every view: a `tooltip` field on UI nodes, and `open-popup-with` for anchored popups that can be pinned and that close when the pointer leaves.

### What you need

- **Grant.** `ui.panels` for the plugin's own views (see [ui panels](/pages/capability-ui-panels/)). Anchoring at a tree row uses the `tree_toolbar` or `row_actions` contributions, which need `ui.chrome` (see [tree chrome](/pages/api-tree-chrome/)).
- **Flags.** `hover-events` on a node to receive `hover-enter` and `hover-leave`. Tooltips need no flag.

### Interface

```wit
record tooltip { lines: styled-lines, delay-ms: option<u16> }   // <= 200 characters; default 500 ms, 0..2000

record ui-node { /* ... */ tooltip: option<tooltip>, tab-index: option<s16>, disabled-reason: option<string> }

variant popup-anchor {
  centered(tuple<constraint, constraint>), at-cell(tuple<u16, u16>), below-key(string),
  at-key(key-anchor), at-tree-node(node-id), at-pointer,
}
enum anchor-side { below, above, right, left }
record key-anchor { key: string, side: anchor-side }
record popup-options { modal: bool, pinnable: bool, dismiss-on-leave: bool, close-on-escape: bool }
open-popup-with: func(popup: string, anchor: popup-anchor, options: popup-options) -> result<_, plugin-error>;
```

| Piece | Rust | TypeScript |
|---|---|---|
| tooltip on a node | `node.tooltip(Tooltip::text("Opens the report"))` | `{ tooltip: { lines, delayMs } }` |
| open an anchored popup | `ui::open_popup_with("detail", PopupAnchor::AtKey(KeyAnchor { key: "row-3".into(), side: AnchorSide::Right }), options)` | `openPopupWith('detail', { tag: 'at-key', val: { key: 'row-3', side: 'right' } }, options)` |

### Behaviour

- **Hover.** The enter delay (500 ms), the leave grace (150 ms) are one host setting for all widgets; a plugin cannot change them. A tooltip's own `delay-ms` (0..2000) overrides only the default 500 ms show delay of that one tooltip; a value above 2000 is clamped to 2000. A tooltip is placed below, or above when there is no room, and never covers the pointer cell.
- **Pinnable.** `pinnable` adds a pin glyph to the popup title. The pin keeps it open until Escape or a click on the pin.
- **Dismiss on leave.** With `dismiss-on-leave` the popup closes when the pointer leaves both the popup and its anchor for longer than the grace time.
- **Focus.** Focus moves into an opened popup and returns to the anchor on close. Escape closes the topmost popup and sends `closed` only when that popup has `close-on-escape` true; with false, Escape is ignored by the popup and the popup stays open until another dismissal. A `modal` popup blocks pointer and key input to every widget outside it until it closes (pointer clicks outside are swallowed and do not dismiss it unless `dismiss-on-leave` applies); a non-modal popup leaves the rest of the UI interactive.
- **Stack.** At most 4 popups per view; a fifth is `quota-exceeded`.
- **Disabled.** A node with `disabled-reason` is drawn muted, takes no input, and its reason shows as the tooltip.

<figure>
<pre class="term">▼ 📂 Ilium
›   🦀 ⠋ Fix auth
›   🖥️    Shell
  ╭  ≡ ● · Fix auth ─────────╮
  │ Claude, working          │
  │ Goal: finish the login   │
  ╰──────────────────────────╯
</pre>
<figcaption>A popover anchored at the tree row of a pane, drawn with the rounded border and the chrome title. Tree glyphs follow the client: expand symbol, the indentation marker, identity icon and the spinner in the activity slot.</figcaption>
</figure>

<figure>
<svg viewBox="0 0 440 120" role="img" aria-label="Hover timing">
<title>Hover timing</title>
<desc>The pointer enters an anchor, the tooltip shows after 500 ms, and a popover with dismiss-on-leave closes 150 ms after the pointer leaves both.</desc>
<line x1="20" y1="60" x2="420" y2="60" stroke="currentColor"/>
<line x1="40" y1="50" x2="40" y2="70" stroke="currentColor"/><text x="40" y="88" font-size="11" text-anchor="middle" fill="currentColor">enter</text>
<line x1="180" y1="50" x2="180" y2="70" stroke="currentColor"/><text x="180" y="88" font-size="11" text-anchor="middle" fill="currentColor">+500 ms: tooltip</text>
<line x1="300" y1="50" x2="300" y2="70" stroke="currentColor"/><text x="300" y="88" font-size="11" text-anchor="middle" fill="currentColor">leave</text>
<line x1="380" y1="50" x2="380" y2="70" stroke="currentColor"/><text x="380" y="88" font-size="11" text-anchor="middle" fill="currentColor">+150 ms: close</text>
</svg>
<figcaption>The timings are the host's and are the same in every plugin view and kernel row.</figcaption>
</figure>

### Examples

**Rust**

```rust
use ilium_plugin_sdk::host::ui::{self, AnchorSide, KeyAnchor, PopupAnchor, PopupOptions};

fn show_detail(row_key: &str) -> Result<(), PluginError> {
    ui::open_popup_with(
        "detail",
        PopupAnchor::AtKey(KeyAnchor { key: row_key.into(), side: AnchorSide::Right }),
        PopupOptions { modal: false, pinnable: true, dismiss_on_leave: true, close_on_escape: true },
    )
}
```

**TypeScript**

```ts
import { openPopupWith } from '@ilium/plugin-sdk/host/ui';

export const showDetail = (rowKey: string) =>
  openPopupWith('detail', { tag: 'at-key', val: { key: rowKey, side: 'right' } },
    { modal: false, pinnable: true, dismissOnLeave: true, closeOnEscape: true });
```

### Errors and limits

| Case | Error |
|---|---|
| Unknown popup name | `not-found` |
| Anchor key not in the view | `invalid-argument` |
| More than 4 popups open | `quota-exceeded` |
| Tooltip over 200 characters | truncated with `…` |

### See also

- [ui](/pages/api-ui/), [menus](/pages/api-menus/), [pointer](/pages/api-pointer/), [toast](/pages/api-toast/), [tree chrome](/pages/api-tree-chrome/).
