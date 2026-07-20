# Claude and Codex `/clear` identity reset

- [x] Define one provider-owned `/clear` transition contract for Claude and Codex, while leaving unrelated agents and non-exact input unchanged.
- [x] Atomically discard the detected session ID and every existing pane-title field, then show `<new>` from the detached server's authoritative input path.
- [x] Cover exact input reconstruction, manual-title replacement, stale title-worker rejection, and both provider lifecycles with focused regressions.
- [x] Run workspace gates, rebuild/install, and verify the cleared identity and `<new>` title through the real installed TUI.

# Agent debug log resize-noise filter

- [x] Carry an explicit resize cause from every client layout mutation through IPC into the persisted per-agent event contract.
- [x] Add a default-enabled top-toolbar filter that hides only left-panel focus/hover animation resize events while preserving genuine terminal, split, and settings resize evidence.
- [x] Apply the same active filter to saved human-readable debug logs and keep the unfiltered retained journal available when the filter is disabled.
- [x] Add focused model, protocol, server, client, rendering, keyboard, mouse, and export regressions for provenance and default filtering.
- [x] Run workspace gates, rebuild/install, and verify the filter against a real installed TUI while the left panel expands and contracts.

# Agent debug log signal and export

- [x] Record detection decisions only when their meaningful identity, activity, goal, or session evidence changes; never mutate a retained entry merely because another poll ran.
- [x] Replace detector internals and counters with plain-language conclusions that explain what ilium decided and which stable process/screen/session evidence justified it.
- [x] Add a top-of-log Save action, editable destination-path prompt, and complete human-readable file export with keyboard and mouse coverage.
- [x] Run focused and workspace gates, rebuild/install, and verify change-only logging plus saved-file output in an isolated real TUI.

# Persisted per-agent debug history

- [x] Define the typed, extensible per-pane event contract in a pure shared crate, including ordered timestamps, levels, categories, summaries, and structured detail fields.
- [x] Persist each agent pane's complete debug history inside the canonical project session snapshot, restore it across restarts, mirror it over IPC, and clean it up with the owning pane.
- [x] Instrument agent detection and session-identity discovery with phase-by-phase evidence plus status, goal, execution, prompt, scheduled-input, queue, focus, resize, trigger, LLM-title, persistence, and error events.
- [x] Add a disabled-by-default User Interface setting, synchronize it with the detached server, and enable it for the current user without changing the default for new installations.
- [x] Add the detected-agent-only right-click menu and an aerated full-right-panel debug timeline with icons, European timestamps, structured details, scrolling, and exact return navigation.
- [x] Add focused core, IPC, server, persistence, client configuration, rendering, keyboard, and mouse regressions, including old-snapshot compatibility and disabled-capture behavior.
- [x] Run focused and workspace gates, rebuild/install, prove installed-binary parity, and verify capture, menu visibility, full-panel rendering, and restart persistence in an isolated tmux-controlled runtime.

# Voice-mode self-stop tool

- [x] Add a dedicated typed `ilium_stop_voice_mode` function tool and explicit model instructions for stop, disable, turn-off, and end-voice requests.
- [x] Return the final Realtime `function_call_output` before cleanly shutting down the owned voice actor, persisting the disabled setting, and resuming any media paused for voice mode.
- [ ] Add focused protocol/control/event-loop regressions, run workspace gates, rebuild/install, and prove the real OpenAI model selects the stop tool and the installed runtime transitions from listening to disabled.

# Session-scoped debug file logging

- [x] Replace the always-on project log with disabled-by-default, timestamped per-server logs under `/tmp/.ilium/<session-id>/` and a live shared client/server logging boundary.
- [x] Instrument major session, IPC, settings, worker, HTTP, LLM, and OpenAI Realtime actions, preserving complete text request/response/error diagnostics while redacting credentials and summarizing binary audio payloads.
- [x] Add a live-persisted Debug settings tab and synchronize its logging toggle with the current detached server.
- [x] Add focused coverage for path generation, disabled/enabled behavior, persistence, IPC, UI, HTTP error bodies, and LLM payload diagnostics.
- [x] Enable logging in this installation, run workspace gates, rebuild/install, and prove the installed TUI plus generated log file through an isolated runtime session.

# Voice command Enter submission invariant

- [x] Make immediate voice text delivery a typed submission that always appends Enter, with no model-controlled opt-out.
- [x] Keep explicit type-without-submitting behavior behind its own tool while scheduled and queued delivery preserve the submission invariant and confirmation can stage internally.
- [x] Add focused regressions, run workspace gates, rebuild/install, and prove the updated voice path against the real runtime boundary.

# Automatic LLM title and structure triggers

- [x] Define a typed, persisted event-to-actions contract with explicit scope rules, safe deduplication, and reasonable defaults.
- [x] Add precise startup, prompt-submission, and semantic agent-lifecycle event seams without duplicating the sound/detection logic.
- [x] Route trigger actions through the existing single-element retitle and project/all-project restructure worker boundaries.
- [x] Add a dedicated, aerated Triggers settings tab with responsive multi-select action chips, keyboard navigation, mouse hit testing, live persistence, and clear safety/context copy.
- [x] Cover configuration, event routing, concurrency guards, rendering, keyboard/mouse behavior, and IPC round trips with focused regressions.
- [x] Run workspace gates, release-install, and controlled installed-TUI verification of the settings and automatic trigger flows.

# Full OpenAI Realtime voice control

- [x] Add a provider-neutral, owned voice runtime with OpenAI Realtime WebSocket transport, microphone capture, speaker playback, interruption, and clean shutdown.
- [x] Add a typed semantic control plane, redacted UI snapshots, complete tool registry, target resolution, deterministic confirmation policy, deduplication, and structured results for every ilium surface.
- [x] Integrate voice events into the client event loop without making `App` async or coupling the provider/audio layers to ratatui, IPC, or ilium domain types.
- [x] Add a live-persisted Voice control settings tab with masked OpenAI key editing, model/voice/reasoning/VAD/audio controls, and multiline custom prompt text.
- [x] Add the bottom-right voice switch beyond the purple status bar, with a large red enabled/recording dot and black disabled dot, plus keyboard and mouse interaction.
- [x] Cover the voice protocol, audio conversion, tool schemas, command execution, security boundaries, settings persistence, bottom control, and full UI capability surface with focused and PTY tests.
- [x] Run workspace tests, strict Clippy, formatting, release build/install, and verify the installed TUI plus a real OpenAI Realtime voice session, including a model-issued function call and accepted tool-result follow-up.
- [x] Make focused-pane and focused-agent dictation the explicit primary workflow, including the exact "send /clear to the currently open terminal" mapping.
- [x] Add a live-persisted `Confirm terminal submissions` Voice setting that defaults off while leaving destructive-action confirmations independent.
- [x] When terminal confirmation is enabled, stage dictated text visibly without Enter, ask whether to submit what is on screen without reading it aloud, and press Enter only after yes.
- [x] Add protocol, prompt-contract, tool-schema, control-plane, and terminal-dispatch regressions for dictated `/clear` and related forwarding phrases.
- [x] Run workspace gates, release-install, and live Realtime verification proving representative dictated forwarding requests reach the installed TUI's active pane in both immediate and staged-confirmation modes.

# Performance and interaction responsiveness (20 improvements)

- [x] Disable sysinfo's machine-wide `/proc/*/stat` descriptor cache before every server detection lifecycle.
- [x] Right-size each detached server's Tokio worker and blocking pools instead of multiplying one thread per CPU by every session.
- [x] Replace the detection loop's one-second polling tick with an exact nearest-pane deadline.
- [x] Wake detection immediately when focus, Enter, or a new pane forces an earlier check.
- [x] Capture VT screen text only for process trees that actually contain a detected agent.
- [x] Skip transcript/session rediscovery while the same verified agent process still owns the pane.
- [x] Refresh command and cwd process fields only for panes that genuinely need session discovery.
- [x] Resolve project paths only for those discovery candidates instead of every classified pane.
- [x] Short-circuit `/proc/<pid>/fd` transcript discovery as soon as ownership becomes ambiguous.
- [x] Fast-path already-equal process/project paths before filesystem canonicalization.
- [x] Replace lossy `try_send` client requests with a bounded, lossless staging queue.
- [x] Coalesce superseded queued PTY resize requests per pane.
- [x] Coalesce superseded queued focus-state requests per pane.
- [x] Cap client-side merged terminal bytes per event-loop turn as well as event count.
- [x] Rate-limit terminal-output redraw floods while keeping input-driven redraws immediate.
- [x] Flatten the sidebar tree once per ordinary render and reuse the result for motion plus scrollbar geometry.
- [x] Reuse one row-motion cell scratch allocation across every animated sidebar row.
- [x] Make bounded OSC-8 hyperlink eviction O(1) with a deque.
- [x] Schedule pending-input countdown redraws at their real 220 ms frame boundary instead of the generic 20 Hz animation tier.
- [x] Lower CPU priority for workspace-search and semantic-icon background workers so they cannot compete with input/render work.
- [x] Run focused regressions, workspace gates, release-install, descriptor/CPU measurements, and installed PTY responsiveness proof.

# CPU semantic icon search

- [x] Replace icon-name/category substring filtering with direct local dense-vector retrieval.
- [x] Own one CPU-only MiniLM ONNX model and one normalized vector per catalogue icon in a background worker, persisting the completed vector matrix for future launches.
- [x] Keep official UTF-8 semantic result chapters ahead of Nerd Font chapters, reject stale queries, and expose loading/failure state in the picker.
- [ ] Run focused tests, workspace gates, release-install, and prove semantic retrieval through the installed TUI.

# Settings toolbar click lifecycle

- [x] Trace the full settings-toolbar mouse press/release path and reproduce the premature close through the real TUI.
- [x] Preserve the active interaction mode while workspace-search maintenance checks for due work or receives a late result.
- [x] Add focused and PTY regressions for press, release, maintenance ticks, and persistent settings visibility.
- [ ] Run workspace gates, rebuild/install, and verify the installed binary through the real PTY/TUI.

# Responsive workspace search

- [x] Keep query edits and native-cursor rendering on the immediate TUI path.
- [x] Debounce workspace scans for one second and execute retained-history matching in one tracked background worker.
- [x] Reject stale worker results by query revision and preserve exact result navigation.
- [x] Run focused tests, release build/install, and verify the delayed search and visible cursor in an installed PTY.

# Workspace search

- [x] Search every attached terminal's retained output history and every open editor buffer from one full-screen command center.
- [x] Add the sidebar search affordance, tree context-menu entry, result metadata, highlighted context, and direct result navigation.
- [x] Cover terminal history, open-file matching, keyboard/mouse navigation, and exact jump behavior; rebuild/install and verify the full-screen UI with the installed binary under a real PTY.
- [ ] Re-run workspace gates after the unrelated board/tree-transition test and strict-Clippy failures in the current dirty workspace are repaired.

# Interactive Kanban navigation and editing

- [x] Add a persisted 20-column default minimum width and horizontal board viewport whose render, scrollbar, keyboard, and mouse geometry share one source of truth.
- [x] Remove redundant card labels and render visible drag source/drop-target feedback throughout a card drag.
- [x] Make the aerated detail panel edit card title/body with immediate file or folder-backend persistence, including clickable persisted task checkboxes.
- [x] Cover the new settings, layout, editing, checkbox, and drag behavior with focused and real PTY tests.
- [x] Run workspace gates, rebuild/install, and verify every interaction through tmux with the installed binaries.

# Kanban card previews and detail panel

- [x] Add a validated, live-persisted Kanban Board setting for 1-10 preview lines with a default of 3.
- [x] Render contiguous multi-line cards and a click-open, one-third-width full-detail panel from shared board geometry.
- [x] Cover settings persistence, layout, rendering, mouse opening/closing, and detail scrolling with focused tests.
- [x] Run workspace gates, rebuild/install, and verify the settings and card-detail flow through the real PTY/TUI.

# File-backed board runtime repair

- [x] Reproduce board creation and operation through the installed binary in an isolated tmux-controlled project.
- [x] Fix every file-backed board interaction defect found by the runtime audit without changing the folder backend.
- [x] Add focused regression and end-to-end PTY coverage for the repaired workflow.
- [x] Run workspace gates, rebuild/install, and verify create, edit, move, reload, and reattach through tmux against the Markdown file.

# Sidebar close successor selection

- [x] Reconcile a removed selection against the sidebar's visible, configured ordering.
- [x] Activate the next surviving row below the closed item, with a previous-row fallback at the end of the list.
- [x] Cover pane, subtree, and automatic-order cases with focused regressions.
- [ ] Run workspace gates, verify the real TUI close flow, rebuild, and install.

# Codex initial-attach scrollback

- [x] Reproduce the missing wheel scrollback and scrollbar with a real resumed Codex session inside an isolated tmux-controlled ilium instance.
- [x] Add a per-connection attach barrier so live terminal output cannot overtake and then be erased by terminal replay.
- [x] Cover the attach ordering and retained-history behavior with focused regressions, then run workspace gates.
- [x] Rebuild, install, and verify the original resumed-Codex flow in the real TUI.

# Markdown-backed board creation fixes

- [x] Expose create-board-from-Markdown on a Markdown editor pane's left-tree context menu.
- [x] Load existing Markdown headings and list items into a newly created file-backed board without overwriting the source file.
- [x] Add focused regressions for context-menu eligibility, request routing, Markdown import, and tree-snapshot hydration.
- [x] Run workspace gates, verify both flows in the real TUI, rebuild, and install.

# Client-only restart from the tree menu

- [x] Add an explicit client exit intent and expose Restart in every tree right-click menu.
- [x] Re-exec the captured client executable with only the current project/session arguments, never a server restart/reset flag.
- [x] Cover the menu, exit contract, and reconstructed invocation with focused regressions.
- [x] Run workspace gates, prove the executable is reloaded while the detached server PID survives, rebuild, and install.

# Correctly rendered tree row emoji actions

- [x] Preserve the original emoji row actions while emitting every fixed-width slot as one atomic terminal run.
- [x] Cover glyph width, atomic buffer diffs, complete strip painting, action ordering, and click geometry.
- [x] Run workspace gates, verify the real TUI interaction, rebuild, and install.

# Strict agent session-ID provenance

- [x] Centralize Claude/Codex transcript identity and project-ownership validation.
- [x] Replace permissive session-ID ranks with class-specific, project-clamped evidence and remove guess-only fallbacks.
- [x] Reject corrupted cross-project resume bindings during snapshot restore and make fresh Claude IDs explicit at launch.
- [x] Cover every discovery rank, duplicate/class-change clearing, cross-project restore, and title lookup with regression tests.
- [x] Run focused and workspace gates, verify the live money/ilium isolation path, rebuild, and install.

# Sidebar tree entry transitions

- [x] Add client-owned, eased insertion/removal transition state at the render-cache boundary.
- [x] Render removals sliding left and additions sliding right into place before the existing creation blink.
- [x] Cover sequencing, timing, buffer movement, and snapshot reconciliation with focused tests.
- [x] Run workspace gates, verify the real TUI interaction, rebuild, and install.

# Sidebar tree ordering

- [x] Add a validated, persisted client-side tree-order setting and pure recursive ordering rules for Manual, Type, Age up/down, and Name A-Z/Z-A.
- [x] Add the checked Order by context submenu and the live User Appearance settings control.
- [x] Make every manual arrow, drag/drop, and keyboard move restore Manual ordering, with focused regression coverage.
- [x] Run focused and workspace gates, verify the real TUI interaction, rebuild, and install.

# vt100 wide-character resize crash

- [x] Vendor the unreleased upstream wide-character resize fix at the shared Cargo dependency boundary.
- [x] Cover the exact 216-to-215-column erase panic through both the PTY reader and client render cache.
- [x] Run the vendored-crate, targeted, workspace test, Clippy, and formatting gates.
- [x] Rebuild and install the release binaries, restart this project session, and verify the fix is live.

# Scheduled pane input countdown

- [x] Add the durable scheduled-input domain/IPC contract and detached-server executor.
- [x] Add the pane-only right-click action and aerated hours/minutes/seconds, text, and Enter dialog.
- [x] Render a human-readable countdown plus reverse clock animation before the pane title.
- [x] Cover validation, replacement, restore, execution, rendering, keyboard, and mouse behavior.
- [x] Run workspace gates, verify the real PTY/TUI flow, rebuild, and install.

# Configurable agent identifiers in the tree

- [x] Add validated `[ui]` settings for full-name, initial, chosen-icon, or hidden agent identifiers.
- [x] Add Claude and Codex icon selectors to User Appearance with live persistence.
- [x] Apply the choices to every agent state without disturbing activity indicators or row alignment.
- [ ] Add focused coverage, run workspace gates, verify the live TUI, rebuild, and install. (PAUSED: superseded by the newer explicit scheduled-input feature request.)

# Tree row-action clarity and icons

- [x] Clear the complete row-action strip so title characters cannot bleed through icon gaps.
- [x] Use stable single-cell edit, up, down, remove, and refresh controls in the title-replacing hover overlay.
- [x] Cover rendered gaps and hit geometry, run workspace gates, verify the installed TUI in a real PTY, rebuild, and install.

# Sidebar selected-row wide-glyph rendering

- [x] Diagnose the selection and hover-overlay width failures around multi-cell icons without changing sidebar selection semantics.
- [x] Add buffer and installed-PTY regression coverage, run workspace gates, and verify the installed release in an isolated tmux session.

# Normal row-action icon rendering

- [x] Restore normal UTF-8 row-action icons as the default and emit each icon plus its cleanup spaces as one terminal run.
- [x] Add the opt-in `Use stable glyphs` User Interface preference, defaulting to off and persisting it in `[ui]`.
- [x] Cover the rendering run, setting persistence, PTY interaction, workspace gates, and installed-binary parity.

# Waiting-background clock animation

- [x] Replace the half-circle frames with the complete half-hour clock emoji sequence.
- [x] Keep waiting-background redraws active and cover frame order, width, and animation-state behavior.
- [x] Use the requested settings, text-editor, and terminal icons throughout the left panel.
- [x] Run workspace gates, verify the animation and icons in the real TUI, rebuild, and install.

# Create agent from editor line

- [x] Add source-line right-click hit testing and a dedicated editor-line context menu.
- [x] Add the Claude/Codex creation dialog with an editable prompt textarea and mouse/keyboard controls.
- [x] Add atomic command-pane creation with initial prompt submission and Enter.
- [x] Cover the interaction and IPC/runtime path, run workspace gates, manually verify the real TUI, rebuild, and install.

# Sound settings and event alerts

- [x] Add a cross-platform sound adapter with system sound discovery, system beep, file playback, and pure event-transition rules.
- [x] Persist sound settings and apply live changes to the detached server over IPC.
- [x] Add the Sound settings tab with source/file selection, previews, discovered folders/files, and event checkboxes.
- [x] Add focused coverage, run workspace gates, manually verify the real TUI and sound playback, rebuild, and install.

# Split view containers

- [x] Implement the persistent split-container domain model, invariants, IPC request, server mutation, and persistence coverage.
- [x] Implement shared client viewport geometry, multi-pane rendering, per-slot sizing, focus, and input routing.
- [x] Add split creation dialogs, shortcut, tree toolbar/context integration, and tree presentation.
- [x] Run focused tests, full workspace checks, and live PTY/TUI verification.

# Comprehensive icon catalogue

- [x] Replace the short hand-written picker list with the complete named Unicode emoji catalogue, retaining terminal-friendly quick picks.
- [x] Present the 3,500+ choices in detailed CLDR categories, including animals, signs, mathematics, computer, travel, tools, food, flags, and more.
- [x] Add responsive category/grid navigation, live name/category search, and matching mouse hit testing.
- [x] Cover catalogue size/category/search and picker geometry; run client, PTY, formatting, Clippy, release-install, and installed-TUI verification.

# Triple-size icon catalogue

- [x] Add the complete Nerd Font icon families alongside the Unicode catalogue, preserving the detailed category/search model.
- [x] Prove the picker has at least three times the original 3,560 choices and still works in the installed TUI.

# Official UTF-8 and Nerd Font catalogue boundary

- [x] Keep portable official UTF-8 categories first and Private Use Area Nerd Font categories second in the catalogue data contract.
- [x] Make the picker show separate family counts and label every category/grid with its family.
- [x] Cover the ordering invariant and run client tests, PTY settings verification, formatting, Clippy, release build, and install.

# Chaptered icon catalogue picker

- [x] Replace the category-sidebar/glyph-grid picker with one official-first chapter document containing aligned icon-and-description cells.
- [x] Add an in-place search field that filters icon cells and removes chapters with no matching icon.
- [x] Cover document ordering, filtering, rendering, keyboard/mouse selection, and verify the installed TUI through an isolated PTY.

# Icon catalogue viewport performance and rendering integrity

- [x] Identify the repeated full-catalogue rebuild and the grapheme-splitting source of leaked emoji fragments from the reported screenshot.
- [x] Cache filtered search results, render only viewport rows, and add keyboard, wheel, page, and scrollbar-track scrolling.
- [x] Prove fast large-catalogue traversal and clean open/scroll/close repainting in the installed TUI, then run workspace gates. (The full workspace test run has one unrelated folder-browser failure in the concurrent project-tree work; the icon PTY test and workspace Clippy/format gates pass.)

# Icon catalogue density switch

- [x] Map the existing viewport-only catalogue renderer, selection geometry, and mouse scrolling contract.
- [x] Restore efficient multi-column rendering as the default while retaining the reliable one-column mode.
- [x] Add a top-right view switch with keyboard and mouse control, then cover and verify both modes in the installed TUI.

# Keyboard shortcut base and sidebar settings access

- [x] Add a persisted Keyboard settings tab with recommended Ctrl+A and Ctrl+B presets plus custom Ctrl+A-Z selection.
- [x] Apply the selected base immediately to shortcut dispatch and every Help shortcut label.
- [x] Explain the conflict for every custom letter and cover terminal-equivalent Ctrl+I/Tab and Ctrl+M/Enter input.
- [x] Add the right-aligned sidebar settings gear, keep footer icons visible while the tree is focused, and verify the complete flow in a real PTY/TUI.

# Mouse interaction

# Folder browser nodes

- [x] Add a persisted folder-root node and folder-only selector.
- [x] Render local files/directories as virtual sidebar descendants and open files in editors.
- [x] Cover domain, client behavior, workspace checks, and installed-binary verification.

# AI project names

- [x] Add a Kilo Gateway client and deterministic project-name inference workflow.
- [x] Persist inferred project names in `.ilium/config.yaml` and render them in the sidebar title.
- [x] Verify no-call-on-existing-name behavior, retry handling, and full workspace checks.

- [x] Define mouse interaction model and shared layout/hit-test contracts.
- [x] Implement tree clicks, context menu, drag/drop, and terminal mouse forwarding.
- [x] Add focused tests and update interaction help.
- [x] Run full verification, rebuild release binary, and install it.

# Tree hover controls

- [x] Design hover controls, toolbar hit regions, and cross-group move semantics.
- [x] Implement bottom creation toolbar and per-row move arrows.
- [x] Test interaction logic, rebuild, and install.

# Agent process metadata

- [x] Remove the agent-icon panel and restore the two-panel layout.
- [x] Add agent PID/session metadata detection and selected-pane title rendering.
- [x] Verify with tests, rebuild release binary, and install.

# Syntax highlighting

- [x] Pick a highlighting engine: `syntect` + `two-face` (bat's bundled Sublime syntax/theme sets) instead of hand-rolled lexers.
- [x] Add `syntax.rs`: per-line token highlighting (`highlight()`), extensionless-filename + extension lookup, foreground/bold/italic/underline only (no background, to avoid a colored box over ilium's dark chrome).
- [x] Add `editor_highlight.rs`: Source-mode renderer recomposing `ratatui_textarea`'s line-number gutter/cursor/current-line/selection styling with per-token syntax color patched underneath (that crate exposes no per-token styling hook, hence the custom renderer) -- falls back to the plain `TextArea` widget for unrecognized languages.
- [x] Wire into `EditorPane` (`content_revision`-cached `highlighted_lines`, horizontal scroll mirror) and `ui::draw_editor`.
- [x] Cover Rust/JS/TS/Python/Markdown/Makefile with tests (recognition, contiguous token coverage, multi-color proof, rendered-buffer cursor/gutter/color assertions); `cargo clippy`/`cargo fmt` clean.

# Markdown rendered-mode spacing

- [x] Reproduce the parser's loss of source blank lines and trace its effect through rendered height and scrolling.
- [x] Preserve source blank lines explicitly across Markdown parsing, rendering, and viewport layout.
- [x] Cover paragraph, heading, list, code-block, multiple-blank-line, wrapping, and scrolling behavior with regression tests.
- [x] Run focused and workspace verification, manually validate the TUI, rebuild the release binary, and install it.

# AI session titles

- [x] Add `transcript_context.rs`: locate the project-verified JSONL transcript, extract separate recent user, assistant, and tool-result windows in chronological order, and compact every dynamic context value independently to its first/last 1,000 Unicode characters.
- [x] Add `session_naming.rs`: XML/Handlebars prompt template combining the current title, pane/session/process/project/transcript metadata, activity/goal state, terminal screen, and typed transcript entries; validate icon plus 2-3-word short and 5-7-word long titles.
- [x] Add a `TitleSource` (`Auto`/`User`) flag to `SavedNode::Terminal` in `workspace_file.rs` so a user-typed rename is never overwritten by auto-inference, and a restored pane never re-triggers inference for a title it already has.
- [x] Wire `App::panes_needing_title_inference`/`mark_title_inference_started`/`apply_inferred_title`/`fail_title_inference` and per-pane worker orchestration in `main.rs` (mirrors the project-name worker, generalized to a map).
- [x] Render the shared braille spinner in place of a pane's name in `tree_ui::pane_label` while its title inference is in flight.
- [x] Cover with unit tests; `cargo clippy --workspace --all-targets` and `cargo fmt --check` clean on all touched files.

# Animated tree-panel width

- [x] Add a deterministic eased width animation and dynamic shared layout geometry.
- [x] Drive expansion from tree hover or keyboard focus, and collapse on pane focus.
- [x] Cover focus, hover, reversal, terminal-width limits, and shared hit-testing with tests.
- [x] Run workspace verification and manually validate the animation in the real TUI.

# Shell command titles

- [x] Persist automatic versus user-specified pane-title ownership in the core tree.
- [x] Use completed input only while the original shell owns the foreground PTY, excluding agents and foreground applications.
- [x] Verify cross-client updates, manual-name opt-out, formatting, clippy, workspace tests, release build, and local installation.

# Recursive folder browser repair

- [x] Trace virtual-row selection and expansion through the tree widget's full identifier-path contract.
- [x] Materialize folder descendants lazily along expanded paths and preserve valid virtual expansion state across snapshots.
- [x] Add nested folder/editor-open regression coverage and prove the complete interaction in an isolated tmux session.
- [x] Run focused and workspace verification, rebuild, and install the repaired binaries.

# Folder-picker mouse selection repair

- [x] Make a repeat click on a visible folder choose that folder rather than the picker's current directory.
- [x] Cover folder-overlay double-click selection and retain real PTY recursive folder coverage.
- [x] Run focused and lint gates, rebuild/install, and prove the installed binary through the real PTY folder workflow.

# Top-level projects and project-scoped AI restructure

- [x] Add a persisted, top-level-only project container with a canonical project directory and migrate existing session trees into their launch project.
- [x] Route entry creation, pickers, moves, folder changes, and project actions through project ownership/cwd rules.
- [x] Replace whole-workspace AI restructuring with concurrent, project-scoped workers, per-project undo, aggregate status, and per-project recycle controls.
- [x] Cover domain/IPC/client behavior, then run workspace gates, install, and prove the flow through an isolated installed-binary TUI session.
