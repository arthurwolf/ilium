---
layout: page
title: Plugin to plugin calls
permalink: /pages/api-plugin-to-plugin/
---

## Plugin to plugin calls

When one plugin calls another, the call acts with two sets of grants: the caller's grants decide whether the caller may ask for the action, and the callee's grants decide what the callee does on the caller's behalf. The host checks both, so a callee cannot be used to reach a resource that the caller could not reach itself.

Plugins work together through four paths. Each path has its own owner page, which holds the full rules. This page compares the four paths, shows how a caller handles the two outcomes that every author meets, and gives the grant rule that applies to all of them.

### Choosing a path

| Path | Use it when | Owner page | Deadline | Error codes and completions | Cancellation |
|---|---|---|---|---|---|
| Service call | Plugin B offers one named method and A needs its answer, such as a title or a lookup. | [Services](/pages/api-services/) | 30 seconds from the call, then `failed(timed-out)`. A provider that is not running has an activation deadline of 2 seconds by default at T2 and T3. | At the call: `not-permitted`, `not-found`, `invalid-argument`, `unsupported`, `busy`, `quota-exceeded`. In the completion: `failed(timed-out)`, `failed(cancelled)`, `failed(<code>)`. | The caller cannot cancel. The call ends with the reply, at 30 seconds, or as `failed(cancelled)` when the provider stops, restarts or is disabled first. |
| Command invoke | A plugin runs a command that another plugin contributes, with a JSON object as arguments. | [Commands](/pages/api-commands/) | 30 seconds, then `failed(timed-out)`. | At the call: `not-found`, `not-permitted`, `invalid-argument`, `conflict`, `busy`, `quota-exceeded`. In the completion: `ok`, `failed(code)`, `failed(timed-out)`, `cancelled`. | `cancelled` when the owning plugin is disabled or the application shuts down while the command runs. |
| Extension run-hook | The owning plugin asks every contributor of one extension point for its answer, in priority order. | [Extension points](/pages/api-extension-points/) | A chain is capped at 100 ms from its first handler call. Each handler has its own `deadline_ms`, 20 ms by default and 5 to 50 ms in range. | At the call: `schema-mismatch`, `reentrant`, `rate-limited` (above 200 calls per second per point). A handler that times out counts as `pass`. | None. A handler that has not started when the cap is reached is skipped as `pass`. |
| Topic message | A plugin sends a message to a peer plugin, or publishes a topic for any subscriber. | [Messages and topics](/pages/api-messages-and-topics/) | No reply deadline. Each submission completes with its own outcome. | At the call: `not-permitted`, `invalid-argument`, `rate-limited`. In the completion: `refused(not-permitted)`, `refused(not-found)`, `refused(busy)`, `refused(invalid-argument)`, `refused(cancelled)`. | None. A refused submission makes the other submissions of its batch complete as `refused(cancelled)`. |

The grant rule of each path is on its owner page. A service call needs `services.use:<name>` on the caller and `services.provide:<name>` on the provider. A command invoke needs `commands.invoke:<pattern>` for a pattern that matches the command id. A topic message needs `messages.peer:<plugin-id>`. A run-hook needs `extensions.provide:<point>` on the owner and `extensions.contribute:<point>` on each contributor.

<div class="note">
<p>Every path ends in a result that the caller must handle. A caller that ignores a refusal or a timeout leaves the user with no feedback, so handle both codes in the first version of the plugin.</p>
</div>

### Grant intersection rule

**[SEC-716]** When plugin A calls plugin B, the host MUST authorise the action against the grants of A for the resource and against the grants of B for what B does on its behalf, and MUST NOT let B use its grants for a request whose origin plugin lacks the same grant; a violation returns `not-permitted` with detail `confused-deputy`.

Test: plugin A, which lacks a grant that plugin B holds, calls B to use that resource. Assert `not-permitted` with detail `confused-deputy`, and assert that B's grant was not used.

### Example: handle a refusal and a timeout

The example uses a service call, because it is the path most often used between two plugins. Plugin A asks plugin B for a title. If the host refuses the call because A lacks `services.use:com.example.titles/suggest`, the example returns nothing and logs a warning. If B does not answer within 30 seconds, the completion is `timed-out`, and the example does the same. Both outcomes are handled without a crash, and the plugin keeps its own work going.

**Rust (caller, `src/lib.rs`)**

```rust
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::host::services;
use ilium_plugin_sdk::prelude::*;

const SUGGEST: &str = "com.example.titles/suggest";

/// Returns the JSON reply of plugin B, or None when B refused the call or did not answer.
async fn suggest_title_json(note: &str) -> Option<String> {
    // Escape the note with a JSON library before you format it in a real plugin.
    let payload = format!(r#"{{"text":"{note}"}}"#);
    match services::call(SUGGEST, "suggest", &payload).await {
        Ok(reply) => Some(reply),
        Err(error) if error.code == ErrorCode::NotPermitted => {
            log::log(Level::Warn, "title service refused: services.use grant is missing");
            None
        }
        Err(error) if error.code == ErrorCode::TimedOut => {
            log::log(Level::Warn, "title service did not answer within 30 seconds");
            None
        }
        Err(error) => {
            log::log(Level::Error, &format!("title failed [{:?}]: {}", error.code, error.message));
            None
        }
    }
}
```

**TypeScript (caller, `src/server.ts`)**

```ts
import { PluginError } from '@ilium/plugin-sdk';
import { call } from '@ilium/plugin-sdk/host/services';
import { log } from '@ilium/plugin-sdk/host/log';

const SUGGEST = 'com.example.titles/suggest';

/** Returns the JSON reply of plugin B, or undefined when B refused the call or did not answer. */
export async function suggestTitleJson(note: string): Promise<string | undefined> {
  try {
    return await call(SUGGEST, 'suggest', JSON.stringify({ text: note }));
  } catch (error) {
    if (error instanceof PluginError && error.code === 'not-permitted') {
      log('warn', 'title service refused: services.use grant is missing');
    } else if (error instanceof PluginError && error.code === 'timed-out') {
      log('warn', 'title service did not answer within 30 seconds');
    } else {
      log('error', `title failed: ${String(error)}`);
    }
    return undefined;
  }
}
```

The caller's manifest needs `"requires": { "services": ["com.example.titles/suggest@0.1.0"] }`. Without the grant the first branch runs, and with a provider that never answers the second branch runs. The provider's side is described in [Services](/pages/api-services/).

### Edge cases

- **The callee is disabled or not installed.** The call returns `not-found` at once. It is not a `timed-out`, so the caller sees a different code from the example above. Handle `not-found` in the same way as a refusal.
- **The callee is busy.** The call returns `busy` at once, and the caller may retry later.
- **The caller is itself on a chain.** A handler that calls another plugin with `run-hook` must follow the depth rule of [Extension points](/pages/api-extension-points/).

### What the user sees

A plugin-to-plugin call has no screen of its own. A refusal shows in the plugin log of the caller, as the example writes it. The user sees a result only through what the caller shows afterwards.

### See also

- [Services](/pages/api-services/), [Commands](/pages/api-commands/), [Extension points](/pages/api-extension-points/) and [Messages and topics](/pages/api-messages-and-topics/) for the full rules of each path.
- [Capabilities and grants](/pages/capabilities/) for how a grant is requested and decided.
- [Peer message and topic payload versions](/pages/compat-peer-message-versioning/) for the version rules of a payload.
- [Errors and limits](/pages/reference-errors-and-limits/) for the codes.

## Requirement statements

- **[API-222]** The api-plugin-to-plugin page MUST contain a table with one row for each plugin-to-plugin path (service call, command invoke, extension run-hook, topic message) giving its deadline, its error codes, whether it can be cancelled, and a link to the owner page of that path.

Test: check that every link in the table resolves, and that each row matches the deadline, error codes and cancellation rules on the owner page of its path.

- **[API-223]** The api-plugin-to-plugin page MUST give one Rust and one TypeScript example of a call from plugin A to plugin B that handles a `not-permitted` refusal and a `timed-out` completion.

Test: build both examples against the SDK, run the caller once without the `services.use` grant and once against a provider that never replies, and assert that each run takes the documented branch and logs the documented warning.
