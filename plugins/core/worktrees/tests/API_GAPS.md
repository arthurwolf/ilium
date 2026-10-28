# API gaps for the worktrees feature tests

Items the tests use that the skeleton or the manual page does not define. All are isolated in
`tests/features/common.rs` where possible; the others are named per bullet.

- `ilium_plugin_core_worktrees::WorktreesClient` and `WorktreesServer` entry types (the crate must export both, one per side).
- `FakeHost::for_manifest(path).side("server"|"client").build()`, `activate::<T>()`, `deliver_one`, `settle`, `submitted`, `advance_clock(ms)`, `cancel_pending`, `failures`, `logs`, `last_notice`, `published(topic)`, `offered_actions`, `project_root`, `set_platform`.
- `host.process()`: `default_exit`, `on_args`, `on_program`, `on_program_timeout`, `calls`, `max_concurrent`; `ProcessSpec` fields `program, arguments, cwd, stdin, timeout_millis`.
- `host.workspace()`: `registered()` returning `WorkspaceSpec { branch, base, owner, path{scope,path} }`, `add_foreign(path, branch)`.
- `host.fs()`: `seed`, `seed_sparse(path, size)`, `read`, `symlink`.
- `host.panes().add_running_in(path)` (running process in a worktree).
- `host.kernel_settings("git").get/set` for the kernel `[git]` tab, with the loader validation messages of `ilium-client/src/config.rs`.
- `host.hook_chain(point)` (`reply_all`, `add`, `last_payload`) and `host.collect_point(point).add`.
- `host.render(ViewTarget::Panel|SettingsTab, columns, rows)` returning a screen with `rows` and `row(i)`; `FakeSession::{install_core, start, server, call_service, pump}`.
- Wire shapes absent from the manual: the existing-worktree request (`{"worktree": id}`), the `remove` channel payload (`{path, branch, merged}`), the `changed` topic extras (`branch`, `detached`, `missing`, `recorded_branch`), the `before-create` payload (`repo, branch, base, agent`), the panel id `worktrees`, the `NodeRemoved` event name.
- `host.order_of_calls_and_submissions().process_before_submission(program, intent)`.

Manual defects: SEC-293 (`post-create-steps` is standard on the page, high in reference-extension-security) and PKG-025 (client grants without a client component in the manifest) make two tests red for manual reasons.

Not ported (no public API or no manual layout): the ilium-git moves-to-plugin parser/runner unit tests, the git_status internals, pty_tui_smoke, worktree_cwd_detection, `measure_live_git_status_with_49_workspace_panes` (a bench), and the client dialog, manager, value_worktree and app UI ports (the manual defines no view ids or layouts for them).
- `FakeHostBuilder::project_root` is also used by `server_host_unactivated_at` (F-067); default agent command `claude` assumed for the `before-create` payload `agent` (F-067, page sample).
