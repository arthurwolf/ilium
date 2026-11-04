---
layout: page
title: telemetry
permalink: /pages/api-telemetry/
---

## telemetry

The `telemetry` import lets a plugin add its own counters to the host's statistics. A counter is a named total that the plugin increases with `count`. The host keeps the counter in memory on the plugin's own side and shows it next to the built-in counters, under the prefix `plugin.`. Nothing is sent over the network, and no other plugin can read the counter. The built-in counters and how long they are kept are in [Telemetry and plugin health](/pages/reference-host-telemetry/).

Use a counter for a number that helps you judge how the plugin behaves, such as how many files an import handled. Do not use one for anything about a person. A counter name is written to the stats output, so it must not contain a file name, a prompt or a user name.

**You will need:**

- No grant. `telemetry` is available to server and client plugins and to scene plugins.
- Rust: the `ilium-plugin-sdk` crate, module `ilium_plugin_sdk::host::telemetry`.
- TypeScript: the `@ilium/plugin-sdk` package, subpath `@ilium/plugin-sdk/host/telemetry`.

### Interface

Interface version: `ilium:host/telemetry@0.1.0`.

```wit
// Excerpt of ilium:host/telemetry in reference-wit
interface telemetry {
  count: func(name: string, delta: s64) -> result<_, plugin-error>;
}
```

### Functions

| Function | Parameters | Returns | What it does |
|---|---|---|---|
| `count` | `name: string`, `delta: s64` | nothing, or `plugin-error` | Adds `delta` to the counter called `name`. A new name starts at zero. A negative `delta` lowers the total. |

In Rust the call is `telemetry::count(name, delta)` and returns `Result<(), PluginError>`. In TypeScript the call is `count(name, delta)`, where `delta` is a `bigint`, and a refusal throws `PluginError`.

### Name and quota rules

- A name must match `^[a-z][a-z0-9_]{0,31}$`: a lowercase letter first, then lowercase letters, digits or underscores, at most 32 characters in total.
- A plugin side may use at most 16 distinct names. The count is per side, so the server and the client of one plugin each have their own 16 names.
- Names are kept for the life of the host process. Using an existing name again never uses up a slot.

<div class="warn">
<p>A counter name is never checked for personal data. Keep names to fixed words that describe an event, such as <code>import_files</code>. The rule that no personal data goes into a counter is in <a href="/pages/reference-host-telemetry/">Telemetry and plugin health</a>.</p>
</div>

### Example in Rust

The plugin counts the files it imported. A refused count is written to the plugin log and does not stop the import.

```rust
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::host::telemetry;

fn record_imported_files(file_count: i64) {
    if let Err(error) = telemetry::count("import_files", file_count) {
        log::log(Level::Warn, &format!("counter refused: {}", error.message));
    }
}
```

### Example in TypeScript

The same call in TypeScript. The delta is a `bigint`, so convert the number first.

```ts
import { count } from '@ilium/plugin-sdk/host/telemetry';
import { log } from '@ilium/plugin-sdk/host/log';

export function recordImportedFiles(fileCount: number): void {
  try {
    count('import_files', BigInt(fileCount));
  } catch (error) {
    log('warn', `counter refused: ${String(error)}`);
  }
}
```

After either call, `ilium plugin stats <id> --json` prints the counter as `plugin.import_files`.

### Edge cases and error codes

| Situation | Result |
|---|---|
| Name does not match the pattern, such as `Bad-Name` | `invalid-argument`. The counter is not created. |
| The new total would pass the s64 range | `invalid-argument`. The total stays as it was. |
| A 17th distinct name on one side | `quota-exceeded`. The existing 16 counters are not changed. |
| A `delta` of 0 | Accepted. The counter is created at zero if it did not exist. |
| The plugin restarts | The total is kept. See the restart rule below. |
| The host process restarts | The total starts again at zero. |
| The plugin is uninstalled | Its counters are discarded. A later install of the same id starts at zero. |

### Restart behaviour

A custom counter keeps its total when the plugin restarts, because the counter belongs to the host process and not to the plugin instance. The counter starts again at zero when the host process restarts. Uninstalling the plugin discards every counter, as [Telemetry and plugin health](/pages/reference-host-telemetry/) states. A plugin that needs a total across host restarts must store it with [storage](/pages/api-storage/).

### What the user sees

A counter is not shown on its own. The user sees it in `ilium plugin stats <id>` and in the Plugins screen, Resources tab, where the custom counters are listed with the built-in ones.

### See also

- [Telemetry and plugin health](/pages/reference-host-telemetry/) for the built-in counters and retention.
- [log](/pages/api-log/) for text that a person reads in the plugin log.
- [storage](/pages/api-storage/) for a value that must survive a host restart.
- [Plugin-to-plugin calls](/pages/api-plugin-to-plugin/) for the other ways plugins work together.

## Requirement statements

- **[API-219]** The `telemetry` import MUST provide `count(name: string, delta: s64) -> result<_, plugin-error>`, and a counter MUST appear in the stats output under the `plugin.` prefix.

Test: call `count` from the Rust SDK and from the TypeScript SDK, then assert that `ilium plugin stats <id> --json` lists each counter as `plugin.<name>`.

- **[API-220]** `count` MUST refuse a name that does not match `^[a-z][a-z0-9_]{0,31}$` with `invalid-argument`, MUST refuse a delta that would overflow the running s64 total with `invalid-argument` while leaving the counter unchanged, and MUST refuse a 17th distinct name on one plugin side with `quota-exceeded`.

Test: call `count` with the name `Bad-Name`, with a delta that overflows the total, and with a 17th new name, then assert each refusal and that the existing total is unchanged.

- **[API-221]** A custom counter MUST keep its total across a plugin restart, MUST start again at zero when the host process restarts, and this page MUST state the same rule as [Telemetry and plugin health](/pages/reference-host-telemetry/).

Test: increment a counter, restart the plugin, and assert that the total is kept; restart the host process and assert that the total is zero.
