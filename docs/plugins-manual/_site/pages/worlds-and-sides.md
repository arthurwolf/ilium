# Worlds and sides

## Worlds and sides

A plugin package holds up to three components, one for each side of Ilium: `server`, `client` and `scene`. The server component runs in one host for each session server. The client component runs in one host for each attached client, so two attached clients mean two instances with separate state. The scene component runs the background animation of one client.

Each component targets a world. The world decides which host imports the component may call and which exports it must provide. The host checks both when it installs the package, and it refuses a package that breaks either rule.

The two halves of a package exchange messages. They never share memory, and each side has its own instance, its own memory and its own limits.

## You will need

- **Grants.** Each side declares the capabilities it needs in the manifest `capabilities` object, with one list per side. The host records the grants per plugin, per side and per user configuration. See [Capabilities and grants](/pages/capabilities/).
- **A toolchain.** Rust stable 1.90 or later with the `wasm32-wasip2` target, or Bun 1.2 or later for TypeScript. The SDK pins the remaining tools, and `bun install` installs them inside the plugin folder. Nothing is installed globally.
- **The SDK.** The crate `ilium-plugin-sdk` for Rust, or the npm package `@ilium/plugin-sdk` for TypeScript.
- **Background reading.** [Architecture](/pages/architecture/) explains the kernel, and [Packages and manifests](/pages/packages/) explains the package layout.

## The three worlds

| Side | Manifest key | World | Required exports | Optional exports |
|---|---|---|---|---|
| Server | `server` | `ilium:server/base@0.1.0` | `lifecycle`, `handler` | `detector`, `job-runner`, `service-provider`, `inference-provider` |
| Client | `client` | `ilium:client/base@0.1.0` | `lifecycle`, `handler` | `views`, `annotator`, `job-runner`, `service-provider`, `inference-provider` |
| Scene | `scene` | `ilium:anim/base@0.1.0` | `lifecycle`, `renderer` | `job-runner` |

The server world runs once per session server and has the widest set of imports. The client world reaches almost the same list, except the CLI, the HTTP API, workspaces and sessions, and it adds the UI, keys, clipboard, help, plugins, keymap and the editor and basic primitives. The scene world is the smallest. It draws through its own surface, and it has no intents, topics or messages.

The table below is generated from section 9 of [WIT reference](/pages/reference-wit/#9-worlds). It shows which world may call each import. Interfaces of `ilium:host` and `ilium:primitives` are named without the package; the two scene-only interfaces are in `ilium:anim`. "Yes" means the world imports it; "no" means the host rejects a component that uses it.

| Imports | Server | Client | Scene |
|---|---|---|---|
| `log`, `clock`, `storage`, `codec`, `fs`, `http`, `jobs`, `geocode` | yes | yes | yes |
| `timers`, `journal`, `tree`, `panes`, `intents-host`, `settings`, `process`, `inference`, `vector`, `sounds`, `topics`, `commands`, `services`, `catalogs`, `kernel-settings`, `client-requests`, `extensions`, `terminal`, `agent` | yes | yes | no |
| `cli`, `http-api`, `workspace`, `sessions` | yes | no | no |
| `audio`, `media`, `location`, `gpu` | no | yes | yes |
| `ui-host`, `keys`, `clipboard`, `help`, `plugins`, `keymap`, `editor`, `basic` | no | yes | no |
| `anim/surface`, `anim/fonts` | no | no | yes |

The `ilium:host/messages` interface no longer exists. Sending a message is the `message` intent, and receiving one is the `message` event.

WASI imports are allowed at their Preview 2 versions for `wasi:cli`, `wasi:io`, `wasi:clocks`, `wasi:random` and `wasi:filesystem`. Standard output and error go to the plugin log. A component that imports `wasi:sockets` or `wasi:http` is refused at install.

<figure>
<svg viewBox="0 0 400 330" role="img" aria-label="Server, client and scene worlds with their imports">
  <defs>
    <marker id="worlds-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
      <path d="M 0 0 L 10 5 L 0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="8" y="8" width="184" height="216" rx="6" fill="none" stroke="currentColor"/>
  <text x="16" y="26" font-size="11" fill="currentColor">ilium:server/base</text>
  <text x="16" y="46" font-size="10" fill="currentColor">log clock timers tree panes</text>
  <text x="16" y="60" font-size="10" fill="currentColor">intents storage settings fs</text>
  <text x="16" y="74" font-size="10" fill="currentColor">codec jobs http process</text>
  <text x="16" y="88" font-size="10" fill="currentColor">inference services sound</text>
  <text x="16" y="102" font-size="10" fill="currentColor">topics cli http-api</text>
  <text x="16" y="116" font-size="10" fill="currentColor">workspace terminal agent</text>
  <text x="16" y="172" font-size="10" fill="currentColor">Exports: lifecycle, handler</text>
  <text x="16" y="186" font-size="10" fill="currentColor">Optional: detector</text>
  <text x="16" y="200" font-size="10" fill="currentColor">job-runner, service-provider</text>
  <text x="16" y="214" font-size="10" fill="currentColor">inference-provider</text>
  <rect x="208" y="8" width="184" height="216" rx="6" fill="none" stroke="currentColor"/>
  <text x="216" y="26" font-size="11" fill="currentColor">ilium:client/base</text>
  <text x="216" y="46" font-size="10" fill="currentColor">log clock timers tree panes</text>
  <text x="216" y="60" font-size="10" fill="currentColor">intents storage settings fs</text>
  <text x="216" y="74" font-size="10" fill="currentColor">codec jobs http process</text>
  <text x="216" y="88" font-size="10" fill="currentColor">inference services sound</text>
  <text x="216" y="102" font-size="10" fill="currentColor">topics ui commands keys</text>
  <text x="216" y="116" font-size="10" fill="currentColor">clipboard audio media</text>
  <text x="216" y="130" font-size="10" fill="currentColor">location help plugins</text>
  <text x="216" y="144" font-size="10" fill="currentColor">terminal agent editor basic</text>
  <text x="216" y="172" font-size="10" fill="currentColor">Exports: lifecycle, handler</text>
  <text x="216" y="186" font-size="10" fill="currentColor">Optional: views, annotator</text>
  <text x="216" y="200" font-size="10" fill="currentColor">job-runner, service-provider</text>
  <text x="216" y="214" font-size="10" fill="currentColor">inference-provider</text>
  <line x1="192" y1="116" x2="208" y2="116" stroke="currentColor" stroke-dasharray="4 3" marker-start="url(#worlds-arrow)" marker-end="url(#worlds-arrow)"/>
  <line x1="300" y1="224" x2="300" y2="243" stroke="currentColor" marker-end="url(#worlds-arrow)"/>
  <rect x="8" y="244" width="384" height="78" rx="6" fill="none" stroke="currentColor"/>
  <text x="16" y="262" font-size="11" fill="currentColor">ilium:anim/base (scene side)</text>
  <text x="16" y="278" font-size="10" fill="currentColor">log clock storage fs codec jobs http</text>
  <text x="16" y="292" font-size="10" fill="currentColor">audio media location ilium:anim/surface</text>
  <text x="16" y="306" font-size="10" fill="currentColor">Exports: lifecycle, renderer. Optional: job-runner</text>
</svg>
<figcaption>The server and client worlds share most imports. The dashed line is the message route between them. The scene world is smaller and draws through its own surface.</figcaption>
</figure>

## Exports each side provides

Each world requires two or three exports. The SDK generates the export glue from the `#[ilium_plugin]` attribute in Rust or from `definePlugin` in TypeScript.

| Export | Side | Rust trait | Required methods |
|---|---|---|---|
| `lifecycle` | all | `Lifecycle` | `activate`, `deactivate` (both have defaults) |
| `handler` | server, client | `Handler` | `on_events(batch: Vec<Event>) -> Vec<Submission>` |
| `views` | client | `Views` | `render(view: ViewTarget, area: Area) -> UiTree` |
| `renderer` | scene | `anim::renderer::Renderer` | `new(spec: SceneSpec)`; the rest have defaults |

The `activate` call receives an `Activation` record. Its fields are `plugin`, `api`, `instance` (a `u64` that rises for each new instance), `side`, `settings-json`, `granted` (the list of capabilities actually granted, so a plugin can hide features it cannot use) and `restored` (bytes that `deactivate` returned in an earlier run).

`deactivate` receives one of five delivered reasons: `shutdown`, `disabled`, `upgrade`, `revoked` or `granted` (listed with their effects in [Lifecycle](/pages/lifecycle/)). The enum has six cases in a fixed order. The `crash-restart` case is reserved and never delivered [ABI-028], because a trap drops the instance without a call. Bytes returned from `deactivate` come back as `restored` on the next activation.

## Optional exports

The manifest `exports` list names only the optional exports, by interface name. Required exports such as `handler` or `renderer` are never listed. The host reads the component's real export list and compares it with the required exports plus the listed ones. A difference refuses the package with `manifest.exports-mismatch`.

The contract also defines optional exports (`catalog-provider`, the provider form of virtual rows, and others). The list is in [Getting started: overview](/pages/getting-started-overview/). The rule is the same: list each one in the manifest.

In Rust, the same names go into the attribute: `#[ilium_plugin(side = "server", exports = ["detector"])]`.

### Optional exports by side and world variant

A **world variant** is a base world plus the export fragments that a plugin implements. The fragment name is `with-<export>`. The table lists what each fragment adds and where it exists.

| Fragment | Adds the export | Server | Client | Scene |
|---|---|---|---|---|
| `with-detector` | `detector` | yes | no | no |
| `with-views` | `views` | no | yes | no |
| `with-annotator` | `annotator` | no | yes | no |
| `with-jobs` | `job-runner` | yes | yes | yes |
| `with-service` | `service-provider` | yes | yes | no |
| `with-inference-provider` | `inference-provider` | yes | yes | no |
| `with-settings-validate` | `settings-validate` | yes | yes | no |
| `with-catalog` | `catalog-provider` | yes | yes | no |
| `with-menus` | `menu-provider` | no | yes | no |
| `with-virtual-rows` | `virtual-rows-provider` | yes | yes | no |
| `with-tokens` | `token-provider` | no | yes | no |
| `with-hook-handler` | `hook-handler`, which dispatches the hook extensions of the plugin on this side | yes | yes | no |
| `with-migrator` | `migrator`, which converts stored data when `data_version` rose | yes | yes | no |

A side lists `hook-handler` in its manifest `exports` when it contributes handlers to hook extension points, its own or another plugin's. Owning a point and running its hooks does not need the export. A plugin that both owns a point and contributes handlers lists `hook-handler` on each side that contributes handlers; a plugin that only owns points never lists it. The `collect` kind of extension point needs no export, because entries are data (see [Extension points](/pages/api-extension-points/)).

### Where each side runs

Each side runs in the process that hosts it: the `server` side in the session server, the `client` and `scene` sides in each attached client, and a CLI entry with `requires_server: false` in the short-lived CLI process. A T2 or T3 side also has a sandbox helper process. The side's instance, its failure count and its restart timer belong to that process, so a client and the server count failures separately. The full process map is in [Host architecture](/pages/reference-host-architecture/).

## Splitting a plugin across sides

Put a feature on the server side when it must keep working while no client is attached, or when it must survive a client detach. Agent signatures, scheduled input, text triggers, the prompt queue, progress monitors and chatroom routing belong there.

Put a feature on the client side when it draws to this user's screen or uses this user's devices. Settings pages, panels, dialogs, pane bodies, scenes, voice and smart copy belong there.

A scene gets its data only from its constructor argument, which carries its settings and the current palette. It cannot read the server side or send to it.

## Messages between sides

The server side and the client side talk through the `message` intent and the `message` event.

| Type | Field | Type | Meaning |
|---|---|---|---|
| intent `message(message-spec)` | `to` | `message-target` | The receiving plugin, side and client |
| | `channel` | `string` | 1 to 64 characters from `[a-z0-9.-]` |
| | `payload` | `list<u8>` | Up to 1 MiB |
| event `message(message-event)` | `from`, `from-side` | `plugin-id`, `plugin-side` | The sending plugin and side |
| | `from-client` | `option<client-id>` | The sending client, if the sender was a client |
| | `channel`, `payload` | as above | As sent |
| record `message-target` | `plugin`, `side`, `client` | `plugin-id`, `plugin-side`, `option<client-id>` | `client: none` from the server reaches every attached client |

A plugin always reaches its own other side without a grant. A message to another plugin needs the grant `messages.peer:<plugin-id>`, with the exact id and no wildcard. A message to an installed plugin that is not active activates it, and the message is delivered after `activate` returns.

Rust names are `Event::Message(MessageEvent)` and `Intent::Message(MessageSpec)`, with `MessageTarget` and `PluginSide` from `ilium_plugin_sdk::types::intents`. TypeScript uses `{ tag: 'message', val: {...} }` for the event. Its `fromClient` and `client` fields are `bigint`, and its `payload` is a `Uint8Array`.

## Worked example in Rust: the server side

The server keeps the notes. When the client sends a note on the channel `notes.add`, the server stores it and sends the count back on `notes.summary` to the same client.

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::intents::{MessageSpec, MessageTarget, PluginSide};

const ADD_CHANNEL: &str = "notes.add";
const SUMMARY_CHANNEL: &str = "notes.summary";

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct NotesServer {
    notes: Vec<String>,
}

// The default methods accept activation and deactivation without state to restore.
impl Lifecycle for NotesServer {}

impl Handler for NotesServer {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            let Event::Message(message) = event else { continue };
            if message.channel != ADD_CHANNEL {
                continue;
            }
            let Ok(text) = String::from_utf8(message.payload) else { continue };
            self.notes.push(text);
            let summary = format!("{} notes", self.notes.len());
            submissions.push(Submission::new(Intent::Message(MessageSpec {
                to: MessageTarget {
                    plugin: "example.notes".into(),
                    side: PluginSide::Client,
                    client: message.from_client,
                },
                channel: SUMMARY_CHANNEL.into(),
                payload: summary.into_bytes(),
            })));
        }
        submissions
    }
}
```

The server needs no grant for this, because it only messages its own client side. `Submission::new` takes the next correlation number and sets no dedup key.

## Worked example in TypeScript: the client side and manifest

The client shows the summary in a panel. When the summary arrives, it stores the text and asks the host to render the panel again.

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { invalidate } from '@ilium/plugin-sdk/host/ui';
import { block, paragraph, tree } from '@ilium/plugin-sdk/ui/build';

const SUMMARY_CHANNEL = 'notes.summary';
const PANEL_VIEW = 'example.notes.notes';
let summaryText = 'No notes yet';

export const { lifecycle, handler, views } = definePlugin({
  side: 'client',
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag !== 'message' || event.val.channel !== SUMMARY_CHANNEL) continue;
      summaryText = new TextDecoder().decode(event.val.payload);
      invalidate({ tag: 'panel', val: PANEL_VIEW });
    }
    return [];
  },
  render() {
    return tree(block(paragraph([summaryText])));
  },
});
```

The manifest declares both sides, the client export and the grant the panel needs:

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "example.notes",
  "name": "Notes",
  "version": "0.1.0",
  "publisher": "Example Labs",
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0",
    "exports": []
  },
  "client": {
    "entry": "client.wasm",
    "world": "ilium:client/base@0.1.0",
    "exports": ["views"]
  },
  "capabilities": {
    "client": ["ui.panels"]
  },
  "contributes": {
    "panels": [{ "id": "notes", "title": "Notes", "modal": false }]
  },
  "files": []
}
```

The `files` list is empty only for this sketch. A real package lists both components with their length and SHA-256, as described on the [packages page](/pages/packages/). The panel id `notes` is global as `example.notes.notes`.

To send a note, the client submits a `message` intent with `to` set to `{ plugin: 'example.notes', side: 'server', client: undefined }` and `channel` set to `notes.add`. The server side then answers as shown above.

## Edge cases and errors

| Situation | Result |
|---|---|
| Channel empty, longer than 64 characters or outside `[a-z0-9.-]` | `refused(invalid-argument)` for that submission |
| Payload over 1 MiB | `refused(invalid-argument)` for that submission |
| Target plugin not installed or not enabled | `refused(not-found)` |
| Message to another plugin without `messages.peer:<plugin-id>` | `refused(not-permitted)`, logged at level warn |
| Message to the plugin's own other side | Always allowed, no grant needed |
| Target plugin not active, and its `activate` fails | The message is discarded and logged at level error under the receiver. The sender is not told |
| Component exports differ from the manifest | Package refused with `manifest.exports-mismatch` |
| Component imports outside its world, or `wasi:sockets` or `wasi:http` | Package refused at install; the installer names the import and the world |
| Trap or deadline breach in an export | The instance is dropped without `deactivate`, and restarts after 1 s, then 5 s |
| Third failure within 10 minutes | The plugin is disabled on that side. On the client side the count is kept per client |
| `ui.invalidate` for a view the plugin does not declare | Ignored; the host logs one warn line that names the view |

A `refused` result applies to one submission. A batch of more than 256 submissions traps the call, and nothing in it is applied [ABI-008]. A malformed submission (for example a missing mandatory dedup key) or a missing grant refuses the whole batch, and the other submissions then complete as `refused(cancelled)` with the message `batch refused: submission <index>` [ABI-009].

## What the user sees

The server side has no screen. The client side shows the Notes panel, which reads `No notes yet` until the first summary arrives, and then `3 notes` after the third note. Errors reach the user through the plugin log, which the command `ilium plugin logs` prints. Each line holds the date and time, the level padded to five characters, the plugin id and the message.

<pre class="term">$ ilium plugin logs example.notes --tail 3
2026-10-11 09:14:02 warn  example.notes refused submit: not-permitted
2026-10-11 09:14:04 error example.notes trap in on-events: index out of bounds
2026-10-11 09:14:04 info  example.notes restarting in 1 s (failure 1 of 3)
</pre>

Illustrative, not generated. The first line is a refused peer message; the last two show a trap and the restart that follows it.

## See also

- [Architecture](/pages/architecture/)
- [Packages and manifests](/pages/packages/)
- [Events and intents](/pages/events-and-intents/)
- [Capabilities and grants](/pages/capabilities/)
- [Messages and topics](/pages/api-messages-and-topics/)
- [Lifecycle](/pages/lifecycle/)
- [Errors and limits](/pages/reference-errors-and-limits/)
- [Exports](/pages/api-exports/)
- [UI](/pages/api-ui/)
- [Scenes](/pages/api-scenes/)
- [Your first plugin](/pages/getting-started-first-plugin/)

API version checked: `ilium:plugin@0.1.0`.
