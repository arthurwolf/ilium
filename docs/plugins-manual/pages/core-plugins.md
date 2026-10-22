---
layout: page
title: Core plugins
permalink: /pages/core-plugins/
---

## Core plugins

A core plugin is the plugin form of a feature Ilium has today. Ilium ships and maintains all 30 of them, and each one has an id of the form `ilium.core.<name>`. Where a feature is native to the kernel, such as starting a process in a pane, the terminal emulator, the layout solver, the keymap or snapshots, the page says so and shows the plugin driving that kernel surface through contract version 0.1. See [What a plugin is](/pages/getting-started-overview/) and [Worlds and sides](/pages/worlds-and-sides/) for the background.

<figure>
<svg viewBox="0 0 860 210" role="img" aria-label="The 30 core plugins grouped into five areas">
<text x="8" y="24" font-weight="bold" font-size="13" fill="currentColor">Tree and panes</text>
<rect x="150" y="10" width="96" height="26" rx="6" fill="none" stroke="currentColor"/><text x="198" y="27" text-anchor="middle" font-size="10" fill="currentColor">Tree</text>
<rect x="250" y="10" width="96" height="26" rx="6" fill="none" stroke="currentColor"/><text x="298" y="27" text-anchor="middle" font-size="10" fill="currentColor">Layout</text>
<rect x="350" y="10" width="96" height="26" rx="6" fill="none" stroke="currentColor"/><text x="398" y="27" text-anchor="middle" font-size="10" fill="currentColor">Terminal</text>
<rect x="450" y="10" width="96" height="26" rx="6" fill="none" stroke="currentColor"/><text x="498" y="27" text-anchor="middle" font-size="10" fill="currentColor">Smart Copy</text>
<rect x="550" y="10" width="96" height="26" rx="6" fill="none" stroke="currentColor"/><text x="598" y="27" text-anchor="middle" font-size="10" fill="currentColor">Editor</text>
<rect x="650" y="10" width="96" height="26" rx="6" fill="none" stroke="currentColor"/><text x="698" y="27" text-anchor="middle" font-size="10" fill="currentColor">Kanban</text>
<text x="8" y="64" font-weight="bold" font-size="13" fill="currentColor">Agents</text>
<rect x="150" y="50" width="96" height="26" rx="6" fill="none" stroke="currentColor"/><text x="198" y="67" text-anchor="middle" font-size="10" fill="currentColor">Agent detection</text>
<rect x="250" y="50" width="96" height="26" rx="6" fill="none" stroke="currentColor"/><text x="298" y="67" text-anchor="middle" font-size="10" fill="currentColor">Agent toolbar</text>
<rect x="350" y="50" width="96" height="26" rx="6" fill="none" stroke="currentColor"/><text x="398" y="67" text-anchor="middle" font-size="10" fill="currentColor">Agent setup</text>
<rect x="450" y="50" width="96" height="26" rx="6" fill="none" stroke="currentColor"/><text x="498" y="67" text-anchor="middle" font-size="10" fill="currentColor">Fleet</text>
<rect x="550" y="50" width="96" height="26" rx="6" fill="none" stroke="currentColor"/><text x="598" y="67" text-anchor="middle" font-size="10" fill="currentColor">Cost</text>
<rect x="650" y="50" width="96" height="26" rx="6" fill="none" stroke="currentColor"/><text x="698" y="67" text-anchor="middle" font-size="10" fill="currentColor">Titles</text>
<text x="8" y="104" font-weight="bold" font-size="13" fill="currentColor">AI and alerts</text>
<rect x="150" y="90" width="96" height="26" rx="6" fill="none" stroke="currentColor"/><text x="198" y="107" text-anchor="middle" font-size="10" fill="currentColor">Inference</text>
<rect x="250" y="90" width="96" height="26" rx="6" fill="none" stroke="currentColor"/><text x="298" y="107" text-anchor="middle" font-size="10" fill="currentColor">Notifications</text>
<rect x="350" y="90" width="96" height="26" rx="6" fill="none" stroke="currentColor"/><text x="398" y="107" text-anchor="middle" font-size="10" fill="currentColor">Voice</text>
<rect x="450" y="90" width="96" height="26" rx="6" fill="none" stroke="currentColor"/><text x="498" y="107" text-anchor="middle" font-size="10" fill="currentColor">Scheduler</text>
<rect x="550" y="90" width="96" height="26" rx="6" fill="none" stroke="currentColor"/><text x="598" y="107" text-anchor="middle" font-size="10" fill="currentColor">Text triggers</text>
<text x="8" y="144" font-weight="bold" font-size="13" fill="currentColor">Work and sessions</text>
<rect x="150" y="130" width="96" height="26" rx="6" fill="none" stroke="currentColor"/><text x="198" y="147" text-anchor="middle" font-size="10" fill="currentColor">Progress</text>
<rect x="250" y="130" width="96" height="26" rx="6" fill="none" stroke="currentColor"/><text x="298" y="147" text-anchor="middle" font-size="10" fill="currentColor">Chatroom</text>
<rect x="350" y="130" width="96" height="26" rx="6" fill="none" stroke="currentColor"/><text x="398" y="147" text-anchor="middle" font-size="10" fill="currentColor">Worktrees</text>
<rect x="450" y="130" width="96" height="26" rx="6" fill="none" stroke="currentColor"/><text x="498" y="147" text-anchor="middle" font-size="10" fill="currentColor">Sessions</text>
<rect x="550" y="130" width="96" height="26" rx="6" fill="none" stroke="currentColor"/><text x="598" y="147" text-anchor="middle" font-size="10" fill="currentColor">Compaction</text>
<rect x="650" y="130" width="96" height="26" rx="6" fill="none" stroke="currentColor"/><text x="698" y="147" text-anchor="middle" font-size="10" fill="currentColor">Recap</text>
<rect x="750" y="130" width="96" height="26" rx="6" fill="none" stroke="currentColor"/><text x="798" y="147" text-anchor="middle" font-size="10" fill="currentColor">Session convert</text>
<text x="8" y="184" font-weight="bold" font-size="13" fill="currentColor">Settings and look</text>
<rect x="150" y="170" width="96" height="26" rx="6" fill="none" stroke="currentColor"/><text x="198" y="187" text-anchor="middle" font-size="10" fill="currentColor">Settings</text>
<rect x="250" y="170" width="96" height="26" rx="6" fill="none" stroke="currentColor"/><text x="298" y="187" text-anchor="middle" font-size="10" fill="currentColor">Keys and help</text>
<rect x="350" y="170" width="96" height="26" rx="6" fill="none" stroke="currentColor"/><text x="398" y="187" text-anchor="middle" font-size="10" fill="currentColor">Search</text>
<rect x="450" y="170" width="96" height="26" rx="6" fill="none" stroke="currentColor"/><text x="498" y="187" text-anchor="middle" font-size="10" fill="currentColor">Ambient look</text>
<rect x="550" y="170" width="96" height="26" rx="6" fill="none" stroke="currentColor"/><text x="598" y="187" text-anchor="middle" font-size="10" fill="currentColor">Scenes</text>
<rect x="650" y="170" width="96" height="26" rx="6" fill="none" stroke="currentColor"/><text x="698" y="187" text-anchor="middle" font-size="10" fill="currentColor">Diagnostics</text>
</svg>
<figcaption>The 30 core plugins in five areas (6, 6, 5, 7 and 6 boxes): tree and panes, agents, AI and alerts, work and sessions, settings and look.</figcaption>
</figure>

[COR-050] Each of the 30 pages in the table MUST have a Complete manifest section, and the number of rows in this table MUST equal the number of core-plugin pages. The traceability of every Ilium feature to these plugins is in [feature traceability](/pages/reference-feature-traceability/), and the tests are in [core parity requirements](/pages/reference-requirements-core-parity/).

### All core plugins

| Plugin | Id | Side | Scope |
|---|---|---|---|
| [Tree](/pages/core-plugin-tree/) | `ilium.core.tree` | both | Groups, folders, moving, renaming, bookmarks and locks in the project tree. |
| [Layout](/pages/core-plugin-layout/) | `ilium.core.layout` | both | Split views, mixed splits, focus, navigation and mouse resizing. |
| [Terminal](/pages/core-plugin-terminal/) | `ilium.core.terminal` | both | Pane processes, screen reads, scrollback, input, links and screen transfer. |
| [Smart Copy](/pages/core-plugin-smart-copy/) | `ilium.core.smart-copy` | both | Selection, the terminal right-click menu and Smart Copy. |
| [Editor](/pages/core-plugin-editor/) | `ilium.core.editor` | client | Editor panes, highlighting, Markdown views and the file explorer. |
| [Kanban](/pages/core-plugin-kanban/) | `ilium.core.kanban` | client | Board panes, Markdown-backed boards and their settings. |
| [Agent detection](/pages/core-plugin-agent-detect/) | `ilium.core.agent-detect` | both | Agent identity, activity states, goals and custom signatures. |
| [Agent toolbar](/pages/core-plugin-agent-toolbar/) | `ilium.core.agent-toolbar` | both | Last-prompt banner, toolbar actions and per-agent frame facts. |
| [Fleet](/pages/core-plugin-fleet/) | `ilium.core.fleet` | both | Messaging many agents, ilium panes, broadcast and the local HTTP API. |
| [Cost](/pages/core-plugin-cost/) | `ilium.core.cost` | both | Spend, quotas, session statistics and cost sorting. |
| [Titles and organisation](/pages/core-plugin-titles/) | `ilium.core.titles` | both | Automatic titles, session naming and AI tree organisation. |
| [Inference providers](/pages/core-plugin-inference/) | `ilium.core.inference` | both | AI providers, models, streaming and privacy controls. |
| [Notifications](/pages/core-plugin-notifications/) | `ilium.core.notifications` | both | Sounds, desktop notifications, toasts and attention marks. |
| [Voice](/pages/core-plugin-voice/) | `ilium.core.voice` | both | Voice control, microphone capture and ilium voice say. |
| [Scheduler](/pages/core-plugin-scheduler/) | `ilium.core.scheduler` | both | Scheduled input, the prompt queue and multi-stage input. |
| [Text triggers](/pages/core-plugin-text-triggers/) | `ilium.core.text-triggers` | both | Rules that watch terminal output and send text. |
| [Progress](/pages/core-plugin-progress/) | `ilium.core.progress` | both | Progress monitors, ilium wait and the progress footer. |
| [Chatroom](/pages/core-plugin-chatroom/) | `ilium.core.chatroom` | both | The shared CHATROOM.md, ilium chat and the message view. |
| [Worktrees](/pages/core-plugin-worktrees/) | `ilium.core.worktrees` | both | Agents in Git worktrees and their branch status. |
| [Sessions](/pages/core-plugin-sessions/) | `ilium.core.sessions` | server | Snapshots, restore, recovery and detach and reattach. |
| [Compaction](/pages/core-plugin-compaction/) | `ilium.core.compaction` | both | Context compaction and the compaction optimiser. |
| [Settings](/pages/core-plugin-settings/) | `ilium.core.settings` | client | The Settings screen, auto-save, onboarding and config files. |
| [Keys and help](/pages/core-plugin-keys-and-help/) | `ilium.core.keys-and-help` | client | Prefix, keybindings, remapping, key reference and help topics. |
| [Workspace search](/pages/core-plugin-search/) | `ilium.core.search` | both | One panel to find agents, terminals and files. |
| [Ambient look](/pages/core-plugin-ambient/) | `ilium.core.ambient` | client | Shared colour, palette, dither and display for backgrounds. |
| [Scenes](/pages/core-plugin-scenes/) | `ilium.core.scenes` | scene | The animated background scenes themselves. |
| [Recap](/pages/core-plugin-recap/) | `ilium.core.recap` | both | `ilium recap`: short and long factual recaps of panes. |
| [Session conversion](/pages/core-plugin-session-convert/) | `ilium.core.session-convert` | both | Converting a transcript between Claude and Codex formats. |
| [Agent setup](/pages/core-plugin-agent-setup/) | `ilium.core.agent-setup` | both | Installing the Chatroom and Progress instruction blocks into agent config files. |
| [Diagnostics](/pages/core-plugin-diagnostics/) | `ilium.core.diagnostics` | both | File logging, log levels and the diagnostics report. |

### Which core plugins can be disabled

26 of the 30 core plugins can be disabled. The four essential plugins cannot be: `ilium.core.tree`, `ilium.core.layout`, `ilium.core.settings` and `ilium.core.sessions`. Safe mode keeps the essential set running ([Safe mode and recovery](/pages/user-safe-mode-and-troubleshooting/)). The Plugins screen shows a lock in the "Can you turn it off" column for them. Failures never disable an essential plugin: its third and later failures restart it after 5 s and raise the essential-failure banner ([LIF-042]). [LIF-276]

- [LIF-276] The Plugins screen MUST not offer a disable switch for the four essential plugins, and MUST offer one for every other core plugin, including `ilium.core.editor`. Test: render the list and press Space on each.

### How other plugins work with core plugins

Core plugins use only the public contract, so every way a third-party plugin can reach them is documented. There are five: observe (events and topics), drive (services and commands), extend (fill an extension point), offer (declare your own points) and replace (`replaces`). Every core page ends with an "Extension points" section that lists its points, services, topics and whether it can be replaced. [Core extension points](/pages/reference-core-extension-points/) lists all of them. [Extending core plugins](/pages/extending-core-plugins/) explains when to use which, and [Extension points](/pages/api-extension-points/) is the reference.

### Coverage

Every user-visible feature of Ilium maps to exactly one core plugin. Each plugin page lists the features it covers by their names in the user documentation, so you can start from a feature you know and find the plugin that implements it.

### How the samples teach the ABI

The samples on these pages are short and focused. Together they illustrate the whole ABI: the matrix assigns each ABI area (API pages, widgets, settings row types, services, events and intents) one primary plugin, and that plugin's samples use it. Across the 30 pages the "ABI used" sections hold 394 links to ABI pages (link occurrences, not distinct pages; the same page is linked by several plugins), between 8 and 20 per plugin. Each sample is given in Rust and in TypeScript, and every name in it appears on the linked ABI page. The `calendar` and `api-gpu` areas are unstable and the samples say so.

Renders and samples marked Illustrative are not test oracles. When a page differs from `src/docs` at the baseline commit, `src/docs` wins and the page is a defect. The exact values a test uses are in the "Behaviour contract" table of the pages that have one (currently the scheduler, chatroom and progress pages; the other core-plugin pages state their values in prose): a table with one row per rule, an id of the form `<plugin>-B<nn>`, the exact value or text, and the baseline source (`src/docs/<file>.md#<anchor>`, or the source file of the baseline crate when the user documentation is silent). The test oracle for a plugin is that table, checked against the baseline binary through the golden outputs in [CLI compatibility](/pages/reference-cli-compatibility/).

### Alert channels

The `notifications` core plugin sends sound, desktop notice and in-app text as separate channels. A failure or refusal in one channel MUST NOT drop another, and the in-app text is always sent.

| ID | Rule | Exact value | Source |
|---|---|---|---|
| CAP-885 | Core notifications fan-out | For one event the in-app text is always delivered; a `refused` or `failed` result on the sound or desktop channel is logged and does not remove the other channels | `src/docs/notifications.md` |

### Replacing a core plugin keeps the user contract

A third-party plugin can take over a feature that a core plugin owns. The replacement must keep what the user and other tools already depend on: the CLI commands, the Settings keys and the stored data of the replaced plugin. If it cannot, it is rejected at install or enable time and the core plugin stays active.

- [COR-097] A plugin that replaces a core plugin's owned feature MUST declare every CLI command, Settings key and data section of the replaced plugin, and the host MUST reject the replacement at enable time, with the missing items listed, when any is absent.
- [COR-098] A replacement MUST read the existing stored data of the replaced plugin without loss, and the host MUST run the parity test of the replaced plugin against the replacement before it becomes the owner.
- [COR-099] Disabling or removing a replacement MUST return ownership to the core plugin with the data written during the replacement still readable by it, or the host MUST refuse the replacement at enable time because it cannot guarantee that.

### Security

Replacement of core plugins follows [SEC-300] to [SEC-306] in [Extension security](/pages/reference-extension-security/).
