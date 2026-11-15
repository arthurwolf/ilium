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

## Round 11b assessment (2026-11-15, worktrees and scheduler cluster)

Scheduler: nothing left to delete as a file. `ilium-server/src/{scheduled_input,prompt_queue}.rs`, `ilium-client/src/{scheduled_input,prompt_queue}.rs`, their tests and the voice template are already gone (the stale first section of `docs/tasks/02-cuts/scheduler.md` still names them as kept). What stays is shared kernel surface with producers elsewhere: the status-rule `ScheduledInput` condition, `ObjectiveSignal::ScheduledInput`, the `IconTarget`, `AgentDebugEventKind::{ScheduledInput*, PromptQueued, ...}` (also matched by `plugins/core/diagnostics/src/view.rs:85-91`) and the two `PromptSubmissionSource` values (used as plain automated-input sources by `ilium-server/tests/crashed_agent_recovery.rs` and the `lib.rs` tests). Removing them breaks users' status-rule and icon config files and another plugin's exhaustive match, so they were not touched.

Worktrees: every feature-only kernel file still exists (workspace, workspace_prune, workspace_owner, workspace_setup, worktree_include, git_status, repo_facts_cache, worktree_dialog, worktree_manager, value_worktree, value_manager, close_pane). The plugin crate is a skeleton plus pure twins, and no part of the engine can be moved on its own:

- `workspace.rs` (create, remove, repo facts, restore target) is the only caller of `workspace_setup`, `worktree_include` and `repo_facts_cache`, and the only non-test caller of `git_status`'s refresh; `workspace_prune` also holds `SpawnRepositoryAdmission`, `spawn_directory_generation` and the custody `CustodyTicket` that `native_terminal_spawn.rs` and `ipc/handlers.rs` pane spawn use. That is spawn-time safety, not feature code.
- The engine's guarantees depend on open file handles: `worktree_include.rs` keeps the `File` of every copied entry so rollback removes only entries whose identity is unchanged (`NoFollowDirectory`), and `workspace_owner`/`workspace_custody` verify ownership markers and tickets by identity. The sandboxed `fs` and `workspace` imports are path-string APIs (`ilium-plugin-host/src/imports/workspace.rs` keeps instance-local records and `WorkspaceAccess` has only `project_dir` and `session_name`), so a guest copy would drop those guarantees. The group plan (`docs/tasks/02-group-plans/workspace.md`, worktrees "Stays kernel") keeps these files in the kernel for this reason.
- A faithful port therefore needs a handle-preserving kernel engine API first: host operations for repo facts, create-in-workspace with rollback, inventory, identity-fenced remove and prune, and close-with-disposition, each returning a typed outcome (extending `wit/host/workspace.wit` through `docs/plugins-manual/pages/api-workspace.md` and `tools/gen_plugin_api.py`, with the facade calling the `workspace*.rs` functions in place). Only then can the front ends move: dialog and `[git]` rows, manager (confirmation identity and typed-path rules), the `ilium close-pane` CLI (also needs invocation environment on the `CliInvoke` wire, key M9), and the status refresh. Each of those is a client or CLI deletion that leaves the engine in the kernel, so "none in kernel" is not reachable for the engine files without redesigning custody.
