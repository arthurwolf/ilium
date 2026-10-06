# ilium — architecture and design

This document covers the product design, process architecture, crate boundaries, agent-detection strategy, and milestone history. For installation and day-to-day use, see [`README.md`](README.md). For the rules that govern changes to this codebase, see [`CLAUDE.md`](CLAUDE.md).

ilium is built around two ideas tmux doesn't have:

1. **Tree-structured pane list.** Sessions are a tree of groups, split views, and panes (not a flat window/pane grid), shown in a left-side panel. The right panel renders either one pane or a persistent vertical/horizontal split containing up to four terminals, editors, or boards. Panes can be freely reordered and moved between containers.
2. **Agent awareness.** ilium periodically inspects each pane's content, detects whether it's running an AI coding agent (Claude Code, Codex CLI, Antigravity CLI, etc.), and shows whether that agent is *thinking* or *done* via icon + color in the tree — so a glance at the sidebar tells you which of your N agent sessions need attention.

Everything else (detach/reattach, persistent sessions, PTY handling, keybindings) follows tmux's model closely enough that tmux muscle memory should mostly transfer.

## Prior art — why not just use X

This space already has prior art, researched before writing this doc:

- **[Zellij](https://github.com/zellij-org/zellij)** — the reference architecture for a modern Rust multiplexer: client/server over a Unix socket, WASM plugin system, floating panes. No tree-of-groups pane list, no agent-state detection. ilium borrows its client/server split.
- **[tmux-based agent managers — claude-squad](https://github.com/smtg-ai/claude-squad)** — wraps tmux + git worktrees to run multiple Claude Code / Codex / Aider sessions with a dashboard. Not a multiplexer itself; it drives tmux.
- **[herdr](https://www.linuxlinks.com/herdr-terminal-based-agent-multiplexer/)** — a Rust, single-binary terminal multiplexer with a sidebar that classifies each pane as blocked / working / done / idle via process-name + output heuristics, zero-config, ~15 agents supported out of the box. This is the closest existing project to ilium's second feature — it validates the approach (process-name + text heuristics is viable and is what real tools ship) but doesn't have the tree/group pane model, only a flat sidebar list.
- **[RMUX (Helvesec)](https://github.com/Helvesec/rmux)** — tmux-compatible (90 commands) Rust multiplexer with typed SDKs (Rust/Python/TS) for programmatic control and a `ratatui-rmux` widget for embedding live panes. No agent-state detection, no tree/groups; it's an automation-first tmux clone.

None of these combine a manipulable *tree* of panes with agent-state detection, which is the actual gap ilium fills. Worth knowing herdr exists if the second feature alone is what's wanted — it's shipping today.

## Core concepts

```
Session
 └── Group            (can contain Groups, Split Views, Panes, and folder roots)
      ├── Pane         (terminal, editor, or board)
      └── Split View   (vertical/horizontal container; zero to four Panes)
           ├── Pane
           └── Pane
```

Each tree node is a `Container`, `Pane`, or persisted folder root. A container is either a normal `Group` or a `SplitView { orientation }`; a pane carries a `PaneContentKind` (`Terminal` | `Editor` | `Board`) and a matching `PaneStatus`. The detection engine drives `PlainShell`/agent statuses for terminal panes; `AgentState` separates turn phase, provider goal, and unread completion when projecting the status into tree icons.

`PaneStatus::AgentUnavailable` retains a historical `AgentRecovery` separately from live ownership. Its process key includes provider class, OS PID and start time; availability distinguishes a matching owned-child exit receipt, foreground shell evidence and unverified ownership. Missing nested processes do not supply an exit cause. `agent_state()` exposes only a live agent; presentation and retained-session consumers use `known_agent_state()` or the recovery record. Unavailability suppresses live turn/goal completion projection without discarding session, prompt or terminal history.

Agent input is serialized through a pane-local gate and an ordered PTY receipt, with global tree/pane locks released before awaiting delivery. Automated bodies and Enter require fresh process/foreground ownership, and an invocation cancellation watch revokes queued delivery on ownership loss. Explicit user-origin IPC keeps manual shell interaction separate. A dedicated uncapped composer tracker records exact receipt-backed prompts; opaque edits keep the latest prompt unavailable and label any previous exact prompt separately. Transcript corrections require the same invocation/session/Enter epoch and a complete post-submission record, and can repair only an unknown latest prompt. Client statistics caches additionally fence provider/session/project/home identity and worker generation, preserving a stopped session while preventing replacement panes from inheriting its metrics.

Native foreground and process-identity preflights run on the existing finite I/O bank after capturing the PTY lifetime and invocation under a short pane guard. The final input/title admission checks the lifetime, agent generation, process birth, session and cancellation epoch again. A 500 ms caller deadline declines an unavailable proof while the physical native worker retains its admission until it returns. Process termination captures the original child-control handle and birth identity before leaving the registry; a five-second caller deadline reports an uncertain outcome rather than claiming that the child stopped. Focused-terminal directory inspection follows the same off-lock capture and revalidation pattern, falling back to the project directory when its proof is unavailable or stale.

PTY destruction and ordinary pane teardown only request cancellation. The existing owned child reaper performs one termination attempt through the original child-control handle and keeps that handle and its admission until actual exit is observed. A blocked native child mutex therefore cannot delay the Tokio caller or start the shutdown deadline late; an expired deadline retains the physical reaper as pending custody rather than claiming that it joined. Native termination failures are logged by that reaper.

### Left panel — the tree

The left panel renders this tree via `tui-tree-widget`: expand/collapse groups, select a pane to focus it on the right, reorder entries one step at a time (hover an entry's up/down arrows, or leader `m` for keyboard move-mode), drag-and-drop a row onto any other row or the empty space below the tree to reparent it there, double-click a real tree entry to open the same Rename prompt as the context menu, and right-click an entry for create/rename/move/close actions.

- Every tree menu also exposes **Restart**, which reloads the client executable and reattaches it to the existing detached server without restarting that server or its PTYs.
- The same menu has a checked **Order by** submenu for Manual, Type, Age up/down, and Name A-Z/Z-A. Automatic modes sort independently inside every normal group while leaving split-view placement untouched. The choice applies live, persists as `[ui].tree_order`, and is mirrored by the User Appearance settings tab. Any arrow, drag/drop, or keyboard structural move returns the setting to Manual before sending the move.
- Terminal rows, including detected agents and plain shells, expose **Hit key(s) X time from now**: the dialog accepts an hours/minutes/seconds delay, optional text, and optional Enter. The detached server persists and delivers that input at the absolute deadline even when no client remains attached. While pending, the row shows a human-readable countdown before the pane title and animates the existing clock sequence backwards.
- User Interface settings expose three explicit left-panel sizing cards: **Fixed** owns one width; **Focus-dependent** owns separate focused and unfocused widths; **Width-dependent** stays at the focused width at or above its terminal-width threshold, then becomes focus-dependent below it. Every value applies live and persists under `[ui]`; full motion eases between policy targets while reduced/off motion snaps directly. A fresh install selects Width-dependent (breakpoint 120 columns, 44-column focused and 24-column unfocused panel), Reduced motion, agent identifiers shown as icons, and the icon set listed in `IconTarget::default_glyph`; these defaults are the maintainer's own settings.
- Structural changes use their own 220 ms eased feedback: removed rows accelerate left and dim before disappearing, while added rows enter in the opposite direction, settle softly, and only then run the existing creation blink.
- Footer actions are visible whenever the panel has keyboard focus or the footer is hovered; creation controls flow from the left and a right-aligned 🎚️ opens Settings.
- At a nested group or split-view boundary, a pane arrow moves the pane into the enclosing group immediately before or after its former container. At a top-level group boundary, it transfers the pane into the adjacent group so panes never become root-level nodes. In keyboard move-mode (leader `m`), left/right (or `h`/`l`) indent the selected node into the nearest preceding sibling group / outdent it into its group's own parent — see M3 below for exactly what's implemented and what's deliberately left simpler.

### Right panel — the presentation target

The right panel renders a normal pane alone, or every child of a selected split view. Two and three children follow the split's orientation; four use a 2 by 2 grid. Selecting a child keeps the whole split visible while making only that child active for keyboard and pointer input.

- For a detected agent, its viewport title includes the real agent PID and its session ID when the CLI exposes one; otherwise it explicitly says that the session is unavailable rather than guessing. Submitting exact `/clear` to Claude or Codex immediately discards that detected session ID and resets an automatic title to `<new>` until the replacement session can be verified and titled. A manually fixed name, short name, icon and ownership survive this reset.
- AI project restructuring treats every existing split view as a user-owned atomic layout. It may rebuild ordinary groups around a split and retitle the panes inside it, but it cannot create, remove, replace, reorient, reorder, or change the membership of a split. Because the original split and pane IDs survive the authoritative tree snapshot, a currently focused split and its active pane remain focused through the restructure.
- Every AI restructure prompt includes the concrete animation catalog and relevant typed scene controls. The reply must supply a deduplicated recommendation table and pointers for the project and every output entry, including groups and split views, even when Semantic rendering is disabled. The client validates concrete kinds, control bounds, conditional dependencies and resource policy before sending an expanded `RecommendedRestructurePlan`. Core checks exact output-path coverage and the captured project generation, builds a candidate tree and publishes structure plus recommendation metadata together. The server serializes apply/undo publication, retains the undo image until restore succeeds and requests the existing debounced snapshot save; an acknowledgement proves the in-memory commit, not completed disk persistence. Recommendations travel in ordinary tree snapshots and survive reattach. Undo restores the previous recommendations while advancing the generation so an older inference cannot overwrite it.
- Click either panel to focus it. When a terminal application enables an xterm mouse protocol (for example `vim`, `htop`, or `lazygit`), ilium forwards clicks, drags, scrolls, and modifiers to that PTY using its requested encoding.
- Terminal history can be navigated with the wheel or Shift+PageUp/Shift+PageDown; Shift+End returns immediately to ilium's live output. Full-screen applications that own their mouse history also receive xterm Ctrl+End (Claude Code's native jump-to-bottom shortcut). While ilium history is being inspected, incoming output and agent resize redraws continue in a separate live parser, so neither can move or corrupt the frozen historical viewport.
- The agent toolbar's Smart Copy action captures an immutable clone of the visible `vt100::Screen`, numbers its visible lines and non-whitespace word runs, and sends only those references to the configured inference provider. The response is streamed as JSONL: every record must resolve wholly against the frozen snapshot before it becomes selectable, so model-authored replacement text and stale live-screen coordinates cannot reach the clipboard. The first valid record dismisses the blocking progress dialog; later records appear incrementally and flash for 500 ms. Overlapping candidates prefer the smallest region and the wheel cycles the alternatives under the pointer. `smart_copy_tokens` adds the pre-scanned fine-grained layer without any model call: per-line regex detectors (URLs, e-mail, IP, endpoints, paths and `file:line` locations, qualified and identifier names, hashes, versions, assignments, flags, quotes, key/value fields, sentences, dates, amounts, phones), a multi-line postal-address matcher, and `vt100` cell-attribute runs (foreground colour, highlight, bold, italic, underline). Path-shaped tokens carry a `PathProbe`; the UI thread resolves them through `PathContext` (pane cwd, its ancestors, the project root, `~`, a few conventional source directories, a bounded stat budget) when the session is built, relabelling hits as `file`/`directory` and dropping implausible misses. Every non-empty line is also a region, and regions are deduplicated by exact geometry with structural, line, detail, token and styled regions in that priority. Clicks toggle a region in a persistent multi-selection that stays inverted and is written to the clipboard on each click. Smart Copy light (`smart_copy_light.rs`, `[terminal] smart_copy_light` / `smart_copy_light_key`, on by default with Ctrl) reuses the same capture and detection but never queues a model request: `App::try_start_smart_copy_light` starts it from a mouse event carrying the configured modifier over a terminal pane, clicks only toggle regions, and `finish_smart_copy_light` copies the selection once on release and installs a `SmartCopyPreview` that `draw_smart_copy_preview` renders with a one-second countdown gauge. Release is inferred (terminals do not report modifier release): a Kitty key-release event, the first mouse event lacking the modifier, focus loss, or `RELEASE_IDLE_GRACE` after the last event once something is selected.
- A terminal pane may own one long-task progress registration. Registration is transactional: the server validates one bounded JSON probe before replacing an existing monitor and returns a correlated acknowledgement with its generation ID. The detached server—not the agent—then performs every recurring probe, keeps task `error` distinct from monitor degradation/failure, persists terminal evidence, and waits for a verified clean agent composer before submitting the result. Each report carries two human-facing descriptions that must make sense without any session context: a one-line `message` (always shown in the footer, followed by a hover marker) and an optional multi-line `details` (What/Why/Now/Next/Watch, at most 8 KiB, shown in a tooltip when the pointer rests on the footer); the managed agent instruction block (`progress-instructions.hbs`, schema version 6) tells agents how to write both. The result message is the monitor's only effect on the agent: it never pauses, resumes, or otherwise touches the agent's `/goal`.
- The second header icon of a detected Claude Code or Codex pane opens the costs-and-stats popover: hover previews it, a click pins it (non-modal, so keystrokes keep reaching the agent; only pointer events over the popover are claimed), and the icon or its close control dismisses it. Its data never touches the server or the wire: `session_stats` parses the agent's own JSONL transcript into a `SessionStats` value, `session_stats_store` runs that parse on a low-priority worker thread per pane, and `session_stats_ui` draws four tabs (Overview, Tokens, Activity, Prompts) onto a tall off-screen buffer that is windowed into the frame, sized to fill the right-hand panel. Its over-time graphs use `ascii_chart`, a port of the asciichart/rasciigraph plotting algorithm (same glyphs, label precision and NaN gaps, validated against rasciigraph's published outputs) that returns a grid of kind-tagged cells so the caller colours each series, and that can downsample by per-column maximum so spikes survive. Transcripts reach several gigabytes with lines above 30 MB, so the parser is incremental (a per-pane `StatsAccumulator` resumes at its byte offset and ignores a trailing partial line), filters raw bytes with a regex set before deserialising, counts tool failures from bytes without parsing tool-result lines, and merges Claude's repeated per-block usage by message id rather than summing it. Token counts are normalised across providers (`input` is uncached input, cache read/write are separate, Codex's cached share is subtracted from its inclusive input count). The popover shows dollars only when the agent recorded them (Claude Code's `cost-state` snapshot); the agent-cost indicators below price tokens themselves.
- Agent-cost indicators (Settings > Agent Cost) reuse those same transcript snapshots; nothing about them touches the server or the wire. `session_stats` additionally keeps per-minute, per-model token buckets. Claude Code writes sub-agent and workflow calls to `<session>/subagents/**/*.jsonl` rather than the main transcript (a measured session billed $1,021 while its main file priced at $19), so the store's worker folds those files in through `StatsAccumulator::ingest_extra_file`, each resuming at its own offset and deduplicated by message id; advisor-model calls appear in no transcript and are covered only by the CLI's recorded `cost-state` total. `cost_model` is the pure policy: a longest-prefix `PriceTable` (Anthropic rows from Anthropic's published list, OpenAI rows from third-party reports of its list, user overrides under `[cost.prices]`; an unpriced model marks the figure a lower bound instead of guessing), five calibrations that turn dollars into one of five levels (fixed bands, median of open agents, percentiles of the user's own past sessions, fraction of a budget, burn rate), sparkline bucketing and spike detection. `cost_history` scans past sessions for the history calibration without parsing whole files: Claude Code's last `cost-state` total, or Codex's last cumulative `total_token_usage`, from a bounded tail of each transcript, cached by path, size and mtime, on one lowest-priority worker. The quota metric (`CostMetric::Quota`) swaps the unit, not the machinery: `StatsAccumulator` turns consecutive Codex `rate_limits` readings of one window period into per-minute `QuotaBucket`s (a reset or lower reading only re-baselines), `cost_history` reads each Codex session's first and last reading for history percentiles, and the tracker feeds those series through the same calibrations, sparkline and spike logic with quota cut points, budget and spike floor. Claude Code transcripts carry no quota, so those rows are marked unavailable and draw nothing. `cost_tracker` derives each agent's `PaneCost` from its `SessionStats` only when the snapshot changed or the sparkline window moved, and rebuilds one immutable `CostOverlay` (per-row level, group roll-ups, totals, ranks) that `tree_ui` only reads, so a mouse-move redraw never re-prices. `cost_overlay` decides what a row shows from each option's enabled flag and visibility (always, or only while that entry is hovered) and paints it just left of the hover action buttons; `cost_settings`/`cost_settings_ui` own the `[cost]` table and the tab. Sorting by cost is `TreeOrder::CostDescending`, derived from `[cost].sort_by_cost` rather than persisted as a UI order, with the ranking threaded through `tree_ordering`, the hit-test cache and selection reconciliation so render order and click targets cannot disagree. The row action buttons form one compact strip flush with the list's right edge, and the hover highlight covers the whole entry.

### Pane states shown in the tree

Every pane row has three icon slots before its title. `ilium_core::project_pane_signals` is the one pure projection from server-owned facts (detected status, progress monitor, scheduled input, terminal output activity) to the last two slots, so the server (sounds, notifications) and every client (rendering, hover text) derive the same answer. Its A/B rule IDs identify the selected precedence rule. `ilium-client/src/status_icons.rs` owns only how a signal looks and what its tooltip says. The grey WHY line at the bottom of each hover popover gives the applicable rule and the observed evidence; the server sends detector provenance with the tree in one attach snapshot and with status in one live update so an icon cannot pair with stale evidence. Glyphs below are the compiled-in defaults from `IconTarget::default_glyph`; every one is configurable under `[ui]` and in Settings → Icons, and agent identifiers render as icons unless `[ui]` selects full names, letters, or none.

| Slot | Default glyph | Meaning |
|---|---|---|
| Identity | 🦀 / 🐢 / ⚛️ / 🤖 | Claude Code / Codex / Antigravity / any other detected agent |
| Identity | 🖥️ (📝 editor, 📉 board) | plain terminal with no agent detected |
| Objective | 🎯 / ⏸️ / 🚧 / ⌛ / 🏁 | the agent's `/goal` is active / paused / blocked / stopped by usage limits / reached (read from the provider's own status row) |
| Objective | ⣀⣀ … ⣿⣿ (cyan, yellow when degraded) | a monitored task's progress in twelve percentage buckets; Settings offers Braille, Blocks, Moons, and Quarters frame families, while `[ui].task_progress_frames` accepts a custom equal-width list of 2–13 frames |
| Objective | ○ / ✅ / ❌ / ⚠ | a monitored task that is registered but not started / done / failed / lost by its monitor (bold while unread, dim once seen) |
| Objective | ⏰ | a scheduled input is pending |
| Now | ⠋ (animated) | agent is **working**; the ten-frame braille spinner runs at 90 ms, and motion level Off freezes it |
| Now | 🕗 (animated clock) | agent is **waiting for background agents/tasks** it started; cycles the 24 half-hour clock faces at 220 ms |
| Now | ✋ (bold) | agent is **waiting on your approval** (y/n prompt or selection) |
| Now | 🌀 | turn over but a background shell or task it started is still running |
| Now | 💤 | agent idle and parked on a live progress monitor: not finished, no bell |
| Now | 🔔 (pulsing) | agent finished a turn you have not seen |
| Now | ● | agent idle at its prompt with nothing running or unread |
| Now | Angular Braille loop | ordinary terminal whose visible character grid changed, or received local key input, within the last 60 seconds |

One raw Idle sample after an active agent turn is provisional: the server keeps the active state, schedules a prompt recheck, and marks completion unread only after a second consecutive Idle sample from the same process identity. A live monitor suppresses completion while the agent is parked. Plain-terminal activity is event-driven rather than polled: after each already-coalesced live `ScreenUpdate`, the client hashes visible character cells without allocating a screen string and refreshes its presentation window only when that hash changes. Replay, resize reflow, cursor motion, and style-only output reset or preserve the comparison baseline without creating false activity; accepted local key input refreshes the same window immediately. The selected Angular loop runs at its existing 90 ms frame speed for the first five seconds, slows to one frame every 500 ms until 60 seconds, then disappears. An idle terminal performs no screen checks or animation work.

The blocked/waiting-for-approval state wasn't explicitly requested but falls out of the same detection pass at near-zero extra cost, and it's the state you most want a distinct color for in practice (herdr treats it as a 4th state for the same reason).

Claude activity classification treats the latest recognized completed-turn summary as a boundary: earlier working and background-wait rows belong to the previous turn. The summary's own still-running suffix and newer activity stay eligible; approval checks retain the full screen. Goal completion remains independent of current activity.

Terminal panes reserve configured space for enabled prompt and progress displays before input begins, including plain shells that may later start an agent. Metadata arrival, wrapping, clearing, detection changes and visibility changes keep that allocation fixed; explicit settings, split changes and outer-terminal resizing may change the child dimensions. Editors and boards do not reserve terminal metadata space, and small panes preserve a terminal cell where physically possible.

Completed progress footers are a client presentation policy. `[ui].completed_progress_hide_after_seconds` defaults to 60; zero keeps them visible. Settings → Agent Monitoring changes the delay in 30-second steps. Done and Error footers expire from the server's last observation time, including during continuous input/output; their text disappears while the reserved rows remain. Expiry does not resize the child, clear reports, acknowledge results, change sidebar signals or affect queued notification delivery.

## Ambient animation rendering

Semantic is an opt-in selection policy, not a scene engine. The selected real tree entry owns Entry scope; its project owns the default Project scope. Virtual tree rows use their nearest domain ancestor, with the displayed pane and a sole project as unambiguous fallbacks. `app_semantic_animation` owns the one global authored animation setting (`App::animation_home`, stored through `project_config` under the client config directory; the launch project's block seeds it once) and caches strict recommendation admission against selection, tree version, generation, recommendation and authored settings. It returns one immutable concrete overlay without writing configuration or requesting inference. Composition, preview, cadence, color facts, loop-cache choice and attribution read that same effective value. Invalid or missing recommendations clear stale scene state and contribute no animation deadline; equivalent authoritative snapshots retain the scene. Explicit settings edits validate the global load state before the existing locked YAML write. Final-painted receipt hooks remain owned by the renderer; effective selection does not manufacture delivery receipts.

Two scene families share one dot raster (`ilium_ambient::Raster`, 2×4 dots per terminal cell) and one packing step (fixed ordered or stippled thresholds, then Unicode Braille):

- **Built-in scenes** (`ilium-client::background_animation`) are deterministic, seekable, monochrome functions of time. Their default Loop playback precomputes 30 frames per second on a low-priority worker into a client-local packed cache. Ready playback copies packed cells without scene calculations. A forward crossfade joins the end to the start; replacing unbounded Live time with a finite loop can change phase once at readiness. Status distinguishes resident packed-frame allocations from projected storage and reports progress/ETA; it does not report whole-process RSS. Each build is limited to 128 MiB of packed storage and 131,072 cells of scratch geometry, with at most two builders admitted per process. A generation owns its receiver, so cancelled builds cannot publish stale frames. Cancellation and joining happen away from the UI; hidden unfinished caches pause while completed caches remain reusable. Live is an explicit mode.
- **Hosted scenes** (the `ilium-ambient` crate: 3D pipes, stars overhead, solar system, Earth at night, satellite clouds, video, audio spectrum, images, hex expedition, Vector TD, voxel landscape, galactic empires, topographic maps, OpenStreetMap) are stateful `Scene` objects that may own worker threads and helper processes. They depend on data, processes or the wall clock, so `AmbientKind::is_live_only()` keeps them out of the loop cache: they always render live. The crate owns scene math, decoding, geocoding and downloads and has no terminal or ratatui dependency; the client owns everything visual and persistent.

`AnimationFrame` (one per client) owns the current hosted scene through `AmbientHost`, keyed by `AmbientSettings::scene_key(kind)`. The scene is rebuilt when that key changes and dropped, which stops its threads and processes, when the kind changes to a built-in scene, when neither the ambient background nor the Settings preview is visible, and when the client exits. Because the background and the preview render through the same frame and host they share one scene instance. `render` runs behind `catch_unwind`: a panicking scene is replaced by a message scene and the client keeps running. A scene receives `time` (scene-relative, times the Speed setting), unscaled `wall` time and the current civil time; it reports `uses_cell_colors()` (then the compositor paints its per-cell colors instead of the user palette), `frames_per_second()` (1 to 30) and a `status()` line.

Live graphs and maps (`ilium-ambient::live_data`) separate bounded external decoding, provider metadata, source-time histories and raster presentation. Graph and earthquake pollers own cancellable low-priority workers; render paths use nonblocking snapshots. Historical Coinbase plans reserve a 60-second client floor per pair across local clients and reopened scenes; failed attempts consume the reservation. This is a client policy, not a provider-enforced numeric quota. Scene retirement retains one bounded recent snapshot for each of the eight Coinbase pairs, keyed by the selected window, with at most 600 samples and 600 genuine candles per entry. Recreated scenes can show those known observations while admission and refill proceed; original provider and receipt times remain unchanged, and a wider window may initially contain partial history. This cache is process-local. Fleet maps subscribe to one process-owned service with eight subscribers per source, one serial fetch/decode and six guarded raw generations across three sources, plus one in-flight result. Atomic source-specific disk snapshots retain original receipt times. Invalid regular cache content may refresh only through the source lock and persisted admission; invalid reservation ledgers fail closed. No requests run without subscribers. Raw-vector destruction stays on the worker through exclusive Arc unwrapping; full custody applies backpressure. Each fleet scene coalesces preparation through the marker worker, with four preparers admitted through actual cleanup. Source, dimensions and marker brightness invalidate incompatible geometry; pending same-source refreshes label previous geometry separately from received data. Color, coastline and poll edits preserve fleet geometry.

The graph catalogue contains stable IDs for crypto OHLC, daily ECB reference rates, active NOAA RTSW observations, ISS orbital estimates, USGS activity, Wikipedia edit-rate/bot-share aggregates and Quicknet randomness. Histories use provider timestamps rather than counting repeated polls as observations. Candle granularity follows the time window; incompatible buckets clear the old candle history. Wikipedia uses bounded SSE aggregation, excludes canary/non-edit/non-Wikipedia messages and stores no article titles or post text. Quicknet rounds supply scheduled source times but are explicitly not BLS-verified.

The three live maps share the embedded Natural Earth coastline. USGS retains all usable reported events, including negative, zero, tiny and unknown magnitudes; invalid magnitude types are rejected. Colliding labels may be omitted, never event markers. OpenSky retains usable airborne positions with optional fix times, never replaces fix time with last contact, and uses a persisted 900-second anonymous global client floor. Boats default to OpenSeaFeed's incomplete broader AIS reception with a 60-second client floor; Digitraffic Finnish-water reception is an explicit 30-second alternative. OpenSeaFeed rejects whole updates above 200,000 records or 32,000,000 decoded bytes. Known non-vessel/group namespaces are excluded and counted; other unusual identities are retained without claiming identity authentication. Position age remains unknown independently of snapshot build, latest ANY AIS update and receipt. Every retained position and genuine heading reaches the preparer; viewport occupancy deduplicates bookkeeping. Credit, provider/license URLs, transformations, coverage and time semantics are separate openable read-only text controls. Positions and timestamps are never extrapolated.

Pi (`ilium-ambient::pi_digits`) computes an exact bounded integer-spigot prefix with single-process generation admission and prepares an atlas from unchanged bundled Cascadia Code on an owned worker. Native text and map labels use the bounded `Scene::native_glyph` hook through the client host; font Braille uses actual glyph coverage. Chess (`ilium-ambient::live_chess`) owns a bounded Lichess TV line stream, validates authoritative FEN positions and draws original supersampled silhouettes. Feed clocks are last-reported values; the protocol supplies no observation timestamp, so only receipt age is claimed. No synthetic replay is used by these live scenes.

Wind (`ilium-ambient::scenes::wind`) is the one scene that reacts to the workspace. The `Scene` contract gained `wants_occupancy` and `occupancy(&OccupancyMask)`: for a scene that asks, `background_composition::screen_occupancy` builds a mask of every cell the compositor may not paint (outside the painted regions, or not a safe blank) from the final workspace buffer before each frame request, and `AnimationSurface::set_occupancy` carries it to the worker with a change counter that is part of the request and render-cache identity, so an unchanged screen costs nothing and other scenes pay nothing. Inside the scene, `flow` compares consecutive masks: a row that gained cells and equals a neighbouring row of the previous mask (whose source row changed) is vertical scroll, a shifted row is horizontal motion, anything else is text appearing from nowhere. `sim` integrates dots under wind, gusts, drag and optional gravity in fixed sub-steps, confines them to empty cells, and turns each classified cell into an impulse (along the motion for scroll, away from the cell for appearance) that also moves dots out of the newly occupied cell. Merged dots are scene-owned native glyphs, so the shared look still colours them.

Carpet (`ilium-ambient::scenes::carpet`) separates typed controls, bounded hidden-body simulations, legal chess, and hatch projection. Compact sphere/capsule heights combine by maximum; a reusable sampled height field bends only the hatch lines, leaving color and dither to the client. Camera projection and inverse ground picking share one transform. The default-enabled Infinite lines control extends the flat hatch lattice to viewport edges, including lines outside the simulation square; disabling it restores square-clipped ink. Exterior lines remain decorative: picking and all simulation coordinates retain the finite ground domain. The client forwards optional field-relative pointer coordinates through `Scene::pointer` without consuming terminal mouse events. Simulation speed integrates elapsed time without replaying backward clock corrections; civil clocks read `Frame::now` and an explicit UTC offset. Automated chess uses an owned cancellable search worker with stale-result guards; live chess reuses authoritative Lichess TV positions and retains captured-piece fades through subsequent feed updates. Presentation reconfiguration preserves games; changing the seed requests a fresh scene.

Snake stores its actual body and previous body for interpolation through turns and growth. A pure bounded breadth-first planner finds food routes inside the free forward arc of a Hamiltonian cycle, preserving the body's cyclic escape order. It commits each route until food is reached; cycle successors provide a legal fallback when no shortcut is available. Mode changes start the selected simulation afresh, including a return from chess. Step-time or easing edits preserve the game, settle the current pose and start a new interval. Each search visits at most one entry per cell and checks four neighbors, on boards capped at 32×32. Tapered capsules, a distinct head, feeding pulses and breathing food share the existing height renderer without exceeding the body-plus-food grid budget. All 44 Carpet defaults come from the captured session preferences; explicit saved values override missing-field defaults.

OpenStreetMap (`ilium-ambient::scenes::openstreetmap`) renders ten provenance-recorded, bundled ODbL extracts, local Overpass JSON or an explicitly configured HTTPS source. Stable destination and list IDs support five offline themes built from those exact bundles and expanded themes whose unbundled anchors require the configured service; a selected arbitrary point never resolves to a nearby bundle. The scene's saved label and coordinates are independent of the shared observer. Explicit Enter submits the picker's full address to the selected Photon, configured Nominatim-compatible, or legacy city-only provider; direct coordinates and map movement are local. The Photon/Nominatim search adapter validates raw coordinates before `GeoLocation` normalization, bounds responses and positive-result cache reads, permits no redirects, and rate limits submitted requests. The optional city-only provider retains Open-Meteo place-name matching and trailing-segment ranking through the same bounded parser, transport and 32-slot persistent cache. Cache slots replace older answers on hash collision but verify the full query digest before use. The picker retains actual worker admission after modal close, discards stale replies after input changes, and binds OSM confirmation to the exact project path captured at opening. An owned map worker decodes bounded inputs outside the render path; decoded bodies are limited to 16 MiB. A two-map cache and the last usable map survive camera and presentation changes without refetching. Projection and layer rasterization share the user palette. The client reserves complete attribution rows above the existing status and voice footer, using the same geometry for rendering, PTY sizing and settings interaction. If the viewport cannot show the complete credit, it suppresses the map. Source queries, timestamps, hashes and licensing are retained with the catalogue assets.

Voxel landscape (`ilium-ambient::scenes::voxel_landscape`) prepares absolute-coordinate 16-block chunks on one owned, cancellable low-priority worker. A pure terrain kernel supplies climate, exclusive solid heights, river occupancy and real cave air intervals. Ecology classifies the unchanged columns into 52 surface environments, then a deterministic wetland layer derives shallow pools and mud islands for both visible occupancy and feature placement; cached kernel columns remain unchanged. The catalogue contains 208 distinct geometry recipes and 64 original procedural 8×8 textures, distributed under this repository's MIT licence; no Minecraft or texture-pack artwork is embedded. Coordinate-owned features and complete village street plans are sampled through a halo, then a bounded overlay resolves solid blocks and air carves. The renderer projects exposed cube faces into a 2D depth buffer with stable global block identities and a total tie order. Camera motion uses absolute scene time, while the UI receives the latest prepared mesh through a single slot and never waits for generation. Material luminance drives the shared dither threshold; per-cell colours independently apply monochrome or pastel palette, hue, saturation and lightness. All scene controls persist through the existing ambient settings.

Saved Java worlds use the separate `ilium-ambient::minecraft` source pipeline and `SavedScene`, selected by `SavedMapsSettings` in the scene registry. `ilium-platform::minecraft` owns installation discovery and lossless directory identities. Blocking catalog, Anvil/NBT decoding, source qualification and native asset preparation run on the scene-owned worker; the UI receives prepared frames. Chunk coverage rejects absent, unfinished or unsupported input rather than inventing air. Each selected save can retain up to three disjoint, fully qualified radius-five windows under its stable map identity; across the four-save catalog this is bounded to 384 MiB of decoded-window charge within the 512 MiB catalog reservation. The planner admits up to 384 source chunks, while explicit coverage keeps disconnected saved regions separate and never fills their gaps. The planner retains its original block-evidence targets separately from the complete inverse-projected renderer source, which must cover the route, viewport and neighbor/tint support. Native solids and fluids share the selected asset bank and one raster depth/ownership frame. Final painted-owner acknowledgements alone advance appearance history; generation, source identity and issued-pose checks reject stale acknowledgements. History uses a separate cache repository and never writes to world files. This source path is still experimental and requires local native/render acceptance; generated voxel terrain remains the default.

Topographic maps (`ilium-ambient::scenes::topographic_maps`) embeds one 12-bit equirectangular elevation PNG per measured body under `ilium-ambient/assets/topography/` (NOAA ETOPO 2022, NASA LOLA/MOLA/Magellan/MESSENGER/Dawn, all public domain; provenance in `manifest.json`, regenerated by `build_assets.py`). Each frame samples the grid per Braille dot through a flat or orthographic camera, derives integer contour levels from the elevation and marks a dot wherever a neighbour's level differs, so lines are one dot wide with no vector step. A world is decoded or generated (fictional worlds: seeded spherical value noise, no seam) on one owned worker; the scene keeps drawing the previous world and dissolves to the next when ready. Everything is a pure function of the animation clock and settings.

Shared look (`ilium-ambient::style`, `ilium-ambient::dither`): colour, brightness and dithering are not scene concerns. `Appearance` (colour mode, 38 palettes, colour source, reverse/shift/spread, brightness, contrast, gamma, colour intensity, hue shift, invert, edge fade, grey tint, pattern contrast/invert, a colour `Filter` with strength (46 true colour transforms in `style_filters.rs`), 7 style presets) is one value on `AnimationSettings`, so changing it changes every animation. `AnimationFrame::pack` turns dot tones into Braille with any `DitherMode` (Bayer 2/4/8/16, stipple, blue noise from a void-and-cluster tile, gradient noise, halftone, line screens, crosshatch, white noise, or Floyd-Steinberg/Atkinson/Sierra-Lite error diffusion). The compositor then colours each ink cell from the scene colour (if any), dot coverage, screen position and time through `Appearance::shade`. The loop cache stores packed geometry only, so look changes never rebuild it. The current palette also reaches every scene: `SceneEnv::palette` at construction and `Scene::set_palette` afterwards (every constructor carries a note that plugins must follow it; `create_scene` wraps every scene in `PaletteScene`, which shifts colour scenes onto the palette). A panel choice (both/left/right) and a frame-rate cap are likewise global. Settings -> Animations is two columns: a compact scene list with prev/next and the global rows on the left, the selected animation's own rows on the right. Rule: see CLAUDE.md, "Background animations: the shared look is mandatory".

Vector TD (`ilium-ambient::scenes::vector_td`) is a self-playing tower defense. A `Director` advances one `Game` in fixed 1/30 s steps from a seeded generator, so it is a pure function of settings and forward time; a gap longer than 20 s of game time is skipped, and time running backwards restarts the match. The grid is 20 cells high and as wide as the screen's aspect ratio allows, so the board fills the terminal. Six axis-aligned maps are snapped onto it; each level builds the path samples, blocked cells and per-tower coverage masks once. The AI (`ai.rs`) values every purchase as expected damage per credit from path coverage, shared-path overlap (slows do not stack) and the needs of the next waves, and also decides when to send a wave early, which towers to replace, and where to build. Clearing a level raises the stage (more tower types, higher upgrade caps, more damage, tougher monsters); losing retries the level with a growing handicap relief. `render.rs` draws thin Braille lines, dithered fills and a 3×5 dot font through `draw::Canvas`, which also records the strongest tone per cell so the colour variant tints each cell by its dominant shape; the black-and-white variant supplies no cell colours.

Galactic empires is a live, seeded simulation in `ilium-ambient::scenes::galactic_empires`. Its connected hyperlane graph, resources, diplomacy, fleet arrivals and ownership advance in fixed half-second simulation ticks. A bounded catch-up budget discards suspend time. Territory rendering combines compact radial star influences into smooth per-owner fields, rebuilding affected channels when ownership changes and interpolating them as the camera moves. Neutral systems retain their own field; marker cells prioritize stars over fleets to preserve ownership colors. New maps default to 480 systems (50% above the previous 320), configurable from 120 to 720. The 27 scene controls use the shared control/persistence contract; map-generation changes rebuild the world, while visual and timing changes reconfigure it without losing simulation state. Territory radius rebuilds only its cached field. Ownership changes require fleet arrival along a real lane. After 800 ticks the largest empire begins a stronger campaign whose monotonic expansion guarantees eventual unification; the winner remains visible for a configurable pause (default 30 simulated seconds) before a derived seed starts another galaxy. The fixed-orientation camera covers a quarter of the galaxy's disk area and orbits clockwise, independently of simulation speed. No server state, network calls or helper processes are involved.

Settings → Animations uses separate scene and control columns with shared scrolling and responsive widths. Controls have Scene, Motion/rendering, Color and Playback/cache sections; loop-duration sliders use a logarithmic scale. `animation_rows` derives the row list from the settings and the hosted scene: one row per scene, then Background, the selected scene's controls (the built-in scene's controls, or `AmbientSettings::controls` of a hosted one), Location for observer-aware scenes, Speed, Dot density, Dither, the palette rows (hidden when the scene paints its own colors), Playback / Loop seconds / cache line (built-in scenes only; the latter two only while Loop), the scene status line, and the full-screen preview action. Slider, choice, toggle and text rendering, keyboard, mouse, scrolling, the scrollbar and Settings-help anchors all read the same `RowModel`; there are no fixed row indices. Edits go through `AnimationSettings::set_common_control` / `set_scene_control` (hosted scenes forward to `AmbientSettings::set_control`); an `Err(message)` is shown and nothing changes. A Text control opens the shared single-line prompt (`Mode::AnimationTextPrompt`, prefilled, validation message shown inside it).

Video source validation is lexical on the UI thread; discovery and filesystem access belong to its worker. Scans stop after 20,000 entries or two seconds, decoded dot dimensions are capped at 1024×512, and the presentation queue holds at most 12 frames. Local probe and frame operations have three- and five-second watchdog deadlines. Video scene teardown only signals cancellation; managed reapers retain thread and child ownership until exit, with admission held for delayed cleanup. At most two production video workers and four helper processes can be active, so rapid source or scene changes cannot create unbounded work. A blocked filesystem syscall may retain a worker slot, but the UI does not wait for it.

Video's `Series` selector chooses Custom discovery or the shipped Germination catalogue (`ilium-ambient/assets/germination.json`). Changing series preserves every other setting, including dormant Custom source text and recursion. Germination schedules original HTTPS URLs from the catalogue; it does not discover local media. Entries retain title, author, licence, source page, encoded length and SHA-256. The worker acquires one verified encoded body before probing or decoding, holds it in `Stored<Vec<u8>>`, and exposes a token-protected loopback endpoint supporting HEAD and single byte ranges. Both ffprobe and ffmpeg receive that endpoint through the existing request/conversion pipeline. Duration probing retains seekability. A verified catalogue GIF decoder sets `-seekable 0` to avoid a second complete-body header scan while preserving accurate input `-ss`, source-time limits and conversion settings; the verified probe supplies its displayed duration. No encoded media file or disk cache is created. Admission precedes body allocation, with a 64 MiB per-body cap and two process-wide body credits held through endpoint retirement. Provider spacing and HTTP-error cooldowns are cancellable client policies; changed content fails integrity validation.

A blocking frame read may finish after resize or cancellation. The player checks cancellation and current geometry again before interpreting EOF or failure, so an obsolete read result cannot complete or fail the current clip. Resize resumes the selected `PlayItem` at its source position; completion uses cumulative frames across resize segments to preserve Repeat one through a short final segment. A catalogued GIF clip begun at zero resumes by replaying the same conversion from the beginning and discarding exactly the cumulative output frames already queued. Each discarded frame retains the native watchdog and the player's cancellation and geometry checks; a source-time excerpt limit includes the replayed prefix. Custom inputs, other formats and excerpts begun later retain accurate seeking. Optional bounded Video diagnostics use the existing log sink and report native read results plus independent producer/render samples. Those samples are not a coherent snapshot or a terminal presentation acknowledgement; diagnostic runs cannot substitute for ordinary native acceptance.

The preview is the real field. The compositor paints one screen-sized field, at exactly the size and clock the background uses, through every safe blank of the Settings screen; the controls panel is opaque and drawn on top. `f` (or the Full screen preview row) hides the panel; any key or click returns. Opening a modal over Settings (prompt, help, location picker) hides the preview and drops a hosted scene until the modal closes.

The Location row opens `Mode::LocationPicker`: an address field searched on a bounded worker (results polled each tick and discarded if the modal closes first), direct `lat, lon` entry parsed locally, and a Braille world map with a crosshair (arrows, Shift for big steps, click). Render and mouse share one layout function. For stars, night lights and clouds, confirmation writes the shared `AmbientSettings.location`; for OpenStreetMap, it writes only `AmbientSettings.openstreetmap` to the validated project captured at modal opening. The OSM picker keeps the modal open on invalid coordinates, changed project binding or failed persistence. A dedicated three-row credit inside the modal names the Natural Earth public-domain land mask and the current search provider/data licence even when an error occupies the status row; the scene’s reserved OSM geometry credit remains separate and untouched.

`background_composition` reveals the field through eligible workspace blanks after ordinary rendering and before overlays. Text, styled spaces, selections, cursors and wide-character continuations remain foreground; editors, boards, chatroom and Smart Copy inspection stay opaque. Decoration changes only the final Ratatui buffer, so PTY content, cached screens, history, copied text and detection inputs remain untouched.

The client checks absolute integer frame boundaries after every event-loop branch, including busy input/output branches, at the active scene's rate (30 for built-in scenes, `Scene::frames_per_second()` for hosted ones, boundaries `ceil(bucket × 10⁹ / rate)` ns): a 1 fps scene wakes the loop once a second. Requests within a frame reuse raster buffers and pixels. Disabled or hidden ambient animation contributes no recurring deadline. Motion Off uses a static scene at time zero; an explicitly opened Animations preview remains live independently of that preference. Settings persist in `.ilium/config.yaml`; the hosted scenes' settings and the location are flattened into the same `animation` mapping, unknown keys are tolerated and an all-default block is not written.

## Process architecture

Client/server, like Zellij and tmux itself — this is what makes detach/reattach and session persistence possible instead of "just a TUI app that dies with the terminal."

```
┌───────────────────────────────────────────────────────────────┐
│  ilium-server (one process per session, spawned on demand)    │
│                                                                  │
│  ┌─────────────┐   ┌───────────────┐   ┌─────────────────┐    │
│  │ ServerState │   │ pane registry │   │ detection loop   │    │
│  │ (ilium_core│   │ (PaneResource:│   │ (tokio task,     │    │
│  │  ::Tree --  │   │  PtySession + │   │  adaptive poll   │    │
│  │  Node/      │   │  vt100 screen │   │  per pane, see   │    │
│  │  NodeKind)  │   │  per pane)    │   │  config.rs)      │    │
│  └─────────────┘   └───────────────┘   └─────────────────┘    │
│ UDS socket: $XDG_RUNTIME_DIR/ilium/<project-slug>-<hash>-<session>.sock │
└────────────────────────────┬────────────────────────────────┘
                              │ length-prefixed bincode frames
                              │ (ilium-ipc: ClientRequest / ServerEvent)
                  ┌───────────┴────────────┐
                  │ ilium-client (ratatui  │
                  │ TUI, one per attached   │
                  │ terminal)               │
                  └─────────────────────────┘
```

## Crate roles

- **ilium-core** — pure domain types: one `Tree` of `Node`s, with `NodeKind::Container(ContainerNode)` for normal groups and split views, `NodeKind::Pane` for terminals/editors/boards, and `NodeKind::Folder` for persisted filesystem roots. `ContainerNode` owns child-kind and split-capacity policy; `Tree::create_split_view` validates and moves selected panes atomically. No I/O, fully unit-testable.
- **ilium-pty** — adapter around `portable-pty` (spawn, resize, write) + `vt100` (parse the byte stream into a screen grid you can read text/cells from), plus xterm mouse-protocol encoding (`mouse.rs`) so a pane's foreground app (`vim`, `htop`, `lazygit`, …) receives clicks/drags/scrolls in whatever encoding it negotiated. One state owner per pane orders parser mutation, geometry, mouse encoding, input and terminal replies; platform workers move bytes.
- **ilium-detect** — the agent-detection engine. Two independent signals, combined:
  - **Identity** (which CLI, if any): walk the PTY's child process tree via `sysinfo` and match process names against the shared built-in provider registry (`claude`, `codex`, `agy`/`antigravity`), plus generic/custom signatures (`opencode`, `aider`, …). This is the primary signal — robust against UI redesigns, unlike text scraping.
  - **Activity** (thinking vs. idle vs. blocked): scan the vt100 screen's visible text for markers. A literal `"esc to interrupt"` substring is one recognized "working" trigger, but real Claude Code builds also render a present-tense status line ending in an ellipsis alongside a live elapsed-time token (e.g. `"✢ Moonwalking… (running stop hooks… 1/2 · 6s · ↓ 4 tokens)"`) — `looks_like_live_status_line` catches that shape instead of matching exact wording, so it survives whichever whimsical verb is showing. A `y/n`-style confirmation line or a numbered selection menu with a `❯` cursor means blocked (`WaitingApproval`); anything else with no agent CLI detected, or an agent CLI with no such marker, is idle.
  - First-party providers implement one pure shared contract for command launch, process-name aliases, resume syntax, CLI argument parsing, labels, and deterministic ordering. Adding a supported provider extends that contract rather than duplicating special cases through the client and server.
- **ilium-agent-session** — the shared transcript-provenance boundary used by both server-side session discovery and client-side LLM titling. It verifies Claude/Codex JSONL stores and Antigravity's UUID database plus `history.jsonl` project binding before accepting a session, preventing cross-project identities from leaking through lossy/global stores. Its pure byte parser returns a raw identity, never verified provenance. Bounded locators can inject an explicitly owned metadata parser; cancellation, admission refusal or worker failure invalidates the entire discovery attempt so partial evidence cannot become a unique match. All transcript and Antigravity history reads use the platform regular-file opener: Unix opens are nonblocking before handle validation, so a path replaced by a FIFO cannot wait for a writer; ordinary symlink resolution remains supported. The crate creates no execution bank or worker.
- **ilium-session-convert** — converts one agent session to the other built-in provider (Claude Code ⇄ Codex) so the conversation continues under the other CLI. Claude→Codex drives Codex's own `externalAgentConfig/import` session importer over a private `codex app-server` stdio child; Codex→Claude is a Rust transcript translator (user prompts, assistant text, shell/tool calls and results; reasoning and token events are dropped). It is a blocking function with step/log/progress events and a cancel flag, called from a client worker thread; the tree menu's **Convert to** action stops the pane's agent (`TerminatePaneProcess`), freezes the pane, shows the step/progress/log dialog, then `ReplacePaneWithCommand` swaps the pane for one resuming the converted session.
- **ilium-remote-compaction** — pure compaction pipeline for Claude Code and Codex transcripts (no async, no HTTP, no PTY). It parses a session file into a neutral conversation (Claude `parentUuid` chain with `compact_boundary`/`preservedSegment`; Codex rollout with `compacted` and `replacement_history`), masks old tool output, redacts secrets, builds a deterministic file/command/error ledger, chunks the input to the summarizer window, renders the technique prompt (Claude Code, Codex, opencode, Gemini CLI, best-of-all-worlds, custom; templates live in `ilium-prompts/templates/compaction/`), and calls an injected `Summarizer` (chunk, merge, retry, re-chunk on `ContextTooLong`, deterministic fallback). It then backs the transcript up and rewrites it atomically as a compaction (a `compact_boundary` plus summary record for Claude, a `compacted` record with replacement history and a fresh `token_count` for Codex), refusing when the file grew meanwhile. `compact_session` reports step/progress/log/token events and honors a cancel flag; `transcript_is_at_pause_point` and `latest_context_usage` feed the client's monitor. The client owns the rest: `remote_compaction_worker` (blocking worker, the `Summarizer` over `ilium-inference` with a long per-request timeout), `remote_compaction_flow` (wait for a pause, freeze, `TerminatePaneProcess`, compact, `ReplacePaneWithCommand` resume, failure recovery, the automatic context monitor with cooldown and a three-failure circuit breaker), `remote_compaction_dialog` (steps, progress, token bars, log, privacy banner) and the **Remote compaction** settings tab. The toolbar Compact button routes here when the feature is enabled.
- **ilium-ambient** — the ambient scene engines: the `Scene` contract, the shared dot `Raster`, data-driven `Control` rows and per-scene settings, the shared `GeoLocation`, address search, the world map and the pipes, stars, night-lights, clouds, video, spectrum and images scenes. No terminal, ratatui or client types; `ilium-client` hosts it (`background_animation::AmbientHost`) and owns the Settings UI, compositing and persistence.
- **ilium-wikipedia** — a data adapter for English Wikipedia’s daily Main Page article pool, bounded cached HTTP/image decoding, semantic rich HTML documents, and an owned cancellable loader. The client’s `background_animation::wikipedia` owns native text and font/Braille page layout, presentation controls, scrolling and terminal-safe composition; the server and PTY source do not participate.
- **ilium-git** — the Git adapter for repository discovery, registered worktrees, branch and dirty-state probes, and worktree mutation. It owns Git command arguments and porcelain parsing; the server owns pane lifecycle and removal policy.
- **ilium-server** — owns all PTYs and the tree (`ServerState`), runs the detection loop, the single scheduled-input executor, and generation-fenced progress coordinators, writes a JSON crash-recovery snapshot to `<project>/.ilium/sessions/<name>.json` after structural or monitored-lifecycle changes, and restores panes, pending deadlines, and conservatively recoverable monitors on startup. The CLI gives it one exact project-session socket, so one process serves exactly one session with no multi-session registry.

**Start-up progress.** A server restoring a large session is busy long before it accepts connections, so it publishes its current work to `<socket>.startup` (`ilium-ipc::startup`: category, item, completed, total; atomic replace, removed when ready). The client takes the terminal before its slow discovery (sounds, audio devices) and shows a three-line centred dialog -- category, item, progress bar -- first written straight to the terminal (`startup_dialog::StartupDialog`), then painted by `ui::draw_startup_dialog` over the interface until `InitialStateSyncComplete`. An unknown total draws a moving bar.

**Terminal parser engines.** The client's `terminal_parsing` has no engine-count cap; only bytes are bounded by `terminal.engine_memory_budget_mib` (default 4096 MiB, 256 to 16384). The same budget limits retained engine state and published snapshots, and the parser reserves twice that amount from the process quota at start (later increases apply after a restart, decreases immediately). Panes leave their engine in place when hidden, so revisiting is instant. Only when a displayed pane is refused for memory does `collect_terminal_parsing` evict the least recently focused hidden engine (`TerminalView::evict_parser`), and the client sends `DiscardTerminalDelivery` so the server forgets what it delivered and replays its journal when that pane is next displayed. Engine retirement on the parser thread wakes the client loop, and the loop retries every 50 ms while a displayed pane has no engine, so a selected pane never waits for unrelated input. A displayed pane without an engine renders an explanatory message instead of black.
- **ilium-client** — the `ratatui` TUI: left tree panel + right presentation target, keybinding dispatch (`keys.rs`/`keymap.rs`), one-step tree reordering, and shared `split_layout` viewport geometry used by rendering, PTY sizing, focus, and mouse routing. It sends `ClientRequest`s to the server and renders the `ScreenUpdate`/`PaneStateSnapshot`/`PaneStatusChanged` events it streams back. Its `TerminalView` also compares allocation-free visible-character fingerprints while applying live output, feeding the client-local ordinary-terminal activity animation without adding a server poll or wire state. It owns built-in editor and board panes plus background LLM-assisted session/project naming and the client-local immutable Smart Copy snapshot/worker lifecycle through `ilium-inference`'s selected-provider boundary. When Kilo paid-proxy egress is enabled, its boot path reads the configured MongoDB collection before entering the terminal and keeps the loaded rows in memory only.
  - Reset planning is client-local: one owned background monitor checks the public Claude and Codex announcement feeds while a client is attached, reports observations over a bounded channel, and stops on detach. Settings persist globally under `[reset_planning]`. The Codex status contract distinguishes an `active_watch` forecast window from a scheduled announcement; the status bar labels the forecast window's expiry as the end of the watch, never as a guaranteed reset time. Scheduled announcements may have a null target time, which is shown as time TBD rather than an invented countdown. Claude's current public catalog contains historical announcements but no future schedule field. Reattach triggers an immediate fresh check, so this presentation state does not enter session snapshots or IPC.
- **ilium-inference** — provider-neutral title/organization/Smart Copy inference. Its base provider contract has concrete Kilo Gateway, local Ollama, OpenAI-compatible, Anthropic, and OpenRouter implementations, with both whole-response and incremental streaming entry points; the client owns its persisted credentials, endpoints, selected models, and the MongoDB source/field mapping for Kilo paid proxies. Proxy records themselves are runtime-only and are never serialized into `config.toml`. Requests use the model's maximum output allowance when known; prompts, not convenience caps, control normal response length. Official OpenAI uses `max_completion_tokens` with documented exact-model maxima, omits forced sampling parameters, and omits the explicit limit for unknown IDs rather than transmitting the 1,000,000-token fallback used by other adapters. OpenAI-compatible catalog discovery authenticates `GET <base>/models`, returns sorted exact IDs without inventing capabilities, and exposes credential-redacted endpoint metadata. The client keeps a last-good catalog and fences asynchronous results with a settings revision so credential/URL/provider edits, including edits away and back, cannot publish stale results. Kilo exposes a live, unauthenticated free-text-model catalog in Settings, with stable Kilo/OpenRouter free-router fallbacks when discovery is unavailable.
- **ilium-execution** — bounded finite-job CPU and I/O banks on real OS threads, with a separate optional bank for persistent services. Typed jobs retain caller-owned rejected input and charged results; cancellation preserves actual completed effects. One explicit quota group is shared at each process composition root. Per-process limits do not imply a machine-wide limit; external library threads require separate admission. The platform worker supervisor remains the sole join-handle owner, and physical worker charges survive callback return and delayed thread retirement. Domain services retain responsibility for stream ordering, durability and actual presentation acknowledgements.
- **ilium-logging** — the shared process-diagnostics boundary used by both the detached server and every attached client. One server start selects one private timestamped file under `/tmp/.ilium/<project-session-id>/`; all attached processes append to that path, and the live `[debug].file_logging_enabled` setting opens or closes their writers without changing call sites. Enabling it records complete HTTP and LLM text requests, responses, and errors while still redacting credential headers and URL parameters and replacing binary audio payloads with size summaries; unrelated sensitive per-agent evidence remains outside this process log unless a broader `RUST_LOG` filter is explicitly requested.
- **ilium-ipc** — `ClientRequest`/`ServerEvent` wire enums plus `write_frame`/`read_frame`: a 4-byte little-endian length prefix followed by that many bytes of bincode payload, generic over any `AsyncRead`/`AsyncWrite` so both the request stream and the event stream reuse the same framing code.
- **ilium-sound** — cross-platform adapter for XDG/Linux, macOS, and Windows system-sound discovery plus bounded native-command playback. It also owns the pure agent-status transition mapping used by the server, while `ilium-client` only presents the discovered catalog and edits the shared settings.
- **ilium-voice** — provider-neutral owned actor for full-duplex audio, streaming sample conversion, interruption, and live-provider transport, and for text turns (`VoiceCommand::SendText`): typed sentences are queued and become one user message plus one response request each, in order, only when the provider is free (no response in flight and no tool outputs pending). Its OpenAI adapter speaks the Realtime WebSocket protocol, but the crate has no dependency on ratatui, IPC, or ilium domain types. The client-side `control` module is the separate semantic capability layer: typed commands, stable tool schemas, target resolution, redacted state snapshots, confirmations, deduplication, and structured results. This composition keeps a future provider adapter from duplicating UI behavior and keeps voice from simulating fragile keyboard/mouse coordinates.
- **ilium** (bin) — `clap`-based CLI: `ilium` attaches or creates the `default` session for the current canonical directory; `ilium new-session <name>`, `ilium ls`, `ilium kill-session <name>`, and `ilium new-pane --session <name> -- <cmd>` remain project-scoped. It spawns `ilium-server` as a separate detached process and hands off to `ilium_client::run` for the TUI.

## Worker and service execution

`ilium-execution` starts fixed OS-thread banks for finite CPU and I/O jobs, plus
an optional bank for persistent services. Async tasks wait for typed receipts;
they do not provide CPU offloading. Each composition root owns the bank and
shares one `QuotaGroup` among its clients. Admission bounds outstanding jobs,
waiting slots, captured inputs, retained results, physical threads and declared
resident storage. A reservation precedes expensive capture or stateful
preparation. Reserved publication remains valid during a concurrent drain;
explicit cancellation returns work that never started. Callback panics produce
failed receipts and leave subsequent finite jobs runnable.

Bank construction also has an explicit partial-start owner:
`Execution::start_with_custody` returns the original failure together with any
bank whose workers already started. The caller cancels and observes that actual
bank before retrying. The existing `Execution::start` API performs this cleanup
on its bootstrap caller; if its five-second join deadline expires, the original
spawn error contains a typed `StartCleanupError` retaining the bank for later
background observation. A callback exit or elapsed deadline never establishes
physical shutdown. This constructor seam passed all 13 debug library tests,
strict all-target Clippy, its five new native cases in release mode and an
optimized library build. The 74 recorded source and manifest inputs matched
current readback; the release artifact and physical child cleanup were verified
(`ilium-worker-foundation-native-879-1312`). A separate integration run passed
42 debug cases across ten admission, quota, retirement, CPU and shutdown test
binaries, plus four optimized CPU, nested-shutdown and startup-ownership cases.
All 74 inputs matched current readback and every child was reaped before the
isolated target was removed (`ilium-worker-foundation-integration-879-1315`).
These checks qualify the execution foundation only. Client startup awaits one
bootstrap owner on the existing runtime.
A bounded transfer acknowledgement moves the actual bank only after the caller
has taken custody; cancellation leaves it with the bootstrap owner until its
native workers join. Failed admission retains the original initialized owners
in `ClientExecutionStartError`. This client seam is also source-integrated and
formatting checked, with five native cases authored but not yet executed.
Later service-startup rollback and complete client shutdown remain separate
integration and verification requirements.

Presentation shutdown retains its completion receiver and original output
failure across cancelled waits. The existing platform supervisor notifies the
presenter only after native join, thread-local destruction and custody release;
the presenter checks that physical result before returning success. It creates
no observer thread or per-call blocking task. Output drain and physical exit
share one five-second deadline, and observation failures retain the actual
ticket and any original output error. This source seam and formatting checks
are integrated; the real TLS and blocked-output cancellation tests are authored
but remain unexecuted pending the client verification stage.

Transcript metadata parsing can use the existing CPU bank through the bounded
locator's supplied parser. Synchronous filesystem traversal and receipt waits
belong to an admitted persistent I/O coordinator, outside the finite I/O bank;
a finite I/O callback must not synchronously wait for nested CPU admission.
Canonical project/store checks remain with the locator after raw parsing.
The parser boundary has passed its 19 native library tests, strict lint and
release library build. Those checks do not prove conversion caller offloading,
physical child cleanup or coherent client/server release behavior, which remain
separate integration requirements.

`Client.child` gives related producers independent feature limits beneath one
aggregate parent. Admission charges every ancestor; sibling identities cannot
multiply that parent's jobs or bytes. The ownership depth is bounded to eight.
The client's general preparation parent allows512 MiB of inputs and512 MiB of
results collectively across interactive and standalone banks. Process-shared
admission groups separately bound all decoder banks to128 MiB and all encoder
banks to128 MiB, bringing the process ceilings to768 MiB per dimension. Multiple
readers awaiting frame bodies cannot consume the encoder's directional headroom.
A dedicated codec CPU thread avoids waiting behind lengthy document jobs. These
declarations still require queued payloads and installed projections to retain
their own charges; bank shutdown does not close an independent bank's group.

The current server IPC path still has uncovered ownership boundaries. Its raw
64-entry direct queues and 1024-entry broadcast queue lack admission for retained
payload bytes before cloning. Completed encoded frames retain finite encoder
job credit through socket flush; two blocked connections can therefore exhaust
the shared two-job encoder tenant. The client normal publisher retains a refused
head and tail, but its final shutdown loops currently move those requests into
an awaiting iterator and lose unpublished originals on error or cancellation.
The frozen real-server baseline reproduced four exact native assertions:
encoder starvation with two blocked transport flushes, stale trigger delivery
after an A-to-B-to-A edit, missing authoritative resynchronization after a
failed earlier delivery, and coordinator destruction of the original matcher
engine. Its compiler passed and every case reached its intended assertion;
these are component failures, not live TUI measurements
(`ilium-worker-server-baseline-879-1321/gate001/primary-audit.json`).
Private trigger corrections, complete-chunk output batching and a same-pass
encoder transfer into the existing shared byte ledger now pass the frozen
server library suite: 497 tests passed and nine were ignored, including all
six new output-batching cases and nine trigger/encoder cases. All-target strict
lint and an optimized server binary also passed. The original byte-ceiling
regression was reproduced separately. An earlier qualification mistakenly
reused the baseline executable for the candidate; the successful retry used
separate targets and verified distinct test inventories and executable hashes.
Eight gate logs, final binary identity, child reaping and scratch removal were
independently audited. This private binary is retained, not installed or active
(`ilium-worker-output-qualification-879-1335/primary-audit.json`).
That first transfer bounded retained storage but still dropped its directly
owned event and frame on the writer task after flush, error or cancellation.
The private three-file successor now places the exact event and its single encoded frame in one
bounded retirement envelope. Codec retention attaches before submission,
covering abandoned result receipts; persistent storage attaches before finite
codec credit releases. All nine actual writer-path retirement tests now pass,
alongside the full frozen server suite: 504 passed, zero failed, nine ignored.
All-target strict lint and an optimized server binary passed. An independent
audit verified five gate logs, all 22 required retirement/trigger/output cases,
release identity, child reaping and scratch removal. The retained binary is
not installed or active (`ilium-worker-retirement-qualification-879-1339/primary-audit.json`).
Raw producer admission before attachment remains unimplemented. The private client
shutdown caller keeps
its actual iterator and FIFO flush receipt in the preadmitted complete root;
queue publication, physical flush and server acceptance remain separate facts.
That caller has passed source review and parsing only. The asset-complete
request-drain component reproduced the original lost-request assertion, then
passed four ownership cases and eleven standalone connection/IPC cases, strict
all-target lint and an optimized library build. Its 80 frozen inputs remained
unchanged and its children and build target were retired. Six tests requiring
the complete App remain deferred; this component result does not qualify the
root caller or complete client (`ilium-worker-request-drain-879-1318/gate003/primary-audit.json`).
The complete client outbox also contains prepared commands. Its private
connection successor preserves those commands and admitted name/project leaves;
the tested ordinary-request component cannot replace that successor unchanged.
Assembly rejected their conflicting connection postimages explicitly. A private
successor now rebases the drain custodian and publication permit onto the complete
`OutboundRequest` envelope, preserving prepared commands, admitted name/project
leaves and all existing connection tests. The current private assembly includes
37 source targets with internally consistent successor hashes, including three
board editor domains. The complete client preflight subsequently rejected
assembly against its 2,047-file frozen baseline before invoking Cargo: several
chains started at later predecessors and one animation collector forked from
an older inference caller. The collector fork is now reconciled. An audit
recovered seven exact initial files and verified 47 earlier source edges;
six historical initial byte images remain unavailable. Six new composites
against the actual frozen baseline now preserve the reviewed latest sources;
independent exact patch replay passed for each. The complete private graph
now selects 86 targets and preserves the earlier prerequisite edges. The
asynchronous worktree caller and later shutdown assembly have independent
exact replay checks; twelve shutdown edges across eight targets replayed
without changing the earlier source selections. Board preparation now uses its
existing completion wake before writer admission closes. Animation installers
and the complete animation/location controller integration remain source
prerequisites before compilation. Pending native qualification is recorded
separately from missing source, so the compiler runner does not require tests
to have already passed before compiling. This does not reconstruct the missing
historical images or qualify the current workspace.
The worktree test migration exposed a hidden-parent reply mismatch: incoming
repository facts and errors accepted the original parent beneath a choice
dialog, but their collectors checked only the top-level mode. Two private
successors use the existing parent lookup for collection and installation while
retaining project and exact dialog identity fences. Both pass scoped formatting
and exact patch replay. Discriminating success/error retention tests are authored
and preserve the original allocation pointers and complete accounting; their
native qualification remains outstanding.
Private preservation
patches retain the baseline's CPU-decoded Settings dispatch, original editor
accessors and admitted prompt fixtures alongside newer board/debug changes.
These patches pass formatting and exact replay, not compilation.
The private startup-failure successor constructs animation defaults inside the
existing admitted I/O job and keeps them in the same retained startup output as
the home path and read error. Four source facets pass independent hash and exact
patch-replay checks; their native tests remain unrun. The proposed App aliases
must be installed before UI access. Source review caught a bootstrap call to an
uninstalled home alias; the sealed client composite now passes
the existing session-directory fallback directly to the I/O job. The animation
surface/worker settings-handle migration remains private and unfinished.
The board
domains capture the original model and command, prepare
a validated candidate on CPU, and publish saved state only after the existing
ordered writer acknowledges durability. Unknown write outcomes retain both
models without automatic replay or rollback. Nine board patches replayed
exactly against their sealed predecessors; the 17 authored board tests, seven
prepared-command drain tests and full-client compilation remain unqualified.
Direct filesystem cleanup also needs to advance accepted preparation before
closing writer admission. An accepted editor writer may already hold a source
promise while its CPU `SaveSource` operation is still pending. The private
editor shutdown facet reuses the actual loan completion path without scheduling
defaults or replay, and retains one shared five-second deadline. Both patches
have independently verified hashes and exact replays; compilation and native
save/readback checks are unrun. Accepted board preparation and closing
instruction commits need the same ordering before their writers close; those
consumers remain integration prerequisites. On deadline, the outer cleanup
retains the whole App rather than claiming unconfirmed writes were saved.
Instruction shutdown must distinguish draining from ordinary user cancellation:
the existing Cancel path cancels preparation and forbids body submission, so
it cannot finish an accepted commit before writer admission. A private parent
facet now reconciles the same original completion without invoking the normal
dialog pump. Its acknowledgement logic is byte-identical to the original,
with exact patch replay and scoped formatting verified. Session draining and
cleanup caller integration remain outstanding; native persistence checks have
not run. Previously user-cancelled sessions must never be resubmitted by shutdown.
The private cleanup caller now drives accepted family preparation before using
the cancellation-retirement helper, which otherwise cancels live CPU receipts.
Both normal and error cleanup filesystem calls pass the existing native input
parent to the drain. Success must also require that parent's original completion
has actually reconciled: taking an App notice alone is insufficient when parent
identity validation fails. Exact caller replays and source checks passed; the
combined native-parent fence and shutdown integration remain uncompiled.
The shutdown fixtures also need a constructor that explicitly omits implicit
test workers. Ordinary test `App::new` initializes several owners through the
global test bank before a fixture can inject its own clients. Replacing those
owners afterward cannot prove that only the isolated bank was started. The newer private fixture assembly selects 91 sources and preserves the previous
baseline, lock and source edges. Independent replay verified its nine assembly
stages; three helper newline corrections are composed as separate exact facets.
Private test-only constructor facets omit all 13 direct and nested shared-bank
acquisitions while preserving ordinary defaults. Four actual App/native-parent/
ordered-writer shutdown cases and corrected editor-save fixtures use that seam;
exact patch replay and formatting passed, but compilation and native execution
have not run. These source checks do not establish isolated resource bounds.
The separate frozen 2,074-file incomplete-source client diagnostic exited 101
with 308 compiler error records and no native tests. Source hashes were stable;
the runner was reaped and both owned scratch directories removed. Besides known
animation/location gaps, shared retirement handles around board rollback models
require `Sync` that their UI textareas cannot provide. This needs an explicit
single-owner model transfer and immutable projection, preserving rollback.
The diagnostic also omitted the qualified conversion candidate. A newer private
assembly incorporates its 23 exact postimages, including two explicit predecessor
bridges, and adds only the existing local execution dependency to the conversion
package's lock entry. Unrelated lock changes remain rejected. The combined client
and ordinary prompt/value caller integration are still uncompiled; package-level
conversion tests do not qualify the full client or current workspace.
The subsequent private assembly selects 125 sources, including the sealed
13-source location preparation and save controller. Its App patch commutes
with the newer shutdown and isolated-test constructor facets; inverse replay
restores the exact previous App. The mouse merge replaces only the location
handler, preserving the newer search and pointer ownership changes. An initial
patch attempt encountered already-applied formatting hunks; the recovered merge
preserves every unrelated byte and retains the failure evidence. No location
native case or combined compiler gate has run. Two further private caller fixes
use the initialized process quota for conversion startup and retain the same
worktree opening request after a lost or already-consumed receipt. The latter
includes an authored, unrun actual CPU receipt ownership regression. Board model
transfer, ordinary value-dialog producers and the broad animation source and
render integration remain active prerequisites to full-client qualification.
Editor runtime caller reconciliation must preserve both ownership and existing
admission. A private exact title-inference facet now classifies owned and loaned
editors as nonterminal panes. Restructure and control snapshots have prepared
immutable editor-context consumers, but their context producers, module/test
registration and fallible App caller remain outside the selected assembly.
The reviewed snapshot candidate would also revert shared catalogue capture to
UI iteration and serialization; only its editor hunks were retained privately.
Projected editor operations must retain their existing snapshot scratch debit
when adding context preparation cost. Replacing that debit with context cost
would leave accepted snapshot allocation unaccounted for. These consumers remain
uncompiled and unrun; source review does not establish complete editor offload.
Three private producer facets now prepare editor context on the existing CPU
load and bulk-operation owners. They preserve the newer generic pane/worktree
fences, acknowledged windows and context-aware editor operations. Both initial
and projected loans retain the augmented scratch cost for retry, and projected
operations add context cost to their existing snapshot cost. Exact patch replay
passed; the pane and bulk files pass single-file formatting. The filesystem
facet retains two inherited formatting differences. Helper/module registration,
fallible App callers and context-refresh test adapters remain outside the graph;
these prepared producers have not compiled or run. Context refresh must use a
distinct operation fence kind rather than the existing prompt-snapshot kind.
The latest private assembly selects 127 sources after six exact board-transfer
patch replays. Mutable board models and rollback acknowledgements have single
owners; immutable read views exclude textareas. The same original model returns
through a bounded loan channel, and presentation surfaces install only after
the actual terminal flush acknowledgement. Read capacity is computed by the
existing I/O producer and CPU candidate preparation, avoiding UI column scans.
The original and candidate textarea declarations remain conservative; legacy
board edits and persistence capture are still open boundaries. Compiler and
native tests have not run on this assembly.
Independent editor producer review verified forward and reverse replays and
preserved retry and snapshot accounting. A prepared successor assigns context
refresh fence kind 20 and makes the editor path read-only outside its owner,
with a setter that invalidates context identity. Exhaustive caller migration
and current-API native fixtures are required before selecting that successor.
The older displaced client-library check was recovered from its original exit
receipt: compiler 101, stable source, no native execution, both recorded runners
absent. Its diagnostics are retained and its two disposable directories removed.
The subsequent private assembly selects 150 sources, adding the context helpers,
fallible project gather, module registration, four adapted context test files,
ordinary value-dialog producers and exhaustive path-accessor facets. Rebased App
patches restore the entire current predecessor on inverse replay. Independent
review found a remaining production snapshot path access and a test include in
the wrong helper scope; both have exact replayed corrections and pass scoped
format checks. These source checks still do not establish compilation or native
behavior. RefreshContext has no production retry scheduler yet. A hidden editor
may have no acknowledged painted window, so a refresh must preserve the original
model and native-input fairness without fabricating geometry or occupying a
global semantic head indefinitely. Independent source review verified the 150
selected hashes and references for 14 context and 10 new leaf cases. Two included
test files now use ordinary comments without changing cases or assertions.
Production refresh scheduling and compiler/native qualification remain open.
The next private assembly selects 154 sources. Eleven Git-token, animation
constructor and cost/sound original-model patches replay forward and backward
with zero fuzz and exact bytes. Git dialogs retain their matching save token
through ordered writer acknowledgement. Cost and sound models expose immutable
originals with CPU-computed or indexed allocation declarations; their startup
producer, reader, update and final-release integration is still incomplete.
The additional modules are registered, but this assembly has not compiled or
run native tests.

Further editor review found a behavior-preservation risk: optional context
preparation adds six times the model declaration plus 1 MiB to every semantic
input job. This can reject edits that fit the previous admission limits.
Optional context must have admission and retry policy separate from semantic
input, without increasing limits. The existing read-only operation capture
already supports an unpainted editor through the actual original model and
installed-window scalar projection; it does not require fabricated terminal
acknowledgements. A separate failure risk remains: a panic in optional context
preparation currently classifies a completed semantic operation as a panicked
model. Regression fixtures for both boundaries are being prepared; no native
failure or correction has yet been qualified. Two actual-original Defaults
operation regressions now exercise the inherited admission plan and a test-only
panic at the optional phase after semantic completion. Their assertions include
original model, undo, cursor and identity preservation plus actual bank drain;
they remain unrun. A 156-source successor also adds Git CPU catalogue opening,
step edits and ordered acknowledgement fences. Its diagnostic compiler check
runs against 2106 frozen files and cannot establish whole-feature acceptance.

Startup still has a distinct producer gap: system-sound and audio-device
discovery run synchronously while the startup dialog is active, before the
shared execution bank starts. Wrapping those raw results later does not move
their production or final failure destruction off the caller. Startup must
transfer admitted original sources from the existing I/O/CPU owners, preserve
execution shutdown on every startup failure, and avoid creating a second bank.
Acknowledged pane surfaces expose another presentation accounting boundary.
The current guarded buffer retains a shared storage declaration for two queued
frames and the diff base, while queue reservations release independently of
external buffer references. Multiple editor or board loans can therefore keep
distinct older buffers alive beyond that declaration. Clearing unused pane
caches does not bound active originals. A private actual-presenter regression
retains acknowledged buffers and checks refusal before another frame allocation;
it preserves all original production code and passes formatting, but has not
compiled or run. The retained-buffer lease, pre-copy admission and final-owner
CPU retirement remain required work, alongside the compositor copy boundary.
Debug-log export is still being integrated across its actual replay/live cache
producers. A private client dispatch successor now offers the whole decoded
event wrapper to the CPU cache owner before unpacking it, restores refused
originals at the FIFO front, and fences later events until publication. Its
exact patch replay and scoped formatter passed; compilation, native ordering
checks remain unverified. The private client lifecycle now configures export
with the existing filesystem tenant and notification, pumps normal completion,
and drains accepted exports before worker closure on every Result cleanup path.
A bounded deadline preserves the whole App, accepted writer and execution owner
in existing shutdown custody; combined filesystem/export errors retain both
causes. The private export worker is now sealed across cache producers, original
snapshot capture, CPU report preparation, ordered writes and status consumers.
An independent audit verified 37 source and patch hashes; nine changed-file
patches replay exactly and 14 sources pass scoped formatting. Nineteen new
native cases, including actual Root deadline custody and persistence readback,
are authored but unrun; full-client compilation remains outstanding.
Its report writer validates the opened handle before truncation, rejecting
non-regular files, FIFOs and symlinks while preserving private permissions.
Where parent-directory synchronization is unsupported, the acknowledgement
explicitly confirms saved file contents while leaving directory metadata
durability unconfirmed. Actual synchronization errors retain the request and
error without automatic replay. Location,
animation-text and create-board preparation plus the full animation source
migration also remain outstanding. A private CPU normalization stage now accepts
only already admitted candidates, preserves the exact candidate on failure,
and publishes independently retiring normalized settings. It reproduces the
existing bounded serialization and serde normalization semantics; its three
new cases are authored but unrun. Exact patch replay and scoped formatting
passed. The existing typed configuration family writer now has a private
animation payload carrying that retiring prepared owner, so queue admission
need not traverse or copy settings. Its adapter-required owned copy executes
on the existing I/O worker. One actual ordered-writer/readback case is authored
but unrun. A further private successor returns the exact prepared owner in a typed durable
animation receipt. Its save intent keeps distinct authored and committed source
versions, rejects foreign candidates and homes, applies only newer durable
versions, and treats duplicate acknowledgements idempotently. One additional
real-bank fence case and the updated writer readback case are authored but
unrun. Exact three-file patch replay and scoped formatting passed. A private App collector successor now handles the typed animation receipt and
prompt-intent variant, with distinct authored and committed source holders.
It reports unexpected receipts and quits without claiming a successful source
transition; write errors do not authorize rollback or replay. Its four patches
replay exactly and scoped formatting passes. A later collector successor
preserves the newer inference Settings-step input acknowledgements while adding
the same animation source holders. Private Root module registration and an
actual startup caller now route project reads and accepted migration through
the existing I/O tenant, then promote the original settings on CPU. This caller
has passed formatting and exact patch replay, but depends on the unsealed App
installer and failed-read fallback ownership. Startup publication, CPU edit
producers and render consumers remain incomplete.
Compilation and native collector tests are unrun, and the legacy raw settings
fields remain until those consumers move together. Preparation does not hide
preceding UI copies. Its private source adapter
now represents both the original startup settings and an independently admitted
prepared settings version, preserving the startup path owner and checked source
revision. Two new identity/refusal tests are authored but unrun; App, writer and
engine caller migration is still required.

The semantic queue inventory covers all 48 broadcast occurrences (46 actual
calls), 79 direct publication/helper expressions and nine allocation/recovery
builders. Broadcast receive currently deep-clones before filtering or encoder
admission, and full synchronization eagerly builds a vector of tree, replay and
metadata payloads. Producer pre-copy admission and receiver sharing must both
change; a wrapper charged after construction is insufficient. Normal lag
recovery does not reproduce every semantic reply, and final broadcast drain
does not repair lag (`ilium-worker-ipc-queue-inventory-879-1317/semantic`). Those
boundaries, opaque regex/VT phases and remaining pending matcher originals are
still open; neither physical retirement nor a passing component gate proves
semantic delivery or completion of the worker architecture.

`Client.try_reserve_detailed` returns the same finite reservation with typed
refusal evidence. A failed quota check records the requested increment, observed
usage and limit for its actual dimension while the admission gate is held. The
boundary identifies the process root or an ancestor distance from the requesting
client, so a free filesystem child can report a refusal by its shared parent.
These immutable numbers describe that check; concurrent releases can make a
later usage snapshot different. Lifecycle, queue and busy-gate refusals carry no
invented quota observation. The reason-only entry point delegates this admission
path and preserves existing retry classification. Ordered filesystem writers
add typed evidence for their own 32 pending-write and 64 MiB retained-byte
limits, overflow, closed state and identifier exhaustion; execution refusal
keeps the rejecting ledger's evidence. Owned admission rejection returns the
original job. Lazy configuration, editor, board and integration adapters show
this distinction without changing capture order, FIFO, persistence or quotas.
The reason-only writer entry points use the same admission path. Seven focused
writer tests pass in the frozen client qualification; full current-workspace,
release and live checks remain outstanding, as do other admission adapters.

Terminal output and replay use a closed, single-copy byte adapter in the IPC
contract. Its bincode representation stays identical to the original byte
vector: a fixed-integer length followed by contiguous bytes. The bounded
decoder borrows that slice, admits its exact output allocation before copying,
and rejects nonborrowing callbacks. This lets the unchanged 32 MiB PTY journal
(including a truncated replay reset prefix) fit the existing 64 MiB decode
limit. Generic collections and user-input byte vectors retain their
conservative growth accounting; raw output ordering and replay watermarks
are unchanged.

`Retained<T>` carries its admission debit through clones and asynchronous
transfers. `Reservation::retention()` shares the original debit during preparation
and refused publication without admitting another payload; the guard alone
does not move destruction to a CPU worker or prove native retirement.
Keeping an error, result or receipt can keep that debit alive after
the callback finishes. Bank storage stays charged while a monitor or client
retains it after workers exit. Platform-owned join supervision retains physical
worker admission through actual thread exit, including blocked native calls and
thread-local destruction. External library workers need explicit
`WorkerAdmission`; a process-local quota does not impose a host-wide limit or
prove an allocator/RSS bound. Declared peak costs and native library parallelism
must be audited at each producer.

`Execution::retirement()` pre-admits up to 64 payload envelopes independently
of ordinary document-job slots. `Retiring<T>` moves a last owner onto the
existing CPU bank; `RetiringArc<T>` gives independently cloned leaves the same
contract. The original storage and job guards stay inside the envelope through
actual destruction. A separate finite recovery mailbox preserves the original
id, type, allocation and retry sender when publication fails. A connected
channel alone does not prove a CPU consumer remains: the last CPU body rescues
stranded entries, and shutdown reports incomplete while recoverable owners
remain. Callers must retain the execution/recovery owner through their final
frames and acknowledgements. These primitives are implemented and have focused
forcing coverage; editor, clipboard and context-menu ownership migration remains
unfinished. Queue admission and declared bytes do not establish a native RSS
bound or guarantee destruction after every permitted execution owner is lost.
An isolated native forcing test drops the combined error and execution bank
while the CPU callback is blocked. After release, both original 64 KiB payloads
retain their addresses and contents; the outer and two nested destructors run
on that same CPU worker. The test verifies all three retirement envelopes are
released and physical thread admission returns only after native exit. It keeps
client and observation capabilities through the check. This qualifies that
disposal sequence, not arbitrary owner loss or complete client shutdown.

The remaining editor migration must cover each independently retained leaf,
including syntax tokens, rendered documents, context text and TextArea undo/yank
storage. Charging a preparation result does not cover a separately cloned body
or defer its last-reader destruction. Context operations capture editor instance
and revision before preparation; admission precedes owned prompt and text copies.
Refused pane creation retains the original editable draft and captured parent,
and records focus only after publication. Editor copy dispatch now submits to
the same bounded clipboard owner and completion acknowledgements as terminal
copies, reporting success only after acknowledgement. The narrow caller is
applied; its frozen candidate compiled and passed the existing clipboard tests.
Current release and live acceptance remain unverified. The other editor
ownership requirements above remain incomplete; this caller change does not
qualify native memory or whole-context capture.

Source editor preparation uses the existing document CPU bank with a per-turn
capture budget, bounded viewport continuations and revision/width/cursor fences.
The UI composes immutable prepared rows and retains their geometry through the
matching presentation acknowledgement. Shutdown cancels preparation, joins the
presenter and releases its last editor frame owners before execution retirement
joins. This source-window integration is applied to the current tree; frozen
candidate checks do not qualify the newly combined tree or its release binary.
The complete editor engine also has an all-target type-check pass against a
frozen private union of the current callers. A corrected native run passed
13 original-ownership editor cases, one hidden undo/yank growth case and one
ordered durable-save readback case; all 1868 captured source hashes remained
unchanged. The original runner rejected a zero-test save filter, and the
corrected module filter executed the real readback test. During a CPU loan, the UI renders immutable acknowledged source rows or
already emitted Markdown cells and one captured set of chrome facts; it does
not read the loaned mutable document. The ordered save collector retains an
original durable completion while the same editor instance is loaned, then
settles that head before younger acknowledgements. The shared ordered writer
readiness and selected native loan/save tests pass. Combined editor/scene
shutdown forcing, the later complete caller union and whole-workspace strict
checks remain separate acceptance gates. These private qualification states
do not imply that the complete engine is applied, released or live.

Chapter line-offset discovery now counts disjoint source ranges rather than
repeated prefixes. Isolated original-versus-candidate chapter fixtures pass;
full-buffer context capture/parsing, TextArea storage and live acceptance remain
unfinished.

The document leaf migration is applied to the current tree: cloned syntax
tokens and rendered text retain their original physical storage charge until
the CPU retirement owner releases the last allocation. Its client build,
Markdown contracts and editor contracts passed, but the new forcing fixtures
initially declared job costs smaller than the captured types and were rejected
before exercising retirement. Corrected fixture declarations passed all six
preparation tests, including independent last-reader release and queued
cancellation, without weakening admission. Current combined compilation and
release acceptance remain unverified; TextArea and context ownership migration
is still incomplete.
The native input owner and complete terminal adapters are now integrated into
the current tree after producer/caller inventory and guarded three-way merges.
Input admission precedes terminal mutation. One admitted owner handles normal
input and terminal capability queries, retains refused semantic events, and
keeps its session claim through terminal restoration. Connection setup reuses
the shared codec bank instead of starting a duplicate bank per connection.
Shutdown errors retain undelivered events and live retirement tickets alongside
earlier client errors. The declared process limit includes these owners; it is
not an allocator or measured RSS guarantee. Frozen candidate checks cover the
selected contracts. Four real Linux PTY runs against the qualified debug native
input child now pass cancellation, release/retry, shared input/query ownership,
and an exact 40 MiB paste followed by a key. They verify physical join and zero
remaining admission; the large paste also verifies compacted capacity. The query
controller provides a synthetic primary device-attributes response only. The
original timeout and controller defects are retained alongside corrected proofs.
These runs do not qualify the Rust parent harness, whole TUI, release behavior,
RSS or matched performance. Ordinary pane-key forwarding still needs retained
retry custody when outbound request admission refuses; native reader preservation
does not prove that downstream contract. Current combined acceptance remains open.
The parent input dispatcher now uses a fixed two-slot backlog, consuming retained
originals before fresh input and draining both slots during shutdown. Overflow
returns both incoming originals without changing existing custody. Seven native
checks cover actual input allocations, the production 64-event motion boundary
and the current dispatcher compiled against retained private App APIs. Both
captured current-client compiler modes now pass all-target type checks with and
without GPU support, plus input-module formatting; all 1850 captured source hashes
and three log hashes were independently verified. V8 download was skipped for
these compiler gates, so they provide no native or release execution proof.
These focused proofs do not qualify downstream semantic admission, the complete
current App or real TUI input latency. A separate private fixed instruction head distinguishes actual
editor installation from model acknowledgement, fences original allocation and
editor instance, and retains partial UTF8/CRLF offsets and terminal failures.
Five native-allocation boundary cases pass; their model facts are synthetic.
Two additional cases use actual shared CPU model callbacks: a partial Unicode
paste cannot release its original until the remaining bytes complete, and a
panicked model retains the same original for shutdown diagnostics. The first
also verifies a following key and actual committed text; both physically join
their test-owned worker banks. The 1882 captured source hashes and native logs
were independently verified. This head remains private pending complete App
caller, global/protected input precedence, completion and shutdown integration.
A private Save As adapter retains a CPU-prepared destination through admission
refusals and reuses the original editor source capture and ordered writer.
Two native cases verify the original CPU snapshot, exact destination allocation,
Unicode disk readback, model-return-before-write ordering and physical bank drain;
all 1882 frozen source hashes and the test log were independently verified.
These cases do not qualify the current App, dialog acknowledgement or full shutdown.
A later current-source capture with the private prompt callers failed its default
all-target compiler gate on missing producer APIs and caller contracts; its 1895
source hashes and diagnostic log were stable. The GPU gate did not run. Shared
editor input, final configuration-source release and whole-App shutdown integration
remain private and incomplete; the earlier compiler passes do not prove this union.
Private shutdown tests now exercise the actual CPU bank in debug and release:
original disposal, nested retirement, deadline ownership retention and retry,
and recovery with a target behind 63 originals of other types. They verify the
original allocation, CPU disposal, physical worker exit and released quotas.
The 48 frozen source hashes, locked dependency versions and four logs were
independently audited. The bounded recovery regression is also included in
`ilium-execution` tests. Its payloads are synthetic; these focused passes do not
qualify complete App shutdown, the current integrated client, or release latency.

GPU ownership remains an explicit gap in the worker inventory. The optional
startup probe discards its native join handle, and all three existing GPU scene
backends share a frame owner that starts a raw thread, joins synchronously on
Drop and clones the entire pixel vector when reading its latest result. Pixel
allocation also bypasses the shared byte admission. The GPU parity test is a
second startup-probe consumer. The existing SceneEnv resources, admitted ambient
worker and platform supervisor provide reusable boundaries. An existing animation
owner has prepared private changes for admitted probe/frame ownership,
nonblocking retirement and immutable charged frames. The pinned Rust 1.96.1
private qualification passed 13 ambient, 21 default GPU, 24 feature GPU and one
parity test, plus strict all-target Clippy in both modes and scoped formatting.
The parity run used an NVIDIA GeForce RTX 3090: 34 cases had zero differences,
and the ten-second temporal control changed 12.08% of pixels. These changes
are not yet integrated into the client lifecycle. Default NotCompiled behavior,
software fallback and matching-
size frame presentation must survive. Driver/device library roles, native buffer
lifetime and hardware acceptance require separate evidence; shared admission is
not proof of driver, GPU-memory or RSS bounds. Probe admission must be retryable
through a bounded pending owner and completion notifications. Scenes receive one
stable eventual runner source before construction, render software immediately,
and acquire the later admitted device without rebuilding simulation state. The
private source qualification does not prove the current workspace, affected
release or live client lifecycle. The client must admit the probe on its existing
execution bank before constructing App/SceneEnv and physically retire GPU owners
after animation drain, before shutting that bank down. Driver threads, native
buffer memory, GPU memory and process RSS remain outside the cooperative ledger.

Subsequent final-reader forcing tests preserved original pixel bytes and frame
credits through CPU disposal. That private hardware run passed parity assertions
but crashed during process teardown, so assertion success was insufficient.
The next private candidate tracks every original frame-worker ticket in a fixed
32-slot controller custody before spawn, fences close against reserved starts,
and drains actual scene and frame joins before releasing the probe/device. Three
RTX 3090 parity processes exited zero with explicit teardown, and 17 managed
GPU tests passed; default strict Clippy then rejected one indexed loop. The
corrected frozen source now passes 17 ambient ownership tests, 21 GPU tests,
real hardware parity, default and GPU-feature strict all-target Clippy, and
formatting. All six commands exited zero and its 490-file source audit found
no drift. This qualifies the private GPU engine; the combined client lifecycle,
release, terminal and matched performance checks remain outstanding.
The crash did not reproduce under the original-binary debugger run, leaving the
exact crash cause unresolved. Client startup, physical scene join and finite-I/O
frame-drain compositions remain private and require combined qualification.

Server notification audio uses one ordered actor with a bounded queue of 64
requests. Playback is a typed finite I/O job on the existing server bank; the
actor retains the original request while waiting for admission. A running native
callback retains its job charge after the actor is cancelled, until playback
physically returns. The original cancellation regression failed before this
change. A frozen callback-source capture passed 458 server tests, all-target
checks, strict Clippy, formatting and a server release build. Its release is
retained separately from the subsequent settings/config integration.

Queued request bytes and immutable sound-settings publication now retain their
own storage admission. Producers reserve before constructing new labels;
startup, IPC updates and config reload publish one immutable admitted settings
allocation, shared by queued and native readers until the last reader releases
it. The config watcher uses the existing finite I/O bank and one bounded
256 KiB read for both fingerprint and parsing. It preserves first-tick refresh,
last-good settings, refusal retry and independent text-trigger refresh. The
13 integrated files match the exact private capture that passed 467 server
tests, including nine required ownership/refusal/watcher regressions, all-target
checks, strict Clippy and formatting. The combined settings server release has built successfully from that frozen
capture, with actual supervised Cargo and runner exit zero. It is retained as
a separate artifact; current-source union and live installation remain unqualified.
Full notification queues retain explicit logged overflow;
DSP preparation preserves existing bounded synthesis and playback order. Library
and child-process memory remains separate from the cooperative allocation ledger.

The captured CLI/server release pair has passed an isolated real-PTY attach,
project/tree render, physical Help input, resize and Help-dismissal check, with
loaded executable identity and scoped shutdown verified. This does not qualify
the complete current-source union, the full TUI suite, GPU hardware, RSS or
matched performance, and the pair has not been installed.

Exceptional scene shutdown must retain the complete original animation owner,
including accepted configuration and receipt queues and unsettled replay proofs.
A native join ticket or an open execution bank alone does not retain those
originals after App destruction. The private client proposal reserves one
exceptional retirement envelope before scene startup, moves the whole surface
without copying its payloads, and bounds receipt-drain observation. A failed
editor and scene shutdown must preserve both original errors under the same
outer execution owner; GPU retirement and bank closure follow proven physical
scene exit. The new exceptional-path fixtures and combined proposal have not
yet passed compiler or native execution qualification.

External file, directory and URL launches now use the existing shared I/O bank.
One client owner retains at most eight ordered requests, with explicit target
byte limits and launch acknowledgements. A newer click never coalesces an
earlier launch. Refusal, OS failure and an unknown worker outcome produce
distinct feedback; shutdown drains accepted requests before closing admission.
Terminal-link confirmation also uses the existing acknowledged clipboard owner
for Copy. The UI moves each prepared launch message with its original charge.

The platform adapter owns one reaper with an explicit 2 MiB stack and 32 fixed
slots covering both in-flight spawns and running children. Argument arrays,
non-UTF-8 paths, dash-prefixed filenames, null stdio and Windows shell error
mapping remain at that boundary. Closing never waits for or kills a user
application; surviving Unix children retain the admitted service through actual
join, and surviving handles retain its bookkeeping charge. The process
declaration now caps explicit owners at 38 roles under the unchanged 4096 MiB allowance;
this does not bound native allocator RSS or impose a host-wide limit. The five
caller/adapter files are applied and formatted. Their new ordering, overload,
shutdown and real-child fixtures are authored; captured qualification is
running, so compilation, execution and release acceptance remain unverified.

Board file completion retains its original charged result when outbound request
admission refuses an open. The collector retries that ordered head without
regenerating the file, copying its request prematurely or recording pending
focus. The captured parent must still accept normal children; a changed creation
dialog cancels only the automatic open and preserves the completed file.
Filesystem shutdown closes and drains editor, board, configuration and
integration owners independently, including when no editor service exists.
Their completion notifications are registered before collection, under the
existing shutdown deadline. Accepted board writes finish; automatic opens are
explicitly cancelled during shutdown and their disk data remains available.

Finite naming, model discovery, restructuring and Smart Copy also share two
native-account provider slots across cooperating clients. The platform adapter
resolves the native account profile independently of HOME, XDG and project paths,
then probes two permanent owner-only lock files without waiting for a held lock.
Namespace discovery and probing execute within an admitted I/O job. The existing
process limiter and the host lock remain owned until the synchronous provider
body returns or unwinds; output collection and catalog CPU preparation do not
retain either permit. Only a preflight refusal returns the original captured
request for a dated retry. An invoked stream is never replayed, including an
empty response, a partial prefix or a provider failure. Persistent voice uses its
separate actor lifecycle. This bounds participating binaries using one stable
account filesystem; it does not constrain older binaries, remote provider work
or deliberate namespace replacement. An isolated two-child test verifies the
real shared lock limit and release on child exit despite different HOME, XDG and
project paths; it does not write the actual account namespace. Caller-ordering
and integrated release qualification remain open.

Normal voice and the onboarding demonstration prepare prompts, tool schemas,
configuration and context changes on the existing finite CPU bank. A separate
finite I/O phase reads an environment credential only when the captured setting
requires it; its completed original follows the same request into CPU preparation.
The actor accepts an owned startup proof and a separately preadmitted metadata
token rather than constructing configuration on the interaction loop. Temporary
metadata refusal retains the same prepared startup for a dated retry; the token
pins the actor's quota, so startup cannot substitute another ownership group. Normal context preparation reserves its exact FIFO slot,
so younger text, push-to-talk and tool commands cannot overtake it. Replacement
settings fence prepared results, while Stop cancels preparation and waits for
actual actor and native retirement before another owner can start.

The retained-source configuration transition is not integrated yet. Canonical
onboarding Test capture still clones the selected voice settings. The private
source-based callers retain the chosen original before admission, with runtime
identity inherited only after the CPU compares the runtime-relevant settings;
policy-only changes preserve a waiting Test. The original caller's policy test
passes natively, while the new source caller and acknowledged configuration
union have only parsing and source-hash evidence. Modal construction has three
passing native library admission/refusal tests. The private CPU editor and save
controller also have fourteen passing native tests covering original text and
hidden history, input ordering, refusal, panic, partial-paste cancellation,
surface and resize fences, and physical worker shutdown. The save-controller
fixtures inject acknowledgement facts; they do not establish actual writer
durability or persistence readback. Dialog caller integration, complete frame
composition, ordered disk acknowledgements and application shutdown remain
unqualified. None of these focused checks establishes a complete client release
or live transition.

Audio callbacks use bounded preallocated sample rings. The existing persistent
DSP owner performs sample conversion in both directions; playback pressure retains
original pending samples there while capture remains serviced. Capture overflow
reports an incomplete utterance. Push-to-talk commit drains through the last
admitted capture ordinal, and shutdown can interrupt a blocked audio-delta enqueue.
These are current source contracts awaiting integrated qualification, not claims
of real-time deadlines or allocator/RSS bounds. The first environment read still
allocates before its accepted-size cap; arbitrary external JSON maps have opaque
spare capacity and require a producer backing declaration. Native backend lifetime
and library thread costs need separate platform qualification.

The client bank constructor and selected-composition fixture now share one
configuration: two CPU, four I/O and one service thread. The selected feature
owners contribute another 21 roles; supervisor/logger/input add three, the
external opener one, and the existing Tokio runtime six. The root ceiling is
derived from those bank constants and the named set: 38 roles under the unchanged
4096 MiB allowance. Earlier fixtures counted only five bank threads, undercounting
the selected feature scenario by two. The selected feature storage declaration is
now 3451 MiB plus 128 KiB before additive bootstrap/runtime storage. A standalone
forcing test starts the real seven-thread bank: the original 36-role ceiling
refuses the remaining 31 declarations, while the corrected ceiling admits them,
refuses a further role, and releases bank admission after all seven native owners
join. Those other roles are synthetic declarations in this test; their engines
and children are not started. Full-client compilation and the updated selected
storage fixture remain unqualified. Retiring owners compete for their original
admission. These declarations do not prove whole-process OS threads, allocator
RSS, native library completeness or measured memory acceptance.

Ambient scene construction receives the composition root's existing execution
client through `AmbientResources` and `SceneEnv`. Finite carpet-chess searches
use that CPU bank, one owned receipt and a captured board identity; cancellation
is checked inside search nodes. Returned moves keep their result admission until
application. This conversion does not complete admission for the other native
scene workers: image decoding, caches, helper processes and opaque library
threads still need producer-specific peak and retirement qualification. Audio
also needs explicit native-stream admission and a shutdown protocol that cannot
wait behind a full UI event queue.

Images discovery transfers the original list and diagnostic allocations through
one shared, independently charged owner; worker, result and scene references
retain that same allocation. Failed list preparation publishes a terminal error
state and returns to idle cadence rather than regenerating an empty cache.
Frozen-source Linux checks cover90 Images tests, all-target Clippy and formatting.
This qualification does not complete PNG/JPEG/GIF/WebP decoding admission,
remote discovery or native library memory accounting.

The presentation owner reserves one physical thread with96 MiB of declared
encoder/resize working storage before spawning, plus97 MiB of independently
retained frame and control storage. Both use the composition root quota. Actual
join releases physical admission; the last retained frame or original failure
receipt releases its separate storage admission. Diff encoding streams against
the last emitted frame without a whole-frame diff vector. Isolated release tests
cover refusal before spawn, blocked retirement and storage after actual join;
full-client output and measured memory qualification remain open.

Snapshot publication flushes and syncs the temporary data file, then uses the
platform durable replacement operation before reporting success. On Unix that
operation also syncs the containing directory; Windows uses write-through
replacement. Existing snapshot persistence readback and oversize-preservation
tests pass on Linux; crash/power-loss and Windows execution remain unverified.

The ordered snapshot disk service now reserves one physical worker from the
existing server execution quota before constructing its channels. Its declaration
includes a requested 2 MiB stack, the existing 64 MiB service allowance and bounded
command/path metadata; these are admission declarations, not measured RSS bounds.
The native supervisor retains that reservation through actual thread join even
after logical service drop. Production startup fails explicitly if the session
execution owner is absent; independent test roots create no additional execution
pool. Captured-source Linux qualification passes all12 snapshot-service tests, including
blocked retirement, exhausted worker admission and retry readback. This establishes
the tested service contracts; release and loaded-runtime qualification remain open.

Presentation acknowledgements retain the exact sources and hit targets actually
painted in their complete frame. Selection, context menus and Smart Copy use
those emitted sources. Detach consumes output acknowledgements while draining
ordered parser work, releasing geometry leases without repinning a finished
interaction; successful animation receipts still reach their exact scene.

Cost derivation and detail-card formatting use one finite CPU receipt on the
existing statistics execution client. Captures contain bounded topology IDs,
settings and immutable statistics identities. Structural revisions fence
installation independently of newer time requests. Installed statistics and
history snapshots carry independent storage leases, releasing finite job slots
after collection. The coordinator owns history scan receipts and drains them
before CPU admission, including when cost display is disabled or CPU work is
still pending. CPU derivation reads an immutable history snapshot. Cache-path
changes immediately invalidate queued or completed scans and fence captured CPU
generations with a separate epoch; ordinary cancellation preserves the previous
complete snapshot. Shutdown releases coordinator references before execution
drain, while independent readers retain the original storage lease. Frozen-source
checks cover10 history tests and26 cost-tracker tests, including queued/ready
cache changes and last-reader retirement. Full engine destruction, current-client
runtime and measured memory qualification remain open. Tree-row acknowledgements
retain the prepared card, its
displayed title and visibility policy; rendering borrows those immutable lines.
Cost CPU admission additionally records the actual rejecting ledger's quota
boundary, requested bytes and used/limit values from the existing admission lock.
The UI diagnostic and retry policy stay unchanged; repeated ticks with the same
diagnostic do not repeat the warning. Refusal leaves the engine in its coordinator,
and a later submission refusal returns the exact engine as before. This diagnostic
change uses the same client, bank and quotas. Scoped formatting passes; its fresh
cost/history/client checks and reproduction of the runtime refusal remain pending.
A changed acknowledged card requests one redraw, while an unchanged source
does not create an acknowledgement/redraw loop. These source contracts still
need integrated compiled and runtime qualification.

Icon search has one supervised stateful owner per client, one replaceable pending
query and one immutable result mailbox. Result storage remains charged through
its last consumer. ONNX uses one execution thread and tokenizers parallelism is
disabled before initialization. Two nonblocking OS lock slots in the shared
model-cache namespace limit concurrent engines across clients sharing that cache;
separate cache namespaces remain independent. The512 MiB native declaration
and cold/warm model memory still require measured qualification.

Media pause/resume effects have one persistent OS-thread owner. Normal voice
and demonstration mode publish bounded desired-state leases; the owner keeps
the exact positively acknowledged paused-player set and compensates a Stop
that arrived while Pause was blocked. Shutdown observes actual supervised
thread exit. The Tokio-backed zbus configuration avoids its extra internal
executor thread; its declared native/message peak still needs runtime proof.

Ambient helper `Worker` owners use that same platform supervisor. Scene drop
signals their existing cancellation flags without joining on the scene thread;
there is no separate ambient reaper queue. The supervisor retains blocked
callbacks until actual exit. Its registry limit covers live and retiring owners,
but shared process admission for those helpers and their native libraries is
still required; this lifecycle change alone does not supply their memory bounds.

Independent immutable caches use `StorageAdmission`: it debits declared storage
without inventing another physical thread. The last retained snapshot keeps
that storage debit even after its producing worker joins. Persistent actors
reserve mailbox/receipt metadata separately from that immutable allocation.
Typed channel adapters can split `Retained` into its original value and an
opaque `Retention` guard. The destination keeps that guard after its payload;
cloning a guard shares one charge and cannot authorize new heap allocations.

Durable Text Trigger reads use the server's bounded I/O bank and accept only
regular files within512 KiB. Regex validation uses its CPU bank. An immutable
settings allocation receives a separate storage lease before the read/validation
job envelope is released; the accepted state holds the lease until replacement.
Releasing the read envelope before CPU admission avoids a deadlock when several
completed reads would otherwise fill all result credits. Invalid or oversized
sources retain the last accepted settings and revision. These changes do not
complete the separately inventoried trigger matching and delivery migration.

Matched Text Trigger deliveries retain their own storage leases in the64-item
FIFO. A full queue waits rather than discarding an already matched decision;
natural PTY EOF drains admitted deliveries. Each rule carries `delay_seconds` (default 60,
max 86400, `#[serde(default)]` so rules stored before the field existed load with
60). The per-pane delivery owner holds a due-time heap (at most 64 pending,
then the channel backpressures); a delivery is sent at detection time plus its
delay. Semantic writes check the original PTY instance after acquiring its input
gate and that the rule (id, enabled, message) is still current; unrelated
settings edits no longer cancel pending sends. Pane replacement and rule
removal, disabling or message edits cancel stale decisions explicitly. The
existing output sampler can still lose transient matches after broadcast gaps;
screen resynchronization does not establish lossless trigger observation.

The server's CPU bank has process-shared decoder and encoder tenants. Four
request envelopes cannot consume the two output envelopes; general evidence
and I/O jobs have separate tenant ceilings. The aggregate declared input and
result ceilings are each 1280 MiB, with general input512/result128 MiB and codec
input128/result192 MiB per admitted job. These bounds are shared across
connections and retain charges while handlers or stream flushes are blocked.
They describe admission envelopes, not measured resident memory.

CPU preparation adapters can query immutable job ceilings across the shared
quota root and every tenant ancestor. Disabled CPU workers or zero job allowances
produce permanent capability refusals. Callers still reserve capacity atomically
to handle current pressure and shutdown races.

The client owns interaction, layout and composition. Its animation service owns
scene construction, simulation, rendering and packing, and returns immutable
frames with native glyphs and styles. Each scene OS owner debits the same client
process thread quota through actual join, including blocked retirement. Packing
reserves a24 MiB frame envelope before allocating cells; the final snapshot
clone releases its independent storage charge even after the engine has joined.
The existing four-engine and three-frame limits also apply. This accounts for
owner threads and published frames; opaque scene caches and library-created
threads require their own domain accounting and do not have an RSS guarantee.
A composed frame retains its exact sealed
scene receipt until terminal emission succeeds. Time requests may replace older
render requests without invalidating the latest complete frame; semantic scene
and settings changes remain ordered. The presentation owner diffs against its
last successfully emitted buffer, encodes it and performs terminal output.
Cursor and layout revisions advance with the output acknowledgement. Mouse
geometry retains the emitted viewport and terminal instance; provisional layout
or pane replacement cannot redirect admitted terminal input.

Document preparation is keyed by instance, source revision, width and settings.
Rendered-source capture uses the same interactive turn budget as source windows
and syntax capture: at most256 KiB and1024 lines in aggregate, with each pane
receiving at most64 KiB per turn. A partial capture retains its original job
admission and typed source retirement. Before the next chunk, the full key is
checked again; a changed editor instance, revision or layout retires the partial
source instead of mixing revisions. Whole-source size checks occur incrementally
rather than scanning the entire buffer on the interaction loop. Cross-line
syntax, Markdown layout, image decode and font preparation run on the finite
banks. The new capture regressions are authored but not yet executed; existing
whole-source limits and the separate TextArea bulk-mutation gap remain open. Ordered file writers acknowledge durable publication before
clearing dirty state, retargeting Save As or dismissing a save dialog. Editor,
board and configuration consumers own conflict and rollback decisions; a failed
acknowledgement never proves that a rename was not published. Directory sync
reports unsupported platforms explicitly. Detection workers gather owned process
and transcript evidence; the server reconciles current revisions and exclusive
session claims centrally.

Server foreground-process evidence now uses the existing finite I/O workers.
Five observation paths capture shell/process evidence, release shared tree and
pane guards, then await a bounded native probe; title and input decisions
revalidate the captured PTY, session, agent and presentation fences before
applying results. The sixth path probes shell ownership within the detection
evidence worker after releasing its cached process-table guard. Refused,
unavailable or late evidence does not grant shell ownership. The native adapter
still uses ToolHelp on Windows and the narrower terminal foreground-process-group
query on Unix. This describes the current source; it does not establish Windows
runtime behavior or replace current-source integration and runtime checks.

Local PNG preparation uses the shared finite CPU bank. Its concrete decoder
preflights bounded chunk metadata, inflater buffers, pixel storage and resize
scratch before construction; cancellation checks also cover encoded-source reads.
The original encoded source and prepared pixel/name storage retain their admission
through queued results and the final scene or cache consumer. These declarations
bound owned work and storage; they do not establish a process-wide RSS limit or
qualify other image formats and remote transport.

Terminal parsing has an ordered OS owner and immutable UI snapshots. Byte,
replay, resize, scroll and negotiated-input barriers retain pane identity and
stream order. Smart Copy captures a matching historical viewport before later
output can change it. IPC framing separates fixed-size header reads, bounded
payload admission and pure encoding/decoding; idle connections do not reserve a
CPU job while waiting for a header. Actual stream flush establishes presentation
or transport delivery, while persistence and server application use their own
acknowledgements. These components are workers and services; the future
user-written JavaScript extension system has a separate contract.

Light Smart Copy reserves a position in the existing ordered clipboard service
before moving its original selection to CPU preparation. Later clipboard reads
and writes cannot pass that position. Prepared text and cached preview facts
retain their storage admission; successful native acknowledgement starts the
preview countdown. Preparation or write failure restores the original selection
for an explicit retry or cancellation. Shutdown drains acknowledgements and
returns any unresolved original through typed custody before the execution bank
closes. These source changes are integrated; their new recovery and ordering
fixtures are awaiting client qualification. Model-assisted selection preparation
and whole-editor bulk mutation remain separate open inventory items.

Process bootstrap now designates the shared quota before logging starts.
The supervisor keeps its permanent physical admission; the ordered logger keeps
its worker admission through actual join and its storage admission through the
last retained owner. The server creates `ServerResources` before logging and
moves that same quota and completion notification into its finite worker bank.
Independent test banks retain their own quota identities. A Linux qualification
forces a valid native thread request to return kernel `EAGAIN` in an isolated
child, verifies registry/custody rollback, restores only that child's soft limit,
and verifies a later real worker joins. This complements the separate actual
quota startup test; it does not prove all native library or process resources.

The interactive client now reuses its five-thread finite bank for codecs and
has an admitted terminal-input owner. Before constructing either existing Tokio
runtime, the process root reserves its two async and four blocking roles with
2 MiB requested stacks. The supervisor retains this fixed runtime declaration
for process lifetime: a timed runtime shutdown can leave blocked callbacks alive,
so dropping a local guard would release capacity prematurely. Same-root repeated
initialization is idempotent; a foreign root or changed declaration is refused.
The selected-client census includes one external-opener reaper and derives a
38-role ceiling from the current seven-thread bank plus its named owners, within
the unchanged 4096 MiB storage allowance. Current full-client qualification is
still required; standalone bank admission is not whole-process acceptance. Foundation qualification
covers runtime admission/refusal, idempotence and retained process custody;
captured-source server and CLI all-target compilation also passes. The later
client changes require their own qualification; no installed-runtime claim follows.

The shared local ordered writer can retain an accepted FIFO head while its
original source is prepared on the existing CPU bank. Its typed readiness
predicate runs before I/O submission; a younger ready write cannot overtake the
head, and waiting for preparation occupies no I/O worker. Existing editor, board,
configuration, integration and permission writers retain their default ready
behavior and original completion wakes. Eight isolated native writer/snapshot
regressions pass, including independent I/O during blocked CPU preparation,
ordered durable readback, shutdown and admission provenance. The full editor
model-loan, deferred save acknowledgement and source-capture caller integration
remains under current-source client verification; this boundary proof does not
establish workspace, release or installed-runtime acceptance.

The snapshot service reserves its persistent OS worker and declared bounded
mailbox/write storage on the server's existing root before spawn. Its physical
admission lasts through actual join. Failed coalesced writes restore the dirty
obligation and reuse the existing750 ms debounce/notification instead of waiting
for a new user mutation. The ordered writer retains earlier failure history even
when a later retry succeeds. Captured-source qualification passes the shared-root and retry regressions,
including authoritative persistence readback. The write envelope alone does not
qualify native JSON or legacy YAML read/migration peaks; result custody and parser
admission have separate owners and qualification receipts. Startup normalization now detects a legacy
project wrapper from the root project predicate instead of cloning the entire
tree for equality comparison. Both launch-project and agent-resume normalization
still run. Its frozen-source qualification passes 30 persistence tests (two ignored),
server all-target compilation, strict Clippy and scoped formatting. These receipts
predate the later result-retirement changes.

Snapshot reads now return typed owned results with a separate storage lease from
the existing server quota. The operation reservation can be released at delivery
while pending recovery retains the decoded result. Restore transfers the same
lease to server state before its first await; queued snapshot capture handles
also retain it when they outlive that state. No second worker or quota is created,
and a pending recovery decision does not consume the write-operation budget.
Two new fixtures cover storage after actual worker join and storage refusal with
original-file readback. Frozen-source qualification passes 14 snapshot-service tests,
30 persistence tests (two ignored), server all-target compilation, strict Clippy
and scoped formatting. Source and log hashes were independently verified. Decoder
preallocation remains open, so this change does not establish a complete
read-memory bound. Production read results now also reserve a destruction
envelope on the existing CPU bank before decoding. Discard and cancelled
acknowledgements hand the original result and its same storage lease to that
bank; no new worker is created. Shutdown releases an unchosen pending recovery
result before closing the bank. Two additional fixtures park both CPU owners
and force retirement-slot refusal to check retained custody and unchanged disk
bytes. Their frozen-source qualification passes all 16 snapshot-service tests,
30 persistence tests (two ignored), server all-target compilation, strict Clippy
and scoped formatting. Source/log hashes match the capture; these receipts predate
the newer JSON admission and overload changes. Partial-restore cancellation and
exceptional shutdown custody remain unqualified.

The read owner additionally admits the actual restoration future frame before
decoding, using its compiler-derived size and the existing retirement metadata
allowance. Restore polls that owned frame on the coordination runtime; cancellation
hands its remaining input, state reference and storage lease to the existing CPU
destruction owner. No async task is claimed as CPU offloading. An unexpected
missing owner retains the original recovery result and refuses successful
resolution. Two fixtures cover cancellation before tree publication
with both CPU workers parked, and frame-admission refusal with exact disk readback.
Frozen-source qualification passes all 21 snapshot-service tests, including these
fixtures. An independent audit verifies 1,792 source files and eight logs. This
does not establish semantic recovery after a partially published restore, old-tree
destruction bounds or exceptional shutdown safety.

A single recovery owner now retains the pending original and one accepted
restore/discard coordination task independently of the requesting socket.
Admission reserves the compiler-derived task frame before transferring that
original. A refused restore or failed ordered discard returns the same original
to the retry slot; direct reply backpressure does not own semantic completion.
Attach waits for resolution, workspace pruning protects an unresolved operation,
and session kill or shutdown closes admission and joins it before clearing the
tree or draining final persistence. Native snapshot I/O and CPU destruction keep
their existing owners. Four real-bank regressions, server all-target strict
Clippy and formatting passed against 1820 unchanged captured files; the five
recovery files are applied. The server release build also passed against the
same 1820-file capture, with independently verified source, log and retained
binary hashes. The artifact is retained separately and is not installed; current
paired runtime acceptance remains outstanding. This release predates the
failure-handling proposal below.

Recovery failure and snapshot shutdown changes are now applied through exact
current-source deltas across seven server files. Accepted recovery failures latch
an error and keep the original-file write fence; KillSession returns an error
without clearing the tree or acknowledging success. A lexical dirty-claim guard
restores pending work on cancellation, unwind or failed write acknowledgement.
Shutdown cooperatively stops and physically joins the original coordinator
before its final conditional flush. Join failure remains latched across cancelled
and repeated shutdown attempts; clean discard and clean startup do not force a
save. Original native regressions reproduced both lost cancellation claims and
missed failed-write shutdown retries before the remedy. The frozen candidate
passes 11 recovery, 25 snapshot and one task-handle cancellation test, strict
server Clippy, scoped formatting and a private release build; all 1820 source
hashes and six log hashes were independently verified. Its new cancellation
case checks actual persisted readback and sticky failure after repeated drain.
The seven applied files match those qualified candidate bytes and pass formatting.
A fresh current-source gate additionally passes all 38 native cases, including
the actual KillSession failure handler, strict server Clippy and scoped formatting;
all 1850 captured source hashes and six log hashes were independently verified.
That gate built no release. The earlier private release is not installed, and
current paired runtime, whole-workspace and performance acceptance remain
outstanding.

The session shutdown sequence now drains accepted workspace semantic tasks while
keeping their existing execution bank open, then flushes and shuts down the ordered
snapshot service, and only then requests bank cancellation. The same production
drain helper is used by an authored regression that parks both actual CPU workers,
queues an accepted semantic mutation, and checks authoritative snapshot readback
after shutdown. Its first qualification failed before execution because the
fixture called a nonexistent receipt method. The test now uses the foundation's
existing `try_take` API and passes scoped formatting; fresh compilation and
execution remain outstanding. Recovery still runs inside the request
handler: ordinary socket closure does not immediately cancel that awaited handler,
but the server aborts connection tasks during normal shutdown. The ordering change
does not preserve a partially published restore across that abort or establish
old-tree destruction bounds. The production startup branches load at most one
snapshot per server lifetime; repeated replacement of the read-storage lease is
a future contract concern, not an observed production reload path.

Native JSON loading now uses the shared allocation-checked Serde visitors before
owned strings and collection growth, while preserving trailing-input rejection.
The caller admits parser scratch separately before queueing the read on the same
server root. Its conservative declaration covers the installed JSON parser's
scratch buffer and old/new Vec capacity coexistence; that lease releases after
the decoder actually returns, while the decoded result keeps its own lease.
Resource-admission errors have a typed snapshot error. Recovery startup returns
that error instead of starting a fresh writer that could overwrite the original.
Operation-capacity and queue-overflow refusals use the same typed error. Snapshot
worker construction failures also use it because construction admits/spawns a
worker before any disk operation. Three later fixtures force operation,
parser and worker-creation pressure, verify original-file readback and retry.
Their frozen-source qualification passes 19 snapshot-service tests, 32 persistence
tests (two ignored), 47 IPC tests (two ignored), the startup-refusal test, server
all-target compilation, server/IPC strict Clippy and scoped formatting. Independent
hash audit verifies 1,792 source files and eight logs. These receipts predate the
restoration-frame changes and legacy encoded-size refusal parity.
Four new fixtures cover the shared decoder, native refusal and readback, trailing
JSON, and actual refused server startup. Their frozen-source qualification passes
47 IPC tests (two ignored), 16 snapshot tests, 32 persistence tests (two ignored),
and the startup-refusal test, plus server all-target compilation, server/IPC strict
Clippy and scoped formatting. All 1,792 captured source hashes and eight log hashes
match. These receipts predate the newer overload and cancellation-frame changes.
Custom deserializer conversions remain an audit obligation; the shared visitors
are not a generic allocator or RSS guarantee.
Norway's eager YAML event/alias graph, migration coexistence, normalization
scratch and partial-restore/shutdown custody still need separate bounds.
Legacy YAML decoded values now use the native snapshot's allocation-checked
Serde policy, so compact repeated aliases cannot bypass collection/string/depth
admission. Refusal returns a typed recovery resource error before conversion or
native publication; malformed YAML keeps its original codec error. Two standalone
exact-LegacyWorkspace tests reproduce the original unchecked expansion, then
verify allocation refusal and ordinary decoding with the real IPC/Norway crates.
The production disk readback, no-publication and valid-alias retry fixture is
authored but unrun. This change does not bound Norway's eager event graph, custom
Serde conversions or migration/normalization coexistence. Integrated qualification
still needs current persistence source.

Legacy YAML encoded-size refusals now use the same typed resource error as native
snapshots, including an early file-metadata check and the bounded reader's
post-read growth check. Startup therefore refuses recovery rather than treating
an oversized legacy source as an empty session. A new fixture
checks exact original-file bytes, absence of native publication, and later valid
migration with native readback. Frozen-source qualification passes that fixture,
all 33 persistence tests (two ignored), 47 IPC tests (two ignored), the startup
refusal test, server all-target compilation, IPC strict Clippy and scoped formatting.
The owned refusal transfer now uses `ControlFlow` to retain the original snapshot
without another allocation. The subsequent captured-source qualification passes
22 snapshot tests, 33 persistence tests (two ignored), 47 IPC tests (two ignored),
the startup refusal test and the full server library (435 passed, nine ignored),
plus server all-target compilation, server/IPC strict Clippy and scoped formatting.
The independent audit verifies all 1,792 captured source hashes and nine log hashes;
actual controller exit is zero. This includes the semantic shutdown regression:
accepted work drains before the final durable snapshot and execution-bank stop.
Concurrent startup-dialog and hidden-terminal delivery changes arrived after that
capture, so these receipts do not qualify the current workspace union. YAML parser
peak memory and semantic restoration after partial publication remain unresolved.

Location search now uses one shared one-job tenant per `ClientExecution` on the
existing finite I/O bank, replacing its separate four-thread gate. Picker replacement
cancels delivery while a blocked callback retains its physical job debit. Typed
outcomes and input revisions fence late results; result and selected-candidate
allocations keep a separate process-root storage admission after job retirement.
Provider error strings follow that same storage contract through display.
Query capture is bounded at 64 KiB without truncating the authored input; configured
address-provider validation and cooldowns stay in their existing adapters.
The new real-bank forcing tests and private prompt reconciliation are unqualified.
The shared `ilium-http` adapter now keeps DNS on the already admitted caller at
all nine existing construction sites (seven formerly using the default resolver,
two map callers using the previous local owned resolver). Locked ureq 3.3.0 spawns
an untracked DNS helper for finite resolver timeouts; the shared resolver selects
its synchronous lookup path and reports an expired deadline after lookup returns.
Native lookup cannot be forcibly cancelled, so a blocked worker retains custody
and admission. Caller proxy, redirects, cooldowns and request settings remain owned
by their existing adapters. Captured qualification passes both resolver forcing
tests, checks and strict Clippy for the HTTP, ambient, Wikipedia, inference and
gateway crates. Its import-format gate failed and was corrected afterward; newer
client integration remains unqualified. These checks establish no measured
process-thread or latency bound.
Location confirmation now carries an allocation-free marker into App. Destination
storage is admitted before normalization and copying into authored settings, save
intents and picker receipts. Version leases follow acknowledged, failed and rollback
settings after picker disposal. Semantic cache misses reserve separate storage
before cloning authored and derived settings; each independently retained view
keeps that resolution's charge, avoiding unlimited escaped views under one fixed
version declaration. Admission refusal leaves the original picker and prior cache
available for retry. Declared copy/scratch allowances are cooperative accounting,
not measured RSS bounds. Five quota, writer-refusal, persistence and retained-view forcing tests pass in
the captured client Cargo run, together with test compilation and package formatting.
The tested Root-owned seams match current source at audit; two concurrently changed
statistics files keep whole-current client integration and release qualification open.
Picker-list admission alone does not qualify downstream consumers.

Startup-dialog observations now use the existing filesystem I/O client rather
than reading a file on each UI-loop pass. One active receipt and one immutable
charged observation retain their original admission; reads are paced at 100 ms
and fenced by the startup-file path. A missing, malformed or oversized record
uses the existing indeterminate display. The reader accepts at most 64 KiB,
without modifying the server's file, and closes on initial sync or terminal
shutdown. A blocked native read keeps its bank debit until it returns. No
additional thread, bank or provider quota is introduced. The oversized-file
regression fails against the original reader and passes with the bounded reader;
three exact-source record tests pass. The subsequent captured client run passes both production startup-reader tests,
including real-bank readback and close, plus three execution composition fixtures,
all five location-save fixtures, compilation and package formatting. The runner exit
and all nine log hashes are verified. Two concurrent statistics-file changes mean
this is captured-source qualification; strict lint, current-union release, runtime
and matched performance acceptance remain open.
The server's best-effort startup publisher now has one State-owned native file
owner on the existing I/O bank. Captured text is bounded and admitted before
copying; the original path remains charged through its last native callback.
Only native callbacks acquire the file mutex. Pane-name capture releases the tree
lock before awaiting publication. Each restore carries a generation token, and
ordered sequence fences prevent delayed writes after finish or an old finish
from deleting a newer phase. Shutdown submits final removal before cancelling
the bank. Refusing a courtesy update leaves the semantic restore input intact.
Four standalone tests using the real bank and native files verify off-caller
execution/readback, stale-write fencing, cancellation while blocked and finish
ordering when a younger update returns first. Finish always closes its own phase;
sequence coalescing applies only to publications. The ordering regression fails
against the earlier publisher and passes after this correction. These
use a minimal State fixture. The subsequent captured full server run passes
443 library tests with nine ignored, including all four production publisher tests,
three ready-log tests, bootstrap tests and legacy allocation-refusal/readback.
All-target compilation, strict Clippy, package formatting and the server release
build pass with actual runner exit zero. An independent audit matches all five
log hashes and 588 files in the server's transitive local dependency closure,
with no captured or current-source drift at audit time. The release binary is
built but not installed or live. Complete client integration, shutdown runtime
acceptance and matched performance measurements remain outstanding.

Rolling session backups now reserve typed job and scratch admission on the existing
server I/O bank before copying paths or session names. Started native work and its
original errors retain admission through actual return or final error disposal.
Copy-before-restore, synced temporary publication, half-hour retry buckets and the
existing newest-first retention tiers are preserved. Retention scans stop at 16,384
entries or 4,096 candidates and check filename/path capacity; sorting and deletion
plans borrow the retained candidate paths. A complete plan precedes any deletion,
so an incomplete scan preserves history. A published backup remains successful
when pruning refuses. Checked borrowed path lengths determine the cooperative
memory declaration; this is not an allocator or RSS guarantee. Six forcing tests
are authored for readback, admission refusal, blocked cancellation, original error
retention and incomplete pruning. The first capture passed its tests but stopped
at strict Clippy on a manual scan counter and unnecessary borrowed-result lifetimes.
After correcting both, the fresh capture passes 12 backup tests, all 451 server
library tests, all-target compilation, strict Clippy, package formatting and the
server release build. Actual runner exit and all six log hashes are verified;
backup and IPC source hashes still match that capture. The binary is built but
not installed or live. Later HTTP changes are outside this qualification.

IPC acceptance now waits before accepting a new stream when the existing tracked
connection registry contains 64 owners. Completion-aware handle polling reaps
actual completed owners and observes their original failure result; one saturation
and one resumed-capacity diagnostic are emitted per wait episode. Shutdown can
cancel the capacity wait through the existing outer select. Transport backpressure
preserves queued stream bytes without creating another worker or quota pool.
Two authored real-endpoint tests exercise four release/reaccept cycles, queued
request ordering, shutdown and terminal-subscription cleanup after abort. Scoped
formatting passes. Both real-endpoint tests and all-target compilation pass.
The separate original capture stopped on the older backup style findings; the
corrected combined server capture subsequently passes strict Clippy, package
formatting and release with this same IPC source. Idle/stalled clients can still
delay new management connections, and
this owner count does not establish a complete byte or RSS bound. Existing registry
insertion may prune a below-saturation completion before its error is observed.

Loopback HTTP project resolution now reserves a job on the existing server I/O
bank before copying the requested project and directory inputs. Admission refusal
returns a retriable service-unavailable response before agent/workspace creation.
Path inputs and constructed paths are limited to 8,192 bytes, scans to 16,384
entries and 512 pending directories at depth two. At most two distinct canonical
candidates are retained; two establish ambiguity, while an incomplete capacity-
limited scan refuses rather than returning a unique prefix. Mid-enumeration
errors also refuse instead of silently yielding a partial candidate set. Existing hidden-path,
unavailable-directory, stale-session-directory and canonical deduplication behavior
is retained. Checked copied-path/FIFO/native-scratch declarations are cooperative
limits, not allocator or RSS guarantees. Success/error JSON buffers inherit the
original job retention through `Bytes::from_owner`, including byte-slice clones;
server-owned workspace creation also retains lookup admission if its HTTP caller
is cancelled. Native callbacks retain physical admission through actual return.
Six authored forcing tests cover bank-thread identity, path readback, last-byte
ownership, original error responses, pre-callback refusal/retry, cancellation and
incomplete/ambiguous scans. The initial test compile stopped because its
cancellation assertion required Debug on the successful retained value. A direct
error match fixes that fixture without expanding production contracts. Scoped
formatting passes; fresh frozen tests, all-target compilation, strict Clippy,
package format and release are running. Independent
advisor delivery was unavailable because required browser Pro was locked and both
prescribed native launch adapters hit the runtime agent-thread limit; exact frozen
sources and failures are retained. Original HTTP request parsing and unrelated
semantic/transport concurrency are separate admission boundaries, not proved
bounded by this lookup job. No HTTP runtime activation or performance benefit is
claimed.

Ready-log metadata publication now moves the original PID/path marker into the
existing server I/O bank, started after successful listener binding. The secure
temporary-file write and atomic rename retain their original fatal error contract.
Cancellation discards delivery but cannot undo a native callback that has started;
its physical job admission remains held until return. Error-path storage remains
charged through the caller's error value. An isolated thread-identity regression
fails against the original direct helper. The subsequent full server qualification
passes the real-bank and bootstrap tests, all-target compilation, strict Clippy,
package formatting and release build with Cargo, rustc, rustdoc and Clippy pinned
together. Earlier mixed-compiler, missing-error-Display, platform cast and orphaned
doc-comment failures remain retained as failed evidence. The release build is not
installed or activated; client, live terminal and performance acceptance remain open.



The platform registry caps running and retiring owners at1024, excluding its
supervisor. Neither that cap nor cooperative declarations establish a whole-process
OS-thread/RSS bound. Complete caller qualification, library thread census and
matched runtime measurements remain separate acceptance gates.

Integration acceptance remains separate from source implementation. In
particular, retained terminal-history pins need publication headroom and an
overload recovery contract; all message queues need byte ownership through their
consumers; and existing provider/library workers need the shared admission wired
through actual retirement. Focused foundation tests do not establish whole
workspace, release, live PTY or performance acceptance.

Matched frame-age measurements require equal clock boundaries. The retained
baseline draws synchronously; a measurement-only overlay now records field
request, field preparation completion and successful terminal flush without
modifying the original baseline. Its source hashes and scoped formatting are
verified; compilation, instrumentation overhead and runtime samples remain
outstanding. The candidate's terminal `prepared_at` is captured after buffer
validation, whereas the baseline overlay's terminal age starts before UI work.
Those terminal `frame_age_us` values must therefore be reported separately.
Animation field-ready age includes cached retrieval, and must not be described
as the age of the original cache generation. Matched workloads, logging settings,
warm-up and whole-process CPU/memory still require real release runs after builds
stop; source timestamps alone prove no performance improvement.

## Ordered terminal ownership

Terminal viewports reserve configured toolbar, prompt and progress space before
input. Metadata content and expiry change visibility within those slots; outer
size, explicit settings and split changes remain geometry inputs.

The PTY owner orders bounded commands and output. It acquires parser mutation
ownership before changing native geometry, verifies the result and preserves the
parser on failure. Query replies complete before a following resize; output may
advance during an ordinary pending input, but replies cannot split that payload.
`PtyInput` receipts distinguish admission from delivery and exact prefixes from
unconfirmed writes. Async callers clone that lifetime-bound handle under brief
registry access and await outside shared tree/pane locks. Final mutations verify
the same runtime again.

Screen presentation uses a nonblocking read and a consistent retained frame
captured immediately before native resize. Ordinary output does not clone a
full fallback screen. Automated readiness and input use current-only reads;
parser contention cannot authorize input from retained text. Paste-mode and
Text Trigger reads await screen/status changes outside registry locks with a
bounded wait and a session identity fence. A missed trigger chunk marks its
shadow for authoritative full-screen resynchronization before further scanning.

A terminal failure closes admission, settles active receipts and retains native
control/writer custody until session close. The existing server output forwarder
also observes owner status so failures surface without another keypress, while
already-published bytes remain available. Every worker has a bounded supervisor
slot and a retained join ticket; timeout reports pending ownership, never a false
join. An uninterruptible native resize remains an explicit hard-deadline limit.

Queued prompts persist an attempt fence before body or Enter. An uncertain
attempt retains authored text and blocks automatic replay; successful recurring
entries rotate with a fresh unattempted state. Auto-answer permits at most one
retry, only for proven zero delivery with the same process, dialog and settings.

## Agent worktrees

An agent pane may own a linked Git worktree. `ilium-core` persists the pane's
`launch_cwd` and `PaneWorkspace` provenance; `Tree::pane_cwd` is the sole
directory source for launch, transcript lookup, and restore. For a project
launched in a repository subdirectory, the same subdirectory is selected
inside the new worktree. A missing worktree remains visible in the tree and
does not resume its agent from the wrong checkout.

`ilium-server` serializes creation within each repository, validates branch
and path again after the client preview, and keeps Git mutation separate from
tree mutation. The server removes only worktrees with verified Ilium ownership
metadata, after exact Git registration and process-tree checks.
Removal also checks ignored and untracked content; ordinary Git worktree
removal alone would discard some ignored files. A retained Ilium worktree
keeps its ownership marker after pane close and regains that identity when
reopened. Foreign existing worktrees remain unowned.
New ownership markers record a custody format revision. Before spawning a
terminal in an owned worktree, the server durably publishes a per-spawn ticket
in Git's metadata for that worktree. Safe removal requires zero tickets after
supervised process termination and a directory-user scan. A failed proof,
crash, unreadable process state, or legacy marker leaves the worktree in place
for manual inspection. The server never silently adopts a foreign worktree.
Prune inventory and mutation both recheck the exact Git registration, marker,
pane references, directory users, and file/branch state. The interactive
close offer is advisory; the same server gates run again for removal.
The configured close policy is stored in the JSON session snapshot per pane,
bound to its worktree identity, rather than in the tree's positional IPC
payload. A missing policy defaults to keeping the worktree.
If marker creation or a post-create command fails after Git has created the
checkout, the server retains it and reports its path. Hooks, filters, or the
setup command may have written files that cannot safely be rolled back.
Branch switching in the current checkout is deferred: that checkout may also
be used by other panes and the user's shell. A dedicated branch for an agent
therefore uses a linked worktree instead of changing the shared checkout.

One session-owned Git status coordinator checks each workspace's HEAD files
every ten seconds, with at most two probes in flight. Full porcelain status
is requested on branch-line hover and agent completion. Status events are
runtime-only, change-only, and replayed to attaching clients. The second
tree line presents the branch and missing/conflict marker; its hover text
explains provenance and the latest verified status.

## Detection design

### Why a process-tree check before text scraping

Text-scraping a banner is what most "detect the AI tool" hacks do, and it breaks the moment the tool changes its splash screen. Walking `/proc` (via `sysinfo`, cross-platform enough for Linux/macOS) to find that the pane's foreground process is literally named `claude`, `codex`, or `agy` is a much harder signal to break by accident, and it's cheap to compute alongside the poll. Text scraping is kept only as the activity signal, where there's no substitute (a process name can't tell you if the agent is mid-turn).

### Poll cadence

"A few times a minute" per pane, but adaptive rather than fixed:

- Panes currently `Working` or `WaitingApproval` poll every ~10s — for `Working` this still makes the state flip to `Done` prompt while avoiding needless process-tree and screen scans across a large fleet; for `WaitingApproval` it also corrects transient matches without leaving a stale "needs input" badge for a full slow-tier interval.
- `Done` is an unread completed-turn alert: both the tree and right-panel title prepend `« [done] »` until the user opens that pane, submits new terminal input, or detection observes renewed non-idle agent activity. Opening the pane clears the bell and marker for every attached client.
- Panes `Idle`/`Done`/`PlainShell` poll slow (~30–60s) — none of those change on their own between polls, no reason to burn CPU reading their screen buffer.
- All intervals configurable in `~/.config/ilium/config.toml`.

## Key crates

| Crate | Role |
|---|---|
| [`ratatui`](https://ratatui.rs/) | TUI widget rendering |
| [`crossterm`](https://docs.rs/crossterm) | terminal backend: raw mode, input events (incl. mouse), alternate screen |
| [`portable-pty`](https://docs.rs/portable-pty) (wezterm) | cross-platform PTY spawn/resize/IO |
| [`vt100`](https://docs.rs/vt100) | terminal-escape-sequence parser → screen grid; source of both the rendered pane content and the text the detection engine scans |
| [`tui-term`](https://docs.rs/tui-term) | ratatui widget that renders a `vt100::Screen` directly — used for the right-hand live pane view |
| [`tui-tree-widget`](https://docs.rs/tui-tree-widget) | ratatui tree widget — left-hand session/group/pane panel |
| [`sysinfo`](https://docs.rs/sysinfo) | process-tree walk for agent identity detection |
| [`tokio`](https://docs.rs/tokio) | async runtime: PTY IO tasks, detection-loop timers, UDS server/client |
| `serde` + `toml` | config file, IPC message (de)serialization |
| `bincode` | wire format for the client↔server IPC frames |
| [`directories`](https://docs.rs/directories) | XDG-correct config/data/socket paths |
| [`clap`](https://docs.rs/clap) | CLI argument parsing |
| `ureq` | HTTP client for `ilium-inference`'s provider calls (background session/project title inference) |
| [`notify-rust`](https://docs.rs/notify-rust) | desktop notifications for agent-finished, approval-needed and monitored-task outcomes (`ilium-server/src/notifications.rs`), switched per event via `config.toml`'s `[notifications]` table (`ilium_sound::NotificationSettings`) — see M5 |

### A note on the vendored `vt100`

`vt100` 0.16.2 can retain an orphaned wide cell after a shrinking resize, then panic when a later erase sequence clears that row. Upstream issue #28 and PR #30 contain the unreleased fix. The workspace keeps a patched copy under `vendor/vt100` and wires it in through `[patch.crates-io]` until a fixed crate release exists.

This is also why ilium is not installable from crates.io: `[patch.crates-io]` applies only to the top-level workspace, so a downstream consumer would silently build against the unpatched crate. `ilium-client/vendor/tui-tree-widget` is vendored as a path dependency for the same class of reason. Both vendored crates keep their upstream licenses in place.

### Native release boundary

The release tooling under `release/` keeps distribution outside the runtime
crates. One manifest drives native build runners, paired archives, installer
selection and Pages metadata. Each installer selects a complete version directory
through one current pointer; the client and server resolve from that same
directory. Native loader, licence, embedding and installed-PTY receipts gate
publication independently of source tests. The hidden release embedding probe
uses the client inference dependency; release scripts own model acquisition and
process observation. See [`release/RELEASING.md`](release/RELEASING.md) for source,
native, publication and public-installation gates.

## Implementation plan

Each milestone is meant to be independently runnable/demoable, not a big-bang integration.

1. **M0 — PTY passthrough skeleton. Done.** One `portable-pty` + `vt100` + `tui-term` pipeline rendering a single full-screen pane, now `ilium-pty`'s `PtySession`.
2. **M1 — In-process multi-pane tree. Done** (superseded by M2). `ilium-core`'s tree model, a left `tui-tree-widget` panel, switching focus between panes, create/close pane, create/close group all shipped first as a single binary; that single-binary form was later split apart in M2 and no longer exists as such.
3. **M2 — Client/server split. Done.** PTY ownership and the tree now live in `ilium-server` (`ServerState`, one process per session, spawned on demand rather than run once per machine); `ilium-client` is a thin `ratatui` renderer over `ilium-ipc`. Concretely, as built:
   - One Unix domain socket per project session at `$XDG_RUNTIME_DIR/ilium/<project-slug>-<digest>-<session>.sock`, matching tmux's per-session-socket model. The snapshot remains with its project at `.ilium/sessions/<session>.json`; a digest disambiguates slug collisions.
   - Wire format: `ilium-ipc::framing` — a 4-byte little-endian length prefix followed by that many bytes of `bincode`-encoded payload, generic over the payload type and over the async stream (so both the request and event streams reuse it, and tests can frame into an in-memory buffer).
   - Message shapes: `ilium_ipc::ClientRequest` includes attach/detach, pane and container creation (`CreateSplitView` is atomic), structural moves, focus/input/resize, settings, and session-lifecycle requests. `ilium_ipc::ServerEvent` carries full `TreeSnapshot`s, terminal replay/live bytes, pane status/session metadata, and explicit errors. New variants are only ever appended to the end of each enum, so bincode discriminants stay stable and a peer built before a feature still decodes everything that preceded it (a test pins this for the voice-text messages). The typed-voice messages are `ClientRequest::{RegisterVoiceTextReceiver, SubmitVoiceText, AnswerVoiceText}` and `ServerEvent::{VoiceTextOffered, VoiceTextResult}`, with payload types in `ilium_ipc::voice_text`. A full tree snapshot rather than a diff keeps attached clients from drifting after structural changes.
   - The `ilium` CLI spawns `ilium-server` as a separate detached OS process (not linked in as a library), then either attaches `ilium_client::run` or sends one short-lived request and exits — this is what buys detach/reattach and session persistence across terminal closes.
4. **M3 — Tree manipulation. Done.** One-step move keybindings (leader `m` toggles keyboard move-mode, up/down or `j`/`k` calls `Tree::move_node_one_step`), the same one-step move via each tree row's hover ↑/↓ arrows, and reordering siblings including pane-crosses-a-container-boundary cases. At a nested normal-group or split-view boundary, `move_node_one_step` exits the pane into the enclosing group immediately before/after its former container; at a top-level group boundary it transfers the pane into the adjacent group so panes never become root-level nodes. Arbitrary reparenting rides `ilium_ipc::ClientRequest::ReparentNode` (`node_id`, `new_parent`, `index`), mirroring `Tree::move_node` directly, handled server-side in `ilium-server/src/ipc/handlers.rs`. Two client-side features are built on top of it:
   - Mouse drag-and-drop (`ilium-client/src/mouse.rs::compute_drop_target`) — mouse-down on a tree row starts tracking it as the drag source, mouse-up over another row drops onto a `Group` (appends as its last child) or a `Pane` (inserts as that pane's immediate predecessor in its parent group), and mouse-up over the empty space below the last row appends at the top level.
   - Keyboard indent/outdent in move-mode (leader `m`, then left/`h` to outdent or right/`l` to indent — `ilium-client/src/keys.rs::compute_indent_target`/`compute_outdent_target`) — indent moves the selected node into the nearest preceding sibling group (appended at its end), outdent moves it out into its group's own parent (positioned right after that group among its new siblings).

   Both client-side computations reject the unambiguously-invalid cases before ever forming a request (dropping/indenting onto the node itself or one of its own descendants, and leaving a pane parentless at the top level); every other rejection (e.g. a stale id from a concurrent structural change) comes back as `ServerEvent::Error` and is shown in the status bar rather than crashing the client. Deliberately left simpler: no visual drop-target highlight during a drag beyond the tree's existing row-hover affordance, and no "indent into previous group" / "outdent" entry in the right-click context menu (a menu click has no natural "which preceding group" to indent into the way a specific drop position or an ordered sibling walk does).
5. **M4 — Agent detection engine. Done.** `ilium-detect`: the shared built-in provider registry plus extensible generic/custom signatures, process-tree identity check via `sysinfo` (`identify_agent`), text-marker activity check (`classify_activity`, covering the working/waiting-approval/idle states plus the numbered-selection-menu and live-status-line cases added after the original design), `ilium-server`'s adaptive poll loop (`detection.rs`, fast interval for `Working` panes, slow for everything else, both configurable), and icon/color wiring into the client's tree render.
6. **M5 — Polish. Partially done.** Everything below is configured in `~/.config/ilium/config.toml` and/or the full-screen Settings view, which includes live-persisted Triggers, Voice control, and Debug tabs alongside the other client surfaces.
   - *Config surface.* Server-side, `ilium-server/src/config.rs`'s `[detection]` table covers the two poll intervals plus `[[detection.custom_signatures]]`. Client-side, keybindings, the configurable `[keyboard].shortcut_base`, `[ui].tree_order`, per-provider agent icons, and the four-color theme override are configured there too.
   - *Inference value saves.* The ordered configuration writer acknowledges durable disk publication before an inference value editor returns to its existing parent view. Save-operation and dialog identities jointly own completion: obsolete receipts cannot dismiss a newer editor or mark its value saved. Admission failures, rejected or lost receipts, and disk errors retain the draft and expose a retryable error. The restructure token-budget editor autosaves valid changes after 600 ms; Enter shares a pending write for the same value, so repeated submissions do not enqueue duplicate writes. Escape revokes the dialog's completion ownership without cancelling an already admitted write, and autosave completion alone never dismisses the editor.
   - *Debug logging.* Defaults off because enabling it records complete HTTP and LLM text requests, responses, provider errors, and application action/error context; credential headers and URL parameters stay redacted, binary audio is summarized, and unrelated sensitive per-agent evidence stays in its separately enabled journal. Enabling `[debug].file_logging_enabled` applies immediately to both client and detached server over IPC; one server lifetime writes `/tmp/.ilium/<project-session-id>/log-<local-start-date-and-time>.txt` with private directory/file permissions.
   - *Triggers.* Maps startup completion, prompt submission, session readiness, useful agent lifecycle transitions, and low-noise plain-terminal checkpoints to zero or more LLM actions: retitle the originating element, restructure its project, or restructure every project. Agent lifecycle events share the sound classifier, prompt origins are explicit across keyboard, voice, scheduled, queued, and initial-agent input, and startup fires only after the complete tree/replay/metadata state stream has loaded. A fresh install enables AI retitling and restructuring: startup restructures every project that has un-restructured activity (a project with none is skipped without a provider call), session-ready/prompt/started-working/background-wait/terminal checkpoints retitle, and a finished agent retitles itself and restructures its own project. Failed automatic restructures are contained by a one-to-thirty-minute retry breaker and reported in the restructure status line. Title style defaults to Labeling.
   - *Title ownership and eligibility.* Agent labels require a genuine request in that pane's project-verified conversation or acknowledged authored text plus Enter for its exact invocation. Startup screens, assistant output, goal bookkeeping, update requests, progress notifications and sibling work supply no permission to title it. The shared transcript adapter distinguishes positive request evidence, verified empty history and unavailable history; unavailable history preserves existing labels. Client inference captures each leaf's presentation revision and conversation/process identity. The server independently verifies request evidence and compares those observations under its tree/runtime locks across every title and restructuring apply path. A stale or ineligible title is dropped while valid structural grouping and animation recommendations can still apply.
   - *Presentation lifetime.* Inferred labels remain automatic even after an explicit AI retitle; literal user renames fix the full name/short-name/icon bundle. Persisted checked presentation revisions fence manual renames, newer AI labels, task submission, reset and undo without tracking ordinary output. Undo records accepted presentation revisions and preserves later title changes. Fixed names survive fresh conversations and recovery. Replacement may transfer an inferred title only after verified same-conversation continuity. Restore repairs only non-fixed legacy AI restructuring ownership; visible text resets only when the current own history is verified empty and has no authored delivery receipt.

   - *Voice control.* Selects the Realtime model, voice, reasoning effort, semantic-VAD/push-to-talk mode, VAD eagerness, input/output devices, local volume, masked API key, additive multiline prompt, and an opt-in terminal-submission confirmation policy; `OPENAI_API_KEY` is also accepted without persisting a credential. The confirmation policy defaults off, so an explicit request forwards dictated text plus Enter immediately. When enabled, ilium first types the text visibly without Enter, asks whether to submit what is on screen without reading it aloud, and presses Enter in that same pane only after yes. The global bottom-right control remains reachable over every panel and modal, with click-to-toggle and global F8/hold-F8 interaction. Its semantic tools cover navigation, every left-tree mutation/launcher, terminal input and queues, editor and board operations, workspace search, the complete settings registry, and session lifecycle; destructive actions remain explicitly confirmed regardless of the terminal-submission preference.
   - *Typed voice input (`ilium voice say`).* Feeds text into the running voice session as if it had been spoken, so the same model, tools, target rules, and confirmation policy apply and text can be mixed with live microphone audio. The voice session lives in an interactive client (it owns the audio devices, the provider socket, and the tool executor), so the server is only a broker and `ilium-voice`/`ilium-ipc` stay free of each other's concerns:

     ```
     ilium voice say ──SubmitVoiceText──▶ ilium-server (voice_relay)
                                              │ VoiceTextOffered (one client at a time, newest first)
                                              ▼
                                        interactive client ── VoiceCommand::SendText ──▶ voice actor ──▶ model ──▶ tools
                                              │ AnswerVoiceText
     ilium voice say ◀──VoiceTextResult───────┘ (relayed to the requesting connection only)
     ```

     Every connection, the one-shot CLI included, performs the same interactive attach handshake, so a client that can host voice announces itself with `RegisterVoiceTextReceiver`. The relay validates the request (`normalize_voice_sentences`), then offers it to registered clients newest first and one at a time: first only as-is (`start_voice: false`), so a client whose voice already runs always wins; then, only when the request asked to start and no session accepted, once more with `start_voice: true` to the newest client that reported voice off or failed. Offering to a single client at a time is what stops two attached TUIs from both acting on one sentence or both switching a microphone on. An unanswered offer times out after 10 s; a request id in flight cannot be reused. The client answers after lifecycle reconciliation, so a start request finds its session (or its failure) before the text is delivered, and "accepted" means the sentences are queued to the live session -- the provider gives no per-turn acknowledgement -- reported with the session phase and whether voice was switched on. The CLI (`ilium/src/voice.rs`) never starts a server, prints JSONL (`progress`, then one `result` or `error`), and addresses the pane's own session from its environment or the `--cwd` project session otherwise. Text and speech share the provider's one-response-at-a-time rule inside the actor, which is why typed sentences queue behind an active response or pending tool outputs instead of cancelling them.

     `ILIUM_VOICE_REALTIME_URL` (a loopback `ws://` URL only, parsed rather than prefix-matched so `ws://127.0.0.1:1@host` is refused) points the provider adapter at a scripted local server, and `ILIUM_VOICE_AUDIO=none` skips opening audio devices. They are test and demo seams: `ilium-voice/tests/typed_text_mock.rs` drives the adapter, and `ilium/tests/voice_say_e2e.rs` runs the real server, the real PTY client, and the CLI against a scripted model whose tool call types into a real terminal pane, with no network, key, or audio hardware.
   - *Kanban board.* Persists a global 1–10-line card-preview height (four lines by default) and minimum column width (45 cells by default) under `[kanban_board]`; narrower viewports page complete columns behind a horizontal scrollbar. Cards render contiguously without redundant top labels, show their source and insertion target throughout mouse drags, and expose clickable Markdown task checkboxes. Clicking the remaining card surface opens an aerated title/notes editor in the rightmost third; each keystroke is committed immediately through either the single-Markdown-file or folder-of-Markdown-files storage adapter.
   - *Sound.* Discovers only folders/files that exist on the current system (XDG/Linux distributions, macOS, and Windows), offers an attributed embedded chirping sound, a selected system file, deterministic synthesized PCM, system beep or mute with preview, and independently enables Agent finished, approval-needed, started-working, and waiting-background events. Changes persist under `[sound]`, reach the current detached server immediately over IPC, and are picked up by other running project servers through a low-frequency global-config watcher. Playback is serialized through a bounded server-owned actor, so it works with no client attached, never duplicates per attached client, and cannot block detection or IPC.
   - *Persistence and notifications.* Each project session persists independently in `.ilium/sessions/<name>.json`; detected Claude, Codex, and Antigravity IDs are converted into their provider-specific resume commands when saved, so restored panes resume their own agent conversations. Desktop notifications are sent without blocking IPC. Two distinct kinds exist: *agent* events (finished turn, approval needed; raised by the detection loop from the same projected signals as the sidebar) and *task* events (a progress monitor's `done`/`error`/lost outcome, raised by `handlers::alert_task_outcome`). `ilium_sound::NotificationSettings` is the one shared `[notifications]` type: master `enabled`, per-event flags (`agent_finished`, `approval_required`, `task_succeeded`, `task_failed`), `suppress_redundant_task_outcomes` and `task_coalesce_seconds`. Defaults notify on everything but task success, because a task finishing while its agent keeps working is routine progress already visible as the sidebar ✅. Task sounds and notifications share one policy: the per-event flag, suppression while the agent is idle or parked (its own finished alert follows; panes with no agent are never suppressed), and per-pane, per-kind coalescing (`TaskOutcomeCoalescer`). Notification text names the kind ("background task finished (agent still working)") and leads with the pane title. The client edits the table in Settings → Sound and writes it to `config.toml`; running servers reload it through the existing config watcher, so there is no IPC request.
7. **Split views. Done.** `ContainerNode` generalizes tree ownership without duplicating membership in client state. Leader `"`, the tree footer split button, or a context action opens an orientation dialog and an optional eligible-pane picker; the server applies one atomic `CreateSplitView` mutation. `RightPanelTarget` and the pure `split_layout` allocator render zero to four panes, resize each visible PTY to its own viewport, and route keyboard/mouse/editor/board interactions only to the active slot.

## Compaction optimizer

Settings → Optimization recommends the auto-compaction threshold that minimizes weighted token cost on the user's own transcripts, and can write it to the agent's configuration. Three layers keep the pieces testable:

- **`ilium-compaction-analysis`** (pure: no I/O, no async, no ratatui) — per-agent log-format parsers (Claude Code and Codex entries in one registry) producing compact `SessionTrace`s, cross-file dedupe of resumed/forked sessions, corpus statistics, the trace-driven replay simulator, the optimizer (argmin, flat bands, bootstrap, per-model split, rework sensitivity) and the per-agent trigger-to-setting semantics (Claude `autoCompactWindow` = trigger + measured offset; Codex `model_auto_compact_token_limit` clamped to 90% of the window).
- **`ilium-client` scan driver** — `compaction_scan` lists and streams the transcripts as one finite `Lane::Io` job per agent (progress by bytes in shared atomics, cancellation between chunks, a `(path, size, mtime)` trace cache, the previous report surviving a failed or cancelled scan) and `compaction_report` turns the analysis into plain view-model rows. A scan starts only from an explicit button press; there is no timer.
- **Writers** — `agent_config_writer` patches exactly one key of `~/.claude/settings.json` or `~/.codex/config.toml` (span-based, every other byte preserved), as a compare-and-swap under a sidecar lock with an atomic rename, and keeps a revert record in Ilium's data directory. It never creates the file and never touches a process: a running agent keeps the limit it loaded.

The tab itself is `compaction_ui` (layout, hit testing, the Apply confirmation) over `compaction_app` (selected agent, current values, pending apply, outcome notes, key and mouse actions; per-tick `drain_events` next to `tick_cost`). `docs/compaction-optimizer-design.md` (git-ignored scratch) records the design history and the user's decisions; this section is the maintained description.

## Guided setup

`ilium-client::onboarding` owns seven-step navigation and responsive presentation. `[onboarding]` stores started/completed state, provider enablement and the resumable step; missing configuration opens setup, existing installations retain their settings, and explicit CLI/Settings entry reopens it. Automatic naming/organization is blocked while setup is open. A revisioned atomic decision fences queued provider work and late results; project-name proposals are persisted only after event-loop acceptance, under the project configuration lock.

The sound studio edits the same bounded `SoundDesign` that `ilium-sound` synthesizes for detached playback. Cached visual waveforms come from its actual PCM. The bundled chirping audio carries its original attribution in `ilium-sound/assets/CHIRPING-LICENSE.md`; file-backed catalogs remain platform adapters.

Keyboard practice owns a private `ilium-core::Tree` and resolves the real configured prefixes and bindings. It has no PTY or outbound IPC. The voice demo owns a separate `VoiceService`, only a `set_lightbulb` capability, and an idempotent local executor. Explicit Test starts audio; page exit, configuration changes and terminal failure stop that actor. Normal voice service is suspended during setup and resumes from the saved preference afterward. Demo transcripts and receipts remain separate from application control.

## Non-goals (for now)

- No WASM plugin system (Zellij already owns that niche well).
- No built-in SSH/remote-session sharing (RMUX already owns that niche).
- No general-purpose external agent-driving SDK or remotely callable automation surface. The explicitly user-operated local voice controller can send terminal input through the same guarded client request path as keyboard input, and `ilium voice say` lets a local process add sentences to that voice conversation, but the model, not the CLI, decides what they mean and the policy layer still applies; neither exposes ilium as an orchestration server.

## Embedded application prompts

`ilium-prompts` owns the application prompt catalog, editable `.hbs` sources, build validation and shared plain-text Handlebars rendering. Client, inference, server and session conversion depend on this pure crate; it performs no runtime file I/O. Typed contracts, serialization, clipping and runtime user values remain with their callers. See [`ilium-prompts/README.md`](ilium-prompts/README.md) for maintenance and [`ilium-prompts/CATALOG.md`](ilium-prompts/CATALOG.md) for the extraction mapping.

### Additive user instructions

`ilium-inference::PromptInstructions` persists six optional fields under `[inference.instructions]`; the existing voice field remains `[voice].custom_prompt`. `instruction_settings::InstructionField` maps both the central LLM Instructions tab and feature tabs to these same values and save paths. Editors preserve authored text; prompt builders trim surrounding whitespace and interpolate it once into optional `.hbs` sections. Naming workers copy current inference settings when dispatching new requests. Restructure includes guidance on every corrective retry, Smart Copy adds only its selection preferences to the system prompt, and Ask for update renders its instructions when invoked. Empty fields preserve the built-in prompt output.
