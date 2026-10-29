# ilium — architecture and design

This document covers the product design, process architecture, crate boundaries, agent-detection strategy, and milestone history. For installation and day-to-day use, see [`README.md`](README.md). For the rules that govern changes to this codebase, see [`CLAUDE.md`](CLAUDE.md).

ilium is built around two ideas tmux doesn't have:

1. **Tree-structured pane list.** Sessions are a tree of groups, split views, and panes (not a flat window/pane grid), shown in a left-side panel. The right panel renders either one pane or a persistent vertical/horizontal split containing up to four terminals, editors, or boards. Panes can be freely reordered and moved between containers.
2. **Agent awareness.** ilium periodically inspects each pane's content, detects whether it's running an AI coding agent (Claude Code, Codex CLI, Antigravity CLI, etc.), and shows whether that agent is *thinking* or *done* via icon + color in the tree — so a glance at the sidebar tells you which of your N agent sessions need attention.

Everything else (detach/reattach, persistent sessions, PTY handling, keybindings) follows tmux's model closely enough that tmux muscle memory should mostly transfer.

Detailed design chapters live in [`architecture/`](architecture/). This index keeps the existing section anchors.

## Prior art — why not just use X

[Detailed design](architecture/prior-art-why-not-just-use-x.md).

## Core concepts

[Detailed design](architecture/core-concepts.md).
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

Server-created PTYs reserve their full persistent worker set before starting the child: five OS threads on Unix and six on Windows, each with a requested 4 MiB stack (`ilium_pty::PTY_WORKER_STACK_BYTES`). PTYs are charged to their own quota group (`ServerExecution::pty_quota_group`), not to the shared server pool, so execution jobs, output replay and recovery frames can never crowd terminals out. That group's limits are a multiple of the per-pane cost: `MAX_OWNED_WORKERS` (8,192) threads and `MAX_OWNED_WORKERS × 4 MiB` of declared stack, so the effective PTY ceiling is the owned-worker table (about 1,600 panes on Unix). The quota holds the charge through physical join, including timed-out shutdown; insufficient admission refuses the launch before a child exists. The shared server pool (execution, output, services) keeps a 4 GiB worker-byte budget, partitioned to preserve visible capture, ordinary output, visible output and exact retirement metadata headroom. Stacks are virtual reservations; these are admission declarations, not measured RSS bounds or proof that every library thread is charged. Capacity gates such as the 128-PTY pool regression test must never be lowered to make a test pass.

A failed pane start during restore never removes the pane. The restore first registers every saved pane as `PaneResource::Unrestored` (its saved kind, progress monitor and last requested size), so its tree node and command stay in every snapshot; `start_snapshot_pane` then replaces the placeholder with the live resource. Panes whose start failed keep the placeholder, a `Restore incomplete` error event names them, and one owned retry task (`unrestored.rs`) retries with backoff (5 s up to every 5 minutes) until each starts or is closed. Input to a placeholder is refused with the failure reason. Each start also copies the loaded snapshot to `<name>.pre-restore.json` (keeping the previous start's as `.pre-restore.1.json`), independent of the rolling-backup setting.

Blocking errors must never be silent. Operations that prevent a pane, project, or session from working retain a scoped, bounded diagnostic through the shared server error service; logging alone is insufficient. Attach replays active diagnostics, recovery clears only its own operation, and the client renders a caution triangle with the concrete messages on hover. Child failures aggregate onto project/container rows; unscoped failures are labelled Session. Locally refused requests retain client-side diagnostics separately. Pane creation has a connection-local request identity and a terminal success/failure result; tree growth is never its acknowledgement. A missing confirmation stops the spinner and reports uncertainty without automatically retrying. Unavailable terminal placeholders display known blocking reasons. New blocking paths must use this reporting contract and include failure/recovery coverage.

`lifecycle_log.rs` is an always-on, size-capped JSONL log (`.ilium/logs/<session>.lifecycle.jsonl`, rotated once at 8 MiB) of server start (executable path and the `<exe>.build.json` install record written by `tools/install-from-receipt.py`), restore result, failed starts, retries, close requests with their requester, closed panes and shutdown. Events go through a bounded queue to one owned writer task that appends on the I/O lane; a full queue drops and counts events.

Each PTY state owner serializes input, resize, mouse and parsed output through
one bounded per-session queue. Native writes run on the transport worker; its
published completion wakes the owner, which waits on the queue condition until
new eligible output, another queued event, the write deadline or cancellation
grace. This avoids millisecond receipt polling while preserving output-before-
command barriers and write deadlines.

### Left panel — the tree

The left panel renders this tree via `tui-tree-widget`: expand/collapse groups, select a pane to focus it on the right, reorder entries one step at a time (hover an entry's up/down arrows, or leader `m` for keyboard move-mode), drag-and-drop a row onto any other row or the empty space below the tree to reparent it there, double-click a real tree entry to open the same Rename prompt as the context menu, and right-click an entry for create/rename/move/close actions.

- Every tree menu also exposes **Restart**, which reloads the client executable and reattaches it to the existing detached server without restarting that server or its PTYs.
- The same menu has a checked **Order by** submenu for Manual, Type, Age up/down, and Name A-Z/Z-A. Automatic modes sort independently inside every normal group while leaving split-view placement untouched. The choice applies live, persists as `[ui].tree_order`, and is mirrored by the User Appearance settings tab. Any arrow, drag/drop, or keyboard structural move returns the setting to Manual before sending the move.
- Terminal rows, including detected agents and plain shells, expose **Hit key(s) X time from now**: the dialog accepts an hours/minutes/seconds delay, optional text, and optional Enter. The detached server persists and delivers that input at the absolute deadline even when no client remains attached. While pending, the row shows a human-readable countdown before the pane title and animates the existing clock sequence backwards.
- User Interface settings expose three explicit left-panel sizing cards (Settings content at least 82 columns wide; narrower screens use the shared selector row): **Fixed** owns one width; **Focus-dependent** owns separate focused and unfocused widths; **Width-dependent** stays at the focused width at or above its terminal-width threshold, then becomes focus-dependent below it. Every value applies live and persists under `[ui]`; full motion eases between policy targets while reduced/off motion snaps directly. A fresh install selects Width-dependent (breakpoint 120 columns, 44-column focused and 24-column unfocused panel), Reduced motion, agent identifiers shown as icons, and the icon set listed in `IconTarget::default_glyph`; these defaults are the maintainer's own settings.
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
- The agent toolbar's Smart Copy action captures an immutable clone of the visible `vt100::Screen`, numbers its visible lines and non-whitespace word runs, and sends only those references to the configured inference provider. The response is streamed as JSONL: every record must resolve wholly against the frozen snapshot before it becomes selectable, so model-authored replacement text and stale live-screen coordinates cannot reach the clipboard. The first valid record dismisses the blocking progress dialog; later records appear incrementally and flash for 500 ms. Overlapping candidates prefer the smallest region and the wheel cycles the alternatives under the pointer. `smart_copy_tokens` adds the pre-scanned fine-grained layer without any model call: per-line regex detectors (URLs, e-mail, IP, endpoints, paths and `file:line` locations, qualified and identifier names, hashes, versions, assignments, flags, quotes, key/value fields, sentences, dates, amounts, phones), a multi-line postal-address matcher, and `vt100` cell-attribute runs (foreground colour, highlight, bold, italic, underline). Path-shaped tokens carry a `PathProbe`; the bounded I/O capture worker resolves them through `PathContext` (pane cwd, its ancestors, the project root, `~`, a few conventional source directories, a bounded stat budget) when the session is built, relabelling hits as `file`/`directory` and dropping implausible misses. Every non-empty line is also a region, and regions are deduplicated by exact geometry with structural, line, detail, token and styled regions in that priority. Clicks toggle a region in a persistent multi-selection that stays inverted and is written to the clipboard on each click. Smart Copy light (`smart_copy_light.rs`, `[terminal] smart_copy_light` / `smart_copy_light_key`, on by default with Ctrl) reuses the same capture and detection but never queues a model request: `App::try_start_smart_copy_light` starts it from a mouse event carrying the configured modifier over a terminal pane, clicks only toggle regions, and `finish_smart_copy_light` copies the selection once on release and installs a `SmartCopyPreview` that `draw_smart_copy_preview` renders with a one-second countdown gauge. Release is inferred (terminals do not report modifier release): a Kitty key-release event, the first mouse event lacking the modifier, focus loss, or `RELEASE_IDLE_GRACE` after the last event once something is selected.
- A terminal pane may own one long-task progress registration. Registration is transactional: the server validates one bounded JSON probe before replacing an existing monitor and returns a correlated acknowledgement with its generation ID. The detached server—not the agent—then performs every recurring probe, keeps task `error` distinct from monitor degradation/failure, persists terminal evidence, and waits for a verified clean agent composer before submitting the result. Each report carries two human-facing descriptions that must make sense without any session context: a one-line `message` (always shown in the footer, followed by a hover marker) and an optional multi-line `details` (What/Why/Now/Next/Watch, at most 8 KiB, shown in a tooltip when the pointer rests on the footer); the managed agent instruction block (`progress-instructions.hbs`, schema version 6) tells agents how to write both. The result message is the monitor's only effect on the agent: it never pauses, resumes, or otherwise touches the agent's `/goal`.
- ProgressTiming retains up to 64 actual probe observations (reported progress, unchanged successful heartbeat, or failed observation), independently of the percent samples used for ETA. The latest accepted task report and its timestamp survive failed probes; IPC and snapshots carry both histories atomically. The footer warns only about “no probe report” (no accepted probe report for ten intervals, at least ten minutes; unchanged successful reports never stall), the report carries an optional `indeterminate` flag that suppresses the ETA, while the progress-specific popover renders current diagnostics beside the accepted report as a UTF-8 tree and a time-scaled recent-check timeline; compact terminals stack its panels. Probe instructions require real counters/phase descriptions and actual local or remote job identity/result checks rather than inferring liveness from a launcher.
- The second header icon of a detected Claude Code or Codex pane opens the costs-and-stats popover: hover previews it, a click pins it (non-modal, so keystrokes keep reaching the agent; only pointer events over the popover are claimed), and the icon or its close control dismisses it. Its data never touches the server or the wire: `session_stats` parses the agent's own JSONL transcript into a `SessionStats` value, and `session_stats_store` admits discovery, incremental reads and aggregation as bounded jobs on the process-shared I/O lane; each pane's accumulator resumes at its byte offset and ignores a trailing partial line. `session_stats_ui` draws four tabs (Overview, Tokens, Activity, Prompts) onto a tall off-screen buffer that is windowed into the frame, sized to fill the right-hand panel. Its over-time graphs use `ascii_chart`, a port of the asciichart/rasciigraph plotting algorithm (same glyphs, label precision and NaN gaps, validated against rasciigraph's published outputs) that returns a grid of kind-tagged cells so the caller colours each series, and that can downsample by per-column maximum so spikes survive. Transcripts reach several gigabytes with lines above 30 MB, so the parser filters raw bytes with a regex set before deserialising, counts tool failures from bytes without parsing tool-result lines, and merges Claude's repeated per-block usage by message id rather than summing it. Token counts are normalised across providers (`input` is uncached input, cache read/write are separate, Codex's cached share is subtracted from its inclusive input count). The popover shows dollars only when the agent recorded them (Claude Code's `cost-state` snapshot); the agent-cost indicators below price tokens themselves.
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

## Form row leaders

[Detailed design](architecture/form-row-leaders.md).

## Ambient animation rendering

[Detailed design](architecture/ambient-animation-rendering.md).

## Process architecture

[Detailed design](architecture/process-architecture.md).
Client/server, like Zellij and tmux itself — this is what makes detach/reattach and session persistence possible instead of "just a TUI app that dies with the terminal."

Custom agents are profiles, not additional detection signatures. A stable profile ID and its Claude or Codex base travel over IPC and remain in the server-owned pane origin; environment rows do not. Before each process start the server reads and validates the current owner-only plaintext configuration, then passes the approved values to the terminal adapter as structured environment entries. The terminal adapter and server retain authority over their reserved variables. Snapshots store only the non-secret identity and resume metadata, so a missing or incompatible profile is a recoverable start failure rather than permission to run the base provider without the profile.

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

[Detailed design](architecture/crate-roles.md).

## Worker and service execution

[Part 1](architecture/worker-and-service-execution-01.md), [Part 2](architecture/worker-and-service-execution-02.md), [Part 3](architecture/worker-and-service-execution-03.md), [Part 4](architecture/worker-and-service-execution-04.md), [Part 5](architecture/worker-and-service-execution-05.md).

## Ordered terminal ownership

[Detailed design](architecture/ordered-terminal-ownership.md).

## Agent worktrees

[Detailed design](architecture/agent-worktrees.md).

## Detection design

[Detailed design](architecture/detection-design.md).

## Scaling to hundreds of agents

[Detailed design](architecture/scaling-to-hundreds-of-agents.md).

## Key crates

[Detailed design](architecture/key-crates.md).

## Implementation plan

[Detailed design](architecture/implementation-plan.md).

## Compaction optimizer

[Detailed design](architecture/compaction-optimizer.md).

## Guided setup

[Detailed design](architecture/guided-setup.md).

## Attention queue

[Detailed design](architecture/attention-queue.md).

## Non-goals (for now)

[Detailed design](architecture/non-goals-for-now.md).

## Embedded application prompts

[Detailed design](architecture/embedded-application-prompts.md).

## Shared resource queues

`ilium-core::resource_queue` owns the pure, exclusive FIFO state machine, retry receipts, invocation fences, ownership generations and queue estimates. `ilium-server::resources` coordinates it through one user-wide catalog; project settings and managed instructions are projections, never competing authorities. `ilium-platform::resource_fs` owns durable local-filesystem locks, replacement, clocks and process birth proofs. Stable project identities travel with structured project metadata; explicit revision-bound rebinding reconciles directory moves. Bounded durable lifecycle events support detail history. Catalog paths are supplied explicitly by the server composition root so isolated tests cannot use real user data. Projection reconciliation also processes open or restored project paths with an empty authoritative binding set, so final-unlink cleanup retries after failure and restart. Failed cleanup keeps the observer active even without clients or reservations; closed, unreferenced paths are reconciled when reopened.

A reservation estimate never expires ownership. Dead owners remain occupied and need attention; uncertain invocation evidence blocks promotion. Guarded recovery separates revocation from an explicit clearance confirmation. Pending acquisition receipts survive restarts until acknowledged by an authenticated status or wait request. Native blocking waits use owned connection tasks and broadcast observations, with a bounded cross-project invocation proof before handoff. Resource request and attach workflows use heap-backed future boundaries so their state does not inflate ordinary connection stacks. Duplicate wait attachments have a finite per-reservation limit and share one presentation entry.

The service retains one process-local durability-read proof, not a cached catalog. It holds an open descriptor for the reconciled file and checks its device, inode and change metadata under the stable lock. Unchanged files reuse that completed synchronization; every replacement requires fresh file and directory synchronization before use. Holding the descriptor prevents inode reuse from hiding an unacknowledged replacement. An absent catalog can reuse a synchronized parent-directory proof while that open directory and its change metadata remain unchanged; every read still opens the catalog first, so a newly published file must acquire its own proof. Fresh processes start without a proof, commits retain durable replacement, and the authoritative catalog is decoded and validated on every read.

`ilium-ipc::resource` carries typed operations and snapshots. The CLI exposes `resource` and `ressource`; the client renders virtual resource rows, queue details, waiting badges and segmented wait progress. Optional grouping changes presentation only, preserving actual pane ancestry and input routing. `ilium-instructions` is the shared atomic writer for resource instructions and existing agent feature blocks. See [the resource guide](src/docs/resource-queues.md) for commands and hardware safety boundaries.
