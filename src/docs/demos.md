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
- [Coordinate local models with Chatroom](#coordinate-local-models-with-chatroom)
- [Respond to confirmation prompts automatically](#respond-to-confirmation-prompts-automatically)
- [Restore a saved session](#restore-a-saved-session)
- [Run agents in their own worktrees](#run-agents-in-their-own-worktrees)
- [Convert a session between Claude and Codex](#convert-a-session-between-claude-and-codex)
- [Queue access to a shared resource](#queue-access-to-a-shared-resource)
- [Recording provenance and review limits](#recording-provenance-and-review-limits)

## Find the next agent that needs you

![Three real Claude sessions demonstrate the attention queue: two wait for an answer or approval and one has unread output; setup is cut](../../assets/demos/27-attention-queue.gif)

The attention queue collects questions, blocked goals, background-agent waits and unread completed turns. **Next: needs you** jumps to an agent; **[+]** opens the overview. The recording was re-recorded on 2026-10-31 with three real Claude (Haiku) sessions in a fictional museum project and instant detection: one asks which gallery to open, one waits for approval to write a file, and one finished with unread output. Keyboard shortcuts `Ctrl+B i` (jump) and `Ctrl+B O` (overview) drive it. Blocked goals and background-agent waits are not shown, and Codex is absent because its usage limit was reached. The long setup (boot, folder-trust prompts) is cut, three reading holds and three captions were added, and the status bar is redacted to hide an unrelated restructuring error. On 2026-11-05 a frame-level edit blanked a two-line header (account plan name and a partial working path) in two 50 ms frames, and the `Ctrl+B O` caption now shows only while the overview is open. A decoded-frame comparison confirms that exactly those 80 of 795 frames changed, and nothing else. The real wall-clock time after "Churned for 3s" ("done 2:02 AM") was masked in the five frames that showed it (decoded comparison: only those frames changed). Keyframe review of the edited file found no other private text; the caption for `Ctrl+B i` lingers about two frames after the overview closes. Explained in [Attention queue](attention-queue.md).

## AI names agents and organizes the project tree

![Three real Claude Code agents work in an isolated project; Ilium regroups the panes by topic with OpenAI gpt-4.1 when an agent finishes, twice, and the pane titles match their content](../../assets/demos/01-ai-tree.gif)

Three real Claude Code agents (Haiku 5.5) work in an isolated project named giant-maps. With restructuring left at its default trigger (agent finished), two automatic restructures run through a real OpenAI gpt-4.1 call and regroup the panes by topic; the three pane titles are then visited and match their content. Recorded 2026-11-05 in an isolated Ilium state with a staged project and prompts typed by a harness (in 12-character chunks, because a long key burst overflows the client input queue, a known client bug not fixed here). One agent wait is cut and two short holds (0.6 s and 1.0 s) are added; captions and a mouse pointer overlay are drawn. The status bar shows "Restructuring projects - giant-maps: empty - skipped" for the first 30 seconds, before the first agent finishes; that is real product output, not an error. The recording was made with the server at normal priority because the machine load starved a background-priority server. Keyframe review (16 keyframes, contact sheets; no username, host, path or key visible) was done by the producer and by an independent reviewer. The previous recording (2026-10-27, SHA-256 `558d7155a707448a717944c832fab820bc619c705872c583ecd2600cd30f75c0`, scripted fixture plan) is kept in the maintainer backup folder. Explained in [Titles and instructions](titles-and-instructions.md).

## Control Ilium and prompt agents with voice

![A real OpenAI Realtime voice session hears six synthesized commands and calls Ilium tools: new terminal, Settings, Triggers tab, close Settings, close terminal, stop voice mode](../../assets/demos/02-voice-control.gif)

A real OpenAI Realtime voice session (gpt-realtime-2.1) runs in an isolated Ilium. The speech is synthesized (OpenAI gpt-4o-mini-tts) and played into a virtual microphone, not spoken live. The model hears six commands and calls Ilium tools, each confirmed in the server log by transcript and tool call: "Open a new terminal" (create terminal), "Open the settings", "Show the Triggers tab", "Close the settings", "Close this terminal" and "Stop voice mode". Two waits of about 34 s between commands are cut and setup (starting the session, connecting, pressing F8) is skipped; the on-screen captions state SYNTHESIZED SPEECH, OPENAI REALTIME and STAGED VIRTUAL MICROPHONE and show each spoken sentence, and a caption leads its screen change by a moment. The project is shown only as `project`; no account, path or key appears (16 keyframes reviewed by the producer and an independent reviewer). A first take asked for the "Voice Control tab" and the tool returned an unknown-tab error (the tab label is "Voice control" or "Voice"), so the recording asks for the Triggers tab instead; the name mismatch is a possible separate fix. Recorded 2026-11-05 on a build containing the uncommitted voice capture-gating fix, so it matches the installed app only after that fix ships. The earlier recording (local recognition, three actions, SHA-256 `bf1669861f61b59120fc661636b700ec5e6b866fca8cde5aa3eef44303f25ef7`) is superseded and kept in the maintainer backup folder. Explained in [Voice](voice.md).

## Give agent panes automatic titles

![Ilium titles a shell pane and two real Claude Code panes with a local model while a name typed by hand stays unchanged](../../assets/demos/03-contextual-titles.gif)

Real recording of Ilium titling panes in an isolated session: one shell pane and two real Claude Code panes started by the demo harness. The second Claude stands in for Codex, which could not be recorded (account usage limit until 2026-11-10; the stored OpenAI key for Codex returned 401). Titles come from local Ollama `qwen3:8b`. The shell titles itself from its output, and a name typed by hand (Stamp desk) stays. A Claude pane gets its title from its first prompt. Edits: the boot (0 to 4.34 s) and a 45 s detection wait are cut, about 33.4 s in total, replaced by the caption "29 s idle wait cut"; reading holds are added, including a 3.6 s hold on the typed prompt; captions are keyed to markers. The project folder was pre-trusted off camera (Claude's folder-trust dialog), and the banner shows the short alias path `/opt/t/data/tmp/worlds/bureau-annex`, not the real scratch path. The Claude banner shows the account plan label "Claude Ultra"; no username, host, quota notice or error text is visible. Machine load at record time was 31; the render ran locally (ni-build has no GIF-render operation). Keyframe review 2026-11-06: 16 evenly spaced keyframes by an independent reviewer plus 6 keyframes of the final render by the producer; sampling does not inspect every frame. The previous fixture-based recording (SHA-256 `d5d8e77bde43de31a88ebeea446beb5d18c0848ff248edafc2f3941f2a037ab7`) is kept in the maintainer backup folder. Explained in [Titles and instructions](titles-and-instructions.md).

## Transfer a terminal screen between panes

![Ilium transfers a terminal report into a real Codex prompt without submitting it](../../assets/demos/05-screen-transfer.gif)

Screen transfer sends a visible report into a real Codex prompt as one bracketed paste. The setup is cut and reading holds are added; the submitted text was verified against the source screen after recording. Explained in [Panes and layout](panes-and-layout.md). Privacy-masked 2026-11-05 after a keyframe review found local scratch paths (which contained the local username); masked rows are filled with the background colour so those lines appear blank, and all other pixels, timing and frame count are unchanged (decoded-frame comparison with the original).

## Inspect agent usage and recorded costs

![Costs and Stats for retained Claude and Codex sessions in current Ilium](../../assets/demos/06-cost-and-stats.gif)

Ilium shows usage and costs for retained Claude and Codex sessions. Setup is cut, and a local file-link target and an account notice are redacted. Claude is logged out during the replay. Explained in [Agent cost](agent-cost.md). Privacy-masked 2026-11-05 after a keyframe review found an account login notice; a floating ACCOUNT NOTICE REDACTED box, already in the original recording, also appears briefly; masked rows are filled with the background colour so those lines appear blank, and all other pixels, timing and frame count are unchanged (decoded-frame comparison with the original).

## Track agent status and current goals

![Real Claude and Codex agents show a goal, an approval wait and an unread completion](../../assets/demos/08-agent-activity.gif)

Three real agents show an active Codex goal, a Claude approval wait and a completed Claude turn. Pane names are staged through the isolated control socket; setup and part of the initial wait are cut, a reading hold is added, and an unrelated status message is visibly redacted. Explained in [Agent monitoring](agent-monitoring.md). Privacy-masked 2026-11-05 after a keyframe review found local scratch paths and a status notice; the status area shows a labelled redaction; masked rows are filled with the background colour so those lines appear blank, and all other pixels, timing and frame count are unchanged (decoded-frame comparison with the original).

## Use agents, editors, and terminals side by side

[Historical recording: A terminal, editor and two idle agent panes remain in a split after visiting another pane (link only)](../../assets/demos/10-mixed-splits.gif)

A terminal, editor and two idle agent panes share a split view. Setup is cut and an account notice is redacted. Explained in [Panes and layout](panes-and-layout.md). Historical and not embedded since the 2026-11-05 keyframe review: one frame shows an unredacted login-expiry account notice and some frames show scratch-path fragments.

## Select and copy terminal output

![Smart Copy selects and pastes a code from real Codex output](../../assets/demos/11-smart-copy.gif)

Smart Copy uses local inference to select and paste a code from real Codex output. The private clipboard matches the source bytes. Setup and a model wait are cut, reading holds are added, and local paths and an account notice are labelled redactions; automatic pane naming did not apply. Explained in [Smart Copy](smart-copy.md).

## Queue prompts for an agent

![Queue two prompts for a real Claude agent; Ilium sends each one after the previous turn ends, in order.](../../assets/demos/13-prompt-queue.gif)

Prompts queued for an agent are delivered one by one as each of its turns finishes. Explained in [Automation](automation.md).

Disclosure: this is a current-build recording of a real Claude (Haiku) agent in an isolated lane; the agent's file output is real. The idle wait before the first prompt and part of the first turn's idle time are cut, and short reading holds are added, as shown by the loop clock. Names in the tree (`Exam University`, `Exam TA`) are staged. The readback that proves FIFO delivery (server log, Claude transcript and the driver key log: separate turns 2.75 s and 1.82 s after each previous turn ended, no key typed after the second Enqueue) was adjusted for this take to ignore the shell line that launches the agent, which the server log also records as a submitted prompt; the rest of its checks are unchanged. Earlier versions of this GIF used a synthetic Codex-shaped fixture. Review limit (2026-11-05): 93 of 1,297 distinct frames were viewed at thumbnail size and the keyword scan did not complete, so no full-frame review is recorded; nothing sensitive was seen in the sampled frames.

## Get results from background tasks

[Historical recording of a real Claude agent monitoring a ten-card task (link only)](../../assets/demos/14-progress-monitor.gif)

Ilium tracks the task and delivers the completed result to a real Claude agent. Setup is cut and an account notice is redacted. Historical and not embedded: a scan found local scratch-path text in some frames (for example in a `python3` command line) and no full-frame review was made. Explained in [Automation](automation.md).

## Search agents, files, and terminals

[Historical recording of workspace search across agents, files and terminals (link only)](../../assets/demos/17-workspace-search.gif)

Workspace search finds agents, files and terminal contents and jumps to the chosen match. This recording uses real Claude and Codex agents, cuts setup and adds reading holds; automatic restructuring is disabled in the isolated demo. Historical and not embedded: a scan found a local scratch path in the search results and no full-frame review was made. Explained in [Panes and layout](panes-and-layout.md).

## See live agent goals in the project tree

![A live Codex goal changes the objective icon in the project tree](../../assets/demos/22-goal-indicators.gif)

A real Codex goal changes from active to paused and back beside the agent in the project tree. Setup is cut; captions, a closing hold and labelled account and path redactions are added. Automatic naming and restructuring are disabled. Explained in [Agent monitoring](agent-monitoring.md).

## Recover a session after a simulated reboot

![A simulated reboot card precedes an isolated Ilium server shutdown and relaunch; a local fixture recovers a saved note](../../assets/demos/24-agents-survive-reboots.gif)

An isolated Ilium server actually shuts down and relaunches, and a local session fixture recovers a saved note. The reboot card is simulated and editorial; no machine reboot or real provider is used. Idle waits are cut and caption holds are added. The PlainShell snapshot-resume fix (commit `61653978`, unit-tested) is in source and a fresh release builds, but this GIF is not real provider recovery footage and was not re-recorded; that needs the user's Claude and Codex quota. Two 2026-11-05 attempts to re-record with real agents produced no GIF: the first (Claude and Codex) ended with a non-zero exit code because Codex stopped at its folder-trust prompt, and about twenty Claude-only retries each failed on a timeout because Ilium's agent detection intermittently marked a healthy Claude pane unavailable under heavy machine load (cause suspected in `shell_owns_terminal` and `agent_input_available`; not fixed). No candidate was reviewed or published, so this stays a historical fixture recording; a real-agent redo needs a quiet machine or a server-side detection fix, and Codex has a usage limit until 2026-11-10. Explained in [Session recovery](session-recovery.md).

## Choose an animation, then watch it play

![Thirteen background animations, each shown in Settings and then in its full-screen preview on the 2026-11-05 release build](../../assets/demos/26-animations.gif)

Refreshed 2026-11-05 on the installed release build (SHA-256 `83f4db21...f420`, 13 chapters, 63.6 s, 1629x851). Each chapter shows about 0.4 s of the real Settings > Animations page, then 4.5 s of that scene's real full-screen preview, with default settings and an `NN / 15` caption bar. The scenes are Moon over moving water, Steam above a tea cup, Kelp in a gentle current, Drifting cloud islands, 3D pipes, Dithr patterns, Vector TD, Window sunlight, Paper-fold trace, Embroidery orbit, Turning wallpaper, Unfinished circle and Threads through a needle. Chapter numbers skip 5 and 9: Two gentle wave sources and Digits of Pi were removed after the keyframe review because the full-screen hint was overprinted by the animation. Omitted as unreadable or too faint: Clouds, Fireflies, Dithered Waves, Voxel Landscape, Solar System, Frost, Lighthouse, Delayed Reflection and Stars. Voxel showed no recognizable terrain (a uniform grey field filling only the top-left, first populated frame about 36 s after full screen with Java default textures on a heavily loaded machine, and blank for more than 8 minutes with the default texture pack). The other scenes, including Video, Images, Spectrum, Night Lights and the data-fed scenes, were not captured. Frames are real client output sampled at 8 fps; nothing is synthesized. The navigation steps between scenes are cut and the first 4 preview frames of each scene are trimmed. Vector TD and Dithr patterns show built-in demo content. Many scenes are dim grey dither on a dark background and read weakly at small size; Vector TD's attribution touches the right edge. Reviewed from 15 keyframes (one per scene) plus two full-width frames, not frame by frame. The previous gallery (2026-10-26, SHA-256 `7705e61f...1431`, 60 scenes) remains in Git history. Explained in [Animations](animations.md).

## Schedule input for a pane

![Ilium schedules input for a real Claude agent and a shell](../../assets/demos/04-scheduled-input.gif)

Ilium sends scheduled input to a real Claude agent and an echo-only terminal fixture in separate takes. Setup is cut, reading holds are added, and local paths are redacted. On 2026-11-05 the captions were retimed by a caption-strip-only edit: the caption no longer changes while the dialog is open, the "agent receives the prompt" caption starts when the prompt first appears, and two captions that would have lasted 100 ms were removed. A decoded comparison confirms only the caption strip changed in the final 37-frame retiming. The first take still has a near-static hold of about 17 s, and the Claude header shows a plan label (not an account notice or quota). Explained in [Automation](automation.md).

## Set interface and board preferences

![The first chapter of the settings tour shows interface and board options](../../assets/demos/07a-settings-tour-a.gif)

The first chapter shows interface and board options. Agent Cost, Optimization and Remote are omitted to protect local data; setup is cut and reading holds are added. Explained in [Settings](settings.md).

## Configure voice and AI providers

![The second chapter of the settings tour shows voice and inference providers](../../assets/demos/07b-settings-tour-b.gif)

The second chapter of the settings tour covers voice, reset planning, inference and instructions. Provider credentials are unconfigured; setup and other pages are cut, with reading holds added. Explained in [Inference and privacy](inference-and-privacy.md).

## Configure automatic titles

![The third chapter of the settings tour shows automatic titles and setup options](../../assets/demos/07c-settings-tour-c.gif)

The third chapter covers titles, triggers, custom agents, debugging, API and setup. Setup paths belong to the isolated demo workspace; other pages are cut and reading holds are added. Live provider and plugin editors are not exercised. Explained in [Titles and instructions](titles-and-instructions.md). Privacy-masked 2026-11-05 after a keyframe review found a local scratch path on the Setup page (late frames); masked rows are filled with the background colour so those lines appear blank, and all other pixels, timing and frame count are unchanged (decoded-frame comparison with the original).

## Rearrange panes without stopping their processes

![Move the Tests fixture pane into Exhibits above Build; the same local fixture process keeps running](../../assets/demos/09-pane-tree.gif)

The real Ilium tree moves a local Tests fixture pane into Exhibits and above Build. Its PID and heartbeat continue through the move; the terminal text comes from a fixture shell, while two real agents remain visible. Setup is cut, captions and a closing hold are added, and automatic naming and restructuring are disabled in the isolated demo. Explained in [Panes and layout](panes-and-layout.md).

## Request updates from active agents

![Ilium asks idle Claude and Codex agents for updates while skipping an agent awaiting approval](../../assets/demos/12-ask-for-update.gif)

Ask for status update sends the built-in prompt to idle Claude and Codex agents. A third agent waiting for command approval receives no prompt. Pane names were staged and automatic naming and restructuring were disabled in the isolated session; setup and waits were cut and a reading pause was added. Explained in [Agent monitoring](agent-monitoring.md). Privacy-masked 2026-11-05 after a keyframe review found local scratch paths and title-inference error text; masked rows are filled with the background colour so those lines appear blank, and all other pixels, timing and frame count are unchanged (decoded-frame comparison with the original).

## Track work on a Kanban board

![Move a staged Kanban card to Doing, mark it complete, add a note, then reopen the board and verify the saved Markdown file. Setup is cut; column width is 15 cells, with added captions, pointer, arrows and holds](../../assets/demos/15-kanban.gif)

The current Ilium board moves the card into Doing, marks it complete and adds a note. The backing Markdown file is opened to verify the saved values, then the board is reopened. The isolated fixture has no provider agents; automatic naming and restructuring are disabled. Explained in [Editors and boards](editors-and-boards.md). Privacy-masked 2026-11-05 after a keyframe review found a local scratch path in the file-picker title; the masked callout area in the final scene is left blank; masked rows are filled with the background colour so those lines appear blank, and all other pixels, timing and frame count are unchanged (decoded-frame comparison with the original).

## Start a Codex agent from a TODO

[Historical recording: Start Codex from a TODO line and have it repair the source (link only)](../../assets/demos/16-agent-from-line.gif)

A real Codex agent starts from a TODO line and repairs the source. The recording cuts setup and adds reading holds; automatic title inference reports that no user transcript is available. Explained in [Editors and boards](editors-and-boards.md). Historical and not embedded since the 2026-11-05 keyframe review: a scratch path with the local username and a title-inference error are visible.

## Detach and reconnect while agents keep running

![A real Claude agent continues writing files while Ilium is detached, then reappears on reconnect. Setup and waiting are cut, an account notice is redacted, and automatic naming and restructuring are disabled.](../../assets/demos/18-detach-reattach.gif)

The client detaches and later reattaches while panes and agents keep running in the server. Explained in [Panes and layout](panes-and-layout.md).

## Edit Markdown in a pane

![Edit and preview a staged Markdown note in Ilium, then read the saved file in a terminal. Setup is cut; captions, pointer, arrows and holds are added.](../../assets/demos/19-markdown-editor.gif)

A staged Markdown note is edited and previewed in a pane, then read back from disk in a terminal. Automatic naming and restructuring are disabled for the recording. Explained in [Editors and boards](editors-and-boards.md).

## Coordinate local models with Chatroom

![Two local models generate messages that a script posts to Ilium Chatroom](../../assets/demos/20-chatroom.gif)

Two local models generate a planner message and a reviewer response in separate panes. A shell script posts both messages to an isolated Chatroom; the models do not call Chatroom tools autonomously. Waits are shortened. The local username and host name in the shell prompt were masked with background-coloured blocks in the 61 frame runs that showed them (decoded comparison against the original); only the cursor remains. The final frames show a "No live agent panes in this project" panel. Explained in [Automation](automation.md).

## Respond to confirmation prompts automatically

![A text trigger responds to an agent confirmation question](../../assets/demos/21-text-triggers.gif)

A text trigger recognises a real Claude agent's confirmation question and answers it automatically. The recording cuts setup and an idle wait, adds reading holds, and disables automatic naming and restructuring in the isolated demo. Explained in [Automation](automation.md).

## Restore a saved session

![An isolated Ilium server shuts down and relaunches; a local fixture recovers a saved note](../../assets/demos/23-snapshot-resume.gif)

This recording shows an actual isolated Ilium server shutdown and relaunch, followed by a local session fixture recovering a saved note. The reboot card is simulated and editorial; no machine reboot or real provider is used. Idle waits are cut and caption holds are added. Explained in [Session recovery](session-recovery.md).

## Run agents in their own worktrees

![Ilium creates a worktree through its native workflow and starts a local fixture in an isolated demo project](../../assets/demos/25-worktree-agent.gif)

The recording shows Ilium creating a real worktree through its native workflow in an isolated demo project and starting a local fixture instead of a provider agent. Terminal states are held, captions are added, and a path is redacted. A second coding agent reviewed all four decoded frames (2026-11-05) and found no sensitive text. The creation step is not shown and the caption says the path is redacted although none is shown. Explained in [Worktrees](worktrees.md).

## Convert a session between Claude and Codex

[Historical recording of Claude and Codex session conversion (link only)](../../assets/demos/26-convert-session.gif)

Real Claude and Codex sessions are converted in both directions and recall a fictional mascot from the conversation. Setup and an idle wait are cut, account notices are masked, and captions are added. Historical and not embedded: a review on 2026-11-05 found that local scratch paths (including the username), a plan label and a session id remain visible, so the "account notices are masked" statement covers notices only, not paths. The mask was corrected for the changed terminal layout; all 973 decoded frames and their durations were compared against the earlier recording outside the intended masks. The isolated capture disables automatic project restructuring and uses local title inference. Explained in [Session recovery](session-recovery.md).

## Queue access to a shared resource

![Two isolated projects queue for, take and release a shared resource in the full interface](../../assets/demos/28-resource-queue.gif)

Two isolated projects share a demonstration resource through staged local fixture participants: one reserves it, the other queues, takes ownership after the first releases it, then releases it. No provider calls or hardware are used. The recording shows the full interface with added captions and redacted project paths. Explained in [Resource queues](resource-queues.md).

## Recording provenance and review limits

The gallery descriptions above state each recording’s subject and disclosed fixtures, edits and limitations. The table below indexes the 31 gallery files as observed on 2026-11-03. The recording date in the Git history is the last asset change, not necessarily the date of capture or proof that the recording represents the current build. Matching aliases are separate files in the same `assets/demos/` directory; equal hashes establish byte equality at this check, not independent capture evidence.

A coding agent inspected overview contact sheets for all 31 files. Those are sampled frames: they do not establish every action, provider identity, timing, outcome or complete visual acceptance. Recheck the asset hash and the affected evidence after any replacement or re-render.

A coding agent reviewed all 699 decoded frames of the Resource Queue replacement, SHA-256 `dc0b1a994610b6296ae3d66d6073e0a89bdaab2b1ae6b168904c38502fc784d2`. Isolated status readbacks identify distinct projects and record ownership, queueing, handoff and final release. The participants are local fixtures; the recording uses captions and path redactions and makes no provider calls.

The original provider-transcript archive manifest records 102 distinct missing source paths for 97 session IDs (41 Claude and 56 Codex). All 102 exact indexed paths were absent when checked on 2026-11-03. This is an aggregate of the original manifest, not proof that no equivalent transcript exists elsewhere; no missing identifiers are reconstructed here.

| Gallery GIF | Matching alias | Last asset change | SHA-256 |
| --- | --- | --- | --- |
| `27-attention-queue.gif` | None | 2026-11-05; timestamp masked | `c7e9b204333e70ccc5cad94cfbbd87deef6c772ca34da86c46585ff509cdea96` |
| `01-ai-tree.gif` | `12-ai-tree.gif` | 2026-11-05; re-recorded with real agents and OpenAI gpt-4.1 | `2beff5bab90956accc99f85245a9c77e5925df100fbd87148ee49f5ae7f22f31` |
| `02-voice-control.gif` | `22-voice-control.gif` | 2026-11-05; re-recorded with OpenAI Realtime and synthesized speech | `4a306fe0dd48457ce0de0e0d538211c8174dcd0e279407b0e0c00c2b0ce38511` |
| `03-contextual-titles.gif` | `20-contextual-titles.gif` | 2026-11-06; re-recorded with two real Claude Code panes | `9a39847a9b20a159e673d7158efa22d5a9b8316e0128bc1be00d1b14fff0e48b` |
| `05-screen-transfer.gif` | `18-screen-transfer.gif` | 2026-11-05; privacy-masked | `4812c05310aa789b44b1ee84b3ef140e0567243c4099f903432597c9027627fc` |
| `06-cost-and-stats.gif` | `23-cost-and-stats.gif` | 2026-11-05; privacy-masked | `45251571e9294d48c27f2b78f5adda10782bff3ff40c9de9e2f881a38ec78ffb` |
| `08-agent-activity.gif` | `01-agent-activity.gif` | 2026-11-05; privacy-masked | `552b9dd3abc26f1ecaa0a8116d31ba505f41bbc7db3f450e6e004319ce74e981` |
| `10-mixed-splits.gif` | `03-mixed-splits.gif` | 2026-10-30 | `b5b9bfadf4f38c5f423e2b0a531bbc5a96a4789426498d4d4b48cb1a5d8b45d7` |
| `11-smart-copy.gif` | `04-smart-copy.gif` | 2026-10-30 | `7924d3c4085d25450c742ad5b6e64b44650b9befe69812405cb4c03b25093d57` |
| `13-prompt-queue.gif` | `06-prompt-queue.gif` | 2026-10-31 | `6992ce97e186233d367a49087c2cf13fead5dfe5ac175ea70fa44460859e936c` |
| `14-progress-monitor.gif` | `07-progress-monitor.gif` | 2026-10-30 | `3eca3c1b8b4dc2e5f91ab0f6b8b67c3fe5e5917274f65b6e3a598cd783255f22` |
| `17-workspace-search.gif` | `10-workspace-search.gif` | 2026-10-30 | `4760941409b9c35883e740799b32647563686254a0b9f2bf1c1dce3de3786fe0` |
| `22-goal-indicators.gif` | `17-goal-indicators.gif` | 2026-10-30 | `f57dc9d67fcb85ddd7a21b41019ea6189e5c4f153a61105bb01a0d20da7ff3fa` |
| `24-agents-survive-reboots.gif` | `19-snapshot-resume.gif`, `23-snapshot-resume.gif` | 2026-10-26; working file updated | `7c24d61380e6c6bf49b23b0d45a209473590066be4eda305f9ef05db5eee1e78` |
| `26-animations.gif` | None | 2026-11-05; re-recorded, scenes 5 and 9 cut | `83f4db21b2313e5653c6375d50641641c133acdf6c6a5149459ade5ce5d7f420` |
| `04-scheduled-input.gif` | `15-scheduled-input.gif` | 2026-11-05; captions retimed | `96e52edeabae056a1acfbebc6b4384ab8ab313acc651288e891b108dcb4ed65a` |
| `07a-settings-tour-a.gif` | `21-settings-tour-a.gif` | 2026-10-30 | `0c078fe5aca220e48c713299b4a0b77deb2fa223a7474689b5473c3de2cecb9e` |
| `07b-settings-tour-b.gif` | `21-settings-tour-b.gif` | 2026-10-30 | `9b086a03958d279cf0ff2afbeee56de77554d7f63b3a5c49b7508c24f7aa8ce4` |
| `07c-settings-tour-c.gif` | `21-settings-tour-c.gif` | 2026-11-05; privacy-masked | `77132acf126048c94b6ff0435f0085086e16e4ad7e644c800d9347694a5755a9` |
| `09-pane-tree.gif` | `02-pane-tree.gif` | 2026-10-30 | `1b2894cf6059ade922c615ed070c70b291415023ffccaa4dad93038d096e9c5e` |
| `12-ask-for-update.gif` | `05-ask-for-update.gif` | 2026-11-05; privacy-masked | `312ad6ca9bc7963cae40216a845b9d6b556e56716f93577e3b7529629d743f5f` |
| `15-kanban.gif` | `08-kanban.gif` | 2026-11-05; privacy-masked | `47fa52861624d0eee39ffe992202c0b795bad9231bd377b4551bf2e94b39f3bb` |
| `16-agent-from-line.gif` | `09-agent-from-line.gif` | 2026-10-30 | `77d3edb861067a3b15927494fa1f11d71876bd9c2541bc4472bdba2667574df3` |
| `18-detach-reattach.gif` | `11-detach-reattach.gif` | 2026-10-30 | `5110603b06dc3dc8c856be745c8204924e0ea3e9c2278354ae2a821e6ca26139` |
| `19-markdown-editor.gif` | `13-markdown-editor.gif` | 2026-10-30 | `c08e3ed6558b85a2769f714bb31db076a64b5934c4f4d316f6328127d7d9ccc5` |
| `20-chatroom.gif` | `14-chatroom.gif` | 2026-11-05; prompt masked | `ed59a501a39d02e85cb7412eb5a4f9ad9b433226255dabd9db362f82aa5c01ff` |
| `21-text-triggers.gif` | `16-text-triggers.gif` | 2026-10-30 | `461bb2c548b75bc4226cc693b60619cc5be33084f3525bc7767f24774721d236` |
| `23-snapshot-resume.gif` | `19-snapshot-resume.gif`, `24-agents-survive-reboots.gif` | 2026-10-26; working file updated | `7c24d61380e6c6bf49b23b0d45a209473590066be4eda305f9ef05db5eee1e78` |
| `25-worktree-agent.gif` | None | 2026-10-31; working file updated | `491060abd123dbf8a9d069c204d23b98f52ffd9941d6be40361807509bc0f1f6` |
| `26-convert-session.gif` | None | 2026-11-04; redaction corrected | `d66428fee7441b36c69eb28b933670e01252a429f3c509b63ad0b6073de0b89b` |
| `28-resource-queue.gif` | None | 2026-11-04 | `dc0b1a994610b6296ae3d66d6073e0a89bdaab2b1ae6b168904c38502fc784d2` |

The previous Snapshot Resume recording had a caption claiming both agents answered, but sampled frames showed only a Codex answer; it was replaced by a seven-frame fixture GIF in an earlier round, and no newer replacement has been published. All seven decoded frames of that fixture GIF were reviewed by the coding agent, SHA-256 `7c24d61380e6c6bf49b23b0d45a209473590066be4eda305f9ef05db5eee1e78`. The capture used an isolated Ilium server shutdown and relaunch and a local session fixture recovering a saved note; the reboot card is simulated, with no machine reboot or real provider. Idle cuts and caption holds are editorial. Recheck the hash and review if the file changes. The animation gallery was re-recorded on 2026-11-05 (see its section). All four decoded frames of the replacement Worktree Agent GIF were reviewed by the coding agent, SHA-256 `491060abd123dbf8a9d069c204d23b98f52ffd9941d6be40361807509bc0f1f6`; the native dialog, created branch, local fixture, captions and redaction are visible. This is visual evidence for that exact file; recheck the hash and review after any replacement. See the individual descriptions above for each capture’s other limits.

The plugin manual has separately maintained copies, not listed as root aliases in the table. Its source pages reference `docs/plugins-manual/assets/demos/`; its fallback builder copies that directory into the served, generated `docs/plugins-manual/_site/assets/demos/`. On 2026-11-03, all 30 same-named GIFs in each manual directory matched their root assets by SHA-256 after the previously missing generated Attention Queue copy was restored. The recordings and their consumers have distinct provenance:

- **Contextual Titles:** the manual source and generated GIF copies were resynced on 2026-11-06 and match the root real-Claude recording at SHA-256 `9a39847a9b20a159e673d7158efa22d5a9b8316e0128bc1be00d1b14fff0e48b`. 16 evenly spaced keyframes were independently reviewed; sampling does not inspect every frame.
- **Prompt Queue:** the manual source and generated copies now match the root real-Claude recording. The source and generated scheduler descriptions disclose the staged names, cuts and reading holds.
- **Snapshot Resume:** root and manual source and generated aliases now match the seven-frame reviewed fixture GIF. The earlier one-Claude review concerns a different recording.
- **Agents Survive Reboots:** root and manual source and generated aliases now match the same reviewed fixture GIF as Snapshot Resume. The reboot card is simulated; the isolated Ilium server shutdown and relaunch are real.
- **Worktree Agent:** the root, manual source and generated GIF copies match the native-workflow recording. A coding agent reviewed all four decoded frames of the hash identified above.

The generated manual is served from `_site/`, so changing source alone does not update that output. No Resource Queue copy was found in either manual asset directory. Recheck copies and their descriptions together after replacements, preserving original recordings.

On 2026-11-05 the manual source and generated copies of the Animations, Chatroom and Attention Queue GIFs were resynced to the root files. The AI Tree, Voice Control, Screen Transfer, Cost and Stats, Agent Activity, Ask for Update, Kanban and Settings tour C copies were resynced to the root files on 2026-11-05; other manual copies may still show earlier recordings, and the manual itself was not rebuilt.

## Refresh status, 2026-11-05

Updated 2026-11-06 (round 4): Contextual Titles was re-recorded and the Screen Transfer re-mask passed an independent review; the rows for Snapshot Resume, Mixed Splits, Agent from Line, Convert Session, Progress Monitor and Workspace Search record the 2026-11-06 attempts or reviews. Their files are unchanged.

Status of each gallery GIF after the 2026-11-05 keyframe review pass. "Refreshed" means re-recorded or edited on the current build with review evidence; "historical" means not refreshed, with the reason; "not embedded" means the GIF is linked, not shown, because the review found private text. Review standard: about 10 to 20 keyframes per GIF as contact sheets plus crops; no GIF was reviewed frame by frame, and decoded-frame comparison was used only to prove mask edits. The 102 missing original provider-session paths remain unrecovered and are not reconstructed.

| GIF | Status | Review evidence and limits |
| --- | --- | --- |
| 26 Animations | Refreshed 2026-11-05, SHA-256 `83f4db21b231...f420` | 13 scenes on the release build; 15 keyframes plus 2 full frames before two overprinted scenes (Two gentle wave sources, Digits of Pi) were cut, then 13 keyframes after. Scenes are dim; 47 of 60 scenes are not shown (omitted list in its section). |
| 27 Attention Queue | Refreshed, timestamp masked 2026-11-05, SHA-256 `c7e9b204333e...ea96` | 5 of 69 stored frames changed (decoded comparison); 16 keyframes viewed; no other private text seen; one caption lingers about two frames. |
| 20/14 Chatroom | Refreshed (scripted posts), prompt masked 2026-11-05, SHA-256 `ed59a501a39d...01ff` | 61 frame runs changed (decoded comparison); 16 keyframes plus two crops viewed; no prompt visible. Shows a "No live agent panes" panel at the end. The plugin-manual copies were resynced. |
| 28 Resource Queue | Refreshed (fixtures) | 16 keyframes viewed; a garbled fixture reservation id may appear in some frames (not private data). |
| 25 Worktree Agent | Refreshed (fixture) | 4-frame GIF; frames 1 and 3 viewed in the latest pass, all 4 in an earlier one; no sensitive text. |
| 13/06 Prompt Queue | Refreshed 2026-10-31 (real Claude) | 18 keyframes viewed; real wall-clock text ("done 12:49 PM") appears in later frames and is not masked; caption lag in two frames. |
| 04/15 Scheduled Input | Refreshed, captions retimed | 18 keyframes viewed; small caption lags; the second take is a labelled local fixture; the agent reply to the typed prompt was not confirmed in the sampled frames. |
| 07a/b settings tours, 09 Pane Tree, 11 Smart Copy, 18 Detach and Reattach, 19 Markdown Editor, 21 Text Triggers, 22 Goal Indicators | Retained, keyframe review passed 2026-11-05 | 12 keyframes each; no private text; Pane Tree has a caption held across about 57 frames. |
| 02/22 Voice Control | Refreshed 2026-11-05, SHA-256 `4a306fe0dd48457ce0de0e0d538211c8174dcd0e279407b0e0c00c2b0ce38511` | Real OpenAI Realtime take (gpt-realtime-2.1) with synthesized speech into a virtual microphone; six commands, each confirmed in the server log; 16 keyframes reviewed by the producer and an independent reviewer; the cut waits are not marked on screen. Supersedes the Vosk three-action recording. Recorded on a build with the uncommitted capture-gating fix. |
| 03/20 Contextual Titles | Refreshed 2026-11-06, SHA-256 `9a39847a9b20...e48b` | Two real Claude Code panes plus a shell, local Ollama qwen3:8b titles; Codex not recorded (usage limit until 2026-11-10, key returned 401), disclosed. Harness green with 32 of 33 checks; provider_spend_measured failed because the run could not be priced in dollars (16268 raw tokens, 6068 fresh). 16 keyframes by an independent reviewer: whole UI visible, captions match, short alias path only; the Claude banner shows the plan label "Claude Ultra". Cuts: boot and a 45 s wait (captioned). The earlier round-3 "exit code 3" was the preflight Ollama check timing out under load, not Ilium detection. |
| 01/12 AI Tree | Refreshed 2026-11-05, SHA-256 `2beff5bab90956accc99f85245a9c77e5925df100fbd87148ee49f5ae7f22f31` | Real Claude Code agents and a real gpt-4.1 restructure; 16 keyframes reviewed by the producer and an independent reviewer; the status bar shows "empty - skipped" for the first 30 seconds; the final caption names the model but the frame shows no model name (the claim rests on the server log of the take). |
| 19/23 Snapshot Resume, 24 Agents Survive Reboots | Historical fixture (one shared file, SHA-256 `7c24d61380e6...1e78`) | Not replaced. Re-attempted 2026-11-06 with one real Claude agent without nice-19 on a machine at load 35-56. The real recording worked (server replaced, live `claude --resume`, Claude recalled its hidden code), but the rendered candidate was rejected on keyframe review: status-bar error text, a scratch path in the banner, a weekly-quota notice, and a transient "Claude unavailable" row after the restart. Not published. A re-record needs the lane's AI restructure disabled, a quota-free account, masks for the path and a quiet machine. Reboot card simulated; idle cuts and caption holds are editorial. |
| 05 Screen Transfer, 06 Cost and Stats, 07c settings tour C, 08 Agent Activity, 12 Ask for Update, 15 Kanban | Refreshed 2026-11-05, privacy-masked | Local scratch paths, account notices and title-inference error text were masked frame by frame (decoded-frame comparison with the originals confirms only the masked rows changed); 7 to 12 keyframes each reviewed by the producer and an independent reviewer. The first Screen Transfer mask missed a path fragment under the arrow animation in about 100 frames; it was re-masked and rechecked. The re-mask (SHA-256 `4812c05310aa...27fc`, 452 frames, 05 and 18 byte-identical) was independently re-reviewed on 2026-11-06: decoded-frame comparison with the backup original shows changes only inside one box (x 373-1150, y 519-765: the Codex cwd header and footer tail) in every frame, 13 keyframes plus edge crops show no path fragment or seam. Cost and Stats shows a floating ACCOUNT NOTICE REDACTED box briefly (already in the original) and Agent Activity a labelled status redaction. Originals are in the maintainer backup folder. |
| 10 Mixed Splits, 16 Agent from Line | Historical, link only | 2026-11-06 keyframe review (16 keyframes each): masking judged infeasible because the login-expiry notice and path fragments (Mixed Splits), or the scratch path in the goal text, the Codex footer path, a wall-clock time and the title-inference error (Agent from Line), appear in several shifting layouts and scrolling text. Not re-recorded. |
| 26 Convert Session | Historical, link only | 2026-11-06 keyframe review (16 keyframes): scratch paths with the username, session ids and wall-clock times (12:24 PM, 12:25 PM) scroll through the conversion log, the pane title and the Codex footer, so a static pixel mask cannot cover them reliably. Not re-recorded. |
| 14/07 Progress Monitor, 17/10 Workspace Search | Historical, link only | Scratch-path text is visible. Not masked and not re-recorded; the 2026-11-05 review stands and these two were not re-viewed on 2026-11-06. |

`14-chatroom-limit-preview.gif` is an orphan file from 2026-09-27 that no page references; it was not reviewed. Blockers: Contextual Titles has no Codex pane until a working Codex login or key exists after the usage limit resets (2026-11-10); Snapshot Resume and Agents Survive Reboots need a quiet machine, a quota-free account, masks and the lane's AI restructure disabled (and Codex after 2026-11-10); Mixed Splits, Agent from Line, Convert Session, Progress Monitor and Workspace Search each need a re-record or pixel masking.
