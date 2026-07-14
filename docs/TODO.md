# Scheduled pane input countdown

- [ ] Add the durable scheduled-input domain/IPC contract and detached-server executor. (IN PROGRESS)
- [ ] Add the pane-only right-click action and aerated hours/minutes/seconds, text, and Enter dialog.
- [ ] Render a human-readable countdown plus reverse clock animation before the pane title.
- [ ] Cover validation, replacement, restore, execution, rendering, keyboard, and mouse behavior.
- [ ] Run workspace gates, verify the real PTY/TUI flow, rebuild, and install.

# Configurable agent identifiers in the tree

- [x] Add validated `[ui]` settings for full-name, initial, chosen-icon, or hidden agent identifiers.
- [x] Add Claude and Codex icon selectors to User Appearance with live persistence.
- [x] Apply the choices to every agent state without disturbing activity indicators or row alignment.
- [ ] Add focused coverage, run workspace gates, verify the live TUI, rebuild, and install. (PAUSED: superseded by the newer explicit scheduled-input feature request.)

# Tree row-action clarity and icons

- [x] Clear the complete row-action strip so title characters cannot bleed through icon gaps.
- [x] Replace edit, up, down, remove, and refresh with the requested emoji while preserving hit behavior.
- [ ] Cover rendered gaps and hit geometry, run workspace gates, verify the live TUI, rebuild, and install. (BLOCKED: unrelated in-progress agent-identifier/session-id changes currently fail strict Clippy and test compilation.)

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

- [x] Add `transcript_prompts.rs`: locate a session's JSONL transcript (via `agent_detect::transcript_path_for_session`), extract the last few genuinely user-typed prompts (Claude `type:"user"`+string content, non-sidechain; Codex `event_msg`/`user_message`), compact each to a bounded head+tail.
- [x] Add `session_naming.rs`: XML/Handlebars prompt template weighting recent prompts, `SessionTitleGenerator` trait over Kilo Gateway, 2-4 word response validation.
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
