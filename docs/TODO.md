# Mouse interaction

# AI project names

- [x] Add a Kilo Gateway client and deterministic project-name inference workflow.
- [x] Persist inferred project names in `.illium/config.yaml` and render them in the sidebar title.
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
- [x] Add `syntax.rs`: per-line token highlighting (`highlight()`), extensionless-filename + extension lookup, foreground/bold/italic/underline only (no background, to avoid a colored box over illium's dark chrome).
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
