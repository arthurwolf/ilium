# pointer

## pointer

Plugin views receive the mouse as a `view-pointer`: the kind of event, the button, the modifier keys, the cell inside the widget and the position inside that cell. Ordinary widgets (buttons, lists) already turn clicks into their own actions; the pointer model is for views that need the raw gesture, such as a drawing canvas, a timeline or a drag handle. Panes of terminals have their own pointer events; see [terminal pointer](/pages/primitives-terminal-pointer/).

### What you need

- **Grant.** None inside the plugin's own views. Reading or claiming the pointer in a terminal is a separate grant (`terminal.pointer`, see [capability terminal-pointer](/pages/capability-terminal-pointer/)).
- **Node flags.** The node sets `pointer-events` (and `focusable`). Without the flag the widget's normal actions apply. `hover-events` adds `hover-enter` and `hover-leave`.

### Interface

```wit
// Excerpt of ilium:types/ui in reference-wit
enum pointer-kind { down, up, move, drag, wheel-up, wheel-down, wheel-left, wheel-right }
enum pointer-button { none, left, middle, right }
flags pointer-modifiers { shift, control, alt }
record view-pointer {
  kind: pointer-kind, button: pointer-button, modifiers: pointer-modifiers,
  column: u16, row: u16,          // cell inside the widget's area, 0-based
  sub-x: u8, sub-y: u8,           // inside the cell, 0..255; 128 when the terminal reports cells only
  clicks: u8,                     // 1, 2 or 3 for down events; 0 otherwise
}
// action-value cases
pointer(view-pointer),
drag-over(drag-over-value),       // record drag-over-value { source-key, target-key, index: u32, accept: drop-accept }
enum drop-accept { none, before, after, inside }
```

Rust: `ActionValue::Pointer(ViewPointer)`. TypeScript: `{ tag: 'pointer', val: ViewPointer }`.

### Delivery

- `down`, `up`, `drag` and the wheel kinds arrive as `ui-action` kind `pointer`. `move` is sent only while a button is held, and `pointer` actions are coalesced to the frame rate.
- Sub-cell coordinates are real only when the terminal reports pixel positions (SGR pixel mode); otherwise both are 128. A plugin must not depend on them for correctness. A canvas that wants finer than one cell can use them to place a half-cell mark.
- `clicks` counts a double or triple click on `down`, with the host's timing.

<figure>
<pre class="term">column 0       column 1       column 2
<span class="t-dim">╭────────────┬────────────┬────────────╮
│ sub 0..255 │ sub 0..255 │ sub 0..255 │
╰────────────┴────────────┴────────────╯</span>
</pre>
<figcaption>Each cell spans 0 to 255 on both axes, left to right and top to bottom; a cell-only terminal reports 128 for both.</figcaption>
</figure>

### Drag feedback

While a drag hovers a `drop-target`, the host sends `drag-over` once per target change. The plugin answers by setting the target's `accept` prop on its next render (`before`, `after`, `inside` or `none`). The host draws the insertion line or the inside highlight and scrolls near the edges.

<figure>
<pre class="term">  Todo
  ──────────────
  ☐ Write notes
  ━━━━━━━━━━━━━━  &lt;- accept: before the next row
  ☐ Review plan
</pre>
<figcaption>A drag over the row "Review plan" with accept before: the host draws the insertion line above the row.</figcaption>
</figure>

### Examples

**Rust**

```rust
fn on_action(&mut self, action: UiAction) -> Vec<Submission> {
    if let ActionValue::Pointer(pointer) = action.value {
        match (pointer.kind, pointer.button) {
            (PointerKind::Down, PointerButton::Left) => {
                self.start_stroke(pointer.column, pointer.row, pointer.sub_x, pointer.sub_y);
            }
            (PointerKind::Drag, _) => self.extend_stroke(pointer.column, pointer.row),
            (PointerKind::WheelUp, _) => self.zoom_in(),
            _ => {}
        }
    }
    Vec::new()
}
```

**TypeScript**

```ts
onAction(action: UiAction): Submission[] {
  if (action.value.tag !== 'pointer') return [];
  const p = action.value.val;
  if (p.kind === 'down' && p.button === 'left') startStroke(p.column, p.row, p.subX, p.subY);
  else if (p.kind === 'drag') extendStroke(p.column, p.row);
  else if (p.kind === 'wheel-up') zoomIn();
  return [];
}
```

### Errors and limits

Pointer events are not requests; they have no error. A node that sets `pointer-events` without `focusable` is refused at render with `invalid-argument`. See [Errors and limits](/pages/reference-errors-and-limits/).

### See also

- [ui](/pages/api-ui/), [popovers](/pages/api-popovers/), [glyph plane](/pages/api-surface-glyph-plane/), [terminal pointer](/pages/primitives-terminal-pointer/).
