# Core extension points

## Core extension points

Every core plugin owns extension points, publishes services and topics, and says whether another plugin may replace it. This page lists all 66 points of the 30 core plugins. Each core page repeats its own part under "Extension points".

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

| Point | Kind | Risk | What it does |
|---|---|---|---|
| `ilium.core.tree/before-move` | hook: veto | standard | Refuse a drag or move of a node before the tree applies it. |
| `ilium.core.tree/before-close` | hook: veto | standard | Refuse closing a pane or group, for example while work is unsaved. |
| `ilium.core.tree/node-sorts` | collect | standard | Add a sort order to the tree Order By choice. |
| `ilium.core.layout/layout-presets` | collect | standard | Offer a named split layout in the layout chooser. |
| `ilium.core.layout/before-split` | hook: veto | standard | Refuse a split, for example above a pane count the plugin allows. |
| `ilium.core.terminal/before-send-input` | hook: veto | high | Refuse or end input that the user or a plugin sends to a terminal pane. |
| `ilium.core.terminal/open-link` | hook: first | standard | Decide how a clicked link opens. The first handler that answers wins. |
| `ilium.core.terminal/link-actions` | collect | standard | Offer actions for a detected link kind in the terminal right-click menu. |
| `ilium.core.smart-copy/recognisers` | collect | standard | Teach Smart Copy a new kind of structure to select. |
| `ilium.core.smart-copy/after-copy` | hook: transform | high | Change the copied text before it reaches the clipboard, for example to remove secrets. |
| `ilium.core.editor/before-save` | hook: transform | standard | Change buffer text just before it is written, for example to format it. |
| `ilium.core.editor/file-openers` | collect | standard | Choose which pane kind opens files with certain extensions. |
| `ilium.core.editor/line-badges` | collect | standard | Add a marker in the editor gutter for lines that match. |
| `ilium.core.kanban/card-actions` | collect | standard | Add an action to the card context menu. |
| `ilium.core.kanban/card-moved` | hook: observe | standard | Learn that a card changed column. |
| `ilium.core.agent-detect/activity-rules` | collect | standard | Add screen patterns that mean working, idle or awaiting approval for one signature. |
| `ilium.core.agent-detect/classify` | hook: transform | standard | Adjust the detected activity before it is published. |
| `ilium.core.agent-toolbar/toolbar-actions` | collect | standard | Add a button to the toolbar of panes that run a signature. |
| `ilium.core.agent-toolbar/banner-segments` | collect | standard | Add a short segment to the last-prompt banner. |
| `ilium.core.fleet/selectors` | collect | standard | Add a named selector to `ilium panes`, `ilium broadcast` and the dialogs. |
| `ilium.core.fleet/before-broadcast` | hook: transform | high | Change or end a message before it goes to many agents. |
| `ilium.core.cost/price-sources` | collect | standard | Supply a price table for models the built-in table does not know. |
| `ilium.core.cost/quota-sources` | collect | standard | Supply a plan quota or remaining allowance for a provider. |
| `ilium.core.titles/before-apply-title` | hook: transform | standard | Change a generated title before it is applied. |
| `ilium.core.titles/title-styles` | collect | standard | Add a title style to the style choice. |
| `ilium.core.inference/before-request` | hook: transform | high | Rewrite or refuse an outgoing model request, for example to remove private text. |
| `ilium.core.inference/after-response` | hook: observe | standard | Learn that a request finished, with its token counts. |
| `ilium.core.inference/provider-profiles` | collect | standard | Offer a ready-made provider setup in the provider chooser. |
| `ilium.core.notifications/before-notify` | hook: veto | standard | Mute or refuse a notification, for example at night. |
| `ilium.core.notifications/channels` | collect | standard | Add a delivery channel such as a chat webhook. |
| `ilium.core.voice/transcript` | hook: transform | standard | Correct the transcript before commands are matched. |
| `ilium.core.voice/voice-intents` | collect | standard | Add a spoken phrase that runs a command. |
| `ilium.core.scheduler/schedule-sources` | collect | standard | Add a trigger source such as a calendar or a reset forecast. |
| `ilium.core.scheduler/before-send-scheduled` | hook: veto | high | Refuse a scheduled or queued input at the moment it would be sent. |
| `ilium.core.text-triggers/rule-packs` | collect | high | Ship a group of rules the user can enable as a set. |
| `ilium.core.text-triggers/before-fire` | hook: veto | high | Refuse or end a rule that is about to send text. |
| `ilium.core.progress/footer-segments` | collect | standard | Add a segment to the progress footer. |
| `ilium.core.progress/probe-kinds` | collect | standard | Add a ready-made probe, such as a CI run or a build. |
| `ilium.core.chatroom/before-post` | hook: transform | standard | Change a message before it is written to the room. |
| `ilium.core.chatroom/message-renderers` | collect | standard | Draw a message kind in a special way, such as a task claim. |
| `ilium.core.worktrees/before-create` | hook: transform | standard | Choose the branch name or refuse a new worktree. |
| `ilium.core.worktrees/post-create-steps` | collect | standard | Add a step that runs after a worktree is created. |
| `ilium.core.worktrees/before-remove` | hook: veto | standard | Refuse removing a worktree. |
| `ilium.core.sessions/snapshot-sections` | collect | standard | Store extra JSON in the session snapshot and receive it back on restore. |
| `ilium.core.sessions/before-restore` | hook: observe | standard | Learn that a restore starts, to prepare state. |
| `ilium.core.compaction/techniques` | collect | standard | Add a compaction technique. |
| `ilium.core.compaction/before-compact` | hook: veto | standard | Refuse a compaction, for example while a task is mid-way. |
| `ilium.core.settings/config-importers` | collect | standard | Offer to import settings from another tool. |
| `ilium.core.settings/onboarding-steps` | collect | standard | Add a step to the guided setup. |
| `ilium.core.keys-and-help/key-hints` | collect | standard | Add a hint to the status bar in a context. |
| `ilium.core.keys-and-help/help-sources` | collect | standard | Add a help source, such as a remote documentation index. |
| `ilium.core.search/sources` | collect | standard | Add a source of results, such as issues or notes. |
| `ilium.core.search/rank` | hook: transform | standard | Change the order of results before they are shown. |
| `ilium.core.ambient/palettes` | collect | standard | Add a palette to the shared palette choice. |
| `ilium.core.ambient/style-presets` | collect | standard | Add a style preset: a full set of shared look values. |
| `ilium.core.scenes/scene-packs` | collect | standard | Offer a bundle of scenes with preset controls. |
| `ilium.core.scenes/scene-data-sources` | collect | standard | Provide live data to data scenes, such as a feed. |
| `ilium.core.recap/fact-sources` | collect | standard | Add facts to a recap, such as the ticket of a branch. |
| `ilium.core.recap/before-recap` | hook: transform | standard | Change the recap text before it is shown or returned. |
| `ilium.core.session-convert/formats` | collect | standard | Add a transcript format that conversion can read or write. |
| `ilium.core.session-convert/after-convert` | hook: observe | standard | Learn that a conversion finished. |
| `ilium.core.agent-setup/instruction-blocks` | collect | standard | Add a marker-delimited instruction block that Setup installs. |
| `ilium.core.agent-setup/targets` | collect | standard | Add a file that Setup can edit, such as a new agent config file. |
| `ilium.core.agent-setup/before-install` | hook: veto | high | Refuse writing a block to a file. |
| `ilium.core.diagnostics/log-sinks` | hook: observe | standard | Receive log records, for example to forward them. |
| `ilium.core.diagnostics/report-sections` | collect | standard | Add a section to the diagnostics report. |

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
| `ilium.core.cost/usage` | get: spend and tokens per pane, session or day |
| `ilium.core.titles/suggest` | title: ask for a title suggestion for text |
| `ilium.core.inference/complete` | complete: one-shot completion through the chosen provider |
| `ilium.core.notifications/notify` | send: raise a notice with a title, a body and an urgency |
| `ilium.core.voice/say` | type: type a sentence into the live voice session |
| `ilium.core.scheduler/queue` | enqueue, list, cancel: manage the prompt queue |
| `ilium.core.text-triggers/test-rule` | test: run a pattern against sample text |
| `ilium.core.progress/report` | start, update, finish: report progress of a task the plugin runs |
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
| `ilium.core.chatroom/posted` | { author, kind, length } |
| `ilium.core.worktrees/changed` | { path, state } |
| `ilium.core.sessions/saved` | { session, bytes } |
| `ilium.core.compaction/finished` | { pane, tokens_before, tokens_after } |
| `ilium.core.settings/changed` | { key } |
| `ilium.core.keys-and-help/changed` | { reason } after a remap |
| `ilium.core.ambient/look-changed` | { palette, mode } |

### Hook modes at a glance

| Mode | Count | Meaning |
|---|---|---|
| `observe` | 5 | Every handler runs; replies ignored. |
| `transform` | 11 | Each handler may replace the payload. |
| `veto` | 10 | Handlers run until one refuses. |
| `first` | 1 | Handlers run until one answers. |

Requirement ids for this page: [EXT-040], [EXT-041], [EXT-042]. See the [requirements index](/pages/reference-requirements/).

### See also

- [Core plugins](/pages/core-plugins/)
- [Extension points](/pages/api-extension-points/)
- [Extending core plugins](/pages/extending-core-plugins/)

### Security

Corrected risk marks: see the table in [Extension security](/pages/reference-extension-security/) ([SEC-280], [SEC-293]).
