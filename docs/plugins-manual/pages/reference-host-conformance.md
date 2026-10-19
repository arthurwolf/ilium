---
layout: page
title: Host conformance matrix
permalink: /pages/reference-host-conformance/
---

## Host conformance matrix

This page is the checklist for a host implementation, and for a test that wants to prove one. It lists every import, export, event, intent and widget of `ilium:plugin@0.1.0` with the side that may use it, the grant it needs, the delivery policy or deadline, and the error a host must return. The names and shapes are defined in the [WIT reference](/pages/reference-wit/). The rules behind the columns are in the [Runtime model](/pages/reference-runtime-model/). Each matrix has a row for every member, so a missing row in a host test is a missing test [ABI-030].

### What you need

- **The WIT reference.** Row names are the names in the [WIT reference](/pages/reference-wit/).
- **A host or a fake host.** The tests below run against the real host or against the test harness described in [Testing](/pages/testing/).
- **The counts.** 45 import rows (39 host interfaces, 4 primitives, 2 scene interfaces), 16 export rows, 56 event rows, 42 intent rows and 51 widget rows. A matrix with another count is out of date.

## 1. How to use the matrices

The columns S, C and A mean server, client and scene. A cell says whether a plugin of that side may import or export the interface. A call from a side that is not allowed fails at link time: the interface is not linked, and the component is refused with `manifest.import-not-requested` when no requested capability unlocks it.

For each row a conformance test does four things:

1. Calls the member from an allowed side with the grant, and checks the documented result.
2. Calls it without the grant, and checks `not-permitted` (or, for an interface the manifest never requested, the trap `host.capability-not-requested`).
3. Calls it with an argument over a limit, and checks `invalid-argument`, `quota-exceeded` or the documented code.
4. For a slow call, checks the `request-completed` outcome and the `take-*` rules: one take, a hold of 30 seconds, and `none` after that [ABI-014].

## 2. Import matrix

Every grant name is in the [capability index](/pages/capabilities/). A slow call is a function that returns `result<request-id, plugin-error>`, and its take partner is in brackets.

| No. | Interface | S | C | A | Grant | Slow calls (take partner) |
|---|---|---|---|---|---|---|
| 1 | `log` | yes | yes | yes | none | none |
| 2 | `clock` | yes | yes | yes | none | none |
| 3 | `timers` | yes | yes | no | none | none |
| 4 | `journal` | yes | yes | no | none for own entries, `journal.read` for others | none |
| 5 | `tree` | yes | yes | no | `tree.read`, `tree.metadata` | none |
| 6 | `panes` | yes | yes | no | `panes.read` | none |
| 7 | `intents-host` | yes | yes | no | by intent | `submit` (completes as `request-completed`) |
| 8 | `storage` | yes | yes | read only | none | none |
| 9 | `settings` | yes | yes | no | none own, `settings.read-all`, `settings.write-all` | none |
| 10 | `codec` | yes | yes | yes | none | none |
| 11 | `fs` | yes | yes | scope reads only | `fs.read:<scope>`, `fs.write:<scope>`, `fs.watch:<scope>`, `fs.pick` | `read-all` (`take-bytes`), `scan` (`take-entries`), `write`, `remove`, `rename`, `lock` (`take-lock`), `pick` (`take-picked`), `read-with-revision` (`take-revisioned`), `write-atomic` (`take-written`) |
| 12 | `http` | yes | yes | yes | `http:<host-pattern>` | `fetch` (`take-response`) |
| 13 | `jobs` | yes | yes | yes | `jobs.run` | `start` (`take-output`) |
| 14 | `process` | yes | yes | no | `process.run:<program>`, `process.long-lived` | `run` (`take-exit`) |
| 15 | `inference` | yes | yes | no | `inference`, `inference.provide` | `complete` (`take-completion`), `models` (`take-models`) |
| 16 | `vector` | yes | yes | no | `vector` | `embed` (`take-vectors`) |
| 17 | `sounds` | yes | yes | no | none | none |
| 18 | `topics` | yes | yes | no | `topics.publish`, `topics.subscribe` | none |
| 19 | `commands` | yes | yes | no | none own, `commands.invoke:<pattern>` | `invoke` |
| 20 | `services` | yes | yes | no | `services.use:<name>`, `services.provide:<name>` | `call` (`take-reply`) |
| 21 | `ui-host` | no | yes | no | none for own views | none |
| 22 | `keys` | no | yes | no | `keys.intercept` | none |
| 23 | `cli` | yes | no | no | `cli.contribute` | `read-stdin` (`take-stdin`) |
| 24 | `http-api` | yes | no | no | `http-api.route:<path>` | none |
| 25 | `workspace` | yes | no | no | `workspace.manage` | none |
| 26 | `clipboard` | no | yes | no | `clipboard.read`, `clipboard.write` | none |
| 27 | `audio` | no | yes | yes | `audio.capture`, `audio.loopback`, `audio.playback` | `play` |
| 28 | `media` | no | yes | yes | `media.video` | none |
| 29 | `location` | no | yes | yes | `location.observer` | none |
| 30 | `help` | no | yes | no | `help.read` | none |
| 31 | `plugins` | no | yes | no | `plugins.manage` | none |
| 32 | `catalogs` | yes | yes | no | none | `query-async` (`take-page`) |
| 33 | `geocode` | yes | yes | yes | `geocode` | `search`, `reverse` (`take-places`) |
| 34 | `sessions` | yes | no | no | `sessions.cross` | `send` (`take-reply`) |
| 35 | `kernel-settings` | yes | yes | no | `settings.read-kernel:<tab-id>` | none |
| 36 | `keymap` | no | yes | no | `keymap.read` | none |
| 37 | `gpu` (unstable `gpu-kernels`) | no | yes | yes | `gpu.kernels` | none |
| 38 | `client-requests` | yes | yes | no | none for declared kinds | `request` (`take-answer`, server side) |
| 39 | `extensions` | yes | yes | no | `extensions.provide:<point>`, `extensions.contribute:<point>` | `run-hook` (`take-reply`) |
| 40 | `terminal` (primitive) | yes | yes | no | `terminal.read-screen`, `terminal.read-cells`, `terminal.history`, `terminal.output-filter`, `terminal.pointer`, `terminal.input-lock` | `history-text` (`take-history`), `search-history` (`take-matches`) |
| 41 | `agent` (primitive) | yes | yes | no | `agent.read`, `agent.read-prompts`, `agent.transcript`, `agent.signatures`, `agent.control` | `transcript` (`take-transcript`) |
| 42 | `editor` (primitive) | no | yes | no | `editor.read`, `editor.edit` | `load-file` (`take-buffer`), `save-buffer` |
| 43 | `basic` (primitive) | no | yes | no | `panes.provide-kind` | none |
| 44 | `surface` (anim) | no | no | yes | none | none |
| 45 | `fonts` (anim) | no | no | yes | none | none |

**Notes.**

- A scene may import `storage` for reads only (`get`, `keys`), and `fs` for the scopes its manifest names. Both are rows with `read only` or `scope reads only` in the A column.
- `geocode` is linked on all three sides. `gpu` is for client and scene only.
- `ui-host` is the host interface `ui`. Its name differs only to avoid a clash with the type interface `ilium:types/ui`.

## 3. Export matrix

An export is named in the manifest `exports` list unless the Required column says yes. A required export is never listed [ABI-018]. The component exports must equal the required exports plus the manifest list, or the install fails with `manifest.exports-mismatch` [ABI-007].

| No. | Export | S | C | A | Required | Manifest name | Grant | Deadline |
|---|---|---|---|---|---|---|---|---|
| 1 | `lifecycle` | yes | yes | yes | yes | none | none | `activate` 2 s, `deactivate` 500 ms |
| 2 | `handler` | yes | yes | no | yes | none | none | 20 ms per batch |
| 3 | `views` | no | yes | no | no | `views` | none | 10 ms |
| 4 | `detector` | yes | no | no | no | `detector` | `agent.read` | 20 ms |
| 5 | `job-runner` | yes | yes | yes | no | `job-runner` | none (kind in `contributes.jobs`) | 2 s CPU, 20 s wall |
| 6 | `service-provider` | yes | yes | no | no | `service-provider` | `services.provide:<name>` | `command_deadline_ms`, 50 ms (T1 ceiling 100 ms, T2/T3 ceiling 50 ms) |
| 7 | `inference-provider` | yes | yes | no | no | `inference-provider` | `inference.provide` | `command_deadline_ms`, 50 ms to call (T1 ceiling 100 ms, T2/T3 ceiling 50 ms), 120 s reply |
| 8 | `annotator` | no | yes | no | no | `annotator` | `editor.read` | 10 ms |
| 9 | `settings-validate` | yes | yes | no | no | `settings-validate` | none | 100 ms |
| 10 | `renderer` | no | no | yes | yes | none | none | frame budget, at most 50 ms |
| 11 | `catalog-provider` | yes | yes | no | no | `catalog-provider` | none (ids in `contributes.catalogs`) | 20 ms |
| 12 | `menu-provider` | no | yes | no | no | `menu-provider` | none | 20 ms |
| 13 | `virtual-rows-provider` | yes | yes | no | no | `virtual-rows-provider` | `tree.decorate` | 20 ms |
| 14 | `token-provider` | no | yes | no | no | `token-provider` | `editor.read` | 10 ms |
| 15 | `hook-handler` | yes | yes | no | no | `hook-handler` | `extensions.contribute:<point>` | 20 ms |
| 16 | `migrator` | yes | yes | no | no | `migrator` | none | 60 s for T1, 30 s for T2 and T3, before `activate` |

**Notes.**

- `hook-handler` is the export that receives `handle-hook` calls for extension points. It exists in the contract and is listed here so that the table has all 16 exports of `ilium:exports`.
- A deadline breach of `menu-provider` or `token-provider` falls back to static content and does not fail the user action.

## 4. Event matrix

An event is delivered through `handler.on-events`. The numbers are the case positions in the `event` variant, and they are part of the ABI [ABI-002]. The table has 56 rows, one per case. The event catalogue in [Event and intent tables](/pages/reference-events-and-intents/) groups three pairs of cases into one row, so it shows 35 base rows for 38 base cases.

| No. | Event | Grant | Delivery policy |
|---|---|---|---|
| 1 | `tree-changed` | tree.read | coalesced (one merged list) |
| 2 | `node-removed` | tree.read | never dropped |
| 3 | `pane-created` | panes.read | never dropped |
| 4 | `pane-exited` | panes.read | never dropped |
| 5 | `pane-focused` | panes.read | coalesced |
| 6 | `pane-resized` | panes.read | coalesced (node) |
| 7 | `pane-title-changed` | panes.read | coalesced (node) |
| 8 | `agent-identified` | agent.read | never dropped |
| 9 | `agent-activity-changed` | agent.read | never dropped |
| 10 | `agent-goal-changed` | agent.read | coalesced (node) |
| 11 | `input-submitted` | agent.read-prompts | never dropped |
| 12 | `output-matched` | terminal.output-filter | drop oldest, counted |
| 13 | `link-activated` | own link_detectors | never dropped |
| 14 | `pointer` | terminal.pointer, while claimed | coalesced (move, drag) |
| 15 | `editor-opened` | editor.read | never dropped |
| 16 | `editor-closed` | editor.read | never dropped |
| 17 | `editor-changed` | editor.read | coalesced (buffer), debounced 100 ms |
| 18 | `editor-saved` | editor.read | never dropped |
| 19 | `settings-changed` | always (own) | coalesced, keys unioned |
| 20 | `timer-fired` | always (own) | coalesced (timer) |
| 21 | `request-completed` | always (own) | never dropped |
| 22 | `job-progress` | always (own) | coalesced (job) |
| 23 | `message` | always, or messages.peer of the sender | never dropped |
| 24 | `topic-published` | topics.subscribe | drop oldest, counted |
| 25 | `command-invoked` | own commands | never dropped |
| 26 | `ui-action` | own views | never dropped (scrolled coalesced) |
| 27 | `key-pressed` | keys.intercept or wants_keys | never dropped |
| 28 | `key-released` | keys.intercept or wants_keys | never dropped |
| 29 | `view-visibility` | own views | coalesced |
| 30 | `fs-changed` | fs.watch | coalesced (watch) |
| 31 | `audio-frames` | audio.capture or audio.loopback | drop oldest, counted |
| 32 | `cli-invoked` | cli.contribute | never dropped |
| 33 | `cli-cancelled` | cli.contribute | never dropped |
| 34 | `http-called` | http-api.route | never dropped |
| 35 | `client-attached` | always (server side) | never dropped |
| 36 | `client-detached` | always (server side) | never dropped |
| 37 | `theme-changed` | always (client side) | coalesced |
| 38 | `events-dropped` | always (own) | coalesced by (event, source), never dropped |
| 39 | `screen-changed` | terminal.read-cells | coalesced (pane), 10 per second |
| 40 | `user-line-submitted` | agent.read-prompts | never dropped |
| 41 | `signal-changed` | agent.read | coalesced (pane, signal) |
| 42 | `agent-model-changed` | agent.read | coalesced (pane) |
| 43 | `editor-pointer` | editor.read | coalesced on move |
| 44 | `editor-viewport-changed` | editor.read | coalesced (pane), 100 ms |
| 45 | `http-stream-data` | the http grant of the stream | never dropped, back-pressure |
| 46 | `http-stream-closed` | the http grant of the stream | never dropped |
| 47 | `child-output` | process.long-lived | never dropped, back-pressure |
| 48 | `child-exited` | process.long-lived | never dropped |
| 49 | `pane-lifecycle-changed` | panes.read | never dropped |
| 50 | `attention-changed` | tree.attention or tree.read | coalesced (node) |
| 51 | `client-request` | own client_requests (client side) | never dropped |
| 52 | `kernel-settings-changed` | settings.read-kernel:<tab> | coalesced (tab), keys unioned |
| 53 | `keymap-changed` | keymap.read | coalesced |
| 54 | `durable-timer-fired` | always (own) | never dropped |
| 55 | `input-uncertain` | grant of the original send-input | never dropped |
| 56 | `display-mode-changed` | own display_modes | coalesced |

## 5. Intent matrix

An intent leaves a plugin in a submission. The numbers are the case positions in the `intent` variant. Every row also fails with the common codes: `not-permitted` for a missing grant (the whole batch is refused), `invalid-argument` for a bad value, and `busy` when the plugin has too many pending requests.

| No. | Intent | Grant | Outcomes and limits beyond `ok` |
|---|---|---|---|
| 1 | `rename-node` | tree.write | conflict for a manual title |
| 2 | `move-node` | tree.write | invalid-argument for a target inside the node |
| 3 | `close-node` | tree.write | conflict for a locked node |
| 4 | `set-node-locked` | tree.write | not-found |
| 5 | `create-pane` | panes.create (+ terminal.write with initial-input) | ok-node; failed(io) |
| 6 | `signal-pane` | panes.signal | pane-not-ready |
| 7 | `send-input` | terminal.write (+ terminal.raw-input, terminal.input-lock) | ok-epoch on agent panes; pane-not-ready; conflict on stale epoch |
| 8 | `agent-action` | agent.control | unsupported; conflict |
| 9 | `set-decoration` | tree.decorate | invalid-argument for an undeclared slot |
| 10 | `clear-decoration` | tree.decorate | common codes only |
| 11 | `set-virtual-rows` | tree.decorate | invalid-argument for an undeclared kind |
| 12 | `play-sound` | sound.play | not-found; rate-limited above 4 per second |
| 13 | `notify` | notify.desktop | rate-limited above 6 per minute |
| 14 | `open-view` | ui.panels | not-found |
| 15 | `close-view` | ui.panels | not-found |
| 16 | `invoke-command` | commands.invoke | failed(timed-out) after 30 s |
| 17 | `message` | messages.peer | not-found; invalid-argument above 1 MiB |
| 18 | `editor-apply-edits` | editor.edit | conflict on a stale revision |
| 19 | `editor-open` | editor.edit | ok-node; failed(io) |
| 20 | `freeze-pane` | panes.lifecycle | unsupported on Windows |
| 21 | `unfreeze-pane` | panes.lifecycle | conflict for a running pane |
| 22 | `replace-pane-command` | panes.lifecycle | ok when the new process runs |
| 23 | `terminate-pane-process` | panes.lifecycle | grace-ms at most 10,000 |
| 24 | `resize-pane` | panes.lifecycle | conflict names the neighbour |
| 25 | `create-group` | tree.restructure | ok-node |
| 26 | `create-folder` | tree.restructure | ok-node |
| 27 | `create-project` | tree.restructure | ok-node |
| 28 | `create-split` | tree.restructure | 2 to 4 panes; ok-node |
| 29 | `set-bookmarked` | tree.write | ok |
| 30 | `set-expanded` | tree.write, or own virtual_rows | ok |
| 31 | `restructure` | tree.restructure | ok-node with undo token; all or nothing |
| 32 | `revert-restructure` | tree.restructure | conflict after a later change |
| 33 | `set-inferred-icon` | tree.decorate | conflict when a manual icon exists |
| 34 | `mark-attention` | tree.attention | ok |
| 35 | `ack-attention` | tree.attention | ok |
| 36 | `open-external` | open-external | not-permitted when declined; http, https, mailto only |
| 37 | `set-status-segment` | ui.chrome | 10 per second per slot (the segment id is the slot) |
| 38 | `clear-status-segment` | ui.chrome | ok |
| 39 | `set-frame-slot` | ui.chrome | invalid-argument when applies_to does not match |
| 40 | `clear-frame-slot` | ui.chrome | ok |
| 41 | `toast` | notify.toast | rate-limited above 3 per 10 s |
| 42 | `dismiss-toast` | notify.toast | ok |

## 6. Widget matrix

A plugin sends widgets inside a `ui-tree`. The numbers are the case positions in the `widget` variant. The column `widget-state` names the case that `ui.widget-state` returns for the widget. The last column lists the action kinds the widget usually sends. The family pages give the exact list.

| No. | Widget | Family | Spec | `widget-state` case | Typical `ui-action` kinds |
|---|---|---|---|---|---|
| 1 | `layout` | Layout | `layout-spec` | none | none |
| 2 | `block` | Layout | `block-spec` | none | none |
| 3 | `tabs` | Layout | `tabs-spec` | tab | selected |
| 4 | `popup` | Layout | `popup-spec` | none | closed |
| 5 | `clear` | Layout | `none` | none | none |
| 6 | `scrollbar` | Layout | `scrollbar-spec` | none | none |
| 7 | `split` | Layout | `split-spec` | split | changed |
| 8 | `separator` | Layout | `separator-spec` | none | none |
| 9 | `dialog` | Layout | `dialog-spec` | none | closed |
| 10 | `scroll-view` | Layout | `scroll-view-spec` | offset | scrolled |
| 11 | `paragraph` | Text | `paragraph-spec` | none | none |
| 12 | `markdown` | Text | `markdown-spec` | none | none |
| 13 | `leader-row` | Text | `leader-spec` | none | none |
| 14 | `key-hints` | Text | `list<key-hint>` | none | none |
| 15 | `diff-view` | Text | `diff-view-spec` | none | none |
| 16 | `%list` | Collections | `list-spec` | selected, ids | selected, activated, dropped |
| 17 | `table` | Collections | `table-spec` | selected, ids | selected, activated, dropped |
| 18 | `tree-list` | Collections | `tree-list-spec` | selected, expanded, ids | selected, activated, dropped |
| 19 | `menu` | Collections | `menu-spec` | selected | activated |
| 20 | `check-list` | Collections | `check-list-spec` | tri | toggled |
| 21 | `list-editor` | Collections | `list-editor-spec` | selected | changed |
| 22 | `gauge` | Data | `gauge-spec` | none | none |
| 23 | `line-gauge` | Data | `line-gauge-spec` | none | none |
| 24 | `sparkline` | Data | `sparkline-spec` | none | none |
| 25 | `bar-chart` | Data | `bar-chart-spec` | none | none |
| 26 | `chart` | Data | `chart-spec` | none | none |
| 27 | `canvas` | Data | `canvas-spec` | none | none |
| 28 | `calendar` | Data | `calendar-spec` | none | none |
| 29 | `progress-row` | Data | `progress-row-spec` | none | none |
| 30 | `meter` | Data | `meter-spec` | none | none |
| 31 | `minimap` | Data | `minimap-spec` | none | none |
| 32 | `button` | Input | `button-spec` | none | activated |
| 33 | `toggle` | Input | `toggle-spec` | checked, tri | toggled |
| 34 | `choice` | Input | `choice-spec` | selected | selected |
| 35 | `number` | Input | `number-spec` | number | changed |
| 36 | `text-input` | Input | `text-input-spec` | text | changed, submitted |
| 37 | `text-area` | Input | `text-area-spec` | text | changed |
| 38 | `schema-form` | Input | `schema-form-spec` | none | none (settings-changed) |
| 39 | `key-capture` | Input | `key-capture-spec` | text | changed, closed |
| 40 | `color-picker` | Input | `color-picker-spec` | text | changed |
| 41 | `path-field` | Input | `path-field-spec` | text | changed, submitted |
| 42 | `location-picker` | Input | `location-picker-spec` | geo | changed |
| 43 | `badge` | Status | `badge-spec` | none | none |
| 44 | `spinner` | Status | `spinner-spec` | none | none |
| 45 | `notice` | Status | `notice-spec` | none | none |
| 46 | `icon` | Status | `icon-spec` | none | none |
| 47 | `image` | Media | `image-spec` | none | none |
| 48 | `surface` | Media | `surface-spec` | none | pointer |
| 49 | `code-view` | Media | `code-view-spec` | none | changed |
| 50 | `terminal-view` | Media | `terminal-view-spec` | none | pointer |
| 51 | `video` | Media | `video-spec` | none | none |

## 7. Error behaviour that every host shares

| Situation | Result | Test |
|---|---|---|
| Call without the grant | `not-permitted` | Call each row without its grant |
| Function of a capability the manifest never requested, in a linked interface | trap `host.capability-not-requested` | Call a function of an unrequested capability |
| Interface that no requested capability unlocks | install refused with `manifest.import-not-requested` | Install a component that imports `clipboard` with no clipboard grant |
| Call from a side that may not import it | not linked, install refused | Import `ui-host` from a server component |
| More than 256 submissions | trap, none applied | Return 257 submissions |
| Malformed submission in a batch | whole batch refused, siblings `refused(cancelled)` | Return one bad submission among 3 |
| Take twice | second take returns `none` | Take a result twice |
| Event queue full | policy of the event case | Fill a queue of 1,024 events |

**Rust test sketch**

```rust
use ilium_plugin_sdk::testing::{FakeHost, Side};

#[test]
fn clipboard_write_needs_its_grant() {
    let mut host = FakeHost::new().side(Side::Client);
    let result = host.call_clipboard_write("hello");
    assert_eq!(result.unwrap_err().code, ErrorCode::NotPermitted);
}
```

**TypeScript test sketch**

```ts
import { createFakeHost } from '@ilium/plugin-sdk/testing';

test('clipboard write needs its grant', () => {
  const host = createFakeHost().side('client');
  expect(() => host.clipboard.writeText('hello')).toThrow(expect.objectContaining({ code: 'not-permitted' }));
});
```

## See also

- [WIT reference](/pages/reference-wit/)
- [Runtime model](/pages/reference-runtime-model/)
- [ABI evolution](/pages/reference-abi-evolution/)
- [Event and intent tables](/pages/reference-events-and-intents/)
- [Worlds and sides](/pages/worlds-and-sides/)
- [Requirements: WIT and ABI](/pages/reference-requirements-wit-abi/)
