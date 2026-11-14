# API gaps of the layout feature tests

- `ilium-plugin-testkit`: `impl FakeHost { pub fn published_topics(&self) -> Vec<(String, String)> }` (topic name, payload JSON, in publish order). Needed by `f_003_the_published_changed_topic_carries_group_and_split_count` and `f_003_no_changed_topic_is_published_when_the_split_is_refused` (F-003, topic `ilium.core.layout/changed`): the harness exposes no way to read topics a plugin published through `messages.publish`.

- `ilium-plugin-testkit`: `FakeHost::render` returns a blank `Screen` and `FakeHost::send_action` does nothing. The port tests of the "New split view" dialog therefore call `new_split::Dialog::render` and deliver `Event::UiAction` with `deliver_one`. Wanted: `render` calls the plugin's `Views::render` and `send_action` delivers a `ui-action` event.

- Foundation G2 and G8: plugin settings rows are read-only and popup views cannot edit text, so the kernel padding dialog, the tree layout editor and the live specimen stay (port-keep). The plugin declares the `padding` tab and the `tree_row_preset` row and holds the pure models (`padding`, `padding_rows`, `tree_layout_rows`); nothing in the kernel reads the rows yet.
- `ilium-plugin-testkit`: `FakeSettings` cannot apply a row edit made through `padding_rows::apply`; the edit is tested on the pure model only.

## Re-check of the foundation against the tree (2026-11-14): what still blocks the moves

- `padding_dialog.rs` / `padding_ui.rs`: number inputs in a dialog need the popup host to draw `Widget::Number` and `TextInput` (`ilium-client/src/plugin_host/popup.rs` draws only `Paragraph`, `Choice`, `CheckList`).
- `padding.rs` / `padding_app.rs` are read by the frame drawing every frame; the plugin can only write the values (`kernel-settings` `set("user-interface.padding...")`). The running client does not reload its in-memory `padding_settings` after a plugin write: wanted `Event::KernelSettingsChanged { tab }` from the G19 event feed (`ilium-plugin-host/src/runtime/events_feed.rs`, absent) and an `App::reload_ui_settings()` called when it arrives.
- `tree_layout_settings.rs`: `settings_ui.rs` draws no plugin-contributed Settings tab; see the tree gaps.
