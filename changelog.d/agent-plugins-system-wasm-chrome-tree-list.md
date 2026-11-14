### Added
- Plugin views and plugin panes now draw `tree-list` widgets: one row per item with indent, expander, icon, label, trailing text and badge. A click selects an item, a double click or Enter activates it, the expander or Right/Left toggles it, and Up/Down move the highlight.
- Dragging a draggable tree item with the mouse onto another item or list sends a `dropped` action, and the row it would land on is underlined while dragging. Kanban plugin boards use this to move cards between and within columns.
- Text triggers run the `before-fire` extension hook before each reply, so another plugin can refuse a send, and publish the `fired` topic after each sent reply.
