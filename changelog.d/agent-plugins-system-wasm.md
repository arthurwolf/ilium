### Added
- Client plugins can open a popup dialog (`open-view` with a popup target): the kernel draws it, reports keys and clicks as actions, and always closes it on `Esc`.
- Plugin SDK: `#[ilium_plugin]` now emits the WebAssembly component glue on `wasm32-wasip2` when the crate feature `component-server` or `component-client` is on, so a core plugin crate builds into a real component for each side.
- New crate `ilium-plugin-bundle` that builds the core plugins as WebAssembly components and exposes them as an embedded catalogue (off by default; `mode = lenient` or `strict` in `ilium-plugin-bundle/bundle-build.conf`, or the environment variable `ILIUM_PLUGIN_BUNDLE_MODE`, turns the build on).
- Plugin host: a composed runtime (`HostRuntime`) that loads the plugin catalogue, links WASI and the host imports per plugin grant, activates plugins on their triggers, delivers events and commands, renders views and disables or enables a plugin while running; a contribution registry for the 23 manifest contribution kinds. Not yet started by the server or the client.
- The server and the client now start the WebAssembly plugin host: the server after the session is restored, the client beside its event loop. Plugins see the live tree, can submit a first set of intents (rename, move, close, lock, bookmark, expand, resize, create pane, group or split, message) and stop cleanly before the session drains.
- `ilium plugin list`, `ilium plugin enable <id>` and `ilium plugin disable <id>`: the choice is stored in `plugins/installed.json` under the data directory and, when a server runs for the project, applied at once. Core plugins cannot be disabled.
- `--no-plugins` starts Ilium (client and a server it spawns) without any plugin; `--safe-plugins` starts only the core ones.
- Settings has a Plugins tab that lists every plugin with its version and state and an on/off switch.
- Plugin API: a plugin can keep its own bytes on a tree node (`tree.metadata`, `tree.set-metadata`) on the server side, read the styled cells of a pane's visible rows, and list the agents of a session.
- Plugin API: a pane whose agent is unavailable still reports its agent, with the activity `none`.

### Changed
- The "New split view" dialog of the Layout plugin reads the tree itself; the plugin now asks for `tree.read` and `agent.read` on the client side.
- The "New split view" dialog is now provided by the Layout plugin instead of built-in code. It looks and works the same with `Enter`, `Space` and the arrow keys; an empty split is created with the **Create empty** button instead of the `e` key, the orientation is switched with `Up` and `Down`, and the toolbar Split button no longer shows a spinner. With the Layout plugin off, the action reports that Layout is not running.
- Plugin contract: `create-split-spec` has an optional `parent` group and accepts 0 to 4 panes, so a plugin can create an empty split view in a chosen group.
- Plugin contract: a check list is flat (`check-item.children` holds item ids), and five host functions whose names collided with a type are now `get-image-info`, `get-widget-state`, `get-theme-extras`, `get-selection` and `get-foreground-process`.
- Plugin contract: `wit/` again matches the manual, which adds the `telemetry`, `self-limits` and `self-describe` interfaces, `codec.hmac-sha256` and `codec.constant-time-equal`.
- Plugin SDK: events now reach a plugin's `Handler` implementation whether or not `exports` names `handler`, as the manual states; before, such a plugin silently received no events.
- Core plugin manifests declare command and view activation triggers in the global form `command:<plugin-id>/<local-id>` and `view:<plugin-id>/<local-id>`.
- Plugin API: a tree node now says what kind of container it is (`node-view.container`: project, group, horizontal or vertical split, worktree) and which agent runs in a pane; `tree.children` and `tree.roots` follow the drawn order; `tree-changed` names renamed, moved and reordered nodes, not only new ones.
- Plugin SDK: every host interface except the three resource ones can be called through `host()`, for example `HostTree::get(&mut *host(), id)`.
- Plugins: a key binding a plugin declares with `mode: "prefix"` now works: pressing its key after the leader runs the plugin's command, unless a built-in or remapped binding already uses that key.
- Plugin API: timers a plugin sets now fire (`timer-fired`, `durable-timer-fired`), and a published topic message reaches the plugins on the same side that subscribe to it.
- Plugin test kit: `FakeHost::render` shows a plugin's view as plain text, `click` and `send_action` deliver the widget action, and `last_tree` returns the rendered tree.

### Fixed
- `ilium plugin kill` writes the audit event `plugin-killed` and an unknown plugin subcommand reports `usage.unknown-command`, the names the manual documents.
