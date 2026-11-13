//! Baseline CLI compatibility (port-compat lane): replays the examples of
//! `src/docs/cli-reference.md` against the plugin build of the real `ilium` binary (COR-040..042)
//! and checks the kernel-owned CLI and animation features (F-078, F-093, F-094, F-102, F-104..F-106, F-111).
//! One module per manual page or user-documentation page covered.

mod animations;
mod cli_reference;
mod common;
mod reference_cli_compatibility;
