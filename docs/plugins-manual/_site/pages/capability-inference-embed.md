# Capability: inference.embed

## inference.embed

The `inference.embed` capability lets a plugin turn text into embedding vectors and keep a small vector index inside the host. The host runs the embeddings through the selected inference provider and holds the exact cosine search, so the plugin does no maths. The index is persisted in the plugin's data scope.

**You will need:**

- An `inference.embed` grant. No argument.
- A selected inference provider that offers embeddings; otherwise the calls return `unsupported`.

<figure>
<svg viewBox="0 0 420 252" role="img" aria-label="Texts become vectors, vectors go into a host-held index and queries return the closest ids">
  <defs><marker id="arr" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto"><path d="M0,0 L8,4 L0,8 z" fill="currentColor"/></marker></defs>
  <rect x="20" y="8" width="380" height="44" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="210" y="27" text-anchor="middle" fill="currentColor" font-size="12">Plugin calls vector.embed(texts, purpose)</text>
  <text x="210" y="43" text-anchor="middle" fill="currentColor" font-size="10">at most 256 texts of 8 KiB</text>
  <line x1="210" y1="52" x2="210" y2="72" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="72" width="380" height="44" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="210" y="91" text-anchor="middle" fill="currentColor" font-size="12">Host asks the selected inference provider</text>
  <text x="210" y="107" text-anchor="middle" fill="currentColor" font-size="10">unsupported when it has no embeddings</text>
  <line x1="210" y1="116" x2="210" y2="136" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="136" width="380" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="210" y="155" text-anchor="middle" fill="currentColor" font-size="12">Plugin upserts vectors and queries top-k</text>
  <text x="210" y="171" text-anchor="middle" fill="currentColor" font-size="10">cosine, k at most 100</text>
  <line x1="210" y1="180" x2="210" y2="200" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="200" width="180" height="40" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="110" y="224" text-anchor="middle" fill="currentColor" font-size="11">vectors, then (id, score) list</text>
  <rect x="220" y="200" width="180" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="310" y="224" text-anchor="middle" fill="currentColor" font-size="11">unsupported, quota-exceeded</text>
</svg>
<figcaption>The index lives in the host, so a query does not copy the vectors into the plugin.</figcaption>
</figure>

### Grant syntax

| Grant | Valid | Reason |
|---|---|---|
| `inference.embed` | yes | the only form |
| `inference.embed:local` | no | the provider is the user's choice |
| `inference` | separate | completions and model lists |

Normal risk. The text goes to the provider the user selected, as with [inference](/pages/capability-inference/).

### What it unlocks

| Name | Kind | Meaning |
|---|---|---|
| `vector.embed` | function | `embed(texts, purpose) -> result<request-id, plugin-error>`; pick up with `take-vectors`. |
| `vector.create-index` | function | `create-index(name, dimensions) -> result<index-id, plugin-error>`. |
| `vector.upsert`, `remove-entries` | functions | Add or replace `(id, vector)` pairs; remove ids. |
| `vector.top-k` | function | `top-k(index, query, k) -> result<list<tuple<string, f32>>, plugin-error>`; `k` at most 100. |
| `vector.drop-index` | function | Delete an index. |

See [Inference](/pages/api-inference/) and the entry limits in [Errors and limits](/pages/errors-and-limits/).

### Manifest

The grant goes into `manifest.json` under `capabilities`, in the list of the side that uses it (server or client).

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.semsearch",
  "capabilities": {
    "server": [
      "inference.embed",
      "fs.write:data"
    ]
  }
}
```

### Worked example

This plugin embeds two notes, stores them in an index and asks for the closest note to a query.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::Event;
use ilium_plugin_sdk::types::intents::{Intent, Submission};
use ilium_plugin_sdk::host::vector;

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct SemSearch;

impl Handler for SemSearch {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::CommandInvoked(invoked) = event else { continue };
            if invoked.command != "com.example.semsearch/demo" { continue; }
            let result = (|| -> Result<Vec<(String, f32)>, PluginError> {
                let notes = vec!["buy milk".to_string(), "plan the release".to_string()];
                let vectors = vector::embed(&notes, "notes").block_on()?;
                let index = vector::create_index("notes", vectors[0].len() as u16)?;
                let entries = notes.iter().cloned().zip(vectors.into_iter()).collect();
                vector::upsert(index, entries)?;
                let query = vector::embed(&["release plan".to_string()], "query").block_on()?;
                vector::top_k(index, &query[0], 1)
            })();
            match result {
                Ok(hits) => log::log(Level::Info, &format!("closest: {:?}", hits)),
                Err(error) => log::log(Level::Warn, &format!("vector failed: {error:?}")),
            }
        }
        Vec::new()
    }
}
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import type { Event, Submission } from '@ilium/plugin-sdk/types';
import { embed, createIndex, upsert, topK } from '@ilium/plugin-sdk/host/vector';
import { log } from '@ilium/plugin-sdk/host/log';

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  async onEvents(batch: Event[]): Promise<Submission[]> {
    for (const event of batch) {
      if (event.tag !== 'command-invoked' || event.val.command !== 'com.example.semsearch/demo') continue;
      try {
        const notes = ['buy milk', 'plan the release'];
        const vectors = await embed(notes, 'notes');
        const index = createIndex('notes', vectors[0].length);
        upsert(index, notes.map((id, i) => [id, vectors[i]] as [string, number[]]));
        const [query] = await embed(['release plan'], 'query');
        log('info', `closest: ${JSON.stringify(topK(index, query, 1))}`);
      } catch (error) {
        log('warn', `vector failed: ${String(error)}`);
      }
    }
    return [];
  },
});
```

Use the same model and dimension for the index and the queries. Changing the provider changes the vectors, so rebuild the index after the user switches models.

### Errors and edge cases

| Situation | Outcome | What the plugin does |
|---|---|---|
| Grant missing or declined | `not-permitted` | Fall back to keyword search. |
| Provider without embeddings | `unsupported` | Tell the user which provider to select. |
| More than 256 texts or a text above 8 KiB | `invalid-argument` | Split the batch. |
| Wrong vector dimension for the index | `invalid-argument` | Create a new index. |
| Index over the entry limit | `quota-exceeded` | Remove old entries. |
| Provider unreachable | `network` | Retry later. |

Index files count against the plugin storage quota.

### What the user sees

The review dialog lists the grant when a package is installed or first activated. The line states that text is sent to the selected inference provider.

<figure>

<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.semsearch 0.1.0 by Example Ltd                                 │
│                                                                            │
│ <span class="t-hi">Server side</span>                                                                │
│ [x] Computes text embeddings <span class="t-dim">……………………………………………………………………………</span> inference.embed │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>

<figcaption>The review dialog for a package that requests inference.embed, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar.</figcaption>
</figure>

Listing the grants of an installed plugin from the terminal:

<figure>

<pre class="term"><span class="t-prompt">$</span> ilium plugin grants com.example.semsearch
<span class="t-dim">Grant                             Risk     State</span>
inference.embed                   normal   <span class="t-ok">granted</span>
</pre>

<figcaption>Output of the grant listing for this plugin. A high-risk grant that the user left unchecked shows as declined.</figcaption>
</figure>

The user can change the grant at any time in Settings > Plugins > com.example.semsearch > Capabilities. A revocation re-activates the plugin with the reduced grant set, and the stubs of the declined imports return `not-permitted`.

### Related pages

- [inference](/pages/capability-inference/).
- [inference.provide](/pages/capability-inference-provide/).
- [Inference](/pages/api-inference/).
- [Storage](/pages/api-storage/).
