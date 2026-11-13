### Added
- Plugin SDK: `#[ilium_plugin]` now emits the WebAssembly component glue on `wasm32-wasip2` when the crate feature `component-server` or `component-client` is on, so a core plugin crate builds into a real component for each side.
- New crate `ilium-plugin-bundle` that builds the core plugins as WebAssembly components and exposes them as an embedded catalogue (off by default; `mode = lenient` or `strict` in `ilium-plugin-bundle/bundle-build.conf`, or the environment variable `ILIUM_PLUGIN_BUNDLE_MODE`, turns the build on).
- Plugin host: a composed runtime (`HostRuntime`) that loads the plugin catalogue, links WASI and the host imports per plugin grant, activates plugins on their triggers, delivers events and commands, renders views and disables or enables a plugin while running; a contribution registry for the 23 manifest contribution kinds. Not yet started by the server or the client.
- The server and the client now start the WebAssembly plugin host: the server after the session is restored, the client beside its event loop. Plugins see the live tree, can submit a first set of intents (rename, move, close, lock, bookmark, expand, resize, create pane, group or split, message) and stop cleanly before the session drains.
- `ilium plugin list`, `ilium plugin enable <id>` and `ilium plugin disable <id>`: the choice is stored in `plugins/installed.json` under the data directory and, when a server runs for the project, applied at once. Core plugins cannot be disabled.
- `--no-plugins` starts Ilium (client and a server it spawns) without any plugin; `--safe-plugins` starts only the core ones.
- Settings has a Plugins tab that lists every plugin with its version and state and an on/off switch.

### Changed
- Plugin contract: a check list is flat (`check-item.children` holds item ids), and five host functions whose names collided with a type are now `get-image-info`, `get-widget-state`, `get-theme-extras`, `get-selection` and `get-foreground-process`.
- Plugin contract: `wit/` again matches the manual, which adds the `telemetry`, `self-limits` and `self-describe` interfaces, `codec.hmac-sha256` and `codec.constant-time-equal`.
- Plugin SDK: events now reach a plugin's `Handler` implementation whether or not `exports` names `handler`, as the manual states; before, such a plugin silently received no events.
- Core plugin manifests declare command and view activation triggers in the global form `command:<plugin-id>/<local-id>` and `view:<plugin-id>/<local-id>`.

### Fixed
- `ilium plugin kill` writes the audit event `plugin-killed` and an unknown plugin subcommand reports `usage.unknown-command`, the names the manual documents.
