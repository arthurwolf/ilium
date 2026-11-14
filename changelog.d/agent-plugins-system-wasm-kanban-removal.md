### Changed
- The Kanban board no longer exists in the Ilium core: the board model, storage, pane, creation and edit dialogs, the `Kanban Board` settings tab and the `[kanban_board]` config table are removed. A board is a plugin pane owned by `ilium.core.kanban`; "New board", "Create board from Markdown" and the voice board tool now run that plugin's commands, and with the plugin off there are no boards in the interface.
- The Kanban plugin opens its own "New board" dialog when `new-board` runs without a path, and accepts agent control actions through `ilium.core.kanban/control`.

### Removed
- `ilium ctl new-board` and the `new_board` control operation (the CLI cannot reach a client plugin).
- The `Board` pane content kind and pane status in the core tree, and the `NewBoard` client request.
