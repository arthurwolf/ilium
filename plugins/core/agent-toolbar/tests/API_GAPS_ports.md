# API gaps found by the agent-toolbar port twins

The manual page defines no surface for the pure-logic domains the original tests cover. The twins in tests/port/ assume one request/reply convention; each bullet names what is missing.

- ilium-plugin-core-agent-toolbar (AgentToolbarClient/AgentToolbarServer `Handler::on_events`): a `Message` event on channel `port.<op>` carrying a JSON request must be answered by one `message` intent whose payload is the JSON reply. Not in the page. Needed by every PORT twin of F-033, F-035, F-081 (all files under tests/port/) to observe logic that has no other wire surface.
- Same crate, ops on the server side: `port.freeze.should-freeze`, `port.freeze.clock`, `port.prompt-transcript.exact-user-prompt-after`, `port.statusline.claude.*`, `port.statusline.antigravity.case`, `port.transcript.*` (F-033 freeze, prompt evidence, statusline capture, transcript entries).
- Same crate, ops on the client side: `port.history-path.resolve`, `port.layout.completed-action`, `port.goal-resume.*`, `port.effort.*`, `port.toolbar.*`, `port.tree-toolbar.*`, `port.icons.*`, `port.icon-picker.*`, `port.status.*`, `port.banner.layout`, `port.banner.pty` (F-035, F-081). The reply field names are in the twins' expected literals.
- Dev-dependency `tempfile`: not needed, the history-path twin passes virtual files in the request instead of a temporary home directory.
- Twins whose original input was a large fixture (claude/codex transcripts, antigravity settings.json variants, status tooltips) carry a case description or the key values only; the exact fixture bytes need a defined request field once the surface exists.
