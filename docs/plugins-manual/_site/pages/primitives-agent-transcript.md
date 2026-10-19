# Agent transcript and signals

## Agent transcript and signals

The base agent primitive tells a plugin which agent runs in a pane and what it is doing. The primitive also gives the facts a cost meter, a context gauge or a handoff tool needs: the model in use, the prompt epoch, the typed transcript of the conversation, and two events, `signal-changed` and `agent-model-changed`. The host parses the agent's session file. The plugin never receives a file path to read.

This page extends [Agent primitive](/pages/primitives-agent/).

### You will need

| Grant | Risk | Unlocks |
|---|---|---|
| `agent.read` | normal | `model`, `prompt-epoch`, `signal-changed`, `agent-model-changed` |
| `agent.transcript` | high | `transcript`, `take-transcript` |
| `agent.read-prompts` | high | `last-prompt`, `user-line-submitted` |

See [agent.read](/pages/capability-agent-read/), [agent.read-prompts](/pages/capability-agent-read-prompts/) and [Capabilities](/pages/reference-capabilities/). Both sides can import the interface.

### Signatures

```wit
record agent-model { model: string, provider: option<string>, context-window: option<u32>, source: string }
model: func(pane: node-id) -> option<agent-model>;       // source: statusline, transcript or signature
prompt-epoch: func(pane: node-id) -> option<u64>;

enum turn-role { user, assistant, tool, system }
record transcript-turn { role: turn-role, text: string, tool: option<string>, at: option<epoch-ms>, tokens: option<u32> }
record transcript-query { pane: node-id, last-turns: u32, max-bytes: u32 }
transcript: func(q: transcript-query) -> result<request-id, plugin-error>;
take-transcript: func(request: request-id) -> option<list<transcript-turn>>;
```

| Rust (`primitives::agent`) | TypeScript | Returns |
|---|---|---|
| `model(pane)` | `model` | `Option<AgentModel>` |
| `prompt_epoch(pane)` | `promptEpoch` | `Option<u64>` |
| `transcript(query)` | `transcript` | `Result<RequestId, PluginError>` |
| `take_transcript(request)` | `takeTranscript` | `Option<Vec<TranscriptTurn>>` |

### Model and epoch

`model` returns the name the agent reports, the provider when known, the context window in tokens when known, and `source`, which says where the host found it: the agent's status line, its transcript, or the signature's default. A model change fires `agent-model-changed { pane, model }`, coalesced per pane. The prompt epoch is the counter that `send-input` compares against (see [Terminal input](/pages/primitives-terminal-input/)); the call replaces a read-and-guess pattern. A successful `send-input` on an agent pane returns the new epoch in its outcome.

### Signals

A signature can declare named signals that the host extracts from the agent's status line or screen: context percentage, session cost, the rate-limit reset time. `signal-changed { pane, signal, value }` carries the signal name and its value as a string. The event is coalesced by pane and signal, so a rapidly changing percentage arrives as its latest value. Signal names come from the signature, so a plugin lists the names it cares about in its own configuration.

### The transcript

`transcript` is a request. The host reads the session file in the background, parses it into turns, and answers through `request-completed`; then `take-transcript` returns the turns, oldest first. `last-turns` is at most 512 and `max-bytes` at most 4 MiB. When the result is larger, the oldest turns are dropped. A turn has a role, its text, the tool name for a tool turn, the time when the file records one, and the token count when the file records one. A signature without a known file format returns `unsupported`; a pane with no session yet returns `not-found`.

<figure>
<svg viewBox="0 0 420 140" role="img" aria-label="Host parses session file into typed turns">
  <rect x="10" y="40" width="110" height="50" rx="6" fill="none" stroke="currentColor"/>
  <text x="65" y="62" text-anchor="middle" fill="currentColor" font-size="11">session file</text>
  <text x="65" y="78" text-anchor="middle" fill="currentColor" font-size="10">path stays in kernel</text>
  <rect x="150" y="40" width="110" height="50" rx="6" fill="none" stroke="currentColor"/>
  <text x="205" y="70" text-anchor="middle" fill="currentColor" font-size="11">host parser</text>
  <rect x="290" y="40" width="120" height="50" rx="6" fill="none" stroke="currentColor"/>
  <text x="350" y="70" text-anchor="middle" fill="currentColor" font-size="11">list of turns</text>
  <path d="M120 65 H150 M260 65 H290" stroke="currentColor"/>
  <text x="210" y="120" text-anchor="middle" fill="currentColor" font-size="11">request-completed, then take-transcript</text>
</svg>
<figcaption>The plugin receives typed turns, never the file or its path.</figcaption>
</figure>

### Rust and TypeScript

This example keeps the latest model per pane and, when an agent goes idle, requests the last 20 turns.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::primitives::agent::{self, TranscriptQuery};

for event in batch {
    match event {
        Event::AgentModelChanged(changed) => {
            self.models.insert(changed.pane, changed.model.model);
        }
        Event::SignalChanged(signal) if signal.signal == "context-percent" => {
            self.context.insert(signal.pane, signal.value);
        }
        Event::AgentActivityChanged(change) if change.activity == AgentActivity::Idle => {
            match agent::transcript(TranscriptQuery {
                pane: change.pane, last_turns: 20, max_bytes: 256 * 1024 }) {
                Ok(request) => { self.pending.insert(request, change.pane); }
                Err(error) => log::log(Level::Warn, &error.message),
            }
        }
        _ => {}
    }
}
```

**TypeScript**

```ts
for (const event of batch) {
  if (event.tag === 'agent-model-changed') {
    models.set(event.val.pane, event.val.model.model);
  } else if (event.tag === 'signal-changed' && event.val.signal === 'context-percent') {
    context.set(event.val.pane, event.val.value);
  } else if (event.tag === 'agent-activity-changed' && event.val.current === 'idle') {
    try {
      const request = transcript({ pane: event.val.node, lastTurns: 20, maxBytes: 256 * 1024 });
      pending.set(request, event.val.node);
    } catch (error) {
      log('warn', String(error));
    }
  }
}
```

Reading the result when the request completes:

**Rust**

```rust
if let Some(turns) = agent::take_transcript(request) {
    let tokens: u32 = turns.iter().filter_map(|turn| turn.tokens).sum();
}
```

**TypeScript**

```ts
const turns = takeTranscript(request) ?? [];
const tokens = turns.reduce((sum, turn) => sum + (turn.tokens ?? 0), 0);
```

### Outcomes, errors and limits

| Call | Error | Cause |
|---|---|---|
| `transcript` | `unsupported` | the signature has no known session file format |
| `transcript` | `not-found` | no session yet for the pane |
| `transcript` | `invalid-argument` | `last-turns` over 512 or `max-bytes` over 4 MiB |
| completion | `io`, `quota-exceeded` | the file could not be read, or the result was too large to hold |
| any | `not-permitted` | the grant is missing |

### What the user sees

Nothing in the pane changes. A plugin typically shows the facts in the tree row or the status bar. The tree row of an agent pane, drawn by the host from the activity, shows the working spinner:

<pre class="term">▼ 📂 Ilium
›   🦀 <span class="t-hi">⠋</span> Fix auth
›   🐢 ● Refactor parser
</pre>

### See also

- [Agent primitive](/pages/primitives-agent/)
- [Terminal input](/pages/primitives-terminal-input/)
- [Events](/pages/api-events/)
