# API gaps and manual assumptions of the session-convert feature tests

Each bullet: crate, exact signature or item, requirement id, one-line reason.

- ilium-plugin-testkit: `FakeHost::process() -> &mut FakeProcess` with `children() -> Vec<FakeChild>` (fields `id`, `program`, `arguments`, `stdin_open`, `line_mode`, `stdin_lines`, `signals`), F-077, app-server tests need to observe the spawned codex child.
- ilium-plugin-testkit: `FakeHost::jobs() -> &mut FakeJobs` with `reported_progress()` and `cancelled() -> Vec<u64>`, F-077, job progress and cancel observation.
- ilium-plugin-testkit: `FakeHost::run_job(kind, input) -> Result<Vec<u8>, PluginError>` and `request_cancel`, F-077, run the codex-to-claude job directly.
- ilium-plugin-testkit: `FakeFs::write(FsPath, &[u8])`, `read(FsPath) -> Option<Vec<u8>>`, `paths(scope) -> Vec<String>`, `inject_failure(op, ErrorCode)`, F-077, seeding transcripts and atomic-write failure.
- ilium-plugin-testkit: `FakeHost::agents().set_session(pane, Option<SessionRef>)` and `FakeHost::tree().set_worktree(pane, bool)`, F-077, menu disabled reasons.
- ilium-plugin-testkit: `FakeHost::menu_items(MenuContext) -> Vec<MenuEntry>`, `FakeHost::key(ViewTarget, &str)`, `FakeHost::deliver_one`, `FakeHost::settle`, `FakeHost::advance_clock(u64)`, `FakeHost::failures()`, F-077, UI and event driving.
- ilium-plugin-testkit: `FakeHost::extensions().hook_payloads(point) -> Vec<String>` and `on_hook(point, HookReply)`, F-077, after-convert observation.
- ilium-plugin-testkit: `FakeHost::storage().get(Bucket, &str) -> Option<Vec<u8>>`, F-077, conversion state readback.
- ilium-plugin-testkit: `FakeSession::plugin(id) -> &mut FakeHost` is `unimplemented!("DEV-486")`, F-077, service tests read intents through it.
- manual: the message protocol between the two sides (channels start, begin, step, progress, finished, failed, cancel, resume, close and their JSON fields) is not specified on the page; the tests assume it, F-077.
- manual: the state key `convert/<pane>` in the session storage bucket, the freeze dedup key `freeze-<pane>`, the job input format and the codex app-server request ids are not specified; assumed, F-077.
- manual: the six step titles of Codex to Claude Code and Claude Code to Codex, the check, arrow and circle marks, the 24-cell bar and the hint row text are inferred from the figures; assumed, F-077.
- manual: where a failed disabled reason, the 40-character label limit and the worktree refusal are worded is only shown by example; assumed, F-077.
- manual: session-convert-Bnn behaviour ids do not exist on the page, so none are covered.
