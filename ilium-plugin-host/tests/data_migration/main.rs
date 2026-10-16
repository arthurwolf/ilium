//! Lane `config-migration`, host crate: plugin data versions, the `migrator` export, backups,
//! rollback and downgrade, multi-server locking, and the durability and atomicity rules of the
//! `storage` import (LIF-100..121, WIT `ilium:exports/migrator.migrate`).
//!
//! One module per manual page. `common` holds the shared helpers. The tests drive the rig named
//! in API_GAPS.md (`ilium_plugin_host::testing::migration`). Run with `--features testing`.

mod api_storage;
mod common;
mod guide_data_migration;
mod wit_migrator;
