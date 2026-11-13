# Capability: clipboard

## clipboard.read and clipboard.write

The `clipboard.read` and `clipboard.write` grants let a client plugin read and write plain text on the system clipboard. Writing is common and low risk. Reading is high risk, because the clipboard often holds passwords and tokens that the user copied from another program. Each direction is a separate grant, and a plugin asks only for the direction it uses.

The grants cover one import, `clipboard`, which belongs to the client world. A server plugin cannot call it, because the clipboard belongs to the user's desktop session.

<figure>
<svg viewBox="0 0 400 170" role="img" aria-label="Client plugin calls write-text and read-text on the host clipboard, each under its own grant">
  <defs>
    <marker id="clip-arrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
      <path d="M0,0 L8,4 L0,8 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="10" y="30" width="130" height="100" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="75" y="58" text-anchor="middle" fill="currentColor" font-size="12">Client plugin</text>
  <text x="75" y="76" text-anchor="middle" fill="currentColor" font-size="10">com.example.notes</text>
  <rect x="260" y="30" width="130" height="100" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="325" y="58" text-anchor="middle" fill="currentColor" font-size="12">Host clipboard</text>
  <text x="325" y="76" text-anchor="middle" fill="currentColor" font-size="10">client side</text>
  <line x1="142" y1="70" x2="256" y2="70" stroke="currentColor" marker-end="url(#clip-arrow)"/>
  <text x="199" y="62" text-anchor="middle" fill="currentColor" font-size="11">write-text</text>
  <text x="199" y="84" text-anchor="middle" fill="currentColor" font-size="10">clipboard.write</text>
  <line x1="256" y1="110" x2="142" y2="110" stroke="currentColor" marker-end="url(#clip-arrow)"/>
  <text x="199" y="102" text-anchor="middle" fill="currentColor" font-size="11">read-text</text>
  <text x="199" y="124" text-anchor="middle" fill="currentColor" font-size="10">clipboard.read</text>
  <text x="200" y="158" text-anchor="middle" fill="currentColor" font-size="10">Each direction needs its own grant.</text>
</svg>
<figcaption>The plugin calls write-text, which needs clipboard.write, and read-text, which needs
clipboard.read. A plugin that only copies text holds clipboard.write alone.</figcaption>
</figure>

## You will need

| Need | Value |
|---|---|
| Side | Client. The import is part of `ilium:client/base@0.1.0`. |
| Grant to write | `clipboard.write`, normal risk. Checked by default in the review dialog. |
| Grant to read | `clipboard.read`, high risk (H). Unchecked by default, and the user checks it. |
| Rust | Crate `ilium-plugin-sdk`, Rust stable 1.90 or later, target `wasm32-wasip2`. |
| TypeScript | Package `@ilium/plugin-sdk`, Bun 1.2 or later. `bun install` adds the toolchain. |
| Text format | UTF-8 plain text. Images and other formats are not supported. |

Import the module in each language:

**Rust**

```rust
use ilium_plugin_sdk::host::clipboard;
```

**TypeScript**

```ts
import * as clipboard from '@ilium/plugin-sdk/host/clipboard';
```

## Grant syntax

A clipboard grant takes no argument. The name is the whole grant string.

| Grant | Valid | Reason |
|---|---|---|
| `clipboard.read` | yes | The read direction. High risk. |
| `clipboard.write` | yes | The write direction. Normal risk. |
| `clipboard` | no | The direction is missing. The manifest is refused. |
| `clipboard.read:text` | no | The grant takes no argument. The manifest is refused with `manifest.grant-argument`. |

## The surface

The import has two functions and no records.

| WIT function | Rust | TypeScript | Grant | Returns | Meaning |
|---|---|---|---|---|---|
| `read-text` | `clipboard::read_text() -> Option<String>` | `clipboard.readText(): string \| undefined` | `clipboard.read` | `option<string>` | The clipboard text, or nothing when the clipboard holds no text. |
| `write-text` | `clipboard::write_text(text: &str) -> Result<(), PluginError>` | `clipboard.writeText(text: string): void`, throws `PluginError` | `clipboard.write` | `result<_, plugin-error>` | Replaces the clipboard text with `text`. |

Both functions are synchronous. They return at once and never return a request id. The names
follow the plugin binding rules: kebab-case becomes snake_case in Rust and
camelCase in TypeScript.

## What the user sees

The review dialog lists one row per grant, in the Client group. The `clipboard.read` row carries
the HIGH mark. A T2 or T3 package shows this dialog at install. A T1 bundled plugin is activated
first without its high-risk grants, and the user sees one prompt per plugin at first activation.

<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.clipboard 0.1.0 by Example Ltd                                 │
│                                                                            │
│ <span class="t-hi">Client side</span>                                                                │
│ [x] Writes to the clipboard <span class="t-dim">………………………………………………………………………………</span> clipboard.write │
│ <span class="t-warn">! [ ] Reads the clipboard</span> <span class="t-dim">………………………………………………………………………………………</span> <span class="t-warn">clipboard.read</span> │
│                                                                            │
│ <span class="t-warn">! marks a high-risk grant. It starts unchecked and is</span>                      │
│ <span class="t-warn">always asked for, even for bundled plugins.</span>                                │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>

The client never asks again on each read. A successful read writes an info line to the plugin
log that records only the length of the text, never the text itself. The same line goes to the
privacy log. The client footer also shows the read for 3 s:

<pre class="term"><span class="t-dim">Client footer, 3 seconds after a successful read</span>
<span class="t-hi">clipboard read by Example Notes</span>
</pre>

## Routes

The client chooses a route for each write. The route decides whether the text reaches the desktop
clipboard at all.

<figure>
<svg viewBox="0 0 400 180" role="img" aria-label="Route chosen by the client for a clipboard write">
  <defs>
    <marker id="clip-route" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
      <path d="M0,0 L8,4 L0,8 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="10" y="16" width="200" height="30" rx="4" fill="none" stroke="currentColor"/>
  <text x="110" y="35" text-anchor="middle" fill="currentColor" font-size="11">Client runs on the desktop?</text>
  <line x1="210" y1="31" x2="248" y2="31" stroke="currentColor" marker-end="url(#clip-route)"/>
  <text x="229" y="26" text-anchor="middle" fill="currentColor" font-size="10">yes</text>
  <rect x="250" y="16" width="140" height="30" rx="4" fill="none" stroke="currentColor"/>
  <text x="320" y="35" text-anchor="middle" fill="currentColor" font-size="11">System clipboard</text>
  <line x1="110" y1="46" x2="110" y2="70" stroke="currentColor" marker-end="url(#clip-route)"/>
  <text x="118" y="62" fill="currentColor" font-size="10">no</text>
  <rect x="10" y="72" width="200" height="30" rx="4" fill="none" stroke="currentColor"/>
  <text x="110" y="91" text-anchor="middle" fill="currentColor" font-size="11">Terminal supports OSC 52?</text>
  <line x1="210" y1="87" x2="248" y2="87" stroke="currentColor" marker-end="url(#clip-route)"/>
  <text x="229" y="82" text-anchor="middle" fill="currentColor" font-size="10">yes</text>
  <rect x="250" y="72" width="140" height="30" rx="4" fill="none" stroke="currentColor"/>
  <text x="320" y="91" text-anchor="middle" fill="currentColor" font-size="11">OSC 52 sequence</text>
  <line x1="110" y1="102" x2="110" y2="126" stroke="currentColor" marker-end="url(#clip-route)"/>
  <text x="118" y="118" fill="currentColor" font-size="10">no</text>
  <rect x="10" y="130" width="380" height="36" rx="4" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
  <text x="200" y="152" text-anchor="middle" fill="currentColor" font-size="11">No route: write unsupported, read none</text>
</svg>
<figcaption>The write route. The local system clipboard comes first, then OSC 52. With neither, a
write returns unsupported and a read returns nothing.</figcaption>
</figure>

## Behaviour, edge cases and errors

| Situation | `write-text` | `read-text` |
|---|---|---|
| Grant held, text on the clipboard | Writes the text. Returns `ok`. | Returns the text. |
| Clipboard holds no text (an image, or nothing) | Not applicable. | Returns `none`. No error. |
| Empty string written | Accepted. Replaces the clipboard text with an empty text. Returns `ok`. | Returns an empty string. |
| Text larger than 1 MiB (1,048,576 UTF-8 bytes) | `invalid-argument`. Nothing is written. The text is never truncated. | Not applicable. |
| No route on this client | `unsupported`. | Returns `none`. |
| Grant declined by the user | `not-permitted`. | Returns `none`. A warn line goes to the plugin log. |
| Grant revoked during the call | `revoked`. The plugin is re-activated with the reduced grant set. | Returns `none`, because `read-text` has no error result. A warn line goes to the plugin log. |
| Grant never requested in the manifest | The plugin traps and counts as a failure. | The plugin traps and counts as a failure. |

Rules that follow from the table:

- A declined or revoked read is not reported as an error. The plugin cannot tell "no text" from
  "not allowed" by the return value alone. The warn line in the plugin log is the only record of
  a refused read.
- `write-text` counts the 1 MiB limit in UTF-8 bytes, not in characters. A text of 300,000
  characters that take four bytes each is 1.2 MB, so it is refused.
- A revoked grant does not make the plugin fail. Revocation re-activates the plugin, as described
  in the grant lifecycle.

## Example 1: copy a note link (Rust)

The manifest requests only the write direction. The plugin copies a link to a note when the user
runs the `copy-note-link` command.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.notes",
  "name": "Example Notes",
  "version": "0.1.0",
  "publisher": "Example Corp",
  "client": {
    "entry": "client.wasm",
    "world": "ilium:client/base@0.1.0",
    "exports": []
  },
  "capabilities": { "client": ["clipboard.write"] }
}
```

**Rust**

```rust
use ilium_plugin_sdk::host::clipboard;
use ilium_plugin_sdk::prelude::*;

const NOTE_LINK_PREFIX: &str = "notes://";

/// Runs when the user invokes the `copy-note-link` command.
/// `note_path` comes from the command arguments, for example "today/weekly-plan".
fn copy_note_link(note_path: &str) -> Result<(), PluginError> {
    let link = format!("{NOTE_LINK_PREFIX}{note_path}");
    // Errors (not-permitted, unsupported, invalid-argument) pass to the caller.
    clipboard::write_text(&link)
}
```

The command handler maps `Err(PluginError)` to a message in the plugin's own pane, so the user
can copy the link by hand when the write fails.

## Example 2: read a note link and handle a missing grant (TypeScript)

This plugin reads the clipboard to open a note that the user copied from elsewhere. It needs both
directions, because it also copies links. The read grant is high risk, so the manifest must say
why the plugin needs it in its `description`.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.notes",
  "name": "Example Notes",
  "version": "0.1.0",
  "publisher": "Example Corp",
  "description": "Opens a note whose link you copied. Reads the clipboard only when you ask it to.",
  "client": {
    "entry": "client.wasm",
    "world": "ilium:client/base@0.1.0",
    "exports": []
  },
  "capabilities": { "client": ["clipboard.read", "clipboard.write"] }
}
```

**TypeScript**

```ts
import { PluginError } from '@ilium/plugin-sdk';
import * as clipboard from '@ilium/plugin-sdk/host/clipboard';

const NOTE_LINK_PREFIX = 'notes://';

/** Returns the note path on the clipboard, or undefined when no note link is there. */
export function noteFromClipboard(): string | undefined {
  // undefined covers an empty clipboard, an image, a declined grant and a revoked grant.
  const text = clipboard.readText();
  if (text === undefined) {
    return undefined;
  }
  const link = text.trim();
  if (!link.startsWith(NOTE_LINK_PREFIX)) {
    return undefined;
  }
  return link.slice(NOTE_LINK_PREFIX.length);
}

/** Copies a note link and reports the outcome, so the caller can fall back to a visible link. */
export function copyNoteLink(notePath: string): 'copied' | 'unsupported' | 'not-permitted' {
  try {
    clipboard.writeText(`${NOTE_LINK_PREFIX}${notePath}`);
    return 'copied';
  } catch (error) {
    if (error instanceof PluginError && error.code === 'unsupported') {
      return 'unsupported';
    }
    if (error instanceof PluginError && error.code === 'not-permitted') {
      return 'not-permitted';
    }
    throw error;
  }
}
```

Error codes arrive as kebab-case strings in TypeScript. A `PluginError` carries `code`, `message`
and `retryAfterMs` (present only for rate and quota errors). Any other thrown value is a bug, so
the example rethrows it.

## Who requests it

Among the first-party plugins in the feature map, three request `clipboard.write`: the text
selection and right-click menu, and the two Smart Copy plugins (light and AI).
No first-party plugin requests `clipboard.read`.

## See also

- [terminal.read-screen](/pages/capability-terminal-read-screen/) for the screen text that Smart
  Copy works from.
- [help.read](/pages/capability-help-read/) for topics that explain the copy shortcuts.
- [Errors and limits](/pages/reference-errors-and-limits/) for the error codes and the trap rule.
- [clipboard API](/pages/api-clipboard/) for the full import reference.

### Security

**[SEC-044]** Clipboard reads are high risk and are logged to the privacy log; a write never reads. See [Secrets](/pages/reference-secrets/).
