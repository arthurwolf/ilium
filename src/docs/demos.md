# Demo gallery

Recorded demonstrations of Ilium. Each animated GIF is stored in [`assets/demos/`](../../assets/demos/) and followed by a link to the relevant feature guide. The project [README](../../README.md) also shows these demos.

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
- [Recover a session after a simulated reboot](#recover-a-session-after-a-simulated-reboot)
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
- [Restore a saved session](#restore-a-saved-session)
- [Run agents in their own worktrees](#run-agents-in-their-own-worktrees)
- [Convert a session between Claude and Codex](#convert-a-session-between-claude-and-codex)
- [Queue access to a shared resource](#queue-access-to-a-shared-resource)
- [Recording provenance and review limits](#recording-provenance-and-review-limits)

## Find the next agent that needs you

![Three real Claude sessions demonstrate the attention queue: two wait for an answer or approval and one has unread output; setup is cut](../../assets/demos/27-attention-queue.gif)

The attention queue collects questions, blocked goals, background-agent waits and unread completed turns. **Next: needs you** jumps to an agent; **[+]** opens the overview. The recording was re-recorded on 2026-10-31 with three real Claude (Haiku) sessions in a fictional museum project and instant detection: one asks which gallery to open, one waits for approval to write a file, and one finished with unread output. Keyboard shortcuts `Ctrl+B i` (jump) and `Ctrl+B O` (overview) drive it. Blocked goals and background-agent waits are not shown, and Codex is absent because its usage limit was reached. The long setup (boot, folder-trust prompts) is cut, three reading holds and three captions were added, and the status bar shows an unrelated naming-worker error. Explained in [Attention queue](attention-queue.md).

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

![A terminal, editor and two idle agent panes remain in a split after visiting another pane](../../assets/demos/10-mixed-splits.gif)

A terminal, editor and two idle agent panes share a split view. Setup is cut and an account notice is redacted. Explained in [Panes and layout](panes-and-layout.md).

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

## Recover a session after a simulated reboot

![A simulated reboot card precedes an isolated Ilium server shutdown and relaunch; a local fixture recovers a saved note](../../assets/demos/24-agents-survive-reboots.gif)

An isolated Ilium server actually shuts down and relaunches, and a local session fixture recovers a saved note. The reboot card is simulated and editorial; no machine reboot or real provider is used. Idle waits are cut and caption holds are added. Explained in [Session recovery](session-recovery.md).

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

## Restore a saved session

![An isolated Ilium server shuts down and relaunches; a local fixture recovers a saved note](../../assets/demos/23-snapshot-resume.gif)

This recording shows an actual isolated Ilium server shutdown and relaunch, followed by a local session fixture recovering a saved note. The reboot card is simulated and editorial; no machine reboot or real provider is used. Idle waits are cut and caption holds are added. Explained in [Session recovery](session-recovery.md).

## Run agents in their own worktrees

![Ilium creates a worktree through its native workflow and starts a local fixture in an isolated demo project](../../assets/demos/25-worktree-agent.gif)

The recording shows Ilium creating a real worktree through its native workflow in an isolated demo project and starting a local fixture instead of a provider agent. Terminal states are held, captions are added, and a path is redacted. A coding agent reviewed all four decoded frames. Explained in [Worktrees](worktrees.md).

## Convert a session between Claude and Codex

![Real Claude and Codex sessions recall a fictional mascot after conversion in both directions](../../assets/demos/26-convert-session.gif)

Real Claude and Codex sessions are converted in both directions and recall a fictional mascot from the conversation. Setup and an idle wait are cut, an account notice is redacted, and captions are added. The isolated capture disables automatic project restructuring and uses local title inference. Explained in [Session recovery](session-recovery.md).

## Queue access to a shared resource

![Staged local fixture participants queue for, take and release a shared resource; captions are added and an account quota notice is masked](../../assets/demos/28-resource-queue.gif)

Two projects share an isolated demonstration resource through staged local fixture participants: one reserves it, the other queues, takes ownership after the first releases it, then releases it. No provider calls are made. The recording shows the full interface, with added captions and a masked account quota notice. Explained in [Resource queues](resource-queues.md).

## Recording provenance and review limits

The gallery descriptions above state each recording’s subject and disclosed fixtures, edits and limitations. The table below indexes the 31 gallery files as observed on 2026-11-03. The recording date in the Git history is the last asset change, not necessarily the date of capture or proof that the recording represents the current build. Matching aliases are separate files in the same `assets/demos/` directory; equal hashes establish byte equality at this check, not independent capture evidence.

A coding agent inspected overview contact sheets for all 31 files. Those are sampled frames: they do not establish every action, provider identity, timing, outcome or complete visual acceptance. Recheck the asset hash and the affected evidence after any replacement or re-render.

A coding agent also reviewed all 47 decoded Resource Queue frames, including the quota mask. That complete-frame review applies to SHA-256 `5f472a874559adeacc320cf5dc5b3aeb7c983104d7ed4c1522fc176b1a4d3d37`; if the GIF changes, the new file requires a fresh review. This review does not establish live provider calls or runtime behavior; the participants are staged local fixtures.

The original provider-transcript archive manifest records 102 distinct missing source paths for 97 session IDs (41 Claude and 56 Codex). All 102 exact indexed paths were absent when checked on 2026-11-03. This is an aggregate of the original manifest, not proof that no equivalent transcript exists elsewhere; no missing identifiers are reconstructed here.

| Gallery GIF | Matching alias | Last asset change | SHA-256 |
| --- | --- | --- | --- |
| `27-attention-queue.gif` | None | 2026-10-31 | `9bf584f4598c743b781f03babfc3d6cc4c9d7d8025bab5bd4ac549ea426b6124` |
| `01-ai-tree.gif` | `12-ai-tree.gif` | 2026-10-27 | `558d7155a707448a717944c832fab820bc619c705872c583ecd2600cd30f75c0` |
| `02-voice-control.gif` | `22-voice-control.gif` | 2026-10-27 | `d427de55589e7100d26189678fc9adbcda1bfc91bfed44a83eaa52e3ae948661` |
| `03-contextual-titles.gif` | `20-contextual-titles.gif` | 2026-10-27 | `280ce34ba90ccbf91833e1406a3e3c410442ba8ed9a3c8364555a61b061e225c` |
| `05-screen-transfer.gif` | `18-screen-transfer.gif` | 2026-10-30 | `d9c854388a166d96fb485b90ec52428ae6080d7b8f86603eb54be89d3dd0b367` |
| `06-cost-and-stats.gif` | `23-cost-and-stats.gif` | 2026-10-30 | `ca7b9ffc5b58e6d9e67363ac92c68366a49b1068f62fcb88870184cd3e540c0f` |
| `08-agent-activity.gif` | `01-agent-activity.gif` | 2026-10-30 | `75a43fd34483ac637cfc214c12c65abbca37353db2ae98a5c109a48d711331fd` |
| `10-mixed-splits.gif` | `03-mixed-splits.gif` | 2026-10-30 | `b5b9bfadf4f38c5f423e2b0a531bbc5a96a4789426498d4d4b48cb1a5d8b45d7` |
| `11-smart-copy.gif` | `04-smart-copy.gif` | 2026-10-30 | `7924d3c4085d25450c742ad5b6e64b44650b9befe69812405cb4c03b25093d57` |
| `13-prompt-queue.gif` | `06-prompt-queue.gif` | 2026-10-31 | `6992ce97e186233d367a49087c2cf13fead5dfe5ac175ea70fa44460859e936c` |
| `14-progress-monitor.gif` | `07-progress-monitor.gif` | 2026-10-30 | `3eca3c1b8b4dc2e5f91ab0f6b8b67c3fe5e5917274f65b6e3a598cd783255f22` |
| `17-workspace-search.gif` | `10-workspace-search.gif` | 2026-10-30 | `4760941409b9c35883e740799b32647563686254a0b9f2bf1c1dce3de3786fe0` |
| `22-goal-indicators.gif` | `17-goal-indicators.gif` | 2026-10-30 | `f57dc9d67fcb85ddd7a21b41019ea6189e5c4f153a61105bb01a0d20da7ff3fa` |
| `24-agents-survive-reboots.gif` | `19-snapshot-resume.gif`, `23-snapshot-resume.gif` | 2026-10-26; working file updated | `7c24d61380e6c6bf49b23b0d45a209473590066be4eda305f9ef05db5eee1e78` |
| `26-animations.gif` | None | 2026-10-27 | `7705e61f2e3f4c38d78a4a9cd76df42ed9126d0ce20077d4ad3371a22d201431` |
| `04-scheduled-input.gif` | `15-scheduled-input.gif` | 2026-10-30 | `4be59a9b4076f7b35a129711e5c7f3a3937538d68cf938ecfd26944d1c20d6c7` |
| `07a-settings-tour-a.gif` | `21-settings-tour-a.gif` | 2026-10-30 | `0c078fe5aca220e48c713299b4a0b77deb2fa223a7474689b5473c3de2cecb9e` |
| `07b-settings-tour-b.gif` | `21-settings-tour-b.gif` | 2026-10-30 | `9b086a03958d279cf0ff2afbeee56de77554d7f63b3a5c49b7508c24f7aa8ce4` |
| `07c-settings-tour-c.gif` | `21-settings-tour-c.gif` | 2026-10-30 | `2e2dc667df60c22be106ee78956c4bfe1b960220990a0bd02bbefdb111b3e136` |
| `09-pane-tree.gif` | `02-pane-tree.gif` | 2026-10-30 | `1b2894cf6059ade922c615ed070c70b291415023ffccaa4dad93038d096e9c5e` |
| `12-ask-for-update.gif` | `05-ask-for-update.gif` | 2026-10-30 | `bef553851a79c05a7a0c1991663191bb0bc729b174b5068f7eabc144696c25f5` |
| `15-kanban.gif` | `08-kanban.gif` | 2026-10-30 | `e4b795993ba983b98859383e62c5ba91ebddf4b207a5e335831320da177eee05` |
| `16-agent-from-line.gif` | `09-agent-from-line.gif` | 2026-10-30 | `77d3edb861067a3b15927494fa1f11d71876bd9c2541bc4472bdba2667574df3` |
| `18-detach-reattach.gif` | `11-detach-reattach.gif` | 2026-10-30 | `5110603b06dc3dc8c856be745c8204924e0ea3e9c2278354ae2a821e6ca26139` |
| `19-markdown-editor.gif` | `13-markdown-editor.gif` | 2026-10-30 | `c08e3ed6558b85a2769f714bb31db076a64b5934c4f4d316f6328127d7d9ccc5` |
| `20-chatroom.gif` | `14-chatroom.gif` | 2026-10-27 | `b447ab06e8f1f479e229a5af5f4ca6f6ed08c8d892dec92f069e9446566b2840` |
| `21-text-triggers.gif` | `16-text-triggers.gif` | 2026-10-30 | `461bb2c548b75bc4226cc693b60619cc5be33084f3525bc7767f24774721d236` |
| `23-snapshot-resume.gif` | `19-snapshot-resume.gif`, `24-agents-survive-reboots.gif` | 2026-10-26; working file updated | `7c24d61380e6c6bf49b23b0d45a209473590066be4eda305f9ef05db5eee1e78` |
| `25-worktree-agent.gif` | None | 2026-10-31; working file updated | `491060abd123dbf8a9d069c204d23b98f52ffd9941d6be40361807509bc0f1f6` |
| `26-convert-session.gif` | None | 2026-10-30 | `7e6eefd6057fc96b071bdb44d50184b6bb3119e3672234e9b6627c63f53e5ccd` |
| `28-resource-queue.gif` | None | 2026-11-03; working file updated | `5f472a874559adeacc320cf5dc5b3aeb7c983104d7ed4c1522fc176b1a4d3d37` |

The previous Snapshot Resume recording had a caption claiming both agents answered, but sampled frames showed only a Codex answer; it has been replaced. All seven decoded frames of the replacement snapshot and reboot fixture GIF were reviewed by the coding agent, SHA-256 `7c24d61380e6c6bf49b23b0d45a209473590066be4eda305f9ef05db5eee1e78`. The capture used an isolated Ilium server shutdown and relaunch and a local session fixture recovering a saved note; the reboot card is simulated, with no machine reboot or real provider. Idle cuts and caption holds are editorial. Recheck the hash and review if the file changes. The animation gallery is historical. All four decoded frames of the replacement Worktree Agent GIF were reviewed by the coding agent, SHA-256 `491060abd123dbf8a9d069c204d23b98f52ffd9941d6be40361807509bc0f1f6`; the native dialog, created branch, local fixture, captions and redaction are visible. This is visual evidence for that exact file; recheck the hash and review after any replacement. See the individual descriptions above for each capture’s other limits.

The plugin manual has separately maintained copies, not listed as root aliases in the table. Its source pages reference `docs/plugins-manual/assets/demos/`; its fallback builder copies that directory into the served, generated `docs/plugins-manual/_site/assets/demos/`. On 2026-11-03, all 30 same-named GIFs in each manual directory matched their root assets by SHA-256 after the previously missing generated Attention Queue copy was restored. The recordings and their consumers have distinct provenance:

- **Prompt Queue:** the manual source and generated copies now match the root real-Claude recording. The source and generated scheduler descriptions disclose the staged names, cuts and reading holds.
- **Snapshot Resume:** root and manual source and generated aliases now match the seven-frame reviewed fixture GIF. The earlier one-Claude review concerns a different recording.
- **Agents Survive Reboots:** root and manual source and generated aliases now match the same reviewed fixture GIF as Snapshot Resume. The reboot card is simulated; the isolated Ilium server shutdown and relaunch are real.
- **Worktree Agent:** the root, manual source and generated GIF copies match the native-workflow recording. A coding agent reviewed all four decoded frames of the hash identified above.

The generated manual is served from `_site/`, so changing source alone does not update that output. No Resource Queue copy was found in either manual asset directory. Recheck copies and their descriptions together after replacements, preserving original recordings.
