# vector and embeddings

## vector and embeddings

The `vector` import turns text into embeddings and searches them. An embedding is a list of numbers (a vector) that places a piece of text in a space where similar meanings sit close together. Cosine similarity measures how close two vectors point: 1.0 means the same direction, 0 means unrelated. A plugin can embed its notes once, store the vectors in an index, and later find the notes nearest to a question.

Two parts make up the import. `embed` sends texts through the inference provider that the user selected and returns one vector per text. The index functions keep vectors in a host-held index and answer nearest-neighbour queries. The host does the maths, so the plugin ships no search code and the query stays fast up to the entry limit.

**You will need:**

- The `inference.embed` grant, on the server or the client side that calls the import. The grant takes no argument and has ordinary risk. The user reviews it at install. See [Capability: inference.embed](/pages/capability-inference/) for the related `inference` grant; there is no separate page for `inference.embed`.
- A selected inference provider that offers embeddings. Without one, `embed` fails with `unsupported`. The index functions work without a provider, because they only store and compare vectors the plugin already holds.
- Your own data scope. Indexes are persisted per plugin and are never shared with other plugins.

### Interface

```wit
// Excerpt of ilium:host/vector in reference-wit
// vector (S C, grant inference.embed)
embed: func(texts: list<string>, purpose: string) -> result<request-id, plugin-error>;   // <= 256 texts of <= 8 KiB
take-vectors: func(request: request-id) -> option<list<list<f32>>>;

type index-id = u32;
create-index: func(name: string, dimensions: u16) -> result<index-id, plugin-error>;    // persisted in the data scope
upsert: func(index: index-id, entries: list<tuple<string, list<f32>>>) -> result<_, plugin-error>;
remove-entries: func(index: index-id, ids: list<string>) -> result<_, plugin-error>;
top-k: func(index: index-id, query: list<f32>, k: u16) -> result<list<tuple<string, f32>>, plugin-error>;   // cosine; k <= 100
drop-index: func(index: index-id);
```

The SDK imports are `ilium_plugin_sdk::host::vector` in Rust and `@ilium/plugin-sdk/host/vector` in TypeScript. Both ship an awaited wrapper for `embed` and a raw `embed_request` / `embedRequest` form that returns the `request-id`. The awaited wrapper does the `request-completed` and `take-vectors` steps for you. See [requests and outcomes](/pages/requests-and-outcomes/) for the pattern.

### Functions

| Function | Rust | TypeScript | Result |
|---|---|---|---|
| `embed` | `vector::embed(texts, purpose).await` | `await embed(texts, purpose)` | `Result<Vec<Vec<f32>>, PluginError>`; `Promise<number[][]>` |
| `embed` request form | `vector::embed_request(texts, purpose)` | `embedRequest(texts, purpose)` | `Result<RequestId, PluginError>`; `bigint` |
| `take-vectors` | `vector::take_vectors(id)` | `takeVectors(id)` | `Option<Vec<Vec<f32>>>`; once only |
| `create-index` | `vector::create_index(name, dimensions)` | `createIndex(name, dimensions)` | `Result<IndexId, PluginError>`; `number` |
| `upsert` | `vector::upsert(index, entries)` | `upsert(index, entries)` | `Result<(), PluginError>`; `void` |
| `remove-entries` | `vector::remove_entries(index, ids)` | `removeEntries(index, ids)` | `Result<(), PluginError>`; `void` |
| `top-k` | `vector::top_k(index, query, k)` | `topK(index, query, k)` | `Result<Vec<(String, f32)>, PluginError>`; `Array<[string, number]>` |
| `drop-index` | `vector::drop_index(index)` | `dropIndex(index)` | nothing |

`purpose` names the feature that asks. It reaches the admission gate and the privacy log, the same way as the `purpose` of a [completion](/pages/api-inference/). Write a short noun phrase such as `note search`.

`embed` returns the vectors in the order of the texts. All vectors of one call have the same length, which is set by the embedding model. Create an index with exactly that length as `dimensions`; a mismatch fails `upsert` with `invalid-argument`. The score that `top-k` returns is the cosine similarity of the query and the entry, highest first.

The index persists in the plugin's data scope. The interface has no call that looks an index up by name, so keep the `index-id` with the plugin's own state next to the index name.

<figure>
<svg viewBox="0 0 420 270" role="img" aria-label="Sequence of embedding texts and querying an index">
<title>Embed, store and search</title>
<desc>The plugin sends texts to the host. The host asks the selected inference provider for vectors and reports request-completed. The plugin stores the vectors in the host index and later queries top-k.</desc>
<defs>
<marker id="vec-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
<path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
</marker>
</defs>
<text x="50" y="16" font-size="12" text-anchor="middle" fill="currentColor">Plugin</text>
<text x="210" y="16" font-size="12" text-anchor="middle" fill="currentColor">Host</text>
<text x="370" y="16" font-size="12" text-anchor="middle" fill="currentColor">Provider</text>
<line x1="50" y1="22" x2="50" y2="262" stroke="currentColor" stroke-dasharray="3 3"/>
<line x1="210" y1="22" x2="210" y2="262" stroke="currentColor" stroke-dasharray="3 3"/>
<line x1="370" y1="22" x2="370" y2="262" stroke="currentColor" stroke-dasharray="3 3"/>
<line x1="50" y1="42" x2="208" y2="42" stroke="currentColor" marker-end="url(#vec-arrow)"/>
<text x="56" y="36" font-size="11" fill="currentColor">embed(texts)</text>
<line x1="210" y1="62" x2="368" y2="62" stroke="currentColor" marker-end="url(#vec-arrow)"/>
<text x="216" y="56" font-size="11" fill="currentColor">embeddings request</text>
<line x1="370" y1="88" x2="212" y2="88" stroke="currentColor" marker-end="url(#vec-arrow)"/>
<text x="232" y="82" font-size="11" fill="currentColor">vectors</text>
<line x1="210" y1="112" x2="52" y2="112" stroke="currentColor" marker-end="url(#vec-arrow)"/>
<text x="62" y="106" font-size="11" fill="currentColor">request-completed</text>
<line x1="50" y1="136" x2="208" y2="136" stroke="currentColor" marker-end="url(#vec-arrow)"/>
<text x="56" y="130" font-size="11" fill="currentColor">take-vectors</text>
<line x1="50" y1="166" x2="208" y2="166" stroke="currentColor" marker-end="url(#vec-arrow)"/>
<text x="56" y="160" font-size="11" fill="currentColor">upsert(index, entries)</text>
<line x1="50" y1="206" x2="208" y2="206" stroke="currentColor" marker-end="url(#vec-arrow)"/>
<text x="56" y="200" font-size="11" fill="currentColor">top-k(index, query, 5)</text>
<line x1="210" y1="232" x2="52" y2="232" stroke="currentColor" marker-end="url(#vec-arrow)"/>
<text x="62" y="226" font-size="11" fill="currentColor">ids with cosine scores</text>
<text x="216" y="254" font-size="11" fill="currentColor">index lives in the host</text>
</svg>
<figcaption>Only embed reaches the provider. The index calls are answered by the host at once, and the query itself needs a vector, so a question is embedded first.</figcaption>
</figure>

### Example 1: index notes and search them

The plugin embeds each note body and stores the vectors in an index named `notes`. To search, it embeds the question and asks the index for the five nearest notes. The manifest asks for the grant:

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "example.notes",
  "capabilities": { "server": ["inference.embed"] }
}
```

**Rust**

```rust
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::host::vector;
use ilium_plugin_sdk::types::errors::PluginError;

const PURPOSE: &str = "note search";
const NOTES_INDEX: &str = "notes";

pub struct NoteIndex {
    index: u32,
}

impl NoteIndex {
    /// Embeds one batch of notes and stores them. `notes` holds (id, body).
    pub async fn build(notes: &[(String, String)]) -> Result<NoteIndex, PluginError> {
        let texts: Vec<String> = notes.iter().map(|(_, body)| body.clone()).collect();
        let vectors = vector::embed(&texts, PURPOSE).await?;
        let dimensions = vectors.first().map(|vector| vector.len()).unwrap_or(0);
        let index = vector::create_index(NOTES_INDEX, dimensions as u16)?;
        let entries: Vec<(String, Vec<f32>)> = notes
            .iter()
            .map(|(id, _)| id.clone())
            .zip(vectors)
            .collect();
        vector::upsert(index, &entries)?;
        log::log(Level::Info, &format!("indexed {} notes", entries.len()));
        Ok(NoteIndex { index })
    }

    /// Returns the ids of the five nearest notes with their cosine scores.
    pub async fn search(&self, question: &str) -> Result<Vec<(String, f32)>, PluginError> {
        let query = vector::embed(&[question.to_string()], PURPOSE).await?;
        let Some(first) = query.into_iter().next() else {
            return Ok(Vec::new());
        };
        vector::top_k(self.index, &first, 5)
    }
}
```

**TypeScript**

```ts
import { embed, createIndex, upsert, topK } from '@ilium/plugin-sdk/host/vector';
import { log } from '@ilium/plugin-sdk/host/log';

const PURPOSE = 'note search';
const NOTES_INDEX = 'notes';

export async function buildIndex(notes: Array<[string, string]>): Promise<number> {
  const vectors = await embed(notes.map(([, body]) => body), PURPOSE);
  const dimensions = vectors[0]?.length ?? 0;
  const index = createIndex(NOTES_INDEX, dimensions);
  const entries: Array<[string, number[]]> = notes.map(([id], position) => [id, vectors[position]]);
  upsert(index, entries);
  log('info', `indexed ${entries.length} notes`);
  return index;
}

export async function search(index: number, question: string): Promise<Array<[string, number]>> {
  const [query] = await embed([question], PURPOSE);
  return query === undefined ? [] : topK(index, query, 5);
}
```

An `embed` call takes at most 256 texts of at most 8 KiB each. Split a larger corpus into batches of 256 and call `upsert` for each batch. Calling `upsert` again with an id that exists replaces that entry, so a changed note is handled by embedding it once more.

### Example 2: remove entries and drop an index

**Rust**

```rust
use ilium_plugin_sdk::host::vector;

fn forget_note(index: u32, note_id: &str) {
    if let Err(error) = vector::remove_entries(index, &[note_id.to_string()]) {
        log::log(Level::Warn, &format!("cannot forget {note_id}: {}", error.message));
    }
}

fn reset_notes(index: u32) {
    vector::drop_index(index);
}
```

**TypeScript**

```ts
import { removeEntries, dropIndex } from '@ilium/plugin-sdk/host/vector';

export function forgetNote(index: number, noteId: string): void {
  removeEntries(index, [noteId]);
}

export function resetNotes(index: number): void {
  dropIndex(index);
}
```

`drop-index` deletes the index and its entries and frees their quota. Do not use the `index-id` after a drop.

### What the user sees

The import has no screen of its own. The user sees the effect in the plugin's panel, such as a list of notes ordered by relevance, and in the admission gate and privacy log, which receive the `purpose` of each `embed` call. The plugin log shows the calls the plugin writes:

<pre class="term">2026-10-12 09:41:03 example.notes info  indexed 212 notes
2026-10-12 09:41:03 example.notes info  vector.embed 212 texts, 2 batches
2026-10-12 09:44:19 example.notes info  vector.top_k notes k=5 best=0.83
2026-10-12 09:44:51 example.notes warn  embed failed: unsupported
</pre>

The capture is hand-composed to show the format of the plugin log lines. The `embed failed: unsupported` line is what a plugin writes when the user has selected a provider that offers no embeddings; the plugin then falls back to a plain text filter.

### Errors

| Call | Code | Cause |
|---|---|---|
| `embed` | `unsupported` | The selected provider offers no embeddings, or none is selected |
| `embed` | `invalid-argument` | More than 256 texts, a text over 8 KiB, or an empty list |
| `embed` | `not-permitted` | The `inference.embed` grant is not held or was revoked |
| `embed` completion | `network`, `rate-limited`, `timed-out` | The provider failed. Retry later |
| `embed` completion | `cancelled` | The plugin cancelled the request, or the application shut down |
| `create-index` | `quota-exceeded` | The plugin already holds the maximum number of indexes (`vector_indexes`) |
| `create-index` | `invalid-argument` | `dimensions` is 0 |
| `upsert` | `quota-exceeded` | The entry count over all indexes reaches `vector_entries` |
| `upsert` | `invalid-argument` | A vector length differs from the dimensions of the index |
| `upsert`, `top-k`, `remove-entries`, `drop-index` | `not-found` | The `index-id` does not exist (never created, already dropped or owned by another plugin). `remove-entries` ignores an id that is not in the index. `drop-index` has no error channel, so an unknown id is ignored |
| `top-k` | `invalid-argument` | The query length differs from the dimensions, or `k` is 0 or above 100 |

### Limits

| Limit | Default | Ceiling T1 | Ceiling T2 and T3 | Effect |
|---|---|---|---|---|
| `vector_indexes` | 4 | 16 | 4 | `quota-exceeded` |
| `vector_entries` (all indexes together) | 10,000 | 200,000 | 50,000 | `quota-exceeded` |
| Texts per `embed` | 256 | | | `invalid-argument` |
| Size of one text | 8 KiB | | | `invalid-argument` |
| `k` of `top-k` | 100 | | | `invalid-argument` |

The default applies unless the manifest asks for more, up to the ceiling of the tier. A plugin with `limits_tier: first-party` follows the T1 column.

### Edge cases

- Switching the embedding model changes the vector length. Existing indexes keep their old dimensions, so create a new index, re-embed, and drop the old one. A plugin should store the name of the model in the index name, such as `notes-v2`.
- An index is exact, not approximate: the answer is the true nearest set. It stays fast up to the entry limit only because the host holds the data.
- Embeddings leave the machine when the provider is remote. Treat `embed` like a completion for privacy.

### See also

- [inference](/pages/api-inference/) for completions and for routing to the selected provider.
- [Capability: inference](/pages/capability-inference/) for the related grant and review text.
- [requests and outcomes](/pages/requests-and-outcomes/) for the `request-id` pattern.
- [storage](/pages/api-storage/) for the plugin data scope that holds the indexes.
- [errors and limits](/pages/reference-errors-and-limits/) for the full tables.

Checked against `ilium:plugin@0.1.0`.
