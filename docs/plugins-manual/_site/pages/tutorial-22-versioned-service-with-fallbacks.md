# 22. A versioned service with fallbacks

## 22. A versioned service with fallbacks

This step builds two plugins that talk through a plugin-defined service. The provider is `example.glossary`. It offers `example.glossary/lookup`, first at version 1.0.0 and then at 1.1.0. The consumer is `example.glossary-hover`. It asks for a definition and keeps working when the provider is missing, too old, slow or wrong.

The service version here is plugin-defined semver. It is the version of the glossary service, not the contract version. The contract is `ilium:plugin@0.1.0` and does not change in this step. The page [Services](/pages/api-services/) lists the call API and the error codes. This page does not repeat that table. It shows how to design for each failure and how to test each one.

You have finished tutorials 1 to 9 and read [Services](/pages/api-services/). Rust and TypeScript behave the same in every step.

### You will need

- Rust: stable 1.90 or later with the `wasm32-wasip2` target, and the crates `ilium-plugin-sdk` and `serde_json`. TypeScript: Bun 1.2 or later and `@ilium/plugin-sdk`.
- Two plugin folders: `example-glossary/` and `example-glossary-hover/`.
- `"unstable": ["plugin-services"]` in both manifests (dev mode, T2 and T3).
- Grants: the provider needs `services.provide:example.glossary/lookup`. The consumer needs `services.use:example.glossary/lookup`.
- A scratch folder for the packages: `/media/arthur/tmp/glossary/` with a `.scratch-reaper` file in it.

### The surface this step uses

| Surface | Used for |
|---|---|
| `contributes.services.provides` | The provider names the service, its version and a schema file. |
| `requires.services` | The consumer names the service and the lowest version it needs. |
| `ServiceProvider::handle` and `ServiceReply` | The provider answers at once with `Done`, or later with `Deferred`. |
| `services::call_request`, `take_reply` | The consumer calls without blocking, then reads the result. |
| `services::reply` | The provider sends a deferred answer. |
| Grants `services.provide` and `services.use` | The bare service name, never a version. |
| Errors `not-found`, `unsupported`, `timed-out`, `busy`, `invalid-argument` | The failures the consumer must handle. |

### The flow

The consumer calls. The provider answers at once, or later.

<figure>
<svg viewBox="0 0 480 300" role="img" aria-label="Sequence of a deferred service call">
  <title>A deferred glossary lookup</title>
  <desc>The user runs a command. The consumer calls lookup. The provider returns deferred. Later the provider replies. The consumer shows the definition, or a fallback on any failure.</desc>
  <defs>
    <marker id="t22-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
      <path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="10" y="8" width="70" height="26" rx="4" fill="none" stroke="currentColor"/>
  <text x="45" y="25" font-size="11" text-anchor="middle" fill="currentColor">User</text>
  <rect x="130" y="8" width="100" height="26" rx="4" fill="none" stroke="currentColor"/>
  <text x="180" y="25" font-size="11" text-anchor="middle" fill="currentColor">Glossary hover</text>
  <rect x="280" y="8" width="90" height="26" rx="4" fill="none" stroke="currentColor"/>
  <text x="325" y="25" font-size="11" text-anchor="middle" fill="currentColor">Host</text>
  <rect x="400" y="8" width="70" height="26" rx="4" fill="none" stroke="currentColor"/>
  <text x="435" y="25" font-size="11" text-anchor="middle" fill="currentColor">Glossary</text>
  <line x1="45" y1="34" x2="45" y2="290" stroke="currentColor" stroke-dasharray="3 4"/>
  <line x1="180" y1="34" x2="180" y2="290" stroke="currentColor" stroke-dasharray="3 4"/>
  <line x1="325" y1="34" x2="325" y2="290" stroke="currentColor" stroke-dasharray="3 4"/>
  <line x1="435" y1="34" x2="435" y2="290" stroke="currentColor" stroke-dasharray="3 4"/>
  <line x1="45" y1="58" x2="180" y2="58" stroke="currentColor" marker-end="url(#t22-arrow)"/>
  <text x="112" y="52" font-size="10" text-anchor="middle" fill="currentColor">define "latency"</text>
  <line x1="180" y1="92" x2="325" y2="92" stroke="currentColor" marker-end="url(#t22-arrow)"/>
  <text x="252" y="86" font-size="10" text-anchor="middle" fill="currentColor">call-request</text>
  <line x1="325" y1="126" x2="435" y2="126" stroke="currentColor" marker-end="url(#t22-arrow)"/>
  <text x="380" y="120" font-size="10" text-anchor="middle" fill="currentColor">handle</text>
  <line x1="435" y1="158" x2="325" y2="158" stroke="currentColor" stroke-dasharray="5 3" marker-end="url(#t22-arrow)"/>
  <text x="380" y="152" font-size="10" text-anchor="middle" fill="currentColor">deferred</text>
  <line x1="435" y1="196" x2="325" y2="196" stroke="currentColor" marker-end="url(#t22-arrow)"/>
  <text x="380" y="190" font-size="10" text-anchor="middle" fill="currentColor">reply</text>
  <line x1="325" y1="228" x2="180" y2="228" stroke="currentColor" marker-end="url(#t22-arrow)"/>
  <text x="252" y="222" font-size="10" text-anchor="middle" fill="currentColor">request-completed</text>
  <line x1="180" y1="268" x2="45" y2="268" stroke="currentColor" marker-end="url(#t22-arrow)"/>
  <text x="112" y="262" font-size="10" text-anchor="middle" fill="currentColor">definition or fallback</text>
</svg>
<figcaption>The dashed line is the provider saying "not yet". The consumer is never blocked while it waits.</figcaption>
</figure>

### Step 1: the provider at 1.0.0

Version 1.0.0 takes a term and returns a definition.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "example.glossary",
  "name": "Glossary",
  "version": "1.0.0",
  "publisher": "Example Developer",
  "description": "Looks up a term in a small built-in glossary.",
  "icon": "book",
  "unstable": ["plugin-services"],
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0",
    "exports": ["service-provider"]
  },
  "capabilities": {
    "server": ["services.provide:example.glossary/lookup"]
  },
  "activation": ["startup"],
  "contributes": {
    "services": {
      "provides": [
        { "name": "example.glossary/lookup", "version": "1.0.0", "schema": "schemas/lookup.json" }
      ]
    }
  }
}
```

`schemas/lookup.json` describes one method, `lookup`. The schema uses only the allowed keywords: `type`, `properties`, `required`, `enum`, `title`, `description`, `default`, `minimum`, `maximum`, `pattern`, `maxLength` and `items`.

```json
{
  "methods": {
    "lookup": {
      "params": {
        "type": "object",
        "required": ["term"],
        "properties": { "term": { "type": "string", "maxLength": 80 } }
      },
      "result": {
        "type": "object",
        "required": ["found", "definition"],
        "properties": {
          "found": { "type": "boolean" },
          "definition": { "type": "string", "maxLength": 2000 }
        }
      }
    }
  }
}
```

**Rust**

```rust
// example-glossary/src/lib.rs
use ilium_plugin_sdk::prelude::*;
use serde_json::{json, Value};

const TERMS: &[(&str, &str)] = &[
    ("latency", "The time between a request and its first answer."),
    ("throughput", "How much work is done in a span of time."),
];

#[ilium_plugin(side = "server", exports = ["service-provider"])]
#[derive(Default)]
struct Glossary;

impl Lifecycle for Glossary {}

/// Pure lookup. Case is ignored. An unknown term is a normal answer, not an error.
pub fn define(term: &str) -> Value {
    let wanted = term.trim().to_lowercase();
    match TERMS.iter().find(|(name, _)| *name == wanted) {
        Some((_, text)) => json!({ "found": true, "definition": text }),
        None => json!({ "found": false, "definition": "" }),
    }
}

impl ServiceProvider for Glossary {
    fn handle(&mut self, call: ServiceCall) -> ServiceReply {
        let params: Value = serde_json::from_str(&call.payload_json).unwrap_or(Value::Null);
        let Some(term) = params["term"].as_str() else {
            // The host checks params against the schema first. This is a second guard.
            return ServiceReply::Done(define("").to_string());
        };
        ServiceReply::Done(define(term).to_string())
    }
}
```

**TypeScript**

```ts
// example-glossary/src/index.ts
import { definePlugin, type ServiceCall, type ServiceReply } from '@ilium/plugin-sdk';

const TERMS: Record<string, string> = {
  latency: 'The time between a request and its first answer.',
  throughput: 'How much work is done in a span of time.',
};

/** Pure lookup. Case is ignored. An unknown term is a normal answer, not an error. */
export function define(term: string): { found: boolean; definition: string } {
  const text = TERMS[term.trim().toLowerCase()];
  return text === undefined ? { found: false, definition: '' } : { found: true, definition: text };
}

export const { lifecycle, serviceProvider } = definePlugin({
  side: 'server',
  handle(call: ServiceCall): ServiceReply {
    const params = JSON.parse(call.payloadJson) as { term?: string };
    return { tag: 'done', val: JSON.stringify(define(params.term ?? '')) };
  },
});
```

Points to notice:

- The grant names the service without a version. The version lives in `provides`.
- The host validates params against the schema before `handle` runs. A wrong shape never reaches your code. The caller gets `invalid-argument`.

### Step 2: evolve to 1.1.0 without breaking anyone

Version 1.1.0 adds a language and a list of aliases. The change is additive. That is the rule that keeps a consumer of 1.0 working: add optional input, add output fields, never remove or rename either.

| Change | Allowed in a minor version? | Why |
|---|---|---|
| New optional param `lang` with a default | yes | An old caller does not send it, and the default applies. |
| New result field `aliases` | yes | An old caller ignores fields it does not know. |
| Remove or rename a field | no | An old caller reads a field that is gone. Make it a major version. |

Change the manifest version fields to 1.1.0 and add the optional pieces to the schema. The old fields stay as they were.

```json
"properties": {
  "term": { "type": "string", "maxLength": 80 },
  "lang": { "type": "string", "enum": ["en", "de"], "default": "en" }
}
```

Add `"aliases": { "type": "array", "items": { "type": "string", "maxLength": 80 } }` to the result and leave `required` as it was. A consumer that asked for 1.0 still gets a compatible answer, because the host matches on major version and a minor at least as high.

Now a slow case. Some lookups need work that cannot finish in one call, such as a model call. The provider returns `Deferred` and answers later with `reply`. The pending map is keyed by the call number that the host gave the provider.

**Rust**

```rust
// example-glossary/src/lib.rs, version 1.1.0
use std::collections::HashMap;

use ilium_plugin_sdk::host::services;

/// Extra definitions for German, as plain data.
const GERMAN: &[(&str, &str)] = &[("latency", "Die Zeit zwischen einer Anfrage und der ersten Antwort.")];

pub fn define_in(term: &str, lang: &str) -> Value {
    if lang == "de" {
        let wanted = term.trim().to_lowercase();
        if let Some((_, text)) = GERMAN.iter().find(|(name, _)| *name == wanted) {
            return json!({ "found": true, "definition": text, "aliases": [] });
        }
    }
    let mut base = define(term);
    base["aliases"] = json!([]);
    base
}

#[ilium_plugin(side = "server", exports = ["service-provider"])]
#[derive(Default)]
struct GlossaryV11 {
    /// Calls waiting for a slow source, keyed by the host's call number.
    waiting: HashMap<u64, String>,
}

impl ServiceProvider for GlossaryV11 {
    fn handle(&mut self, call: ServiceCall) -> ServiceReply {
        let params: Value = serde_json::from_str(&call.payload_json).unwrap_or(Value::Null);
        let term = params["term"].as_str().unwrap_or("").to_string();
        let lang = params["lang"].as_str().unwrap_or("en").to_string();
        if params["slow"].as_bool() == Some(true) {
            // Answer later. The caller keeps waiting, for at most 30 s.
            self.waiting.insert(call.call, term);
            return ServiceReply::Deferred;
        }
        ServiceReply::Done(define_in(&term, &lang).to_string())
    }
}

impl GlossaryV11 {
    /// Called from the plugin's event handler when the slow source reports that it is done
    /// (in the demo, the `timer-fired` event of a timer started when `waiting` was filled).
    /// A late call gets `not-found`, which is ignored.
    fn finish(&mut self, call: u64) {
        if let Some(term) = self.waiting.remove(&call) {
            let _ = services::reply(call, Ok(define_in(&term, "en").to_string()));
        }
    }
}
```

**TypeScript**

```ts
// example-glossary/src/index.ts, version 1.1.0
import { reply } from '@ilium/plugin-sdk/host/services';

const GERMAN: Record<string, string> = {
  latency: 'Die Zeit zwischen einer Anfrage und der ersten Antwort.',
};

export function defineIn(term: string, lang: string) {
  const key = term.trim().toLowerCase();
  if (lang === 'de' && GERMAN[key] !== undefined) {
    return { found: true, definition: GERMAN[key], aliases: [] as string[] };
  }
  return { ...define(term), aliases: [] as string[] };
}

/** Calls waiting for a slow source, keyed by the host's call number. */
const waiting = new Map<bigint, string>();

// Wired as `handle` in definePlugin, as in version 1.0.0: definePlugin({ side: 'server', handle: handleCall }).
export function handleCall(call: ServiceCall): ServiceReply {
  const params = JSON.parse(call.payloadJson) as { term?: string; lang?: string; slow?: boolean };
  if (params.slow === true) {
    waiting.set(call.call, params.term ?? '');
    return { tag: 'deferred' };
  }
  return { tag: 'done', val: JSON.stringify(defineIn(params.term ?? '', params.lang ?? 'en')) };
}

/**
 * Called from the plugin's event handler when the slow source reports that it is done
 * (in the demo, the `timer-fired` event of a timer started when `waiting` was filled).
 * A late call gets `not-found`, which is ignored.
 */
export function finish(call: bigint): void {
  const term = waiting.get(call);
  if (term === undefined) return;
  waiting.delete(call);
  try {
    reply(call, { tag: 'ok', val: JSON.stringify(defineIn(term, 'en')) });
  } catch {
    // The caller has gone away. Nothing to do.
  }
}
```

Nothing calls `finish` by itself: the plugin's event handler must call it with the call number stored in the pending map when the slow source completes. If it never does, the caller gets `timed-out` after 30 s, so the handler for the timeout must also remove the entry from the map.

Points to notice:

- A deferred call has an owner: the pending map. If the provider restarts, the host cancels every deferred call. The caller sees `cancelled`. Do not try to save the map.

### Step 3: the consumer

The consumer asks for the lowest version it can live with. It does not require 1.1.0, because it works without `lang` and `aliases`.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "example.glossary-hover",
  "name": "Glossary hover",
  "version": "0.1.0",
  "publisher": "Example Developer",
  "description": "Defines a term. Falls back to plain text when the glossary is not usable.",
  "icon": "book",
  "unstable": ["plugin-services"],
  "requires": { "services": ["example.glossary/lookup@1.0.0"] },
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0",
    "exports": []
  },
  "capabilities": {
    "server": ["services.use:example.glossary/lookup"]
  },
  "activation": ["startup"],
  "contributes": {
    "commands": [
      { "id": "example.glossary-hover/define", "title": "Define a term", "side": "server" }
    ],
    "topics": { "publishes": ["example.glossary-hover/definition"] }
  }
}
```

`requires.services` makes the host wait at activation. It waits for a compatible provider, 2 s by default and 5 s at most. If none shows up, the consumer still activates and its first call fails with `not-found`. Install never fails because a provider is missing.

The consumer must handle eight outcomes. Write them as one pure function, so each can be tested alone.

| Outcome | Cause | What the consumer shows |
|---|---|---|
| Definition | Normal case | The definition |
| `found: false` | The term is not in the glossary | `No entry for "<term>".` |
| `not-found` | No provider, or it is off, or the call was late | `Glossary is not available.` |
| `unsupported` | The provider is older than the method needs | `Glossary is too old.` |
| `timed-out` | The provider took over 30 s | `Glossary did not answer.` |
| `busy` | The caller already has 64 pending calls | `Glossary is busy.` |
| `cancelled` | The provider restarted with the call open | `Glossary is not available.` (the fallback) |
| `invalid-argument` or garbage | A broken schema or a broken result | `Glossary gave an unusable answer.` |

Garbage matters most. A provider is a different author's code. Read the result like any untrusted input. Check the type of every field and cap the length before you show it.

**Rust**

```rust
// example-glossary-hover/src/lib.rs
use std::collections::HashMap;

use ilium_plugin_sdk::host::services;
use ilium_plugin_sdk::host::topics;
use ilium_plugin_sdk::prelude::*;
use serde_json::{json, Value};

const SERVICE: &str = "example.glossary/lookup";
const MAX_SHOWN: usize = 400;

/// What the user sees for one lookup. A failure never panics and never goes blank.
pub fn describe(term: &str, outcome: Result<String, String>) -> String {
    let text = match outcome {
        Ok(text) => text,
        Err(code) => {
            return match code.as_str() {
                "not-found" => "Glossary is not available.".to_string(),
                "unsupported" => "Glossary is too old.".to_string(),
                "timed-out" => "Glossary did not answer.".to_string(),
                "busy" => "Glossary is busy.".to_string(),
                "cancelled" => "Glossary is not available.".to_string(),
                _ => "Glossary gave an unusable answer.".to_string(),
            };
        }
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return "Glossary gave an unusable answer.".to_string();
    };
    match (value["found"].as_bool(), value["definition"].as_str()) {
        (Some(true), Some(definition)) if !definition.is_empty() => {
            definition.chars().take(MAX_SHOWN).collect()
        }
        (Some(false), _) => format!("No entry for \"{term}\"."),
        _ => "Glossary gave an unusable answer.".to_string(),
    }
}

#[ilium_plugin(side = "server", exports = ["handler"])]
#[derive(Default)]
struct GlossaryHover {
    asking: HashMap<RequestId, String>,
}

impl Lifecycle for GlossaryHover {}

impl Handler for GlossaryHover {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            match event {
                Event::CommandInvoked(invoked) => {
                    let args: Value = serde_json::from_str(&invoked.arguments_json).unwrap_or(Value::Null);
                    let term = args["term"].as_str().unwrap_or("").to_string();
                    let payload = json!({ "term": term }).to_string();
                    match services::call_request(SERVICE, "lookup", &payload) {
                        Ok(id) => {
                            self.asking.insert(id, term);
                        }
                        // The call could not even start. Show the fallback now.
                        Err(error) => show(&term, Err(error.code)),
                    }
                }
                Event::RequestCompleted(done) => {
                    let Some(term) = self.asking.remove(&done.request) else { continue };
                    let outcome = services::take_reply(done.request).map_err(|error| error.code);
                    show(&term, outcome);
                }
                _ => {}
            }
        }
        Vec::new()
    }
}

fn show(term: &str, outcome: Result<String, String>) {
    let text = describe(term, outcome);
    let payload = json!({ "term": term, "text": text }).to_string();
    let _ = topics::publish("example.glossary-hover/definition", &payload);
}
```

**TypeScript**

```ts
// example-glossary-hover/src/index.ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { callRequest, takeReply } from '@ilium/plugin-sdk/host/services';
import { publish } from '@ilium/plugin-sdk/host/topics';

const SERVICE = 'example.glossary/lookup';
const MAX_SHOWN = 400;

type Outcome = { ok: string } | { error: string };

/** What the user sees for one lookup. A failure never throws and never goes blank. */
export function describe(term: string, outcome: Outcome): string {
  if ('error' in outcome) {
    switch (outcome.error) {
      case 'not-found': return 'Glossary is not available.';
      case 'unsupported': return 'Glossary is too old.';
      case 'timed-out': return 'Glossary did not answer.';
      case 'busy': return 'Glossary is busy.';
      case 'cancelled': return 'Glossary is not available.';
      default: return 'Glossary gave an unusable answer.';
    }
  }
  let value: { found?: unknown; definition?: unknown };
  try {
    value = JSON.parse(outcome.ok);
  } catch {
    return 'Glossary gave an unusable answer.';
  }
  if (value.found === true && typeof value.definition === 'string' && value.definition !== '') {
    return value.definition.slice(0, MAX_SHOWN);
  }
  if (value.found === false) return `No entry for "${term}".`;
  return 'Glossary gave an unusable answer.';
}

const asking = new Map<bigint, string>();

function show(term: string, outcome: Outcome): void {
  publish('example.glossary-hover/definition', JSON.stringify({ term, text: describe(term, outcome) }));
}

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'command-invoked') {
        const args = JSON.parse(event.val.argumentsJson) as { term?: string };
        const term = args.term ?? '';
        try {
          asking.set(callRequest(SERVICE, 'lookup', JSON.stringify({ term })), term);
        } catch (error) {
          show(term, { error: (error as { code?: string }).code ?? 'failed' });
        }
      } else if (event.tag === 'request-completed') {
        const term = asking.get(event.val.request);
        if (term === undefined) continue;
        asking.delete(event.val.request);
        try {
          show(term, { ok: takeReply(event.val.request) });
        } catch (error) {
          show(term, { error: (error as { code?: string }).code ?? 'failed' });
        }
      }
    }
    return [];
  },
});
```

Points to notice:

- `describe` is the whole degradation policy, in one pure function. Every row of the table above has a test.
- The call is non-blocking. A slow provider does not freeze the consumer. The 30 s timeout becomes `timed-out`, which the table covers.
- `found: false` is shown differently from an error. The user learns that the glossary works and the term is unknown.

### Step 4: one provider per name, and the user's choice

Two plugins may provide `example.glossary/lookup`. The host does not merge them. The first enabled provider owns the name. The second stays installed and idle.

- The Plugins screen shows both. The one that owns the name says `active`. The other says `waiting: example.glossary owns lookup`.
- The user can pick the owner on the Surface view of the service. The switch turns the other provider off. Consumers see no change except a brief `not-found` while the new provider starts.

The requirement `example.glossary/lookup@1.0.0` is satisfied by a provider at 1.0.0 or at 1.1.0. It is not satisfied by 2.0.0, because the major differs. 
### Step 5: security and limits

| Item | Rule |
|---|---|
| Who may call | Only a plugin that holds `services.use:<name>`. The grant is standard risk. |
| Who may answer | Only a plugin that holds `services.provide:<name>`. |
| What the caller sends | Text. Do not send secrets to a service you do not own. A provider is another author's code. |
| What the provider returns | Untrusted. Check types and cap lengths before you use or show them. |
| Pending calls | At most 64 per caller. More give `busy`. |
| Time | 30 s from the call. Then `timed-out`. |

### Step 6: test each failure

`FakeSession` runs two plugins against one registry, so install the provider and the caller in one session and call across them. Test the pure policy directly as well. This follows [Testing](/pages/testing/) and [Test harness reference](/pages/reference-test-harness/). Never test by sleeping 30 s.

**Rust**

```rust
// example-glossary-hover/src/lib.rs
#[cfg(test)]
mod tests {
    use super::*;

    fn ok(text: &str) -> Result<String, String> { Ok(text.to_string()) }
    fn err(code: &str) -> Result<String, String> { Err(code.to_string()) }

    #[test]
    fn normal_definition() {
        assert_eq!(describe("latency", ok(r#"{"found":true,"definition":"A wait."}"#)), "A wait.");
    }

    #[test]
    fn unknown_term() {
        assert_eq!(describe("zzz", ok(r#"{"found":false,"definition":""}"#)), "No entry for \"zzz\".");
    }

    #[test]
    fn provider_absent() {
        assert_eq!(describe("x", err("not-found")), "Glossary is not available.");
    }

    #[test]
    fn provider_too_old() {
        assert_eq!(describe("x", err("unsupported")), "Glossary is too old.");
    }

    #[test]
    fn provider_slow() {
        assert_eq!(describe("x", err("timed-out")), "Glossary did not answer.");
        assert_eq!(describe("x", err("busy")), "Glossary is busy.");
        assert_eq!(describe("x", err("cancelled")), "Glossary is not available.");
    }

    #[test]
    fn provider_returns_garbage() {
        assert_eq!(describe("x", ok("not json")), "Glossary gave an unusable answer.");
        assert_eq!(describe("x", ok(r#"{"found":"yes"}"#)), "Glossary gave an unusable answer.");
        assert_eq!(describe("x", err("invalid-argument")), "Glossary gave an unusable answer.");
    }

    #[test]
    fn long_definition_is_capped() {
        let long = format!(r#"{{"found":true,"definition":"{}"}}"#, "a".repeat(5000));
        assert_eq!(describe("x", ok(&long)).chars().count(), MAX_SHOWN);
    }
}
```

**TypeScript**

```ts
// example-glossary-hover/src/index.test.ts
import { expect, test } from 'bun:test';
import { describe as describeOutcome } from './index';

const ok = (text: string) => ({ ok: text });
const err = (code: string) => ({ error: code });

test('normal definition', () => {
  expect(describeOutcome('latency', ok('{"found":true,"definition":"A wait."}'))).toBe('A wait.');
});
test('unknown term', () => {
  expect(describeOutcome('zzz', ok('{"found":false,"definition":""}'))).toBe('No entry for "zzz".');
});
test('provider absent, old and slow', () => {
  expect(describeOutcome('x', err('not-found'))).toBe('Glossary is not available.');
  expect(describeOutcome('x', err('unsupported'))).toBe('Glossary is too old.');
  expect(describeOutcome('x', err('timed-out'))).toBe('Glossary did not answer.');
  expect(describeOutcome('x', err('busy'))).toBe('Glossary is busy.');
  expect(describeOutcome('x', err('cancelled'))).toBe('Glossary is not available.');
});
test('provider returns garbage', () => {
  expect(describeOutcome('x', ok('not json'))).toBe('Glossary gave an unusable answer.');
  expect(describeOutcome('x', ok('{"found":"yes"}'))).toBe('Glossary gave an unusable answer.');
  expect(describeOutcome('x', err('invalid-argument'))).toBe('Glossary gave an unusable answer.');
});
test('long definition is capped', () => {
  const long = JSON.stringify({ found: true, definition: 'a'.repeat(5000) });
  expect(describeOutcome('x', ok(long)).length).toBe(400);
});
```

The provider has tests too: `define` and `define_in` for an unknown term, case and `de`, and `handle` with `slow` set, which must give `Deferred`.

Then check the live path, using the steps in [Debugging](/pages/debugging/).

1. Install the provider at 1.0.0 and then the consumer. Run the command with the term `latency`.
2. Turn the provider off. Run the command again. The text is `Glossary is not available.`
3. Install provider 1.1.0 over 1.0.0. Run the command again. The text is the same as step 1.

### What you see

With the provider on, the consumer publishes its topic, and the user sees the text in a message line.

<figure>
<pre class="term">
<span class="t-prompt">&gt;</span> Define a term: latency
<span class="t-ok">The time between a request and its first answer.</span>
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The message line is drawn by whichever plugin listens to the topic.</figcaption>
</figure>

With the provider off, the same command shows the fallback.

<figure>
<pre class="term">
<span class="t-prompt">&gt;</span> Define a term: latency
<span class="t-warn">Glossary is not available.</span>
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The consumer keeps working. Only this answer is missing.</figcaption>
</figure>

### Edge cases and errors: what can go wrong

| Code or symptom | Cause | Fix |
|---|---|---|
| `not-found` | No enabled provider, a late reply, or the name is wrong | Show the fallback. Check the name and the Plugins screen. |
| `not-permitted` | The consumer lacks `services.use:<name>` | Add the grant. A version in the grant is wrong. |
| `invalid-argument` | Params do not match the schema, or a `done` result does not match the result schema | Fix the payload. Fix the provider's result. |
| `unsupported` | The provider has no such method | Ask for a higher version, or fall back. |
| `busy` | The caller already has 64 pending calls | Retry later. Cache answers. |
| `timed-out` | No reply in 30 s | Show the fallback. The provider should defer less. |
| `cancelled` | The provider restarted with a deferred call open | Show the fallback. Retry once if it is safe. |
| Second provider idle | One provider per name | Pick the owner on the Surface view. |

### See also

- [Services](/pages/api-services/) for the call API, the errors and the schema keywords.
- [Extension points](/pages/api-extension-points/) for the other way to let plugins work together.
- [Tutorial 23](/pages/tutorial-23-review-swarm/) for calls to core services.
- [Testing](/pages/testing/), [Debugging](/pages/debugging/) and [Manifest reference](/pages/reference-manifest/).

API checked against `ilium:plugin@0.1.0`.
