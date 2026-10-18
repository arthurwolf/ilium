---
layout: page
title: Editor viewport and tokens
permalink: /pages/primitives-editor-viewport/
---

## Editor viewport and tokens

The editor primitive lets a plugin read buffers and add decorations and gutter marks. This page covers what a minimap, a scroll-sync tool, a hover provider or a language plugin needs: the visible range of an editor pane, pointer events over text, marks on the minimap, and a `token-provider` export that supplies syntax tokens for languages the plugin declares. The syntax engine of the built-in editor stays a host service.

This page extends [Editor primitive](/pages/primitives-editor/). The editor lives on the client side.

### You will need

| Grant | Unlocks |
|---|---|
| `editor.read` | `viewport`, `set-minimap-marks`, `editor-pointer`, `editor-viewport-changed`, the `token-provider` export, the `contributes.languages` manifest key |

See [editor.read](/pages/capability-editor-read/). A manifest that exports `token-provider` also needs `contributes.languages`, at most 8 entries, each with `id`, `title`, `extensions` and optional `file_names`.

### Signatures

```wit
record editor-viewport { buffer: buffer-id, pane: node-id, first-line: u32, last-line: u32, columns: u16, rows: u16 }
viewport: func(pane: node-id) -> option<editor-viewport>;
record minimap-mark { line: u32, color: color, lane: u8 }      // lane 0..2
set-minimap-marks: func(buffer: buffer-id, layer: string, marks: list<minimap-mark>) -> result<_, plugin-error>;

// export, with-tokens
enum token-kind { keyword, string, number, comment, type-name, function, variable,
                  operator, punctuation, constant, tag, attribute, plain }
record token { line: u32, start: u32, length: u32, kind: token-kind }
tokens: func(buffer: buffer-id, first-line: u32, lines: list<string>, state-in: option<list<u8>>)
  -> tuple<list<token>, option<list<u8>>>;
```

| Rust (`primitives::editor`) | TypeScript | Returns |
|---|---|---|
| `viewport(pane)` | `viewport` | `Option<EditorViewport>` |
| `set_minimap_marks(buffer, layer, marks)` | `setMinimapMarks` | `Result<(), PluginError>` |

Events: `editor-viewport-changed(editor-viewport)`, coalesced per pane at 100 ms, and `editor-pointer { buffer, pane, position, pointer }`, where `position` is a `text-position` (line and column in Unicode scalar values) and `pointer` is the `view-pointer` of [Terminal pointer](/pages/primitives-terminal-pointer/). Hover, click and drag over text produce it; `move` events are coalesced.

### Viewport

`first-line` and `last-line` are the first and last buffer lines that are at least partly visible; `columns` and `rows` are the size of the text area. Wrapped lines count once. The event arrives after scrolling, resizing or switching the buffer in the pane. A plugin that mirrors a second view, such as a preview, follows `first-line` and answers with its own scroll.

### Minimap marks

The minimap is the narrow strip at the right of an editor pane; the host draws it from the text density with the ramp `·`, `-`, `=`, `≡`, `≣`, `█`. A plugin adds marks to three lanes (0 to 2) to show diagnostics, search hits or changes. A mark has a line and a colour; the layer name scopes the set to the plugin, and each call replaces the layer. At most 4,096 marks per layer. A layer name is 1 to 32 characters from `[a-z0-9-]` and is scoped to the calling plugin. A plugin has at most 16 layers per buffer. Marks are removed when the plugin stops.

<pre class="term">╭──≡ ● · main.rs───────────────────────────────────────╮
│fn main() {                                       <span class="t-dim">·</span>   │
│    <span class="t-hi">let</span> total = add(1, 2);                      <span class="t-dim">-</span>     │
│    <span class="t-err">oops();</span>                                     <span class="t-err">≡</span>     │
│    println!("{total}");                          <span class="t-dim">-</span>   │
│}                                                 <span class="t-dim">·</span>   │
╰──────────────────────────────────────────────────────╯
</pre>

### Token provider

A language plugin implements `tokens`. The host calls it for the visible lines plus a margin, with `state-in` from the previous block so multi-line constructs such as block comments work, and stores the returned state for the next call. The deadline is 10 ms per call. After a timeout the lines stay plain and the host calls again only at the next change of the buffer or viewport (no timer, no retry limit). The plugin chooses a `token-kind`; it never picks colours, because the theme maps kinds to colours.

<figure>
<svg viewBox="0 0 420 140" role="img" aria-label="Host asks token-provider for visible lines with carried state">
  <rect x="10" y="30" width="120" height="70" rx="6" fill="none" stroke="currentColor"/>
  <text x="70" y="55" text-anchor="middle" fill="currentColor" font-size="11">editor (host)</text>
  <text x="70" y="75" text-anchor="middle" fill="currentColor" font-size="10">visible lines + margin</text>
  <rect x="290" y="30" width="120" height="70" rx="6" fill="none" stroke="currentColor"/>
  <text x="350" y="55" text-anchor="middle" fill="currentColor" font-size="11">token-provider</text>
  <text x="350" y="75" text-anchor="middle" fill="currentColor" font-size="10">10 ms per call</text>
  <path d="M130 50 H290" stroke="currentColor"/>
  <path d="M290 80 H130" stroke="currentColor"/>
  <text x="210" y="44" text-anchor="middle" fill="currentColor" font-size="10">lines, state-in</text>
  <text x="210" y="96" text-anchor="middle" fill="currentColor" font-size="10">tokens, state-out</text>
</svg>
<figcaption>The carried state lets a block comment span calls.</figcaption>
</figure>

### Rust and TypeScript

A minimal provider for a language whose comments start with `#`, plus a viewport follower:

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::exports::{TokenProvider, Token, TokenKind};

impl TokenProvider for HashLang {
    fn tokens(&mut self, _buffer: BufferId, first_line: u32, lines: Vec<String>,
              state_in: Option<Vec<u8>>) -> (Vec<Token>, Option<Vec<u8>>) {
        let mut found = Vec::new();
        for (offset, line) in lines.iter().enumerate() {
            if let Some(start) = line.find('#') {
                found.push(Token {
                    line: first_line + offset as u32,
                    start: line[..start].chars().count() as u32,
                    length: line[start..].chars().count() as u32,
                    kind: TokenKind::Comment,
                });
            }
        }
        (found, state_in)
    }
}

// follow the viewport
if let Event::EditorViewportChanged(view) = event {
    editor::set_minimap_marks(view.buffer, "marks", vec![
        MinimapMark { line: view.first_line, color: Color::role(ColorRole::Accent), lane: 0 }])?;
}
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import { setMinimapMarks } from '@ilium/plugin-sdk/primitives/editor';

export const { lifecycle, handler, tokenProvider } = definePlugin({
  side: 'client',
  tokens(buffer, firstLine, lines, stateIn) {
    const found = [];
    lines.forEach((line, offset) => {
      const start = line.indexOf('#');
      if (start >= 0) {
        found.push({ line: firstLine + offset, start: [...line.slice(0, start)].length,
                     length: [...line.slice(start)].length, kind: 'comment' });
      }
    });
    return [found, stateIn];
  },
  onEvents(batch) {
    for (const event of batch) {
      if (event.tag === 'editor-viewport-changed') {
        setMinimapMarks(event.val.buffer, 'marks',
          [{ line: event.val.firstLine, color: { tag: 'role', val: 'accent' }, lane: 0 }]);
      }
    }
    return [];
  },
});
```

### Outcomes, errors and limits

| Case | Result |
|---|---|
| `viewport` for a pane that is not an editor | `none` |
| `set-minimap-marks` over 4,096 marks, lane over 2, or a bad layer name | `invalid-argument` |
| `set-minimap-marks` on a 17th layer of the buffer | `quota-exceeded` |
| `set-minimap-marks` for an unknown buffer | `not-found` |
| `tokens` over 10 ms | lines stay plain; the host calls again at the next change of the buffer or viewport (no timer, so no interval), with no retry limit, and each call has the same 10 ms deadline |
| Missing grant | `not-permitted` |

### See also

- [Editor primitive](/pages/primitives-editor/)
- [Terminal pointer](/pages/primitives-terminal-pointer/)
- [Accessibility and theming](/pages/accessibility-and-theming/)
