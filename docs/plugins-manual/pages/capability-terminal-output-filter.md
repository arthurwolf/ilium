---
layout: page
title: "terminal.output-filter"
permalink: /pages/capability-terminal-output-filter/
---

The `terminal.output-filter` grant lets a server-side plugin watch the output of terminal panes for lines that match a regular expression. The host matches the lines itself, so the plugin never sees the full output stream. For each match the plugin receives one `output-matched` event. A rate limit on each filter stops a noisy pane from flooding the plugin: matches above the limit are dropped and counted.

<figure>
<svg viewBox="0 0 320 200" role="img" aria-label="Terminal output passes through the host filter, which sends output-matched events to the plugin">
  <defs><marker id="arr-output-filter" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto"><path d="M0,0 L10,5 L0,10 z" fill="currentColor"/></marker></defs>
  <rect x="50" y="10" width="220" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="160" y="36" text-anchor="middle" fill="currentColor" font-size="12">Terminal pane output</text>
  <line x1="110" y1="54" x2="110" y2="78" stroke="currentColor" marker-end="url(#arr-output-filter)"/>
  <text x="124" y="70" fill="currentColor" font-size="11">output lines</text>
  <rect x="50" y="78" width="220" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="160" y="104" text-anchor="middle" fill="currentColor" font-size="12">Host filter (regex, rate limit)</text>
  <line x1="110" y1="122" x2="110" y2="146" stroke="currentColor" marker-end="url(#arr-output-filter)"/>
  <text x="124" y="138" fill="currentColor" font-size="11">output-matched</text>
  <rect x="50" y="146" width="220" height="44" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="160" y="172" text-anchor="middle" fill="currentColor" font-size="12">Server plugin</text>
</svg>
<figcaption>The host matches each completed output line against the plugin's filters. The rate limit applies before delivery, so only accepted matches reach the plugin as output-matched events.</figcaption>
</figure>

## You will need

- The grant `terminal.output-filter`, on the server side only. The grant is per side: a client-side plugin cannot use it.
- A server-side plugin. Rust plugins depend on the crate `ilium-plugin-sdk` and TypeScript plugins on `@ilium/plugin-sdk`. See [Toolchain](/pages/getting-started-toolchain/) for the build setup.
- Either a `output_filters` entry in the manifest, for filters known at install time, or a runtime call to `add-output-filter`, for rules that the user switches on. Runtime filters need the grant but no manifest entry.

## Grant syntax

- Valid: `terminal.output-filter`.
- Invalid: `terminal.output-filter:agents`. The pane selection is a field of the filter, not of the grant.

## Surface

### Manifest entry

Each entry in `contributes.output_filters` is installed when the plugin starts.

| Field | Type | Required | Default | Meaning |
|---|---|---|---|---|
| `id` | string | yes | none | Filter id, unique within the plugin. It appears in `output-matched` events. |
| `regex` | string | yes | none | Pattern, see [Matching](#matching). Up to 512 bytes (the manifest schema `maxLength`). |
| `panes` | `"all"`, `"agents"` or `"terminals"` | yes | none | Which panes the filter watches. |
| `rate_per_second` | integer, 1 to 100 | no | 10 | Accepted matches per second, per pane. |

### Runtime calls

Both calls are synchronous and live in the `ilium:primitives/terminal` interface.

```wit
// Excerpt of ilium:primitives/terminal in reference-wit
record output-filter { id: string, regex: string, rate-per-second: u16 }
add-output-filter: func(pane: node-id, filter: output-filter) -> result<_, plugin-error>;
remove-output-filter: func(pane: node-id, filter-id: string);
```

| WIT | Rust (`ilium_plugin_sdk::primitives::terminal`) | TypeScript (`@ilium/plugin-sdk/primitives/terminal`) | Meaning |
|---|---|---|---|
| `add-output-filter` | `add_output_filter(pane, OutputFilter)` returns `Result<(), PluginError>` | `addOutputFilter(pane, filter)` returns `void`, throws `PluginError` | Adds a rule for one pane. An existing id on that pane is replaced. |
| `remove-output-filter` | `remove_output_filter(pane, filter_id)` | `removeOutputFilter(pane, filterId)` | Removes a rule by id. No result, no error code. |
| `output-filter` record | struct `OutputFilter { id, regex, rate_per_second }` | `{ id, regex, ratePerSecond }` | The filter. `rate-per-second` is `u16`. |

The pane is a `NodeId` in Rust and a `bigint` in TypeScript, because node ids are `u64`.

### Event

| WIT field | Rust (`OutputMatchedEvent`) | TypeScript (`val` of `output-matched`) | Type | Meaning |
|---|---|---|---|---|
| `node` | `node` | `node` | `u64` | The pane that printed the line. |
| `filter-id` | `filter_id` | `filterId` | string | The id of the filter that matched. |
| `line` | `line` | `line` | string | The completed line, with escape sequences removed. |
| `row` | `row` | `row` | `u32` | Zero-based line number in the pane's output since it started. |

The event arrives in `on_events` (Rust) or `onEvents` (TypeScript) as `Event::OutputMatched(...)` or `{ tag: 'output-matched', val: ... }`.

## Matching

- The host matches each completed line, after escape sequences are removed, so colour codes do not hide a match. A partial line that has not received its line end is not matched yet.
- The dialect is the Rust `regex` crate. It has no look-around and no back-references. A pattern matches anywhere in the line; use `^` and `$` to anchor it.
- One line can match several filters. Each matching filter raises its own event.
- A pane selection of `all` means every terminal pane. `agents` means panes whose agent is identified, and `terminals` means terminal panes with no identified agent.

## Rate limit

Each filter instance on each pane has its own limit of `rate_per_second` accepted matches. A match above the limit is dropped and counted, not delivered. The next batch that the plugin receives starts with one `events-dropped` event. It carries a list of `{ event, source, count }` records, where `event` is `output-matched` and `source` is the filter id. The `events-dropped` event itself is never dropped.

## Limits

| Limit | Default | Ceiling T1 | Ceiling T2 and T3 | Over the limit |
|---|---|---|---|---|
| Filters per plugin | 32 | 128 | 32 | `quota-exceeded` |
| Filters across all plugins | n/a | 512 | 512 | `quota-exceeded` |
| Queued `output-matched` events per plugin | 1,024 | 4,096 | 1,024 | Oldest dropped, counted |

`output-matched` is a drop-oldest event kind in the [runtime model](/pages/reference-runtime-model/) (ABI-033): when the queue is full, the oldest queued drop-oldest event is evicted and counted in `events-dropped`. Events that are never dropped, such as `agent-activity-changed`, are not evicted. Matches above the rate limit are dropped and counted as described under Rate limit.

## Risk and tier rules

- This grant is not marked high risk in the capability table.
- A T1 bundled plugin receives it automatically up to its manifest ceiling.
- A T2 or T3 plugin needs an explicit grant in the review dialog, and the user's choice is remembered.
- Filters see the output of the panes they select. A filter on `all` sees every terminal, so use `agents` or `terminals` when the plugin does not need more.

## Worked example 1: a manifest filter in Rust

This plugin counts rate-limit messages from the model service in agent panes. The manifest asks for the grant on the server side and declares the filter. The rate limit is two accepted matches per second per pane.

```json
{
  "capabilities": {
    "server": ["terminal.output-filter"]
  },
  "contributes": {
    "output_filters": [
      {
        "id": "rate-limit-seen",
        "regex": "rate limit (reached|exceeded)",
        "panes": "agents",
        "rate_per_second": 2
      }
    ]
  }
}
```

The Rust code implements the server-side traits. Lifecycle uses the defaults, so the manifest
filter needs no code to install.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct RateLimitWatcher {
    accepted_matches: u32,
}

impl Lifecycle for RateLimitWatcher {}

impl Handler for RateLimitWatcher {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            match event {
                Event::OutputMatched(matched) if matched.filter_id == "rate-limit-seen" => {
                    self.accepted_matches += 1;
                }
                // Drops are counted by the host (Event::EventsDropped); nothing to do here.
                _ => {}
            }
        }
        Vec::new()
    }
}
```

The `Event` and `Submission` names come from the prelude. Matches above the rate limit never
reach `on_events`, so `accepted_matches` counts only delivered matches.

## Worked example 2: a runtime filter in TypeScript

This text trigger watches one pane for a Rust compiler error code. The user turns it on and off
from a setting. The filter fires at most once per second. The host removes the filter when the
plugin deactivates, so the plugin does not need to remove it there.

**TypeScript**

```ts
import { definePlugin, type PluginError } from '@ilium/plugin-sdk';
import type { Event } from '@ilium/plugin-sdk/types';
import { addOutputFilter, removeOutputFilter } from '@ilium/plugin-sdk/primitives/terminal';

const BUILD_FILTER_ID = 'build-failed';
const watchedPanes = new Set<bigint>();

/** Turns the trigger on for one pane. Returns false when the grant is declined. */
export function enableTrigger(pane: bigint): boolean {
  try {
    addOutputFilter(pane, {
      id: BUILD_FILTER_ID,
      regex: 'error\\[E',
      ratePerSecond: 1,
    });
    watchedPanes.add(pane);
    return true;
  } catch (error) {
    const pluginError = error as PluginError;
    if (pluginError.code === 'not-permitted') {
      return false;
    }
    throw error;
  }
}

export function disableTrigger(pane: bigint): void {
  removeOutputFilter(pane, BUILD_FILTER_ID);
  watchedPanes.delete(pane);
}

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  activate() {
    return;
  },
  deactivate() {
    watchedPanes.clear();
    return undefined;
  },
  onEvents(batch: Event[]) {
    for (const event of batch) {
      if (event.tag === 'output-matched' && event.val.filterId === BUILD_FILTER_ID) {
        // A text trigger runs its actions here, for example a sound or a notice.
        // The intents that do that are on the notifications page.
      }
    }
    return [];
  },
});
```

## Edge cases and errors

| Situation | Result |
|---|---|
| Grant declined or revoked | `add-output-filter` returns `not-permitted`. Manifest filters are not installed. No events arrive. |
| Pane does not exist | `not-found`. |
| Regex does not compile, or the pattern is over 512 bytes, or the compiled program is over 1 MiB | `invalid-argument`. |
| `rate_per_second` is 0 or over 100, or the id is empty | `invalid-argument`. |
| More than 32 filters on T2 or T3 (128 on T1), or 512 across all plugins | `quota-exceeded`. |
| Same id added twice on one pane | The second call replaces the first. |
| Same id on another pane | Allowed. Filter ids are scoped to the pane. |
| Matches above `rate_per_second` | Dropped and counted in `events-dropped`. |
| Pane closes | Its runtime filters are removed. No event is raised. |
| Plugin deactivates | All of its runtime filters are removed. |

Every refused call is written to the plugin log, which `ilium plugin logs` shows.

## What the user sees

The review dialog lists the grant as a checkbox. A declined grant is shown dimmed, and the plugin
keeps its other features.

<figure>
<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.terminal 0.1.0 by Example Ltd                                  │
│                                                                            │
│ <span class="t-hi">Server side</span>                                                                │
│ [x] Filters terminal output <span class="t-dim">……………………………………………………………</span> terminal.output-filter │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>The review dialog for a package that requests terminal.output-filter, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar. Checked rows are granted; a high-risk row starts unchecked.</figcaption>
</figure>

After a refused call, the plugin log names the function and the reason:

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin logs example-plugin
<span class="t-warn">warn</span>  terminal.add-output-filter: not-permitted (grant declined)
<span class="t-dim">      import ilium:primitives/terminal, pane 42</span>
<span class="t-err">error</span> filter "build-failed" not installed, no events will arrive
</pre>
<figcaption>Illustrative, not generated. The log lines a refused runtime filter leaves behind. The format is a placeholder.</figcaption>
</figure>

A text trigger that cannot watch output should say so in its settings page rather than fail
silently.

<div class="warn">
<p>A filter on <code>all</code> reads every terminal's output, including panes that show secrets such as tokens printed by a command. Prefer <code>agents</code> or <code>terminals</code>, and keep the regex narrow.</p>
</div>

<div class="note">
<p>A filter sees only completed lines. A prompt that waits for input without a newline is not matched until the line ends.</p>
</div>

Requirement ids for this page: [PRI-006]. See the [requirements index](/pages/reference-requirements/).

## See also

- [terminal.read-screen](/pages/capability-terminal-read-screen/) for reading the rows around a match.
- [sound.play](/pages/capability-sound-play/) and [notify.desktop](/pages/capability-notify-desktop/), the actions a text trigger usually runs.
- [Terminal primitives](/pages/primitives-terminal/) for the complete terminal interface.
- [Events and intents](/pages/api-events/) for the event envelope and the batch rules.
- [Errors and limits](/pages/reference-errors-and-limits/) for the error codes and quota model.

Checked against the API version `ilium:plugin@0.1.0`.

### Security

**[SEC-046]** Filter patterns are compiled with size limits; a catastrophic pattern is refused with `invalid-argument`. See [Security testing](/pages/reference-security-testing/).
