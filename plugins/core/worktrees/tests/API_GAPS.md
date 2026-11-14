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

## Port wave notes (pure modules)

The pure rules moved into the plugin crate: `src/git/{mod,parse,commands}.rs` (parsers, argument builders, removal gates), `src/location.rs` (branch slug, location template, branch-name validation), `src/include.rs` (worktree-include matching, matcher passed in), `src/wrap_up.rs` (wrap-up prompt, template `templates/worktree-wrap-up.hbs`), `src/close_policy.rs` (dispositions, default close policy, refusal classification). Twin tests are in `tests/port/{git_pure,rules}.rs`, fed by captured git output in `tests/fixtures/git/`.

Still red until real plugin wiring exists: the FakeHost tests under `tests/features` and the rest of `tests/port` (the entry types are skeletons). Not moved yet: the status/probe schedule (`git_status.rs`, `repo_facts_cache.rs`), the dialog and manager views, the `plan` service.

- G10: `process.run:git`, `process.run:sh`, `workspace.manage`, `fs.read:project`, `fs.write:worktrees` imports are not wired in the host, so the runner side cannot execute the argument vectors the pure modules build.
- G8: panel, dialog and manager views cannot edit text fields or scroll, so the create dialog branch field is not portable.
- G2: settings rows are read-only, so the `git` kernel tab keeps its editing in the kernel (`settings.write-kernel:git`).
- G11: the `ilium close-pane --worktree` CLI stays in the kernel (`ilium/src/close_pane.rs`); only its pure disposition rules are mirrored in `close_policy.rs`.
- G3: intents are not routed, so the plugin cannot receive the client `new-agent-in-worktree` intent from the kernel menus.
- G18: the `ignore` and `walkdir` crates are not guaranteed wasm-safe, so `include.rs` takes the matcher as a closure.
- Hook handlers: no `HookHandler` trait, so the three `extensions.provide` grants have no handler yet.

## Move blockers (re-verified 2026-11-14, second pass)

The per-file reasons and the blocker keys B1 to B10 (each with its evidence in the source) are in `docs/tasks/02-cuts/worktrees.md`. The earlier K1 (no `intent_routing.rs`), K3 (no popup text input) and K6 (no terminal menu entries) of this section are closed by the consumer streams; the rest were re-read against the current tree.
