---
layout: page
title: Reference index
permalink: /pages/reference/
---

## Reference index

The reference pages list exact names, limits and codes. Use them when you need a value, not an explanation. The concept pages explain why things work the way they do.

This page shows which reference page answers which question, the values you look up most often, and how one name is spelled in each SDK. Every value here is a decided value. The four reference pages hold the complete tables.

<figure>
<svg viewBox="0 0 420 200" role="img" aria-label="The reference index links to four reference pages">
  <title>Reference index and its four pages</title>
  <defs>
    <marker id="reference-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
      <path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="8" y="80" width="120" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="68" y="104" font-size="11" text-anchor="middle" fill="currentColor">Reference index</text>
  <line x1="128" y1="100" x2="178" y2="26" stroke="currentColor" marker-end="url(#reference-arrow)"/>
  <line x1="128" y1="100" x2="178" y2="68" stroke="currentColor" marker-end="url(#reference-arrow)"/>
  <line x1="128" y1="100" x2="178" y2="110" stroke="currentColor" marker-end="url(#reference-arrow)"/>
  <line x1="128" y1="100" x2="178" y2="152" stroke="currentColor" marker-end="url(#reference-arrow)"/>
  <rect x="180" y="12" width="220" height="28" rx="6" fill="none" stroke="currentColor"/>
  <text x="290" y="30" font-size="11" text-anchor="middle" fill="currentColor">Manifest keys: every key</text>
  <rect x="180" y="54" width="220" height="28" rx="6" fill="none" stroke="currentColor"/>
  <text x="290" y="72" font-size="11" text-anchor="middle" fill="currentColor">Capabilities: grants and tiers</text>
  <rect x="180" y="96" width="220" height="28" rx="6" fill="none" stroke="currentColor"/>
  <text x="290" y="114" font-size="11" text-anchor="middle" fill="currentColor">Events and intents: payloads</text>
  <rect x="180" y="138" width="220" height="28" rx="6" fill="none" stroke="currentColor"/>
  <text x="290" y="156" font-size="11" text-anchor="middle" fill="currentColor">Errors and limits: codes, caps</text>
  <text x="210" y="190" font-size="10" text-anchor="middle" fill="currentColor">Four pages, one per kind of value</text>
</svg>
<figcaption>The reference index points to four pages, one for each kind of value a plugin looks up.</figcaption>
</figure>

## Pages

| Page | Lists | Use it when |
| --- | --- | --- |
| [Manifest keys](/pages/reference-manifest/) | Every key in `manifest.json`, its type, its default and the value it accepts. | You write or check the manifest. |
| [Capabilities](/pages/reference-capabilities/) | Every grant string, its argument form and its risk tier. | You add a grant to `capabilities`. |
| [Events and intents](/pages/reference-events-and-intents/) | Every event case and intent case, its payload and its outcomes. | You write `on-events` or build submissions. |
| [Errors and limits](/pages/reference-errors-and-limits/) | The error codes, the default resource limits, the per-call caps and the rate rules. | You handle a failed call or size a value. |
| [Documentation lifecycle](/pages/reference-documentation-lifecycle/) | Who keeps the glossary, error catalogue, troubleshooting, FAQ and feature traceability pages true, and when they are reviewed. | You change a term, code or feature and need to know who updates the manual. |

## What you need

- The plugin API version `ilium:plugin@0.1.0`. The manifest key `api` must name the version the plugin was built against, and the component must export the same version. See [Versioning](/pages/versioning/).
- The Rust crate `ilium-plugin-sdk` or the npm package `@ilium/plugin-sdk`, at the versions pinned by the SDK release. Rust stable 1.90 or later and Bun 1.2 or later are required for the build.
- The `ilium plugin` subcommand of the `ilium` binary. It runs every check named on these pages. No other tool is needed.

## Quick lookup

| Need | Value | Source |
| --- | --- | --- |
| Grant string | `name` or `name:argument`. The first colon splits them; later colons belong to the argument. | [Capability table](/pages/reference-capabilities/) |
| Grant risk tiers | `always`, `normal` and `high`. A `high` grant is unchecked by default at install. | [Capability table](/pages/reference-capabilities/) |
| Memory limit | Default 32 MiB. Ceiling 256 MiB for a bundled plugin, 128 MiB for other tiers. | [Resource limits](/pages/reference-errors-and-limits/#resource-limits-in-the-manifest) |
| Restart back-off | 1 s after the first failure, 5 s after the second, disabled at the third within 10 minutes. | [Restart and disable rule](/pages/reference-errors-and-limits/#restart-and-disable-rule) |
| Disable rule | The third failure inside 10 minutes disables the plugin on that side. | [Restart and disable rule](/pages/reference-errors-and-limits/#restart-and-disable-rule) |
| Submission dedup key | 1 to 128 characters from `[A-Za-z0-9._:/-]`. Window of 10 minutes. | [Dedup keys](/pages/reference-events-and-intents/#dedup-keys) |
| Submissions per return | 256 per `on-events` call. | [Fixed caps](/pages/reference-errors-and-limits/#requestable-limits) |
| Pending requests | 64 per plugin by default. Over that, `busy`. | [Resource limits](/pages/reference-errors-and-limits/#resource-limits-in-the-manifest) |
| Storage | Key up to 256 bytes. Value up to 1 MiB. Bucket 16 MiB by default. | [Resource limits](/pages/reference-errors-and-limits/#resource-limits-in-the-manifest) |
| Log line | Up to 16 KiB; longer lines are cut and end with `…[truncated]`. | [Fixed caps](/pages/reference-errors-and-limits/#requestable-limits) |
| Command reply | 30 s. Inference reply: 120 s. | [Error codes and limits](/pages/reference-errors-and-limits/#requestable-limits) |
| Rate of `notify` | 6 per minute per plugin. | [Error codes and limits](/pages/reference-errors-and-limits/#requestable-limits) |
| Rate of `play-sound` | 4 per second per plugin. | [Error codes and limits](/pages/reference-errors-and-limits/#requestable-limits) |
| Rate of `topics.publish` | 100 per second per plugin. | [Error codes and limits](/pages/reference-errors-and-limits/#requestable-limits) |
| Rate of `toast` | 3 per 10 s per plugin. | [Error codes and limits](/pages/reference-errors-and-limits/#requestable-limits) |
| Rate of `geocode` | 1 per second per plugin. | [Error codes and limits](/pages/reference-errors-and-limits/#requestable-limits) |
| Status segment and frame slot update | 10 per second per slot. | [Error codes and limits](/pages/reference-errors-and-limits/#requestable-limits) |
| Contribution caps | `tree_toolbar` 4, `row_actions` 4, `tree_orderings` 4, `status_segments` 2, `pane_frame_slots` 4, `display_modes` 2, `catalogs` 8, `client_requests` 16, `languages` 8. | Manifest keys |
| Events and intents | 18 extended events, 23 extended intents, `ok-epoch`, 17 ui-action kinds, 12 action-value cases. | Events and intents |
| Settings rows | 18 row types. Six new: `location`, `preview`, `notice`, `map`, `multi-choice`, `meter`. | Manifest keys |
| Manifest error codes | 13 new codes, `manifest.catalog-unknown` to `manifest.scene-prerender`. | Errors and limits |

## Error codes

Every host call that can fail returns `plugin-error { code, message, retry-after-ms }`. The table lists the 15 codes.

| Code | Meaning | Retry |
| --- | --- | --- |
| `not-permitted` | The grant is missing, the user declined it, the argument is outside the grant, the path escapes its scope, or a policy forbids the action. | No |
| `not-found` | The named node, pane, buffer, file, command, plugin, provider, call, sound or device does not exist. | No |
| `invalid-argument` | The argument breaks a rule the plugin can check before the call: syntax, range, a size cap or an undeclared id. | No; fix the call |
| `conflict` | State changed or is held: a stale revision or epoch, a manual title, a held lock, or an existing file for `create-new`. | Yes, after reading state again |
| `quota-exceeded` | A stored or held quota is full: storage, disk, metadata, timers, watches, grabs, filters, captures or the CPU budget. | After freeing a resource, or after `retry-after-ms` for the CPU budget |
| `rate-limited` | A rate rule throttles the call. `retry-after-ms` is always set. | Yes, after `retry-after-ms` |
| `busy` | Transient capacity is full: pending requests, the job queue, an open picker or an exclusive device. | Yes, later |
| `timed-out` | A deadline passed: a lock wait, HTTP, a process, job time, or a service or command reply. | Yes |
| `cancelled` | The plugin, the user or a shutdown cancelled the request, or another submission of the batch was refused. | Maybe |
| `unsupported` | The host or platform cannot do it: a format, codec, channel count or missing action, or no clipboard. | No |
| `pane-not-ready` | A restored terminal pane has not started its process yet. | Yes, after `pane-created` or `tree-changed` |
| `revoked` | The authorization epoch changed during the call. The plugin is re-activated. | No |
| `io` | An operating system I/O error other than a missing file. | Maybe |
| `network` | DNS, connect, TLS or transport failure. | Yes |
| `internal` | A host bug, or a trap inside a job instance. | No |

`not-permitted`, `revoked` and `internal` can come back from every fallible function. Each function's own list of further codes is on the errors and limits page.

## Names in each SDK

The WIT name is the source. The SDKs derive the other spellings by fixed rules.

| WIT form | Rust | TypeScript |
| --- | --- | --- |
| Error code `rate-limited` | `ErrorCode::RateLimited` | `'rate-limited'` |
| Field `retry-after-ms` | `retry_after_ms` | `retryAfterMs` |
| Function `read-all` in `fs` | `fs::read_all(path).await` | `readAll(path)` from `@ilium/plugin-sdk/host/fs` |
| `u64` or `s64` | `u64` or `i64` | `bigint`, always |
| `option<T>` | `Option<T>` | `T` or `undefined` |
| Errors from a fallible call | `Result<T, PluginError>` | returns `T`; throws `PluginError` |

## Worked example: Rust

This function reads a notes file. A file that does not exist yet counts as empty. Every other error goes to the caller. The code uses the `not-found` completion of `fs.read-all`.

**Rust**

```rust
use ilium_plugin_sdk::host::fs;
use ilium_plugin_sdk::prelude::*;

/// Reads a notes file. A file that does not exist yet counts as empty.
async fn read_notes(path: FsPath) -> Result<Option<Vec<u8>>, PluginError> {
    match fs::read_all(&path).await {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.code == ErrorCode::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}
```

The prelude brings in `FsPath`, `PluginError` and `ErrorCode`. The caller decides what `None` means. A notes plugin, for example, writes an empty document on its first save.

## Worked example: TypeScript

This module turns a `rate-limited` error into a wait time. Any call that the host may throttle can use it, such as `topics.publish`. The caller schedules the next attempt with a one-shot timer from `@ilium/plugin-sdk/host/timers`.

**TypeScript**

```ts
import { PluginError } from '@ilium/plugin-sdk';

/**
 * Returns how many milliseconds to wait before retrying a failed call,
 * or undefined when waiting cannot help.
 */
export function delayBeforeRetry(error: unknown): number | undefined {
  if (!(error instanceof PluginError)) return undefined;
  if (error.code !== 'rate-limited') return undefined;
  return error.retryAfterMs;
}
```

A `PluginError` carries `code`, `message` and `retryAfterMs`. The function returns `undefined` for every other code, so the caller rethrows it.

## Where the extended items are listed

The four reference pages list the extended items in their own sections: [manifest keys](/pages/reference-manifest/#chrome-and-contribution-keys), [events and intents](/pages/reference-events-and-intents/), [errors and limits](/pages/reference-errors-and-limits/#requestable-limits) and [capabilities](/pages/reference-capabilities/). Twenty-four grants are listed there, and twelve of them are high risk. The settings row table lives in the [settings schema reference](/pages/reference-settings-schema/).

The render shows the lookup the CLI offers for a key. Illustrative, not generated.

<pre class="term">╭  ≡ ● · Plugin limits──────────────────────────╮
│ status_segments …… 2   row_actions …… 4       │
│ tree_toolbar ……… 4   tree_orderings … 4       │
│ pane_frame_slots … 4   display_modes … 2      │
╰────────────────────────────────────────────── ╯
</pre>

The caps of the contribution keys as a table render. Illustrative, not generated.

## Edge cases and errors

- A grant with a wildcard in the wrong place is refused at install with `manifest.grant-wildcard`. `commands.invoke:com.example.notes/*` is valid. `commands.invoke:*/notes` is not.
- A manifest limit above the tier ceiling is clamped and logged. It is never refused.
- A trap ends the plugin instance and is not an error code. Deadline breaches, memory over the limit and more than 256 submissions are traps.
- A dedup key with a character outside the allowed set is a malformed submission. It refuses the whole batch.
- A duplicate submission completes as `duplicate(request-ref)`. It is not an error and has no effect.
- A value over a per-call cap that the plugin can know in advance returns `invalid-argument` at once, and nothing is written. Two exceptions do not fail: `log.log` cuts the line and `terminal.read-screen` clamps its row count.

## What the user sees

- The install dialog lists the grants by risk tier. A `high` grant is listed and unchecked; the user must check it.
- Refusals and traps are logged with the plugin id, the interface and the function. `ilium plugin logs` shows them (see [Debugging](/pages/debugging/)).
- A keybinding conflict makes the losing binding inactive and lists it in Settings > Keys.
- A plugin disabled by the third failure stays disabled until the user enables it again. A manual enable resets the failure count.

## Check output

Run `ilium plugin check` on a package before you install it. The check names the key, the problem and the page that lists the correct value.

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin check demo.iliumpkg
<span class="t-err">err</span>  manifest.grant-argument-missing
     /capabilities/server/0 commands.invoke
     fix: add an argument, for example
     commands.invoke:com.example.notes/*
1 error, 0 warnings
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The text form of check. A failed grant is an error with exit code 3. See <a href="/pages/reference-check-and-lint-rules/">Check and lint rules</a>.</figcaption>
</figure>

Requirement ids for this page: [API-073]. See the [requirements index](/pages/reference-requirements/).

## See also

- [Manifest keys](/pages/reference-manifest/)
- [Capabilities](/pages/reference-capabilities/)
- [Event and intent tables](/pages/reference-events-and-intents/)
- [Error codes and limits](/pages/reference-errors-and-limits/)
- [Capability model](/pages/capabilities/)
- [Errors and limits concept](/pages/reference-errors-and-limits/)
- [Events and intents concept](/pages/events-and-intents/)
- [Lifecycle](/pages/lifecycle/)
- [Rust SDK](/pages/rust-sdk/)
- [TypeScript SDK](/pages/typescript-sdk/)
- [Glossary](/pages/glossary/)
- [Documentation lifecycle](/pages/reference-documentation-lifecycle/)
