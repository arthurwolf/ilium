### Changed
- "Create agent from line" on the editor line menu is now the Editor plugin's command `ilium.core.editor/create-agent-from-line`. The plugin opens its own dialog (agent choice among Claude, Codex and Antigravity, plus the editable task text that starts as the line's `/goal` prompt) and creates the agent pane beside the editor. With the Editor plugin off the menu entry reports that the plugin is not running and nothing is created.
- The Editor plugin manifest gains the command and the grants `panes.create-command:codex`, `panes.create-command:agy` and `notify.toast`.

### Removed
- The kernel create-agent dialog: its mode, state, prompt, geometry, keyboard and mouse handling, the nested provider catalog and the `agent/goal-from-line` prompt template (the plugin ships its own copy).
