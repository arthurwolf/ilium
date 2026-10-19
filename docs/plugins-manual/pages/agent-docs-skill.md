---
layout: page
title: The Ilium plugin skill
permalink: /pages/agent-docs-skill/
---

## The Ilium plugin skill

An Agent Skill is a folder with a `SKILL.md` file that Claude Code loads on demand. The kit holds a skill, `ilium-plugin-sdk`, that carries the plugin workflow without filling the context of every session. This page shows its layout, front matter and content, and how to install it.

## You will need

- Claude Code. Skills in this layout are read from `.claude/skills/<name>/SKILL.md` in a project, from `~/.claude/skills/<name>/SKILL.md` for all projects of one user, and from a nested `.claude/skills` folder for a subfolder.
- The kit folder `skills/ilium-plugin/` from `/agent-kit/`.

## Layout

<pre class="term">.claude/skills/ilium-plugin/
├── SKILL.md       <span class="t-dim">front matter, workflow, hard rules (under 500 lines)</span>
╰── reference.md   <span class="t-dim">package layout, sides, deadlines, errors, test harness</span>
</pre>

## How it loads

<figure>
<svg viewBox="0 0 520 110" role="img" aria-label="Skill name and description always load; SKILL.md body loads when used; reference.md loads when read">
  <rect x="8" y="30" width="150" height="50" rx="6" fill="none" stroke="currentColor"/>
  <text x="83" y="52" text-anchor="middle" fill="currentColor" font-size="12">name, description</text>
  <text x="83" y="69" text-anchor="middle" fill="currentColor" font-size="10">always in context</text>
  <rect x="185" y="30" width="150" height="50" rx="6" fill="none" stroke="currentColor"/>
  <text x="260" y="52" text-anchor="middle" fill="currentColor" font-size="12">SKILL.md body</text>
  <text x="260" y="69" text-anchor="middle" fill="currentColor" font-size="10">when the task matches</text>
  <rect x="362" y="30" width="150" height="50" rx="6" fill="none" stroke="currentColor"/>
  <text x="437" y="52" text-anchor="middle" fill="currentColor" font-size="12">reference.md</text>
  <text x="437" y="69" text-anchor="middle" fill="currentColor" font-size="10">when the body links it</text>
  <line x1="158" y1="55" x2="185" y2="55" stroke="currentColor"/>
  <line x1="335" y1="55" x2="362" y2="55" stroke="currentColor"/>
</svg>
<figcaption>Progressive disclosure: only the description costs context until the skill is used.</figcaption>
</figure>

## The front matter

```yaml
---
name: ilium-plugin-sdk
description: Build, change, test and package Ilium plugins (WebAssembly components in Rust or TypeScript). Use when the project has an Ilium manifest.json, when the user asks for an Ilium plugin, ...
---
```

`name` defaults to the folder name and sets the slash command. `description` is what Claude reads to decide whether to load the skill, so it says what the skill does and when to use it; the first 1,536 characters of the description are kept. Other fields, such as `allowed-tools`, are optional and the kit does not use them. Claude Code ignores field names it does not know, so spelling mistakes fail silently.

## What the skill contains

| Section | Content |
|---|---|
| Find the facts first | Fetch `llms.txt`, then only the pages for the task as Markdown; say so when the manual lacks a fact. |
| Workflow | Seven steps from `ilium plugin new` to `ilium plugin pack`, including the fake-host test. |
| Both languages | The Rust and TypeScript names to use in every answer. |
| Hard rules | Never block, smallest grants, handle every outcome, no invented glyphs. |
| `reference.md` | Package layout, sides and worlds, deadlines and limits, error codes, test harness table. |

## Install

<pre class="term"><span class="t-prompt">$</span> BASE=http://127.0.0.1:4327/agent-kit/skills/ilium-plugin
<span class="t-prompt">$</span> mkdir -p .claude/skills/ilium-plugin
<span class="t-prompt">$</span> curl -fs $BASE/SKILL.md -o .claude/skills/ilium-plugin/SKILL.md
<span class="t-prompt">$</span> curl -fs $BASE/reference.md -o .claude/skills/ilium-plugin/reference.md
</pre>

Commit the folder to share it with the team. Use `~/.claude/skills/ilium-plugin/` instead to have it in every project.

## See also

- [Project instruction files](/pages/agent-docs-project-files/)
- [Keeping the kit current](/pages/agent-docs-maintenance/)
