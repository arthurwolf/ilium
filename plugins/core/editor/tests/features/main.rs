//! Lane core-tree-term, part "features": the Editor core plugin (core-plugin-editor.md).
//! Native mode: the plugin is linked as a library (`ilium_plugin_core_editor`) and driven through
//! FakeHost (events, intents, rendered screens) or, for exports FakeHost has no entry for
//! (token provider, service provider, widget trees), directly. See ../API_GAPS.md.

mod fixtures;
mod support;

mod events;
mod extensions;
mod manifest;
mod markdown;
mod render_editor;
mod service;
mod settings;
mod tokens;
