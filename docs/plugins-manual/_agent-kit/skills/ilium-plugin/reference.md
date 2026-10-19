<!-- contract: 0.1 -->
<!-- generated-with: manual -->
# Ilium plugin quick reference

Facts below are condensed from the manual. The manual wins on any difference.

## Package layout (`.iliumpkg`)

`manifest.json` (required), `server.wasm`, `client.wasm`, `scene.wasm` (one per declared side), `assets/**`, `help/*.md`, `sounds/*`, `src/**` (never loaded), `signature` (for signed packages). Every file other than the manifest and signature is listed in the manifest `files` inventory with its length and SHA-256.

## Sides

| Side | World | Runs | Required exports |
|---|---|---|---|
| server | `ilium:server/base` | once per session server | `lifecycle`, `handler` |
| client | `ilium:client/base` | once per attached client | `lifecycle`, `handler` |
| scene | `ilium:anim/base` | the background animation of one client | `renderer` |

## Deadlines and limits (defaults, third-party plugins)

| Item | Value |
|---|---|
| `on-events` | 20 ms, then trap |
| `views.render` | 10 ms, then trap |
| `activate` | 2,000 ms |
| `deactivate` | 500 ms |
| Memory per instance | 32 MiB (ceiling 128 MiB) |
| Events per batch | at most 256 |
| `ui-tree` nodes | 4,096 |

Source: the manual page Errors and limits.

## Errors

Every host call that can fail returns a `plugin-error` with `code`, `message` and optional `retry-after-ms`. Codes include `not-permitted`, `invalid-argument`, `rate-limited`, `quota-exceeded`, `timed-out` and `busy`. A rate-limited call is retried after `retry-after-ms`.

## Test harness

| Task | Rust | TypeScript |
|---|---|---|
| Fake host | `ilium_plugin_sdk::testing::FakeHost` | `createFakeHost()` from `@ilium/plugin-sdk/testing` |
| Run | `cargo test` | `bun test` |
| Grant | `.grant("tree.read")` | `.grant('tree.read')` |
