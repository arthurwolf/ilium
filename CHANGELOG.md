# Changelog
History: see CHANGELOG.archive.md


All notable changes to ilium are recorded here, newest first. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

The workspace version is `0.1.1`.
The `v0.1.0` tag (2026-10-02, `6906553`) marks a release-pipeline qualification run
and is not an ancestor of `master`; its content is covered by the 2026-09-29 to
2026-10-02 sections. Pre-release development history is grouped by
development period. Dates are commit dates (`YYYY-MM-DD`).

## [Unreleased]

### Fixed
- Migrated Kilo StepFun defaults and test fixtures to `stepfun/step-5-preview-free`. Running processes and installed binaries require a separate release update.
- Inland lake shaping now fades at wetland climate boundaries, preventing abrupt height and water changes at neighboring columns.
- Saved-world preparation progress now rounds partial phases consistently.

- Unfreezing a frozen pane reserves request capacity before focus updates, preventing focus traffic from consuming the slot needed to resume the saved agent session.
- Agent Cost metric and calibration settings now use shared left/right selectors and full choice dialogs.
- Compact Settings navigation uses readable short labels beside semantic icons when full tab titles do not fit.
- Animation loop caches now report when their fixed worker and frame-storage cost can never fit the shared quota; temporary pressure remains retryable.
- Frozen-source replay admission now charges quota for retained native-image vector capacity using checked size arithmetic.
- Agent effort selector guidance now states that left-click advances and right-click reverses; `+` opens the full choice list.
- The AI Title style setting now uses the shared selector, including its full choice list and consistent pointer direction.
- Appearance panel sizing and Agent Monitoring display mode now use shared selectors with complete choice lists.
- Restoring a session never deletes saved panes any more:
  - A pane that cannot start is kept in the tree with its saved command, shown in a "Restore incomplete" message, and retried automatically (5 s, 15 s, 30 s, 60 s, 120 s, then every 5 minutes). Previously it was removed, and the next save erased it from the snapshot.
  - Each start keeps an exact copy of the loaded snapshot as `.ilium/sessions/<name>.pre-restore.json`, even with backups turned off.
  - A plain `claude` pane whose generated conversation id was not verified yet is saved with that id, and restored with `claude --resume <id>` when the transcript exists. Previously it restarted as a blank `claude`.
- Terminal capacity: each PTY now has its own quota that scales with the pane count (up to about 1,600 panes on Unix), instead of sharing the server's 512 MiB pool, which stopped a 128-pane restore after about 25 panes. PTY worker stacks went from 2 MiB to 4 MiB. The shared server pool is now 4 GiB.
- New always-on lifecycle log `.ilium/logs/<session>.lifecycle.jsonl`: server start with the executable and its install record, restore result, failed and retried starts, who requested each close, closed panes, shutdown.
- `make install` now installs only from a verified build receipt (`make install RECEIPT=<job id>`) and writes `<binary>.build.json` beside each executable.
- Sessions with hundreds of agents stay responsive (many-agent scale pass, see `PERFORMANCE.md`):
  - Detection no longer fails every tick once an agent has no discoverable session yet. The evidence and discovery reservations previously filled the shared 128 MiB result budget, so every tick failed and rescanned the whole host about three times a second, and new agents were never identified.
  - Host process scans skip per-thread entries, which cut each scan by roughly ten times on a workstation running agents.
  - Forced process-table refreshes are rate-limited, and failing detection ticks back off exponentially instead of retrying at full speed.
  - Detection batches now take the panes with the oldest deadlines first. Newer, high-numbered panes are no longer starved behind lower IDs.
  - The server can host more than about 195 panes; the owned-worker thread and memory ceilings were raised (see the capacity entry above for the current limits).
  - Idle panes no longer wake their PTY write pump every 10 ms, and the child reaper backs off to one check a second.
  - Title changes send only the changed pane to clients instead of the whole tree.
  - The client redraws background status, evidence, prompt, progress and git updates at the normal 30 fps frame cap instead of forcing an immediate full redraw for each event. It no longer redraws for events that change nothing visible.
  - The sidebar no longer sorts and formats the contents of collapsed groups. Name sorting lower-cases each name once per sort instead of once per comparison.
  - Cost tracking now covers up to 1,024 agent panes; it previously stopped silently above 128.
  - Per-tick model-icon, auto-freeze and cost housekeeping no longer copies every Codex screen or walks every pane when that work is not due or is disabled.

- Animation text prompt pastes now replay in bounded turns while preserving trailing-line-ending trimming and text-character semantics.
- Normal tree and editor paste now uses the existing bounded key-replay path, keeping large pastes from monopolizing the interactive loop while preserving native terminal paste and leader cancellation.
- The server now reserves 64 MiB within its existing 2 GiB worker-memory ceiling for selected-terminal recovery frames, so unrelated storage pressure cannot indefinitely starve visible pane output.
- Animation Settings now reclaim footer rows on compact terminals so scene, global, and control sections remain visible and independently scrollable.
- Saved-world animation footers recognize the current Scene status label and keep activity-log help available from the panel.
- Server-created PTY sessions now reserve their persistent transport and ownership workers against the shared process quota before launching a child.
- Dense gusted Wind scenes now use runtime-dispatched SIMD integration when supported, with a portable scalar fallback and cached canonical particle state.
- Wind integration now uses FMA on AVX2 and AVX-512 CPUs when runtime feature detection confirms support, retaining non-FMA vector and scalar fallbacks.
- Dense Wind gust updates reuse per-column and per-row interpolation coordinates when rebuilding the cached cell-force field, reducing repeated per-cell indexing arithmetic.
- Dense Wind gust-cache rebuilds reuse horizontally interpolated field rows across screen rows, reducing repeated interpolation work without changing operation order.
- Wind gust-field generation reuses x-only trigonometric phases and combines them with per-row phases, reducing repeated transcendental work while keeping cache invalidation tied to screen width and field dimensions.
- Dense Wind rasterization switches to deduplicated writes near the measured 16k-dot crossover, avoiding bitset overhead at lower densities.
- Wind collision sweeps now scalarize only the high-displacement SIMD lanes while unaffected particles remain vectorized.
- Wind SIMD collision fallback now visits only flagged lanes and skips the AVX-512 lane scan when no fallback is needed.
- Wind SIMD avoids collision-boundary conversions for empty-screen wrapping steps.
- Wind pointer forces keep integration vectorized and evaluate only SIMD-identified hit lanes with the exact scalar force.
- Dense gusted Wind validates cached force samples during generation, avoiding a second full-field scan before SIMD integration while retaining scalar fallback for non-finite fields.
- Help keyboard summaries now adapt to the available dialog width and mark clipped descriptions.
- Confirmation dialogs now shorten their keyboard guidance to fit compact widths while retaining the Y/N, Enter, and Esc actions.
- Appearance sizing guidance now wraps to the available Settings content width without losing its wording.
- Animation output errors now report that terminal output could not be confirmed, and uncertain post-flush receipts retain their original presentation ownership until settlement is known.
- Animation configuration admission now reserves queue capacity for the ordered scene-pause transition when a view is hidden.
- Terminal frame handoff no longer compacts the cell vector on the interactive thread; byte admission accounts for its retained capacity.
- Saved-world preparation now estimates remaining time from measured overall progress and keeps elapsed time and measured stage ETA visible in the activity footer.
- Visible terminal subscriptions now replay retained output in bounded frames, rotating across selected panes so one large history cannot hold the other panes' first content behind it.
- A newer visible-pane selection now supersedes an older replay while it waits for storage admission, so stale history cannot delay content for the pane the user just selected.
- Focus changes within a split now reprioritize recovery so the clicked pane's retained terminal content is sent before other visible panes.
- Terminal presentation now charges and requests an explicit 2 MiB native output-thread stack alongside its bounded encoder and resize storage.
- Worktree rollback now checks process users through bounded I/O admission and retains the capped evidence until it decides whether removal is safe.
- Session detection now alternates bounded transcript reads on the I/O bank with metadata decoding on the CPU bank; the coordinator alone reconciles ordered exclusive session claims.
- Sound Studio preview synthesis now checks cancellation during PCM generation, allowing stale edits to release the shared CPU lane before rendering the full sound.
- System-audio capture drops callbacks above 8,192 frames before allocating a downmixed buffer, bounding conversion work on the device callback thread.
- Spectrum analysis, helper-process readers and CPAL callback paths reserve shared ambient worker capacity; platforms without a declared CPAL thread budget refuse that backend, and unobservable CoreAudio roles stay charged through process exit.
- GPU scene workers now use shared physical-worker and frame-storage admission, cap outputs at four million pixels, and retire blocked device calls without blocking scene teardown.
- GPU capability probing now runs on an admitted ambient worker; existing scenes keep a stable runner source that resolves when the device is published, and client shutdown joins the original probe worker.

### Added

- `ilium chat context` gains `--since-last-read` (prints only records the reading pane or agent session has not seen, and nothing when none are new) and `--max-bytes` (newest records win; skipped ones are counted). The installed chatroom hooks now use both with a 2 KB cap, a session start gets four times that, and older hook commands are upgraded in place. New rooms also ignore `CHATROOM.archive.md` in git.
- Frozen agent panes show a configurable marker in the Icons settings, defaulting to a snowflake.

### Fixed

- Sidebar directory scans now reject per-entry I/O errors instead of publishing a partial folder listing.
- Saved Minecraft worlds no longer render chunks that Minecraft has not finished generating; a cheap generation-status check rejects them before the full decode.
- Parser pool Off now removes the aggregate parser-memory ceiling; initialized panes remain available for faster revisits at higher RAM use, while an explicit positive budget retains finite pooling and hidden-pane eviction.
- Chatroom reference routing now acknowledges delivered records individually, avoiding replay of a completed prefix when a later record in the same bounded batch fails.
- Antigravity status-line restoration now recovers saved custom fields when `/statusline off` normalizes the setting and adds metadata, while preserving newly changed fields.
- The frozen-screen Unfreeze button and tree context-menu action now request restoration of the pane's saved agent session.

### Changed

- Automatic agent setup now writes the Chatroom and Progress teaching only to the global Claude and Codex instruction files. It no longer adds copies to each project's `CLAUDE.md` or creates a project `AGENTS.md`, which duplicated the global text and hid the project `CLAUDE.md` from Codex. Projects whose features are covered globally no longer get a setup prompt. The Progress teaching (version 9) now requires a blocking `ilium progress wait` once other work runs out.
- Numeric controls now use one-cell `-` and `+` step buttons, leaving more room for the centered value while retaining `*` for direct entry.
- Dense Wind rasterization now checks finite coordinate bounds directly before mapping dots to raster bins, avoiding per-dot cell flooring.
- The startup progress dialog now carries Ilium's shared chrome title in its rounded frame, matching the rest of the interface.
- Voice Studio settings now use rounded titled cards with breathing room and independent overflow bars for settings and transcript panes.
- Sound Studio waveform and envelope previews now use shared titled frames with room for their plots at compact and wide terminal sizes.
- Compact icon assignment rows now retain the shared left, catalog, and right selector controls instead of falling back to suggestion-only cycling.
- Compact Settings headers now keep the title, Close button, keyboard hint and Guided setup action separate, collapsing secondary controls to preserve readable labels at narrow widths.
- Animation plugin control editors now use the shared rounded titled frame, inset content and action hit regions that match their visible buttons.
- Trigger event choices now sit inside rounded sections; wrapped action hit targets and selected-event visibility follow the framed rows.
- Simple Settings pages now separate each option group with a quiet shared-theme rule while keeping row clicks aligned.
- Costs & stats now extend section titles into restrained horizontal rules, separating dense metric groups without adding vertical rows.
- Help's two-column shortcut reference now separates action groups with a muted vertical rule while preserving its existing spacing.
- Animation Settings now uses its existing gutter to separate shared controls from scene-specific controls without narrowing either column.
- Shared value-selector dialogs now put their clipped title in the rounded top border and give the option list back the former title row, while retaining an interior title on panels too narrow for the chrome.
- Terminal parser pooling now defaults Off, retaining initialized hidden parsers to avoid aggregate pool waits; users can opt into a finite pool to trade lower retained memory for possible eviction and slower replay on revisit.
- Server PTY worker admission now has capacity for at least 128 Unix sessions while retaining a shared process-wide safety budget and replay-storage headroom.
- Parser-pool settings help now explains the no-pool memory and revisit trade-off alongside the bounded-pool memory, eviction and replay trade-off.
- Panes and layout documentation now reflects the default-off parser pool and explains its RAM, eviction, and replay trade-offs.
- Server logs now include the requested and process-wide storage bytes when visible-terminal event admission waits longer than two seconds.

### Fixed

- Native release jobs now install the pinned host Rust toolchain without redundantly requesting the same host target, avoiding rustup component collisions on Apple Silicon runners.
- JavaScript video handles now return bounded decoder metadata from `info()` using the latest native snapshot, without issuing another service request.
- Source-backed pre-rendered animation clips now stay in the in-memory replay cache; only source-free procedural clips use the disk-streaming path, preserving native source ownership during capture and playback.
- The terminal context menu's "Copy history file path" action now resolves Codex transcripts in large session stores. Codex history lookup previously used a 4096-entry scan budget, which a real store with about 8,200 entries exhausted, so the action disappeared. Menu, remote compaction and session conversion now share one interactive budget (`INTERACTIVE_TRANSCRIPT_LOOKUP_LIMITS`, 262,144 entries) that keeps the bounded, fail-closed scan. The server now also passes the transcript path it verified from the agent's open files to the client, which re-verifies it before use, so the menu action no longer depends on the scan budget when that path is available.
- Saved-world preparation now includes bounded chunk-rejection samples when projected source coverage is incomplete, so missing chunks can be diagnosed as absent, partial, mismatched, or unreadable.
- Saved-world preparation now ignores progress and activity callbacks from cancelled viewport requests, keeping the current route's progress and detailed log visible.
- Debug Settings now wraps its guidance to the panel width and keeps the file-logging toggle mouse target aligned on compact panels.
- Text Triggers now separates status, target, match pattern, and reply, wraps long rules to the content width, and keeps mouse selection aligned with each wrapped rule.
- Settings document scrollbars now support track clicks to jump through the current page and wheel scrolling directly over the bar.
- The Settings navigation rail now scrolls through tabs with the mouse wheel and selects a tab when its scrollbar track is clicked.
- Inference Settings now presents Kilo Gateway's training and secret-handling warning in a rounded privacy panel that wraps without moving row hit targets.
- Cost Settings now separate their measurement, rating, indicator, and sparkline sections with responsive rounded title bars.
- Bundled Beach now includes the verified Rich phase-cache build, with Rust, installer, release-tool and benchmark identities aligned to its archive digest.
- Numeric controls use heavy Unicode `➖` and `➕` step buttons with two-cell hit targets when space permits, and compact ASCII `-` and `+` buttons on narrow rows so decrement, increment, centered value, and `*` entry remain available.
- Settings and other UI choice fields now show previous/next arrows and open a searchable full-option catalog; clicking the value advances with the left button and reverses with the right. Sound/inference help and the Cost/Remote Compaction guides now explain the catalog opener and direct number entry.
- Plugin activation now reserves quota for the bounded package read before allocating or reading archive bytes, then releases that temporary charge when verification finishes.
- Terminal panes now keep the loading hint until actual content arrives, and large intact output publishes bounded partial previews while parsing so selected panes can render before the full journal finishes.
- Multi-pane terminal recovery now preserves client slot order instead of allowing hash iteration to choose which journal is replayed first.
- Currently displayed terminal panes get priority both when pending commands are admitted and when queued parser work is scheduled, reducing first-content delay under hidden-pane backlog.
- The dedicated terminal parser service thread now keeps inherited scheduling priority so selected-pane first content does not compete as below-normal background work.
- Wind dots now detect occupied cells crossed between simulation endpoints, including fast multi-cell movement; collision-free AVX2/AVX-512 batches retain runtime-dispatched vector execution.
- Sound Studio previews now validate and play on the server’s bounded execution lanes, then report the playback outcome to the requesting client.
- Image folder enumeration now runs on the shared bounded I/O lane, streams directory entries without an unbounded per-directory vector, and retains its bounded result until list publication.

### Added

- Location picking now offers a bidirectional Address/Coordinates selector with a full-choice dialog and bounded latitude/longitude controls with direct numeric entry.
- Saved-world preparation now reports measured progress while enumerating Minecraft saves-root entries, including the pinned-directory scan used by the native animation path.
- `ilium progress wait [MONITOR_ID]` (also `ilium wait`) blocks until a progress monitor reports done or error, fails, is replaced or is cleared, prints one JSONL record, and exits 0 done, 3 task error, 4 monitor failed, 5 replaced/cleared, 6 timeout. While it waits, the server returns the result to it instead of typing a notification into the agent's prompt; if the wait is killed, the typed notification still arrives.
- `ilium progress set --wait` registers a monitor and waits for it in one command.
- The progress instructions Ilium installs for agents now tell them to register and wait with `ilium progress set --wait` (with `--timeout-seconds` below their shell tool's limit, or in the background when their tool reports the exit), to keep waiting on a returned still-running command with the longest wait the tool allows, and to fall back to the typed result message on an older Ilium build. They also state that this wait overrides general rules against sleep loops and long waits.
- `ilium progress set` now refuses (code `monitor-active`) to replace a monitor that is still running unless `--replace` is given, so a second job can no longer silently discard the first job's monitor and result.

- Anthropic inference settings can load the live model catalog from `GET /v1/models` and select a model from that list, matching the OpenAI-compatible provider.
- Project and folder context menus can open a recipient-selectable message dialog for agent terminals; filesystem folders include agents whose launch directories are within the folder path. Sending can optionally press Enter.

- `ilium new-pane -- <cmd>` panes now close themselves after their command exits (never sooner than 3 seconds after creation, and not while a progress monitor on the pane is still live). Pass `--keep-open` to keep the pane. Built-in `--worktree` agent panes are unaffected.

- Updated the bundled Carpet animation package and release/installer digests to match the current authoring archive, including its TV-source disposal guard. Release-inventory and runtime qualification remain pending.

- Added bounded native source-sequence capture plumbing for pre-rendered animation clips, with timeline-bound clip identity and per-sample frozen source selection; current Rust integration remains unqualified.

### Changed

- Saved-world capture receipts now record terminal-cell and pixel dimensions plus zoom for each frame and its manifest, so rendered images can be compared with their actual viewport settings.
- Wind scene rendering now walks canonical SIMD position arrays directly and keeps them resident across pointer changes, avoiding both per-dot AoS/SoA selection and unnecessary state flushes.
- Dense wrapped Wind SIMD now gathers force components from a contiguous two-float cell field without repacking the full field into separate component buffers each substep.
- Wind merge rendering now counts cell occupancy and records subpixel coverage in one particle pass, then paints only cells below the merge threshold.
- Wind rasterization now maps finite in-bounds positions directly to Braille subpixels using exact power-of-two scaling; invalid non-finite positions leave raster and ownership untouched.
- Pointer-active dense Wind scenes now keep unaffected AVX2/AVX-512 blocks vectorized and use deterministic scalar integration for blocks intersecting the pointer-force radius.
- Kilo Gateway's inference privacy warning wraps to the available panel width, and compact text prompts retain their action hint on narrow terminals.
- Remote-compaction progress dialogs now keep the privacy recipient, failure message, and recovery actions visible on narrow terminals.
- Worktree-close progress dialogs keep both their status and Escape cancel hint visible on narrow terminals.
- Generic image metadata inspection and decoding now run as separate admitted, cancellable shared-CPU jobs; HTTPS cache reads and downloads use the shared bounded I/O lane, returned bytes remain storage-charged through preparation, and source-pixel budgeting admits one ordinary 4K image within the aggregate worker quota.

- Pinned animation handles use Unix device/inode metadata for files and directories; Windows positional reads use a reopened, identity-checked file handle to isolate its cursor. Cross-platform saved-world directory enumeration and native runtime verification remain outstanding.

- Sound Studio waveform previews now use the shared bounded CPU bank, coalesce to the latest edit, reject stale studio revisions, and keep preview storage charged until it is replaced or closed.

- The saved-world capture probe accepts and validates a selected texture pack's internal archive root, matching the configured UI asset subtree.

- Generic image decoding now observes cancellation between bounded source reads, and built-in animation caches survive changes to package preferences, source tab and semantic scope that do not affect native frames.

- Live fleet feeds now reserve bounded worker and retained-storage capacity before fetching; the charge follows each immutable fleet batch through scene and marker preparation.

- Linux distribution package checks now use owned systemd containers with verified namespace, delegation, bounded evidence and cleanup records. The six distribution lanes retain package lifecycle and installed-animation checks; native execution qualification remains pending.

- Desktop notification delivery now uses pre-admitted jobs on the bounded server I/O lane. Detection and task-outcome coordination no longer waits for the notification daemon; oversized or overloaded advisory alerts are logged and skipped.

- Live detector-setting writes now use bounded server I/O admission and remain ordered with validation and apply-after-persistence semantics.

- Initial server-state synchronization reserves bounded snapshot storage before copying and retains the admission through each queued event; repeated state churn now returns an explicit retry error.

- Plugin issue details and Costs & stats use the shared panel title treatment, retaining the stats pin indicator and close control.

- Overflow scrollbars use continuous vertical tracks across Settings content, icon assignments, the icon catalogue, the main sidebar, terminal scrollback and both editor views. Fitting content keeps its existing uncluttered presentation.

- Terminal parser pooling defaults to Off, retaining initialized hidden panes for faster revisits. Explicit finite budgets remain supported; Terminal settings explain the RAM versus replay tradeoff.

- Frozen terminal snapshots are serialized on the shared CPU lane and saved by the ordered I/O writer; restoration reads and parses bounded snapshots on shared workers and publishes only for the still-frozen pane generation. Focused worker and durable-readback qualification is pending.

- Saved-world preparation now reports a provisional ETA for remaining route checks after measuring candidate work, including the sample count and upper-bound assumption.
- The saved-world animation panel keeps the route ETA beside the progress bar ahead of longer status details, so short terminals retain the most useful remaining-work estimate.
- Saved-world preparation now shows a monotonic whole-task progress bar across phases, scan stages, and map changes; the activity detail retains the current stage's measured count and percentage.
- Saved-world scan progress now exposes bounded candidate-search work as its own stage, and closes early searches using measured work rather than the full search budget.
- Saved-world directory enumeration now reports incremental pinned-entry counts while region files are being listed, so preparation progress advances during the directory scan itself.

- Refreshed the bundled Beach and Carpet archives and release digest pins from their qualified TypeScript authoring builds.

- Server-side title evidence collection now uses bounded I/O admission and refuses oversized candidate batches or transcripts without granting a title update.

- Document the first GitHub Packages visibility transition and safe retry of the same qualified release bundle.


- Rust compilation now uses the installed ni-vm whole-job dispatcher: configured Cargo/rustc commands transfer inputs automatically, compile remotely, return hash-verified artifacts and fail without local fallback. The project compiler wrapper also rejects raw local compilation. Native build scripts, macros, linking, remote tests and local returned-artifact execution were qualified independently of Ilium release gates.

- Worktree include discovery, copying, and rollback now use the bounded server I/O lane; copying checks cooperative cancellation between source entries and files while retaining identity-safe rollback behavior.

- Added a native Linux sandbox qualification runner that binds test and helper checksums, executes exact cases in ordinary user services, and rejects skipped or mismatched results; Linux release sealing now requires all four bound execution receipts and compares their artifact metadata against the retained file (also retained separately for each architecture during candidate aggregation), and the minimum-system VM receives the matching audited helper and test artifact with Bubblewrap explicitly installed during guest setup. Native execution and full-suite qualification of this integration remain pending. Timeout paths now stop only a verified fresh test-service identity and retain partial diagnostics; all four focused cleanup/checksum tests passed remotely on unchanged inputs.

- Linux release builds retain a checksum-bound native sandbox integration executable for isolated qualification, explicitly marked compiled but not executed.

- Smart Copy Preview fits narrow screens without panicking and sizes copied Unicode text by terminal columns.

- Remote compaction skips the final summary merge when retained chunk summaries exceed 16 MiB or 256 entries, while keeping the latest progressive summary.

- Plugin permission review now uses a titled rounded frame, with consent choices and submit clicks sharing the frame's inner geometry; wrapped details have their own overflow track above the fixed actions. The stored scroll position follows the visible end of the details so upward scrolling responds immediately after repeated downward scrolling.

- Agent debug history now shows a continuous vertical overflow track in its reserved text gutter.

- The project and folder message dialog uses scope-neutral wording when no agents are available.

- Agent cost detail cards and onboarding practice tree/pane frames now use the main UI's rounded borders and title spacing, preserving the practice view's focus colours.

- Address-result lists in the Location picker show overflow beside the labels, with scrollbar clicks excluded from address selection.

- The Worktrees inventory shows overflow in a dedicated scrollbar column, keeping labels and row clicks separate from the track; compact dialogs allocate bounded header, detail and action regions without extending past the frame.

- Linux animation isolation now has a guarded preparation API for a fresh delegated controller service, with exact service identity, exclusive process membership and controller readback checks. Automatic installed-launch integration remains pending.

- Triggers now uses the Settings content scrollbar instead of covering its first row with an overflow count; plugin issue details use the shared rounded border.

- Workspace Search now uses the shared titled outer frame, keeps its inner scope line responsive without repeating the title, separates its overflow scrollbar from result text and clicks, and gives each result a quiet divider; the file/folder picker also shows overflow in a dedicated column outside file rows and confirmation controls.
- Release instructions now specify the first GHCR package visibility change and recovery on the same qualified tagged run, preserving the existing digest and anonymous verification.
- The compaction Apply confirmation now shows text overflow in a dedicated scrollbar column without covering the confirmation text or actions.
- Compact Icons settings keep whole quick-choice slots and the catalogue action visible before showing the sidebar preview; the icon catalogue scrollbar has a separate content column.
- Settings navigation and content now have titled rounded frames, with dedicated scrollbar and help columns that preserve content cells.
- Kanban columns, cards and detail editors now use the shared rounded panel chrome, with the main interface title style on the detail panel and a spacer row between cards; selected cards stay visible as keyboard navigation advances, overflowing columns have a separate scrollbar, long Unicode column titles keep their card counts visible, and clicks/drop targets follow the same viewport and spacing.
- Every Settings navigation tab now has a semantic UTF-8 icon, aligned by terminal-cell width; short terminals show a dedicated navigation scrollbar beside the active tab's visible window.
- Choice and number dialogs, Settings help panels, onboarding choice cards, animation explanations and text-trigger storage errors now share the main interface's rounded panel borders.
- Agent Monitoring model-icon changes now send generation-fenced Antigravity status-line actions to open panes and defer saved settings restoration until the server confirms the live command returned to a verified composer.
- Attention mode now excludes progress reports from tree icon priority by default, keeping current agent activity visible after task or monitor failures. Enable “Progress reports in Attention” in Agent Monitoring to restore the previous priority; progress footers and notifications retain their reports.
- Generated Overworld landscapes keep selected full texture packs as the default and offer an opt-in source choice for installed, hash-pinned Java 1.19.3 assets; viewport receipts include material coverage, skipped states, and model substitutions.

- Saved-world painted-frame receipts retain the exact shared texture-bank coverage and native/selected archive digests, so capture probes can reject blank or incompletely textured frames.

- Saved-world animation settings keep the current phase and measured ETA visible while the preparation activity log scrolls independently.

- V8 package research now records the current 20-file Carpet performance manifest and the RAM-only module/asset loader contract; matching-helper qualification and current-archive timing remain pending.

### Fixed

- Keep complete Kanban keyboard hints visible at narrow widths using a measured footer that gives the viewport back its rows when space is tight.

- Keep distinct Settings navigation icons and recognizable tab names visible in narrow left panels.

- Give every onboarding step a shared rounded, titled content frame with matching inner drawing, scrolling and pointer geometry.

- Wrap Sound and Voice Settings explanations to the available width while keeping help, controls, scrolling and mouse targets aligned with their measured rows.

- Preserve complete dialog action labels in narrow buttons by showing shortcut hints only when both fit.

- Use a single-column keyboard reference and compact footer guidance in narrow Help dialogs.

- Keep queued-draft delivery explanations readable in compact dialogs, including counted and continuous repeats.

- Native Linux release qualification now requires a captured service cgroup identity and verified kernel-path removal; inactive systemd state alone cannot pass cleanup. Real native qualification remains pending.

- Queued prompt drafts now wrap and follow the editing cursor inside their framed field, keeping long drafts readable without truncating queued text. Short terminals use a compact layout that retains the draft, delivery controls, warning and enqueue button.

- Settings wheel input now follows the pointer: the left menu changes tabs, right-panel content scrolls independently, and header/frame edges leave both unchanged.

- Corrected Settings choice registry sizing and status-icon value rendering types so all registered choices remain available to compilation.

- Release contract tests reject native sandbox runner cases absent from the checked Rust test source.

- Disposable Linux release guests install FFmpeg and the user-session D-Bus prerequisites before decoder and ordinary-user sandbox qualification.

- Use the shared main-panel title styling for Settings icon assignments and sidebar preview, Kanban card title and notes editors, and the agent-message dialog; keep the Agents heading readable on narrow screens.

- Keep agent recipient overflow in a dedicated scrollbar column so border and track clicks cannot toggle recipients.

- Align release packaging and both installer integrity checks with the shipped Beach and Carpet archive hashes and the Rust release inventory.

- Terminal snapshot publication under memory pressure yields to other panes and retries without parsing commands twice. Delayed initialization shows its allocation status while preserving painted content.

- Anthropic inference requests no longer send `temperature`; `claude-haiku-5-5` rejects it with a 400 error. Requests use the provider default.
- Retain the verified GitHub Packages bundle digest and retry identity in diagnostics when anonymous manifest access fails; publication still requires public verification.


- Antigravity model-capture shutdown now leaves user-owned status-line changes alone and disables a previously disabled renderer without deleting its saved command. Timer-only wakes no longer count as status-line delivery.

- Plugin animation runtime failures now appear in the Plugin issues panel only while a plugin is selected, preventing native-scene status from being misreported as a plugin error.

- Bound native release Cargo builds to 16 jobs and nested native builders to one job, overriding inherited runner parallelism.

- First-release installer recovery checks every public installer route and tolerates an unavailable origin only after confirming that both release and deployment channels remain empty.

- Help keeps its Escape dismissal hint visible when a short or narrow popup cannot fit the complete footer.
- Antigravity prompt readiness now requires the live cursor after the empty `>` composer and rejects drafts, wrong rows or selection menus before command delivery.
- Title-evidence transcript reads now use the same bounded line and byte budget as metadata discovery; limit exhaustion leaves the title evidence unavailable instead of allocating an oversized JSONL record.
- The CLI dispatch match now includes the internal Claude model-capture command, preserving its pre-runtime-only boundary and avoiding a non-exhaustive match compile error.
- Installation guidance keeps the source-build fallback without retaining publication-status text that becomes stale when the first release is published.
- Cost-history calibration now reports unreadable or malformed transcript roots and entries as incomplete scans instead of silently treating them as absent history.
- Topographic and OpenStreetMap loaders now reserve shared worker capacity before decoding and charge retained heightfields/maps until the scene releases them; temporary capacity refusal retries without blocking rendering.
- Native release jobs install the pinned Rust toolchain in a private Rustup directory, preventing preinstalled runner components from conflicting with the macOS release build.
- Remote compaction now uses bounded shared I/O admission, keeps semantic completion in typed receipts, and cannot stall cancellation on a full UI event queue. A dropped progress event after transcript commit no longer turns a successful rewrite into a reported failure or masks other domain errors. Transcript capture refuses files larger than 128 MiB before whole-file allocation. Rewrite verification checks only persisted appended records and refuses appended output above 16 MiB, avoiding a second full-transcript parse.
- Session conversion now runs through the shared bounded I/O bank with receipt-backed completion and nonblocking progress delivery. Claude and Codex transcript reads and generated output are capped at 128 MiB, and the Codex import ledger at 16 MiB, before unbounded allocation or persistence. Provider error normalization no longer duplicates complete unbounded messages.
- Overworld pack settings migrate legacy selections to the eight retained packs, preserve per-pack custom sources, and retire Jicklus, F8thful, and Whimscape from runtime selection.
- Saved-world preparation keeps its activity log advancing while saved-history access is busy and reports a measured stage ETA with later unmeasured work called out.
- Saved-world preparation labels the total estimate incomplete, shows measured active-stage timing and the bounded remaining route-candidate count, and clears candidate state when preparation stops.
- Clicking the Model Icons label in Agent Monitoring now toggles the setting, matching clicks on its value control.
- Agents parked on an `ilium progress` monitor could wait forever. A result queued when an agent exited, restarted or was resumed after a reboot became permanently undeliverable; a monitor whose observation task died, was restored while monitoring was disabled, or was stopped by the progress kill switch was dropped without telling the agent. A new reconciler (every 20 s) now turns a monitor that lost its observation task into failed evidence, and re-delivers any settled outcome the agent has not received (marked as a possible duplicate when an earlier attempt may have landed), up to 5 attempts per monitor. The kill switch and a restore with monitoring disabled keep sticky "outcome unknown" evidence instead of removing the monitor, and the agent is told once monitoring is enabled again. `progress_set` output now names `ilium progress status` as the recovery command.

### Added

- Qualified tag releases publish their exact GitHub release asset bundle to a repository-linked public GitHub Container Registry package. Publication reconciles matching retries without replacing a conflicting tag and verifies every asset through an anonymous pull before recording success. Publication-intent recording supports Windows contract checks while retaining Linux directory durability.
- Project and folder menus offer “Send message to all” with selectable agent recipients, a multiline message, and an optional final Enter; folder status requests reach all contained agents. Multiline drafts stay open without queuing input when any selected terminal lacks bracketed-paste support, preventing message newlines from submitting commands.
- The headless scene probe can write frame PNGs using the same ordered-dither mask and cell colors as its Braille output, with JSONL artifact metadata.

### Changed
- Prepare the first release from `master` as `v0.1.1`, with matching versions across all 28 workspace packages and their internal dependencies; preserve the historical `v0.1.0` qualification tag.
- Agent Monitoring status-icon selectors now use the shared left/right step controls; the `+` button and keyboard shortcut both open the full icon catalogue.
- Numeric controls use the heavy UTF-8 `➖` and `➕` step symbols with two-cell hit targets, while keeping the value centered and `*` for direct entry.
- The bundled Beach and Carpet animation packages match their qualified TypeScript authoring projects, and release/install identity checks use the matching digests. Carpet's Lichess TV authoring now requests bounded source-sequence capture for pre-rendered mode; live mode uses the latest admitted position. Actual helper playback, cancellation, source retirement and emitted-frame parity remain unqualified, so a single current position is not treated as a historical clip.
- Agent compilation instructions now require ni-vm, up to 16 cores per compilation, isolated remote `/data/` directories, scp artifact retrieval, and immediate task-owned remote cleanup.
- Dense wrapped Wind gust simulation now keeps runtime-dispatched AVX2/AVX-512 particle state in SoA form across frames, flushing to the existing dot layout at frame boundaries and retaining the portable scalar fallback.
- Wind rendering deduplicates dots that land on the same Braille subpixel and paints their flat raster indices directly, preserving raster ownership semantics.
