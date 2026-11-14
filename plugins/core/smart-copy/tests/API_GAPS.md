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

## Move blockers (verified 2026-11-14)

The `smart-copy` kernel files stay PORT-KEEP after the gap streams. The list below is the exact reason per file; the cuts file `docs/tasks/02-cuts/smart-copy.md` carries the key table.

- `ilium-client/src/{smart_copy,smart_copy_light,smart_copy_selection,smart_copy_tokens}.rs`: called synchronously by the selection and copy path in `mouse.rs` and `app.rs` (K4).
- `ilium-client/src/smart_copy_workers.rs`: needs `inference` (K5).
- Copying needs the `clipboard` import (K5).
- `ilium-client/src/{terminal_context_menu,terminal_selection}.rs`: the terminal pane menu shows no plugin entries (K6).
- Extension points `recognisers` and `after-copy` and the `link-actions` entry: manifest and manual changes (K9).

Blocker keys (verified against the tree on 2026-11-14; each names the exact missing piece):

- K1 intents. `ilium-server/src/plugin_host/facade.rs` `apply_intent` routes only RenameNode, MoveNode, CloseNode, SetNodeLocked, CreatePane, ResizePane, CreateGroup, CreateSplit, SetBookmarked, SetExpanded, Message, SetStatusSegment, ClearStatusSegment, SetDecoration, ClearDecoration, SignalPane, OpenView and CloseView. Every other arm ends in `unsupported("this intent")`: `Intent::SendInput` (needs `Delivery::{Typed, Submitted, Pasted}` plus `stages` / `prompt_epoch` mapped to the server requests `UserKeyInput`, `SubmitTerminalText`, `PasteTerminalText`, `SchedulePaneInput`), `EditorOpen`, `ApplyEdits`, `Notify`, `PlaySound`, `OpenExternal`, `AgentAction`. `ilium-server/src/plugin_host/intent_routing.rs` does not exist.
- K2 pane kinds. `ilium_ipc::NewPaneKind::Plugin { kind_id, title }` exists on the wire only. `ilium-core` has no plugin pane content kind (no `kind_id` in `ilium-core/src/lib.rs`) and the client draws no plugin pane, so board and editor panes cannot leave the kernel.
- K3 popups. `ilium-client/src/plugin_host/popup.rs` (the `Mode::PluginPopup` host) draws message lines, `paragraph`, `choice` and `check-list` only. Text inputs and scrolling exist in the right-hand panel views (`plugin_host/view.rs`, `views.rs`) and not in modal popups. A kernel dialog with text entry or a long scrolling list therefore cannot become a plugin popup. Wanted: `popup.rs` renders and edits `Widget::TextInput`, `Widget::NumberInput` and `Widget::ScrollView` with the same state as `view.rs`.
- K4 kernel to plugin calls. The kernel call sites of these features run synchronously inside kernel loops (server detection loop, server IPC handlers, the client selection path). A plugin answers asynchronously through events. Wanted: a kernel-side call, for example `PluginHost::call_service(plugin: &str, service: &str, payload_json: &str, deadline: Duration) -> Result<String, CallError>` on the server and `App::call_plugin_service(...)` on the client, with `CallError::Unavailable` when the plugin is disabled so the kernel feature is off while the plugin is off.
- K5 imports that are not equipped on a running plugin instance (`ilium-plugin-host/src/runtime.rs` instance import set and `wiring/wave_two.rs`): `clipboard` (`ClipboardImport` exists, used only by the testing harness), `editor` (`EditorImport`, harness only), `client-requests` (harness only). Skeletons without a backend: `inference` (`InferenceImport::new(ids)`, no provider call), `sounds` (`SoundsImport`), `audio`. `workspace` is an in-memory `WorkspaceImport` that is not backed by the kernel custody in `ilium-server/src/workspace*.rs`.
- K6 tree and terminal chrome. `ilium-client/src/terminal_context_menu.rs` lists no plugin entries and contributed toolbar buttons and row actions are not drawn (G6 is not built).
- K7 command line environment. There is no env or argv import in `wit/host/`. The `cli` import offers `caller-pane` and `selected-panes` only. A moved `ilium chat` needs `ILIUM_CHATROOM_AUTHOR`, `AGENT_NAME`, `ILIUM_PANE_ID`, the pane worktree variable `ilium_ipc::pane_env::WORKTREE`, optional hook JSON on standard input (`stdin: optional`; the manifest says `none`), and a project scope that reaches the main checkout of a linked worktree (the `fs` project scope is rooted at `Backends::project_dir` only).
- K8 settings. A contributed row is stored under `[plugins."<id>"]` in `config.toml`. The kernel tabs these plugins own are `[git]`, `[kanban_board]`, `[editor]`, `[text_triggers]`, `[triggers]` and `[icons]`, read by kernel code that must keep working (server workspace setup, `ilium-client/src/config.rs`). `settings.read-kernel:<tab>` and `settings.write-kernel:<tab>` work through `ConfigFileSettings`, but the running client and server do not reload after a plugin write and no `kernel-settings-changed` event exists (contract 20.5). Moving a tab needs both a migration of the stored keys and kernel readers that read the plugin store.
- K9 manual lock. The manual page and its REQ-tagged tests pin the manifests: for example `plugins/core/worktrees/tests/features/manifest.rs` line 240 asserts there is no `settings` contribution, and `plugins/core/text-triggers/tests/features/manifest.rs` asserts the exact grant lists. The group plan's contributions (settings rows with the built-in key names, extra grants, extension entries, a `plan` service) contradict those pins. They need a decision to change the manual page and the pins together; `tools/req_coverage.py` stays at exit 0 only while tagged tests are kept.

What the foundation does provide today (so a later move can use it): editable plugin settings rows in the Plugins list (`plugin_host/settings_rows.rs`), text input and scrolling in panel views (`plugin_host/view.rs`), link detectors (`plugin_host/links.rs`), the `process`, `fs`, `http` and `jobs` imports per instance, topics and services across sides, `SignalPane` and forwarded `OpenView`/`CloseView`, and `ilium <name>` routing to plugin CLI commands (`ilium/src/plugin_cli.rs`).
