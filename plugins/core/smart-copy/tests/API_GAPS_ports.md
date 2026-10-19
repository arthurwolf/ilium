# API gaps for smart-copy port tests

Each line: crate | signature | id | why.

- ilium-plugin-sdk (testing) | `FakeTerminal::set_rows(pane, &[&str])` must interpret SGR escape sequences (`\u{1b}[34m`, `\u{1b}[1m`, `\u{1b}[7;42m`) as cell styles for `read-cells` | PORT:ilium-client/src/smart_copy.rs::coloured_and_bold_runs_become_candidates | styled runs are candidates
- ilium-plugin-sdk (testing) | `FakeHost::menu_items(&mut self, MenuContext) -> Vec<MenuEntry>`, `terminal().set_selection`, `clipboard().last_text/fail_next_write`, `inference().reply_text/fail_next/requests` as listed in API_GAPS.md | all ports | same fakes as the features part
- ilium-plugin-sdk | `Key::named(&str) -> Key` | picker ports | key events
- ilium-plugin-sdk (testing) | `FakeHost::render_view(&mut self, ViewTarget, u16, u16) -> String` | PORT:ilium-client/src/ui.rs::smart_copy_preview_fits_narrow_screens | same name as in API_GAPS.md
- ilium-plugin-sdk (testing) | open actions: a `menu-items` entry for "Open in editor" / "Open externally" and a command-intent for pasting into a pane have no testkit fake (FakeHost::fs_exists) | PORT:ilium-client/src/app.rs::right_click_on_an_existing_file_offers_editor_and_external_open_actions | no way to declare existing files; twin skipped
- plugins/core/smart-copy | ports of app.rs context_menu_paste_*, editor_right_click_*, right_click_on_an_existing_file_* | F-016 | no fake for open-in-editor/paste intents or editor line capture; twins skipped
- plugins/core/smart-copy | ports of paste_cursor.rs (8), smart_copy_workers.rs (8), smart_copy_selection.rs (5), context_menu_layout.rs::empty_and_zero_height_menus_have_no_selectable_rows, pty_tui_smoke right_click_restart | F-016/F-065 | host-internal (input leases, CPU bank, FIFO slots, worker threads, viewport arithmetic, process restart) with no plugin channel; twins skipped
- ilium-plugin-sdk (testing) | `FakeHost::files() -> FakeFiles` with `add_existing(&str)` | PORT:ilium-client/src/app.rs::right_click_on_an_existing_file_offers_editor_and_external_open_actions, PORT:ilium-client/src/app.rs::editor_right_click_on_an_existing_path_offers_open_externally | declares existing files so the menu can offer the open actions
- ilium-plugin-sdk (testing) | `FakeInference::fail_next(&str)` accepting the codes "Io", "Denied", "Namespace", "Closed" and `FakeClipboard::fail_next_write(&str)` accepting "Closed", "Denied" | PORT:ilium-client/src/smart_copy_workers.rs::*, PORT:ilium-client/src/smart_copy_selection.rs::* | error-code channels of the ports
