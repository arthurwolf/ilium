## Key crates

| Crate | Role |
|---|---|
| [`ratatui`](https://ratatui.rs/) | TUI widget rendering |
| [`crossterm`](https://docs.rs/crossterm) | terminal backend: raw mode, input events (incl. mouse), alternate screen |
| [`portable-pty`](https://docs.rs/portable-pty) (wezterm) | cross-platform PTY spawn/resize/IO |
| [`vt100`](https://docs.rs/vt100) | terminal-escape-sequence parser → screen grid; source of both the rendered pane content and the text the detection engine scans |
| [`tui-term`](https://docs.rs/tui-term) | ratatui widget that renders a `vt100::Screen` directly — used for the right-hand live pane view |
| [`tui-tree-widget`](https://docs.rs/tui-tree-widget) | ratatui tree widget — left-hand session/group/pane panel |
| [`sysinfo`](https://docs.rs/sysinfo) | process-tree walk for agent identity detection |
| [`tokio`](https://docs.rs/tokio) | async runtime: PTY IO tasks, detection-loop timers, UDS server/client |
| `serde` + `toml` | config file, IPC message (de)serialization |
| `bincode` | wire format for the client↔server IPC frames |
| [`directories`](https://docs.rs/directories) | XDG-correct config/data/socket paths |
| [`clap`](https://docs.rs/clap) | CLI argument parsing |
| `ureq` | HTTP client for `ilium-inference`'s provider calls (background session/project title inference) |
| [`notify-rust`](https://docs.rs/notify-rust) | desktop notifications for agent-finished, approval-needed and monitored-task outcomes (`ilium-server/src/notifications.rs`), switched per event via `config.toml`'s `[notifications]` table (`ilium_sound::NotificationSettings`) — see M5 |

### A note on the vendored `vt100`

`vt100` 0.16.2 can retain an orphaned wide cell after a shrinking resize, then panic when a later erase sequence clears that row. Upstream issue #28 and PR #30 contain the unreleased fix. The workspace keeps a patched copy under `vendor/vt100` and wires it in through `[patch.crates-io]` until a fixed crate release exists.

This is also why ilium is not installable from crates.io: `[patch.crates-io]` applies only to the top-level workspace, so a downstream consumer would silently build against the unpatched crate. `ilium-client/vendor/tui-tree-widget` is vendored as a path dependency for the same class of reason. Both vendored crates keep their upstream licenses in place.

### Native release boundary

The release tooling under `release/` keeps distribution outside the runtime
crates. One manifest drives native build runners, paired archives, installer
selection and Pages metadata. Each installer selects a complete version directory
through one current pointer; the client and server resolve from that same
directory. Native loader, licence, embedding and installed-PTY receipts gate
publication independently of source tests. The hidden release embedding probe
uses the client inference dependency; release scripts own model acquisition and
process observation. See [`release/RELEASING.md`](../release/RELEASING.md) for source,
native, publication and public-installation gates.

GitHub Packages distribution stays inside release tooling. Its registry job
follows the final public release and installer-channel checks, bundles the exact
qualified native assets without rebuilding them, and binds every file to the
immutable Release receipt. Registry acceptance requires matching anonymous
download hashes and public repository-linked package metadata. A matching version
retry verifies existing bytes; a conflicting tag is preserved and fails the job.
The OCI bundle is a distribution artifact and adds no runtime container layer.
