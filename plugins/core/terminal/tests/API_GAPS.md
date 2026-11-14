# API gaps found by the terminal tests

Open gaps (the terminal plugin is written against the real host API; these block only its tests):

- ilium-plugin-testkit: `HostTerminal` (`read_screen`, `history_text`, `take_history`) is unmodelled in the bridge. `read_screen` answers `None` and `history_text` answers `Err(Unsupported)`, so `FakeTerminal::set_screen` / `set_history` write fields nothing reads. The F-017 transfer tests (tests/features/transfer.rs) cannot pass until the bridge reads them: `read_screen(pane, part, max_rows)` from `set_screen`, `history_text` returning a `RequestId` that completes as `Event::RequestCompleted(RequestRef::Call(id))` with `take_history` answering `set_history`.
- ilium-plugin-testkit: `HostIntentsHost::submit` is unmodelled and answers `Err`. The `send-text` method and the history-less `send-screen` fallback submit through it. It should record the submission so `session.server(id).submitted()` returns it.
- ilium-plugin-testkit: timers are recorded (`active_timers`) but not fired; the screen-changed tests deliver `Event::TimerFired` by hand.
- ilium-plugin-testkit: `FakeSession::call_extension(point, payload_json)` (EXT-052, EXT-055) runs a chain of an extension point owned by an installed plugin; not needed by the current tests.
- foundation G4: topics and services reach only plugins on the same side, so a client plugin cannot call the server `transfer` service or subscribe to `screen-changed` yet.
- foundation G14: the kernel keeps the terminal pane kind; `screen_diff` is the repaint rule only, the renderer is not moved.

## Re-check of the foundation against the tree (2026-11-14): what still blocks the moves

- Terminal links are moved (2026-11-14): `ilium-client/src/terminal_links.rs` is deleted. The plugin's `url`, `path-line` and `path` detectors are the only link detection, `link-activated` becomes `editor-open` or `open-external`, and the client confines a file to the project or the home directory (`plugin_host/effects.rs::resolve_path`). Known behaviour changes: a file link no longer carries the column, and a path outside the project and the home directory no longer opens as a link (the kernel `stat`ted it before, but only inside cwd or home as well). With the Terminal plugin off there is no link detection except OSC 8.
- Server intent `SendInput` is not routed (`ilium-server/src/plugin_host/facade.rs apply_intent`), so `transfer.send-text` stays `unsupported`; `screen_transfer.rs` stays because `App::screen_transfer_actions_from` reads and pastes synchronously.
