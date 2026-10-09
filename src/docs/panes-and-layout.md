# Panes and layout

Everything in an Ilium session lives in one tree: projects contain groups, groups contain panes, and some groups are split views that show several panes at once. This page explains the tree, the three kinds of pane (terminal, editor, board), how to group, split, move, rename and search, how scrolling and history work, and how sessions and detaching behave. For the key sequences themselves see [Getting started](getting-started.md#7-every-keybinding).

Contents:

- [The two areas of the screen](#the-two-areas-of-the-screen)
- [The tree](#the-tree)
- [Creating panes](#creating-panes)
- [Terminal panes](#terminal-panes)
- [Editor panes](#editor-panes)
- [Board panes](#board-panes)
- [Groups](#groups)
- [Split views](#split-views)
- [Moving, ordering and renaming](#moving-ordering-and-renaming)
- [Closing](#closing)
- [Focus and navigation](#focus-and-navigation)
- [Scrolling and history](#scrolling-and-history)
- [Workspace search](#workspace-search)
- [Detach and reattach](#detach-and-reattach)
- [Sessions per project](#sessions-per-project)
- [Troubleshooting](#troubleshooting)

## The two areas of the screen

The screen has a **left panel** (the project tree) and a **right panel** (the pane area).

- The right panel shows the focused pane, or all the panes of a split view.
- The left panel keeps the tree visible at all times, with one row per project, group, split view and pane, plus status indicators for agents.

You can size the left panel in **Settings → Appearance**. The default policy is **Width-dependent**: the panel is 44 columns wide (its focused width) when the terminal is at least 120 columns wide or when the tree itself has focus, and 24 columns wide otherwise. The other policies are **Fixed** (default 32 columns, whatever has focus) and **Focus-dependent** (44 columns with focus in the tree, 24 otherwise). Widths are limited to 16 through 80 columns, and the width threshold can be set between 40 and 500 columns.

## The tree

The tree is the authoritative description of your workspace and is stored on the server, so every attached client sees the same tree.

| Entry | What it is |
| --- | --- |
| Project | A top-level entry that owns a working directory. Entries created beneath it inherit that directory. The launch directory of the session is the first project. |
| Group | A container for panes and other groups. Use groups to organise work, for example one group per feature or per agent role. |
| Folder | A directory shown live in the sidebar (`Ctrl+B` `F`). Clicking a file in it opens it in an editor pane. |
| Split view | A special group that displays two to four of its panes together in the right panel. |
| Terminal pane | A shell, or any program, running in a pseudo-terminal. Agents are terminal panes in which Ilium has detected an agent CLI. |
| Editor pane | A text editor on one file, with Markdown preview and autosave. |
| Board pane | A Kanban board stored in a Markdown file or folder. |
| Chatroom | An optional row for a project's `CHATROOM.md` (see [Automation](automation.md)). |

Row indicators show agent identity, activity (working, idle, waiting for approval, done), goals, monitored background tasks, bookmarks and locks. See [Agent monitoring](agent-monitoring.md) for what they mean.

### Tree order

By default children appear in the order you arranged them (**Manual**). You can display each parent's children by type, by age (ascending or descending) or by name (ascending or descending) from the right-click **Order by** submenu or in Settings. Changing the display order does not change the stored manual order. An additional cost order (most expensive first) is driven by the Agent Cost settings; see [Agent cost](agent-cost.md).

### Bookmarks, locks and automatic clean-up

- Right-click a row to **bookmark** it.
- Right-click a project, group or folder to **lock it closed**. A locked row ignores ordinary clicks that would expand it, but a double-click still renames it, and **Unlock** stays available even when the lock option is switched off in Settings.
- **Auto-remove empty groups** (Settings, on by default) closes a group or folder automatically when its last item is closed, repeating up through ancestors that become empty. Projects and split views are never removed automatically.
- **Project separators** (Settings) draw a line after each project's complete subtree.

## Creating panes

All creation commands place the new entry in the selected group (or the group of the selected entry).

1. **Terminal:** `Ctrl+B` `c`. The shell starts in the directory chosen by **Settings → Terminal → New-pane directory**:
   - **Project root** (default): the project's directory.
   - **Focused terminal:** the live working directory of the focused terminal pane.
   - **Last used:** the directory of the most recently used terminal.
2. **Run a command in a new terminal:** `Ctrl+B` `!`, type the command line, press Enter. Esc cancels.
3. **Agent:** from the right-click menu **New agent** submenu, or start one in a terminal by running its CLI. For an agent in its own Git worktree use `Ctrl+B` `W` (see [Worktrees](worktrees.md)).
4. **Editor:** `Ctrl+B` `e`, then pick a file.
5. **Board:** `Ctrl+B` `B`, then choose the storage format and location.
6. **Group:** `Ctrl+B` `g`, choose the destination in the list, type a name, press Enter.
7. **Folder:** `Ctrl+B` `F`, choose a directory.
8. **From the CLI:** `ilium new-pane -- <command>` adds a terminal pane without attaching. See the [CLI reference](cli-reference.md#ilium-new-pane).

The right-click menu on the empty tree space, a group or a project offers the same entries (New terminal here, New editor here, New group, New split view, Open folder).

## Terminal panes

A terminal pane runs a real process in a pseudo-terminal owned by the server, not by your client. That is why panes keep running when you detach.

- **Input:** everything you type goes to the process, except Ilium shortcuts, which start with the prefix. Press the prefix twice to send a literal prefix key.
- **Agent detection:** Ilium identifies agent CLIs from the process tree and reads their activity from the screen. Claude Code, Codex and Antigravity are built in; you can add custom signatures in the configuration. An agent pane gains an optional toolbar, a last-prompt banner and a progress footer.
- **Scrollback budget:** each pane keeps terminal output up to a memory budget. The default is 8 MiB per pane; **Settings → Terminal** (or `scrollback_budget_mib` in `[terminal]`) accepts 4 to 512 MiB in steps of 4. The budget applies to terminal views created afterwards; it does not make old, already discarded output reappear.
- **Parser pool:** Off (default, `engine_memory_budget_mib = 0`) imposes no aggregate parser-memory cap. Initialized panes and snapshots stay available for faster revisits without waiting for pool capacity, so RAM use grows with pane count and can become substantial. Turn the pool on with a budget from 256 to 16384 MiB in steps of 256; it can reclaim eligible hidden parsers to reduce retained memory, but revisiting an evicted pane requires replay and held snapshots can delay updates. Displayed panes stay available. Per-pane geometry and operation limits, bounded pins, captures, and command queues still apply; this setting is not a total process-memory cap. **Settings → Terminal** applies changes live; the same value can be set in `[terminal]`.
- **Text selection:** drag with the left mouse button to select and copy, by default. See the mouse section of [Getting started](getting-started.md#12-using-the-mouse).
- **Right-click:** copy options, paste, send the screen to a neighbouring pane, open a URL or file under the pointer.
- **When an agent ends:** a pane whose Claude or Codex process exits or loses terminal ownership keeps its agent identity and shows an unavailable indicator, and you can still copy its prompt and history. See [Agent monitoring](agent-monitoring.md).

## Editor panes

An editor pane opens one file, rooted at the project directory.

1. Press `Ctrl+B` `e`. A file picker opens. `Tab` switches between browsing and editing the path directly; `Esc` cancels.
2. Edit in Source view. Long lines clip or wrap, line numbers and the minimap can be toggled, and autosave runs after a short delay (default on; the toggle key is `Ctrl+B` `a`).
3. Markdown files can switch between Source and Rendered view with `Ctrl+B` `v`. Rendered view is read-only and scrolls with `Up`, `Down`, `PageUp` and `PageDown`.
4. Save with `Ctrl+B` `S`. To save under another name Ilium prompts for a path (`Enter` writes, `Esc` cancels).
5. Closing an editor with unsaved changes asks for confirmation.

Details, defaults and the editor settings are in [Editors and boards](editors-and-boards.md).

## Board panes

A board is a Kanban board. Create one with `Ctrl+B` `B`, choose how it is stored (a Markdown file or a folder of cards) and where. Cards live in files you own, so they survive the board pane being closed and can be versioned with Git. Board layout settings (card preview lines, minimum column width) are in **Settings → Kanban board**. See [Editors and boards](editors-and-boards.md). The right-click menu of a Markdown editor pane also offers **Create board from Markdown**.

## Groups

Groups organise the tree.

1. Press `Ctrl+B` `g`.
2. In the dialog use `Up` and `Down` to choose where the group goes. The list starts on the selected group, or the group of the selected pane, or the top level.
3. Type the name and press Enter.

Select a group and press `Enter` or `Space` (or click) to expand or collapse it. `Ctrl+B` `)` jumps to the first pane in the next group and `Ctrl+B` `(` to the previous one (with the navigation prefix). `Ctrl+B` `n` and `p` cycle through panes inside the current group.

## Split views

A split view shows two, three or four panes of a group at the same time, so an agent, a terminal and an editor can sit side by side. A split view can hold at most **four** panes.

### Layouts

| Panes | Vertical split | Horizontal split |
| --- | --- | --- |
| 1 | The pane fills the area. | The pane fills the area. |
| 2 or 3 | Equal columns, side by side. | Equal rows, stacked. |
| 4 | A 2 by 2 grid. | A 2 by 2 grid. |

"Vertical" means the dividing lines are vertical, so panes sit side by side. "Horizontal" means panes are stacked. With four panes the layout is always a 2 by 2 grid. Diagonal panes are not neighbours for directional focus, but `Ctrl+B` `o` and `;` still visit all of them.

### Create a split view

1. Press `Ctrl+B` `"` (or right-click and choose **New split view**).
2. In the first dialog choose the orientation: `Left`, `Right`, `Up`, `Down` or `Tab` toggles between vertical and horizontal.
3. Press `Enter` to continue and choose the members, or press `e` to create an empty split and fill it later.
4. In the member list, `Up` and `Down` (or `k` and `j`) move, `Space` ticks or unticks a pane. Panes that are already in another split view are not offered. Ticking a fifth pane shows "A split view can contain at most four panes".
5. Press `Enter` to create the split view.

The split view appears as a row in the tree with its members as children. Select it and press `Enter`, click it, or choose **Show split view** to display it.

### Working in a split view

- Click any pane to focus it, or move with `Ctrl+B` and an arrow key (directional), `o` (next) or `;` (previous).
- Each pane has its own toolbar, last-prompt banner and progress footer slots, reserved up front so the pane's terminal size does not change when those texts update.
- On a separator between two terminals a small control lets you transfer the visible screen of one terminal into its neighbour, which is useful for handing context from one agent to another. The same action is in the terminal's right-click menu as **Paste screen into** (left, right, above, below, depending on the neighbour).
- A four-pane grid exposes eight directional transfers: left and right for each row, up and down for each column.

## Moving, ordering and renaming

### Move with the keyboard

1. Select the entry in the tree (`Ctrl+B` `w`, then arrows).
2. Press `Ctrl+B` `m` to enter move mode.
3. `Up` or `k` moves it up among its siblings; `Down` or `j` moves it down.
4. `Left` or `h` outdents it out of its group, placing it right after that group. `Right` or `l` indents it into the nearest preceding sibling group.
5. Press `Enter`, `m` or `Esc` to finish.

Moving never stops or restarts a pane's process.

### Move with the mouse

- Drag a row and drop it on another row to move it into that group, or on empty space below the last row to place it at the top level.
- Use the hover buttons (move up, move down) or the right-click **Move up** and **Move down**.

### Rename

1. Select the entry and press `Ctrl+B` `,`, or double-click the row, or use the pencil button, or right-click and choose **Rename**.
2. Type the new name and press `Enter` (`Esc` cancels).

A name you enter yourself is protected: automatic titles never overwrite it, nor its short title and icon. Asking AI to retitle an agent pane produces an automatic name and follows the same checks as other automatic titles. See [Titles and instructions](titles-and-instructions.md).

## Closing

- Select an entry and press `Ctrl+B` `x`, use the row's close button, or right-click and choose **Close**.
- A terminal pane closes immediately. A group that still has children, or an editor with unsaved changes, asks first: `y` or `Enter` confirms, `n` or `Esc` cancels.
- The root of the tree cannot be closed.
- Closing a pane that Ilium created in a Git worktree can offer to remove the worktree when it is safe; see [Worktrees](worktrees.md).

## Focus and navigation

There are two focus targets: the **tree** and a **pane**.

| Goal | Keys |
| --- | --- |
| Focus the tree | `Ctrl+B` `w` |
| Focus the active pane | `Ctrl+B` `P` |
| Next or previous visible pane | `Ctrl+B` `o`, `Ctrl+B` `;` |
| Pane left, right, above, below | `Ctrl+B` then an arrow key (no modifiers) |
| Next or previous pane in the current group | navigation prefix then `n` or `p` |
| First pane of the next or previous group | navigation prefix then `)` or `(` |

When the tree has focus, `Up`/`Down`/`Left`/`Right` (or `k`/`j`/`h`/`l`) walk the rows and `Enter` or `Space` opens the selected one.

## Scrolling and history

Terminal panes keep a history that you can scroll without disturbing the running program.

| Action | How |
| --- | --- |
| One page up or down | `Ctrl+B` `[` and `Ctrl+B` `]`, or `Shift+PageUp` and `Shift+PageDown` |
| Back to the live screen | `Shift+End`, or just type something |
| Scroll with the wheel | Mouse wheel over the pane |
| Send Ctrl+End to the program | `Ctrl+End` |

Typing a key that reaches the terminal returns the view to the live tail, as in an ordinary terminal emulator. The server retains the pane's output, so a client that attaches later can replay and scroll it, and workspace search can search it. Right-click **Copy full terminal history** copies everything retained for that pane. History size is bounded by the scrollback budget (see [Terminal panes](#terminal-panes)).

Rendered Markdown editors scroll with `Up`, `Down`, `PageUp`, `PageDown` and the wheel.

## Workspace search

Press `Ctrl+B` `f` (or use the **Search** button in the tree footer, or right-click and choose **Search workspace**) to open full-screen search.

1. Start typing. Results are found after you pause typing for about one second, so typing stays fluid even over large histories. Stale scans are discarded when you keep typing.
2. Results are labelled by kind: **AGENT**, **SHELL**, **FILE** and **BOARD**, each with surrounding text and the match highlighted. They come from the retained terminal history of every pane and from open editor buffers.
3. Move with `Up`, `Down`, `PageUp` and `PageDown`.
4. Press `Enter` to jump to the hit: Ilium focuses the pane and scrolls a terminal to the matching output, or puts an editor at the matching position.
5. Press `Esc` to close search without jumping.

Search is local to your client and works on the content it holds; nothing is sent to an AI provider.

## Detach and reattach

The session lives in a background server, one per project session. Your interface is only a client of it.

1. Press `Ctrl+B` `d` to detach. The interface exits; panes, agents and their running processes continue.
2. Close the terminal window if you like. Agents keep working.
3. Run `ilium` in the project directory (or `ilium --cwd <project>`) to reattach. The tree, panes and their histories come back.
4. Several clients can be attached at the same time. A change in one appears in the others.
5. Press `Ctrl+B` `&` only when you want to end the session and stop every pane.

If the server itself is gone (for example after a reboot), the next `ilium` starts a fresh server and restores the layout from the project snapshot. Programs are relaunched, and verified Claude Code, Codex and Antigravity sessions can be resumed. Unsaved process state does not survive. See [Session recovery](session-recovery.md).

## Sessions per project

A session is identified by the canonical project directory plus a name.

- A bare `ilium` uses the session named `default` for the current directory. Two terminals in the same directory share one session; a terminal in another directory has its own.
- `ilium new-session <name>` creates or attaches to another session in the same project, for example `ilium new-session review`. Names may contain letters, digits, hyphens and underscores and must be at most 48 characters.
- `ilium ls` lists this project's sessions and whether each is running. A stopped session stays listed because its snapshot remains.
- `ilium kill-session <name>` ends a running session and its panes.
- Snapshots are written to `<project>/.ilium/sessions/<name>.json`, and rolling backups to `<project>/.ilium/backups/`. Add `.ilium/` to `.gitignore`. Backups exclude files you edit inside pane applications.
- The session's local socket lives in a private runtime directory (`$XDG_RUNTIME_DIR/ilium/` on Linux, or a short per-user directory under `/tmp` when that is not set). Its name combines a readable slug with a digest of the project path, so different projects never collide.
- Each project can also keep a few per-project settings (such as its name, icon and project separators) in `.ilium/config.yaml`. The background animation is not one of them: it is a single global setting.

On startup the server decides what to do with an existing snapshot according to **Settings → Session → Recovery policy**: restore automatically (default), ask before restoring, or start fresh. Automatic backups are on by default.

## Troubleshooting

| Symptom | Cause and fix |
| --- | --- |
| New terminals start in the wrong directory | Check **Settings → Terminal → New-pane directory**. The default is the project root. |
| Cannot add a fifth pane to a split view | A split view holds at most four. Make a second split view or a group. |
| A pane I want to add to a split is missing from the picker | It already belongs to another split view. Move it out first. |
| Search finds nothing in an old pane | Output beyond the scrollback budget is no longer retained. Raise the budget for new panes. |
| A group disappeared | **Auto-remove empty groups** is on (the default) and its last item was closed. Turn it off in Settings to keep empty groups. |
| Clicking a folder row does not expand it | The row is locked closed. Use **Unlock** from its context menu. |
| Reattaching shows an empty tree | The recovery policy is **Start fresh**, or the session was reset with `--reset-session`. |
| `Ctrl+B` `&` ended everything | That key kills the session. Use `Ctrl+B` `d` to leave without stopping anything. |
