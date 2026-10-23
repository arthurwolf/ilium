---
layout: page
title: Rendering pipeline for plugins
permalink: /pages/reference-host-rendering/
---

## Rendering pipeline for plugins

The client draws on every frame. A plugin never draws on a frame. This page defines how plugin output reaches the screen: retained contributions, views, annotations, token providers and scenes. It defines which thread does what, how often a plugin can be asked to render, what the client shows while a plugin is slow, stale or gone, and the size caps. Requirement ids are `[HST-250]` to `[HST-272]` and `[HST-495]`.

## You will need

- [Retained mode](/pages/retained-mode/) for the plugin author's view of the same model.
- [Host scheduling](/pages/reference-host-scheduling/) for threads and deadlines.
- [Plugin traffic over IPC](/pages/reference-host-ipc/) for how server-owned contributions reach the client.

## The rule

The client UI thread paints from **data it already holds**. It reads the mirrored tree, the retained contribution mirror, the last `ui-tree` of each open view, and the last complete scene frame. It never waits for a plugin and never calls Wasmtime. When a plugin is slow, the UI shows the previous result. When a plugin is gone, the UI shows the previous result marked stale.

- **[HST-250]** The client MUST paint every frame from held data only, and a frame MUST NOT be late because a plugin call is pending.
- **[HST-251]** A plugin MUST NOT be asked to render per frame. It is asked when its view opens, after `ui.invalidate`, after a change of the view's area (a resize), after a theme change, and after a user action in the view.

## Retained contributions

| Kind | Owner | Stored in | Drawn by |
|---|---|---|---|
| Decorations (badges, tints, markers) | server or client plugin | Contribution registry (server), mirror (client) | Tree view, per row |
| Status segments, frame slots | server or client plugin | Same | Status bar, frame |
| Virtual rows | server plugin | Same | Tree view |
| Row actions, toolbar buttons | server or client plugin | Same | Tree and pane chrome |
| Annotations (text spans) | client plugin | Client only | Terminal and editor views |

A plugin replaces the whole contribution list of one kind per call, or sends per-id puts and removes. The server (or the client, for client-owned kinds) validates it against the caps below and publishes a delta (see [Plugin traffic over IPC](/pages/reference-host-ipc/)).

| Cap | Value |
|---|---|
| Decorations per plugin | `decorations` limit: 4,096 by default, 16,384 for T1 at most |
| Decorations per node per plugin | 8 |
| Status segments per plugin | 2 |
| Virtual rows per parent | 1,024 |
| Entry size | 16 KiB |
| Contribution update rate | Status segments and frame slots: 10 per second per slot. Info, choices, row notes, row and item states, choice items, rich info and meters: 10 per second per plugin, shared together |

Over a cap the update is refused: `contribution.too-many` for a count cap, except the decorations count cap, which returns `refused(quota-exceeded)` ([errors and limits](/pages/reference-errors-and-limits/)); `contribution.too-large` for an entry size; or `rate-limited`. The earlier contributions stay.

- **[HST-252]** Retained contributions MUST be held by the owner of the point (server registry or client store) and drawn from a copy in the client.
- **[HST-253]** The host MUST enforce the contribution caps above and MUST keep the previous contributions on refusal.

### Stale contributions

A plugin that traps, is disabled, is restarted or is waiting for a helper keeps its contributions on screen. The host marks them **stale**: drawn dimmed with a marker in the health view. They are cleared in these cases only: the plugin replaces or clears them; the plugin is disabled by the user; the plugin is uninstalled; or the stale state lasts 60 s after the plugin side stops running without a restart scheduled.

- **[HST-254]** The host MUST mark the contributions of a stopped plugin side as stale and MUST clear them at the end of 60 s without a scheduled restart, at uninstall, and at user disable.
- **[HST-255]** A stale contribution MUST NOT accept user actions that would call the plugin; the action shows "plugin is restarting" instead.

## Views

A view is a panel, a pane body, a popup or a settings screen that a plugin describes with a `ui-tree`. The host calls `views.render` (client side) with the view id, the size in cells and the view state. The plugin returns a `ui-tree`.

Render triggers, in order of when they are coalesced:

| Trigger | Coalescing |
|---|---|
| View opened | Immediate |
| `ui.invalidate` | Several calls before the render starts become one render |
| Area change (resize) | One render with the new area, coalesced with the other triggers; at most one render per view per 16 ms. Until it returns, the host lays out the previous tree in the new area through the same constraint solver, so the screen never shows a stale size |
| Theme or font change | The host repaints the last tree of every open view at once with the new role colours, delivers `theme-changed`, and triggers exactly one render per open view, coalesced with any other trigger in the same 16 ms window. The plugin need not call `ui.invalidate` |
| User action in the view | The `ui-action` event is delivered first; the render follows after `on-events` returns |

The client draws the last tree until the new tree arrives. The new tree replaces it between frames, never partway through a frame. The render deadline is `render_deadline_ms`: 10 ms by default, 20 ms at most. A render that exceeds it is interrupted by epoch interruption and traps with `trap.deadline`, and the logged reason is `ui.render-timeout`, which counts as one failure under the restart rule. The view keeps its previous tree, drawn with the stale marker, until a later render succeeds.

The tree is diffed by the client against the previous tree, keyed by node `key`. Only changed subtrees are re-laid out.

| Cap | Value |
|---|---|
| `ui-tree` size | `ui_tree_kib`: 1,024 KiB by default, 4,096 KiB for T1 at most |
| Nodes per tree | `ui_tree_nodes`: 4,096 by default, 16,384 for T1 at most |
| Depth | 32, the root node being depth 1 |
| Text per node | 64 KiB |
| Views open per plugin | 16 |
| Renders per view | At most one in flight, at most one queued |

A tree over a cap, or any other invalid tree, is a plugin bug. There is one outcome: the host keeps the previous tree (marked stale), logs one error, and the instance traps with `trap.invalid-ui-tree`, whose logged detail reason is a `ui.*` code, counted under the restart rule, as in [Errors and limits](/pages/reference-errors-and-limits/). `views.render` returns a `ui-tree`, not a `result`, so no tree error is ever returned as `invalid-argument`. The codes are:

| Code | Cause |
|---|---|
| `ui.invalid-tree` | Bad structure (a nonzero root, an out-of-range, backward or duplicated child index, an unreachable node), a prop out of range, more than 256 check-list items, more items than `max-items`, a bad validation regex, a tab title count that differs from the child count |
| `ui.duplicate-key` | Two nodes in one tree share a `key`, or two items in one widget share an item id |
| `ui.too-deep` | A node at depth 33 or deeper (root = 1) |
| `ui.too-large` | More than 4,096 nodes (`ui_tree_nodes`) or more than 1 MiB (`ui_tree_kib`) |
| `ui.widget-not-allowed` | A menu, list-editor or other input widget outside the allowed subset in a settings page |
A `views.render` that exceeds its deadline is a hard deadline breach. The instance traps with `trap.deadline`, and the logged detail reason is `ui.render-timeout`.

<figure>
<svg viewBox="0 0 440 190" role="img" aria-label="View rendering: trigger, coalesce, render call, ui-tree, diff, paint">
<title>View render pipeline</title>
<desc>A trigger such as invalidate is coalesced. The dispatcher calls views.render on the plugin executor. The returned ui-tree is checked against caps and handed to the UI thread. The UI thread diffs and paints in the next frame. The old tree is drawn until then.</desc>
<defs>
<marker id="rp-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
<path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
</marker>
</defs>
<rect x="6" y="20" width="80" height="40" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
<text x="46" y="44" font-size="10" text-anchor="middle" fill="currentColor">Trigger</text>
<rect x="116" y="20" width="90" height="40" rx="6" fill="none" stroke="currentColor"/>
<text x="161" y="44" font-size="10" text-anchor="middle" fill="currentColor">Coalesce</text>
<rect x="236" y="20" width="100" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
<text x="286" y="38" font-size="10" text-anchor="middle" fill="currentColor">views.render</text>
<text x="286" y="52" font-size="9" text-anchor="middle" fill="currentColor">10 ms</text>
<rect x="366" y="20" width="68" height="40" rx="6" fill="none" stroke="currentColor"/>
<text x="400" y="44" font-size="10" text-anchor="middle" fill="currentColor">Caps</text>
<line x1="86" y1="40" x2="116" y2="40" stroke="currentColor" marker-end="url(#rp-arrow)"/>
<line x1="206" y1="40" x2="236" y2="40" stroke="currentColor" marker-end="url(#rp-arrow)"/>
<line x1="336" y1="40" x2="366" y2="40" stroke="currentColor" marker-end="url(#rp-arrow)"/>
<rect x="236" y="110" width="198" height="40" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
<text x="335" y="134" font-size="10" text-anchor="middle" fill="currentColor">UI thread: diff, layout, paint</text>
<line x1="400" y1="60" x2="400" y2="110" stroke="currentColor" marker-end="url(#rp-arrow)"/>
<text x="404" y="90" font-size="9" fill="currentColor">next frame</text>
<text x="6" y="134" font-size="10" fill="currentColor">Until then the UI draws</text>
<text x="6" y="148" font-size="10" fill="currentColor">the previous tree.</text>
</svg>
<figcaption>The plugin executor runs the render. The UI thread swaps trees between frames.</figcaption>
</figure>

- **[HST-256]** `views.render` MUST be called only on the triggers above, and MUST have at most one call in flight and one queued per view.
- **[HST-257]** The client MUST draw the previous tree until a valid new tree arrives, and MUST swap trees only between frames.
- **[HST-258]** A tree over a cap MUST trap the instance with the named error, MUST NOT be shown, and the previous tree MUST stay on screen, marked stale.
- **[HST-259]** Several `ui.invalidate` calls before a render starts MUST produce one render, and a render MUST be at least 16 ms after the previous render of the same view.

### Cell width of view text

Plugin text has character limits, and layout works in cells. SEC-292 limits contributor text such as reasons, labels and titles to 120 characters, and SEC-380 clips text to its field limit with `…`. A character is one Unicode scalar value ([WIT conventions](/pages/reference-wit/#2-conventions)). A character limit is not a cell limit: 120 CJK characters need 240 cells. Layout always uses the width rule below.

1. **Width rule.** The width of a string is the sum, over its extended grapheme clusters (UAX #29, Unicode 15.1), of these values: 2 when the base has East Asian Width W or F, when the cluster contains U+FE0F, or when it is an emoji ZWJ sequence; 0 when the cluster holds only controls, ZWJ or default-ignorable code points; 1 otherwise, including East Asian Ambiguous characters and Nerd Font private-use glyphs. The [cell width](/pages/ui-model/#cell-width) section of the UI model gives the same rule with its explanation.
2. **Unicode version.** The grapheme rules and the East Asian Width values are both those of Unicode 15.1. A host that moves to another Unicode version changes these widths, so the version change is a change to this rule, and the vectors below are run again.
3. **Cutting.** Text is cut to a box only at a grapheme boundary, so a combining mark never loses its base and a wide glyph is never split. A cut ends with `…`, which takes one cell. The cut text is the longest prefix, at grapheme boundaries, whose width with `…` added fits the box. A text longer than its box is cut this way ([Text widgets](/pages/widgets-text/)).
4. **Layout measures, never counts.** A plugin measures with `display-width` and does not size a column by character count ([layout](/pages/layout/)). A glyph that still crosses the right edge of a box is drawn as spaces, never as half a glyph ([cell width](/pages/ui-model/#cell-width)).
5. **Open point: ZWJ.** SEC-380 removes zero-width characters from plugin text, and SEC-104 in [grant review](/pages/reference-grant-review-and-consent/) removes zero-width joiners from names. If sanitising removes the ZWJ before measurement, the family emoji in the vectors below becomes three emoji and measures 6 cells, not 2. The vector assumes the ZWJ inside an emoji sequence survives. The owner must decide this before the vector is fixed.

Test vectors. Each row is drawn in a labelled cell of the given box on the fake host. The test asserts that the row's cell width equals the expected width and that no glyph is split. Escapes are Unicode code points.

| Text | Code points | Cells | Box | Drawn text (cells) |
|---|---|---|---|---|
| `abc` | 3 | 3 | 3 | `abc` (3) |
| `漢字語` (U+6F22 U+5B57 U+8A9E) | 3 | 6 | 5 | `漢字…` (5) |
| `中文` (U+4E2D U+6587) | 2 | 4 | 3 | `中…` (3) |
| Fullwidth `Ａ` (U+FF21) | 1 | 2 | 1 | `…` (1) |
| `e` + U+0301 (combining acute) | 2 | 1 | 1 | `é` (1), no cut |
| `cafe` + U+0301 | 5 | 4 | 4 | `café` (4), no cut |
| `cafe` + U+0301 | 5 | 4 | 3 | `ca…` (3) |
| `❤` U+2764 + U+FE0F | 2 | 2 | 1 | `…` (1) |
| Thumbs up U+1F44D + skin tone U+1F3FD | 2 | 2 | 2 | no cut |
| Family U+1F468 U+200D U+1F469 U+200D U+1F467 | 5 | 2 | 2 | no cut (see open point 5) |
| `漢字 OK` (U+6F22 U+5B57 U+0020 U+004F U+004B) | 5 | 7 | 6 | `漢字 …` (6) |
| Nerd Font glyph U+F121 | 1 | 1 | 1 | no cut |
| `■` U+25A0 (ambiguous) | 1 | 1 | 1 | no cut |
| Lone ZWJ U+200D | 1 | 0 | 0 | measured only: 0 cells |

## Annotations and token providers

An annotator marks spans in a terminal or editor view (for example a link or an error line). The host calls `annotate` with the visible text lines and receives spans. The call has a soft deadline of 10 ms. The host calls it:

- when the visible content changes, at most once per 100 ms per view;
- never for rows outside the visible area plus 20 lines;
- never while the user is scrolling faster than 60 lines per second (the previous spans stay, and one call follows when scrolling stops).

A token provider (`tokens`) colours editor lines in the same way, with a 10 ms soft deadline. If the call is late, the lines stay plain and the host asks again at the next change.

Spans per call are capped at 2,048. Extra spans are cut and the log gets `annotate.too-many`.

- **[HST-260]** The host MUST call `annotate` and `tokens` only for visible rows plus 20 lines and at most once per 100 ms per view.
- **[HST-261]** A late `annotate` or `tokens` result MUST be discarded, and spans above 2,048 per call MUST be cut.

## Scenes

A scene is an animated background that a plugin renders. Scenes run on the dedicated **scene thread** of the client. The thread owns one Store per active scene and calls `renderer.render` at the frame rate of the scene.

- The frame rate is the scene's value capped by `frame_rate_cap` (1 to 60, default 30).
- Frames are **double buffered**. The scene thread writes into the back buffer. When `present` is called and the frame is complete, the buffers swap. The UI thread reads the front buffer. The UI thread never sees a half-written frame.
- The deadline is soft. A frame that is late is skipped, the front buffer stays, and the breach is counted. Thirty late frames in a row trap the scene instance. A late frame does not slow the UI.
- With a pre-rendered clip, the scene renders the clip once and loops it from host memory. The host does not call `render` again until the settings or palette change.
- The shared look (palette, colour mode, dithering) is applied by the host after the scene writes, never by the scene.
- A scene may use the GPU only through the `gpu.kernels` capability, and the host runs those kernels on its own queue, with a budget of 4 ms per frame.

| Cap | Value |
|---|---|
| Surface size | 512 cells wide, 256 cells high |
| Surface write per frame | 16 MiB |
| Frame rate | 60 |
| Scenes active per client | 1 |
| Clip memory | 256 MiB in total across clips; one clip is limited to 128 MiB ([scenes](/pages/api-scenes/)) |

- **[HST-262]** Scene frames MUST be rendered on the scene thread into a back buffer, and the UI thread MUST read only a complete front buffer.
- **[HST-263]** A late scene frame MUST be skipped without delaying the UI, and the host MUST apply the shared look to each presented frame.
- **[HST-264]** The host MUST NOT call `render` for a pre-rendered clip while the clip is valid.
- **[HST-265]** Only one scene MUST be active per client, and the host MUST stop the scene thread work when the background is off or the window is hidden.

## Reconciliation, hit testing, focus and scrolling

The client keeps **view state** that the plugin does not own: scroll offsets, focus, text cursor positions, open popups and the expansion state of trees and menus. This state is keyed by the node `key` of the `ui-tree`, so a new tree with the same keys keeps its state.

- **Key-based reconciliation.** The client matches new nodes to old nodes by `key` across the whole view, so a keyed node that moves to another parent keeps its host-owned state. A matched node keeps its view state. A node without a match is new and starts with default state. A node that disappears loses its state after 2 seconds, so a plugin that rebuilds a tree with a short gap does not reset the scroll offset.
- **Hit testing.** The client resolves a pointer position to a node on the UI thread, from the laid-out tree, from the top layer to the bottom: popups, then the focused panel's overlays, then the content, then the frame. A node that is disabled or has `inert` set is skipped. The result is an event `ui-action` or `pointer` with the node `key`, queued for the plugin. The plugin is not called to decide the hit.
- **Focus.** Focus belongs to the client. A plugin may request focus for a key in the returned tree. The client grants it only when the view is focused or the user opened the view. Tab order follows the rule in [Focus order](/pages/ui-model/#focus-order). Focus moves are never delayed by a plugin call.
- **Scrolling.** The client scrolls a scrollable node by itself, from held data, without calling the plugin. A plugin that shows more rows than fit supplies all rows up to the node cap, or uses a virtual list, which asks for a window of rows through `list-virtual-children` as the user scrolls.
- **Input latency.** A key or pointer event that only changes client-held state (focus, scroll, caret in a text field) is applied in the same frame, before the plugin hears about it.

- **[HST-269]** The client MUST reconcile trees by `key` across the whole view, MUST keep view state for matched nodes, and MUST keep the state of a removed key for 2 seconds.
- **[HST-270]** The client MUST resolve hits on the UI thread in the order popups, overlays, content, frame, MUST skip disabled and inert nodes, and MUST NOT call the plugin to resolve a hit.
- **[HST-271]** The client MUST own focus and scroll state, MUST apply them in the frame in which the input arrives, and MUST grant a focus request from a plugin only for a view the user has focused or opened.
- **[HST-272]** Two nodes in one tree with the same `key` MUST trap the instance with `ui.duplicate-key`.
- **[HST-495]** A popup id is the `key` of a popup or dialog node and MUST be unique across all open views of one plugin instance. `open-popup` MUST search the last committed tree of each open view, MUST return `not-found` for no match and `invalid-argument` for two matches, and a fifth open popup in one view MUST return `quota-exceeded`. `close-popup` on an id that is not open MUST be a no-op that returns `ok`. A popup whose node is missing from the next tree MUST close and send `closed`.

## Server-owned contributions on the client

A server plugin owns most decorations, virtual rows and status segments. The client draws a copy. This has three consequences.

1. **Latency.** A change that a server plugin makes reaches the screen after the IPC delta and the next client frame. The 99th percentile on a local connection is 5 ms plus one frame.
2. **Staleness at detach.** A client that is detached keeps the last mirror. After re-attach it receives a snapshot. The UI shows the old contributions as stale until the snapshot arrives (see [Plugin traffic over IPC](/pages/reference-host-ipc/)).
3. **Client-owned points.** A plugin half on the client that owns a point (an annotator, a token provider, a view) does not depend on the server.

- **[HST-266]** A change made by a server plugin to a decoration MUST reach the client mirror in at most 5 ms at the 99th percentile on an idle local connection (benchmark B20).
- **[HST-267]** A detached client MUST mark mirrored server contributions stale until a snapshot arrives.

## Accessibility and theme

The client applies theme tokens and the accessibility settings to every plugin tree. A plugin supplies roles and labels and never colours. The host repaints at once and then triggers one render per open view on a theme change (see Views). See [Accessibility and theming](/pages/accessibility-and-theming/).

- **[HST-268]** A theme or font change MUST repaint the last tree of every open view at once, MUST deliver `theme-changed`, and MUST trigger exactly one render per open view coalesced within a 16 ms window. The client MUST recolour retained contributions without calling the plugin.

### Labels and names

The terminal has no accessibility tree. A screen reader, or the linear screen-reader mode of [Accessibility and theming](/pages/accessibility-and-theming/), reads a plugin's controls only through their text, so a control without a name cannot be used by that user.

1. **Which nodes need a name.** Every node that takes focus: the inherently focusable widgets of the [focus order](/pages/ui-model/#focus-order) table, and any node with `flags.focusable`. Decorative nodes, such as separators, spinners and glyph badges, need no name.
   Test: a fixture tree with a separator and a spinner and no labels is accepted.
2. **What counts as a name.** A non-empty text label. A glyph, an icon or a node key is not a name, so an icon-only button supplies a `label` (see [USR-306](/pages/accessibility-and-theming/#production-additions-t7-users-privacy-a11y)). The label is tested after sanitising (the list in [Text sanitising](/pages/ui-model/#text-sanitising)): a label that is empty once control characters and escape sequences are removed, or that holds only spaces, counts as empty.
   Test: a button labelled with only the sequence `ESC [ 31 m` followed by two spaces is refused as unnamed.
3. **What the host does when a name is missing.** The view validator refuses the tree. This is an invalid tree under [HST-258](#views): the previous tree stays on screen marked stale, the host logs one error that names the widget id, and the instance traps with `trap.invalid-ui-tree` whose detail reason is `ui.label-missing`. The host never invents a name from the glyph, the key or the widget type, because a generated name would be read out as if the plugin had written it.
   Test: a tree with one unlabelled button is refused; the error names that button's id; the earlier tree is kept and drawn stale; with `[ui] screen_reader = true` the labelled version is read with its label and its role word.

### Focus order and reachability

Focus belongs to the client ([HST-271](#views)). The default Tab order is the rule in [Focus order](/pages/ui-model/#focus-order): nodes with `tab-index` 1 and up in ascending value, then the other nodes in tree pre-order, and nodes with a negative value are skipped by Tab. The rules below are what a plugin can rely on, and what the host must keep to.

1. **Tree order is reading order.** A plugin builds the tree top to bottom and left to right, as [USR-303](/pages/accessibility-and-theming/#production-additions-t7-users-privacy-a11y) requires. An explicit `tab-index` overrides the reading order for every node that carries one, so a plugin sets it only for a deliberate exception.
   Test: a fixture with nodes A, B, C, D and E in pre-order and `tab-index` values none, 2, 1, -1 and 0 is walked with Tab in the order C, B, A, E. D is never visited by Tab.
2. **No action behind a negative index only.** A node with a negative `tab-index` is reached by a click and by `set-focus`, but not by Tab or Shift+Tab. An action that a keyboard user must run needs a focusable route, such as a button or a row in the Tab order.
   Test: a keyboard-only walk (Tab, Shift+Tab, Enter) reaches every action in the fixture, and the negative-index node is reached only by `set-focus`.
3. **Disabled and hidden nodes.** A disabled node stays in the Tab order, ignores input and shows its `disabled-reason` as its tooltip. A hidden node is skipped. A plugin removes a control from the order by hiding it, not by disabling it.
   Test: Tab stops on a disabled node, Enter sends no `ui-action`, and the reason appears as the tooltip; a hidden node is never visited.
4. **Wrap and escape.** Tab wraps from the last node to the first in the view, and Shift+Tab wraps the other way. Escape leaves a modal plugin dialog, in the order given in [Escape](/pages/ui-model/#escape).
   Test: Tab from the last node lands on the first; a modal dialog opened in the fixture closes on Escape and focus returns to the node that opened it.
5. **Popups.** Focus moves into an opened popup and returns to its anchor when the popup closes ([Focus](/pages/ui-model/#focus)).
   Test: `open-popup` moves focus into the popup; `close-popup` returns focus to the anchor key.
6. **Focus requests from a plugin.** The client grants a plugin's focus request only for a view the user has focused or opened (HST-271). Otherwise the request is ignored and focus stays where it was.
   Test: a focus request from a view the user never opened leaves focus unchanged.
7. **Open point: focused key removed.** The manual does not say where focus goes when the focused node leaves the tree. The removed key's view state is kept for 2 seconds ([HST-269](#reconciliation-hit-testing-focus-and-scrolling)), but that state does not decide focus. A plugin must not rely on a particular node taking focus in that case. The owner must set the rule before it can be tested.

### Reduced motion

The user turns on reduce motion in Settings (the `[ui]` key `motion_level`, see [configuration](/pages/reference-config-and-data-migration/)). The client passes the setting to plugins as `reduced-motion` in theme extras, and a change arrives as `theme-changed` ([Accessibility and theming](/pages/accessibility-and-theming/#theming-and-readable-output)). The rules below apply to every animation a plugin can show: decorations, views and scenes.

1. **A plugin reads the setting and stops its own animation.** It reads `reduced-motion` from `theme-extras()` when it activates and on every `theme-changed`, and it stops any timer that only animates.
   Test: with reduced motion on, a plugin animation timer fires no further redraws after the `theme-changed` that carries the setting.
2. **Host-run animation shows one static frame.** The spinner, the busy state of a progress row and the frames of `set-decoration` show their first frame only ([events and intents](/pages/reference-events-and-intents/) and [intents API](/pages/api-intents/)). The host draws no hover or scroll animation.
   Test: a `set-decoration` with four frames shows frame 1 and no later frame for one second.
3. **A plugin that keeps redrawing is capped.** Under reduced motion the host delivers at most one full-view redraw per second to a plugin that animates ([USR-307](/pages/accessibility-and-theming/#production-additions-t7-users-privacy-a11y)). A plugin timer below 1000 ms that only triggers redraws is counted, and the first such timer in a session logs one `ui.motion-ignored` warning.
   Test: a plugin that calls `ui.invalidate` ten times a second for three seconds receives at most three renders, and the warning is logged once.
4. **Scenes draw one still frame.** Under reduced motion a scene draws one frame, as the spinner does, and does not render again while the setting stays on. A pre-rendered clip shows its first frame under the same rule. This is option (a) in the open points of the [ambient animation page](/pages/core-plugin-ambient/). The owner has not decided it, so this rule is provisional until that decision is made. This page sets no flash-rate limit for scenes, and the same open points cover it.
   Test: a scene with reduced motion on is rendered once, and the front buffer keeps that frame for two seconds.

## Errors

| Id | When | Effect |
|---|---|---|
| `contribution.too-many` | A count cap is exceeded, other than the decorations cap | Update refused |
| `contribution.too-large` | An entry is over 16 KiB | Update refused |
| `ui.invalid-tree` | Bad structure or prop (see Views) | Trap; counts as a failure |
| `ui.too-large` | More than `ui_tree_nodes` nodes or `ui_tree_kib` KiB | Trap; counts as a failure |
| `ui.too-deep` | Depth over 32 (root = 1) | Trap; counts as a failure |
| `ui.widget-not-allowed` | Widget outside the allowed subset | Trap; counts as a failure |
| `ui.render-timeout` | Render over its deadline | Trap; counts as a failure |
| `annotate.too-many` | More than 2,048 spans | Extra spans cut |
| `rate-limited` | Update rate exceeded | Update refused |
| `ui.duplicate-key` | Two nodes in one tree share a key, or two items in one widget share an item id | Trap; counts as a failure |

## Edge cases

| Situation | Result |
|---|---|
| Plugin traps while a view is open | The last tree stays, marked stale, with a "restarting" notice |
| A view is open and the plugin is disabled | The view closes with a notice |
| `ui.invalidate` called inside `views.render` | Ignored once; logged `ui.invalidate-in-render` |
| Terminal resized while a render is in flight | One more render queued with the new size |
| `open-popup` names a key that two open views contain | `invalid-argument`; popup ids are unique across the open views of one plugin instance |
| Scene `render` returns without `present` | Frame dropped; warn once per second |

## See also

- [UI](/pages/api-ui/), [Surface](/pages/api-surface-glyph-plane/) and [Scenes](/pages/api-scenes/).
- [Performance budgets and benchmarks](/pages/reference-host-budgets/) for the render budgets.
- [Requirements: kernel and host](/pages/reference-requirements-kernel-host/).

API version checked: `ilium:plugin@0.1.0`.
