# Capability: help.read

## help.read

The `help.read` grant lets a client-side plugin read the Markdown help topics of every installed plugin, and the kernel's own topics. A plugin can show a reference page inside its own panel instead of shipping a copy of that page. Any plugin can read any installed topic, so one plugin can show another plugin's help next to its own.

The grant covers reading only. Declaring a topic in a package needs no grant.

<figure>
<svg viewBox="0 0 400 230" role="img" aria-label="A reading plugin calls topic with a global id. The kernel looks the id up in the help folders of installed packages and returns Markdown or none.">
  <defs>
    <marker id="arr-help" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
      <path d="M0,0 L8,4 L0,8 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="20" y="10" width="360" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="200" y="34" text-anchor="middle" fill="currentColor" font-size="12">Reading plugin (help.read)</text>
  <line x1="130" y1="50" x2="130" y2="90" stroke="currentColor" marker-end="url(#arr-help)"/>
  <text x="140" y="74" fill="currentColor" font-size="10">topic(id)</text>
  <rect x="20" y="92" width="360" height="40" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="200" y="116" text-anchor="middle" fill="currentColor" font-size="12">Kernel topic lookup by global id</text>
  <line x1="130" y1="132" x2="130" y2="172" stroke="currentColor" marker-end="url(#arr-help)"/>
  <text x="140" y="156" fill="currentColor" font-size="10">reads help/ file</text>
  <rect x="20" y="174" width="360" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="200" y="198" text-anchor="middle" fill="currentColor" font-size="11">Installed packages: help/ folders</text>
  <line x1="340" y1="92" x2="340" y2="52" stroke="currentColor" marker-end="url(#arr-help)"/>
  <text x="332" y="76" text-anchor="end" fill="currentColor" font-size="10">Markdown or none</text>
</svg>
<figcaption>The topic call reads one Markdown file from an installed package. The kernel returns none when no installed plugin declares the id.</figcaption>
</figure>

### What you need

| Requirement | Detail |
|---|---|
| Grant `help.read` | Reads any installed help topic. The grant takes no argument. Its risk is ordinary, not high (H). |
| Side | `client`. The `help` import is part of `ilium:client/base`. A server-side plugin cannot call it. |
| Language | Rust with `ilium-plugin-sdk`, or TypeScript with `@ilium/plugin-sdk`. |
| Topic files | Each topic you offer is declared under `contributes.help`, with its Markdown file in the `help/` folder. |

### Grant syntax

The grant takes no argument. The kernel checks the form when the package is installed.

| Grant | Valid | Result at install |
|---|---|---|
| `help.read` | yes | the only form |
| `help.read:com.example.notes` | no | `manifest.grant-argument`: `capability "help.read" takes no argument (got "help.read:com.example.notes")` |
| `help` | no | `manifest.grant-unknown`, because `help` is not a grant name |

### The surface

Rust paths start at `ilium_plugin_sdk::host`. TypeScript paths start at `@ilium/plugin-sdk/host`.

| Function | Rust | TypeScript | WIT | Meaning |
|---|---|---|---|---|
| `topic(id)` | `help::topic(id: &str) -> Option<String>` | `topic(id: string): string \| undefined` | `topic: func(id: string) -> option<string>` | Returns the Markdown text of the topic with that global id, or none. |

The function is synchronous. It reads the topic from the installed package, so it needs no request and no completion. Its WIT signature returns `option`, not `result`, so it has no error channel and never returns a `PluginError`. A missing topic, a declined grant and an unknown id all produce `none`.

The `id` argument is a global topic id, written `<plugin-id>/<local-id>`:

- `<plugin-id>` is the id of the plugin that declares the topic, such as `com.example.notes`.
- `<local-id>` is the `id` the manifest gives the topic, such as `notes.search`.
- The kernel's own topics use the prefix `ilium/`.

A local id on its own (`notes.search`) matches nothing and returns `none`.

### Manifest contribution

A plugin that offers help topics lists each one under `contributes.help`:

| Key | Type | Meaning |
|---|---|---|
| `id` | string | Local id. The global id is `<plugin-id>/<id>`. |
| `file` | string | Path of the Markdown file inside the package, under `help/`, such as `help/search.md`. |
| `title` | string | Name shown in the help index. |

```json
"contributes": {
  "help": [ { "id": "notes.search", "file": "help/search.md", "title": "Searching notes" } ]
}
```

The topic file is plain Markdown. Its first heading is conventionally the topic title. The host returns the file text unchanged.

### Worked example in Rust

This client plugin shows the search topic of a notes plugin in its own panel. When the topic cannot be read, the panel shows one fallback line that points to the manual. The plugin declares `help.read` in `capabilities.client`.

Manifest:

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.help",
  "name": "Help panel",
  "version": "0.1.0",
  "publisher": "Example Labs",
  "client": { "entry": "client.wasm", "world": "ilium:client/base@0.1.0", "exports": [] },
  "capabilities": { "client": ["help.read"] }
}
```

Source, `src/lib.rs`:

**Rust**

```rust
use ilium_plugin_sdk::host::help;

const SEARCH_TOPIC_ID: &str = "com.example.notes/notes.search";
const FALLBACK_TEXT: &str =
    "Search help is not available. Open the plugin manual for the full reference.";

/// Markdown for the search help panel. The kernel returns none when the
/// grant is declined or the topic is missing, so the fallback line is shown.
fn search_help_markdown() -> String {
    help::topic(SEARCH_TOPIC_ID).unwrap_or_else(|| FALLBACK_TEXT.to_string())
}

/// First heading of the topic, used as the panel title.
fn search_help_title() -> String {
    search_help_markdown()
        .lines()
        .find(|line| line.starts_with('#'))
        .map(|line| line.trim_start_matches('#').trim().to_string())
        .unwrap_or_else(|| "Help".to_string())
}
```

The view code calls `search_help_title()` for the panel heading and `search_help_markdown()`
for the body.

### Worked example in TypeScript

The same panel in TypeScript. The topic file ships in the notes plugin, not in this plugin.

**TypeScript**

```ts
import { topic } from '@ilium/plugin-sdk/host/help';

const SEARCH_TOPIC_ID = 'com.example.notes/notes.search';
const FALLBACK_TEXT =
  'Search help is not available. Open the plugin manual for the full reference.';

/** Markdown for the search help panel, or one fallback line. */
export function searchHelpMarkdown(): string {
  return topic(SEARCH_TOPIC_ID) ?? FALLBACK_TEXT;
}

/** Heading of the topic, used as the panel title. */
export function searchHelpTitle(): string {
  const firstHeading = searchHelpMarkdown()
    .split('\n')
    .find((line) => line.startsWith('#'));
  return firstHeading ? firstHeading.replace(/^#+\s*/, '').trim() : 'Help';
}
```

The notes plugin ships the topic file `help/search.md` with this text:

```markdown
# Searching notes

Type a word in the search box to find it in every note.
```

With the grant allowed, `searchHelpTitle()` returns `Searching notes`, and `searchHelpMarkdown()`
returns the whole file text.

### Edge cases and errors

The function has no error codes. The universal codes of the plugin API do not apply to it, because
a missing answer is `none`, not an error.

| Situation | Result | Log line |
|---|---|---|
| Id declared by an installed, enabled plugin | the Markdown text | none |
| Id declared by a disabled plugin | the Markdown text; topics of a disabled plugin stay readable | none |
| Kernel topic with the `ilium/` prefix | the Markdown text | none |
| Id not declared, misspelt, or a local id without the plugin prefix | `none` | none |
| `help.read` declined by the user | `none` | one warn line in the plugin log |
| `help.read` revoked while the plugin runs | `none` | one warn line in the plugin log |
| `help` import used from server code | not available: the import is not in `ilium:server/base` | build error |
| `contributes.help` entry whose `file` is missing or outside `help/` | package check fails | install error |

The function never throws. Code that needs the topic text must handle `none` in every case. A
declined grant and a missing topic look the same to the caller, so the warn line in the log is the
only way to tell them apart.

### What the user sees

When the user installs a T2 or T3 package that asks for `help.read`, the review dialog shows the
grant as an ordinary row. The row is checked by default, because the grant is not high risk. A T1
bundled plugin receives the grant up to its manifest ceiling without a dialog.

<figure>

<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.help 0.1.0 by Example Ltd                                      │
│                                                                            │
│ <span class="t-hi">Client side</span>                                                                │
│ [x] Reads help topics <span class="t-dim">………………………………………………………………………………………………………………</span> help.read │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>

<figcaption>The review dialog for a package that requests help.read, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar. Checked rows are granted; a high-risk row starts unchecked.</figcaption>

</figure>

When the user declines the grant, the plugin keeps running. Its panel shows the fallback line, and
the plugin log gets a warn line. The developer can check the state from the terminal:

<pre class="term"><span class="t-prompt">$</span> ilium plugin grants com.example.help
<span class="t-err">declined</span>  client  help.read
<span class="t-prompt">$</span> ilium plugin logs com.example.help
<span class="t-warn">warn</span>  help.topic returned none: help.read is declined
</pre>

<figcaption>Illustrative, not generated. A declined grant shows in <code>grants</code>, and the warn line shows in <code>logs</code>.</figcaption>

### Risk and tier rules

- `help.read` has ordinary risk. It is not in the high-risk set, so the review dialog checks it by default.
- T1 bundled plugins receive the grant automatically, up to their manifest ceiling.
- T2 and T3 packages show the grant in the review dialog. The user can clear it there or later in Settings.
- Topic text comes only from the `help/` folder of an installed package. The kernel never fetches help over the network.

### Who requests it

First-party plugins from [feature traceability](/pages/reference-feature-traceability/):

- 10 Help and key reference, which shows the topics of every installed plugin.
- 90 Settings UI, which links each settings page to its help topic.

### See also

- [help and plugins](/pages/api-help-and-plugins/) for the full `help` and `plugins` imports, including the `plugin-info` record.
- [settings.read-all and settings.write-all](/pages/capability-settings-all/) for the settings pages that link to help.
- [plugins.manage](/pages/capability-plugins-manage/) for turning plugins on and off, the other grant in the same area.
- [fs.read](/pages/capability-fs-read/) for the package files a plugin reads for itself.
- [Your first plugin](/pages/getting-started-first-plugin/) for the manifest and build steps.

Checked against `ilium:plugin@0.1.0` (API draft of 2026-10-11).
