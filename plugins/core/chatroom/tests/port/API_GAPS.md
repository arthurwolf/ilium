# API gaps of the chatroom port twins

The twins use the same testkit additions and assumptions as `tests/API_GAPS.md` (feature part). Additional or confirmed items:

- ilium-plugin-sdk (testing) `FakeHost::advance_clock(&mut self, milliseconds: u64)`: moves the fake clock and runs timers (PORT app.rs reconcile 1 s and repair 15 s cadence, F-062).
- ilium-plugin-sdk (testing) `FakeFs::tail_log(&self) -> Vec<TailCall { max_bytes: usize, returned_cursor: u64 }>`: observe `fs::tail` calls (PORT chatroom_router.rs read bounds and cursor, F-062).
- ilium-plugin-sdk (testing) `FakeHost::inject_failure(&mut self, call: &str, error: PluginError)`: fail the next host call named `topics.publish` once (PORT append_reader_can_resume_after_the_last_successfully_delivered_record).
- ilium-plugin-sdk (testing) `FakeFs::{mkdir, symlink, replace_atomically, write_quiet}` and `FakeHost::{service_calls, published_topics, is_disabled, diagnostics}` as in `tests/API_GAPS.md`.
- Value assumption: `Intent::SendInput` forwards one submission per line of the notification, an empty line as an empty text or `"\r"` (PORT notification_lines_preserve_each_enter_boundary); the manual does not describe the `@name` forwarding (see `tests/API_GAPS.md`).
- Value assumption: a drag of the scrollbar and the mouse wheel reach the plugin as the `scrolled` action of widget key `scroll` with the row offset from the oldest message (PORT mouse.rs, chatroom_ui.rs); the page names no wheel step of 3 rows.
- Value assumption: the plugin removes ESC from displayed and tailed content (chatroom-B02, "other control characters are removed").
