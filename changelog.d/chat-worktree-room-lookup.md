### Fixed
- `ilium chat send`, `context` and `tail` now find the project's room from a linked Git worktree (and from a shell that left its pane's worktree) instead of failing with "this project does not have a chatroom" while the interface shows the room.
- `ilium chat tail` now reports `this project does not have a chatroom` when no room exists, like `send`, instead of printing nothing. `chat context` stays silent for session hooks.
