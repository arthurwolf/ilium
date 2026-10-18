# API gaps for smart-copy feature tests

Each line: crate | signature | id | why.

- ilium-plugin-sdk (testing) | `FakeHost::render_view(&mut self, target: &ViewTarget, width: u16, height: u16) -> Vec<String>` | F-065 | golden picker screen text
- ilium-plugin-sdk (testing) | `FakeHost::menu_items(&mut self, MenuContext) -> Vec<MenuEntry>` | F-016 | menu-provider export
- ilium-plugin-sdk (testing) | `FakeHost::terminal() -> FakeTerminal` with `pointer_claims(pane) -> Vec<Option<PointerModifiers>>`, `set_rows(pane,&[&str])`, `set_selection(pane, Selection)` | F-016, F-065 | claims and screen inputs
- ilium-plugin-sdk (testing) | `FakeHost::clipboard()` with `last_text() -> Option<String>`, `fail_next_write(&str)` | F-065, F-066 | copy outcome
- ilium-plugin-sdk (testing) | `FakeHost::inference()` with `reply_text(&str)`, `fail_next(&str)`, `requests() -> Vec<CompletionRequest>` | F-066 | model ranking
- ilium-plugin-sdk (testing) | `FakeHost::hook_chain(point)` with `reply_all`, `add(id, priority, HookReply)`, `last_payload() -> Option<Value>` | EXT-042 | after-copy chain
- ilium-plugin-sdk (testing) | `FakeHost::contribute_entry(point, id, json)`, `published(topic) -> Vec<String>`, `settle()`, `failures()`, `activate::<P>()`, `deliver_one(Event) -> Vec<Submission>` | F-065, F-066 | collect points, topic, async settle
- ilium-plugin-sdk | `Key::named(&str) -> Key`; `MenuEntry` fields `shortcut`, `separator_before`, `disabled_reason`, `is_submenu`, `parent` | F-016 | key events and menu assertions
- ilium-plugin-sdk (testing) | `ErrorCode` returned by `settings().set` for invalid values with loader message | COR-011 | settings validation
- ilium-plugin-sdk | `types::Key` (see Key::named line above) and `types::Event::Key(Key)` variant | F-016 | key events
