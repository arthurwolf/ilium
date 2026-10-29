# Demo gallery

Recorded demonstrations of Ilium, one per feature. Each demo is an animated GIF stored in [`assets/demos/`](../../assets/demos/) and is followed by a link to the page that explains the feature step by step. The short selection shown on the project [README](../../README.md) is drawn from this gallery.

## Contents

- [Find the next agent that needs you](#find-the-next-agent-that-needs-you)
- [AI names agents and organizes the project tree](#ai-names-agents-and-organizes-the-project-tree)
- [Control Ilium and prompt agents with voice](#control-ilium-and-prompt-agents-with-voice)
- [Give agent panes automatic titles](#give-agent-panes-automatic-titles)
- [Transfer a terminal screen between panes](#transfer-a-terminal-screen-between-panes)
- [Inspect agent usage and recorded costs](#inspect-agent-usage-and-recorded-costs)
- [Track agent status and current goals](#track-agent-status-and-current-goals)
- [Use agents, editors, and terminals side by side](#use-agents-editors-and-terminals-side-by-side)
- [Select and copy terminal output](#select-and-copy-terminal-output)
- [Queue prompts for an agent](#queue-prompts-for-an-agent)
- [Get results from background tasks](#get-results-from-background-tasks)
- [Search agents, files, and terminals](#search-agents-files-and-terminals)
- [See live agent goals in the project tree](#see-live-agent-goals-in-the-project-tree)
- [Resume agent sessions after a reboot](#resume-agent-sessions-after-a-reboot)
- [Choose an animation, then watch it play](#choose-an-animation-then-watch-it-play)
- [Schedule input for a pane](#schedule-input-for-a-pane)
- [Set interface and board preferences](#set-interface-and-board-preferences)
- [Configure voice and AI providers](#configure-voice-and-ai-providers)
- [Configure automatic titles](#configure-automatic-titles)
- [Rearrange panes without stopping their processes](#rearrange-panes-without-stopping-their-processes)
- [Request updates from active agents](#request-updates-from-active-agents)
- [Track work on a Kanban board](#track-work-on-a-kanban-board)
- [Start a Codex agent from a TODO](#start-a-codex-agent-from-a-todo)
- [Detach and reconnect while agents keep running](#detach-and-reconnect-while-agents-keep-running)
- [Edit Markdown in a pane](#edit-markdown-in-a-pane)
- [Coordinate Claude and Codex with Chatroom](#coordinate-claude-and-codex-with-chatroom)
- [Respond to confirmation prompts automatically](#respond-to-confirmation-prompts-automatically)
- [Restore panes and resume agent sessions](#restore-panes-and-resume-agent-sessions)
- [Run agents in their own worktrees](#run-agents-in-their-own-worktrees)
- [Convert a session between Claude and Codex](#convert-a-session-between-claude-and-codex)
- [Additional file](#additional-file)
- [Duplicate file names](#duplicate-file-names)

## Find the next agent that needs you

![Real Claude and Codex sessions demonstrate the attention queue, with setup cuts and reading holds](../../assets/demos/27-attention-queue.gif)

The attention queue collects questions, blocked goals, background-agent waits and unread completed turns. **Next: needs you** jumps to an agent; **[+]** opens the overview. The recording uses instant detection and real agent sessions in a fictional museum project. Setup and idle gaps are cut, with frames held for reading. Explained in [Attention queue](attention-queue.md).

## AI names agents and organizes the project tree

![A scripted plan regroups current Ilium panes by topic; local fixtures provide no model inference](../../assets/demos/01-ai-tree.gif)

A scripted plan regroups current Ilium panes by topic. Local fixture output supplies the plan; no model inference is made. Explained in [Titles and instructions](titles-and-instructions.md).

## Control Ilium and prompt agents with voice

![Typed voice commands drive current Ilium through scripted local tools; no microphone or hosted inference is used](../../assets/demos/02-voice-control.gif)

Typed commands pass through a local loopback fixture to real Ilium tools and a real shell. Agent panes are local fixtures; no microphone or hosted inference is shown. Explained in [Voice](voice.md).

## Give agent panes automatic titles

![Current Ilium applies an automatic title from a supplied local response fixture; the manually named Keep pane stays unchanged](../../assets/demos/03-contextual-titles.gif)

Current Ilium applies a supplied local response fixture as the automatic title; the manually named Keep pane stays unchanged. Explained in [Titles and instructions](titles-and-instructions.md).

## Transfer a terminal screen between panes

![Ilium transfers a complete visible report to a local terminal fixture; the receiver verifies the bytes](../../assets/demos/05-screen-transfer.gif)

Screen transfer sends a visible report to the terminal on the right as one bracketed paste. Both terminals use local fixtures; the receiver verifies the bytes. Explained in [Panes and layout](panes-and-layout.md).

## Inspect agent usage and recorded costs (historical capture)

![Historical Costs and Stats capture from an older Ilium build; current-build live-resume acceptance remains unverified.](../../assets/demos/06-cost-and-stats.gif)

This recording predates the current-build refresh. The current capture has not passed its real-idle check because Codex remained in its resume-directory picker. The existing animation remains a historical illustration. Explained in [Agent cost](agent-cost.md).

## Track agent status and current goals

![Local scripted fixtures show agent status and a sample goal in the current Ilium tree; no provider session is shown](../../assets/demos/08-agent-activity.gif)

Local scripted agent fixtures show status changes and a sample goal in the tree. No live provider session is shown. Explained in [Agent monitoring](agent-monitoring.md).

## Use agents, editors, and terminals side by side

![View terminal, editor, and board panes together in a split view](../../assets/demos/10-mixed-splits.gif)

Terminal, editor and board panes share one split view, with the project tree still visible. Explained in [Panes and layout](panes-and-layout.md).

## Select and copy terminal output

![Ilium Smart Copy selects a lot code from a local fixture ledger; fixture and clipboard readback verify the copied bytes](../../assets/demos/11-smart-copy.gif)

Smart Copy highlights and copies a complete source lot code. The terminal and inference use local fixtures; the private clipboard is checked against the source bytes. Explained in [Smart Copy](smart-copy.md).

## Queue prompts for an agent

![Queue two prompts for a local Codex-shaped fixture; Ilium sends them in order after each turn. No model; output is synthetic.](../../assets/demos/13-prompt-queue.gif)

Prompts queued for an agent are delivered one by one as each of its turns finishes. Explained in [Automation](automation.md).

## Get results from background tasks

![A local Codex-shaped fixture runs a monitored five-card census task in current Ilium and delivers its result](../../assets/demos/14-progress-monitor.gif)

A local script acts as a Codex-shaped fixture while Ilium registers the monitor, shows progress and posts the completed result. No provider request is made. Explained in [Automation](automation.md).

## Search agents, files, and terminals

![Find agents, files, and terminals, then jump to a match](../../assets/demos/17-workspace-search.gif)

Workspace search finds agents, files and terminal contents and jumps to the chosen match. Explained in [Panes and layout](panes-and-layout.md).

## See live agent goals in the project tree

![A live Codex goal changes the objective icon in the project tree](../../assets/demos/22-goal-indicators.gif)

A live Codex goal changes the objective icon beside the agent in the project tree. Explained in [Agent monitoring](agent-monitoring.md).

## Resume agent sessions after a reboot

![After a reboot sequence, Ilium restores the pane layout and resumes Claude and Codex](../../assets/demos/24-agents-survive-reboots.gif)

After a restart, Ilium restores the pane layout and resumes the Claude and Codex sessions. Explained in [Session recovery](session-recovery.md).

## Choose an animation, then watch it play

![Historical animation gallery from an earlier Ilium build; current-build Voxel preview remains visually unqualified](../../assets/demos/26-animations.gif)

The current-pinned replacement has not passed visual review: the ready Voxel scene still appears as a faint dot field, even with color enabled and pattern contrast at 200%. This historical asset remains until that scene is recognizable. Explained in [Animations](animations.md).

## Schedule input for a pane

![A five-second countdown sends ping to a labelled local echo-only terminal fixture](../../assets/demos/04-scheduled-input.gif)

Current Ilium sends ping once after a five-second countdown. The receiving terminal is a labelled local echo-only fixture. Explained in [Automation](automation.md).

## Set interface and board preferences

![The first chapter of the settings tour shows interface and board options](../../assets/demos/07a-settings-tour-a.gif)

The first chapter of the settings tour: interface and board options. Explained in [Settings](settings.md).

## Configure voice and AI providers

![The second chapter of the settings tour shows voice and inference providers](../../assets/demos/07b-settings-tour-b.gif)

The second chapter of the settings tour: voice and AI provider configuration. Explained in [Inference and privacy](inference-and-privacy.md).

## Configure automatic titles

![The third chapter of the settings tour shows automatic titles and setup options](../../assets/demos/07c-settings-tour-c.gif)

The third chapter of the settings tour: automatic titles and setup options. Explained in [Titles and instructions](titles-and-instructions.md).

## Rearrange panes without stopping their processes

![Move the Tests fixture pane into Exhibits above Build; the same local fixture process keeps running](../../assets/demos/09-pane-tree.gif)

The real Ilium tree moves a local Tests fixture pane into Exhibits and above Build. Its PID and heartbeat continue through the move; terminal text comes from a disclosed local fixture shell, not an agent. Explained in [Panes and layout](panes-and-layout.md).

## Request updates from active agents

![Ilium asks two idle local agent fixtures for an update and skips a working fixture](../../assets/demos/12-ask-for-update.gif)

Ask for status update sends the built-in prompt to two idle local fixtures. The working fixture receives no input. Explained in [Agent monitoring](agent-monitoring.md).

## Track work on a Kanban board

![Move a Kanban card to Doing, mark it complete, edit its note, then reopen the board and verify the saved Markdown file](../../assets/demos/15-kanban.gif)

The current Ilium board moves the card into Doing and marks it complete, then edits the board-backed Markdown file and reopens it to verify the saved title and note. The two visible agent panes are idle local CLI-shaped fixtures; the demo makes no provider requests and does not demonstrate live agent classification. Explained in [Editors and boards](editors-and-boards.md).

## Start a Codex agent from a TODO

![Start Codex from a TODO line and have it repair the source](../../assets/demos/16-agent-from-line.gif)

An agent is started from a TODO line in a source window and works on that line. Explained in [Editors and boards](editors-and-boards.md).

## Detach and reconnect while agents keep running

![Reattach while agents and panes keep running](../../assets/demos/18-detach-reattach.gif)

The client detaches and later reattaches while panes and agents keep running in the server. Explained in [Panes and layout](panes-and-layout.md).

## Edit Markdown in a pane

![Edit a Markdown note inside Ilium](../../assets/demos/19-markdown-editor.gif)

A Markdown note is edited in a pane next to the agents. Explained in [Editors and boards](editors-and-boards.md).

## Coordinate Claude and Codex with Chatroom

![The current Ilium Chatroom displays locally scripted Claude and Codex messages; provider agents are not running](../../assets/demos/20-chatroom.gif)

The current-build Chatroom view shows two local simulation messages. Provider agents are not running. Explained in [Automation](automation.md).

## Respond to confirmation prompts automatically

![A text trigger responds to an agent confirmation question](../../assets/demos/21-text-triggers.gif)

A text trigger recognises an agent's confirmation question and answers it automatically. Explained in [Automation](automation.md).

## Restore panes and resume agent sessions

![Ilium restores panes and Claude and Codex continue from resumed context](../../assets/demos/23-snapshot-resume.gif)

Panes are restored from a snapshot and the agents continue from their resumed context. Explained in [Session recovery](session-recovery.md).

## Run agents in their own worktrees

![Ilium creates agent/map-east-side; a local Codex-shaped fixture runs in that worktree, and a terminal shows the branch and file diff](../../assets/demos/25-worktree-agent.gif)

The capture creates a linked Git worktree, then runs a documented local Codex-shaped fixture from its absolute path inside that worktree. It shows the branch and resulting file diff. Typing is shortened, and three editorial captions were added. The built-in provider launch is not intercepted or represented. Explained in [Worktrees](worktrees.md).

## Convert a session between Claude and Codex

![A Claude session converts to Codex, which still remembers the secret word, then converts back to Claude](../../assets/demos/26-convert-session.gif)

A session is converted from Claude to Codex and back while keeping its context. Explained in [Session recovery](session-recovery.md).
