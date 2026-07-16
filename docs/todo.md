# Board

## Application shell and terminal rendering
- [ ] Offer ilium as a standalone desktop application.
- [ ] Let the standalone application use libghostty for terminal rendering, if platform support allows it; otherwise expose it as an optional renderer.
- [ ] Add optional GPU-accelerated rendering to the standalone application.
- [ ] Add browser-style tabs to the standalone application.
- [-] Support horizontal and vertical split panes with a clear representation in the tree, similar to the VS Code terminal list.
- [ ] Support horizontal or vertical tab layouts, positioned at the top, bottom, left, or right.
- [ ] Optionally show recent activity (for example, `<1m`) to the right of each tree entry.
- [ ] Evaluate whether [tui-term](https://github.com/a-kenji/tui-term) is a better terminal-widget foundation.
- [ ] Evaluate [vt100-rust](https://github.com/doy/vt100-rust) as an alternative terminal parser.
- [x] Complete the project-wide rename from `illium` to `ilium`.
- [ ] Show separate “create folder” and “create folder at root” actions in the bottom toolbar.
- [ ] Keep bottom-toolbar icons visible whenever the left panel is focused, while showing extended actions such as “create with options” only when the pointer approaches.
- [ ] Add “create with options” menus for terminals, groups, agents, and other supported node types to the bottom toolbar.
- [ ] Add a settings section for configuring the bottom toolbar: location, visibility behavior, icons, and icon order.
- [ ] During first-run setup, test whether the terminal renders the toolbar’s rounded-corner glyphs; if not, offer to install a compatible font and configure supported terminals such as VS Code and GNOME Terminal.

## Settings, appearance, and input

- [ ] Add accessible status palettes: color-blind-safe alternatives and a monochrome-with-glyph mode.
- [ ] Add vi-style keyboard shortcuts.
- [x] Let users choose Ctrl+A, Ctrl+B, or another supported shortcut prefix, while explaining conflicts and recommended choices.
- [ ] Document OpenMoji setup if ilium’s emoji-heavy interface requires it.
- [ ] Add a first-run icon preview that offers to install improved icon support, at least on Ubuntu.
- [x] Support themes, including selected themes inspired by VS Code and terminal tools such as Oh My Zsh.
- [x] Build a complete configuration screen with a polished user experience.
- [ ] Add voice dictation as an input method.
- [ ] Add full voice navigation and interface control using current GPT models, including voice-directed workspace reorganization.

## Agent lifecycle, providers, and intelligence
- [x] Detect when an agent is sleeping or waiting for an external event and show a dedicated animation.
- [ ] Define a clean provider abstraction for agent detection, resume behavior, session metadata, and other provider-specific capabilities beyond Claude and Codex.
- [ ] Add provider implementations for Gemini, OpenCode, and other agent types.
- [ ] Let users fork an agent into a new session from its context menu when the provider supports session forking, starting with Claude Code.
- [ ] When an agent launched from a terminal exits, return the pane to its original terminal state.
- [~] Add “Magic” LLM-assisted workspace organization: inspect terminal screenshots, group related panes, and relabel nodes.
- [ ] Optionally run small LLM tasks through a low-instrumentation Claude Code process instead of Kilo Gateway, with no tools or custom system prompt and a structured JSON result written to a temporary file.
- [ ] Detect whether an agent has an active goal or dynamic workflow and show an appropriate flag.
- [ ] Generate two agent titles: a compact title for the narrow left panel and a slightly longer title for its expanded state.
- [ ] Detect provider-specific goal-completion output—such as Claude’s `✔ Goal achieved (...)` message—and show a success flag; determine the equivalent Codex signal.

## Files, search, and boards
- [ ] Add a file-navigation entry to the left panel.
- [ ] Add a file-search entry to the left panel.
- [-] Add a Kanban panel with cards on the left, details on the right, focus-responsive sizing, keyboard navigation, and directional card movement. Its creation dialog should support either one file per column or one folder per column with one file per card.
- [ ] Add a left-panel file explorer with drag-file-to-agent interaction.
- [ ] Search across terminal and agent histories, then open the selected result in the correct pane at the matching position.
- [ ] Add an LLM-assisted tool that converts a TODO file into a Kanban board using project context.
- [x] Save and restore terminal buffers and scrollback history.
- [x] Add a left-panel folder node, distinct from a group, for navigating files and opening them quickly.

## Git and worktree workflows
- [ ] Optionally show the current Git branch in tabs: always, never, or only when it differs from configured defaults such as `main` and `master`.
- [ ] Let users create an agent in a new worktree from both context-menu actions and the agent-creation dialog.
- [ ] Let a folder be associated with a worktree from the folder-creation dialog.
- [ ] Add a Git review panel or tree entry for status, diff review, and LLM-assisted commit creation from the available changes.
- [ ] Optionally show compact green/red diff statistics, such as `+32/-11`, beside the branch on a second line for terminals, sessions, groups, and worktrees.
- [ ] Create pull requests from worktree-associated groups, terminals, or agents.
- [ ] Detect agents that share a branch or worktree, show a conflict-risk warning, and offer actions to resolve or dismiss it.
- [ ] Add a setting that controls whether a worktree-associated group or folder may contain nodes using a different worktree or no worktree.

## Sandboxing, infrastructure, and shared resources
- [ ] Add container-based sandboxing for workers and agents with shared authentication, inspired by aoe.
- [ ] Explore Kubernetes orchestration and whether agents should have any control over it.
- [ ] Expose Docker sandbox choices when creating groups, agents, and terminals, including joining an existing sandbox.
- [ ] Add a machine-control panel or tree entry where visual agents can inspect screenshots and interact with useful on-screen controls.
- [ ] Let groups define exclusive resources, such as a GPU or compiler slot, through shared semaphores. Waiting agents should sleep, coordinate priority through group chat, and escalate decisions to the user through a structured question format.
- [ ] Give worktree-associated groups and folders a distinct icon and context-menu actions such as LLM-assisted commit and push.

## Agent collaboration and reusable workflows
- [ ] Add specification/plan creation and execution workflows.
- [ ] Add a user-visible agent group chat backed by a shared text file.
- [ ] Add group-chat setup that exposes agent session IDs and names through ilium’s APIs, updates project `CLAUDE.md`/`AGENTS.md` instructions, and opens a monitoring pane so agents can coordinate without interfering with one another.
- [ ] Where supported, add project-scoped hooks under `.claude/` that prompt agents to read changed group-chat messages and post important updates or warnings.
- [ ] Add agent roles: start agents with role-specific prompts, let idle agents respond to relevant group-chat mentions, and queue mentions while they are busy.
- [ ] Add reusable task templates with prompt templates and forms; submitting a form should either launch an agent with the generated prompt or copy the prompt to the clipboard.

## APIs, notifications, and external integrations
- [ ] Expose documented HTTP and IPC APIs.
- [-] Add configurable sound notifications.
- [ ] Show attention-worthy ilium status and notifications in the user’s tmux status bar.
- [x] Send desktop/OS notifications.
- [ ] Add Telegram and other chat-channel panels.

## Demos
- [ ] Create a restart-recovery demo framed as a machine crash: show BIOS and boot animations, then have an agent diagnose overheating and jokingly connect it to the earlier question about mayonnaise as CPU thermal paste.
