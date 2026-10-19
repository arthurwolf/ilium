# 16. Remember a counter in storage

## 16. Remember a counter in storage

A plugin starts from nothing every time Ilium loads it. Anything it kept in a variable is gone. This tutorial shows how to keep a number across restarts. The plugin has a command that adds one to a counter and shows the new value in a toast. Close Ilium, open it again, run the command, and the count goes on from where it stopped.

This tutorial is standalone. It starts a fresh plugin, so you do not need the earlier tutorials. You only need to have finished [Your first plugin in five minutes](/pages/getting-started-first-plugin/).

Some words first.

- **Storage** is a small key-value store that the host gives to every plugin. A **key** is a name, such as `count`. A **value** is a block of bytes saved under that name.
- A **bucket** is a separate drawer of storage. Ilium has three: `global`, `project` and `session`.
- A **command** is a named action that the user runs from the palette.
- A **handler** is the function of the plugin that receives events.
- An **intent** is a request the plugin hands back, such as "show this toast".
- A **grant** is a permission that the user allows at install.
- **Little-endian** is a way to write a number as bytes, lowest byte first. The number 1 as eight bytes is `01 00 00 00 00 00 00 00`.

Tutorial plugin: `example.counter`. Side: client. Languages: Rust (target `wasm32-wasip2`) and TypeScript. Every listing is given in both.

### You will need

- [Your first plugin in five minutes](/pages/getting-started-first-plugin/) finished. Nothing else.
- One grant, on the client side: `notify.toast`, for the toast. Storage needs no grant. It is always on, so the manifest does not list it.
- Create the project:

```bash
ilium plugin new counter --id example.counter --lang rust --side client --name "Counter"
```

Use `--lang typescript` for the TypeScript version.

### What this plugin does

The command `Count one more` reads the number saved under the key `count`, adds one, saves the result, and shows `Counted 3 times`. A second command, `Reset the count`, removes the key.

### The surface this step uses

| Name | Kind | Fields and types | Meaning |
|---|---|---|---|
| `storage.get` | host function | `bucket`, `key: string` returns bytes or nothing | Reads a value. A missing key is not an error. |
| `storage.put` | host function | `bucket`, `key`, `value: bytes` | Saves a value. Atomic. May fail with `invalid-argument` or `quota-exceeded`. |
| `storage.remove` | host function | `bucket`, `key` | Deletes a key. Deleting a missing key does nothing. |
| `storage.keys` | host function | `bucket`, `prefix` returns list of keys | Lists keys that start with the prefix. |
| `command-invoked` | event | `command`, `arguments-json`, `origin`, `context` | The user ran a command. |
| `toast` | intent | `id`, `tone`, `title`, `body`, `actions`, `duration-ms` | Shows a toast. Needs `notify.toast`. |

### The three buckets

| Bucket | Shared with | Lives until |
|---|---|---|
| `global` | Every project on this machine | The plugin data is deleted |
| `project` | Every session of the same project directory | The project data is deleted |
| `session` | The one named session | That session is deleted |

This tutorial uses `project`. A counter that belongs to a project should not leak into the next project.

### Rules to know before you write

- A key is a non-empty string of at most 256 bytes.
- A value is at most 1 MiB. An empty value is allowed.
- Each bucket holds 16 MiB by default.
- A `put` replaces the whole value or changes nothing. A failed `put` leaves the old value.
- `get` of a missing key returns nothing. It is not an error.

### Manifest

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "example.counter",
  "name": "Counter",
  "version": "0.1.0",
  "publisher": "Example Developer",
  "client": {
    "entry": "client.wasm",
    "world": "ilium:client/base@0.1.0",
    "exports": []
  },
  "capabilities": {
    "client": ["notify.toast"]
  },
  "contributes": {
    "commands": [
      { "id": "count", "title": "Count one more", "category": "Counter" },
      { "id": "reset", "title": "Reset the count", "category": "Counter" }
    ]
  }
}
```

### The smallest plugin

Storage holds bytes, so the plugin turns the number into eight bytes and back. It writes the number as a 64-bit unsigned integer, little-endian. The pure functions `decode_count` and `encode_count` do that. They need no host, so a plain test can check them.

**Rust**

```rust
// src/lib.rs. A counter that survives restarts.
use ilium_plugin_sdk::host::storage::{self, Bucket};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::CommandInvokedEvent;
use ilium_plugin_sdk::types::intents::{Intent, ToastSpec};
use ilium_plugin_sdk::types::text::Tone;

const COUNT: &str = "example.counter/count";
const KEY: &str = "count";

#[ilium_plugin(side = "client")]
#[derive(Default)]
struct Counter;

impl Lifecycle for Counter {}

/// Eight bytes, little-endian, to a number.
pub fn decode_count(bytes: &[u8]) -> u64 {
    match <[u8; 8]>::try_from(bytes) {
        Ok(array) => u64::from_le_bytes(array),
        Err(_) => 0,
    }
}

pub fn encode_count(count: u64) -> [u8; 8] {
    count.to_le_bytes()
}

fn toast(title: String) -> Submission {
    Submission::new(Intent::Toast(ToastSpec {
        id: Some("counter".into()),
        tone: Tone::Info,
        title,
        body: None,
        actions: vec![],
        duration_ms: None,
    }))
}

impl Handler for Counter {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            let Event::CommandInvoked(CommandInvokedEvent { command, .. }) = event else {
                continue;
            };
            if command == COUNT {
                let current = storage::get(Bucket::Project, KEY)
                    .map(|bytes| decode_count(&bytes))
                    .unwrap_or(0);
                let next = current + 1;
                let _ = storage::put(Bucket::Project, KEY, &encode_count(next));
                submissions.push(toast(format!("Counted {next} times")));
            }
        }
        submissions
    }
}
```

**TypeScript**

```ts
// src/client.ts. A counter that survives restarts.
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import * as storage from '@ilium/plugin-sdk/host/storage';

const COUNT = 'example.counter/count';
const KEY = 'count';
let nextCorrelation = 1n;

/** Eight bytes, little-endian, to a number. */
export function decodeCount(bytes: Uint8Array | undefined): bigint {
  if (bytes === undefined || bytes.length !== 8) return 0n;
  return new DataView(bytes.buffer, bytes.byteOffset, 8).getBigUint64(0, true);
}

export function encodeCount(count: bigint): Uint8Array {
  const bytes = new Uint8Array(8);
  new DataView(bytes.buffer).setBigUint64(0, count, true);
  return bytes;
}

function toast(title: string): Submission {
  return {
    intent: {
      tag: 'toast',
      val: { id: 'counter', tone: 'info', title, body: undefined, actions: [], durationMs: undefined },
    },
    correlation: nextCorrelation++,
    dedupKey: undefined,
  };
}

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    for (const event of batch) {
      if (event.tag !== 'command-invoked') continue;
      if (event.val.command === COUNT) {
        const next = decodeCount(storage.get('project', KEY)) + 1n;
        storage.put('project', KEY, encodeCount(next));
        submissions.push(toast(`Counted ${next} times`));
      }
    }
    return submissions;
  },
});
```

### Line by line

1. `storage::get` returns the saved bytes, or nothing on the first run.
2. `decode_count` turns eight bytes into a number. Any other length returns 0. That makes a damaged value harmless.
3. The plugin adds one and calls `storage::put`. The new value replaces the old one in one step.
4. The toast id `counter` makes a quick second press replace the first toast.

The Rust version throws away the result of `put` with `let _`. That is the flaw the next section fixes.

### One improvement: handle failure, and add a reset

Two things can go wrong.

- `put` can fail. If the bucket is full, the code is `quota-exceeded`. If the host refuses the call, the code is `invalid-argument`. The smallest version would show `Counted 4 times` even though nothing was saved.
- The saved value can be damaged by another version of the plugin. A value that is not eight bytes is treated as corrupt and starts again at 0.

The fix shows the truth in the toast, and writes a line to the plugin log so the failure can be found later. The log is read with `ilium plugin logs example.counter`.

**Rust**

```rust
use ilium_plugin_sdk::host::log::{self, Level};

const RESET: &str = "example.counter/reset";

/// Adds one and saves. Returns the new count, or a short reason.
fn bump() -> Result<u64, String> {
    let current = storage::get(Bucket::Project, KEY)
        .map(|bytes| decode_count(&bytes))
        .unwrap_or(0);
    let next = current + 1;
    storage::put(Bucket::Project, KEY, &encode_count(next)).map_err(|error| {
        log::log(Level::Warn, &format!("counter save failed: {error:?}"));
        "could not save the count".to_string()
    })?;
    Ok(next)
}

// In on_events, replace the body of `if command == COUNT` with:
//     let title = match bump() {
//         Ok(next) => format!("Counted {next} times"),
//         Err(reason) => format!("Not counted: {reason}"),
//     };
//     submissions.push(toast(title));
// and add a second branch:
//     if command == RESET {
//         storage::remove(Bucket::Project, KEY);
//         submissions.push(toast("Count reset".into()));
//     }
```

**TypeScript**

```ts
import { log } from '@ilium/plugin-sdk/host/log';

const RESET = 'example.counter/reset';

/** Adds one and saves. Returns the new count, or a short reason. */
function bump(): { ok: true; next: bigint } | { ok: false; reason: string } {
  const next = decodeCount(storage.get('project', KEY)) + 1n;
  try {
    storage.put('project', KEY, encodeCount(next));
  } catch (error) {
    log('warn', `counter save failed: ${String(error)}`);
    return { ok: false, reason: 'could not save the count' };
  }
  return { ok: true, next };
}

// In onEvents, replace the body of the COUNT branch with:
//     const result = bump();
//     submissions.push(toast(result.ok ? `Counted ${result.next} times`
//                                      : `Not counted: ${result.reason}`));
// and add a second branch:
//     if (event.val.command === RESET) {
//       storage.remove('project', KEY);
//       submissions.push(toast('Count reset'));
//     }
```

In TypeScript, `put` throws on failure, so the code uses `try` and `catch`. In Rust, `put` returns a `Result`, so the code uses `map_err`.

### Listing what you saved

`storage::keys(bucket, prefix)` lists the keys that start with a prefix, sorted by bytes. An empty prefix lists them all. Use a prefix to group keys. For a plugin that counts several things, name the keys `count/errors` and `count/warnings`, and list `count/` to see them.

### Points to notice

- **Storage is bytes.** Choose an encoding and keep it. Eight bytes little-endian is one good choice. JSON text is another.
- **A missing key is normal.** The first run has no value, and `get` returns nothing.
- **Read, change, write.** The handler runs one batch at a time, so two presses cannot interleave inside one plugin instance.
- **The bucket sets the reach.** A `project` counter is separate in every project directory.
- **Uninstall asks.** When the user removes the plugin, Ilium asks whether to keep or delete its data.

### Edge cases and errors

| Case | What happens | Code or result |
|---|---|---|
| First run, no key | `get` returns nothing, the count starts at 0 | No error |
| Saved value is not 8 bytes | The plugin treats it as corrupt and starts at 0 | No error |
| Bucket is full | `put` fails and keeps the old value | `quota-exceeded` |
| Key empty or over 256 bytes | `put` fails | `invalid-argument` |
| Value over 1 MiB | `put` fails | `invalid-argument` |
| Reset when no key | Nothing to remove | No error |
| Two projects open | Each has its own count | No error |
| `notify.toast` declined | The toast is refused | `refused(not-permitted)` |

### Common mistakes

| Mistake | What you see | Fix |
|---|---|---|
| Used the `global` bucket for a per-project counter | The count is shared by all projects | Use `project`. |
| Listed `storage` under `capabilities` | The install is refused with `manifest.grant-unknown` | Delete it. Storage needs no grant. |
| Wrote the number as text, read it as 8 bytes | The count resets to 0 every time | Use one encoding on both sides. |
| Ignored the result of `put` | A toast claims a save that did not happen | Check the result and tell the user. |
| Kept the count in a variable only | The count restarts at 0 after a restart | Save it with `put`. |

### Test it

The two byte functions are pure. Test them with plain tests. Test the whole command with the fake host, which keeps a storage of its own.

**Rust**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ilium_plugin_sdk::testing::FakeHost;

    fn command(id: &str) -> Event {
        Event::CommandInvoked(CommandInvokedEvent {
            command: id.into(),
            arguments_json: "{}".into(),
            origin: "palette".into(),
            context: Default::default(),
        })
    }

    #[test]
    fn bytes_round_trip() {
        assert_eq!(decode_count(&encode_count(41)), 41);
        assert_eq!(decode_count(&[1, 2, 3]), 0);
    }

    #[test]
    fn the_count_goes_up_between_calls() {
        let mut host = FakeHost::new().grant("notify.toast");
        host.activate::<Counter>();
        host.deliver(vec![command(COUNT)]);
        host.deliver(vec![command(COUNT)]);
        // With the testing feature, storage::get reads the storage of the fake host of this test.
        let stored = storage::get(Bucket::Project, KEY).expect("saved");
        assert_eq!(decode_count(&stored), 2);
    }
}
```

**TypeScript**

```ts
// src/client.test.ts. Run with: bun test
import { expect, test } from 'bun:test';
import { createFakeHost } from '@ilium/plugin-sdk/testing';
import * as storage from '@ilium/plugin-sdk/host/storage';
import { decodeCount, encodeCount, handler, lifecycle } from './client';

test('bytes round trip', () => {
  expect(decodeCount(encodeCount(41n))).toBe(41n);
  expect(decodeCount(new Uint8Array([1, 2, 3]))).toBe(0n);
});

test('the count goes up between calls', async () => {
  const host = createFakeHost().grant('notify.toast');
  await host.activate({ lifecycle, handler });
  const press = { tag: 'command-invoked' as const, val: { command: 'example.counter/count',
    argumentsJson: '{}', origin: 'palette', context: {} } };
  host.deliver([press]);
  host.deliver([press]);
  expect(decodeCount(storage.get('project', 'count'))).toBe(2n);
});
```

Run `cargo test` or `bun test`. Then run the plugin live with `ilium plugin dev`, press the command twice, restart Ilium, and press it again. The toast should say `Counted 3 times`.

### What you see

The palette lists the two commands under their category.

<figure>
<pre class="term">╭  ≡ ● · Command palette ──────────────────────╮
│ <span class="t-prompt">&gt;</span> count                                      │
│                                              │
│ <span class="t-dim">Counter</span>                                      │
│ <span class="t-hi">Count one more</span>                               │
│ Reset the count                              │
│                                              │
│ <span class="t-dim">Enter run · Esc close</span>                        │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The two commands of the plugin, grouped under the category Counter.</figcaption>
</figure>

After the third press, across a restart, the toast carries the saved value.

<figure>
<pre class="term">╭  ≡ ● · Counted 3 times ──────────────────────╮
│ ━━━━━━━━━━━━━━━━━━━━━━━━━━━──────────────────│
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The count continues after a restart because it lives in project storage.</figcaption>
</figure>

### Try it yourself

1. Change the bucket to `session`. Start a second session and see that its count starts at 0.
2. Add a second key `last` that stores the time of the last press, and list both with `storage::keys`.
3. Make `Reset the count` ask first by sending a toast with two actions.

### See also

- [Storage API](/pages/api-storage/) has the buckets, limits, error codes and the quota setting.
- [Manifest reference](/pages/reference-manifest/) lists the command fields and `limits.storage_bucket_mib`.
- [Commands API](/pages/api-commands/) explains how a command reaches the handler.
- [Toast API](/pages/api-toast/) has every toast field and its limits.
- [Testing](/pages/testing/) shows the fake host.
- [Tutorial 14](/pages/tutorial-14-toast-and-status/) shows a toast on an event.

API checked against `ilium:plugin@0.1.0`.

**Next:** [Tutorial 17, a help topic and a custom sound](/pages/tutorial-17-help-and-sound/).
