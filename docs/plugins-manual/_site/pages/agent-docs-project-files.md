# Project instruction files

## Project instruction files

Each coding agent reads one or two files from the repository without being asked. The kit holds a version of those files for an Ilium plugin project. This page shows what each tool reads, what is in the kit, how to install it and how to keep it current.

## You will need

- A plugin project from `ilium plugin new`. See [Set up a toolchain](/pages/getting-started-toolchain/).
- The kit at `/agent-kit/` on this site, or the folder `_agent-kit/` in the manual sources. The [kit README](/agent-kit/README.md) lists every file.

## What each tool reads

| Tool | File | Facts that matter |
|---|---|---|
| AGENTS.md tools | `AGENTS.md` at the root | Plain Markdown, no required headings. The nearest file up the tree wins, and a prompt from the user overrides it. Monorepos may add nested files. |
| Claude Code | `CLAUDE.md` in the working folder or a parent | Target under 200 lines. `@path` imports are expanded at launch, up to four hops deep, relative to the importing file. With only an `AGENTS.md` and no `CLAUDE.md`, Claude Code reads `AGENTS.md`; once a `CLAUDE.md` exists it reads that file only, so the kit file imports `@AGENTS.md`. |
| Cursor | `.cursor/rules/*.mdc` | Front matter `description`, `globs` and `alwaysApply`. A plain `.md` file in that folder is ignored. Cursor also reads `AGENTS.md` in the root and nested folders. |
| GitHub Copilot | `.github/copilot-instructions.md` | Repository-wide. Path rules go in `.github/instructions/NAME.instructions.md` with `applyTo`. Copilot also reads `AGENTS.md`. |

<figure>
<svg viewBox="0 0 520 150" role="img" aria-label="CLAUDE.md imports AGENTS.md; Cursor and Copilot read their own files and AGENTS.md">
  <rect x="190" y="10" width="140" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="260" y="35" text-anchor="middle" fill="currentColor" font-size="12">AGENTS.md (source)</text>
  <rect x="10" y="100" width="130" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="75" y="123" text-anchor="middle" fill="currentColor" font-size="11">CLAUDE.md</text>
  <rect x="195" y="100" width="130" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="260" y="123" text-anchor="middle" fill="currentColor" font-size="11">ilium-plugin.mdc</text>
  <rect x="380" y="100" width="130" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="445" y="123" text-anchor="middle" fill="currentColor" font-size="11">copilot-instructions</text>
  <line x1="75" y1="100" x2="215" y2="50" stroke="currentColor"/>
  <text x="120" y="68" fill="currentColor" font-size="10">@AGENTS.md</text>
  <line x1="260" y1="100" x2="260" y2="50" stroke="currentColor" stroke-dasharray="4 3"/>
  <line x1="445" y1="100" x2="305" y2="50" stroke="currentColor" stroke-dasharray="4 3"/>
</svg>
<figcaption>Solid line: an import. Dashed lines: copies that repeat the essentials and are kept in step by hand.</figcaption>
</figure>

## What the kit contains

| Kit file | Content |
|---|---|
| `AGENTS.md` | The full rules: documentation URLs, toolchain, the `ilium plugin` commands, eight contract rules, Rust and TypeScript idioms, testing, and a list of things never to do. |
| `CLAUDE.md` | An import of `@AGENTS.md` and two Claude-specific lines. |
| `.cursor/rules/ilium-plugin.mdc` | A short rule that attaches to `manifest.json`, Rust and TypeScript sources. |
| `.github/copilot-instructions.md` | A short summary that points at `AGENTS.md`. |

The long rules live in one place, `AGENTS.md`. The tool files repeat only the essentials, so a change has few places to land.

## Install

<pre class="term"><span class="t-prompt">$</span> BASE=http://127.0.0.1:4327/agent-kit
<span class="t-prompt">$</span> curl -fsSO $BASE/AGENTS.md -O $BASE/CLAUDE.md
<span class="t-prompt">$</span> mkdir -p .cursor/rules .github
<span class="t-prompt">$</span> curl -fs $BASE/.cursor/rules/ilium-plugin.mdc \
    -o .cursor/rules/ilium-plugin.mdc
<span class="t-prompt">$</span> curl -fs $BASE/.github/copilot-instructions.md \
    -o .github/copilot-instructions.md
</pre>

Then replace the host `http://127.0.0.1:4327` in each file with the address of the manual you use, and commit the files. The skill has its own install step on [the skill page](/pages/agent-docs-skill/).

## Excerpt: what the agent is told

This is the core of the kit `AGENTS.md`, the part that matters most for a first change.

**Rust**

```rust
// Rule 1: events in, intents out. Return quickly; never block.
fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
    batch.into_iter().filter_map(|event| self.handle(event)).collect()
}
```

**TypeScript**

```ts
// Rule 1: events in, intents out. Return quickly; never block.
onEvents(batch: Event[]): Submission[] {
  return batch.flatMap((event) => this.handle(event));
}
```

The bodies of `handle` depend on the plugin; the tutorials show complete ones. See [Events and intents](/pages/events-and-intents/).

## Keep the files in step with the contract

Every kit file starts with a stamp. In Markdown it is an HTML comment; in the Cursor rule and the skill it is a comment after the front matter.

```text
contract: 0.1
generated-with: manual
```

`contract` is the version of the plugin contract the facts were checked against. `generated-with: manual` marks a file that people maintain. When the contract changes, compare the stamp of the installed files with the stamp on the site, and fetch the files again if they differ. The release checklist is on [Keeping the kit current](/pages/agent-docs-maintenance/).

## See also

- [Providing the docs to agents](/pages/agent-docs/)
- [Versioning](/pages/versioning/)
