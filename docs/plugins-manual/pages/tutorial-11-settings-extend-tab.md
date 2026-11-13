---
layout: page
title: "11. Add settings to an existing tab"
permalink: /pages/tutorial-11-settings-extend-tab/
---

## 11. Add settings to an existing tab

Tutorial 10 gave a plugin a tab of its own. This tutorial covers the other case: a plugin adds one section to a tab that the kernel already draws. The example plugin, `example.clipboard-history` (named "Clipboard history"), adds a block titled "Terminal capture" to the Terminal tab of Settings. The built-in Terminal rows keep their positions and values. The plugin's block sits below them, under a divider that names the plugin.

The rules are strict on purpose. A plugin can add a section at an anchor. It cannot hide, relabel, reorder or restyle a built-in row, and it cannot write a built-in value. Each step below follows from those rules. The block uses a toggle stored per project, a number that hides itself, and a secret that applies after restart. Every row type is on the [row gallery](/pages/reference-settings-rows/).

### You will need

- [Tutorial 10](/pages/tutorial-10-settings-new-tab/) finished, or any plugin with a working manifest and a client build. This step does not need the tab that tutorial 10 adds. The keys here differ from those of tutorial 10, because keys are unique across the whole plugin.
- The grant `settings.contribute:terminal`, which step 1 adds. The plugin's own values need no grant.
- The [settings extension concept](/pages/settings-extension/) for the rail and the ordering rules, and the [settings schema reference](/pages/reference-settings-schema/) for every row field.

### Step 1: declare the section in the manifest

Add one entry to `contributes.settings` with `"kind": "section"`, and add the grant to `capabilities.client`. The `tab` field names the built-in Terminal tab by its frozen id, `terminal`. Settings labels and ids are different things: the label is "Terminal", and the manifest uses the id `terminal`, which never changes even if the label does.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "example.clipboard-history",
  "name": "Clipboard history",
  "version": "1.3.0",
  "publisher": "Example",
  "description": "Keeps a searchable history of copied text.",
  "icon": "clipboard-list",
  "client": {
    "entry": "client.wasm",
    "world": "ilium:client/base@0.1.0",
    "exports": []
  },
  "capabilities": {
    "client": ["settings.contribute:terminal"]
  },
  "activation": ["startup"],
  "contributes": {
    "settings": [
      {
        "kind": "section",
        "id": "terminal-capture",
        "tab": "terminal",
        "placement": "after:main",
        "order": 500,
        "title": "Terminal capture",
        "scope": "client",
        "rows": [
          { "type": "toggle", "key": "record_panes", "title": "Record pane copies",
            "default": true, "scope": "project",
            "description": "Also store text copied from terminal panes." },
          { "type": "number", "key": "min_length", "title": "Ignore copies under",
            "minimum": 0, "maximum": 200, "unit": "chars", "default": 3,
            "visible_when": { "key": "record_panes", "equals": true },
            "description": "Short copies such as a single word are skipped." },
          { "type": "text", "key": "capture_token", "title": "Capture token",
            "secret": true, "reveal_last": 4, "max_length": 128, "default": "",
            "applies": "restart",
            "description": "Applies after restart." }
        ]
      }
    ]
  }
}
```

Each field has one job:

- `kind` set to `"section"` adds rows to a tab that already exists. A `"tab"` entry would add a new rail entry instead.
- `tab` is `terminal`. The grant must name the same id. An unknown id fails with `manifest.settings-target-unknown`. A tab that accepts no plugin sections, such as `animations`, `debug`, `api`, `about` or `setup`, fails with `manifest.settings-target-closed`.
- `placement` chooses the anchor, explained below.
- `order` (0 to 1000, default 500) breaks ties with other plugins at the same anchor.
- `scope` `"client"` means every key in the section is stored on the client side. The values go to `[plugins."example.clipboard-history"]` in `config.toml`. They never go to the `[terminal]` table.
- `rows` holds up to 24 rows per section. This section has three.
- `scope: "project"` on `record_panes` stores that one value for the open project. The project value wins while that project is open.
- `visible_when` on `min_length` hides the row when `record_panes` is off. The hidden row keeps its value.
- `secret` with `reveal_last: 4` masks `capture_token` as `••••` and shows its last four characters. `applies: "restart"` adds `↻` and the note `Applies after restart.`

The first render shows the Terminal tab before the plugin is installed. Its kernel rows are the starting point that the section must leave unchanged.

<figure>
<pre class="term">╭  ≡ ● · Terminal───────────────────────────────────────╮
│                                                       │
│  Smart Copy light key …………………………………… ← Control  +  →  │
│<span class="t-dim">    The modifier that starts Smart Copy light.         </span>│
│<span class="t-dim">  ──────────────────────                               </span>│
│  Parser pool ………………………………………………………………… -  0 MiB  + *  │
│<span class="t-dim">    No aggregate parser memory cap.                    </span>│
│<span class="t-dim">  ──────────────────────                               </span>│
╰───────────────────────────────────────────────────────╯</pre>
<figcaption>Before: the end of the Terminal tab. The kernel rows are the starting point the section must leave unchanged.</figcaption>
</figure>

An anchor is a named point in a kernel tab. Every tab has `main`, the block of kernel rows. Terminal exposes only that one anchor, so the placements that work on Terminal are:

- `"after:main"` puts the block after the last kernel row. It is the form used above. Because the anchor is named, the position stays stable if the kernel later adds sections, since kernel anchors are never renamed or removed.
- `"end"`, the default, also puts the block after the last kernel row. On Terminal the two placements give the same result. They differ on a tab with several named anchors. The Remote compaction tab exposes `behaviour`, `technique-per-agent`, `custom-prompt`, `input-shaping` and `model`, so `after:input-shaping` places a block between two kernel sections. The diagram below shows that case.

`"before:main"` is refused at install with `manifest.settings-anchor`. Kernel rows always come first, so a plugin cannot push the rows of a one-section tab down.

To use the second variant, change the placement line to `"placement": "end"`, or remove the line, since `end` is the default. The manifest is valid either way, and the Settings screen shows the same block on Terminal.

<figure>
<svg viewBox="0 0 400 280" role="img" aria-label="Plugin blocks inserted at named anchors of the Remote compaction tab">
  <defs>
    <marker id="anchor-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
      <path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
    </marker>
  </defs>
  <text x="20" y="14" font-size="11" fill="currentColor">Remote compaction tab, top to bottom</text>
  <rect x="20" y="24" width="230" height="26" rx="4" fill="none" stroke="currentColor"/>
  <text x="135" y="41" font-size="11" text-anchor="middle" fill="currentColor">behaviour (kernel)</text>
  <rect x="20" y="56" width="230" height="26" rx="4" fill="none" stroke="currentColor"/>
  <text x="135" y="73" font-size="11" text-anchor="middle" fill="currentColor">technique-per-agent (kernel)</text>
  <rect x="20" y="88" width="230" height="26" rx="4" fill="none" stroke="currentColor"/>
  <text x="135" y="105" font-size="11" text-anchor="middle" fill="currentColor">custom-prompt (kernel)</text>
  <rect x="20" y="120" width="230" height="26" rx="4" fill="none" stroke="currentColor"/>
  <text x="135" y="137" font-size="11" text-anchor="middle" fill="currentColor">input-shaping (kernel)</text>
  <rect x="20" y="158" width="230" height="30" rx="4" fill="none" stroke="currentColor" stroke-dasharray="5 3"/>
  <text x="135" y="177" font-size="11" text-anchor="middle" fill="currentColor">Plugin block (dashed)</text>
  <rect x="20" y="200" width="230" height="26" rx="4" fill="none" stroke="currentColor"/>
  <text x="135" y="217" font-size="11" text-anchor="middle" fill="currentColor">model (kernel)</text>
  <rect x="20" y="238" width="230" height="26" rx="4" fill="none" stroke="currentColor" stroke-dasharray="5 3"/>
  <text x="135" y="255" font-size="11" text-anchor="middle" fill="currentColor">Plugin block (dashed)</text>
  <line x1="254" y1="173" x2="268" y2="173" stroke="currentColor" marker-end="url(#anchor-arrow)"/>
  <text x="272" y="177" font-size="11" fill="currentColor">after:input-shaping</text>
  <line x1="254" y1="251" x2="268" y2="251" stroke="currentColor" marker-end="url(#anchor-arrow)"/>
  <text x="272" y="255" font-size="11" fill="currentColor">end (after model)</text>
</svg>
<figcaption>The kernel lays out its own rows first. Each plugin block is spliced in at its anchor, so kernel rows never move.</figcaption>
</figure>

### Step 2: read the values and react to changes

Section values live in the plugin's own store, so the plugin reads them the same way as values of its own tab. The get call returns every own key of the calling side with defaults filled in, so one call gives the plugin `record_panes`, `min_length` and `capture_token` even before the user changes anything. The section grant gives no access to Terminal's own values, such as the scrollback budget (see "What you cannot do").

The event `settings-changed` carries the changed key names, the origin and the receiving side. Reload the values whenever a key of this section is listed. The token applies after restart, so the handler ignores a change of `capture_token` until the plugin restarts. The origin tells the plugin why the write happened: `user` means a row on the Settings screen changed, `reset` means the section's `[ Reset ]` control ran, `plugin` means the plugin's own `set-json` call, and `file` means `config.toml` was edited on disk and reloaded. A project value arrives as the same event when the user opens another project.

The complete client files follow. Parse the JSON with serde in Rust, and check its shape in TypeScript, because parsed JSON is untyped.

**Rust**

```rust
// src/lib.rs. Client side of clipboard-history 1.3.0: the Terminal section.
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::host::settings;
use ilium_plugin_sdk::{ilium_plugin, Event, Handler, SettingsOrigin, Submission};
use serde::Deserialize;

/// Values of the section `terminal-capture`, keyed as in the manifest.
#[derive(Deserialize, Default, Clone)]
pub struct TerminalCapture {
    pub record_panes: bool,
    pub min_length: u32,
    pub capture_token: String,
}

pub fn load_terminal_capture() -> Result<TerminalCapture, serde_json::Error> {
    serde_json::from_str(&settings::get_json())
}

#[ilium_plugin(side = "client")]
#[derive(Default)]
struct ClipboardHistory {
    capture: TerminalCapture,
}

impl ClipboardHistory {
    fn reload(&mut self) {
        match load_terminal_capture() {
            Ok(capture) => self.capture = capture,
            Err(error) => log::log(Level::Warn, &error.to_string()),
        }
    }

    /// Decides whether one copy from a terminal pane is stored.
    fn should_store(&self, text: &str) -> bool {
        self.capture.record_panes && text.chars().count() as u32 >= self.capture.min_length
    }
}

impl Handler for ClipboardHistory {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::SettingsChanged(change) = event else { continue };
            // capture_token applies after restart, so only the other keys reload.
            let live = change.keys.iter().any(|key| key == "record_panes" || key == "min_length");
            if live {
                self.reload();
            }
            if change.origin == SettingsOrigin::Reset {
                log::log(Level::Info, "terminal capture reset from the Settings screen");
            }
        }
        Vec::new()
    }
}
```

**TypeScript**

```ts
// src/index.ts. Client side of clipboard-history 1.3.0: the Terminal section.
import { definePlugin, type Event } from '@ilium/plugin-sdk';
import { log } from '@ilium/plugin-sdk/host/log';
import { getJson } from '@ilium/plugin-sdk/host/settings';

export type TerminalCapture = {
  record_panes: boolean;
  min_length: number;
  capture_token: string;
};

function isTerminalCapture(value: unknown): value is TerminalCapture {
  if (typeof value !== 'object' || value === null) return false;
  const record = value as Record<string, unknown>;
  return (
    typeof record.record_panes === 'boolean' &&
    typeof record.min_length === 'number' &&
    typeof record.capture_token === 'string'
  );
}

export function loadTerminalCapture(): TerminalCapture | null {
  const parsed: unknown = JSON.parse(getJson());
  return isTerminalCapture(parsed) ? parsed : null;
}

let capture: TerminalCapture | null = null;

/** Decides whether one copy from a terminal pane is stored. */
export function shouldStore(text: string): boolean {
  if (!capture) return false;
  return capture.record_panes && [...text].length >= capture.min_length;
}

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  activate() {
    capture = loadTerminalCapture();
  },
  onEvents(batch: Event[]) {
    for (const event of batch) {
      if (event.tag !== 'settings-changed') continue;
      // capture_token applies after restart, so only the other keys reload.
      const live = event.val.keys.some((key) => key === 'record_panes' || key === 'min_length');
      if (live) capture = loadTerminalCapture();
      if (event.val.origin === 'reset') log('info', 'terminal capture reset from the Settings screen');
    }
    return [];
  },
});
```

#### What you see

The Terminal tab after install, with the plugin block at the bottom. The values match the defaults above.

<figure>
<pre class="term">╭  ≡ ● · Terminal───────────────────────────────────────╮
│  Parser pool ………………………………………………………………… -  0 MiB  + *  │
│<span class="t-dim">    No aggregate parser memory cap.                    </span>│
│<span class="t-dim">  ──────────────────────                               </span>│
│<span class="t-hi">▏ ⎘ CLIPBOARD HISTORY ──────────────────────── plugin ─</span>│
│<span class="t-hi">▏ Terminal capture                                     </span>│
│▏  Record pane copies ……………………………………………………………… ‹ On ›  │
│<span class="t-dim">▏    Also store text copied from terminal panes.       </span>│
│<span class="t-dim">▏  ──────────────────────                              </span>│
│▏  Ignore copies under …………………………………… -  3 chars  + *  │
│<span class="t-dim">▏    Short copies are skipped.                         </span>│
│<span class="t-dim">▏  ──────────────────────                              </span>│
│▏  Capture token ………………………………………………………………… ••••a1b2 ↻  │
│<span class="t-dim">▏    Applies after restart.                            </span>│
╰───────────────────────────────────────────────────────╯</pre>
<figcaption>After: the plugin block follows the kernel rows at <code>after:main</code>. The <code>▏</code> gutter marks every plugin line, and the divider names the plugin.</figcaption>
</figure>

Turn `Record pane copies` off. The dim `•` marks the changed value. `Ignore copies under` is hidden: `visible_when` is false, so it takes no line, keeps its value, and search skips it.

<figure>
<pre class="term">╭  ≡ ● · Terminal───────────────────────────────────────╮
│<span class="t-hi">▏ ⎘ CLIPBOARD HISTORY ──────────────────────── plugin ─</span>│
│<span class="t-hi">▏ Terminal capture                                     </span>│
│<span class="t-hi">▏  Record pane copies• ………………………………………………………… ‹ Off ›  </span>│
│<span class="t-dim">▏    Also store text copied from terminal panes.       </span>│
╰───────────────────────────────────────────────────────╯</pre>
<figcaption><code>Record pane copies</code> turned off. <code>Ignore copies under</code> is hidden by <code>visible_when</code>; its value is kept.</figcaption>
</figure>

The secret shows the last four characters, and the restart marker tells the user the new token is not live yet.

<figure>
<pre class="term">╭  ≡ ● · Terminal───────────────────────────────────────╮
│<span class="t-hi">▏ ⎘ CLIPBOARD HISTORY ──────────────────────── plugin ─</span>│
│<span class="t-hi">▏ Terminal capture                                     </span>│
│▏  Capture token ………………………………………………………………… ••••a1b2 ↻  │
│<span class="t-dim">▏    Applies after restart.                            </span>│
╰───────────────────────────────────────────────────────╯</pre>
<figcaption>The secret row. <code>reveal_last: 4</code> shows the last four characters; <code>↻</code> and the note say the value applies after restart.</figcaption>
</figure>

### Step 3: two plugins on the same tab

Several plugins can add sections to one kernel tab. The kernel orders them by a fixed rule, and install order and enable order never change the result:

1. The anchor (kernel rows first, then `after:` blocks, then `end` blocks).
2. The `order` value, ascending.
3. The plugin id, compared bytewise.
4. The declaration order inside the plugin.

Each plugin's sections at one anchor sit next to each other under one divider. Suppose a second plugin, `example.terminal-bell`, adds a "Bell" section with `"placement": "after:main"` and `"order": 200`. Its block comes first, because 200 is lower than 500:

```json
{
  "kind": "section",
  "id": "bell",
  "tab": "terminal",
  "placement": "after:main",
  "order": 200,
  "title": "Bell",
  "scope": "client",
  "rows": [
    { "type": "toggle", "key": "ring_on_copy", "title": "Ring on copy", "default": false,
      "description": "Ring the terminal bell when a pane copy is stored." }
  ]
}
```

If both plugins used `order` 500, the clipboard block would come first, because `example.clipboard-history` sorts before `example.terminal-bell`.

Users cannot reorder plugin sections. The rule is the only order, so a plugin author controls its place only through `placement` and `order`, and a second plugin cannot push the first one's rows out of its place. Two machines with the same plugins show the same Terminal tab, whatever order the plugins were installed in.

<figure>
<pre class="term">╭  ≡ ● · Terminal───────────────────────────────────────╮
│<span class="t-hi">▏ ♪ TERMINAL BELL ──────────────────────────── plugin ─</span>│
│<span class="t-hi">▏ Bell                                                 </span>│
│▏  Ring on copy …………………………………………………………………………… ‹ Off ›  │
│<span class="t-dim">▏    Ring the terminal bell when a copy is stored.     </span>│
│<span class="t-hi">▏ ⎘ CLIPBOARD HISTORY ──────────────────────── plugin ─</span>│
│<span class="t-hi">▏ Terminal capture                                     </span>│
│▏  Record pane copies ……………………………………………………………… ‹ On ›  │
│<span class="t-dim">▏    Also store text copied from terminal panes.       </span>│
╰───────────────────────────────────────────────────────╯</pre>
<figcaption>Two plugins at the same anchor. Each block has its own divider, and the lower <code>order</code> (200) comes first.</figcaption>
</figure>

### Step 4: limits

The kernel enforces these limits for sections in built-in tabs. Install refuses a manifest that exceeds a per-plugin limit with `manifest.settings-limit`:

| Item | Limit |
|---|---|
| Plugin sections per kernel tab, one plugin | 2 |
| Plugin sections per kernel tab, all plugins | 6 |
| Rows per section (a `group` counts its children) | 24 |
| Value keys per plugin | 128 |
| Choice options | 64 |
| Text value | 4,096 characters |

These limits keep the Terminal tab readable. A plugin that needs more than two sections in one tab merges its rows into fewer sections. A plugin that needs more room gives itself a tab of its own, as tutorial 10 shows.

A seventh plugin section on one tab is not drawn. The kernel drops the extra sections in ordering order and writes a load warning, so the plugin keeps running and the values stay stored. The render shows the warning as `ilium plugin logs` prints it.

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin logs example.clipboard-history
<span class="t-warn">2026-10-12 09:14:02 warn  host   example.clipboard-history</span>
    settings.section-dropped: section
    "example.clipboard-history/terminal-capture"
    in tab "terminal" is not shown: the tab
    already has 6 plugin sections
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The load warning in the log when a tab is full. Nothing is removed from the stored values.</figcaption>
</figure>

### Step 5: install and the permission prompt

Build the package, then install it with the package command. The install checks the manifest and its grants before it writes the plugin:

```bash
ilium plugin install ./example.clipboard-history.iliumpkg
```

The review dialog lists the grant in plain words. The grant `settings.contribute:terminal` appears as one line, normal risk, and the always-granted settings of the plugin's own values need no line. Approving the install lets the plugin draw sections in the Terminal tab and nothing else. It gives no read or write access to Terminal values.

<figure>
<pre class="term">╭  ≡ ● · Install Clipboard history 1.3.0?────────────────────╮
│ By Example. Keeps a searchable history of copied text.     │
│                                                            │
│ Permissions                                                │
│   Adds settings to the Terminal tab            low risk    │
│   Always granted: settings of this plugin.                 │
│                                                            │
│                          [ Cancel ]  [ Install ]           │
╰────────────────────────────────────────────────────────────╯</pre>
<figcaption>The review dialog with the grant for the Terminal tab.</figcaption>
</figure>

### Step 6: see it, search it, reset it

Open Settings, press Tab until the Terminal rail entry is selected, and scroll down past the Parser pool row. The block is drawn after the last kernel row. Press Up or Down to move between rows: the kernel handles those keys, so the plugin rows never see raw key events. A value changes at once for a toggle and after 600 ms for a number step.

Saving follows the Settings rules. A toggle or a choice writes at once. A number step writes 600 ms after the last step, so several quick presses become one write and one `settings-changed` event. Closing Settings with Esc or `✕ Close` flushes any pending write, so nothing is lost on the way out.

Press `/` and type part of a name to filter. A match inside a built-in tab shows the plugin divider above the matching row, so the owner stays visible. Esc clears the filter.

<figure>
<pre class="term">⚙ Settings                                                               ✕ Close
<span class="t-dim">/ ignore                                                        [ Guided setup ]</span>
╭  ≡ ● · Navigation╮ ╭  ≡ ● · Terminal─────────────────────────────────────────╮
<span class="t-hi">│ ❯  Terminal 1    │</span> │                                                         │
│                  │ │<span class="t-hi">▏ ⎘ CLIPBOARD HISTORY ──────────────────────── plugin ─  </span>│
│                  │ │<span class="t-hi">▏ Terminal capture                                       </span>│
│                  │ │▏  Ignore copies under …………………………………… -  3 chars  + *    │
│                  │ │<span class="t-dim">▏    Short copies are skipped.                           </span>│
│                  │ │                                                         │
╰──────────────────╯ ╰─────────────────────────────────────────────────────────╯</pre>
<figcaption>The filter with <code>ignore</code>. The rail keeps the Terminal tab with a count of 1. The content shows the matching row under its section title and the plugin divider.</figcaption>
</figure>

The `[ Reset ]` control of the section asks before it acts. A reset sends one patch that sets each key of the section to `null`, and the event arrives with the origin `reset`.

<figure>
<pre class="term">╭  ≡ ● · Reset─────────────────────────────────╮
│ Reset 3 settings of Clipboard history?       │
│                                              │
│                        [ N ]  [ Y ]          │
│ Y/N answer · Enter confirm · Esc cancel      │
╰──────────────────────────────────────────────╯</pre>
<figcaption>The Reset control of the section asks before it sets each key to its default.</figcaption>
</figure>

Now turn the plugin off in Settings > Plugins. Its rows stay visible and keep their values, but they are dimmed and read-only, and the divider says `plugin · off`. Rows remain focusable so that `?` still opens their help.

<figure>
<pre class="term">╭  ≡ ● · Terminal───────────────────────────────────────╮
│<span class="t-hi">▏ ⎘ CLIPBOARD HISTORY ────────────────── plugin · off ─</span>│
│<span class="t-hi">▏ Terminal capture                                     </span>│
│▏  Record pane copies ……………………………………………………………… ‹ On ›  │
│<span class="t-dim">▏    Read-only while the plugin is turned off.         </span>│
╰───────────────────────────────────────────────────────╯</pre>
<figcaption>The plugin turned off. The values are kept, and the rows are dimmed and read-only.</figcaption>
</figure>

The `▏` gutter marks every line of the plugin block. The gutter stays on screen when the user scrolls past the divider, so the owner of the rows is always visible.

### Before and after

| Aspect | Before the plugin | After the plugin |
|---|---|---|
| Terminal rows | The kernel rows | The same kernel rows, unchanged |
| Lines below Parser pool | None | A divider, a title and three plugin rows |
| Stored values | `[terminal]` only | Plugin values in `[plugins."example.clipboard-history"]`; `record_panes` also per project |
| Rail | The kernel tabs | The same tabs; no new entry |

The built-in rows are the same before and after, and the plugin adds a labelled block with no new rail entry. Users see the plugin's contribution in place, and a plugin cannot take over any part of the Terminal tab that belongs to the kernel.

### What you cannot do

The limits below follow directly from the rule that plugins add and never alter.

- **Change, hide, reorder or relabel a built-in row.** The kernel lays out the kernel rows first and splices plugin blocks in afterwards. There is no field that touches a kernel row.
- **Place a block before `main`.** `"before:main"` is refused, so kernel rows of a one-section tab always come first.
- **Write a built-in value.** The grant allows drawing sections only. A patch can only name keys of the plugin's own sections.
- **Read a built-in value without its own grant.** `settings.contribute` gives no read access. Reading needs `settings.read-kernel:terminal`, shown below. The read is one-way and secrets are redacted.
- **Add a section to a closed tab.** `animations`, `debug`, `api`, `about` and `setup` accept no plugin sections.

To read a Terminal value, add `settings.read-kernel:terminal` to `capabilities.client`. Changes arrive as the event `kernel-settings-changed`.

**Rust**

```rust
use ilium_plugin_sdk::host::kernel_settings;

// Needs the grant settings.read-kernel:terminal. Read-only, secrets are redacted.
fn scrollback_budget() -> Option<u64> {
    let json = kernel_settings::read("terminal").ok()?;
    let values: serde_json::Value = serde_json::from_str(&json).ok()?;
    values["scrollback_mib"].as_u64()
}
```

**TypeScript**

```ts
import { read } from '@ilium/plugin-sdk/host/kernel-settings';

// Needs the grant settings.read-kernel:terminal. Read-only, secrets are redacted.
export function scrollbackBudget(): number | undefined {
  const values = JSON.parse(read('terminal')) as { scrollback_mib?: number };
  return values.scrollback_mib;
}
```

### Troubleshooting

| Symptom or code | Cause and fix |
|---|---|
| `manifest.settings-target-grant` | The section targets `terminal` but the manifest has no `settings.contribute:terminal` capability. Add the grant to `capabilities.client`. |
| `manifest.settings-target-unknown` | The `tab` value is not a kernel tab id. Use the lowercase id from the table in the concept page, such as `terminal`, not the label "Terminal". |
| `manifest.settings-target-closed` | The tab accepts no plugin sections, such as `debug`. Choose a tab that accepts sections. |
| `manifest.settings-anchor` | The placement names a section that the tab does not have, for example `after:output`. Use `main` on Terminal, or a named anchor of the tab. |
| `manifest.settings-limit` | A count is over its limit, such as a third section in one tab. Merge the rows into fewer sections. |
| `manifest.settings-key-clash` | Two rows in the plugin use the same `key`. Keys are unique across all tabs and sections of the plugin. |
| `manifest.settings-default` | A `default` does not match its row, for example a `min_length` default of 300 with a maximum of 200. Correct the default. |
| A row is missing | Its `visible_when` is false. Change the controlling row. |
| `settings.section-dropped` | The tab already has six plugin sections. This is a warning only. Remove a plugin or move a section to a tab with room. |
| `settings.value-ignored` | A stored value no longer matches its row, for example after the maximum was lowered. The default applies, and the stored text stays in the file. |

Kernel anchors are never removed, so a `after:main` placement keeps working through kernel updates. The `settings.anchor-missing` warning applies only to an anchor that the plugin itself declared in one of its own sections.

### Next steps

- Read [Settings extension](/pages/settings-extension/) for the registry, the anchor table of every kernel tab and the lifecycle states.
- Read [Settings schema reference](/pages/reference-settings-schema/) for every row type and field, and the error codes.
- Read [Row gallery](/pages/reference-settings-rows/) for every row in every state.
- Read [Capability: all settings](/pages/capability-settings-all/) for the difference between `settings.contribute` and the all-settings grants.
- Return to [tutorial 10](/pages/tutorial-10-settings-new-tab/) to give the plugin its own tab next to this section.
