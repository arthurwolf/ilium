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

## Production additions: T6-core-plugins

- [COR-203] Every core plugin page MUST contain a Contract section listing exactly: owned services, owned topics, owned extension points, owned commands, owned CLI names, owned stored sections, and the essential/disableable/replaceable flags, and these MUST equal the manifest and the replacement matrix row.
- [COR-204] Each of the 30 core plugins MUST ship a conformance test suite named ilium-core-<name>-parity that runs against the plugin or its replacement and covers every Contract entry, and the host MUST fail the build with exit code 1 when a core plugin has no suite.
- [COR-205] The command ilium plugin conformance <id> MUST run the parity suite of the named core plugin against the currently active owner, print one line per test as pass or fail, and exit 0 only when all pass.
- [COR-208] Disabling a core plugin that other enabled plugins require MUST be refused with core.has-dependents listing every dependent, unless the request carries cascade=true, in which case dependents MUST be disabled in reverse dependency order and be re-enabled together on enable.
- [COR-209] Disabling or enabling a disableable core plugin MUST take effect within 2 seconds without restarting the server, MUST drop the plugin's UI contributions in the same frame as deactivation, and MUST emit one plugin.state-changed event.
- [COR-210] Every stored section of a core plugin MUST carry an integer schema_version, and a plugin MUST refuse to load a section whose version is greater than its own with error core.data-newer, leaving the section unmodified.
- [COR-213] For each row of the fallback table, an automated test MUST disable the plugin with a client attached and assert that the named fallback behaviour is visible in the next rendered frame and that no pane input is lost.
- [COR-220] A core plugin MUST NOT use a kernel-private API: every host call it makes MUST be available to a T1 third-party plugin holding the same capability, and a build-time test MUST fail on any import not in the public WIT.
- [COR-221] Core plugins MUST hold only the capabilities in their manifest; ilium plugin permissions <id> for a core plugin MUST list exactly those and the user MUST NOT be able to grant extra capabilities to a core plugin.
- [COR-222] The bundled core plugins MUST be verified against a hash table compiled into the server at startup, and a mismatch MUST abort loading that plugin with core.integrity-failed and, for an essential plugin, fall back to the kernel fallback.
- [COR-223] With no client attached the server MUST keep all startup core plugins running and MUST NOT activate client-side UI contributions; attaching a client MUST activate them within 1 second.
- [COR-224] A non-startup core plugin MUST activate on its first trigger within 300 ms plus its own activation time, and the trigger event MUST be delivered to it exactly once after activation.
- [COR-225] Each core plugin instance MUST run under the same memory and fuel limits as T2 plugins unless its manifest declares a larger limit, and the declared limits MUST be listed in ilium plugin info <id>.
- [COR-226] A degraded core plugin MUST NOT have its stored sections modified or deleted by the kernel, and its last committed values MUST remain readable by other plugins through the read-only service snapshot.
- [COR-228] If a replacement of a core plugin fails 3 times within 60 seconds after becoming owner, the kernel MUST return ownership to the original core plugin, mark the replacement failed with core.replacement-rolled-back, and post a notice.
- [COR-229] At most one enabled plugin MAY replace a given core plugin id; enabling a second MUST be refused with core.replacement-conflict naming the current owner.
- [COR-230] Core plugins MUST be versioned with the server build; at boot the server MUST refuse an installed override of a core plugin whose declared host ABI is incompatible, falling back to the bundled copy and logging core.abi-mismatch.

### Out-of-band fixes for core plugins

There is no core-plugin patch channel. [PKG-234] stands: a package whose `id` starts with `ilium.` is refused with `manifest.id-reserved`, even with a valid signature, and core plugins change only with a new Ilium release. No `ilium.core.*` package is accepted from the store, a file, a URL or the host release channel of [PKG-953], and no release key signs one ([SEC-426] covers host binaries only).

- **Delivery path.** A fix to a core plugin ships inside a host patch release. [DEV-966] requires that patch for every host minor still inside its support period ([SEC-966], [DEV-964]: at least 12 months), and a patch MUST NOT change any WIT interface, limit, error code or manifest key. The patched plugin reaches users with the binary, through the release channel and rollout of [PKG-953].
- **Timeline.** The fix-to-release time is bounded by the disclosure rules of [Security response](/pages/reference-security-response/): acknowledgement within 3 working days, embargo of at most 90 days from the report or until a fix is released ([SEC-321]), and a patched Wasmtime within 14 days of the upstream fix, 48 hours when exploited in the wild ([SEC-331]). Sandbox escapes are rated Critical ([HST-520]).
- **What [COR-230] means.** It does not create an update path. It guards against a stale or foreign copy of a core plugin found in the plugin store, for example left by an older host. At boot the server compares the declared host ABI of any such copy; when it is incompatible, the server uses the copy bundled with the running binary and logs `core.abi-mismatch`. A compatible copy is still not used: the bundled copy must match the hash table of [COR-222], and a mismatch gives `core.integrity-failed`.
- **Mitigation before the release.** Until the patch ships, the user can disable a disableable core plugin ([COR-209], effective within 2 seconds) or, for the four essential plugins, use safe mode ([Safe mode and recovery](/pages/user-safe-mode-and-troubleshooting/)). The advisory names the plugin id and the workaround.
- **Failure modes.** A patched build whose core plugin fails first activation is a release defect: the user keeps the previous binary, since the installer keeps the earlier version ([Host release verification](/pages/reference-host-release-verification/)); the kernel fallback of [Replaceable and disableable](/pages/reference-kernel-and-boot/#replaceable-and-disableable) covers the essential plugins at run time.

Test: place a copy of `ilium.core.titles` with an incompatible ABI in the plugin store and assert `core.abi-mismatch` and bundled behaviour; install a package with id `ilium.core.titles` and assert `manifest.id-reserved`.

### Retiring a core plugin

The project may remove one of the 30 bundled core plugins in a host release. It follows the same notice rule as a plugin feature ([DEV-885]) and the end-of-life display of [PKG-710].

1. **Notice.** The plugin is marked `deprecated` for at least two minor releases before the release that drops it. The mark shows in the Plugins tab (a "Deprecated" badge with the removal release and the successor id, if any), in `ilium plugin list`, and in `ilium plugin deprecations`, and the release notes name it. A deprecated plugin keeps working and keeps its capabilities.
2. **Dropping release.** The release removes the bundled plugin. The count of core plugins and the table above change in the same release, and [COR-050] is updated with them.
3. **Stored data.** Nothing is deleted. The plugin's snapshot sections, Settings keys and project buckets stay on disk and are kept unmodified ([HST-692]). `ilium plugin doctor` lists them as orphaned data ([HST-696]), and `ilium plugin export` writes them out. Removal of orphaned data is a user action under [LIF-290], never automatic.
4. **Dependants.** A plugin that `requires` the retired id, or contributes to its points, goes dormant with the notice `core.retired` naming the id and the successor; nothing is deleted and it resumes if the id returns. When a successor core plugin takes over the surface, the release names it and the surface ids are handed over unchanged, so contributions keep working ([EXT-051]).
5. **Offer as a package.** The project MAY publish the retired plugin as a T2 package under a non-reserved id (never `ilium.`, [PKG-234]). The package is a new plugin: it does not inherit the stored sections automatically, and the user imports them with the migration steps of [LIF-958].

A retired id cannot be reused by a third party, because `ilium.` stays reserved. Listing a retired core plugin in `replaces` fails with `manifest.replaces-unknown`.

### Contract sections: the ten domain pages that still lack one

[COR-203] is not yet met on the pages of Terminal, Agent detection, Titles, Settings, Ambient look, Scenes, Notifications, Tree, Layout and Keys and help: none has a `### Contract` heading. Until each page has one, [COR-204] and [COR-205] have nothing to check for them. Each page MUST add the section in this form:

| Row | Content |
|---|---|
| Owned services | `services:` items of the matrix row |
| Owned topics | `topics:` items |
| Owned extension points | `points:` items |
| Owned commands | `commands:` items |
| Owned CLI names | `cli:` items and the CLI names column |
| Owned stored sections | the "Stored data" column, as snapshot section ids and Settings tables |
| Flags | `essential`, `disableable`, `replaceable` |
| Parity suite | `ilium-core-<name>-parity` |

Values come from the [replacement matrix](/pages/reference-core-replacement-matrix/) and [Kernel and boot](/pages/reference-kernel-and-boot/#replaceable-and-disableable):

| Page | Services / topics / points | Commands, CLI | Stored sections | Flags (essential / disableable / replaceable) | Suite |
|---|---|---|---|---|---|
| Terminal | `transfer`; `screen-changed`; `before-send-input`, `open-link`, `link-actions` | none | Settings table `[terminal]` | no / yes / yes | `ilium-core-terminal-parity` |
| Agent detection | `state`; `state-changed`; `activity-rules`, `classify` | none | `[ui]`, `[detection]` | no / yes / yes | `ilium-core-agent-detect-parity` |
| Titles | `suggest`; `title-applied`; `before-apply-title`, `title-styles` | none | `[inference]` (Titles), `[triggers]` | no / yes / yes | `ilium-core-titles-parity` |
| Settings | the Settings screen services | none | owns the config files | yes / no / no | `ilium-core-settings-parity` |
| Ambient look | `look`, `set-setting`; `look-changed`; `palettes`, `style-presets` | none | project YAML `animation` | no / yes / yes | `ilium-core-ambient-parity` |
| Scenes | `catalog`; none; `scene-packs`, `scene-data-sources` | none | see below | no / yes / yes | `ilium-core-scenes-parity` |
| Notifications | `notify`; `raised`; `before-notify`, `channels` | none | `[sound]`, `[notifications]`, key `muted` | no / yes / yes | `ilium-core-notifications-parity` |
| Tree | `bookmarks`; `changed`; `before-move`, `before-close`, `node-sorts` | none | see below | yes / no / yes | `ilium-core-tree-parity` |
| Layout | `presets`; `changed`; `layout-presets`, `before-split` | none | see below | yes / no / yes | `ilium-core-layout-parity` |
| Keys and help | `bindings`; `changed`; `key-hints`, `help-sources` | `show-keys`, `show-plugins` | `[keyboard]`, `[keybindings]` | no / yes / yes | `ilium-core-keys-and-help-parity` |

Settings is not replaceable and has no matrix row ([DEV-846] row list); its Contract states `replaceable: false` and lists the settings screen and config file ownership from the kernel page.

**Cells that read `none named` (matrix open decision 2).** Tree, Layout and Scenes own the following stored sections, which the Contract MUST name so the parity suite can check them: Tree owns the tree and bookmarks sections of the snapshot (section id `ilium.core.tree`); Layout owns the layout section (`ilium.core.layout`, split ratios and focus order); Scenes owns the scene choice and scene control values, stored in the project YAML `animation` table shared with Ambient look, with the scene section id `ilium.core.scenes` for per-pane scene state. Where the baseline stores nothing for a plugin, the Contract says `none` and the suite asserts no section is written.

**Undeclared `surface` (matrix open decision 3).** A core manifest that declares `replaceable: true` MUST carry a `surface` equal to the matrix row's items; the Contract section and the manifest are compared by the suite, and a difference fails it ([COR-203]). A manifest without `surface` for a replaceable plugin fails the build.

Test: for each of the ten pages, `rg '### Contract'` finds one heading and `ilium plugin conformance <id>` ([COR-205]) runs the named suite and covers every row.

### Platform notes per core plugin

The user-visible behaviour of these plugins differs by platform. The support matrix, terminals and colour rules are in [Platform support](/pages/reference-platform-support/) ([HST-389], [HST-390]). The tested platforms are Linux (x86_64, aarch64), macOS (aarch64) and Windows (x86_64); all core plugins are T1 and run on all three ([HST-381]).

| Plugin | Linux | macOS | Windows |
|---|---|---|---|
| Terminal | PTY from the OS | PTY from the OS | ConPTY; the terminal host sets key reports and colour depth |
| Agent detection | Process-tree walk through procfs | Process-tree walk through the system process list | Process-tree walk through the Windows process snapshot; process names carry `.exe` and are matched without it |
| Notifications | Desktop notice through the desktop notification service; sound through the system audio backend | The system notification centre; the system audio backend | The Windows toast service; the system audio backend; a missing backend gives `refused` on that channel only ([CAP-885]) |
| Scenes | GPU backend if the host offers one, otherwise CPU | Same rule | Same rule; the `api-gpu` area is unstable |
| Ambient look | Colour depth set by the outer terminal | Terminal.app may offer fewer colours than iTerm2 | Windows Terminal; colours are mapped to the nearest the terminal offers ([HST-390]) |

A plugin whose platform backend is absent reports it in `ilium plugin info`; it does not fail the other platforms' behaviour. T2 and T3 plugins are refused on macOS and Windows with `sandbox.unavailable` ([HST-380]).

### Replacement and the replaced plugin's data: one rule

[COR-098] (the replacement reads the replaced plugin's data without loss) and [SEC-300] with [SEC-306] (a replacement never inherits data, and the replaced plugin's data folder stays inaccessible) are reconciled by splitting the data in two classes.

**Transfers on replacement** (the user contract of [COR-097]): snapshot sections declared under the replaced section ids, Settings keys and tables, and buckets keyed by the surface id of the replaced plugin (the "Stored data to leave alone" column of the [matrix](/pages/reference-core-replacement-matrix/)).

**Never transfers**: the replaced plugin's private `data/<id>/` folder ([SEC-306]), its secrets, and its grants and consents ([SEC-300]).

**How the host hands data over.** The transferable stores are not copied. The host gives the replacement a read-write view of those stores under the surface ids, not under the replaced plugin's id, and the view is the same storage the core plugin uses. Because one storage is shared, nothing needs to flow back: on `replacement-off` ([COR-099]) ownership returns to the core plugin and everything written during the replacement is readable by it. Each store keeps its `schema_version`; a section newer than the core plugin reads fails `core.data-newer` ([COR-210]) and the replacement is then refused at enable time, as [COR-099] requires. The audit log records `replacement-on` and `replacement-off` ([SEC-306]) and the store hand-over as one `replacement-data-view` record naming the stores.

**Parity test ([COR-098]).** The suite writes sample data with the core plugin, enables the replacement, reads every transferable store and compares it byte for byte, writes new data, disables the replacement, and asserts the core plugin reads both. It also asserts that the replacement opening `data/<core id>/` fails with a permission error and that no secret or grant is visible.

## Localisation of core plugin text

This section sets the rules for the text the 30 core plugins show, the times and numbers they display, and whether their detection depends on language. The authoring rules for third-party plugins are in [Internationalisation](/pages/guide-internationalisation/) and [Localisation and text direction](/pages/api-ui/#localisation-and-text-direction). The rules below apply them to core plugins. They add no requirement IDs.

### Which text is translatable

Translatable text is every title, label, help text, notice, toast body and error message that a person reads in a core plugin view or a Settings row. Each one has a message id in the plugin's own catalogue, `i18n/<tag>.json`, a flat object of message id to string, as in [api-ui](/pages/api-ui/#localisation-and-text-direction). English is the fallback for every id.

These stay in English in every language:

- identifiers: plugin ids such as `ilium.core.chatroom`, command ids, setting keys, error codes such as `core.data-newer`, event and topic names, stored section names, grant ids and capability names;
- CLI command names, flags and output lines, such as `chatroom message sent` ([chatroom](/pages/core-plugin-chatroom/)), and log lines, because scripts, hooks and agents match them;
- text that people or agents write: chatroom messages, notes, branch names and agent output. This is data, shown exactly as written.

Lookup order is the exact tag, then the language without region, then `en`. A missing id shows the English text and never the id. If a catalogue fails to load, the core plugin shows English. The permission review and the recovery page follow the same rule, and the recovery page is always in English ([user permissions](/pages/user-permissions-explained/)).

Settings row titles and help come from the plugin manifest, which has one value, so they stay in the manifest language, English. This follows the rule for third-party settings in [Internationalisation](/pages/guide-internationalisation/#your-own-strings).

### Language setting

The User Interface tab of [Settings](/pages/core-plugin-settings/) has theme, motion, density and stable glyphs. It has no language choice in this release, so core plugins show English. If a language choice is added, it belongs to the Settings plugin, and each core plugin reads it through [settings](/pages/api-settings/) and then uses the lookup order above.

### Times

- **Stored times are UTC.** Plugin data keeps UTC milliseconds, never local text ([Internationalisation](/pages/guide-internationalisation/#time)). The audit log `ts` is UTC ([audit log](/pages/reference-audit-log/)), and crash records hold the time in UTC as RFC 3339 ([diagnostics](/pages/core-plugin-diagnostics/)).
- **Absolute times are shown as `YYYY-MM-DD HH:mm:ss`** with the zone written out, in every language. A relative form such as `3 min ago` is allowed for recent events.
- **Chatroom records are the one local-time exception.** A record line is `- YYYY-MM-DD HH:mm:ss +HH:MM | <author> | <content>`, local time with its offset ([chatroom-B01](/pages/core-plugin-chatroom/)). The offset keeps the time unambiguous for agents that read `CHATROOM.md` on other machines. Changing this format is a change of the file format, not a translation.
- **Log file names** use the local date and time (`log-<local date and time>.txt`, [diagnostics](/pages/core-plugin-diagnostics/)). They are names for people to sort, not timestamps to parse.

### Numbers, money and units

- The decimal mark is a dot and there is no group separator, in the UI as well as in machine-readable output. This release has no decimal-mark setting.
- Money is stored as integer cents and shown with two decimal places and its currency sign, such as `$6.12` ([cost](/pages/core-plugin-cost/)). Rounding happens only when formatting.
- Sizes use metric units. Token counts use a `k` suffix, such as `412k` ([cost](/pages/core-plugin-cost/)).

### Detection and display language

Detection does not depend on the language that Ilium shows.

- **Agent identity uses process names**, such as `process_names` in the signature registry ([agent detection](/pages/core-plugin-agent-detect/)). Process names do not change with language.
- **Activity states come from the text an agent CLI prints**, matched by the built-in signatures and by `activity-rules` regular expressions ([signatures](/pages/capability-agent-signatures/)). The built-in patterns are English, for example `esc to interrupt` for working and `Do you want to proceed?` for approval. They match the agent's output, so an agent that prints in another language may not match them. A user then adds an `activity-rules` entry with a regular expression for that wording.
- **The interstitial auto-answer rule** matches the English text of Claude Code's "resume full session" prompt ([agent detection](/pages/core-plugin-agent-detect/)), with the same limit.

### Tests for core plugin text

- Load a test catalogue and check that a core view and a Settings row use its text.
- Remove one message id from a catalogue and check that the English text is shown and the id is not.
- Break a catalogue and check that the recovery page is in English.
- Once a language choice exists, run the English detection fixtures with that language selected and check that the classification does not change. This confirms that detection reads the agent's output and not the display language.
