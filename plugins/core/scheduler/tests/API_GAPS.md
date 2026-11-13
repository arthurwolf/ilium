# API gaps for ilium.core.scheduler feature tests

- ilium-plugin-core-scheduler manifest.json: no `contributes` block (commands, menus, popups, settings, extensions.provide, services.offer, topics); command ids `ilium.core.scheduler.schedule-input|queue-prompt|clear-queue` and popup view ids `scheduler.schedule|scheduler.queue` are assumed in common.rs (B01..B15).
- FakeSession service JSON for `ilium.core.scheduler/queue` methods schedule, pending, enqueue, list, cancel, menu, journal is unspecified; shapes assumed in common.rs (F-055, F-057, B07..B21).
- FakeSession: no specified client-to-server route for dialog submit; tests rely on `session.pump()` (B07).
- FakeSession::install_core stub does not exercise the real plugin; tests go red only through the unimplemented skeleton.
- Journal entry kind `queued-prompt` and the `attempted_delivery` field name are assumed (B19, B20).
- `NodeId` has no Display; the page samples use `{pane}`, tests use `.0`.
- FakeHost: no accessor for armed durable timers (B08).
- FakeHost key chord names (`ctrl+enter`, `shift+tab`, `esc`, `space`, `enter`) are assumed lowercase.
- Dialog field keys (hours, minutes, seconds, payload, send-enter, prompt, delivery, times) are inferred from page sample comments.
- Harness has no menu-entry inspection (B15 uses assumed service method `menu`).
- Session storage key for the snapshot section is unspecified (B21); test only checks JSON validity.
- FakeSession: no `published_topics`, `contribute_hook`, `hook_calls` accessors; the three tests needing them sit behind `cfg(feature = "api-gaps")` in extension_points.rs (topic `fired`, veto, veto payload).
- ports (tests/port): service methods `frame_delay` ({remaining_ms, frame_ms} -> {delay_ms}; B-redraw cadence of the countdown, F-055) and `countdown` ({remaining_ms} -> {text}; "1h 01m 01s", "now", "1m 02s") of `ilium.core.scheduler/queue` are assumed; `pending` entries are assumed to carry `remaining_ms`.
- ports: the client number-row stepper keys (`left`/`right` on a focused number or delivery row) and the mouse click targets (`FakeHost::click`) on the checkbox, the choice arrows and the schedule button are assumed; the page does not specify them (PORT clock_steps, repeat_count, mouse targets).
- ports: server-side refusal text for overflowing durations ("The duration is too large") is taken from the client original; the manual gives no server message for overflow.
