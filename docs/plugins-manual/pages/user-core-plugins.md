---
layout: page
title: Core plugins for users
permalink: /pages/user-core-plugins/
---

## Core plugins for users

Everything Ilium does is done by a plugin. The 30 core plugins ship with Ilium, need no install and are maintained by the Ilium project. This page tells you what each one does in plain words, what you lose when you turn it off, which ones other plugins may replace, and what happens to your data when you do. If you write plugins, read [Core plugins](/pages/core-plugins/) instead.

Contract version: `ilium:plugin@0.1.0`. Requirement ids have the form `[USR-nnn]`, collected in [User and administrator requirements](/pages/reference-requirements-user-admin/).

<div class="note">
<p>The set is fixed: the plugins marked "no" in the column "Can you turn it off" below cannot be turned off with your switch. Two things still turn one off: safe mode in `all-optional` mode keeps only Tree, Layout, Settings and Sessions running, and a confirmed third-party plugin that declares `replaces` turns the built-in one off (see Replacing one). A core plugin never needs an install, and the kernel recovery page can never be turned off or replaced.</p>
</div>

### Contents

- What a core plugin is for you
- The 30 core plugins
- Turning one off
- Replacing one
- What a replacement does to your data
- Updates
- Errors and edge cases
- Requirements

### What a core plugin is for you

You do not have to know about core plugins to use Ilium. They are the parts of the program, split so that each part has one job and a documented edge. The split gives you three things.

- You can turn a feature off if you never use it. The feature disappears from menus, keys and settings.
- You can see and change a feature's permissions like any other plugin's. A core plugin is fully trusted (T1) but it uses the same permission rules for high-risk abilities. See [Permissions explained](/pages/user-permissions-explained/).
- Another plugin can add to a core plugin, for example a step that runs after Smart Copy, or can replace it. See [The Extensions tab](/pages/user-extensions-tab/).

Core plugins have ids of the form `ilium.core.<name>`. They appear in **Settings > Plugins** with the tier T1 and the badge `core`. They do not appear in a package list, because they have no package.

- [USR-040] The Plugins tab and `ilium plugin list` MUST list all 30 core plugins with the tier T1, and MUST NOT offer to install or uninstall one. Test: list plugins on a fresh profile, count 30 rows with ids `ilium.core.*`, and try `ilium plugin uninstall ilium.core.tree`, which must fail with `plugin.not-removable`.

### The 30 core plugins

The column "Can you turn it off" lists the fixed set of plugins that your own switch cannot turn off (safe mode and a confirmed replacement are the only other ways). "Replaceable" says whether a third-party plugin may take over the job with `replaces`.

#### Tree and panes

| Plugin | What it does for you | If you turn it off | Can you turn it off | Replaceable |
|---|---|---|---|---|
| Tree (`ilium.core.tree`) | The project tree: groups, folders, moving, renaming, bookmarks, locks | The tree stops working. Ilium has nothing to show | no | yes |
| Layout (`ilium.core.layout`) | Split views, focus, moving between panes, resizing with the mouse | Panes can no longer be arranged | no | yes |
| Terminal (`ilium.core.terminal`) | Terminal panes: processes, scrollback, input, links | No terminal panes | yes | yes |
| Smart Copy (`ilium.core.smart-copy`) | Selecting text, the right-click menu and Smart Copy | Plain selection only | yes | yes |
| Editor (`ilium.core.editor`) | Editor panes, highlighting, Markdown view, file explorer | Editor panes show a placeholder | yes | yes |
| Kanban (`ilium.core.kanban`) | Board panes backed by Markdown files | Board panes show a placeholder | yes | yes |

#### Agents

| Plugin | What it does for you | If you turn it off | Can you turn it off | Replaceable |
|---|---|---|---|---|
| Agent detection (`ilium.core.agent-detect`) | Knows which coding agent runs in a pane and whether it works, waits or is idle | No agent badges or states. Features that use them go quiet | yes | yes |
| Agent toolbar (`ilium.core.agent-toolbar`) | The last-prompt banner and per-agent buttons | The banner and buttons disappear | yes | yes |
| Fleet (`ilium.core.fleet`) | Send a message to many agents, the local HTTP API | `ilium` pane messaging and the HTTP API stop | yes | yes |
| Cost (`ilium.core.cost`) | Spend, quotas and session statistics | Spend indicators disappear | yes | yes |
| Titles and organisation (`ilium.core.titles`) | Automatic titles and AI tidying of the tree | Panes keep their names | yes | yes |

#### AI and alerts

| Plugin | What it does for you | If you turn it off | Can you turn it off | Replaceable |
|---|---|---|---|---|
| Inference providers (`ilium.core.inference`) | Connects Ilium to AI providers and models | Every feature that uses AI stops. Plugins that ask for `inference` get `not-permitted` with reason `unavailable` | yes | yes |
| Notifications (`ilium.core.notifications`) | Sounds, desktop notifications, toasts, attention marks | No alerts | yes | yes |
| Voice (`ilium.core.voice`) | Voice control and speech output | No voice features | yes | yes |
| Scheduler (`ilium.core.scheduler`) | Scheduled input and the prompt queue | Scheduled items do not run | yes | yes |
| Text triggers (`ilium.core.text-triggers`) | Rules that watch terminal output and send text | The rules do not fire | yes | yes |

#### Work and sessions

| Plugin | What it does for you | If you turn it off | Can you turn it off | Replaceable |
|---|---|---|---|---|
| Progress (`ilium.core.progress`) | Progress monitors and `ilium wait` | Monitors stop reporting | yes | yes |
| Chatroom (`ilium.core.chatroom`) | The shared chat file between agents and `ilium chat` | `ilium chat` fails | yes | yes |
| Worktrees (`ilium.core.worktrees`) | Agents in Git worktrees and branch status | Worktree features disappear | yes | yes |
| Sessions (`ilium.core.sessions`) | Snapshots, restore, detach and reattach | Ilium cannot save or restore a session | no | no |
| Compaction (`ilium.core.compaction`) | Compacting agent context | The compaction features stop | yes | yes |

#### Settings and look

| Plugin | What it does for you | If you turn it off | Can you turn it off | Replaceable |
|---|---|---|---|---|
| Settings (`ilium.core.settings`) | The Settings screen, auto-save, guided setup | The Settings screen goes away and the recovery page is the only way to change plugins | no | no |
| Keys and help (`ilium.core.keys-and-help`) | The prefix key, key bindings, the key reference, help topics | Ilium keeps only the fixed kernel keys | yes | yes |
| Workspace search (`ilium.core.search`) | One panel to find agents, terminals and files | No search panel | yes | yes |
| Ambient look (`ilium.core.ambient`) | Colour, palette and dither for backgrounds | Backgrounds lose the shared look | yes | yes |
| Scenes (`ilium.core.scenes`) | The animated background scenes | No animated backgrounds | yes | yes |
| Recap (`ilium.core.recap`) | `ilium recap`, short and long summaries of panes | `ilium recap` fails | yes | yes |
| Session conversion (`ilium.core.session-convert`) | Converts a transcript between agent formats | The converter command fails | yes | yes |
| Agent setup (`ilium.core.agent-setup`) | Installs the Chatroom and Progress instruction blocks into agent config files | No setup tab | yes | yes |
| Diagnostics (`ilium.core.diagnostics`) | File logging, log levels, the diagnostics report | No file logging | yes | yes |

A plugin you replace or turn off keeps its settings and data on disk. See [Plugin files on disk](/pages/reference-plugin-files/).

The two plugins that cannot be replaced, Settings and Sessions, hold data and roles that a replacement cannot take over safely. The kernel recovery page, which is not a plugin, can never be turned off or replaced either.

- [USR-041] The Plugins tab MUST refuse to turn off a core plugin marked "no" in the table, and MUST say why. Test: press Space on `ilium.core.tree` and assert the refusal text.
- [USR-042] A request to replace `ilium.core.settings` or `ilium.core.sessions` MUST fail at install with `manifest.replaces-not-replaceable`. Test: install a package with such a `replaces` entry.

### Turning one off

Open **Settings > Plugins**, select the core plugin and press `Space`. Or run `ilium plugin disable ilium.core.cost`. The feature disappears at once, with its menu items, keys and settings pages. Other plugins that need the feature show "Waiting for <owner>" and wake up when you turn it back on. Your settings and data stay.

Turning off a feature that other features use can have effects you did not expect. Example: with Agent detection off, features that react to an agent finishing, such as notifications about agents, get no events. The plugin page in Settings shows a list "Used by" before you confirm.

### Replacing one

A third-party plugin may declare `replaces` for a core plugin. That means: while this plugin is on, the built-in one is turned off. Ilium asks you to confirm every replacement. One plugin may replace at most 4 core plugins.

<figure>
<pre class="term"><span class="t-hi">╭─ Replace a built-in plugin? ─────────────────╮</span>
│ Notes Titles wants to replace:               │
│                                              │
│   Titles and organisation                    │
│                                              │
│ While it is on, the built-in plugin is       │
│ off. Turning it off or removing it turns     │
│ the built-in plugin back on. The built-in    │
│ plugin's data stays and is not read.         │
│                                              │
│   [ Replace ]   [ Install without ]          │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The replace confirmation.</figcaption>
</figure>

- Choose **Replace** to confirm. The replacement then owns the job.
- Choose **Install without** to install the plugin and keep the built-in one. The replacement is not active.
- To undo a replacement later, turn the replacement off in the Plugins tab or uninstall it.

- [USR-043] Ilium MUST ask the user to confirm every `replaces` entry at install, MUST allow at most 4 entries, and MUST NOT replace without a yes. Test: install a package with 5 entries and assert the install is refused; with 1 entry assert the prompt and the choice **Install without**.

### What a replacement does to your data

The built-in plugin's data stays on disk, unread and unchanged. The replacement does not read it. It starts with its own data folder. If you turn the replacement off or remove it, the built-in plugin comes back and finds its data as it was.

If the replacement imports the old data on its own initiative, it needs a permission to do so, for example reading a path, and you see that in the review dialog.

### Updates

Core plugins are updated together with Ilium. You never update one on its own. When you update Ilium, every core plugin changes to the version inside it. Third-party plugins are unaffected. A third-party plugin built for an older interface version may be refused after an Ilium update with the message "built for an interface version this Ilium does not support" (code `package.api-unsupported`). See [Install and manage plugins](/pages/user-install-and-manage/).

### Accessibility of core plugin screens

This section gives, for each screen a core plugin draws, the keyboard path, the text form of what is shown, and the focus order. The general rules for host-drawn widgets are in [Accessibility and theming](/pages/accessibility-and-theming/) and [USR-300] to [USR-307] in the requirements list. The host does not read your operating system's accessibility settings. Set the options in `[ui]` yourself: `screen_reader = true` for the linear reader mode, `contrast = "high"` for high contrast, and the **Reduce motion** row on the User Interface tab (`motion_level`) for less motion.

Baseline for every core plugin screen:

- Tab and Shift+Tab follow the focus order in [Focus order](/pages/ui-model/#focus-order). Tab never traps focus, and Esc always leaves a modal dialog.
- The focused widget differs from the others by an attribute besides colour: reverse, bold or a `>` marker. Exactly one widget per window has focus.
- With `screen_reader = true`, each focusable element is one line in focus order, with its role word and state (button, checked, unchecked, selected, N of M). Box-drawing characters and spinner frames are not read.
- Sounds and desktop notices are not a screen-reader channel. Each notice carries its status word in its title, for example "Failed" or "Needs approval" ([Notifications](/pages/core-plugin-notifications/)).

| Screen | Keyboard path | Text form of the content | Focus order |
|---|---|---|---|
| Voice strip (F8 or voice button) | Hold `F8`, release to send, `Esc` cancels | The state line "Listening", the partial transcript line and the hint line are text. The level meter is glyphs only | Not stated: the manual gives the strip no focus stop |
| Voice confirmation "Close pane?" | `Esc` Cancel, `Enter` Close pane | The line "You said: ..." and the line stating what closes | Modal: the two buttons, `Esc` leaves |
| Voice typed input | `ilium voice say` from the command line, no screen needed | JSONL output, one object per line | Not applicable |
| Location picker (Braille world map) | `Tab` switches Address and Coordinates. Arrows move the crosshair one Braille dot, Shift+Arrows ten dots. In Address, type a place: the search starts after 300 ms and lists up to 5 results. Up and Down choose a result, `Enter` uses it, `Escape` cancels | The Selected row names the place and shows the point. The map itself is a picture with no text form | Not stated in the manual; Tab switches Address and Coordinates |
| Kanban board | Not documented: the manual names no key for moving a card (open item below) | Column names and card titles are text. A board backed by a Markdown file can be read as text in an [Editor](/pages/core-plugin-editor/) pane | Columns and cards in reading order, left to right and top to bottom |
| Tree decorations (badges, subtitles, footers) | Decorations are not focusable. Use the tree row that carries them, which is a focusable tree list | A decoration's text is part of its tree row. The manual does not say how a screen reader announces it, and an icon without a label has no text form | Follows the tree row order |
| Animated backgrounds (Ambient look, Scenes) | Settings rows are keyboard-operated like any settings row | The backgrounds carry no text. The settings rows and the preview row are text | Settings rows in their tab order |

Notes on the rows above:

- Under **Reduce motion**, a plugin that animates draws one static frame, and the host stops decorative animations. The exact behaviour per surface is in [Reduce motion and photosensitivity](/pages/core-plugin-ambient/#reduce-motion-and-photosensitivity). The manual states no flash-rate limit for scenes, so Reduce motion is not a tested guarantee for users with photosensitive epilepsy.
- A stale tree decoration (its plugin stopped responding) is shown in the muted colour only, as [Tree decorations](/pages/capability-tree-decorate/) describes. A colour-only state does not meet [USR-300] and needs a text cue. Until that cue exists, a screen-reader user cannot tell a stale decoration from a current one.
- No platform is named as screen-reader supported yet. The release check [USR-951] is pending, and [USR-950] applies: until a release names the reader and terminal it passed with, do not describe that platform as screen-reader supported.

Open items, stated so they can be tested: the Kanban card-move keys, a text cue for a stale tree decoration, and a text equivalent for the voice level meter. Each needs an owner decision before a release can claim full coverage.

### Errors and edge cases

| Situation | What happens |
|---|---|
| You uninstall a core plugin | Refused with `plugin.not-removable` (USR-040). Turn it off instead |
| Two plugins replace the same core plugin | Only one can be on. Turning on the second turns off the first and tells you |
| A replacement fails three times | It is turned off by the usual rule and the built-in plugin comes back |
| You turn off a core plugin that a replacement needs | The replacement shows "Waiting for <owner>" |
| Your administrator blocks replacements | The install fails with `policy.blocked` |

### See also

- [Plugins for users](/pages/user-plugins-overview/)
- [The Extensions tab](/pages/user-extensions-tab/)
- [Core plugins](/pages/core-plugins/)
- [Extending core plugins](/pages/extending-core-plugins/)
- [Core extension points](/pages/reference-core-extension-points/)

API version checked: `ilium:plugin@0.1.0`.
