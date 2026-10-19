# Roadmap

## Roadmap

The plugin platform is built in ordered phases. Phases 0 to 6 make up version 1 (v1). Phase 7, which moves the native editor and the native board into plugins, is the first milestone after v1. This page lists every phase, the gate it must pass before the next phase opens, the rule that keeps v1 plugins working after v1, and the decisions that closed the open questions of the plan.

A gate is an ordered list of checks. A phase opens only when every check in its gate passes. A feature reaches plugins in the Ilium release that closes its phase.

<figure>
<svg viewBox="0 0 400 220" role="img" aria-label="Gate flow for each phase">
  <defs>
    <marker id="roadmap-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
      <path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="20" y="20" width="150" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="95" y="44" text-anchor="middle" font-size="11" fill="currentColor">Build the phase</text>
  <rect x="230" y="20" width="150" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="305" y="44" text-anchor="middle" font-size="11" fill="currentColor">All gate checks pass?</text>
  <line x1="170" y1="40" x2="230" y2="40" stroke="currentColor" marker-end="url(#roadmap-arrow)"/>
  <rect x="230" y="100" width="150" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="305" y="124" text-anchor="middle" font-size="11" fill="currentColor">Next phase opens</text>
  <line x1="320" y1="60" x2="320" y2="100" stroke="currentColor" marker-end="url(#roadmap-arrow)"/>
  <text x="328" y="84" font-size="10" fill="currentColor">yes</text>
  <rect x="20" y="100" width="150" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="95" y="124" text-anchor="middle" font-size="11" fill="currentColor">Fix, then re-run</text>
  <path d="M 260 60 V 80 H 140 V 100" fill="none" stroke="currentColor" marker-end="url(#roadmap-arrow)"/>
  <text x="268" y="76" font-size="10" fill="currentColor">no</text>
  <line x1="60" y1="100" x2="60" y2="60" stroke="currentColor" marker-end="url(#roadmap-arrow)"/>
  <text x="66" y="84" font-size="10" fill="currentColor">re-run</text>
  <rect x="20" y="168" width="360" height="34" rx="6" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
  <text x="200" y="189" text-anchor="middle" font-size="11" fill="currentColor">After v1, a contract change may only add items.</text>
</svg>
<figcaption>Each gate runs in order. A failed check sends the work back to the phase, and the next phase waits.</figcaption>
</figure>

## What you need

You need no tools to read this page. To write a plugin against a phase, you need:

- The API version `ilium:plugin@0.1.0`. The manifest `api` key is `"0.1.0"`, and it must match the component's `ilium:api-version` section.
- For Rust plugins: stable Rust 1.90 or later with the `wasm32-wasip2` target, and the crate `ilium-plugin-sdk`.
- For TypeScript plugins (from Phase 3): Bun 1.2 or later and the package `@ilium/plugin-sdk`. The SDK pins its own component tools, so nothing is installed globally.
- An Ilium release that carries the phase you use. Features of a later phase are not in an earlier release.

## Phases

| Phase | Name | What it delivers | Gate |
|---|---|---|---|
| 0 | Decide and measure | Records the decisions, measures the benchmarks against their targets, and proves the toolchain path | Decisions recorded, benchmark numbers recorded, toolchain path proven |
| 1 | Registries | Replaces each closed enum with a registry that has native implementations | All existing tests, clippy and fmt pass. No visible behaviour change |
| 2 | Contract and host | The WIT packages, the plugin host, the Rust SDK and the first live contract | A Rust hello plugin installs from a package, receives events, sets a decoration, contributes a command and a settings page, survives a forced trap, and hot-reloads in dev mode |
| 2A | Ratatui-mirror UI API | The `ui` import, the `ui-tree` model, host-owned widget state, the popup stack and theme tokens | A widget gallery plugin, in Rust and in TypeScript, renders every widget case, and the golden buffers match for both |
| 2B | Pane primitives | The `terminal`, `agent`, `editor` and `basic` primitives | A board prototype builds a three-column card board as a basic pane kind, with drag and drop, persists it, and shows it again after a server restart |
| 2C | Scoped file access, codec and jobs | The `fs` import, the preopens for every scope, codec helpers and background jobs | Escape tests pass: `..`, absolute paths, symlinks and hard links out of every scope are refused, for direct calls and for `std::fs` through preopens |
| 3 | TypeScript build path | The TypeScript build, the `@ilium/plugin-sdk` package and the animation contract | The same conformance fixtures pass for the Rust and the TypeScript guests on Wasmtime |
| 4a to 4e | Early feature migrations | Features move from the kernel into first-party plugins in five waves | The common wave gate, plus the extra gate of each wave (see below) |
| 5 | Late feature migrations | Features with crash-safety contracts move last | Crash-safety tests pass for each feature (see below) |
| 6 | Third-party readiness | Package signing and the trust store, the review dialog for every capability, the `ilium plugin` commands, and the complete guides | A plugin written only from the published docs, by an agent with no access to the Ilium source, builds, installs and passes its own tests, in Rust and in TypeScript |
| 7 | Editors and boards as plugins (after v1) | The editor and the board move to plugins | See the Phase 7 section |

Phase 2A comes before Phase 2B because a basic pane's body is a `ui-tree`, so the widget API must exist before the pane primitives are built on it.

## The common wave gate

Every migration wave in Phase 4 must pass this gate before the next wave starts:

- The feature behaves as before, and its existing tests pass after they are ported.
- The frame and processing budgets in the [performance page](/pages/performance/) are met with every wave plugin loaded.
- Crash isolation holds. Kill a plugin's instance during `on-events`. The server keeps answering IPC within 50 ms, the plugin restarts after 1 s, and a third kill within 10 minutes disables it with a notice.

The crash rule replaces the older "second crash within 60 s" wording, so the restart rule below is the one that applies.

## Phase 4 waves

| Wave | Features | Extra gate |
|---|---|---|
| 4a | Pilots: agent toolbar buttons (client side), sound and notification selection rules (server side), one extra agent signature through the signature registry, text triggers limited to notify and sound actions, and tree sort-key providers | None beyond the common gate |
| 4b | Presentation: agent messaging, cost, titles and naming, Smart Copy, terminal links and context menu, the git status line, chatroom views, goal tracking, bookmarks, locks, workspace search, help and guided setup | None beyond the common gate |
| 4c | Portable scenes that need only the `package` and `cache` scopes, with no network and no media service, then inference providers as services. The V8 animation runtime is retired at the end of this wave | Each scene's output matches the native scene by the parity method |
| 4d | File-reading, caching and live-data scenes: live-data scenes, the maps and Wikipedia scenes, and image scenes, through `http`, the picked and template file scopes, `cache`, `codec` and jobs | Each scene matches its native output within the frame budget. With the network off, live-data scenes show cached or empty state, never an error loop. The cache stays within the `data` plus `cache` limit and evicts the oldest entries first |
| 4e | Saved-world scenes: the voxel scene reads saved worlds through the `minecraft-saves` scope, `codec.inflate` and jobs | A fixture saved world renders identically to the native voxel scene. Parsing runs in a job and never blocks a frame. The plugin cannot read any file outside the saves scope |

## Wave gate tests and what stays native

Each wave has one named gate test and a list of what stays in the kernel while the wave runs. The core plugins that each wave delivers are listed in [feature traceability](/pages/reference-feature-traceability/) by wave. [COR-060] [COR-061]

| Wave | Core plugins delivered | Gate test | What stays native |
|---|---|---|---|
| 4a | Agent toolbar buttons, Notifications rules, Text triggers (notify and sound actions), agent signatures, tree sort keys | `wave-4a-parity`: replay the baseline agent fixtures and compare toolbar, sounds and tree order | PTY, tree model, detection loop timing, sound output device |
| 4b | Fleet, Cost, Titles, Smart Copy, Terminal links, Worktrees, Chatroom, Progress, Recap, Voice, Scheduler, Sessions, Compaction, Settings, Keys and help, Search, Diagnostics, Agent setup, Session convert, Tree, Layout | `wave-4b-cli-and-settings`: run every example of the CLI reference and compare every settings tab with the baseline | Config store, snapshot writer, keymap, HTTP listener, recovery page |
| 4c | Inference, portable scenes, Ambient look | `wave-4c-scene-output`: each scene output equals the native scene by checksum | Look painter, dither, scene host, raster |
| 4d | Live-data, maps, Wikipedia and image scenes | `wave-4d-scene-budget`: native output within the frame budget | Scene host, network egress policy |
| 4e | Voxel scene | `wave-4e-saved-world`: fixture saved world renders identically | Scene host |
| 7 | Editor, Kanban | `wave-7-editor-board-parity`: editor and board scenarios of the user documentation | Pane kinds registry, file system scopes |

The editor and the board stay native until Phase 7. Their pages ([editor](/pages/core-plugin-editor/), [kanban](/pages/core-plugin-kanban/)) describe the plugin form and say so at the top.

## Phase 5 and crash safety

Phase 5 needs two kernel additions first. The keys of `send-input` are journaled in the snapshot before the PTY write, so a server restart does not repeat input. Progress of a decoration is persisted in the decoration store.

Each feature in Phase 5 passes its own gate. Kill the server in the middle of the operation, restart it, and check that no delivery is duplicated or lost. The features include scheduled input, the prompt queue, progress monitors, worktree workflows, session resume and conversion, voice, the file explorer on the basic primitive, the settings UI with its kernel recovery page, and the chatroom router.

## Phase 6: what v1 ships

- Packages are files that work with no network. They are signed for tier T2. An optional static registry protocol (decision D27) adds search, update and revocation. Ilium ships no registry and runs no central store. An unsigned or tampered T2 package is refused with a message that names the failing check.
- A separate sandbox helper process runs every T2 and T3 plugin on Linux. A T2 plugin that opens a file outside its scopes, opens a socket or spawns a process without a grant is denied by the helper.
- On Windows and macOS, installing or loading a T2 or T3 package is refused with a message that names the missing sandbox. Bundled T1 plugins keep working on every platform.
- The v1 contract is frozen at the end of Phase 6. The WIT packages are snapshotted, and from then on only `@since` additions are allowed.

<pre class="term">
<span class="t-prompt">$</span> ilium plugin install ./agent.iliumpkg
<span class="t-err">error</span> manifest.signature-invalid  bad signature
<span class="t-dim">step</span>  install stopped before any code was loaded
</pre>

Illustrative, not generated. A tampered T2 package stops at the signature check, and the error code names the failing check.

## Phase 7: editors and boards as plugins

Phase 7 is the first milestone after v1. It moves the native editor and the native board into plugins built on the `editor` and `basic` primitives.

- An editor plugin provides the editor primitive. It covers editing, saving and autosave, the minimap, line numbers, syntax highlighting through an annotator, large-file source windows, and the Markdown source and rendered views. It is added as the export `editor-provider` with the grant `editor.provide`. The provider receives the open buffers with `adopt` and returns them with `release`, and while it runs the kernel routes every other plugin's `editor` calls to it.
- A board plugin, built on the basic primitive, covers kanban boards and boards created from Markdown files.
- The native editor and board code is removed once both plugins pass. The kernel keeps only the `code-view` hot path, which serves both the native provider and the plugin provider.
- A snapshot written by v1 with open editor and board panes restores into the plugin-provided panes, with the same files, cursor positions and board contents.

The Phase 7 gate is strict:

- No ABI break. A WIT diff between the v1 snapshot and the Phase 7 packages shows only additions marked `@since`. Every v1 conformance fixture and every first-party v1 plugin loads and passes unchanged.
- The editor and board feature tests from v1 pass against the plugin versions.
- Keystroke latency in the editor and board stays within the performance budget, measured with the same harness as Phase 2B.

<figure>
<svg viewBox="0 0 360 150" role="img" aria-label="Phase timeline to v1 and after">
  <defs>
    <marker id="roadmap-timeline-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
      <path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <line x1="10" y1="60" x2="300" y2="60" stroke="currentColor" marker-end="url(#roadmap-timeline-arrow)"/>
  <line x1="236" y1="20" x2="236" y2="110" stroke="currentColor" stroke-dasharray="4 3"/>
  <text x="236" y="16" text-anchor="middle" fill="currentColor" font-size="10">v1</text>
  <circle cx="20" cy="60" r="6" fill="none" stroke="currentColor"/>
  <circle cx="54" cy="60" r="6" fill="none" stroke="currentColor"/>
  <circle cx="88" cy="60" r="6" fill="none" stroke="currentColor"/>
  <circle cx="122" cy="60" r="6" fill="none" stroke="currentColor"/>
  <circle cx="156" cy="60" r="6" fill="none" stroke="currentColor"/>
  <circle cx="190" cy="60" r="6" fill="none" stroke="currentColor"/>
  <circle cx="224" cy="60" r="6" fill="none" stroke="currentColor"/>
  <text x="20" y="84" text-anchor="middle" fill="currentColor" font-size="10">0</text>
  <text x="54" y="84" text-anchor="middle" fill="currentColor" font-size="10">1</text>
  <text x="88" y="84" text-anchor="middle" fill="currentColor" font-size="10">2</text>
  <text x="122" y="84" text-anchor="middle" fill="currentColor" font-size="10">3</text>
  <text x="156" y="84" text-anchor="middle" fill="currentColor" font-size="10">4</text>
  <text x="190" y="84" text-anchor="middle" fill="currentColor" font-size="10">5</text>
  <text x="224" y="84" text-anchor="middle" fill="currentColor" font-size="10">6</text>
  <circle cx="282" cy="60" r="6" fill="none" stroke="currentColor" stroke-dasharray="3 2"/>
  <text x="282" y="84" text-anchor="middle" fill="currentColor" font-size="10">7</text>
  <rect x="8" y="104" width="232" height="30" rx="6" fill="none" stroke="currentColor"/>
  <text x="124" y="123" text-anchor="middle" fill="currentColor" font-size="10">Each gate passes before the next</text>
  <rect x="256" y="104" width="96" height="30" rx="6" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
  <text x="304" y="123" text-anchor="middle" fill="currentColor" font-size="10">Additive only</text>
</svg>
<figcaption>Phases 0 to 6 form v1. Phase 7 follows v1, and it may only add items, never change the v1 surface.</figcaption>
</figure>

## Contract additions by phase

The contract has 42 additions, AX1 to AX42, so that plugins can express every Ilium feature. They are part of the one contract. Each addition lands in the phase whose features need it, with conformance fixtures and a Rust guest and a TypeScript guest in the same change. An addition that ships before the Phase 6 freeze is part of the v1 contract. One that does not ship by then stays unreleased and breaks nothing. The decisions D12 to D25 are final (see [Design decisions](/pages/design-decisions/)).

<pre class="term"><span class="t-hi">╭──≡ ● · Roadmap · gates─────────────────────────────────────────────────╮</span>
│ [x] Phase 0  Decide and measure …………………………………………………………………… gate passed │
│ [x] Phase 1  Registries ………………………………………………………………………………………… gate passed │
│ [ ] Phase 2  Contract and host ……………………………………………………………………… in progress │
│ [ ] Phase 2A Ratatui-mirror UI API …………………………………………………………… waits for 2 │
│ [ ] Phase 2B Pane primitives ………………………………………………………………………… waits for 2A │
│                                                                        │
│ Gate 2 · high-risk grants refused   <span class="t-ok">━━━━━━━</span><span class="t-dim">─────</span> 3 of 5                │
│                                                                        │
│ <span class="t-dim">Illustrative layout of a gate checklist, not a live status.</span>            │
╰────────────────────────────────────────────────────────────────────────╯
</pre>

| Phase | Additions placed here |
|---|---|
| 2 | The nine new contribution keys and `limits_tier` with their 13 manifest error codes. The 19 new grants, the new argument forms and the high-risk rule. `catalogs` and the host catalogues. The new settings rows and row-state calls. `kernel-settings.read` and `keymap.read`. Command `category` and `keywords`, the new `when` keys. The intents `toast`, `dismiss-toast` and `open-external`. `client-requests`. The `shared` cache bucket |
| 2A | The 19 new widgets and their props. Hover, tooltips, anchored and pinnable popovers, focus order, view pointer events and drag feedback. The menu model and the `menu-provider` export. Rich spans. The `location-picker` widget and the `location` and `preview` rows. The widened settings-view subset. Theme tokens for reduced motion, density and icon mode |
| 2B | Terminal cell reads, history, snapshots and modes. Pane lifecycle intents. Tree structure intents and `restructure` with undo. Title provenance, inferred icons and attention. Tree and chrome contribution points, status segments, frame slots and the `virtual-rows-provider` export. Rich decorations. Staged, raw and mouse input, preconditions, the input lock and interactive `terminal-view`. Agent model, prompt epoch and typed transcripts. Editor viewport and minimap marks. Basic-pane activity and dirty flag |
| 2C | `fs` revisions, atomic writes and tail, and the Gemini and Antigravity agent scopes. HTTP streams and hosts taken from a setting. `process.spawn` with the `child` resource |
| 3 | Surface v2: glyph plane, owner ids with receipts, font handles and pre-rendered loops. The `geocode` and `catalogs` imports in the animation world |
| 4c to 4d | `vector` embeddings once inference providers are services. Live-data and map scenes use HTTP streams and the shared cache |
| 5 | Durable timers, the pane journal and journaled `send-input` with `input-uncertain`. `sessions.list-live` and `sessions.send`. Video from package bytes and file ranges, and the `video` widget with the media scenes |
| 7 | The `token-provider` export and `languages` contributions move with the editor plugin. Before v1 they may stay `@unstable` |
| Not scheduled | `gpu` stays `@unstable(feature = gpu-kernels)` and GPU scenes stay native. WebSocket stays `@unstable` |

### Extra gates for the additions

- **Phase 2.** Every high-risk grant of the additions is refused without an explicit grant, for a T1 fixture too. `limits_tier: "first-party"` on a package that is not a bundled `ilium.*` plugin is refused with `manifest.limits-tier`. A `settings.read-kernel:api` read never returns a secret.
- **Phase 2A.** The widget-gallery fixture renders all 51 widgets in Rust and TypeScript with matching golden buffers. The `location-picker` and the `location` row render cell for cell like the native world-map picker. The filterable choice dialog over `ilium.icons` pages in as the user scrolls and filters. Tooltips, spinner frames and decoration frames make no plugin call. A settings view that embeds a refused widget is refused with `invalid-argument`. Leaders follow the `…` rule: grey, one blank cell each side, blank under 5 cells, never part of a hit target.
- **Phase 2B.** A failing operation inside `restructure` leaves the tree unchanged, and `revert-restructure` restores the previous structure. The input lock never exceeds 2 s and no user key is lost. `resize-pane` refusals name the limiting neighbour. An inferred title never replaces a manual one.
- **Phase 5.** Killing the server between a journaled `send-input` write and its outcome produces exactly one `input-uncertain` after restart and no replay. A durable timer due while Ilium was stopped fires once with its lateness.

<pre class="term"><span class="t-prompt">$</span> ilium plugin install ./other.iliumpkg
<span class="t-err">error</span> manifest.limits-tier  limits_tier first-party refused
<span class="t-dim">step</span>  install stopped before any code was loaded
</pre>

Illustrative, not generated. The first Phase 2 gate check for `limits_tier` fails like this on a third-party package.

One risk is open. The additions bring 13 host interface groups and 19 widgets, so the placement above ships each group only with the feature that uses it. The outcome `ok-epoch` stays a separate case of `outcome`.

## The additive-only rule after v1

After v1, a contract change may only add items. Each addition carries `@since(version = <version>)` with the version that introduced it. Nothing exported in v1 is altered or removed. This covers the editor and basic primitives, the widget variant cases, and the event and intent cases. New widget cases are appended to the variant. A change that would alter or remove a v1 item is redesigned rather than shipped.

Plugin ids are part of the public surface too. Pane kind ids, command ids, settings keys, decoration slots and topic names keep their spelling across plugin versions. The kernel stores them as strings in snapshots.

Compatibility works in two steps:

- The host loads the current minor version and the previous one through hand-written adapters.
- A plugin built for a newer minor than the host is refused at install. The message names both versions.

Experimental items carry `@unstable(feature = <name>)` instead. Such an item is visible only to a plugin whose manifest lists the feature in the `unstable` key. In 0.1 there are two of them: `ui-calendar`, which enables the `calendar` widget, and `plugin-services`, which enables `services.call`. The `chart` widget is stable.

## The restart rule

The restart rule applies to each plugin on each side. On the client side, each client counts separately. A failing view counts against its plugin, not against the view.

| Event | Result |
|---|---|
| First failure | The plugin restarts after 1 s |
| Second failure | The plugin restarts after 5 s |
| Third failure within 10 minutes | The plugin is disabled on that side, and a notice names it |

A failure counts for 10 minutes and then expires. A successful run does not clear it sooner. A trap or a missed deadline counts as a failure. A `revoked` re-activation, a disable chosen by the user, an upgrade and a shutdown never count. Re-enabling the plugin by hand resets the count to zero.

## Decisions that closed the open items

These answers replace the questions that the plan left open. Each one is recorded in the decisions file with its identifier.

| Question | Answer | Decision |
|---|---|---|
| Background-job model | Every job runs in a second instance of the component, in its own store. The plugin's own instance is never used | A21, X5 |
| TypeScript compiler | `componentize-js`. `componentize-qjs` is not used | U8 |
| Optional exports | Checked against the component's export list. The manifest lists only the optional exports | N9 |
| Settings and payload schemas | A JSON-Schema subset, draft 2020-12, checked by one pinned validator | N28 |
| Resource limits | The 0.1 defaults in the [errors and limits page](/pages/reference-errors-and-limits/). A measured value may replace one, and the change is dated in the decisions file | Section 3 |
| `calendar` and `chart` | `chart` is stable. `calendar` stays unstable behind `ui-calendar` | W19, N33 |
| Prompt text for plugins without the grant | Nothing is sent, not even a length or a hash | C3 |
| Key-release events | Delivered only when the terminal supports the kitty keyboard protocol | C45 |
| `device.gpu` | Not part of the contract (`gpu.kernels` is `@unstable`). A manifest that names it is refused with `manifest.grant-unknown` | Added by page writers (`porting-iliumanim`) |

### See also

- [Design decisions](/pages/design-decisions/)
- [Versioning](/pages/versioning/)
- [Security model](/pages/security/)
- [Errors and limits](/pages/reference-errors-and-limits/)
- [Performance](/pages/performance/)
- [Glossary](/pages/glossary/)

Checked against the API version `ilium:plugin@0.1.0`.
