# Settings

Ilium keeps almost every preference in one full-screen Settings view and in a plain-text configuration file, `config.toml`. Changes made in Settings take effect immediately and are written to disk automatically; you never press a global "Save". This page explains how to open and navigate Settings, lists every tab and the options on it, describes the configuration files and where they live on each operating system, and covers the guided first-run setup.

## Contents

- [Opening and using Settings](#opening-and-using-settings)
- [Auto-save behaviour](#auto-save-behaviour)
- [Settings tabs at a glance](#settings-tabs-at-a-glance)
- [Tab reference](#tab-reference)
- [Configuration files](#configuration-files)
- [`config.toml` structure](#configtoml-structure)
- [Project configuration: `.ilium/config.yaml`](#project-configuration-iliumconfigyaml)
- [Guided setup and onboarding](#guided-setup-and-onboarding)
- [Per-option help](#per-option-help)
- [Troubleshooting](#troubleshooting)

## Opening and using Settings

1. Press the prefix, then `:` (by default `Ctrl+B :`). The footer gear icon and the voice assistant can also open Settings.
2. The tab list is on the left; the selected tab's options are on the right. Click a tab, or press `Tab` to move to the next tab.
3. Move between options with the Up and Down arrows (or `k` and `j`). Where an option can be stepped, the Left and Right arrows (or `h` and `l`) change it; `Enter` or `Space` activates or edits it. Text and key fields open an editor: type, then press `Enter` to confirm or `Esc` to cancel.
4. Press `Esc` or `q` (or click **Close**) to leave Settings. The header also shows a **Guided setup** button that reopens the first-run walkthrough (see [Guided setup and onboarding](#guided-setup-and-onboarding)).

Choice controls show `← value + →`. Click the arrows to step backward or forward, or click `+` to open the full clickable list. A left click on the value moves forward; a right click moves backward. The list shows the current choice and any unavailable options with their reasons.

Number controls show `- value + *`, with the value centered. Click `-` or `+` to change it by the field's normal step. Click `*` to type an exact value, then press `Enter` to confirm or `Esc` to cancel. The field's limits still apply. On rows too narrow to fit both step buttons alongside the value and entry target, Ilium keeps the centered value and `*` entry target and omits the step buttons. These controls are also used for choices and numbers in animation settings, dialogs, toolbars and guided setup.

Use `Ctrl+B ?` (or the Keyboard tab) to see the active key map. Each tab ends or begins with the controls specific to it; some tabs (for example Animations, Keyboard and Setup) have richer interactions described below.

## Auto-save behaviour

- Toggles, selectors and steppers are saved the moment they change.
- Text and number inputs commit when you press `Enter`. Some numeric editors (for example the restructure token budget) also save valid changes automatically after a short pause (600 ms) while you type.
- The inference editors stay open until the new value has been durably written to disk. If saving fails (for example because the file is not writable), your input stays in the editor with an error message; press `Enter` to retry. `Esc` closes the editor without cancelling a write that has already been admitted.
- Each tab only rewrites its own table in `config.toml` and preserves every other table, including tables written by the detached server. You can safely hand-edit unrelated tables while Ilium runs, but see [Troubleshooting](#troubleshooting) for the advice on editing the same table concurrently.
- Server-owned values (detection poll intervals, custom agent signatures, notifications, sound, session recovery, the HTTP API port, progress-monitor enablement) are picked up by the running server through its config watcher, so they do not require a restart, with the exception noted for the API port below.

## Settings tabs at a glance

The tabs appear in this order:

| Tab | What it controls | Main config table |
| --- | --- | --- |
| User Interface | Left panel sizing, tree order, identifiers, colour scheme, motion, density, toolbars, banners, progress footer, selection and tree behaviours | `[ui]`, `[theme]` |
| Icons | Every configurable sidebar, toolbar and menu glyph | `[ui.icons]` |
| Agent Monitoring | Display mode, polling, custom agent signatures, status icons, progress footer | `[ui]`, `[detection]` |
| Agent Cost | Spend indicators and what counts as expensive | `[cost]` |
| Optimization | Scan Codex or Claude Code transcripts and apply the cheapest auto-compaction threshold | none (writes the agent's own config) |
| Remote compaction | Summarize a Claude or Codex session through the Inference model and resume from it | `[remote_compaction]` |
| Keyboard | Prefix keys and per-action key remapping, complete presets | `[keyboard]`, `[keybindings]` |
| Terminal | Scrollback budget, new-pane directory, Smart Copy light | `[terminal]` |
| Editor | Line display, line numbers, minimap, autosave, Markdown default | `[editor]` |
| Session | Recovery policy and automatic snapshot backups | `[session]` |
| Git | Worktree creation defaults | `[git]` |
| Kanban Board | Board card preview and column width | `[kanban_board]` |
| Animations | Ambient backgrounds and their shared look | project `.ilium/config.yaml` |
| Sound | Alert sound source, per-event sounds, desktop notifications | `[sound]`, `[notifications]` |
| Voice control | Voice model, devices, input mode, confirmation policy | `[voice]` |
| Reset planning | Public Claude and Codex reset announcement monitoring | `[reset_planning]` |
| Inference | AI provider, models, credentials, restructure budget | `[inference]` |
| LLM Instructions | All seven custom instruction inputs in one place | `[inference.instructions]`, `[voice]` |
| Titles | Title style and entry-naming instructions | `[inference]` |
| Triggers | Which events run AI titling and tree organization | `[triggers]` |
| Text Triggers | Regular-expression rules that send text to panes | `[text_triggers]` |
| Debug | File logging | `[debug]` |
| API | Port of the local HTTP agent-creation API | `[api]` |
| About | Version and project information | none |
| Setup | Install or refresh the Chatroom and Progress instructions in agent configuration files | `[agent_setup]` |

## Tab reference

### User Interface

Left panel sizing:

| Option | Values | Default |
| --- | --- | --- |
| Left panel sizing mode | Fixed, Focus-dependent, Width-dependent | Width-dependent |
| Fixed panel width | 16 to 80 cells (visible in Fixed mode) | 32 |
| Unfocused panel width | 16 to 80 cells (Focus-dependent and Width-dependent) | 24 |
| Focused panel width | 16 to 80 cells (Focus-dependent and Width-dependent) | 44 |
| Minimum terminal width | 40 to 500 cells (Width-dependent only) | 120 |

Fixed keeps one width. Focus-dependent shrinks the panel to the unfocused width when neither pointer nor keyboard focus is in the tree. Width-dependent does the same only on terminals narrower than the minimum terminal width; on roomy terminals the panel stays at its focused width. Hidden values stay saved when you switch cards, so comparing modes never loses your tuning.

General options:

| Option | Values | Default |
| --- | --- | --- |
| Tree order | Manual, Type, Age up (newest first), Age down (oldest first), Name A to Z, Name Z to A, and cost-based ordering | Manual |
| Project separators | On / Off. Draws a frame-style line after each project's visible tree | Off |
| Tree row management buttons | On / Off. Shows Rename and Move up/down on hover | Off |
| Agent identifier | Full name, Single letter, Selected icon, Nothing | Selected icon |
| Color theme | Dark or light preset | Dark |
| Motion level | Full, Reduced, Off (Off also freezes activity animation) | Reduced |
| Sidebar density | Compact, Standard, Comfortable | Standard |
| Stable glyphs | On / Off. Plain one-cell symbols for hover actions only | Off |
| Inferred-title icons | On / Off. Shows an AI-provided icon before an inferred title | Off |
| Agent debug menu | On / Off. Investigation-only agent history snapshots; costs CPU, memory and disk | Off |
| Context-menu icons | On / Off | On |
| Agent toolbar | On / Off. Toolbar above detected-agent panes | On |
| Agent toolbar labels | On / Off | On |
| Last prompt banner | On / Off. Shows the last exactly reconstructed prompt submitted to a detected agent | On |
| Last prompt banner lines | 1 to 20 | 4 |
| Terminal text selection | On / Off. Left-drag selects locally instead of forwarding to the foreground app | On |
| Lock-closed items | On / Off. Allows locking a project, group or folder row closed | On |
| Auto-remove empty groups | On / Off. Closes a group or folder when its last item closes | On |

The agent debug menu, progress monitor enablement and a few other options are mirrored into server state so a running server picks them up live. Stable glyphs is an explicit opt-in: Ilium never replaces its normal UTF-8 icons with plain glyphs by itself.

The progress-monitor options (Progress monitor, Progress footer lines, Progress fill style) live on the Agent Monitoring tab in the current build; see [Agent monitoring](agent-monitoring.md).

### Icons

Choose the glyph for each configurable role: group, top-level destination, project, vertical and horizontal split, folder, worktree branch, terminal, editor, board, each agent family, bookmark and locked-closed markers, tree toolbar buttons (Search, Restructure, Settings), hover-row controls (Rename, Move up, Move down, Close, Retitle), project restructure, Ask for update, screen-transfer arrows, Open in OS, and the agent toolbar actions (Compact, Clear, Configure, Stop, Copy screen, Smart Copy, Copy last message, Change effort, Change model, model choices, Exit, Fast mode, Text selection). Up and Down select a role, Left and Right cycle its glyph, and `Enter`, `Space` or `+` opens the icon picker. `R` toggles between the preview and the real rendering. Custom assignments are stored under `[ui.icons]`.

### Agent Monitoring

| Option | Notes |
| --- | --- |
| Display mode | Normal shows objective and current activity separately; Attention emphasises the highest-priority signal |
| Attention running indicator | How Attention mode still shows that an agent is working |
| Progress monitor | Turns supervision of registered long-running tasks on or off (default on) |
| Progress footer lines | 1 to 20 detail rows below the status line (default 4) |
| Hide completed progress after | Seconds a finished task footer stays; 0 or Never keeps it (default 60, steps of 30) |
| Progress fill style | Character family for progress frames |
| Working poll interval | Seconds between detection checks for working agents (default 10) |
| Idle poll interval | Seconds between checks for idle agents (default 45) |
| Custom agent signatures | Lowercase process-name substring plus agent class (`claude`, `codex`, `antigravity` or `other`) |
| Status icons | One symbol per status: working, waiting on background workers, background task running, waiting approval, finished unread, idle, goal states (active, paused, blocked, usage-limited, reached), parked awaiting monitor, task pending, task done, task error, monitor failed, scheduled input |
| Instructions | The Ask for update instruction input (see [Titles and instructions](titles-and-instructions.md)) |

Poll intervals below a few hundred milliseconds are clamped up by the server. Custom signatures are written to the server's `[[detection.custom_signatures]]` array. See [Agent monitoring](agent-monitoring.md) for the meaning of each status.

### Agent Cost

Controls spend indicators and how "expensive" is decided (fixed cuts, burn cuts, quota budget percentage, or a USD budget). Custom thresholds are set under `[cost]`. See [Agent cost](agent-cost.md).

### Optimization

Scans your agent transcripts (`~/.codex/sessions`, `~/.claude/projects`) and recommends the cheapest auto-compaction threshold for your own sessions. The two sub-tabs, **Codex** and **Claude Code**, each open on a **Scan sessions** button: nothing is scanned until you press it. The scan lists the files, reads them with a progress bar (by bytes, with the phase, files done of total, elapsed time and current file; `Esc` or **Cancel** stops it) and then shows the report: the recommendation with an **Optimal: ... - Apply to <agent>** button on top, a comparison of the CLI default, your current setting and the recommendation, corpus and compaction statistics, the cost mix, the fixed prefix, a simulation table with a chart, per-model optima, rework sensitivity, compaction regimes and warnings. Per-model optima are shown for information only: the setting is one global value and cannot be applied per model. A failed or cancelled scan keeps the previous report.

**Apply** opens a confirmation that names the resolved file, the key, the old and new value, the exact change and any environment, project or profile override that would shadow it. Only that one key changes (`autoCompactWindow` in `~/.claude/settings.json`, `model_auto_compact_token_limit` in `~/.codex/config.toml`), comments and everything else stay byte for byte, and only **new** agent sessions use it; running sessions keep the limit they loaded. A **Revert** button appears after an apply and restores the previous value unless the file was edited since. Ilium never restarts an agent. Keys: `←`/`→` agent, `s` scan, `a` apply, `e` apply the simulated optimum, `r` revert, `↑`/`↓`/`PgUp`/`PgDn` scroll. See [Agent cost](agent-cost.md) for the price table the dollar figures use.

### Remote compaction

Off by default. When on, the Compact button of a Claude or Codex pane summarizes the transcript with the provider and model chosen in the **Inference** tab instead of sending `/compact`, then resumes the agent from that summary. The tab holds the automatic trigger (a context threshold, a wait for a pause and a cooldown), the summarizing technique per agent (the real upstream prompts of Claude Code, Codex, opencode and Gemini CLI, an Ilium blend, or your own prompt), how much recent history stays verbatim, tool-output trimming, secret redaction, the summarizer input window and the number of backups kept. The transcript, which can contain prompts, code and secrets, is sent to that provider: a privacy box at the top of the tab says so and closes for good with `x`, `Delete` or its `[x]` button. The tab also reads each agent's own compaction trigger (user and project settings files, `CLAUDE_CODE_AUTO_COMPACT_WINDOW`, `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE`, the disable variables, Codex `model_auto_compact_token_limit`), shows it next to the CLI default, and warns when it is at or below the remote threshold, because the agent would then always compact first. Percentages assume the agent's default context window. See [Remote compaction](remote-compaction.md).

### Keyboard

The tab selects the general leader prefix (`shortcut_base`, a `Ctrl`-letter, default `Ctrl+B`) and a separate tree-navigation prefix (`navigation_shortcut_base`) used for group cycling and jumping. Below them is the full list of actions; open a row, then press a printable, arrow or Page key to remap it live. Only unassigned keys are active, and `Esc` closes the chooser unchanged. **Complete presets** load the GNU Screen or tmux key map in one step, including leader and bindings.

The remappable actions are: new terminal, new agent worktree, new editor, new board, close selection, new group, new split view, open folder, rename selection, move mode, focus tree, focus active pane, focus next or previous pane, focus pane left/right/above/below, cycle next or previous in group, jump next or previous group, scrollback up or down, run command, save editor, workspace search, help, detach client, kill session, toggle editor view, toggle line numbers, toggle minimap, toggle autosave, and open settings.

In `config.toml`, `[keyboard]` selects the prefixes and `[keybindings]` maps an action name to a key, for example:

```toml
[keyboard]
shortcut_base = "ctrl+a"

[keybindings]
new_terminal = "c"
cycle_next_in_group = "down"
```

Inside tmux, double the prefix to pass it through. Remapping only changes which key triggers an existing action; new actions cannot be defined.

### Terminal

| Option | Values | Default |
| --- | --- | --- |
| Scrollback budget | MiB retained per pane for new terminal views | 8 |
| New-pane directory | Project root, Focused terminal, Last used | Project root |
| Smart Copy light | On / Off | On |
| Smart Copy light key | Ctrl, Alt, Shift | Ctrl |

A larger scrollback budget does not resurrect output that has already scrolled away and does not grow existing views. With Smart Copy light on, hold the chosen key over a terminal, click regions to select them, and release the key to copy. See [Smart Copy](smart-copy.md).

### Editor

| Option | Values | Default |
| --- | --- | --- |
| Long lines | Clip, Wrap | Clip |
| Line numbers | On / Off | On |
| Minimap | On / Off | On |
| Autosave | On / Off | On |
| Autosave delay | Wait after the last edit (milliseconds) | 1000 |
| Markdown default | Source or Rendered for newly opened Markdown | Source |

These are defaults for new buffers; per-editor keys (toggle view, line numbers, minimap, autosave) override them for the focused editor. See [Editors and boards](editors-and-boards.md).

### Session

| Option | Values | Default |
| --- | --- | --- |
| Recovery policy | Restore automatically, Ask before restoring, Start fresh | Restore automatically |
| Automatic backups | On / Off. Periodic copies of the project's session snapshot, thinning older copies | On |

The policy applies to the detached server's next start. See [Session recovery](session-recovery.md).

### Git

| Option | Values | Default |
| --- | --- | --- |
| Default where | Here, New worktree, Existing worktree | Here |
| Branch prefix | Text | `agent/` |
| Worktree location template | Path template with `{repo_parent}`, `{repo_name}`, `{project}`, `{branch_slug}` | `{repo_parent}/{repo_name}.worktrees/{branch_slug}` |
| Default base | Current branch, Default branch | Current branch |
| Branch line | Worktree agents, Off | Worktree agents |
| Setup command | Command run after a worktree is created (blank means none) | blank |
| Close policy | Keep, Offer removal when safe | Keep |

These settings only change defaults offered by the new-agent flow; existing agents and branches never move. See [Worktrees](worktrees.md).

### Kanban Board

| Option | Range | Default |
| --- | --- | --- |
| Card preview lines | 1 to 10 | 4 |
| Minimum column width | 10 to 80 cells | 45 |

Narrower viewports scroll horizontally between complete columns.

### Animations

Pick an ambient background scene and tune the shared look (colour, palette, brightness, contrast, dithering, display placement, frame-rate cap, speed and density) plus scene-specific controls. Animation settings are stored per project in `.ilium/config.yaml` under `animation`, not in `config.toml`. See [Animations](animations.md).

### Sound

| Option | Notes |
| --- | --- |
| Source | OS beep (default), sound file, bundled chirping, generated, or muted |
| File | Choose among sounds Ilium discovered on this system |
| Preview | Plays the selected sound once |
| Sound events | Agent finished (on by default), Approval required, Agent started, Waiting on background, Task succeeded, Task failed |
| Desktop notifications | Master switch plus agent finished, approval required, task succeeded, task failed |
| Suppress redundant task outcomes | Hides task-outcome alerts while the agent is idle or parked, since its own finished alert follows |
| Coalesce | Collapses same-kind task alerts on one pane closer than this many seconds (0 disables; steps of 10; maximum 600) |

See [Notifications](notifications.md).

### Voice control

Model, voice, reasoning effort, input mode, VAD eagerness, input and output devices, output volume, terminal submission confirmation, media pause, custom prompt, API key and a **Reconnect now** button. Every option is documented in [Voice](voice.md).

### Reset planning

| Option | Notes |
| --- | --- |
| Monitor Claude resets | Checks the public Claude reset feed hourly |
| Monitor Codex resets | Checks public Codex announcements hourly; shows a countdown only when an exact time is supplied |
| Time display | Exact seconds or rounded human wording |

Forecasts use public announcements; they cannot predict private rolling limits.

### Inference

Provider selection, endpoints, credentials, models, **Load available models**, **Test provider**, and the restructure token budget. Fully described in [Inference and privacy](inference-and-privacy.md). The Organization, Shared context, Project naming and Smart Copy instruction inputs are also available at the bottom of this tab.

### LLM Instructions

Shows all seven optional instruction inputs together. See [Titles and instructions](titles-and-instructions.md).

### Titles

Title style (Labeling or Summarization) and the Entry naming instruction input. See [Titles and instructions](titles-and-instructions.md).

### Triggers

Maps eight events to AI actions (Retitle element, Restructure project, Restructure all projects). Defaults:

| Event | Default actions |
| --- | --- |
| Startup complete | Restructure all projects |
| Agent session ready | Retitle element |
| Agent receives a prompt | Retitle element |
| Agent starts working | Retitle element |
| Agent waits on background work | Retitle element |
| Agent needs approval | none |
| Agent finishes work | Retitle element and Restructure project |
| Terminal activity checkpoint | Retitle element |

The terminal checkpoint fires after every second submitted plain-shell command. Startup is global; the other agent events act on the originating pane and its project. Finishing work permits retitling plus one restructure scope, not both restructure scopes at once. Disable a trigger before entering sensitive content if you do not want AI requests to run; see [Inference and privacy](inference-and-privacy.md).

### Text Triggers

Rules pairing a regular expression with a literal message. When an enabled regex matches a completed output line, Ilium sends the message plus Enter to the selected target after the rule's **Delay** (whole seconds, default 60, `0` sends immediately). See [Automation](automation.md).

### Debug

**File logging** (default off). When on, instrumented actions, errors and complete text LLM exchanges are written to a timestamped session log; enabling applies at once to the client and the detached server. See [Inference and privacy](inference-and-privacy.md#logging).

### API

**Port** of the loopback-only `POST /create_agent` API (default 8872). A changed port takes effect when the detached server next starts. The listener is unauthenticated and local-only; keep it private. See [Automation](automation.md).

### About

Version and project information.

### Setup

Installs or refreshes the marked **Chatroom** and **Progress** instruction blocks in global and project agent instruction files (Claude and Codex), preserving surrounding text. Up and Down select a target, `Enter` or `Space` installs or removes the block (or opens the path editor for a global file row), and `R` resets a global file path to its default. Chatroom setup also creates `CHATROOM.md` and hooks. See [Automation](automation.md).

## Configuration files

| File | Scope | Format | Purpose |
| --- | --- | --- | --- |
| `config.toml` | Per user, global | TOML | Everything on the tabs above except animations |
| `.ilium/config.yaml` | Per project | YAML | Project name, icon, project separators, animation settings |
| `.ilium/sessions/<name>.json` | Per project session | JSON | Session snapshots (see [Session recovery](session-recovery.md)) |

### Where `config.toml` lives

Ilium uses the platform configuration directory for an application named `ilium`:

| OS | Path |
| --- | --- |
| Linux | `~/.config/ilium/config.toml` (respects `XDG_CONFIG_HOME`) |
| macOS | `~/Library/Application Support/ilium/config.toml` |
| Windows | `%APPDATA%\ilium\config\config.toml` |

Set the environment variable `ILIUM_CONFIG_DIR` to a directory to override the location entirely; a relative value is resolved against the working directory once. A missing file is not an error: every setting falls back to its default, and the file is created when you first change a setting.

## `config.toml` structure

The file is a set of tables. Only the tables and keys you want to change need to exist.

| Table | Contents |
| --- | --- |
| `[keyboard]`, `[keybindings]` | Prefix keys; action-to-key remapping |
| `[theme]` | Four colour overrides: `accent_bg`, `accent_fg`, `border_focused`, `border_unfocused` (hex such as `#1f6feb`) |
| `[ui]`, `[ui.icons]` | Interface options and icon assignments (keys below) |
| `[sound]`, `[notifications]` | Sound source, file and events; desktop notification flags |
| `[kanban_board]` | `card_preview_lines`, `minimum_column_width` |
| `[inference]` and sub-tables | Provider, title style, token budget, instructions, per-provider settings |
| `[triggers]` | Event-to-actions lists |
| `[text_triggers]` | Text trigger rules |
| `[agent_setup]` | Global instruction file paths and "never ask" choices |
| `[terminal]` | `scrollback_budget_mib`, `engine_memory_budget_mib`, `new_pane_directory`, `smart_copy_light`, `smart_copy_light_key` |
| `[editor]` | `line_display`, `show_line_numbers`, `show_minimap`, `autosave_enabled`, `autosave_delay_ms`, `markdown_rendered_by_default` |
| `[session]` | `recovery_policy`, `backups_enabled` |
| `[git]` | `default_where`, `branch_prefix`, `worktree_location_template`, `default_base`, `branch_line`, `setup_command`, `default_close_policy` |
| `[voice]` | See [Voice](voice.md) |
| `[debug]` | `file_logging_enabled` |
| `[api]` | `port` |
| `[reset_planning]` | Reset monitoring options |
| `[cost]` | Cost indicator thresholds |
| `[onboarding]` | Guided-setup progress; no credentials are stored here |
| `[detection]` | `working_poll_seconds`, `idle_poll_seconds`, `auto_answer_interstitial_prompts`, `[[detection.custom_signatures]]` |

Accepted spellings for common enumerated values:

| Key | Values |
| --- | --- |
| `ui.left_panel_sizing_mode` | `fixed`, `focus_dependent`, `terminal_width_dependent` |
| `ui.tree_order` | `manual`, `type`, `age_ascending`, `age_descending`, `name_ascending`, `name_descending` |
| `ui.agent_identifier_mode` | `full_name`, `letter`, `icon`, `hidden` |
| `ui.color_scheme` | `dark`, `light` |
| `ui.motion_level` | `full`, `reduced`, `off` |
| `ui.sidebar_density` | `compact`, `standard`, `comfortable` |
| `terminal.new_pane_directory` | `project_root`, `focused_terminal`, `last_used` |
| `terminal.smart_copy_light_key` | `ctrl`, `alt`, `shift` |
| `editor.line_display` | `clip`, `wrap` |
| `session.recovery_policy` | `restore_automatically`, `ask_before_restore`, `start_fresh` |

Other `[ui]` keys: `left_panel_fixed_width`, `left_panel_unfocused_width`, `left_panel_focused_width`, `left_panel_minimum_terminal_width`, `claude_agent_icon`, `codex_agent_icon`, `antigravity_agent_icon`, `use_stable_glyphs`, `show_inferred_title_icons`, `show_tree_row_management_controls`, `agent_debug_menu_enabled`, `show_context_menu_icons`, `agent_toolbar_enabled`, `show_toolbar_labels`, `last_prompt_enabled`, `last_prompt_max_lines`, `agent_monitoring_mode`, `attention_running_indicator`, `progress_monitor_enabled`, `progress_max_lines`, `completed_progress_hide_after_seconds`, `terminal_text_selection_enabled`, `lock_closed_enabled`, `auto_remove_empty_groups`, `task_progress_frames`.

An invalid value (for example a tree width outside 16 to 80, or an unknown enumerated string) makes that table fail validation with a message naming the key; fix or remove the key.

### Example

```toml
[ui]
left_panel_sizing_mode = "fixed"
left_panel_fixed_width = 36
motion_level = "reduced"

[terminal]
scrollback_budget_mib = 16
new_pane_directory = "focused_terminal"

[editor]
autosave_delay_ms = 1500

[inference]
selected_provider = "ollama"

[debug]
file_logging_enabled = false
```

## Project configuration: `.ilium/config.yaml`

Each project has a durable YAML file at `<project>/.ilium/config.yaml`. It is separate from session snapshots (which are volatile recovery state) and survives a fresh session. Known keys:

| Key | Meaning |
| --- | --- |
| `project name` | Display name of the project (set by you or by AI project naming) |
| `project icon` | Icon shown beside the project |
| `show project separators` | Draw the separator after this project's tree |
| `animation` | The ambient background settings: scene `kind`, shared look and per-scene controls |

Unknown keys are preserved when Ilium rewrites the file, so later settings are never erased. The file is written under an exclusive lock; avoid editing it while Ilium is mid-write.

## Guided setup and onboarding

On a fresh install (no configuration file yet), or when an earlier walkthrough was started but not finished, Ilium opens a guided setup before anything else. Reopen it at any time with `ilium --onboarding` (also accepted with `ilium new-session <name> --onboarding`) or the **Guided setup** button at the top of Settings.

The seven steps are:

1. **AI assistance** - choose Kilo Gateway, paid APIs, local Ollama, or Skip. Skipping goes straight to step 3.
2. **Connect AI** - enter the credentials or URL for the chosen provider and verify it.
3. **Your sound** - choose a bundled sound, a system sound or a custom one.
4. **Sound studio** - preview and adjust the alert sound.
5. **Your controls** - choose a tmux-style or GNU Screen-style key map, or customise.
6. **Playground** - practise the keys safely.
7. **Voice control** - optional microphone test.

A choice is required before leaving steps 1, 3 and 5. Progress is saved in `[onboarding]` so an interrupted walkthrough resumes. No credentials or live test results are stored in that table; provider settings are saved by their own tabs.

Automatic AI requests (titles, organization) stay paused until setup finishes. If you skip AI, they stay off. Existing configurations without an `[onboarding]` table keep their current provider.

## Per-option help

Most Settings rows have a help affordance. Press `?` on a selected row (or click its help marker) to open an explanation with an illustrative specimen. Help never performs the action it describes: it makes no provider requests, plays no sound, starts no microphone and changes no files. The help catalogue covers the interface, icons, agent monitoring, cost, keyboard, sound, voice, inference, Git, Editor, Session, Terminal, Triggers, Text Triggers, Kanban, Debug and API options.

## Troubleshooting

- **A setting does not seem to stick.** Check that the configuration directory is writable. Failed writes keep your input visible in the editor with an error; press `Enter` to retry.
- **Config fails to load.** The error names the table and key. Remove or correct that key; other tables load independently.
- **Hand edits are overwritten.** Ilium rewrites only the table you changed in Settings but from its in-memory copy. Edit the same table by hand only while that tab is closed, then reopen Settings.
- **Where is my config on Windows or macOS?** See [Where `config.toml` lives](#where-configtoml-lives).
- **API port change has no effect.** The detached server reads the port at start; it applies on the server's next start.
- **Left panel too wide or narrow.** Check the sizing mode: Width-dependent only shrinks the panel on terminals narrower than the minimum terminal width.
- **Experiments in a temporary configuration.** Run Ilium with `ILIUM_CONFIG_DIR` pointing at an empty directory to start from defaults without touching your real file.

Related: [Getting started](getting-started.md), [Inference and privacy](inference-and-privacy.md), [Voice](voice.md), [Session recovery](session-recovery.md), [CLI reference](cli-reference.md).
