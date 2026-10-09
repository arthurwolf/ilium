# Changelog

All notable changes to ilium are recorded here, newest first. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

The workspace version is `0.1.1`.
The `v0.1.0` tag (2026-10-02, `6906553`) marks a release-pipeline qualification run
and is not an ancestor of `master`; its content is covered by the 2026-09-29 to
2026-10-02 sections. Pre-release development history is grouped by
development period. Dates are commit dates (`YYYY-MM-DD`).

## [Unreleased]

- Dense gusted Wind scenes now use runtime-dispatched SIMD integration when supported, with a portable scalar fallback and cached canonical particle state.
- Help keyboard summaries now adapt to the available dialog width and mark clipped descriptions.
- Confirmation dialogs now shorten their keyboard guidance to fit compact widths while retaining the Y/N, Enter, and Esc actions.
- Appearance sizing guidance now wraps to the available Settings content width without losing its wording.
- Animation output errors now report that terminal output could not be confirmed, and uncertain post-flush receipts retain their original presentation ownership until settlement is known.
- Saved-world preparation now estimates remaining time from measured overall progress and keeps elapsed time and measured stage ETA visible in the activity footer.
- Visible terminal subscriptions now replay retained output in bounded frames, rotating across selected panes so one large history cannot hold the other panes' first content behind it.
- A newer visible-pane selection now supersedes an older replay while it waits for storage admission, so stale history cannot delay content for the pane the user just selected.
- Terminal presentation now charges and requests an explicit 2 MiB native output-thread stack alongside its bounded encoder and resize storage.
- Worktree rollback now checks process users through bounded I/O admission and retains the capped evidence until it decides whether removal is safe.
- Session detection now alternates bounded transcript reads on the I/O bank with metadata decoding on the CPU bank; the coordinator alone reconciles ordered exclusive session claims.
- Sound Studio preview synthesis now checks cancellation during PCM generation, allowing stale edits to release the shared CPU lane before rendering the full sound.
- System-audio capture drops callbacks above 8,192 frames before allocating a downmixed buffer, bounding conversion work on the device callback thread.
- Spectrum analysis, helper-process readers and CPAL callback paths reserve shared ambient worker capacity; platforms without a declared CPAL thread budget refuse that backend, and unobservable CoreAudio roles stay charged through process exit.
- GPU scene workers now use shared physical-worker and frame-storage admission, cap outputs at four million pixels, and retire blocked device calls without blocking scene teardown.
- GPU capability probing now runs on an admitted ambient worker; existing scenes keep a stable runner source that resolves when the device is published, and client shutdown joins the original probe worker.

### Added

- Frozen agent panes show a configurable marker in the Icons settings, defaulting to a snowflake.

### Fixed

- Parser pool Off now removes the aggregate parser-memory ceiling; initialized panes remain available for faster revisits at higher RAM use, while an explicit positive budget retains finite pooling and hidden-pane eviction.
- Chatroom reference routing now acknowledges delivered records individually, avoiding replay of a completed prefix when a later record in the same bounded batch fails.
- Antigravity status-line restoration now recovers saved custom fields when `/statusline off` normalizes the setting and adds metadata, while preserving newly changed fields.
- The frozen-screen Unfreeze button and tree context-menu action now request restoration of the pane's saved agent session.

### Changed

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

### 2026-10-07

#### Added
- Minecraft landscape demo now preserves the first saved-world composites and adds a corrected historical ordered-dither mask reconstruction.
- Remote compaction settings detect each agent's own compaction trigger (`autoCompactWindow`, `CLAUDE_CODE_AUTO_COMPACT_WINDOW`, `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE`, the disable variables, Codex `model_auto_compact_token_limit`, project files), show the value in force next to the CLI default, and warn when it fires at or below the remote threshold so remote compaction would never start.
- Bounded worker execution foundation with shared CPU, I/O, storage and retirement admission for long-lived services and finite jobs.
- Startup sound scanning and audio-device enumeration run on the shared I/O pool, with admitted catalogue storage retained through UI use.
- Named agent references: chatroom `@name` mentions now route verbatim event lines to matching live agent panes, with each line submitted by a real Enter.
- Freeze agent panes: stop the agent process and keep its screen visible.
- Growth ambient scene and a `wind_performance` benchmark example.
- Animation plugin backend: saved-world factory, native world and presentation hosts, pinned `secure_fs` staging for plugin files.
- Generated native worlds can publish a bounded `terrain` model mesh to V8 as typed vertex/index planes.
- Bounded `host.gpu.render` mesh rasterization with quota-backed image handles, column-major cameras, optional per-vertex texture coordinates and explicit close custody.
- Image sampling and resizing can borrow an image registered by another native producer in the same animation instance; closing remains with the original producer.
- Native and Plugin animation settings tabs, bundled Beach and Carpet package catalogue entries, and actionable package issue reports.
- Live resume integration test for `ilium-remote-compaction`.
- Optional Agent Monitoring model icons from each Claude, Codex, or Antigravity CLI session's verified selected model, with provider-icon fallback when evidence is unavailable; Ilium safely manages and restores the Claude and Antigravity status-line bridges.

#### Changed
- Number controls now use one-cell ASCII `-` and `+` step buttons around a centered value; `*` opens direct keyboard entry.
- Local Linux Cargo builds default to four jobs with compiler affinity restricted to two physical cores; a launcher applies the same limit to the entire build process tree.
- Wind gust sampling now scales its cached field to terminal dimensions, limiting interpolation spacing to two cells for smoother motion.
- Wind particle integration caches per-dot inverse masses, removing repeated mass divisions from the per-frame integration paths.
- Wind diffusion prunes unreachable neighbor buckets with particle bounds, rejects distant pairs before square roots, and rebuilds mass caches in dot order when settings or population change.
- Wind reuses merge-count and glyph buffers across frames to reduce allocations when merged dots are enabled.
- Beach Rich scans overlapping wet bands by index instead of creating an array iterator for every raster dot; the pinned 320×152 field remains byte-identical.
- Tree and terminal context menus now share grouped actions, clearer labels, padded rows, aligned submenu indicators, themed selection, and scrollable short-screen navigation.
- Text Trigger previews compile and match on the shared CPU worker pool, with bounded revision-fenced results and the last completed preview retained while preparing.
- Audio-device enumeration limits device count and formatted-name storage, returning a typed error instead of publishing an incomplete catalogue when admission fails.
- Saved audio-device lookup reuses the same bounded name preparation and returns the original matched device in one pass; excessive scans or names fail explicitly.
- Sound discovery limits filesystem traversal to 65,536 entries across all sound roots, including unsupported files and directories, and reports incomplete scans in the existing truncation notice.
- Sound discovery caps retained sound paths, canonical deduplication identities, names and collection labels at 8 MiB of actual allocated text capacity, reporting partial catalogues when the limit is reached.
- Sound-root collection admits at most 64 candidates and 256 KiB of allocated path/label text, and rejects oversized Linux XDG directory lists before splitting; accepted roots still scan when other candidates are refused.
- GPU texture sampling requires the producer's published image registry entry, supports source and video image aliases, and preserves the producer's close ownership.
- Replay capture requests reject missing, nonpositive or unsafe byte limits before dispatch and validate native recording identities and digests.
- Animation compute jobs reserve scratch and output storage before CPU launch, avoiding storage-admission races with handle publication and refusing insufficient storage before publishing a job.
- Auto-freeze delay has centered numeric controls and exact whole-second entry, with the existing 15-minute arrow steps.
- Client shutdown retains unpublished request batches and their actual FIFO flush receipt across cancellation, reporting uncertain delivery without replaying accepted prefixes.
- Client shutdown also retains pending or failed flush receipts when the final batch contains no requests.
- Client worker-bank shutdown retains its actual owner on join failure or observer-construction panic, keeps canceled observation with the background cleanup owner, and preserves an earlier cleanup error alongside execution custody.
- Sound previews in settings and onboarding preserve request-admission refusal messages and report accepted previews as requested; control commands return refusals when no preview was queued.
- Remote Compaction numeric settings use centered controls with visible units and bounded exact keyboard entry while preserving their existing increment ladders.
- Server encoded output transfers finite encoder credit into bounded storage before socket flush; original frames and events retain admission until CPU retirement.
- Settings and animation controls now use shared centered numeric steppers (`− value + *`) and three-part selectors (`← value + →`); selector values advance with left click, reverse with right click, and the plus control opens the complete option catalog.
- Wind scene: simulation and settings improvements, mouse input handling.
- Beach Rich reuses a bounded per-viewport coordinate grid while preserving the exact default pixel field.
- Plugin diagnostics now retain bounded path-and-message details and expose them in a delayed hover popover; runtime script errors are surfaced in the same report.
- Plugin discovery failures before catalogue creation now appear in the same hoverable issue report instead of only a status row.
- Generated-world model requests reuse admitted prepared faces and refuse unknown or saved-source model names with structured errors.
- GPU mesh requests require an accepted `device.gpu` mesh grant and use the native image registry for returned pixels.
- Plugin mesh rendering applies the camera matrix and reserves geometry scratch storage; uncertain image-close delivery retains ownership until helper retirement.
- Minecraft scenes: world catalog, region, saved-scene and window handling updates; voxel landscape surface biomes, flora, geology and terrain fields.
- Saved-world animation preparation reports measured progress through region inventory, headers, chunk slots and payload decoding, with bounded activity history and ETA only after a measured stage rate is available.
- Saved-world preparation restarts its progress, elapsed clock, and route-specific activity log for each new viewport route, reports cumulative bounded route-survey work, and keeps selected-setting help visible while the log can be paged.
- Minecraft Overworld rivers now fill a connected water column to the authored surface while wetlands retain mixed wet and dry mangrove roots; riparian flora can admit water one cell above a bank.
- Minecraft Overworld terrain now includes deterministic elevated inland lakes, and ground cover forms seed-stable broad patches while retaining average admission across seeds.
- Overworld tree placement now uses seed-stable spatial patches to cluster wooded areas while preserving each biome's average tree cover.
- Compaction report, scan and UI fixes; cost and remote-compaction settings dialogs; value-choice settings rows.
- Cost and Stats lookup streams large Codex session stores and Claude sub-agent trees without collecting unrelated transcript paths, and reports measured discovery-limit failures.
- Client dialog host and worktree dialog cleanup.
- Voice Realtime connections cap incoming WebSocket messages and parsed JSON events at 8 MiB to bound per-event allocation.

#### Fixed
- Plugin issue popovers use a neutral heading for both package-inspection and runtime errors.
- Unfreezing resumes the session saved with a frozen pane even after its cached session ID is cleared; frozen panes have a separate configurable snowflake marker.

### 2026-10-04 to 2026-10-06

#### Added
- Remote compaction and compaction analysis crates (`ilium-remote-compaction`, `ilium-compaction-analysis`).
- Startup custody in the execution pool and client; runtime admission and snapshot I/O in the server.
- Smart copy selection, terminal input ownership, external open and a goal-resume link in the client.
- Minecraft world loading, session catalog, tours and saved scenes; extended JS animation host with a bootstrap presentation facade and native world regions.
- Animation visibility controls, settings rows and ridge cache.
- User documentation under `src/docs/`.

#### Changed
- Vendored `crossterm` 0.29.0 and `ratatui-image` 11.0.6.
- Release pipeline, installers, licence files and release workflow updated.
- Transcript files open through `secure_fs` and reject FIFOs.
- `ARCHITECTURE.md`, README and agent rules refreshed.

### 2026-10-02 to 2026-10-03

#### Added
- First-run guided setup, chirping notification sound and sound synthesis.
- Selectable quota-percent metric for agent cost.
- Shared look, dithering and a batch of new ambient scenes; carpet scene rewrite; voxel landscape split; live-data graph and fleet layers; chess feed; carpet snake planner; OpenStreetMap credit rows.
- Minecraft-backed voxel landscape: asset pipeline, native colour, biome and fluid pipeline, surface generation and geology; Minecraft directory discovery in `ilium-platform`.
- Semantic animation recommendations.
- Replacement panes carry over the conversation title.
- Text-trigger configuration persistence and faster PTY input.
- Ordered PTY owner, asynchronous PTY writer and crashed-agent recovery; agent prompt transcripts and recovery UI.

#### Fixed
- Clarified that the quoted Minecraft landscape request has no matching transcript in the local session archive.
- Source-free animation plugins resolve empty native permission plans without opening an empty consent screen; requested rights still require explicit review.
- The client declares capacity for the selected V8 helper using the same child and transport worker count as helper admission; retiring helpers still retain their shared quota charge.
- Untrusted animation resource requests open native selection before consent, then require a fresh permission review for the selected resource; selection alone never authorizes access.
- Remote-compaction technique choices use stable saved identifiers, and clicking an inert privacy-banner area retains the Settings screen.
- Plugin activation issue reports preserve native startup, authority and cleanup failure reasons instead of a generic refusal.
- Plugin issue popovers dismiss when the pointer leaves the settings content area or a click/editor takes ownership.
- Animation helper initialization failures now send their bounded error reason through the authenticated binary transport instead of disappearing into a generic pipe EOF.
- The headless release animation check binds each render context to its native frame seed, as required by the authenticated frame lifecycle.
- The headless release animation check loads each package's native permission ledger before broker preparation, including packages declaring optional capabilities, and explicitly retires its execution bank.
- Windows release lane: Visual Studio toolchain selection, `dumpbin` resolution, dependency review of system DLLs, deterministic tests, LF line endings pinned for release inputs, fsync of the staged archive.
- PTY suite stability under load; Windows client no longer writes the bottom-right cell.

### 2026-09-29 to 2026-10-01

#### Added
- `ilium-ambient`: stateful animated background scenes, including video, solar system (stars, horizon toggle, satellites), cloudlet and pond.
- `ilium-gpu`: optional wgpu rendering for fbm clouds.
- `ilium-prompts`: Handlebars prompt templates; all inline prompt text moved into it.
- `ilium-session-convert`: convert agent sessions between Claude Code and Codex, with a UI.
- Agent cost indicators, completed-progress expiry and in-group cycling keys in the client.
- Custom LLM instructions in Settings and feature tabs.
- Linux packages, Windows installers and a native release pipeline; native installer commands in the docs.

#### Fixed
- Detection: Claude goal history, background-wait completion, completed-turn summary as an activity boundary, macOS command-line refresh per tick.
- Ambient background paints over inkless agent TUI blanks.
- Release pipeline: glibc-compatible Linux ORT runtimes, first-publication handling of an empty GitHub release channel and unconfigured Pages, Windows MSVC toolset selection, macOS audit, licence declarations for the GPU dependency closure.
- Test portability and stability on macOS and Windows.

### 2026-09-25 to 2026-09-28

#### Added
- Server-side text triggers with screen-state deduplication; absence tracked from the observed erase.
- Selectable title styles for AI-authored pane titles, including a labelling style driven by request history.
- Costs-and-stats popover for agent sessions.
- `ilium voice say` types sentences into a live voice session.
- Pane state signals, goal control and progress-outcome attention; separate agent turn, goal and unread-completion state.
- Agent worktree lifecycle and pane state integration.
- Progress monitor lifecycle v2, agent setup and session backups.
- Configurable agent monitoring controls; reset planning; chatroom limit preview.
- Agent coordination instructions and Codex chatroom hooks; process-custody rule in agent instructions.
- Kilo Gateway paid proxies loaded from MongoDB at client boot.
- Accepted feature demonstration recordings, demo gallery and refreshed README.

#### Fixed
- Agent goal state read only from provider status rows.
- Tree row repaint after width changes (VS16 emoji).

#### Changed
- Several commits in this period were committed under the message `stuff`; their content is the reset planning, smart copy, agent monitoring, status icons, render cache, README and demo work listed above.

### 2026-09-21

#### Added
- Expanded terminal workflows and runtime hardening.

### 2026-08-22 to 2026-08-26

#### Added
- Last-prompt banner per pane, with transcript fallback, content-sized word wrapping.
- Lock-closed gesture for folders, projects and groups with persisted expand/collapse state.
- Hidden paid-proxy egress option for Kilo Gateway calls.
- Server-run progress monitor for long-running tasks in a pane.

### 2026-08-17 to 2026-08-21

#### Added
- Performance audit: four optimization passes across client, server and IPC.

#### Fixed
- Claude Code panes no longer report Done while a background shell still runs.
- Whole-workspace audit, one commit per crate: arrow-moves stay in the current project (`ilium-core`); Codex live-status-line matching (`ilium-detect`); private-file writes and process control (`ilium-platform`); endpoint, stream and Windows accept-loop errors (`ilium-transport`); trailing bytes rejected on frame decode (`ilium-ipc`); mouse-encoding cap, query flooding and reaper races (`ilium-pty`); no default OpenAI base URL (`ilium-inference`); PCM16 alignment and release flushing (`ilium-voice`); sound path passing on Windows (`ilium-sound`); restore ordering (`ilium-agent-debug`, `ilium-agent-session`); write-buffer clearing (`ilium-logging`); detection, persistence and lifecycle races (`ilium-server`); IPC connection lifecycle, HTTP API and title parsing; board serialization, markdown rendering, editor path handling, voice and control validation, terminal selection, links and mouse edge cases, naming and icon worker panics (`ilium-client`).
- Destructive session commands guard against a session-name mismatch.

### 2026-08-03 to 2026-08-13

#### Added
- Cross-platform CI gate with a pinned toolchain; Windows end-to-end TUI coverage.
- `ilium-platform` crate (every OS-specific decision in one place) and `ilium-transport` (session transport: Unix domain socket on Unix, named pipe on Windows); client, server and CLI moved onto them.
- Cross-platform fixture programs for the fake agent CLIs.
- OS-level open-externally action, agent-pane action toolbar with text labels and distinct Claude model icons.
- Local terminal text selection with its own setting; auto-answer of Claude Code's "resume full session" dialog.
- Buffered terminal writer in the client.

#### Fixed
- macOS: socket path, process lookup, descriptor tables, `XDG_RUNTIME_DIR`, script-launched agents.
- Windows: path slugs, extended-length paths, process termination, ConPTY reads, shell availability, footer pinned to the bottom row, line wrapping off for the TUI session.
- Detection loop survives a panicking tick; native agent process preferred over an interpreter wrapper.
- Manual retitle click is queued instead of dropped while a worker is busy.
- Snapshot kill state is atomic; snapshot permissions use `secure_fs`.
- Machine-local identity removed from the tree before publishing.

#### Changed
- Published docs split from design notes; crate metadata prepared.
- Many tests now report server reasoning, process rows and frame geometry on failure.

### 2026-07-12 to 2026-07-29

#### Added
- Project created as a workspace refactor into layered crates: `ilium-ipc` (wire types, bincode framing), `ilium-pty`, `ilium-detect` (agent identity and activity classification), `ilium-server` (owns tree and PTYs, adaptive detection loop, UDS IPC), `ilium-client` (ratatui TUI), and a slim `ilium` CLI.
- Drag-and-drop and indent/outdent reparenting (`ReparentNode` over IPC).
- Snapshot respawn on server startup; crash-recovery snapshots.
- Desktop notifications for Working to Done/Idle transitions.
- Config surface: custom detection signatures, keybinding remap, theme colours.
- PTY-driven TUI smoke test and live agent-detection end-to-end test.
- Renamed to `ilium` (2026-07-13); split views and sound settings; agent icons; create-from-line; keyboard remap.
- `ilium-inference`: provider-neutral LLM boundary for naming; tree restructure engine; new-pane cwd policy; session recovery prompt; settings tabs; clickable OSC-8 hyperlinks.
- Project-scoped tree workspace controls; icon customisation with CPU-only semantic icon search; visual keybinding remapper with presets; bracketed paste; completion-driven prompt queue.
- Realtime voice control and dictation; inferred title icons; stacked modals; configurable automatic LLM triggers.
- Session diagnostics, persistent agent diagnostics and state fencing; provider payload diagnostics in debug logs.
- Project chatrooms, configurable tree navigation, mouse-operable dialogs, terminal and editor context actions; sequence-aware terminal replay with pane-scoped lag recovery; deferred initial agent prompts.
- Agent orchestration expansion; cleared agent panes move to the project root.
- Controlled tmux session recipe for TUI automation documented.

#### Fixed
- Socket path overflowing `sockaddr_un.sun_path`.
- Confirmation and selection prompt false positives in detection; `WaitingApproval` poll cadence.
- Session-ID detection tier removed because it trusted inherited environment variables.
- Snapshot temp-file cleanup; re-save after a failed pane restore.
- PTY, server task and worker lifecycle leaks.
- State validation before persistence; bounded replay and debug histories; PTY, socket, shutdown and snapshot races.
