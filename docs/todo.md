# General

* [ ] Option for a standalone app.
* [ ] In standalone app, move rendering to libghostty (might be mac-only, maybe make this just a preference?)
* [ ] In standalone app, move to GPU based rendering (optionally)
* [ ] In standalone app, add browser tabs.
* [-] Horizontal and vertical split panes, with nice display/catch in the tree view (see vs code terminal list)
* [x] Detect when an agent is sleeping/waiting for things, and have a specific animation for it.
* [ ] HTTP API, and IPC.
* [ ] vi-mode shortcuts.
* [ ] Options for both vertical and horizontal tabs. Options for top/bottom, left/right tabs.
* [ ] Option for git branches to show up in tabs, with 3 options: yes/no/show branch only when it's not X (main, master)
* [ ] Option for recency (<1m) to the right of each entry in the tree.
* [x] Options for Ctrl+A or Ctrl+B or others (explore which works/can work, give all options but explain which are good ones)
* [ ] Option to create an agent with a worktree (different menu options **and** agent creation dialog)
* [ ] Option for a given folder to "be" a worktree/be associated with a worktree (option in folder creation dialog)
* [ ] Docker sandboxing, isolating workers/agents in containers with shared auth (idea from aoe)
* [ ] Git review panel/entry in the tree that lets us see the current git status, and review diffs, and automagically create commits using LLMs based on the data we have.
* [-] Sound notifications, configurable.
* [ ] Add support for more than claude and codex, have nice abstractions (object oriented) in the code, have support for the resume stuff, the detection stuff, etc.
* [ ] Support for displaying useful status information like notifications about stuff that needs attention in user tmux bar.
* [ ] Kubernetes control/orchestration with control from the agents (?)
* [ ] Right click on a given agent to fork it into a new forked session using the stuff in claude code that lets us do that.
* [ ] File navigation entry in the list.
* [ ] File search entry in the list.
* [ ] Support for themes, and import some themes from vs code and some other terminal-based goodies like oh-my-zsh
* [ ] Desktop/OS notifications.
* [ ] Full configuration screen with great user experience.
* [ ] Kanban board tab/panel type (with cards on the left and details on the right and auto-resize based on focus with mouse/keyboard, plus keyboard navigation, and up/down/left/right arrows to move them around), with creation dialog that lets you have it either be file based (one file for each column) or folder based (one folder for each column, each entry is a file)
* [ ] Left-panel file explorer, and "drag file to agent" feature.
* [ ] Optional git +32/-11 green/red dispaly next to the branch "second line" showing the additions/removals in git for this terminal/session/group/worktree/etc.
* [ ] Worktree "groups"/folders have a special icon, and right click menu that lets us do things like commit/push with llm.
* [ ] Explore if https://github.com/a-kenji/tui-term is a good option for our terminals.
* [ ] When an agent exits, if it was created from a terminal that terminal "becomes" a terminal again, correctly.
* [ ] Explore https://github.com/doy/vt100-rust
* [ ] Add support for gemini, opencode, and other agent types.
* [ ] Create pull requests from a worktree (either a group or a terminal or agent etc)
* [ ] Fix the project-wide ilium/ilium typo.
* [ ] Docker sandboxes as an option for groups and for agents and for terminals, letting them "share"/enter an existing sandbox too, as part of the dialog/modal when creating a new agent/terminal/etc.
* [ ] Spec/Plan creation and execution.
* [ ] Machine control panel/tree entry, lets us use visual agents and screen caps to detect useful information from screen caps and click in interresting/useful places?
* [ ] Voice dictation/input.
* [ ] Fully voice navigation and user interface usage using the latest gpt models, including letting it re-organize everything.
* [ ] « Magic »: LLM-based re-organizing and re-labelling of everything including giving the LLM screen caps of each terminal and asking it to sort them into folders and rename each thing.
* [ ] Make it a settings option if a worktree-associated group/folder can have terminals/things that use another worktree/none, or if that's forbidden.
* [ ] Search through everything including terminal histories and agent histories, with a search result pane where if you click on a result it gets you to that pane/window at the right place etc.
* [ ] Agent group chat, with each agent being given access to a chat group (in the form of a text file), and the user can monitor this.
* [ ] Agent group chat -> agents know their session ID, can get their name from ilium with api calls, have a chat setup thing where we add instructiions to the project's CLAUDE.md/AGENTS.md to "set up" the group chat giving them the means to monitor it and opening a group chat monitoring "pane", with the main goal being that agents don't interfere with each other, or sending instructions to all agents.
* [ ] Agent group chat -> if possible add per-project (.claude/) hooks that tell the agent to check the group chat if it has changed etc, and to update it when it does something important, or with warnings for others, etc.
* [ ] Todo file to kanban board tool (with llm and context)
* [ ] Optionally, instead of using kilo-gateway for small llm tasks, use claude code with low instrumentation (no tools, no system prompt) and have us run the agent "in the background" giving it a simple task and asking it to answer with json to a /tmp/ file or something
* [ ] Optional git conflict detection where we mark agents that share a branch/worktree with a warning icon and/or callout at the bottom, and an option/button to solve the problem or ignore it.
* [ ] Agent "roles" where agents are started with a "role" prompt and monitor the group chat for @mentions and when received and if they are not doing something already they take care of it (and if they are doing something there's a qaueue or something)
* [ ] Task "templates" where we can define templates for tasks with prompt templates and forms and we have a list of those templates and we can run/create one where we fill a form and that's used to generate the prompt and start a new agent with that prompt or put it in the copy/paste buffer.
* [ ] Telegram/chat channel panels/windows.
* [ ] Optionally add ressources to groups, like a GPU only one can use at a time, or only one is allowed to compile at a time, etc. A semaphore they share. Agents that don't have the allowance just sleep until they do/can. They should use the group chat to figure out who is next and negociate that based on priorities and potentially ask the user questions with a special question format.
* [ ] Detect if an agent (claude) has a goal active, and add a goal "flag" to it if it does. Same thing for dynamic workfflows.
* [ ] Have both icons for "create folder" and "create folder at root" in the menu at the bottom of the left par
* [ ] Icons at the bottom of the left bar should show anytime the left bar is focused, but their "extensions" like "create with options" should show only when the mouse gets close
* [ ] Add "create with options" menus for terminals, groups, agents, etc in the menu bar at the botto of the left panel
* [ ] When we get titles for the agents from the llm, get two titles, a short one for the left panel being thin and a longer one baut not too long for the left panel being wider.
* [ ] In agents detect the pattern « ✔ Goal achieved (1h · 1 turn · 210.7k tokens) » (this one is for claude, figure out the codex one) and add a "goal" or "success" type flag/icon to that agent
* [x] Save and restore the buffer/history of terminals.
* [x] New "type" in the left panel, folder (not group) to navigate/see files on disk and easily/quickly open them.
