# Always-granted capabilities

Some imports need no grant at all. Every plugin gets them on every side that carries them, and a manifest never has to ask for them. Calls to these imports cannot fail with `not-permitted` for capability reasons. This page lists the set, gives the exact SDK paths and function signatures, and shows what each call can still return. Every other capability has its own page in this section.

## Overview

The always-granted set covers the plugin's own state and its basic tools:

- writing log lines and reading the clock,
- timers that fire back into the plugin,
- a key-value store in three buckets, and the plugin's own settings values,
- compression, image decoding, hashing and text rasterizing,
- read and write access to three folders (package, data, cache) that belong to the plugin,
- messages between the plugin's server side and client side,
- asking the host to render again a view the plugin declares.

A plugin that keeps its state in storage and writes its own logs needs nothing in its manifest.

## You will need

- Nothing in the manifest. This set is available to every plugin.
- A manifest with no `capabilities` key behaves exactly like `"capabilities": {}`, and like a side
  with an empty list. The plugin then has the always-granted set and nothing else.
- The SDK for your language: the Rust crate `ilium-plugin-sdk` or the npm package
  `@ilium/plugin-sdk`. Plugins depend only on that package.

The table uses these side letters: `S` is the server side, `C` the client side and `A` the scene
(animation) side.

## The always-granted set

| Grant or import | Sides | Functions and types | What it gives the plugin |
|---|---|---|---|
| `log` | S C A | `log(level, message)`, `level` is `trace`, `debug`, `info`, `warn` or `error` | One log line per call. |
| `clock` | S C A | `now-unix-millis() -> u64`, `monotonic-millis() -> u64` | Wall-clock and monotonic time in milliseconds. |
| `timers` | S C | `set(after-millis, repeat-millis?) -> result<timer-id, plugin-error>`, `cancel(timer)` | Timers that fire as the `timer-fired` event. |
| `storage` | S C A | `get`, `put`, `remove`, `keys`, `write-batch` and `lease` on `bucket`: `global`, `project`, `session`, `shared` | Key-value entries that only this plugin can see. |
| `settings` | S C | `get-json() -> string`, `set-json(patch)` | Read and write the plugin's own setting values. |
| `codec` | S C A | `inflate`, `deflate`, `image-info`, `decode-image`, `sha256`, `rasterize-text` | Compression, images, hashes and text as pixels. |
| `fs.read:package` | S C A | The read functions of `fs` (`read`, `read-all`, `list-dir`, `metadata`) | Read only, on the package's `assets/` directory. |
| `fs.read:data`, `fs.write:data` | S C A | The read functions and the write functions (`write`, `remove`, `rename`, `create_dir`) | The plugin's private data directory. |
| `fs.read:cache`, `fs.write:cache` | S C A | The same functions as `data` | The plugin's evictable cache directory. |
| `messages` (own side) | S C | Intent `message`, event `message` | Messages to and from the other side of the same plugin. |
| `ui.invalidate` (own views) | C | `invalidate(view-target)` | Asks the host to render again a view the plugin declares. |

The `fs` rows use the file scopes `package`, `data` and `cache`. Messages to another plugin are not
in this set: they need `messages.peer:<plugin-id>`.

## Module reference

Each import has one module in the SDK. Rust names follow the generated-name rules (S5): functions
are `snake_case`, `u64` is `u64`, and `bucket` becomes the enum `Bucket`. TypeScript uses camelCase
and `bigint` for every `u64`.

| Import | Rust path | TypeScript import |
|---|---|---|
| `log` | `ilium_plugin_sdk::host::log::{self, Level}` | `@ilium/plugin-sdk/host/log` |
| `clock` | `ilium_plugin_sdk::host::clock` | `@ilium/plugin-sdk/host/clock` |
| `timers` | `ilium_plugin_sdk::host::timers` | `@ilium/plugin-sdk/host/timers` |
| `storage` | `ilium_plugin_sdk::host::storage` | `@ilium/plugin-sdk/host/storage` |
| `settings` | `ilium_plugin_sdk::host::settings` | `@ilium/plugin-sdk/host/settings` |
| `codec` | `ilium_plugin_sdk::host::codec` | `@ilium/plugin-sdk/host/codec` |
| `fs` | `ilium_plugin_sdk::host::fs` | `@ilium/plugin-sdk/host/fs` |
| `messages` | `Intent::Message`, `Event::Message` | `Intent` and `Event` from `@ilium/plugin-sdk/types` |
| `ui.invalidate` | `ilium_plugin_sdk::host::ui::invalidate(view)` | `invalidate(view)` from `@ilium/plugin-sdk/host/ui` |

The `TimerId` type is `ilium_plugin_sdk::types::ids::TimerId`.

Storage, settings and codec signatures, in WIT form:

```wit
enum bucket { global, project, session, shared }
get: func(bucket: bucket, key: string) -> option<list<u8>>;
put: func(bucket: bucket, key: string, value: list<u8>) -> result<_, plugin-error>;
remove: func(bucket: bucket, key: string);
keys: func(bucket: bucket, prefix: string) -> list<string>;
write-batch: func(bucket: bucket, entries: list<entry>) -> result<_, plugin-error>;
lease: func(key: string, ttl-millis: u32) -> bool;

get-json: func() -> string;
set-json: func(patch: string) -> result<_, plugin-error>;

sha256: func(data: list<u8>) -> list<u8>;
inflate: func(format: compression, data: list<u8>, max-output: u32)
    -> result<list<u8>, plugin-error>;
```

## What this set does not include

Anything outside the table needs a grant. Reading a project file needs `fs.read:project`, reading
another plugin's settings needs `settings.read-all`, and sending to another plugin needs
`messages.peer:<plugin-id>`. Each has its own page.

<div class="note">

The data and cache scopes belong to one plugin, and the package scope is read only. Files that the
user keeps elsewhere, such as a project directory or a file they picked, need a grant such as
`fs.read:project` or `fs.read:picked`.

</div>

<figure>
<svg viewBox="0 0 400 200" role="img" aria-label="A plugin reaches always-granted imports directly and scoped imports only through the grant check">
<title>Always-granted versus scoped imports</title>
<defs>
<marker id="always-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
<path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
</marker>
</defs>
<rect x="130" y="6" width="140" height="30" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
<text x="200" y="25" font-size="12" text-anchor="middle" fill="currentColor">Plugin</text>
<line x1="170" y1="36" x2="91" y2="78" stroke="currentColor" marker-end="url(#always-arrow)"/>
<line x1="230" y1="36" x2="309" y2="78" stroke="currentColor" marker-end="url(#always-arrow)"/>
<rect x="6" y="78" width="170" height="44" rx="6" fill="none" stroke="currentColor"/>
<text x="91" y="96" font-size="11" text-anchor="middle" fill="currentColor">Always granted</text>
<text x="91" y="112" font-size="10" text-anchor="middle" fill="currentColor">log, clock, storage, codec</text>
<rect x="224" y="78" width="170" height="44" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
<text x="309" y="96" font-size="11" text-anchor="middle" fill="currentColor">Grant check</text>
<text x="309" y="112" font-size="10" text-anchor="middle" fill="currentColor">e.g. fs.read:project</text>
<line x1="91" y1="122" x2="91" y2="150" stroke="currentColor" marker-end="url(#always-arrow)"/>
<line x1="309" y1="122" x2="309" y2="150" stroke="currentColor" marker-end="url(#always-arrow)"/>
<rect x="6" y="152" width="170" height="36" rx="6" fill="none" stroke="currentColor"/>
<text x="91" y="174" font-size="11" text-anchor="middle" fill="currentColor">Call runs within quotas</text>
<rect x="224" y="152" width="170" height="36" rx="6" fill="none" stroke="currentColor"/>
<text x="309" y="174" font-size="11" text-anchor="middle" fill="currentColor">not-permitted without grant</text>
</svg>
<figcaption>Imports in the always-granted set skip the grant check. Every other import needs its own grant.</figcaption>
</figure>

## Worked example in Rust

This server-side plugin keeps a launch count in the project bucket, logs it on activation and
writes a heartbeat every 60 seconds. Its manifest asks for no capabilities:

```json
{
  "server": { "entry": "server.wasm", "world": "ilium:server/base@0.1.0", "exports": [] }
}
```

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::{clock, log, storage, timers};
use ilium_plugin_sdk::host::log::Level;
use ilium_plugin_sdk::host::storage::Bucket;
use ilium_plugin_sdk::types::ids::TimerId;

const LAUNCH_COUNT_KEY: &str = "launch-count";

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct LaunchCounter {
    heartbeat: Option<TimerId>,
}

fn read_launch_count() -> u64 {
    storage::get(Bucket::Project, LAUNCH_COUNT_KEY)
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|text| text.parse().ok())
        .unwrap_or(0)
}

impl Lifecycle for LaunchCounter {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        let count = read_launch_count() + 1;
        storage::put(Bucket::Project, LAUNCH_COUNT_KEY, count.to_string().into_bytes())?;
        log::log(Level::Info, &format!("project opened {count} times"));
        self.heartbeat = Some(timers::set(60_000, Some(60_000))?);
        Ok(())
    }

    fn deactivate(&mut self, _reason: DeactivationReason) -> Option<Vec<u8>> {
        if let Some(timer) = self.heartbeat.take() {
            timers::cancel(timer);
        }
        None
    }
}

impl Handler for LaunchCounter {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            if let Event::TimerFired(_) = event {
                let uptime = clock::monotonic_millis();
                log::log(Level::Debug, &format!("heartbeat at {uptime} ms"));
            }
        }
        Vec::new()
    }
}
```

The `?` operator works on every import that returns `result<_, plugin-error>`: the SDK maps the
error to `PluginError` with its code and message.

## Worked example in TypeScript

This client-side plugin records when the project was last opened in the `global` bucket and logs
the previous value. The manifest is the same shape as the Rust one, with the client side:

```json
{
  "client": { "entry": "client.wasm", "world": "ilium:client/base@0.1.0", "exports": [] }
}
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import type { Event, Submission } from '@ilium/plugin-sdk/types';
import { log } from '@ilium/plugin-sdk/host/log';
import { nowUnixMillis } from '@ilium/plugin-sdk/host/clock';
import { get, put } from '@ilium/plugin-sdk/host/storage';

const LAST_OPENED_KEY = 'last-opened-ms';
const encoder = new TextEncoder();
const decoder = new TextDecoder();

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  activate() {
    const previous = get('global', LAST_OPENED_KEY);
    const previousText = previous === undefined ? 'never' : decoder.decode(previous);
    log('info', `previous open at unix ms: ${previousText}`);
    put('global', LAST_OPENED_KEY, encoder.encode(nowUnixMillis().toString()));
  },
  deactivate() {
    return undefined;
  },
  onEvents(_batch: Event[]): Submission[] {
    return [];
  },
});
```

In TypeScript `put` throws a `PluginError` on failure. `get` has no failure channel (the WIT
return is `option`): it returns `undefined` for a missing key and never throws. `nowUnixMillis()` returns a `bigint`, so the example converts it with `toString()`.

## Limits, errors and edge cases

Always-granted imports never return `not-permitted` for capability reasons. They can still return
the universal codes (`not-permitted` for a path escape, `revoked`, `internal`) and the codes below.

| Import | Condition | Result |
|---|---|---|
| `log` | line over 16 KiB | cut and marked |
| `log` | more than 200 lines per second (1,000 at T1) | dropped and counted |
| `timers.set` | interval or delay under 50 ms | `invalid-argument` |
| `timers.set` | 64 held timers (256 at T1) | `quota-exceeded` |
| `timers.cancel` | unknown id | ignored |
| `storage.get` | missing key | `none` |
| `storage.put` | key empty or over 256 bytes, value over 1 MiB | `invalid-argument` |
| `storage.put` | bucket over 16 MiB (256 MiB at T1) | `quota-exceeded` |
| `storage.write-batch` | more than 256 entries or 4 MiB of key and value bytes | `invalid-argument` |
| `storage.put` in `shared` | bucket over `shared_cache_mib` | oldest entries evicted, no error |
| `settings.set-json` | patch over 256 KiB, or a value that breaks the schema | `invalid-argument` |
| `settings.set-json` | key of the other side | `invalid-argument` |
| `codec` | input over 16 MiB, or corrupt data | `invalid-argument` |
| `codec.inflate` | output over 64 MiB | `invalid-argument` |
| `codec.decode-image` | more than 67,108,864 pixels | `invalid-argument` |
| `fs.read` | synchronous read over 1 MiB | `invalid-argument` |
| `fs.write` | one call over 16 MiB | `invalid-argument` |
| `fs.*` | folder over 256 MiB (`data` and `cache` together) | `failed(quota-exceeded)` |
| `messages` | payload over 1 MiB | `refused(invalid-argument)` |
| `ui.invalidate` | view not declared by the plugin | ignored, one warn line |

Ordinary timers are not persisted. A restart, a disable or an upgrade clears them, so a plugin re-arms its
timers in `activate`. Durable timers (`timers.at`) are the exception: they survive a restart and a deactivation (see [lifecycle](/pages/lifecycle/#durable-timers-and-further-triggers)). Storage survives restarts: the `session` bucket is deleted with its named
session, and the `global` and `project` buckets are kept or deleted on uninstall, as the user
chooses.

Every check that the host can make from the call alone (syntax, size, scope) returns synchronously.
A check that needs the disk, the network or another process is reported later, through a completion.
The `log`, `clock`, `timers`, `storage`, `settings` and `codec` imports are synchronous. The `fs`
functions that touch the disk for a long time follow the completion rule of the file API.

## What the user sees

- Log lines appear in `ilium plugin logs <id>`, one per entry, as `YYYY-MM-DD HH:mm:ss`, the level
  padded to five characters, the plugin id and the message. `--tail <n>` prints the last `n` lines.
- The review dialog for a T2 or T3 package does not list always-granted imports. The `always`
  risk tier is not shown.
- A T1 bundled plugin uses this set without any prompt.
- Storage, settings and fs data are private to the plugin. Nothing in the set is visible to another
  plugin, and the user sees only the values that the plugin's own settings page displays.

<pre class="term">
<span class="t-prompt">$</span> ilium plugin logs launch-counter --tail 4
2026-10-11 10:02:14 info  launch-counter project opened 3 times
2026-10-11 10:03:14 debug launch-counter heartbeat at 61204 ms
2026-10-11 10:04:14 debug launch-counter heartbeat at 121209 ms
2026-10-11 10:05:02 error launch-counter storage.put: quota-exceeded
</pre>

Illustrative, not generated. The plugin logs its launch count, a heartbeat each minute, and one
refusal from `storage.put` when the project bucket is full. The plugin id and the numbers are
placeholders.

## See also

- [tree.read](/pages/capability-tree-read/), the first capability page.
- [fs.read](/pages/capability-fs-read/), the scoped file grant that complements the file scopes in
  this set.
- [log](/pages/api-log/), [clock and timers](/pages/api-clock-and-timers/),
  [storage](/pages/api-storage/), [codec](/pages/api-codec/) and [settings](/pages/api-settings/)
  for the full function reference.
- [Messages and topics](/pages/api-messages-and-topics/) and
  [capability messages.peer](/pages/capability-messages-peer/) for messages between plugins.
- [Panels](/pages/capability-ui-panels/), the capability that declares the views `ui.invalidate`
  redraws.
- [Errors and limits](/pages/reference-errors-and-limits/) for the full table of error codes.

Checked against the API version `ilium:plugin@0.1.0`.

### Security

Always-granted calls are not individually audited. The audit log records security decisions only (installs, grants, consents, trust, policy and sandbox events; see [audit log](/pages/reference-audit-log/)), so storage, settings and `fs` writes, reads, timers, codec calls and log lines do not appear in it. Inference use is in the inference privacy log, which is kernel-owned ([SEC-305]).
