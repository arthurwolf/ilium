---
layout: page
title: "Capability: inference"
permalink: /pages/capability-inference/
---

## inference

The `inference` capability lets a plugin ask the language model provider that the user selected for a completion, and ask which models that provider offers. The plugin never names a provider. The kernel routes each request to the chosen provider, and the AI admission gate checks every request before it leaves the machine. A request that the gate refuses never reaches the provider.

Use this capability for features that need a model's answer, such as a generated note title or a tag suggestion. Keep every feature working without it. A user who has no provider, or who declined the grant, must still get the plain, non-AI result.

**You need:**

- an `inference` grant on the server or client side. Scenes cannot request it. A background job runs with the grants of its plugin side and may call `inference` as well (see [jobs](/pages/capability-jobs-run/)).
- a provider selected by the user in Settings. Without one, every call returns `not-found` at once.
- no language runtime beyond the SDK for your language. The examples below use Rust and TypeScript.

A plugin that supplies a model is covered on the [inference.provide](/pages/capability-inference-provide/) page.

### Grant syntax

The grant takes no argument.

| Grant | Valid | Reason |
|---|---|---|
| `inference` | yes | the only form |
| `inference:anthropic` | no | the grant takes no argument; the user picks the provider |
| `inference.provide` | no | a different grant, for plugins that supply a provider |

### Surface

Every call that returns a `request-id` is a slow call. It returns at once and completes later as a `request-completed` event. The SDK wraps each one in an `async` function, so most plugins never see the raw form.

| WIT function | Rust (`host::inference`) | TypeScript (`host/inference`) | Returns |
|---|---|---|---|
| `complete(request)` | `complete(request).await` | `await complete(request)` | `Completion` |
| `take-completion(id)` | `take_completion(id)` | `takeCompletion(id)` | `Option<Completion>` |
| `models()` | `models().await` | `await models()` | `Vec<String>` |
| `take-models(id)` | `take_models(id)` | `takeModels(id)` | `Option<Vec<String>>` |

The raw `complete` and `models` functions, which return a request id, are available as `complete_request` and `models_request` in Rust.

| WIT record | Fields | Rust | TypeScript |
|---|---|---|---|
| `chat-message` | `role: string`, `content: string` | `ChatMessage` | `{ role, content }` |
| `completion-request` | `messages: list<chat-message>`, `max-output-tokens: option<u32>`, `json-schema: option<string>`, `purpose: string` | `CompletionRequest` | `CompletionRequest` with `maxOutputTokens`, `jsonSchema` |
| `completion` | `text: string`, `model: string`, `input-tokens: u32`, `output-tokens: u32` | `Completion` | `{ text, model, inputTokens, outputTokens }` |

The Rust `Option` fields map to `undefined` in TypeScript when unset.

### Request fields

- `messages` must hold at least one message. Each `role` is `system`, `user` or `assistant`. Any other value is `invalid-argument`.
- `max-output-tokens: none` asks for the model's maximum output. Set a value only when the feature needs a shorter answer. The value `0` is `invalid-argument`.
- `json-schema` asks the provider for output that matches the schema. The schema is at most 64 KiB and must be valid JSON Schema. The provider can still return text that does not match, so parse and check the result in your code.
- `purpose` names the feature that makes the request. It is 1 to 64 bytes from `a-z`, `0-9` and `-`. The admission gate and the privacy log both use it, so name the feature honestly. Do not reuse one purpose for two features.

### How a call flows

<figure>

<svg viewBox="0 0 320 300" role="img" aria-label="Inference flow from complete call through admission gate to selected provider and completion">
  <defs>
    <marker id="inf-arrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
      <path d="M0,0 L8,4 L0,8 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="10" y="8" width="300" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="160" y="32" text-anchor="middle" font-size="12" fill="currentColor">Plugin calls complete(request)</text>
  <line x1="160" y1="48" x2="160" y2="66" stroke="currentColor" marker-end="url(#inf-arrow)"/>
  <rect x="10" y="66" width="300" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="160" y="90" text-anchor="middle" font-size="12" fill="currentColor">Admission gate: kernel policy for AI requests</text>
  <line x1="160" y1="106" x2="160" y2="124" stroke="currentColor" marker-end="url(#inf-arrow)"/>
  <rect x="10" y="124" width="300" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="160" y="148" text-anchor="middle" font-size="12" fill="currentColor">Kernel routes to the user's chosen provider</text>
  <line x1="160" y1="164" x2="160" y2="182" stroke="currentColor" marker-end="url(#inf-arrow)"/>
  <rect x="10" y="182" width="300" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="160" y="206" text-anchor="middle" font-size="12" fill="currentColor">Provider answers with a completion</text>
  <line x1="160" y1="222" x2="160" y2="240" stroke="currentColor" marker-end="url(#inf-arrow)"/>
  <rect x="10" y="240" width="300" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="160" y="264" text-anchor="middle" font-size="12" fill="currentColor">Event request-completed, then take-completion</text>
</svg>
<figcaption>The dashed step is kernel policy. The plugin does not choose the provider or bypass the gate.</figcaption>

</figure>

The kernel answers a call in one of three ways. A refusal by policy returns `not-permitted` and writes no privacy line. A throttle returns `rate-limited` with `retry-after-ms`. An admitted request adds one line to the privacy log with the plugin id, the `purpose`, the provider id, the model and the token counts. Settings > Privacy lists that log.

A completed result stays available for 30 s or until you take it, whichever comes first. A take is one-shot: a second `take-completion` for the same id returns `none`.

### Worked example: a note title in Rust

This example starts a request from a handler without blocking it. The title arrives in the log.

**Rust**

```rust
use ilium_plugin_sdk::host::inference::{self, ChatMessage, CompletionRequest};
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::prelude::*;

/// Asks the user's chosen provider for a five-word title for a note.
async fn suggest_note_title(note_text: String) -> Result<String, PluginError> {
    let request = CompletionRequest {
        messages: vec![
            ChatMessage {
                role: "system".into(),
                content: "Reply with a title of five words.".into(),
            },
            ChatMessage { role: "user".into(), content: note_text },
        ],
        max_output_tokens: Some(32),
        json_schema: None,
        purpose: "note-title".into(),
    };
    let completion = inference::complete(request).await?;
    Ok(completion.text)
}

/// Starts the request and returns at once, so the event handler is not held up.
fn on_note_saved(note_text: String) {
    task::spawn(async move {
        match suggest_note_title(note_text).await {
            Ok(title) => log::log(Level::Info, &format!("note title: {title}")),
            Err(error) => log::log(Level::Warn, &format!("no title: {:?}", error.code)),
        }
    });
}
```

The `?` operator returns the error to the caller. `Err` holds a `PluginError` with a `code`, a `message` and an optional `retry_after_ms`.

### Worked example: tags with a JSON schema in TypeScript

This example asks for structured output and handles the codes that matter for it.

**TypeScript**

```ts
import { complete } from '@ilium/plugin-sdk/host/inference';
import { log } from '@ilium/plugin-sdk/host/log';
import type { PluginError } from '@ilium/plugin-sdk';

const TAG_SCHEMA = JSON.stringify({
  type: 'object',
  properties: { tags: { type: 'array', items: { type: 'string' } } },
  required: ['tags'],
});

/** Returns up to three tags for a note. An empty list means no tags, not a failure. */
export async function suggestTags(noteText: string): Promise<string[]> {
  try {
    const completion = await complete({
      messages: [
        { role: 'system', content: 'Return up to three lowercase tags for the note.' },
        { role: 'user', content: noteText },
      ],
      maxOutputTokens: 64,
      jsonSchema: TAG_SCHEMA,
      purpose: 'tree-organisation',
    });
    const parsed = JSON.parse(completion.text) as { tags?: string[] };
    return parsed.tags ?? [];
  } catch (error) {
    const failure = error as PluginError;
    if (failure.code === 'not-found') {
      log('info', 'no AI provider selected; skipping tags');
      return [];
    }
    if (failure.code === 'rate-limited') {
      log('warn', `tags throttled, retry after ${failure.retryAfterMs ?? 0} ms`);
      return [];
    }
    throw error;
  }
}
```

The function returns an empty list for the two codes that mean "no tags right now". Every other code is a bug or an outage, so it is thrown to the caller.

Use `models()` to show the user which models the provider offers, for example in a picker. The list is empty when the provider offers no models. That is not an error.

### Errors and edge cases

| Code | When it happens | Retry |
|---|---|---|
| `not-permitted` | the grant is missing or declined; the gate refuses the request by policy, for example a purpose or provider the user blocked | no |
| `not-found` | no provider is selected; the call does not wait for one | after the user selects a provider |
| `invalid-argument` | no messages, a bad role, an empty or malformed `purpose`, `max-output-tokens: 0`, input over 4 MiB, or a schema over 64 KiB or invalid | no, fix the call |
| `rate-limited` | the gate throttles the request; `retry-after-ms` is set | yes, after the delay |
| `timed-out` | the provider does not answer within 120 s | yes |
| `network` | the provider cannot be reached | yes |
| `busy` | the pending request list of the plugin is full | yes, later |
| `quota-exceeded` | the CPU budget of the execution quota group is spent; `retry-after-ms` is set | yes, after the delay |
| `revoked` | the user revoked the grant during the call; the pending request completes as failed | no; the plugin is re-activated |

The provider can also return its own error code, which the kernel passes on unchanged.

If your feature must wait for a provider, declare the service requirement in the manifest. The plugin then activates only when a compatible provider runs:

```json
{
  "requires": { "services": ["ilium:inference/provider@0.1.0"] }
}
```

### What the user sees

The install dialog shows one row for the grant, in the server or client group. The row wording is fixed by the platform:

<figure>

<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.inference 0.1.0 by Example Ltd                                 │
│                                                                            │
│ <span class="t-hi">Server side</span>                                                                │
│ [x] Asks the selected language model <span class="t-dim">………………………………………………………………………</span> inference │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>

<figcaption>The review dialog for a package that requests inference, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar. Checked rows are granted; a high-risk row starts unchecked.</figcaption>

</figure>

- A T1 bundled plugin receives the grant automatically, up to its manifest ceiling.
- The grant is normal risk, not high risk. A T2 or T3 package lists it checked by default. The user can uncheck it, and the plugin then sees the grant missing in its `granted` list.
- A grant declined at install is shown dimmed and unchecked. Your feature must keep working without it.
- The user can revoke the grant later in Settings > Plugins > Capabilities. Calls in flight return `revoked`.

### Risk and privacy rules

- Requests send the user's project content to the chosen provider. Send only the text the feature needs, and never a whole project when a note is enough.
- Every admitted request is recorded in the privacy log. Use a `purpose` that the user can recognise in that log.
- The grant does not let the plugin pick a provider or bypass the gate.

#### Log retention

Two logs record an inference call. Neither holds prompt or reply text ([COR-950](/pages/core-plugin-inference/#what-is-kept-and-for-how-long)).

| Log | Location (Linux) | Kept | Deleted by |
|---|---|---|---|
| Privacy log, `inference.jsonl` | `~/.local/state/ilium/privacy/inference.jsonl` | 90 days or 16 MiB, whichever is smaller. When the cap is reached, the oldest records go first. | Settings > Privacy > Clear |
| Plugin log, one file per plugin | `<state>/plugins/logs/<session-slug>/<id>.log` (`<state>` is `~/.local/state/ilium` on Linux) | The active file and four rotated files. The file rotates at 4 MiB, and rotated files older than 14 days are removed at server start. After an uninstall, the log is removed 7 days later. | `ilium plugin logs --clear <id>`; `--purge` or `ilium plugin erase-data` |

- The privacy log keeps the fields listed in [SEC-265](/pages/user-plugin-security-and-privacy/#the-privacy-log): `purpose`, `provider`, `model` and token counts. Ilium does not redact these fields early. A record leaves the log only by age, by the size cap or by Clear.
- The plugin log keeps the lines your plugin writes with `log`, plus host lines for refusals and traps. Its limit is 200 lines per second. Extra lines are dropped and counted in one `warn` summary line. Do not write prompt, reply or note text into it.
- The plugin cannot read its own log. Only the user who owns the folder can read it ([reference: plugin files](/pages/reference-plugin-files/#the-plugin-log)).
- A user who wants the record gone can clear the privacy log at once. The plugin log holds whatever the plugin wrote, so review it before sending it to support.
- When a bug report needs evidence, ask for `ilium plugin logs <id>` and `ilium privacy log --plugin <id>`. The privacy log shows which calls were admitted, but not what they said.

### Who requests it

First-party plugins from [feature traceability](/pages/reference-feature-traceability/):

- 41 Automatic pane titles.
- 42 Session naming.
- 43 Tree organisation, which also uses `json-schema`.
- 46 Inferred-title icons.
- 66 Smart Copy AI, which also uses `json-schema`.
- 77 Convert a session between Claude and Codex.
- 80 Remote compaction.

### See also

- [inference.provide](/pages/capability-inference-provide/) for plugins that supply a provider.
- [http](/pages/capability-http/) for requests to hosts that are not kernel-routed.
- [Services](/pages/services/) for the service requirement that makes a plugin wait for a provider.
- [api-inference](/pages/api-inference/) for the full import and function reference.

Checked against `ilium:plugin@0.1.0` (API draft of 2026-10-11).

### Security

Every request is recorded in the privacy log; see [User security and privacy](/pages/user-plugin-security-and-privacy/) ([SEC-265]).
