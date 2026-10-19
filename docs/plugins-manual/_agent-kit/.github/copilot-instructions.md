<!--
  contract: 0.1
  generated-with: manual
  File: .github/copilot-instructions.md for an Ilium plugin project.
  Copilot also reads AGENTS.md; keep both in step.
-->
# Copilot instructions

This repository is an Ilium plugin written in Rust or TypeScript. Follow AGENTS.md. Summary:

- Documentation index for agents: http://127.0.0.1:4327/llms.txt. Fetch the page for the task as Markdown. Do not invent API, manifest keys or capability names.
- Build with `ilium plugin build`, validate with `ilium plugin check`, pack with `ilium plugin pack`. TypeScript uses Bun, never node.
- The plugin receives events and returns intents. Rust: `fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission>`. TypeScript: `onEvents(batch: Event[]): Submission[]`.
- Capabilities and contributions are declared in `manifest.json`; request the smallest set.
- Never block the handler (20 ms deadline). Use requests, timers and background jobs.
- Retained mode: set decorations and toolbar state once, not per event.
- Tests use the fake host in both languages.
