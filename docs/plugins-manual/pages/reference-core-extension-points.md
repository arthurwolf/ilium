---
layout: page
title: Core extension points
permalink: /pages/reference-core-extension-points/
---

## Core extension points

Every core plugin owns extension points, publishes services and topics, and says whether another plugin may replace it. This page lists all 70 points of the 32 core plugins. Each core page repeats its own part under "Extension points".

Read [Extension points](/pages/api-extension-points/) for the mechanism, [Extending core plugins](/pages/extending-core-plugins/) for guidance, and [Services](/pages/services/) for the service rules. Point ids are `<plugin-id>/<point>`. A contributor holds `extensions.contribute:<id>`. Points marked high make that grant high risk.

### Summary

| Plugin | Id | Points | Services | Topics | Replaceable |
|---|---|---|---|---|---|
| [Tree](/pages/core-plugin-tree/) | `ilium.core.tree` | 3 | 1 | 1 | yes |
| [Layout](/pages/core-plugin-layout/) | `ilium.core.layout` | 2 | 1 | 1 | yes |
| [Terminal](/pages/core-plugin-terminal/) | `ilium.core.terminal` | 3 | 1 | 1 | yes |
| [Smart Copy](/pages/core-plugin-smart-copy/) | `ilium.core.smart-copy` | 2 | 0 | 1 | yes |
| [Editor](/pages/core-plugin-editor/) | `ilium.core.editor` | 3 | 1 | 1 | yes |
| [Kanban](/pages/core-plugin-kanban/) | `ilium.core.kanban` | 2 | 1 | 1 | yes |
| [Agent detection](/pages/core-plugin-agent-detect/) | `ilium.core.agent-detect` | 2 | 1 | 1 | yes |
| [Agent toolbar](/pages/core-plugin-agent-toolbar/) | `ilium.core.agent-toolbar` | 2 | 1 | 0 | yes |
| [Fleet](/pages/core-plugin-fleet/) | `ilium.core.fleet` | 2 | 2 | 1 | yes |
| [Cost](/pages/core-plugin-cost/) | `ilium.core.cost` | 2 | 1 | 1 | yes |
| [Titles and organisation](/pages/core-plugin-titles/) | `ilium.core.titles` | 2 | 1 | 1 | yes |
| [Inference providers](/pages/core-plugin-inference/) | `ilium.core.inference` | 3 | 1 | 1 | yes |
| [Notifications](/pages/core-plugin-notifications/) | `ilium.core.notifications` | 2 | 1 | 1 | yes |
| [Voice](/pages/core-plugin-voice/) | `ilium.core.voice` | 2 | 1 | 1 | yes |
| [Scheduler](/pages/core-plugin-scheduler/) | `ilium.core.scheduler` | 2 | 1 | 1 | yes |
| [Text triggers](/pages/core-plugin-text-triggers/) | `ilium.core.text-triggers` | 2 | 1 | 1 | yes |
| [Progress](/pages/core-plugin-progress/) | `ilium.core.progress` | 2 | 1 | 1 | yes |
| [Attention queue](/pages/core-plugin-attention/) | `ilium.core.attention` | 2 | 1 | 1 | yes |
| [Shared resources](/pages/core-plugin-resources/) | `ilium.core.resources` | 2 | 1 | 1 | yes |
| [Chatroom](/pages/core-plugin-chatroom/) | `ilium.core.chatroom` | 2 | 2 | 1 | yes |
| [Worktrees](/pages/core-plugin-worktrees/) | `ilium.core.worktrees` | 3 | 1 | 1 | yes |
| [Sessions](/pages/core-plugin-sessions/) | `ilium.core.sessions` | 2 | 1 | 1 | no |
| [Compaction](/pages/core-plugin-compaction/) | `ilium.core.compaction` | 2 | 1 | 1 | yes |
| [Settings](/pages/core-plugin-settings/) | `ilium.core.settings` | 2 | 2 | 1 | no |
| [Keys and help](/pages/core-plugin-keys-and-help/) | `ilium.core.keys-and-help` | 2 | 1 | 1 | yes |
| [Workspace search](/pages/core-plugin-search/) | `ilium.core.search` | 2 | 1 | 0 | yes |
| [Ambient look](/pages/core-plugin-ambient/) | `ilium.core.ambient` | 2 | 1 | 1 | yes |
| [Scenes](/pages/core-plugin-scenes/) | `ilium.core.scenes` | 2 | 1 | 0 | yes |
| [Recap](/pages/core-plugin-recap/) | `ilium.core.recap` | 2 | 1 | 0 | yes |
| [Session conversion](/pages/core-plugin-session-convert/) | `ilium.core.session-convert` | 2 | 1 | 0 | yes |
| [Agent setup](/pages/core-plugin-agent-setup/) | `ilium.core.agent-setup` | 3 | 1 | 0 | yes |
| [Diagnostics](/pages/core-plugin-diagnostics/) | `ilium.core.diagnostics` | 2 | 1 | 0 | yes |

### All points

| Point | Kind | Risk | Side | Version | Deadline | Max | Dynamic | User override | What it does |
|---|---|---|---|---|---|---|---|---|---|
| `ilium.core.tree/before-move` | hook: veto | standard | server | 0.1.0 (default) | 20 ms (default) | 8 handlers (default) | no (default) | yes | Refuse a drag or move of a node before the tree applies it. |
| `ilium.core.tree/before-close` | hook: veto | standard | server | 0.1.0 (default) | 20 ms (default) | 8 handlers (default) | no (default) | yes | Refuse closing a pane or group, for example while work is unsaved. |
| `ilium.core.tree/node-sorts` | collect | standard | server | 0.1.0 (default) | none | 64 entries (default) | no (default) | no | Add a sort order to the tree Order By choice. |
| `ilium.core.layout/layout-presets` | collect | standard | server | 0.1.0 (default) | none | 64 entries (default) | no (default) | no | Offer a named split layout in the layout chooser. |
| `ilium.core.layout/before-split` | hook: veto | standard | server | 0.1.0 (default) | 20 ms (default) | 8 handlers (default) | no (default) | yes | Refuse a split, for example above a pane count the plugin allows. |
| `ilium.core.terminal/before-send-input` | hook: veto | high | server | 0.1.0 (default) | 20 ms (default) | 8 handlers (default) | no (default) | yes | Refuse or end input that the user or a plugin sends to a terminal pane. |
| `ilium.core.terminal/open-link` | hook: first | high | client | 0.1.0 (default) | 20 ms (default) | 8 handlers (default) | no (default) | no | Decide how a clicked link opens. The first handler that answers wins. |
| `ilium.core.terminal/link-actions` | collect | standard | client | 0.1.0 (default) | none | 64 entries (default) | no (default) | no | Offer actions for a detected link kind in the terminal right-click menu. |
| `ilium.core.smart-copy/recognisers` | collect | standard | server | 0.1.0 (default) | none | 64 entries (default) | yes | no | Teach Smart Copy a new kind of structure to select. |
| `ilium.core.smart-copy/after-copy` | hook: transform | high | server | 0.1.0 (default) | 20 ms (default) | 8 handlers (default) | no (default) | no | Change the copied text before it reaches the clipboard, for example to remove secrets. |
| `ilium.core.editor/before-save` | hook: transform | high | client | 0.1.0 (default) | 20 ms (default) | 8 handlers (default) | no (default) | no | Change buffer text just before it is written, for example to format it. |
| `ilium.core.editor/file-openers` | collect | standard | client | 0.1.0 (default) | none | 64 entries (default) | no (default) | no | Choose which pane kind opens files with certain extensions. |
| `ilium.core.editor/line-badges` | collect | standard | client | 0.1.0 (default) | none | 64 entries (default) | no (default) | no | Add a marker in the editor gutter for lines that match. |
| `ilium.core.kanban/card-actions` | collect | standard | client | 0.1.0 (default) | none | 64 entries (default) | no (default) | no | Add an action to the card context menu. |
| `ilium.core.kanban/card-moved` | hook: observe | standard | client | 0.1.0 (default) | 20 ms (default) | 8 handlers (default) | no (default) | no | Learn that a card changed column. |
| `ilium.core.agent-detect/activity-rules` | collect | standard | server | 0.1.0 (default) | none | 64 entries (default) | no (default) | no | Add screen patterns that mean working, idle or awaiting approval for one signature. |
| `ilium.core.agent-detect/classify` | hook: transform | high | server | 0.1.0 (default) | 20 ms (default) | 8 handlers (default) | no (default) | no | Adjust the detected activity before it is published. |
| `ilium.core.agent-toolbar/toolbar-actions` | collect | standard | client | 0.1.0 (default) | none | 64 entries (default) | no (default) | no | Add a button to the toolbar of panes that run a signature. |
| `ilium.core.agent-toolbar/banner-segments` | collect | standard | client | 0.1.0 (default) | none | 64 entries (default) | no (default) | no | Add a short segment to the last-prompt banner. |
| `ilium.core.fleet/selectors` | collect | standard | server | 0.1.0 (default) | none | 64 entries (default) | no (default) | no | Add a named selector to `ilium panes`, `ilium broadcast` and the dialogs. |
| `ilium.core.fleet/before-broadcast` | hook: transform | high | server | 0.1.0 (default) | 20 ms (default) | 8 handlers (default) | no (default) | no | Change or end a message before it goes to many agents. |
| `ilium.core.cost/price-sources` | collect | standard | server | 0.1.0 (default) | none | 64 entries (default) | no (default) | no | Supply a price table for models the built-in table does not know. |
| `ilium.core.cost/quota-sources` | collect | standard | server | 0.1.0 (default) | none | 64 entries (default) | no (default) | no | Supply a plan quota or remaining allowance for a provider. |
| `ilium.core.titles/before-apply-title` | hook: transform | standard | server | 0.1.0 (default) | 20 ms (default) | 8 handlers (default) | no (default) | no | Change a generated title before it is applied. |
| `ilium.core.titles/title-styles` | collect | standard | server | 0.1.0 (default) | none | 64 entries (default) | no (default) | no | Add a title style to the style choice. |
| `ilium.core.inference/before-request` | hook: transform | high | server | 0.1.0 (default) | 20 ms (default) | 8 (default) | no (default) | no | Rewrite or refuse an outgoing model request, for example to remove private text. |
| `ilium.core.inference/after-response` | hook: observe | standard | server | 0.1.0 (default) | 20 ms (default) | 8 (default) | no (default) | no | Learn that a request finished, with its token counts. |
| `ilium.core.inference/provider-profiles` | collect | high | client | 0.1.0 (default) | none | 64 (default) | no (default) | no | Offer a ready-made provider setup in the provider chooser. |
| `ilium.core.notifications/before-notify` | hook: veto | standard | server | 0.1.0 (default) | 20 ms (default) | 8 handlers (default) | no (default) | no | Mute or refuse a notification, for example at night. |
| `ilium.core.notifications/channels` | collect | high | server | 0.1.0 (default) | none | 64 entries (default) | no (default) | no | Add a delivery channel such as a chat webhook. |
| `ilium.core.voice/transcript` | hook: transform | high | client | 0.1.0 (default) | 20 ms (default) | 8 handlers (default) | no (default) | no | Correct the transcript before commands are matched. |
| `ilium.core.voice/voice-intents` | collect | standard | client | 0.1.0 (default) | none | 64 entries (default) | no (default) | no | Add a spoken phrase that runs a command. |
| `ilium.core.scheduler/schedule-sources` | collect | standard | server | 0.1.0 (default) | none | 64 entries (default) | no (default) | no | Add a trigger source such as a calendar or a reset forecast. |
| `ilium.core.scheduler/before-send-scheduled` | hook: veto | high | server | 0.1.0 (default) | 20 ms (default) | 8 handlers (default) | no (default) | no | Refuse a scheduled or queued input at the moment it would be sent. |
| `ilium.core.text-triggers/rule-packs` | collect | high | server | 0.1.0 (default) | none | 64 entries (default) | no (default) | no | Ship a group of rules the user can enable as a set. |
| `ilium.core.text-triggers/before-fire` | hook: veto | high | server | 0.1.0 (default) | 20 ms (default) | 8 handlers (default) | no (default) | no | Refuse or end a rule that is about to send text. |
| `ilium.core.progress/footer-segments` | collect | standard | client | 0.1.0 (default) | none | 64 entries (default) | no (default) | no | Add a segment to the progress footer. |
| `ilium.core.progress/probe-kinds` | collect | standard | server | 0.1.0 (default) | none | 64 entries (default) | no (default) | no | Add a ready-made probe, such as a CI run or a build. |
| `ilium.core.attention/item-sources` | collect | standard | server | 0.1.0 (default) | none | 64 entries (default) | no (default) | no | Add a source of attention items. |
| `ilium.core.attention/before-jump` | hook: veto | standard | client | 0.1.0 (default) | 20 ms (default) | 8 handlers (default) | no (default) | no | Refuse a jump the queue is about to make. |
| `ilium.core.resources/resource-kinds` | collect | standard | server | 0.1.0 (default) | none | 64 entries (default) | no (default) | no | Add a kind of resource with its own identity hint. |
| `ilium.core.resources/before-grant` | hook: veto | high | server | 0.1.0 (default) | 20 ms (default) | 8 handlers (default) | no (default) | no | Hold back a grant that the queue is about to make. |
| `ilium.core.chatroom/before-post` | hook: transform | high | server | 0.1.0 (default) | 20 ms (default) | 8 handlers (default) | no (default) | no | Change a message before it is written to the room. |
| `ilium.core.chatroom/message-renderers` | collect | standard | client | 0.1.0 (default) | none | 64 entries (default) | no (default) | no | Draw a message kind in a special way, such as a task claim. |
| `ilium.core.worktrees/before-create` | hook: transform | standard | server | 0.1.0 (default) | 20 ms (default) | 8 handlers (default) | no (default) | no | Choose the branch name or refuse a new worktree. |
| `ilium.core.worktrees/post-create-steps` | collect | high | server | 0.1.0 (default) | none | 64 entries (default) | no (default) | no | Add a step that runs after a worktree is created. |
| `ilium.core.worktrees/before-remove` | hook: veto | standard | server | 0.1.0 (default) | 20 ms (default) | 8 handlers (default) | no (default) | yes | Refuse removing a worktree. |
| `ilium.core.sessions/snapshot-sections` | collect | standard | server | 0.1.0 (default) | none | 64 entries (default) | no (default) | no | Store extra JSON in the session snapshot and receive it back on restore. |
| `ilium.core.sessions/before-restore` | hook: observe | standard | server | 0.1.0 (default) | 20 ms (default) | 8 handlers (default) | no (default) | no | Learn that a restore starts, to prepare state. |
| `ilium.core.compaction/techniques` | collect | standard | server | 0.1.0 (default) | none | 64 entries (default) | no (default) | no | Add a compaction technique. |
| `ilium.core.compaction/before-compact` | hook: veto | standard | server | 0.1.0 (default) | 20 ms (default) | 8 handlers (default) | no (default) | yes | Refuse a compaction, for example while a task is mid-way. |
| `ilium.core.settings/config-importers` | collect | standard | server | 0.1.0 (default) | none | 64 entries (default) | no (default) | no | Offer to import settings from another tool. |
| `ilium.core.settings/onboarding-steps` | collect | standard | client | 0.1.0 (default) | none | 64 entries (default) | no (default) | no | Add a step to the guided setup. |
| `ilium.core.keys-and-help/key-hints` | collect | standard | client | 0.1.0 (default) | none | 64 entries (default) | no (default) | no | Add a hint to the status bar in a context. |
| `ilium.core.keys-and-help/help-sources` | collect | standard | client | 0.1.0 (default) | none | 64 entries (default) | no (default) | no | Add a help source, such as a remote documentation index. |
| `ilium.core.search/sources` | collect | standard | server | 0.1.0 (default) | none | 64 entries (default) | no (default) | no | Add a source of results, such as issues or notes. |
| `ilium.core.search/rank` | hook: transform | standard | server | 0.1.0 (default) | 20 ms (default) | 8 handlers (default) | no (default) | no | Change the order of results before they are shown. |
| `ilium.core.ambient/palettes` | collect | standard | client | 0.1.0 (default) | none | 64 entries (default) | no (default) | no | Add a palette to the shared palette choice. |
| `ilium.core.ambient/style-presets` | collect | standard | client | 0.1.0 (default) | none | 64 entries (default) | no (default) | no | Add a style preset: a full set of shared look values. |
| `ilium.core.scenes/scene-packs` | collect | standard | scene | 0.1.0 (default) | none | 64 entries (default) | no (default) | no | Offer a bundle of scenes with preset controls. |
| `ilium.core.scenes/scene-data-sources` | collect | standard | scene | 0.1.0 (default) | none | 64 entries (default) | no (default) | no | Provide live data to data scenes, such as a feed. |
| `ilium.core.recap/fact-sources` | collect | standard | server | 0.1.0 | none | 16 | no | no | Add facts to a recap, such as the ticket of a branch. |
| `ilium.core.recap/before-recap` | hook: transform | standard | server | 0.1.0 | 20 | 8 | no | no | Change the recap text before it is shown or returned. |
| `ilium.core.session-convert/formats` | collect | standard | server | 0.1.0 | none | 8 | no | no | Add a transcript format that conversion can read or write. |
| `ilium.core.session-convert/after-convert` | hook: observe | standard | server | 0.1.0 | 20 | 8 | no | no | Learn that a conversion finished. |
| `ilium.core.agent-setup/instruction-blocks` | collect | high | server | 0.1.0 | none | 8 | no | no | Add a marker-delimited instruction block that Setup installs. |
| `ilium.core.agent-setup/targets` | collect | high | server | 0.1.0 | none | 16 | no | no | Add a file that Setup can edit, such as a new agent config file. |
| `ilium.core.agent-setup/before-install` | hook: veto | high | server | 0.1.0 | 20 | 4 | no | no | Refuse writing a block to a file. |
| `ilium.core.diagnostics/log-sinks` | hook: observe | high | server | 0.1.0 | 20 | 8 | no | no | Receive log records, for example to forward them. |
| `ilium.core.diagnostics/report-sections` | collect | standard | server | 0.1.0 | none | 32 | no | no | Add a section to the diagnostics report. |

### All services

| Service | What it does |
|---|---|
| `ilium.core.tree/bookmarks` | list, add, remove: read and edit the project bookmarks |
| `ilium.core.layout/presets` | list, apply: list the layout presets and apply one to a group |
| `ilium.core.terminal/transfer` | send-screen: copy the visible screen of one pane to another pane |
| `ilium.core.editor/buffers` | list, read, replace-range: read and edit open buffers |
| `ilium.core.kanban/boards` | list, get, move-card, add-card: read and edit boards |
| `ilium.core.agent-detect/state` | get, list: current identity and activity of panes |
| `ilium.core.agent-toolbar/last-prompt` | get: last prompt text of an agent pane |
| `ilium.core.fleet/broadcast` | send: send a message to a selection |
| `ilium.core.fleet/panes` | list: list panes of every session |
| `ilium.core.cost/usage` | usage: spend and tokens per pane, session or day |
| `ilium.core.titles/suggest` | title: ask for a title suggestion for text |
| `ilium.core.inference/complete` | complete: one-shot completion through the chosen provider |
| `ilium.core.notifications/notify` | send: raise a notice with a title, a body and an urgency |
| `ilium.core.voice/say` | type: type a sentence into the live voice session |
| `ilium.core.scheduler/queue` | enqueue, list, cancel: manage the prompt queue |
| `ilium.core.text-triggers/test-rule` | test: run a pattern against sample text |
| `ilium.core.progress/report` | start, update, finish: report progress of a task the plugin runs |
| `ilium.core.attention/queue` | list, next: read the queue and the agent the next press would visit |
| `ilium.core.resources/reservations` | list, status: read the resources of a project and the state of one reservation |
| `ilium.core.chatroom/post` | send: post a message |
| `ilium.core.chatroom/tail` | get: read recent messages |
| `ilium.core.worktrees/create` | create: create a worktree and return its path |
| `ilium.core.sessions/snapshots` | list, save, restore: manage snapshots |
| `ilium.core.compaction/compact` | start: start a compaction of a pane |
| `ilium.core.settings/get` | get: read a setting by key |
| `ilium.core.settings/set` | set: write a setting by key |
| `ilium.core.keys-and-help/bindings` | list: current key bindings with labels |
| `ilium.core.search/query` | query: search all sources |
| `ilium.core.ambient/look` | get: current shared look and palette |
| `ilium.core.scenes/catalog` | list: list scenes and their controls |
| `ilium.core.recap/recap` | get: short or long recap of panes |
| `ilium.core.session-convert/convert` | start: convert a transcript between two formats |
| `ilium.core.agent-setup/install` | install, remove, status: manage blocks |
| `ilium.core.diagnostics/report` | build: build the diagnostics report |

### All topics

| Topic | Payload |
|---|---|
| `ilium.core.tree/changed` | { node, change } after any structural change |
| `ilium.core.layout/changed` | { group, split_count } after a split is added, removed or resized |
| `ilium.core.terminal/screen-changed` | { pane, first-row, last-row, revision } when visible rows change, at most 10 per second |
| `ilium.core.smart-copy/copied` | { kind, length } after text is copied |
| `ilium.core.editor/saved` | { buffer, path } after a save |
| `ilium.core.kanban/card-moved` | { board, card, to_column } |
| `ilium.core.agent-detect/state-changed` | { pane, signature, previous, current } |
| `ilium.core.fleet/broadcast-sent` | { targets, length } |
| `ilium.core.cost/usage-changed` | { pane, total } when the total of a pane changes |
| `ilium.core.titles/title-applied` | { node, title } |
| `ilium.core.inference/request-finished` | { purpose, provider, millis } |
| `ilium.core.notifications/raised` | { event, pane, urgency } |
| `ilium.core.voice/heard` | { text, command } |
| `ilium.core.scheduler/fired` | { schedule, pane } |
| `ilium.core.text-triggers/fired` | { rule, pane } |
| `ilium.core.progress/changed` | { monitor, status, percent } |
| `ilium.core.attention/changed` | { count, top } |
| `ilium.core.resources/changed` | { resource, state, waiting } |
| `ilium.core.chatroom/posted` | { author, kind, length } |
| `ilium.core.worktrees/changed` | { path, state } |
| `ilium.core.sessions/saved` | { session, bytes } |
| `ilium.core.compaction/finished` | { pane, tokens_before, tokens_after } |
| `ilium.core.settings/changed` | { key } |
| `ilium.core.keys-and-help/changed` | { reason } after a remap |
| `ilium.core.ambient/look-changed` | { palette, mode } |

### Typed schemas

The three tables below give the type of every field. They use one notation: `string`, `integer`, `number`, `boolean`, `list of <type>`, an object in braces, and a choice of quoted values. A field is required unless it is marked (optional). An object accepts no field that its row does not list. Every string is UTF-8. A hook payload, a collect entry and a topic payload carry a node id or a pane id as a string of decimal digits. Service params and results carry it as a JSON integer.

The machine-readable form is JSON Schema. For a point it is the `payload_schema`, `entry_schema` and `reply_schema` of the point in the complete manifest of the owning core plugin page. For a service method and a topic it is the file `core-surface.schema.json` of the SDK, which holds one `params` and one `result` schema for each method and one `payload` schema for each topic, keyed by the full name.

#### Point schemas

A payload or an entry over 64 KiB fails as `extension.cap-exceeded`, and one that does not match its row fails as `extension.schema-mismatch` ([Extension points](/pages/api-extension-points/)). The contributor grant is `extensions.contribute:<point>`.

| Point | Schema of | Fields | Reply fields |
|---|---|---|---|
| `ilium.core.agent-detect/activity-rules` | collect entry | `id`: string; `signature`: string; `state`: string; `regex`: string; `region`: "last-lines" \| "status-row" \| "whole-screen" | none |
| `ilium.core.agent-detect/classify` | hook payload | `pane`: string; `signature`: string; `state`: string; `reason`: string | `state`: string |
| `ilium.core.agent-setup/instruction-blocks` | collect entry | `id`: string; `title`: string; `marker`: string; `file`: string | none |
| `ilium.core.agent-setup/targets` | collect entry | `id`: string; `title`: string; `path_template`: string; `scope`: "global" \| "project" | none |
| `ilium.core.agent-setup/before-install` | hook payload | `target`: string; `block`: string; `path`: string | none |
| `ilium.core.agent-toolbar/toolbar-actions` | collect entry | `id`: string; `signature`: string; `label`: string; `icon`: string; `command`: string; `requires_action`: boolean | none |
| `ilium.core.agent-toolbar/banner-segments` | collect entry | `id`: string; `priority`: integer; `max_width`: integer | none |
| `ilium.core.ambient/palettes` | collect entry | `id`: string; `title`: string; `stops`: list of <string> | none |
| `ilium.core.ambient/style-presets` | collect entry | `id`: string; `title`: string; `values`: object | none |
| `ilium.core.chatroom/before-post` | hook payload | `author`: string; `text`: string; `kind`: string | `text`: string |
| `ilium.core.chatroom/message-renderers` | collect entry | `id`: string; `kind`: string; `view`: string | none |
| `ilium.core.compaction/techniques` | collect entry | `id`: string; `title`: string; `service`: string | none |
| `ilium.core.compaction/before-compact` | hook payload | `pane`: string; `technique`: string; `tokens`: integer | none |
| `ilium.core.cost/price-sources` | collect entry | `id`: string; `model_pattern`: string; `input_per_mtok`: number; `output_per_mtok`: number; `cache_read_per_mtok`: number; `currency`: string | none |
| `ilium.core.cost/quota-sources` | collect entry | `id`: string; `provider`: string; `service`: string | none |
| `ilium.core.diagnostics/log-sinks` | hook payload | `level`: "warn" \| "error"; `plugin`: string; `target`: string; `message`: string; `time`: string | none |
| `ilium.core.diagnostics/report-sections` | collect entry | `id`: string; `title`: string; `service`: string | none |
| `ilium.core.editor/before-save` | hook payload | `buffer`: string; `path`: string; `language`: string; `text`: string | `text`: string |
| `ilium.core.editor/file-openers` | collect entry | `id`: string; `extensions`: list of <string>; `pane_kind`: string; `priority`: integer | none |
| `ilium.core.editor/line-badges` | collect entry | `id`: string; `regex`: string; `glyph`: string; `tooltip`: string | none |
| `ilium.core.fleet/selectors` | collect entry | `id`: string; `title`: string; `rule`: object | none |
| `ilium.core.fleet/before-broadcast` | hook payload | `message`: string; `targets`: list of <string>; `source`: string | `message`: string |
| `ilium.core.inference/before-request` | hook payload | `purpose`: string; `provider`: string; `model`: string; `messages_length`: integer; `text`: string | `text`: string |
| `ilium.core.inference/after-response` | hook payload | `purpose`: string; `provider`: string; `model`: string; `input_tokens`: integer; `output_tokens`: integer; `millis`: integer | none |
| `ilium.core.inference/provider-profiles` | collect entry | `id`: string; `title`: string; `base_url`: string; `models_path`: string; `key_name`: string | none |
| `ilium.core.kanban/card-actions` | collect entry | `id`: string; `title`: string; `command`: string | none |
| `ilium.core.kanban/card-moved` | hook payload | `board`: string; `card`: string; `from_column`: string; `to_column`: string | none |
| `ilium.core.keys-and-help/key-hints` | collect entry | `id`: string; `context`: string; `chord`: string; `label`: string | none |
| `ilium.core.keys-and-help/help-sources` | collect entry | `id`: string; `title`: string; `service`: string | none |
| `ilium.core.layout/layout-presets` | collect entry | `id`: string; `title`: string; `template`: string | none |
| `ilium.core.layout/before-split` | hook payload | `node`: string; `direction`: string; `pane_count`: integer | none |
| `ilium.core.notifications/before-notify` | hook payload | `event`: string; `pane`: string; `title`: string; `urgency`: string; `channels`: list of <string> | none |
| `ilium.core.notifications/channels` | collect entry | `id`: string; `title`: string; `service`: string | none |
| `ilium.core.progress/footer-segments` | collect entry | `id`: string; `priority`: integer; `max_width`: integer | none |
| `ilium.core.progress/probe-kinds` | collect entry | `id`: string; `title`: string; `command_template`: string; `interval_seconds`: integer | none |
| `ilium.core.attention/item-sources` | collect entry | `id`: string; `title`: string; `service`: string | none |
| `ilium.core.attention/before-jump` | hook payload | `pane`: string; `kind`: string; `origin`: string | none |
| `ilium.core.resources/resource-kinds` | collect entry | `id`: string; `title`: string; `identity_hint`: string | none |
| `ilium.core.resources/before-grant` | hook payload | `resource`: string; `reservation`: string; `pane`: string; `estimate_seconds`: integer | none |
| `ilium.core.recap/fact-sources` | collect entry | `id`: string; `title`: string; `service`: string | none |
| `ilium.core.recap/before-recap` | hook payload | `pane`: integer; `kind`: "short" \| "long"; `text`: string, at most 65536 bytes | `text`: string, at most 65536 bytes |
| `ilium.core.scenes/scene-packs` | collect entry | `id`: string; `title`: string; `scenes`: list of <{scene:string,controls:object}> | none |
| `ilium.core.scenes/scene-data-sources` | collect entry | `id`: string; `title`: string; `service`: string | none |
| `ilium.core.scheduler/schedule-sources` | collect entry | `id`: string; `title`: string; `service`: string | none |
| `ilium.core.scheduler/before-send-scheduled` | hook payload | `pane`: string; `text_length`: integer; `schedule`: string; `source`: string | none |
| `ilium.core.search/sources` | collect entry | `id`: string; `title`: string; `kind`: string; `service`: string | none |
| `ilium.core.search/rank` | hook payload | `query`: string; `hits`: list of <{id:string,kind:string,score:number}> | `hits`: list of <{id:string,kind:string,score:number}> |
| `ilium.core.session-convert/formats` | collect entry | `id`: string; `title`: string; `direction`: "read" \| "write" \| "both"; `service`: string | none |
| `ilium.core.session-convert/after-convert` | hook payload | `from`: string; `to`: string; `messages`: integer; `path`: string | none |
| `ilium.core.sessions/snapshot-sections` | collect entry | `id`: string; `max_bytes`: integer | none |
| `ilium.core.sessions/before-restore` | hook payload | `session`: string; `pane_count`: integer | none |
| `ilium.core.settings/config-importers` | collect entry | `id`: string; `title`: string; `service`: string | none |
| `ilium.core.settings/onboarding-steps` | collect entry | `id`: string; `title`: string; `order`: integer | none |
| `ilium.core.smart-copy/recognisers` | collect entry | `id`: string; `kind`: string; `regex`: string; `multiline`: boolean; `label`: string | none |
| `ilium.core.smart-copy/after-copy` | hook payload | `text`: string; `kind`: string; `pane`: string | `text`: string |
| `ilium.core.terminal/before-send-input` | hook payload | `pane`: string; `length`: integer; `source`: string; `preview`: string | none |
| `ilium.core.terminal/open-link` | hook payload | `pane`: string; `link_kind`: string; `text`: string | `action`: "open-external" \| "run-command" \| "ignore"; `command` (optional): string |
| `ilium.core.terminal/link-actions` | collect entry | `id`: string; `link_kind`: string; `title`: string; `command`: string | none |
| `ilium.core.text-triggers/rule-packs` | collect entry | `id`: string; `title`: string; `rules`: list of <{pattern:string,reply:string,enabled:boolean}> | none |
| `ilium.core.text-triggers/before-fire` | hook payload | `rule`: string; `pane`: string; `match`: string; `reply_length`: integer | none |
| `ilium.core.titles/before-apply-title` | hook payload | `node`: string; `proposed`: string; `source`: "transcript" \| "process" \| "manual-trigger"; `previous`: string | `proposed`: string |
| `ilium.core.titles/title-styles` | collect entry | `id`: string; `title`: string; `instruction`: string | none |
| `ilium.core.tree/before-move` | hook payload | `node`: string; `from_parent`: string; `to_parent`: string; `index`: integer; `source`: "mouse" \| "keys" \| "command" \| "plugin" | none |
| `ilium.core.tree/before-close` | hook payload | `node`: string; `kind`: string; `has_children`: boolean | none |
| `ilium.core.tree/node-sorts` | collect entry | `id`: string; `title`: string; `slot`: string | none |
| `ilium.core.voice/transcript` | hook payload | `text`: string; `confidence`: number; `language`: string | `text`: string |
| `ilium.core.voice/voice-intents` | collect entry | `id`: string; `phrases`: list of <string>; `command`: string; `confirm`: boolean | none |
| `ilium.core.worktrees/before-create` | hook payload | `repo`: string; `branch`: string; `base`: string; `agent`: string | `branch`: string |
| `ilium.core.worktrees/post-create-steps` | collect entry | `id`: string; `title`: string; `command`: string; `working_directory`: string | none |
| `ilium.core.worktrees/before-remove` | hook payload | `path`: string; `branch`: string; `dirty`: boolean; `merged`: boolean | none |

#### Service schemas

The caller grant of every method is `services.use:<service name>`, and a replacement provides the name with `services.provide:<service name>`. Params are one JSON object of at most 64 KiB, and a result is one JSON object of at most 1 MiB, unless a row states a lower bound. `none` in the Params column means an empty object. Every method can return the codes of [Services API](/pages/api-services/#edge-cases-and-error-codes): `not-permitted`, `not-found` (no provider), `invalid-argument` (params that do not match the row), `unsupported` (a method that is not in the table), `busy`, `quota-exceeded`, `failed(timed-out)` and `failed(cancelled)`. The Errors column lists what a method adds. The Rate column is the number of calls per second that one caller plugin may make to the method; a call above it returns `rate-limited` with `retry-after-ms`.

| Service | Method | Params | Result | Errors | Rate |
|---|---|---|---|---|---|
| `ilium.core.tree/bookmarks` | `list` | none | `bookmarks`: list of { `node`: integer, `title`: string }, at most 256 | none | 20 |
| `ilium.core.tree/bookmarks` | `add` | `node`: integer | `added`: boolean (false when the node is already bookmarked) | `not-found` (no such node), `quota-exceeded` (256 bookmarks) | 5 |
| `ilium.core.tree/bookmarks` | `remove` | `node`: integer | `removed`: boolean | none | 5 |
| `ilium.core.layout/presets` | `list` | none | `presets`: list of { `id`: string, `title`: string, `template`: string }, at most 64 | none | 20 |
| `ilium.core.layout/presets` | `apply` | `preset`: string; `group`: integer | `pane_count`: integer | `not-found` (preset or group), `failed(conflict)` (a `before-split` handler refused) | 5 |
| `ilium.core.terminal/transfer` | `send-screen` | `from`: integer; `to`: integer | `bytes`: integer | `not-found` (either pane), `failed(conflict)` (a `before-send-input` handler refused) | 5 |
| `ilium.core.editor/buffers` | `list` | none | `buffers`: list of { `buffer`: string, `path`: string, `language`: string, `modified`: boolean, `revision`: integer }, at most 256 | none | 20 |
| `ilium.core.editor/buffers` | `read` | `buffer`: string; `first_line` (optional): integer from 0, default 0; `line_count` (optional): integer 1 to 10000, default 10000 | `text`: string, at most 1 MiB; `revision`: integer; `total_lines`: integer | `not-found` (no such buffer) | 20 |
| `ilium.core.editor/buffers` | `replace-range` | `buffer`: string; `revision`: integer; `start_line`, `start_column`, `end_line`, `end_column`: integer from 0; `text`: string, at most 256 KiB | `revision`: integer (the new revision) | `not-found` (no such buffer), `conflict` (the `revision` is not the current one), `invalid-argument` (the range is outside the buffer) | 5 |
| `ilium.core.kanban/boards` | `list` | none | `boards`: list of { `id`: string, `title`: string, `columns`: list of string }, at most 64 | none | 20 |
| `ilium.core.kanban/boards` | `get` | `board`: string | `id`: string; `title`: string; `columns`: list of { `id`: string, `title`: string, `cards`: list of { `id`: string, `title`: string, `pane` (optional): integer } } | `not-found` (no such board) | 20 |
| `ilium.core.kanban/boards` | `move-card` | `board`: string; `card`: string; `to_column`: string; `index` (optional): integer from 0, default the end | `moved`: boolean | `not-found` (board, card or column) | 5 |
| `ilium.core.kanban/boards` | `add-card` | `board`: string; `column`: string; `title`: string, 1 to 200 characters; `body` (optional): string, at most 16 KiB | `card`: string (the new card id) | `not-found` (board or column), `quota-exceeded` (1000 cards on the board) | 5 |
| `ilium.core.agent-detect/state` | `get` | `pane`: integer | `pane`: integer; `signature`: string, empty when no agent runs; `state`: string, one of the agent state names of [Agent detection](/pages/core-plugin-agent-detect/); `since`: string (RFC 3339 time) | `not-found` (no such pane) | 20 |
| `ilium.core.agent-detect/state` | `list` | none | `panes`: list of the `get` result, one for each pane of the session | none | 20 |
| `ilium.core.agent-toolbar/last-prompt` | `get` | `pane`: integer | `text`: string, at most 64 KiB, empty when no prompt was seen; `time`: string (RFC 3339), empty with an empty text | `not-found` (no such pane) | 20 |
| `ilium.core.fleet/broadcast` | `send` | `selector`: string (a selector id of the `selectors` point) or `targets`: list of integer, at most 256, exactly one of the two; `message`: string, 1 byte to 64 KiB | `sent`: integer; `refused`: integer | `not-found` (selector), `invalid-argument` (both or neither of `selector` and `targets`), `failed(conflict)` (a `before-broadcast` guard refused) | 1 |
| `ilium.core.fleet/panes` | `list` | none | `panes`: list of { `session`: string, `pane`: integer, `title`: string, `signature`: string, `state`: string }, at most 4096 | none | 20 |
| `ilium.core.cost/usage` | `usage` | `window` (optional): "session" \| "day" \| "all", default "session" | `panes`: object that maps a pane id in decimal digits to { `input_tokens`: integer, `output_tokens`: integer, `cache_read_tokens`: integer, `cost`: number, `currency`: string (ISO 4217) } | none | 20 |
| `ilium.core.titles/suggest` | `title` | `text`: string, 1 byte to 64 KiB; `style` (optional): string (an id of the `title-styles` point) | `title`: string, at most 80 characters | `not-found` (style), `unavailable` (no inference provider is set) | 5 |
| `ilium.core.inference/complete` | `complete` | `purpose`: string, 1 to 64 characters; `text`: string, 1 byte to 256 KiB; `max_output_tokens` (optional): integer 1 to 8192, default 1024 | `text`: string, at most 1 MiB; `provider`: string; `model`: string; `input_tokens`: integer; `output_tokens`: integer | `unavailable` (no provider is set or it cannot be reached), `failed(conflict)` (a `before-request` guard refused) | 5 |
| `ilium.core.notifications/notify` | `send` | `title`: string, 1 to 120 characters; `body` (optional): string, at most 1024 characters; `urgency` (optional): "low" \| "normal" \| "critical", default "normal"; `pane` (optional): integer | `raised`: boolean (false when a `before-notify` handler refused or the user muted the event) | `not-found` (pane) | 5 |
| `ilium.core.voice/say` | `type` | `text`: string, 1 to 2000 characters | `typed`: boolean | `unavailable` (no voice session is live) | 5 |
| `ilium.core.scheduler/queue` | `enqueue` | `pane`: integer; `text`: string, 1 byte to 64 KiB; `at` (optional): string (RFC 3339), default the next idle moment of the pane | `item`: string (the queue item id) | `not-found` (pane), `quota-exceeded` (256 queued items) | 5 |
| `ilium.core.scheduler/queue` | `list` | `pane` (optional): integer | `items`: list of { `item`: string, `pane`: integer, `at`: string, `text_length`: integer }, at most 256 | none | 20 |
| `ilium.core.scheduler/queue` | `cancel` | `item`: string | `cancelled`: boolean | none | 5 |
| `ilium.core.text-triggers/test-rule` | `test` | `pattern`: string, 1 to 1024 characters; `sample`: string, at most 64 KiB | `matched`: boolean; `matches`: list of { `start`: integer, `end`: integer }, at most 64 | `invalid-argument` (the pattern does not compile) | 5 |
| `ilium.core.progress/report` | `start` | `title`: string, 1 to 120 characters; `pane` (optional): integer; `total` (optional): integer from 1 | `monitor`: string (the monitor id) | `quota-exceeded` (32 open monitors of the caller) | 5 |
| `ilium.core.progress/report` | `update` | `monitor`: string; `percent` (optional): integer 0 to 100; `done` (optional): integer from 0; `message` (optional): string, at most 200 characters | `updated`: boolean | `not-found` (monitor of another caller or closed) | 10 |
| `ilium.core.progress/report` | `finish` | `monitor`: string; `status`: "done" \| "error" \| "cancelled"; `message` (optional): string, at most 200 characters | `finished`: boolean | `not-found` (monitor) | 5 |
| `ilium.core.attention/queue` | `list` | none | `items`: list of { `pane`: string; `kind`: "question" \| "stuck" \| "waiting-on" \| "changed"; `priority`: integer; `snoozed`: boolean } | none | 10 |
| `ilium.core.attention/queue` | `next` | none | `pane`: string, or empty when the queue is empty | none | 10 |
| `ilium.core.resources/reservations` | `list` | `project` (optional): string | `resources`: list of { `slug`: string; `state`: string; `owner` (optional): string; `waiting`: integer } | `not-found` (project) | 10 |
| `ilium.core.resources/reservations` | `status` | `reservation`: string | `state`: "owned" \| "queued" \| "released" \| "cancelled" \| "needs_attention"; `position`: integer | `not-found` (reservation) | 10 |
| `ilium.core.chatroom/post` | `send` | `text`: string, 1 byte to 16 KiB; `kind` (optional): string, 1 to 32 characters, default "message" | `posted`: boolean; `sequence`: integer | `failed(conflict)` (a `before-post` guard refused) | 5 |
| `ilium.core.chatroom/tail` | `get` | `count` (optional): integer 1 to 200, default 50; `after` (optional): integer (a sequence number) | `messages`: list of { `sequence`: integer, `author`: string, `kind`: string, `text`: string, `time`: string (RFC 3339) } | none | 20 |
| `ilium.core.worktrees/create` | `create` | `repo`: string (absolute path, at most 4096 bytes); `branch`: string, 1 to 255 characters; `base` (optional): string, default the default branch of the repository | `path`: string; `branch`: string (after the `before-create` chain) | `not-found` (repo), `conflict` (the branch or the folder exists), `failed(<code>)` (git failed) | 1 |
| `ilium.core.sessions/snapshots` | `list` | none | `snapshots`: list of { `id`: string, `session`: string, `time`: string (RFC 3339), `bytes`: integer, `pane_count`: integer }, at most 256 | none | 20 |
| `ilium.core.sessions/snapshots` | `save` | `label` (optional): string, at most 80 characters | `id`: string; `bytes`: integer | `quota-exceeded` (the snapshot size limit of [Sessions](/pages/core-plugin-sessions/)) | 1 |
| `ilium.core.sessions/snapshots` | `restore` | `id`: string | `pane_count`: integer | `not-found` (snapshot) | 1 |
| `ilium.core.compaction/compact` | `start` | `pane`: integer; `technique` (optional): string (an id of the `techniques` point), default the user setting | `started`: boolean; `tokens`: integer (the estimate before compaction) | `not-found` (pane or technique), `busy` (a compaction of the pane runs), `failed(conflict)` (a `before-compact` handler refused) | 1 |
| `ilium.core.settings/get` | `get` | `key`: string, 1 to 128 characters | `value`: any JSON value; `scope`: "client" \| "session" \| "project"; `locked`: boolean | `not-found` (no such setting) | 20 |
| `ilium.core.settings/set` | `set` | `key`: string, 1 to 128 characters; `value`: any JSON value, at most 64 KiB | `changed`: boolean | `not-found` (no such setting), `invalid-argument` (the value fails the row check), `not-permitted` (the policy locks the key, or the key belongs to another plugin) | 5 |
| `ilium.core.keys-and-help/bindings` | `list` | `context` (optional): string | `bindings`: list of { `chord`: string, `command`: string, `label`: string, `context`: string }, at most 2048 | none | 20 |
| `ilium.core.search/query` | `query` | `query`: string, 1 to 512 characters; `sources` (optional): list of string (ids of the `sources` point), at most 16; `limit` (optional): integer 1 to 200, default 50 | `hits`: list of { `id`: string, `kind`: string, `score`: number, `title`: string, `pane` (optional): integer } | `not-found` (a source id) | 5 |
| `ilium.core.ambient/look` | `get` | none | `mode`: "color" \| "greyscale" \| "monotone"; `palette`: string (palette id); `stops`: list of string (`#rrggbb`), at most 16; `brightness`, `contrast`, `gamma`: number | none | 20 |
| `ilium.core.scenes/catalog` | `list` | none | `scenes`: list of { `id`: string, `title`: string, `controls`: list of { `id`: string, `title`: string, `kind`: string, `value`: any JSON value } }, at most 256 | none | 20 |
| `ilium.core.recap/recap` | `get` | `panes`: list of integer, at most 64; `length` (optional): "short" \| "long" \| "both", default "short" | `recaps`: list of { `pane`: integer, `short` (optional): string, `long` (optional): string, each at most 64 KiB } | `not-found` (a pane) | 5 |
| `ilium.core.session-convert/convert` | `start` | `pane`: integer; `target`: "codex" \| "claude" | `conversion`: string (the conversion id) | `not-found` (pane), `unsupported` (no transcript of a known format) | 1 |
| `ilium.core.agent-setup/install` | `install` | `block`: string; `target`: string | `status`: "installed" \| "unchanged" | `not-found` (block or target), `failed(conflict)` (a `before-install` handler refused) | 5 |
| `ilium.core.agent-setup/install` | `remove` | `block`: string; `target`: string | `status`: "removed" \| "absent" | `not-found` (block or target) | 5 |
| `ilium.core.agent-setup/install` | `status` | none | `targets`: list of { `target`: string, `path`: string, `blocks`: list of string } | none | 20 |
| `ilium.core.diagnostics/report` | `build` | `include_log_tail` (optional): boolean, default false | `text`: string, at most 256 KiB | none | 1 |

#### Topic schemas

A subscriber lists the topic under `contributes.topics.subscribes` and holds `topics.subscribe`. A payload is one JSON object of at most 64 KiB. The core plugin publishes each topic at most 10 times per second for one subject (one node, pane, board, key or monitor) and merges faster changes into the last one.

| Topic | Fields |
|---|---|
| `ilium.core.tree/changed` | `node`: string; `change`: "added" \| "removed" \| "moved" \| "renamed" \| "locked" \| "unlocked" \| "bookmarked" \| "unbookmarked" |
| `ilium.core.layout/changed` | `group`: string; `split_count`: integer from 0 |
| `ilium.core.terminal/screen-changed` | `pane`: string; `first-row`: integer from 0; `last-row`: integer, not below `first-row`; `revision`: integer |
| `ilium.core.smart-copy/copied` | `kind`: string (a recogniser kind, or "plain"); `length`: integer (bytes) |
| `ilium.core.editor/saved` | `buffer`: string; `path`: string |
| `ilium.core.kanban/card-moved` | `board`: string; `card`: string; `to_column`: string |
| `ilium.core.agent-detect/state-changed` | `pane`: string; `signature`: string; `previous`: string; `current`: string (agent state names) |
| `ilium.core.fleet/broadcast-sent` | `targets`: integer (the number of panes); `length`: integer (bytes) |
| `ilium.core.cost/usage-changed` | `pane`: string; `total`: number (the cost in the currency of the pane) |
| `ilium.core.titles/title-applied` | `node`: string; `title`: string, at most 80 characters |
| `ilium.core.inference/request-finished` | `purpose`: string; `provider`: string; `millis`: integer |
| `ilium.core.notifications/raised` | `event`: string; `pane`: string, empty for a notice of no pane; `urgency`: "low" \| "normal" \| "critical" |
| `ilium.core.voice/heard` | `text`: string, at most 2000 characters; `command`: string (the command id that the sentence matched, else empty) |
| `ilium.core.scheduler/fired` | `schedule`: string; `pane`: string |
| `ilium.core.text-triggers/fired` | `rule`: string; `pane`: string |
| `ilium.core.progress/changed` | `monitor`: string; `status`: "running" \| "done" \| "error" \| "cancelled"; `percent`: integer 0 to 100, or -1 when unknown |
| `ilium.core.attention/changed` | `count`: integer from 0; `top`: string (the pane the next press would visit, or empty) |
| `ilium.core.resources/changed` | `resource`: string; `state`: "free" \| "taken" \| "needs_attention" \| "paused" \| "unavailable"; `waiting`: integer from 0 |
| `ilium.core.chatroom/posted` | `author`: string; `kind`: string; `length`: integer (bytes) |
| `ilium.core.worktrees/changed` | `path`: string; `state`: "created" \| "removed" \| "dirty" \| "clean" \| "merged" |
| `ilium.core.sessions/saved` | `session`: string; `bytes`: integer |
| `ilium.core.compaction/finished` | `pane`: string; `tokens_before`: integer; `tokens_after`: integer |
| `ilium.core.settings/changed` | `key`: string, 1 to 128 characters |
| `ilium.core.keys-and-help/changed` | `reason`: "remap" \| "reset" \| "plugin" \| "import" |
| `ilium.core.ambient/look-changed` | `palette`: string; `mode`: "color" \| "greyscale" \| "monotone" |

- **[PRM-109]** The system must give in the three tables above, and in the machine-readable schemas that this section names, the type, bounds and allowed values of every field of every core service method, hook point, collect point and topic, with the error codes, the grant and the size and rate limits of each service method. The conformance suite must hold one case for each row. The case must validate a real service reply, hook payload, collect entry and topic payload of the bundled core plugin, and of an installed plugin that lists the core plugin in `replaces`, against the schema, and must fail naming the row and the field on any difference.

### Hook modes at a glance

| Mode | Count | Meaning |
|---|---|---|
| `observe` | 5 | Every handler runs; replies ignored. |
| `transform` | 11 | Each handler may replace the payload. |
| `veto` | 12 | Handlers run until one refuses. |
| `first` | 1 | Handlers run until one answers. |

Requirement ids for this page: [EXT-040], [EXT-041], [EXT-042]. See the [requirements index](/pages/reference-requirements/).

### See also

- [Core plugins](/pages/core-plugins/)
- [Extension points](/pages/api-extension-points/)
- [Extending core plugins](/pages/extending-core-plugins/)

### Security

Corrected risk marks: see the table in [Extension security](/pages/reference-extension-security/) ([SEC-280], [SEC-293]).

## Requirement statements

- **[EXT-040]** The core set MUST hold 70 points across 32 core plugins. Hook modes MUST number observe 5, transform 11, veto 12, first 1.
- **[EXT-041]** A point id MUST be <plugin-id>/<point>, where plugin-id is the id of this plugin or the id of a core plugin named in this package's replaces list; such names are accepted only for items listed in that core plugin's surface.
- **[EXT-042]** Every core plugin MUST state whether another plugin may replace it, and its points, services and topics MUST be listed on the core points page.
- **[EXT-050]** A command field in an entry MUST name a command declared by the contributor or one for which it holds commands.invoke, otherwise the install MUST fail with manifest.extension-entry-invalid. An entry that runs a program (worktrees/post-create-steps.command, progress/probe-kinds.command_template) MUST need a matching process.run grant of the contributor (manifest.grant-dependency otherwise).
- **[EXT-052]** Every core point MUST have a Side (server, client or scene), Version, Deadline, Max and Dynamic value in the core points table. A handler declared on the other side MUST fail the install with manifest.extension-side, a handler MUST get the table's deadline, and register on a point whose Dynamic value is no MUST fail with extension.not-dynamic.
