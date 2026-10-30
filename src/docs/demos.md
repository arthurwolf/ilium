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
- [Queue access to a shared resource](#queue-access-to-a-shared-resource)
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

![Ilium transfers a terminal report into a real Codex prompt without submitting it](../../assets/demos/05-screen-transfer.gif)

Screen transfer sends a visible report into a real Codex prompt as one bracketed paste. The setup is cut and reading holds are added; the submitted text was verified against the source screen after recording. Explained in [Panes and layout](panes-and-layout.md).

## Inspect agent usage and recorded costs

![Costs and Stats for retained Claude and Codex sessions in current Ilium](../../assets/demos/06-cost-and-stats.gif)

Ilium shows usage and costs for retained Claude and Codex sessions. Setup is cut, and a local file-link target and an account notice are redacted. Claude is logged out during the replay. Explained in [Agent cost](agent-cost.md).

## Track agent status and current goals

![Real Claude and Codex agents show a goal, an approval wait and an unread completion](../../assets/demos/08-agent-activity.gif)

Three real agents show an active Codex goal, a Claude approval wait and a completed Claude turn. Pane names are staged through the isolated control socket; setup and part of the initial wait are cut, a reading hold is added, and an unrelated status message is visibly redacted. Explained in [Agent monitoring](agent-monitoring.md).

## Use agents, editors, and terminals side by side

![View terminal, editor, and board panes together in a split view](../../assets/demos/10-mixed-splits.gif)

Terminal, editor and board panes share one split view, with the project tree still visible. Explained in [Panes and layout](panes-and-layout.md).

## Select and copy terminal output

![Smart Copy selects and pastes a code from real Codex output](../../assets/demos/11-smart-copy.gif)

Smart Copy uses local inference to select and paste a code from real Codex output. The private clipboard matches the source bytes. Setup and a model wait are cut, reading holds are added, and local paths and an account notice are labelled redactions; automatic pane naming did not apply. Explained in [Smart Copy](smart-copy.md).

## Queue prompts for an agent

![Queue two prompts for a real Claude agent; Ilium sends each one after the previous turn ends, in order.](../../assets/demos/13-prompt-queue.gif)

Prompts queued for an agent are delivered one by one as each of its turns finishes. Explained in [Automation](automation.md).

Disclosure: this is a current-build recording of a real Claude (Haiku) agent in an isolated lane; the agent's file output is real. The idle wait before the first prompt and part of the first turn's idle time are cut, and short reading holds are added, as shown by the loop clock. Names in the tree (`Exam University`, `Exam TA`) are staged. The readback that proves FIFO delivery (server log, Claude transcript and the driver key log: separate turns 2.75 s and 1.82 s after each previous turn ended, no key typed after the second Enqueue) was adjusted for this take to ignore the shell line that launches the agent, which the server log also records as a submitted prompt; the rest of its checks are unchanged. Earlier versions of this GIF used a synthetic Codex-shaped fixture.

## Get results from background tasks

![A real Claude agent monitors a ten-card task and receives its completed result](../../assets/demos/14-progress-monitor.gif)

Ilium tracks the task and delivers the completed result to a real Claude agent. Setup is cut and an account notice is redacted. Explained in [Automation](automation.md).

## Search agents, files, and terminals

![Find agents, files, and terminals, then jump to a match](../../assets/demos/17-workspace-search.gif)

Workspace search finds agents, files and terminal contents and jumps to the chosen match. This recording uses real Claude and Codex agents, cuts setup and adds reading holds; automatic restructuring is disabled in the isolated demo. Explained in [Panes and layout](panes-and-layout.md).

## See live agent goals in the project tree

![A live Codex goal changes the objective icon in the project tree](../../assets/demos/22-goal-indicators.gif)

A real Codex goal changes from active to paused and back beside the agent in the project tree. Setup is cut; captions, a closing hold and labelled account and path redactions are added. Automatic naming and restructuring are disabled. Explained in [Agent monitoring](agent-monitoring.md).

## Resume agent sessions after a reboot

![After a reboot sequence, Ilium restores the pane layout and resumes Claude and Codex](../../assets/demos/24-agents-survive-reboots.gif)

A staged reboot montage leads to retained footage of Ilium restoring the pane layout and resuming Claude and Codex. The reboot and boot screens are simulated. Explained in [Session recovery](session-recovery.md).

## Choose an animation, then watch it play

![Historical animation gallery from an earlier Ilium build; current-build Voxel preview remains visually unqualified](../../assets/demos/26-animations.gif)

The current-pinned replacement has not passed visual review: the ready Voxel scene still appears as a faint dot field, even with color enabled and pattern contrast at 200%. This historical asset remains until that scene is recognizable. Explained in [Animations](animations.md).

## Schedule input for a pane

![Ilium schedules input for a real Claude agent and a shell](../../assets/demos/04-scheduled-input.gif)

Ilium sends scheduled input to a real Claude agent and an echo-only terminal fixture in separate takes. Setup is cut, reading holds are added, and local paths are redacted. Explained in [Automation](automation.md).

## Set interface and board preferences

![The first chapter of the settings tour shows interface and board options](../../assets/demos/07a-settings-tour-a.gif)

The first chapter shows interface and board options. Agent Cost, Optimization and Remote are omitted to protect local data; setup is cut and reading holds are added. Explained in [Settings](settings.md).

## Configure voice and AI providers

![The second chapter of the settings tour shows voice and inference providers](../../assets/demos/07b-settings-tour-b.gif)

The second chapter of the settings tour covers voice, reset planning, inference and instructions. Provider credentials are unconfigured; setup and other pages are cut, with reading holds added. Explained in [Inference and privacy](inference-and-privacy.md).

## Configure automatic titles

![The third chapter of the settings tour shows automatic titles and setup options](../../assets/demos/07c-settings-tour-c.gif)

The third chapter covers titles, triggers, custom agents, debugging, API and setup. Setup paths belong to the isolated demo workspace; other pages are cut and reading holds are added. Live provider and plugin editors are not exercised. Explained in [Titles and instructions](titles-and-instructions.md).

## Rearrange panes without stopping their processes

![Move the Tests fixture pane into Exhibits above Build; the same local fixture process keeps running](../../assets/demos/09-pane-tree.gif)

The real Ilium tree moves a local Tests fixture pane into Exhibits and above Build. Its PID and heartbeat continue through the move; the terminal text comes from a fixture shell, while two real agents remain visible. Setup is cut, captions and a closing hold are added, and automatic naming and restructuring are disabled in the isolated demo. Explained in [Panes and layout](panes-and-layout.md).

## Request updates from active agents

![Ilium asks idle Claude and Codex agents for updates while skipping an agent awaiting approval](../../assets/demos/12-ask-for-update.gif)

Ask for status update sends the built-in prompt to idle Claude and Codex agents. A third agent waiting for command approval receives no prompt. Pane names were staged and automatic naming and restructuring were disabled in the isolated session; setup and waits were cut and a reading pause was added. Explained in [Agent monitoring](agent-monitoring.md).

## Track work on a Kanban board

![Move a staged Kanban card to Doing, mark it complete, add a note, then reopen the board and verify the saved Markdown file. Setup is cut; column width is 15 cells, with added captions, pointer, arrows and holds.](../../assets/demos/15-kanban.gif)

The current Ilium board moves the card into Doing, marks it complete and adds a note. The backing Markdown file is opened to verify the saved values, then the board is reopened. The isolated fixture has no provider agents; automatic naming and restructuring are disabled. Explained in [Editors and boards](editors-and-boards.md).

## Start a Codex agent from a TODO

![Start Codex from a TODO line and have it repair the source](../../assets/demos/16-agent-from-line.gif)

A real Codex agent starts from a TODO line and repairs the source. The recording cuts setup and adds reading holds; automatic title inference reports that no user transcript is available. Explained in [Editors and boards](editors-and-boards.md).

## Detach and reconnect while agents keep running

![A real Claude agent continues writing files while Ilium is detached, then reappears on reconnect. Setup and waiting are cut, an account notice is redacted, and automatic naming and restructuring are disabled.](../../assets/demos/18-detach-reattach.gif)

The client detaches and later reattaches while panes and agents keep running in the server. Explained in [Panes and layout](panes-and-layout.md).

## Edit Markdown in a pane

![Edit and preview a staged Markdown note in Ilium, then read the saved file in a terminal. Setup is cut; captions, pointer, arrows and holds are added.](../../assets/demos/19-markdown-editor.gif)

A staged Markdown note is edited and previewed in a pane, then read back from disk in a terminal. Automatic naming and restructuring are disabled for the recording. Explained in [Editors and boards](editors-and-boards.md).

## Coordinate Claude and Codex with Chatroom

![The current Ilium Chatroom displays locally scripted Claude and Codex messages; provider agents are not running](../../assets/demos/20-chatroom.gif)

The current-build Chatroom view shows two local simulation messages. Provider agents are not running. Explained in [Automation](automation.md).

## Respond to confirmation prompts automatically

![A text trigger responds to an agent confirmation question](../../assets/demos/21-text-triggers.gif)

A text trigger recognises a real Claude agent's confirmation question and answers it automatically. The recording cuts setup and an idle wait, adds reading holds, and disables automatic naming and restructuring in the isolated demo. Explained in [Automation](automation.md).

## Restore panes and resume agent sessions

![Ilium restores panes and Claude and Codex continue from resumed context](../../assets/demos/23-snapshot-resume.gif)

Panes are restored from a snapshot and the agents continue from their resumed context. Explained in [Session recovery](session-recovery.md).

## Run agents in their own worktrees

![Ilium creates agent/map-east-side; a local Codex-shaped fixture runs in that worktree, and a terminal shows the branch and file diff](../../assets/demos/25-worktree-agent.gif)

The capture creates a linked Git worktree, then runs a documented local Codex-shaped fixture from its absolute path inside that worktree. It shows the branch and resulting file diff. Typing is shortened, and three editorial captions were added. The built-in provider launch is not intercepted or represented. Explained in [Worktrees](worktrees.md).

## Convert a session between Claude and Codex

![Real Claude and Codex sessions recall a fictional mascot after conversion in both directions](../../assets/demos/26-convert-session.gif)

Real Claude and Codex sessions are converted in both directions and recall a fictional mascot from the conversation. Setup and an idle wait are cut, an account notice is redacted, and captions are added. The isolated capture disables automatic project restructuring and uses local title inference. Explained in [Session recovery](session-recovery.md).

## Queue access to a shared resource

![Two agents queue for a shared resource and release it in turn](../../assets/demos/28-resource-queue.gif)

Two real agents reserve an isolated demonstration resource: one waits, takes ownership after the first releases it, then releases it. The recording is cropped and pauses are shortened. Explained in [Resource queues](resource-queues.md).
