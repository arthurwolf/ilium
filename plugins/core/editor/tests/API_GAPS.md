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
