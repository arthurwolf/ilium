<!--
  contract: 0.1
  generated-with: manual
-->
# Ilium plugin agent kit

Ready-made instruction files for a project that builds an Ilium plugin. The site serves this folder at `/agent-kit/`. Every file points the agent at the manual index (`/llms.txt`). Replace the host `http://127.0.0.1:4327` in each file with the address of the manual you use.

| File | Tool | Install as |
|---|---|---|
| `AGENTS.md` | Codex, Jules, Gemini CLI, Zed, Warp, Cursor, Copilot and others | `AGENTS.md` at the repository root |
| `CLAUDE.md` | Claude Code | `CLAUDE.md` at the repository root; it imports `@AGENTS.md` |
| `.cursor/rules/ilium-plugin.mdc` | Cursor | the same path in the project |
| `.github/copilot-instructions.md` | GitHub Copilot | the same path in the project |
| `skills/ilium-plugin/SKILL.md` and `reference.md` | Claude Code Agent Skill | `.claude/skills/ilium-plugin/` in the project, or `~/.claude/skills/ilium-plugin/` for all projects |

Install in one step from the site:

```bash
BASE=http://127.0.0.1:4327/agent-kit
curl -fsSO $BASE/AGENTS.md -O $BASE/CLAUDE.md
mkdir -p .cursor/rules .github .claude/skills/ilium-plugin
curl -fs $BASE/.cursor/rules/ilium-plugin.mdc -o .cursor/rules/ilium-plugin.mdc
curl -fs $BASE/.github/copilot-instructions.md -o .github/copilot-instructions.md
curl -fs $BASE/skills/ilium-plugin/SKILL.md -o .claude/skills/ilium-plugin/SKILL.md
curl -fs $BASE/skills/ilium-plugin/reference.md -o .claude/skills/ilium-plugin/reference.md
```

Each file starts with a `contract:` stamp. When the plugin contract version changes, fetch the files again. The files are written by hand (`generated-with: manual`) and checked at each release.
