---
layout: page
title: extension points
permalink: /pages/api-extension-points/
---

## extension points

An extension point is a named slot that one plugin owns and other plugins fill. The owner decides what the slot means and when it is used. There are two kinds. A **collect** point holds data: contributors add entries and the owner reads the list when it needs it. A **hook** point holds code: contributors add handlers and the owner runs them at a fixed place in its own flow. Core plugins use the same mechanism as every other plugin, so a third-party plugin can extend a core plugin, and a third-party plugin can offer points of its own. The kernel checks the grants, the schemas and the deadlines on every use.

Extension points do not replace services, topics or the kernel slots. A service has one provider and many callers. A topic has one publisher and many subscribers. An extension point has one owner and many contributors. [Services](/pages/services/) explains the other choice. [Extending core plugins](/pages/extending-core-plugins/) shows all five ways side by side.

### What you need

- **Grants.** The owner needs `extensions.provide:<plugin-id>/<point>` for each point it owns. A contributor needs `extensions.contribute:<plugin-id>/<point>` for each point it fills. There is no wildcard and no "contribute to any" grant. See [extensions.provide](/pages/capability-extensions-provide/) and [extensions.contribute](/pages/capability-extensions-contribute/).
- **Manifest.** The owner lists its points in `contributes.extension_points`. A contributor lists its entries and handlers in `contributes.extensions`. Any side that has hook extensions lists `hook-handler` in `exports`. See [reference: manifest](/pages/reference-manifest/).
- **Worlds.** The `extensions` import and the `hook-handler` export exist in `ilium:server/base` and `ilium:client/base`. The scene world has neither.
- **Toolchain.** Rust: stable 1.90 or later with the `wasm32-wasip2` target and the `ilium-plugin-sdk` crate. TypeScript: Bun 1.2 or later and `@ilium/plugin-sdk`. Nothing is installed globally.

### Names and manifest entries

| Item | Form | Example |
|---|---|---|
| Point id | 1 to 64 characters from `a-z`, `0-9` and `-` | `after-copy` |
| Global point name | `<plugin-id>/<id>`; the prefix is the owner's plugin id | `ilium.core.smart-copy/after-copy` |
| Owner grant | `extensions.provide:<plugin-id>/<point>` | `extensions.provide:com.example.notes/before-export` |
| Contributor grant | `extensions.contribute:<plugin-id>/<point>` | `extensions.contribute:com.example.notes/before-export` |
| Owner manifest entry | object in `contributes.extension_points` | see below |
| Contributor manifest entry | object in `contributes.extensions` | see below |

A grant carries the bare global name, with no version and no wildcard. A plugin owns at most 16 points and contributes at most 64 extensions.

The fields of an owner entry (`id`, `kind`, `title`, `mode`, the schemas, `risk`, `deadline_ms`, `max_handlers`, `max_entries`, `dynamic`, `version`, `on_failure`, `final_check`) and of a contributor entry (`point`, `id`, `priority`, `entry`, `handler`, `guard`) are defined once, in [Manifest reference, extension points](/pages/reference-manifest/#extension-points-and-replacement). The schemas use the keywords listed in [Data schema subset](/pages/reference-settings-schema/#data-schema-subset); any other keyword fails the install with `manifest.schema-keyword`.

Owner manifest, with one collect point and one hook point:

```json
{
  "id": "com.example.notes",
  "capabilities": {
    "server": [
      "extensions.provide:com.example.notes/recognisers",
      "extensions.provide:com.example.notes/before-export"
    ]
  },
  "contributes": {
    "extension_points": [
      { "id": "recognisers", "kind": "collect", "title": "Note recognisers",
        "entry_schema": { "type": "object", "required": ["id", "regex"],
          "properties": { "id": { "type": "string" },
                          "regex": { "type": "string", "maxLength": 512 } } },
        "max_entries": 64, "version": "0.1.0" },
      { "id": "before-export", "kind": "hook", "mode": "transform",
        "title": "Before export",
        "payload_schema": { "type": "object",
          "properties": { "text": { "type": "string", "maxLength": 65536 } } },
        "reply_schema": { "type": "object",
          "properties": { "text": { "type": "string", "maxLength": 65536 } } },
        "risk": "standard", "deadline_ms": 20, "max_handlers": 8, "version": "0.1.0" }
    ]
  }
}
```

The owner does not export `hook-handler`, because it runs hooks and does not handle them. A plugin that also contributes hooks to other points adds the export.

Contributor manifest:

```json
{
  "id": "com.example.tickets",
  "exports": ["hook-handler"],
  "capabilities": {
    "server": [
      "extensions.contribute:com.example.notes/recognisers",
      "extensions.contribute:com.example.notes/before-export"
    ]
  },
  "contributes": {
    "extensions": [
      { "point": "com.example.notes/recognisers", "id": "jira-key", "priority": 50,
        "entry": { "id": "jira", "regex": "[A-Z][A-Z0-9]+-[0-9]+" } },
      { "point": "com.example.notes/before-export@0.1.0", "id": "strip-tokens",
        "priority": 30, "handler": "strip_tokens" }
    ]
  }
}
```

### Interface

```wit
// ilium:host/extensions, imported by owners and contributors
interface extensions {
  record extension-entry {
    extension: string,      // contributor-chosen id, unique per contributor and point
    contributor: plugin-id,
    priority: u8,
    entry-json: string,     // the validated entry
  }
  enum extension-error {
    unknown-point, not-granted, schema-mismatch, cap-exceeded,
    reentrant, not-dynamic, not-found,
  }

  // owner side (needs extensions.provide:<point>)
  list: func(point: string) -> result<list<extension-entry>, extension-error>;
  run-hook: func(point: string, payload-json: string) -> result<request-id, extension-error>;
  take-reply: func(request: request-id) -> option<string>;

  // contributor side, collect points with dynamic = true (needs extensions.contribute:<point>)
  register: func(point: string, extension: string, priority: u8, entry-json: string)
    -> result<_, extension-error>;
  unregister: func(point: string, extension: string) -> result<_, extension-error>;
}

// export fragment `with-hook-handler`, listed as "hook-handler" in `exports`
variant hook-reply { pass, replace(string), refuse(string), answer(string) }
record hook-call { call: u64, point: string, extension: string, payload-json: string }
handle-hook: func(call: hook-call) -> result<hook-reply, plugin-error>;
```

### Functions

| Function | Grant | Rust | TypeScript | What it does |
|---|---|---|---|---|
| `list` | `extensions.provide:<point>` | `extensions::list(point)` | `list(point)` | Returns the active entries of a point, sorted by the user's order, then tier band, priority, plugin id and extension id (see Order below). Dormant and disabled entries are left out. |
| `run-hook` | `extensions.provide:<point>` | `extensions::run_hook(point, payload_json).await` | `await runHook(point, payloadJson)` | Runs the chain and resolves when it ends. The raw form returns a request id: `extensions::run_hook_request` and `runHookRequest`. |
| `take-reply` | `extensions.provide:<point>` | `extensions::take_reply(request)` | `takeReply(request)` | Returns the final payload of a `transform` chain only when at least one `replace` was accepted, or the answer of a `first` chain. Returns nothing for `observe` and `veto`, for a `transform` chain in which every handler passed (the owner then uses its original payload), and on a second take. |
| `register` | `extensions.contribute:<point>` | `extensions::register(point, extension, priority, entry_json)` | `register(point, extension, priority, entryJson)` | Adds or replaces an entry of a `dynamic` collect point. The entry is validated again. |
| `unregister` | `extensions.contribute:<point>` | `extensions::unregister(point, extension)` | `unregister(point, extension)` | Removes an entry that this plugin registered. |
| `handle-hook` | export `hook-handler` | `HookHandler::handle_hook` | `handleHook` in `definePlugin` | Receives one hook call and returns one reply. |

The `HookHandler` trait, `HookCall` and `HookReply` are in `ilium_plugin_sdk::exports` in Rust. In TypeScript the types come from `@ilium/plugin-sdk` and the handler is the `handleHook` member of `definePlugin`.

| Field or case | Type | Meaning |
|---|---|---|
| `ExtensionEntry.extension` | string | The id the contributor gave the entry in its manifest or in `register`. It is the identity of the entry. The owner shows and stores it as `<contributor-id>/<id>`, so equal ids from two plugins never collide. |
| `ExtensionEntry.contributor` | plugin id | The plugin that contributed the entry. |
| `ExtensionEntry.priority` | `u8` | The declared priority (0 to 100), unchanged by tier. The list is already in run order. |
| `ExtensionEntry.entry_json` | string | The validated entry. |
| `HookCall.point` | string | The global point name. |
| `HookCall.extension` | string | The `handler` name from the manifest. Dispatch on it. |
| `HookCall.call` | `u64` (`bigint` in TypeScript) | The number of this call. Useful in log lines. |
| `HookCall.payload_json` | string | The payload. For `transform` it is the output of the previous handler. |
| `HookReply::Pass` | none | No opinion. Always allowed. |
| `HookReply::Replace(json)` | JSON string | A new payload. Allowed in `transform` only. |
| `HookReply::Refuse(reason)` | string | Stops the action. Allowed in `veto` only. The reason is shown in logs and notices. |
| `HookReply::Answer(json)` | JSON string | The answer. Allowed in `first` only. |

In TypeScript the replies are `{ tag: 'pass' }`, `{ tag: 'replace', val }`, `{ tag: 'refuse', val }` and `{ tag: 'answer', val }`.

`register` takes a `u8` priority, but the manifest range is 0 to 100. A `register` with a priority above 100 fails with base code `invalid-argument` and a message that starts with `extension.schema-mismatch`.

<a id="hooks"></a>

### Hook modes

| Mode | Handlers called | Reply allowed | Result |
|---|---|---|---|
| `observe` | All, in priority order. Replies are ignored. | `pass` | Outcome `ok`. `take-reply` returns nothing. |
| `transform` | In run order. Each handler gets the output of the previous one. | `pass`, `replace(json)` | Outcome `ok`. `take-reply` returns the final payload when at least one replace was accepted, otherwise nothing. |
| `veto` | In priority order, until one refuses. | `pass`, `refuse(reason)` | `refused` with code `extension.vetoed` on the first refusal, otherwise `ok`. |
| `first` | In priority order, until one answers. | `pass`, `answer(json)` | Outcome `ok`. `take-reply` returns the answer, or nothing when nobody answered. |
| `guard` (after a `transform` chain whose point sets `final_check`) | Each contributor that declared `guard: true`, once, after the transform chain, with the final payload. | `pass`, `refuse(reason)` | Any `refuse` ends the chain as `refused(conflict)` with `extension.vetoed`. Otherwise the outcome is `ok`. |

A reply that the mode does not allow counts as `pass` and is logged as `extension.reply-not-allowed`. An `answer` on a `first` point that fails `reply_schema` counts as `pass` and is logged as `extension.schema-mismatch`.

For `transform`, a `replace` carries the whole payload. The host validates it against `payload_schema` and checks that every field not listed in `reply_schema.properties` equals its input value. Otherwise the reply counts as `pass` and is logged as `extension.schema-mismatch` (a breach). `reply_schema` on a `transform` point therefore lists the changeable fields only, and the next handler always gets the `payload_schema` shape. [EXT-014]

**Guards and failure policy.** A `transform` owner whose result drives a privileged action sets `final_check: true`. After the transform chain, the host calls once each contributor entry that has `guard: true` (it needs a `handler`) with the final payload. A guard replies `pass` or `refuse`. On a `high` point of mode `veto` or `transform`, the owner may set `on_failure` to `pass` or `refuse` (default `refuse`): with `refuse`, a handler that fails on a plugin-started action refuses it with `extension.vetoed` (reason `guard-failed`). [SEC-284] [SEC-287]

**User override.** A `veto` chain, or a guard, on an action the user started can be overridden by the user: see [SEC-291]. After an override the chain completes `ok` with no reply, and the owner's log gets `extension.veto-overridden` with the contributor and the point. The core points that allow it are marked in the **User override** column of [Core extension points](/pages/reference-core-extension-points/).

### Rules for payloads, deadlines, ordering and failure

- **Payloads.** A payload, a reply and a hook chain result are each capped at 64 KiB. An entry is capped at 16 KiB. The host checks the payload against `payload_schema` before the first handler runs. A failing payload returns `schema-mismatch` from `run-hook` and no handler runs.
- **What a contributor sees.** A handler gets the payload of the owner's `payload_schema` and nothing else. It does not see the owner's state, the other handlers or the other contributors.
- **Deadlines.** Each handler gets `deadline_ms`, 20 milliseconds by default. The range is 5 to 50 milliseconds for every tier: the T1 ceiling and the T2 and T3 ceiling are both 50 milliseconds. This is separate from the plugin's own `event_deadline_ms`, whose ceiling is 50 milliseconds at T1 and 20 milliseconds at T2 and T3. The whole chain gets 100 milliseconds. Handlers that have not started when the chain reaches 100 ms are skipped and logged (see Chain cap).
- **Order.** The user order is a stored list of (plugin id, extension id) per point. Items in it run first, in that order. Items not in it follow, sorted by tier band (T1 before T2 and T3), then priority (lower first), then plugin id, then extension id (byte-wise ASCII comparison). A contributor installed after the user reordered therefore lands after the stored items. An update keeps the stored position, and an uninstall removes it. The same order applies to `list` on collect points. Declared priorities are kept: `ExtensionEntry.priority` returns the declared value. [EXT-007] [EXT-008]
- **Fail open.** A handler that times out, traps or errors counts as `pass`, except at a high-risk veto or transform point whose `on_failure` is `refuse` (the default there), where it refuses the plugin-initiated action with `extension.vetoed` (SEC-287). Elsewhere a timeout never vetoes. A **breach** is a timeout, a trap, an error return, a reply over 64 KiB, a `replace` or `answer` that fails its schema, a CPU budget overrun, or a skip because the contributor's instance was busy. `reply-not-allowed` and a chain-cap skip are not breaches. The counter is per contributor, point and extension id. Any call that returns an accepted reply within its deadline resets it to 0. The third consecutive breach stops the extension. The stop ends when the contributor's instance restarts or the user presses Reset, and it is not persisted. The user sees a notice and a Reset button. [EXT-021]
- **Re-entry.** A `run-hook` on any point that is already on the current chain stack, directly or indirectly (A runs B, and B's handler runs A), fails with `extension.reentrant`.
- **Other host calls.** A handler may call other host functions that its grants allow. The calls must finish within its deadline, and they must be synchronous. A handler can only start an inner chain with `run-hook` (see Nested chains); it cannot wait for it.
- **Where handlers run.** A handler runs in the contributor's own instance, in the process that hosts the owner's side. T1 handlers run in that process. T2 and T3 handlers cost one helper round trip, and that round trip counts inside the handler deadline (a 5 ms handler has about 4.5 ms for its own work). Do not put a hook on a path hotter than about 100 calls per second.
- **Asynchronous chain.** `run-hook` does not block the owner's current call. It returns a request id at once, the chain runs on the host, and the result arrives as `request-completed`, where `take-reply` returns the reply. The SDK `await` form hides this. The 100 ms chain cap therefore does not conflict with the owner's 20 ms `on-events` deadline. An owner that needs the answer inside the same call cannot use a hook; it keeps its own default and applies the reply when it arrives.
- **Sequential.** The handlers of one chain run strictly one after another in priority order, never in parallel. A handler that cannot start within its own deadline (its plugin is busy) is skipped as `pass` and counts a breach. A handler whose instance is still activating is skipped as `pass` and is not a breach.
- **Chain cap.** The 100 ms cap is measured from the start of the first handler call. A handler that starts before 100 ms runs until its own `deadline_ms`, even if that ends after the cap. Handlers not started at 100 ms are skipped as `pass`, are logged `extension.chain-cap` and are not breaches. [EXT-005]
- **Does the action wait?** For `veto`, `transform` and `first` points the owner holds that one action until the chain completes (at most the 100 ms cap plus one deadline) and keeps serving other events meanwhile. For `observe` points the action does not wait, and handlers may run after it. [EXT-029]
- **No process crossing.** A hook runs only handlers hosted by the process that owns the point. A client-owned point runs only the handlers in that client, never in another client or in the server. A server-owned point runs only server handlers.
- **Nested chains.** The outermost chain is depth 1. A `run-hook` from a handler at depth 3 fails at once with `extension-error` `reentrant`, base code `conflict` and a message that starts with `extension.depth-exceeded`. It counts as `pass` for that handler and not as a breach. The inner chain runs after the handler returns. Its completion arrives at the handler's plugin as `request-completed`, and its time does not count toward the outer deadline.
- **Side.** A handler runs on the side that owns the call: server for a server-owned point, client for a client-owned point. A contributor declares its handler on the same side.
- **[HST-470]** `run-hook` MUST NOT block the owner's current call. It MUST return a request id at once and complete as `request-completed`.
- **[HST-471]** Handlers of one chain MUST run strictly one after another in priority order, and a handler that cannot start within its own deadline MUST be skipped as `pass` and count a breach.
- **[HST-472]** The 100 ms chain cap MUST be measured from the start of the first handler, and handlers not started at the cap MUST be skipped and logged `extension.chain-cap`.
- **[HST-473]** The helper round trip of a T2 or T3 handler MUST count inside the handler deadline.
- **[HST-474]** A hook MUST run only the handlers hosted by the process that owns the point.
- **[HST-475]** A chain started inside a handler MUST be limited to depth 3. The inner chain MUST run after the handler returns and MUST NOT count toward the outer deadline.
- **Contributions change.** The kernel sends the topic `ilium.extensions/changed` to the owner once per point after any change to its active set: install, uninstall, update, enable or disable of a contributor, `register`, `unregister`, a user toggle or reorder, a breach stop, a Reset, or a dormant/active change. The payload is `{ "point": "<global point name>" }`. Changes within 50 ms are merged into one event. Nobody but the owner gets it. No `contributes.topics.subscribes` entry or grant is needed; listing it is allowed and has no effect. The owner calls `list` again. [EXT-026]
- **Dormant contributions.** A contribution is dormant when its owner is not installed, is disabled, or does not satisfy its `@version`. An inactive owner does not make it dormant. A dormant contribution stays in the package and counts for nothing. It becomes active when the owner appears. Install never fails because an owner is missing.
- **Point full.** Install never fails because a point is full. The active set is the first `max_handlers` handlers (or `max_entries` entries) in run order. The rest are dormant with the reason `point full (<n>)`. A reorder by the user or a removal recomputes the set and sends `ilium.extensions/changed`. [EXT-030]

### Diagram: owner, registry and contributors

<figure>
<svg viewBox="0 0 440 210" role="img" aria-label="An owner reads a registry that holds contributions in run order">
<title>Owner, registry and contributors</title>
<desc>Three contributors send entries or handlers to the extension registry, each with a priority. The owner calls list or run-hook on the registry. The registry returns the entries, or runs the handlers in priority order from low to high.</desc>
<defs>
<marker id="api-ext-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
<path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
</marker>
</defs>
<rect x="8" y="14" width="120" height="36" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
<text x="68" y="36" font-size="11" text-anchor="middle" fill="currentColor">Contributor A (30)</text>
<rect x="8" y="86" width="120" height="36" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
<text x="68" y="108" font-size="11" text-anchor="middle" fill="currentColor">Contributor B (50)</text>
<rect x="8" y="158" width="120" height="36" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
<text x="68" y="180" font-size="11" text-anchor="middle" fill="currentColor">Contributor C (70)</text>
<line x1="128" y1="32" x2="178" y2="84" stroke="currentColor" marker-end="url(#api-ext-arrow)"/>
<line x1="128" y1="104" x2="178" y2="104" stroke="currentColor" marker-end="url(#api-ext-arrow)"/>
<line x1="128" y1="176" x2="178" y2="124" stroke="currentColor" marker-end="url(#api-ext-arrow)"/>
<rect x="178" y="62" width="124" height="84" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
<text x="240" y="86" font-size="11" text-anchor="middle" fill="currentColor">Point registry</text>
<text x="240" y="104" font-size="10" text-anchor="middle" fill="currentColor">checks grants</text>
<text x="240" y="118" font-size="10" text-anchor="middle" fill="currentColor">checks schemas</text>
<text x="240" y="132" font-size="10" text-anchor="middle" fill="currentColor">runs A, then B, then C</text>
<line x1="302" y1="104" x2="342" y2="104" stroke="currentColor" marker-start="url(#api-ext-arrow)" marker-end="url(#api-ext-arrow)"/>
<rect x="342" y="62" width="90" height="84" rx="6" fill="none" stroke="currentColor"/>
<text x="387" y="96" font-size="11" text-anchor="middle" fill="currentColor">Owner</text>
<text x="387" y="112" font-size="10" text-anchor="middle" fill="currentColor">list or</text>
<text x="387" y="126" font-size="10" text-anchor="middle" fill="currentColor">run-hook</text>
</svg>
<figcaption>Contributors never talk to the owner. The registry orders them by the user's order, then priority, and it fails open when a handler is slow.</figcaption>
</figure>

### Example: Rust owner

The owner `com.example.notes` has one collect point, `recognisers`, and one `transform` hook point, `before-export`. On a command it reads the recognisers and runs the hook over the note text. When the topic `ilium.extensions/changed` arrives, it reads the list again.

`src/lib.rs`:

```rust
use ilium_plugin_sdk::host::extensions;
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::task;

const RECOGNISERS: &str = "com.example.notes/recognisers";
const BEFORE_EXPORT: &str = "com.example.notes/before-export";

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct Notes {
    recogniser_entries: Vec<String>,
}

impl Lifecycle for Notes {
    fn activate(&mut self) {
        self.reload_recognisers();
    }
}

impl Notes {
    fn reload_recognisers(&mut self) {
        match extensions::list(RECOGNISERS) {
            Ok(entries) => {
                self.recogniser_entries = entries.into_iter().map(|e| e.entry_json).collect();
            }
            Err(error) => log::log(Level::Warn, &format!("list failed: {error:?}")),
        }
    }
}

impl Handler for Notes {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            match event {
                Event::TopicPublished(published) if published.topic == "ilium.extensions/changed" => {
                    self.reload_recognisers();
                }
                Event::CommandInvoked(invoked) if invoked.command == "com.example.notes/export" => {
                    task::spawn(export_note("Release notes for Friday".to_string()));
                }
                _ => {}
            }
        }
        Vec::new()
    }
}

async fn export_note(text: String) {
    // Escape the text with a JSON library in a real plugin.
    let payload = format!(r#"{{"text":"{text}"}}"#);
    match extensions::run_hook(BEFORE_EXPORT, &payload).await {
        Ok(Some(final_payload)) => log::log(Level::Info, &format!("export: {final_payload}")),
        Ok(None) => log::log(Level::Info, "export: no handler changed the text"),
        Err(error) => log::log(Level::Warn, &format!("export failed: {}", error.message)),
    }
}
```

`run_hook` resolves with `Some(json)` for `transform` and `first` chains, with `None` for `observe` and `veto`, and with `Err` when the chain ended in `refused` or `failed`. For a `veto` point the owner checks `error.extension_code() == Some("extension.vetoed")` and does not perform the action. For a `transform` point the owner uses the original text when `take_reply` finds nothing.

### Example: Rust contributor

The contributor `com.example.tickets` handles the hook by name. The `extension` field of the call is the `handler` value from the manifest.

`src/lib.rs`:

```rust
use ilium_plugin_sdk::exports::{HookCall, HookHandler, HookReply};
use ilium_plugin_sdk::prelude::*;

#[ilium_plugin(side = "server", exports = ["hook-handler"])]
#[derive(Default)]
struct Tickets;

impl Lifecycle for Tickets {}

impl Handler for Tickets {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}

impl HookHandler for Tickets {
    fn handle_hook(&mut self, call: HookCall) -> HookReply {
        match call.extension.as_str() {
            "strip_tokens" => strip_tokens(&call.payload_json),
            _ => HookReply::Pass,
        }
    }
}

fn strip_tokens(payload_json: &str) -> HookReply {
    // Parse with a JSON library in a real plugin. This stand-in keeps the example short.
    if !payload_json.contains("ghp_") {
        return HookReply::Pass;
    }
    HookReply::Replace(payload_json.replace("ghp_", "[removed]"))
}
```

The contributor's package needs the manifest shown above. A handler must stay under its deadline and must not block. Return `Pass` when you have nothing to change.

### Example: TypeScript owner

The same owner in TypeScript. `runHook` resolves with the final payload, or `undefined` when the mode returns none.

`src/server.ts`:

```ts
import { definePlugin, PluginError, type Event, type Submission } from '@ilium/plugin-sdk';
import { list, runHook } from '@ilium/plugin-sdk/host/extensions';
import { log } from '@ilium/plugin-sdk/host/log';

const RECOGNISERS = 'com.example.notes/recognisers';
const BEFORE_EXPORT = 'com.example.notes/before-export';

let recogniserEntries: string[] = [];

function reloadRecognisers(): void {
  try {
    recogniserEntries = list(RECOGNISERS).map((entry) => entry.entryJson);
  } catch (error) {
    log('warn', `list failed: ${String(error)}`);
  }
}

async function exportNote(text: string): Promise<void> {
  try {
    const finalPayload = await runHook(BEFORE_EXPORT, JSON.stringify({ text }));
    log('info', finalPayload === undefined ? 'export: unchanged' : `export: ${finalPayload}`);
  } catch (error) {
    if (error instanceof PluginError && error.extensionCode === 'extension.vetoed') {
      log('info', `export refused: ${error.message}`);
      return;
    }
    log('warn', `export failed: ${String(error)}`);
  }
}

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  activate() {
    reloadRecognisers();
  },
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'topic-published' && event.val.topic === 'ilium.extensions/changed') {
        reloadRecognisers();
      }
      if (event.tag === 'command-invoked' && event.val.command === 'com.example.notes/export') {
        void exportNote('Release notes for Friday');
      }
    }
    return [];
  },
});
```

### Example: TypeScript contributor

`src/server.ts`:

```ts
import { definePlugin, type HookCall, type HookReply } from '@ilium/plugin-sdk';

export const { lifecycle, handler, hookHandler } = definePlugin({
  side: 'server',
  onEvents() {
    return [];
  },
  handleHook(call: HookCall): HookReply {
    if (call.extension !== 'strip_tokens') {
      return { tag: 'pass' };
    }
    const payload = JSON.parse(call.payloadJson) as { text?: string };
    const text = payload.text ?? '';
    const cleaned = text.replace(/ghp_[A-Za-z0-9]{36}/g, '[removed]');
    if (cleaned === text) {
      return { tag: 'pass' };
    }
    return { tag: 'replace', val: JSON.stringify({ ...payload, text: cleaned }) };
  },
});
```

### Entry-bound services

Some collect entries carry a `service` field: `compaction/techniques`, `cost/quota-sources`, `notifications/channels`, `recap/fact-sources`, `scenes/scene-data-sources`, `scheduler/schedule-sources`, `search/sources`, `session-convert/formats`, `settings/config-importers`, `keys-and-help/help-sources` and `diagnostics/report-sections`.

An entry field `service` names a service whose prefix is the contributor's own id and that the contributor provides, or the entry fails with `manifest.extension-entry-invalid`. The owner of the point may call it without its own `services.use` grant: the kernel admits the call because the entry is active. The normal 30 second timeout and the 1 MiB caps apply. The method, parameters and result of each point are fixed on the core plugin's page, in the row for that point. [EXT-043]

### Dynamic collect points

A collect point with `"dynamic": true` lets a contributor add and remove entries at run time. A static point takes entries from the manifest only. The owner sees both kinds in `list`.

**Rust**

```rust
use ilium_plugin_sdk::host::extensions;

// manifest: "extensions.contribute:com.example.notes/recognisers"
fn add_project_key(key_prefix: &str) -> Result<(), String> {
    let entry = format!(r#"{{"id":"project","regex":"{key_prefix}-[0-9]+"}}"#);
    extensions::register("com.example.notes/recognisers", "project-key", 50, &entry)
        .map_err(|error| format!("{error:?}"))
}
```

**TypeScript**

```ts
import { register } from '@ilium/plugin-sdk/host/extensions';

// manifest: "extensions.contribute:com.example.notes/recognisers"
export function addProjectKey(keyPrefix: string): void {
  const entry = JSON.stringify({ id: 'project', regex: `${keyPrefix}-[0-9]+` });
  register('com.example.notes/recognisers', 'project-key', 50, entry);
}
```

Dynamic is a property of the point, and the core points that allow it are marked in the **Dynamic** column of [Core extension points](/pages/reference-core-extension-points/). `register` with an id that the plugin already used replaces that entry. A plugin can only remove entries it registered. A call on a point that is not `dynamic` returns `not-dynamic`.

### What the user sees

The review dialog groups a package's extensions under **Adds to other plugins**. Each line names the owner, the point and what the handler can do. A high-risk point carries a warning.

<figure>
<pre class="term">
╭──────────────────────────────────────────────╮
│ Review: com.example.tickets 0.1.0            │
│                                              │
│ Adds to other plugins                        │
│   Smart Copy > Recognisers                   │
│     adds a text pattern                      │
│   Smart Copy > After copy                    │
│     can change copied text                   │
│   Text triggers > Before fire                │
│     can stop a trigger from sending text     │
│     (high risk)                              │
│                                              │
│   [allow]  [decline]                         │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The review dialog groups a package's contributions by owner. The high-risk line is unchecked until the user ticks it.</figcaption>
</figure>

On Settings > Plugins, the plugin page has an **Extensions** tab. For each point the plugin owns, it lists the contributors in run order, each with a toggle and move up and down buttons. For each extension the plugin contributes, it lists the point and a toggle. The user's order overrides the priority. The host stores the toggles, not the plugin.

<figure>
<pre class="term">
╭──────────────────────────────────────────────╮
│ Smart Copy > Extensions                      │
│                                              │
│ After copy (transform)                       │
│  1 [x] com.example.tickets    strip_tokens   │
│  2 [x] com.example.redact     mask_emails    │
│  3 [ ] com.example.legacy     old_filter     │
│                                              │
│ Recognisers (collect)                        │
│    [x] com.example.tickets    jira-key       │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The Extensions tab of an owner. Arrows move a row, and the order replaces the priority.</figcaption>
</figure>

Two states need a note.

- **Dormant.** An extension that is dormant shows the reason `waiting for <owner-id>` (owner missing or disabled), `needs <point>@<version>, <owner-id> has <version>` (version mismatch) or `point full (<n> handlers)`. It does nothing, and the toggle stays as the user set it.
- **Stopped.** An extension that breached three times in a row, whatever the cause, shows `stopped after 3 failures` and a Reset button. The stop lasts until the instance restarts or Reset is pressed, and it is not saved to disk. [LIF-270]

- [LIF-270] A stop after three breaches MUST end at an instance restart of the contributor or Reset and MUST NOT be persisted. Test: stop an extension, restart the contributor, and call it.

<figure>
<pre class="term">
╭──────────────────────────────────────────────╮
│ com.example.tickets > Extensions             │
│                                              │
│ Smart Copy > After copy                      │
│   [x] strip_tokens                           │
│   stopped after 3 failures       [Reset]     │
│                                              │
│ Notes Plus > Export hook                     │
│   [x] stamp_header                           │
│   waiting for com.example.notes-plus         │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. A stopped extension and a dormant one on the contributor's Extensions tab.</figcaption>
</figure>

### Edge cases and error codes

Extension codes do not add a case to the 15-case `error-code` enum. Each extension code is carried by a normal `plugin-error`: the `code` field holds the mapped base code from the table below, and the `message` field starts with the extension code, a colon and a space, for example `extension.vetoed: com.example.quiet-hours: quiet hours are on` [EXT-003]. The message names the contributor's plugin id; the user interface shows the display name instead (host prefix rule [SEC-292]). The SDKs parse this prefix and expose it as `error.extension_code()` in Rust and `error.extensionCode` in TypeScript, so a plugin never matches on free text. The prefix is the only part of `message` that is a contract.

The WIT `extension-error` enum (7 cases) is the sync result of `list`, `run-hook`, `register` and `unregister`; the SDK converts each case to the mapped `plugin-error`. Codes found later arrive as `refused(...)` or `failed(...)` in the completion of the `run-hook` request, with the same mapping. Codes marked log-only never reach the caller; they are written to the log of the plugin that caused them.

| Extension code | Base `code` | Where it appears |
|---|---|---|
| `extension.unknown-point` | `not-found` | sync |
| `extension.not-granted` | `not-permitted` | sync |
| `extension.schema-mismatch` | `invalid-argument` | sync for a caller payload or entry; log-only for a bad handler reply |
| `extension.cap-exceeded` | `invalid-argument` for a payload, reply or entry over its size cap; `quota-exceeded` when a point already holds `max_entries` | sync |
| `extension.reentrant` | `conflict` | sync |
| `extension.not-dynamic` | `unsupported` | sync |
| `extension.vetoed` | `conflict` | completion `refused(conflict)` |
| `extension.depth-exceeded` | `conflict` | sync; travels as `reentrant` |
| `extension.entry-rejected` | `invalid-argument` | log-only, to the contributor's log and the owner's log |
| `rate-limited` (no extension prefix) | `rate-limited` | sync from `run-hook`, with `retry-after-ms`, above 200 calls per second per point |
| `extension.handler-timeout`, `extension.reply-not-allowed`, `extension.disabled`, `extension.chain-cap`, `extension.veto-overridden` | none | log-only |

The 7-case `extension-error` enum is unchanged: `extension.depth-exceeded` travels as `reentrant`, and `rate-limited` is the base error code.

A payload over 64 KiB is always `extension.cap-exceeded` (base `invalid-argument`), not `extension.schema-mismatch`; a payload that fits the size cap but fails `payload_schema` is `extension.schema-mismatch` [EXT-004]. When a chain reaches 100 ms, the handlers after the cap are skipped and each skipped handler is logged with `extension.chain-cap` [EXT-005].

| Code | When | Retry |
|---|---|---|
| `extension.unknown-point` | The point is not declared by an installed and enabled owner. | No |
| `extension.not-granted` | The caller lacks `extensions.provide` or `extensions.contribute` for the point. | No |
| `extension.schema-mismatch` | The payload or an entry fails its schema. A bad `replace` or `answer` is logged with this code and counts as `pass`. | No, fix the data |
| `extension.cap-exceeded` | The point already holds `max_entries` entries, or the payload, reply or entry is over its size cap. | No |
| `extension.reentrant` | A handler called `run-hook` on a point already on the current chain, or the chain is deeper than 3. | No |
| `extension.depth-exceeded` | A `run-hook` from a handler at depth 3. Counts as `pass`, not as a breach. | No |
| `extension.entry-rejected` | The owner refused an entry outside its allowlist ([SEC-283]). | No, fix the entry |
| `rate-limited` | More than 200 `run-hook` calls per second on one point. | After `retry-after-ms` |
| `extension.not-dynamic` | `register` or `unregister` on a point that is not `dynamic`. | No |
| `not-found` (host error) | `unregister` for an entry this plugin did not register. | No |
| `extension.handler-timeout` | A handler passed its deadline. Counts as `pass` and as one breach. | No |
| `extension.reply-not-allowed` | The reply is not allowed in the point's mode. Counts as `pass`. | No |
| `extension.vetoed` | A `veto` handler or a guard refused. The completion is `refused(conflict)`, the message is `extension.vetoed: <contributor plugin id>: <reason>`. | Not by the owner, the action stays refused |
| `extension.disabled` | The extension was stopped after three breaches in a row or turned off by the user. Not called. Log-only. | After the user presses Reset in Settings > Plugins. A breach stop also ends when the plugin restarts. A user turn-off persists across restarts |

Install errors:

| Code | When |
|---|---|
| `manifest.extension-point-duplicate` | Two points have the same id in one plugin. |
| `manifest.extension-point-invalid` | A bad id, kind, mode or schema. |
| `manifest.extension-entry-invalid` | An entry fails the point's `entry_schema`. |
| `manifest.extension-side` | A handler is declared on a different side from the owner's call. |
| `manifest.extension-limit` | More than 16 points or 64 extensions in one plugin. |
| `manifest.grant-missing-extension` | An extension has no `extensions.contribute` grant, or a point has no `extensions.provide` grant. |
| `manifest.exports-mismatch` | A plugin with hook extensions does not list `hook-handler` in `exports`. |
| `manifest.grant-wildcard` | A grant for a point ends in `*`. |
| `manifest.point-risk` | An owner declares `risk: "standard"` for a point that meets [SEC-280]. |
| `manifest.grant-dependency` | A contributor to a point with a sensitive data class lacks the matching read grant ([SEC-282]), or a command-running entry lacks `process.run`. |
| `manifest.replaces-tier` | A T3 package, or a T2 package whose surface needs T1-only grants, replaces a core plugin. |
| `policy.replacement` | The admin policy `extensions.allow_replacement = false` refuses a replacement. |
| `manifest.schema-keyword` | A data schema uses a keyword outside the [Data schema subset](/pages/reference-settings-schema/#data-schema-subset). |

Further edge cases:

- **Owner disabled.** The contributions to its points go dormant at once. Handlers in a running chain finish or time out.
- **Contributor removed while a chain runs.** The chain skips its handler and logs `extension.disabled`.
- **Equal priority.** Two extensions on one point with the same priority run in plugin id order, then in extension id order [EXT-007].
- **Two contributors, same id.** The id is unique per contributor and point, so both stay. The user sees them as two rows.
- **Version mismatch.** A contribution asks for `point@X.Y.Z`. The version is a caret range (PKG-317, PKG-318, PKG-328 in [Dependency resolution](/pages/reference-dependency-resolution/)): the owner's major must equal the requested major, the owner's minor must be at least the requested minor, and the patch is ignored; for major 0 the owner's minor must equal the requested minor instead. Otherwise it stays dormant and the Plugins screen says why. A contribution without `@version` matches any owner version. Examples with an owner at `1.2.0`: `@1.0.0` accepted, `@1.2.0` accepted, `@1.3.0` dormant, `@2.0.0` dormant. With an owner at `0.1.2`: `@0.1.0` and `@0.1.9` accepted, `@0.2.0` dormant [EXT-006].
- **Hook with no handlers.** `run-hook` resolves at once with outcome `ok` and no reply. The owner continues with its original data.
- **Slow owner path.** The owner pays for the chain. The action waits for a `veto`, `transform` or `first` chain, at most the 100 ms cap plus one deadline. A handler that is skipped at the cap counts as `pass`, so a `veto` chain that never reached a refusal lets the action go on.
- **No retry by the kernel.** The kernel never re-runs a failed chain. The owner decides.

### Used by

- Every core plugin that has an **Extension points** section uses this mechanism for it. [Reference: core extension points](/pages/reference-core-extension-points/) lists them all.
- [Extending core plugins](/pages/extending-core-plugins/) shows the quiet hours scenario: a `veto` hook on `ilium.core.notifications/before-notify`.

The host pre-activates the inactive contributors of an active owner in the background, so the first call does not wait for a cold start. A handler skipped because its instance is still activating counts as `pass`, not as a breach. [LIF-274]

- [LIF-274] The host MUST activate inactive contributors of an active owner in the background. Test: owner activates, contributor is `active` without a call.

Requirement ids for this page: [EXT-002], [EXT-003], [EXT-004], [EXT-005], [EXT-006], [EXT-007], [EXT-008], [EXT-010], [EXT-011], [EXT-012], [EXT-013], [EXT-014], [EXT-020], [EXT-021], [EXT-022], [EXT-024], [EXT-025], [EXT-026], [EXT-027], [EXT-028], [EXT-029], [EXT-030], [EXT-043]. See the [requirements index](/pages/reference-requirements/).

### See also

- [Extending core plugins](/pages/extending-core-plugins/) for the concept guide and the decision table.
- [Reference: core extension points](/pages/reference-core-extension-points/) for the points of every core plugin.
- [extensions.provide](/pages/capability-extensions-provide/) and [extensions.contribute](/pages/capability-extensions-contribute/) for the grant rules.
- [services](/pages/api-services/), [messages and topics](/pages/api-messages-and-topics/) and [menu provider](/pages/api-menu-provider/) for the other ways plugins work together.
- [exports](/pages/api-exports/) for `hook-handler` and its `with-hook-handler` fragment.
- [requests and outcomes](/pages/requests-and-outcomes/) for `request-completed` and `refused`.
- [errors and limits](/pages/reference-errors-and-limits/) and [reference: errors and limits](/pages/reference-errors-and-limits/) for the full code table.
- [Rust SDK](/pages/rust-sdk/) and [TypeScript SDK](/pages/typescript-sdk/) for the module layout of each language.
- [reference: manifest](/pages/reference-manifest/) for the manifest keys, and [versioning](/pages/versioning/) for point versions.

Checked against `ilium:plugin@0.1.0`.

### Security

Hook failure policy, budgets and nesting: see [Extension security](/pages/reference-extension-security/) ([SEC-284] to [SEC-290]).

Requirements for this page: [Extension points requirements](/pages/reference-requirements-extension-points/).
