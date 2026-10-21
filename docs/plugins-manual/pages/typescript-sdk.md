---
layout: page
title: TypeScript SDK reference
permalink: /pages/typescript-sdk/
---

## TypeScript SDK reference

The TypeScript SDK, `@ilium/plugin-sdk`, gives a TypeScript plugin generated types for the WIT contract, `definePlugin` and `defineScene` to bind the plugin's exports, `async` wrappers for slow host calls, and a test double for running handlers without the host. Bun 1.2 or newer bundles and runs it; tests run with `bun test`.

The host loads a plugin as a WebAssembly component. It calls the plugin's exports with batches of events and applies the submissions the plugin returns. The SDK maps both directions to ordinary functions, objects and promises.

The page documents the contract in full. Every listing appears in Rust and in TypeScript, and the Rust SDK reference page mirrors this one section by section; see [Rust SDK reference](/pages/rust-sdk/). The widget catalogue has 51 widgets in seven families, and the host adds thirteen interfaces, four optional exports and 24 grants.

### You will need

- Bun 1.2 or newer.
- The `ilium` command, for `ilium plugin new`, `build` and `pack`.
- A plugin manifest; see [Manifest reference](/pages/reference-manifest/).

Create a project with `ilium plugin new hello-ts --id com.example.hello --lang typescript --side client`.

## Project layout and package files

`ilium plugin new <dir> --id <plugin-id> --lang <rust|typescript> --side <server|client|both|scene>` writes the manifest and the language files. The two languages produce the same package: one component per declared side, signed and listed in the manifest.

**Rust**

```toml
[package]
name = "hello-plugin"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
ilium-plugin-sdk = "0.1"

[dev-dependencies]
ilium-plugin-sdk = { version = "0.1", features = ["testing"] }
```

**TypeScript**

```json
{
  "name": "hello-plugin",
  "version": "0.1.0",
  "type": "module",
  "dependencies": { "@ilium/plugin-sdk": "0.1.0" },
  "scripts": { "test": "bun test" }
}
```

The generated files are `manifest.json`, `Cargo.toml` and `src/lib.rs` for Rust, and `manifest.json`, `package.json` and `src/<side>.ts` for TypeScript (`src/server.ts`, `src/client.ts` or `src/scene.ts`). `ilium plugin new` does not run `bun install`; run it yourself. A folder that holds both `Cargo.toml` and `package.json` stops the build with `toolchain.language-ambiguous`.

## How a plugin is built

`ilium plugin build` compiles the plugin for WASI Preview 2, wraps the result as a component and writes one file per declared side to `dist/`, named after the manifest `entry` (for example `dist/server.wasm`). `ilium plugin pack` writes the `.iliumpkg` package to the same folder. The build runs on the author's machine. The host checks the file inventory and the signature at install time, so an SDK build never skips those checks. A component links only the imports its manifest requests, so the build needs no extra flag for the optional host services.

<figure>
<svg viewBox="0 0 380 100" role="img" aria-label="TypeScript plugin build path">
  <defs><marker id="bp-a" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto"><path d="M0 0 L10 5 L0 10 z" fill="currentColor"/></marker></defs>
  <rect x="4" y="34" width="70" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="39" y="56" text-anchor="middle" fill="currentColor" font-size="11">TS sources</text>
  <path d="M74 52 L96 52" stroke="currentColor" fill="none" marker-end="url(#bp-a)"/>
  <text x="85" y="22" text-anchor="middle" fill="currentColor" font-size="10">bundle</text>
  <rect x="98" y="34" width="70" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="133" y="56" text-anchor="middle" fill="currentColor" font-size="11">Component</text>
  <path d="M168 52 L190 52" stroke="currentColor" fill="none" marker-end="url(#bp-a)"/>
  <text x="179" y="22" text-anchor="middle" fill="currentColor" font-size="10">pack</text>
  <rect x="192" y="34" width="70" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="227" y="56" text-anchor="middle" fill="currentColor" font-size="11">.iliumpkg</text>
  <path d="M262 52 L284 52" stroke="currentColor" fill="none" marker-end="url(#bp-a)"/>
  <text x="273" y="22" text-anchor="middle" fill="currentColor" font-size="10">install</text>
  <rect x="286" y="34" width="70" height="36" rx="6" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
  <text x="321" y="56" text-anchor="middle" fill="currentColor" font-size="11">Host</text>
</svg>
<figcaption>The SDK covers the first two steps. The host checks the package inventory and signature in the last step.</figcaption>
</figure>

<pre class="term"><span class="t-prompt">$</span> ilium plugin build
<span class="t-dim">{"type":"progress","step":"compile"}</span>
<span class="t-ok">{"type":"artifact","path":"dist/server.wasm"}</span>
<span class="t-dim">{"type":"summary","warnings":0,"errors":0}</span>
{"type":"result","ok":true}
</pre>

Illustrative, not generated. Every line is one JSON object in the real output; the lines are shortened for width.

## Exports and entry points

Every server and client component exports a lifecycle and a handler. The manifest `exports` list names the optional exports the component adds, and the entry must match that list exactly (`manifest.exports-mismatch` otherwise). Four of the optional exports are `catalog-provider`, `menu-provider`, `virtual-rows-provider` and `token-provider`.

| Manifest export | Rust trait and methods | TypeScript member of `definePlugin` | Side |
|---|---|---|---|
| (always) lifecycle | `Lifecycle`: `activate(&mut self, Activation) -> Result<(), PluginError>`, `deactivate(&mut self, DeactivationReason) -> Option<Vec<u8>>` | `activate(context)`, `deactivate(reason)` returned as `lifecycle` | S C |
| (always) handler | `Handler`: `on_events(&mut self, Vec<Event>) -> Vec<Submission>` | `onEvents(batch)` returned as `handler` | S C |
| `views` | `Views`: `render(&mut self, ViewTarget, Area) -> UiTree` | `render(view, area)` returned as `views` | C |
| `detector` | `Detector`: `identify`, `classify` | `identify(processes)`, `classify(signature, screen)` returned as `detector` | S |
| `job-runner` | `JobRunner`: `run(kind, input) -> Result<Vec<u8>, PluginError>` | `run(kind, input)` returned as `jobRunner` | S C A |
| `service-provider` | `ServiceProvider`: `handle(call) -> ServiceReply` | `handle(call)` returned as `serviceProvider` | S C |
| `inference-provider` | `InferenceProvider`: `complete(call)`, `models()` | `complete(call)`, `models()` returned as `inferenceProvider` | S C |
| `annotator` | `Annotator`: `annotate(buffer, revision, first_line, lines)` | `annotate(buffer, revision, firstLine, lines)` returned as `annotator` | C |
| `catalog-provider` | `CatalogProvider`: `query_catalog(&mut self, CatalogQuery) -> Result<CatalogPage, PluginError>` | `queryCatalog(query)` returned as `catalogProvider` | S C |
| `menu-provider` | `MenuProvider`: `menu_items(&mut self, MenuContext) -> Vec<MenuEntry>` | `menuItems(context)` returned as `menuProvider` | C |
| `virtual-rows-provider` | `VirtualRowsProvider`: `list_virtual_children(&mut self, row_kind, parent, cursor) -> Result<(Vec<VirtualRowChild>, Option<String>), PluginError>` | `listVirtualChildren(rowKind, parent, cursor)` returned as `virtualRowsProvider` | S C |
| `token-provider` | `TokenProvider`: `tokens(&mut self, buffer, first_line, lines, state_in) -> (Vec<Token>, Option<Vec<u8>>)` | `tokens(buffer, firstLine, lines, stateIn)` returned as `tokenProvider` | C |
| `hook-handler` | `HookHandler`: `handle_hook(&mut self, call: HookCall) -> HookReply` | `hookHandler(call)` member of `definePlugin` | S C |

The method and member names of the four new exports follow the naming rule below; the WIT function names are `query-catalog`, `menu-items`, `list-virtual-children` and `tokens`. The deadlines are 20 ms for the catalogue, menu and virtual-row providers and 10 ms per call for the token provider.

**Rust**

```rust
#[ilium_plugin(side = "client", exports = ["views", "catalog-provider", "token-provider"])]
#[derive(Default)]
struct MyPlugin { /* state */ }
```

**TypeScript**

```ts
export const { lifecycle, handler, views, catalogProvider, tokenProvider } = definePlugin({
  side: 'client',
  activate(context) {},
  onEvents(batch) { return []; },
  render(view, area) { /* returns a UiTree */ },
  queryCatalog(query) { /* returns a CatalogPage */ },
  tokens(buffer, firstLine, lines, stateIn) { /* returns [tokens, state] */ },
});
```

The Rust attribute takes `side` (`"server"`, `"client"` or `"scene"`, required) and `exports` (the manifest names). A listed export without its trait implementation is a compile error, and the struct must implement `Default`. In TypeScript, `definePlugin` takes `side` and the members you implement; the returned names are the interface names in camelCase, and the entry module must export exactly the names the manifest lists plus `lifecycle` and `handler`.

## Modules, import paths and names

Each WIT package maps to one Rust module and one TypeScript subpath. One rule covers every name: WIT kebab-case becomes snake_case for Rust functions, fields and modules, camelCase for TypeScript functions and fields, UpperCamelCase or PascalCase for types, and kebab-case strings for TypeScript variant tags and enum values.

| WIT form | Rust | TypeScript |
|---|---|---|
| function `read-cells` | `read_cells` | `readCells` |
| record `catalog-item`, field `disabled-reason` | `CatalogItem { disabled_reason }` | `CatalogItem { disabledReason }` |
| variant `event`, case `screen-changed(x)` | `Event::ScreenChanged(x)` | `{ tag: 'screen-changed', val: x }` |
| enum `tone`, case `warning` | `Tone::Warning` | `'warning'` |
| flags `node-flags` | `NodeFlags::HOVER_EVENTS \| NodeFlags::POINTER_EVENTS` | `{ hoverEvents: true, pointerEvents: true }` |
| `option<T>`, `result<T, plugin-error>` | `Option<T>`, `Result<T, PluginError>` | `T \| undefined`, returns `T` or throws `PluginError` |
| `u64`, `s64` | `u64`, `i64` | `bigint`, always |
| `list<u8>`, `list<f32>` | `Vec<u8>`, `Vec<f32>` | `Uint8Array`, `number[]` |
| `tuple<A, B>` | `(A, B)` | `[A, B]` |
| resource `child`, `font` | `Child`, `Font` handles that terminate or release on drop | `Child`, `Font` classes with `close()` |
| WIT keyword `loop`, `type` | `looped`, `type_` | `loop`, `type` |

| Module | Rust | TypeScript |
|---|---|---|
| Common names | `ilium_plugin_sdk::prelude::*` | `@ilium/plugin-sdk` |
| Shared records | `types::{ids, errors, text, paths, editing, layout, geo, net, tree, ui, requests, intents, events, lifecycle}` | `@ilium/plugin-sdk/types` |
| Host imports, core | `host::{fs, http, jobs, process, settings, storage, timers, ui, ...}` | `@ilium/plugin-sdk/host/<interface>` |
| Host imports, services | `host::{extensions, catalogs, geocode, sessions, vector, kernel_settings, keymap, journal, client_requests, gpu}` | `host/extensions`, `host/catalogs`, `host/geocode`, `host/sessions`, `host/vector`, `host/kernel-settings`, `host/keymap`, `host/journal`, `host/client-requests`, `host/gpu` |
| Primitives | `primitives::{terminal, agent, editor, basic}` | `@ilium/plugin-sdk/primitives/<interface>` |
| Scene surface | `anim::{surface, renderer, fonts}` | `@ilium/plugin-sdk/anim/surface`, `anim/renderer`, `anim/fonts` |
| UI tree builder | `ui::build::{Tree, NodeBuilder}` | `@ilium/plugin-sdk/ui/build` |
| Export traits | `exports::{Lifecycle, Handler, Views, ..., CatalogProvider, MenuProvider, VirtualRowsProvider, TokenProvider, HookHandler}` | returned by `definePlugin` |
| Executor | `task`, `request` | built into `definePlugin` |
| Fake host | `testing` (feature `testing`) | `@ilium/plugin-sdk/testing` |

A type lives in the module of the WIT interface that declares it, and the module list above for `types` is the list of `ilium:types` interfaces. The types of other packages are not in `types`: `host::geocode::GeocodeQuery`, `host::catalogs::CatalogQuery`, `host::inference::{ChatMessage, CompletionRequest}`, `exports::menu_provider::MenuContext`, `exports::virtual_rows_provider::VirtualRowChild` and `exports::token_provider::{Token, TokenKind}`. In TypeScript the same types come from `@ilium/plugin-sdk/host/geocode`, `host/catalogs`, `host/inference`, `exports/menu-provider`, `exports/virtual-rows-provider` and `exports/token-provider`. There is no `types::catalogs`, `types::menus`, `types::tokens`, `types::fs`, `types::agent`, `types::editor` or `types::primitives` module.

Which worlds may import which interface is fixed per side, and the table in [Worlds and sides](/pages/worlds-and-sides/) lists it: `vector`, `kernel-settings`, `journal`, `client-requests`, `catalogs` and `extensions` exist on server and client; `geocode` on server, client and scenes; `keymap` on the client only; `sessions` on the server only; `gpu` on client and scenes; `fonts` on scenes only.

## Values, submissions and outcomes

**Rust**

```rust
// A submission carries an intent and a correlation number.
let plain = Submission::new(Intent::DismissToast(DismissToastSpec { id: "copied".into() }));
// A keyed submission is de-duplicated for 10 minutes.
let keyed = Submission::keyed(
    Intent::FreezePane(FreezePaneSpec { pane }),
    dedup::key_for(&["freeze", &pane.to_string()]),
);
// Outcome has a case that carries the new prompt epoch.
fn describe(outcome: &Outcome) -> &'static str {
    match outcome {
        Outcome::Ok | Outcome::OkNode(_) | Outcome::OkEpoch(_) => "applied",
        Outcome::Duplicate(_) => "already done",
        Outcome::Refused(_) | Outcome::Failed(_) => "not applied",
        Outcome::Cancelled => "cancelled",
    }
}
```

**TypeScript**

```ts
// A submission is a plain object; correlation is a bigint.
const plain: Submission = {
  intent: { tag: 'dismiss-toast', val: { id: 'copied' } },
  correlation: 1n,
  dedupKey: undefined,
};
// A keyed submission is de-duplicated for 10 minutes.
const keyed: Submission = {
  intent: { tag: 'freeze-pane', val: { pane } },
  correlation: 2n,
  dedupKey: `freeze:${pane}`,
};
// Outcome has a case that carries the new prompt epoch.
function describe(outcome: Outcome): string {
  switch (outcome.tag) {
    case 'ok': case 'ok-node': case 'ok-epoch': return 'applied';
    case 'duplicate': return 'already done';
    case 'refused': case 'failed': return 'not applied';
    default: return 'cancelled';
  }
}
```

The outcome has the case `ok-epoch(u64)`, returned by `send-input` on an agent pane. `Outcome::is_ok()` in Rust and `isOk()` in TypeScript treat `ok`, `ok-node` and `ok-epoch` alike. Choose your own correlation numbers below 2^63; the SDK uses numbers from 2^63 upward. A dedup key is 1 to 128 characters from `[A-Za-z0-9._:/-]`.

## Calls, requests and errors

Fast host calls return at once. Slow calls have an async wrapper named after the WIT function: Rust `fs::read_all(&path).await`, TypeScript `await readAll(path)`. The SDK runs one single-threaded executor and resolves each awaited call from the `request-completed` event, removing that event from the batch before the handler sees it. The suffix rules for the starting form (`_request` / `Request`) and for names that end in `-json` are in [Wrapper names in the SDKs](/pages/reference-wit/#wrapper-names-in-the-sdks). Every call that returns a `request-id` follows the same pattern: `geocode::search`, `fs::write_atomic`, `vector::embed`, `terminal::history_text`, `agent::transcript`, `sessions::send` and `client_requests::request`.

<figure>
<svg viewBox="0 0 400 240" role="img" aria-label="Lifecycle of an awaited host call">
  <defs><marker id="rq-a" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto"><path d="M0 0 L10 5 L0 10 z" fill="currentColor"/></marker></defs>
  <rect x="20" y="14" width="200" height="32" rx="6" fill="none" stroke="currentColor"/>
  <text x="120" y="34" text-anchor="middle" font-size="11" fill="currentColor">Plugin awaits a host call</text>
  <path d="M120 46 L120 72" stroke="currentColor" fill="none" marker-end="url(#rq-a)"/>
  <rect x="20" y="74" width="200" height="32" rx="6" fill="none" stroke="currentColor"/>
  <text x="120" y="94" text-anchor="middle" font-size="11" fill="currentColor">Host completes the request</text>
  <path d="M120 106 L120 132" stroke="currentColor" fill="none" marker-end="url(#rq-a)"/>
  <rect x="20" y="134" width="200" height="32" rx="6" fill="none" stroke="currentColor"/>
  <text x="120" y="154" text-anchor="middle" font-size="11" fill="currentColor">Event enters the batch</text>
  <path d="M120 166 L120 192" stroke="currentColor" fill="none" marker-end="url(#rq-a)"/>
  <rect x="20" y="194" width="200" height="32" rx="6" fill="none" stroke="currentColor"/>
  <text x="120" y="214" text-anchor="middle" font-size="11" fill="currentColor">Awaiting task resumes</text>
</svg>
<figcaption>The SDK removes each awaited completion from the batch, so the handler sees only completions that no await is waiting for.</figcaption>
</figure>

**Rust**

```rust
// Several awaited calls in one task; each await is one request.
async fn refresh(pane: NodeId) {
    let text = terminal::history_text(pane, 1 << 20).await;           // terminal.history
    let vectors = vector::embed(&["hello".to_string()], "search").await; // inference.embed
    match (text, vectors) {
        (Ok(history), Ok(_)) => log::log(Level::Info, &format!("{} bytes", history.len())),
        (Err(error), _) | (_, Err(error)) => {
            if matches!(error.code, ErrorCode::RateLimited) {
                log::log(Level::Warn, &format!("retry in {} ms", error.retry_after_ms.unwrap_or(0)));
            }
        }
    }
}
// Started from activate: task::spawn(refresh(pane));
```

**TypeScript**

```ts
// Several awaited calls in one function; each await is one request.
async function refresh(pane: bigint): Promise<void> {
  try {
    const history = await historyText(pane, 1 << 20);                 // terminal.history
    await embed(['hello'], 'search');                                  // inference.embed
    log('info', `${history.length} bytes`);
  } catch (error) {
    if (error instanceof PluginError && error.code === 'rate-limited') {
      log('warn', `retry in ${error.retryAfterMs} ms`);
    }
  }
}
// Started from activate: void refresh(pane);
```

A `PluginError` has `code`, `message` and `retry_after_ms` (`retryAfterMs` in TypeScript). The fifteen codes are `not-permitted`, `not-found`, `invalid-argument`, `conflict`, `quota-exceeded`, `rate-limited`, `busy`, `timed-out`, `cancelled`, `unsupported`, `pane-not-ready`, `revoked`, `io`, `network` and `internal`; the meanings and retry rules are on [Errors and limits](/pages/reference-errors-and-limits/). The host uses them like this:

| Situation | Code |
|---|---|
| Stale `expected` revision in `write_atomic`, `rename-node` with a stale `expected-revision`, `if-current` mismatch in `send-input` | `conflict` |
| Mouse events with mouse reporting off, `freeze-pane` on Windows, `agent::transcript` for a signature without a format, `embed` without a provider | `unsupported` |
| Geocoding before the user consented, a declined `open-external` | `not-permitted` |
| More than 3 toasts in 10 s, geocoding faster than 1 per second | `rate-limited` with `retry_after_ms` |
| Detached client in `client_requests::request` | `not-found` |
| No answer within the request timeout | `timed-out` |
| Popup stack full (4 per view) | `quota-exceeded` |

A contract violation is not an error value. Calling an import the manifest never requested, returning an invalid UI tree, exceeding a fixed cap or breaching a deadline traps the instance. `on_events`, `render` and `deactivate` have no channel for a `PluginError`, so they cannot report one.

## Building UI trees

A client plugin with the `views` export returns a `UiTree` from `render`. Both SDKs provide a builder that fills the defaults WIT cannot express and the node indices. The widget catalogue has 51 widgets in seven families; the props, events and keyboard rules of each are on the [widgets pages](/pages/widgets-index/).

Builder rules:

- Rust: `Tree::new()` creates a tree. The required prop is the constructor argument, for example `tree.paragraph(lines)` or `tree.dialog("Delete file?")`. Every optional prop is a chained setter named after its WIT field in snake_case (`.min_first(20)`, `.reorderable(true)`). `.key("k")` sets the action id, `.flags(...)` and `.tooltip(...)` set the node flags and the hover help. `tree.finish(root)` returns the `UiTree`.
- TypeScript: `@ilium/plugin-sdk/ui/build` exports one function per widget in camelCase, and `tree(root)`. Containers take `(props, children)`; leaves take the required prop first and an optional props object second. Enum values are the WIT case names as strings.
- The widget `key` is the action id: a `ui-action` event carries it, and a widget without a key sends nothing.

| Family | Widgets (Rust `tree.` constructor, TypeScript function) |
|---|---|
| Layout and containers | `layout`, `block`, `tabs`, `popup`, `clear`, `scrollbar`, `split`, `separator`, `dialog`, `scroll_view` / `scrollView` |
| Text | `paragraph`, `markdown`, `leader_row` / `leaderRow`, `key_hints` / `keyHints`, `diff_view` / `diffView` |
| Collections | `list`, `table`, `tree_list` / `treeList`, `menu`, `check_list` / `checkList`, `list_editor` / `listEditor` |
| Data display | `gauge`, `line_gauge` / `lineGauge`, `sparkline`, `bar_chart` / `barChart`, `chart`, `canvas`, `calendar`, `progress_row` / `progressRow`, `meter`, `minimap` |
| Input | `button`, `toggle`, `choice`, `number`, `text_input` / `textInput`, `text_area` / `textArea`, `schema_form` / `schemaForm`, `key_capture` / `keyCapture`, `color_picker` / `colorPicker`, `path_field` / `pathField`, `location_picker` / `locationPicker` |
| Status and feedback | `badge`, `spinner`, `notice`, `icon` |
| Media and embedded | `image`, `surface`, `code_view` / `codeView`, `terminal_view` / `terminalView`, `video` |

Every node also accepts these node fields: `tooltip`, `tab_index` / `tabIndex` (focus order, lower first) and `disabled_reason` / `disabledReason` (the node is drawn muted and the reason is its tooltip). `NodeFlags` gains `HOVER_EVENTS` and `POINTER_EVENTS` (`hoverEvents`, `pointerEvents`). Overlays are not widgets: a tooltip is a node field, a popover is a non-modal `popup`, a toast is an intent, and the command palette is a host surface.

### Layout and containers

A file panel with a resizable `split`, a `separator` and a `dialog`. The dialog sits in a `length(0)` slot, so it takes no space until it is opened.

**Rust**

```rust
// ui::build::Tree; types::layout::{Direction, Constraint}; types::ui::DialogKind
impl Views for Explorer {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        let mut tree = Tree::new();
        let files = tree.paragraph(self.file_lines()).key("files");
        let preview = tree.markdown(self.preview_source()).key("preview");
        let body = tree.split(Direction::Horizontal)
            .position(350).min_first(20).min_second(30)
            .child(files).child(preview).key("main");
        let rule = tree.separator(Direction::Horizontal).label("Selected file");
        let confirm = tree.dialog("Delete file?")
            .kind(DialogKind::DangerConfirm)
            .message(vec![vec![Span::plain(self.selected_name())]])
            .key("confirm-delete");
        let root = tree
            .layout(Direction::Vertical, [Constraint::Fill(1), Constraint::Length(1), Constraint::Length(0)])
            .child(body).child(rule).child(confirm);
        tree.finish(root)
    }
}
```

**TypeScript**

```ts
import { tree, layout, split, separator, dialog, paragraph, markdown } from '@ilium/plugin-sdk/ui/build';

render(view, area) {
  return tree(layout({ direction: 'vertical', constraints: [{ tag: 'fill', val: 1 }, { tag: 'length', val: 1 }, { tag: 'length', val: 0 }] }, [
    split({ key: 'main', direction: 'horizontal', position: 350, minFirst: 20, minSecond: 30 }, [
      paragraph(fileLines(), { key: 'files' }),
      markdown(previewSource(), { key: 'preview' }),
    ]),
    separator('horizontal', { label: 'Selected file' }),
    dialog({ key: 'confirm-delete', title: 'Delete file?', kind: 'danger-confirm', message: [[plain(selectedName())]] }, []),
  ]));
},
```

The dialog opens with `ui::open_popup("confirm-delete", ...)` in Rust and `openPopup('confirm-delete', ...)` in TypeScript. This is how the opened view looks in a 62-column area, with the danger border of the dialog drawn in the warning colour:

<pre class="term">╭  ≡ ● · Files───────────────────────────────────────────────╮
│ ▼ src                            │ # Notes                 │
│ › ▶ lib.rs                       │ Plugin entry point.     │
│ › main.rs                        │                         │
│──Selected file─────────────────────────────────────────    │
│                                                            │
│  ╭  Delete file?────────────────╮                          │
│  │ main.rs                      │                          │
│  │ Esc Cancel   Enter OK        │                          │
│  ╰──────────────────────────────╯                          │
│ Esc cancel · Enter ok                                      │
╰────────────────────────────────────────────────────────────╯
</pre>

Not generated; the glyphs are those the client draws (rounded corners, `─` rules, `│` dividers, `·` between hints).

### Text

**Rust**

```rust
// types::text::{RichSpan, SpanTarget}; types::ui::{DiffLayout}
let advice = vec![vec![
    RichSpan::plain("Threshold too low. "),
    RichSpan::new("Apply 180k", Style::default())
        .target(SpanTarget::Action("apply-180k".into()))
        .tooltip("Writes the recommended threshold and keeps a revert point"),
]];
let text = tree.paragraph(Vec::new()).rich(advice).key("advice");   // `rich` replaces `lines`
let diff = tree.diff_view(self.hunks.clone())
    .layout(DiffLayout::Split).language("toml").word_diff(true).key("diff");
```

**TypeScript**

```ts
import { paragraph, diffView, richSpan } from '@ilium/plugin-sdk/ui/build';

const text = paragraph([], { key: 'advice', rich: [[
  richSpan('Threshold too low. '),
  richSpan('Apply 180k', {
    target: { tag: 'action', val: 'apply-180k' },
    tooltip: 'Writes the recommended threshold and keeps a revert point',
  }),
]] });
const diff = diffView(hunks, { key: 'diff', layout: 'split', language: 'toml', wordDiff: true });
```

A click on the span arrives as a `ui-action` on key `advice` with kind `activated` and value `text("apply-180k")`. `markdown` gains `task_click`, `images` and `base`; with `task_click`, a task item sends `toggled` with the line number and the plugin rewrites its source.

### Collections

**Rust**

```rust
// types::ui::{CheckItem, ItemFormat}; types::text::TriState
let members = tree.check_list(vec![
    CheckItem::new("agents", "All agents").state(TriState::Mixed).children(["p1", "p2"]),
    CheckItem::new("p1", "claude: refactor").state(TriState::On),
    CheckItem::new("p2", "codex: tests").state(TriState::Off).disabled_reason("frozen"),
]).all_toggle(true).key("members");
let globs = tree.list_editor(self.ignore_globs.clone())
    .item_format(ItemFormat::Glob).reorderable(true).max_items(64).key("ignore");
// events: ActionValue::ItemState(id, state) on "members"; ActionValue::Text(json_array) on "ignore"
```

**TypeScript**

```ts
import { checkList, listEditor } from '@ilium/plugin-sdk/ui/build';

const members = checkList([
  { id: 'agents', label: 'All agents', state: 'mixed', children: ['p1', 'p2'] },
  { id: 'p1', label: 'claude: refactor', state: 'on', children: [] },
  { id: 'p2', label: 'codex: tests', state: 'off', disabledReason: 'frozen', children: [] },
], { key: 'members', allToggle: true });
const globs = listEditor(ignoreGlobs, { key: 'ignore', itemFormat: 'glob', reorderable: true, maxItems: 64 });
// events: { tag: 'item-state', val: [id, state] } and { tag: 'text', val: jsonArray }
```

`menu` takes `entries` (submenus to depth 2, icons, tri-state checks, disabled reasons, tooltips, danger items); `list`, `table` and `tree_list` gain filter, multi-select (`ActionValue::Ids`), `follow_tail`, item tooltips and the responsive `columns` of `table`.

### Data display

**Rust**

```rust
// types::ui::{Dataset, ChartRender, Resample, MeterSegment, ProgressState}; types::text::Tone
let tokens = tree.chart(vec![
    Dataset::new("input", self.input_series()).gaps(true).resample(Resample::MinMax),
    Dataset::new("output", self.output_series()).gaps(true).resample(Resample::MinMax),
]).render(ChartRender::TextLine).y_label_width(6).key("tokens");
let spend = tree.meter(vec![
    MeterSegment { label: "today".into(), value: 4.20, tone: Tone::Info },
    MeterSegment { label: "over budget".into(), value: 0.80, tone: Tone::Danger },
]).max(5.0).show_values(true).key("spend");
let job = tree.progress_row("Replaying 412 sessions")
    .state(ProgressState::Running).percent(37.0).message("session 152 of 412").key("job");
```

**TypeScript**

```ts
import { chart, meter, progressRow } from '@ilium/plugin-sdk/ui/build';

const tokens = chart([
  { name: 'input', points: inputSeries(), gaps: true, resample: 'min-max' },
  { name: 'output', points: outputSeries(), gaps: true, resample: 'min-max' },
], { key: 'tokens', render: 'text-line', yLabelWidth: 6 });
const spend = meter([
  { label: 'today', value: 4.2, tone: 'info' },
  { label: 'over budget', value: 0.8, tone: 'danger' },
], { key: 'spend', max: 5, showValues: true });
const job = progressRow('Replaying 412 sessions', { key: 'job', state: 'running', percent: 37, message: 'session 152 of 412' });
```

The progress row draws the same status glyphs as the progress footer of the client:

<pre class="term">╭  ≡ ● · Jobs────────────────────────────────────────────────╮
│ ▶ RUNNING  37%  ███░░░░░░░  session 152 of 412             │
│ ✓ DONE  Replay finished                                    │
│ ✕ ERROR  Replay failed                                     │
│ ! MONITOR FAILED  outcome unknown                          │
╰────────────────────────────────────────────────────────────╯
</pre>

### Input

**Rust**

```rust
// types::ui::{ChoiceStyle, ViewMode}; types::geo::GeoPoint
let palette = tree.choice(Vec::new()).source("ilium.palettes")
    .style(ChoiceStyle::Dialog).filter(true).key("palette");
let icon = tree.choice(Vec::new()).source("ilium.icons")
    .style(ChoiceStyle::Dialog).view_modes([ViewMode::List, ViewMode::Grid]).key("icon");
let chord = tree.key_capture().chord(self.chord.clone()).reserved_check(true).key("chord");
let place = tree.location_picker()
    .value(self.place)              // Option<GeoPoint>
    .label(self.place_label.clone())
    .search(true)                   // needs location.geocode
    .allow_current(true)            // needs location.observer
    .zoom(true)
    .key("place");

// in on_events
if let Event::UiAction(UiAction { key, kind: ActionKind::Submitted, value: ActionValue::Geo(point), .. }) = event {
    if key == "place" { self.place = Some(point); }
}
```

**TypeScript**

```ts
import { choice, keyCapture, locationPicker } from '@ilium/plugin-sdk/ui/build';

const palette = choice([], { key: 'palette', source: 'ilium.palettes', style: 'dialog', filter: true });
const icon = choice([], { key: 'icon', source: 'ilium.icons', style: 'dialog', viewModes: ['list', 'grid'] });
const chord = keyCapture({ key: 'chord', chord, reservedCheck: true });
const place = locationPicker({ key: 'place', value: place, label: placeLabel, search: true, allowCurrent: true, zoom: true });

// in onEvents
if (event.tag === 'ui-action' && event.val.key === 'place'
    && event.val.kind === 'submitted' && event.val.value.tag === 'geo') {
  place = event.val.value.val; // { lat, lon }
}
```

In a settings view, `location_picker` writes without any event handling when it has a `bind` prop (Rust `.bind("sky.location")`, TypeScript `bind: 'sky.location'`) naming one of the plugin's own `location` rows. The other input props: `choice` `items`, `source`, `filter`, `view_modes`, `disabled_marker`, `allow_custom` and `selected_id`; `number` `style` (`stepper`, `slider`), `steps`, `special_values`, `scale` and `debounce_ms`; `toggle` `style` (`switch`, `checkbox`, `icon`) and `tri_state`; `text_input` `debounce_ms`, `reveal_last`, `validation` and `suggestions`; `color_picker` `value`, `palette` and `allow_theme_roles`; `path_field` `kind`, `extensions` and `editable`.

Choice, number, toggle and text rows draw with the leader rule: the label, one blank cell, dots, one blank cell, the control. A gap shorter than five cells stays blank, and the dots are never a hit target.

<pre class="term">╭  ≡ ● · Settings────────────────────────────────────────────╮
│ Palette …………………………… ← Sunset … → +                         │
│ Speed ………………………………… - 42 + *                               │
│ Notifications ……………… [ On ]                                │
│ Density ………… ─────●─────                                   │
╰────────────────────────────────────────────────────────────╯
</pre>

<pre class="term">╭  ≡ ● · Palette─────────────────────────────────────────────╮
│ ● Sunset                                                   │
│   Ocean                                                    │
│   Mono (needs Color mode)                              ┃   │
│ 3 of 24 options · Current: Sunset                      │   │
╰────────────────────────────────────────────────────────────╯
</pre>

### Status and feedback

**Rust**

```rust
// types::ui::SpinnerFrames; types::text::Tone
let unread = tree.badge().count(self.unread).tone(Tone::Info).key("unread");
let busy = tree.spinner(SpinnerFrames::Braille).interval_ms(100).label("Indexing");
let warn = tree.notice(vec![vec![RichSpan::plain("The provider sends place names to photon.komoot.io.")]])
    .tone(Tone::Warning).title("Privacy").actions([("ok", "Understood")]).dismissible(true).key("privacy");
let model = tree.icon().slot("model.claude").label("Claude");
```

**TypeScript**

```ts
import { badge, spinner, notice, icon, richSpan } from '@ilium/plugin-sdk/ui/build';

const unread = badge({ key: 'unread', count: unreadCount, tone: 'info' });
const busy = spinner('braille', { intervalMs: 100, label: 'Indexing' });
const warn = notice([[richSpan('The provider sends place names to photon.komoot.io.')]], {
  key: 'privacy', tone: 'warning', title: 'Privacy', actions: [['ok', 'Understood']], dismissible: true,
});
const model = icon({ slot: 'model.claude', label: 'Claude' });
```

The spinner steps its frames in the host (`⠋ ⠙ ⠹ ⠸ ⠼ ⠴ ⠦ ⠧ ⠇ ⠏` for `braille`), so the plugin is not called per frame, and reduced motion shows one static frame.

### Media and embedded

**Rust**

```rust
// types::ui::{VideoSource, MediaFit}
let clip = tree.video(VideoSource::Asset("loops/shore.webm".into()))
    .autoplay(true).looped(true).fit(MediaFit::Cover).key("clip");
let term = tree.terminal_view(self.pane).interactive(true).scrollback(true).key("term"); // needs terminal.read-screen, and terminal.raw-input because it is interactive
```

**TypeScript**

```ts
import { video, terminalView } from '@ilium/plugin-sdk/ui/build';

const clip = video({ tag: 'asset', val: 'loops/shore.webm' }, { key: 'clip', autoplay: true, loop: true, fit: 'cover' });
const term = terminalView(pane, { key: 'term', interactive: true, scrollback: true }); // needs terminal.read-screen, and terminal.raw-input because it is interactive
```

### Hover, pointer, drag feedback, popovers and theme tokens

**Rust**

```rust
// Hover and pointer events are opt-in per node.
let cell = tree.paragraph(self.cell_lines()).key("cell")
    .flags(NodeFlags::FOCUSABLE | NodeFlags::HOVER_EVENTS | NodeFlags::POINTER_EVENTS)
    .tooltip(Tooltip::text("Drag to select"));
// A non-modal popover anchored to a widget of the same view.
ui::open_popup_with("stats", PopupAnchor::AtKey(KeyAnchor { key: "cell".into(), side: AnchorSide::Below }),
    PopupOptions { modal: false, pinnable: true, dismiss_on_leave: true, close_on_escape: true })?;
// Motion and density follow the user.
let extras = ui::theme_extras();
let frames = if extras.reduced_motion { 1 } else { 8 };
```

**TypeScript**

```ts
import { openPopupWith, themeExtras } from '@ilium/plugin-sdk/host/ui';

// Hover and pointer events are opt-in per node.
const cell = paragraph(cellLines(), {
  key: 'cell',
  flags: { focusable: true, hoverEvents: true, pointerEvents: true },
  tooltip: { lines: [[plain('Drag to select')]], delayMs: undefined },
});
// A non-modal popover anchored to a widget of the same view.
openPopupWith('stats', { tag: 'at-key', val: { key: 'cell', side: 'below' } },
  { modal: false, pinnable: true, dismissOnLeave: true, closeOnEscape: true });
// Motion and density follow the user.
const frames = themeExtras().reducedMotion ? 1 : 8;
```

Widget state set by the host includes `check-list` item states (`tri`), `geo`, `number`, `split` (per mille) and `ids` cases. At most four popups are open per view.

## Reacting to UI actions

A `ui-action` event carries `view`, `key`, `kind` (`ActionKind`) and `value` (`ActionValue`). There are 17 kinds (including `hover-enter`, `hover-leave`, `pointer`, `drag-over`, `resized` and `filter-changed`) and 12 value cases (including `pointer`, `drag-over`, `item-state`, `geo` and `ids`). `scrolled`, `pointer` and `drag-over` are coalesced; the others are never dropped.

| New kind | Sent by | Value |
|---|---|---|
| `hover-enter`, `hover-leave` | nodes with `HOVER_EVENTS` | none |
| `pointer` | nodes with `POINTER_EVENTS` | `pointer(ViewPointer)` with kind, button, modifiers, column, row, `sub_x`, `sub_y`, clicks |
| `drag-over` | drop targets during a drag | `drag-over(DragOverValue { source_key, target_key, index, accept })` |
| `resized` | `split` | `number` (per mille) |
| `filter-changed` | `list`, `choice` | `text`, debounced 150 ms |

| New value | Rust | TypeScript |
|---|---|---|
| view pointer | `ActionValue::Pointer(ViewPointer)` | `{ tag: 'pointer', val: {...} }` |
| drag feedback | `ActionValue::DragOver(DragOverValue)` | `{ tag: 'drag-over', val: {...} }` |
| check item | `ActionValue::ItemState(String, TriState)` | `{ tag: 'item-state', val: [id, state] }` |
| place | `ActionValue::Geo(GeoPoint)` | `{ tag: 'geo', val: { lat, lon } }` |
| selection | `ActionValue::Ids(Vec<String>)` | `{ tag: 'ids', val: ['a', 'b'] }` |

**Rust**

```rust
// Answer a drag: the next render sets the target's accept prop.
let Event::UiAction(UiAction { key, kind, value, .. }) = event else { return };
match (kind, value) {
    (ActionKind::DragOver, ActionValue::DragOver(over)) => {
        self.accept = if over.index == 0 { DropAccept::Before } else { DropAccept::Inside };
    }
    (ActionKind::Pointer, ActionValue::Pointer(pointer)) if pointer.kind == PointerKind::Down => {
        self.start_selection(&key, pointer.column, pointer.row, pointer.sub_x);
    }
    (ActionKind::Resized, ActionValue::Number(per_mille)) => self.remember_split(per_mille),
    _ => {}
}
```

**TypeScript**

```ts
// Answer a drag: the next render sets the target's accept prop.
if (event.tag !== 'ui-action') return;
const { key, kind, value } = event.val;
if (kind === 'drag-over' && value.tag === 'drag-over') {
  accept = value.val.index === 0 ? 'before' : 'inside';
} else if (kind === 'pointer' && value.tag === 'pointer' && value.val.kind === 'down') {
  startSelection(key, value.val.column, value.val.row, value.val.subX);
} else if (kind === 'resized' && value.tag === 'number') {
  rememberSplit(value.val);
}
```

## Events and intents

The 18 events listed below arrive in `on_events` / `onEvents` like any other. The 23 intents and the five extended ones are submitted the same way (see [Events and intents](/pages/reference-events-and-intents/) for every payload). Names follow the naming rule: `Event::ScreenChanged(..)` and `{ tag: 'screen-changed', val }`, `Intent::FreezePane(FreezePaneSpec)` and `{ tag: 'freeze-pane', val }`.

| Area | Intents | Extended intents |
|---|---|---|
| Pane lifecycle (`panes.lifecycle`) | `freeze-pane`, `unfreeze-pane`, `replace-pane-command`, `terminate-pane-process`, `resize-pane` | `send-input` (`if-current`, `lock-user-input`, returns `ok-epoch`) |
| Tree (`tree.restructure`, `tree.write`, `tree.attention`) | `create-group`, `create-folder`, `create-project`, `create-split`, `set-bookmarked`, `set-expanded`, `restructure`, `revert-restructure`, `set-inferred-icon`, `mark-attention`, `ack-attention` | `rename-node` (`expected-revision`), `set-virtual-rows` (parents, drops) |
| Chrome (`ui.chrome`, `notify.toast`, `open-external`) | `open-external`, `set-status-segment`, `clear-status-segment`, `set-frame-slot`, `clear-frame-slot`, `toast`, `dismiss-toast` | `set-decoration` (spans, tooltip, frames), `notify` (coalesce key) |

**Rust**

```rust
// Freeze a pane for a stable read, then send guarded input.
let freeze = Submission::keyed(
    Intent::FreezePane(FreezePaneSpec { pane }),
    dedup::key_for(&["freeze", &pane.to_string()]),
);
let send = Submission::new(Intent::SendInput(SendInputSpec {
    if_current: Some(self.last_epoch),
    ..SendInputSpec::text(pane, "continue\n")
}));
vec![freeze, send]
```

**TypeScript**

```ts
// Freeze a pane for a stable read, then send guarded input.
return [
  { intent: { tag: 'freeze-pane', val: { pane } }, correlation: 1n, dedupKey: `freeze:${pane}` },
  { intent: { tag: 'send-input', val: { pane, text: 'continue\n', ifCurrent: lastEpoch } },
    correlation: 2n, dedupKey: undefined },
];
```

The 18 events, all appended to the `event` variant: `screen-changed`, `user-line-submitted`, `signal-changed`, `agent-model-changed`, `editor-pointer`, `editor-viewport-changed`, `http-stream-data`, `http-stream-closed`, `child-output`, `child-exited`, `pane-lifecycle-changed`, `attention-changed`, `client-request`, `kernel-settings-changed`, `keymap-changed`, `durable-timer-fired`, `input-uncertain` and `display-mode-changed`. In Rust they are `Event::ScreenChanged(..)` and so on; in TypeScript `{ tag: 'screen-changed', val }`. Each needs the grant listed on [Events and intents](/pages/reference-events-and-intents/). Match on the tag and ignore tags you do not know.

## Host services

Each service is a module in the table above, gated by a grant. All of the following run in both SDKs; slow calls are awaited as described earlier.

**Rust**

```rust
// catalogs (server or client): query a provider, page with the cursor.
let page = catalogs::query(CatalogQuery { catalog: "ilium.palettes".into(), filter: Some("sun".into()), cursor: None, limit: 50 }).await?;
// geocode (client, location.geocode): search and reverse.
let places = geocode::search("Lisbon", 5).await?;            // Vec<GeoPlace>
// fs: atomic write with a revision guard, and a tail for logs.
let written = fs::write_atomic(&path, bytes, Some(revision)).await?;     // Revisioned
let chunk = fs::tail(&path, cursor, 64 * 1024).await?;                    // TailChunk
// http: a streamed response (server-sent events).
let stream = http::open_stream(StreamRequest { request, framing: StreamFraming::ServerSentEvents, max_chunk_bytes: 16 * 1024 }).await?;
// vector: embed text, store, search (inference.embed).
let ids = vector::embed(&["notes on retries".to_string()], "search").await?;
// process: spawn a long-lived child (process.long-lived).
let child = process::spawn(SpawnSpec { process: spec, stdin_open: true, line_mode: true })?;
child.write_stdin(b"ping\n")?;
// durable timers and journal.
timers::at(1_790_000_000_000, "nightly")?; // epoch in ms, then the timer name
journal::log_event(None, "moved", &payload)?;
// kernel settings and keymap (read-only).
let value = kernel_settings::read("terminal")?; // the tab; JSON text
let bindings = keymap::bindings();
// client requests: ask the attached client to do something.
let answer = client_requests::request("pick-file", &args).await?;
// sessions (server): list and send to another session.
let all = sessions::list_live()?;
```

**TypeScript**

```ts
import * as catalogs from '@ilium/plugin-sdk/host/catalogs';
import * as geocode from '@ilium/plugin-sdk/host/geocode';
import * as fs from '@ilium/plugin-sdk/host/fs';
import * as http from '@ilium/plugin-sdk/host/http';
import * as vector from '@ilium/plugin-sdk/host/vector';
import * as proc from '@ilium/plugin-sdk/host/process';
import * as timers from '@ilium/plugin-sdk/host/timers';
import * as journal from '@ilium/plugin-sdk/host/journal';
import * as kernel from '@ilium/plugin-sdk/host/kernel-settings';
import * as keymap from '@ilium/plugin-sdk/host/keymap';
import * as clientRequests from '@ilium/plugin-sdk/host/client-requests';
import * as sessions from '@ilium/plugin-sdk/host/sessions';

const page = await catalogs.query({ catalog: 'ilium.palettes', filter: 'sun', cursor: undefined, limit: 50 });
const places = await geocode.search('Lisbon', 5);                         // GeoPlace[]
const written = await fs.writeAtomic(path, bytes, revision);              // Revisioned
const chunk = await fs.tail(path, cursor, 64 * 1024);                     // TailChunk
const stream = await http.stream({ request, framing: 'server-sent-events', maxChunkBytes: 16 * 1024 });
const ids = await vector.embed(['notes on retries'], 'search');
const child = proc.spawn({ process: spec, stdinOpen: true, lineMode: true });
child.writeStdin(new TextEncoder().encode('ping\n'));
timers.setDurable('nightly', 86_400_000n, true);
journal.append('moved', payload);
const value = kernel.get('terminal.scrollback');
const chord = keymap.lookup('palette.open');
const answer = await clientRequests.request('pick-file', args);
const all = sessions.list();
```

Terminal, agent, editor and basic primitives gain the following functions, each behind the grant named in the table.

| Primitive | Rust | TypeScript | Grant |
|---|---|---|---|
| terminal | `read_cells`, `modes`, `foreground_process`, `freeze_snapshot`, `history_text`, `search_history`, `lock_input` | `readCells`, `modes`, `foregroundProcess`, `freezeSnapshot`, `historyText`, `searchHistory`, `lockInput` | `terminal.read-cells`, `terminal.history`, `terminal.input-lock` |
| agent | `model`, `prompt_epoch`, `transcript` | `model`, `promptEpoch`, `transcript` | `agent.transcript` for `transcript` |
| editor | `viewport`, `set_minimap_marks` | `viewport`, `setMinimapMarks` | existing editor grants |
| basic | `report_activity`, `set_dirty` | `reportActivity`, `setDirty` | none beyond `basic` |
| cli | additions listed on [cli](/pages/api-cli/) | same, camelCase | `cli` |
| ui | `open_popup_with`, `theme_extras` | `openPopupWith`, `themeExtras` | `ui.chrome` for chrome surfaces |

`gpu` is `@unstable`: import it only from a scene and expect it to change. High-risk grants (marked H in the grant table) prompt the user with a separate consent line. Examples: `terminal.history`, `terminal.input-lock`, `terminal.raw-input`, `panes.lifecycle`, `tree.restructure`, `sessions.cross`, `process.long-lived`, `agent.transcript`, `agent.read-prompts` and `process.run:<program>`. See [Capabilities](/pages/reference-capabilities/) and [Security](/pages/security/).

## Settings rows, meters and row state

The manifest `settings` object declares 18 row types. The plugin updates a row's state at run time without republishing the manifest.

**Rust**

```rust
// Meter row filled from the plugin, and a state banner on a row.
ui::set_meter("usage.spend", vec![MeterSegment { label: "today".into(), value: 4.2, tone: Tone::Info }])?;
ui::set_row_state("provider.key", RowState { kind: RowStateKind::Warning, message: "Key expires in 3 days".into() })?;
```

**TypeScript**

```ts
// Meter row filled from the plugin, and a state banner on a row.
setMeter('usage.spend', [{ label: 'today', value: 4.2, tone: 'info' }]);
setRowState('provider.key', { kind: 'warning', message: 'Key expires in 3 days' });
```

The manifest side, identical for both languages:

```json
{
  "contributes": {
    "settings": {
      "sections": [{
        "id": "usage", "title": "Usage",
        "rows": [
          { "id": "usage.spend", "type": "meter", "label": "Spend" },
          { "id": "provider.key", "type": "text", "label": "API key", "secret": true },
          { "id": "sky.location", "type": "location", "label": "Location" }
        ]
      }]
    }
  }
}
```

See [Settings schema](/pages/reference-settings-schema/) for all 18 row types.

## Provider exports

Each optional export is a small trait or object.

**Rust**

```rust
impl CatalogProvider for MyPlugin {
    fn query_catalog(&mut self, query: CatalogQuery) -> Result<CatalogPage, PluginError> {
        let items = self.palettes.iter()
            .filter(|p| query.filter.as_deref().map_or(true, |f| p.name.contains(f)))
            .map(|p| CatalogItem::new(&p.id, &p.name)).collect();
        Ok(CatalogPage { items, next: None, total: Some(self.palettes.len() as u64) })
    }
}
impl TokenProvider for MyPlugin {
    fn tokens(&mut self, buffer: BufferId, first_line: u32, lines: Vec<String>, state_in: Option<Vec<u8>>)
        -> (Vec<Token>, Option<Vec<u8>>) {
        let found = lines.iter().enumerate()
            .filter_map(|(i, l)| l.find('#').map(|c| Token { line: first_line + i as u32, start: c as u32, length: (l.len() - c) as u32, kind: TokenKind::Comment }))
            .collect();
        (found, state_in)
    }
}
```

**TypeScript**

```ts
const provider = definePlugin({
  side: 'client',
  queryCatalog(query) {
    const items = palettes
      .filter((p) => query.filter === undefined || p.name.includes(query.filter))
      .map((p) => ({ id: p.id, label: p.name }));
    return { items, next: undefined, total: BigInt(palettes.length) };
  },
  tokens(buffer, firstLine, lines, stateIn) {
    const found = lines.flatMap((l, i) => l.includes('#')
      ? [{ line: firstLine + i, start: l.indexOf('#'), length: l.length - l.indexOf('#'), kind: 'comment' as const }]
      : []);
    return [found, stateIn];
  },
});
```

The manifest lists the exports and the contribution keys that point at them: `catalogs`, `tree_toolbar`, `row_actions`, `tree_orderings`, `status_segments`, `pane_frame_slots`, `display_modes`, `client_requests` and `languages`, plus the top-level `limits_tier`. All are explained on [Manifest reference](/pages/reference-manifest/).

## Extension points

The `extensions` host interface lets a plugin read the contributions to its own points and run hooks. A plugin that contributes handlers implements `hook-handler`. The contract is in [Extension points](/pages/api-extension-points/). The examples below are illustrations and are not compiled.

**Rust**

```rust
use ilium_plugin_sdk::host::extensions;
use ilium_plugin_sdk::exports::HookHandler;

// Owner: needs extensions.provide:com.example.notes/after-save
let entries = extensions::list("com.example.notes/recognisers")?;
let outcome = extensions::run_hook("com.example.notes/after-save", &payload_json).await;
// Outcome ok: take the reply. refused(extension.vetoed): do not perform the action.

// Contributor: needs extensions.contribute:com.example.notes/after-save
impl HookHandler for MyPlugin {
    fn handle_hook(&mut self, call: HookCall) -> HookReply {
        match call.extension.as_str() {
            "strip-tokens" => HookReply::Replace(strip(&call.payload_json)),
            "no-secrets" if has_secret(&call.payload_json) => HookReply::Refuse("secret in text".into()),
            _ => HookReply::Pass,
        }
    }
}
```

**TypeScript**

```ts
import { list, runHook } from '@ilium/plugin-sdk/host/extensions';

// Owner: needs extensions.provide:com.example.notes/after-save
const entries = list('com.example.notes/recognisers');
const outcome = await runHook('com.example.notes/after-save', payloadJson);

// Contributor: needs extensions.contribute:com.example.notes/after-save
const plugin = definePlugin({
  side: 'server',
  hookHandler(call) {
    if (call.extension === 'strip-tokens') return { tag: 'replace', val: strip(call.payloadJson) };
    if (hasSecret(call.payloadJson)) return { tag: 'refuse', val: 'secret in text' };
    return { tag: 'pass' };
  },
});
```

Reply constructors: Rust `HookReply::Pass`, `HookReply::Replace(json)`, `HookReply::Refuse(reason)` and `HookReply::Answer(json)`. TypeScript `{ tag: 'pass' }`, `{ tag: 'replace', val: json }`, `{ tag: 'refuse', val: reason }` and `{ tag: 'answer', val: json }`. A reply that the point's mode does not allow counts as a pass. `register` and `unregister` work only on collect points with `dynamic: true`. The owner receives the topic `ilium.extensions/changed` and calls `list` again.

## Scenes

A scene plugin (`side = "scene"`) draws into a surface and follows the shared look; colour, dither and display settings are never scene settings.

**Rust**

```rust
#[ilium_plugin(side = "scene", exports = ["renderer"])]
#[derive(Default)]
struct Waves { phase: f32 }

impl Renderer for Waves {
    fn frame(&mut self, surface: &mut Surface, delta_ms: u32) {
        self.phase += delta_ms as f32 * 0.001;
        surface.clear();
        for x in 0..surface.width() { surface.set_tone(x, (self.phase + x as f32 * 0.2).sin() * 0.5 + 0.5); }
    }
}
```

**TypeScript**

```ts
export const { lifecycle, handler, renderer } = defineScene({
  frame(surface, deltaMs) {
    phase += deltaMs * 0.001;
    surface.clear();
    for (let x = 0; x < surface.width(); x++) surface.setTone(x, Math.sin(phase + x * 0.2) * 0.5 + 0.5);
  },
});
```

The glyph-plane surface adds font loading through `anim::fonts` (`Font` resource) and palette delivery. See [Porting IliumAnim](/pages/porting-iliumanim/) and [Performance](/pages/performance/).

## Worked example: a working badge

The plugin puts a short badge on every agent that starts working.

**Rust**

```rust
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::AgentActivityEvent;
use ilium_plugin_sdk::types::intents::SetDecorationSpec;
use ilium_plugin_sdk::types::tree::AgentActivity;

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct Hello;

impl Lifecycle for Hello {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        log::log(Level::Info, &format!("hello from {}", context.plugin));
        Ok(())
    }
}

impl Handler for Hello {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            let Event::AgentActivityChanged(AgentActivityEvent { node, current, .. }) = event
            else { continue };
            if current != AgentActivity::Working { continue; }
            submissions.push(Submission::new(Intent::SetDecoration(SetDecorationSpec {
                node, slot: "hello".into(), content: vec![Span::plain("hi")],
                icon: None, sort_key: None, persist: false,
                ..Default::default()   // the optional fields: spans, tooltip, frames, tone
            })));
        }
        submissions
    }
}
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import { log } from '@ilium/plugin-sdk/host/log';

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  activate(context) { log('info', `hello from ${context.plugin}`); },
  onEvents(batch) {
    return batch.flatMap((event) =>
      event.tag === 'agent-activity-changed' && event.val.current === 'working'
        ? [{ intent: { tag: 'set-decoration', val: {
              node: event.val.node, slot: 'hello', content: [{ text: 'hi' }], persist: false } },
             correlation: 0n, dedupKey: undefined }]
        : []);
  },
});
```

The manifest, shared by both:

```json
{
  "server": { "entry": "server.wasm", "world": "ilium:server/base@0.1.0", "exports": [] },
  "capabilities": { "server": ["agent.read", "tree.decorate"] },
  "contributes": {
    "decoration_slots": [{ "id": "hello", "placement": "badge", "priority": 50 }]
  }
}
```

## Testing

The test double runs a plugin against a fake host in the same process. Rust enables it with the `testing` feature; TypeScript imports `@ilium/plugin-sdk/testing` and runs under `bun test`. The exact methods, limits and error codes are in [Test harness reference](/pages/reference-test-harness/). The how-to is [Testing plugins](/pages/testing/).

| Operation | Rust (`testing::FakeHost`) | TypeScript (`createFakeHost()`) |
|---|---|---|
| Create from the manifest | `FakeHost::for_manifest("manifest.json")?` | `createFakeHost().forManifest('manifest.json')` |
| Grant a capability | `.grant("tree.decorate")` before activation, `grant_now` after | `.grant('tree.decorate')`, `grantNow` after |
| Activate | `host.activate::<Hello>()` returns a `PluginHandle` | `await host.activate({ lifecycle, handler })` or `await host.activateModule('../src/server')` |
| Build events | `events::activity(node, previous, current)`, `events::command(name, arguments_json)` | `events.activity(node, previous, current)`, `events.command(name, argumentsJson)` |
| Deliver events | `host.deliver(events)` returns `Vec<Submission>` | `host.deliver(events)` returns the submissions |
| Deliver outcomes | `host.settle()` | `await host.settle()` |
| Render | `host.render(ViewTarget::Panel("main".into()), 62, 12)` | `host.render({ tag: 'panel', val: 'main' }, 62, 12)` |
| Read the screen | `screen.row(n)`, `screen.cell(row, column)` | `screen.row(n)`, `screen.cell(row, column)` |

```rust
#[test]
fn badge_is_set_when_agent_works() {
    let mut host = FakeHost::for_manifest("manifest.json").unwrap();
    host.activate::<Hello>();
    let submissions = host.deliver(vec![events::activity(7, AgentActivity::Idle, AgentActivity::Working)]);
    assert_eq!(submissions.len(), 1);
}
```

```ts
import { test, expect } from 'bun:test';
import { createFakeHost, events } from '@ilium/plugin-sdk/testing';

test('badge is set when agent works', async () => {
  const host = createFakeHost().forManifest('manifest.json');
  await host.activateModule('../src/server');
  expect(host.deliver([events.activity(7n, 'idle', 'working')]).length).toBe(1);
});
```

The harness has a golden-file helper, `assert_golden`, a pointer helper, `click`, and `FakeSession` for tests with several plugins. See the reference page.

## Package surface

The SDK does not build packages. The `ilium plugin` command does that, from the files of the project. A plugin author touches the package in three places: the manifest, the `help/` folder and the `dist/` output. The build stages, the package layout and the signing steps are in [Plugin command-line reference](/pages/reference-plugin-cli/) and [Packages](/pages/packages/). `ilium plugin pack` builds an unsigned package. `ilium plugin sign` signs it.

## SDK versioning policy

The SDK version has its own number, separate from the contract `ilium:plugin@0.1.0`. A new SDK release that keeps the contract compatible raises the minor or patch number. An SDK release that needs a new contract version raises the contract and writes the change in the changelog of the SDK. `ilium plugin check` fails with `manifest.api-version` when the component and the manifest name different contract versions. See [Versioning](/pages/versioning/).

## Edge cases

- A batch holds at most 256 events and a submission list at most 256 entries; a larger list traps the instance with `trap.submission-batch`.
- A crashed instance restarts after 1 s, then 5 s; the third failure within 10 minutes disables the plugin until the user enables it.
- `on_events` has a 20 ms deadline, `render` 10 ms, `activate` 2000 ms and `deactivate` 500 ms. A breach traps the instance.
- Instance memory is capped at 32 MiB (the `memory_mib` manifest key can raise it, up to 128 MiB for T2 and T3 plugins).
- A log message over 16 KiB is cut and ends with `…[truncated]`.
- An await that never completes is cancelled with `timed-out` when its request timeout elapses.
- `toolchain.sdk-missing`, `toolchain.language-ambiguous`, `manifest.exports-mismatch` and `package.old-format` stop `ilium plugin build` with a clear message.

## What the user sees

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin logs com.example.hello
<span class="t-dim">2026-10-12 09:14:02</span> info  server com.example.hello hello from com.example.hello
<span class="t-dim">2026-10-12 09:14:07</span> <span class="t-warn">warn</span>  server com.example.hello retry in 250 ms
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The log format is defined in <a href="/pages/reference-debugging-tools/">Debugging tools</a>.</figcaption>
</figure>

### See also

- [Rust SDK reference](/pages/rust-sdk/) and [TypeScript SDK reference](/pages/typescript-sdk/)
- [Lifecycle](/pages/lifecycle/) and [Exports](/pages/api-exports/)
- [Widgets index](/pages/widgets-index/)
- [Events and intents](/pages/reference-events-and-intents/)
- [Errors and limits](/pages/reference-errors-and-limits/)
- [Manifest reference](/pages/reference-manifest/)
- [Versioning](/pages/versioning/)

Checked against the API version `ilium:plugin@0.1.0`.

