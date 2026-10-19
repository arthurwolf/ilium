---
layout: page
title: Feature traceability
permalink: /pages/reference-feature-traceability/
---

## Feature traceability

This page maps every user-visible Ilium feature to its owner after the move to plugins. The ids F-001 to F-111 are the feature numbers of the feature inventory (`docs/research/plugin-platform/feature-inventory.md`, features 1 to 111). F-112 to F-116 are features the inventory folds into other rows; they get their own rows so each has its own parity test. The "User doc" column names the page in `src/docs/` that describes the feature today. The "Kind" column uses the inventory classification: native-kernel, plugin-wasm, primitive-extended and plugin-js. The "Wave" column uses the phases and waves of the [roadmap](/pages/roadmap/). The COR ids point to [core parity requirements](/pages/reference-requirements-core-parity/).

[COR-003] Two checks run over this table. A row whose Kind is `native-kernel` names `kernel` first in its Owner cell. Every table named in the Config keys column appears in the key-by-key table of [config and data migration](/pages/reference-config-and-data-migration/). [COR-001] Every feature in this table MUST keep working with the same behaviour when its owner plugin is enabled. [COR-002] Every row MUST have one primary owner, the first one named in its Owner cell; later names are supporting owners. A feature that two pages claim is listed once, under "Resolved duplicates".

Related pages: [kernel and boot](/pages/reference-kernel-and-boot/), [config and data migration](/pages/reference-config-and-data-migration/), [CLI compatibility](/pages/reference-cli-compatibility/), [core plugins](/pages/core-plugins/).

### 1. Tree, layout, keys and search

| Id | Feature | User doc | Kind | Owner | Side | Contract surface | Config keys | Data files | Wave | COR ids |
|---|---|---|---|---|---|---|---|---|---|---|
| F-001 | Project tree of projects, groups, folders, panes | `panes-and-layout` | native-kernel | kernel (tree model; the tree plugin's presentation is its own row) | both | tree.* | [ui] tree_order | snapshot tree | P1 kernel; 4b tree | COR-010 |
| F-002 | Groups, folders, top-level destinations | `panes-and-layout` | native-kernel | kernel | both | tree.* | none | snapshot tree | P1 | COR-010 |
| F-003 | Split views, 4 layouts | `panes-and-layout` | native-kernel | kernel | client | layout.*, ui.panels | [ui] left_panel_sizing_mode, left_panel_fixed_width, left_panel_unfocused_width, left_panel_focused_width, left_panel_minimum_terminal_width | snapshot layout | P1; 4b | COR-010 |
| F-004 | Move, reorder, rename, drag | `panes-and-layout` | native-kernel | kernel; tree plugin | both | tree.*, mouse | none | snapshot tree | P1; 4b | COR-010 |
| F-005 | Tree ordering (manual, type, age, name, cost) | `panes-and-layout` | plugin-wasm | tree plugin; cost adds sort key | client | sort-key extension point | [ui] tree_order | none | 4a | COR-010 |
| F-006 | Bookmarks, locks, auto-remove empty groups | `panes-and-layout` | plugin-wasm | tree plugin | both | tree.*, decorations | [ui] lock_closed_enabled, auto_remove_empty_groups | snapshot decorations | 4b | COR-010 |
| F-007 | Prefix, keybindings, presets, remap | `settings` | native-kernel | kernel keymap; keys-and-help plugin | client | keymap.* | [keyboard], [keybindings] | none | P1 kernel; 4b | COR-010 |
| F-008 | Mouse (click, drag, wheel, hit targets) | `panes-and-layout` | native-kernel | kernel | both | mouse events | [ui] mouse keys | none | P1 | COR-010 |
| F-009 | Workspace search | `panes-and-layout` | plugin-wasm | search plugin | both | ui.modal, fs.read | none | none | 4b | COR-011 |
| F-010 | Help and key reference | `settings` | plugin-wasm | keys-and-help plugin | client | help.contribute | none | help topics | 4b | COR-011 |

### 2. Terminal and pane processes

| Id | Feature | User doc | Kind | Owner | Side | Contract surface | Config keys | Data files | Wave | COR ids |
|---|---|---|---|---|---|---|---|---|---|---|
| F-011 | Pane processes | `how-it-works` | native-kernel | kernel PTY | server | panes.* | none | none | P1 | COR-010 |
| F-012 | Terminal emulation and screen state | `how-it-works` | native-kernel | kernel vt100 | server | terminal.read-screen | none | none | P1 | COR-010 |
| F-013 | Terminal view and scrollback budget | `panes-and-layout` | native-kernel | kernel; terminal plugin | client | terminal.* | [terminal] scrollback_budget_mib, engine_memory_budget_mib | none | P1 | COR-010 |
| F-014 | Terminal input and paste | `panes-and-layout` | native-kernel | kernel; terminal plugin | both | terminal.write | none | none | P1 | COR-010 |
| F-015 | Clickable links in terminal | `panes-and-layout` | primitive-extended | terminal plugin | client | terminal decorations | none | none | 4b | COR-011 |
| F-016 | Local text selection and right-click menu | `smart-copy` | primitive-extended | smart-copy plugin | client | terminal.select, ui.menus | [ui] terminal_text_selection_enabled | none | 4b | COR-011 |
| F-017 | Screen transfer between panes | `panes-and-layout` | primitive-extended | terminal plugin | client | terminal.read-screen, terminal.write | none | none | 4b | COR-011 |
| F-018 | Pane title from terminal | `titles-and-instructions` | primitive-extended | terminal plugin; titles plugin | both | panes.title | none | none | 4b | COR-011 |

### 3. Editors and boards

| Id | Feature | User doc | Kind | Owner | Side | Contract surface | Config keys | Data files | Wave | COR ids |
|---|---|---|---|---|---|---|---|---|---|---|
| F-019 | Editor panes | `editors-and-boards` | native-kernel | kernel now; editor plugin in phase 7 | client | pane kinds | [editor] | edited files | P7 | COR-050 |
| F-020 | Syntax highlighting and large-file windows | `editors-and-boards` | plugin-wasm | editor plugin (phase 7) | client | token provider | [editor] | none | P7 | COR-050 |
| F-021 | Markdown source and rendered views | `editors-and-boards` | plugin-wasm | editor plugin (phase 7) | client | render | [editor] | none | P7 | COR-050 |
| F-022 | File explorer and picker | `editors-and-boards` | primitive-extended | editor plugin | client | fs.read, ui.modal | none | none | P7 | COR-050 |
| F-023 | Create agent from an editor line or TODO | `editors-and-boards` | primitive-extended | editor plugin calls fleet | both | panes.create | none | none | P7 | COR-050 |
| F-024 | Kanban boards | `editors-and-boards` | native-kernel | kernel now; kanban plugin in phase 7 | client | pane kinds | [kanban_board] | board files | P7 | COR-050 |
| F-025 | Board created from a Markdown file | `editors-and-boards` | plugin-wasm | kanban plugin (phase 7) | client | fs.read | none | board files | P7 | COR-050 |

### 4. Agent detection, toolbar and messaging

| Id | Feature | User doc | Kind | Owner | Side | Contract surface | Config keys | Data files | Wave | COR ids |
|---|---|---|---|---|---|---|---|---|---|---|
| F-026 | Agent identity from process tree | `agent-monitoring` | primitive-extended | agent-detect plugin via signature registry | server | detect.signature | [detection] | none | 4a | COR-011 |
| F-027 | Activity classification | `agent-monitoring` | primitive-extended | agent-detect plugin | server | detect.classify | [detection] | none | 4a | COR-011 |
| F-028 | Pane status signals | `agent-monitoring` | primitive-extended | agent-detect plugin | both | agent.read | none | none | 4b | COR-011 |
| F-029 | Goal state tracking | `agent-monitoring` | primitive-extended | agent-detect plugin | both | agent.read | none | none | 4b | COR-011 |
| F-030 | Display modes and custom signatures | `agent-monitoring` | primitive-extended | agent-detect plugin | both | detect.signature | [ui] agent_monitoring_mode; [detection] custom_signatures | none | 4a | COR-011 |
| F-031 | Session stats and popover | `agent-cost` | plugin-wasm | cost plugin | both | agent.read, ui.panels | [cost] | stats store | 4b | COR-011 |
| F-032 | Agent debug history | `agent-monitoring` | primitive-extended | diagnostics plugin | both | agent.read | [ui] agent_debug_menu_enabled | debug history | 4b | COR-011 |
| F-033 | Last-prompt banner | `agent-monitoring` | primitive-extended | agent-toolbar plugin | both | agent.transcript, ui.chrome | [ui] last_prompt_enabled, last_prompt_max_lines | none | 4b | COR-011 |
| F-034 | Agent icons and identifiers | `agent-monitoring` | primitive-extended | agent-detect plugin | client | ui.chrome | [ui] claude_agent_icon, codex_agent_icon, antigravity_agent_icon, agent_identifier_mode | none | 4b | COR-011 |
| F-035 | Agent toolbar | `agent-monitoring` | primitive-extended | agent-toolbar plugin | both | ui.chrome, terminal.write | [ui] agent_toolbar_enabled, show_toolbar_labels | none | 4a | COR-011 |
| F-036 | Agent messaging and Ask for update | `automation` | primitive-extended | fleet plugin | both | terminal.write, panes.read | none | none | 4b | COR-011 |
| F-037 | Configure agent (agent config files) | `titles-and-instructions` | plugin-wasm | agent-setup plugin | client | fs.read:agent-config:<agent>, agent-config.propose:<agent> | none | agent config files | 4b | COR-011 |

### 5. Cost, titles and organisation

| Id | Feature | User doc | Kind | Owner | Side | Contract surface | Config keys | Data files | Wave | COR ids |
|---|---|---|---|---|---|---|---|---|---|---|
| F-038 | Spend and quota indicators | `agent-cost` | plugin-wasm | cost plugin | both | ui.chrome, services | [cost] | none | 4b | COR-011 |
| F-039 | Sort tree by cost | `agent-cost` | plugin-wasm | cost plugin | client | sort-key extension point | [ui] tree_order | none | 4a | COR-011 |
| F-040 | Price table | `agent-cost` | plugin-wasm | cost plugin | client | settings.contribute:agent-cost | [cost.prices] | none | 4b | COR-011 |
| F-041 | Automatic pane titles | `titles-and-instructions` | plugin-wasm | titles plugin | client | inference, panes.title | [inference] title_style; [inference.instructions] | none | 4b | COR-011 |
| F-042 | Session naming | `titles-and-instructions` | plugin-wasm | titles plugin | client | inference | [inference] title_style; [inference.instructions] | none | 4b | COR-011 |
| F-043 | Tree organisation | `titles-and-instructions` | plugin-wasm | titles plugin | client | inference, tree.* | [inference] title_style; [inference.instructions] | none | 4b | COR-011 |
| F-044 | AI triggers mapping events to actions | `titles-and-instructions` | plugin-wasm | titles plugin | client | events, inference | [triggers] | none | 4b | COR-011 |
| F-045 | Seven custom instruction inputs | `titles-and-instructions` | plugin-wasm | titles plugin | client | settings.contribute | [inference.instructions] | none | 4b | COR-011 |
| F-046 | Inferred-title icons | `titles-and-instructions` | plugin-wasm | titles plugin | client | ui.chrome | [ui] show_inferred_title_icons | none | 4b | COR-011 |

### 6. Notifications and voice

| Id | Feature | User doc | Kind | Owner | Side | Contract surface | Config keys | Data files | Wave | COR ids |
|---|---|---|---|---|---|---|---|---|---|---|
| F-047 | Sound alerts | `notifications` | plugin-wasm | notifications plugin | server | sound.play | [sound], [notifications] | sound files | 4a | COR-011 |
| F-048 | Desktop notifications | `notifications` | plugin-wasm | notifications plugin | server | notify.desktop | [notifications] | none | 4a | COR-011 |
| F-049 | Coalescing of redundant task alerts | `notifications` | plugin-wasm | notifications plugin | server | timers | [notifications] | none | 4a | COR-011 |
| F-050 | Voice session | `voice` | plugin-wasm | voice plugin | both | audio, inference | [voice] | none | 4b | COR-011 |
| F-051 | Voice confirmations | `voice` | plugin-wasm | voice plugin | client | ui.modal | [voice] | none | 4b | COR-011 |
| F-052 | ilium voice say | `cli-reference` | plugin-wasm | voice plugin | both | cli.contribute, client requests | none | none | 4b | COR-040 |
| F-053 | Voice onboarding demo | `voice` | plugin-wasm | voice plugin | client | audio | none | none | 4b | COR-011 |
| F-054 | Voice device selection | `voice` | plugin-wasm | voice plugin | client | audio | [voice] | none | 4b | COR-011 |

### 7. Automation, progress and chatroom

| Id | Feature | User doc | Kind | Owner | Side | Contract surface | Config keys | Data files | Wave | COR ids |
|---|---|---|---|---|---|---|---|---|---|---|
| F-055 | Scheduled input | `automation` | plugin-wasm | scheduler plugin | server | timers, terminal.write | none | snapshot section ilium.core.scheduler | 4b | COR-011 |
| F-056 | Text triggers | `automation` | primitive-extended | text-triggers plugin | server | terminal.output-filter, terminal.write | [text_triggers] | none | 4a | COR-011 |
| F-057 | Prompt queue | `automation` | primitive-extended | scheduler plugin | server | agent.read, terminal.write | none | snapshot section ilium.core.scheduler | 4b | COR-011 |
| F-058 | Local HTTP API POST /create_agent | `automation` | native-kernel | kernel | server | http-api.route | [api] port | none | P1; 4b | COR-010 |
| F-059 | Progress monitor | `automation` | plugin-wasm | progress plugin | server | process.run, ui.chrome | [ui] progress_monitor_enabled, progress_max_lines | monitors in snapshot | 4b | COR-011 |
| F-060 | ilium progress CLI | `cli-reference` | plugin-wasm | progress plugin | server | cli.contribute | none | none | 4b | COR-040 |
| F-061 | Progress footer in tree | `automation` | plugin-wasm | progress plugin | client | ui.chrome | [ui] completed_progress_hide_after_seconds, task_progress_frames | none | 4b | COR-011 |
| F-062 | Chatroom | `automation` | plugin-wasm | chatroom plugin | both | cli.contribute, fs.write:project | none | CHATROOM.md, .ilium/chat-readers/ | 4b | COR-011 |
| F-063 | Agent setup (hooks and instructions) | `automation` | plugin-wasm | agent-setup plugin | client | fs.read:agent-config:<agent>, agent-config.propose:<agent> | [agent_setup] | agent config files | 4b | COR-011 |
| F-064 | Context-injection hook | `automation` | plugin-wasm | chatroom plugin | server | cli.contribute | none | chat-readers | 4b | COR-040 |

### 8. Smart Copy and worktrees

| Id | Feature | User doc | Kind | Owner | Side | Contract surface | Config keys | Data files | Wave | COR ids |
|---|---|---|---|---|---|---|---|---|---|---|
| F-065 | Smart Copy light | `smart-copy` | primitive-extended | smart-copy plugin | client | terminal.select | [terminal] smart_copy_light, smart_copy_light_key | none | 4b | COR-011 |
| F-066 | Smart Copy AI | `smart-copy` | primitive-extended | smart-copy plugin | client | inference | [terminal] smart_copy_light, smart_copy_light_key; [inference.instructions] | none | 4b | COR-011 |
| F-067 | Agent in a new Git worktree | `worktrees` | plugin-wasm | worktrees plugin | both | git, panes.create | [git] | worktrees | 4b | COR-011 |
| F-068 | Start an agent in an existing worktree | `worktrees` | plugin-wasm | worktrees plugin | client | git | [git] | none | 4b | COR-011 |
| F-069 | Worktree status and branch line | `worktrees` | plugin-wasm | worktrees plugin | server | git | [git] | none | 4b | COR-011 |
| F-070 | Worktree cleanup and prune | `worktrees` | plugin-wasm | worktrees plugin | server | git | [git] | none | 4b | COR-011 |
| F-071 | Worktreeinclude and post-create command | `worktrees` | plugin-wasm | worktrees plugin | server | git, process.run | [git] | none | 4b | COR-011 |
| F-072 | Git and worktree defaults | `worktrees` | plugin-wasm | worktrees plugin | client | settings.contribute:git | [git] | none | 4b | COR-011 |

### 9. Sessions, compaction and inference

| Id | Feature | User doc | Kind | Owner | Side | Contract surface | Config keys | Data files | Wave | COR ids |
|---|---|---|---|---|---|---|---|---|---|---|
| F-073 | JSON snapshots | `session-recovery` | native-kernel | kernel snapshot writer | server | snapshot sections | none | session json | P1 | COR-030 |
| F-074 | Rolling snapshot backups | `session-recovery` | native-kernel | kernel | server | none | none | backups | P1 | COR-030 |
| F-075 | Restore after restart | `session-recovery` | native-kernel | kernel; sessions plugin for policy UI | server | lifecycle | [session] recovery_policy, backups_enabled | none | P1; 4b | COR-030 |
| F-076 | Resume verified agent sessions | `session-recovery` | primitive-extended | sessions plugin via agent-session primitive | server | agent.read, agent.control, fs.read:agent-sessions:<agent> | [session] recovery_policy, backups_enabled | transcripts | 4b | COR-011 |
| F-077 | Convert a session between Claude and Codex | `session-recovery` | plugin-wasm | session-convert plugin | server | agent.transcript, fs.read:agent-sessions:<agent> | none | transcripts | 4b | COR-011 |
| F-078 | Session management and flags | `cli-reference` | native-kernel | kernel | CLI | none | none | none | P1 | COR-040 |
| F-079 | Ordered event journal | `how-it-works` | native-kernel | kernel | server | events | none | none | P1 | COR-010 |
| F-080 | Remote compaction | `remote-compaction` | primitive-extended | compaction plugin | server | inference, agent.transcript | [remote_compaction] | ledger | 4b | COR-011 |
| F-081 | Compact button routing | `remote-compaction` | primitive-extended | agent-toolbar plugin | client | terminal.write | [remote_compaction] | none | 4a | COR-011 |
| F-082 | Compaction optimiser | `remote-compaction` | plugin-wasm | compaction plugin | server | agent.transcript, jobs | [remote_compaction] | none | 4b | COR-011 |
| F-083 | Agent auto-compact trigger reader | `remote-compaction` | plugin-wasm | compaction plugin | client | fs.read:agent-config:<agent> | none | agent config files | 4b | COR-011 |
| F-084 | AI providers | `inference-and-privacy` | plugin-wasm | inference plugin | server | inference service | [inference] | none | 4c | COR-011 |
| F-085 | AI admission gate and privacy | `inference-and-privacy` | native-kernel | kernel gate; inference plugin UI | both | inference | [inference], [onboarding] | none | P1 | COR-010 |
| F-086 | Debug file logging | `settings` | native-kernel | kernel | both | logging | [debug] file_logging_enabled | logs | P1 | COR-010 |
| F-087 | Background worker banks | `how-it-works` | native-kernel | kernel | both | jobs.run | none | none | P1 | COR-010 |
| F-088 | Reset planning | `inference-and-privacy` | plugin-wasm | cost plugin | server | http, timers | [reset_planning] | none | 4b | COR-011 |

### 10. Settings, animations and CLI

| Id | Feature | User doc | Kind | Owner | Side | Contract surface | Config keys | Data files | Wave | COR ids |
|---|---|---|---|---|---|---|---|---|---|---|
| F-089 | Guided setup | `getting-started` | plugin-wasm | settings plugin | client | ui.modal | none | none | 4b | COR-011 |
| F-090 | Settings UI | `settings` | plugin-wasm | settings plugin | client | settings.* | none | none | 4b | COR-011 |
| F-091 | Config persistence and live mirror | `settings` | native-kernel | kernel config store | both | settings.* | config.toml | config.toml | P1 | COR-030 |
| F-092 | Themes, motion, density, stable glyphs | `settings` | native-kernel | kernel | client | theme | [ui] theme keys | none | P1 | COR-010 |
| F-093 | Shared look | `animations` | native-kernel | kernel look painter; ambient plugin for controls | client | scene host | project YAML animation | none | P1; 4c | COR-010 |
| F-094 | Shared dithering and display | `animations` | native-kernel | kernel; ambient plugin for controls | client | scene host | project YAML animation | none | P1; 4c | COR-010 |
| F-095 | Natural cached-loop scenes | `animations-nature-and-abstract` | plugin-wasm | scenes plugin | scene | renderer | project YAML animation | none | 4c | COR-011 |
| F-096 | Procedural quiet scenes and machines | `animations-nature-and-abstract` | plugin-wasm | scenes plugin | scene | renderer | project YAML animation | none | 4c | COR-011 |
| F-097 | Geometry scenes, pipes, wind | `animations-nature-and-abstract` | plugin-wasm | scenes plugin | scene | renderer | project YAML animation | none | 4c | COR-011 |
| F-098 | Simulations and games | `animations-media-and-games` | plugin-wasm | scenes plugin | scene | renderer | project YAML animation | none | 4c | COR-011 |
| F-099 | Live-data scenes | `animations-maps-space-and-live-data` | plugin-wasm | scenes plugin | scene | renderer, http | project YAML animation | none | 4d | COR-011 |
| F-100 | Maps, space and Wikipedia | `animations-maps-space-and-live-data` | plugin-wasm | scenes plugin | scene | renderer, http | project YAML animation | none | 4d, 4e | COR-011 |
| F-101 | Media (video, spectrum, images) | `animations-media-and-games` | plugin-wasm | scenes plugin | scene | renderer, codec | project YAML animation | none | 4d | COR-011 |
| F-102 | Packaged JavaScript animations | `animations` | plugin-js | scene plugins (animation scenes as wasm packages; the JavaScript runtime is retired) | scene | renderer | project YAML animation | iliumanim packages | 4c | COR-011 |
| F-103 | Semantic animation | `animations` | plugin-wasm | scenes plugin with titles | both | inference | project YAML animation | none | 4c | COR-011 |
| F-104 | Optional GPU backend | `animations` | native-kernel | kernel | client | none | none | none | P1 | COR-010 |
| F-105 | ilium attach, --cwd, --onboarding | `cli-reference` | native-kernel | kernel | CLI | none | none | none | P1 | COR-040 |
| F-106 | new-session, ls, kill-session | `cli-reference` | native-kernel | kernel | CLI | none | none | none | P1 | COR-040 |
| F-107 | ilium new-pane | `cli-reference` | plugin-wasm | fleet plugin | CLI | cli.contribute | none | none | 4b | COR-040 |
| F-108 | ilium wait | `cli-reference` | plugin-wasm | progress plugin (alias of progress wait) | CLI | cli.contribute | none | none | 4b | COR-011 |
| F-109 | ilium panes | `cli-reference` | plugin-wasm | fleet plugin | CLI | cli.contribute | none | none | 4b | COR-040 |
| F-110 | ilium broadcast | `cli-reference` | plugin-wasm | fleet plugin | CLI | cli.contribute | none | none | 4b | COR-040 |
| F-111 | Hidden internal commands | `cli-reference` | native-kernel | kernel | CLI | none | none | none | P1 | COR-040 |
| F-112 | Pane recap (`ilium recap`) | `cli-reference` | plugin-wasm | recap plugin | server | cli.contribute, inference | none | none | 4b | COR-041 |
| F-113 | Ask for update | `automation` | plugin-wasm | fleet plugin | client | ui.menus, terminal.write | none | none | 4b | COR-011 |
| F-114 | Create agent from editor line | `editors-and-boards` | plugin-wasm | editor plugin | client | ui.menus, panes.create | none | none | 7 | COR-011 |
| F-115 | Compaction optimiser (Optimization tab) | `settings` | plugin-wasm | compaction plugin | server | settings.write-kernel:optimization | none | none | 4b | COR-011 |
| F-116 | Reset forecasts and Reset planning tab | `inference-and-privacy` | plugin-wasm | cost plugin | server | settings.write-kernel:reset-planning | [reset_planning] | none | 4b | COR-011 |

### Resolved duplicates

Five features were claimed by two core-plugin pages. Each now has one owner. The other page links to the owner.

| Feature | Claimed by | Owner | Reason |
|---|---|---|---|
| Session conversion | compaction and session-convert | [session-convert](/pages/core-plugin-session-convert/) | Compaction only resumes its summary into a new session. Conversion between Claude and Codex is one feature (F-077). |
| Setup tab and agent setup | chatroom and agent-setup | [agent-setup](/pages/core-plugin-agent-setup/) | It writes the agent instruction files for chatroom and progress (F-037, F-063). |
| Onboarding | settings and voice | [settings](/pages/core-plugin-settings/) | The guided setup is one flow (F-089). Voice owns only the demo step (F-053). |
| `new-pane --worktree` | fleet and worktrees | [worktrees](/pages/core-plugin-worktrees/) creates the worktree; [fleet](/pages/core-plugin-fleet/) owns the command | The command parses arguments. The worktree service does the Git work (F-067, F-107). |
| Plugins screen keys and help | keys-and-help and settings | [settings](/pages/core-plugin-settings/) owns the screen; [keys-and-help](/pages/core-plugin-keys-and-help/) owns help topics | One screen, one owner. |

### Reconciliation with the feature inventory

- The inventory has 111 features. This table has 116 rows: the 111 inventory features plus F-112 to F-116, which the inventory folds into other features. No feature is dropped.
- Feature 108 (`ilium wait`) is native-kernel in the inventory. The manual makes it an alias of `ilium progress wait`, so the progress plugin owns it and the Kind is `plugin-wasm`. The kernel keeps no `wait` command.
- Features 19, 20, 21, 24 and 25 (editors and boards) are native-kernel in the inventory. They become plugins in phase 7, so the table names the editor and kanban plugins as the final owners and gives rows owned only by a plugin (F-020, F-021, F-025, F-107) the Kind `plugin-wasm` (see [roadmap](/pages/roadmap/)).
- Features 93 and 94 (shared look, dithering and display) stay in the kernel painter. The [ambient look](/pages/core-plugin-ambient/) plugin owns only the controls.
- Feature 58 (HTTP API) stays in the kernel as the listener. The fleet plugin registers `POST /create_agent` through `http-api.route`.
- Feature 102 (packaged JavaScript animations) is plugin-js in the inventory. The roadmap retires the V8 runtime at the end of wave 4c. Scenes become wasm packages.
- Features 72, 77 and 88 have no source path in the inventory; their owners follow the user documentation.
