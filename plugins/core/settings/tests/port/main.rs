//! Port twins of the settings tests of the original ilium-client (ledger tools/test_port_map.tsv,
//! class adapt-to-plugin-api, target plugins/core/settings): the same observable behaviour,
//! asserted through the plugin API with FakeHost (F-089, F-090).

mod common;
mod helpers;

mod app;
mod budget;
mod budget_receipts;
mod dialogs;
mod layout;
mod misc;
mod modal_stack;
mod output;
mod text_prompt;
mod theme;
mod values;
mod wizard;
