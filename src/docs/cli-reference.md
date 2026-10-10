# CLI reference

The `ilium` command attaches the terminal interface to a project session and offers short one-shot subcommands for scripts and agents: listing and ending sessions, adding panes, starting agents in Git worktrees, the file-backed agent Chatroom, long-task progress reporting, typed voice input, and listing or messaging agents across every running session. This page documents every subcommand, flag, output format, environment variable and exit behaviour, with examples.

Contents:

- [Synopsis](#synopsis)
- [Global options](#global-options)
- [Quick table of commands](#quick-table-of-commands)
- [ilium (attach)](#ilium-attach)
- [ilium new-session](#ilium-new-session)
- [ilium ls](#ilium-ls)
- [ilium kill-session](#ilium-kill-session)
- [ilium new-pane](#ilium-new-pane)
- [ilium chat](#ilium-chat)
- [ilium progress](#ilium-progress)
- [ilium voice say](#ilium-voice-say)
- [Pane selection](#pane-selection)
- [ilium panes](#ilium-panes)
- [ilium broadcast](#ilium-broadcast)
- [Environment variables](#environment-variables)
- [Output formats and exit status](#output-formats-and-exit-status)
- [Other binaries and hidden commands](#other-binaries-and-hidden-commands)
- [Recipes](#recipes)
- [Troubleshooting](#troubleshooting)

## Synopsis

```text
ilium [OPTIONS] [COMMAND]

Commands:
  new-session   Create (if not already running) and attach to a named session
  ls            List this project's known sessions and whether each is running
  kill-session  Gracefully end a running session
  new-pane      Add a pane running a command, without attaching the interface
  chat          Read, initialize, or post to this project's file-backed agent room
  progress      Report or clear a long-running task's progress from inside a pane
  voice         Voice control from the command line (`voice say`)
  panes         List the panes of every running session that pass the pane selection
  broadcast     Send a message to every running agent that passes the pane selection
  help          Print help for the program or a subcommand
```

`ilium --help` and `ilium <command> --help` print the same information from the program itself, and `ilium --version` prints the version.

## Global options

These options are defined once and accepted by every subcommand.

| Option | Default | Meaning |
| --- | --- | --- |
| `--cwd <CWD>` | `.` | Project directory for the session being attached to or created. Every pane starts here and the editor file picker opens rooted here. Every command that addresses a session is scoped to this canonical project directory. The directory must exist. |
| `--restart-server` | off | Replace this project's running server before attaching, keeping its snapshot. See below. |
| `--reset-session` | off | Delete this session's snapshot and start it empty. Destructive. Conflicts with `--restart-server`. |
| `--onboarding` | off | Open the guided setup even when this installation is already configured. |
| `-h`, `--help` | | Print help. |
| `-V`, `--version` | | Print the version. |

Which commands use which option:

- `--cwd` is used by every command that addresses a session or a project: bare `ilium`, `new-session`, `ls`, `kill-session`, `new-pane`, `chat` and `voice say`. `panes` and `broadcast` scan every session on the machine and use it only for `--here` and relative `--project` values. `progress` ignores it; it addresses the pane it is running in through its environment (see [ilium progress](#ilium-progress)).
- `--restart-server`, `--reset-session` and `--onboarding` only act when attaching (bare `ilium` and `ilium new-session`). The other subcommands accept them but ignore them.

### --restart-server

Stops this project session's running server (found through the socket peer, so it can never affect another project) and starts a new one from the current executable. The server first saves its tree and exits; the new server restores that snapshot. Use it after installing a new `ilium-server` binary. Because the old server's processes end, panes are relaunched from the snapshot, as after a reboot. Programs lose any unsaved process state. See [Session recovery](session-recovery.md).

### --reset-session

Stops the server, deletes this session's snapshot, and starts an empty server. For the `default` session it also removes the older project-wide `.ilium/sessions.yml` if present, so the reset really is empty. It never touches another project or another named session. There is no confirmation prompt.

```sh
ilium --reset-session
ilium --reset-session new-session review
```

## Quick table of commands

| Command | What it does | Output |
| --- | --- | --- |
| `ilium` | Attach to, or create, the current project's `default` session. | Interactive interface |
| `ilium new-session <name>` | Create or attach to a named session. | Interactive interface |
| `ilium ls` | List this project's sessions. | Plain text |
| `ilium kill-session <name>` | End a running session and its panes. | Plain text |
| `ilium new-pane -- <cmd>` | Add a terminal pane running a command. | Plain text |
| `ilium new-pane --worktree --branch <b> -- <agent>` | Start a built-in agent in a new Git worktree. | JSONL |
| `ilium chat init` | Create `CHATROOM.md` and the agent hooks. | Plain text |
| `ilium chat send --message <text>` | Post a message to the room. | Plain text |
| `ilium chat context` / `ilium chat tail` | Print recent room records. | Plain text |
| `ilium progress check\|set\|status\|clear` | Manage this pane's long-task monitor. | JSONL |
| `ilium voice say <sentence>...` | Type sentences into the live voice session. | JSONL |
| `ilium panes [selection]` | List panes across every running session. | JSONL |
| `ilium broadcast [selection] <message>...` | Send a message to every selected agent. | JSONL |

## ilium (attach)

```sh
ilium [--cwd <dir>] [--restart-server | --reset-session] [--onboarding]
```

1. Resolves the canonical project directory from `--cwd`.
2. Ensures a server is running for the session `default`, starting `ilium-server` as a detached background process if not. The client waits up to five seconds for the server to become ready.
3. Attaches the interface. It stays attached until you detach (`Ctrl+B` `d`), kill the session (`Ctrl+B` `&`), or the connection drops.

Running `ilium` again in the same directory re-attaches to the same session. Several clients can attach at once.

The interface itself can restart only the client process (for example from a menu **Restart** entry): the detached server keeps its panes and tree throughout. The restarted client is started again with just the project and session identity, so `--restart-server` and `--reset-session` never carry over.

## ilium new-session

```sh
ilium new-session <name>
```

Creates, if it is not already running, and attaches to the session `<name>` for the current project. Names must be 1 to 48 characters from letters, digits, hyphen and underscore. Anything else fails with `session name "..." must contain only letters, digits, hyphens, or underscores`.

```sh
ilium new-session review
ilium --cwd ~/code/api new-session hotfix
```

Sessions of one project are independent trees with separate snapshots in `<project>/.ilium/sessions/<name>.json`.

## ilium ls

```sh
ilium ls
```

Lists this project's sessions, one per line, with the name padded to 24 characters followed by `running` or `not running`:

```text
default                  running
review                   not running
```

Prints `no sessions` when there are none. A session whose server has stopped stays listed as long as its snapshot file exists, so you can deliberately reopen it. Only sessions of the project given by `--cwd` are shown, and snapshot files whose names are not valid session names are skipped.

## ilium kill-session

```sh
ilium kill-session <name>
```

Gracefully ends a running session: every pane is killed and the tree is torn down. The command first checks that the session is live, verifies the server attached to the session you named (it refuses to send the destructive request to a server serving a different session name), sends the request, and waits up to five seconds for the server to close the connection. On success it prints:

```text
session "review" killed
```

Errors: `session "<name>" is not running` when no server answers; other server-reported errors are shown as `ilium: the server reported an error: ...`. The snapshot is not deleted by this command; use `--reset-session` for that.

To end a session from inside the interface use `Ctrl+B` `&`, which does the same without a confirmation.

## ilium new-pane

```sh
ilium new-pane [--keep-open] [--session-name <name>] -- <cmd> [args...]
ilium new-pane --worktree --branch <branch> [--base <ref>] [--session-name <name>] -- <agent>
```

Adds a pane to a running or starting session without attaching the interface. Run `ilium` afterwards to see it. If the session's server is not running, `new-pane` starts it.

| Option | Default | Meaning |
| --- | --- | --- |
| `--session-name <name>` | `default` | Project-local session that receives the pane. |
| `--keep-open` | off | Keep the pane open after the command exits. Without it, a plain `new-pane` command pane closes itself shortly after its command ends (at least 3 seconds after creation, and not while a live progress monitor is registered on it). Cannot be combined with `--worktree`. |
| `--worktree` | off | Start a built-in agent in a new Git worktree on its own branch. Requires `--branch`. |
| `--branch <branch>` | | New branch for `--worktree`. Must not already exist. Requires `--worktree`. |
| `--base <ref>` | the repository's default base | Starting ref for `--worktree`. Requires `--worktree`. |
| `-- <cmd>...` | required | The command and its arguments. Everything after `--` is taken literally. |

### Plain panes

The command and arguments are joined into one shell command line, quoting only the arguments that need it, so boundaries are kept: `ilium new-pane -- ls "my folder"` runs `ls 'my folder'`. The server runs it as `$SHELL -c <command line>` (or through `cmd.exe` on Windows when `SHELL` is not a POSIX shell) and shows the command as the pane's name. The pane starts in the project root and is added to the top level of the tree. Because the whole command line is passed to your shell, shell syntax works only when you quote it into a single argument.

```sh
ilium new-pane -- claude
ilium new-pane --session-name review -- codex
ilium new-pane -- tail -f /var/log/syslog
ilium new-pane -- sh -c 'npm run build && npm test'
```

Output on success is one plain-text line, not JSON:

```text
pane created in project session "default"
```

The command waits up to five seconds for the server to confirm that the pane count grew. If no confirmation arrives it fails with `no confirmation received from the server`.

### Worktree panes

`--worktree` starts an agent in a linked Git worktree so it cannot disturb your working tree or other agents. Exactly one command is accepted, and it must be a built-in agent command: `claude`, `codex` or `agy` (Antigravity). Anything else fails with `--worktree requires exactly one built-in agent command: claude, codex, or agy`.

```sh
ilium new-pane --worktree --branch agent/fix-login -- codex
ilium new-pane --worktree --branch agent/spike --base main -- claude
```

What happens:

1. The branch name is validated with Git's naming rules (not empty, no spaces, `~ ^ : ? * [ \`, control characters, `..`, `@{`, `//`, no leading `-`, no trailing `/` or `.`, no component starting with `.` or ending in `.lock`). Invalid names fail before anything is created.
2. The persisted **Settings → Git** setup command is read, if one is set. A malformed `config.toml` makes the command fail rather than silently skipping setup.
3. The server is started if needed, and the command attaches, then asks the server for the repository facts.
4. The worktree is created at a sibling directory of the main checkout: `<parent>/<main-checkout-name>.worktrees/<branch-slug>`, where the slug is the branch lower-cased with runs of non-alphanumeric characters replaced by `-` (at most 48 characters, `task` if nothing is left). For example, branch `agent/fix-login` in `/home/me/code/app` becomes `/home/me/code/app.worktrees/agent-fix-login`. Unlike the interactive dialog, the CLI does not use the configurable location template or branch prefix.
5. The setup command, if configured, runs after creation (Linux only), then the agent starts.
6. Git hooks and filters run as they normally do during creation.

The new branch must not exist. `--base` defaults to the repository's default base ref. Cleanup of worktrees Ilium created is offered later when they are clean, merged and unused; unclear ownership or process state blocks removal. See [Worktrees](worktrees.md).

Worktree output is JSONL on stdout, one object per line:

| `type` | Fields | When |
| --- | --- | --- |
| `progress` | `request_id`, `stage` | At each stage: `querying-repository`, then `creating-worktree`, `preparing`, `running-setup` (only with a setup command) and `starting`. |
| `result` | `request_id`, `pane_id`, `branch`, `base`, `worktree_path` | The agent pane exists. |
| `error` | `request_id`, `message` | Any failure. The process then exits non-zero. |

Example:

```text
{"type":"progress","request_id":4821,"stage":"querying-repository"}
{"type":"progress","request_id":4821,"stage":"creating-worktree"}
{"type":"progress","request_id":4821,"stage":"starting"}
{"type":"result","request_id":4821,"pane_id":17,"branch":"agent/fix-login","base":"main","worktree_path":"/home/me/code/app.worktrees/agent-fix-login"}
```

Timeouts: 5 seconds to attach, 30 seconds for the repository query, 180 seconds for creation. When the creation confirmation times out the message says creation may still have completed, so check the session and `git worktree list` before retrying.

## ilium chat

Chatroom is a file-backed message log (`CHATROOM.md` in the project root) that agents use to coordinate. These commands work on the file directly and do not need a running server or an attached interface. See [Automation](automation.md).

```sh
ilium chat init
ilium chat send --message <text> [--author <name>]
ilium chat context [--limit <n>] [--since-last-read] [--max-bytes <n>] [--reader <key>]
ilium chat tail [--limit <n>]
```

| Subcommand | Option | Default | Behaviour |
| --- | --- | --- | --- |
| `init` | | | Creates `CHATROOM.md` if it does not exist, adds `/CHATROOM.md` and `/CHATROOM.archive.md` to `.gitignore`, and installs hook entries for Claude (`.claude/settings.local.json`) and Codex (`.codex/hooks.json`) on `SessionStart` and `UserPromptSubmit`, so agents see recent messages at the start of a session and before each prompt. Existing content in those files is preserved. Refuses to use a symlinked or non-regular `CHATROOM.md`. Prints `chatroom ready at <path>`. |
| `send` | `--message <text>` (required) | | Appends one record. Fails if the project has no chatroom, if the author or message is empty, or if the message is longer than 4000 characters. Prints `chatroom message sent`. |
| `send` | `--author <name>` | `$ILIUM_CHATROOM_AUTHOR`, then `$AGENT_NAME`, then `agent` | The name recorded with the message. |
| `context` | `--limit <n>` | 40 | Prints the last `n` records preceded by posting guidance, in the form injected into an agent turn by the lifecycle hooks. Prints an empty-room notice with the guidance when there are no messages. |
| `context` | `--since-last-read` | off | Prints only records the reader has not seen yet and nothing when none are new. The reader is `--reader`, else the `ILIUM_PANE_ID` pane, else the `session_id` of the hook input on stdin. A reader seen for the first time, or a `SessionStart` hook, gets the full form with four times the `--max-bytes` budget. Without a reader key it behaves like plain `context`. |
| `context` | `--max-bytes <n>` | unlimited | Caps the output size. Newest records win, a single oversized newest record is cut, and skipped records are counted in a closing note that points to `ilium chat tail`. |
| `context` | `--reader <key>` | | Explicit reader key for `--since-last-read`. Read positions are stored under `.ilium/chat-readers/`. |
| `tail` | `--limit <n>` | 100 | Prints the last `n` records for reading in a terminal, one per line: `<timestamp> \| <author> \| <content>`. |

Details:

- Timestamps are local time with an offset, like `2026-10-04 14:32:10 +02:00`.
- `send`, `context` and `tail` look for the nearest `CHATROOM.md` in the working directory or any parent, so they work from a subdirectory of the project. If none exists `send` and the read commands fall back to the current directory (where `send` then fails because no room exists).
- Appends take an advisory lock, so several agents can post at once without interleaving.
- Fields are sanitised to one line each; control characters in a human-edited file are neutralised before they reach a model.
- A row for the Chatroom also appears in the project tree when a project has one.

Examples:

```sh
ilium chat init
ilium chat send --author claude --message "Taking the auth refactor in src/auth. Please avoid it."
ilium chat tail --limit 20
ilium chat context --limit 40
```

## ilium progress

`ilium progress` lets an agent (or a script it wrote) report a long-running task to the pane it runs in. The agent registers a cheap probe command; the Ilium server runs that probe on an interval and shows the result in the pane footer, in the tree and in notifications. Because the server, not the agent, does the polling, the agent can stop spending turns on status checks.

```sh
ilium progress check  --command '<probe>'
ilium progress set    --command '<probe>' [--interval-seconds <n>] [--wait [--timeout-seconds <n>]] [--replace]
ilium progress wait   [<monitor_id>] [--timeout-seconds <n>]
ilium wait            [<monitor_id>] [--timeout-seconds <n>]
ilium progress status
ilium progress clear  [--monitor-id <id> | --all]
```

Requirements:

- It must run **inside an Ilium terminal pane**. The server injects the environment that identifies the pane. Outside a pane the command prints a `progress_request_failed` record with code `pane-identity-unavailable` and exits non-zero with `not running inside an ilium-managed pane: ILIUM_PANE_ID is not set`.
- The progress monitor feature must be enabled (**Settings → Appearance → Progress monitor**, `progress_monitor_enabled` in `[ui]`; on by default). When it is off, requests are rejected with code `disabled`.
- A pane holds up to 8 monitors at once, one per task, in registration order. The server keeps them in memory and in the session snapshot; a cleared monitor cannot be resurrected by a stale agent (see `--monitor-id`).

### The probe contract

The probe is a shell command line (at most 16 KiB). It must print exactly one JSON object on stdout and exit 0:

| Field | Required | Rules |
| --- | --- | --- |
| `job_id` | yes | A stable, non-empty identifier of the task, at most 256 bytes. It must not change between probes of one monitor. |
| `status` | yes | One of `not-started-yet`, `running`, `error`, `done`. |
| `percent` | yes | A finite number from 0 to 100. `done` is shown as exactly 100; any other value for `done` is rejected. |
| `message` | recommended | One line, at most 2048 bytes, always visible in the pane footer. It should make sense to someone who has not read the agent's session. |
| `details` | optional | Multi-line text, at most 8192 bytes, shown when the user hovers the footer. |
| `error` | required when `status` is `error`, forbidden otherwise | Text, at most 4096 bytes. |

Limits: the probe may run for at most 30 seconds, produce at most 64 KiB on stdout and 16 KiB on stderr, and must exit successfully. A task that reports `error` is a task failure; a probe that cannot be run or read is a monitor failure, and the two are reported separately. After repeated consecutive observation failures (up to three) the monitor is marked failed rather than the task.

**Working directory and environment.** The server spawns the probe, not your shell. It does not inherit the pane's current directory, exported variables or aliases. It starts in the server's fixed working directory, the session's project root. Use absolute paths, or an unconditional `cd /absolute/path && ...` at the start.

**Cost.** The probe is a brand-new process on every tick, for as long as the monitor exists. Keep it cheap: read a small status file or counter, avoid walking trees, avoid network calls. If the check is inherently heavy, raise `--interval-seconds` instead of letting it run every second.

### progress check

Runs one probe through the server and validates it, without installing anything. Always do this before `set`.

```sh
ilium progress check --command 'cat /tmp/build.status'
```

On success prints:

```text
{"type":"progress_check","request_id":...,"pane_id":12,"checked_at_unix_millis":...,"report":{"job_id":"build-7","status":"running","percent":42,"message":"Compiling crate 14 of 22","details":"","error":null}}
```

### progress set

Validates the probe, then atomically adds a monitor to this pane, and waits for a correlated acknowledgement carrying the monitor ID and the accepted first report. Silence is never treated as acceptance. Other monitors of the pane are not touched. Registering the exact command of a monitor that is still running returns that monitor instead of adding a second one. Settled monitors whose outcome was already delivered make room for new ones; a ninth monitor that is still needed is refused with `too-many-monitors`.

| Option | Default | Meaning |
| --- | --- | --- |
| `--command <probe>` | required | The probe command line. |
| `--interval-seconds <n>` | 1 | How often the probe re-runs, from 1 to 86400 (24 hours). |
| `--wait` | off | After the acknowledgement line, block like `ilium progress wait <monitor_id>` and print its `progress_wait` line. |
| `--timeout-seconds <n>` | none | With `--wait`: stop waiting after this many seconds (exit 6). Use a value below your shell tool's command time limit. |
| `--replace` | off | Clear every monitor of this pane first. Use only on purpose. |

```sh
ilium progress set --command 'cat /tmp/build.status' --interval-seconds 5
```

```text
{"type":"progress_set","request_id":...,"pane_id":12,"monitor_id":3,"progress":{"monitor_id":3,"report":{...},"monitor_health":{"state":"healthy"},"last_observed_unix_millis":...}}
```

Keep the `monitor_id`. After this, do not poll: the server watches the task and tells the agent when it reaches `done` or `error`. A live progress monitor can also suppress "agent finished" alerts while work continues. See [Agent monitoring](agent-monitoring.md) and [Notifications](notifications.md).

### progress wait

Blocks until one monitor settles, then prints one `progress_wait` record and exits with a status an agent can act on. `ilium wait` is the same command. This is the normal way for an agent to wait: one blocking command instead of ending its turn or polling.

| Argument | Meaning |
| --- | --- |
| `<monitor_id>` | The monitor to wait for. Optional when the pane has exactly one monitor; with several, the command refuses with `monitor-ambiguous` (exit 2) and lists their IDs in `monitor_ids`. |
| `--timeout-seconds <n>` | Stop waiting after this many seconds. The monitor keeps running; run the same command again to keep waiting. |

```text
{"type":"progress_wait","pane_id":12,"monitor_id":3,"outcome":"done","exit_code":0,"waited_seconds":412,"composer_notice":"suppressed","next":"...","progress":{...}}
```

| Exit | `outcome` | Meaning |
| --- | --- | --- |
| 0 | `done` | The task reported `done`. |
| 2 | (`progress_rejected`, code `monitor-ambiguous`) | No ID given and the pane has several monitors. |
| 3 | `error` | The task reported `error`. |
| 4 | `monitor-failed` | The probe stopped working; the task outcome is unknown, so check the task directly. |
| 5 | `cleared`, `no-monitor` | The monitor was cleared while waiting, or does not exist. |
| 6 | `still-running` | `--timeout-seconds` elapsed. |

While a wait is held, the server hands the outcome to it instead of typing a notification into the agent's prompt (`composer_notice` is `suppressed`). If the wait is killed first, the typed notification still arrives; `may-also-arrive` means a typed message about the same monitor is a duplicate.

### progress status

Prints every monitor of the pane, in registration order, with its latest report and health.

```text
{"type":"progress_status","request_id":...,"pane_id":12,"monitor_count":2,"running":true,"monitors":[{"monitor_id":3,...},{"monitor_id":4,...}]}
```

`monitors` is empty when the pane has no monitor. `running` is `true` while any monitor's task is still live. `monitor_health.state` is `healthy`, `degraded` or `failed`; the last two carry `consecutive_failures` and `last_error`.

### progress clear

Stops monitors and clears their retained results.

| Option | Meaning |
| --- | --- |
| `--monitor-id <id>` | Clear only this monitor. If it no longer exists, the request is rejected with `stale-monitor` and the other monitors are untouched. |
| `--all` | Clear every monitor of the pane. |
| (neither) | Clears the only monitor; refused with `monitor-ambiguous` when the pane has several. |

```text
{"type":"progress_clear","request_id":...,"pane_id":12,"cleared":true,"cleared_monitor_ids":[3]}
```

### Failures

Every failure is reported on stdout as one JSON record and the process exits non-zero.

| `type` | Meaning |
| --- | --- |
| `progress_rejected` | The server refused the request. Fields: `operation`, `request_id`, `pane_id`, `code`, `message`. |
| `progress_request_failed` | The request never completed. Fields: `operation`, `request_id`, `pane_id` (or `null`), `code`, `message`. Codes: `pane-identity-unavailable`, `connection-failed`, `request-send-failed`, `transport-error`. |

Rejection codes: `disabled`, `invalid-request`, `invalid-probe-report`, `probe-spawn-failed`, `probe-timed-out`, `probe-exited-non-zero`, `probe-output-too-large`, `probe-io-failed`, `pane-not-found`, `stale-monitor`, `too-many-monitors`, `monitor-ambiguous`. The CLI waits up to 45 seconds for the server's correlated reply.

### A complete example

```sh
# 1. Start the long job so its status file exists.
./run-tests.sh > /tmp/tests.log 2>&1 &

# 2. A cheap probe that prints one JSON object. Absolute paths only.
probe='cat /tmp/tests.status.json'

# 3. Validate, then register and block until the task settles.
ilium progress check --command "$probe"
ilium progress set   --command "$probe" --interval-seconds 5 --wait --timeout-seconds 590

# 4. If it printed exit 6 (still running), keep waiting on the monitor ID it printed:
ilium progress wait 3 --timeout-seconds 590

# 5. After handling the result:
ilium progress clear --monitor-id 3
```

## ilium voice say

```sh
ilium voice say [--start] [--session-name <name>] [--timeout-s <seconds>] <SENTENCE>...
printf '%s\n' "focus the first agent" "say hello to it" | ilium voice say -
```

Types sentences into the running voice conversation as if they had been spoken. Each sentence becomes one user turn, in order; the voice model interprets it and acts on it (for example by typing into the focused agent), and typed text can be mixed freely with live microphone audio. See [Voice](voice.md).

| Argument or option | Default | Meaning |
| --- | --- | --- |
| `<SENTENCE>...` | required | Sentences, in order. A lone `-` reads one sentence per non-empty line from standard input, in place. Put `--` first when a sentence starts with a hyphen. |
| `--start` | off | Switch voice control on when it is off (persisted, exactly as pressing `F8` does), or restart a session that failed to start, instead of failing with `voice-off`. |
| `--session-name <name>` | `default` | Session to address when not run from inside an Ilium pane. |
| `--timeout-s <n>` | 30 | How long to wait for the voice session to accept the text, 1 to 600. Starting voice opens audio devices and the provider connection first, so allow longer with `--start`. |

Rules:

- The voice session lives in an **attached interactive client**: it owns the microphone, the provider connection and the tool executor. The command never starts a server and fails with `session-not-running` or `no-voice-client` when no client is attached.
- Run from inside an Ilium pane it addresses that pane's session (`session_source` is `pane-env`). Elsewhere it uses `--cwd` and `--session-name` (`session_source` is `cwd`).
- Each sentence is trimmed and bounds-checked: at most 32 sentences per call, each at most 4000 characters, none empty.
- A result means the text is in the live session's queue. The provider sends no per-turn acknowledgement, so **check the target for the outcome**. Destructive voice actions still ask for confirmation, and terminal submission includes Enter unless you enabled the confirmation option.
- A server started before this command existed cannot answer it; restart Ilium to load the current server.

Output is JSONL: first a `progress` record, then exactly one `result` or `error`:

```text
{"type":"progress","command":"voice say","stage":"sending","request_id":...,"session":"default","socket":"/run/user/1000/ilium/....sock","session_source":"cwd","sentence_count":2,"start":true}
{"type":"result","command":"voice say","ok":true,"request_id":...,"session":"default","sentence_count":2,"accepted_sentences":2,"voice_phase":"listening","started_voice":true,"delivery":"queued-to-voice-session"}
```

`voice_phase` is `connecting`, `listening`, `recording`, `thinking` or `speaking`. An `error` record carries `code`, `message` and a `hint`:

| Code | Meaning and hint |
| --- | --- |
| `invalid-request` | No sentences, an empty sentence, too many or too long. |
| `no-voice-client` | Attach an interactive client with `ilium`. |
| `voice-off` | Pass `--start`, or press `F8` in the client. |
| `voice-unavailable` | Fix the voice settings (API key, audio devices) in **Settings → Voice control**. |
| `client-unresponsive` | The attached client did not answer; is it frozen or suspended? |
| `invalid-session` | The session name or `--cwd` is not valid. |
| `session-not-running` | Start Ilium in this project with `ilium`, or pass `--cwd` and `--session-name`. |
| `connection-failed`, `request-send-failed`, `server-closed-connection` | The server was unreachable or stopped answering; check `ilium ls`. |
| `timeout` | No answer within `--timeout-s`. |
| `stdin-unreadable` | The `-` input could not be read. |

The process exits non-zero after an `error` record. Human-readable diagnostics go to stderr.

## Pane selection

`ilium panes` and `ilium broadcast` share one set of selection options, so a selection means the same thing in both. Use `ilium panes` with a selection first to see exactly which panes `ilium broadcast` would reach.

Both commands look at **every running session on this machine**, in every project: they list the live sockets in the session socket directory and read each session's pane tree in turn. A session started under a different `ILIUM_SOCKET_DIR` or `XDG_RUNTIME_DIR` is not visible.

| Option | Meaning |
| --- | --- |
| `-m`, `--match <TEXT>` | Keep panes whose searched fields contain TEXT, ignoring case. |
| `-e`, `--regex <PATTERN>` | Keep panes whose searched fields match the regular expression (Rust `regex` syntax, unanchored, case-insensitive). |
| `--field <FIELD>` | Fields searched by `--match` and `--regex`: `name` (title and short title), `project`, `cwd`, `agent`, `session`. Default: all. |
| `-v`, `--invert` | Keep panes that no `--match` or `--regex` matches. Needs at least one of them. |
| `-p`, `--project <PROJECT>` | Keep panes in these projects. A value with a path separator, or `.`, `..`, `~`, is a directory and selects every project at or below it (relative values are resolved against `--cwd`). A bare value is a project folder name, compared ignoring case. |
| `--exclude-project <PROJECT>` | Drop panes in these projects (same syntax). Exclusion wins over `--project`. |
| `--here` | Keep only panes of the project that contains `--cwd` (the current directory by default). |
| `-s`, `--session <NAME>` | Keep panes in these session names. |
| `-a`, `--agent <AGENT>` | Keep panes running these agents: `claude`, `codex`, `antigravity`, or a custom agent's name. |
| `--state <STATE>` | Keep agents in these states: `working`, `waiting-approval`, `waiting-subagents`, `settling`, `idle` (includes `done`), `done` (finished a turn nobody has looked at yet). |
| `--kind <KIND>` | Keep panes of these kinds: `agent`, `shell`, `editor`, `board`, `unavailable-agent` (an agent pane whose program is not running). |
| `--pane <ID>` | Keep these pane ids. Ids are unique only within one session, so combine with `--session` or `--project` when several sessions run. |

How options combine:

- Different options narrow the selection (AND). Repeating an option, or giving it comma-separated values, widens that option (OR): `--project ilium,lumen` and `--project ilium --project lumen` are the same.
- `--match` and `--regex` together form the text filter: a pane passes when **any** of them matches **any** searched field. `--invert` flips only the text filter, never the other options.
- An invalid regular expression, `--invert` without a pattern, or `--here` outside a readable directory prints an `error` record with code `invalid-selection` (or `invalid-request` for `broadcast`) and exits 2.

## ilium panes

```sh
ilium panes [selection options]
ilium panes --agent codex --state idle
```

Lists the selected panes, one `pane` record each, then a `summary`:

```text
{"type":"pane","session":"default","pane_id":12,"name":"refactor parser","kind":"agent","agent":"Codex","state":"working","project":"/home/me/dev/ilium","cwd":"/home/me/dev/ilium","is_self":false}
{"type":"summary","command":"panes","ok":true,"sessions":3,"unreachable_sessions":0,"panes":1}
```

`kind`, `agent` and `state` use the values of the selection options; `agent` and `state` are `null` for panes that are not agents. `is_self` marks the pane running the command. A session that is running but cannot be read produces a `warning` record with code `session-unreachable`, and `ok` is then `false`; the command still exits 0.

## ilium broadcast

```sh
ilium broadcast [selection options] [--when-idle] [--dry-run] [--include-self] <MESSAGE>...
ilium broadcast --project ilium,lumen "Re-read CLAUDE.md before your next step."
ilium broadcast --regex 'review|audit' --field name --invert --dry-run "Stop and report."
cat notice.md | ilium broadcast --agent claude -
```

Sends one message to every selected **agent**: it is typed into the agent's prompt and submitted with Enter, exactly as the interface's "Send message to all" does. Shells, editors, boards, and agent panes whose program is not running are never typed into, whatever the selection. Sessions are visited one at a time, and each agent's delivery is confirmed by its server before the next.

| Argument or option | Default | Meaning |
| --- | --- | --- |
| `<MESSAGE>...` | required | The message; several words are joined with spaces. A lone `-` reads the whole message from standard input. Put `--` first when the message starts with a hyphen. |
| `--file <PATH>` | | Read the message from a file instead (relative to the current directory). |
| `--when-idle` | off | Send now only to idle agents. Busy agents get the message in their prompt queue and receive it when their current turn finishes. |
| `--dry-run` | off | Print what would happen to each recipient without sending anything. |
| `--include-self` | off | Also send to the pane running this command. By default it is skipped. |
| `--timeout-seconds <n>` | 15 | How long to wait for each delivery's confirmation, 1 to 600. |
| selection options | all agents | See [Pane selection](#pane-selection). |

A trailing newline is removed. A multi-line message is sent as one bracketed paste, so the receiving program must have bracketed paste switched on (Claude Code and Codex do); otherwise that recipient reports a `failed` result.

Output is JSONL: a `progress` record, a `warning` per session that could not be read, one `result` per selected agent, then a `summary`:

```text
{"type":"progress","command":"broadcast","stage":"sending","sessions":3,"message_bytes":41}
{"type":"result","command":"broadcast","outcome":"delivered","session":"default","pane_id":12,"name":"refactor parser","kind":"agent","agent":"Codex","state":"working","project":"/home/me/dev/ilium","cwd":"/home/me/dev/ilium","is_self":false}
{"type":"summary","command":"broadcast","ok":true,"dry_run":false,"sessions":3,"unreachable_sessions":0,"recipients":1,"delivered":1,"queued":0,"planned":0,"skipped":0,"failed":0}
```

`outcome` is `delivered`, `queued` (`--when-idle`), `would-send` or `would-queue` (`--dry-run`), `skipped` (the calling pane) or `failed`; `skipped` and `failed` carry a `reason`. A `failed` delivery that timed out may still arrive.

Exit status: 0 when at least one agent was reached (or planned) and nothing failed; 1 when nothing was selected, any delivery failed, or any running session could not be read; 2 for an invalid selection or an empty message.

## Environment variables

Variables set by Ilium inside every terminal pane:

| Variable | Meaning |
| --- | --- |
| `ILIUM_PANE_ID` | This pane's numeric ID. Used by `ilium progress` to address the pane. |
| `ILIUM_SESSION_NAME` | The session the pane belongs to. |
| `ILIUM_SESSION_SOCKET` | The session's socket path. `ilium progress` and `ilium voice say` use it to reach the right server. |
| `ILIUM_WORKTREE` | Canonical root of the pane's Git worktree. Present only for a workspace-backed pane. |
| `ILIUM_BRANCH` | The worktree's branch name, when the worktree setup command runs. |

Variables you can set:

| Variable | Meaning |
| --- | --- |
| `ILIUM_CHATROOM_AUTHOR`, `AGENT_NAME` | Default author for `ilium chat send` (in that order, then `agent`). |
| `ILIUM_CONFIG_DIR` | Use this directory instead of the platform default for `config.toml` (`~/.config/ilium` on Linux, `~/Library/Application Support/ilium` on macOS, `%APPDATA%\ilium` on Windows). A relative value is resolved against the current directory. |
| `ILIUM_SOCKET_DIR` | Directory for session sockets and locks. Honoured on every platform. |
| `ILIUM_DEBUG_LOG_DIR` | Root directory for debug logs. |
| `XDG_RUNTIME_DIR` | On Linux, the default parent of the `ilium/` socket directory. When it is not set, a short per-user directory under `/tmp` is used. |
| `XDG_CONFIG_HOME`, `XDG_DATA_HOME` | Standard locations honoured on Linux. |
| `SHELL`, `COMSPEC` | The shell that runs `new-pane` commands. On Windows, a POSIX-style `SHELL` (for example Git Bash) keeps POSIX quoting. |

Relative values for the `ILIUM_*_DIR` overrides are made absolute so that the client and the detached server agree on the same path.

Logging is off by default. When enabled in Settings, logs are written under the debug log root and can contain project prompts and request bodies (credentials redacted). Treat them as sensitive. The five newest logs per session are kept.

## Output formats and exit status

| Command | stdout | Failure output |
| --- | --- | --- |
| `ilium`, `new-session` | the interface | `ilium: <error>` on stderr |
| `ls`, `kill-session`, `new-pane` (plain), `chat` | plain text | `ilium: <error>` on stderr |
| `new-pane --worktree` | JSONL (`progress`, `result`, `error`) | an `error` record, plus `ilium: <error>` on stderr |
| `progress` | JSONL, one record per call | a `progress_rejected` or `progress_request_failed` record, plus the error on stderr |
| `voice say` | JSONL (`progress`, then `result` or `error`) | an `error` record, plus the error on stderr |
| `panes`, `broadcast` | JSONL (see each command) | an `error` record for an invalid selection or message (exit 2); `broadcast` exits 1 when nothing was selected or any delivery failed |

Every JSONL line is a single JSON object with a `type` field. Parse stdout line by line and branch on `type`; paths and IDs are explicit fields.

Exit status:

- `0` on success.
- `1` on any failure (invalid directory or session name, session not running, server did not start within 5 seconds, server-reported error, rejected request, timeout).
- `2` for command-line usage errors reported by the argument parser, such as an unknown flag, a missing required argument, or `--restart-server` combined with `--reset-session`.

Error messages worth knowing:

| Message | Meaning |
| --- | --- |
| `--cwd "..." is not a valid directory` | The directory does not exist. |
| `could not locate the ilium-server binary next to ... or on PATH` | The server binary is missing. Keep `ilium` and `ilium-server` together. |
| `session "<name>"'s server did not become ready within 5s` | Server start timed out. See the log under the debug log root. |
| `session "<name>" is not running` | The command needs a live session. |
| `project session socket path is too long` | The session name and digest overflow the socket path limit. Use a shorter name or set `ILIUM_SOCKET_DIR` to a short path. |

## Other binaries and hidden commands

- `ilium-server`: the background daemon, one per project session. You normally never start it yourself; `ilium` spawns it detached with internal arguments. It also accepts unauthenticated `POST /create_agent` requests on `127.0.0.1:8872`; any local process can submit one, so keep that listener private (the port is `[api].port` in `config.toml`). See [Automation](automation.md).
- `ilium-animation-helper`: the confined helper that runs bundled `.iliumanim` animations. It and the animations must stay beside `ilium`. See [Animations](animations.md).
- Hidden subcommands exist for internal and release qualification use (`clipboard-helper`, `release-embedding-probe`, `release-animation-probe`). They are not part of the user interface and may change without notice.
- Flatpak builds forward the `ilium` command to the host so panes can use host tools. See [Installation](installation.md).

## Recipes

Start an agent for a task in its own worktree and keep the final JSONL line (the `result` record) for a script:

```sh
ilium new-pane --worktree --branch agent/fix-login -- codex | tail -n 1
```

Add a long-running dev server as a pane in a named session:

```sh
ilium new-pane --session-name dev -- npm run dev
ilium new-session dev     # attach later
```

Tell the voice assistant something from a script, starting voice if needed:

```sh
ilium --cwd ~/code/app voice say --start "what agents are running?"
```

Post a hand-off to the room when you finish:

```sh
ilium chat send --author "$AGENT_NAME" --message "Finished the parser change; tests pass in src/parse."
```

Check every session of several projects:

```sh
for dir in ~/code/*/; do echo "== $dir"; ilium --cwd "$dir" ls; done
```

Throw away a session's saved layout and begin again:

```sh
ilium --reset-session new-session scratch
```

## Troubleshooting

| Symptom | Cause and fix |
| --- | --- |
| `ilium progress` says it is not inside a pane | You ran it from a normal terminal. Run it inside an Ilium terminal pane, where `ILIUM_PANE_ID` is set. |
| Progress shows 0% forever | The probe uses relative paths. The server runs it from the project root, not the pane's directory. Use absolute paths. |
| `progress set` rejected with `disabled` | Enable **Progress monitor** in **Settings → Appearance**. |
| `progress` rejected with `invalid-probe-report` | The probe printed more than one object, a non-JSON line, a changed `job_id`, a bad `status`, a `percent` outside 0 to 100, or `error` with the wrong status. Run `progress check` and read the message. |
| `voice say` fails with `no-voice-client` | No interactive client is attached. Run `ilium` in the project first. |
| `voice say` fails with `server-closed-connection` or `timeout` | The running server predates the command, or the client is unresponsive. Restart Ilium to load the current server. |
| `new-pane --worktree` says the branch exists | Choose an unused branch name. |
| `new-pane` prints `no confirmation received from the server` | The server is busy or the session is not the one you meant. Check `ilium ls` and `--session-name`. |
| `chat send` says the project does not have a chatroom | Run `ilium chat init` first. |
| After installing a new build the old behaviour persists | A running server keeps its old executable. Use `ilium --restart-server` (panes are relaunched from the snapshot) when you are ready. |
