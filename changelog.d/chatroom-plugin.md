### Changed
- The project chatroom (`CHATROOM.md`, `ilium chat init|send|context|tail`, the chatroom panel, `@name` forwarding to agent panes and the lifecycle hooks) now runs as the `ilium.core.chatroom` plugin. Disabling the plugin removes the panel, the project menu entries and the `ilium chat` command. Records are stamped in UTC and a forwarded mention reads "with session unknown"; reader cursors restart once.
- A plugin CLI command can declare `env` (environment variables it reads) and `project_marker` (a file whose nearest ancestor directory, or main Git checkout, is the project). Optional standard input no longer waits for an open pipe that sends nothing.

### Removed
- The built-in chatroom router, `ilium chat` command, chatroom sidebar row, panel and the "Add chatroom to project" menu entry. Agent setup no longer creates `CHATROOM.md` when it installs project guidance; use `ilium chat init`.
