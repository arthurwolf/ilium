# tree.write

## Overview

The `tree.write` grant lets a plugin change the project tree. It can rename a node, move it, close it, or set its lock flag. The plugin never edits the tree itself. It returns submissions from `on-events`, or calls `intents.submit` from a timer or request callback. The kernel checks each batch against the grants, applies the accepted submissions in order, and gives every submission exactly one outcome. That outcome arrives as a `request-completed` event.

The grant keeps two user decisions intact. A title the user typed is never overwritten by a plugin, and a locked node cannot be closed by a plugin.

## You need

- The grant `tree.write` in the manifest, on each side that submits the intents (`server`, `client`, or both). See [capabilities](/pages/capabilities/).
- The grant [tree.read](/pages/capability-tree-read/) when the plugin must find nodes or receive `tree-changed`.
- The grant [panes.read](/pages/capability-panes-read/) when the plugin reacts to `pane-created`, as the examples below do.
- Rust: `use ilium_plugin_sdk::prelude::*;` with the `ilium_plugin` attribute and `Handler`. TypeScript: `definePlugin` from `@ilium/plugin-sdk`.

### Grant syntax

- Valid: `tree.write`
- Invalid: `tree.write:tree`. The grant takes no argument. The manifest is refused at install with `manifest.grant-argument`, and the text is `capability "tree.write" takes no argument (got "tree.write:tree")`.

### Risk and tier

- The grant is not marked high risk.
- A T1 bundled plugin receives it automatically, up to its manifest ceiling.
- A T2 or T3 plugin needs an explicit grant in the review dialog. The grant is remembered.
- Grants are per side (`server` and `client`).

## Surface

The grant has no functions. It opens four intents, each returned as a submission. The WIT records below are the payloads.

```wit
record submission { intent: intent, correlation: u64, dedup-key: option<string> }
enum placement { before, after, inside-first, inside-last }
record rename-node-spec { node: node-id, title: string, source: title-source, expected-revision: option<u64> }
record move-node-spec { node: node-id, target: node-id, placement: placement }
record set-node-locked-spec { node: node-id, locked: bool }
```

| Intent | Payload | Meaning | Outcomes beyond `ok` |
|---|---|---|---|
| `rename-node` | `rename-node-spec`: `node`, `title` (string), `source` (`plugin` or `inferred`), `expected-revision` (optional) | Sets the title of a node. The title is 1 to 256 characters. | `refused(not-found)`; `refused(conflict)` when the user named the node by hand; `refused(invalid-argument)` for a title of the wrong length |
| `move-node` | `move-node-spec`: `node`, `target` (node-id), `placement` | Moves a node before or after `target`, or into `target` as first or last child. | `refused(not-found)`; `refused(invalid-argument)` when `target` is the node itself or lies inside it |
| `close-node` | `node` (node-id) | Closes the node, as the user does in the tree. | `refused(not-found)`; `refused(conflict)` when the node is locked |
| `set-node-locked` | `set-node-locked-spec`: `node`, `locked` (bool) | Sets or clears the lock flag. | `refused(not-found)` |

Every intent can also return `refused(not-permitted)` when the grant is missing or declined.

The `placement` values, in Rust and TypeScript:

| Value | Rust | TypeScript |
|---|---|---|
| Before `target` | `Placement::Before` | `'before'` |
| After `target` | `Placement::After` | `'after'` |
| First child of `target` | `Placement::InsideFirst` | `'inside-first'` |
| Last child of `target` | `Placement::InsideLast` | `'inside-last'` |

Import paths:

- Rust: `ilium_plugin_sdk::types::intents::{Intent, Submission, RenameNodeSpec, MoveNodeSpec, SetNodeLockedSpec}` and `ilium_plugin_sdk::types::tree::Placement`. `Submission::new(intent)` takes the next SDK correlation number. `Submission::keyed(intent, key)` sets a dedup key.
- TypeScript: `Intent` and `Submission` from `@ilium/plugin-sdk/types`. Write the correlation as a `bigint`, for example `1n`.

### Batch and key rules

- A batch holds at most 256 submissions. A return of more than 256 traps the plugin, and none of the batch is applied.
- A batch is refused as a whole for one of two reasons: a malformed submission, or a missing grant. The offending submission gets `refused(invalid-argument)` or `refused(not-permitted)`. Every other submission gets `refused(cancelled)` with the message `batch refused: submission <index>`.
- Submissions apply in order. A node closed by an earlier submission in the batch makes a later submission on it fail with `failed(not-found)`.
- `dedup-key` is optional for these intents. A key of 1 to 128 characters from `A-Z`, `a-z`, `0-9`, `.`, `_`, `:`, `/` and `-` is reserved for 10 minutes once the submission is accepted. A repeat completes as `duplicate(<original>)`. A refused submission does not reserve its key, so a retry with the same key is valid.

## Manual titles stay with the user

When the user types a title for a node, the kernel marks the node as named by hand. From then on, a `rename-node` on that node returns `refused(conflict)`, whatever plugin sends it. The mark is removed when the user clears the title, which returns the node to automatic naming.

The plugin cannot read the mark, because `node-view` has no field for it. It learns of the mark only from the outcome:

```text
rename-node { node: 42, title: "Build server", source: plugin }
-> refused(plugin-error { code: conflict,
                          message: "node 42 was named manually",
                          retry-after-ms: none })
```

Treat this outcome as final for that node until the user clears the title. Do not retry in a loop.

## Lock flag

The lock flag has one effect on `tree.write`: a locked node cannot be closed. `close-node` on it returns `refused(conflict)`. A rename or a move of a locked node is applied. A plugin can also clear the flag, so set it only on nodes the plugin owns.

## Examples

### Lock new terminal panes (Rust)

This server-side plugin locks every new terminal pane, so a stray close cannot remove it. It logs any refused lock. The manifest requests `"capabilities": { "server": ["panes.read", "tree.write"] }`.

**Rust**

```rust
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::intents::SetNodeLockedSpec;

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct LockNewTerminals;

impl Handler for LockNewTerminals {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            match event {
                Event::PaneCreated(pane) if pane.content_kind == "ilium.terminal" => {
                    let spec = SetNodeLockedSpec { node: pane.node, locked: true };
                    submissions.push(Submission::new(Intent::SetNodeLocked(spec)));
                }
                Event::RequestCompleted(result) => {
                    if let Outcome::Refused(error) = result.outcome {
                        log::log(Level::Warn, &format!("lock refused: {}", error.message));
                    }
                }
                _ => {}
            }
        }
        submissions
    }
}
```

### Name new terminal panes (TypeScript)

This server-side plugin names each new terminal pane `Shell`. The dedup key stops a repeated event for the same pane from renaming it again within 10 minutes. When the user has already named the pane, the outcome is `conflict`, and the plugin keeps the user's title. The manifest requests the same grants as the Rust example.

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import type { Event, Submission } from '@ilium/plugin-sdk/types';
import { log } from '@ilium/plugin-sdk/host/log';

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    for (const event of batch) {
      if (event.tag === 'pane-created' && event.val.contentKind === 'ilium.terminal') {
        submissions.push({
          intent: { tag: 'rename-node', val: { node: event.val.node, title: 'Shell', source: 'plugin' } },
          correlation: event.val.node,
          dedupKey: `shell-title:${event.val.node}`,
        });
      }
      if (event.tag === 'request-completed') {
        const outcome = event.val.outcome;
        if (outcome.tag === 'refused' && outcome.val.code === 'conflict') {
          log('info', 'the user named this pane, so the title stays');
        }
      }
    }
    return submissions;
  },
});
```

## Edge cases and errors

| Situation | Outcome |
|---|---|
| The node id does not exist, or the node was removed before the batch was accepted | `refused(not-found)` |
| The node was removed by an earlier submission in the same batch | `failed(not-found)` |
| A title is empty or longer than 256 characters | `refused(invalid-argument)` |
| The user named the node by hand | `refused(conflict)` on `rename-node` |
| `close-node` on a locked node | `refused(conflict)` |
| `move-node` with a target inside the moved node | `refused(invalid-argument)` |
| A malformed submission | The whole batch is refused. |
| More than 256 submissions in one return | The plugin traps, and none of the batch is applied. |
| The grant is declined | `refused(not-permitted)` for the first `tree.write` submission in a batch, `refused(cancelled)` for the rest. The plugin's other intents and events are not affected. |
| The grant is revoked | The authorization epoch changes. Calls in flight return `revoked`. The plugin is deactivated with `revoked`, then re-activated with the reduced grant set. |
| A repeat of an accepted `dedup-key` within 10 minutes | `duplicate(<original>)`, with no effect |

Every `not-permitted` refusal is also written to the plugin log, which `ilium plugin logs` shows. Conflicts are not logged by the kernel, so a plugin that needs a record of them writes its own log line, as the examples do.

## What the user sees

The review dialog lists the grant when a T2 or T3 plugin is installed. Each side is shown separately.

<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.tree 0.1.0 by Example Ltd                                      │
│                                                                            │
│ <span class="t-hi">Server side</span>                                                                │
│ [x] Renames and moves tree nodes <span class="t-dim">………………………………………………………………………………</span> tree.write │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>

Illustrative, not generated. The wording is a placeholder.

A successful rename or lock appears in the tree at once. The user sees the new title, and a lock marker on the node. A conflict or a refusal leaves the tree unchanged, and the user sees no message from the kernel.

<pre class="term"><span class="t-hi">project</span>
├─ <span class="t-ok">Build server</span>            <span class="t-dim">pane 42  locked</span>
╰─ Shell                        <span class="t-dim">pane 57</span>
</pre>

Illustrative, not generated. The node named by hand keeps its title, and the locked node shows its flag.

## Submission flow

<figure>
<svg viewBox="0 0 400 236" role="img" aria-label="A batch of intents is checked as a whole, then each submission gets an outcome">
  <defs>
    <marker id="tw-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7"
            markerHeight="7" orient="auto-start-reverse">
      <path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="20" y="8" width="360" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="200" y="32" font-size="11" text-anchor="middle" fill="currentColor">Plugin returns a batch from on-events</text>
  <line x1="200" y1="48" x2="200" y2="72" stroke="currentColor" marker-end="url(#tw-arrow)"/>
  <text x="212" y="64" font-size="10" fill="currentColor">submitted intents</text>
  <rect x="20" y="72" width="360" height="40" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="200" y="96" font-size="11" text-anchor="middle" fill="currentColor">Kernel checks the whole batch against the grant</text>
  <line x1="200" y1="112" x2="200" y2="136" stroke="currentColor" marker-end="url(#tw-arrow)"/>
  <text x="212" y="128" font-size="10" fill="currentColor">applied in order</text>
  <rect x="20" y="136" width="360" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="200" y="160" font-size="11" text-anchor="middle" fill="currentColor">Each submission gets exactly one outcome</text>
  <line x1="200" y1="176" x2="200" y2="200" stroke="currentColor" marker-end="url(#tw-arrow)"/>
  <text x="212" y="192" font-size="10" fill="currentColor">ok, refused or failed</text>
  <rect x="20" y="200" width="360" height="28" rx="6" fill="none" stroke="currentColor"/>
  <text x="200" y="218" font-size="11" text-anchor="middle" fill="currentColor">Plugin receives request-completed</text>
</svg>
<figcaption>A batch is checked as a whole, then each submission is applied and answered with its own outcome.</figcaption>
</figure>

## See also

- [tree.read](/pages/capability-tree-read/) for the reads that find the nodes to change.
- [panes.create](/pages/capability-panes-create/) for creating the panes these intents rename, move or lock.
- [api-tree](/pages/api-tree/) for the `node-view` fields.
- [api-intents](/pages/api-intents/) for the full intent catalogue and the dedup rules.
- [api-events](/pages/api-events/) for `request-completed`, `tree-changed` and `pane-created`.
- [errors and limits](/pages/reference-errors-and-limits/) for the error codes.

Checked against the API version `ilium:plugin@0.1.0`.
