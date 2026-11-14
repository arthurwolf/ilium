### Added
- Plugin topics and service calls cross between the server and the client runtime. A service provider on the other side answers within 5 seconds or the call ends with `timed-out`; the provider sees the caller as `remote:<plugin id>`.
- Server plugins can send signals to a pane process (`signal-pane`) and open or close panels, settings pages, onboarding steps and popups in every attached client.
- The plugin wire protocol carries CLI invocation records, forwarded intents, a pane signal request and a plugin pane kind.
- The server routes every plugin intent: input to panes, renames, tree restructuring, pane and group creation, notifications, file opens and input requests. Intents only a client can carry out (focus, attention marks, opening a URL or file, opening a Settings page, dismissing a toast) reach the attached clients and run there.
- Plugin key bindings take multi-character chords (`ctrl+shift+k`, `f5`, `alt+plus`) in the global, prefix and per-pane-kind modes, and plugins can grab keys for a pane until the next key they do not match.
- Plugins receive the kernel events feed: pane created and renamed, agent identified, activity and goal changes, focus moves, theme, key map and kernel settings changes, attention marks, and clients attaching and detaching.
- Agent signatures a plugin contributes (`contributes.signatures`) are used by agent detection while the plugin is enabled.
- Service calls are checked against the provider's schema file before the provider sees them, and immediate replies before the caller does; a mismatch ends the call with `invalid-argument` naming the failing field.
- Metadata plugins keep on tree nodes is saved in the session snapshot and restored with the tree.
- Plugin attention marks show in the attention queue with a plugin tag until the plugin clears them or the pane closes.
- Durable plugin timers on the client side survive a restart.

### Changed
- The plugin wire protocol version is now 2. A client built before this change is refused with `ipc.protocol-mismatch` until it is updated.
