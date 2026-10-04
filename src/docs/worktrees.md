# Git worktrees for agents

An agent that works in the same checkout as you, or as another agent, can overwrite files that someone else is editing. Ilium can start each agent in its own linked Git worktree on its own branch instead, so several agents (and you) work side by side without trampling each other. This page explains how to start an agent in a worktree from the keyboard, the tree menus and the command line, what the creation dialog fields mean, the Settings -> Git options (including the post-create command), how `.worktreeinclude` copies local files into new worktrees, how cleanup works, and which conditions block removal.

Contents:

- [What a worktree agent is](#what-a-worktree-agent-is)
- [Start an agent in a new worktree](#start-an-agent-in-a-new-worktree)
- [Start an agent in an existing worktree](#start-an-agent-in-an-existing-worktree)
- [Start from the command line](#start-from-the-command-line)
- [Settings -> Git](#settings---git)
- [Post-create command](#post-create-command)
- [Copy local files with `.worktreeinclude`](#copy-local-files-with-worktreeinclude)
- [What the tree shows](#what-the-tree-shows)
- [Clean up worktrees](#clean-up-worktrees)
- [Removal blockers and states](#removal-blockers-and-states)
- [Branch deletion](#branch-deletion)
- [Limits and edge cases](#limits-and-edge-cases)
- [Troubleshooting](#troubleshooting)

Related pages: [Session recovery](session-recovery.md) (what happens to worktree panes after a restart), [CLI reference](cli-reference.md), [Panes and layout](panes-and-layout.md), [Settings](settings.md).

## What a worktree agent is

A Git *linked worktree* is a second working directory that shares the repository's history with the first one but has its own checked-out branch and files. Ilium creates one for the agent, starts the agent inside it, and remembers that the pane belongs to that worktree.

- Ilium records the worktree for the pane. Launching, finding the agent's transcript and restoring after a restart all use that directory.
- If you started Ilium in a subdirectory of the repository, the same subdirectory is selected inside the new worktree.
- A worktree that disappears from disk stays visible in the tree. Ilium will not resume the agent from the wrong checkout.
- Only Ilium-created worktrees carry an ownership marker. Worktrees you made yourself are never adopted or removed by Ilium.
- Uncommitted changes in your current checkout are **not** carried into the new worktree. The creation dialog reminds you when there are some.
- Switching the branch of the shared current checkout is deliberately not offered, because other panes and your own shell may be using it. A dedicated branch for an agent always uses a linked worktree.

## Start an agent in a new worktree

1. Make sure the project is inside a Git repository. Otherwise the worktree entries are disabled with an explanation.
2. Press `Ctrl+B` then `W` (new agent in a worktree). Alternatively use the tree's new-agent menu, pick the provider (Claude Code, Codex or Antigravity) and choose **In new worktree...**
3. The dialog **New agent in a worktree** opens. It first reads the repository (**Checking repository...**), then offers:

| Field | Meaning |
| --- | --- |
| Prompt (optional first message) | Text sent to the agent as its first message. A branch name is suggested from it |
| Branch | The new branch. Must not already exist and must be a valid Git branch name. Suggested as the branch prefix plus a slug of your prompt; once you type your own, it is no longer auto-changed |
| Where | New worktree or an existing one |
| Advanced | Reveals provider, base reference and path |
| Provider | Which agent to start |
| Base | The reference the new branch starts from (current branch or the repository's default branch, per Settings -> Git) |
| Path | Where the worktree will be created. Must be absolute and normalised, and must not overlap an existing checkout or Git metadata |
| Close policy | Whether Ilium should later offer to remove this worktree when you close the pane |

4. Press `Enter` (or `Ctrl+Enter`) to create. `Tab` moves between fields, the arrow keys choose, `Esc` cancels.
5. Progress shows each stage: **Creating worktree**, **Preparing files**, **Running setup** (only if a post-create command is set) and **Starting agent**. The dialog stays open until the server confirms.

Creation runs Git checkout hooks and filters, which the dialog states. Git validation and filesystem checks run again on the server after the dialog's advisory checks, and creation is serialised per repository so two agents cannot race.

If creation fails after Git already made the checkout (for example because a hook or the setup command failed), Ilium keeps the checkout and reports its path, because hooks, filters or the command may have written files that cannot be rolled back safely.

## Start an agent in an existing worktree

Choose **In existing worktree...** from the new-agent menu, or set **Where** to Existing worktree in the dialog. Select one of the registered worktrees listed. The agent starts in it on whatever branch it has (or a detached HEAD). A worktree that already has a live agent cannot be selected again. Using an existing worktree never makes Ilium its owner, so Ilium will not offer to remove it.

## Start from the command line

```sh
ilium new-pane --worktree --branch agent/fix-login -- codex
ilium new-pane --worktree --branch agent/fix-login --base main -- claude
ilium new-pane --session-name default --worktree --branch agent/spike -- agy
```

| Flag | Meaning |
| --- | --- |
| `--worktree` | Start a built-in agent in a new Git worktree on its own branch. Requires `--branch` |
| `--branch <name>` | New branch. Must not already exist. Validated as a Git branch name |
| `--base <ref>` | Starting reference. Defaults to the repository's default base. Requires `--worktree` |
| `--session-name <name>` | Session that receives the pane (default `default`) |
| `-- <cmd>` | Exactly one built-in agent command: `claude`, `codex` or `agy` |

Behaviour:

- The server is started if it is not already running. No terminal UI is attached; run `ilium` afterwards to see the new pane.
- The worktree path is derived from your Settings -> Git location template, the same as in the dialog. The saved post-create command and branch settings also apply.
- Output is JSONL for scripting. On success a `result` record contains `request_id`, `pane_id`, `branch`, `base` and `worktree_path`. On failure an `error` record is printed.
- If the confirmation times out, creation may still have completed. Inspect the session and `git worktree list` before retrying.

## Settings -> Git

All values are stored under `[git]` in `config.toml` and are validated before saving.

| Setting | Key | Default | Meaning |
| --- | --- | --- | --- |
| Default where | `default_where` | `here` | What the new-agent menu highlights first: `here`, `new_worktree` or `existing_worktree`. `here` keeps ordinary one-click creation; existing agents never move |
| Branch prefix | `branch_prefix` | `agent/` | Prefix for suggested branch names. Must produce a valid branch name. No branch is created or renamed by changing it |
| Worktree location | `worktree_location_template` | `{repo_parent}/{repo_name}.worktrees/{branch_slug}` | Path template for new worktrees |
| Default base | `default_base` | `current` | Whether new branches start from the `current` branch or the repository's `default_branch` |
| Branch line | `branch_line` | `worktree_only` | Show the branch line under agent rows for `worktree_only` agents, or `off` |
| Setup command | `setup_command` | empty | Optional command run after a new worktree is created (Linux only) |
| Close policy | `default_close_policy` | `keep` | `keep`, or `offer_removal_when_safe` when closing a worktree agent |

Location template placeholders:

| Placeholder | Expands to |
| --- | --- |
| `{repo_parent}` | The directory containing the main checkout |
| `{repo_name}` | The main checkout's directory name |
| `{project}` | The project directory |
| `{branch_slug}` | The branch name made filesystem-safe |

The template must contain `{branch_slug}`. Unknown or unbalanced placeholders are rejected. If the result is not absolute, Ilium uses `<main checkout>/.ilium/worktrees/<branch slug>`.

Example with the default template: branch `agent/fix-login` in `/home/me/acme` creates `/home/me/acme.worktrees/agent-fix-login`.

## Post-create command

An optional shell command that runs once after a new worktree is created, for things Git does not do by itself such as initialising submodules or fetching Git LFS objects.

```toml
[git]
setup_command = "git submodule update --init --recursive && git lfs pull"
```

- **Linux only.** On other platforms a non-empty command is refused with an explanatory error.
- At most 8192 bytes, no NUL characters. Blank means no command.
- It runs in the new worktree, with a time limit of 120 seconds. Its output is drained but never kept, logged or shown, because setup may print secrets.
- If it fails or times out, the checkout and any copied files are kept and the failure is reported with the path. Fix the cause and finish the setup by hand.
- The dialog warns when the repository has submodules, because Git does not populate them in a new worktree.
- Settings shows the command text only. Editing it never runs it.

## Copy local files with `.worktreeinclude`

Untracked files such as `.env` are not present in a fresh worktree. List them in a `.worktreeinclude` file at the root of the **source** checkout and Ilium copies the matching files into each new worktree during the **Preparing files** stage.

```gitignore
.env
.env.local
config/secrets.json
```

Rules:

- The file uses the same pattern syntax as `.gitignore` and is matched relative to the source root.
- A missing file means nothing is copied.
- Only regular files are copied. Existing destination files and symbolic links are never followed or replaced.
- Limits: the include file at most 64 KiB, at most 1000 files and 64 MiB in total. Exceeding a limit, an invalid pattern or an unreadable file makes the preparation fail before the agent starts. Created files are removed again on failure.

## What the tree shows

- A worktree agent shows a branch line under its row (see **Branch line**). Hover for the provenance and the latest verified Git status.
- Branch and HEAD files are checked every ten seconds by one coordinator per session, with at most two probes at a time. A full status is requested when you hover the branch line and when an agent completes.
- Right-click a worktree pane to open **Worktree**: **Copy branch**, **Copy path**, **New terminal here**, **Open folder in sidebar** and **Remove worktree...**. Removal is disabled with a reason for panes whose worktree Ilium did not create. The confirmation reads: "Remove the worktree at ...? The server checks for changes and running processes before removal. The branch is kept."

## Clean up worktrees

Ilium never deletes a worktree without a check, and never silently. There are two entry points:

1. **When you close the pane.** If **Close policy** is `offer_removal_when_safe` (set per pane when it was created, or as the global default), closing offers removal when it is safe. The offer reads "Remove the clean, merged worktree at ... too?" with **Keep** as the default. The offer is advisory: the server repeats every safety check before removing anything. A missing policy means the worktree is kept.
2. **The worktree manager.** Right-click a project or a pane in it and choose **Manage worktrees...**. It lists every worktree registered in the repository with its blockers.

Manager keys:

| Key | Action |
| --- | --- |
| Up / Down | Select a worktree |
| `S` | Safe remove: only a clean, merged worktree |
| `D` | Discard files: remove a worktree that has uncommitted or untracked content |
| `R` | Refresh the inventory |
| `Esc` | Close |

**Safe remove** steps:

1. Select the worktree and press `S`. The prompt reads: Remove exact clean, merged worktree. Default is Cancel.
2. Press `B` to choose what happens to the branch (see below).
3. Press `Y` to confirm, or `Esc` to cancel.

**Discard files** steps:

1. Press `D`. The prompt asks you to type the worktree's full path exactly. Nothing is removed until the typed text matches character for character.
2. Press `Tab` to choose the branch behaviour, then `Enter` to confirm.

The result screen reports the outcome, whether anything was changed, and whether the path, Git registration and Git metadata are still present.

## Removal blockers and states

Ilium offers cleanup only for worktrees it created and can prove it owns. Anything unclear blocks removal rather than guessing. Each row in the manager shows its safe blockers and discard blockers; typical causes are:

- The checkout is the main worktree, bare, missing or prunable. These need manual repair.
- A foreign worktree: you may start agents in it but Ilium does not adopt or remove it.
- Ownership marker or metadata unreadable, or from a legacy format. Left in place for manual inspection.
- Uncommitted tracked changes (safe remove only), or ignored and untracked content (ordinary Git removal would discard some ignored files; Ilium checks them).
- Not merged into its recorded merge target (safe remove only).
- A live pane still references the worktree, or a process, or any other user of the directory, is still running there.
- A spawn ticket is still outstanding or the process-state scan could not prove the directory is unused. Safe removal requires zero tickets after processes have been supervised to exit.
- The repository is busy in another Ilium operation. Retry after it finishes.

Outcomes: a removal is *removed*, *blocked* (a blocker appeared), or *uncertain* (Git reported an error partway). On an uncertain result Ilium does not retry automatically and attempts no branch deletion or ownership rewrite. Inspect the path and Git state, then press `R` to refresh.

A retained Ilium worktree keeps its ownership marker after the pane closes, and regains that identity if you open an agent in it again.

## Branch deletion

Branch deletion is separate from worktree removal and is never implied by it. In the confirmation you choose either:

- **keep branch** (default), or
- **delete merged branch safely (-d)**, which uses `git branch -d` semantics and so refuses an unmerged branch.

Discarding files requires typing the full path, as described above.

## Limits and edge cases

- Creation, removal and the safety proofs depend on process inspection and Linux behaviour in places (the setup command is Linux only).
- The manager list is bounded. If the repository has more worktrees than fit, the header says the list is truncated.
- A base reference must be given; the dialog refuses an empty one. Choose the base before pressing create.
- The branch must be new. If the name exists, choose another name or use an existing worktree.
- Worktree paths that overlap an existing checkout or Git metadata are rejected.
- A project that is not in a Git repository has its worktree creation entries disabled with a reason.
- Restoring a session after a restart resumes worktree agents from their own directory; see [Session recovery](session-recovery.md).

## Troubleshooting

| Symptom | Explanation and fix |
| --- | --- |
| **In new worktree...** is greyed out | Not a Git repository, or the repository facts could not be read. Hover for the reason |
| "Branch ... already exists" | Choose a new branch name or use an existing worktree |
| "Worktree location must include {branch_slug}" | Fix the template in Settings -> Git |
| Setup command rejected | It is Linux only, longer than 8192 bytes, or contains a NUL character |
| Agent started but files like `.env` are missing | Add them to `.worktreeinclude` in the source checkout |
| Cannot remove a worktree | Open **Manage worktrees...** and read the blockers; stop any process using the directory, merge the branch, or use Discard files |
| "repository is busy in another Ilium operation" | Another create or remove is running; retry in a moment |
