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

## Move blockers (verified 2026-11-14)

The `worktrees` kernel files stay PORT-KEEP after the gap streams. The list below is the exact reason per file; the cuts file `docs/tasks/02-cuts/worktrees.md` carries the key table.

- `ilium-git/src/{lib,parse,runner}.rs`: called synchronously by `ilium-server/src/{state,workspace,workspace_owner,workspace_custody,workspace_prune,git_status,repo_facts_cache}.rs` and `ilium-server/src/ipc/connection.rs` (K4). A plugin would run git through `process`, but the kernel never calls the plugin, and `workspace` is not backed by the kernel custody (K5).
- `ilium-server/src/{git_status,repo_facts_cache,worktree_include}.rs`: called from `ilium-server/src/{state,lib,recovery,detection,workspace}.rs` and `ipc/handlers.rs` (K4).
- `ilium-client/src/worktree_dialog.rs` (branch, base and location text fields) and `worktree_manager.rs` (scrolling list): K3.
- `ilium-client/src/{value_worktree,value_manager}.rs`: the `[git]` Settings rows, K8.
- `ilium/src/close_pane.rs`: the built-in `close-pane` subcommand drives the live session and needs the caller's environment (K7). The pure disposition rules are already mirrored in `plugins/core/worktrees/src/close_policy.rs`.
- Not registered: service `ilium.core.worktrees/plan` (request `{repo_facts, branch, base, where}`, reply `{git_args: [[string]], path}`). It is pure and could be written now from `src/location.rs` and `src/git/commands.rs`, but adding it to `contributes.services.provides` changes the manifest and the manual block that `tests/features/manifest.rs` pins (K9).

Blocker keys (verified against the tree on 2026-11-14; each names the exact missing piece):

- K1 intents. `ilium-server/src/plugin_host/facade.rs` `apply_intent` routes only RenameNode, MoveNode, CloseNode, SetNodeLocked, CreatePane, ResizePane, CreateGroup, CreateSplit, SetBookmarked, SetExpanded, Message, SetStatusSegment, ClearStatusSegment, SetDecoration, ClearDecoration, SignalPane, OpenView and CloseView. Every other arm ends in `unsupported("this intent")`: `Intent::SendInput` (needs `Delivery::{Typed, Submitted, Pasted}` plus `stages` / `prompt_epoch` mapped to the server requests `UserKeyInput`, `SubmitTerminalText`, `PasteTerminalText`, `SchedulePaneInput`), `EditorOpen`, `ApplyEdits`, `Notify`, `PlaySound`, `OpenExternal`, `AgentAction`. `ilium-server/src/plugin_host/intent_routing.rs` does not exist.
- K2 pane kinds. `ilium_ipc::NewPaneKind::Plugin { kind_id, title }` exists on the wire only. `ilium-core` has no plugin pane content kind (no `kind_id` in `ilium-core/src/lib.rs`) and the client draws no plugin pane, so board and editor panes cannot leave the kernel.
- K3 popups. `ilium-client/src/plugin_host/popup.rs` (the `Mode::PluginPopup` host) draws message lines, `paragraph`, `choice` and `check-list` only. Text inputs and scrolling exist in the right-hand panel views (`plugin_host/view.rs`, `views.rs`) and not in modal popups. A kernel dialog with text entry or a long scrolling list therefore cannot become a plugin popup. Wanted: `popup.rs` renders and edits `Widget::TextInput`, `Widget::NumberInput` and `Widget::ScrollView` with the same state as `view.rs`.
- K4 kernel to plugin calls. The kernel call sites of these features run synchronously inside kernel loops (server detection loop, server IPC handlers, the client selection path). A plugin answers asynchronously through events. Wanted: a kernel-side call, for example `PluginHost::call_service(plugin: &str, service: &str, payload_json: &str, deadline: Duration) -> Result<String, CallError>` on the server and `App::call_plugin_service(...)` on the client, with `CallError::Unavailable` when the plugin is disabled so the kernel feature is off while the plugin is off.
- K5 imports that are not equipped on a running plugin instance (`ilium-plugin-host/src/runtime.rs` instance import set and `wiring/wave_two.rs`): `clipboard` (`ClipboardImport` exists, used only by the testing harness), `editor` (`EditorImport`, harness only), `client-requests` (harness only). Skeletons without a backend: `inference` (`InferenceImport::new(ids)`, no provider call), `sounds` (`SoundsImport`), `audio`. `workspace` is an in-memory `WorkspaceImport` that is not backed by the kernel custody in `ilium-server/src/workspace*.rs`.
- K6 tree and terminal chrome. `ilium-client/src/terminal_context_menu.rs` lists no plugin entries and contributed toolbar buttons and row actions are not drawn (G6 is not built).
- K7 command line environment. There is no env or argv import in `wit/host/`. The `cli` import offers `caller-pane` and `selected-panes` only. A moved `ilium chat` needs `ILIUM_CHATROOM_AUTHOR`, `AGENT_NAME`, `ILIUM_PANE_ID`, the pane worktree variable `ilium_ipc::pane_env::WORKTREE`, optional hook JSON on standard input (`stdin: optional`; the manifest says `none`), and a project scope that reaches the main checkout of a linked worktree (the `fs` project scope is rooted at `Backends::project_dir` only).
- K8 settings. A contributed row is stored under `[plugins."<id>"]` in `config.toml`. The kernel tabs these plugins own are `[git]`, `[kanban_board]`, `[editor]`, `[text_triggers]`, `[triggers]` and `[icons]`, read by kernel code that must keep working (server workspace setup, `ilium-client/src/config.rs`). `settings.read-kernel:<tab>` and `settings.write-kernel:<tab>` work through `ConfigFileSettings`, but the running client and server do not reload after a plugin write and no `kernel-settings-changed` event exists (contract 20.5). Moving a tab needs both a migration of the stored keys and kernel readers that read the plugin store.
- K9 manual lock. The manual page and its REQ-tagged tests pin the manifests: for example `plugins/core/worktrees/tests/features/manifest.rs` line 240 asserts there is no `settings` contribution, and `plugins/core/text-triggers/tests/features/manifest.rs` asserts the exact grant lists. The group plan's contributions (settings rows with the built-in key names, extra grants, extension entries, a `plan` service) contradict those pins. They need a decision to change the manual page and the pins together; `tools/req_coverage.py` stays at exit 0 only while tagged tests are kept.

What the foundation does provide today (so a later move can use it): editable plugin settings rows in the Plugins list (`plugin_host/settings_rows.rs`), text input and scrolling in panel views (`plugin_host/view.rs`), link detectors (`plugin_host/links.rs`), the `process`, `fs`, `http` and `jobs` imports per instance, topics and services across sides, `SignalPane` and forwarded `OpenView`/`CloseView`, and `ilium <name>` routing to plugin CLI commands (`ilium/src/plugin_cli.rs`).
