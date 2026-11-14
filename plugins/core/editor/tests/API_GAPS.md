# API gaps for the editor feature tests

Tests are in tests/features/. Items the skeleton or harness does not offer yet:

1. FakeHost dispatches only activate, deactivate and on_events (NativeEntry). The token provider, the
   service provider and widget trees are driven by calling EditorClient directly (tokens.rs, service.rs).
2. The wire shape of the `ilium.core.editor/buffers` service (list, read, replace-range) is not on the page.
   Payload fields are taken from WIT names in kebab-case (buffer, first-line, count, base-revision, edits,
   range, replacement; list entries carry buffer, language, line-count, dirty). Confirm and adjust.
3. The page lists the TODO click but not the exact prompt wording; the expected default prompt is taken from
   src/docs/automation.md ("/goal please do the following task: ...").
4. No manual text or harness entry for the file explorer and file picker (F-022): only a page-presence test.
5. FakeHost cannot invoke extension-point hooks (before-save) or read topics (ilium.core.editor/saved);
   only declarations are tested.
6. Manual inconsistencies: editor.provide is marked reserved but the manifest grants it; the fragment manifest
   lists 7 grants and the complete manifest 8; token kind is named Type on one page and TypeName in generated
   types; the figure shows a 1-column minimap while the implementation uses 8 columns.
7. Goldens (tests/features/golden/editor_main_rs_{40,80,120}.txt) were derived from the page figure, text only.
8. Names of the settings page rows (Long lines, Line numbers, Minimap, Autosave, Autosave delay, Markdown
   default) come from src/docs/editors-and-boards.md and need confirmation.

## Ports part (tests/port)

- ilium-plugin-testkit `ScreenCell`: `pub reversed: bool` (F-019, PORT:ilium-client/src/editor_highlight.rs::cursor_cell_is_reversed_and_current_line_is_underlined) - the cursor-cell reverse-video assertion cannot be written; only underline is checked.
- ilium-plugin-testkit `FakeHost`: a way to read the buffers-service `list` entry shape (`buffers[].dirty`, `buffers[].path`) is assumed from the manual (PORT:ilium-client/src/editor_pane.rs::empty_buffer_has_no_path_and_is_not_dirty); the exact wire keys need confirmation.
- Editor plugin: no channel for path-to-language inference without an editor-opened language (PORT:ilium-client/src/syntax.rs::recognizes_rust_js_ts_python_and_markdown_by_extension); twins pass `language: None` and expect the plugin to infer it from the path.
- Editor plugin: no observable channel for the minimap click target line or the source-viewport wheel scroll (PORT:ilium-client/src/minimap.rs::line_for_click_*, PORT:ilium-client/src/editor_pane.rs::source_wheel_scroll_*); twins assert the click is handled only.
- Editor plugin: toolbar action key for the pixel-header toggle is assumed to be "pixel-headers" (`FakeHost::send_action`, PORT:ilium-client/src/editor_pane.rs::heading_rendering_switches_between_graphics_and_plain_text).

## Ports part 2 (tests/port/wheel_and_dirty.rs, markdown_media.rs)

- Editor plugin: one `WheelDown`/`WheelUp` `EditorPointerEvent` is assumed to move the source viewport by 3 rows (the original `handle_pane_mouse` step) and the pane to clamp at last line minus viewport (PORT:ilium-client/tests/editor_navigation.rs::*); the manual states neither.
- Editor plugin: `FakeHost::key(ViewTarget::Pane, "ctrl+home")` is assumed to move the cursor to line 0 and reveal it (PORT:ilium-client/src/editor_pane.rs::cursor_navigation_reveals_cursor_after_source_wheel_scroll).
- Editor plugin: toolbar action key "save" (`ActionKind::Activated`) on a pathless buffer is assumed to submit nothing (PORT:ilium-client/src/editor_pane.rs::save_without_path_errors); the error channel (notification) is not specified.
- Editor plugin: Markdown view decodes BMP images from `project:` files (PORT:ilium-client/src/markdown/render.rs::local_image_paints_its_color_into_the_buffer) and paints them as half-block cells with `Color::Rgb(0xRRGGBB)` background; the image cell representation is not on the page.
- ilium-plugin-testkit `Screen`: no accessor for the raster size of a pixel heading; raster tier size twins (markdown/raster.rs) assert only relative row layout.
- Close confirmation (app.rs) and the identity tooltip (ui.rs) are kernel UI; twins assert the dirty flag of the buffers-service `list` they derive from.

## Port wave notes (workspace group)

- Moved as a pure module with tests: `src/open_target.rs` (URL scheme gate, lexical normalization, cwd/home confinement, file-or-directory classification from the host's answer). Tests: `tests/port/open_target_pure.rs`. Token detection (`terminal_links::link_at`) stays in the terminal plugin.
- Not moved yet: `markdown/`, `syntax/`, `buffer/`, `chrome/`, `document.rs`, `agent_from_line.rs`, `close_dirty.rs` (templates copy), the service schema, and the `contributes.extensions` entry for `ilium.core.terminal/open-link`. The editor pane kind is G14, file access is G10 (`fs`, `editor`, `clipboard`), scrolling and text edit in views are G8, agent creation from a line is G3.

## Move blockers (verified 2026-11-14)

The `editor` kernel files stay PORT-KEEP after the gap streams. The list below is the exact reason per file; the cuts file `docs/tasks/02-cuts/editor.md` carries the key table.

- `ilium-client/src/editor_*.rs`, `markdown/`, `minimap.rs`, `syntax.rs`, `source_*.rs`, `explorer_overlay.rs`, `filesystem/editor*.rs`: the editor is a kernel pane kind (K2).
- `ilium-client/src/open_target.rs`, `workspace_file.rs`: the terminal click path. `plugin_host/links.rs` now consumes contributed link detectors, so the `path-line` detector could be contributed, but its effect is `Intent::EditorOpen` (K1) and the plan keeps the kernel handler as the permanent fallback, so there is nothing to cut.
- `ilium-client/src/agent_from_line.rs`: creates an agent and types a goal into it: `Intent::CreatePane` is routed, `Intent::SendInput` is not (K1).
- Imports `editor` and `clipboard` are not equipped (K5).
- Settings rows `editor.*` and the `settings.read-kernel:editor` grant are not in the manifest; adding them is a K8 and K9 decision.

Blocker keys (verified against the tree on 2026-11-14; each names the exact missing piece):

- K1 intents. `ilium-server/src/plugin_host/facade.rs` `apply_intent` routes only RenameNode, MoveNode, CloseNode, SetNodeLocked, CreatePane, ResizePane, CreateGroup, CreateSplit, SetBookmarked, SetExpanded, Message, SetStatusSegment, ClearStatusSegment, SetDecoration, ClearDecoration, SignalPane, OpenView and CloseView. Every other arm ends in `unsupported("this intent")`: `Intent::SendInput` (needs `Delivery::{Typed, Submitted, Pasted}` plus `stages` / `prompt_epoch` mapped to the server requests `UserKeyInput`, `SubmitTerminalText`, `PasteTerminalText`, `SchedulePaneInput`), `EditorOpen`, `ApplyEdits`, `Notify`, `PlaySound`, `OpenExternal`, `AgentAction`. `ilium-server/src/plugin_host/intent_routing.rs` does not exist.
- K2 pane kinds. `ilium_ipc::NewPaneKind::Plugin { kind_id, title }` exists on the wire only. `ilium-core` has no plugin pane content kind (no `kind_id` in `ilium-core/src/lib.rs`) and the client draws no plugin pane, so board and editor panes cannot leave the kernel.
- K3 popups. `ilium-client/src/plugin_host/popup.rs` (the `Mode::PluginPopup` host) draws message lines, `paragraph`, `choice` and `check-list` only. Text inputs and scrolling exist in the right-hand panel views (`plugin_host/view.rs`, `views.rs`) and not in modal popups. A kernel dialog with text entry or a long scrolling list therefore cannot become a plugin popup. Wanted: `popup.rs` renders and edits `Widget::TextInput`, `Widget::NumberInput` and `Widget::ScrollView` with the same state as `view.rs`.
- K4 kernel to plugin calls. The kernel call sites of these features run synchronously inside kernel loops (server detection loop, server IPC handlers, the client selection path). A plugin answers asynchronously through events. Wanted: a kernel-side call, for example `PluginHost::call_service(plugin: &str, service: &str, payload_json: &str, deadline: Duration) -> Result<String, CallError>` on the server and `App::call_plugin_service(...)` on the client, with `CallError::Unavailable` when the plugin is disabled so the kernel feature is off while the plugin is off.
- K5 imports that are not equipped on a running plugin instance (`ilium-plugin-host/src/runtime.rs` instance import set and `wiring/wave_two.rs`): `clipboard` (`ClipboardImport` exists, used only by the testing harness), `editor` (`EditorImport`, harness only), `client-requests` (harness only). Skeletons without a backend: `inference` (`InferenceImport::new(ids)`, no provider call), `sounds` (`SoundsImport`), `audio`. `workspace` is an in-memory `WorkspaceImport` that is not backed by the kernel custody in `ilium-server/src/workspace*.rs`.
- K6 tree and terminal chrome. `ilium-client/src/terminal_context_menu.rs` lists no plugin entries and contributed toolbar buttons and row actions are not drawn (G6 is not built).
- K7 command line environment. There is no env or argv import in `wit/host/`. The `cli` import offers `caller-pane` and `selected-panes` only. A moved `ilium chat` needs `ILIUM_CHATROOM_AUTHOR`, `AGENT_NAME`, `ILIUM_PANE_ID`, the pane worktree variable `ilium_ipc::pane_env::WORKTREE`, optional hook JSON on standard input (`stdin: optional`; the manifest says `none`), and a project scope that reaches the main checkout of a linked worktree (the `fs` project scope is rooted at `Backends::project_dir` only).
- K8 settings. A contributed row is stored under `[plugins."<id>"]` in `config.toml`. The kernel tabs these plugins own are `[git]`, `[kanban_board]`, `[editor]`, `[text_triggers]`, `[triggers]` and `[icons]`, read by kernel code that must keep working (server workspace setup, `ilium-client/src/config.rs`). `settings.read-kernel:<tab>` and `settings.write-kernel:<tab>` work through `ConfigFileSettings`, but the running client and server do not reload after a plugin write and no `kernel-settings-changed` event exists (contract 20.5). Moving a tab needs both a migration of the stored keys and kernel readers that read the plugin store.
- K9 manual lock. The manual page and its REQ-tagged tests pin the manifests: for example `plugins/core/worktrees/tests/features/manifest.rs` line 240 asserts there is no `settings` contribution, and `plugins/core/text-triggers/tests/features/manifest.rs` asserts the exact grant lists. The group plan's contributions (settings rows with the built-in key names, extra grants, extension entries, a `plan` service) contradict those pins. They need a decision to change the manual page and the pins together; `tools/req_coverage.py` stays at exit 0 only while tagged tests are kept.

What the foundation does provide today (so a later move can use it): editable plugin settings rows in the Plugins list (`plugin_host/settings_rows.rs`), text input and scrolling in panel views (`plugin_host/view.rs`), link detectors (`plugin_host/links.rs`), the `process`, `fs`, `http` and `jobs` imports per instance, topics and services across sides, `SignalPane` and forwarded `OpenView`/`CloseView`, and `ilium <name>` routing to plugin CLI commands (`ilium/src/plugin_cli.rs`).
