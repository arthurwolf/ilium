---
layout: page
title: Inference providers
permalink: /pages/core-plugin-inference/
---

## Inference providers

Inference providers is the plugin form of Ilium's AI provider layer. It answers every model request that another plugin makes with the [inference](/pages/api-inference/) import, lists the available models, streams replies, and offers a Test provider button. Its id is `ilium.core.inference` and it runs on both sides: the server side talks to the provider over HTTP, and the client side draws the provider panel. It is the kernel-known `inference-provider` service, and it serves the model list as a catalogue.

The privacy controls stay in the kernel. The AI admission gate checks every request against the `purpose` that the caller named, whatever plugin serves the model, so this plugin cannot widen what is sent.

### What it covers

- **Providers, loading models, test provider.** Choose a provider, load its models, and send a short request to check it (Providers, Loading models, Test provider in the user documentation).
- **AI admission gate and privacy controls.** What is sent where, and the guidance on privacy (What is sent where, Privacy guidance).

### See it

<figure><img src="/assets/demos/07b-settings-tour-b.gif" alt="A tour of the middle Settings tabs, including the AI provider options" loading="lazy"><figcaption>The provider options in the Settings tour.</figcaption></figure>

### What the user sees

The settings tab has the provider choice, the model choice read from a catalogue, the Test provider button and an info row. A test sends a short request and shows the reply as it arrives.

<figure>
<pre class="term">╭─ Inference ──────────────────────────────────────────╮
│ Provider ………………………………… ‹ OpenAI-compatible ›         │
│ Model …………………………………………………… ‹ gpt-5.6-terra ›         │
│ Test provider …………………………………………… [ Run test ]         │
│ Last test ……………………………… 812 ms, gpt-5.6-terra         │
│<span class="t-dim"> Prompts and screen text go to the provider you</span>       │
│<span class="t-dim"> choose. Nothing is sent when no provider is set.</span>     │
╰──────────────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 56 columns. Illustrative. The Inference tab with leaders between label and value.</figcaption>
</figure>

<figure>
<pre class="term">╭─ Test provider ──────────────────────────────╮
│<span class="t-dim"> Sending: Reply with the word ok.</span>             │
│ ok                                           │
│ Reply received from gpt-5.6-terra            │
│<span class="t-ok"> Test passed in 812 ms</span>                        │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. A streaming test reply filling in line by line, then the result.</figcaption>
</figure>

### Manifest

This block is a fragment. It lists the contributions. The whole manifest is in [Complete manifest](#complete-manifest).

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.inference",
  "capabilities": {
    "server": ["inference.provide", "http:api.openai.com", "http:localhost"],
    "client": ["ui.panels"]
  }
}
```

### Complete manifest

This is the whole manifest of the plugin at `ilium:plugin@0.1.0`. The Manifest section above shows the contributions in detail; here the required keys, the sides, the `capabilities` per side and the `activation` triggers are all present. Activation follows [startup and lazy activation](/pages/reference-kernel-and-boot/#startup-and-lazy-activation-of-the-30-core-plugins). [COR-050]

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.inference",
  "name": "Inference",
  "version": "0.1.0",
  "publisher": "Ilium",
  "description": "Inference providers is the plugin form of Ilium's AI provider layer.",
  "license": "MIT",
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0"
  },
  "capabilities": {
    "server": [
      "inference.provide",
      "http:api.openai.com",
      "http:localhost"
    ],
    "client": [
      "ui.panels",
      "settings.write-kernel:inference",
      "settings.write-kernel:llm-instructions"
    ]
  },
  "activation": [
    "service:ilium:inference/provider"
  ],
  "limits_tier": "first-party"
}
```

### Kernel tabs this plugin owns

This plugin is the single owner of the following built-in Settings tabs. It holds `settings.write-kernel:<tab>` for each one (T1 only, one holder per tab, see [kernel settings](/pages/api-kernel-settings-and-keymap/#writing-a-kernel-tab)). The values stay in the same `config.toml` tables as before, so no user setting moves. [COR-030] [COR-033]

| Tab id | Label | Config table | Readers |
|---|---|---|---|
| `inference` | Inference | `[inference]` | Titles, Recap, Search, Smart copy, Compaction, Voice |
| `llm-instructions` | LLM Instructions | `[inference.instructions]`, `[voice]` | Titles, Recap, Smart copy, Voice |

If the plugin is disabled, the tab stays in the rail with its stored values shown read-only and the notice "Inference providers is disabled. Values are kept." The kernel keeps reading the keys it consumes itself (listed in the Consumer column of the key table in [config and data migration](/pages/reference-config-and-data-migration/)). Behaviour that only this plugin provides is off. The exact result for this plugin is its row in [kernel and boot](/pages/reference-kernel-and-boot/). [COR-022]

### Samples

#### 1. An OpenAI-compatible provider

The plugin exports `inference-provider` (see [plugin exports](/pages/api-exports/) and [services](/pages/services/)) and holds the [inference.provide grant](/pages/capability-inference-provide/). `complete` accepts the call and returns at once. A task answers with `provider-reply` and the same call number.

**Rust**

```rust
use ilium_plugin_sdk::host::inference::{self, Completion, ProviderCall};
use ilium_plugin_sdk::prelude::*;

// manifest: "capabilities": { "server": ["inference.provide",
//   "http:api.openai.com", "http:localhost"] },
//   exports: ["inference-provider"]
#[ilium_plugin(side = "server", exports = ["inference-provider"])]
#[derive(Default)]
struct Provider;

impl InferenceProvider for Provider {
    /// Accepts the call and returns at once. A task answers later with
    /// provider_reply and the same call number.
    fn complete(&mut self, call: ProviderCall) -> Result<(), PluginError> {
        let number = call.call;
        let request = call.request;
        task::spawn(async move {
            // `ask_endpoint` is the plugin's own code: one HTTP round trip.
            let outcome = ask_endpoint(&request).await.map(|text| Completion {
                text,
                model: "gpt-5.6-terra".into(),
                input_tokens: 0,
                output_tokens: 0,
            });
            inference::provider_reply(number, outcome);
        });
        Ok(())
    }

    fn models(&mut self) -> Vec<String> {
        vec!["gpt-5.6-terra".into()]
    }
}
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import { providerReply, type ProviderCall } from '@ilium/plugin-sdk/host/inference';

// manifest: "capabilities": { "server": ["inference.provide",
//   "http:api.openai.com", "http:localhost"] },
//   exports: ["inference-provider"]
export const { lifecycle, handler, inferenceProvider } = definePlugin({
  side: 'server',
  // Accepts the call and returns at once; the promise answers later.
  complete(call: ProviderCall): void {
    // askEndpoint is the plugin's own code: one HTTP round trip.
    void askEndpoint(call.request).then(
      (text) => providerReply(call.call, {
        tag: 'ok',
        val: { text, model: 'gpt-5.6-terra', inputTokens: 0, outputTokens: 0 },
      }),
      (error: { code: string; message: string }) => providerReply(call.call, {
        tag: 'err',
        val: { code: error.code, message: error.message, retryAfterMs: undefined },
      }),
    );
  },
  models(): string[] {
    return ['gpt-5.6-terra'];
  },
});
```

#### 2. Stream tokens

Replies arrive piece by piece over server-sent events, using [HTTP streams](/pages/api-http-streams/) and the [http grant](/pages/capability-http/). The plugin appends each `http-stream-data` chunk and finishes the reply on `http-stream-closed`; see [events](/pages/api-events/).

**Rust**

```rust
use ilium_plugin_sdk::host::http::{self, HttpRequest, StreamFraming, StreamRequest};
use ilium_plugin_sdk::host::inference::{self, Completion};
use ilium_plugin_sdk::prelude::*;

// manifest: "capabilities": { "server": ["http:api.openai.com"] }
fn open_reply_stream(body: Vec<u8>) -> Result<StreamId, PluginError> {
    http::open_stream(StreamRequest {
        request: HttpRequest {
            method: "POST".into(),
            url: "https://api.openai.com/v1/chat/completions".into(),
            headers: vec![],
            body: Some(body),
            timeout_millis: 120_000,
        },
        framing: StreamFraming::ServerSentEvents,
        max_chunk_bytes: 16_384,
    })
}

impl Provider {
    fn on_stream_event(&mut self, event: Event) {
        match event {
            // The host parses the SSE framing: one chunk is one event payload.
            Event::HttpStreamData(data) => {
                self.text.push_str(&delta_from_payload(&data.chunk));
            }
            Event::HttpStreamClosed(closed) => {
                let outcome = closed.outcome.map(|_| Completion {
                    text: std::mem::take(&mut self.text),
                    model: "gpt-5.6-terra".into(),
                    input_tokens: 0,
                    output_tokens: 0,
                });
                inference::provider_reply(self.call, outcome);
            }
            _ => {}
        }
    }
}
```

**TypeScript**

```ts
import { openStream } from '@ilium/plugin-sdk/host/http';
import { providerReply } from '@ilium/plugin-sdk/host/inference';
import type { Event } from '@ilium/plugin-sdk';

// manifest: "capabilities": { "server": ["http:api.openai.com"] }
export function openReplyStream(body: Uint8Array): bigint {
  return openStream({
    request: {
      method: 'POST',
      url: 'https://api.openai.com/v1/chat/completions',
      headers: [],
      body,
      timeoutMillis: 120_000,
    },
    framing: 'server-sent-events',
    maxChunkBytes: 16_384,
  });
}

let text = '';

export function onStreamEvent(event: Event, call: bigint): void {
  if (event.tag === 'http-stream-data') {
    // The host parses the SSE framing: one chunk is one event payload.
    text += deltaFromPayload(event.val.chunk);
  } else if (event.tag === 'http-stream-closed') {
    const done = event.val.outcome;
    providerReply(call, done.tag === 'ok'
      ? { tag: 'ok', val: { text, model: 'gpt-5.6-terra', inputTokens: 0, outputTokens: 0 } }
      : { tag: 'err', val: done.val });
    text = '';
  }
}
```

#### 3. Models as a catalogue

The model choice row reads its options from the plugin instead of listing them. The plugin exports `catalog-provider` (see [catalogues](/pages/api-catalogs/) and [services](/pages/services/)), and the `choice` row, described in [settings row types](/pages/reference-settings-rows/), names the catalogue.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::ui::CatalogItem;
use ilium_plugin_sdk::host::catalogs::{CatalogPage, CatalogQuery};

// manifest: "contributes": { "catalogs": [{ "id": "models" }] },
//   exports: ["catalog-provider"]; the settings row says
//   { "type": "choice", "key": "model", "source": { "catalog": "models" } }
impl CatalogProvider for Provider {
    fn query_catalog(&mut self, query: CatalogQuery) -> Result<CatalogPage, PluginError> {
        let items: Vec<CatalogItem> = self
            .models
            .iter()
            .filter(|name| name.contains(&query.filter))
            .map(|name| CatalogItem::new(name, name))
            .collect();
        Ok(CatalogPage { items, next: None, total: Some(self.models.len() as u32) })
    }
}
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import type { CatalogPage, CatalogQuery } from '@ilium/plugin-sdk/host/catalogs';

// manifest: "contributes": { "catalogs": [{ "id": "models" }] },
//   exports: ["catalog-provider"]; the settings row says
//   { "type": "choice", "key": "model", "source": { "catalog": "models" } }
const models: string[] = ['gpt-5.6-terra', 'gpt-5.6-luna'];

export const { lifecycle, handler, catalogProvider } = definePlugin({
  side: 'server',
  queryCatalog(query: CatalogQuery): CatalogPage {
    const items = models
      .filter((name) => name.includes(query.filter))
      .map((name) => ({ id: name, title: name, keywords: [] }));
    return { items, next: undefined, total: models.length };
  },
});
```

#### 4. Test provider

A `button` row runs a test call (see [settings extension](/pages/settings-extension/) and [settings row types](/pages/reference-settings-rows/)). When `request-completed` arrives, the plugin writes latency and model into an `info` row. The outcome model is in [requests and outcomes](/pages/requests-and-outcomes/).

**Rust**

```rust
use ilium_plugin_sdk::host::{clock, inference, settings};
use ilium_plugin_sdk::host::inference::{ChatMessage, CompletionRequest};
use ilium_plugin_sdk::prelude::*;

// settings rows: { "type": "button", "id": "test", "label": "Run test",
//     "command": "test-provider" }
//   { "type": "info", "id": "last_test", "title": "Last test",
//     "text": "Not run yet", "live": true }
impl Handler for Provider {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            match event {
                Event::CommandInvoked(invoked) if invoked.command.ends_with("/test-provider") => {
                    let request = CompletionRequest {
                        messages: vec![ChatMessage {
                            role: "user".into(),
                            content: "Reply with the word ok.".into(),
                        }],
                        max_output_tokens: Some(16),
                        json_schema: None,
                        purpose: "provider-test".into(),
                    };
                    self.started = clock::monotonic_millis();
                    self.test = inference::complete_request(request).ok();
                }
                Event::RequestCompleted(done) if Some(done.request) == self.test => {
                    self.test = None;
                    let elapsed = clock::monotonic_millis() - self.started;
                    let text = match inference::take_completion(done.request) {
                        Some(c) => format!("{elapsed} ms, {}", c.model),
                        None => "failed".to_string(),
                    };
                    let _ = settings::set_info("last_test", &text);
                }
                _ => {}
            }
        }
        Vec::new()
    }
}
```

**TypeScript**

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { monotonicMillis } from '@ilium/plugin-sdk/host/clock';
import { completeRequest, takeCompletion } from '@ilium/plugin-sdk/host/inference';
import { setInfo } from '@ilium/plugin-sdk/host/settings';

// settings rows: { "type": "button", "id": "test", "label": "Run test",
//     "command": "test-provider" }
//   { "type": "info", "id": "last_test", "title": "Last test",
//     "text": "Not run yet", "live": true }
let test: bigint | undefined;
let started = 0n;

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'command-invoked' && event.val.command.endsWith('/test-provider')) {
        started = monotonicMillis();
        test = completeRequest({
          messages: [{ role: 'user', content: 'Reply with the word ok.' }],
          maxOutputTokens: 16,
          jsonSchema: undefined,
          purpose: 'provider-test',
        });
      } else if (event.tag === 'request-completed' && event.val.request === test) {
        test = undefined;
        const completion = takeCompletion(event.val.request);
        setInfo('last_test', completion
          ? `${monotonicMillis() - started} ms, ${completion.model}`
          : 'failed');
      }
    }
    return [];
  },
});
```

#### 5. Provider picker panel

A small client panel switches provider with a `choice` control from [input widgets](/pages/widgets-input/) and writes the selection to the plugin's settings.

**Rust**

```rust
use ilium_plugin_sdk::host::settings;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::layout::Area;
use ilium_plugin_sdk::types::ui::{ActionKind, ActionValue, UiTree, ViewTarget};
use ilium_plugin_sdk::ui::build::Tree;

// manifest: "capabilities": { "client": ["ui.panels"] }
impl Views for ProviderPanel {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        let mut tree = Tree::new();
        let picker = tree
            .choice("Provider", self.providers.clone(), self.selected)
            .key("provider");
        tree.finish(picker)
    }
}

impl Handler for ProviderPanel {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::UiAction(action) = event else { continue };
            if let ("provider", ActionKind::Changed, ActionValue::Index(i)) =
                (action.key.as_str(), action.kind, action.value)
            {
                self.selected = i;
                let patch = format!("{{\"provider\": {:?}}}", self.providers[i as usize]);
                let _ = settings::set_json(&patch);
            }
        }
        Vec::new()
    }
}
```

**TypeScript**

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { setJson } from '@ilium/plugin-sdk/host/settings';
import { choice, tree } from '@ilium/plugin-sdk/ui/build';

// manifest: "capabilities": { "client": ["ui.panels"] }
const providers = ['OpenAI-compatible', 'Local server'];
let selected = 0;

export const { lifecycle, handler, views } = definePlugin({
  side: 'client',
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag !== 'ui-action' || event.val.key !== 'provider') continue;
      if (event.val.kind === 'changed' && event.val.value.tag === 'index') {
        selected = event.val.value.val;
        setJson(JSON.stringify({ provider: providers[selected] }));
      }
    }
    return [];
  },
  render() {
    return tree(choice('Provider', providers, selected, { key: 'provider' }));
  },
});
```

### Extension points

Other plugins change what `ilium.core.inference` does through the points below. The plugin owns each point (grant `extensions.provide:ilium.core.inference/<point>`). Contributors need `extensions.contribute:ilium.core.inference/<point>`. See [Extension points](/pages/api-extension-points/) for the mechanism and [Extending core plugins](/pages/extending-core-plugins/) for the five ways to work with a core feature.

| Point | Kind | What it does | Payload | Risk |
|---|---|---|---|---|
| `before-request` | hook (transform) | Rewrite or refuse an outgoing model request, for example to remove private text. | purpose, provider, model, messages_length, text; reply: text | high |
| `after-response` | hook (observe) | Learn that a request finished, with its token counts. | purpose, provider, model, input_tokens, output_tokens, millis | standard |
| `provider-profiles` | collect | Offer a ready-made provider setup in the provider chooser. | entry: id, title, base_url, models_path, key_name | high |

**Services** (grant `services.use:<name>`):

- `ilium.core.inference/complete`: complete: one-shot completion through the chosen provider

**Topics** (list under `contributes.topics.subscribes`):

- `ilium.core.inference/request-finished`: { purpose, provider, millis }

Providers implemented by plugins use the `inference-provider` export and `inference.provide`.

**Replaceable:** yes. A replacement lists `"replaces": ["ilium.core.inference"]` and provides the whole surface (see [Extending core plugins](/pages/extending-core-plugins/#replacing-a-core-plugin)).

#### Example: Replaces home-directory paths in outgoing text with `~`

A package adds a `transform` handler. Each handler gets the previous output and may replace it.

```json
{
  "api": "ilium:plugin@0.1.0",
  "exports": [
    "hook-handler"
  ],
  "capabilities": { "server": ["extensions.contribute:ilium.core.inference/before-request"] },
  "contributes": {
    "extensions": [
      {
        "point": "ilium.core.inference/before-request",
        "id": "redact-paths",
        "priority": 50,
        "handler": "redact_paths"
      }
    ]
  }
}
```

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;

impl HookHandler for Plugin {
    fn handle_hook(&mut self, call: HookCall) -> HookReply {
        match call.extension.as_str() {
            "redact_paths" => {
                // call.payload_json holds: purpose, provider, model, messages_length, text
                let mut payload: serde_json::Value = match serde_json::from_str(&call.payload_json) {
                    Ok(value) => value,
                    Err(_) => return HookReply::Pass,
                };
                // Change the fields the hook owns, for example payload["text"].
                let new_payload_json = payload.to_string();
                HookReply::Replace(new_payload_json)
            }
            _ => HookReply::Pass,
        }
    }
}
```

**TypeScript**

```ts
definePlugin({
  handleHook(call) {
    if (call.extension !== 'redact_paths') return { tag: 'pass' };
    // call.payloadJson holds: purpose, provider, model, messages_length, text
    const payload = JSON.parse(call.payloadJson);
    // Change the fields the hook owns, for example payload.text.
    const newPayloadJson = JSON.stringify(payload);
    return { tag: 'replace', val: newPayloadJson };
  },
});
```

A handler that is late, traps or returns a reply the mode does not allow counts as `pass`. See [Extension points](/pages/api-extension-points/#hooks).

### Extension points this plugin owns

The page does not state the side that runs these points, and its manifest has no `extensions.provide` grant for them, so the side is `unknown`. The page also gives no version, deadline, max or dynamic values, so those are the documented defaults and are marked `(default)`.

| Point | Side | Version | Deadline | Max | Dynamic |
|---|---|---|---|---|---|
| `ilium.core.inference/before-request` | server | 0.1.0 (default) | 20 ms (default) | 8 (default) | no (default) |
| `ilium.core.inference/after-response` | server | 0.1.0 (default) | 20 ms (default) | 8 (default) | no (default) |
| `ilium.core.inference/provider-profiles` | client | 0.1.0 (default) | none (default) | 64 (default) | no (default) |

Side basis: `ilium.core.inference/before-request` is server because the point rewrites the outgoing model request, which the server side sends to the provider over HTTP, and the page's handler example is granted `extensions.contribute` on the server.

Side basis: `ilium.core.inference/after-response` is server because it observes the finished request and its token counts, which come from the server-side HTTP reply.

Side basis: `ilium.core.inference/provider-profiles` is client because its entries feed the provider chooser, which the page draws in a client panel with a `choice` control.

### ABI used

- [inference](/pages/api-inference/) and [inference.provide](/pages/capability-inference-provide/)
- [plugin exports](/pages/api-exports/) and [services](/pages/services/)
- [HTTP streams](/pages/api-http-streams/) and [the http grant](/pages/capability-http/)
- [catalogues](/pages/api-catalogs/)
- [events](/pages/api-events/)
- [settings extension](/pages/settings-extension/) and [settings row types](/pages/reference-settings-rows/)
- [requests and outcomes](/pages/requests-and-outcomes/)
- [input widgets](/pages/widgets-input/)

### See also

- [Core plugins](/pages/core-plugins/)
- [Titles and organisation](/pages/core-plugin-titles/), a main consumer
- [Security](/pages/security/)

### Security

Risk marks of this plugin's extension points follow the corrected table in [Extension security](/pages/reference-extension-security/) ([SEC-293]).
