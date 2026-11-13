//! Lane core-tree-term, part "ports": FakeHost/FakeSession twins of the original client tests that
//! moved into the Editor core plugin (tools/test_port_map.tsv, class adapt-to-plugin-api, target
//! plugins/core/editor). Each twin asserts the observable behaviour of the original through the
//! plugin API (rendered screen, intents, token provider, buffers service). See ../API_GAPS.md.

#[path = "../features/support.rs"]
mod support;

mod helpers;

mod agent_from_line;
mod checkbox;
mod editor_chrome;
mod editor_highlight;
mod editor_pane;
mod editor_toolbar;
mod kernel_contract;
mod markdown_document;
mod markdown_media;
mod markdown_view;
mod minimap;
mod syntax;
mod wheel_and_dirty;
