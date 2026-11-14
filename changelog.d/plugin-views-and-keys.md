### Added
- Plugin views can show multi-line text areas: Enter starts a new line, the text wraps or scrolls sideways, an optional line-number gutter shows, and the plugin hears the edit after a short pause.
- Long plugin paragraphs marked scrollable get a scrollbar and follow the mouse wheel.
- Long plugin dialogs, such as briefings, queues and help texts, scroll with the wheel, the scrollbar, PageUp/PageDown, Home/End and, on text-only panels, the arrow keys.
- The Keys and help plugin draws its key reference: every key binding, grouped by area with dotted leaders, the prefix and the preset. F1 after the prefix or the command palette opens it, and it scrolls when the list is long.
- The key map that plugins read now lists plugin key bindings along with the prefix keys, each with its full chord (`ctrl+shift+f`, `f1`).
- Plugins can switch an editor pane's line wrap, line numbers, minimap and autosave with the `kernel.set-editor-option` command.
- When plugins add their own setup steps, the first-run wizard shows them as a last step, "From your plugins", inside the wizard with Back, Skip and Finish.

### Changed
- Editing the `[padding]` table in `config.toml` now updates the layout in the running client.

### Fixed
- A plugin key binding written with a capital F, such as `F1`, now works.
