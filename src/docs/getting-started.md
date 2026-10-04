# Getting started

This guide takes you from a fresh install to everyday use of Ilium: starting your first session, the first-run setup, the `Ctrl+B` prefix and every key binding, remapping keys, using Ilium inside tmux, and using the mouse. Ilium is a terminal multiplexer for AI coding agents. It keeps agent CLIs (Claude Code, Codex, Antigravity), shells, editors and boards in one project tree, with each agent's activity visible at a glance.

Contents:

- [1. Before you start](#1-before-you-start)
- [2. Install](#2-install)
- [3. Start your first session](#3-start-your-first-session)
- [4. First-run setup](#4-first-run-setup)
- [5. Your first ten minutes](#5-your-first-ten-minutes)
- [6. The prefix and how key sequences work](#6-the-prefix-and-how-key-sequences-work)
- [7. Every keybinding](#7-every-keybinding)
- [8. Keys while a pane is focused](#8-keys-while-a-pane-is-focused)
- [9. Keys in the tree and in dialogs](#9-keys-in-the-tree-and-in-dialogs)
- [10. Remapping keys](#10-remapping-keys)
- [11. Using Ilium inside tmux or Screen](#11-using-ilium-inside-tmux-or-screen)
- [12. Using the mouse](#12-using-the-mouse)
- [13. Detaching and coming back](#13-detaching-and-coming-back)
- [14. Troubleshooting](#14-troubleshooting)
- [Where to go next](#where-to-go-next)

## 1. Before you start

You need:

- A UTF-8 terminal with 256-colour support. Linux is the primary platform; macOS and Windows are supported with different test coverage.
- Your agent CLIs installed separately (for example `claude`, `codex`). Ilium does not install them. It detects them by looking at the processes running in each pane.
- No Rust toolchain, unless you build from source.

Terminals, agent detection, and session storage work without any AI service. AI-written titles, tree organisation and the optional Smart Copy suggestions use the provider you choose during first-run setup. See [Inference and privacy](inference-and-privacy.md).

## 2. Install

Release downloads and hosted installers were not published when this documentation was written, so for now [build from source](building-from-source.md). When releases are available, the installer commands are:

Linux and macOS:

```sh
curl -fsSL https://ilium-setup.pages.dev/install.sh | sh
```

Windows PowerShell:

```powershell
irm https://ilium-setup.pages.dev/install.ps1 | iex
```

Package formats (deb, rpm, AppImage, Snap, Flatpak, macOS ZIP/PKG/DMG, Windows setup `.exe`/`.msi`/ZIP), checksum verification, and removal are covered in [Installation](installation.md). To upgrade, re-run the installer or install the newer package.

Verify the install:

```sh
ilium --version
```

The client (`ilium`) starts a separate server process (`ilium-server`) and expects to find it next to itself or on `PATH`. Keep the binaries together. See [How it works](how-it-works.md).

## 3. Start your first session

1. Open a terminal and change into your project directory:

   ```sh
   cd ~/code/my-project
   ```

2. Run:

   ```sh
   ilium
   ```

3. Ilium starts a background server for this project (if one is not already running) and attaches the interface to it. The first time, the guided setup opens (see [section 4](#4-first-run-setup)).

4. You now see two areas: the project tree on the left and the pane area on the right. Press `Ctrl+B` then `c` to create a terminal pane. Run your agent CLI in it, for example `claude`.

A session belongs to the directory you launched Ilium from. That canonical directory is the project boundary: every pane starts there by default, the editor's file picker opens there, and snapshots are stored in `<project>/.ilium/sessions/`. A bare `ilium` always owns that directory's session named `default`. Running `ilium` again in the same directory re-attaches to it; running it in another directory gives you a different, independent session.

You can pass the project explicitly from anywhere:

```sh
ilium --cwd ~/code/my-project
```

For named extra sessions in the same project, see [Panes and layout: sessions](panes-and-layout.md#sessions-per-project) and the [CLI reference](cli-reference.md).

## 4. First-run setup

On a fresh installation (no config file yet), or when a previous setup was started but not finished, Ilium opens a guided setup before you reach the normal interface. Automatic AI requests stay paused until setup finishes.

The setup has seven steps. You can go back at any step.

| Step | Title | What you do |
| --- | --- | --- |
| 1 | AI assistance | Choose how Ilium may use an LLM: Kilo Gateway, paid APIs, a local provider (Ollama), or skip (disable). |
| 2 | Connect AI | Enter the provider details and test the connection. Skipped when you chose to disable AI. A failed test does not trap you in the setup or silently change your choice. |
| 3 | Your sound | Choose the notification sound source: bundled, a system sound, or a custom file. |
| 4 | Sound studio | Configure and preview the sound. |
| 5 | Your controls | Choose a keyboard preset: tmux, GNU Screen, or custom. |
| 6 | Playground | Practise the keys in an isolated practice tree that is not connected to your real session. It teaches new terminal, focus, split, close, group, move, rename, jump and cycle actions and shows which keys you have learned. |
| 7 | Voice control | Optional voice test. |

Notes:

- The AI choice matters for privacy. Kilo Gateway sends prompts to its service. Existing configurations keep the provider they already had. See [Inference and privacy](inference-and-privacy.md).
- Reopen the setup at any time with `ilium --onboarding`, or from **Settings** with the **Guided setup** button. The flag is accepted by every command but only matters when attaching.
- A setup that you started but did not finish resumes the next time you run `ilium`. Completing it is what stops it opening automatically.

## 5. Your first ten minutes

A short walk-through that touches the core features. Every step uses the default keys.

1. **Create a terminal.** `Ctrl+B` `c`. A shell opens in the selected group, in the project root.
2. **Start an agent in it.** Type `claude` (or `codex`) and press Enter. Ilium recognises the agent from its process and starts showing its identity and activity in the tree.
3. **Create a second terminal.** `Ctrl+B` `c` again.
4. **Put both side by side.** `Ctrl+B` `"` opens the split dialog. Pick vertical (side by side) or horizontal (stacked), press Enter, tick the two panes with Space, and press Enter. See [Panes and layout](panes-and-layout.md#split-views).
5. **Move between panes.** `Ctrl+B` `o` (next pane), `Ctrl+B` `;` (previous), or `Ctrl+B` then an arrow key.
6. **Rename an entry.** Select it in the tree and press `Ctrl+B` `,`. Names you type yourself are protected from automatic retitling.
7. **Search.** `Ctrl+B` `f` opens workspace search across terminal history, agents and open files.
8. **Open settings and help.** `Ctrl+B` `:` for Settings, `Ctrl+B` `?` for the live key reference.
9. **Detach.** `Ctrl+B` `d`. Your agents keep running. Run `ilium` again in the same directory to come back.

## 6. The prefix and how key sequences work

Ilium shortcuts are two-key sequences in the style of tmux: press the prefix, release it, then press one action key. The default prefix is `Ctrl+B`, so "new terminal" is `Ctrl+B` then `c`.

Rules that are worth knowing:

- **Two prefixes.** There is a general prefix (the "shortcut base") and a separate tree-navigation prefix. Both default to `Ctrl+B`. The navigation prefix covers only the four tree-traversal actions (cycle next/previous pane in the group, jump to next/previous group). Keeping it separate means you can move the general prefix to `Ctrl+A` and still have `Ctrl+B` `n`/`p`/`(`/`)` for navigation.
- **The prefix is any letter.** Both prefixes can be set to `Ctrl+A` through `Ctrl+Z`. `Ctrl+A` (GNU Screen) and `Ctrl+B` (tmux) are the recommended choices; Settings shows a warning for every other letter explaining which terminal or shell convention it shadows (for example `Ctrl+C` interrupts, `Ctrl+D` sends end-of-file, and `Ctrl+I` is indistinguishable from Tab).
- **Doubling the prefix** sends one literal prefix keystroke to the focused pane. Press `Ctrl+B` `Ctrl+B` and the focused terminal receives a single `Ctrl+B` byte. This is the equivalent of tmux's `send-prefix`, and it keeps a shell's own `Ctrl+B` (backward character), or a nested tmux, reachable. It works for whichever prefix you pressed first.
- **Pasting cancels a pending prefix.** If you press the prefix and then paste, the paste is dropped rather than interpreted as shortcuts.
- **Unknown second keys are ignored.** After a prefix, a key that is not bound to any action simply cancels the pending prefix.
- **Printable keys ignore Shift/Ctrl on the second key**, so `Ctrl+B` `W` means the key that types a capital W. Arrow and Page keys must be pressed without modifiers, so a pane's own `Ctrl+Arrow` or `Alt+Page` shortcuts are never stolen.
- **Modal dialogs and Help** swallow other keys. While Help is open, only `Esc` or the prefix plus the Help key closes it.
- **F8 is global.** It toggles voice control from anywhere, including while a dialog owns text input. See [Voice](voice.md).

## 7. Every keybinding

All shortcuts below use the general prefix (default `Ctrl+B`) unless noted. The "Config name" column is the key used in the `[keybindings]` table of `config.toml` (see [Remapping keys](#10-remapping-keys)). The key reference inside Ilium (`Ctrl+B` `?`) always reflects your live bindings.

### Create

| Keys | Action | Config name |
| --- | --- | --- |
| `Ctrl+B` `c` | New terminal pane in the selected group | `new_terminal` |
| `Ctrl+B` `W` | New agent in a Git worktree (choose provider, branch and location) | `new_agent_worktree` |
| `Ctrl+B` `e` | New editor pane (opens a file picker) | `new_editor` |
| `Ctrl+B` `B` | New board (choose storage format and location) | `new_board` |
| `Ctrl+B` `g` | New group (choose where in a dialog) | `new_group` |
| `Ctrl+B` `"` | New vertical or horizontal split view | `new_split_view` |
| `Ctrl+B` `F` | Open a folder in the sidebar | `new_folder` |
| `Ctrl+B` `!` | Prompt for a command and run it in a new terminal pane in the selected group | `run_command` |

### Organise

| Keys | Action | Config name |
| --- | --- | --- |
| `Ctrl+B` `,` | Rename the selected entry | `rename` |
| `Ctrl+B` `m` | Toggle move mode for the selected entry (see [section 9](#9-keys-in-the-tree-and-in-dialogs)) | `toggle_move` |
| `Ctrl+B` `x` | Close the selected pane or group (asks first for a non-empty group or an unsaved editor) | `close_pane` |

### Navigate

| Keys | Action | Config name |
| --- | --- | --- |
| `Ctrl+B` `w` | Focus the tree panel | `focus_tree` |
| `Ctrl+B` `P` | Focus the active pane | `focus_pane` |
| `Ctrl+B` `o` | Focus the next visible pane | `focus_next_pane` |
| `Ctrl+B` `;` | Focus the previous visible pane | `focus_previous_pane` |
| `Ctrl+B` `Left` | Focus the visible pane to the left | `focus_pane_left` |
| `Ctrl+B` `Right` | Focus the visible pane to the right | `focus_pane_right` |
| `Ctrl+B` `Up` | Focus the visible pane above | `focus_pane_up` |
| `Ctrl+B` `Down` | Focus the visible pane below | `focus_pane_down` |
| navigation prefix `n` | Cycle to the next pane in the current group | `cycle_next_in_group` |
| navigation prefix `p` | Cycle to the previous pane in the current group | `cycle_previous_in_group` |
| navigation prefix `)` | Jump to the first pane in the next group | `jump_next_group` |
| navigation prefix `(` | Jump to the first pane in the previous group | `jump_previous_group` |

The four navigation actions use the tree-navigation prefix, which also defaults to `Ctrl+B`. The arrow keys only work with no modifiers held.

### Terminal history and search

| Keys | Action | Config name |
| --- | --- | --- |
| `Ctrl+B` `[` | Scroll the focused terminal one page up | `scrollback_up` |
| `Ctrl+B` `]` | Scroll the focused terminal one page down | `scrollback_down` |
| `Ctrl+B` `f` | Workspace search over terminal history and open editor buffers (results are labelled agent, shell, file and board) | `search` |

### Editor panes

| Keys | Action | Config name |
| --- | --- | --- |
| `Ctrl+B` `S` | Save the focused editor | `save` |
| `Ctrl+B` `v` | Toggle the focused editor between Source and Rendered (Markdown files only) | `toggle_editor_view_mode` |
| `Ctrl+B` `N` | Toggle line numbers in the focused editor | `toggle_line_numbers` |
| `Ctrl+B` `b` | Toggle the minimap in the focused editor | `toggle_minimap` |
| `Ctrl+B` `a` | Toggle autosave (about one second after each edit) in the focused editor | `toggle_autosave` |

### Settings, help and session control

| Keys | Action | Config name |
| --- | --- | --- |
| `Ctrl+B` `:` | Open Settings (the gear in the tree footer does the same) | `settings` |
| `Ctrl+B` `?` | Show or hide the key reference | `help` |
| `Ctrl+B` `d` | Detach this client and leave the session running | `detach` |
| `Ctrl+B` `&` | Kill this project session and disconnect every client | `quit` |

`Ctrl+B` `&` takes effect immediately and ends every pane in the session. There is no confirmation. Use `Ctrl+B` `d` when you only want to leave.

### Global key

| Key | Action |
| --- | --- |
| `F8` | Toggle voice control. Works in every mode and dialog. |

There are 35 bindable actions in total. Each action has exactly one key, and a key cannot be bound to two actions.

## 8. Keys while a pane is focused

When a terminal pane has focus, almost every key goes to the program running in it. Ilium intercepts only:

| Key | Effect |
| --- | --- |
| the prefix, then an action key | An Ilium shortcut (see above). |
| the prefix twice | One literal prefix keystroke reaches the pane. |
| `Shift+PageUp` / `Shift+PageDown` | Scroll terminal history by one page. |
| `Shift+End` | Jump back to the live screen. |
| `Ctrl+End` | Send Ctrl+End (`ESC [ 1 ; 5 F`) to the application (the key reference lists it as "Ctrl+End app"). |
| `F8` | Toggle voice control. |

Typing any other key that reaches the terminal also returns the view to the live screen, as in an ordinary terminal emulator. Pastes into a terminal pane are delivered as one paste operation.

In an editor pane in Source mode, keys go to the text editor. In Rendered mode (Markdown), `Up`, `Down`, `PageUp` and `PageDown` scroll the rendered document. See [Editors and boards](editors-and-boards.md).

## 9. Keys in the tree and in dialogs

When the tree has focus (`Ctrl+B` `w`, or click it):

| Key | Effect |
| --- | --- |
| `Up` / `k` | Select the previous row. |
| `Down` / `j` | Select the next row. |
| `Left` / `h` | Collapse the row, or move to its parent. |
| `Right` / `l` | Expand the row. |
| `Enter` / `Space` | Open the selected entry: toggle a group, focus a pane, show a split view, open a file from a sidebar folder, or show a project's Chatroom. |

**Move mode** (`Ctrl+B` `m`) reorders and re-parents the selected entry without the mouse:

| Key | Effect |
| --- | --- |
| `Up` / `k` | Move the entry up one position among its siblings. |
| `Down` / `j` | Move it down one position. |
| `Left` / `h` | Outdent: move it out of its current group, placing it right after that group. |
| `Right` / `l` | Indent: move it into the nearest preceding sibling group, at the end. |
| `Enter` / `m` / `Esc` | Leave move mode. |

Nothing is sent when a move is impossible (already first or last, no preceding group to indent into, or outdenting would leave a pane without a group). Moving never restarts a pane's process.

Common dialog keys:

| Context | Keys |
| --- | --- |
| Rename and command prompt | `Enter` confirms, `Esc` cancels. |
| New group | `Up` / `Down` pick the destination, type the name, `Enter` confirms, `Esc` cancels. |
| New split, step 1 | `Left` / `Right` / `Up` / `Down` / `Tab` switch vertical and horizontal, `Enter` continues to the pane picker, `e` creates an empty split, `Esc` cancels. |
| New split, step 2 | `Up` / `Down` (or `k` / `j`) move, `Space` ticks a pane, `Enter` creates the split, `Esc` cancels. |
| Close confirmation | `y` or `Enter` confirms, `n` or `Esc` cancels. |
| Context menu | `Up` / `Down` select, `Enter` runs, `Esc` or `Left` closes a submenu or the menu. |
| Workspace search | Type to search, `Up` / `Down` / `PageUp` / `PageDown` move, `Enter` opens the hit, `Esc` closes. |
| Settings | `Tab` / `Shift+Tab` switch tabs, `Up` / `Down` (or `k` / `j`) move, `Left` / `Right` (or `h` / `l`, `Enter`, `Space`) change a value, `PageUp` / `PageDown` scroll, `Esc` or `q` closes. |
| Help | `Esc`, or the prefix then the Help key, closes. |

## 10. Remapping keys

You can change the prefixes and the second key of every action. There are three ways, all producing the same stored settings.

### In Settings

1. Press `Ctrl+B` `:` and open the **Keyboard** tab.
2. Pick a complete preset (**tmux** or **GNU Screen**), or edit individual rows.
3. The general prefix and the tree-navigation prefix each have a selector from `Ctrl+A` to `Ctrl+Z`, with the two recommended presets first and a warning for any letter that shadows a common terminal key.
4. Select an action to rebind it, then press the new key. Only keys from the bindable set can be chosen: letters, digits, and the punctuation characters on a standard keyboard (including Space), plus the arrow keys and Page Up / Page Down.
5. Changes apply immediately, and both input and the `Ctrl+B` `?` reference use the new table. Settings are saved automatically.

### In config.toml

Global settings live in `~/.config/ilium/config.toml` on Linux (`~/Library/Application Support/ilium` on macOS, `%APPDATA%\ilium` on Windows). `ILIUM_CONFIG_DIR` overrides the directory. The two tables are:

```toml
[keyboard]
shortcut_base = "a"              # general prefix: Ctrl+A
navigation_shortcut_base = "b"   # tree-navigation prefix: Ctrl+B

[keybindings]
new_terminal = "c"
new_split_view = "s"
rename = "r"
focus_pane_left = "left"
scrollback_up = "page_up"
detach = "d"
```

Rules:

- `shortcut_base` and `navigation_shortcut_base` must each be exactly one ASCII letter, case-insensitive. Anything else is an error.
- In `[keybindings]`, each key is an action's config name (see the tables in [section 7](#7-every-keybinding)) and each value is one printable character, or one of `up`, `down`, `left`, `right`, `page_up`, `page_down` (`arrow_up` style and `pageup` spellings are also accepted). Remapping only moves existing actions to other keys; it cannot create new actions.
- You only list what you change. Unlisted actions keep their default key.
- Two actions cannot share a key. The load fails with "binds more than one action to the key".
- An unknown action name or an invalid key value is an error.

If `config.toml` fails to parse or validate, Ilium logs the error and starts with default settings instead of refusing to start. If your keys suddenly look default after an edit, check for one of the errors above.

### The two presets

The **tmux** preset sets the prefix to `Ctrl+B` and uses tmux's own keys where Ilium has an equivalent action: `c` new terminal, `x` close, `,` rename, `"` split, `o` and `;` next and previous pane, `n` / `p` / `(` / `)` cycle and group jump, `[` / `]` scrollback, `d` detach, `&` kill, `:` settings, `!` run command, `f` search, `w` focus tree, `S` save, and the arrow keys for directional focus.

The **GNU Screen** preset sets the prefix to `Ctrl+A` and uses Screen's keys: `c` new, `k` close, `A` rename, `S` split, `n` / `p` next and previous pane, `H` / `J` / `K` / `L` directional focus, `d` detach, `\` kill, `o` settings, `:` run command, `/` search, `t` focus tree, `s` save, and `Down` / `Up` / `PageDown` / `PageUp` for cycle and group jump. Ilium-only actions (editor, board, folder, move, editor toggles) stay on free keys.

Applying a preset replaces the whole binding table, so apply a preset first and then make individual changes.

## 11. Using Ilium inside tmux or Screen

Running Ilium inside tmux or Screen works, but both programs and Ilium want the same prefix by default, and the outer program sees every key first.

Options, from most to least recommended:

1. **Give Ilium a different prefix.** Set Ilium's prefix to `Ctrl+A` (the GNU Screen preset, or `shortcut_base = "a"`). Then `Ctrl+B` stays tmux's and `Ctrl+A` is Ilium's, with no conflict. If you use Screen instead of tmux, do the opposite.
2. **Pass the prefix through tmux.** With the same prefix in both, tmux consumes the first `Ctrl+B`. Press `Ctrl+B` twice to make tmux forward one `Ctrl+B` to Ilium (tmux's `send-prefix`), which Ilium then treats as the start of a shortcut. So `Ctrl+B` `Ctrl+B` `c` creates a terminal in Ilium. This is workable but clumsy.
3. **Ilium's own doubling.** When a literal `Ctrl+B` should reach the program inside an Ilium pane (for example a nested tmux), press Ilium's prefix twice. That sends one prefix keystroke to the focused pane.

For mouse use inside tmux, enable tmux's mouse support so wheel and click events reach Ilium.

## 12. Using the mouse

Ilium is fully usable with the mouse. Terminals report mouse events to Ilium, which then decides whether to act on them or forward them to the program in a pane.

**Tree**

- Click a row to select it and focus a pane. Clicking a group toggles it.
- Double-click a row to rename it.
- Drag a row onto another row to move it into that group, or onto the empty space below the last row to put it at the top level. Dropping onto the dragged entry's own descendants, and a release outside the tree, cancel the move.
- Scroll the wheel over the tree to scroll it.
- Right-click a row, or the empty space, for the context menu: search, focus, new terminal / agent / editor / group / split / folder, rename, move up and down, order by, bookmark, lock closed, worktree actions, ask for update, queue a prompt, schedule input, convert an agent session between Claude and Codex, close, and **Settings**. Which entries appear depends on what you clicked.
- Hover a row to show its small buttons (rename, move up and down, close, retitle). Whether hover buttons appear is a setting.
- The tree footer has toolbar buttons including Search, Restructure, and Settings (gear).

**Terminal panes**

- Click a pane to focus it.
- Drag to select text. By default a left-button drag over a terminal selects locally and copies to the clipboard (setting **Terminal text selection** in the Appearance tab, `terminal_text_selection_enabled` in `[ui]`). Turn it off when you want a mouse-aware program such as a full-screen editor to receive the drag instead.
- Scroll the wheel over a pane to move through terminal history. Scrolling and selection stay local even when a program has turned on its own mouse tracking after an agent stops.
- Right-click a terminal for: copy selection, copy last submitted prompt (agents), copy line, copy visible terminal, copy full terminal history, copy the path to the agent's history file, paste, paste the screen into a neighbouring pane, show or hide the agent toolbar, show the agent debug log (when enabled), and open a URL or file under the pointer.
- **Smart Copy light:** hold Ctrl (configurable to Alt or Shift, or off, under **Settings → Terminal**) over a terminal pane, click regions to select them, and release the key to copy. See [Smart Copy](smart-copy.md).

**Dialogs**

Dialog buttons, list rows, and settings controls respond to clicks. In the Animations settings, a slider keeps its drag until you release the button, even if the pointer leaves it.

## 13. Detaching and coming back

- `Ctrl+B` `d` detaches only your client. Panes, agents and their processes keep running in the background server.
- Run `ilium` (or `ilium --cwd <project>`) again to re-attach to the same session. Several clients can attach to one session at once.
- `Ctrl+B` `&` ends the session and every pane in it.
- If the machine restarts, Ilium restores the layout and relaunches pane programs, resuming verified Claude Code, Codex or Antigravity sessions where it can. See [Session recovery](session-recovery.md).

`ilium ls` lists this project's sessions and whether each is running. `ilium kill-session <name>` ends one from outside. See the [CLI reference](cli-reference.md).

## 14. Troubleshooting

| Symptom | Likely cause and fix |
| --- | --- |
| `Ctrl+B` does nothing or the shell moves its cursor | The shortcut base was changed, or the key reached an outer tmux first. Check **Settings → Keyboard** and [section 11](#11-using-ilium-inside-tmux-or-screen). |
| Every Tab or Enter opens a pending prefix | The prefix is set to `Ctrl+I` or `Ctrl+M`, which terminals cannot tell apart from Tab and Enter. Choose another letter. |
| Custom keybindings ignored | `config.toml` has a parse error, an unknown action name, an invalid key, or a duplicate key. Ilium falls back to defaults and logs the reason. |
| The mouse does nothing inside tmux | Enable tmux's `mouse` option so mouse events are forwarded. |
| Terminal text will not select | **Terminal text selection** is off, or a program in the pane captured the mouse. Toggle the setting, or hold your outer terminal's selection override key. |
| Setup opens every time | The guided setup was started but not completed. Finish it once. |
| The `ilium` command attaches but panes never start | The server could not be started, for example because `ilium-server` is not beside `ilium`. See [Installation](installation.md) and [How it works](how-it-works.md). |
| `ilium: session ... is not running` | You ran a command that needs a live session, such as `kill-session` or `voice say`. Start Ilium in that project first. |

## Where to go next

- [Panes and layout](panes-and-layout.md): groups, splits, boards, search, history, sessions.
- [CLI reference](cli-reference.md): every subcommand and flag.
- [Settings](settings.md): every tab in the settings screen.
- [Agent monitoring](agent-monitoring.md), [Agent cost](agent-cost.md), [Notifications](notifications.md).
- [Worktrees](worktrees.md) and [Session recovery](session-recovery.md).
