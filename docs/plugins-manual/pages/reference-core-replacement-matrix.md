---
layout: page
title: Core replacement matrix
permalink: /pages/reference-core-replacement-matrix/
---

## Core replacement matrix

A package can take the place of a core plugin with `"replaces"`. The replacement must then keep what other plugins and the user already depend on. This page lists, for each replaceable core plugin, the surface items its replacement must provide, the stored data it must leave alone, the CLI names it must keep, the settings tabs and commands that disappear while the built-in plugin is off, and the install error codes that a wrong replacement gets.

Requirement ids of this page are `DEV-846` to `DEV-849`. The rules of replacement itself are in [Extending core plugins](/pages/extending-core-plugins/#replacing-a-core-plugin), and the worked example is [tutorial 21](/pages/tutorial-21-replace-a-core-plugin/). The complete list of core points is in [Core extension points](/pages/reference-core-extension-points/).

### You need

- The rules of [Extending core plugins](/pages/extending-core-plugins/#replacing-a-core-plugin): the surface check, one replacement at a time, and what happens on uninstall.
- The [tutorial 21](/pages/tutorial-21-replace-a-core-plugin/) for a complete replacement of `ilium.core.titles`.
- The page of each plugin in the matrix, which is linked from its row.

### Which plugins are in the matrix

The matrix has one row for each core plugin whose page says `**Replaceable:** yes`. There are 28 rows. Two core plugins are not replaceable and have no row: `ilium.core.sessions` and `ilium.core.settings`. A package that lists either one in `replaces` fails with `manifest.replaces-not-replaceable`.

### How to read a row

- **Surface items** are the names that a replacement must provide, written as `kind:name`: `points:` for extension points, `services:` for services, `topics:` for published topics, `commands:` for commands, `pane-kinds:` for pane kinds and `cli:` for CLI commands. The names are the ones after the plugin id, so `services:suggest` means `ilium.core.titles/suggest`. The kinds are the ones that `replaces` checks ([reference manifest](/pages/reference-manifest/#replacement)).
- **Stored data to leave alone** lists the config tables of the plugin's Settings tabs, the snapshot sections and the storage keys that its page names. A replacement MUST NOT read or write them ([DEV-847]). Where a page names no stored key, the cell says `none named`.
- **CLI names to keep** lists the `ilium` subcommands that the plugin owns, as they appear in the [CLI compatibility](/pages/reference-cli-compatibility/) page.
- **Lost while off** lists the Settings tabs, commands, pane kinds and routes that disappear while the built-in plugin is off. The stored data stays on disk and the replacement does not read it.
- **Install error codes** names the code that a missing surface item gives. The other codes of the replacement check are shared by every row and are listed below the matrix.

### The matrix

| Plugin | Surface items to provide | Stored data to leave alone | CLI names to keep | Lost while off | Install error codes |
|---|---|---|---|---|---|
| [Agent detection](/pages/core-plugin-agent-detect/) `ilium.core.agent-detect` | `points:activity-rules`, `points:classify`, `services:state`, `topics:state-changed` | Config tables `[ui]` and `[detection]` (tab Agent Monitoring) | none | Tab Agent Monitoring | `manifest.replaces-surface-incomplete` |
| [Agent setup](/pages/core-plugin-agent-setup/) `ilium.core.agent-setup` | `points:instruction-blocks`, `points:targets`, `points:before-install`, `services:install`, `commands:refresh` | Config table `[agent_setup]`; instruction blocks in the agent files, marked `<!-- ilium-agent-feature: ... -->` | none | Tab Setup; command `refresh` | `manifest.replaces-surface-incomplete` |
| [Agent toolbar](/pages/core-plugin-agent-toolbar/) `ilium.core.agent-toolbar` | `points:toolbar-actions`, `points:banner-segments`, `services:last-prompt`, `commands:compact`, `commands:history` | none named | none | Commands `compact` and `history`; the toolbar actions | `manifest.replaces-surface-incomplete` |
| [Ambient look](/pages/core-plugin-ambient/) `ilium.core.ambient` | `points:palettes`, `points:style-presets`, `services:look`, `services:set-setting`, `topics:look-changed` | Config in the project YAML `animation` (tab Animations); last semantic scene suggestion in global storage | none | Tab Animations; services `look` and `set-setting` | `manifest.replaces-surface-incomplete` |
| [Chatroom](/pages/core-plugin-chatroom/) `ilium.core.chatroom` | `points:before-post`, `points:message-renderers`, `services:post`, `services:tail`, `topics:posted` | `CHATROOM.md` at the project root; hook text in the agent files | `chat` (`init`, `send`, `context`, `tail`) | none | `manifest.replaces-surface-incomplete` |
| [Compaction](/pages/core-plugin-compaction/) `ilium.core.compaction` | `points:techniques`, `points:before-compact`, `services:compact`, `topics:finished`, `commands:compact` | Config tables `[remote_compaction]` (tab Remote compaction); the agent config files `settings.json` and `config.toml`, which the plugin never writes | none | Tabs Optimization and Remote compaction; command `compact` | `manifest.replaces-surface-incomplete` |
| [Cost](/pages/core-plugin-cost/) `ilium.core.cost` | `points:price-sources`, `points:quota-sources`, `services:usage`, `topics:usage-changed` | Config tables `[cost]` and `[reset_planning]`; project storage keys `day/<day>` | none | Tabs Agent Cost and Reset planning | `manifest.replaces-surface-incomplete` |
| [Diagnostics](/pages/core-plugin-diagnostics/) `ilium.core.diagnostics` | `points:log-sinks`, `points:report-sections`, `services:report`, `commands:build-report` | none named | none | Command `build-report` | `manifest.replaces-surface-incomplete` |
| [Editor](/pages/core-plugin-editor/) `ilium.core.editor` | `points:before-save`, `points:file-openers`, `points:line-badges`, `services:buffers`, `services:set-setting`, `topics:saved`, `pane-kinds:editor` | Config table `[editor]` | none | Tab Editor; pane kind `editor` | `manifest.replaces-surface-incomplete` |
| [Fleet](/pages/core-plugin-fleet/) `ilium.core.fleet` | `points:selectors`, `points:before-broadcast`, `services:broadcast`, `services:panes`, `topics:broadcast-sent`, `commands:ask-for-update` | Config table `[api]` (tab API) | `panes`, `broadcast`, `new-pane` | Tab API; command `ask-for-update` | `manifest.replaces-surface-incomplete` |
| [Inference providers](/pages/core-plugin-inference/) `ilium.core.inference` | `points:before-request`, `points:after-response`, `points:provider-profiles`, `services:complete`, `topics:request-finished` | Config tables `[inference]`, `[inference.instructions]` and `[voice]` | none | Tabs Inference and LLM Instructions | `manifest.replaces-surface-incomplete` |
| [Kanban](/pages/core-plugin-kanban/) `ilium.core.kanban` | `points:card-actions`, `points:card-moved`, `services:boards`, `services:set-setting`, `topics:card-moved`, `pane-kinds:board` | Config table `[kanban_board]` | none | Tab Kanban Board; pane kind `board` | `manifest.replaces-surface-incomplete` |
| [Keys and help](/pages/core-plugin-keys-and-help/) `ilium.core.keys-and-help` | `points:key-hints`, `points:help-sources`, `services:bindings`, `topics:changed`, `commands:show-keys`, `commands:show-plugins` | Config tables `[keyboard]` and `[keybindings]` | none | Commands `show-keys` and `show-plugins`; section Plugin keys in tab Keyboard | `manifest.replaces-surface-incomplete` |
| [Layout](/pages/core-plugin-layout/) `ilium.core.layout` | `points:layout-presets`, `points:before-split`, `services:presets`, `topics:changed` | none named | none | none named. Layout is essential, so it cannot be turned off, but it can be replaced | `manifest.replaces-surface-incomplete` |
| [Notifications](/pages/core-plugin-notifications/) `ilium.core.notifications` | `points:before-notify`, `points:channels`, `services:notify`, `topics:raised` | Config tables `[sound]` and `[notifications]`; plugin storage key `muted` | none | Tab Sound | `manifest.replaces-surface-incomplete` |
| [Progress](/pages/core-plugin-progress/) `ilium.core.progress` | `points:footer-segments`, `points:probe-kinds`, `services:report`, `topics:changed` | none named | `progress` (alias `wait`) | Route `GET /progress.json`, which the plugin answers | `manifest.replaces-surface-incomplete` |
| [Recap](/pages/core-plugin-recap/) `ilium.core.recap` | `points:fact-sources`, `points:before-recap`, `services:recap`, `cli:recap` | Stored recap records, in six states; storage key not named | `recap` | none named | `manifest.replaces-surface-incomplete` |
| [Scenes](/pages/core-plugin-scenes/) `ilium.core.scenes` | `points:scene-packs`, `points:scene-data-sources`, `services:catalog` | none named | none | none named | `manifest.replaces-surface-incomplete` |
| [Scheduler](/pages/core-plugin-scheduler/) `ilium.core.scheduler` | `points:schedule-sources`, `points:before-send-scheduled`, `services:queue`, `topics:fired` | Snapshot section `ilium.core.scheduler` with `scheduled_input` and `prompt_queue` per pane | none | none named | `manifest.replaces-surface-incomplete` |
| [Workspace search](/pages/core-plugin-search/) `ilium.core.search` | `points:sources`, `points:rank`, `services:query`, `commands:open` | none named | none | Command `open` and its keybinding | `manifest.replaces-surface-incomplete` |
| [Session conversion](/pages/core-plugin-session-convert/) `ilium.core.session-convert` | `points:formats`, `points:after-convert`, `services:convert`, `commands:convert` | Session storage key `convert/<pane id>` | none | Command `convert`; its context menu entry | `manifest.replaces-surface-incomplete` |
| [Smart copy](/pages/core-plugin-smart-copy/) `ilium.core.smart-copy` | `points:recognisers`, `points:after-copy`, `topics:copied`, `commands:smart-copy` | none named | none | Command `smart-copy`; its context menu entry | `manifest.replaces-surface-incomplete` |
| [Terminal](/pages/core-plugin-terminal/) `ilium.core.terminal` | `points:before-send-input`, `points:open-link`, `points:link-actions`, `services:transfer`, `topics:screen-changed` | Config table `[terminal]` | none | Tab Terminal | `manifest.replaces-surface-incomplete` |
| [Text triggers](/pages/core-plugin-text-triggers/) `ilium.core.text-triggers` | `services:test-rule`, `points:rule-packs`, `points:before-fire`, `topics:fired` | Config table `[text_triggers]` (tab Text Triggers) | none | Tab Text Triggers, output rules on every agent pane | `manifest.replaces-surface-incomplete` |
| [Titles and organisation](/pages/core-plugin-titles/) `ilium.core.titles` | `points:before-apply-title`, `points:title-styles`, `services:suggest`, `topics:title-applied` | Config tables `[inference]` (tab Titles) and `[triggers]` (tab Triggers) | none | Tabs Titles and Triggers; the tree-tidy action and the preview panel ([tutorial 21](/pages/tutorial-21-replace-a-core-plugin/)) | `manifest.replaces-surface-incomplete` |
| [Tree](/pages/core-plugin-tree/) `ilium.core.tree` | `points:before-move`, `points:before-close`, `points:node-sorts`, `services:bookmarks`, `topics:changed` | none named | none | none named. Tree is essential, so it cannot be turned off | `manifest.replaces-surface-incomplete` |
| [Voice](/pages/core-plugin-voice/) `ilium.core.voice` | `points:transcript`, `points:voice-intents`, `services:say`, `topics:heard`, `commands:voice.toggle` | Config table `[voice]` (tab Voice control) | `voice` | Tab Voice control; command `voice.toggle` | `manifest.replaces-surface-incomplete` |
| [Worktrees](/pages/core-plugin-worktrees/) `ilium.core.worktrees` | `points:before-create`, `points:post-create-steps`, `points:before-remove`, `services:create`, `services:remove`, `topics:changed`, `commands:new-agent` | Config table `[git]` (tab Git) | none | Tab Git; command `new-agent` | `manifest.replaces-surface-incomplete` |

### Install error codes

Every replacement is checked with the same codes, whichever row it replaces. The surface row above names the one that a missing item gives.

| Code | When it is given |
|---|---|
| `manifest.replaces-unknown` | The id is not a core plugin. |
| `manifest.replaces-not-replaceable` | The id is `ilium.core.sessions` or `ilium.core.settings`. |
| `manifest.replaces-surface-incomplete` | A surface item of the row is missing. The error lists every missing item. |
| `manifest.replaces-cycle` | Two packages replace each other. |
| `manifest.replaces-tier` | The package is neither bundled nor trusted, so it may not replace a core plugin ([Extension security](/pages/reference-extension-security/), SEC-301). |
| `manifest.schema` | The list has more than 4 ids, or a duplicate id. |

### Requirements

- [DEV-846] A replacement of a core plugin in the matrix MUST provide every surface item of its row, with the same names, and each point and service MUST keep the same kind and the same major version as the plugin it replaces. A replacement that misses an item MUST fail at install with `manifest.replaces-surface-incomplete`, and the error MUST list each missing item.
- [DEV-847] A replacement MUST NOT read, write or delete the config tables, snapshot sections and storage keys named in its row. Its stored data MUST stay on disk unchanged when the built-in plugin comes back.
- [DEV-848] A replacement MUST keep every CLI name listed in its row. Each name MUST give the arguments, output and exit codes of the CLI reference, so that `ilium chat context --since-last-read` and the other names used by scripts and agent hooks keep working.
- [DEV-849] The matrix MUST have one row for each core plugin whose page says `**Replaceable:** yes`, and for no other plugin. Each row MUST match the points, services, topics, commands and pane kinds of that page.

### Open decisions

1. **Requirement ids DEV-850 and DEV-851 are already in use.** They belong to the performance requirements of [Performance](/pages/performance/) and are registered in the dev tooling register. This page uses only `DEV-846` to `DEV-849`. Choice: keep the assigned range and use four ids. Consequence: if the owner needs six ids, the two extra ids must come from `DEV-854` onward, outside the range that was assigned.
2. **Most core manifests do not declare a surface.** [Reference manifest](/pages/reference-manifest/#replacement) and PKG-029 say that a replaceable core plugin carries a `surface` list. Only 6 of the 28 core pages show one. Choice: the rows are derived from each page's complete manifest and from its services and topics lists. Consequence: if the core manifests are changed to declare a surface, every row must be checked against that declaration, and any difference is a defect in this page.
3. **Most pages name no stored key.** Where a page names no storage key or snapshot key, the cell says `none named`. Choice: list only what the pages name. Consequence: a replacement may still touch data that no page names. A complete list needs the storage keys from the plugin code.
4. **Kanban tab id differs.** The manifest of [Kanban](/pages/core-plugin-kanban/) names the settings tab `kanban`, while its kernel tab table names `kanban-board`. Choice: the row uses `kanban-board`, the id of the kernel tab. Consequence: a check that looks for the tab `kanban` will not find the tab that is lost while the plugin is off.
5. **Two plugins share the config table `[inference]`.** The Titles tab and the Inference tab both name `[inference]` in their kernel tab tables. Choice: both are listed as stored data. Consequence: a replacement of one plugin must not change the keys that the other one reads, and the owner of each key needs to be stated on the kernel page.
6. **Titles commands are not in the manifest.** The core [Titles](/pages/core-plugin-titles/) manifest declares no command, but [tutorial 21](/pages/tutorial-21-replace-a-core-plugin/) says the tree-tidy action goes away with the plugin. Choice: the row follows the tutorial. Consequence: if the manifest is the reference, the tree-tidy action drops out of the row.
7. **Command `compact` is declared twice.** [Agent toolbar](/pages/core-plugin-agent-toolbar/) and [Compaction](/pages/core-plugin-compaction/) both declare it. Choice: both rows list it. Consequence: replacing one of them leaves the other one answering `compact`. This page does not decide which owner keeps the name.
8. **The install code list is not the same in two places.** [Extending core plugins](/pages/extending-core-plugins/#replacing-a-core-plugin) lists four codes. The [manifest schema](/pages/reference-manifest-schema/) also lists `manifest.replaces-tier`. Choice: this page lists five codes and `manifest.schema`. Consequence: a reader who copies only the four-code list will not expect the tier refusal.

### See also

- [Extending core plugins](/pages/extending-core-plugins/) for the five ways to work with a core feature.
- [Tutorial 21: Titles Lite](/pages/tutorial-21-replace-a-core-plugin/) for a complete replacement.
- [CLI compatibility](/pages/reference-cli-compatibility/) for the CLI names of each owner.
- [Kernel and boot](/pages/reference-kernel-and-boot/) for what the user keeps when a core plugin is off.
- [Core extension points](/pages/reference-core-extension-points/) for the points, services and topics of every core plugin.
