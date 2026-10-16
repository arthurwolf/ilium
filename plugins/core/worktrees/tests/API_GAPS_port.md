# API gaps of the port twins (tests/port)

Everything of tests/API_GAPS.md applies (the port suite reuses tests/features/common.rs by path). Additionally:

- `ilium-plugin-testkit` `FakeHostBuilder::project_root(self, root: impl Into<std::path::PathBuf>) -> FakeHostBuilder` (F-067): twins need a named main checkout (`/projects/acme`, `/projects/{branch_slug}-repo`, `C:\repos\app`).
- Git command forms assumed for repository facts (ports of worktree_dialog.rs): `git branch --format=%(refname:short)` (local branches), `git rev-parse --abbrev-ref HEAD` (current branch), `git symbolic-ref --short refs/remotes/origin/HEAD` (default branch, `origin/` stripped), `git log -1 --format=%s` (commit subject). The manual names none of them (F-067, F-069).
- Wire shape of the request with neither branch nor path: `{}` makes the server derive `<branch_prefix>task` (suffix `-2`, `-3` on collision with a local branch), base from `default_base`, path from the location template (F-067, F-072).
- Extra `changed` payload fields assumed: `modified` (number), `branch`, `missing` (bool), `last_commit_subject` (string or null), `full_checked_at_unix_millis` (number or null) (F-069).
- Probe retry schedule after a failed probe: 10 s, then doubling, capped at 300 s (original `retry_delay`); not stated on the manual page (F-069).
- Request key `path` (explicit worktree folder) in the `new-agent` payload (F-067).
- `FakeHost::set_platform("windows")` must also make a `C:\...` project root resolve with backslash separators (F-067).
- Registered `WorkspaceSpec.owner: Option<NodeId>` and `branch: String` field types are assumed from the page sample (F-067).

Not ported (no plugin surface or no layout in the manual):
- ilium-client/src/tree_ui.rs entering_worktree_moves_and_dims_both_lines_together, mixed_tree_rows_hit_both_worktree_lines_and_select_both, separator_client_hits_exclude_unpainted_rows_and_keep_clipped_worktree_lines: kernel tree row layout and hit testing.
- ilium-client/src/value_worktree.rs (4 tests): the dialog value controls and their hit testing; the manual defines no view ids.
- ilium-client/src/worktree_dialog.rs geometry_is_shared_for_draw_and_mouse_on_small_terminal, new_worktree_preview_warns_about_submodules_and_uncopied_changes: dialog geometry and preview text, not on the page.
- ilium-client/src/worktree_manager.rs action_hits_follow_the_rendered_ascii_labels, branch_chrome_matches_actual_render_without_reducing_path_detail, compact_layout_keeps_header_details_and_actions_inside_the_frame, inventory_overflow_track_is_visible_and_does_not_select_a_worktree: manager layout and hit testing.
- ilium-client/src/app.rs new_agent_submenu twin covers only the `default_where` setting; the submenu rendering is kernel UI.
- ilium-server/tests/worktree_cwd_detection.rs (session cwd detection, kernel), ilium/tests/pty_tui_smoke.rs (kernel TUI), measure_live_git_status_with_49_workspace_panes (a bench).

Gap-filler twins (tests/port/ui_twins.rs), all plugin-level; kernel layout/hit testing still has no manual surface:
- Git tab row text `Worktree folder` and `Post-create command` at widths 40, 80, 120 (F-072, page figure 1).
- `branch_prefix` accepted as a git key, `default_where` refusing unknown values with an `Err` (F-072).
- `CreatePaneSpec.title` equals the branch (F-067); `WorkspaceSpec.branch: String` (F-067).
- Panel render accepts heights 1..=3 and returns exactly that many rows (F-069).
