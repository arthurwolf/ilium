---
layout: page
title: Pane primitives
permalink: /pages/primitives/
---

## Pane primitives

A pane holds one piece of content: a shell, an agent session, a text buffer or a widget tree. A primitive is the host-provided kind of pane that a plugin builds on. Ilium has four primitives: `terminal`, `agent`, `editor` and `basic`. Each is a WIT interface in the package `ilium:primitives`. The kernel keeps the per-byte and per-keystroke work. A plugin reads state, adds rules and decorations, and, for a `basic` pane, supplies the whole pane body.

A plugin imports the interface, never the implementation behind it. The implementation can move later, for example the editor after version 1, without changing the plugin's imports.

## You will need

- Grants in the manifest `capabilities` block for each call you make (table below). A declined grant links the import to a stub, and calls through it return `not-permitted`.
- Rust 1.90 or later with the `wasm32-wasip2` target, or Bun 1.2 or later for TypeScript. See [Toolchain](/pages/getting-started-toolchain/).
- The side that owns the call. The server world imports `terminal` and `agent`. The client world imports `editor` and `basic`. Terminal reads work on both sides.
- The model in [Architecture](/pages/architecture/) and [The UI model](/pages/ui-model/).

## The four primitives

In the tables below, S is the server side and C is the client side.

| Primitive | Interface | Sides | The kernel owns | Plugins add | Grants |
|---|---|---|---|---|---|
| `terminal` | `ilium:primitives/terminal` | S and C (read, highlights), S (filters), C (pointer) | PTY, vt100 screen, scrollback, selection, paste, mouse | screen reads, output filters, highlight layers, input | `terminal.read-screen`, `terminal.output-filter`, `terminal.pointer`, `terminal.write` |
| `agent` | `ilium:primitives/agent` | S and C (read), S (signatures) | process-tree walk, poll schedule, signatures, activity state, toolbar strip | agent and prompt reads, signatures, toolbar buttons, actions | `agent.read`, `agent.read-prompts`, `agent.signatures`, `agent.control` |
| `editor` | `ilium:primitives/editor` | C | the native editor: buffers, undo, file custody, 1 s autosave, highlighting | buffer reads, decorations, gutter marks, edits, file loads and saves | `editor.read`, `editor.edit` |
| `basic` | `ilium:primitives/basic` | C | the generic frame: focus, scrollbars, split geometry, key and mouse routing | title, footer, focus, size, and the body from `views.render` | `panes.provide-kind` |

`pane-info` reports `primitive` as `terminal`, `editor` or `basic`. An agent pane is a terminal pane whose process a signature identifies, so the `agent` interface adds no pane type.

<figure>
<pre class="term">╭──≡ ● · terminal───────────╮ ╭──≡ ● · agent──────────────╮
│<span class="t-prompt">$</span> cargo test               │ │🦀 <span class="t-hi">⠋</span> Fix auth              │
│running 14 tests           │ │goal: ship v0              │
│test result: ok            │ │                           │
╰───────────────────────────╯ ╰───────────────────────────╯
╭──≡ ● · editor─────────────╮ ╭──≡ ● · basic──────────────╮
│fn main() {                │ │Notes                      │
│    run();                 │ │Rows below come from the   │
│}                          │ │plugin view tree           │
╰───────────────────────────╯ ╰───────────────────────────╯
</pre>
<figcaption>The four primitives, each in a rounded Ilium panel with the chrome title. Every border in Ilium is rounded.</figcaption>
</figure>

## Further capabilities per primitive

Each primitive has further capabilities. Each has its own page with signatures, Rust and TypeScript examples, errors and limits.

| Primitive | Capability | Page | Grants |
|---|---|---|---|
| `terminal` | cell attributes, frozen snapshot, history text and search, modes, foreground process, `screen-changed`, `user-line-submitted` | [Terminal reads](/pages/primitives-terminal-reads/) | `terminal.read-cells`, `terminal.history` |
| `terminal` | sub-cell pointer, `move`, modifier gestures, drag-over | [Terminal pointer](/pages/primitives-terminal-pointer/) | `terminal.pointer` |
| `terminal` | `send-input` stages, raw bytes, mouse, preconditions, input lock, journal, interactive `terminal-view` | [Terminal input](/pages/primitives-terminal-input/) | `terminal.raw-input`, `terminal.input-lock` |
| `agent` | model fact, prompt epoch, typed transcript, `signal-changed`, `agent-model-changed` | [Agent transcript and signals](/pages/primitives-agent-transcript/) | `agent.transcript` |
| `editor` | viewport, pointer, minimap marks, token provider | [Editor viewport and tokens](/pages/primitives-editor-viewport/) | none new |
| `basic` | `report-activity`, `set-dirty` | [Basic primitive](/pages/primitives-basic/) | none new |

## Basic: a pane kind on the basic primitive

A plugin declares a pane kind under `pane_kinds` in its manifest. Its body is the plugin's `views.render` export for that pane.

```json
"pane_kinds": [
  {
    "id": "notes",
    "title": "Notes",
    "icon": "notes",
    "primitive": "basic",
    "wants_keys": true,
    "persist": "metadata"
  }
]
```

| Field | Meaning |
|---|---|
| `id` | Local id. The content kind of its panes is `<plugin-id>.<id>`, here `com.example.notes.notes`. |
| `primitive` | Always `"basic"`. |
| `wants_keys` | When true, keys the focused widget does not use arrive as `key-pressed` for the pane. |
| `persist` | Required, no default. `metadata` keeps state in node metadata. `storage` keeps it in session-bucket keys under `pane/<node-id>/`, deleted when the pane closes. `none` recreates the pane empty after a restart. |

| Rust, `primitives::basic` | TypeScript, `/primitives/basic` | Type | Meaning |
|---|---|---|---|
| `set_title(pane, title)` | `setTitle` | `result<_, plugin-error>` | Title row. Over 256 characters: `invalid-argument`. |
| `set_status(pane, status)` | `setStatus` | `result<_, plugin-error>` | One-line footer, as `styled-text`. |
| `request_focus(pane)` | `requestFocus` | `result<_, plugin-error>` | Focuses the pane. |
| `size(pane)` | `size` | `option<tuple<u16, u16>>` | Inner columns and rows. `none` for a node that is not the plugin's kind. |

Basic panes have no process, so `pane-not-ready` never applies. The three functions that change the frame return `not-permitted` for any node that is not one of the plugin's kinds.

<div class="warn">
<p>Basic functions exist only in the client world. A server build that imports them fails to compile.</p>
</div>

### Example: a notes pane in Rust

This client plugin sets the title and footer of each notes pane when the pane is created. Its manifest needs `panes.read` (for `pane-created`) and `panes.provide-kind`.

```rust
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::primitives::basic;
use ilium_plugin_sdk::types::tree::PaneInfo;

const NOTES_KIND: &str = "com.example.notes.notes";

#[ilium_plugin(side = "client")]
#[derive(Default)]
struct NotesPanes {
    created: u32,
}

impl Lifecycle for NotesPanes {}

impl Handler for NotesPanes {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::PaneCreated(pane) = event else { continue };
            if pane.content_kind == NOTES_KIND {
                self.decorate(pane);
            }
        }
        Vec::new()
    }
}

impl NotesPanes {
    fn decorate(&mut self, pane: PaneInfo) {
        self.created += 1;
        if let Err(error) = basic::set_title(pane.node, "Notes") {
            log::log(Level::Warn, &format!("set_title refused: {}", error.message));
            return;
        }
        let footer = format!("{} notes open in this session", self.created);
        let status = vec![Span::plain(footer.as_str())];
        if let Err(error) = basic::set_status(pane.node, status) {
            log::log(Level::Warn, &format!("set_status refused: {}", error.message));
        }
    }
}
```

## Terminal: screen, filters, highlights and input

| Rust / TypeScript | Grant | Returns and rules |
|---|---|---|
| `read_screen(pane, part, max_rows)` / `readScreen` | `terminal.read-screen` | `option<Screen>`. `part` is `visible` or `scrollback`. `max_rows` above 10,000 is clamped. |
| `selection(pane)` / `selection` | `terminal.read-screen` | `option<Selection>`: text, first and last row and column. |
| `add_output_filter(pane, filter)` / `addOutputFilter` | `terminal.output-filter` (S) | `result`. Regex at most 512 bytes, rate 1 to 100 per second. An existing id on the pane is replaced. |
| `remove_output_filter(pane, id)` / `removeOutputFilter` | `terminal.output-filter` (S) | No result. |
| `claim_pointer(pane)` / `claimPointer` | `terminal.pointer` (C) | `result`. `conflict` when another plugin holds the claim. |
| `release_pointer(pane)` / `releasePointer` | `terminal.pointer` (C) | No result. |
| `set_highlights(pane, layer, regions)` / `setHighlights` | `terminal.pointer` | `result`. Layer names match `[a-z0-9-]{1,32}`. At most 4,096 regions per layer, 16 layers per pane. |
| `clear_highlights(pane, layer)` / `clearHighlights` | `terminal.pointer` | No result. |
| `prompt_epoch(pane)` / `promptEpoch` | `terminal.write` | `option<u64>`: the count of submitted prompts. |

A `Screen` holds `columns`, `rows` (one string per row), `cursor-row`, `cursor-column`, `title` and a `revision`. Filter matches arrive as `output-matched` events with `node`, `filter-id`, `line` and `row`. Surplus matches above a filter's rate are dropped and counted in one `events-dropped` event. A highlight region outside the screen is clipped silently. A region whose first position comes after its last is `invalid-argument`.

Input is the intent `send-input`: `pane`, `text` (at most 64 KiB), `delivery` (`typed`, `submitted` or `pasted`), an optional `prompt-epoch` and a mandatory `dedup-key`. The kernel checks in this order and stops at the first failure:

1. Shape, key and text size. Failure refuses the batch with `invalid-argument`.
2. The `terminal.write` grant. Failure refuses the batch with `not-permitted`.
3. The dedup key. A reserved key completes as `duplicate(original)`.
4. The pane exists and is a terminal pane. Failure is `not-found` or `unsupported`.
5. The pane is restored. A restored pane still waiting for its process gives `pane-not-ready`.
6. The rate. Failure is `rate-limited`.
7. The prompt epoch, if given, matches the pane. Otherwise `conflict`.
8. `mouse`: reporting is enabled. Otherwise `unsupported`.
9. The `if-current` precondition, if given, matches the pane. Otherwise `conflict` with the message `precondition: <field>`.
10. No other plugin holds the input lock of the pane. Otherwise `conflict` with the message `input-locked`.
11. The key is reserved and the text is written.

A refused submission does not reserve its key, so a retry with the same key after fixing the cause is valid.

### Example: a build-error filter in TypeScript

This server plugin adds an output filter to each new terminal pane and reads the visible screen on a match. Its manifest needs `panes.read`, `terminal.output-filter` and `terminal.read-screen`.

```ts
import { definePlugin, PluginError } from '@ilium/plugin-sdk';
import type { Event, Submission } from '@ilium/plugin-sdk/types';
import { addOutputFilter, readScreen } from '@ilium/plugin-sdk/primitives/terminal';
import { log } from '@ilium/plugin-sdk/host/log';

const FILTER_ID = 'build-errors';
const ERROR_PATTERN = 'error(\\[E[0-9]+\\])?:';

function installFilter(pane: bigint): void {
  try {
    addOutputFilter(pane, { id: FILTER_ID, regex: ERROR_PATTERN, ratePerSecond: 5 });
  } catch (error) {
    if (error instanceof PluginError) {
      log('warn', `filter refused for pane ${pane}: ${error.code}, ${error.message}`);
    }
  }
}

function reportErrorTail(pane: bigint): void {
  const screen = readScreen(pane, 'visible', 40);
  if (screen === undefined) return;
  const lastRow = screen.rows[screen.rows.length - 1] ?? '';
  log('info', `pane ${pane} shows an error at screen revision ${screen.revision}: ${lastRow}`);
}

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'pane-created' && event.val.primitive === 'terminal') {
        installFilter(event.val.node);
      }
      if (event.tag === 'output-matched' && event.val.filterId === FILTER_ID) {
        reportErrorTail(event.val.node);
      }
    }
    return [];
  },
});
```

Adding a filter with an id the pane already has replaces it, so the handler is safe to run again after a restart.

## Agent: agents, signatures and actions

| Rust / TypeScript | Grant | Returns and rules |
|---|---|---|
| `agents()` / `agents` | `agent.read` | One `{ pane, view }` per identified agent. An empty list when declined. |
| `view(pane)` / `view` | `agent.read` | `option<AgentView>`: `signature`, `activity`, `objective`, `goal`. |
| `session(pane)` / `session` | `agent.read` | `option<SessionRef>`: `scope`, `path`, `session-id`, inside `agent-sessions:<agent>`. |
| `last_prompt(pane)` / `lastPrompt` | `agent.read-prompts` | `option<string>`. Synchronous. |
| `register_signature(spec)` / `registerSignature` | `agent.signatures` (S) | `result`. Same validation as manifest signatures. |
| `set_toolbar_state(button, pane, enabled, label)` / `setToolbarState` | `agent.control` | `result`. A button that is not the plugin's gives `not-found`. A label over 32 cells gives `invalid-argument`. |

Activity values are `none`, `working`, `idle`, `awaiting-approval`, `done` and `menu`. In Rust they are `AgentActivity::AwaitingApproval` and so on. In TypeScript they are strings such as `'awaiting-approval'`.

A signature names an agent CLI. It has `id` (1 to 64 characters from `[a-z0-9-]`), `process-names` (1 to 32), `aliases` (at most 32), `icon` (a Tabler icon name), `resume` (an argv template with `{session}`), `actions` (at most 32), `session-scope` and `submit-keys`. A plugin signature has the global id `<plugin-id>.<id>`. When two origins match one process name, a user signature wins over a plugin signature, which wins over a built-in one (`claude`, `codex`, `antigravity`).

An action is the intent `agent-action`. It sends the key sequence the signature declares for the action id and needs a `dedup-key`. By default an action runs only in the states `idle`, `done`, `awaiting-approval` or `menu`. A `compact` action sent while the agent is `working` completes as `refused(conflict)` with `agent is working`.

<figure>
<pre class="term">▼ 📂 Ilium
›   🦀 <span class="t-hi">⠋</span> Fix auth      <span class="t-dim">(agent pane, working)</span>
›   🖥️    Shell
</pre>
<figcaption>How an agent pane appears in the tree while it works: the identity icon, then the spinner in the activity slot, then the title.</figcaption>
</figure>

## Editor: buffers, decorations and edits

The editor is native. Plugins reach it through the interface that a later editor plugin will also provide.

| Rust / TypeScript | Grant | Returns and rules |
|---|---|---|
| `buffers()` / `buffers` | `editor.read` | `list<BufferRef>`: `buffer`, `pane`, `path`, `language`, `revision`, `line-count`, `dirty`. |
| `buffer_for_pane(pane)` / `bufferForPane` | `editor.read` | `option<BufferRef>`. |
| `read_lines(buffer, first_line, count)` / `readLines` | `editor.read` | `result<list<string>>`. `count` over 10,000 or a line past the end: `invalid-argument`. |
| `selections(buffer)` / `selections` | `editor.read` | `list<TextRange>`, columns in Unicode scalar values. |
| `set_decorations(buffer, layer, items)` / `setDecorations` | `editor.read` | `result`. Items hold a `range`, a `style` and an optional `hover`. |
| `set_gutter(buffer, layer, marks)` / `setGutter` | `editor.read` | `result`. Marks hold a `line`, an `icon`, a `tooltip` and a `command`. |
| `create_buffer(initial, language)` / `createBuffer` | `editor.edit` | `result<buffer-id>`. Up to 16 MiB, 64 buffers per plugin. |
| `load_file(path)` / `loadFile` | `editor.edit`, `fs.read` | Request id, then `take_buffer(request)` gives the buffer id. |
| `save_buffer(buffer, path)` / `saveBuffer` | `editor.edit`, `fs.write` | Request id. `conflict` when the file changed on disk since load. |
| `close_buffer(buffer)` / `closeBuffer` | `editor.edit` | No result. |

Edits go through the intent `editor-apply-edits` with a base revision. A stale revision gives `conflict`. Decorations and gutter layers belong to the plugin that set them and are cleared when it stops.

<div class="note">
<p>The editor provider is reserved and is not part of <code>ilium:plugin@0.1.0</code>. The editor is a built-in pane kind, the grant <code>editor.provide</code> is refused with <code>manifest.grant-unknown</code>, and no <code>editor-provider</code> interface exists in the WIT reference. After the freeze only additive changes are allowed. See <a href="/pages/versioning/">Versioning</a>.</p>
</div>

<figure>
<svg viewBox="0 0 420 232" role="img" aria-label="Editor primitive routing now and after the move">
  <defs>
    <marker id="primitives-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto">
      <path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <text x="10" y="16" font-size="11" fill="currentColor">Now</text>
  <rect x="8" y="24" width="112" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="64" y="45" font-size="11" text-anchor="middle" fill="currentColor">Plugin caller</text>
  <line x1="120" y1="41" x2="150" y2="41" stroke="currentColor" marker-end="url(#primitives-arrow)"/>
  <rect x="150" y="24" width="262" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
  <text x="281" y="40" font-size="11" text-anchor="middle" fill="currentColor">ilium:primitives/editor</text>
  <text x="281" y="55" font-size="10" text-anchor="middle" fill="currentColor">service registry</text>
  <line x1="281" y1="64" x2="281" y2="84" stroke="currentColor" marker-end="url(#primitives-arrow)"/>
  <rect x="150" y="84" width="262" height="32" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="281" y="104" font-size="11" text-anchor="middle" fill="currentColor">Native editor (kernel)</text>
  <text x="10" y="146" font-size="11" fill="currentColor">After the move</text>
  <rect x="8" y="154" width="112" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="64" y="175" font-size="11" text-anchor="middle" fill="currentColor">Plugin caller</text>
  <line x1="120" y1="171" x2="150" y2="171" stroke="currentColor" marker-end="url(#primitives-arrow)"/>
  <rect x="150" y="154" width="262" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
  <text x="281" y="170" font-size="11" text-anchor="middle" fill="currentColor">ilium:primitives/editor</text>
  <text x="281" y="185" font-size="10" text-anchor="middle" fill="currentColor">same interface, new provider</text>
  <line x1="281" y1="194" x2="281" y2="208" stroke="currentColor" marker-end="url(#primitives-arrow)"/>
  <rect x="150" y="208" width="262" height="20" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="281" y="222" font-size="10" text-anchor="middle" fill="currentColor">Editor plugin (after version 1)</text>
</svg>
<figcaption>The caller imports the same interface before and after the move. Only the provider changes.</figcaption>
</figure>

The editor stays native because every keystroke passes through it, and it keeps the undo history, file custody and autosave. A plugin call per keystroke would cross the plugin boundary every time. The `code-view` widget therefore edits buffers natively.

## Edge cases and errors

| Situation | Result |
|---|---|
| Basic frame call on a node that is not the plugin's kind | `not-permitted`. `size` returns `none`. |
| Undeclared pane kind in `create-pane` | `invalid-argument`. |
| Restored terminal or agent pane still waiting for its process | `pane-not-ready`. Retry after `pane-created` or `tree-changed` for it. |
| Pane, node or buffer closed during a call | `not-found`. |
| Invalid filter regex, rate 0 or over 100 | `invalid-argument`. |
| Layers, filters or buffers over quota | `quota-exceeded`. |
| `send-input` with a reserved key | `duplicate(original)`. No second write. |
| `send-input` with a stale prompt epoch | `conflict`. Read `prompt_epoch` again. |
| `agent-action` while working | `refused(conflict)`, `agent is working`. |
| Edit on a stale revision, or save after an outside change | `conflict`. Read the state again. |
| Filter matches above the rate | Dropped and counted in `events-dropped`. |

Every function that returns a result can also return `not-permitted`, `revoked` and `internal`. See [Errors and limits](/pages/reference-errors-and-limits/).

## What the user sees

- A basic pane shows the title in its title row, the footer from `set_status`, and the body from `views.render`.
- Highlight layers paint their regions over the terminal rows. Output filters have no visible effect; they only feed the plugin.
- An agent pane shows its toolbar buttons. `set_toolbar_state` turns each button on or off.
- Editor decorations and gutter marks appear at once and disappear when the plugin stops.
- A refused call returns an error to the plugin. The user sees a message only when the plugin shows one.

## See also

- [Terminal reads](/pages/primitives-terminal-reads/), [Terminal pointer](/pages/primitives-terminal-pointer/), [Terminal input](/pages/primitives-terminal-input/), [Agent transcript](/pages/primitives-agent-transcript/) and [Editor viewport](/pages/primitives-editor-viewport/)
- [Basic primitive](/pages/primitives-basic/), [Agent primitive](/pages/primitives-agent/), [Editor primitive](/pages/primitives-editor/) and [Terminal primitive](/pages/primitives-terminal/)
- [Panes API](/pages/api-panes/) and [Widgets index](/pages/widgets-index/)
- [Events and intents](/pages/events-and-intents/) and [Capabilities](/pages/capabilities/)
- [File access](/pages/file-access/) and [Services](/pages/services/)
- [Rust SDK](/pages/rust-sdk/) and [TypeScript SDK](/pages/typescript-sdk/)

API version checked: `ilium:plugin@0.1.0`.

Requirements for this page: [Primitives requirements](/pages/reference-requirements-primitives/).
