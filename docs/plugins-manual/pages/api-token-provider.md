---
layout: page
title: token provider
permalink: /pages/api-token-provider/
---

## token provider

The `token-provider` export teaches the editor a language the host does not know. The host calls it with lines of text, and the plugin answers with tokens: a line, a start column, a length and a kind. The host then draws the tokens with its theme colours. The plugin never chooses a colour. The syntax engine, its caches and its size budget stay a host service; the provider only supplies tokens for the languages the plugin declares.

### You will need

| Need | Value |
|---|---|
| Side | Client only. The export fragment is `with-tokens` in `ilium:client/base`. |
| Grant | `editor.read`, for both the export and the `languages` contribution. |
| Manifest | `"exports": ["token-provider"]` and one or more `contributes.languages` entries. |
| Rust | Crate `ilium-plugin-sdk`, trait `ilium_plugin_sdk::exports::TokenProvider`. |
| TypeScript | Package `@ilium/plugin-sdk`, member `tokens` of `definePlugin`. |
| Activation | The trigger `language:<id>` starts a stopped instance when a buffer of that language becomes visible. |

### Manifest

```json
{ "contributes": { "languages": [
  { "id": "ilium-log", "title": "Ilium log", "extensions": ["ilog"], "file_names": ["progress.log"] }
] } }
```

`extensions` are written without the dot. At most 8 languages per plugin; more is refused with `manifest.contribution-limit`.

### Interface

```wit
// Excerpt of ilium:exports/token-provider in reference-wit
enum token-kind { keyword, string, number, comment, type-name, function, variable,
                  operator, punctuation, constant, tag, attribute, plain }
record token { line: u32, start: u32, length: u32, kind: token-kind }

tokens: func(buffer: buffer-id, first-line: u32, lines: list<string>, state-in: option<list<u8>>)
    -> tuple<list<token>, option<list<u8>>>;
```

`line` is an absolute buffer line (0 based), `start` and `length` count characters of that line. The second tuple element is the lexer state at the end of the block. The host hands it back as `state-in` for the next block, so a construct that spans lines, such as a block comment, continues correctly.

### Functions

| WIT function | Rust | TypeScript | Result |
|---|---|---|---|
| `tokens` | `TokenProvider::tokens(&mut self, buffer: BufferId, first_line: u32, lines: Vec<String>, state_in: Option<Vec<u8>>) -> (Vec<Token>, Option<Vec<u8>>)` | `tokens(buffer: bigint, firstLine: number, lines: string[], stateIn?: Uint8Array): [Token[], Uint8Array \| undefined]` | tokens of the block and the carried state |

### How the call behaves

- **Which lines.** The host calls for the visible lines plus a margin of 20 lines above and below (one block, as in the figure below), not for the whole file. It passes the carried state of the previous block, so the plugin keeps no cursor of its own.
- **Deadline.** 10 ms per call. On a timeout the lines stay plain and the host retries later. The breach counts toward the deadline rule.
- **Colours.** Colours come from the theme by `token-kind`. `plain` draws in the normal text colour. A plugin cannot pick a colour, so a theme change recolours every language at once.

<figure>
<svg viewBox="0 0 420 200" role="img" aria-label="Token provider call with carried state">
<defs>
<marker id="tok-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
<path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
</marker>
</defs>
<title>Token blocks and carried state</title>
<desc>The host sends block one with no state, receives tokens and a state, then sends block two with that state.</desc>
<rect x="20" y="30" width="110" height="34" rx="6" fill="none" stroke="currentColor"/>
<text x="75" y="52" font-size="12" text-anchor="middle" fill="currentColor">lines 0 to 39</text>
<rect x="20" y="120" width="110" height="34" rx="6" fill="none" stroke="currentColor"/>
<text x="75" y="142" font-size="12" text-anchor="middle" fill="currentColor">lines 40 to 79</text>
<rect x="250" y="75" width="150" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
<text x="325" y="99" font-size="12" text-anchor="middle" fill="currentColor">Plugin: tokens()</text>
<line x1="130" y1="47" x2="248" y2="88" stroke="currentColor" marker-end="url(#tok-arrow)"/>
<text x="150" y="58" font-size="10" fill="currentColor">state-in: none</text>
<line x1="250" y1="108" x2="132" y2="132" stroke="currentColor" marker-end="url(#tok-arrow)"/>
<text x="168" y="130" font-size="10" fill="currentColor">tokens + state-out</text>
<line x1="130" y1="145" x2="300" y2="117" stroke="currentColor" marker-end="url(#tok-arrow)"/>
<text x="160" y="170" font-size="10" fill="currentColor">state-in: the bytes returned above</text>
</svg>
<figcaption>The state returned for one block is the input of the next, so a multi-line comment keeps its colour across the seam.</figcaption>
</figure>

### Example: a log language

The plugin colours a time stamp, a level word and the rest of a line. It keeps no state, so the second tuple element is unset.

**Grants:** `editor.read`

**Rust**

```rust
use ilium_plugin_sdk::exports::TokenProvider;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::primitives::editor::BufferId;
use ilium_plugin_sdk::exports::token_provider::{Token, TokenKind};

impl TokenProvider for Plugin {
    fn tokens(
        &mut self,
        _buffer: BufferId,
        first_line: u32,
        lines: Vec<String>,
        _state_in: Option<Vec<u8>>,
    ) -> (Vec<Token>, Option<Vec<u8>>) {
        let mut out = Vec::new();
        for (offset, text) in lines.iter().enumerate() {
            let line = first_line + offset as u32;
            // "2026-10-12 09:15:03 warn message"
            if text.len() >= 19 {
                out.push(Token { line, start: 0, length: 19, kind: TokenKind::Number });
            }
            for (word, kind) in [("warn", TokenKind::Keyword), ("error", TokenKind::Constant)] {
                if let Some(at) = text.find(word) {
                    out.push(Token { line, start: at as u32, length: word.len() as u32, kind });
                }
            }
        }
        (out, None)
    }
}
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import type { Token } from '@ilium/plugin-sdk/types';

export const { tokenProvider } = definePlugin({
  side: 'client',
  tokens(_buffer, firstLine, lines): [Token[], Uint8Array | undefined] {
    const out: Token[] = [];
    lines.forEach((text, offset) => {
      const line = firstLine + offset;
      if (text.length >= 19) out.push({ line, start: 0, length: 19, kind: 'number' });
      for (const [word, kind] of [['warn', 'keyword'], ['error', 'constant']] as const) {
        const at = text.indexOf(word);
        if (at >= 0) out.push({ line, start: at, length: word.length, kind });
      }
    });
    return [out, undefined];
  },
});
```

Byte offsets and character offsets differ for non-ASCII text. The contract counts characters of the line, so a Rust plugin converts `str::find` byte positions with `chars().count()` when a line is not ASCII.

### What the user sees

The editor draws the tokens with the theme colours of each kind. A line with a warning keeps the time stamp in the number colour and the word `warn` in the keyword colour:

<pre class="term">
 41 │ 2026-10-12 09:15:03 info  server started
 42 │ 2026-10-12 09:15:07 warn  config key ignored
 43 │ 2026-10-12 09:15:09 error plugin stopped
</pre>

The render shows the layout only. Colours are not shown here because they come from the user's theme and the plugin never sets them.

### Errors and limits

| Situation | Result |
|---|---|
| Deadline of 10 ms exceeded | Lines stay plain, host retries, breach counted. |
| More than 8 languages | `manifest.contribution-limit`. |
| Trap in `tokens` | The lines stay plain and the plugin restarts under the usual rule. |

### See also

- [Plugin exports](/pages/api-exports/) for the export list and deadlines.
- [Primitives: editor](/pages/primitives-editor/) for the `annotator` path and `editor.viewport`.
- [capability: editor read](/pages/capability-editor-read/) for `editor.read`.
- [Widgets: media and embedded](/pages/widgets-media-and-embedded/) for `code-view`, which uses the same tokens.

Contract: `ilium:plugin@0.1.0`, export fragment `with-tokens`.
