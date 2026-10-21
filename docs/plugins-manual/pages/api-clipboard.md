---
layout: page
title: clipboard
permalink: /pages/api-clipboard/
---

## clipboard

The `clipboard` import reads and writes plain text on the system clipboard. The clipboard belongs to the user's desktop session, so the client runs this import and a server-side plugin cannot call it. Writing is common and low risk. Reading is high risk: the clipboard often holds passwords and other secrets that the user copied from another program. A plugin must therefore hold the `clipboard.read` grant before it can read, and every read is logged.

Two functions cover the whole import. `read-text` returns the clipboard text. `write-text` replaces it. Both are synchronous: they return at once and never return a `request-id`.

### You will need

| Need | Value |
|---|---|
| Side | Client. The import belongs to the client world, `ilium:client/base`. |
| Grant to write | `clipboard.write`, normal risk. |
| Grant to read | `clipboard.read`, high risk. A T2 or T3 plugin asks in the install review dialog; a T1 bundled plugin asks at its first activation. |
| Rust | Crate `ilium-plugin-sdk`, Rust stable 1.90 or later, target `wasm32-wasip2`. |
| TypeScript | Package `@ilium/plugin-sdk`, Bun 1.2 or later. `bun install` adds the toolchain. |
| Text format | UTF-8 plain text only. |

Import the module in each language:

```rust
use ilium_plugin_sdk::host::clipboard;
```

```ts
import * as clipboard from '@ilium/plugin-sdk/host/clipboard';
```

### Interface

```wit
// Excerpt of ilium:host/clipboard in reference-wit
read-text: func() -> option<string>;                        // clipboard.read
write-text: func(text: string) -> result<_, plugin-error>;  // clipboard.write
```

### Functions

| WIT function | Rust | TypeScript | Grant | Result | What it does |
|---|---|---|---|---|---|
| `read-text` | `clipboard::read_text() -> Option<String>` | `clipboard.readText(): string \| undefined` | `clipboard.read` | `option<string>` | Returns the plain text on the clipboard. Returns nothing when the clipboard holds no text, for example an image. |
| `write-text` | `clipboard::write_text(text: &str) -> Result<(), PluginError>` | `clipboard.writeText(text: string): void`, throws `PluginError` | `clipboard.write` | `result<_, plugin-error>` | Replaces the clipboard text with `text`. |

The names follow the binding rules (S5): kebab-case becomes snake_case in Rust and camelCase in TypeScript.

### How the import behaves

- **Plain text only.** Only UTF-8 text is covered. Images and other formats are not part of the contract. `read-text` returns nothing for an image. Text longer than 1 MiB is cut at a character boundary.
- **Write limit.** `write-text` accepts at most 1 MiB (1,048,576 UTF-8 bytes). A larger text returns `invalid-argument`, nothing is written, and the text is never truncated.
- **Route.** The client writes through the system clipboard when it runs on the user's desktop. When it runs in a terminal that supports OSC 52, it writes through an OSC 52 sequence. With neither route, `write-text` returns `unsupported` and `read-text` returns nothing.
- **Logging.** Every successful read writes an info line to the plugin log that records only the length, never the text. The same line goes to the privacy log. The user is not asked before each read.
- **Footer.** After each read, the client footer shows `clipboard read by <plugin name>` for 3 s.
- **Prompts.** The review dialog lists `Read the clipboard` for `clipboard.read` and `Write to the clipboard` for `clipboard.write`. A T2 or T3 plugin asks for the high-risk grant in the install review dialog; a T1 plugin asks at its first activation. There is no prompt per call.

<figure>
<svg viewBox="0 0 400 200" role="img" aria-label="Copy flow from terminal selection to the clipboard">
<defs>
<marker id="arrow-flow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
<path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
</marker>
</defs>
<title>Copy selection flow</title>
<desc>The terminal primitive holds the selection. The plugin reads it and writes the text to the desktop clipboard, which only the client owns.</desc>
<rect x="40" y="8" width="320" height="34" rx="6" fill="none" stroke="currentColor"/>
<text x="200" y="30" font-size="12" text-anchor="middle" fill="currentColor">Terminal primitive (kernel)</text>
<line x1="200" y1="42" x2="200" y2="78" stroke="currentColor" marker-end="url(#arrow-flow)"/>
<text x="212" y="64" font-size="11" fill="currentColor">terminal.selection</text>
<rect x="40" y="80" width="320" height="34" rx="6" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
<text x="200" y="102" font-size="12" text-anchor="middle" fill="currentColor">Plugin: copy handler</text>
<line x1="200" y1="114" x2="200" y2="150" stroke="currentColor" marker-end="url(#arrow-flow)"/>
<text x="212" y="136" font-size="11" fill="currentColor">clipboard.write-text</text>
<rect x="40" y="152" width="320" height="34" rx="6" fill="none" stroke="currentColor"/>
<text x="200" y="174" font-size="12" text-anchor="middle" fill="currentColor">Desktop clipboard (client)</text>
</svg>
<figcaption>Copying a selection. The plugin reads the selection through the terminal primitive and never reads the clipboard.</figcaption>
</figure>

<figure>
<svg viewBox="0 0 400 230" role="img" aria-label="Route chosen by the client for a clipboard write">
<defs>
<marker id="arrow-route" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
<path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
</marker>
</defs>
<title>Clipboard write route</title>
<desc>The client checks for a local desktop first, then for OSC 52 support. If neither exists, the write is unsupported.</desc>
<rect x="20" y="40" width="150" height="30" rx="4" fill="none" stroke="currentColor"/>
<text x="95" y="60" font-size="11" text-anchor="middle" fill="currentColor">Client runs locally?</text>
<line x1="170" y1="55" x2="228" y2="55" stroke="currentColor" marker-end="url(#arrow-route)"/>
<text x="199" y="48" font-size="10" text-anchor="middle" fill="currentColor">yes</text>
<rect x="230" y="40" width="150" height="30" rx="4" fill="none" stroke="currentColor"/>
<text x="305" y="60" font-size="11" text-anchor="middle" fill="currentColor">System clipboard</text>
<line x1="95" y1="70" x2="95" y2="108" stroke="currentColor" marker-end="url(#arrow-route)"/>
<text x="106" y="92" font-size="10" fill="currentColor">no</text>
<rect x="10" y="110" width="170" height="30" rx="4" fill="none" stroke="currentColor"/>
<text x="95" y="130" font-size="11" text-anchor="middle" fill="currentColor">Terminal supports OSC 52?</text>
<line x1="180" y1="125" x2="228" y2="125" stroke="currentColor" marker-end="url(#arrow-route)"/>
<text x="204" y="118" font-size="10" text-anchor="middle" fill="currentColor">yes</text>
<rect x="230" y="110" width="150" height="30" rx="4" fill="none" stroke="currentColor"/>
<text x="305" y="130" font-size="11" text-anchor="middle" fill="currentColor">OSC 52 sequence</text>
<line x1="95" y1="140" x2="95" y2="178" stroke="currentColor" marker-end="url(#arrow-route)"/>
<text x="106" y="162" font-size="10" fill="currentColor">no</text>
<rect x="10" y="180" width="170" height="30" rx="4" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
<text x="95" y="200" font-size="11" text-anchor="middle" fill="currentColor">Result: unsupported</text>
</svg>
<figcaption>The write route. With no route, a write returns unsupported and a read returns nothing.</figcaption>
</figure>

### Grants and refusals

| Situation | `write-text` | `read-text` |
|---|---|---|
| Grant held | Writes the text. | Returns the text, or nothing when the clipboard has no text. |
| Grant declined by the user | `not-permitted`. | Returns nothing. A warn line is written to the plugin log. |
| Grant revoked during the call | `revoked`. The plugin is re-activated. | Returns nothing, because `read-text` has no error result. |
| Grant never requested in the manifest | Traps. The plugin fails and is restarted under the rules in [errors and limits](/pages/reference-errors-and-limits/). | Traps, the same as an audio call without an audio grant. |

### Using the clipboard from the server side

The clipboard belongs to the client, so a server-side plugin cannot call this import. The server side has a typed way to ask its own client side: a client request (see [client requests](/pages/api-client-requests/)). The client side holds the clipboard grant, reads the text, and answers. The server side never gets a grant for the clipboard, and the read is logged on the client as usual.

<figure>
<svg viewBox="0 0 440 190" role="img" aria-label="Server side asks the client side to read the clipboard">
  <title>Clipboard read through a client request</title>
  <defs>
    <marker id="clipboard-creq-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
      <path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
    </marker>
  </defs>
  <text x="70" y="16" font-size="12" text-anchor="middle" fill="currentColor">Server side</text>
  <text x="370" y="16" font-size="12" text-anchor="middle" fill="currentColor">Client side</text>
  <line x1="70" y1="24" x2="70" y2="180" stroke="currentColor" stroke-dasharray="3 3"/>
  <line x1="370" y1="24" x2="370" y2="180" stroke="currentColor" stroke-dasharray="3 3"/>
  <line x1="72" y1="46" x2="368" y2="46" stroke="currentColor" marker-end="url(#clipboard-creq-arrow)"/>
  <text x="220" y="40" font-size="10" text-anchor="middle" fill="currentColor">request(client, "clipboard-text")</text>
  <rect x="270" y="62" width="200" height="22" rx="4" fill="none" stroke="currentColor"/>
  <text x="370" y="77" font-size="10" text-anchor="middle" fill="currentColor">clipboard.read-text()</text>
  <line x1="368" y1="110" x2="72" y2="110" stroke="currentColor" marker-end="url(#clipboard-creq-arrow)"/>
  <text x="220" y="104" font-size="10" text-anchor="middle" fill="currentColor">respond(call, ok(json))</text>
  <text x="70" y="142" font-size="10" fill="currentColor">take-answer(request) returns the JSON text.</text>
  <text x="70" y="160" font-size="10" fill="currentColor">The grant clipboard.read stays on the client side only.</text>
</svg>
<figcaption>The server side gets clipboard text through the client side, which owns the desktop clipboard and the grant.</figcaption>
</figure>

The full code for this exchange, in Rust and TypeScript, is the second worked example on the [client requests](/pages/api-client-requests/) page.

### Example 1: copy the selected text of a pane

A context-menu item named `copy-selection` copies the selected text of a pane. The plugin reads the selection through the terminal primitive and writes it to the clipboard. It never reads the clipboard. The pane comes from the `context` field of the `command-invoked` event.

**Grants:** `terminal.read-screen`, `clipboard.write`

**Rust**

```rust
use ilium_plugin_sdk::host::clipboard;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::primitives::terminal;

/// Called from the handler of the `copy-selection` command.
fn copy_selection(pane: NodeId) -> Result<(), PluginError> {
    let Some(selection) = terminal::selection(pane) else {
        return Ok(());
    };
    if selection.text.is_empty() {
        return Ok(());
    }
    clipboard::write_text(&selection.text)
}
```

**TypeScript**

```ts
import * as clipboard from '@ilium/plugin-sdk/host/clipboard';
import * as terminal from '@ilium/plugin-sdk/primitives/terminal';

/** Called from the handler of the `copy-selection` command. */
export function copySelection(pane: bigint): void {
  const selection = terminal.selection(pane);
  if (selection === undefined || selection.text.length === 0) {
    return;
  }
  clipboard.writeText(selection.text);
}
```

Outcome: after the menu item runs, the clipboard holds the selected text. Pasting it into another pane gives the same characters. A pane with no selection writes nothing and shows no error.

### Example 2: paste the clipboard into a pane

A command named `paste-clipboard` sends the clipboard text to a pane as a paste. It needs two grants: `clipboard.read` for the text and `terminal.write` for the send. The `send-input` intent requires a `dedup-key`, and its text is limited to 64 KiB. A clipboard text over 64 KiB is therefore skipped with a warn line. It is never truncated.

**Grants:** `clipboard.read`, `terminal.write`

**Rust**

```rust
use ilium_plugin_sdk::host::clipboard;
use ilium_plugin_sdk::host::clock;
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::intents::{Delivery, SendInputSpec};

const MAX_PASTE_BYTES: usize = 64 * 1024;

/// Builds the paste submission for `pane`, or nothing when there is nothing to paste.
fn paste_clipboard(pane: NodeId) -> Option<Submission> {
    let text = clipboard::read_text()?;
    if text.is_empty() {
        return None;
    }
    if text.len() > MAX_PASTE_BYTES {
        log::log(Level::Warn, "clipboard text over 64 KiB, paste skipped");
        return None;
    }
    let intent = Intent::SendInput(SendInputSpec {
        pane,
        text,
        delivery: Delivery::Pasted,
        prompt_epoch: None,
    });
    let dedup_key = format!("paste:{}", clock::now_unix_millis());
    Some(Submission::keyed(intent, dedup_key))
}
```

**TypeScript**

```ts
import * as clipboard from '@ilium/plugin-sdk/host/clipboard';
import * as clock from '@ilium/plugin-sdk/host/clock';
import { log } from '@ilium/plugin-sdk/host/log';
import type { Submission } from '@ilium/plugin-sdk/types';

const MAX_PASTE_BYTES = 64 * 1024;

/** Builds the paste submission for `pane`, or nothing when there is nothing to paste. */
export function pasteClipboard(pane: bigint): Submission | undefined {
  const text = clipboard.readText();
  if (text === undefined || text.length === 0) {
    return undefined;
  }
  if (new TextEncoder().encode(text).length > MAX_PASTE_BYTES) {
    log('warn', 'clipboard text over 64 KiB, paste skipped');
    return undefined;
  }
  return {
    intent: {
      tag: 'send-input',
      val: { pane, text, delivery: 'pasted', promptEpoch: undefined },
    },
    correlation: 0n,
    dedupKey: `paste:${clock.nowUnixMillis()}`,
  };
}
```

The caller returns the submission from `on-events` (Rust) or `onEvents` (TypeScript). The key is built from the current millisecond, so a paste of new text is not swallowed as a duplicate. Two pastes in the same millisecond share a key, and the second completes as `duplicate(original)`.

Outcome: the `send-input` submission completes as `ok` in the next batch, and the text appears at the prompt of the pane as a paste. If the pane is still restoring, the submission completes as `refused(pane-not-ready)`. A refused submission does not reserve its key, so the same key is valid for a retry.

### What the user sees

The plugin log shows one line per call. The footer confirms each read.

<pre class="term">
<span class="t-prompt">$</span> ilium plugin logs clipboard-helper
2026-10-11 12:04:11 <span class="t-ok">ok</span> clipboard.write 12 chars
2026-10-11 12:04:19 <span class="t-ok">ok</span> clipboard.read 42 chars
2026-10-11 12:04:27 <span class="t-warn">warn</span> clipboard.read declined
2026-10-11 12:04:33 <span class="t-err">error</span> clipboard.write too big
</pre>

<p><em>Illustrative, not generated.</em> The log records lengths, never the clipboard text.</p>

<pre class="term">
<span class="t-dim">[work]  Pane 3  ~/dev/ai/ilium  main</span>
<span class="t-prompt">$</span> git log --oneline -1
<span class="t-hi">clipboard read by clipboard-helper</span>
<span class="t-prompt">$</span>
</pre>

<p><em>Illustrative, not generated.</em> The footer line stays for 3 s after each read.</p>

### Errors

| Function | Code | When |
|---|---|---|
| `write-text` | `invalid-argument` | The text is over 1 MiB (1,048,576 UTF-8 bytes). Nothing is written. |
| `write-text` | `unsupported` | Neither the system clipboard nor OSC 52 is available. |
| `write-text` | `not-permitted` | The user declined `clipboard.write`. |
| `write-text` | `revoked` | The grant was revoked during the call. |
| `write-text` | `internal` | A host bug. |
| `read-text` | none | The clipboard has no text, the grant is declined or revoked, or there is no route. `read-text` has no error result. |

### Limits

| Item | Value | Error |
|---|---|---|
| `write-text` size | 1 MiB (1,048,576 UTF-8 bytes) | `invalid-argument`, nothing written |
| Text format | UTF-8 plain text | an image reads as nothing |
| Read frequency | no limit; every read is logged | none |
| Prompt for the high-risk grant | once, at first activation | none |

### Used by

- Text selection and right-click menu (feature 16) writes the selection with `clipboard.write-text`.
- Smart Copy light (feature 65) writes the selection on key release. It uses `clipboard.write-text` with a grab from the keys import.

### See also

- [keys](/pages/api-keys/) for the grab used by Smart Copy light.
- [terminal primitive](/pages/primitives-terminal/) for the selection, the screen and `send-input`.
- [intents](/pages/api-intents/) for `send-input` and the submission rules.
- [clock and timers](/pages/api-clock-and-timers/) for `now-unix-millis`.
- [client requests](/pages/api-client-requests/) for reading the clipboard from the server side.
- [capability: clipboard](/pages/capability-clipboard/) for the grants, the logging and the review dialog.
- [errors and limits](/pages/reference-errors-and-limits/) for the trap and restart rules.

Contract: `ilium:host/clipboard`. The server side reaches it through client requests.

### Security

**[SEC-044]** Clipboard reads need the high-risk `clipboard.read` grant. See [Secrets](/pages/reference-secrets/).
