# API gaps: kernel-boot, ilium binary crate tests

- ilium: `pub mod plugin_cmd;` in `ilium/src/lib.rs` (today `mod plugin_cmd;` is declared only in the binary `main.rs:29`, and the crate has no lib target) so `ilium::plugin_cmd::{parse_start_mode, SAFE_PLUGINS_ENV, NO_PLUGINS_ENV}` are reachable from integration tests (HST-033, 043).
- ilium Cargo.toml: dev-dependencies `ilium-plugin-host` with feature `testing`, `proptest`, `tempfile` (all tests in this directory).
- ilium: clap flags `--safe-plugins` and `--no-plugins` on the top-level command and `ilium plugin list --json` must be wired for the binary runs (HST-033, 034); the tests are red until they are. `list-sessions` is assumed to be the session listing subcommand.
