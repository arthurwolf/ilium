### Added
- Server plugins now receive `screen-changed` (at most 10 per second, coalesced per pane, only for plugins granted `terminal.read-cells`), `pane-focused` when a client moves focus, `attention-changed` when a plugin marks or acknowledges a node, and the activity, title and goal changes of single panes, not only of whole-tree snapshots.
- Plugins reading a pane's screen now get the real cursor and screen revision, and `get-foreground-process` reports the detected agent or the pane's own process with its working directory.
- A restore the user confirms now runs the `ilium.core.sessions/before-restore` hook and waits for it, at most 2 seconds, before panes are replaced.
- The server answers a new plugin-call request with journal reads and writes in the session's plugin journal it keeps, the store client-side plugins are to share with server-side ones.
