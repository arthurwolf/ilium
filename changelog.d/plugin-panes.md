### Added
- Plugin panes: a plugin that declares a basic pane kind (`contributes.pane_kinds`) can create panes of that kind. They sit in the tree like any other pane, show the plugin-set title, take the keyboard, the mouse and pastes when focused, keep their own view state in a split view, and come back after a restart from the session snapshot. When the plugin is turned off, the pane shows an "off" frame until it is turned on again.
- `ilium-control` pane details and the client control snapshot report plugin panes as `plugin`, with their pane kind id.
- Background animation engine: external scenes can be registered through a factory, built with the shared palette and always wrapped in the shared look, the groundwork for scenes supplied by plugins.
