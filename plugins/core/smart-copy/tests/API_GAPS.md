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

## Port wave notes (workspace group)

- Moved as pure modules with tests: `src/light.rs` (preview clipping, countdown, summary, idle release; time as `u64` milliseconds), `src/tokens.rs` (URL tail trimming, path and hash plausibility, label clipping). Tests: `tests/port/light_pure.rs`, `tests/port/tokens_pure.rs`.
- Not moved yet: the regex line scanner and `styled_runs` of `smart_copy_tokens.rs` (needs a `regex` dependency in `Cargo.lock` for the wasm build, G18, and screen text input, G10), `smart_copy.rs` ranking and candidate list, `smart_copy_selection.rs`, `smart_copy_workers.rs` (needs `inference`, G10), `menu.rs`. The command popup and the real `Client` wiring stay open.
- G10: `clipboard` and `inference` host imports are not wired; copying and model ranking cannot run in a plugin yet.
- G4: no client-side `transfer` of screen text, so the command must take `{"rows": [...]}` in its arguments.

## Move blockers (re-verified 2026-11-14, second pass)

The per-file reasons and the blocker keys B1 to B10 (each with its evidence in the source) are in `docs/tasks/02-cuts/smart-copy.md`. The earlier K1 (no `intent_routing.rs`), K3 (no popup text input) and K6 (no terminal menu entries) of this section are closed by the consumer streams; the rest were re-read against the current tree.
