---
layout: page
title: Providing the docs to agents
permalink: /pages/agent-docs/
---

## Providing the docs to agents

A coding agent that builds an Ilium plugin needs two things: the contract (this manual) and a short set of project rules. This chapter covers how the manual reaches an agent, which instruction file each tool reads, and how the files stay in step with the contract. The site serves the manual for machines as well as for people. It publishes an `llms.txt` index, one `llms-full.txt` file with every page, a plain Markdown copy of each page, and a kit of ready-made instruction files under `/agent-kit/`.

## You will need

- A plugin project, or the wish to create one with `ilium plugin new`. See [Set up a toolchain](/pages/getting-started-toolchain/).
- The address of this manual. The examples use `http://127.0.0.1:4327`, the address the build records in `_config.yml`. Replace it with the address where you read this page.
- Any coding agent. The kit covers Claude Code, Codex, Cursor and GitHub Copilot, and any tool that reads `AGENTS.md`.

## How an agent finds the docs

<figure>
<svg viewBox="0 0 520 190" role="img" aria-label="An agent reads a project instruction file, which points to llms.txt, which links to Markdown pages">
  <rect x="8" y="20" width="110" height="56" rx="6" fill="none" stroke="currentColor"/>
  <text x="63" y="44" text-anchor="middle" fill="currentColor" font-size="12">Plugin project</text>
  <text x="63" y="62" text-anchor="middle" fill="currentColor" font-size="11">AGENTS.md</text>
  <rect x="205" y="20" width="110" height="56" rx="6" fill="none" stroke="currentColor"/>
  <text x="260" y="44" text-anchor="middle" fill="currentColor" font-size="12">Docs site</text>
  <text x="260" y="62" text-anchor="middle" fill="currentColor" font-size="11">/llms.txt</text>
  <rect x="402" y="20" width="110" height="56" rx="6" fill="none" stroke="currentColor"/>
  <text x="457" y="44" text-anchor="middle" fill="currentColor" font-size="12">One page</text>
  <text x="457" y="62" text-anchor="middle" fill="currentColor" font-size="11">/pages/x.md</text>
  <line x1="118" y1="48" x2="205" y2="48" stroke="currentColor"/>
  <line x1="315" y1="48" x2="402" y2="48" stroke="currentColor"/>
  <text x="161" y="40" text-anchor="middle" fill="currentColor" font-size="10">URL</text>
  <text x="358" y="40" text-anchor="middle" fill="currentColor" font-size="10">link</text>
  <rect x="205" y="120" width="110" height="46" rx="6" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
  <text x="260" y="140" text-anchor="middle" fill="currentColor" font-size="11">/llms-full.txt</text>
  <text x="260" y="156" text-anchor="middle" fill="currentColor" font-size="10">every page</text>
  <line x1="260" y1="76" x2="260" y2="120" stroke="currentColor" stroke-dasharray="4 3"/>
</svg>
<figcaption>The project file holds the address. The index links each page as Markdown. The full file is the one-request alternative.</figcaption>
</figure>

The loop has three steps. The agent starts a session and loads the instruction file of the project automatically. The file names the index URL. When the agent needs a fact, it fetches `llms.txt`, picks a page, and fetches that page as Markdown. An agent that can hold a large context may fetch `llms-full.txt` once instead.

## Which file for which tool

| Tool | File and location | Read automatically |
|---|---|---|
| Codex, Jules, Gemini CLI, Zed, Warp, Devin, Amp and others | `AGENTS.md` at the repository root; the nearest file up the tree wins | Yes (agents.md lists the tools; Gemini CLI and Aider need a setting) |
| Claude Code | `CLAUDE.md` in the project or a parent folder; imports with `@path` | Yes. It reads `AGENTS.md` only when no `CLAUDE.md` exists, so a thin `CLAUDE.md` imports it |
| Claude Code skills | `.claude/skills/<name>/SKILL.md` | The name and description load at start; the body loads when the skill applies |
| Cursor | `.cursor/rules/*.mdc` with `description`, `globs`, `alwaysApply` front matter; also reads `AGENTS.md` | Yes, by rule type |
| GitHub Copilot | `.github/copilot-instructions.md`; path rules in `.github/instructions/*.instructions.md`; also reads `AGENTS.md` | Yes |
| Any tool with a fetch or browse tool | `llms.txt` | No. The project file must give the URL |

The sources are the pages at agents.md, code.claude.com (memory and skills), cursor.com (rules), docs.github.com (Copilot repository instructions) and llmstxt.org. The details are on [the project files page](/pages/agent-docs-project-files/).

<div class="note">
<p>An instruction file is context, not enforcement. The host enforces grants, limits and deadlines whatever an agent writes. The file helps an agent get it right the first time.</p>
</div>

## In this chapter

- [llms.txt and Markdown pages](/pages/agent-docs-llms-txt/): how this site publishes the index, the full file and the per-page Markdown.
- [Project instruction files](/pages/agent-docs-project-files/): the AGENTS.md, CLAUDE.md, Cursor and Copilot templates.
- [The Ilium plugin skill](/pages/agent-docs-skill/): an Agent Skill for Claude Code.
- [Keeping the kit current](/pages/agent-docs-maintenance/): what is generated, what is maintained by hand, and the release checklist.

## See also

- [Packages and manifests](/pages/packages/)
- [Versioning](/pages/versioning/)
