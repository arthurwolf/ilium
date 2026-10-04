# Session recovery

Ilium keeps your work organised across detaches, restarts and crashes. Panes are owned by a background server, so detaching never stops them. If the server itself stops (a reboot, a crash, an upgrade), Ilium rebuilds the layout from a snapshot on disk and relaunches what each pane was running, resuming Claude Code, Codex or Antigravity conversations where it can prove which conversation belonged to the pane. This page explains what is saved and where, the rolling backups, what is and is not restored, the recovery-policy setting, how agent sessions are resumed, how to convert a session between Claude Code and Codex, the command-line switches that control sessions, and the limits.

Contents:

- [Detach, reattach and sessions](#detach-reattach-and-sessions)
- [What is saved and where](#what-is-saved-and-where)
- [Rolling backups](#rolling-backups)
- [What is restored after a restart](#what-is-restored-after-a-restart)
- [Recovery policy](#recovery-policy)
- [Resuming agent sessions](#resuming-agent-sessions)
- [Convert a session between Claude and Codex](#convert-a-session-between-claude-and-codex)
- [Command-line control](#command-line-control)
- [Limits](#limits)
- [Troubleshooting](#troubleshooting)

Related pages: [Worktrees](worktrees.md), [Agent monitoring](agent-monitoring.md), [CLI reference](cli-reference.md), [How it works](how-it-works.md), [Settings](settings.md).

## Detach, reattach and sessions

- A **session** is one server process plus its panes and tree. It is scoped to a **project**: the canonical directory you launch Ilium from (`--cwd`, default the current directory). A bare `ilium` owns that directory's session named `default`.
- `Ctrl+B d` detaches. Panes keep running. Run `ilium` in the project directory again to reattach.
- `Ctrl+B &` kills the session, ending its panes.
- You can run extra named sessions per project with `ilium new-session <name>` and list them with `ilium ls`.
- Sessions of different projects never affect each other.

## What is saved and where

| Item | Location |
| --- | --- |
| Session snapshot | `<project>/.ilium/sessions/<session name>.json` |
| Rolling backups | `<project>/.ilium/backups/<session name>/` |
| Global configuration | `~/.config/ilium/config.toml` on Linux |
| Server socket | A per-session socket under `$XDG_RUNTIME_DIR/ilium/` (or the OS temp directory) |

Add `.ilium/` to your `.gitignore` so session data stays out of Git.

The snapshot is a JSON description of the tree plus what each pane needs to be started again. It is written automatically after structural changes (creating, closing, moving or renaming entries, title updates, monitored task lifecycle changes), debounced by about 750 ms so bursts of changes become one write. It is a recovery aid, not a database: while the server runs, the live server is the source of truth. Snapshot writes are atomic (data is written to a temporary file and synced before replacing the old file), and files are owner-only.

What a snapshot contains per pane includes: its place in the tree, titles, the command or resume command to relaunch, the working directory (and worktree provenance for [worktree agents](worktrees.md)), pending scheduled-input deadlines, and recoverable progress monitors.

## Rolling backups

The native snapshot is overwritten as you work, so Ilium also keeps periodic copies. Toggle **Settings -> Session -> Automatic backups** (`[session].backups_enabled`, default on).

- A copy is taken when the server starts (before a restore or a "start fresh" can replace the snapshot) and then at most once per half-hour while the snapshot exists.
- Copies are named `<milliseconds>-<uuid>.json` inside `.ilium/backups/<session>/` and are published atomically.
- Older copies are thinned automatically, keeping the newest per time slot:

| Age of copy | Kept |
| --- | --- |
| Under 1 day | One per half hour, up to 48 |
| Under 7 days | One per day, up to 7 |
| Under 35 days | One per week, up to 4 |
| Older, within about 12 months | One per month, up to 12 |
| Older still | One per year (no limit) |

- Turning backups off stops new copies and keeps existing ones. Turning them on again takes a fresh copy at once.
- Backups copy the session snapshot only. They exclude files edited inside pane applications, so they are not a backup of your work.
- Retention is thinning, not indefinite: do not rely on a specific old state surviving.

### Restore from a backup by hand

1. Detach, and end the session if it is running (`ilium kill-session default`).
2. Pick a file in `.ilium/backups/default/`. The number in the name is a Unix timestamp in milliseconds.
3. Copy it over `.ilium/sessions/default.json`.
4. Run `ilium`. The normal recovery policy applies on start.

## What is restored after a restart

When the server starts and finds a snapshot, it replaces the empty tree with the snapshot tree and relaunches each pane:

- The layout: projects, groups, folders, split views, order, titles.
- Terminal panes are started again with their saved command in their saved working directory.
- Detected Claude Code, Codex and Antigravity panes are relaunched with their provider's resume command (see below).
- Editor panes reopen the file path they had (or an empty picker if none was chosen). Unsaved editor buffers are not part of the snapshot.
- Pending scheduled inputs keep their deadlines, and conservatively recoverable progress monitors are re-established.
- A pane whose command can no longer be started (for example its program was uninstalled) is logged and dropped rather than left as an empty node.
- A worktree pane whose directory is missing stays visible and is not resumed from a different checkout.

What does not survive:

- **Unsaved process state.** A shell's variables, a running program's memory, a build in progress. Programs are relaunched, not suspended.
- Terminal scrollback is not restored: a relaunched program starts with a fresh screen.
- Anything an agent was doing in its own session beyond what the provider itself stores.

## Recovery policy

**Settings -> Session -> Recovery policy** (`[session].recovery_policy`) decides what the server does with a prior snapshot at its next start.

| Policy | Value | Effect |
| --- | --- | --- |
| Restore automatically (default) | `restore_automatically` | Rebuild the tree and relaunch panes without asking |
| Ask before restoring | `ask_before_restore` | The server starts with an empty tree and, when a client attaches, asks "Restore previous session?" showing the number of stored panes. Choose **Restore** or **Discard** |
| Start fresh | `start_fresh` | Ignore the snapshot and start with an empty tree. The previous snapshot is backed up first when backups are enabled |

```toml
[session]
recovery_policy = "restore_automatically"
backups_enabled = true
```

An unrecognised value is a configuration error, not a silent fallback, because this setting controls a safety decision. Changing the policy affects the next start, not the running session.

While an "ask" decision is pending you can already create panes. Restoring then replaces the tree, and panes created in the meantime are cleaned up instead of leaking.

## Resuming agent sessions

During normal use Ilium notices which conversation each detected agent is running and stores a provider-specific resume command in the snapshot. After a restart the pane is relaunched with that command, so the agent opens its own conversation again instead of a blank one.

| Agent | Command used |
| --- | --- |
| Claude Code | `claude --resume <session id>` |
| Codex | `codex resume <session id>` |
| Antigravity | `agy --conversation <conversation id>` |

Conditions:

- A resume command is saved only when Ilium has **verified** the session (the identifier belongs to this pane's agent, project and launch directory). Otherwise the pane just starts the agent fresh.
- Resume works only while the provider still has the data. If a transcript was deleted or rotated, the agent cannot resume it.
- Resuming restores the conversation, not process state. Commands the agent started earlier are not running any more.
- Claude Code may show a "resume full session" prompt. Ilium answers that known prompt once per agent process (setting `auto_answer_interstitial_prompts` under `[detection]`).
- Detected agents in a plain terminal that were launched with other flags are relaunched from their persisted origin; custom signatures have no resume contract.

## Convert a session between Claude and Codex

You can continue a Claude Code conversation under Codex, or a Codex conversation under Claude Code, without starting over.

1. Right-click the agent's terminal pane in the tree and choose **Convert to Codex** (for a Claude pane) or **Convert to Claude Code** (for a Codex pane). The entry appears only when the pane has a verified session, no other conversion is running, and the pane is not a worktree pane.
2. Ilium checks that the source transcript exists, freezes the pane, opens the **Convert ... session to ...** dialog and stops the agent process.
3. The dialog shows each step with progress and a log. Press `Esc` to cancel while it is converting.
   - Claude to Codex: locate the Claude Code transcript, inspect it, start a private `codex app-server`, import the session using Codex's own importer, verify the Codex rollout, stop the app-server.
   - Codex to Claude: locate the Codex rollout, parse it, convert it to Claude Code lines, write the Claude Code transcript, verify it, register it with Claude Code. User prompts, assistant text and shell or tool calls with results are carried over. Reasoning and token events are dropped.
4. On success the frozen pane is replaced by a new pane that resumes the converted session in the other agent.
5. If conversion fails and the original agent had already been stopped, press `Enter` to resume the original session, or `Esc` to close the dialog.

The original transcript is read, not modified. Antigravity sessions cannot be converted.

## Command-line control

| Command or flag | Effect |
| --- | --- |
| `ilium` | Attach to this project's `default` session, starting it (and restoring the snapshot per policy) if needed |
| `ilium new-session <name>` | Create if necessary and attach to a named session |
| `ilium ls` | List this project's sessions and whether each is running |
| `ilium --restart-server` | Replace this project's running server before attaching, **keeping the snapshot**. Use it after installing a new server binary |
| `ilium --reset-session` | **Delete** this project's named session snapshot and start empty. Destructive; affects only this project. Cannot be combined with `--restart-server` |
| `ilium kill-session <name>` | Gracefully end the session and every pane in it |
| `ilium --cwd <dir> ...` | Address a different project |

Notes:

- `--restart-server` restarts only the server process for this project. Panes are relaunched from the snapshot, so running programs are restarted, not carried over. Be aware that it interrupts what your panes are doing.
- After you install a new build, a server that is already running keeps the old executable loaded until it is restarted.
- `kill-session` waits until the server confirms the shutdown, then prints `session "<name>" killed`. If no session of that name is running it reports an error.
- Both `--restart-server` and `--reset-session` apply to the project given by `--cwd`.

## Limits

- Process state does not survive: only layout and relaunch information do.
- Recovery depends on provider data (Claude Code, Codex and Antigravity keep their own conversations). Ilium resumes only what it can verify.
- Snapshot and backup files live inside the project directory and are owner-only. They are not encrypted.
- Backups are thinned over time and cover the snapshot, not the files you edit.
- One server serves exactly one session; there is no cross-project registry.
- Machine restarts end all panes: Ilium relaunches them at the next start.

## Troubleshooting

| Symptom | Explanation and fix |
| --- | --- |
| Panes are empty after a restart | The pane had no verified agent session, or the provider data is gone. The agent starts fresh |
| Nothing restored | Recovery policy is Start fresh, or you chose Discard. Restore a copy from `.ilium/backups/<session>/` |
| I see "Restore previous session?" | Policy is Ask before restoring. Choose Restore to rebuild or Discard to start fresh |
| A pane vanished on restore | Its command could no longer be spawned and was dropped. Check the program is still installed |
| New server binary not in use | Run `ilium --restart-server` (this restarts the project's server and relaunches panes) |
| Session data appears in `git status` | Add `.ilium/` to `.gitignore` |
| Conversion entry missing | Not a Claude or Codex agent, no verified transcript yet, a worktree pane, or another conversion is running |
