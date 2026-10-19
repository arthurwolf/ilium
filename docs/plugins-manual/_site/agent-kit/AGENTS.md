<!--
  contract: 0.1
  generated-with: manual
  File: AGENTS.md for an Ilium plugin project. Copy to the repository root.
  Read by Codex, Jules, Gemini CLI, Zed, Warp, Copilot, Cursor and others (agents.md).
-->
# AGENTS.md

This project is an Ilium plugin: a WebAssembly component (wasm32-wasip2) written in Rust or TypeScript that runs inside the Ilium plugin host.

## Documentation

The full developer manual is machine readable. Read it before you guess an API.

- Index: http://127.0.0.1:4327/llms.txt (replace the host with the address of the manual you use)
- Everything in one file: http://127.0.0.1:4327/llms-full.txt
- One page as Markdown: the page address with `.md` instead of the trailing slash, for example http://127.0.0.1:4327/pages/events-and-intents.md
- Tool-specific copies of these rules: http://127.0.0.1:4327/agent-kit/

Fetch `llms.txt` first, pick the page for the task, fetch that page. If a fact is not in the manual, say so; do not invent an API, a capability or a manifest key.

## Toolchain and commands

- Rust plugin: stable Rust 1.90 or later with the `wasm32-wasip2` target. SDK crate `ilium-plugin-sdk`.
- TypeScript plugin: Bun 1.2 or later (never node, ts-node or tsx). SDK package `@ilium/plugin-sdk`. Run `bun install` once.
- `ilium plugin new <dir> --id <plugin-id> --lang rust|typescript --side server|client|both|scene`
- `ilium plugin build [<dir>]` writes the components to `dist/`.
- `ilium plugin check <file.iliumpkg>` validates a package and names the page that explains each problem.
- `ilium plugin dev [<dir>]` rebuilds and reloads on every change.
- `ilium plugin pack [<dir>]` writes `dist/<id>-<version>.iliumpkg`.
- Plugin builds run on this machine. Never edit files under `dist/` or the WIT files in the SDK.

## The contract in eight rules

1. Events in, intents out. The handler receives a batch of events and returns a list of submissions. Each submission carries one intent. Results arrive later as `request-completed` events.
2. Capabilities are declared in `manifest.json` and granted by the user at install. A call without its grant fails with `not-permitted`. Request the smallest set; scope arguments such as `fs.read:project` where the manual shows them.
3. Never block. `on-events` has a 20 ms deadline and `views.render` 10 ms. Anything slow (files, HTTP, processes, searches) is a request or a background job that completes as an event.
4. Retained mode. Decorations, virtual rows and toolbar state are set once and stay until changed or cleared. Do not resend them every event. Views are widget trees returned from `render`; call `ui.invalidate` to ask for a new one.
5. Declare what you contribute. Every decoration slot, virtual row kind, command, toolbar button, setting and pane kind is listed under `contributes` in the manifest first.
6. State is yours. Keep state in the plugin struct or object, persist with `storage`, and return restore bytes from `deactivate`. The host may restart the instance at any time.
7. Idempotent submissions. Give notices, sounds and similar intents a dedup key. Handle every outcome, including refusals and `rate-limited` with its `retry-after-ms`.
8. Match the contract version of the manifest `api` key. Do not change it by hand; rebuild with the SDK that matches the Ilium you target.

## Idioms

**Rust**

- Mark the plugin struct with `#[ilium_plugin(side = "server")]` (or `"client"`, `"scene"`) and implement the `Lifecycle` and `Handler` traits. `fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission>`.
- Return `Result` with the SDK `PluginError`; do not `unwrap` on host results. No threads, no blocking sleeps.
- Names are snake_case; error fields are `code`, `message`, `retry_after_ms`.

**TypeScript**

- Export `definePlugin({ side: "server", lifecycle, handler })` (or `defineScene`). `onEvents(batch: Event[]): Submission[]`.
- Return typed results; do not throw for expected failures. No `await` on slow host work inside a handler; submit a request instead.
- Names are camelCase; error fields are `code`, `message`, `retryAfterMs`; variant tags are kebab-case strings.

## Testing

Test against the fake host: `ilium_plugin_sdk::testing::FakeHost` with `cargo test` in Rust, `createFakeHost()` from `@ilium/plugin-sdk/testing` with `bun test` in TypeScript. Grant the same capabilities as the manifest, deliver events, assert the submissions, and render views into a cell grid to compare text. No terminal or running Ilium is needed.

## Never

- Never invent an API, WIT function, manifest key or capability name. Look it up or report the gap.
- Never request a capability you do not use, or a broad scope where a narrow one works.
- Never block the handler, spin, or poll. Use timers, jobs and events.
- Never draw per frame, and never use other glyphs than the manual shows: borders are rounded, leaders are the single character U+2026.
- Never write outside the granted file scopes, and never store secrets in settings or logs.
- Never edit generated output (`dist/`, `llms.txt`) or hand-edit a signed package.
- Never describe the Rust version only. A feature documented for both languages must work in both.
