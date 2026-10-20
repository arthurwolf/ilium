---
layout: page
title: CLI compatibility
permalink: /pages/reference-cli-compatibility/
---

## CLI compatibility

Today every `ilium` subcommand is built into the binary. After the move to plugins, the rest are owned by core plugins. A user, a script or an agent that runs `ilium chat send` or `ilium progress set --wait` MUST see the same arguments, the same output records and the same exit codes. This page lists every command, who owns it, and the changes to the `cli` contract that make this possible.

Related pages: [cli](/pages/api-cli/), [capability cli.contribute](/pages/capability-cli-contribute/), [manifest reference](/pages/reference-manifest/), [kernel and boot](/pages/reference-kernel-and-boot/), [feature traceability](/pages/reference-feature-traceability/), [core parity requirements](/pages/reference-requirements-core-parity/).

### Command ownership

#### Kernel commands

The kernel parses these commands before any plugin runs. A plugin MUST NOT declare them. They work with `--no-plugins` and in safe start.

| Command | Why it stays in the kernel |
|---|---|
| `ilium` (attach) | Starts the server and the client. |
| `ilium new-session <name>` | Creates the session and its socket. |
| `ilium ls` | Reads the session sockets. |
| `ilium kill-session <name>` | Ends a session. |
| `ilium plugin ...` | Installs, enables and removes plugins. See [plugin commands](/pages/getting-started-toolchain/). |
| `ilium --help`, `ilium --version` | Help must work with no plugin. |
| `kernel.open-recovery` | The recovery page needs no plugin. |
| `--cwd`, `--session-name`, `--safe-plugins`, `--no-plugins` | Global options read before plugins load. |

#### Commands owned by core plugins

| Command | Owner plugin | Page | Needs a server | Output | Notes |
|---|---|---|---|---|---|
| `ilium new-pane` | fleet | [fleet](/pages/core-plugin-fleet/) | yes | text for plain panes, JSONL for `--worktree` | Worktree creation calls the worktrees service. |
| `ilium panes` | fleet | [fleet](/pages/core-plugin-fleet/) | no | JSONL | Visits every running session. |
| `ilium broadcast` | fleet | [fleet](/pages/core-plugin-fleet/) | no | JSONL | Uses the pane selector. |
| `ilium chat init`, `send`, `context`, `tail` | chatroom | [chatroom](/pages/core-plugin-chatroom/) | no | text | Works on `CHATROOM.md` directly. |
| `ilium progress check`, `set`, `wait`, `status`, `clear` | progress | [progress](/pages/core-plugin-progress/) | yes | JSONL | `ilium wait` is a second declared name. |
| `ilium wait` | progress | [progress](/pages/core-plugin-progress/) | yes | JSONL | Alias of `ilium progress wait`. |
| `ilium voice say` | voice | [voice](/pages/core-plugin-voice/) | yes | JSONL | Asks the plugin's client side. |
| `ilium recap` | recap | [recap](/pages/core-plugin-recap/) | yes | JSONL or Markdown | Uses the pane selector. |

The chat hooks that `ilium chat init` installs call `ilium chat context --since-last-read`. These calls MUST keep working when the chatroom plugin is the owner.

### Contract changes

The current `cli` contract allows one subcommand per plugin and one output form per command. Core plugins need more. These changes apply to `ilium:plugin@0.1.0` and are already reflected in [cli](/pages/api-cli/) and [capability cli.contribute](/pages/capability-cli-contribute/).

| Change | Before | After |
|---|---|---|
| Several commands per plugin | `cli` is one object | `cli` is an object or a list of at most 16 objects. Names MUST be unique inside the plugin. |
| Aliases | none | Each entry MAY set `aliases`, a list of names that follow the same name rule. `ilium wait` is an alias of `progress wait`. |
| Mixed output | `jsonl` or `text` | `output` MAY also be `"mixed"`. Both `write-line` and `write-text` are allowed. `new-pane` uses it. |
| Reserved names | none | The names the kernel owns (`plugin`, `session`, `kill-session`, `help`, `version`) are reserved. A manifest that uses one fails with `manifest.cli-name-reserved`. A name owned by a bundled core plugin (`recap`, `progress`, `wait`, `chat`, `panes`, `broadcast`, `new-pane`, `voice`) may be declared only by that plugin; any other plugin fails with `manifest.cli-name-taken`. |
| Startup meaning | one meaning | `startup` runs per side. The command table is read when the server side loads. |

[COR-040] A plugin MUST declare every `ilium` subcommand it owns in the `cli` key. A built-in command that has no declared owner MUST NOT exist after the move.

[COR-041] The arguments, option names, defaults, output records, exit codes and error codes of every command in the table MUST equal those in the CLI reference of the user documentation (`src/docs/cli-reference.md`) at the baseline commit.

### Disabled owner

When the owner plugin is disabled, the kernel keeps the command name but answers it.

| Situation | Result |
|---|---|
| `ilium chat send ...` and chatroom is disabled | One JSON record `{"type":"error","code":"cli.owner-disabled","plugin":"ilium.core.chatroom","message":"..."}` on standard output, a hint on standard error, exit code 1. |
| The owner failed to load | Same record with `cli.owner-failed`. |
| The command is not installed at all | `unknown command`, exit code 2, as today. |
| `ilium --help` | Lists the command with the mark `(disabled)`. |

[COR-042] A disabled or failed owner MUST give exit code 1 and an error record with the code `cli.owner-disabled` or `cli.owner-failed`. The record MUST name the plugin id and MUST NOT be a parse error.

### Sample fixes

The core-plugin pages had samples that broke the contract. They are corrected as follows.

| Page | Problem | Fix |
|---|---|---|
| fleet | `cli` was a list while the contract allowed one object; world `ilium:client/views@0.1.0` does not exist. | List allowed; world is `ilium:client/base@0.1.0`. |
| progress | Declared `progress` only; `wait` is a second name. | `aliases` or a second entry. |
| recap | `argument_groups` existed but the group was not defined. | Group `pane-selector` documented in [cli](/pages/api-cli/). |
| chatroom | Needed no server but did not say so. | `requires_server: false`. |

### Environment variables the plugins rely on

The kernel sets these inside every terminal pane. Plugins read them through the host, not through the process environment.

| Variable | Set by | Read by | Host function |
|---|---|---|---|
| `ILIUM_PANE_ID` | kernel | progress, chatroom (reader key), recap | `caller-pane` |
| `ILIUM_SESSION_NAME` | kernel | fleet, voice | `cli-invoked` payload `cwd` plus session lookup |
| `ILIUM_SESSION_SOCKET` | kernel | progress, voice | resolved by the kernel |
| `ILIUM_WORKTREE`, `ILIUM_BRANCH` | worktrees | agent setup scripts | none; set at pane spawn |
| `ILIUM_CHATROOM_AUTHOR`, `AGENT_NAME` | user | chatroom | listed in the `cli-invoked` environment subset |
| `ILIUM_CONFIG_DIR`, `ILIUM_SOCKET_DIR`, `ILIUM_DEBUG_LOG_DIR` | user | kernel | none |
| `ILIUM_SAFE_PLUGINS`, `ILIUM_NO_PLUGINS` | user | kernel | none |

The kernel MUST pass only the listed variables to a plugin. A plugin cannot read other variables of the caller.

### Requirements

[COR-040] and [COR-041] and [COR-042] above are collected, with their tests, in [core parity requirements](/pages/reference-requirements-core-parity/).
