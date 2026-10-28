---
name: ilium-plugin
description: Build, change, test and package Ilium plugins (WebAssembly components in Rust or TypeScript). Use when the project has an Ilium manifest.json, when the user asks for an Ilium plugin, command, panel, pane kind, setting, decoration or ambient scene, or mentions .iliumpkg or the ilium plugin commands.
---
<!-- contract: 0.1 -->
<!-- generated-with: manual -->
<!-- Install as .claude/skills/ilium-plugin/SKILL.md (with reference.md beside it). -->

# Ilium plugin development

## Find the facts first

1. Fetch the index `http://127.0.0.1:4327/llms.txt` (use the host of your manual).
2. Fetch only the pages for the task, as Markdown (page address with `.md`). Common pages: `getting-started-first-plugin`, `events-and-intents`, `capabilities`, `reference-manifest`, `retained-mode`, `ui-model`, `testing`, `errors-and-limits`, `rust-sdk`, `typescript-sdk`.
3. If the manual does not state a fact, tell the user. Do not invent API.

## Workflow

1. Create: `ilium plugin new <dir> --id <plugin-id> --lang rust|typescript --side server|client|both|scene`. TypeScript: run `bun install` (Bun only, never node).
2. Decide the sides and the capabilities. Write them into `manifest.json` (`capabilities`, `contributes`, `exports`). See [reference.md](reference.md).
3. Write the handler: events in, intents out. Keep it under the 20 ms deadline; push slow work to requests or jobs.
4. Use retained mode for decorations, virtual rows and toolbar state.
5. Test with the fake host in the language of the project.
6. Run `ilium plugin build`, then `ilium plugin check` on the package, then `ilium plugin dev` for a live loop. Package with `ilium plugin pack`.
7. Report the commands run and their output.

## Both languages

When you show or write an API use, give the Rust form and the TypeScript form, unless the project uses one language. Rust: `ilium-plugin-sdk`, `#[ilium_plugin(side = "server")]`, `Lifecycle` and `Handler` traits. TypeScript: `@ilium/plugin-sdk`, `definePlugin`, `defineScene`, camelCase names, kebab-case variant strings.

## Hard rules

- Never block; never poll; never draw per frame.
- Smallest grants; declare every contribution in the manifest.
- Handle every outcome, including refusals.
- No invented glyphs: borders are rounded, form leaders are the single character U+2026.
- Do not edit `dist/`, signed packages or SDK WIT files.
