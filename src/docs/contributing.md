# Contributing

This page explains how to report problems, set up a workspace, run the checks a change must pass, and follow the project's contribution rules. It also covers licences and third-party notices.

## Contents

- [Reporting issues](#reporting-issues)
- [Setting up a workspace](#setting-up-a-workspace)
- [Workspace checks](#workspace-checks)
- [Contribution rules](#contribution-rules)
- [Testing policy](#testing-policy)
- [Documentation](#documentation)
- [Licence and third-party notices](#licence-and-third-party-notices)

## Reporting issues

Report problems in [issues](https://github.com/arthurwolf/ilium/issues).

1. Search existing issues first.
2. Include your Ilium version (`ilium --version`), operating system, terminal emulator, the command you ran, and the observed behaviour with what you expected.
3. Add steps to reproduce, and a screenshot or terminal capture if the problem is visual.
4. Remove private content from logs before attaching them. Debug file logging records full request and response text; see [How it works](how-it-works.md#diagnostics-and-logs).

## Setting up a workspace

1. Follow [Building from source](building-from-source.md) to clone the repository and install the prerequisites. rustup reads `rust-toolchain.toml` and installs the pinned Rust version with `clippy` and `rustfmt`.
2. Read [ARCHITECTURE.md](../../ARCHITECTURE.md) before touching code, and [AGENTS.md](../../AGENTS.md) for the contribution rules.
3. Build with `cargo build --locked` or `make build`.

## Workspace checks

Run all three before considering a change done:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --no-fail-fast
```

Treat new clippy warnings as things to fix, not to silence with `#[allow]`, unless there is a documented reason.

## Contribution rules

The full text is in [AGENTS.md](../../AGENTS.md). The essentials:

- One crate per architectural layer. Do not collapse layers for convenience.
- `ilium-core` has no I/O and no async. `ilium-detect` is a pure classification function. `ilium-pty` knows nothing about the tree or agents. `ilium-client` never touches the PTY, terminal-parser or process-list libraries; if it needs more from the server, extend the IPC protocol in `ilium-ipc`.
- Operating-system specifics belong in `ilium-platform` (and `ilium-transport` for the IPC channel), not in `#[cfg]` branches elsewhere.
- Adding support for a new agent CLI means adding one registry entry in `ilium-detect`, not a new branch.
- Rust style: `snake_case`, `PascalCase`, `SCREAMING_SNAKE_CASE`; full descriptive names; `Result` everywhere fallible with no `unwrap` or `expect` outside tests; `thiserror` error enums; every spawned async task has an owner that can cancel it.
- Use the `directories` crate for configuration and data paths; never hardcode `~`.
- Do not replace normal UTF-8 icons with plain glyphs to fix a rendering problem; fix the width or rendering behaviour instead.
- Background animations must use the shared look controls (colour, palette, dither, display) and add only scene-specific settings. A new animation is not done until it appears in the Settings list, round-trips its controls, has a help topic and is mentioned in the documentation.
- Scope: preserve the existing JavaScript animation-package extension runtime; worker/service work does not add another user-extension runtime or loader. Remote or SSH sharing and an agent-driving SDK are also outside scope (see the non-goals in ARCHITECTURE.md). The project is new, so there are no compatibility shims or version-two files.
- Process custody: never restart or kill a user's running Ilium server from a contribution workflow; build and verify the artefact and report that a running server still has the old executable.

## Testing policy

- Pure crates use plain unit tests with no I/O.
- Detection tests run against captured screen-text fixtures, never a real agent binary. A failing fixture is a signal to update the signature registry, not a flaky test.
- `ilium-pty` tests spawn a real PTY with a trivial command.
- IPC messages are round-trip tested, and a version mismatch must fail loudly.
- End-to-end tests drive the real binary under a genuine PTY and a live fake-agent process. Visual quality on a real terminal emulator remains a manual check.

## Documentation

`README.md` is the short user entry point and `src/docs/` holds the full user documentation. Keep internal design history out of user documents; design belongs in [ARCHITECTURE.md](../../ARCHITECTURE.md). The repository's `docs/` directory is git-ignored scratch space; never put anything there that a fresh clone needs.

## Licence and third-party notices

Ilium is [MIT licensed](../../LICENSE). The vendored `vt100` patch and `tui-tree-widget` fork retain their MIT licences. Cascadia Code is distributed under the SIL Open Font License 1.1; see the [font notice](../../ilium-client/assets/fonts/NOTICE.md). The release tooling assembles a reviewed third-party notice bundle (`THIRD-PARTY.txt`) that ships with every release package; packaging refuses to publish without it. See the [release guide](../../release/RELEASING.md) for the publication gates.
