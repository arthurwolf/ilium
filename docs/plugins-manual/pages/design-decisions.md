---
layout: page
title: Design decisions
permalink: /pages/design-decisions/
---

## Design decisions

The user made seven decisions on 2026-10-11 about the plugin platform, D1 to D7. On the same day the user overrode one earlier rule so that plugins can add tabs and sections to Settings, recorded as SE1 to SE20. On 2026-10-12 the user asked that the API and the widget catalogue express everything Ilium has. That work added 42 additions (AX1 to AX42) and raised eighteen open questions, D8 to D25. This page records each decision: the question it answered, the decision, the reason and the effect on plugin authors.

D1 to D27 and SE1 to SE20 are decided (D3 is superseded in part by D27, D9 is replaced by D13). Most entries state the choice and the alternative that was not taken. Where a decision differs from the original recommendation, the page says so.

In plain terms, a plugin is a WebAssembly component. Rust and TypeScript both compile to that form and run under one contract, one set of limits and one set of capability grants. Third-party code runs in a separate helper process. First-party code runs inside the Ilium process. The decisions set these boundaries, and the other pages apply them.

### You will need

Nothing to install or grant. This page explains design choices and asks nothing of you. Read it when a rule in another page looks strict, for example when a T2 package is refused on Windows, when a timer refuses an interval under 50 ms, or when a high-risk grant is off by default even for a bundled plugin. The [Glossary](/pages/glossary/) defines the terms tier, side and grant that this page uses.

### Summary

| ID | Decision | Status | What it means for you |
|---|---|---|---|
| D1 | JavaScript and TypeScript compile to WebAssembly components | Decided | Both languages share one contract, one set of limits and one set of grants |
| D2 | Saved-world and file-reading scenes are plugins | Decided | Scenes read files through scoped grants; editor and basic primitives exist to build on |
| D3 | Local and signed packages first, a registry later | Superseded in part by D27 | You can always distribute a file; a T2 package needs a signature from a key the user trusts |
| D4 | Editors and boards are native in version 1 | Decided | Use the editor primitive; a plugin that provides the editor is a post-v1 feature |
| D5 | A separate-process sandbox is mandatory for T2 and T3 | Decided | Third-party code runs in a helper process; Windows and macOS refuse it |
| D6 | Bundled plugins are precompiled and start lazily | Decided | Activation triggers decide when your plugin first runs |
| D7 | One `.iliumpkg` format replaces `.iliumanim` | Decided | Rebuild old packages; the `limits` object gains five fields |
| SE1 to SE20 | Plugins add tabs and sections to Settings | Decided | Rows are declared in the manifest, labelled by the kernel and stored under your plugin id |
| D8 | `path` rows store a string only | Decided | A path picked in Settings gives no file scope |
| D9 | Reading built-in values | Replaced by D13 | See D13 |
| D10 | No user reordering of plugin sections | Decided | Section order is fixed |
| D11 | Kernel rows keep their own reset and debounce behaviour | Decided | Only plugin rows have the `r` reset and the `•` modified mark |
| D12 | First-party limits tier only for bundled `ilium.*` plugins | Decided | `limits_tier: "first-party"` is refused for anyone else |
| D13 | Kernel settings are readable; the tab owner may write | Decided | Any plugin can read a kernel value. Only the one owner plugin of a tab can write it |
| D14 | The host owns geocoding | Decided | You never call a geocoder through `http` |
| D15 | Embeddings only through the selected inference provider | Decided | `vector.embed` can return `unsupported` |
| D16 | Cross-session commands are high risk and same-plugin only | Decided | You can reach only your own commands in other sessions |
| D17 | Raw input and the input lock are high risk | Decided | The lock lasts at most 2 s |
| D18 | GPU access is `@unstable` and host kernels only | Decided | You cannot ship shader code |
| D19 | No `preview` rows in kernel tabs | Decided | Previews live in your own tab |
| D20 | Toasts need a grant and a rate limit | Decided | 3 toasts per 10 s per plugin |
| D21 | Durable timers fire late once; journaled input never replays | Decided | You may get `input-uncertain` after a crash |
| D22 | Agent transcripts are typed turns from the host | Decided | No raw transcript files |
| D23 | A host taken from a setting is confirmed by the user | Decided | The first request to a new host waits for the user |
| D24 | Package assets: 64 MiB, 256 MiB for T1 | Decided | Video and font assets must fit |
| D25 | `open-external` asks once per target origin | Decided | The first open of a site waits for the user |
| D26 | One uniform mechanism for extension points, hooks fail open, replacement needs a declared surface, no wildcard grants | Decided | Any plugin can offer a slot and any plugin can fill it, one point at a time |
| D27 | An optional static registry protocol, no central store | Decided | Anyone can host a signed registry or mirror; Ilium ships with none configured |

## D1: JavaScript and TypeScript compiled to components

**Question.** How does JavaScript or TypeScript run in the platform? The original draft proposed generated V8 glue. The alternative is to compile the code to WebAssembly components.

**Decision.** JavaScript and TypeScript are compiled to a WebAssembly component with `componentize-js`. `componentize-qjs` is not used. The SDK pins `@bytecodealliance/componentize-js` and `@bytecodealliance/jco` as exact dependencies of `@ilium/plugin-sdk`, and Bun 1.2 or later is the only global tool. V8 stays only for the existing animation packages until the animation migration (Phase 4c).

**Reason.** The contract is synchronous by design, so it fits a component build exactly. One host, one linker and one conformance suite then cover both languages. The alternative would need a WIT-to-V8 generator with no upstream, a second dispatcher and a doubled conformance matrix. This decision replaces the original recommendation of generated V8 glue.

**Effect on authors.** A TypeScript plugin faces the same limits, grants and error codes as a Rust plugin. `ilium plugin build` detects the language from the files in the project folder. A `Cargo.toml` means Rust, and a `package.json` means TypeScript. If both are present and `--lang` is absent, the build fails. See the [TypeScript SDK reference](/pages/typescript-sdk/) and the [Rust SDK reference](/pages/rust-sdk/).

## D2: Scope of plugins

**Question.** Which built-in features must be native, and which can be plugins?

**Decision.** The user disagreed with native-by-default for several areas. Saved-world scenes are plugins. They read their files through scoped `fs.read:<scope>` grants and use the codec helpers. File-reading scenes also use scoped reads. Editors get an editor primitive, a host-provided pane kind that plugins extend or build on. The basic primitive is a generic pane kind from which a plugin can build a kanban board, for example. Plugins get a UI-building API that mirrors the ratatui widget model, and that API is enough to build settings screens.

**Reason.** Saved-world and file-reading scenes can be plugins once the ABI gives scoped file access and processing helpers. The primitives let editors and boards be built on the same contract that plugins use, so a later move needs no ABI change.

**Effect on authors.** A scene that reads a folder declares a grant such as `fs.read:<scope>` in its manifest. The user approves that grant in the review dialog. Build screens with `ilium_plugin_sdk::ui::build` in Rust or `@ilium/plugin-sdk/ui/build` in TypeScript. Each widget is listed in the [widget pages](/pages/widgets-index/). The [primitives](/pages/primitives/) and [settings screens](/pages/settings-screens/) pages show the rest of the surface.

## D3: Third-party scope for version 1

**Question.** Should version 1 include a marketplace?

**Decision.** Local and signed packages are the base of the system. A package is a file that users pass on, and `ilium plugin install <file.iliumpkg>` installs it. Ilium ships no central marketplace and no default registry. Decision D27 adds an optional, self-hostable registry protocol on top of the file; it replaces the earlier wording "no registry and no download".

**Reason.** A marketplace costs signing infrastructure, index hosting and a review policy. Local signed files need none of those to be loaded and verified.

**Effect on authors.** A T2 package is signed with Ed25519. The signature covers the canonical bytes of `manifest.json`, and the manifest's `publisher` must match the key's recorded publisher. You create a key with `ilium plugin keygen --publisher "<name>" --out <dir>`. A T3 package is unsigned. It installs only with `--allow-unsigned`, or through `ilium plugin dev`, and it carries a dev badge. The [Security model](/pages/security/) page lists every tier.

## D4: Editors and boards native in version 1

**Question.** Are the editor and boards native pane kinds, or plugins?

**Decision.** Native in version 1. Plugin support for them is a planned phase after v1. The primitives are shaped so that moving the editor and boards later needs no ABI break.

**Reason.** The editor would be the first user of a pane-body contract that needs keymap arbitration, cursor, selection and clipboard semantics. The user chose native in v1 and a planned post-v1 phase, so the primitives are designed to make that move cheap.

**Effect on authors.** The editor primitive exists in v1, and a plugin can use it. A plugin that provides the editor needs the `editor.provide` grant, which only T1 bundled plugins can receive, and it arrives after v1. A calling plugin names the primitive, not a provider, so it does not change when the provider moves. See the [Roadmap](/pages/roadmap/).

<figure>
<svg viewBox="0 0 360 150" role="img" aria-label="Editor primitive routed to a provider">
  <title>Provider routing for the editor primitive</title>
  <desc>A caller plugin calls the editor primitive. The kernel router sends the call to the registered provider. In version 1 that provider is native. After v1, a T1 plugin may register as the provider, and the caller does not change.</desc>
  <defs>
    <marker id="arrow-editor" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
      <path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="6" y="56" width="96" height="34" rx="6" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
  <text x="54" y="77" text-anchor="middle" fill="currentColor" font-size="10">Caller plugin</text>
  <rect x="132" y="56" width="96" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="180" y="77" text-anchor="middle" fill="currentColor" font-size="10">Kernel router</text>
  <rect x="240" y="10" width="112" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="296" y="24" text-anchor="middle" fill="currentColor" font-size="10">Native provider</text>
  <text x="296" y="40" text-anchor="middle" fill="currentColor" font-size="10">version 1</text>
  <rect x="240" y="106" width="112" height="34" rx="6" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
  <text x="296" y="120" text-anchor="middle" fill="currentColor" font-size="10">Plugin provider</text>
  <text x="296" y="136" text-anchor="middle" fill="currentColor" font-size="10">after v1, T1 only</text>
  <path d="M104 73 L130 73" stroke="currentColor" fill="none" marker-end="url(#arrow-editor)"/>
  <path d="M230 66 L238 40" stroke="currentColor" fill="none" marker-end="url(#arrow-editor)"/>
  <path d="M230 80 L238 116" stroke="currentColor" fill="none" stroke-dasharray="4 3" marker-end="url(#arrow-editor)"/>
</svg>
<figcaption>The caller names the editor primitive, not a provider. The kernel router chooses the provider, so moving the editor to a plugin changes the router target and not the caller.</figcaption>
</figure>

## D5: Separate-process sandbox for third-party plugins

**Question.** Should third-party WebAssembly run in the Ilium process?

**Decision.** No. A separate-process sandbox is mandatory for third-party plugins (T2 and T3). The helper process runs the plugin host for those packages inside the sandbox, which opens only the directories the user granted. The sandbox exists on Linux only. Windows and macOS refuse to load third-party packages until a sandbox exists on those platforms.

**Reason.** Running third-party code in-process would rely on Wasmtime alone for isolation. The design record counts twelve Wasmtime advisories in two months, which is the reason for a separate process boundary.

**Effect on authors.** A T2 or T3 plugin runs in the helper and sees only its granted folders. A `process.run:sh` grant is refused for T2 and T3 at install with `manifest.grant-tier`. On Windows and macOS, the install is refused with the message below. First-party T1 plugins run in-process on every platform. See the [Security model](/pages/security/).

<figure>
<svg viewBox="0 0 360 150" role="img" aria-label="Sandbox placement for each tier">
  <title>Where each tier runs</title>
  <desc>T0 and T1 run in the Ilium process. T2 and T3 run in a helper process behind the sandbox boundary, which is mandatory for them. Calls from plugins reach the kernel through host imports.</desc>
  <defs>
    <marker id="arrow-sandbox" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
      <path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="6" y="6" width="348" height="62" rx="8" fill="none" stroke="currentColor"/>
  <text x="16" y="22" fill="currentColor" font-size="10">Ilium process: T0 kernel, T1 plugins (any platform)</text>
  <rect x="22" y="32" width="110" height="26" rx="5" fill="none" stroke="currentColor"/>
  <text x="77" y="49" text-anchor="middle" fill="currentColor" font-size="10">Kernel (T0)</text>
  <rect x="180" y="32" width="110" height="26" rx="5" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
  <text x="235" y="49" text-anchor="middle" fill="currentColor" font-size="10">T1 plugins</text>
  <rect x="6" y="84" width="348" height="60" rx="8" fill="none" stroke="currentColor" stroke-width="3"/>
  <text x="180" y="100" fill="currentColor" font-size="10">Helper process: T2, T3</text>
  <rect x="22" y="110" width="170" height="26" rx="5" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
  <text x="107" y="127" text-anchor="middle" fill="currentColor" font-size="10">Third-party plugins</text>
  <text x="200" y="127" fill="currentColor" font-size="10">Windows, macOS: refused</text>
  <path d="M60 110 L60 58" stroke="currentColor" fill="none" marker-end="url(#arrow-sandbox)"/>
</svg>
<figcaption>The helper boundary is mandatory for third-party code. The refusal on Windows and macOS stays until a sandbox exists on those platforms.</figcaption>
</figure>

<pre class="term"><span class="t-err">err</span>  Cannot install example.notes
     0.1.0. Third-party plugins need the plugin sandbox,
     which is not available on Windows yet. Bundled plugins
     keep working.
<span class="t-ok">ok</span>   ilium.notes 0.1.0 (T1) listed,
     starts on first use
</pre>

Illustrative, not generated. A signed third-party package refused on Windows, next to a first-party plugin that starts on first use.

## D6: Bundled plugins precompiled and lazy

**Question.** How are first-party WebAssembly plugins delivered and started?

**Decision.** Bundled plugins are embedded in the release precompiled, as `.cwasm`, and start lazily on first need. Wasmtime's on-demand allocator is the default. A plugin starts when one of its activation triggers fires. Without an `activation` list, it starts on the first use of any of its contributions.

**Reason.** Compiling about forty plugins with Cranelift at every start costs time at each launch. The design record notes that Zellij abandoned Wasmtime over that cost. Precompiled components with lazy activation meet the start-time and memory targets of the [performance budget](/pages/performance/): under 5 ms and under 4 MiB resident memory for each precompiled Rust component.

**Effect on authors.** Use `activation` in the manifest to choose the trigger. The triggers are `startup`, `event:<event-name>`, `command:<id>`, `view:<id>`, `pane-kind:<id>`, `scene:<id>`, `cli:<name>`, `http:<path>`, `service:<interface>`, `topic:<topic>` and `settings:<tab-id>`. A plugin that must run at launch declares `startup`. Start order is in the [lifecycle](/pages/lifecycle/) page, and the manifest keys are in the [manifest reference](/pages/reference-manifest/).

## D7: One package format

**Question.** Should the animation package format stay alongside the plugin format?

**Decision.** One `.iliumpkg` format replaces `.iliumanim`, with no compatibility layer. The package is a deterministic ZIP32 archive with stored entries, sorted paths and fixed timestamps. The `limits` object gains memory, table, event-deadline, render-deadline and command-deadline fields. The loader does not accept the old format.

**Reason.** Keeping both formats would mean two loaders to maintain. The project has no users yet, so a one-time rebuild costs little.

**Effect on authors.** An `.iliumanim` package must be rebuilt as a `.iliumpkg`. The [Porting from .iliumanim](/pages/porting-iliumanim/) page lists the changes. The `limits` keys and their ceilings are below. A request above the ceiling is clamped and logged. It is never refused.

| Key | Default | T1 ceiling | T2 and T3 ceiling | Exceeded |
|---|---|---|---|---|
| `memory_mib` | 32 | 256 | 128 | instance trapped |
| `table_elements` | 10,000 | 50,000 | 20,000 | instance trapped |
| `event_deadline_ms` | 20 | 50 | 20 | instance trapped |
| `command_deadline_ms` | 50 | 100 | 50 | instance trapped |
| `render_deadline_ms` | 10 | 20 | 10 | instance trapped |

### Worked example: a limit request above the ceiling

A signed T2 plugin asks for a longer event deadline than its tier allows. The manifest excerpt below requests 64 MiB of memory and a 30 ms event deadline.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "example.notes",
  "name": "Notes",
  "version": "0.1.0",
  "publisher": "Example Labs",
  "limits": {
    "memory_mib": 64,
    "event_deadline_ms": 30
  },
  "server": { "entry": "server.wasm", "world": "ilium:server/base@0.1.0", "exports": [] }
}
```

The memory request fits the T2 ceiling of 128 MiB and is used as given. The 30 ms request is above the T2 ceiling of 20 ms, so the host clamps it and writes one line to the log:

<pre class="term"><span class="t-warn">warn</span> example.notes:
     limit event_deadline_ms 30 clamped to 20 (T2 ceiling)
</pre>

Illustrative, not generated. The log line the host writes when it clamps a manifest limit.

## The Settings extension decisions

On 2026-10-11 the user overrode the earlier rule that a plugin cannot attach to a kernel settings page. The twenty decisions SE1 to SE20 follow from that override. The page [Plugin settings extension](/pages/settings-extension/) explains them in full. In short:

| ID | Decision |
|---|---|
| SE1, SE2 | A plugin may add tabs to Settings and sections to built-in tabs. A custom page is a plugin tab with `"render": "view"`, and `settings_pages` is removed |
| SE3, SE4 | Every kernel tab has a frozen kebab-case id. Kernel tabs expose anchors, which are only ever added |
| SE5 | `animations`, `debug`, `api`, `about`, `setup` and the recovery page accept no plugin sections |
| SE6 | Plugins add, never alter. Values live only under `[plugins."<plugin-id>"]` |
| SE7, SE8 | Rows are declared inline. There are 18 types. Object list items have at most 8 primitive fields |
| SE9 | Plugin tabs carry `· plugin` in the frame title. Plugin blocks in kernel tabs carry one divider per plugin per anchor and a `▏` gutter on every line. Plugin text is plain and sanitized |
| SE10 | Kernel tabs host only static plugin rows plus `set-info`, `set-choices` and `set-row-note`. View-rendered content is allowed only in the plugin's own tab |
| SE11 | The grant `settings.contribute:<kernel-tab-id>` allows sections in that tab and nothing else |
| SE12, SE13 | Limits are enforced at load time. Ordering is anchor, then `order`, then plugin id, then declaration order |
| SE14 | Values are never deleted by disable, failure, upgrade or uninstall. Only `--purge` or "Forget settings" deletes them |
| SE15 | Toggles and choices save at once. Number steps and text save 600 ms after the last change, and text areas after 1 s |
| SE16, SE17 | `settings-changed` carries `keys`, `origin` and `side`. The optional export `settings-validate` has a 100 ms deadline |
| SE18 to SE20 | Id grammar, a kernel search filter over kernel and plugin rows, and a kernel-owned settings registry |

The extended rows are part of these decisions: SE7's twelve base row types are joined by six more (`location`, `preview`, `notice`, `map`, `multi-choice` and `meter`), SE8's object items may have up to 8 fields, and SE10 still keeps `preview` rows and every view-rendered content out of kernel tabs (D19).

## Decisions D8 to D25

The decisions below are final. Each entry states the choice, and the second half says what the alternative would have changed. The reference for the choices is the design record of the plugin platform; the effect on plugin code is on the pages named in each entry.

### D8, D10, D11: Settings questions

| ID | Question | Decided | If the user chooses otherwise |
|---|---|---|---|
| D8 | Does a `path` row grant file access to the picked path? | No. The row stores a string only. A plugin that must open the file asks for an `fs.read` grant, or uses `path-field`, which returns a picked scope and never a raw path | The plugin could open the picked path without a broad grant, at the cost of a persistent file scope created from Settings |
| D9 | Reading built-in values | Replaced by [D13](#d13-kernel-settings-are-readable-and-the-tab-owner-may-write): kernel settings are readable and the tab owner may write | See D13 |
| D10 | May the user reorder plugin sections inside a built-in tab? | No. The order is anchor, then `order`, then plugin id | A per-user order that must be stored and reconciled when plugins appear or disappear |
| D11 | Do kernel rows adopt the `r` reset, the `•` modified mark and the 600 ms debounce of plugin rows? | No. Kernel rows keep their own reset and debounce behaviour. Only plugin rows have the `r` reset and the `•` modified mark | Kernel and plugin rows would behave the same everywhere |

### D12: the first-party limits tier

**Question.** Who may use `limits_tier: "first-party"`, which lifts the settings caps to 8 tabs, 2,048 value keys, 64 rows per section, 1,024 list items and 512 choice options?

**Decision.** Only bundled T1 plugins whose id starts with `ilium.`. Any other package is refused at install with `manifest.limits-tier`. The tier raises only the settings caps and the `list-editor` cap. The ceilings of the requestable limits stay as they are.

**Alternative.** Signed T2 plugins could ask for it too, which a settings-heavy third-party plugin might need. The caps would then stop protecting the Settings screen.

**Effect on authors.** Write `"limits_tier": "standard"` or omit the key. If your package is first-party and bundled, set `"first-party"`, and keep the id under `ilium.`.

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin install ./example.notes-1.0.0.iliumpkg
<span class="t-err">error</span> manifest.limits-tier  limits_tier first-party needs a
      bundled plugin whose id starts with ilium.
<span class="t-dim">step</span>  install stopped before any code was loaded
</pre>
<figcaption>Illustrative, not generated. A third-party package that asks for the first-party tier stops at the install checks.</figcaption>
</figure>

### D13: kernel settings are readable and the tab owner may write

**Question.** May a plugin read a built-in setting, such as a key that Smart Copy uses?

**Decision.** Yes, read for every plugin, write for one owner per tab. The grant `settings.read-kernel:<kernel-tab-id>` allows `kernel-settings.read` for one tab and the event `kernel-settings-changed`. The argument is required and has no wildcard. Secrets are always returned as `<redacted>`. The `api` tab is high risk. The matching `keymap.read` grant returns the keymap and sends `keymap-changed`. The grant `settings.write-kernel:<kernel-tab-id>` allows `kernel-settings.write` for one tab. It is T1 only and high risk, and at most one plugin holds it for a tab (the core plugin that owns the tab, see [config and data migration](/pages/reference-config-and-data-migration/)). No plugin writes the keymap. Every other plugin links to the row with `open-external`.

**Alternative.** Keep D9's "not provided". A plugin that needs a kernel value would then duplicate it as its own setting and drift from the kernel.

**Effect on authors.** Read a value, and when the user must change it, link to the built-in row with the intent `open-external` and the target `ilium://settings/<tab-id>#<key>`. The code below reads one key of one tab. The grant is `settings.read-kernel:terminal`.

**Rust**

```rust
use ilium_plugin_sdk::host::kernel_settings;
use ilium_plugin_sdk::prelude::*;

/// Reads one value of a built-in tab. Returns `None` when the grant is missing
/// or the kernel has no such key.
fn kernel_value(tab: &str, key: &str) -> Option<serde_json::Value> {
    let json = kernel_settings::read(tab).ok()?;
    let values: serde_json::Value = serde_json::from_str(&json).ok()?;
    values.get(key).cloned()
}
```

**TypeScript**

```ts
import { PluginError } from '@ilium/plugin-sdk';
import { read } from '@ilium/plugin-sdk/host/kernel-settings';

/** Reads one value of a built-in tab. Returns undefined when the grant is missing. */
function kernelValue(tab: string, key: string): unknown {
  try {
    const values = JSON.parse(read(tab)) as Record<string, unknown>;
    return values[key];
  } catch (error) {
    if (error instanceof PluginError && error.code === 'not-permitted') {
      return undefined;
    }
    throw error;
  }
}
```

### D14: geocoding

**Question.** Who geocodes place names for `location.geocode` and the location picker?

**Decision.** The host. It uses Photon, with Nominatim as fallback, asks the user once for consent, limits each plugin to 1 request per second and caches results for 24 h.

**Alternative.** Plugins call geocoders through `http`. That offers more choice, but every plugin then ships its own politeness and consent, and the user cannot see where place names go.

**Effect on authors.** Use the `geocode` calls or the `location-picker` widget. See [Security model](/pages/security/) for the consent prompt and for what a declined consent returns.

### D15: embeddings

**Question.** Where do embeddings for `vector.embed` come from?

**Decision.** Only from the selected inference provider. A provider without embeddings makes the call return `unsupported`.

**Alternative.** A bundled local embedding model. It works offline but adds tens of megabytes and a native inference path.

**Effect on authors.** Check for `unsupported` and keep a keyword fallback. The grant is `inference.embed`.

### D16: cross-session commands

**Question.** May a plugin run commands in the user's other Ilium sessions?

**Decision.** Yes, with the high-risk grant `sessions.cross`. It is available to T1 and signed T2 plugins only, and only to the same plugin in the other session, which must have the grant too. Kernel commands of another session are never reachable.

**Alternative.** Available to T3 as well. That is simpler, but an unsigned plugin could drive every open session.

**Effect on authors.** Declare the command in your own manifest, ask for `sessions.cross` on the server side, and expect `not-permitted` when the other session lacks the plugin or the grant.

### D17: raw terminal input and the input lock

**Question.** May a plugin send raw bytes or mouse events to a pane, and hold the keyboard while it types?

**Decision.** Both are high risk (`terminal.raw-input`, `terminal.input-lock`). The lock lasts at most 2 s, and the user's keys are queued during it.

**Alternative.** No raw input. Plugins then cannot drive TUI programs that need mouse or control bytes, so screen-transfer and agent-control plugins stay partial.

**Effect on authors.** Keep a locked sequence short. See [Security model](/pages/security/).

### D18: GPU access

**Question.** Can a plugin use the GPU?

**Decision.** Only through `gpu.kernels`, which is `@unstable(feature = gpu-kernels)` and appears in development builds only. The host runs its own kernels (blur, threshold, palette map, ordered dither, scale). A plugin never ships a shader.

**Alternative.** `wasi:webgpu` once it is mature. That gives full shaders, with a large new attack surface and driver-dependent results.

**Effect on authors.** GPU scenes stay native behind the scene adapter. A manifest that names `device.gpu` is refused with `manifest.grant-unknown`.

### D19: `preview` rows in kernel tabs

**Decision.** No. A `preview` row exists in a plugin's own tab only (SE10), because a kernel tab never runs plugin code while it paints.

**Alternative.** Allow it in kernel tabs, which breaks SE10.

### D20: toasts

**Question.** May a plugin show an in-app toast?

**Decision.** Yes, with the grant `notify.toast`, at most 3 per 10 s per plugin and 3 pending. A toast has a title of at most 60 characters, a body of at most 200 characters and up to 2 actions.

**Alternative.** No grant. Any plugin could then cover the screen with toasts.

**Effect on authors.** The code under D25 sends one toast and asks the host to open a status page. A toast and an `open-external` are submissions, so both can be refused.

<figure>
<pre class="term"><span class="t-hi">╭─ Copied 3 regions ───────────────────────╮</span>
│ Select a pane and press Enter to paste.  │
│ <span class="t-ok">━━━━━━━━━━━━━━━━</span><span class="t-dim">────────────────────────</span> │
╰──────────────────────────────────────────╯
</pre>
<figcaption>Illustrative, not generated. A toast as the client draws it, with the shrinking countdown line below the text. The kernel places the stack.</figcaption>
</figure>

### D25: confirming `open-external`

**Question.** Does the user confirm when a plugin opens a link or file?

**Decision.** The host asks the first time a plugin opens each target origin and remembers the answer per plugin. Only `http`, `https` and `mailto` URLs, files and `ilium://settings/...` targets are allowed.

**Alternative.** Ask every time, which is safer and tedious for link-heavy panels, or never ask, which lets a plugin open any site.

**Effect on authors.** The first open of a site waits for the user. A decline completes the submission as `refused(not-permitted)`.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::intents::{ExternalTarget, OpenExternalSpec, ToastSpec};

fn status_submissions() -> Vec<Submission> {
    vec![
        Submission::new(Intent::Toast(ToastSpec {
            id: Some("copied".into()),
            tone: Tone::Success,
            title: "Copied 3 regions".into(),
            body: None,
            actions: vec![],
            duration_ms: None,
        })),
        Submission::new(Intent::OpenExternal(OpenExternalSpec {
            target: ExternalTarget::Url("https://status.example.com".into()),
        })),
    ]
}
```

**TypeScript**

```ts
import type { Submission } from '@ilium/plugin-sdk';

function statusSubmissions(): Submission[] {
  return [
    {
      intent: { tag: 'toast', val: { id: 'copied', tone: 'success', title: 'Copied 3 regions', body: undefined, actions: [], durationMs: undefined } },
      correlation: 1n,
      dedupKey: undefined,
    },
    {
      intent: { tag: 'open-external', val: { target: { tag: 'url', val: 'https://status.example.com' } } },
      correlation: 2n,
      dedupKey: undefined,
    },
  ];
}
```

### D21: timers and the input journal

**Decision.** Durable timers (`timers.at`) persist across restarts and fire once, late, with the lateness in `durable-timer-fired`. A `send-input` with `journal: true` that was written to the pane but not confirmed before a crash is reported once as `input-uncertain` and never replayed.

**Alternative.** Replay unconfirmed input after a restart. No prompt is lost, and a prompt could be sent twice.

**Effect on authors.** Handle `input-uncertain` by asking the user, not by sending again.

### D22 and D23: transcripts and hosts from a setting

- **D22.** Agent transcripts are typed turns that the host parses, behind the high-risk grant `agent.transcript`. The alternative is raw file access through `fs.read:agent-sessions:*`, where every plugin parses every agent format and the kernel cannot redact.
- **D23.** A host taken from a setting (`http:setting:<key>`) is confirmed by the user before the first request to each new host. The alternative is to trust the setting, which silently opens a new host when the setting changes.

### D24: package asset size

**Decision.** 64 MiB by default and for T2 and T3, up to 256 MiB for T1. A larger package is refused with `manifest.asset-size`. The alternative is 16 MiB everywhere, which makes packages smaller but does not fit the video and font assets of the media scenes.

## D26: extension points, hooks and replacement

**Question.** How do plugins add to each other, and how does a third party change a core plugin, without a private API?

**Decision.** One mechanism serves every plugin, core or not. An owner declares a point under `contributes.extension_points`. A point is either `collect` (the owner reads data entries) or `hook` (handlers run inside the owner's flow, in one of four modes: `observe`, `transform`, `veto`, `first`). A contributor fills it under `contributes.extensions`. Two grants guard it: `extensions.provide:<plugin-id>/<point>` and `extensions.contribute:<plugin-id>/<point>`. A core plugin that can be replaced declares `replaceable` and a `surface`, and a package with `replaces` that provides the whole surface may take its place after the user confirms.

**Why one uniform mechanism.** Core plugins use only the public contract, so everything they can do a third-party plugin can do with the same grants. Services have one provider and topics have no reply. The manual needed a third shape, many contributors to one owner, and building it once keeps one set of limits, one review dialog group and one error table. The existing mechanisms stay.

**Why hooks fail open.** A handler is someone else's code inside the owner's flow. If a timeout, a trap or an error stopped the owner, one slow or broken plugin could freeze text triggers, titles or fleet actions. A failing handler counts as a pass at every point except the high veto points, which fail closed with `on_failure: refuse` ([SEC-287]), three breaches in a row stop that extension, and the whole chain is capped at 100 ms. Only a `veto` handler that answers on purpose can stop an action.

**Why replacement needs a surface.** A replacement that provides only part of a core plugin would leave dangling services, commands, pane kinds and topics for other plugins. The `surface` list is the checklist, and `manifest.replaces-surface-incomplete` names what is missing. Replacement needs the user's confirmation, not a grant, because the user decides which built-in behaviour to give up. `ilium.core.sessions` and `ilium.core.settings` are not replaceable, because they protect saved data and the Plugins screen itself.

**Why no wildcard grants.** A wildcard such as "contribute to any point" would let one package place code in every flow. The user could not read the review dialog and decide. Each point is named, so the dialog can show `Text triggers > Before fire: can stop a trigger from sending text (high risk)`. The grant `extensions.contribute-any` does not exist.

**Alternative.** Per-plugin special hooks, which give core plugins a private API, or a single global event bus with veto rights, which cannot show the user what a package may stop.

**Effect on authors.** To let others extend your plugin, declare a point and read it with `list` or `run-hook`. To extend another plugin, ask for the grant for that exact point and list `hook-handler` in `exports` if you add handlers. See [Extension points](/pages/api-extension-points/) and [Extending core plugins](/pages/extending-core-plugins/).

## The API additions and their decisions

The additions AX1 to AX42 are all additive. "Shape" additions add optional fields to a record that already exists, and they are part of the first frozen contract. After that freeze, the same need is met only by new records and new functions. The table lists every addition.

| Addition | What it adds |
|---|---|
| AX1 | Hover, `tooltip` prop, anchored and pinnable non-modal popups |
| AX2 | Catalogue `choice`: option records, typed filter, disabled reasons, previews, paged sources, host catalogues |
| AX3 | Settings row semantics: `visible_when`, step ladders, special values, slider style, project scope, restart marker, per-row debounce, row state, styled info |
| AX4 | Terminal cell reads, frozen snapshot, history text, modes, foreground process, `screen-changed`, `user-line-submitted` |
| AX5 | Pane lifecycle intents: freeze, unfreeze, replace command, terminate process, resize with refusal |
| AX6 | Tree structure intents and atomic restructure batches with undo |
| AX7 | Title provenance (`source`, expected revision), inferred icon, attention marks |
| AX8 | `open-external` intent through the kernel checks: the scheme list and the per-origin consent (no separate target allowlist) |
| AX9 | Tree and chrome contribution points; lazy virtual-row children; drops on virtual rows |
| AX10 | Rich decorations: styled spans, tooltip, frame animation, trailing segments, hover visibility |
| AX11 | Pointer model: view pointer with sub-cell coordinates, drag-over feedback, terminal move and modifier gesture |
| AX12 | Settings views may embed display widgets; `preview` and `location` rows; `location-picker` widget; `location.geocode` service |
| AX13 | Picker widgets: `color-picker`, `path-field`, icon and glyph choice through catalogues; `icon` widget |
| AX14 | CLI plain text, stderr, bounded stdin, TTY test, caller pane, server-less commands, pane-selector group, cross-session list |
| AX15 | Surface v2: glyph plane, owner ids with receipts, font handles, video from bytes or ranges, `video` widget, pre-rendered loops, larger assets |
| AX16 | `fs` atomic write with expected revision, read with revision, tail with cursor, agent home scopes for Gemini and Antigravity |
| AX17 | Menu model: dynamic items with click context, submenus, icons, disabled reasons, toolbar submenus |
| AX18 | Click targets in styled text; Markdown task clicks and images |
| AX19 | Streaming HTTP (SSE, NDJSON, chunked), `@unstable` WebSocket, politeness; hosts taken from a setting with consent |
| AX20 | `send-input` stages with delays, raw bytes, mouse; preconditions; input lock; interactive `terminal-view` |
| AX21 | Agent facts: typed transcript, model fact, prompt epoch echo, `signal-changed`, `agent-model-changed` |
| AX22 | Embeddings and a host-held vector index |
| AX23 | In-app `toast`, `notice` row and widget, `notify` coalescing |
| AX24 | `chart` gains resampling, gaps, text-line rendering (asciichart parity) |
| AX25 | Form widgets: tri-state toggle, `check-list`, `key-capture`, focus order, secret reveal, slider and stepper styles in views |
| AX26 | Table column priority and minimum width, follow-tail, panel placement and mode label |
| AX27 | Streaming child processes (`process.spawn`) |
| AX28 | Durable timers and a journaled `send-input` with `uncertain` reporting |
| AX29 | Read-only kernel settings and keymap reads |
| AX30 | Limit tiers (`limits_tier: "first-party"`) |
| AX31 | Editor viewport, pointer, minimap marks, token provider export |
| AX32 | Command palette: kernel surface; commands gain `category` and `keywords` |
| AX33 | GPU kernels run by the host, `@unstable` |
| AX34 | Server-to-client requests between the two sides of one plugin |
| AX35 | Shared cache bucket for a plugin's processes |
| AX36 | Record lists: object items of up to 8 fields, item status, remote confirmation |
| AX37 | Small items: text debounce, `when` keys, basic pane activity and dirty flag, `map` row, pane event journal, reduced motion and density tokens |
| AX38 | `list-editor` widget and typed list items (glob, URL, host, path, regex, chord) |
| AX39 | Layout additions: `split` with draggable divider, `separator`, `dialog`, `scroll-view` |
| AX40 | Data display additions: `progress-row`, `meter`, `minimap`; `meter` row |
| AX41 | `diff-view` widget |
| AX42 | Status and feedback family: `badge`, `spinner`, `notice`, `icon` |

The decisions of the extended contract that need explanation are these:

- **Kernel keeps the engine, plugins own the choice.** Every addition names the part that stays in the kernel: dithering and the shared look, the vt100 emulation, the tree layout solver, the toast stack, the command palette, the syntax engine and every GPU shader. The plugin supplies content and intent. The kernel draws and decides.
- **Intents, not mutations.** Pane lifecycle, tree restructure, attention, status segments, frame slots, toasts and `open-external` are intents. The kernel checks them against the grant and the current state, and a submission completes with one outcome. A new outcome case, `ok-epoch`, carries the new prompt epoch for `send-input` on an agent pane.
- **Read paths are typed and redacted.** Terminal cells, scrollback, transcripts, kernel settings and the keymap are read paths, each behind its own grant, and all of them are read only.
- **High risk is explicit even for T1.** The ten high-risk grants of the extended set always need an explicit grant, and the dialog shows them in the warning style.
- **Streaming, not polling.** HTTP streams, spawned children and screen changes arrive as events whose delivery policy is stated per event: coalesced, or never dropped with back-pressure.

## What the user sees

- The review dialog lists every capability a plugin requests, before the first run. The user grants or declines each one. High-risk rows start unchecked.
- A T2 package is shown with its publisher and signature status. A T3 package carries a dev badge for as long as it is installed.
- A bundled plugin with high-risk grants is asked at its first start, and again at each later start after Not now, at most three prompts in total. The first open of a site, a new host from a setting, and the first place lookup each ask once.
- A plugin that is disabled after three failures shows a notice in the tree footer. The user re-enables it in settings or on the kernel recovery page.
- A refused install shows the three-line message from D5 and leaves the other plugins running.
- Plugin rows in a built-in Settings tab sit under a divider that names the plugin, with a `▏` gutter.

## Errors and edge cases

| Situation | Result | Code or text |
|---|---|---|
| T2 or T3 package installed on Windows or macOS | Install refused | `Cannot install <name> <version>.` |
| `process.run:sh` requested by a T2 or T3 package | Install refused | `manifest.grant-tier` |
| `limits_tier: "first-party"` in a package that is not a bundled `ilium.*` plugin | Install refused | `manifest.limits-tier` |
| Package assets above the tier limit | Install refused | `manifest.asset-size` |
| Limit request above the tier ceiling | Clamped to the ceiling and logged | warn line in the plugin log |
| Third failure within 10 minutes | Plugin disabled with a notice | tree footer notice |
| Capability revoked during a call | Call fails and the plugin is re-activated | `revoked` |
| Old `.iliumanim` package installed | Refused, rebuild required | the loader does not accept the format |
| Declined `open-external`, geocoding or a new host from a setting | The call or submission is refused | `not-permitted` |
| Journaled input in flight when the server stops | Reported once, never replayed | `input-uncertain` |

<div class="note">
<p>The restart delays after a failure are 1 s, then 5 s; the third failure in the window disables the plugin. A failure counts for 10 minutes. A manual re-enable resets the count to zero. A revocation, a disable, an upgrade or a shutdown never counts as a failure.</p>
</div>

<div class="warn">
<p>Do not promise Windows or macOS support for a T2 or T3 plugin. Until a sandbox exists there, a third-party package does not install on those systems at all. Test third-party plugins on Linux, and state the platform limit in the plugin description. Check this page before a release.</p>
</div>

### See also

- [Security model](/pages/security/)
- [Roadmap](/pages/roadmap/)
- [Porting from .iliumanim](/pages/porting-iliumanim/)
- [Glossary](/pages/glossary/)
- [Lifecycle](/pages/lifecycle/)
- [Packages](/pages/packages/)
- [Plugin settings extension](/pages/settings-extension/)
- [Versioning](/pages/versioning/)
- [Rust SDK reference](/pages/rust-sdk/)
- [TypeScript SDK reference](/pages/typescript-sdk/)

Checked against the API version `ilium:plugin@0.1.0`. D1 to D27 and SE1 to SE20 are decided.

### Security

**SD1 No unconfined fallback.** If the sandbox cannot be built, the package does not load ([SEC-055]). **SD2 Fail closed for high veto points.** `on_failure: refuse` is the default ([SEC-287]). **SD3 Project values untrusted by default.** ([SEC-310]). **SD4 Key pinning.** ([SEC-142]).

## D27: an optional registry protocol

**Question.** D3 said there is no registry and no download. Packages now need discovery, updates and revocation. Does that contradict D3?

**Decision.** Ilium defines a TUF-style static registry: a folder of signed JSON files and package blobs on any HTTPS host or file share. The client verifies the metadata with a root key that the user or administrator pins. Ilium ships with no registry configured and operates no central store. Ratings are out of scope for 0.1. Automatic updates are off by default. A package file install keeps working with no registry. Mirrors and offline bundles serve isolated machines.

**Reason.** Static files need no server code, no database and no account system. A user or company can run one. Revocation and update checks need signed, expiring metadata, and TUF gives these properties in a known form.

**Effect on authors.** You can still pass a file on. You can also publish to a registry with `ilium plugin publish`. See [Registry reference](/pages/reference-registry/), [Signing and trust](/pages/reference-signing-and-trust/) and [Publishing guide](/pages/guide-publishing/). D3 stays valid for its signing and trust-per-key parts.
