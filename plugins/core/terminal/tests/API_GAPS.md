# API gaps found by the terminal tests

Open gaps (the terminal plugin is written against the real host API; these block only its tests):

- ilium-plugin-testkit: `HostTerminal` (`read_screen`, `history_text`, `take_history`) is unmodelled in the bridge. `read_screen` answers `None` and `history_text` answers `Err(Unsupported)`, so `FakeTerminal::set_screen` / `set_history` write fields nothing reads. The F-017 transfer tests (tests/features/transfer.rs) cannot pass until the bridge reads them: `read_screen(pane, part, max_rows)` from `set_screen`, `history_text` returning a `RequestId` that completes as `Event::RequestCompleted(RequestRef::Call(id))` with `take_history` answering `set_history`.
- ilium-plugin-testkit: `HostIntentsHost::submit` is unmodelled and answers `Err`. The `send-text` method and the history-less `send-screen` fallback submit through it. It should record the submission so `session.server(id).submitted()` returns it.
- ilium-plugin-testkit: timers are recorded (`active_timers`) but not fired; the screen-changed tests deliver `Event::TimerFired` by hand.
- ilium-plugin-testkit: `FakeSession::call_extension(point, payload_json)` (EXT-052, EXT-055) runs a chain of an extension point owned by an installed plugin; not needed by the current tests.
- foundation G5: no consumer routes a link click to the plugin, so `ilium-client/src/terminal_links.rs` stays (port-keep) next to `plugins/core/terminal/src/links.rs`.
- foundation G4: topics and services reach only plugins on the same side, so a client plugin cannot call the server `transfer` service or subscribe to `screen-changed` yet.
- foundation G14: the kernel keeps the terminal pane kind; `screen_diff` is the repaint rule only, the renderer is not moved.
- manual (F-015): `link_detectors[0].command = "link-open"` has no `contributes.commands` entry; EXT-050 says a command field must name a declared command.

## Re-check of the foundation against the tree (2026-11-14): what still blocks the moves

- G5 consumer is not wired: `ilium-client/src/plugin_host/links.rs` exists but `ilium-client/src/plugin_host.rs` has no `pub mod links;`, and nothing calls `links::plugin_link_at` from the click path (`ilium-client/src/app.rs` near the `terminal_links::link_at` call, `ilium-client/src/terminal_context_preparation.rs` near `link_at(&line, ...)`). Wanted: `App::plugin_link_at(&self, pane, line_text, column) -> Option<links::LinkMatch>` that runs `ClientPlugins::invoke_command(match.command, match.arguments_json(), CommandOrigin::LinkClick)` and queues `Event::LinkActivated`; then `terminal_links.rs` (`link_at`, `TerminalLink`, the 8 twin tests) can be cut.
- `link-open` is named by both detectors but is not in `contributes.commands`; add the command (and the manual block) together with the consumer.
- Client intents `EditorOpen` and `OpenExternal` are not routed (`ilium-client/src/plugin_host/host.rs` handles only Message, OpenView, CloseView, status segments and decorations), so `TerminalClient`'s `LinkActivated -> EditorOpen` submission is refused today. Wanted: `ClientEffect::EditorOpen { path, line, placement }` and `ClientEffect::OpenExternal { target }` applied by `open_target.rs` / `external_open.rs`.
- Server intent `SendInput` is not routed (`ilium-server/src/plugin_host/facade.rs apply_intent`), so `transfer.send-text` stays `unsupported`; `screen_transfer.rs` stays because `App::screen_transfer_actions_from` reads and pastes synchronously.
