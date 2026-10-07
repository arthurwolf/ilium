# Changelog

All notable changes to ilium are recorded here, newest first. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

ilium has not yet published a release from `master`. The workspace version is `0.1.0`.
The `v0.1.0` tag (2026-10-02, `6906553`) marks a release-pipeline qualification run
and is not an ancestor of `master`; its content is covered by the 2026-09-29 to
2026-10-02 sections. Until a release is cut from `master`, history is grouped by
development period. Dates are commit dates (`YYYY-MM-DD`).

## [Unreleased]

### 2026-10-07

#### Added
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

#### Changed
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
- Plugin diagnostics now retain bounded path-and-message details and expose them in a delayed hover popover; runtime script errors are surfaced in the same report.
- Plugin discovery failures before catalogue creation now appear in the same hoverable issue report instead of only a status row.
- Generated-world model requests reuse admitted prepared faces and refuse unknown or saved-source model names with structured errors.
- GPU mesh requests require an accepted `device.gpu` mesh grant and use the native image registry for returned pixels.
- Plugin mesh rendering applies the camera matrix and reserves geometry scratch storage; uncertain image-close delivery retains ownership until helper retirement.
- Minecraft scenes: world catalog, region, saved-scene and window handling updates; voxel landscape surface biomes, flora, geology and terrain fields.
- Minecraft Overworld rivers now fill a connected water column to the authored surface while wetlands retain mixed wet and dry mangrove roots; riparian flora can admit water one cell above a bank.
- Minecraft Overworld terrain now includes deterministic elevated inland lakes, and ground cover forms seed-stable broad patches while retaining average admission across seeds.
- Overworld tree placement now uses seed-stable spatial patches to cluster wooded areas while preserving each biome's average tree cover.
- Compaction report, scan and UI fixes; cost and remote-compaction settings dialogs; value-choice settings rows.
- Client dialog host and worktree dialog cleanup.

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
