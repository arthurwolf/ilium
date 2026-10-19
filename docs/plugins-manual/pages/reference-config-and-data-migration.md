---
layout: page
title: Config and data migration
permalink: /pages/reference-config-and-data-migration/
---

## Config and data migration

Ilium already stores user data: a global `config.toml`, a project `.ilium/config.yaml`, session snapshots, rolling backups, agent instruction files and a chatroom file. Core plugins must read and write the same files, with the same key names, so that a user who installs the plugin build loses nothing and can read every file with the old tools. This page states the migration rules, lists every key and file with its owner, and defines the one mechanism that plugins use to store state in a session snapshot.

Contract version: `ilium:plugin@0.1.0`. The platform is not implemented. Requirement ids have the form `[COR-nnn]` and are collected in [Core plugin parity requirements](/pages/reference-requirements-core-parity/).

### Contents

- Migration rules
- Settings ownership: kernel tables and plugin tables
- Who owns each tab
- `config.toml`, key by key
- Plugin tables in `config.toml`
- Project YAML
- Session snapshots
- Snapshot sections
- Data files
- Agent configuration files that Ilium writes
- A worked migration
- Errors and edge cases
- Requirements

### Migration rules

1. **No key is renamed or moved.** Every key in `config.toml` and `.ilium/config.yaml` keeps its table and name. A feature that becomes a plugin keeps reading the kernel key through `settings.read-kernel:<tab>`. It does not copy the value to `[plugins."ilium.core.<name>"]`.
2. **The plugin tables are additive.** The platform adds `[plugins]`, `[plugins.limits]` and `[plugins."<id>"]`. A build that does not know them MUST ignore and preserve them.
3. **Unknown keys are preserved.** The kernel keeps every key it does not know when it rewrites a file, as it does today for the project YAML.
4. **A value that fails validation fails its table, not the file.** The message names the key. The other tables still load. A file that cannot be parsed at all is not a failed table: the host leaves it byte for byte unchanged, uses the built-in defaults for every key of that file for this run, never rewrites it while it is unparseable, prints one `warning` with the code `config.parse-error` naming the file and the line, and still boots ([COR-090]).
5. **A format that changes gets a version.** `config.toml` carries an integer `config_version`. Absent means 0, the format of the build before plugins. The snapshot carries `version`. Each plugin section in a snapshot carries `schema_version`.
6. **Migrations run once, on read, and are idempotent.** A migration reads the old shape, writes the new one to a temporary file in the same folder and renames it. Running it twice gives the same file.
7. **A migration keeps a copy.** Before the first write of a new `version`, the kernel keeps the old file next to it as `<stem>.pre-migration.<old-version><ext>`, where the file name splits into stem and extension at the last dot and `<old-version>` is the old `version` number. For example `main.json` at version 3 is kept as `main.pre-migration.3.json`. Snapshots also keep the existing `.pre-restore.json` copies (see Session recovery in the user documentation).
8. **No downgrade promise for snapshots.** A snapshot written by the plugin build has `version` 4. A build that only knows version 3 reads it without the plugin sections and without error (unknown fields are ignored), but the plugin state is not restored.
9. **Seeds are not re-run.** Default values are applied only for missing keys. A default never overwrites a stored value.

### Settings ownership: kernel tables and plugin tables

Two stores exist and they do not overlap.

| Store | Where | Who writes | Who reads |
|---|---|---|---|
| Kernel tables | `[ui]`, `[terminal]`, `[inference]` and the other 25 tab tables of `config.toml`, plus the project YAML | The kernel when the user edits a row. The one owner plugin of the tab, with `settings.write-kernel:<tab>` | Any plugin with `settings.read-kernel:<tab>` |
| Plugin tables | `[plugins."<id>"]` and `.ilium/config.yaml` key `plugins.<id>` | The plugin, through the always-granted `settings.set-json` | The plugin. Other plugins with `settings.read-all` (T1 only) |

The rules for the kernel tables, which close the question of "who may write a kernel value" (this replaces the read-only wording of D13, see [design decisions](/pages/design-decisions/#d13-kernel-settings-are-readable-and-the-tab-owner-may-write)):

- **[COR-030]** A kernel tab MUST have at most one owner plugin. A tab whose owner is not installed has no owner: it stays readable and no install or boot step fails because of it. Only the owner plugin MAY hold `settings.write-kernel:<tab>`. A second plugin that declares the same grant fails to install with `manifest.settings-owner`.
- **[COR-031]** `settings.write-kernel:<tab>` is a T1-only, high-risk grant. A T2 or T3 package that declares it fails with `manifest.grant-tier`. Any plugin, of any tier, MAY hold `settings.read-kernel:<tab>`.
- The write goes through the same validator as the Settings screen. A value outside its range fails with `settings.invalid-value` and nothing is written. A write is atomic for the whole tab object: all keys in the call apply or none do.
- The rows of a kernel tab stay drawn by the kernel from the [settings schema](/pages/reference-settings-schema/). The owner plugin does not draw them again. The owner uses the write grant for changes the user did not type on the row: an import, a migration, a price update, a reset to defaults.
- A tab with no core plugin owner (a feature that stays native, such as the Editor and Kanban Board tabs in v1) has no owner. The kernel writes it.

### Who owns each tab

The 25 frozen tab ids and their owners. The owner is the only plugin that may hold `settings.write-kernel:<tab>`. The last column lists plugins that read the tab.

| Tab id | Label | `config.toml` table | Owner | Readers | Keys written |
|---|---|---|---|---|---|
| `user-interface` | User Interface | `[ui]`, `[theme]` | Settings | Tree, Agent toolbar, Titles, Diagnostics, Terminal | Every `[ui]` key except the ten listed under `agent-monitoring`, plus `[theme]` |
| `icons` | Icons | `[ui.icons]` | Settings | Tree, Agent toolbar | `[ui.icons]` |
| `agent-monitoring` | Agent Monitoring | `[ui]`, `[detection]` | Agent detect | Progress, Agent toolbar, Notifications | `[ui]` `agent_identifier_mode`, `agent_monitoring_mode`, `attention_running_indicator`, `claude_agent_icon`, `codex_agent_icon`, `antigravity_agent_icon`, `progress_monitor_enabled`, `progress_max_lines`, `completed_progress_hide_after_seconds`, `task_progress_frames`; all of `[detection]` |
| `agent-cost` | Agent Cost | `[cost]` | Cost | Fleet, Tree | `[cost]` |
| `optimization` | Optimization | none (writes the agent's own config) | Compaction | none | none |
| `remote-compaction` | Remote compaction | `[remote_compaction]` | Compaction | none | `[remote_compaction]` |
| `keyboard` | Keyboard | `[keyboard]`, `[keybindings]` | Keys and help | none | `[keyboard]`, `[keybindings]` |
| `terminal` | Terminal | `[terminal]` | Terminal | Smart copy | `[terminal]` |
| `editor` | Editor | `[editor]` | Editor (kernel while native) | Terminal | `[editor]` |
| `session` | Session | `[session]` | Sessions | none | `[session]` |
| `git` | Git | `[git]` | Worktrees | Fleet | `[git]` |
| `kanban-board` | Kanban Board | `[kanban_board]` | Kanban (kernel while native) | none | `[kanban_board]` |
| `animations` | Animations | project YAML `animation` | Ambient look | Scenes | project YAML `animation` |
| `sound` | Sound | `[sound]`, `[notifications]` | Notifications | none | `[sound]`, `[notifications]` |
| `voice-control` | Voice control | `[voice]` | Voice | none | Every `[voice]` key except `custom_prompt` |
| `reset-planning` | Reset planning | `[reset_planning]` | Cost | Notifications | `[reset_planning]` |
| `inference` | Inference | `[inference]` | Inference | Titles, Recap, Search, Smart copy, Compaction, Voice | Every `[inference]` key except `title_style` and `instructions` |
| `llm-instructions` | LLM Instructions | `[inference.instructions]`, `[voice]` | Inference | Titles, Recap, Smart copy, Voice | `[inference.instructions]` and `[voice] custom_prompt` |
| `titles` | Titles | `[inference]` | Titles | none | `[inference] title_style` only |
| `triggers` | Triggers | `[triggers]` | Titles | none | `[triggers]` |
| `text-triggers` | Text Triggers | `[text_triggers]` | Text triggers | none | `[text_triggers]` |
| `debug` | Debug | `[debug]` | Settings | Diagnostics | `[debug]` |
| `api` | API | `[api]` | Fleet | none | `[api]` |
| `about` | About | none | Settings | none | none |
| `setup` | Setup | `[agent_setup]` | Agent setup | none | `[agent_setup]` |

The table order is the rail order, equal to `src/docs/settings.md`: User Interface, Icons, Agent Monitoring, Agent Cost, Optimization, Remote compaction, Keyboard, Terminal, Editor, Session, Git, Kanban Board, Animations, Sound, Voice control, Reset planning, Inference, LLM Instructions, Titles, Triggers, Text Triggers, Debug, API, About, Setup. Plugin tabs follow after About and Setup. The labels are the baseline labels. The high-risk marking of the API tab is a notice row inside the tab, not part of the label.

**[COR-034]** The "Keys written" column is enforced by the host. A write through `settings.write-kernel:<tab>` that names a key outside the column fails with `settings.key-not-owned` and nothing is written, even when another tab of the same table holds that key. Several tabs share the `[ui]`, `[inference]` and `[voice]` tables; each tab rewrites only its own keys, as the baseline Settings screen does.

Twelve core plugins do not own a tab: Agent toolbar, Chatroom, Diagnostics, Layout, Progress, Recap, Scenes, Scheduler, Search, Session convert, Smart copy and Tree. They read the tabs of the plugins whose feature they depend on. Tree options such as tree order and project separators live on the User Interface tab and are written by the kernel when the user edits a row.

The tab `agent-cost` is also the id of the Cost plugin's earlier plugin tab. A plugin tab id is local to its plugin (`self:<tab-id>`) and never collides with a kernel id, but the two names must not both appear in one rail. The Cost plugin therefore declares no tab of its own. It adds sections to the kernel tab with `settings.contribute:agent-cost`. See [Plugin settings extension](/pages/settings-extension/).

### `config.toml`, key by key

The tables below are the complete list of keys the kernel knows today (see the [Settings page](/pages/settings-screens/) for values and defaults). The column **Consumer** names the core plugin whose behaviour the key changes. **Live** says whether a running side sees the change without a restart.

| Table | Keys | Consumer | Live |
|---|---|---|---|
| `[ui]` panel sizing | `left_panel_sizing_mode`, `left_panel_fixed_width`, `left_panel_unfocused_width`, `left_panel_focused_width`, `left_panel_minimum_terminal_width` | Kernel layout | yes |
| `[ui]` tree | `tree_order`, `show_tree_row_management_controls`, `show_project_separators`, `lock_closed_enabled`, `auto_remove_empty_groups` | Tree | yes |
| `[ui]` agents | `agent_identifier_mode`, `agent_monitoring_mode`, `attention_running_indicator`, `claude_agent_icon`, `codex_agent_icon`, `antigravity_agent_icon`, `show_inferred_title_icons` | Agent detect, Titles | yes |
| `[ui]` look | `color_scheme`, `motion_level`, `sidebar_density`, `use_stable_glyphs`, `show_context_menu_icons` | Kernel theme and menus | yes |
| `[ui]` toolbar | `agent_toolbar_enabled`, `show_toolbar_labels`, `last_prompt_enabled`, `last_prompt_max_lines` | Agent toolbar | yes |
| `[ui]` other | `agent_debug_menu_enabled`, `terminal_text_selection_enabled` | Diagnostics, Terminal | yes |
| `[ui]` progress | `progress_monitor_enabled`, `progress_max_lines`, `completed_progress_hide_after_seconds`, `task_progress_frames` | Progress | yes (server-owned) |
| `[ui.icons]` | one key per icon role | Tree, Agent toolbar | yes |
| `[theme]` | `accent_bg`, `accent_fg`, `border_focused`, `border_unfocused` | Kernel theme | yes |
| `[detection]` | `working_poll_seconds`, `idle_poll_seconds`, `auto_answer_interstitial_prompts`, `[[detection.custom_signatures]]` | Agent detect | yes (server-owned) |
| `[cost]` | `metric`, `quota_window`, `calibration`, `fixed_cuts`, `burn_cuts`, `budget_usd`, `quota_fixed_cuts`, `quota_burn_cuts`, `quota_budget_percent`, `history_days`, `sparkline_window_minutes`, `sparkline_cells`, `level_glyph`, `level_dollars`, `meter`, `sparkline`, `group_totals`, `detail_card`, `header_total`, `burn_marker`, `sort_by_cost`, `prices` | Cost | yes |
| `[remote_compaction]` | `enabled`, `automatic`, `threshold_percent`, `claude_technique`, `codex_technique`, `other_technique`, `custom_prompt`, `tail_tokens`, `protected_recent_tool_tokens`, `tool_result_chars`, `redact_secrets`, `summarizer_context_tokens`, `pause_timeout_seconds`, `cooldown_minutes`, `keep_backups`, `privacy_banner_dismissed` | Compaction | yes |
| `[keyboard]`, `[keybindings]` | `[keyboard]` holds the prefix keys and the preset; `[keybindings]` holds one key per action id, named by the action id in the key reference | Kernel keymap, Keys and help | yes |
| `[terminal]` | `scrollback_budget_mib`, `engine_memory_budget_mib`, `new_pane_directory`, `smart_copy_light`, `smart_copy_light_key` | Terminal, Smart copy | yes (`engine_memory_budget_mib` for new panes) |
| `[editor]` | `line_display`, `show_line_numbers`, `show_minimap`, `autosave_enabled`, `autosave_delay_ms`, `markdown_rendered_by_default` | Editor | yes |
| `[session]` | `recovery_policy`, `backups_enabled` | Sessions, kernel snapshot writer | yes (server-owned) |
| `[git]` | `default_where`, `branch_prefix`, `worktree_location_template`, `default_base`, `branch_line`, `setup_command`, `default_close_policy` | Worktrees | yes |
| `[kanban_board]` | `card_preview_lines`, `minimum_column_width` | Kanban | yes |
| `[sound]`: `source`, `file`, `design`, `events`. `[notifications]`: `enabled`, `agent_finished`, `approval_required`, `task_succeeded`, `task_failed`, `suppress_redundant_task_outcomes`, `task_coalesce_seconds` | Notifications | yes (server-owned) |
| `[voice]` | `enabled`, `api_key`, `model`, `voice`, `reasoning_effort`, `input_mode`, `vad_eagerness`, `input_device_name`, `output_device_name`, `output_volume_percent`, `confirm_terminal_submissions`, `pause_media_while_active`, `custom_prompt` | Voice | yes |
| `[reset_planning]` | `monitor_claude`, `monitor_codex`, `time_style` | Cost | yes |
| `[inference]` | `selected_provider`, `title_style`, `restructure_prompt_token_limit`, the sub-table `[inference.instructions]` (next row), and the provider tables `kilo_gateway`, `ollama`, `openai`, `anthropic`, `openrouter` | Inference, Titles | yes |
| `[inference.instructions]` | seven instruction inputs | Inference, Titles, Recap, Smart copy | yes |
| `[triggers]` | eight event keys, each with an action list; the key names are the event names in the Triggers tab of `src/docs/settings.md` | Titles | yes |
| `[text_triggers]` | rule list | Text triggers | yes (server-owned) |
| `[agent_setup]` | the global instruction file paths and the never-ask choices; the field names are those of the Setup tab in `src/docs/settings.md` | Agent setup | yes |
| `[debug]` | `file_logging_enabled` | Kernel logger, Diagnostics | yes |
| `[api]` | `port` | Kernel HTTP listener | at next server start |
| `[onboarding]` | guided-setup progress, no credentials | Settings | yes |

Credentials: keys with secrets (API keys) keep their present storage. A plugin that reads a tab sees `"<redacted>"` in place of every secret (see [kernel settings](/pages/api-kernel-settings-and-keymap/)). A plugin that must send a credential does so through a service that the owner provides (for example the Inference plugin calls the provider itself), never by reading the secret.

### Plugin tables in `config.toml`

```toml
# config.toml
config_version = 1

[plugins]
auto_update = "notify"          # off, notify, install
auto_update_interval_hours = 24
channel = "stable"
metered = false
migration_backup_mib = 2048     # 0 disables the pre-migration backup, max 1048576

[plugins.limits]
event_queue = 2048

[plugins."com.example.notes"]
density = "compact"

[plugins."com.example.notes".limits]
memory_mib = 64
```

- `[plugins]` keys are owned by the plugin host. Other keys are refused with `config.unknown-key`.
- `[plugins.limits]` sets a limit for every plugin. `[plugins."<id>".limits]` sets it for one plugin and takes precedence over `[plugins.limits]`, but a plugin table can never raise a value above the ceiling. The effective value is `min(ceiling, v)`, where `v` is the first set of the `[plugins."<id>".limits]` value, the `[plugins.limits]` value, the manifest value and the default, in that order (LIF-143).
- `[plugins."<id>"]` holds the plugin's own settings. The reserved key `limits` is the only sub-table the host reads there.
- A core plugin keeps its values in the kernel tables listed above, not in `[plugins."ilium.core.<name>"]`. A core plugin MAY have a plugin table for values that are new in the plugin build, and each such key is listed on its page.
- The always-granted `settings` import reads and writes only the plugin's own table. It never sees `[plugins.limits]` of another plugin.

### Project YAML

`<project>/.ilium/config.yaml` keeps its four keys. Unknown keys are preserved.

| Key | Owner | Consumer |
|---|---|---|
| `project name` | Titles (project naming) | Tree |
| `project icon` | Titles | Tree |
| `show project separators` | Tree | Tree |
| `animation` | Ambient look | Scenes, kernel painter |
| `plugins.<id>` | the plugin | the plugin (project scope of its settings) |

The `animation` key holds the scene `kind`, the shared look and the per-scene controls. A scene control is stored under `animation.scene.<scene-id>`. The shared look is stored once, under `animation.look`, and is never repeated per scene. The file is written under an exclusive lock, and a plugin write goes through the same lock, so the kernel and a plugin never write at the same moment.

### Session snapshots

The snapshot is `<project>/.ilium/sessions/<session name>.json`. It is written by the kernel, atomically, 750 ms after the last change at most. At the baseline commit the file has `version` 3 and these fields.

| Field | Meaning | Owner in the plugin build |
|---|---|---|
| `version` | Format version of the whole file | Kernel |
| `tree` | Nodes, groups, titles, order | Kernel |
| `panes` | One entry per pane: `node_id` and `kind` (shell, agent with its resume command, editor, board) | Kernel |
| `agent_debug_logs` | Pane histories for the agent debug menu | Diagnostics section |
| `progress_monitors` | Durable monitor registrations and result delivery state | Progress section |
| `workspace_close_preferences` | Close policy of a worktree workspace | Worktrees section |
| `tree` pane fields `scheduled_input`, `prompt_queue` | Pending scheduled input and prompt queue | Scheduler section |

Version 4 adds one field:

| Field | Type | Meaning |
|---|---|---|
| `plugin_sections` | object, key is plugin id | One entry per plugin that offered a section. Each entry is `{ "schema_version": <int>, "data": <json> }` |

The migration from 3 to 4 moves the three top-level fields and the two pane fields into `plugin_sections` under the ids of their owners (`ilium.core.diagnostics`, `ilium.core.progress`, `ilium.core.worktrees`, `ilium.core.scheduler`). The scheduler section is `plugin_sections["ilium.core.scheduler"] = {"schema_version":1,"data":{"panes":[{"pane_id":<n>,"scheduled_input":<v3 value or null>,"prompt_queue":<v3 value>}]}}`. The kernel keeps publishing the objective signal "scheduled input pending" from that section while the scheduler is disabled. It runs once, in the kernel, while no plugin exists. The old fields are removed from the file after the old file is kept as `<stem>.pre-migration.3<ext>` (rule 7). If the owner plugin is not installed, the section is kept as stored JSON and written back unchanged for 30 days of server time, then dropped with a warning in the lifecycle log (see Snapshot sections).

### Snapshot sections

One mechanism stores plugin state in a session snapshot: the extension point `ilium.core.sessions/snapshot-sections`. The earlier idea of storing opaque bytes at `deactivate` is not a persistence path. The bytes returned by `deactivate(restore)` live in the host's memory, are handed to the new instance on an update or a restart of the same session, and are lost when the server stops (see [Host architecture](/pages/reference-host-architecture/)). The rules:

| Item | Rule |
|---|---|
| Entry | `{ "id": "<section id>", "title": "...", "max_bytes": <n> }` contributed to `snapshot-sections`. `max_bytes` is at most 262144. The owner is the contributing plugin. One section per plugin in v1 |
| Write | The plugin implements the request `snapshot-section` in its handler and returns JSON. It is asked at every snapshot, at most once per 750 ms |
| Size | At most `max_bytes` per section, and never more than 256 KiB (262144 bytes). A larger value is dropped with `snapshot.section-too-large` and the previous stored value is kept |
| Version | The plugin returns `schema_version`. The host stores it with the data |
| Read | At boot step 7 the host sends `restore-section { schema_version, data }` to the plugin before any other event |
| Unknown version | If the plugin does not accept the `schema_version`, it answers `section-unsupported`. The section stays unchanged in the file |
| Missing plugin | A section of an uninstalled plugin, including one created by the 3 to 4 migration, is written back unchanged for 30 days of server time, then dropped with a warning in the lifecycle log |
| Crash safety | The kernel writes the last stored data of a failed plugin unchanged. A failed plugin never blocks the snapshot |

[COR-032] covers this. The session storage bucket of the `storage` import is a second, separate mechanism: key-value data that a plugin owns and the host copies into the snapshot as the bucket `session`. A core plugin uses snapshot sections for state that other parts of Ilium must see at restore time, such as scheduled sends (snapshot section `ilium.core.scheduler`) and progress monitors, and the bucket for its private state.

### Data files

| File | Location | Format | Written by | Plugin owner | Notes |
|---|---|---|---|---|---|
| `config.toml` | `<config>/config.toml` | TOML | Kernel, tab owners | per tab | `ILIUM_CONFIG_DIR` overrides the folder |
| `.ilium/config.yaml` | `<project>/.ilium/config.yaml` | YAML | Kernel, Ambient look, Titles | per key | Unknown keys preserved |
| Session snapshot | `<project>/.ilium/sessions/<name>.json` | JSON | Kernel | Sessions offers sections | Atomic |
| Pre-restore copies | `<project>/.ilium/sessions/<name>.pre-restore.json` and `.pre-restore.1.json` | JSON | Kernel | none | Last two starts |
| Rolling backups | `<project>/.ilium/backups/<name>/<ms>-<uuid>.json` | JSON | Kernel | Sessions (policy) | When `backups_enabled` |
| Lifecycle log | `<project>/.ilium/logs/<name>.lifecycle.jsonl` | JSONL | Kernel | Sessions | Rotated once at 8 MiB |
| Worktrees | `<repo parent>/<main checkout name>.worktrees/<branch slug>` | Git worktree | Worktrees | Worktrees | From the template `{repo_parent}/{repo_name}.worktrees/{branch_slug}`; created by `new-pane --worktree` |
| Chatroom | `<project>/CHATROOM.md` and `CHATROOM.archive.md` | Markdown | Chatroom | Chatroom | `/CHATROOM.md` and `/CHATROOM.archive.md` are added to `.gitignore` |
| Boards | any `.md` path the user picks | Markdown | Kanban | Kanban | Every change is saved to the file |
| Debug logs | `$ILIUM_DEBUG_LOG_DIR/…` | text | Kernel | Diagnostics | Five newest per session |
| Plugin storage | `<data>/plugins/storage/<id>.db`, `<project>/.ilium/plugins/<id>.db` | key-value | the plugin | the plugin | See [Plugin files](/pages/reference-plugin-files/) |
| Claude statusline registry | `<data>/claude-model-statusline-projects.json` | JSON | Agent detect (client side) | Agent detect | Holds session id, project path, model id |

All paths follow the folder rules of [Plugin files on disk](/pages/reference-plugin-files/). A core plugin never writes to a path outside this table without a grant (`fs.write:<scope>`).

### Agent configuration files that Ilium writes

These files belong to the agent tools, not to Ilium. Ilium edits only its own marked block, or its own entry, and leaves the rest of the file unchanged. Agent setup owns all of them, except where noted.

| File | Scope | What Ilium writes | Owner |
|---|---|---|---|
| `~/.claude/CLAUDE.md` | global | Marked Chatroom and Progress instruction blocks | Agent setup |
| `~/.codex/AGENTS.md` | global | The same blocks. Paths come from `[agent_setup]` | Agent setup |
| `<project>/CLAUDE.md`, `<project>/AGENTS.md` | project | The same blocks. A symlink is followed and written once | Agent setup |
| `<project>/.claude/settings.local.json` | project | Chatroom hook entries. A temporary `statusLine` entry that captures the model, restored on exit | Agent setup (hooks), Agent detect (statusLine) |
| `<project>/.codex/hooks.json` | project | Chatroom hook entries | Agent setup |
| `<project>/.gitignore` | project | `/CHATROOM.md` and `/CHATROOM.archive.md` lines | Chatroom |
| Agent session files (Claude, Codex, Gemini, Antigravity) | global | Written only by Session convert (new files) and Compaction (resume files). Existing files are never edited | Session convert, Compaction |

A file in the project folder is written with `fs.write:project` and a revision check. A file in an agent's configuration folder (the rows marked global) is never written by the plugin: the plugin proposes the new content with `agent-config.propose:<agent>`, the kernel shows the user the diff, and the host writes only the files the user accepts ([agent-config.propose](/pages/capability-agent-config-propose/)). Both routes check the revision, so a change made by another process in the meantime is detected and the change is refused with `conflict`. See [fs atomic writes](/pages/api-fs-atomic-and-tail/).

### A worked migration

A user has a build before plugins with this `config.toml` and snapshot, and installs the plugin build.

```toml
[terminal]
scrollback_budget_mib = 16

[detection]
working_poll_seconds = 1
```

1. The server starts. It reads `config.toml`. No `config_version`, so it is 0. No key changes. The server adds nothing until the user changes a setting.
2. The server reads the snapshot with `version` 3 and 2 progress monitors. The kernel keeps `main.pre-migration.3.json`, then writes version 4 with `plugin_sections["ilium.core.progress"] = { "schema_version": 1, "data": { "monitors": [ … ] } }`.
3. The host starts. Progress is a startup plugin. At step 7 it receives `restore-section`. It runs one probe per monitor before it reinstalls it (the same rule as today).
4. The user opens Settings. The Terminal tab shows 16 for the scrollback. The Terminal plugin reads `[terminal]` through `settings.read-kernel:terminal` and has the same value. No copy exists in `[plugins."ilium.core.terminal"]`.
5. The user changes the value to 32. The kernel writes `[terminal] scrollback_budget_mib = 32`, sends `kernel-settings-changed` and the plugin reads again.

A short sample of the plugin side of step 3:

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use serde_json::{json, Value};

const SCHEMA_VERSION: u32 = 1;

impl Handler for ProgressPlugin {
    fn on_request(&mut self, request: Request) -> Response {
        match request.kind.as_str() {
            "snapshot-section" => Response::json(json!({
                "schema_version": SCHEMA_VERSION,
                "data": { "monitors": self.monitors_as_json() }
            })),
            "restore-section" => {
                let schema_version = request.field_u64("schema_version").unwrap_or(0);
                if schema_version != u64::from(SCHEMA_VERSION) {
                    return Response::error("section-unsupported");
                }
                let data: Value = request.field_json("data").unwrap_or(Value::Null);
                self.restore_monitors(&data);
                Response::ok()
            }
            _ => Response::error("unknown-request"),
        }
    }
}
```

**TypeScript**

```ts
import { definePlugin, type Request, type Response } from '@ilium/plugin-sdk';

const SCHEMA_VERSION = 1;
let monitors: unknown[] = [];

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  activate() {},
  onRequest(request: Request): Response {
    if (request.kind === 'snapshot-section') {
      return { tag: 'json', val: { schema_version: SCHEMA_VERSION, data: { monitors } } };
    }
    if (request.kind === 'restore-section') {
      const body = request.body as { schema_version?: number; data?: { monitors?: unknown[] } };
      if (body.schema_version !== SCHEMA_VERSION) return { tag: 'error', val: 'section-unsupported' };
      monitors = body.data?.monitors ?? [];
      return { tag: 'ok' };
    }
    return { tag: 'error', val: 'unknown-request' };
  },
});
```

The request names are the only contract here. The SDK helper names follow [Requests and outcomes](/pages/requests-and-outcomes/).

### Errors and edge cases

| Situation | Result | Code |
|---|---|---|
| Two plugins declare `settings.write-kernel:<tab>` for one tab | The second install is refused | `manifest.settings-owner` |
| A T2 package declares `settings.write-kernel:<tab>` | Install refused | `manifest.grant-tier` |
| A write is outside the range of a row | Nothing is written | `settings.invalid-value` |
| A write names a key that belongs to another tab of the same table | Nothing is written | `settings.key-not-owned` |
| A write names a key the kernel does not know in that tab | Nothing is written | `settings.unknown-key` |
| `config.toml` has a table the build does not know | Preserved | none |
| The migration from 3 to 4 is interrupted | The next start reads the old file and runs it again | none |
| A snapshot section is larger than its `max_bytes` (at most 256 KiB) | The section is dropped, the old value is kept | `snapshot.section-too-large` |
| A section's plugin rejects its `schema_version` | The section is kept unchanged | `section-unsupported` |
| The `.ilium/config.yaml` lock is held by another process | The write waits up to 2 s, then fails | `timed-out` |
| An agent file changed during a write | The write is refused, the user is told | `conflict` |

### Requirements

- [COR-030] A kernel tab MUST have at most one plugin that holds `settings.write-kernel:<tab>`, and a second declaration MUST fail with `manifest.settings-owner`. Test: install two fixture T1 plugins that ask for `settings.write-kernel:terminal`; the second install fails.
- [COR-031] `settings.write-kernel:<tab>` MUST be T1 only and high risk, and `settings.read-kernel:<tab>` MUST be available to every tier. Test: a T2 package with the write grant fails with `manifest.grant-tier`; a T3 package with the read grant installs.
- [COR-032] Plugin state in a snapshot MUST be stored only through `snapshot-sections` with a `schema_version`, a section larger than its `max_bytes` (at most 256 KiB) MUST be dropped without losing the previous value, and a failed plugin MUST NOT block the snapshot. Test: write a 300 KiB section, check the old value stays; crash the plugin, check that the file still has the last value.
- [COR-090] A `config.toml` or `.ilium/config.yaml` that cannot be parsed MUST be left unchanged on disk and MUST NOT be rewritten while it stays unparseable. The host MUST use the built-in defaults for that file for the run, MUST print one `warning` with the code `config.parse-error` that names the file, and MUST still boot. Test: start a server with `config.toml` holding `[ui` and assert the bytes are unchanged, the warning, and a running server.
- [COR-033] No key in `config.toml` or `.ilium/config.yaml` MAY be renamed or moved by the plugin build, and the plugin build MUST preserve unknown keys and tables. Test: run the plugin build on a fixture config with an unknown key, change one setting, compare the files with a diff that must show only that change.

### See also

- [Settings screens](/pages/settings-screens/) and [Settings schema](/pages/reference-settings-schema/)
- [Kernel settings and keymap](/pages/api-kernel-settings-and-keymap/) and [settings.read-kernel](/pages/capability-settings-read-kernel/)
- [Plugin settings extension](/pages/settings-extension/) and [Settings API](/pages/api-settings/)
- [Plugin files on disk](/pages/reference-plugin-files/)
- [Kernel and boot reference](/pages/reference-kernel-and-boot/)
- [Feature traceability](/pages/reference-feature-traceability/)
- [Core plugins](/pages/core-plugins/)
