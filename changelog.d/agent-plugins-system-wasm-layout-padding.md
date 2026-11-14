### Changed
- Panel padding moved into the Layout plugin. Its page lists the mode, the width threshold, the shared right-hand choice and, for the tree, the shared right-hand panels, terminals and agents, editors and Kanban boards, one linked value or the four sides each. The plugin keeps the values under `[plugins."ilium.core.layout"]` and sends the table to the client when it starts and after every edit; disabling the plugin removes the page and leaves panels unpadded. The old `[padding]` table of `config.toml` is no longer read.

### Removed
- The built-in Padding tab and its live preview dialog.
