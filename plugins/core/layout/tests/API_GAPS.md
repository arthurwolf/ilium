# API gaps of the layout feature tests

- `ilium-plugin-testkit`: `impl FakeHost { pub fn published_topics(&self) -> Vec<(String, String)> }` (topic name, payload JSON, in publish order). Needed by `f_003_the_published_changed_topic_carries_group_and_split_count` and `f_003_no_changed_topic_is_published_when_the_split_is_refused` (F-003, topic `ilium.core.layout/changed`): the harness exposes no way to read topics a plugin published through `messages.publish`.

- `ilium-plugin-testkit`: `FakeHost::render` returns a blank `Screen` and `FakeHost::send_action` does nothing. The port tests of the "New split view" dialog therefore call `new_split::Dialog::render` and deliver `Event::UiAction` with `deliver_one`. Wanted: `render` calls the plugin's `Views::render` and `send_action` delivers a `ui-action` event.
