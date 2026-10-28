# storage

## storage

The `storage` import keeps small key-value data for a plugin. Every value lives in one of three buckets: `global`, `project` or `session`. The bucket decides which projects or sessions can see the value and how long it lasts. Each `put` replaces one value atomically, so a reader never sees half of a write. Storage is for small state such as counters, cursors and settings-like flags. For large data, use [fs](/pages/api-fs/).

**You need:**

- **Grant:** none. Storage is always granted in the server (S), client (C) and scene (A) worlds, so the manifest does not list it.
- **Language:** Rust with the SDK, or TypeScript with `@ilium/plugin-sdk`. Both wrap the same interface.
- **Import:** Rust `ilium_plugin_sdk::host::storage`; TypeScript `@ilium/plugin-sdk/host/storage`.
- **Toolchain:** the SDK pins its own versions. See [Rust SDK](/pages/rust-sdk/) or [TypeScript SDK](/pages/typescript-sdk/).

### Interface

```wit
// Excerpt of ilium:host/storage in reference-wit
enum bucket { global, project, session }
get: func(bucket: bucket, key: string) -> option<list<u8>>;
put: func(bucket: bucket, key: string, value: list<u8>) -> result<_, plugin-error>;
remove: func(bucket: bucket, key: string);
keys: func(bucket: bucket, prefix: string) -> list<string>;
record entry { key: string, value: option<list<u8>> }   // none removes the key
write-batch: func(bucket: bucket, entries: list<entry>) -> result<_, plugin-error>;
```

### Surface

| Function | Rust | TypeScript | Returns | Error codes |
|---|---|---|---|---|
| `get` | `get(bucket, key) -> Option<Vec<u8>>` | `get(bucket, key): Uint8Array \| undefined` | the value, or nothing | none |
| `put` | `put(bucket, key, &[u8]) -> Result<(), PluginError>` | `put(bucket, key, Uint8Array): void`, throws | nothing | `invalid-argument`, `quota-exceeded`, `io` (`no-space`, [LIF-119]), `conflict` (`stale-data-version`, [LIF-121]) |
| `remove` | `remove(bucket, key) -> Result<(), PluginError>` | `remove(bucket, key): void`, throws | nothing | `conflict` (`stale-data-version`, [LIF-121]) |
| `lease` | `lease(key, ttl_millis) -> bool` | `lease(key, ttlMillis): boolean` | true for the one caller that may refresh a `shared` entry | none; a caller that does not win the lease gets false, which is not an error |
| `keys` | `keys(bucket, prefix) -> Vec<String>` | `keys(bucket, prefix): string[]` | matching keys | none |
| `write-batch` | `write_batch(bucket, &[Entry]) -> Result<(), PluginError>` | `writeBatch(bucket, entries): void`, throws | nothing | `invalid-argument`, `quota-exceeded`, `io` (`no-space`, [LIF-119]), `conflict` (`stale-data-version`, [LIF-121]) |

The bucket type is the enum `Bucket` with the variants `Global`, `Project`, `Session` and `Shared` in Rust. In TypeScript it is the string `'global'`, `'project'`, `'session'` or `'shared'` (see [The shared bucket and the lease](#the-shared-bucket-and-the-lease)). All six functions (`get`, `put`, `remove`, `keys`, `write-batch` and `lease`) are synchronous: each call returns when the host has applied it, with no request id to wait for.

`put` also returns the universal codes `not-permitted`, `revoked` and `internal` (see [errors and limits](/pages/reference-errors-and-limits/)). In practice these appear only when the grant is revoked during a call.

### Buckets

| Bucket | Shared with | Lifetime | Typical use |
|---|---|---|---|
| `global` | every project on this machine, for this plugin | until the plugin's data is deleted | a preference that applies everywhere |
| `project` | every session that opens the same canonical project directory | until the project's data is deleted | counters, last-run markers, per-project state |
| `session` | only the one named session | deleted when that session is deleted | cursors, pane state |

A session's name comes from its file under `.ilium/sessions/`. The bare `ilium` command owns the `default` session. The kernel does not delete a plugin's storage keys when a pane closes; a plugin that keys data by pane removes it itself.

### Rules

- **Keys:** a non-empty UTF-8 string of at most 256 bytes. The host stores the key byte for byte, with no trimming or case folding. A slash has no special meaning to the host; the `pane/<node-id>/` pattern above is a convention.
- **Values:** a byte array of at most 1 MiB per `put`. An empty value is allowed.
- **Quota:** each bucket holds 16 MiB by default. The manifest field `limits.storage_bucket_mib` may request less, or more up to the host ceiling: 256 MiB for tier 1 and 16 MiB for tiers 2 and 3. The size of a bucket is the sum of its key bytes and value bytes. A `put` that replaces a value counts only the difference.
- **Atomic replacement:** a `put` either replaces the whole value or changes nothing. A refused `put` keeps the previous value.
- **Keys listing:** `keys` returns every key that starts with the prefix, sorted by byte order. An empty prefix returns all keys of the bucket. Results are not paged.
- **Missing keys:** `get` returns nothing, and `remove` does nothing. Neither is an error.

### Batches, durability and concurrency

- [LIF-117] `write-batch` MUST apply all its entries or none of them. A batch holds at most 256 entries and at most 4 MiB of key and value bytes together; more is `invalid-argument`. A refused batch keeps every previous value. Test: write a batch whose last entry breaks the quota and read all keys.
- [LIF-118] `put`, `remove` and `write-batch` MUST return only after the change is flushed to stable storage, so that a crash or power loss after the return cannot lose it. A crash before the return leaves the previous value. Test: kill the process in the middle of a batch and read back.
- [LIF-119] A write that fails because the disk is full MUST return `io` with the message `no-space`, never `quota-exceeded`, which is only for the plugin's own quota. Test: fill the disk and `put`. See [offline and low disk](/pages/reference-offline-and-low-disk/).
- [LIF-120] Several session servers can write the same `global` or `project` bucket. The host MUST serialise writers with a lock per bucket, so that each `put` and each batch is atomic across processes, and the last writer of a key wins. There is no compare-and-set; use a version key and `write-batch` if you need one. Test: two servers write the same key and read it back.
- [LIF-121] The bucket also carries a data version stamp that the plugin cannot see. A write from an instance older than the stamp MUST return `conflict` with the message `stale-data-version`. See [data migration](/pages/guide-data-migration/). Test: see LIF-113.

A storage database that fails its integrity check is moved aside and the plugin starts with empty storage ([offline and low disk](/pages/reference-offline-and-low-disk/)).

<figure>
<svg viewBox="0 0 400 230" role="img" aria-label="A plugin calls the storage import, which routes each value to one of three buckets">
<title>Storage buckets and their reach</title>
<desc>The plugin calls get, put, remove and keys. The storage import sends each call to one of three buckets: global for all projects, project for one project directory, and session for one named session.</desc>
<defs>
<marker id="storage-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
<path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
</marker>
</defs>
<rect x="130" y="8" width="140" height="36" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
<text x="200" y="31" font-size="12" text-anchor="middle" fill="currentColor">Plugin</text>
<line x1="200" y1="44" x2="200" y2="68" stroke="currentColor" marker-end="url(#storage-arrow)"/>
<text x="208" y="60" font-size="10" fill="currentColor">get, put, remove, keys</text>
<rect x="8" y="70" width="384" height="36" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
<text x="200" y="92" font-size="12" text-anchor="middle" fill="currentColor">Storage import, atomic put</text>
<line x1="64" y1="106" x2="64" y2="138" stroke="currentColor" marker-end="url(#storage-arrow)"/>
<line x1="200" y1="106" x2="200" y2="138" stroke="currentColor" marker-end="url(#storage-arrow)"/>
<line x1="336" y1="106" x2="336" y2="138" stroke="currentColor" marker-end="url(#storage-arrow)"/>
<rect x="8" y="140" width="112" height="60" rx="6" fill="none" stroke="currentColor"/>
<text x="64" y="162" font-size="12" text-anchor="middle" fill="currentColor">global</text>
<text x="64" y="180" font-size="10" text-anchor="middle" fill="currentColor">all projects</text>
<rect x="144" y="140" width="112" height="60" rx="6" fill="none" stroke="currentColor"/>
<text x="200" y="162" font-size="12" text-anchor="middle" fill="currentColor">project</text>
<text x="200" y="180" font-size="10" text-anchor="middle" fill="currentColor">one project dir</text>
<rect x="280" y="140" width="112" height="60" rx="6" fill="none" stroke="currentColor"/>
<text x="336" y="162" font-size="12" text-anchor="middle" fill="currentColor">session</text>
<text x="336" y="180" font-size="10" text-anchor="middle" fill="currentColor">one session</text>
<text x="8" y="222" font-size="10" fill="currentColor">The bucket decides which projects or sessions see a value.</text>
</svg>
<figcaption>One storage import serves three buckets. The bucket argument, not the plugin's code, sets
the scope of each value.</figcaption>
</figure>

### Example 1: a save counter per project

The counter lives in the `project` bucket. Two projects get two counters, because each project directory is its own bucket scope.

**Grants:** none (always granted)

**Rust**

```rust
use ilium_plugin_sdk::host::storage::{self, Bucket};
use ilium_plugin_sdk::prelude::*;

const COUNTER_KEY: &str = "saves";

fn read_counter() -> u64 {
    storage::get(Bucket::Project, COUNTER_KEY)
        .and_then(|bytes| <[u8; 8]>::try_from(bytes).ok())
        .map_or(0, u64::from_le_bytes)
}

fn increment_counter() -> Result<u64, PluginError> {
    let next = read_counter() + 1;
    storage::put(Bucket::Project, COUNTER_KEY, &next.to_le_bytes())?;
    Ok(next)
}
```

**TypeScript**

```ts
import * as storage from '@ilium/plugin-sdk/host/storage';

const COUNTER_KEY = 'saves';

export function readCounter(): bigint {
  const bytes = storage.get('project', COUNTER_KEY);
  if (bytes === undefined || bytes.length !== 8) {
    return 0n;
  }
  return new DataView(bytes.buffer, bytes.byteOffset, 8).getBigUint64(0, true);
}

export function incrementCounter(): bigint {
  const next = readCounter() + 1n;
  const bytes = new Uint8Array(8);
  new DataView(bytes.buffer).setBigUint64(0, next, true);
  storage.put('project', COUNTER_KEY, bytes);
  return next;
}
```

After two saves in the same project, `increment_counter()` returns `2`. The TypeScript `incrementCounter()` returns `2n`. In another project the same call starts again at `1`.

### Example 2: a cursor per pane, with a key listing

This example stores one cursor per pane in the `session` bucket. It uses the `pane/<node-id>/`
convention, so `keys` can list the panes and `remove` can forget one. The pane identifier is passed in
as a string, so the example does not depend on how `NodeId` is represented.

**Grants:** none (always granted)

**Rust**

```rust
use ilium_plugin_sdk::host::storage::{self, Bucket};
use ilium_plugin_sdk::prelude::*;

fn cursor_key(pane_key: &str) -> String {
    format!("pane/{pane_key}/cursor")
}

fn save_cursor(pane_key: &str, cursor: u32) -> Result<(), PluginError> {
    storage::put(Bucket::Session, &cursor_key(pane_key), &cursor.to_le_bytes())
}

fn load_cursor(pane_key: &str) -> u32 {
    storage::get(Bucket::Session, &cursor_key(pane_key))
        .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
        .map_or(0, u32::from_le_bytes)
}

fn panes_with_cursor() -> Vec<String> {
    storage::keys(Bucket::Session, "pane/")
        .into_iter()
        .filter_map(|key| {
            let pane_key = key.strip_prefix("pane/")?.strip_suffix("/cursor")?;
            Some(pane_key.to_owned())
        })
        .collect()
}

fn forget_cursor(pane_key: &str) {
    storage::remove(Bucket::Session, &cursor_key(pane_key));
}
```

**TypeScript**

```ts
import * as storage from '@ilium/plugin-sdk/host/storage';

function cursorKey(paneKey: string): string {
  return `pane/${paneKey}/cursor`;
}

export function saveCursor(paneKey: string, cursor: number): void {
  const bytes = new Uint8Array(4);
  new DataView(bytes.buffer).setUint32(0, cursor, true);
  storage.put('session', cursorKey(paneKey), bytes);
}

export function loadCursor(paneKey: string): number {
  const bytes = storage.get('session', cursorKey(paneKey));
  if (bytes === undefined || bytes.length !== 4) {
    return 0;
  }
  return new DataView(bytes.buffer, bytes.byteOffset, 4).getUint32(0, true);
}

export function panesWithCursor(): string[] {
  return storage
    .keys('session', 'pane/')
    .filter((key) => key.endsWith('/cursor'))
    .map((key) => key.slice('pane/'.length, -'/cursor'.length));
}

export function forgetCursor(paneKey: string): void {
  storage.remove('session', cursorKey(paneKey));
}
```

Saving cursor `7` for pane `p3` and then calling `panes_with_cursor()` returns `["p3"]`. After
`forget_cursor("p3")`, the list is empty and `load_cursor("p3")` returns `0`.

### What the user sees

Storage has no screen of its own. A refused `put` is a return value to the plugin, so the user sees
the failure only where the plugin reports it, for example in its log. The plugin log line has the
format `YYYY-MM-DD HH:mm:ss`, the level, the plugin id and the message:

<pre class="term"><span class="t-prompt">$</span> ilium plugin logs demo-plugin --tail 3
<span class="t-dim">2026-10-11 09:14:02</span> <span class="t-ok">info </span> demo-plugin saves now 2
<span class="t-dim">2026-10-11 09:14:31</span> <span class="t-err">error</span> demo-plugin put saves failed: quota-exceeded
<span class="t-dim">2026-10-11 09:14:31</span> <span class="t-warn">warn </span> demo-plugin kept the value from 09:14:02
</pre>

Illustrative, not generated. The plugin writes these lines itself; the host only stores them and
formats them. The key, values and times are placeholders.

When a plugin is uninstalled, the user chooses whether to keep or delete its data and storage. A
deleted session bucket goes with its session.

**Export before you remove.** `ilium plugin export <id> --out <file>` copies a plugin's data, buckets and settings values into one archive, and `ilium plugin import <id> --from <file>` restores it on another machine. Details: LIF-290 to LIF-292 in [Install, update and uninstall](/pages/reference-install-update-uninstall/).

### Errors

| Code | Raised by | Cause | Action |
|---|---|---|---|
| `invalid-argument` | `put` | key empty or over 256 bytes, or value over 1 MiB | fix the call; nothing is written |
| `quota-exceeded` | `put` | the bucket's size (key bytes plus value bytes) would pass its quota; a full disk is `io` (`no-space`), not this code | remove keys or shrink values, then retry |
| `io` (`no-space`) | `put`, `write-batch` | the disk is full; never reported as `quota-exceeded` ([LIF-119]) | free disk space, then retry |
| `conflict` (`stale-data-version`) | `put`, `remove`, `write-batch` | the instance is older than the bucket's data version stamp ([LIF-121]) | restart the plugin on the current version |

A value over 1 MiB is always `invalid-argument`, never `quota-exceeded`. This lets a plugin tell a bad
call from a full bucket. The host never truncates a value.

### The shared bucket and the lease

The enum gains a fourth case, `shared`. It is one cache bucket per plugin, shared by the plugin's server side, all its client sides and every session of the same user. The host may clear it at any time, so it holds only data that can be rebuilt: a downloaded index, a rendered image, a geocoding answer. Entries beyond `shared_cache_mib` (64 MiB by default; 256 for tier 1, 128 for tiers 2 and 3) evict the oldest first.

```wit
// Excerpt of ilium:host/storage in reference-wit
enum bucket { global, project, session, shared }
lease: func(key: string, ttl-millis: u32) -> bool;     // true for the one caller that may refresh
```

Several sessions can notice at the same moment that a cache entry is stale. The lease makes only one of them refresh it: `lease` returns true to exactly one caller per `key` until `ttl-millis` passes, and false to the rest, who should read the old value or wait. The lease is not a lock for correctness; it prevents duplicate work.

| Bucket | Shared with | Cleared by the host | Count against |
|---|---|---|---|
| `shared` | server side, client sides and all sessions of the user | any time | `shared_cache_mib` |

<figure>
<svg viewBox="0 0 440 150" role="img" aria-label="Two sessions and the shared cache lease">
<title>Shared cache lease</title>
<desc>Two sessions find a stale entry. The first gets the lease and refreshes it. The second gets false and reads the old value.</desc>
<defs><marker id="sb" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0,0 L10,5 L0,10 z" fill="currentColor"/></marker></defs>
<rect x="8" y="12" width="110" height="34" rx="6" fill="none" stroke="currentColor"/><text x="63" y="33" font-size="12" text-anchor="middle" fill="currentColor">session A</text>
<rect x="8" y="100" width="110" height="34" rx="6" fill="none" stroke="currentColor"/><text x="63" y="121" font-size="12" text-anchor="middle" fill="currentColor">session B</text>
<rect x="190" y="52" width="120" height="44" rx="6" fill="none" stroke="currentColor"/><text x="250" y="72" font-size="12" text-anchor="middle" fill="currentColor">shared bucket</text><text x="250" y="88" font-size="11" text-anchor="middle" fill="currentColor">key + lease</text>
<line x1="118" y1="29" x2="190" y2="62" stroke="currentColor" marker-end="url(#sb)"/><text x="130" y="38" font-size="11" fill="currentColor">lease: true</text>
<line x1="118" y1="117" x2="190" y2="86" stroke="currentColor" marker-end="url(#sb)"/><text x="130" y="112" font-size="11" fill="currentColor">lease: false</text>
<text x="330" y="66" font-size="11" fill="currentColor">A refreshes, then put</text>
<text x="330" y="106" font-size="11" fill="currentColor">B reads the old value</text>
</svg>
<figcaption>Only one caller per key refreshes the entry while its lease is open.</figcaption>
</figure>

**Rust**

```rust
use ilium_plugin_sdk::host::storage::{self, Bucket};

fn index_bytes() -> Option<Vec<u8>> {
    if storage::lease("index", 60_000) {
        let fresh = rebuild_index();                       // slow work, done once
        let _ = storage::put(Bucket::Shared, "index", &fresh);
        return Some(fresh);
    }
    storage::get(Bucket::Shared, "index")                  // someone else refreshes
}
```

**TypeScript**

```ts
import { get, put, lease } from '@ilium/plugin-sdk/host/storage';

export function indexBytes(): Uint8Array | undefined {
  if (lease('index', 60_000)) {
    const fresh = rebuildIndex();
    put('shared', 'index', fresh);
    return fresh;
  }
  return get('shared', 'index');
}
```

Errors: the 1 MiB value cap applies to `shared` like every bucket, so a larger value is `invalid-argument`. `shared` never returns `quota-exceeded` for size: when the bucket exceeds `shared_cache_mib`, the oldest entries go. The key rules are those of the other buckets.

### See also

- [fs](/pages/api-fs/) for files, which suit large data.
- [Data migration](/pages/guide-data-migration/) for changing the layout of stored data between versions.
- [settings](/pages/api-settings/) for values that the user edits.
- [Always-granted capabilities](/pages/capability-always-granted/) for the other imports that need no grant.
- [Errors and limits](/pages/reference-errors-and-limits/) for the error codes and the full limit table.
