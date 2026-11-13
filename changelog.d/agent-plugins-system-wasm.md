### Added
- Plugin SDK: `#[ilium_plugin]` now emits the WebAssembly component glue on `wasm32-wasip2` when the crate feature `component-server` or `component-client` is on, so a core plugin crate builds into a real component for each side.
- New crate `ilium-plugin-bundle` that builds the core plugins as WebAssembly components and exposes them as an embedded catalogue (off by default; `ILIUM_PLUGIN_BUNDLE_MODE=lenient` or `strict` turns the build on).

### Changed
- Plugin SDK: events now reach a plugin's `Handler` implementation whether or not `exports` names `handler`, as the manual states; before, such a plugin silently received no events.
- Core plugin manifests declare command and view activation triggers in the global form `command:<plugin-id>/<local-id>` and `view:<plugin-id>/<local-id>`.

### Fixed
- `ilium plugin kill` writes the audit event `plugin-killed` and an unknown plugin subcommand reports `usage.unknown-command`, the names the manual documents.
