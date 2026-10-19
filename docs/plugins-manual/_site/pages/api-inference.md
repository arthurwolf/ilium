# inference

## inference

The `inference` import sends a chat completion to the language model provider that the user selected in Settings. The plugin never names a provider. It names the feature that asks, through `purpose`, and the AI admission gate and the privacy log use that name. A plugin can ask for a reply that matches a JSON Schema, and it can list the provider's models. A plugin can also be the provider: it exports `inference-provider` and answers the calls that the host routes to it.

A call returns a request id at once, and the completion arrives later as a `request-completed` event. The Rust and TypeScript wrappers hide that step and return the completion itself.

**You will need:** the `inference` grant on the server or client side that makes the call. Jobs can also call it with the same grant (see [jobs](/pages/api-jobs/)). A plugin that supplies a model instead needs `inference.provide` and must export `inference-provider` (see Providing models below). Embeddings, which use the same provider, are a separate grant, `inference.embed`, described in [vector and embeddings](/pages/api-vector/). The Rust examples use the `serde` and `serde_json` crates. The TypeScript examples need the `@ilium/plugin-sdk` package installed with `bun install`.

<div class="note">
<p>The grant takes no argument. <code>inference:anthropic</code> is refused at install, because the user chooses the provider, not the plugin.</p>
</div>

### Interface

```wit
// inference (S C, grant inference)
record chat-message { role: string, content: string }
record completion-request { messages: list<chat-message>, max-output-tokens: option<u32>,
                            json-schema: option<string>, purpose: string }
record completion { text: string, model: string, input-tokens: u32, output-tokens: u32 }
complete: func(request: completion-request) -> result<request-id, plugin-error>;
take-completion: func(request: request-id) -> option<completion>;
models: func() -> result<request-id, plugin-error>;
take-models: func(request: request-id) -> option<list<string>>;
provider-reply: func(call: u64, outcome: result<completion, plugin-error>) -> result<_, plugin-error>;   // inference.provide

// inference-provider (export of S C, fragment with-inference-provider)
record provider-call { call: u64, request: completion-request }
complete: func(call: provider-call) -> result<_, plugin-error>;       // answer later with provider-reply
models: func() -> list<string>;
```

The SDK imports are `ilium_plugin_sdk::host::inference` in Rust and `@ilium/plugin-sdk/host/inference` in TypeScript (see [Rust SDK](/pages/rust-sdk/) and [TypeScript SDK](/pages/typescript-sdk/) for the module layout).

### Functions

| Function | Rust | TypeScript | Grant | Result |
|---|---|---|---|---|
| `complete` | `inference::complete(request).await` | `await complete(request)` | `inference` | `Completion`. Resolves when the provider answers. Fails with `PluginError`. |
| `complete` (raw) | `inference::complete_request(request)` | `completeRequest(request)` | `inference` | `RequestId`, returned at once. |
| `take-completion` | `inference::take_completion(id)` | `takeCompletion(id)` | `inference` | `Option<Completion>`. Returns the completion once. |
| `models` | `inference::models().await` | `await models()` | `inference` | `Vec<String>` of model names. |
| `models` (raw) | `inference::models_request()` | `modelsRequest()` | `inference` | `RequestId`, returned at once. |
| `take-models` | `inference::take_models(id)` | `takeModels(id)` | `inference` | `Option<Vec<String>>`. Returns the list once. |
| `provider-reply` | `inference::provider_reply(call, outcome)` | `providerReply(call, outcome)` | `inference.provide` | `Result<(), PluginError>`. Answers one call that the host sent to the plugin's own provider export. |

Use the async wrapper for new code. Use the raw form only when the plugin must hold the request id across events, or inside a job, where `jobs.wait-request` replaces the event.

### Records

| Record | Field | Rust | TypeScript | Meaning |
|---|---|---|---|---|
| `chat-message` | `role` | `String` | `string` | `system`, `user` or `assistant`. The provider receives the role as sent. |
| | `content` | `String` | `string` | The text of the message. |
| `completion-request` | `messages` | `Vec<ChatMessage>` | `ChatMessage[]` | The conversation, in order. The list must not be empty. |
| | `max_output_tokens` | `Option<u32>` | `number \| undefined` | `None` asks for the model's maximum output. The SDK sets no smaller default. |
| | `json_schema` | `Option<String>` | `string \| undefined` | A JSON Schema document, as text. The reply is still text in `completion.text`. |
| | `purpose` | `String` | `string` | The feature's name, for example `note-tags`. The gate and the privacy log use it. |
| `completion` | `text` | `String` | `string` | The reply, exactly as the provider sent it. |
| | `model` | `String` | `string` | The model that answered. |
| | `input_tokens` | `u32` | `number` | Tokens in the request. |
| | `output_tokens` | `u32` | `number` | Tokens in the reply. |
| `provider-call` | `call` | `u64` | `bigint` | The number to pass back to `provider-reply`, unchanged. |
| | `request` | `CompletionRequest` | `CompletionRequest` | The request to answer. |

### Routing to the selected provider

<figure>
<svg viewBox="0 0 400 280" role="img" aria-label="Routing an inference call to the selected provider">
  <title>Routing to the selected provider</title>
  <desc>A completion request passes the admission gate. The kernel routes it to the provider that the user selected. The provider answers, and the reply returns to the calling plugin.</desc>
  <defs>
    <marker id="inf-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto">
      <path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
    </marker>
  </defs>
  <text x="10" y="50" font-size="11" fill="currentColor">complete(request)</text>
  <rect x="10" y="60" width="130" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="75" y="86" font-size="12" text-anchor="middle" fill="currentColor">Calling plugin</text>
  <path d="M140 82 H160 V42 H180" fill="none" stroke="currentColor" marker-end="url(#inf-arrow)"/>
  <rect x="180" y="20" width="200" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="280" y="46" font-size="12" text-anchor="middle" fill="currentColor">Admission gate and log</text>
  <line x1="280" y1="64" x2="280" y2="110" stroke="currentColor" marker-end="url(#inf-arrow)"/>
  <text x="288" y="90" font-size="10" fill="currentColor">allowed</text>
  <rect x="180" y="110" width="200" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="280" y="130" font-size="12" text-anchor="middle" fill="currentColor">Selected provider</text>
  <text x="280" y="146" font-size="10" text-anchor="middle" fill="currentColor">chosen by the user</text>
  <line x1="280" y1="154" x2="280" y2="200" stroke="currentColor" marker-end="url(#inf-arrow)"/>
  <text x="288" y="180" font-size="10" fill="currentColor">routed</text>
  <rect x="180" y="200" width="200" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="280" y="218" font-size="12" text-anchor="middle" fill="currentColor">Provider plugin or built-in</text>
  <text x="280" y="234" font-size="10" text-anchor="middle" fill="currentColor">exports inference-provider</text>
  <path d="M180 222 H75 V104" fill="none" stroke="currentColor" stroke-dasharray="4 3" marker-end="url(#inf-arrow)"/>
  <text x="84" y="160" font-size="10" fill="currentColor">reply</text>
  <text x="10" y="270" font-size="11" fill="currentColor">The user's choice decides where each call goes.</text>
</svg>
<figcaption>The plugin names no provider. The dashed return path carries the reply back to the calling plugin, which reads it with <code>take_completion</code> or the awaited wrapper.</figcaption>
</figure>

If no provider is selected, the call fails at once with `not-found`. It does not wait. A plugin that must have a provider declares `requires.services` with `ilium:inference/provider@0.1.0`, and the host activates it only when a provider runs.

The sequence for a plugin that is itself the provider shows where `provider-reply` fits.

<figure>
<svg viewBox="0 0 430 250" role="img" aria-label="Sequence of a completion answered by a provider plugin">
<title>Provider plugin sequence</title>
<defs>
<marker id="infseq-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
<path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
</marker>
</defs>
<text x="55" y="16" font-size="12" text-anchor="middle" fill="currentColor">Caller</text>
<text x="215" y="16" font-size="12" text-anchor="middle" fill="currentColor">Host</text>
<text x="375" y="16" font-size="12" text-anchor="middle" fill="currentColor">Provider plugin</text>
<line x1="55" y1="24" x2="55" y2="240" stroke="currentColor" stroke-dasharray="3 3"/>
<line x1="215" y1="24" x2="215" y2="240" stroke="currentColor" stroke-dasharray="3 3"/>
<line x1="375" y1="24" x2="375" y2="240" stroke="currentColor" stroke-dasharray="3 3"/>
<line x1="55" y1="44" x2="213" y2="44" stroke="currentColor" marker-end="url(#infseq-arrow)"/>
<text x="62" y="39" font-size="10" fill="currentColor">complete(request)</text>
<line x1="213" y1="62" x2="57" y2="62" stroke="currentColor" stroke-dasharray="4 3" marker-end="url(#infseq-arrow)"/>
<text x="62" y="77" font-size="10" fill="currentColor">request id at once</text>
<line x1="215" y1="98" x2="373" y2="98" stroke="currentColor" marker-end="url(#infseq-arrow)"/>
<text x="222" y="93" font-size="10" fill="currentColor">complete(provider-call)</text>
<line x1="373" y1="120" x2="217" y2="120" stroke="currentColor" stroke-dasharray="4 3" marker-end="url(#infseq-arrow)"/>
<text x="222" y="135" font-size="10" fill="currentColor">ok (accepted)</text>
<line x1="375" y1="158" x2="217" y2="158" stroke="currentColor" marker-end="url(#infseq-arrow)"/>
<text x="222" y="153" font-size="10" fill="currentColor">provider-reply(call, outcome)</text>
<line x1="213" y1="196" x2="57" y2="196" stroke="currentColor" stroke-dasharray="4 3" marker-end="url(#infseq-arrow)"/>
<text x="62" y="191" font-size="10" fill="currentColor">request-completed</text>
<text x="62" y="224" font-size="10" fill="currentColor">take-completion(request)</text>
</svg>
<figcaption>The provider's <code>complete</code> returns at once; the completion follows through <code>provider-reply</code>. An error from <code>complete</code> fails the caller's request.</figcaption>
</figure>

### Example 1: suggest tags for a note

A command arrives with the note text in its `arguments-json`, as `{"note": "..."}`. The plugin asks for tags as JSON that matches a small schema, then logs them. The command `com.example.notes/tag-note` is declared in the manifest (see [commands](/pages/api-commands/)).

**Grants:** `inference` on the server side.

**Rust**

```rust
use ilium_plugin_sdk::host::inference::{self, ChatMessage, CompletionRequest};
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::CommandInvokedEvent;
use serde::Deserialize;

const TAG_SCHEMA: &str = r#"{
  "type": "object",
  "properties": { "tags": { "type": "array", "items": { "type": "string" } } },
  "required": ["tags"]
}"#;

#[derive(Deserialize)]
struct TagArguments {
    note: String,
}

#[derive(Deserialize)]
struct TagReply {
    tags: Vec<String>,
}

async fn suggest_tags(note: String) -> Vec<String> {
    let request = CompletionRequest {
        messages: vec![
            ChatMessage { role: "system".into(), content: "Reply with tags only.".into() },
            ChatMessage { role: "user".into(), content: note },
        ],
        max_output_tokens: None,
        json_schema: Some(TAG_SCHEMA.into()),
        purpose: "note-tags".into(),
    };
    match inference::complete(request).await {
        Ok(completion) => serde_json::from_str::<TagReply>(&completion.text)
            .map(|reply| reply.tags)
            .unwrap_or_default(),
        Err(error) => {
            log::log(Level::Warn, &format!("note tags failed: {:?}", error.code));
            Vec::new()
        }
    }
}

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct NoteTags;

impl Handler for NoteTags {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::CommandInvoked(CommandInvokedEvent { command, arguments_json, .. }) = event
            else { continue };
            if command != "com.example.notes/tag-note" {
                continue;
            }
            let Ok(arguments) = serde_json::from_str::<TagArguments>(&arguments_json)
            else { continue };
            task::spawn(async move {
                let tags = suggest_tags(arguments.note).await;
                log::log(Level::Info, &format!("note tags: {}", tags.join(", ")));
            });
        }
        Vec::new()
    }
}
```

**TypeScript**

```ts
import { definePlugin, type Event, type PluginError, type Submission } from '@ilium/plugin-sdk';
import { complete, type CompletionRequest } from '@ilium/plugin-sdk/host/inference';
import { log } from '@ilium/plugin-sdk/host/log';

const TAG_SCHEMA = JSON.stringify({
  type: 'object',
  properties: { tags: { type: 'array', items: { type: 'string' } } },
  required: ['tags'],
});

async function suggestTags(note: string): Promise<string[]> {
  const request: CompletionRequest = {
    messages: [
      { role: 'system', content: 'Reply with tags only.' },
      { role: 'user', content: note },
    ],
    maxOutputTokens: undefined,
    jsonSchema: TAG_SCHEMA,
    purpose: 'note-tags',
  };
  try {
    const completion = await complete(request);
    return (JSON.parse(completion.text) as { tags: string[] }).tags;
  } catch (error) {
    log('warn', `note tags failed: ${(error as PluginError).code}`);
    return [];
  }
}

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag !== 'command-invoked' || event.val.command !== 'com.example.notes/tag-note') {
        continue;
      }
      const { note } = JSON.parse(event.val.argumentsJson) as { note: string };
      void suggestTags(note).then((tags) => log('info', `note tags: ${tags.join(', ')}`));
    }
    return [];
  },
});
```

The `take_completion` outcome for the example's reply looks like this:

```text
text: {"tags": ["plugins", "api"]}
model: claude-haiku-5-5
input-tokens: 118, output-tokens: 14
```

<pre class="term"><span class="t-prompt">$</span> ilium plugin logs com.example.notes --tail 2
<span class="t-dim">2026-10-12 09:30:12</span> info  note tags: plugins, api
<span class="t-dim">2026-10-12 09:31:40</span> warn  note tags failed: RateLimited
</pre>

The plugin log after one successful tag request and one throttled request. The lines are written by the plugin, so the text is an illustration of the form, not captured output.

### Example 2: list the models and offer them in a picker

The settings page of a plugin shows the models that the provider offers. An empty list means that no provider is selected or that the call failed, so the picker can still open.

**Grants:** `inference` on the client side.

**Rust**

```rust
use ilium_plugin_sdk::host::inference;
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::prelude::*;

/// Model names for a settings picker. An empty list means that no provider is selected
/// or that the call failed.
async fn available_models() -> Vec<String> {
    match inference::models().await {
        Ok(models) => models,
        Err(error) if error.code == ErrorCode::NotFound => Vec::new(),
        Err(error) => {
            log::log(Level::Warn, &format!("model list failed: {:?}", error.code));
            Vec::new()
        }
    }
}
```

**TypeScript**

```ts
import type { PluginError } from '@ilium/plugin-sdk';
import { models } from '@ilium/plugin-sdk/host/inference';
import { log } from '@ilium/plugin-sdk/host/log';

export async function availableModels(): Promise<string[]> {
  try {
    return await models();
  } catch (error) {
    const code = (error as PluginError).code;
    if (code !== 'not-found') {
      log('warn', `model list failed: ${code}`);
    }
    return [];
  }
}
```

### Models as a catalogue

A settings row or a `choice` widget does not need the code above. The host catalogue `ilium.models` lists the configured inference models, and a `choice` takes it as its source with `source: catalog("ilium.models")` (see [settings extension](/pages/settings-extension/)). The catalogue needs the `inference` grant, which is the only host catalogue with a grant. A plugin that wants the entries in code reads them with `catalogs.query`, a synchronous call for host catalogues:

**Rust**

```rust
use ilium_plugin_sdk::host::catalogs::{self, CatalogQuery};

fn model_titles() -> Vec<String> {
    let query = CatalogQuery {
        catalog: "ilium.models".into(),
        filter: String::new(),
        cursor: None,
        limit: 64,
    };
    match catalogs::query(&query) {
        Ok(page) => page.items.into_iter().map(|item| item.title).collect(),
        Err(_) => Vec::new(), // not-permitted without `inference`, not-found if absent
    }
}
```

**TypeScript**

```ts
import { query } from '@ilium/plugin-sdk/host/catalogs';

export function modelTitles(): string[] {
  try {
    const page = query({ catalog: 'ilium.models', filter: '', cursor: undefined, limit: 64 });
    return page.items.map((item) => item.title);
  } catch {
    return []; // not-permitted without `inference`
  }
}
```

An item whose provider is unavailable carries a `disabled-reason` of at most 40 characters, and the choice dialog shows it. The dialog is the client's value dialog: `●` marks the current option and a disabled option gets the suffix `[unavailable]` with its reason beneath.

<figure>
<pre class="term">╭  ≡ ● · Model─────────────────────────────╮
│ <span class="t-hi">● claude-haiku-5-5</span>                       │
│   claude-sonnet-5-5                      │
│   local-qwen [unavailable]               │
│     Unavailable: server offline          │
│ 2 of 3 options · Current: claude-haiku-… │
│ Enter applies · Esc cancels              │
╰──────────────────────────────────────────╯
</pre>
<figcaption>A choice dialog over the <code>ilium.models</code> catalogue. The row format, the <code>●</code> marker, the <code>[unavailable]</code> suffix and the two footer lines follow the client's choice dialog (value_dialog.rs). The model names are made up.</figcaption>
</figure>

### Providing models (`inference.provide`)

A plugin that supplies the model answers the calls that the host routes to it. It needs the `inference.provide` grant (without it, install refuses the package, and a declined grant makes the plugin unavailable in Settings). Its manifest lists `inference-provider` in `exports`, not the WIT fragment name. The grant page is [Capability: inference.provide](/pages/capability-inference-provide/).

| Name | Rust | TypeScript | Meaning |
|---|---|---|---|
| `inference-provider.complete` (export) | `fn complete(&mut self, call: ProviderCall) -> Result<(), PluginError>` | `complete(call)` | The host asks for one completion. The provider answers later with `provider_reply`. An `Err` fails the caller's request. |
| `inference-provider.models` (export) | `fn models(&mut self) -> Vec<String>` | `models()` | The model names that the provider offers. |
| `inference.provider-reply` (import) | `inference::provider_reply(call, outcome)` | `providerReply(call, outcome)` | Answers one call with a `Completion` or a `PluginError`. |

`ProviderCall` holds `call: u64` (a `bigint` in TypeScript) and the `request: CompletionRequest`. Keep the `call` number and pass it back unchanged.

A provider that also offers embeddings serves them through the same selection; a provider without them makes `vector.embed` fail with `unsupported` (see [vector and embeddings](/pages/api-vector/)).

**Rust**

```rust
use ilium_plugin_sdk::host::inference::{self, Completion, ProviderCall};
use ilium_plugin_sdk::prelude::*;

#[ilium_plugin(side = "server", exports = ["inference-provider"])]
#[derive(Default)]
struct LocalModel;

impl InferenceProvider for LocalModel {
    fn complete(&mut self, call: ProviderCall) -> Result<(), PluginError> {
        let number = call.call;
        let prompt = call.request.messages.last().map(|m| m.content.clone()).unwrap_or_default();
        task::spawn(async move {
            // `run_local_model` is the plugin's own code, for example an http call
            // to a server on the user's machine.
            let outcome = run_local_model(&prompt).await.map(|text| Completion {
                text,
                model: "local-qwen".into(),
                input_tokens: 0,
                output_tokens: 0,
            });
            inference::provider_reply(number, outcome);
        });
        Ok(())
    }

    fn models(&mut self) -> Vec<String> {
        vec!["local-qwen".into()]
    }
}
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import { providerReply, type ProviderCall } from '@ilium/plugin-sdk/host/inference';

export const { lifecycle, handler, inferenceProvider } = definePlugin({
  side: 'server',
  complete(call: ProviderCall): void {
    const prompt = call.request.messages.at(-1)?.content ?? '';
    // `runLocalModel` is the plugin's own code.
    void runLocalModel(prompt).then(
      (text) => providerReply(call.call, {
        tag: 'ok',
        val: { text, model: 'local-qwen', inputTokens: 0, outputTokens: 0 },
      }),
      (error: { code: string; message: string }) => providerReply(call.call, {
        tag: 'err',
        val: { code: error.code, message: error.message, retryAfterMs: undefined },
      }),
    );
  },
  models(): string[] {
    return ['local-qwen'];
  },
});
```

### Errors

Every error is a `plugin-error` with a `code`, a `message` and an optional `retry-after-ms`. Errors from a call that returns a request id are of two kinds. Synchronous errors are returned at once, and nothing is sent. Completion errors arrive later, through the request.

| Code | Returned when | Retry |
|---|---|---|
| `not-permitted` | The `inference` grant is declined, the admission policy refuses the `purpose` or the provider, or `provider-reply` runs without `inference.provide`. | No |
| `not-found` | No provider is selected. The call returns at once. For `provider-reply`, the call is unknown, answered or expired. | No |
| `invalid-argument` | No messages, input over 4 MiB, a schema over 64 KiB or not valid JSON, or a reply text over 4 MiB. | No, fix the call |
| `rate-limited` | The admission gate throttles the request to the provider. `retry-after-ms` is set. | Yes, after the delay |
| `timed-out` | The provider does not answer within 120 s. | Yes |
| `network` | The provider cannot be reached, or the connection fails. | Yes |
| `busy` | The plugin already has 64 pending requests. | Yes, later |
| `quota-exceeded` | The CPU budget of the execution group is spent. `retry-after-ms` is set. | Yes, after the delay |
| `cancelled` | The plugin or the host cancelled the request. | No |
| `revoked` | The authorization changed during the call. The plugin is activated again. | No |
| `internal` | A host bug. | No |

A provider's own failure, for example an HTTP error from its service, passes through with the provider's code. Related interfaces use the same codes: `catalogs.query` returns `not-found` for an unknown catalogue and `not-permitted` for `ilium.models` without `inference`, and `vector.embed` returns `unsupported` when the provider has no embeddings.

### Limits

| Limit | Value | Result when exceeded |
|---|---|---|
| Messages, all together | 4 MiB | `invalid-argument` |
| `json_schema` text | 64 KiB | `invalid-argument` |
| Time to a completion | 120 s | `timed-out` |
| Reply text in `provider-reply` | 4 MiB | `invalid-argument` |
| Pending requests per plugin (`pending_requests`) | 64 by default | `busy` |
| Catalogue page (`ilium.models`) | 256 items | clamped |

A plugin can raise `pending_requests` in its manifest `limits`. The ceiling is 256 for T1 plugins and 64 for T2 and T3 plugins. The host clamps a higher request to the ceiling and logs it.

### What the user sees

- **Install review.** The review dialog shows the grant as a row of the side that asks. The user can decline it, and a plugin without the grant still installs.

<figure>
<pre class="term">╭  ≡ ● · Install com.example.notes 0.1.0───────────────────────────────────╮
│ Example Corp · signed (T2)                                               │
│                                                                          │
│ Client                                                                   │
│ [x] Use your AI model provider ……………………………………………………………… inference        │
│     Requests go to the provider you selected in Settings.                │
│                                                                          │
│   Esc Cancel                                          Enter Install      │
╰──────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>The review row for <code>inference</code> in the install dialog, composed by hand from the client's glyphs: rounded border, chrome title, the <code>[x]</code> checkbox and the grey <code>…</code> leader. The wording is the manual's illustration, not a capture.</figcaption>
</figure>

- **Provider choice.** Requests go to the provider that the user selected in Settings (the Inference tab). The plugin cannot choose another one.
- **Privacy log.** Each request that the gate admits adds one line to the privacy log, with the plugin id, the `purpose`, the provider, the model and the token counts. Settings > Privacy shows that log.
- **Plugin log.** The lines that the plugin writes appear in `ilium plugin logs`, as in Example 1.

### Edge cases

- **Declined grant.** `complete` and `models` return `not-permitted`. Keep the feature working without AI, for example by using the first line of a note as its title.
- **Reply that is not JSON.** The host returns the text as the provider sent it. A parse error is the plugin's own problem, so the plugin handles it.
- **Second read.** `take_completion` and `take_models` return the value once.
- **Jobs.** A job instance calls `complete` with the same grant. It waits for the reply with `jobs.wait-request` (see [jobs](/pages/api-jobs/)).
- **Provider grant declined.** A provider plugin whose `inference.provide` grant is declined is never selected, and Settings shows it as unavailable.
- **Provider that never replies.** The caller's request fails with `timed-out` after 120 s. A late `provider-reply` then returns `not-found`.

### Used by

- Automatic pane titles (feature 41) and session naming (feature 42) ask for a short title.
- Smart Copy AI (feature 66) asks for a `json-schema` answer that picks text on screen.
- Tree organisation (feature 43) uses structured output for its suggested moves and names.
- AI triggers (feature 44) run completions on command events.
- The icon picker (feature 20) ranks glyphs by meaning through the embeddings of the same provider.

### See also

- [vector and embeddings](/pages/api-vector/) for `inference.embed` and the host-held index.
- [http](/pages/api-http/) for calls to a provider that a plugin reaches directly. Those calls also pass the admission gate.
- [Plugin exports](/pages/api-exports/) for the `inference-provider` export that a provider plugin implements.
- [Services](/pages/api-services/) for the `requires.services` rule that makes a plugin wait for a provider.
- [jobs](/pages/api-jobs/) for waiting on a completion inside a job.
- [Capability: inference](/pages/capability-inference/) and [Capability: inference.provide](/pages/capability-inference-provide/) for grant rules.

Checked against `ilium:plugin@0.1.0`.

### Security

Every request is recorded in the privacy log; see [User security and privacy](/pages/user-plugin-security-and-privacy/) ([SEC-264]).
