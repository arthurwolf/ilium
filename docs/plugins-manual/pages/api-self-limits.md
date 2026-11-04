---
layout: page
title: Self limits
permalink: /pages/api-self-limits/
---

## Self limits

The `limits` import tells a plugin how much of each budget it has left. A plugin can read the effective value of a budget and the amount that is still free, and the time at which the budget refills. With this a plugin can slow down, batch its work or warn the user before a call would be refused with `quota-exceeded`.

The import only reports. It does not change a limit, and it does not show the budgets of another plugin or the account totals of the user. The host sets every value, and the user can change some of them in Settings. The limits that the host applies to a plugin, with their source, are shown to the user by `ilium plugin limits <id>`, as [Errors and limits](/pages/reference-errors-and-limits/) describes.

**You will need:**

- No grant. `limits` is available to server and client plugins.
- Rust: the `ilium-plugin-sdk` crate, module `ilium_plugin_sdk::host::self_limits`.
- TypeScript: the `@ilium/plugin-sdk` package, subpath `@ilium/plugin-sdk/host/self-limits`.

### Interface

Interface version: `ilium:host/self-limits@0.1.0`.

```wit
// Excerpt of ilium:host/self-limits in reference-wit
interface self-limits {
  record budget {
    id: string,
    unit: string,
    effective: u64,
    remaining: u64,
    resets-at-ms: option<u64>,
  }
  limits: func() -> list<budget>;
}
```

### Functions

| Function | Parameters | Returns | What it does |
|---|---|---|---|
| `limits` | none | list of `budget` | Returns one entry for each budget listed below. The call cannot fail, so it has no error channel. |

The record `budget` has five fields:

| Field | Meaning |
|---|---|
| `id` | The budget id from the table below. |
| `unit` | The unit of `effective` and `remaining`: `mib`, `bytes`, `tokens`, `requests` or `handles`. |
| `effective` | The value the host applies now, after the manifest, the user setting and the admin policy. |
| `remaining` | The amount that is still free in the current window. It is never more than `effective`. |
| `resets-at-ms` | The time at which `remaining` returns to `effective`, as milliseconds since the Unix epoch, or none when the budget does not refill over time. |

### The budgets

| Id | Unit | Refills | What `remaining` means |
|---|---|---|---|
| `inference-tokens` | `tokens` | At 00:00 UTC each day | Daily token allowance left for [inference](/pages/api-inference/). |
| `inference-requests` | `requests` | At 00:00 UTC each day | Daily request allowance left for [inference](/pages/api-inference/). |
| `fs-read-mib` | `mib` | Never | The size limit of `fs.read-all`. It is a cap on one call, so `remaining` equals `effective`. |
| `fs-write-mib` | `mib` | Never | The size limit of one `fs` write call, `fs_write_mib`. It is a cap on one call, so `remaining` equals `effective`. |
| `handles-open` | `handles` | Never | The live handles allowed (files, sockets, subscriptions, timers, child processes), less the handles open now. |
| `memory-bytes` | `bytes` | Never | The memory allowed for the plugin side, less the memory it uses now. |
| `event-queue-bytes` | `bytes` | Never | The event queue memory allowed, less the memory the queue is charged now. |

<div class="note">
<p>A budget that never refills has <code>resets-at-ms</code> set to none. A plugin should not wait for it to reset. It should free the resource that uses it.</p>
</div>

### Example in Rust

The plugin reads its daily request allowance and skips optional work when only a few requests are left.

```rust
use ilium_plugin_sdk::host::self_limits;

fn can_afford_summary() -> bool {
    self_limits::limits()
        .iter()
        .find(|budget| budget.id == "inference-requests")
        .map(|budget| budget.remaining > 10)
        .unwrap_or(false)
}
```

### Example in TypeScript

The same check in TypeScript, with a warning when the daily token allowance is almost spent.

```ts
import { limits } from '@ilium/plugin-sdk/host/self-limits';

export function tokensLeftToday(): number {
  const budget = limits().find((entry) => entry.id === 'inference-tokens');
  return budget === undefined ? 0 : Number(budget.remaining);
}
```

### Edge cases

- **Two reads with no spend between them** return the same values. The host does not change a budget between calls except when the plugin spends from it or the window resets.
- **Another plugin** has its own budgets. `limits` returns only the budgets of the calling side, so a server side does not see the client side's memory, and a plugin does not see another plugin's values.
- **After a spend** the remaining amount falls by the spent amount at once. An admitted inference request counts at the moment the gate admits it, as [inference](/pages/api-inference/) describes.
- **At the reset time** the daily inference budgets return to `effective`, and `resets-at-ms` moves to the next day.
- **A refused call** spends nothing, so the remaining amount does not change.

### What the user sees

The user sees the same budgets in Settings > Inference, where each plugin with a daily budget has a meter. A plugin that reads `limits` shows nothing by itself. Any message to the user is written by the plugin.

### See also

- [inference](/pages/api-inference/) for the daily token and request budgets.
- [Errors and limits](/pages/reference-errors-and-limits/) for the refusal codes and the limits that apply.
- [Telemetry and plugin health](/pages/reference-host-telemetry/) for the memory and handle counters behind `memory-bytes` and `handles-open`.
- [help and plugins](/pages/api-help-and-plugins/) for the other self-description import, [self-describe](/pages/api-self-describe/).

## Requirement statements

- **[API-224]** The host MUST provide a read-only `limits()` import that returns, for the calling plugin side, the effective value and the remaining amount of each budget the plugin can reach (the filesystem read and write caps, the daily inference tokens and requests, live handles, memory bytes and event queue bytes), with the reset time where a budget resets.

Test: read the limits, make one inference request that the gate admits, read the limits again, and assert that `inference-requests` fell by one.

- **[API-225]** `limits()` MUST NOT reveal another plugin's budget or the user's account totals, and it MUST return the same values for two calls with no intervening spend.

Test: read the limits from two plugins and assert that each sees only its own budgets, then read twice with no call in between and assert that the two results are equal.
